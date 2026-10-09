//! The ISA conformance vectors (docs/SETUN26_ISA.md §16), made by
//! tools/gen_isavectors.py from an independent model of the specification.
//! Each vector runs one instruction twice: through `Cpu::step`, and through
//! `Cpu::run_batch` (the decoded fast path with its address windows).

use std::collections::{BTreeMap, BTreeSet};

use setun_core::cpu::Cpu;
use setun_core::isa::cr;
use setun_core::{Io, Memory};
use ternary::{Tryte, Word};

const ITB: i64 = 14580;
const HANDLER: i64 = 30000;
const FRAMES: i64 = 27;
const MODE_FIELDS: [(&str, usize, i64); 9] = [
    ("RING", 27, 1),
    ("IE", 26, 0),
    ("PG", 25, 0),
    ("FE", 24, 1),
    ("IOPL", 23, 0),
    ("TF", 22, 0),
    ("S", 21, 0),
    ("C", 20, 0),
    ("VE", 19, 1),
];

fn word(v: i64) -> Word {
    Word::new(v).unwrap_or_else(|| panic!("{v} is not a word"))
}

#[derive(Default)]
struct VecIo {
    out: Vec<(i64, i64)>,
}

impl Io for VecIo {
    fn port_in(&mut self, port: i64) -> Word {
        Word::wrap(3 * port as i128 + 1)
    }
    fn port_out(&mut self, port: i64, value: Word) {
        self.out.push((port, value.value()));
    }
    fn pending_vector(&mut self) -> Option<i64> {
        None
    }
}

#[derive(Default, Clone)]
struct Side {
    r: BTreeMap<i64, i64>,
    v: BTreeMap<i64, [i64; 3]>,
    mode: BTreeMap<String, i64>,
    cr: BTreeMap<i64, i64>,
    mem: BTreeMap<i64, i64>,
    other: BTreeMap<String, String>,
    out: Vec<(i64, i64)>,
}

fn parse_side(s: &str) -> Side {
    let mut d = Side::default();
    for tok in s.split_whitespace() {
        let (k, v) = tok.split_once('=').unwrap_or_else(|| panic!("bad token {tok}"));
        let num = || v.parse::<i64>().unwrap_or_else(|_| panic!("bad number in {tok}"));
        if let Some(n) = k.strip_prefix("cr") {
            d.cr.insert(n.parse().unwrap(), num());
        } else if k.starts_with('r') && k[1..].parse::<i64>().is_ok() {
            d.r.insert(k[1..].parse().unwrap(), num());
        } else if k.starts_with('v') && k[1..].parse::<i64>().is_ok() {
            let w: Vec<i64> = v.split(',').map(|x| x.parse().unwrap()).collect();
            d.v.insert(k[1..].parse().unwrap(), [w[0], w[1], w[2]]);
        } else if k.starts_with('t') && k[1..].parse::<i64>().is_ok() {
            d.mem.insert(k[1..].parse().unwrap(), num());
        } else if MODE_FIELDS.iter().any(|f| f.0 == k) {
            d.mode.insert(k.to_string(), num());
        } else if k == "out" {
            let (p, x) = v.split_once(':').unwrap();
            d.out.push((p.parse().unwrap(), x.parse().unwrap()));
        } else {
            d.other.insert(k.to_string(), v.to_string());
        }
    }
    d
}

struct Vector {
    name: String,
    pre: Side,
    code: Vec<(i64, i64)>,
    post: Side,
}

fn vectors() -> Vec<Vector> {
    let text = include_str!("isa_vectors.txt");
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let parts: Vec<&str> = l.split(" | ").collect();
            assert_eq!(parts.len(), 4, "bad line {l}");
            let code = parts[2]
                .split_whitespace()
                .map(|t| {
                    let (a, w) = t.split_once(':').unwrap();
                    (a.parse().unwrap(), w.parse().unwrap())
                })
                .collect();
            Vector { name: parts[0].to_string(), pre: parse_side(parts[1]), code, post: parse_side(parts[3]) }
        })
        .collect()
}

fn mode_word(fields: &BTreeMap<String, i64>) -> Word {
    let mut m = Word::ZERO;
    for (name, pos, default) in MODE_FIELDS {
        m = m.with_field(pos..=pos, *fields.get(name).unwrap_or(&default));
    }
    m
}

fn code_trytes(code: &[(i64, i64)]) -> BTreeMap<i64, i64> {
    let mut m = BTreeMap::new();
    for &(a, w) in code {
        let w = word(w);
        m.insert(a - 1, w.field(1..=9));
        m.insert(a, w.field(10..=18));
        m.insert(a + 1, w.field(19..=27));
    }
    m
}

fn setup(v: &Vector) -> (Cpu, Memory) {
    let mut mem = Memory::new(FRAMES);
    let mut cpu = Cpu::new();
    for k in -121..=121 {
        mem.write_word(ITB + 3 * k, word(HANDLER + 3 * k)).unwrap();
    }
    for (&a, &t) in &v.pre.mem {
        mem.write_tryte(a, Tryte::new(t).unwrap()).unwrap();
    }
    for &(a, w) in &v.code {
        mem.write_word(a, word(w)).unwrap();
    }
    for n in 1..=8 {
        cpu.set_cr(n, word(if n == cr::ITB { ITB } else { 0 }));
    }
    for (&n, &x) in &v.pre.cr {
        cpu.set_cr(n, word(x));
    }
    cpu.set_cr(cr::MODE, mode_word(&v.pre.mode));
    for (&r, &x) in &v.pre.r {
        cpu.set_reg(r, word(x));
    }
    for (&r, &x) in &v.pre.v {
        cpu.set_vreg(r, x.map(word));
    }
    cpu.pc = Word::ZERO;
    cpu.halted = false;
    (cpu, mem)
}

fn want(bad: &mut Vec<String>, what: String, got: i64, exp: i64) {
    if got != exp {
        bad.push(format!("{what}: got {got}, want {exp}"));
    }
}

/// The differences between the machine and the vector's after state.
fn check(v: &Vector, cpu: &Cpu, mem: &Memory, io: &VecIo) -> Vec<String> {
    let mut bad = Vec::new();
    for r in -13..=13 {
        let exp = if r == 0 { 0 } else { *v.post.r.get(&r).or(v.pre.r.get(&r)).unwrap_or(&0) };
        want(&mut bad, format!("r{r}"), cpu.reg(r).value(), exp);
    }
    for r in -13..=13 {
        let exp = if r == 0 { [0; 3] } else { *v.post.v.get(&r).or(v.pre.v.get(&r)).unwrap_or(&[0; 3]) };
        let got = cpu.vreg(r).map(|w| w.value());
        for k in 0..3 {
            want(&mut bad, format!("v{r}[{k}]"), got[k], exp[k]);
        }
    }
    for (name, pos, default) in MODE_FIELDS {
        let exp = *v.post.mode.get(name).or(v.pre.mode.get(name)).unwrap_or(&default);
        want(&mut bad, name.to_string(), cpu.mode(pos), exp);
    }
    let trap: Option<i64> = v.post.other.get("trap").map(|t| t.parse().unwrap());
    for n in 1..=8 {
        if trap.is_some() && (n == cr::FADDR || n == cr::FCAUSE) {
            continue;
        }
        let default = if n == cr::ITB { ITB } else { 0 };
        let exp = *v.post.cr.get(&n).or(v.pre.cr.get(&n)).unwrap_or(&default);
        want(&mut bad, format!("cr{n}"), cpu.cr(n).value(), exp);
    }
    if let Some(t) = trap {
        let fc = cpu.cr(cr::FCAUSE);
        want(&mut bad, "FCAUSE vector".into(), fc.field(23..=27), t);
        let field = |k: &str| v.post.other.get(k).map(|x| x.parse::<i64>().unwrap());
        if let Some(r) = field("fring") {
            want(&mut bad, "FCAUSE ring".into(), fc.field(21..=21), r);
        }
        if let Some(a) = field("facc") {
            want(&mut bad, "FCAUSE access".into(), fc.field(22..=22), a);
        }
        if let Some(i) = field("finfo") {
            want(&mut bad, "FCAUSE info".into(), fc.field(1..=18), i);
        }
        if let Some(a) = field("faddr") {
            want(&mut bad, "FADDR".into(), cpu.cr(cr::FADDR).value(), a);
        }
    }
    want(&mut bad, "pc".into(), cpu.pc.value(), v.post.other["pc"].parse().unwrap());
    want(&mut bad, "halted".into(), cpu.halted as i64, v.post.other.contains_key("halted") as i64);
    if io.out != v.post.out {
        bad.push(format!("out: got {:?}, want {:?}", io.out, v.post.out));
    }
    // memory: every tryte the vector names, and 27 on each side of them
    let code = code_trytes(&v.code);
    let named: BTreeSet<i64> = v.pre.mem.keys().chain(v.post.mem.keys()).chain(code.keys()).copied().collect();
    let table = (ITB - 365)..=(ITB + 365);
    let mut seen = BTreeSet::new();
    for &a in &named {
        for b in a - 27..=a + 27 {
            if table.contains(&b) || !seen.insert(b) {
                continue;
            }
            let exp = *v.post.mem.get(&b).or(v.pre.mem.get(&b)).or(code.get(&b)).unwrap_or(&0);
            let got = mem.read_tryte(b).map(|t| t.value()).unwrap_or(i64::MIN);
            want(&mut bad, format!("t{b}"), got, exp);
        }
    }
    bad
}

#[test]
fn every_vector_by_step() {
    let vs = vectors();
    assert!(vs.len() > 3000, "only {} vectors", vs.len());
    let mut failures = Vec::new();
    for v in &vs {
        let (mut cpu, mut mem) = setup(v);
        let mut io = VecIo::default();
        cpu.step(&mut mem, &mut io);
        let bad = check(v, &cpu, &mem, &io);
        if !bad.is_empty() {
            failures.push(format!("{}: {}", v.name, bad.join("; ")));
        }
    }
    assert!(failures.is_empty(), "{} of {} vectors fail:\n{}", failures.len(), vs.len(), failures[..failures.len().min(30)].join("\n"));
}

#[test]
fn every_vector_by_batch() {
    // run_batch stops before port I/O and does nothing with TF set; those
    // vectors are covered by step alone
    let vs = vectors();
    let mut failures = Vec::new();
    let mut ran = 0;
    for v in &vs {
        let (mut cpu, mut mem) = setup(v);
        let mut io = VecIo::default();
        if cpu.run_batch(&mut mem, &mut io, 1) == 0 {
            continue;
        }
        ran += 1;
        let bad = check(v, &cpu, &mem, &io);
        if !bad.is_empty() {
            failures.push(format!("{}: {}", v.name, bad.join("; ")));
        }
    }
    assert!(ran > vs.len() * 9 / 10, "only {ran} ran");
    assert!(failures.is_empty(), "{} of {} vectors fail:\n{}", failures.len(), ran, failures[..failures.len().min(30)].join("\n"));
}

#[test]
fn the_vectors_cover_every_instruction() {
    // each family in the file, so a generator change cannot drop one quietly
    let vs = vectors();
    let fams: BTreeSet<String> = vs.iter().map(|v| v.name.split('.').next().unwrap().to_string()).collect();
    for f in ["alu", "aluq", "alui", "fld", "mem", "memd", "memr", "liq", "li", "lea", "jal", "jalr", "brq", "br", "br3", "sys", "io", "ioi", "reserved", "fpu", "valu", "vmem", "vmov", "vector", "mmu"] {
        assert!(fams.contains(f), "no {f} vectors");
    }
    let traps: BTreeSet<i64> = vs.iter().filter_map(|v| v.post.other.get("trap").map(|t| t.parse().unwrap())).collect();
    for t in [0, -1, -2, -3, -4, -5, -6, -7, -8, -10, -12, -13] {
        assert!(traps.contains(&t), "no vector traps to {t}");
    }
}
