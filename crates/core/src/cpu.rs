//! Setun-386 CPU: registers, decode/execute, traps and the MMU.

use crate::io::Io;
use crate::isa::{alu, cond, cr, falu, fcvt, fld, fsr, io as port, mem, mode, op, sys, vector};
use crate::memory::Memory;
use crate::{HALF_PAGE, PAGE};
use setun_real::Real;
use ternary::{pow3, split_product, wrap_trits, Trit, Tryte, Word};

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
    /// Decoded instructions per code slot (see `Memory::code_slot`), one
    /// per word; dropped when the frame is written.
    icache: Vec<Option<Box<[Dec]>>>,
}

/// A decoded instruction. `Generic` ones go through [`Cpu::execute`].
#[derive(Clone, Copy)]
struct Dec {
    op: Op,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
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

const DEC_EMPTY: Dec = Dec { op: Op::Generic, x: 0, y: 0, z: 0, len: 0, f: 0, imm: 0, imm2: 0 };
const WORDS_PER_FRAME: usize = (PAGE / 3) as usize;

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
            icache: Vec::new(),
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
    pub fn run_batch(&mut self, m: &mut Memory, io: &mut dyn Io, budget: u64) -> u64 {
        let mut n = 0;
        self.batch = true;
        while n < budget {
            if self.halted {
                n = budget;
                break;
            }
            if self.mode(mode::TF) == 1 {
                break;
            }
            self.inst_pc = self.pc;
            self.cycle = self.cycle.wrapping_add(1);
            match self.execute_fast(m, io) {
                Ok(()) => {
                    n += 1;
                    if self.stop_batch {
                        self.stop_batch = false;
                        break;
                    }
                }
                Err(f) if f.vector == DEFER => {
                    self.cycle = self.cycle.wrapping_sub(1);
                    break;
                }
                Err(f) => {
                    let epc = if f.resume_next { self.pc } else { self.inst_pc };
                    self.deliver(m, f, epc, 0);
                    n += 1;
                }
            }
        }
        self.batch = false;
        n
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
        let mut d = Dec { op: Op::Generic, x: x as i8, y: y as i8, z: z as i8, len, f: b as i16, imm: 0, imm2: 0 };
        if len_trit == -1 {
            return d; // the FPU
        }
        match a {
            op::ALU => d.op = Op::AluR,
            op::ALUQ => (d.op, d.imm) = (Op::AluK, z),
            op::ALUI => (d.op, d.imm) = (Op::AluK, imm),
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

    /// Drop decoded instructions of frames that have been written.
    #[cold]
    fn drop_written_code(&mut self, m: &mut Memory) {
        for s in m.code_dirty.drain(..) {
            if let Some(c) = self.icache.get_mut(s) {
                *c = None;
            }
        }
    }

    /// [`Cpu::execute`] through the decoded-instruction cache.
    #[inline]
    fn execute_fast(&mut self, m: &mut Memory, io: &mut dyn Io) -> Result<(), Fault> {
        if !m.code_dirty.is_empty() {
            self.drop_written_code(m);
        }
        let pc = self.pc.value();
        if pc.rem_euclid(3) != 0 {
            return self.execute(m, io);
        }
        let pa = self.xlate(m, pc, Access::Fetch)?;
        let frame = (pa + HALF_PAGE).div_euclid(PAGE);
        let Some(slot) = m.code_slot(frame) else { return self.execute(m, io) };
        let i = ((pa - frame * PAGE + HALF_PAGE - 1) / 3) as usize;
        if self.icache.len() != m.code_slots() {
            self.icache = (0..m.code_slots()).map(|_| None).collect();
        }
        let d = match &self.icache[slot] {
            Some(c) if c[i].len != 0 => c[i],
            _ => {
                let w1 = m.read_word(pa).map_err(|_| Fault::mem(vector::BUS_ERROR, pc, Access::Fetch))?.value();
                let last = i + 1 == WORDS_PER_FRAME;
                let two = bal_split(bal_split(bal_split(w1, 19_683).0, 19_683).0, 6561).0 == 1;
                if two && last {
                    return self.execute(m, io); // the second word is in the next frame
                }
                let w2 = if two { m.read_word(pa + 3).map(|w| w.value()).unwrap_or(0) } else { 0 };
                let d = Self::decode(w1, w2);
                let c = self.icache[slot].get_or_insert_with(|| vec![DEC_EMPTY; WORDS_PER_FRAME].into_boxed_slice());
                c[i] = d;
                m.watch_code(slot);
                d
            }
        };
        let next = pc + d.len as i64;
        let (x, y, z) = (d.x as i64, d.y as i64, d.z as i64);
        match d.op {
            Op::AluR => {
                let (va, vb) = (self.reg(y), self.reg(z));
                self.alu_to(d.f as i64, x, va, vb)?;
            }
            Op::AluK => {
                let va = self.reg(y);
                self.alu_to(d.f as i64, x, va, Word::new(d.imm).unwrap())?;
            }
            Op::Ldt => {
                let va = w(self.reg(y).value() as i128 + d.imm as i128).value();
                let t = self.load_tryte(m, va)?;
                self.set_reg(x, t.resize());
            }
            Op::Ldw => {
                let va = w(self.reg(y).value() as i128 + d.imm as i128).value();
                let v = self.load_word_as(m, va, Access::Read)?;
                self.set_reg(x, v);
            }
            Op::Stt => {
                let va = w(self.reg(y).value() as i128 + d.imm as i128).value();
                self.store_tryte(m, va, self.reg(x).resize())?;
            }
            Op::Stw => {
                let va = w(self.reg(y).value() as i128 + d.imm as i128).value();
                self.store_word(m, va, self.reg(x))?;
            }
            Op::Swap => {
                let va = w(self.reg(y).value() as i128 + d.imm as i128).value();
                let old = self.load_word_as(m, va, Access::Read)?;
                self.store_word(m, va, self.reg(x))?;
                self.set_reg(x, old);
            }
            Op::Jalr => {
                let target = w(self.reg(y).value() as i128 + d.imm as i128);
                self.set_reg(x, w(next as i128));
                self.pc = target;
                return Ok(());
            }
            Op::Li => self.set_reg(x, Word::new(d.imm).unwrap()),
            Op::Lea => self.set_reg(x, w(pc as i128 + d.imm as i128)),
            Op::Jal => {
                self.set_reg(x, w(next as i128));
                self.pc = w(pc as i128 + 3 * d.imm as i128);
                return Ok(());
            }
            Op::Br => {
                let flag = if d.f == 1 { self.s() } else { self.c() };
                if d.imm2 >> (flag + 1) & 1 != 0 {
                    self.pc = w(pc as i128 + 3 * d.imm as i128);
                    return Ok(());
                }
            }
            Op::Br3 => match self.s() {
                -1 => {
                    self.pc = w(pc as i128 + 3 * d.imm as i128);
                    return Ok(());
                }
                1 => {
                    self.pc = w(pc as i128 + 3 * d.imm2 as i128);
                    return Ok(());
                }
                _ => {}
            },
            Op::Generic => {
                let _ = z;
                return self.execute(m, io);
            }
        }
        self.pc = w(next as i128);
        Ok(())
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
        let imm6 = y * 27 + z;
        let ill = || Fault::new(vector::ILLEGAL);

        // A[1], the length trit, is the leading trit of A.
        let len_trit = bal_split(a, 6561).0;
        let (imm, next) = match len_trit {
            0 => (0, pc + 3),
            1 => (self.load_word_as(m, pc + 3, Access::Fetch)?.value(), pc + 6),
            _ => return self.exec_fpu(m, a, b, x, y, z, pc),
        };
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

    /// Wrap an exact arithmetic result and record its carry trit.
    fn arith(&mut self, v: i128) -> Word {
        let lo = wrap_trits(v, 27);
        self.set_c(((v - lo) / pow3(27)) as i64);
        Word::wrap(lo)
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
        let (x, y) = (a.value() as i128, b.value() as i128);
        let div0 = || Fault::new(vector::DIVIDE);
        let r = match f {
            alu::ADD => self.arith(x + y),
            alu::SUB => self.arith(x - y),
            alu::ADC => self.arith(x + y + self.c() as i128),
            alu::SBC => self.arith(x - y - self.c() as i128),
            alu::MUL => {
                let (hi, lo) = split_product::<27>(x * y);
                self.set_c(hi.sign() as i64);
                lo
            }
            alu::MULH => split_product::<27>(x * y).0,
            alu::DIV | alu::MOD => {
                if y == 0 {
                    return Err(div0());
                }
                let (x, y) = (x as i64, y as i64); // 64-bit: much faster than i128
                w((if f == alu::DIV { x / y } else { x % y }) as i128)
            }
            alu::DIVR | alu::MODR => {
                if y == 0 {
                    return Err(div0());
                }
                let (x, y) = (x as i64, y as i64);
                let md = y.abs();
                let mut r = x.rem_euclid(md);
                if 2 * r > md {
                    r -= md;
                }
                w((if f == alu::DIVR { (x - r) / y } else { r }) as i128)
            }
            alu::MIN => a.tritwise_min(b),
            alu::MAX => a.tritwise_max(b),
            alu::TMUL => a.tritwise_mul(b),
            alu::TSAT => a.tritwise_sat(b),
            alu::CONS => a.tritwise_cons(b),
            alu::SH => a.shift(y.clamp(-40, 40) as i32),
            alu::CMP => {
                self.set_s(Trit::from_i64((x - y).signum() as i64));
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
            alu::ABS => w(x.abs()),
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
