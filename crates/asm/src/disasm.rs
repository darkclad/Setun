//! Setun-386 disassembler.
//!
//! Output uses the assembly syntax of [`crate::text`], and every line is
//! checked by reassembling it: if the text would not produce the identical
//! word(s) (non-canonical encodings, data), the word is shown as `.word`.
//! So disassembly output always reassembles byte-for-byte.

use crate::text;
use setun_core::isa::{alu, cond, cr, falu, fcvt, fld, fmem, io, mem, op, sys};
use ternary::Word;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fields {
    a: i64,
    b: i64,
    x: i64,
    y: i64,
    z: i64,
    imm6: i64,
    imm9: i64,
}

fn fields(w: Word) -> Fields {
    let f = |r: std::ops::RangeInclusive<usize>| w.field(r);
    Fields {
        a: f(1..=9),
        b: f(10..=18),
        x: f(19..=21),
        y: f(22..=24),
        z: f(25..=27),
        imm6: f(22..=27),
        imm9: f(19..=27),
    }
}

fn r(n: i64) -> String {
    format!("r{n}")
}

fn alu_name(f: i64) -> Option<&'static str> {
    Some(match f {
        alu::ADD => "add",
        alu::SUB => "sub",
        alu::ADC => "adc",
        alu::SBC => "sbc",
        alu::MUL => "mul",
        alu::MULH => "mulh",
        alu::DIV => "div",
        alu::MOD => "mod",
        alu::DIVR => "divr",
        alu::MODR => "modr",
        alu::MIN => "min",
        alu::MAX => "max",
        alu::TMUL => "tmul",
        alu::TSAT => "tsat",
        alu::CONS => "cons",
        alu::SH => "sh",
        alu::CMP => "cmp",
        alu::TRIT => "trit",
        alu::NEG => "neg",
        alu::SGN => "sgn",
        alu::ABS => "abs",
        alu::TLZ => "tlz",
        _ => return None,
    })
}

fn cond_name(c: i64) -> Option<&'static str> {
    Some(match c {
        cond::ALWAYS => "b",
        cond::LT => "blt",
        cond::EQ => "beq",
        cond::GT => "bgt",
        cond::LE => "ble",
        cond::GE => "bge",
        cond::NE => "bne",
        cond::OV => "bov",
        cond::NOV => "bnov",
        cond::NEVER => "bnever",
        _ => return None,
    })
}

fn cr_name(n: i64) -> String {
    match n {
        cr::MODE => "mode".into(),
        cr::PTB => "ptb".into(),
        cr::FADDR => "faddr".into(),
        cr::FCAUSE => "fcause".into(),
        cr::ITB => "itb".into(),
        cr::EPC => "epc".into(),
        cr::EMODE => "emode".into(),
        cr::KSP => "ksp".into(),
        cr::SCRATCH => "scratch".into(),
        cr::CPUID => "cpuid".into(),
        cr::CYCLE => "cycle".into(),
        n => format!("cr{n}"),
    }
}

fn mem_name(b: i64) -> Option<(&'static str, i64)> {
    Some(match b {
        mem::LDT => ("ldt", 1),
        mem::LDW => ("ldw", 3),
        mem::STT => ("stt", 1),
        mem::STW => ("stw", 3),
        mem::SWAP => ("swap", 3),
        _ => return None,
    })
}

fn addr(base: i64, off: i64) -> String {
    match off {
        0 => format!("[{}]", r(base)),
        o if o > 0 => format!("[{}+{o}]", r(base)),
        o => format!("[{}{o}]", r(base)),
    }
}

/// Raw decode to text (unchecked). Returns (text, words consumed).
fn decode(w1: Word, w2: Option<Word>, pc: i64) -> Option<(String, usize)> {
    let f = fields(w1);
    let long = w1.trit(1) as i64 == 1;
    if w1.trit(1) as i64 == -1 {
        return decode_fpu(f, w1, w2);
    }
    let imm = if long { w2?.value() } else { 0 };
    let n = if long { 2 } else { 1 };
    let tgt = |off: i64| pc + 3 * off;
    let s = match f.a {
        op::SYS => match f.b {
            sys::ILL => "ill".into(),
            sys::NOP => "nop".into(),
            sys::HLT => "hlt".into(),
            sys::BRK => "brk".into(),
            sys::SYS => format!("sys {}", f.imm9),
            sys::IRET => "iret".into(),
            sys::MFC => format!("mfc {}, {}", r(f.x), cr_name(f.y)),
            sys::MTC => format!("mtc {}, {}", cr_name(f.x), r(f.y)),
            sys::TLBINV => format!("tlbinv {}", r(f.y)),
            sys::TLBFLUSH => "tlbflush".into(),
            sys::FENCE => "fence".into(),
            _ => return None,
        },
        op::ALU | op::ALUQ | op::ALUI => {
            let name = alu_name(f.b)?;
            let unary = matches!(f.b, alu::NEG | alu::SGN | alu::ABS | alu::TLZ);
            let b = match f.a {
                op::ALU => r(f.z),
                op::ALUQ => f.z.to_string(),
                _ => imm.to_string(),
            };
            let dot = if f.a == op::ALUI && imm.abs() <= 13 { ".l" } else { "" };
            if unary {
                format!("{name} {}, {}", r(f.x), r(f.y))
            } else if f.b == alu::CMP {
                format!("cmp{dot} {}, {b}", r(f.y))
            } else if f.a == op::ALU && f.b == alu::ADD && f.z == 0 {
                format!("mov {}, {}", r(f.x), r(f.y))
            } else {
                format!("{name}{dot} {}, {}, {b}", r(f.x), r(f.y))
            }
        }
        op::MEM | op::MEMD => {
            let (name, size) = mem_name(f.b)?;
            if f.a == op::MEM {
                format!("{name} {}, {}", r(f.x), addr(f.y, f.z * size))
            } else {
                let dot = if imm % size == 0 && (imm / size).abs() <= 13 { ".l" } else { "" };
                format!("{name}{dot} {}, {}", r(f.x), addr(f.y, imm))
            }
        }
        op::IO | op::IOI => {
            let name = match f.b {
                io::IN => "in",
                io::OUT => "out",
                _ => return None,
            };
            if f.a == op::IO {
                format!("{name} {}, {}", r(f.x), addr(f.y, f.z))
            } else {
                format!("{name} {}, {imm}", r(f.x))
            }
        }
        op::JALR => {
            if (f.x, f.y, f.z) == (0, 12, 0) {
                "ret".into()
            } else {
                format!("jalr {}, {}, {}", r(f.x), r(f.y), f.z)
            }
        }
        op::LIQ => format!("li {}, {}", r(f.x), f.imm6),
        op::LI => {
            let dot = if imm.abs() <= 364 { ".l" } else { "" };
            format!("li{dot} {}, {imm}", r(f.x))
        }
        op::LEA => format!("lea {}, {}", r(f.x), pc + imm),
        op::JAL => {
            if f.x == 12 {
                format!("call {}", tgt(imm))
            } else {
                format!("jal {}, {}", r(f.x), tgt(imm))
            }
        }
        op::BRQ => format!("{} {}", cond_name(f.x)?, tgt(f.imm6)),
        op::BR => {
            let dot = if imm.abs() <= 364 { ".l" } else { "" };
            format!("{}{dot} {}", cond_name(f.x)?, tgt(imm))
        }
        op::BR3 => {
            let w2 = Word::new(imm).unwrap();
            format!("br3 {}, {}", tgt(w2.field(1..=9)), tgt(w2.field(10..=18)))
        }
        op::FLD => {
            let w2 = Word::new(imm).unwrap();
            let name = match f.b {
                fld::EXT => "ext",
                fld::INS => "ins",
                _ => return None,
            };
            format!("{name} {}, {}, {}, {}", r(f.x), r(f.y), w2.field(1..=9), w2.field(10..=18))
        }
        _ => return None,
    };
    Some((s, n))
}

fn fr(n: i64) -> String {
    format!("f{n}")
}

/// The Setun-387 instructions (ISA §14.5).
fn decode_fpu(f: Fields, w1: Word, w2: Option<Word>) -> Option<(String, usize)> {
    let long = w1.trit(2) as i64 == 1;
    let imm = if long { w2?.value() } else { 0 };
    let n = if long { 2 } else { 1 };
    let s = match f.a {
        op::FALU => {
            let bin = match f.b {
                falu::FADD => Some("fadd"),
                falu::FSUB => Some("fsub"),
                falu::FMUL => Some("fmul"),
                falu::FDIV => Some("fdiv"),
                falu::FREM => Some("frem"),
                falu::FMIN => Some("fmin"),
                falu::FMAX => Some("fmax"),
                _ => None,
            };
            let un = match f.b {
                falu::FSQRT => Some("fsqrt"),
                falu::FNEG => Some("fneg"),
                falu::FABS => Some("fabs"),
                falu::FRND => Some("frnd"),
                falu::FTRUNC => Some("ftrunc"),
                _ => None,
            };
            if let Some(name) = bin {
                format!("{name} {}, {}, {}", fr(f.x), fr(f.y), fr(f.z))
            } else if let Some(name) = un {
                format!("{name} {}, {}", fr(f.x), fr(f.y))
            } else if f.b == falu::FCMP {
                format!("fcmp {}, {}", fr(f.y), fr(f.z))
            } else {
                return None;
            }
        }
        op::FCVT => match f.b {
            fcvt::FCVTIF => format!("fcvtif {}, {}", fr(f.x), r(f.y)),
            fcvt::FCVTFI => format!("fcvtfi {}, {}", r(f.x), fr(f.y)),
            fcvt::FTOW => format!("ftow {}, {}", r(f.x), fr(f.y)),
            fcvt::FFRW => format!("ffrw {}, {}", fr(f.x), r(f.y)),
            fcvt::FEXP => format!("fexp {}, {}", r(f.x), fr(f.y)),
            fcvt::FSCL => format!("fscl {}, {}, {}", fr(f.x), fr(f.y), r(f.z)),
            fcvt::FMFS => format!("fmfs {}", r(f.x)),
            fcvt::FMTS => format!("fmts {}", r(f.y)),
            fcvt::FCLASS => format!("fclass {}, {}", r(f.x), fr(f.y)),
            fcvt::FCONST => {
                let names = ["0", "one", "pi", "e", "ln3", "log3e", "log3_10"];
                format!("fconst {}, {}", fr(f.x), names.get(f.y as usize).filter(|_| f.y >= 0)?)
            }
            fcvt::FMOV => format!("fmov {}, {}", fr(f.x), fr(f.y)),
            _ => return None,
        },
        op::FMEM | op::FMEMD => {
            let (name, size) = match f.b {
                fmem::FLDS => ("flds", 3),
                fmem::FLDD => ("fldd", 9),
                fmem::FSTS => ("fsts", 3),
                fmem::FSTD => ("fstd", 9),
                _ => return None,
            };
            if f.a == op::FMEM {
                format!("{name} {}, {}", fr(f.x), addr(f.y, f.z * size))
            } else {
                let dot = if imm % size == 0 && (imm / size).abs() <= 13 { ".l" } else { "" };
                format!("{name}{dot} {}, {}", fr(f.x), addr(f.y, imm))
            }
        }
        op::FLI => {
            let v = setun_real::Real::from_word27(imm);
            let t = v.to_decimal(12);
            let t = if t.contains(['.', 'e']) { t } else { format!("{t}.0") };
            format!("fli {}, {t}", fr(f.x))
        }
        _ => return None,
    };
    Some((s, n))
}

/// Disassemble one instruction at `pc`. Returns the text and the number of
/// words it occupies. Falls back to `.word` when the text would not
/// reassemble to the same words.
pub fn disasm_one(w1: Word, w2: Option<Word>, pc: i64) -> (String, usize) {
    if let Some((text, n)) = decode(w1, w2, pc) {
        let expect: Vec<Word> = if n == 2 { vec![w1, w2.unwrap()] } else { vec![w1] };
        let src = format!(".org {pc}\n{text}\n");
        if let Ok(p) = text::assemble(&src) {
            if p.segments.first().map(|s| &s.words) == Some(&expect) {
                return (text, n);
            }
        }
    }
    (format!(".word {}", w1.value()), 1)
}

/// Disassemble a run of words starting at word address `origin`, one line
/// per instruction: `address  words(base 27)  text`.
pub fn disasm(words: &[Word], origin: i64) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < words.len() {
        let pc = origin + 3 * i as i64;
        let (text, n) = disasm_one(words[i], words.get(i + 1).copied(), pc);
        let hex: Vec<String> = words[i..i + n].iter().map(|w| w.to_b27()).collect();
        out += &format!("{pc:>14}  {:<20} {text}\n", hex.join(" "));
        i += n;
    }
    out
}

/// Disassemble to plain source (no addresses), which reassembles exactly.
pub fn disasm_source(words: &[Word], origin: i64) -> String {
    let mut out = format!("        .org {origin}\n");
    let mut i = 0;
    while i < words.len() {
        let pc = origin + 3 * i as i64;
        let (text, n) = disasm_one(words[i], words.get(i + 1).copied(), pc);
        out += &format!("        {text}\n");
        i += n;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples() {
        let src = "\
            .org 30
            add r1, r2, r3
            sub r1, r2, -4
            mul r4, r5, 27
            add.l r1, r2, 5
            mov r6, r7
            neg r1, r2
            cmp r3, 5
            ldw r1, [r13+6]
            ldt r1, [r2-1]
            stw r1, [r2+1000]
            in r2, [r3+2]
            out r2, 486
            li r1, 42
            li r1, 100000
            li.l r1, 3
            ret
            call 30
            blt 30
            br3 30, 33
            ext r1, r2, 19, 9
            mfc r1, fcause
            mtc itb, r2
            sys 42
            hlt
        ";
        let p = text::assemble(src).unwrap();
        let s = &p.segments[0];
        let listing = disasm(&s.words, s.origin);
        for m in ["add r1, r2, r3", "sub r1, r2, -4", "mul r4, r5, 27", "add.l r1, r2, 5", "mov r6, r7",
                  "cmp r3, 5", "ldw r1, [r13+6]", "stw r1, [r2+1000]", "in r2, [r3+2]", "out r2, 486",
                  "li.l r1, 3", "ret", "call 30", "blt 30", "br3 30, 33", "ext r1, r2, 19, 9",
                  "mfc r1, fcause", "mtc itb, r2", "sys 42", "hlt"] {
            assert!(listing.contains(m), "missing `{m}` in\n{listing}");
        }
        // Round trip.
        let again = text::assemble(&disasm_source(&s.words, s.origin)).unwrap();
        assert_eq!(again.segments[0].words, s.words);
    }

    #[test]
    fn data_and_odd_encodings_fall_back_to_word() {
        let odd = setun_core::isa::s6(op::LIQ, 5, 1, 3); // LIQ with a stray B field
        let words = [Word::new(123_456).unwrap(), odd, Word::ZERO];
        let text = disasm_source(&words, 0);
        assert!(text.contains(&format!(".word {}", odd.value())), "{text}");
        let again = text::assemble(&text).unwrap();
        assert_eq!(again.segments[0].words, words.to_vec());
    }
}
