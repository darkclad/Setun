//! Setun-386 CPU: registers, decode/execute, traps and the MMU.

use crate::io::Io;
use crate::isa::{alu, cond, cr, falu, fcvt, fld, fsr, io as port, mem, mode, op, sys, vector};
use crate::memory::Memory;
use crate::{HALF_PAGE, PAGE};
use setun_real::Real;
use ternary::{wrap_trits, Trit, Tryte, Word};

/// PC after reset: −3^10, the centre of BIOS ROM frame −3.
pub const RESET_PC: i64 = -59_049;

/// `CPUID` value: ISA version (v0.5) in the low trits, and trit 1 (+1):
/// the Setun-387 FPU is present.
pub const CPUID_VERSION: i64 = 5;
pub const CPUID: i64 = CPUID_VERSION + 2_541_865_828_329; // + 3^26

/// Kind of memory access, encoded as in `FCAUSE[22]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read = -1,
    Fetch = 0,
    Write = 1,
}

/// A trap raised while executing an instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fault {
    pub vector: i64,
    /// Faulting virtual address (for memory faults), else 0.
    pub addr: i64,
    pub access: Access,
    /// Extra info for `FCAUSE[1:18]` (e.g. the `SYS` number).
    pub info: i64,
    /// `EPC` = the next instruction (SYS, BRK) rather than this one.
    pub resume_next: bool,
}

impl Fault {
    fn new(vector: i64) -> Self {
        Fault { vector, addr: 0, access: Access::Read, info: 0, resume_next: false }
    }

    fn mem(vector: i64, addr: i64, access: Access) -> Self {
        Fault { addr, access, ..Fault::new(vector) }
    }
}

/// Outcome of one [`Cpu::step`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// An instruction executed (possibly ending in a trap or interrupt).
    Ran,
    /// Waiting in `HLT` for an interrupt.
    Halted,
    /// A fault while delivering a double fault: the machine was reset.
    TripleFault,
}

pub struct Cpu {
    regs: [Word; 27],
    pub pc: Word,
    crs: [Word; 27],
    /// `MODE` kept unpacked, one trit per slot (index = trit position), so
    /// flag reads and writes cost nothing. Packed on `MFC`/trap entry.
    mode_trits: [i8; 28],
    /// Instruction counter behind the read-only `CYCLE` register.
    cycle: u64,
    pub halted: bool,
    /// Current instruction's address, for fault reporting.
    inst_pc: Word,
    /// The Setun-387: f−13..f13 (f0 reads as zero) and FSR (ISA §14.2).
    fregs: [Real; 27],
    fsr: Word,
    /// Inside [`Cpu::run_batch`]: port I/O is deferred to a single step.
    batch: bool,
    /// End the batch after this instruction (it may have changed IE or TF).
    stop_batch: bool,
    /// Software TLB, one direct-mapped table per access kind. It is never
    /// visible: entries are flushed whenever PTB or a cached page-table
    /// entry changes (see `Memory::pt_dirty`), so every access behaves
    /// exactly like a full walk.
    tlb: Box<[TlbEntry]>,
    /// Basic blocks of decoded instructions. `block_at[slot][word]` is 1 +
    /// the index into `blocks` of the block starting at that word of code
    /// slot `slot` (see `Memory::code_slot`), or 0. A slot's table is
    /// dropped when its frame is written; everything is cleared when the
    /// arena grows too big.
    block_at: Vec<Option<Box<[u32]>>>,
    blocks: Vec<Block>,
    /// Bumped by anything that may make a chain link stale: a change of
    /// translation or ring, dropped code, a cleared arena.
    epoch: u64,
    ops: Vec<Dec>,
    /// The page the last instruction was fetched from: virtual addresses
    /// `fw_lo..=fw_lo + PAGE - 1` map to physical `va + fw_delta`, code slot
    /// `fw_slot`. Cleared whenever translation or the ring may change.
    fw_lo: i64,
    fw_delta: i64,
    fw_slot: usize,
    /// The same for the last page read (`rw_*`) and written (`ww_*`); a
    /// write window only exists once the page is dirty, like a TLB entry.
    rw_lo: i64,
    rw_delta: i64,
    ww_lo: i64,
    ww_delta: i64,
    /// The RAM frame behind each data window (`usize::MAX`: not RAM).
    rw_frame: usize,
    ww_frame: usize,
}

/// A decoded instruction. `Generic` ones go through [`Cpu::execute`].
#[derive(Clone, Copy)]
struct Dec {
    op: Op,
    /// Major opcode (for `Generic`).
    a: i16,
    x: i8,
    y: i8,
    z: i8,
    /// 3 or 6 trytes; 0 = slot not decoded yet.
    len: u8,
    /// ALU function, or the branch condition's flag (1 = S, 2 = C).
    f: i16,
    /// Immediate, offset or second operand, depending on `op`.
    imm: i64,
    /// BR3's positive offset; a branch's taken-mask (bit k: flag = k - 1).
    imm2: i32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Op {
    AddR,
    SubR,
    CmpR,
    AddK,
    SubK,
    CmpK,
    AluR,
    AluK,
    Ldt,
    Ldw,
    Stt,
    Stw,
    Swap,
    Jalr,
    Li,
    Lea,
    Jal,
    Br,
    Br3,
    Generic,
}

const WORDS_PER_FRAME: usize = (PAGE / 3) as usize;
/// The longest block, in instructions.
const MAX_BLOCK: usize = 256;
/// Past this many decoded instructions, all blocks are thrown away.
const MAX_OPS: usize = 1 << 22;

/// A basic block: `count` decoded instructions from `ops[first]`, and the
/// blocks it last went on to (looked up by target address, valid only in
/// the epoch they were made in).
#[derive(Clone, Copy)]
struct Block {
    first: u32,
    count: u32,
    links: [Link; 2],
    next_link: u8,
}

#[derive(Clone, Copy, Default)]
struct Link {
    target: i64,
    id: u32,
    epoch: u64,
}

/// What an instruction in a block did.
enum Ctl {
    /// Fall through to the next instruction.
    Next,
    /// Control went to this address (the block ends).
    Jump(i64),
    /// Run it through `exec_major` / `exec_fpu` (the block ends).
    Generic,
}

#[derive(Clone, Copy)]
struct TlbEntry {
    vpage: i64,
    frame: i64,
    /// The access is allowed from this ring up (both levels' RING trits).
    min_ring: i64,
}

const TLB_SIZE: usize = 1024;
const TLB_EMPTY: TlbEntry = TlbEntry { vpage: i64::MIN, frame: 0, min_ring: 2 };

/// Pseudo-vector: "this instruction needs a single step" (port I/O in a batch).
const DEFER: i64 = i64::MIN;

/// Split `v` into `(v div m, v mod m)` with a balanced remainder. Called with
/// constant `m` so the divisions compile to multiplications.
#[inline(always)]
fn bal_split(v: i64, m: i64) -> (i64, i64) {
    let h = (m - 1) / 2;
    let lo = (v + h).rem_euclid(m) - h;
    ((v - lo) / m, lo)
}

fn idx(r: i64) -> usize {
    (r + 13) as usize
}

fn w(v: i128) -> Word {
    Word::wrap(v)
}

/// 3^27 and the largest word.
const W_MOD: i64 = 7_625_597_484_987;
const W_MAX: i64 = 3_812_798_742_493;

/// Wrap a sum of at most a few words into a word: one correction suffices
/// while |v| <= 3·W_MAX + 1. Returns (word value, carry trit).
#[inline(always)]
fn wrap_carry(v: i64) -> (i64, i64) {
    if v > W_MAX {
        (v - W_MOD, 1)
    } else if v < -W_MAX {
        (v + W_MOD, -1)
    } else {
        (v, 0)
    }
}

/// A word from a sum of at most a few words (addresses, PC-relative).
#[inline(always)]
fn ww(v: i64) -> Word {
    Word::new(wrap_carry(v).0).expect("wrapped")
}

/// Split a product of two words into (high word, low word), without the
/// slow 128-bit arithmetic: the quotient estimate from f64 is off by at most
/// one; the remainder is small, so wrapping 64-bit arithmetic gets it
/// exactly, and the high half always fits in a word (|hi| < W_MAX / 2 + 1).
#[inline(always)]
fn split_product27(x: i64, y: i64) -> (i64, i64) {
    let mut hi = (x as f64 * y as f64 / W_MOD as f64).round() as i64;
    let mut lo = x.wrapping_mul(y).wrapping_sub(hi.wrapping_mul(W_MOD));
    while lo > W_MAX {
        lo -= W_MOD;
        hi += 1;
    }
    while lo < -W_MAX {
        lo += W_MOD;
        hi -= 1;
    }
    (hi, lo)
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Cpu {
    pub fn new() -> Self {
        let mut c = Cpu {
            regs: [Word::ZERO; 27],
            pc: Word::ZERO,
            crs: [Word::ZERO; 27],
            mode_trits: [0; 28],
            cycle: 0,
            halted: false,
            inst_pc: Word::ZERO,
            fregs: [Real::ZERO; 27],
            fsr: Word::ZERO,
            batch: false,
            stop_batch: false,
            tlb: vec![TLB_EMPTY; 3 * TLB_SIZE].into_boxed_slice(),
            block_at: Vec::new(),
            blocks: Vec::new(),
            epoch: 1,
            ops: Vec::new(),
            fw_lo: i64::MIN,
            fw_delta: 0,
            fw_slot: 0,
            rw_lo: i64::MIN,
            rw_delta: 0,
            ww_lo: i64::MIN,
            ww_delta: 0,
            rw_frame: usize::MAX,
            ww_frame: usize::MAX,
        };
        c.reset();
        c
    }

    /// Reset state (ISA §9): ring +1, paging and interrupts off, PC at ROM.
    pub fn reset(&mut self) {
        self.regs = [Word::ZERO; 27];
        self.crs = [Word::ZERO; 27];
        self.crs[idx(cr::CPUID)] = Word::new(CPUID).unwrap();
        self.mode_trits = [0; 28];
        self.cycle = 0;
        self.set_mode(mode::RING, 1);
        self.set_mode(mode::FE, 1);
        self.fregs = [Real::ZERO; 27];
        self.fsr = Word::new(fsr::RESET).unwrap();
        self.pc = Word::new(RESET_PC).unwrap();
        self.halted = false;
        self.tlb_flush();
    }

    fn tlb_flush(&mut self) {
        self.tlb.fill(TLB_EMPTY);
        self.windows_off();
    }

    /// Forget the fetch and data windows.
    #[inline]
    fn windows_off(&mut self) {
        self.epoch += 1;
        self.fw_lo = i64::MIN;
        self.rw_lo = i64::MIN;
        self.ww_lo = i64::MIN;
    }

    /// Physical address of `va` for `acc` through a window, or the full
    /// path (alignment check for words, TLB, walk) and a new window.
    #[inline(always)]
    fn data_pa(&mut self, m: &mut Memory, va: i64, acc: Access, word: bool) -> Result<i64, Fault> {
        let (lo, delta) = if acc == Access::Write { (self.ww_lo, self.ww_delta) } else { (self.rw_lo, self.rw_delta) };
        let off = va.wrapping_sub(lo);
        if (0..PAGE).contains(&off) && (!word || off % 3 == 1) && !m.pt_dirty {
            return Ok(va + delta);
        }
        self.data_pa_slow(m, va, acc, word)
    }

    #[cold]
    fn data_pa_slow(&mut self, m: &mut Memory, va: i64, acc: Access, word: bool) -> Result<i64, Fault> {
        if word && va.rem_euclid(3) != 0 {
            return Err(Fault::mem(vector::MISALIGNED, va, acc));
        }
        let pa = self.xlate(m, va, acc)?;
        let start = (pa + HALF_PAGE).div_euclid(PAGE) * PAGE - HALF_PAGE;
        let lo = va - (pa - start);
        let frame = m.ram_frame(pa).unwrap_or(usize::MAX);
        if acc == Access::Write {
            (self.ww_lo, self.ww_delta, self.ww_frame) = (lo, pa - va, frame);
        } else {
            (self.rw_lo, self.rw_delta, self.rw_frame) = (lo, pa - va, frame);
        }
        Ok(pa)
    }

    /// A word load: straight from the RAM frame through the read window,
    /// else the general path.
    #[inline(always)]
    fn load_word_fast(&mut self, m: &mut Memory, va: i64) -> Result<Word, Fault> {
        let off = va.wrapping_sub(self.rw_lo);
        if (0..PAGE).contains(&off) && off % 3 == 1 && !m.pt_dirty && self.rw_frame != usize::MAX {
            return Ok(Word::new(m.ram_word(self.rw_frame, (off / 3) as usize)).expect("a word"));
        }
        let pa = self.data_pa_slow(m, va, Access::Read, true)?;
        m.read_word(pa).map_err(|_| Fault::mem(vector::BUS_ERROR, va, Access::Read))
    }

    /// A word store, likewise.
    #[inline(always)]
    fn store_word_fast(&mut self, m: &mut Memory, va: i64, v: Word) -> Result<(), Fault> {
        let off = va.wrapping_sub(self.ww_lo);
        if (0..PAGE).contains(&off) && off % 3 == 1 && !m.pt_dirty && self.ww_frame != usize::MAX {
            m.set_ram_word(self.ww_frame, (off / 3) as usize, v.value());
            return Ok(());
        }
        let pa = self.data_pa_slow(m, va, Access::Write, true)?;
        m.write_word(pa, v).map_err(|_| Fault::mem(vector::BUS_ERROR, va, Access::Write))
    }

    // ---- register access -------------------------------------------------

    pub fn reg(&self, r: i64) -> Word {
        self.regs[idx(r)]
    }

    pub fn set_reg(&mut self, r: i64, v: Word) {
        if r != 0 {
            self.regs[idx(r)] = v;
        }
    }

    /// FPU register `f` (f0 is always zero).
    pub fn freg(&self, f: i64) -> Real {
        self.fregs[idx(f)]
    }

    pub fn set_freg(&mut self, f: i64, v: Real) {
        if f != 0 {
            self.fregs[idx(f)] = v;
        }
    }

    pub fn fsr(&self) -> Word {
        self.fsr
    }

    pub fn cr(&self, n: i64) -> Word {
        match n {
            cr::MODE => {
                let v = self.mode_trits[1..].iter().fold(0i64, |acc, &t| acc * 3 + t as i64);
                Word::new(v).unwrap()
            }
            cr::CYCLE => Word::wrap(self.cycle as i128),
            _ => self.crs[idx(n)],
        }
    }

    pub fn set_cr(&mut self, n: i64, v: Word) {
        self.windows_off();
        if n == cr::PTB {
            self.tlb_flush();
        }
        if n == cr::MODE {
            for (i, t) in v.trits().iter().enumerate() {
                self.mode_trits[i + 1] = *t as i8;
            }
        } else {
            self.crs[idx(n)] = v;
        }
    }

    #[inline]
    pub fn mode(&self, pos: usize) -> i64 {
        self.mode_trits[pos] as i64
    }

    #[inline]
    pub fn set_mode(&mut self, pos: usize, v: i64) {
        if pos == mode::RING || pos == mode::PG {
            self.windows_off();
        }
        self.mode_trits[pos] = v.signum() as i8;
    }

    pub fn ring(&self) -> i64 {
        self.mode(mode::RING)
    }

    /// Sign flag S.
    pub fn s(&self) -> i64 {
        self.mode(mode::S)
    }

    /// Carry flag C.
    pub fn c(&self) -> i64 {
        self.mode(mode::C)
    }

    // ---- MMU (ISA §5) ----------------------------------------------------

    /// Translate a virtual address for `acc`, walking the 9|9|9 tables when
    /// paging is on.
    pub fn translate(&self, m: &mut Memory, va: i64, acc: Access) -> Result<i64, Fault> {
        if self.mode(mode::PG) != 1 {
            return Ok(va);
        }
        let a = Word::new(va).expect("virtual address is a word");
        let bus = |_| Fault::mem(vector::BUS_ERROR, va, acc);
        let ring = self.ring();

        let e1_addr = self.cr(cr::PTB).value() + 3 * a.field(1..=9);
        let e1 = m.read_word(e1_addr).map_err(bus)?;
        check_pte(e1, acc, ring, va)?;

        let e2_addr = PAGE * e1.field(1..=18) + 3 * a.field(10..=18);
        let e2 = m.read_word(e2_addr).map_err(bus)?;
        check_pte(e2, acc, ring, va)?;

        // Accessed / dirty trit: 0 untouched, −1 accessed, +1 written.
        let ad = e2.trit(22) as i64;
        let new_ad = if acc == Access::Write { 1 } else if ad == 0 { -1 } else { ad };
        if new_ad != ad {
            m.write_word(e2_addr, e2.with_field(22..=22, new_ad)).map_err(bus)?;
        }
        Ok(PAGE * e2.field(1..=18) + a.field(19..=27))
    }

    /// [`Cpu::translate`] through the TLB.
    #[inline]
    fn xlate(&mut self, m: &mut Memory, va: i64, acc: Access) -> Result<i64, Fault> {
        if self.mode_trits[mode::PG] != 1 {
            return Ok(va);
        }
        if m.pt_dirty {
            self.tlb_flush();
            m.clear_page_table_watch();
        }
        let vpage = (va + HALF_PAGE).div_euclid(PAGE);
        let i = (acc as i64 + 1) as usize * TLB_SIZE + (vpage as usize & (TLB_SIZE - 1));
        let e = self.tlb[i];
        if e.vpage == vpage && self.mode_trits[mode::RING] as i64 >= e.min_ring {
            return Ok(e.frame * PAGE + (va - vpage * PAGE));
        }
        self.walk_and_fill(m, va, acc, vpage, i)
    }

    /// A TLB miss: the full walk of [`Cpu::translate`], then cache the result.
    /// The entry is only cached once the accessed (or, for writes, dirty)
    /// trit is set, so hits never need to update it.
    #[cold]
    fn walk_and_fill(&mut self, m: &mut Memory, va: i64, acc: Access, vpage: i64, i: usize) -> Result<i64, Fault> {
        let a = Word::new(va).expect("virtual address is a word");
        let bus = |_| Fault::mem(vector::BUS_ERROR, va, acc);
        let ring = self.ring();

        let e1_addr = self.cr(cr::PTB).value() + 3 * a.field(1..=9);
        let e1 = m.read_word(e1_addr).map_err(bus)?;
        check_pte(e1, acc, ring, va)?;

        let e2_addr = PAGE * e1.field(1..=18) + 3 * a.field(10..=18);
        let e2 = m.read_word(e2_addr).map_err(bus)?;
        check_pte(e2, acc, ring, va)?;

        let ad = e2.trit(22) as i64;
        let new_ad = if acc == Access::Write { 1 } else if ad == 0 { -1 } else { ad };
        if new_ad != ad {
            m.write_word_quiet(e2_addr, e2.with_field(22..=22, new_ad)).map_err(bus)?;
        }
        let frame = e2.field(1..=18);
        m.watch_page_table(e1_addr);
        m.watch_page_table(e2_addr);
        let min_ring = (e1.trit(21) as i64).max(e2.trit(21) as i64);
        self.tlb[i] = TlbEntry { vpage, frame, min_ring };
        Ok(PAGE * frame + (va - vpage * PAGE))
    }

    fn load_tryte(&mut self, m: &mut Memory, va: i64) -> Result<Tryte, Fault> {
        let pa = self.xlate(m, va, Access::Read)?;
        m.read_tryte(pa).map_err(|_| Fault::mem(vector::BUS_ERROR, va, Access::Read))
    }

    fn store_tryte(&mut self, m: &mut Memory, va: i64, t: Tryte) -> Result<(), Fault> {
        let pa = self.xlate(m, va, Access::Write)?;
        m.write_tryte(pa, t).map_err(|_| Fault::mem(vector::BUS_ERROR, va, Access::Write))
    }

    fn load_word_as(&mut self, m: &mut Memory, va: i64, acc: Access) -> Result<Word, Fault> {
        if va.rem_euclid(3) != 0 {
            return Err(Fault::mem(vector::MISALIGNED, va, acc));
        }
        let pa = self.xlate(m, va, acc)?;
        m.read_word(pa).map_err(|_| Fault::mem(vector::BUS_ERROR, va, acc))
    }

    fn store_word(&mut self, m: &mut Memory, va: i64, v: Word) -> Result<(), Fault> {
        if va.rem_euclid(3) != 0 {
            return Err(Fault::mem(vector::MISALIGNED, va, Access::Write));
        }
        let pa = self.xlate(m, va, Access::Write)?;
        m.write_word(pa, v).map_err(|_| Fault::mem(vector::BUS_ERROR, va, Access::Write))
    }

    // ---- traps (ISA §7) --------------------------------------------------

    /// Enter the trap handler for `f`. `epc` is what `EPC` receives.
    fn deliver(&mut self, m: &mut Memory, f: Fault, epc: Word, depth: u8) -> Step {
        let ring = self.ring();
        let fcause = Word::ZERO
            .with_field(23..=27, f.vector)
            .with_field(22..=22, f.access as i64)
            .with_field(21..=21, ring)
            .with_field(1..=18, wrap_trits(f.info as i128, 18) as i64);
        self.set_cr(cr::EMODE, self.cr(cr::MODE));
        self.set_cr(cr::EPC, epc);
        self.set_cr(cr::FADDR, Word::wrap(f.addr as i128));
        self.set_cr(cr::FCAUSE, fcause);
        if ring < 1 {
            self.swap_stacks();
        }
        self.set_mode(mode::RING, 1);
        self.set_mode(mode::IE, 0);
        self.set_mode(mode::TF, 0);
        self.halted = false;

        let slot = self.cr(cr::ITB).value() + 3 * f.vector;
        match self.load_word_as(m, slot, Access::Read) {
            Ok(handler) => {
                self.pc = handler;
                Step::Ran
            }
            Err(_) if depth == 0 => {
                let df = Fault::new(vector::DOUBLE_FAULT);
                self.deliver(m, df, epc, 1)
            }
            Err(_) => {
                self.reset();
                Step::TripleFault
            }
        }
    }

    fn swap_stacks(&mut self) {
        let sp = self.reg(13);
        self.set_reg(13, self.cr(cr::KSP));
        self.set_cr(cr::KSP, sp);
    }

    // ---- execution -------------------------------------------------------

    /// Execute one instruction (or take one pending interrupt).
    pub fn step(&mut self, m: &mut Memory, io: &mut dyn Io) -> Step {
        if self.mode(mode::IE) == 1 {
            if let Some(v) = io.pending_vector() {
                let pc = self.pc;
                return self.deliver(m, Fault { resume_next: true, ..Fault::new(v) }, pc, 0);
            }
        }
        if self.halted {
            return Step::Halted;
        }

        let tf = self.mode(mode::TF) == 1;
        self.inst_pc = self.pc;
        self.cycle = self.cycle.wrapping_add(1);

        match self.execute(m, io) {
            Ok(()) => {
                if tf && !self.halted {
                    let pc = self.pc;
                    return self.deliver(m, Fault::new(vector::STEP), pc, 0);
                }
                Step::Ran
            }
            Err(f) => {
                let epc = if f.resume_next { self.pc } else { self.inst_pc };
                self.deliver(m, f, epc, 0)
            }
        }
    }

    /// Run up to `budget` steps with no device activity in between: the
    /// caller guarantees no interrupt can become pending during the batch
    /// and none is deliverable at its start. Stops early, without executing
    /// it, before an instruction that does port I/O; after one that may
    /// change `IE`/`TF` (`IRET`, `MTC`); and when `TF` is set. A halted CPU
    /// uses up the whole budget at once. Returns the steps taken, each one
    /// exactly what [`Cpu::step`] would have done.
    ///
    /// Code runs a basic block at a time: the PC, `CYCLE` and the faulting
    /// PC are only brought up to date at the block's end, on a fault (where
    /// they are recomputed exactly) and before an instruction that is not
    /// handled inline.
    pub fn run_batch(&mut self, m: &mut Memory, io: &mut dyn Io, budget: u64) -> u64 {
        if self.mode(mode::TF) == 1 {
            return 0;
        }
        let mut n = 0;
        self.batch = true;
        // the block just left cleanly, and the epoch then (for chaining)
        let (mut from, mut from_epoch) = (0u32, 0u64);
        while n < budget {
            if self.halted {
                n = budget;
                break;
            }
            if !m.code_dirty.is_empty() {
                self.drop_written_code(m);
            }
            let pc = self.pc.value();
            let chained = from != 0 && from_epoch == self.epoch && !m.pt_dirty;
            let mut id = 0;
            if chained {
                for l in &self.blocks[from as usize - 1].links {
                    if l.target == pc && l.epoch == self.epoch {
                        id = l.id;
                        break;
                    }
                }
            }
            if id == 0 {
                let off = pc.wrapping_sub(self.fw_lo);
                let found = if (0..PAGE).contains(&off) && off % 3 == 1 && !m.pt_dirty {
                    Some((self.fw_slot, (off / 3) as usize))
                } else {
                    self.fetch_slot(m, pc)
                };
                id = match found {
                    Some((slot, i)) => match self.block_at.get(slot).and_then(|t| t.as_ref()).map(|t| t[i]) {
                        Some(id) if id != 0 => id,
                        _ => self.build_block(m, pc + self.fw_delta, slot, i),
                    },
                    None => 0,
                };
                if id != 0 && chained && from_epoch == self.epoch {
                    let epoch = self.epoch;
                    let b = &mut self.blocks[from as usize - 1];
                    b.links[b.next_link as usize] = Link { target: pc, id, epoch };
                    b.next_link ^= 1;
                }
            }
            from = 0;
            if id == 0 {
                // misaligned, a fetch fault, no code slot, or an instruction
                // whose second word is in the next frame: the reference path
                let (counted, stop) = self.single(m, io);
                n += counted as u64;
                if stop {
                    break;
                }
                continue;
            }
            let Block { first, count, .. } = self.blocks[id as usize - 1];
            let limit = (count as u64).min(budget - n) as usize;
            let cycle0 = self.cycle;
            let mut pc = pc;
            let mut k = 0usize;
            let mut stop = false;
            let mut pc_done = false;
            while k < limit {
                let d = self.ops[first as usize + k];
                match self.exec_op(m, &d, pc) {
                    Ok(Ctl::Next) => {
                        pc += d.len as i64;
                        k += 1;
                        if matches!(d.op, Op::Stt | Op::Stw | Op::Swap) && (!m.code_dirty.is_empty() || m.pt_dirty) {
                            break; // it wrote code or a page table: look again
                        }
                    }
                    Ok(Ctl::Jump(target)) => {
                        pc = target;
                        k += 1;
                        break;
                    }
                    Ok(Ctl::Generic) => {
                        self.pc = ww(pc);
                        self.inst_pc = self.pc;
                        self.cycle = cycle0.wrapping_add(k as u64 + 1);
                        pc_done = true;
                        let (a, b) = (d.a as i64, d.f as i64);
                        let (x, y, z) = (d.x as i64, d.y as i64, d.z as i64);
                        let r = if bal_split(a, 6561).0 == -1 {
                            self.exec_fpu(m, a, b, x, y, z, pc)
                        } else {
                            self.exec_major(m, io, pc, a, b, x, y, z, d.imm, pc + d.len as i64)
                        };
                        match r {
                            Ok(()) => {
                                k += 1;
                                stop = std::mem::take(&mut self.stop_batch);
                            }
                            Err(f) if f.vector == DEFER => {
                                self.cycle = self.cycle.wrapping_sub(1);
                                stop = true;
                            }
                            Err(f) => {
                                let epc = if f.resume_next { self.pc } else { self.inst_pc };
                                self.deliver(m, f, epc, 0);
                                k += 1;
                            }
                        }
                        break;
                    }
                    Err(f) => {
                        // nothing of the instruction happened but the trap
                        self.pc = ww(pc);
                        self.inst_pc = self.pc;
                        self.cycle = cycle0.wrapping_add(k as u64 + 1);
                        pc_done = true;
                        let epc = self.inst_pc;
                        self.deliver(m, f, epc, 0);
                        k += 1;
                        break;
                    }
                }
            }
            if !pc_done {
                self.pc = ww(pc);
                self.cycle = cycle0.wrapping_add(k as u64);
                (from, from_epoch) = (id, self.epoch);
            }
            n += k as u64;
            if stop {
                break;
            }
        }
        self.batch = false;
        n
    }

    /// One step on the reference path (no interrupt check): returns whether
    /// it counted as a step, and whether the batch must end.
    fn single(&mut self, m: &mut Memory, io: &mut dyn Io) -> (bool, bool) {
        self.inst_pc = self.pc;
        self.cycle = self.cycle.wrapping_add(1);
        match self.execute(m, io) {
            Ok(()) => (true, std::mem::take(&mut self.stop_batch)),
            Err(f) if f.vector == DEFER => {
                self.cycle = self.cycle.wrapping_sub(1);
                (false, true)
            }
            Err(f) => {
                let epc = if f.resume_next { self.pc } else { self.inst_pc };
                self.deliver(m, f, epc, 0);
                (true, false)
            }
        }
    }

    /// Translate a fetch from `pc` and open the fetch window on its page.
    /// `None` when the reference path must handle it (misaligned, faulting,
    /// or not in RAM or ROM).
    fn fetch_slot(&mut self, m: &mut Memory, pc: i64) -> Option<(usize, usize)> {
        if pc.rem_euclid(3) != 0 {
            return None;
        }
        let pa = self.xlate(m, pc, Access::Fetch).ok()?;
        let frame = (pa + HALF_PAGE).div_euclid(PAGE);
        let slot = m.code_slot(frame)?;
        self.fw_lo = pc - (pa - (frame * PAGE - HALF_PAGE));
        self.fw_delta = pa - pc;
        self.fw_slot = slot;
        Some((slot, ((pa - frame * PAGE + HALF_PAGE - 1) / 3) as usize))
    }

    /// Decode the block starting at physical `pa` (word `i` of code slot
    /// `slot`); returns its id, or 0 if not even its first instruction fits.
    #[cold]
    fn build_block(&mut self, m: &mut Memory, pa: i64, slot: usize, i: usize) -> u32 {
        if self.block_at.len() != m.code_slots() {
            self.block_at = (0..m.code_slots()).map(|_| None).collect();
        }
        if self.ops.len() > MAX_OPS {
            self.epoch += 1;
            self.ops.clear();
            self.blocks.clear();
            self.block_at.iter_mut().for_each(|t| *t = None);
        }
        let first = self.ops.len();
        let mut k = i;
        while k < WORDS_PER_FRAME && self.ops.len() - first < MAX_BLOCK {
            let at = pa + 3 * (k - i) as i64;
            let Ok(w1) = m.read_word(at).map(|w| w.value()) else { break };
            let two = bal_split(bal_split(bal_split(w1, 19_683).0, 19_683).0, 6561).0 == 1;
            if two && k + 1 >= WORDS_PER_FRAME {
                break; // the second word is in the next frame
            }
            let w2 = if two { m.read_word(at + 3).map(|w| w.value()).unwrap_or(0) } else { 0 };
            let d = Self::decode(w1, w2);
            self.ops.push(d);
            k += if two { 2 } else { 1 };
            if matches!(d.op, Op::Jal | Op::Jalr | Op::Br | Op::Br3 | Op::Generic) {
                break;
            }
        }
        let count = self.ops.len() - first;
        if count == 0 {
            return 0;
        }
        self.blocks.push(Block { first: first as u32, count: count as u32, links: [Link::default(); 2], next_link: 0 });
        let id = self.blocks.len() as u32;
        self.block_at[slot].get_or_insert_with(|| vec![0; WORDS_PER_FRAME].into_boxed_slice())[i] = id;
        m.watch_code(slot);
        id
    }

    /// Decode the instruction with words `w1`, `w2` (the second only matters
    /// for 2-word forms).
    fn decode(w1: i64, w2: i64) -> Dec {
        let (ab, opnd) = bal_split(w1, 19_683);
        let (a, b) = bal_split(ab, 19_683);
        let (xy, z) = bal_split(opnd, 27);
        let (x, y) = bal_split(xy, 27);
        let imm6 = y * 27 + z;
        let len_trit = bal_split(a, 6561).0;
        let len = if len_trit == 1 { 6 } else { 3 };
        let imm = if len_trit == 1 { w2 } else { 0 };
        // `imm` starts as the raw second word, as `exec_major` wants it
        let mut d = Dec { op: Op::Generic, a: a as i16, x: x as i8, y: y as i8, z: z as i8, len, f: b as i16, imm, imm2: 0 };
        if len_trit == -1 {
            return d; // the FPU
        }
        match a {
            op::ALU => {
                d.op = match b {
                    alu::ADD => Op::AddR,
                    alu::SUB => Op::SubR,
                    alu::CMP => Op::CmpR,
                    _ => Op::AluR,
                }
            }
            op::ALUQ | op::ALUI => {
                d.imm = if a == op::ALUQ { z } else { imm };
                d.op = match b {
                    alu::ADD => Op::AddK,
                    alu::SUB => Op::SubK,
                    alu::CMP => Op::CmpK,
                    _ => Op::AluK,
                }
            }
            op::MEM | op::MEMD => {
                let size = if matches!(b, mem::LDT | mem::STT) { 1 } else { 3 };
                d.imm = if a == op::MEM { size * z } else { imm };
                d.op = match b {
                    mem::LDT => Op::Ldt,
                    mem::LDW => Op::Ldw,
                    mem::STT => Op::Stt,
                    mem::STW => Op::Stw,
                    mem::SWAP => Op::Swap,
                    _ => Op::Generic,
                };
            }
            op::JALR => (d.op, d.imm) = (Op::Jalr, 3 * z),
            op::LIQ => (d.op, d.imm) = (Op::Li, imm6),
            op::LI => (d.op, d.imm) = (Op::Li, imm),
            op::LEA => (d.op, d.imm) = (Op::Lea, imm),
            op::JAL => (d.op, d.imm) = (Op::Jal, imm),
            op::BRQ | op::BR => {
                let off = if a == op::BRQ { imm6 } else { imm };
                let t = Word::new(x).unwrap().resize::<3>().trits();
                let (pos, neg) = (t.contains(&Trit::P), t.contains(&Trit::N));
                if x == cond::NEVER || !(pos && neg) {
                    let want = if pos { Trit::P } else { Trit::N };
                    let mut mask = 0;
                    if x != cond::NEVER {
                        for (k, &tk) in t.iter().enumerate() {
                            if tk == want {
                                mask |= 1 << k;
                            }
                        }
                    }
                    d.op = Op::Br;
                    d.f = if pos { 1 } else { 2 };
                    d.imm = off;
                    d.imm2 = mask;
                }
            }
            op::BR3 => {
                let w = Word::new(imm).unwrap();
                d.op = Op::Br3;
                d.imm = w.field(1..=9);
                d.imm2 = w.field(10..=18) as i32;
            }
            _ => {}
        }
        d
    }

    /// Drop the blocks of frames that have been written.
    #[cold]
    fn drop_written_code(&mut self, m: &mut Memory) {
        self.epoch += 1;
        for s in m.code_dirty.drain(..) {
            if let Some(t) = self.block_at.get_mut(s) {
                *t = None;
            }
        }
    }

    /// Execute one decoded instruction at `pc` (all but the generic ones).
    #[inline(always)]
    fn exec_op(&mut self, m: &mut Memory, d: &Dec, pc: i64) -> Result<Ctl, Fault> {
        #[cfg(feature = "opstats")]
        opstats::count(d);
        let next = pc + d.len as i64;
        let (x, y, z) = (d.x as i64, d.y as i64, d.z as i64);
        match d.op {
            Op::AddR => {
                let r = self.arith(self.reg(y).value() + self.reg(z).value());
                self.set_s(r.sign());
                self.set_reg(x, r);
            }
            Op::SubR => {
                let r = self.arith(self.reg(y).value() - self.reg(z).value());
                self.set_s(r.sign());
                self.set_reg(x, r);
            }
            Op::CmpR => self.set_s(Trit::from_i64(self.reg(y).value() - self.reg(z).value())),
            Op::AddK => {
                let r = self.arith(self.reg(y).value() + d.imm);
                self.set_s(r.sign());
                self.set_reg(x, r);
            }
            Op::SubK => {
                let r = self.arith(self.reg(y).value() - d.imm);
                self.set_s(r.sign());
                self.set_reg(x, r);
            }
            Op::CmpK => self.set_s(Trit::from_i64(self.reg(y).value() - d.imm)),
            Op::AluR => {
                let (va, vb) = (self.reg(y), self.reg(z));
                self.alu_to(d.f as i64, x, va, vb)?;
            }
            Op::AluK => {
                let va = self.reg(y);
                self.alu_to(d.f as i64, x, va, Word::new(d.imm).unwrap())?;
            }
            Op::Ldt => {
                let va = wrap_carry(self.reg(y).value() + d.imm).0;
                let pa = self.data_pa(m, va, Access::Read, false)?;
                let t = m.read_tryte(pa).map_err(|_| Fault::mem(vector::BUS_ERROR, va, Access::Read))?;
                self.set_reg(x, Word::new(t.value()).expect("a tryte fits"));
            }
            Op::Ldw => {
                let va = wrap_carry(self.reg(y).value() + d.imm).0;
                let v = self.load_word_fast(m, va)?;
                self.set_reg(x, v);
            }
            Op::Stt => {
                let va = wrap_carry(self.reg(y).value() + d.imm).0;
                let pa = self.data_pa(m, va, Access::Write, false)?;
                m.write_tryte(pa, self.reg(x).resize()).map_err(|_| Fault::mem(vector::BUS_ERROR, va, Access::Write))?;
            }
            Op::Stw => {
                let va = wrap_carry(self.reg(y).value() + d.imm).0;
                self.store_word_fast(m, va, self.reg(x))?;
            }
            Op::Swap => {
                let va = wrap_carry(self.reg(y).value() + d.imm).0;
                let old = self.load_word_as(m, va, Access::Read)?;
                self.store_word(m, va, self.reg(x))?;
                self.set_reg(x, old);
            }
            Op::Jalr => {
                let target = ww(self.reg(y).value() + d.imm);
                self.set_reg(x, ww(next));
                return Ok(Ctl::Jump(target.value()));
            }
            Op::Li => self.set_reg(x, Word::new(d.imm).unwrap()),
            Op::Lea => self.set_reg(x, ww(pc + d.imm)),
            Op::Jal => {
                self.set_reg(x, ww(next));
                return Ok(Ctl::Jump(ww(pc + 3 * d.imm).value()));
            }
            Op::Br => {
                let flag = if d.f == 1 { self.s() } else { self.c() };
                if d.imm2 >> (flag + 1) & 1 != 0 {
                    return Ok(Ctl::Jump(ww(pc + 3 * d.imm).value()));
                }
                return Ok(Ctl::Jump(ww(next).value()));
            }
            Op::Br3 => {
                return Ok(Ctl::Jump(match self.s() {
                    -1 => ww(pc + 3 * d.imm).value(),
                    1 => ww(pc + 3 * d.imm2 as i64).value(),
                    _ => ww(next).value(),
                }))
            }
            Op::Generic => {
                let _ = z;
                return Ok(Ctl::Generic);
            }
        }
        Ok(Ctl::Next)
    }

    /// Fetch, decode and execute the instruction at `PC`. On success `PC`
    /// points at the next instruction; on a fault nothing but `PC` (and, for
    /// `resume_next` faults, the advanced `PC`) has changed.
    fn execute(&mut self, m: &mut Memory, io: &mut dyn Io) -> Result<(), Fault> {
        let pc = self.pc.value();
        let w1 = self.load_word_as(m, pc, Access::Fetch)?;
        // Decode: A = trits 1–9, B = 10–18, operand tryte = 19–27 (X Y Z).
        let (ab, opnd) = bal_split(w1.value(), 19_683);
        let (a, b) = bal_split(ab, 19_683);
        let (xy, z) = bal_split(opnd, 27);
        let (x, y) = bal_split(xy, 27);

        // A[1], the length trit, is the leading trit of A.
        let len_trit = bal_split(a, 6561).0;
        let (imm, next) = match len_trit {
            0 => (0, pc + 3),
            1 => (self.load_word_as(m, pc + 3, Access::Fetch)?.value(), pc + 6),
            _ => return self.exec_fpu(m, a, b, x, y, z, pc),
        };
        self.exec_major(m, io, pc, a, b, x, y, z, imm, next)
    }

    /// Execute a decoded non-FPU instruction: major `a`, function `b`,
    /// operands, the second word `imm` (0 for 1-word forms), `next` = its end.
    #[allow(clippy::too_many_arguments)]
    fn exec_major(
        &mut self,
        m: &mut Memory,
        io: &mut dyn Io,
        pc: i64,
        a: i64,
        b: i64,
        x: i64,
        y: i64,
        z: i64,
        imm: i64,
        next: i64,
    ) -> Result<(), Fault> {
        let imm6 = y * 27 + z;
        let ill = || Fault::new(vector::ILLEGAL);
        let next_w = w(next as i128);
        let rel = |off: i64| w(pc as i128 + 3 * off as i128);

        match a {
            op::SYS => return self.exec_sys(b, x, y, z, next_w),
            op::ALU => {
                let (va, vb) = (self.reg(y), self.reg(z));
                self.alu_to(b, x, va, vb)?;
            }
            op::ALUQ => {
                let va = self.reg(y);
                self.alu_to(b, x, va, Word::new(z).unwrap())?;
            }
            op::ALUI => {
                let va = self.reg(y);
                self.alu_to(b, x, va, Word::new(imm).unwrap())?;
            }
            op::MEM | op::MEMD => {
                let base = self.reg(y).value() as i128;
                let size = if matches!(b, mem::LDT | mem::STT) { 1 } else { 3 };
                let off = if a == op::MEM { size * z as i128 } else { imm as i128 };
                let va = w(base + off).value();
                match b {
                    mem::LDT => {
                        let t = self.load_tryte(m, va)?;
                        self.set_reg(x, t.resize());
                    }
                    mem::LDW => {
                        let v = self.load_word_as(m, va, Access::Read)?;
                        self.set_reg(x, v);
                    }
                    mem::STT => self.store_tryte(m, va, self.reg(x).resize())?,
                    mem::STW => self.store_word(m, va, self.reg(x))?,
                    mem::SWAP => {
                        let old = self.load_word_as(m, va, Access::Read)?;
                        self.store_word(m, va, self.reg(x))?;
                        self.set_reg(x, old);
                    }
                    _ => return Err(ill()),
                }
            }
            op::IO | op::IOI => {
                if self.batch {
                    return Err(Fault::new(DEFER));
                }
                if self.ring() < self.mode(mode::IOPL) {
                    return Err(Fault::new(vector::PRIVILEGE));
                }
                let port = if a == op::IO { self.reg(y).value() + z } else { imm };
                match b {
                    port::IN => {
                        let v = io.port_in(port);
                        self.set_reg(x, v);
                    }
                    port::OUT => io.port_out(port, self.reg(x)),
                    _ => return Err(ill()),
                }
            }
            op::JALR => {
                let target = w(self.reg(y).value() as i128 + 3 * z as i128);
                self.set_reg(x, next_w);
                self.pc = target;
                return Ok(());
            }
            op::LIQ => self.set_reg(x, Word::new(imm6).unwrap()),
            op::LI => self.set_reg(x, Word::new(imm).unwrap()),
            op::LEA => self.set_reg(x, w(pc as i128 + imm as i128)),
            op::JAL => {
                self.set_reg(x, next_w);
                self.pc = rel(imm);
                return Ok(());
            }
            op::BRQ | op::BR => {
                let off = if a == op::BRQ { imm6 } else { imm };
                if self.condition(x)? {
                    self.pc = rel(off);
                    return Ok(());
                }
            }
            op::BR3 => {
                let w2 = Word::new(imm).unwrap();
                match self.s() {
                    -1 => {
                        self.pc = rel(w2.field(1..=9));
                        return Ok(());
                    }
                    1 => {
                        self.pc = rel(w2.field(10..=18));
                        return Ok(());
                    }
                    _ => {}
                }
            }
            op::FLD => {
                let w2 = Word::new(imm).unwrap();
                let (pos, len) = (w2.field(1..=9), w2.field(10..=18));
                if pos < 1 || len < 1 || pos + len - 1 > 27 {
                    return Err(ill());
                }
                let (p, e) = (pos as usize, (pos + len - 1) as usize);
                let r = match b {
                    fld::EXT => Word::new(self.reg(y).field(p..=e)).unwrap(),
                    fld::INS => self.reg(x).with_field(p..=e, self.reg(y).value()),
                    _ => return Err(ill()),
                };
                self.set_s(r.sign());
                self.set_reg(x, r);
            }
            _ => return Err(ill()),
        }
        self.pc = next_w;
        Ok(())
    }

    // ---- the Setun-387 (ISA §14) ------------------------------------------

    /// Record exception flags: set the sticky FSR flags, and trap (vector
    /// −11, before any result is written) on the first unmasked one.
    fn fpu_flags(&mut self, flags: u8) -> Result<(), Fault> {
        if flags == 0 {
            return Ok(());
        }
        let bits = [setun_real::IV, setun_real::DZ, setun_real::OF, setun_real::UF, setun_real::IX];
        for (i, bit) in bits.iter().enumerate() {
            if flags & bit != 0 {
                let k = fsr::FLAG_FIRST + i;
                self.fsr = self.fsr.with_field(k..=k, 1);
            }
        }
        for (i, bit) in bits.iter().enumerate() {
            if flags & bit != 0 && self.fsr.trit(fsr::MASK_FIRST + i) as i64 != 1 {
                return Err(Fault { info: i as i64 + 1, ..Fault::new(vector::FP_EXCEPTION) });
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn exec_fpu(&mut self, m: &mut Memory, a: i64, b: i64, x: i64, y: i64, z: i64, pc: i64) -> Result<(), Fault> {
        let ill = || Fault::new(vector::ILLEGAL);
        // A[2] sets the length. Reserved majors are ILL whether or not the
        // unit is enabled; known ones trap as unavailable when it is off.
        let two_words = match bal_split(a + 6561, 2187).0 {
            0 => false,
            1 => true,
            _ => return Err(ill()),
        };
        if !matches!(a, op::FALU | op::FCVT | op::FMEM | op::FMEMD | op::FLI) {
            return Err(ill());
        }
        if self.mode(mode::FE) != 1 {
            return Err(Fault::new(vector::FPU_UNAVAILABLE));
        }
        let (imm, next) =
            if two_words { (self.load_word_as(m, pc + 3, Access::Fetch)?.value(), pc + 6) } else { (0, pc + 3) };
        match a {
            op::FALU => {
                let (fa, fb) = (self.freg(y), self.freg(z));
                let (r, flags) = match b {
                    falu::FADD => fa.add(fb),
                    falu::FSUB => fa.sub(fb),
                    falu::FMUL => fa.mul(fb),
                    falu::FDIV => fa.div(fb),
                    falu::FREM => fa.rem(fb),
                    falu::FMIN => (fa.min(fb), 0),
                    falu::FMAX => (fa.max(fb), 0),
                    falu::FSQRT => fa.sqrt(),
                    falu::FNEG => (fa.neg(), 0),
                    falu::FABS => (fa.abs(), 0),
                    falu::FRND => fa.round_int(false),
                    falu::FTRUNC => fa.round_int(true),
                    falu::FCMP => {
                        match fa.cmp(fb) {
                            Some(o) => {
                                self.set_mode(mode::S, o as i64);
                                self.set_c(0);
                            }
                            None => {
                                self.set_mode(mode::S, 0);
                                self.set_c(1); // unordered
                            }
                        }
                        self.pc = w(next as i128);
                        return Ok(());
                    }
                    _ => return Err(ill()),
                };
                self.fpu_flags(flags)?;
                self.set_freg(x, r);
            }
            op::FCVT => match b {
                fcvt::FCVTIF => self.set_freg(x, Real::from_i64(self.reg(y).value())),
                fcvt::FCVTFI => {
                    let (v, f) = self.freg(y).to_i64();
                    self.fpu_flags(f)?;
                    self.set_reg(x, Word::new(v).unwrap());
                }
                fcvt::FTOW => {
                    let (v, f) = self.freg(y).to_word27();
                    self.fpu_flags(f)?;
                    self.set_reg(x, Word::new(v).unwrap());
                }
                fcvt::FFRW => self.set_freg(x, Real::from_word27(self.reg(y).value())),
                fcvt::FEXP => {
                    let (e, f) = self.freg(y).exponent();
                    self.fpu_flags(f)?;
                    self.set_reg(x, Word::new(e).unwrap());
                }
                fcvt::FSCL => {
                    let (r, f) = self.freg(y).scale(self.reg(z).value());
                    self.fpu_flags(f)?;
                    self.set_freg(x, r);
                }
                fcvt::FMFS => self.set_reg(x, self.fsr),
                fcvt::FMTS => {
                    // only the flag and mask fields can be written
                    let v = self.reg(y);
                    let mut f = Word::ZERO;
                    for k in (fsr::MASK_FIRST..fsr::MASK_FIRST + 5).chain(fsr::FLAG_FIRST..fsr::FLAG_FIRST + 5) {
                        f = f.with_field(k..=k, v.trit(k) as i64);
                    }
                    self.fsr = f;
                }
                fcvt::FCLASS => self.set_reg(x, Word::new(self.freg(y).class()).unwrap()),
                fcvt::FCONST => self.set_freg(x, Real::constant(y).ok_or_else(ill)?),
                fcvt::FMOV => self.set_freg(x, self.freg(y)),
                _ => return Err(ill()),
            },
            op::FMEM | op::FMEMD => {
                let words = b.abs();
                if !(1..=2).contains(&words) {
                    return Err(ill());
                }
                let size = if words == 1 { 3 } else { 9 };
                let off = if a == op::FMEM { size * z as i128 } else { imm as i128 };
                let va = w(self.reg(y).value() as i128 + off).value();
                let n = if words == 1 { 1 } else { 3 };
                if b > 0 {
                    let mut ws = [0i64; 3];
                    for (k, slot) in ws.iter_mut().enumerate().take(n) {
                        *slot = self.load_word_as(m, va + 3 * k as i64, Access::Read)?.value();
                    }
                    self.set_freg(x, Real::from_words(ws));
                } else {
                    let ws = if words == 1 {
                        let (v, f) = self.freg(x).to_word27();
                        self.fpu_flags(f)?;
                        [v, 0, 0]
                    } else {
                        self.freg(x).to_words()
                    };
                    // all or nothing: every word must be writable first
                    for k in 0..n {
                        let at = va + 3 * k as i64;
                        if at.rem_euclid(3) != 0 {
                            return Err(Fault::mem(vector::MISALIGNED, at, Access::Write));
                        }
                        self.xlate(m, at, Access::Write)?;
                    }
                    for (k, v) in ws.iter().enumerate().take(n) {
                        self.store_word(m, va + 3 * k as i64, Word::new(*v).unwrap())?;
                    }
                }
            }
            op::FLI => self.set_freg(x, Real::from_word27(imm)),
            _ => return Err(ill()),
        }
        self.pc = w(next as i128);
        Ok(())
    }

    fn exec_sys(&mut self, b: i64, x: i64, y: i64, z: i64, next: Word) -> Result<(), Fault> {
        let privileged = matches!(
            b,
            sys::HLT | sys::IRET | sys::MFC | sys::MTC | sys::TLBINV | sys::TLBFLUSH
        );
        if privileged && self.ring() != 1 {
            return Err(Fault::new(vector::PRIVILEGE));
        }
        match b {
            sys::NOP | sys::FENCE => {}
            sys::TLBINV | sys::TLBFLUSH => self.tlb_flush(),
            sys::HLT => self.halted = true,
            sys::BRK => {
                self.pc = next;
                return Err(Fault { resume_next: true, ..Fault::new(vector::BREAKPOINT) });
            }
            sys::SYS => {
                self.pc = next;
                let n = x * 729 + y * 27 + z;
                return Err(Fault { info: n, resume_next: true, ..Fault::new(vector::SYSCALL) });
            }
            sys::IRET => {
                self.stop_batch = self.batch;
                self.set_cr(cr::MODE, self.cr(cr::EMODE));
                self.pc = self.cr(cr::EPC);
                if self.ring() < 1 {
                    self.swap_stacks();
                }
                return Ok(());
            }
            sys::MFC => {
                let v = self.cr(y);
                self.set_reg(x, v);
            }
            sys::MTC => {
                if !(cr::MODE..=cr::SCRATCH).contains(&x) {
                    return Err(Fault::new(vector::ILLEGAL));
                }
                self.set_cr(x, self.reg(y));
                self.stop_batch = self.batch;
            }
            _ => return Err(Fault::new(vector::ILLEGAL)),
        }
        self.pc = next;
        Ok(())
    }

    fn condition(&self, c: i64) -> Result<bool, Fault> {
        if c == cond::NEVER {
            return Ok(false);
        }
        let t = Word::new(c).unwrap().resize::<3>().trits();
        let (pos, neg) = (t.contains(&Trit::P), t.contains(&Trit::N));
        if pos && neg {
            return Err(Fault::new(vector::ILLEGAL));
        }
        let flag = if pos { self.s() } else { self.c() };
        let want = if pos { Trit::P } else { Trit::N };
        Ok(t[(flag + 1) as usize] == want)
    }

    fn set_s(&mut self, s: Trit) {
        self.set_mode(mode::S, s as i64);
    }

    fn set_c(&mut self, c: i64) {
        self.set_mode(mode::C, c);
    }

    /// Wrap an exact arithmetic result (at most a few words) and record its
    /// carry trit.
    #[inline(always)]
    fn arith(&mut self, v: i64) -> Word {
        let (lo, c) = wrap_carry(v);
        self.set_c(c);
        Word::new(lo).expect("wrapped")
    }

    fn alu_to(&mut self, f: i64, rd: i64, a: Word, b: Word) -> Result<(), Fault> {
        if let Some(r) = self.alu(f, a, b)? {
            self.set_reg(rd, r);
        }
        Ok(())
    }

    /// ALU operation (ISA §3.3). Sets S always and C for arithmetic.
    /// Returns `None` for `CMP`, which writes no register.
    fn alu(&mut self, f: i64, a: Word, b: Word) -> Result<Option<Word>, Fault> {
        let (x, y) = (a.value(), b.value());
        let div0 = || Fault::new(vector::DIVIDE);
        let r = match f {
            alu::ADD => self.arith(x + y),
            alu::SUB => self.arith(x - y),
            alu::ADC => self.arith(x + y + self.c()),
            alu::SBC => self.arith(x - y - self.c()),
            alu::MUL => {
                let (hi, lo) = split_product27(x, y);
                self.set_c(hi.signum());
                Word::new(lo).expect("wrapped")
            }
            alu::MULH => Word::new(split_product27(x, y).0).expect("wrapped"),
            alu::DIV | alu::MOD => {
                if y == 0 {
                    return Err(div0());
                }
                ww(if f == alu::DIV { x / y } else { x % y })
            }
            alu::DIVR | alu::MODR => {
                if y == 0 {
                    return Err(div0());
                }
                let md = y.abs();
                let mut r = x.rem_euclid(md);
                if 2 * r > md {
                    r -= md;
                }
                ww(if f == alu::DIVR { (x - r) / y } else { r })
            }
            alu::MIN => a.tritwise_min(b),
            alu::MAX => a.tritwise_max(b),
            alu::TMUL => a.tritwise_mul(b),
            alu::TSAT => a.tritwise_sat(b),
            alu::CONS => a.tritwise_cons(b),
            alu::SH => a.shift(y.clamp(-40, 40) as i32),
            alu::CMP => {
                self.set_s(Trit::from_i64((x - y).signum()));
                return Ok(None);
            }
            alu::TRIT => {
                if (1..=27).contains(&y) {
                    Word::new(a.trit(y as usize) as i64).unwrap()
                } else {
                    Word::ZERO
                }
            }
            alu::NEG => -a,
            alu::SGN => Word::new(a.sign() as i64).unwrap(),
            alu::ABS => ww(x.abs()),
            alu::TLZ => Word::new(a.leading_zero_trits() as i64).unwrap(),
            _ => return Err(Fault::new(vector::ILLEGAL)),
        };
        self.set_s(r.sign());
        Ok(Some(r))
    }
}

/// Check one page-table entry against an access (ISA §5.3).
fn check_pte(e: Word, acc: Access, ring: i64, va: i64) -> Result<(), Fault> {
    let (v, x, r) = (e.trit(19) as i64, e.trit(20) as i64, e.trit(21) as i64);
    if v == 0 {
        return Err(Fault::mem(vector::PAGE_FAULT, va, acc));
    }
    let ok = ring >= r
        && match acc {
            Access::Write => v == 1,
            Access::Fetch => x != 0,
            Access::Read => x != -1,
        };
    if ok {
        Ok(())
    } else {
        Err(Fault::mem(vector::PROTECTION, va, acc))
    }
}

/// Dynamic instruction mix (feature `opstats`): counts per decoded kind,
/// ALU function and, for the generic path, major opcode.
#[cfg(feature = "opstats")]
pub mod opstats {
    use std::sync::Mutex;
    pub static COUNTS: Mutex<Vec<(String, u64)>> = Mutex::new(Vec::new());

    pub(super) fn count(d: &super::Dec) {
        let key = match d.op {
            super::Op::AluR | super::Op::AluK => format!("{:?} f{}", d.op, d.f),
            o => format!("{o:?}"),
        };
        let mut c = COUNTS.lock().unwrap();
        match c.iter_mut().find(|(k, _)| *k == key) {
            Some(e) => e.1 += 1,
            None => c.push((key, 1)),
        }
    }

    pub fn report() -> Vec<(String, u64)> {
        let mut c = COUNTS.lock().unwrap().clone();
        c.sort_by(|a, b| b.1.cmp(&a.1));
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_product27_matches_the_exact_split() {
        let edge = [0, 1, -1, 2, W_MAX, -W_MAX, W_MAX - 1, 3_812_798_742_492 / 3, 1_162_261_467, -1_162_261_467];
        let mut vals: Vec<i64> = edge.to_vec();
        let mut s = 0x2545_F491_4F6C_DD1Du64;
        for _ in 0..20_000 {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            vals.push((s % (2 * W_MAX as u64 + 1)) as i64 - W_MAX);
        }
        for (k, &a) in vals.iter().enumerate() {
            let b = vals[(k * 7919 + 13) % vals.len()];
            for (x, y) in [(a, b), (a, a), (a, -b), (W_MAX, b)] {
                let (hi, lo) = split_product27(x, y);
                let (ehi, elo) = ternary::split_product::<27>(x as i128 * y as i128);
                assert_eq!((hi, lo), (ehi.value(), elo.value()), "{x} * {y}");
            }
        }
    }
}
