//! The real27/real81 conformance vectors (docs/REAL.md §8), made by an
//! independent exact-arithmetic reference (tools/gen_realvectors.py),
//! against this library: every result and every flag, through the
//! fixed-width fast paths and through the exact (bignum) ones.

use setun_real::{Real, DZ, IV, IX, OF, UF};
use std::cmp::Ordering;

const VECTORS: &str = include_str!("vectors.txt");

fn real(s: &str) -> Real {
    let (e, m) = s.split_once(':').unwrap_or_else(|| panic!("not a real: {s}"));
    Real { e: e.parse().unwrap(), m: m.parse().unwrap() }
}

fn flags(s: &str) -> u8 {
    if s == "-" {
        return 0;
    }
    s.split('+')
        .map(|f| match f {
            "IV" => IV,
            "DZ" => DZ,
            "OF" => OF,
            "UF" => UF,
            "IX" => IX,
            _ => panic!("unknown flag {f}"),
        })
        .sum()
}

/// One vector: (op, operands, result, flags).
fn parse(line: &str) -> (&str, Vec<&str>, &str, &str) {
    let (lhs, rhs) = line.split_once(" = ").unwrap_or_else(|| panic!("bad vector: {line}"));
    let (op, args) = lhs.split_once(' ').unwrap();
    // a decimal operand is quoted and has no spaces
    let args: Vec<&str> = args.split(' ').collect();
    let (res, fl) = rhs.rsplit_once(' ').unwrap();
    (op, args, res, fl)
}

/// The library's answer, as text in the vectors' notation; `fast` picks
/// the fast paths where the library has both.
fn answer(op: &str, a: &[&str], fast: bool) -> (String, u8) {
    let r = |x: Real| format!("{}:{}", x.e, x.m);
    let two = |f: fn(Real, Real, bool) -> (Real, u8)| {
        let (v, fl) = f(real(a[0]), real(a[1]), fast);
        (r(v), fl)
    };
    match op {
        "add" => two(|x, y, f| x.add_with(y, f)),
        "sub" => {
            let (x, y) = (real(a[0]), real(a[1]));
            let (v, fl) = x.sub(y);
            (r(v), fl)
        }
        "mul" => two(|x, y, f| x.mul_with(y, f)),
        "div" => two(|x, y, f| x.div_with(y, f)),
        "rem" => two(|x, y, _| x.rem(y)),
        "min" => (r(real(a[0]).min(real(a[1]))), 0),
        "max" => (r(real(a[0]).max(real(a[1]))), 0),
        "cmp" => {
            let s = match real(a[0]).cmp(real(a[1])) {
                Some(Ordering::Less) => "lt",
                Some(Ordering::Equal) => "eq",
                Some(Ordering::Greater) => "gt",
                None => "un",
            };
            (s.into(), 0)
        }
        "sqrt" => {
            let (v, fl) = real(a[0]).sqrt_with(fast);
            (r(v), fl)
        }
        "rnd" | "trunc" => {
            let (v, fl) = real(a[0]).round_int(op == "trunc");
            (r(v), fl)
        }
        "to27" => {
            let (v, fl) = real(a[0]).to_real27();
            (r(v), fl)
        }
        "neg" => (r(real(a[0]).neg()), 0),
        "abs" => (r(real(a[0]).abs()), 0),
        "class" => (real(a[0]).class().to_string(), 0),
        "exp" => {
            let (v, fl) = real(a[0]).exponent();
            (v.to_string(), fl)
        }
        "cvtfi" => {
            let (v, fl) = real(a[0]).to_i64();
            (v.to_string(), fl)
        }
        "cvtif" => (r(Real::from_i64(a[0].parse().unwrap())), 0),
        "scale" => {
            let (v, fl) = real(a[0]).scale(a[1].parse().unwrap());
            (r(v), fl)
        }
        "dec" => {
            let (v, fl) = Real::from_decimal(a[0].trim_matches('"')).unwrap();
            (r(v), fl)
        }
        _ => panic!("unknown op {op}"),
    }
}

fn check(fast: bool) {
    let mut n = 0;
    let mut bad = Vec::new();
    for line in VECTORS.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let (op, args, res, fl) = parse(line);
        let (got, gf) = answer(op, &args, fast);
        if got != res || gf != flags(fl) {
            bad.push(format!("{line}\n    got {got} flags {gf}"));
        }
        n += 1;
    }
    assert!(n > 10_000, "only {n} vectors");
    assert!(bad.is_empty(), "{} of {n} vectors differ ({}):\n{}", bad.len(), if fast { "fast paths" } else { "exact paths" }, bad[..bad.len().min(20)].join("\n"));
}

#[test]
fn the_library_meets_the_standard() {
    check(true);
}

#[test]
fn the_exact_paths_meet_it_too() {
    check(false);
}
