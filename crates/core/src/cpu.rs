//! Setun-386 CPU: registers, decode/execute, traps and the MMU.

use crate::io::Io;
use crate::isa::{alu, cond, cr, fld, io as port, mem, mode, op, sys, vector};
use crate::memory::Memory;
use crate::PAGE;
use ternary::{pow3, split_product, wrap_trits, Trit, Tryte, Word};

/// PC after reset: −3^10, the centre of BIOS ROM frame −3.
pub const RESET_PC: i64 = -59_049;

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
    pub halted: bool,
    /// Current instruction's address, for fault reporting.
    inst_pc: Word,
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
            halted: false,
            inst_pc: Word::ZERO,
        };
        c.reset();
        c
    }

    /// Reset state (ISA §9): ring +1, paging and interrupts off, PC at ROM.
    pub fn reset(&mut self) {
        self.regs = [Word::ZERO; 27];
        self.crs = [Word::ZERO; 27];
        self.set_mode(mode::RING, 1);
        self.pc = Word::new(RESET_PC).unwrap();
        self.halted = false;
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

    pub fn cr(&self, n: i64) -> Word {
        self.crs[idx(n)]
    }

    pub fn set_cr(&mut self, n: i64, v: Word) {
        self.crs[idx(n)] = v;
    }

    pub fn mode(&self, pos: usize) -> i64 {
        self.crs[idx(cr::MODE)].trit(pos) as i64
    }

    pub fn set_mode(&mut self, pos: usize, v: i64) {
        let m = self.crs[idx(cr::MODE)];
        self.crs[idx(cr::MODE)] = m.with_field(pos..=pos, v);
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

    fn load_tryte(&self, m: &mut Memory, va: i64) -> Result<Tryte, Fault> {
        let pa = self.translate(m, va, Access::Read)?;
        m.read_tryte(pa).map_err(|_| Fault::mem(vector::BUS_ERROR, va, Access::Read))
    }

    fn store_tryte(&self, m: &mut Memory, va: i64, t: Tryte) -> Result<(), Fault> {
        let pa = self.translate(m, va, Access::Write)?;
        m.write_tryte(pa, t).map_err(|_| Fault::mem(vector::BUS_ERROR, va, Access::Write))
    }

    fn load_word_as(&self, m: &mut Memory, va: i64, acc: Access) -> Result<Word, Fault> {
        if va.rem_euclid(3) != 0 {
            return Err(Fault::mem(vector::MISALIGNED, va, acc));
        }
        let pa = self.translate(m, va, acc)?;
        m.read_word(pa).map_err(|_| Fault::mem(vector::BUS_ERROR, va, acc))
    }

    fn store_word(&self, m: &mut Memory, va: i64, v: Word) -> Result<(), Fault> {
        if va.rem_euclid(3) != 0 {
            return Err(Fault::mem(vector::MISALIGNED, va, Access::Write));
        }
        let pa = self.translate(m, va, Access::Write)?;
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
        let cycle = self.cr(cr::CYCLE);
        self.set_cr(cr::CYCLE, w(cycle.value() as i128 + 1));

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

    /// Fetch, decode and execute the instruction at `PC`. On success `PC`
    /// points at the next instruction; on a fault nothing but `PC` (and, for
    /// `resume_next` faults, the advanced `PC`) has changed.
    fn execute(&mut self, m: &mut Memory, io: &mut dyn Io) -> Result<(), Fault> {
        let pc = self.pc.value();
        let w1 = self.load_word_as(m, pc, Access::Fetch)?;
        let a = w1.field(1..=9);
        let b = w1.field(10..=18);
        let (x, y, z) = (w1.field(19..=21), w1.field(22..=24), w1.field(25..=27));
        let ill = || Fault::new(vector::ILLEGAL);

        let (imm, next) = match w1.trit(1) {
            Trit::Z => (0, pc + 3),
            Trit::P => (self.load_word_as(m, pc + 3, Access::Fetch)?.value(), pc + 6),
            Trit::N => return Err(ill()),
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
            op::LIQ => self.set_reg(x, Word::new(w1.field(22..=27)).unwrap()),
            op::LI => self.set_reg(x, Word::new(imm).unwrap()),
            op::LEA => self.set_reg(x, w(pc as i128 + imm as i128)),
            op::JAL => {
                self.set_reg(x, next_w);
                self.pc = rel(imm);
                return Ok(());
            }
            op::BRQ | op::BR => {
                let off = if a == op::BRQ { w1.field(22..=27) } else { imm };
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

    fn exec_sys(&mut self, b: i64, x: i64, y: i64, z: i64, next: Word) -> Result<(), Fault> {
        let privileged = matches!(
            b,
            sys::HLT | sys::IRET | sys::MFC | sys::MTC | sys::TLBINV | sys::TLBFLUSH
        );
        if privileged && self.ring() != 1 {
            return Err(Fault::new(vector::PRIVILEGE));
        }
        match b {
            sys::NOP | sys::FENCE | sys::TLBINV | sys::TLBFLUSH => {}
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
                w(if f == alu::DIV { x / y } else { x % y })
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
                w(if f == alu::DIVR { (x - r) / y } else { r })
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
