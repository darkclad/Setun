//! The real27/real81 conformance vectors (docs/REAL.md §8) on the FPU
//! itself: each operation as its instruction (FADD ... FCLASS), in a ROM
//! program that loops over a table of operands and stores every result
//! with the FSR flags (FCMP: the S and C it sets) for the test to read.

use setun_real::{Real, DZ, IV, IX, OF, UF};
use trine::{run_rom, Outcome};

const VECTORS: &str = include_str!("../../real/tests/vectors.txt");

/// The instruction for a vector op, and its shape: what the operands are
/// (r real, i integer) and what the result is (r real, i integer word,
/// w the real27 word, c a comparison).
fn shape(op: &str) -> Option<(&'static str, &'static str, char)> {
    Some(match op {
        "add" => ("fadd f3, f1, f2", "rr", 'r'),
        "sub" => ("fsub f3, f1, f2", "rr", 'r'),
        "mul" => ("fmul f3, f1, f2", "rr", 'r'),
        "div" => ("fdiv f3, f1, f2", "rr", 'r'),
        "rem" => ("frem f3, f1, f2", "rr", 'r'),
        "min" => ("fmin f3, f1, f2", "rr", 'r'),
        "max" => ("fmax f3, f1, f2", "rr", 'r'),
        "sqrt" => ("fsqrt f3, f1", "r", 'r'),
        "neg" => ("fneg f3, f1", "r", 'r'),
        "abs" => ("fabs f3, f1", "r", 'r'),
        "rnd" => ("frnd f3, f1", "r", 'r'),
        "trunc" => ("ftrunc f3, f1", "r", 'r'),
        "cmp" => ("fcmp f1, f2", "rr", 'c'),
        "scale" => ("fscl f3, f1, r6", "ri", 'r'),
        "cvtif" => ("fcvtif f3, r6", "i", 'r'),
        "cvtfi" => ("fcvtfi r6, f1", "r", 'i'),
        "exp" => ("fexp r6, f1", "r", 'i'),
        "class" => ("fclass r6, f1", "r", 'i'),
        "to27" => ("ftow r6, f1", "r", 'w'),
        _ => return None,
    })
}

fn real(s: &str) -> Real {
    let (e, m) = s.split_once(':').unwrap();
    Real { e: e.parse().unwrap(), m: m.parse().unwrap() }
}

fn flags(s: &str) -> u8 {
    if s == "-" {
        return 0;
    }
    s.split('+').map(|f| match f { "IV" => IV, "DZ" => DZ, "OF" => OF, "UF" => UF, "IX" => IX, _ => panic!("{f}") }).sum()
}

/// FSR's flag trits (23..27: IV DZ OF UF IX) as the library's flag bits.
fn fsr_flags(fsr: i64) -> u8 {
    let f = fsr.rem_euclid(243);
    let mut out = 0;
    for (w, bit) in [(81, IV), (27, DZ), (9, OF), (3, UF), (1, IX)] {
        if f / w % 3 == 1 {
            out |= bit;
        }
    }
    out
}

/// FSR with every exception masked and no flag: trits 14..18 = +1.
const MASKED: i64 = 19_683 + 59_049 + 177_147 + 531_441 + 1_594_323;
const OUT: i64 = 49_209; // results: 4 words per vector, in RAM

struct V<'a> {
    line: &'a str,
    args: Vec<&'a str>,
    res: &'a str,
    fl: u8,
}

fn run_batch(op: &str, vs: &[V]) -> Vec<String> {
    let (ins, ops, res) = shape(op).unwrap();
    let mut table = String::new();
    for v in vs {
        for (k, kind) in ops.chars().enumerate() {
            if kind == 'r' {
                let w = real(v.args[k]).to_words();
                table += &format!("        .word {}, {}, {}\n", w[0], w[1], w[2]);
            } else {
                table += &format!("        .word {}, 0, 0\n", v.args[k]);
            }
        }
        if ops.len() == 1 {
            table += "        .word 0, 0, 0\n";
        }
    }
    let load_a = if ops.starts_with('i') { "        ldw r6, [r10]\n" } else { "        fldd f1, [r10]\n" };
    let load_b = match ops {
        "rr" => "        fldd f2, [r10+9]\n",
        "ri" => "        ldw r6, [r10+9]\n",
        _ => "",
    };
    let store = match res {
        'r' => "        fstd f3, [r9]\n",
        'c' => "        li r6, 0\n        beq s_set\n        li r6, 1\n        bgt s_set\n        li r6, -1\ns_set:  adc r5, r0, r0\n        stw r6, [r9]\n        stw r5, [r9+3]\n",
        _ => "        stw r6, [r9]\n",
    };
    let src = format!(
        "        .org -59049
        li    r8, {MASKED}
        la    r10, table
        li    r9, {OUT}
        li    r11, {n}
next:
{load_a}{load_b}        fmts  r8
        fmov  f3, f0
        {ins}
{store}        fmfs  r7
        stw   r7, [r9+9]
        add   r10, r10, 18
        add   r9, r9, 12
        sub   r11, r11, 1
        bgt   next
        li    r1, -999999001          ; crt0's exit: the tag, code 0, power off
        out   r1, 3
        out   r0, 3
        li    r1, -1
        out   r1, 2
        hlt
table:
{table}",
        n = vs.len()
    );
    let prog = setun_asm::text::assemble(&src).unwrap_or_else(|e| panic!("{op}: {e}"));
    let image = setun_asm::rom_image(&prog.segments).unwrap();
    let r = run_rom(&image, 50_000_000, "");
    assert_eq!(r.outcome, Outcome::Exit(0), "{op}: the program did not finish");
    let m = &r.machine.mem;
    let word = |a: i64| m.read_word(a).unwrap().value();
    let mut bad = Vec::new();
    for (k, v) in vs.iter().enumerate() {
        let at = OUT + 12 * k as i64;
        let fl = fsr_flags(word(at + 9));
        let (got, want) = match res {
            'r' => {
                let x = Real::from_words([word(at), word(at + 3), word(at + 6)]);
                (format!("{}:{}", x.e, x.m), v.res.to_string())
            }
            'w' => (word(at).to_string(), real(v.res).to_words()[0].to_string()),
            'c' => {
                let (s, c) = (word(at), word(at + 3));
                let got = match (s, c) {
                    (_, 1) => "un",
                    (-1, _) => "lt",
                    (0, _) => "eq",
                    _ => "gt",
                };
                (got.to_string(), v.res.to_string())
            }
            _ => (word(at).to_string(), v.res.to_string()),
        };
        // FCMP sets no flags; its C is the unordered answer
        let fl_ok = res == 'c' || fl == v.fl;
        if got != want || !fl_ok {
            bad.push(format!("{}\n    FPU: {got} flags {fl}", v.line));
        }
    }
    bad
}

#[test]
fn the_fpu_meets_the_standard() {
    let mut by_op: Vec<(String, Vec<V>)> = Vec::new();
    for line in VECTORS.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let (lhs, rhs) = line.split_once(" = ").unwrap();
        let (op, args) = lhs.split_once(' ').unwrap();
        if shape(op).is_none() {
            continue; // decimal text is the assembler's and the compilers' (not an instruction)
        }
        let (res, fl) = rhs.rsplit_once(' ').unwrap();
        let v = V { line, args: args.split(' ').collect(), res, fl: flags(fl) };
        match by_op.iter_mut().find(|(o, _)| o == op) {
            Some((_, l)) => l.push(v),
            None => by_op.push((op.to_string(), vec![v])),
        }
    }
    let mut bad = Vec::new();
    let mut n = 0;
    for (op, vs) in &by_op {
        for chunk in vs.chunks(300) {
            bad.extend(run_batch(op, chunk));
            n += chunk.len();
        }
    }
    assert!(n > 10_000, "only {n} vectors");
    assert!(bad.is_empty(), "{} of {n} vectors differ on the FPU:\n{}", bad.len(), bad[..bad.len().min(20)].join("\n"));
}
