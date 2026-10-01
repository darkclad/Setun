//! The Setun-387 in the emulator (ISA §14), driven by assembly programs.

use setun_asm::{disasm, text};
use setun_core::{Machine, Step};
use setun_real::Real;
use ternary::Word;

/// A trap table at 7290 whose handlers for vectors −1, −11 and −12 store
/// the vector in r10, FCAUSE in r11, EPC in r12, and halt.
const TRAPS: &str = r#"
        .org 7257               ; ITB + 3·(−11)
        .word trap_fp
        .word trap_off          ; −12 would be at 7254: see below
        .org 7254
        .word trap_off
        .org 7287               ; −1
        .word trap_ill
        .org 7500
trap_fp:  li r10, -11
          b  trap_out
trap_off: li r10, -12
          b  trap_out
trap_ill: li r10, -1
trap_out: mfc r11, fcause
          mfc r12, epc
          hlt
"#;

fn run(src: &str) -> Machine {
    let src = format!("{TRAPS}\n.org 0\n    li r9, 7290\n    mtc itb, r9\n{src}\n");
    let p = text::assemble(&src).unwrap_or_else(|e| panic!("{e}"));
    let mut m = Machine::new(3, 0);
    for s in &p.segments {
        m.mem.load_words(s.origin, &s.words);
    }
    m.cpu.pc = Word::new(0).unwrap();
    for _ in 0..100_000 {
        if m.step() == Step::Halted {
            break;
        }
    }
    m
}

fn i(v: i64) -> Real {
    Real::from_i64(v)
}

fn dec(s: &str) -> Real {
    Real::from_decimal(s).unwrap().0
}

/// FSR flag trit for exception k (0 IV, 1 DZ, 2 OF, 3 UF, 4 IX).
fn flag(m: &Machine, k: usize) -> i64 {
    m.cpu.fsr().trit(23 + k) as i64
}

#[test]
fn arithmetic_and_conversions() {
    let m = run(r#"
        li     r1, 2
        fcvtif f1, r1            ; 2
        fsqrt  f2, f1            ; √2
        fmul   f3, f2, f2        ; ≈ 2
        fconst f4, pi
        li     r2, 3
        fcvtif f5, r2
        fdiv   f6, f4, f5        ; π/3
        fli    f7, 0.1
        fli    f8, -2.5
        fadd   f9, f8, f5        ; 0.5 (as close as real27 gets)
        ftow   r3, f7
        fexp   r4, f4            ; π = m·3^2
        fclass r5, f6            ; +2 normal
        fcvtfi r6, f8            ; -2.5 → -2 (ties to even)
        fneg   f10, f4
        fabs   f11, f10
        frnd   f12, f8           ; -2
        ftrunc f13, f9           ; 0
        fmin   f-1, f4, f8
        fmax   f-2, f4, f8
        frem   f-3, f4, f5       ; π − 3
        li     r7, 2
        fscl   f-4, f5, r7       ; 3 · 9 = 27
        fmov   f-5, f6
        fconst f-6, e
        hlt
    "#);
    let f = |k| m.cpu.freg(k);
    assert_eq!(f(1), i(2));
    assert!(f(3).sub(i(2)).0.abs().cmp(dec("1e-32")) == Some(std::cmp::Ordering::Less), "√2² ≈ 2");
    let pi = Real::constant(2).unwrap();
    assert_eq!(f(6), pi.div(i(3)).0);
    assert_eq!(f(7), Real::from_word27(dec("0.1").to_word27().0), "FLI takes a real27");
    assert_eq!(m.cpu.reg(3).value(), dec("0.1").to_word27().0);
    assert_eq!(m.cpu.reg(4).value(), 2);
    assert_eq!(m.cpu.reg(5).value(), 2);
    assert_eq!(m.cpu.reg(6).value(), -2);
    assert_eq!(f(10), pi.neg());
    assert_eq!(f(11), pi);
    assert_eq!(f(12), i(-2));
    assert_eq!(f(13), Real::ZERO);
    assert_eq!((f(-1), f(-2)), (f(8), pi), "the real27 -2.5 that FLI loaded");
    assert_eq!(f(-3), pi.sub(i(3)).0);
    assert_eq!(f(-4), i(27));
    assert_eq!(f(-5), f(6));
    assert_eq!(f(-6), Real::constant(3).unwrap());
    assert_eq!(flag(&m, 4), 1, "IX: √2 was rounded");
    assert_eq!((flag(&m, 0), flag(&m, 1), flag(&m, 2)), (0, 0, 0), "nothing invalid");
    assert_eq!(m.cpu.freg(0), Real::ZERO, "f0 stays zero");
}

#[test]
fn f0_is_zero() {
    let m = run("    fconst f0, pi\n    fmov f1, f0\n    hlt");
    assert_eq!(m.cpu.freg(0), Real::ZERO);
    assert_eq!(m.cpu.freg(1), Real::ZERO);
}

#[test]
fn loads_and_stores() {
    let m = run(r#"
        fconst f1, pi
        li     r9, 9000
        fstd   f1, [r9]          ; 1-word form: offset 0
        fldd   f2, [r9]
        fstd   f1, [r9+18]       ; 1-word: 18 = 2 · 9
        fldd   f3, [r9+18]
        fstd   f1, [r9+300]      ; 2-word form
        fldd   f4, [r9+300]
        fsts   f1, [r9+30]       ; real27, rounded (1-word: 30 = 10 · 3)
        flds   f5, [r9+30]
        ldw    r1, [r9+30]
        hlt
    "#);
    let pi = Real::constant(2).unwrap();
    assert_eq!([m.cpu.freg(2), m.cpu.freg(3), m.cpu.freg(4)], [pi; 3]);
    assert_eq!(m.cpu.freg(5), pi.to_real27().0);
    assert_eq!(m.cpu.reg(1).value(), pi.to_word27().0);
    // the three words in memory, most significant first
    let w: Vec<i64> = (0..3).map(|k| m.mem.read_word(9000 + 3 * k).unwrap().value()).collect();
    assert_eq!(w, pi.to_words().to_vec());
    assert_eq!(flag(&m, 4), 1, "FSTS rounded");
}

#[test]
fn compare_and_branch() {
    let m = run(r#"
        li     r1, 1
        fcvtif f1, r1
        fconst f2, pi
        fcmp   f1, f2
        blt    less
        li     r2, 99
        hlt
less:   li     r2, 1
        fdiv   f3, f0, f0        ; 0/0: the NaN (IV is masked)
        fcmp   f3, f1
        bov    unordered         ; C = +1
        li     r3, 99
        hlt
unordered:
        li     r3, 1
        fcmp   f2, f2
        beq    equal
        hlt
equal:  li     r4, 1
        hlt
    "#);
    assert_eq!([m.cpu.reg(2).value(), m.cpu.reg(3).value(), m.cpu.reg(4).value()], [1, 1, 1]);
    assert!(m.cpu.freg(3).is_nan());
    assert_eq!(flag(&m, 0), 1, "IV raised, masked");
}

#[test]
fn masked_specials() {
    let m = run(r#"
        li     r1, 1
        fcvtif f1, r1
        fdiv   f2, f1, f0        ; 1/0 = ∞, DZ
        fsub   f3, f2, f2        ; ∞ − ∞ = NaN, IV
        fneg   f4, f2            ; −∞
        fcvtfi r2, f4            ; −∞ → the most negative word, IV
        fclass r3, f2            ; +3
        fclass r4, f3            ; 4
        hlt
    "#);
    assert_eq!(m.cpu.freg(2), Real::INF);
    assert!(m.cpu.freg(3).is_nan());
    assert_eq!(m.cpu.freg(4), Real::NEG_INF);
    assert_eq!(m.cpu.reg(2).value(), -setun_real::WORD_MAX);
    assert_eq!([m.cpu.reg(3).value(), m.cpu.reg(4).value()], [3, 4]);
    assert_eq!((flag(&m, 0), flag(&m, 1)), (1, 1));
}

#[test]
fn an_unmasked_exception_traps_first() {
    // masks all on except DZ (trit 15): 3^9 · (81 + 9 + 3 + 1)
    let fsr = 19_683 * (81 + 9 + 3 + 1);
    let m = run(&format!(
        r#"
        li     r1, {fsr}
        fmts   r1
        li     r2, 1
        fcvtif f1, r2
        fli    f2, 7.0
here:   fdiv   f2, f1, f0        ; traps: f2 must stay 7
        li     r13, 99           ; not reached
        hlt
    "#
    ));
    assert_eq!(m.cpu.reg(10).value(), -11, "vector −11");
    let fcause = m.cpu.reg(11);
    assert_eq!(fcause.field(23..=27), -11);
    assert_eq!(fcause.field(1..=18), 2, "the exception: DZ");
    assert_eq!(m.cpu.freg(2), i(7), "nothing written");
    assert_eq!(flag(&m, 1), 1, "the flag is set even so");
    assert_ne!(m.cpu.reg(13).value(), 99);
    // EPC is the faulting instruction
    let p = text::assemble(&format!("{TRAPS}\n.org 0\n    li r9, 7290\n    mtc itb, r9\n    li r1, {fsr}\n    fmts r1\n    li r2, 1\n    fcvtif f1, r2\n    fli f2, 7.0\nhere: nop")).unwrap();
    assert_eq!(m.cpu.reg(12).value(), p.symbols["here"]);
}

#[test]
fn the_unit_can_be_switched_off() {
    // clear MODE.FE (trit 24, weight 3^3): FPU instructions then trap to −12
    let m = run(r#"
        mfc    r1, mode
        sub    r1, r1, 27
        mtc    mode, r1
off:    fadd   f1, f2, f3
        hlt
    "#);
    assert_eq!(m.cpu.reg(10).value(), -12);
    assert_eq!(m.cpu.reg(11).field(23..=27), -12);
}

#[test]
fn reserved_encodings_are_illegal() {
    // A[2] = − (A = −8000), an unknown FALU function, and FCONST 7
    for word in [-8000 * 387_420_489i64, -6561 * 387_420_489 + 20 * 19_683, -6560 * 387_420_489 + 10 * 19_683 + 7 * 27] {
        let m = run(&format!("    .word {word}\n    hlt"));
        assert_eq!(m.cpu.reg(10).value(), -1, "{word}");
    }
    // a reserved major is ILL even with the unit off
    let m = run(&format!("    mfc r1, mode\n    sub r1, r1, 27\n    mtc mode, r1\n    .word {}\n    hlt", -8000 * 387_420_489i64));
    assert_eq!(m.cpu.reg(10).value(), -1);
}

#[test]
fn fsr_and_cpuid() {
    let m = run(r#"
        fmfs   r1                ; the reset value: all masks, no flags
        li     r2, -3812798742493  ; every trit −1: reserved ones are ignored
        fmts   r2
        fmfs   r3
        mfc    r4, cpuid
        hlt
    "#);
    assert_eq!(m.cpu.reg(1).value(), 19_683 * 121);
    // only the mask and flag fields take the −1s; the reserved trits stay 0
    assert_eq!(m.cpu.reg(3).value(), -(19_683 * 121 + 121));
    assert_eq!(m.cpu.reg(4).value(), setun_core::cpu::CPUID);
    assert_eq!(m.cpu.reg(4).trit(1) as i64, 1, "CPUID trit 1: the FPU is present");
}

#[test]
fn every_fpu_instruction_disassembles_back() {
    let src = r#"
        .org 0
        fadd f1, f2, f3
        fsub f-13, f13, f0
        fmul f4, f5, f6
        fdiv f7, f8, f9
        frem f1, f2, f3
        fmin f1, f2, f3
        fmax f1, f2, f3
        fsqrt f1, f2
        fneg f1, f2
        fabs f1, f2
        frnd f1, f2
        ftrunc f1, f2
        fcmp f1, f2
        fcvtif f1, r2
        fcvtfi r1, f2
        ftow r1, f2
        ffrw f1, r2
        fexp r1, f2
        fscl f1, f2, r3
        fmfs r1
        fmts r2
        fclass r1, f2
        fconst f1, pi
        fconst f1, log3_10
        fmov f1, f2
        flds f1, [r2+3]
        fldd f1, [r2+18]
        fsts f1, [r2-39]
        fstd f1, [r2+900]
        fli f1, 2.5
        fli f2, -0.125
    "#;
    let p = text::assemble(src).unwrap();
    let words = &p.segments[0].words;
    let mut k = 0;
    while k < words.len() {
        let (t, n) = disasm::disasm_one(words[k], words.get(k + 1).copied(), 3 * k as i64);
        assert!(!t.starts_with(".word"), "word {k} did not disassemble: {t}");
        k += n;
    }
}

#[test]
fn assembler_refusals() {
    let e = |src: &str| text::assemble(src).unwrap_err().msg;
    assert!(e("fadd f1, f2, r3").contains("FPU register expected"));
    assert!(e("fadd r1, f2, f3").contains("FPU register expected"));
    assert!(e("fadd f14, f1, f2").contains("FPU register expected") || e("fadd f14, f1, f2").contains("unknown"));
    assert!(e("fconst f1, tau").contains("unknown constant `tau`"));
    assert!(e("fconst f1, 9").contains("out of range"));
    assert!(e("fcvtif f1, f2").contains("register expected"));
    assert!(e(".real27 abc").contains("real literal expected"));
    assert!(e("fli f1").contains("bad operands"));
}

#[test]
fn real_directives() {
    let p = text::assemble(".org 0\n.real27 1, -2.5, 0.1\n.real81 3.0").unwrap();
    let w: Vec<i64> = p.segments[0].words.iter().map(|w| w.value()).collect();
    assert_eq!(w[0], i(1).to_word27().0);
    assert_eq!(w[1], dec("-2.5").to_word27().0);
    assert_eq!(w[2], dec("0.1").to_word27().0);
    assert_eq!(&w[3..6], &i(3).to_words());
}
