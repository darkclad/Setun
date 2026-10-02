//! Opcode numbers and instruction encoders (ISA §3).
//!
//! Word 1 layout, trits MSB first:
//! `A[1:9]` major, `B[10:18]` function, `X[19:21]`, `Y[22:24]`, `Z[25:27]`.

use ternary::{Tri, Word};

/// Major opcodes (tryte A).
pub mod op {
    // 1-word
    pub const SYS: i64 = 0;
    pub const ALU: i64 = 1;
    pub const ALUQ: i64 = 2;
    pub const MEM: i64 = 3;
    pub const IO: i64 = 4;
    pub const JALR: i64 = 5;
    pub const LIQ: i64 = 6;
    pub const BRQ: i64 = 7;
    // 2-word (A[1] = +)
    pub const ALUI: i64 = 6561;
    pub const MEMD: i64 = 6562;
    pub const LI: i64 = 6563;
    pub const LEA: i64 = 6564;
    pub const JAL: i64 = 6565;
    pub const BR: i64 = 6566;
    pub const BR3: i64 = 6567;
    pub const IOI: i64 = 6568;
    pub const FLD: i64 = 6569;
    // the FPU (A[1] = −; A[2] = 0: 1-word, + : 2-word; ISA §14.5)
    pub const FALU: i64 = -6561;
    pub const FCVT: i64 = -6560;
    pub const FMEM: i64 = -6559;
    pub const FMEMD: i64 = -4374;
    pub const FLI: i64 = -4373;
}

/// FALU functions (tryte B, ISA §14.5).
pub mod falu {
    pub const FADD: i64 = 1;
    pub const FSUB: i64 = 2;
    pub const FMUL: i64 = 3;
    pub const FDIV: i64 = 4;
    pub const FREM: i64 = 5;
    pub const FMIN: i64 = 6;
    pub const FMAX: i64 = 7;
    pub const FSQRT: i64 = 8;
    pub const FNEG: i64 = 9;
    pub const FABS: i64 = 10;
    pub const FRND: i64 = 11;
    pub const FTRUNC: i64 = 12;
    pub const FCMP: i64 = 13;
}

/// FCVT functions (tryte B, ISA §14.5).
pub mod fcvt {
    pub const FCVTIF: i64 = 1;
    pub const FCVTFI: i64 = 2;
    pub const FTOW: i64 = 3;
    pub const FFRW: i64 = 4;
    pub const FEXP: i64 = 5;
    pub const FSCL: i64 = 6;
    pub const FMFS: i64 = 7;
    pub const FMTS: i64 = 8;
    pub const FCLASS: i64 = 9;
    pub const FCONST: i64 = 10;
    pub const FMOV: i64 = 11;
}

/// FMEM / FMEMD functions (tryte B): ± the size in words.
pub mod fmem {
    pub const FLDS: i64 = 1;
    pub const FLDD: i64 = 2;
    pub const FSTS: i64 = -1;
    pub const FSTD: i64 = -2;
}

/// FSR fields (ISA §14.2): flag trits 23..27 and mask trits 14..18, in the
/// order IV DZ OF UF IX.
pub mod fsr {
    pub const FLAG_FIRST: usize = 23;
    pub const MASK_FIRST: usize = 14;
    /// All masks set, no flags: the reset value.
    /// (Trit k weighs 3^(27−k): the masks are 3^13 … 3^9.)
    pub const RESET: i64 = 19_683 * (81 + 27 + 9 + 3 + 1);
}

/// SYS group functions (tryte B).
pub mod sys {
    pub const ILL: i64 = 0;
    pub const NOP: i64 = 1;
    pub const HLT: i64 = 2;
    pub const BRK: i64 = 3;
    pub const SYS: i64 = 4;
    pub const IRET: i64 = 5;
    pub const MFC: i64 = 6;
    pub const MTC: i64 = 7;
    pub const TLBINV: i64 = 8;
    pub const TLBFLUSH: i64 = 9;
    pub const FENCE: i64 = 10;
}

/// ALU functions (tryte B).
pub mod alu {
    pub const ADD: i64 = 1;
    pub const SUB: i64 = 2;
    pub const ADC: i64 = 3;
    pub const SBC: i64 = 4;
    pub const MUL: i64 = 5;
    pub const MULH: i64 = 6;
    pub const DIV: i64 = 7;
    pub const MOD: i64 = 8;
    pub const DIVR: i64 = 9;
    pub const MODR: i64 = 10;
    pub const MIN: i64 = 11;
    pub const MAX: i64 = 12;
    pub const TMUL: i64 = 13;
    pub const TSAT: i64 = 14;
    pub const CONS: i64 = 15;
    pub const SH: i64 = 16;
    pub const CMP: i64 = 17;
    pub const TRIT: i64 = 18;
    pub const NEG: i64 = 19;
    pub const SGN: i64 = 20;
    pub const ABS: i64 = 21;
    pub const TLZ: i64 = 22;
}

/// Memory functions (tryte B).
pub mod mem {
    pub const LDT: i64 = 1;
    pub const LDW: i64 = 2;
    pub const STT: i64 = 3;
    pub const STW: i64 = 4;
    pub const SWAP: i64 = 5;
}

/// Port I/O functions (tryte B).
pub mod io {
    pub const IN: i64 = 1;
    pub const OUT: i64 = 2;
}

/// Trit-field functions (tryte B).
pub mod fld {
    pub const EXT: i64 = 1;
    pub const INS: i64 = 2;
}

/// Branch conditions (3-trit `cond`, ISA §3.5), as balanced values.
pub mod cond {
    const fn c(a: i64, b: i64, c: i64) -> i64 {
        a * 9 + b * 3 + c
    }
    pub const LT: i64 = c(1, 0, 0);
    pub const EQ: i64 = c(0, 1, 0);
    pub const GT: i64 = c(0, 0, 1);
    pub const LE: i64 = c(1, 1, 0);
    pub const GE: i64 = c(0, 1, 1);
    pub const NE: i64 = c(1, 0, 1);
    pub const ALWAYS: i64 = c(1, 1, 1);
    pub const OV: i64 = c(-1, 0, -1);
    pub const NOV: i64 = c(0, -1, 0);
    pub const NEVER: i64 = 0;
}

/// Control register numbers (ISA §2.3).
pub mod cr {
    pub const MODE: i64 = 0;
    pub const PTB: i64 = 1;
    pub const FADDR: i64 = 2;
    pub const FCAUSE: i64 = 3;
    pub const ITB: i64 = 4;
    pub const EPC: i64 = 5;
    pub const EMODE: i64 = 6;
    pub const KSP: i64 = 7;
    pub const SCRATCH: i64 = 8;
    pub const CPUID: i64 = 9;
    pub const CYCLE: i64 = 10;
}

/// MODE trit positions (ISA §2.2).
pub mod mode {
    pub const RING: usize = 27;
    pub const IE: usize = 26;
    pub const PG: usize = 25;
    pub const IOPL: usize = 23;
    pub const TF: usize = 22;
    pub const S: usize = 21;
    pub const C: usize = 20;
    pub const FE: usize = 24;
}

/// Vector numbers (ISA §7.1).
pub mod vector {
    pub const SYSCALL: i64 = 0;
    pub const ILLEGAL: i64 = -1;
    pub const PRIVILEGE: i64 = -2;
    pub const PAGE_FAULT: i64 = -3;
    pub const PROTECTION: i64 = -4;
    pub const MISALIGNED: i64 = -5;
    pub const DIVIDE: i64 = -6;
    pub const BREAKPOINT: i64 = -7;
    pub const STEP: i64 = -8;
    pub const DOUBLE_FAULT: i64 = -9;
    pub const BUS_ERROR: i64 = -10;
    pub const FP_EXCEPTION: i64 = -11;
    pub const FPU_UNAVAILABLE: i64 = -12;
}

fn head(a: i64, b: i64) -> Word {
    Word::ZERO.with_field(1..=9, a).with_field(10..=18, b)
}

fn reg(r: i64) -> i64 {
    assert!(Tri::new(r).is_some(), "register/field {r} out of −13..=13");
    r
}

/// 1-word R/Q format: `A B x y z` (registers or a small immediate in `z`).
pub fn rrr(a: i64, b: i64, x: i64, y: i64, z: i64) -> Word {
    head(a, b)
        .with_field(19..=21, reg(x))
        .with_field(22..=24, reg(y))
        .with_field(25..=27, reg(z))
}

/// 1-word S format: `A B x imm6`.
pub fn s6(a: i64, b: i64, x: i64, imm6: i64) -> Word {
    assert!((-364..=364).contains(&imm6), "imm6 {imm6} out of range");
    head(a, b).with_field(19..=21, reg(x)).with_field(22..=27, imm6)
}

/// 1-word with a 9-trit operand field (`SYS n`).
pub fn s9(a: i64, b: i64, imm9: i64) -> Word {
    assert!((-9841..=9841).contains(&imm9), "imm9 {imm9} out of range");
    head(a, b).with_field(19..=27, imm9)
}

/// 2-word I/J format: `A B x y | imm27`.
pub fn wide(a: i64, b: i64, x: i64, y: i64, imm: i64) -> [Word; 2] {
    [
        head(a, b).with_field(19..=21, reg(x)).with_field(22..=24, reg(y)),
        Word::new(imm).expect("imm27 out of range"),
    ]
}

/// 2-word `BR3`: word 2 = `[offN : 9][offP : 9][0 : 9]`.
pub fn br3(off_n: i64, off_p: i64) -> [Word; 2] {
    [
        head(op::BR3, 0),
        Word::ZERO.with_field(1..=9, off_n).with_field(10..=18, off_p),
    ]
}

/// 2-word `FLD`: word 2 = `[pos : 9][len : 9][0 : 9]`.
pub fn fld(b: i64, rd: i64, ra: i64, pos: i64, len: i64) -> [Word; 2] {
    [
        head(op::FLD, b).with_field(19..=21, reg(rd)).with_field(22..=24, reg(ra)),
        Word::ZERO.with_field(1..=9, pos).with_field(10..=18, len),
    ]
}
