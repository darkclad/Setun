//! A peephole pass over one function's assembly, driven by register
//! liveness (the S/C flags count as a register: `mov`, `push`, `pop` and
//! ALU instructions set them, branches read them).
//!
//! - **Forward a copy:** `mov r1, X` then an instruction reading r1 reads X
//!   instead, when r1 is dead afterwards (or rewritten by it).
//! - **Retarget a result:** an instruction computing r1 followed by
//!   `mov Y, r1` computes Y directly, when r1 is dead after the move.
//! - **Drop dead code:** a side-effect-free instruction whose results are
//!   all dead.
//!
//! Anything it does not understand (inline assembly, an unknown mnemonic,
//! a jump it cannot follow) makes it leave the function alone.

use std::collections::HashMap;

const FLAGS: u32 = 1 << 27;
/// sp and fp: writes to them always stay.
const FRAME: u32 = (1 << (13 + 13)) | (1 << (11 + 13));

fn reg_num(s: &str) -> Option<i64> {
    match s {
        "sp" => Some(13),
        "fp" => Some(11),
        "lr" => Some(12),
        "zero" => Some(0),
        _ => s.strip_prefix('r')?.parse::<i64>().ok().filter(|n| (-13..=13).contains(n)),
    }
}

fn bit(r: i64) -> u32 {
    1 << (r + 13)
}

/// Registers named in an operand: a register, or a memory operand's base.
fn regs_of(op: &str) -> u32 {
    if let Some(r) = reg_num(op) {
        return bit(r);
    }
    if let Some(inner) = op.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        // "[r-3+6]": the base may itself contain a minus
        let base = match inner.strip_prefix("r-") {
            Some(rest) => &inner[..rest.find(['+', '-']).map_or(inner.len(), |k| k + 2)],
            None => inner.split(['+', '-']).next().unwrap_or(""),
        };
        if let Some(r) = reg_num(base) {
            return bit(r);
        }
    }
    0
}

/// r1..r6: the argument registers.
fn args() -> u32 {
    (1..=6).map(bit).fold(0, |a, b| a | b)
}

/// What a call may change: the caller-saved registers, lr and the flags.
fn clobbered() -> u32 {
    let mut m = FLAGS | bit(12);
    for r in (1..=10).chain(-13..=-10) {
        m |= bit(r);
    }
    m
}

#[derive(Clone)]
struct Ins {
    op: String,
    args: Vec<String>,
}

impl Ins {
    fn parse(line: &str) -> Ins {
        let t = line.trim();
        let (op, rest) = t.split_once(char::is_whitespace).unwrap_or((t, ""));
        let args = if rest.trim().is_empty() { Vec::new() } else { rest.split(',').map(|a| a.trim().to_string()).collect() };
        Ins { op: op.to_string(), args }
    }

    fn text(&self) -> String {
        if self.args.is_empty() {
            format!("        {}", self.op)
        } else {
            format!("        {} {}", self.op, self.args.join(", "))
        }
    }
}

#[derive(Clone, PartialEq)]
enum Ctl {
    Next,
    Jump(String),
    Branch(Vec<String>),
    Exit,
}

/// How an instruction behaves.
struct Info {
    uses: u32,
    defs: u32,
    /// Only writes `defs`: safe to delete when they are dead.
    pure: bool,
    /// The destination is operand 0, which can be renamed.
    dst0: bool,
    ctl: Ctl,
}

const ALU3: &[&str] = &[
    "add", "sub", "adc", "sbc", "mul", "mulh", "div", "mod", "divr", "modr", "min", "max", "tmul", "tsat", "cons", "sh", "trit",
];
const ALU2: &[&str] = &["neg", "sgn", "abs", "tlz"];
const BRANCHES: &[&str] = &["blt", "beq", "bgt", "ble", "bge", "bne", "bov", "bnov", "bnever"];

fn info(i: &Ins) -> Option<Info> {
    let a = &i.args;
    let reg0 = || a.first().and_then(|x| reg_num(x)).map(bit).unwrap_or(0);
    let src = |from: usize| a.iter().skip(from).fold(0, |m, x| m | regs_of(x));
    let op = i.op.as_str();
    let inf = |uses, defs, pure, dst0| Some(Info { uses, defs, pure, dst0, ctl: Ctl::Next });
    match op {
        _ if ALU3.contains(&op) && a.len() == 3 => {
            let carry_in = if matches!(op, "adc" | "sbc") { FLAGS } else { 0 };
            let faults = matches!(op, "div" | "mod" | "divr" | "modr");
            inf(src(1) | carry_in, reg0() | FLAGS, !faults, true)
        }
        _ if ALU2.contains(&op) && a.len() == 2 => inf(src(1), reg0() | FLAGS, true, true),
        "mov" if a.len() == 2 => {
            if reg_num(&a[1]).is_some() {
                inf(src(1), reg0() | FLAGS, true, true)
            } else {
                inf(0, reg0(), true, true) // a constant: LI, no flags
            }
        }
        "li" | "la" | "lea" if a.len() == 2 => inf(0, reg0(), true, true),
        "cmp" if a.len() == 2 => inf(src(0), FLAGS, true, false),
        "ldw" | "ldt" if a.len() == 2 => inf(src(1), reg0(), false, true),
        "stw" | "stt" if a.len() == 2 => inf(src(0), 0, false, false),
        "push" if a.len() == 1 => inf(src(0) | bit(13), bit(13) | FLAGS, false, false),
        "pop" if a.len() == 1 => inf(bit(13), reg0() | bit(13) | FLAGS, false, false),
        "ext" if a.len() == 4 => inf(src(1), reg0() | FLAGS, true, true),
        "ins" if a.len() == 4 => inf(src(0), reg0() | FLAGS, true, false),
        "in" if a.len() == 2 => inf(src(1), reg0(), false, false),
        "out" if a.len() == 2 => inf(src(0), 0, false, false),
        "mfc" if a.len() == 2 => inf(0, reg0(), false, false),
        "mtc" if a.len() == 2 => inf(src(1), 0, false, false),
        "nop" => inf(0, 0, false, false),
        "hlt" | "fence" | "tlbflush" => inf(0, 0, false, false),
        "tlbinv" => inf(src(0), 0, false, false),
        "sys" => inf(args() | bit(13), bit(1) | FLAGS, false, false),
        "call" if a.len() == 1 => inf(args() | bit(13), clobbered(), false, false),
        "jal" if a.len() == 2 => inf(args() | bit(13), clobbered() | reg0(), false, false),
        "jalr" if a.len() >= 2 && reg0() != bit(0) => inf(src(1) | args() | bit(13), clobbered() | reg0(), false, false),
        "ret" if a.is_empty() => {
            let mut uses = bit(1) | bit(11) | bit(12) | bit(13);
            for r in -9..=-1 {
                uses |= bit(r);
            }
            Some(Info { uses, defs: 0, pure: false, dst0: false, ctl: Ctl::Exit })
        }
        "b" if a.len() == 1 => Some(Info { uses: 0, defs: 0, pure: false, dst0: false, ctl: Ctl::Jump(a[0].clone()) }),
        _ if BRANCHES.contains(&op) && a.len() == 1 => {
            Some(Info { uses: FLAGS, defs: 0, pure: false, dst0: false, ctl: Ctl::Branch(vec![a[0].clone()]) })
        }
        "br3" if a.len() == 2 => Some(Info { uses: FLAGS, defs: 0, pure: false, dst0: false, ctl: Ctl::Branch(a.clone()) }),
        // the FPU: integer registers by position; every instruction may trap
        "fldd" | "flds" | "fstd" | "fsts" if a.len() == 2 => inf(src(1), 0, false, false),
        "fcvtif" | "ffrw" if a.len() == 2 => inf(src(1), 0, false, false),
        "fcvtfi" | "ftow" | "fexp" | "fclass" | "fmfs" if !a.is_empty() => inf(0, reg0(), false, false),
        "fmts" if a.len() == 1 => inf(src(0), 0, false, false),
        "fscl" if a.len() == 3 => inf(src(2), 0, false, false),
        "fcmp" => inf(0, FLAGS, false, false),
        _ if op.starts_with('f') => inf(0, 0, false, false), // f-register-only arithmetic
        _ => None,
    }
}

enum Line {
    Label(String),
    Ins(Ins),
    Other,
}

fn classify(l: &str) -> Line {
    if l.starts_with(char::is_whitespace) {
        if l.trim().is_empty() {
            Line::Other
        } else {
            Line::Ins(Ins::parse(l))
        }
    } else if let Some(name) = l.trim().strip_suffix(':') {
        Line::Label(name.to_string())
    } else {
        Line::Other
    }
}

/// Liveness after each line (indexes into `lines`); None if the function
/// cannot be analysed.
fn liveness(lines: &[String]) -> Option<Vec<u32>> {
    let parsed: Vec<Line> = lines.iter().map(|l| classify(l)).collect();
    let mut labels = HashMap::new();
    for (k, l) in parsed.iter().enumerate() {
        if let Line::Label(n) = l {
            labels.insert(n.clone(), k);
        }
    }
    let mut infos = Vec::with_capacity(lines.len());
    for l in &parsed {
        infos.push(match l {
            Line::Ins(i) => Some(info(i)?),
            Line::Label(_) => None,
            Line::Other => return None,
        });
    }
    // successors of each line
    let n = lines.len();
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
    for k in 0..n {
        let next = if k + 1 < n { vec![k + 1] } else { vec![] };
        succ[k] = match &infos[k] {
            None => next,
            Some(inf) => match &inf.ctl {
                Ctl::Next => next,
                Ctl::Exit => vec![],
                Ctl::Jump(t) => vec![*labels.get(t)?],
                Ctl::Branch(ts) => {
                    let mut v = next;
                    for t in ts {
                        v.push(*labels.get(t)?);
                    }
                    v
                }
            },
        };
    }
    // iterate to a fixpoint: live_in[k] = uses ∪ (live_out[k] − defs)
    let mut live_in = vec![0u32; n];
    let mut live_out = vec![0u32; n];
    loop {
        let mut changed = false;
        for k in (0..n).rev() {
            let out = succ[k].iter().fold(0, |m, &s| m | live_in[s]);
            let inn = match &infos[k] {
                Some(inf) => inf.uses | (out & !inf.defs),
                None => out,
            };
            if out != live_out[k] || inn != live_in[k] {
                live_out[k] = out;
                live_in[k] = inn;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    Some(live_out)
}

/// Replace register `from` by `to` in the source operands of `i`.
fn substitute(i: &mut Ins, first_src: usize, from: &str, to: &str) {
    for a in i.args.iter_mut().skip(first_src) {
        if a == from {
            *a = to.to_string();
        } else if let Some(inner) = a.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            if let Some(rest) = inner.strip_prefix(from) {
                if rest.is_empty() || rest.starts_with(['+', '-']) {
                    *a = format!("[{to}{rest}]");
                }
            }
        }
    }
}

/// A memory operand as (base register, the rest): "[r-11+29847]" gives
/// (Some(-11), "+29847"), "[29844]" gives (None, "29844").
fn mem_parts(op: &str) -> Option<(Option<i64>, String)> {
    let inner = op.strip_prefix('[')?.strip_suffix(']')?;
    let base_bits = regs_of(op);
    if base_bits == 0 {
        return Some((None, inner.to_string()));
    }
    let r = (-13..=13).find(|&r| bit(r) == base_bits)?;
    let name_len = match inner.strip_prefix("r-") {
        Some(rest) => rest.find(['+', '-']).map_or(inner.len(), |k| k + 2),
        None => inner.find(['+', '-']).unwrap_or(inner.len()),
    };
    Some((Some(r), inner[name_len..].to_string()))
}

/// Local value numbering: the first instruction that would put into its
/// destination the value already there (and whose flags are not needed),
/// which can go. Values are followed through straight-line code and on
/// into the fall-through of a conditional branch; a label (another way
/// in) forgets everything. A load's value is tied to the memory version,
/// which any store, push, call or unknown instruction moves on.
fn redundant(lines: &[String], live: &[u32]) -> Option<usize> {
    let mut val: HashMap<i64, u32> = HashMap::new(); // register -> value number
    let mut exprs: HashMap<String, u32> = HashMap::new();
    let mut next = 1u32;
    let mut epoch = 0u32;
    let fresh = |next: &mut u32| {
        *next += 1;
        *next
    };
    for (k, l) in lines.iter().enumerate() {
        let i = match classify(l) {
            Line::Ins(i) => i,
            _ => {
                val.clear();
                epoch += 1;
                continue;
            }
        };
        let a = &i.args;
        let op = i.op.as_str();
        // the value number of a source operand (registers and constants)
        let operand = |s: &str, val: &mut HashMap<i64, u32>, next: &mut u32| -> String {
            match reg_num(s) {
                Some(0) => "#0".to_string(),
                Some(r) => format!("v{}", *val.entry(r).or_insert_with(|| fresh(next))),
                None => format!("#{s}"),
            }
        };
        let flags_dead = live[k] & FLAGS == 0;
        // (destination, its new value's key, whether the instruction sets flags)
        let computed: Option<(i64, String, bool)> = match op {
            _ if ALU3.contains(&op) && a.len() == 3 && !matches!(op, "adc" | "sbc") => {
                let (x, y) = (operand(&a[1], &mut val, &mut next), operand(&a[2], &mut val, &mut next));
                reg_num(&a[0]).map(|d| (d, format!("{op}({x},{y})"), true))
            }
            _ if ALU2.contains(&op) && a.len() == 2 => {
                let x = operand(&a[1], &mut val, &mut next);
                reg_num(&a[0]).map(|d| (d, format!("{op}({x})"), true))
            }
            "mov" if a.len() == 2 => {
                let flags = reg_num(&a[1]).is_some();
                let x = operand(&a[1], &mut val, &mut next);
                reg_num(&a[0]).map(|d| (d, format!("={x}"), flags))
            }
            "li" | "la" | "lea" if a.len() == 2 => reg_num(&a[0]).map(|d| (d, format!("{op}#{}", a[1]), false)),
            "ldw" | "ldt" if a.len() == 2 => match (reg_num(&a[0]), mem_parts(&a[1])) {
                (Some(d), Some((base, rest))) => {
                    let b = base.map_or("abs".to_string(), |r| operand(&format!("r{r}"), &mut val, &mut next));
                    Some((d, format!("{op}[{b}{rest}]@{epoch}"), false))
                }
                _ => None,
            },
            "cmp" => {
                continue; // flags only
            }
            _ => None,
        };
        match computed {
            Some((d, key, sets_flags)) => {
                // "=vN" (a copy) is the value N itself
                let v = match key.strip_prefix("=v") {
                    Some(n) => n.parse().ok(),
                    None => None,
                };
                let v = v.unwrap_or_else(|| *exprs.entry(key).or_insert_with(|| fresh(&mut next)));
                if d != 0 && bit(d) & FRAME == 0 && val.get(&d) == Some(&v) && (flags_dead || !sets_flags) {
                    return Some(k);
                }
                if d != 0 {
                    val.insert(d, v);
                }
            }
            None => {
                match info(&i).map(|inf| inf.ctl) {
                    // a conditional branch: its fall-through keeps the values
                    Some(Ctl::Branch(_)) => continue,
                    _ => {}
                }
                match op {
                    // stores move memory on; registers stay
                    "stw" | "stt" | "fstd" | "fsts" | "out" => epoch += 1,
                    _ => {
                        val.clear();
                        epoch += 1;
                    }
                }
            }
        }
    }
    None
}

/// Optimize one function's lines in place.
pub fn run(lines: &mut Vec<String>) {
    // every change removes a line, so this ends
    loop {
        let Some(live) = liveness(lines) else { return };
        let mut changed = false;
        let mut k = 0;
        while k < lines.len() {
            let Line::Ins(a) = classify(&lines[k]) else {
                k += 1;
                continue;
            };
            let Some(ia) = info(&a) else { return };
            // drop dead code (never a write to sp or fp: what lies below sp
            // is not ours, an interrupt may push there at any moment)
            if ia.pure && ia.defs & live[k] == 0 && ia.defs & FRAME == 0 {
                lines.remove(k);
                changed = true;
                break; // liveness is stale now
            }
            if a.op == "mov" && a.args.len() == 2 && a.args[0] == a.args[1] && live[k] & FLAGS == 0 {
                lines.remove(k);
                changed = true;
                break;
            }
            let next = k + 1;
            if next < lines.len() {
                if let Line::Ins(b) = classify(&lines[next]) {
                    let Some(ib) = info(&b) else { return };
                    // forward a copy: mov r1, X ; I(r1) -> I(X)
                    if a.op == "mov" && a.args.len() == 2 && reg_num(&a.args[1]).is_some() && a.args[0] != a.args[1] {
                        let (t, x) = (a.args[0].clone(), a.args[1].clone());
                        let tb = bit(reg_num(&t).unwrap());
                        let first_src = usize::from(ib.dst0);
                        let reads = b.args.iter().skip(first_src).any(|o| regs_of(o) & tb != 0);
                        let dead_after = live[next] & tb == 0 || (ib.defs & tb != 0 && ib.dst0 && b.args[0] == t);
                        let only_src = !ib.dst0 || b.args[0] != t || ib.defs & tb != 0;
                        let simple = ALU3.contains(&b.op.as_str())
                            || ALU2.contains(&b.op.as_str())
                            || matches!(b.op.as_str(), "mov" | "cmp" | "stw" | "stt" | "ldw" | "ldt" | "out");
                        if reads && dead_after && only_src && simple && live[k] & FLAGS == 0 {
                            let mut nb = b.clone();
                            substitute(&mut nb, first_src, &t, &x);
                            lines[next] = nb.text();
                            lines.remove(k);
                            changed = true;
                            break;
                        }
                    }
                    // retarget a result: I -> r1 ; mov Y, r1 -> I -> Y
                    if ia.dst0
                        && ia.pure | matches!(a.op.as_str(), "ldw" | "ldt" | "div" | "mod" | "divr" | "modr")
                        && b.op == "mov"
                        && b.args.len() == 2
                        && b.args[1] == a.args[0]
                        && reg_num(&b.args[0]).is_some()
                        && b.args[0] != b.args[1]
                    {
                        let t = bit(reg_num(&a.args[0]).unwrap());
                        if live[next] & (t | FLAGS) == 0 {
                            let mut na = a.clone();
                            na.args[0] = b.args[0].clone();
                            lines[k] = na.text();
                            lines.remove(next);
                            changed = true;
                            break;
                        }
                    }
                }
            }
            k += 1;
        }
        if !changed {
            // last, once the local rewrites are done (they turn li r1, X ;
            // mov Y, r1 into li Y, X, which deleting the li would prevent)
            match redundant(lines, &live) {
                Some(k) => {
                    lines.remove(k);
                }
                None => return,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The line value numbering would delete next, if any.
    fn next_redundant(asm: &str) -> Option<String> {
        let lines: Vec<String> = asm.lines().map(|l| l.to_string()).filter(|l| !l.trim().is_empty()).collect();
        let live = liveness(&lines).expect("analysable");
        redundant(&lines, &live).map(|k| format!("{k}: {}", lines[k].trim()))
    }

    #[test]
    fn a_reload_after_a_branch_goes() {
        let asm = "        mul r2, r-5, 3
        ldw r1, [r2+29844]
        cmp r1, r-4
        ble .L1
        mul r2, r-5, 3
        ldw r1, [r2+29844]
        stw r1, [r2+29847]
.L1:
        ret";
        assert_eq!(next_redundant(asm).as_deref(), Some("4: mul r2, r-5, 3"));
        let without = asm.replacen("        mul r2, r-5, 3\n        ldw r1, [r2+29844]\n        stw", "        ldw r1, [r2+29844]\n        stw", 1);
        assert_eq!(next_redundant(&without).as_deref(), Some("4: ldw r1, [r2+29844]"));
    }

    #[test]
    fn what_may_have_changed_stays() {
        // a store in between: the load must be done again
        let store = "        mul r2, r-5, 3
        ldw r1, [r2+29844]
        stw r-6, [r-7]
        ldw r1, [r2+29844]
        ret";
        assert_eq!(next_redundant(store), None);
        // a label in between: another way in
        let label = "        mul r2, r-5, 3
        ldw r1, [r2+29844]
.L2:
        mul r2, r-5, 3
        ldw r1, [r2+29844]
        stw r1, [r2+3]
        b .L2";
        assert_eq!(next_redundant(label), None);
        // the index changed
        let changed = "        mul r2, r-5, 3
        ldw r1, [r2+29844]
        add r-5, r-5, 1
        mul r2, r-5, 3
        ldw r1, [r2+29844]
        ret";
        assert_eq!(next_redundant(changed), None);
        // the flags of the second multiply are read
        let flags = "        mul r-2, r-5, 3
        cmp r-5, 0
        mul r-2, r-5, 3
        bgt .L3
.L3:
        ret";
        assert_eq!(next_redundant(flags), None);
        // a call in between may change registers and memory
        let call = "        mul r-2, r-5, 3
        call f
        mul r-2, r-5, 3
        ret";
        assert_eq!(next_redundant(call), None);
    }
}
