//! Setun-386 assembler back end: a two-pass program builder with labels.
//!
//! Every item has a size known when it is added, so pass 1 assigns
//! addresses and pass 2 encodes, resolving label references. Addresses are
//! word addresses (word centres, multiples of 3); branch offsets count words.
//!
//! ```
//! use setun_asm::Asm;
//! use setun_core::isa::cond;
//! let mut a = Asm::new(0);
//! a.li(1, 10).li(2, 0);
//! a.label("top").add(2, 2, 1).subq(1, 1, 1).brq(cond::GT, "top").hlt();
//! let words = a.assemble().unwrap();
//! assert_eq!(words.len(), 6);
//! ```

use setun_core::isa::{self, alu, cond, fld, mem, op, sys};
use std::collections::HashMap;
use std::fmt;
use ternary::{pow3, Word};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsmError {
    UndefinedLabel(String),
    DuplicateLabel(String),
    OutOfRange { what: &'static str, value: i64, at: i64 },
}

impl fmt::Display for AsmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AsmError::UndefinedLabel(l) => write!(f, "undefined label `{l}`"),
            AsmError::DuplicateLabel(l) => write!(f, "label `{l}` defined twice"),
            AsmError::OutOfRange { what, value, at } => {
                write!(f, "{what} {value} out of range (instruction at {at})")
            }
        }
    }
}

impl std::error::Error for AsmError {}

/// Label lookup available while encoding.
pub struct Ctx<'a> {
    labels: &'a HashMap<String, i64>,
    /// Address of the item being encoded.
    pub here: i64,
}

impl Ctx<'_> {
    pub fn addr(&self, label: &str) -> Result<i64, AsmError> {
        self.labels.get(label).copied().ok_or_else(|| AsmError::UndefinedLabel(label.into()))
    }

    /// Word offset from the current instruction to `label`.
    pub fn rel(&self, label: &str) -> Result<i64, AsmError> {
        Ok((self.addr(label)? - self.here) / 3)
    }
}

type Encoder = Box<dyn Fn(&Ctx) -> Result<Vec<Word>, AsmError>>;

enum Item {
    Words(Vec<Word>),
    Label(String),
    Lazy(usize, Encoder),
}

pub struct Asm {
    origin: i64,
    items: Vec<Item>,
}

fn check(what: &'static str, v: i64, lim: i64, at: i64) -> Result<i64, AsmError> {
    if v.abs() <= lim {
        Ok(v)
    } else {
        Err(AsmError::OutOfRange { what, value: v, at })
    }
}

/// Pack trytes big-endian into words (padded with zero trytes).
pub fn pack_trytes(trytes: &[i64]) -> Vec<Word> {
    trytes
        .chunks(3)
        .map(|c| {
            let t = |i: usize| c.get(i).copied().unwrap_or(0) as i128;
            Word::wrap(t(0) * pow3(18) + t(1) * pow3(9) + t(2))
        })
        .collect()
}

impl Asm {
    /// A program whose first word is centred at `origin` (a multiple of 3).
    pub fn new(origin: i64) -> Self {
        assert_eq!(origin.rem_euclid(3), 0, "origin must be word-aligned");
        Asm { origin, items: Vec::new() }
    }

    pub fn origin(&self) -> i64 {
        self.origin
    }

    fn push(&mut self, w: Word) -> &mut Self {
        self.items.push(Item::Words(vec![w]));
        self
    }

    fn push2(&mut self, w: [Word; 2]) -> &mut Self {
        self.items.push(Item::Words(w.to_vec()));
        self
    }

    fn lazy(&mut self, len: usize, f: impl Fn(&Ctx) -> Result<Vec<Word>, AsmError> + 'static) -> &mut Self {
        self.items.push(Item::Lazy(len, Box::new(f)));
        self
    }

    pub fn label(&mut self, name: &str) -> &mut Self {
        self.items.push(Item::Label(name.into()));
        self
    }

    /// Resolve labels and encode. Returns the program's words.
    pub fn assemble(&self) -> Result<Vec<Word>, AsmError> {
        Ok(self.assemble_with_labels()?.0)
    }

    pub fn assemble_with_labels(&self) -> Result<(Vec<Word>, HashMap<String, i64>), AsmError> {
        let mut labels = HashMap::new();
        let mut at = self.origin;
        for it in &self.items {
            match it {
                Item::Words(w) => at += 3 * w.len() as i64,
                Item::Lazy(n, _) => at += 3 * *n as i64,
                Item::Label(l) => {
                    if labels.insert(l.clone(), at).is_some() {
                        return Err(AsmError::DuplicateLabel(l.clone()));
                    }
                }
            }
        }
        let mut out = Vec::new();
        for it in &self.items {
            let here = self.origin + 3 * out.len() as i64;
            match it {
                Item::Words(w) => out.extend_from_slice(w),
                Item::Lazy(n, f) => {
                    let w = f(&Ctx { labels: &labels, here })?;
                    debug_assert_eq!(w.len(), *n);
                    out.extend(w);
                }
                Item::Label(_) => {}
            }
        }
        Ok((out, labels))
    }

    // ---- data --------------------------------------------------------------

    pub fn word(&mut self, v: i64) -> &mut Self {
        self.push(Word::new(v).expect("data word out of range"))
    }

    /// Word holding a label's address.
    pub fn word_addr(&mut self, label: &str) -> &mut Self {
        let l = label.to_string();
        self.lazy(1, move |c| Ok(vec![Word::new(c.addr(&l)?).unwrap()]))
    }

    /// Trytes packed 3 per word. The first tryte is at `label − 1` for a
    /// label placed just before.
    pub fn trytes(&mut self, t: &[i64]) -> &mut Self {
        self.items.push(Item::Words(pack_trytes(t)));
        self
    }

    /// A NUL-terminated string, one character (Unicode code point) per tryte.
    pub fn string(&mut self, s: &str) -> &mut Self {
        let mut t: Vec<i64> = s.chars().map(|c| c as i64).collect();
        t.push(0);
        self.trytes(&t)
    }

    // ---- ALU ---------------------------------------------------------------

    pub fn alu(&mut self, f: i64, rd: i64, ra: i64, rb: i64) -> &mut Self {
        self.push(isa::rrr(op::ALU, f, rd, ra, rb))
    }

    pub fn aluq(&mut self, f: i64, rd: i64, ra: i64, q: i64) -> &mut Self {
        self.push(isa::rrr(op::ALUQ, f, rd, ra, q))
    }

    pub fn alui(&mut self, f: i64, rd: i64, ra: i64, imm: i64) -> &mut Self {
        self.push2(isa::wide(op::ALUI, f, rd, ra, imm))
    }

    pub fn add(&mut self, rd: i64, ra: i64, rb: i64) -> &mut Self {
        self.alu(alu::ADD, rd, ra, rb)
    }

    pub fn sub(&mut self, rd: i64, ra: i64, rb: i64) -> &mut Self {
        self.alu(alu::SUB, rd, ra, rb)
    }

    pub fn addq(&mut self, rd: i64, ra: i64, q: i64) -> &mut Self {
        self.aluq(alu::ADD, rd, ra, q)
    }

    pub fn subq(&mut self, rd: i64, ra: i64, q: i64) -> &mut Self {
        self.aluq(alu::SUB, rd, ra, q)
    }

    pub fn addi(&mut self, rd: i64, ra: i64, imm: i64) -> &mut Self {
        self.alui(alu::ADD, rd, ra, imm)
    }

    pub fn mov(&mut self, rd: i64, ra: i64) -> &mut Self {
        self.alu(alu::ADD, rd, ra, 0)
    }

    pub fn cmp(&mut self, ra: i64, rb: i64) -> &mut Self {
        self.alu(alu::CMP, 0, ra, rb)
    }

    pub fn cmpq(&mut self, ra: i64, q: i64) -> &mut Self {
        self.aluq(alu::CMP, 0, ra, q)
    }

    pub fn cmpi(&mut self, ra: i64, imm: i64) -> &mut Self {
        self.alui(alu::CMP, 0, ra, imm)
    }

    /// Load a constant: 1 word if it fits ±364, else 2.
    pub fn li(&mut self, rd: i64, v: i64) -> &mut Self {
        if v.abs() <= 364 {
            self.push(isa::s6(op::LIQ, 0, rd, v))
        } else {
            self.push2(isa::wide(op::LI, 0, rd, 0, v))
        }
    }

    /// Load a label's address (2 words).
    pub fn la(&mut self, rd: i64, label: &str) -> &mut Self {
        let l = label.to_string();
        self.lazy(2, move |c| Ok(isa::wide(op::LI, 0, rd, 0, c.addr(&l)?).to_vec()))
    }

    pub fn ext(&mut self, rd: i64, ra: i64, pos: i64, len: i64) -> &mut Self {
        self.push2(isa::fld(fld::EXT, rd, ra, pos, len))
    }

    pub fn ins(&mut self, rd: i64, ra: i64, pos: i64, len: i64) -> &mut Self {
        self.push2(isa::fld(fld::INS, rd, ra, pos, len))
    }

    // ---- memory ------------------------------------------------------------

    /// 1-word access at `ra + q·size`.
    pub fn mem(&mut self, f: i64, rd: i64, ra: i64, q: i64) -> &mut Self {
        self.push(isa::rrr(op::MEM, f, rd, ra, q))
    }

    pub fn ldt(&mut self, rd: i64, ra: i64, q: i64) -> &mut Self {
        self.mem(mem::LDT, rd, ra, q)
    }

    pub fn stt(&mut self, rd: i64, ra: i64, q: i64) -> &mut Self {
        self.mem(mem::STT, rd, ra, q)
    }

    pub fn ldw(&mut self, rd: i64, ra: i64, q: i64) -> &mut Self {
        self.mem(mem::LDW, rd, ra, q)
    }

    pub fn stw(&mut self, rd: i64, ra: i64, q: i64) -> &mut Self {
        self.mem(mem::STW, rd, ra, q)
    }

    /// 2-word access at `ra + disp` (trytes).
    pub fn memd(&mut self, f: i64, rd: i64, ra: i64, disp: i64) -> &mut Self {
        self.push2(isa::wide(op::MEMD, f, rd, ra, disp))
    }

    pub fn push_reg(&mut self, r: i64) -> &mut Self {
        self.subq(13, 13, 3).stw(r, 13, 0)
    }

    pub fn pop_reg(&mut self, r: i64) -> &mut Self {
        self.ldw(r, 13, 0).addq(13, 13, 3)
    }

    // ---- I/O ---------------------------------------------------------------

    /// `IN rd, [ra + q]`.
    pub fn inp(&mut self, rd: i64, ra: i64, q: i64) -> &mut Self {
        self.push(isa::rrr(op::IO, isa::io::IN, rd, ra, q))
    }

    /// `OUT rd, [ra + q]`.
    pub fn out(&mut self, rd: i64, ra: i64, q: i64) -> &mut Self {
        self.push(isa::rrr(op::IO, isa::io::OUT, rd, ra, q))
    }

    pub fn in_port(&mut self, rd: i64, port: i64) -> &mut Self {
        self.push2(isa::wide(op::IOI, isa::io::IN, rd, 0, port))
    }

    pub fn out_port(&mut self, rd: i64, port: i64) -> &mut Self {
        self.push2(isa::wide(op::IOI, isa::io::OUT, rd, 0, port))
    }

    // ---- control flow --------------------------------------------------------

    /// 1-word conditional branch (±364 words).
    pub fn brq(&mut self, c: i64, label: &str) -> &mut Self {
        let l = label.to_string();
        self.lazy(1, move |ctx| {
            let off = check("BRQ offset", ctx.rel(&l)?, 364, ctx.here)?;
            Ok(vec![isa::s6(op::BRQ, 0, c, off)])
        })
    }

    /// 2-word conditional branch (any distance).
    pub fn br(&mut self, c: i64, label: &str) -> &mut Self {
        let l = label.to_string();
        self.lazy(2, move |ctx| Ok(isa::wide(op::BR, 0, c, 0, ctx.rel(&l)?).to_vec()))
    }

    pub fn jmp(&mut self, label: &str) -> &mut Self {
        self.brq(cond::ALWAYS, label)
    }

    /// 3-way branch on S: negative → `neg`, positive → `pos`, zero falls through.
    pub fn br3(&mut self, neg: &str, pos: &str) -> &mut Self {
        let (n, p) = (neg.to_string(), pos.to_string());
        self.lazy(2, move |ctx| {
            let on = check("BR3 offset", ctx.rel(&n)?, 9841, ctx.here)?;
            let op_ = check("BR3 offset", ctx.rel(&p)?, 9841, ctx.here)?;
            Ok(isa::br3(on, op_).to_vec())
        })
    }

    pub fn jal(&mut self, rd: i64, label: &str) -> &mut Self {
        let l = label.to_string();
        self.lazy(2, move |ctx| Ok(isa::wide(op::JAL, 0, rd, 0, ctx.rel(&l)?).to_vec()))
    }

    pub fn call(&mut self, label: &str) -> &mut Self {
        self.jal(12, label)
    }

    pub fn jalr(&mut self, rd: i64, ra: i64, q: i64) -> &mut Self {
        self.push(isa::rrr(op::JALR, 0, rd, ra, q))
    }

    pub fn ret(&mut self) -> &mut Self {
        self.jalr(0, 12, 0)
    }

    // ---- system --------------------------------------------------------------

    pub fn sys(&mut self, b: i64, x: i64, y: i64) -> &mut Self {
        self.push(isa::rrr(op::SYS, b, x, y, 0))
    }

    pub fn hlt(&mut self) -> &mut Self {
        self.sys(sys::HLT, 0, 0)
    }

    pub fn nop(&mut self) -> &mut Self {
        self.sys(sys::NOP, 0, 0)
    }

    pub fn iret(&mut self) -> &mut Self {
        self.sys(sys::IRET, 0, 0)
    }

    pub fn mfc(&mut self, rd: i64, c: i64) -> &mut Self {
        self.sys(sys::MFC, rd, c)
    }

    pub fn mtc(&mut self, c: i64, ra: i64) -> &mut Self {
        self.sys(sys::MTC, c, ra)
    }

    pub fn syscall(&mut self, n: i64) -> &mut Self {
        self.push(isa::s9(op::SYS, sys::SYS, n))
    }
}

/// Convert assembled words into trytes (for ROM images and disk sectors).
pub fn words_to_trytes(words: &[Word]) -> Vec<i16> {
    words
        .iter()
        .flat_map(|&w| setun_core::memory::word_to_trytes(w).map(|t| t.value() as i16))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use setun_core::{Machine, Step};

    fn run(a: &Asm) -> Machine {
        let mut m = Machine::new(2, 0);
        m.mem.load_words(a.origin(), &a.assemble().unwrap());
        m.cpu.pc = Word::new(a.origin()).unwrap();
        for _ in 0..100_000 {
            if m.step() == Step::Halted {
                break;
            }
        }
        m
    }

    #[test]
    fn loop_with_labels() {
        let mut a = Asm::new(0);
        a.li(1, 10).li(2, 0);
        a.label("top").add(2, 2, 1).subq(1, 1, 1).brq(cond::GT, "top").hlt();
        assert_eq!(run(&a).cpu.reg(2).value(), 55);
    }

    #[test]
    fn forward_references_calls_and_strings() {
        let mut a = Asm::new(300);
        a.la(1, "msg").subq(1, 1, 1).li(3, 0);
        a.call("strlen").hlt();
        a.label("strlen");
        a.label("next").ldt(2, 1, 0).cmpq(2, 0).brq(cond::EQ, "done");
        a.addq(1, 1, 1).addq(3, 3, 1).jmp("next");
        a.label("done").ret();
        a.label("msg").string("Привет, Setun!");
        assert_eq!(run(&a).cpu.reg(3).value(), 14);
    }

    #[test]
    fn three_way_and_far_branches() {
        let mut a = Asm::new(0);
        a.li(1, -5).cmpq(1, 0).br3("neg", "pos").li(2, 0).hlt();
        a.label("neg").li(2, -1).br(cond::ALWAYS, "end");
        a.label("pos").li(2, 1);
        a.label("end").hlt();
        assert_eq!(run(&a).cpu.reg(2).value(), -1);
    }

    #[test]
    fn errors() {
        let mut a = Asm::new(0);
        a.jmp("nowhere");
        assert_eq!(a.assemble(), Err(AsmError::UndefinedLabel("nowhere".into())));
        let mut a = Asm::new(0);
        a.label("x").label("x");
        assert_eq!(a.assemble(), Err(AsmError::DuplicateLabel("x".into())));
        let mut a = Asm::new(0);
        a.brq(cond::ALWAYS, "far");
        for _ in 0..400 {
            a.nop();
        }
        a.label("far");
        assert!(matches!(a.assemble(), Err(AsmError::OutOfRange { .. })));
    }
}
