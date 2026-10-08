//! `long`, Trine's 81-trit integer: arithmetic against a model, at every
//! optimization level, and the type rules.

use trine::{build_source, run_rom, Options, Outcome};

const W: i128 = 7_625_597_484_987; // 3^27
const WMAX: i128 = (W - 1) / 2;

/// A long as its three balanced words, high first (the model).
#[derive(Clone, Copy, Debug, PartialEq)]
struct L([i128; 3]);

impl L {
    /// From words low first, carrying anything past a word upward and
    /// dropping what falls off the top (the wrap at 81 trits).
    fn norm(mut lo_first: [i128; 3]) -> L {
        let mut carry = 0;
        for w in lo_first.iter_mut() {
            let v = *w + carry;
            let lo = ternary::wrap_trits(v, 27);
            carry = (v - lo) / W;
            *w = lo;
        }
        L([lo_first[2], lo_first[1], lo_first[0]])
    }
    fn neg(self) -> L {
        L([-self.0[0], -self.0[1], -self.0[2]])
    }
    fn add(self, o: L) -> L {
        L::norm([self.0[2] + o.0[2], self.0[1] + o.0[1], self.0[0] + o.0[0]])
    }
    fn mul(self, o: L) -> L {
        let (a, b) = ([self.0[2], self.0[1], self.0[0]], [o.0[2], o.0[1], o.0[0]]);
        let mut r = [0i128; 3];
        for i in 0..3 {
            for j in 0..3 - i {
                // carried at once: a product of two words is far inside i128
                let p = a[i] * b[j];
                let lo = ternary::wrap_trits(p, 27);
                r[i + j] += lo;
                if i + j + 1 < 3 {
                    r[i + j + 1] += (p - lo) / W;
                }
                let n = L::norm(r);
                r = [n.0[2], n.0[1], n.0[0]];
            }
        }
        L::norm(r)
    }
    fn cmp(self, o: L) -> std::cmp::Ordering {
        let ((na, a), (nb, b)) = (self.mag(), o.mag());
        match (na, nb) {
            (false, false) => a.cmp(&b),
            (true, true) => b.cmp(&a),
            (false, true) => std::cmp::Ordering::Greater,
            (true, false) => std::cmp::Ordering::Less,
        }
    }
    fn sign(self) -> i32 {
        self.0.iter().find(|w| **w != 0).map_or(0, |w| w.signum() as i32)
    }
    /// (negative, magnitude): the value exactly (|v| < 3^81 / 2 < 2^128).
    fn mag(self) -> (bool, u128) {
        let n = self.sign() < 0;
        let v = if n { self.neg() } else { self };
        let w = W as u128;
        let m = (v.0[0] as u128).wrapping_mul(w * w).wrapping_add((v.0[1] as u128).wrapping_mul(w)).wrapping_add(v.0[2] as u128);
        (n, m)
    }
    fn from_mag(neg: bool, m: u128) -> L {
        let w = W as u128;
        let v = L::norm([(m % w) as i128, ((m / w) % w) as i128, (m / (w * w)) as i128]);
        if neg { v.neg() } else { v }
    }
    fn divmod(self, o: L) -> (L, L) {
        let ((na, a), (nb, b)) = (self.mag(), o.mag());
        (L::from_mag(na != nb, a / b), L::from_mag(na, a % b))
    }
    fn words(self) -> String {
        format!("{},{},{}", self.0[0], self.0[1], self.0[2])
    }
    fn text(self) -> String {
        let (n, m) = self.mag();
        format!("{}{m}", if n { "-" } else { "" })
    }
    fn trits(self) -> Vec<i128> {
        // 81 trits, most significant first
        let mut t = Vec::new();
        for w in self.0 {
            let mut v = w;
            let mut ws = vec![0; 27];
            for k in (0..27).rev() {
                let d = ternary::wrap_trits(v, 1);
                ws[k] = d;
                v = (v - d) / 3;
            }
            t.extend(ws);
        }
        t
    }
    fn from_trits(t: &[i128]) -> L {
        let mut w = [0i128; 3];
        for (k, d) in t.iter().enumerate() {
            w[k / 27] = w[k / 27] * 3 + d;
        }
        L(w)
    }
    fn shift(self, n: i64) -> L {
        let t = self.trits();
        let mut r = vec![0; 81];
        for (k, d) in t.iter().enumerate() {
            let to = k as i64 - n;
            if (0..81).contains(&to) {
                r[to as usize] = *d;
            }
        }
        L::from_trits(&r)
    }
    fn tritwise(self, o: L, max: bool) -> L {
        let (a, b) = (self.trits(), o.trits());
        let r: Vec<i128> = a.iter().zip(&b).map(|(x, y)| if max { *x.max(y) } else { *x.min(y) }).collect();
        L::from_trits(&r)
    }
    /// As a Trine literal: decimal, with unary minus.
    fn lit(self) -> String {
        let (n, m) = self.mag();
        if n { format!("(-{m})") } else { format!("{m}") }
    }
}

fn values() -> Vec<L> {
    let max = L([WMAX, WMAX, WMAX]);
    let mut v = vec![
        L([0, 0, 0]),
        L([0, 0, 1]),
        L([0, 0, -1]),
        L([0, 0, 7]),
        L([0, 0, WMAX]),
        L([0, 1, -WMAX]), // WMAX + 1: the first value past a word
        L([0, 1, 0]),
        L([1, 0, 0]),
        L([0, -1, 5]),
        max,
        max.neg(),
        L([WMAX / 3, 12345, -678]), // the divisor's top trit 0 / not, both ways
    ];
    // random 81-trit values, and short ones
    let mut s: u64 = 0x2545_F491_4F6C_DD1D;
    for k in 0..8 {
        let mut t = vec![0i128; 81];
        let start = if k % 2 == 0 { 0 } else { 54 };
        for d in t.iter_mut().skip(start) {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            *d = (s % 3) as i128 - 1;
        }
        v.push(L::from_trits(&t));
    }
    v
}

fn run_all_levels(src: &str, steps: u64) -> String {
    let mut first: Option<String> = None;
    for l in 0..=4u8 {
        let o = Options { level: Some(l), ..Options::rom() };
        let b = build_source("long.tri", src, None, &o).unwrap_or_else(|e| panic!("-O{l}: {e}"));
        let r = run_rom(&b.image, steps, "");
        assert_eq!(r.outcome, Outcome::Exit(0), "-O{l}: {}", r.serial);
        match &first {
            None => first = Some(r.serial),
            Some(f) => assert_eq!(&r.serial, f, "-O{l} differs from -O0"),
        }
    }
    first.unwrap()
}

/// The values a program wrote to SYSCTL.DEBUG (port 3), the same at every
/// level (crt0's exit tag and code end it).
fn debug_all_levels(src: &str, steps: u64) -> Vec<i64> {
    let mut first: Option<Vec<i64>> = None;
    for l in 0..=4u8 {
        let o = Options { level: Some(l), ..Options::rom() };
        let b = build_source("long.tri", src, None, &o).unwrap_or_else(|e| panic!("-O{l}: {e}"));
        let r = run_rom(&b.image, steps, "");
        assert_eq!(r.outcome, Outcome::Exit(0), "-O{l}: {}", r.serial);
        let log = r.machine.dev.debug_log.clone();
        match &first {
            None => first = Some(log),
            Some(f) => assert!(&log == f, "-O{l} differs from -O0"),
        }
    }
    let mut v = first.unwrap();
    v.truncate(v.len() - 2); // the exit tag and code
    v
}

const PRELUDE: &str = "import \"console.tri\";\nimport \"long.tri\";\n\
fn show(v: long) { var b: [41]char; ltoa(v, &b[0]); print(&b[0]); print(\" \"); }\n\
fn nl() { print(\"\\n\"); }\n";

#[test]
fn arithmetic_matches_the_model() {
    let vs = values();
    let mut src = String::from(PRELUDE);
    // each result as its words, high first (through an *int view)
    src += "fn words(v: long) { var p: *int = &v as *int; out(3, p[0]); out(3, p[1]); out(3, p[2]); }\n";
    src += &format!("var v: [{}]long;\nfn main() -> int {{\n", vs.len());
    for (k, x) in vs.iter().enumerate() {
        src += &format!("    v[{k}] = {};\n", x.lit());
    }
    src = src.replace("fn show(v: long) { var b: [41]char; ltoa(v, &b[0]); print(&b[0]); print(\" \"); }\n", "");
    src += &format!(
        "    var i: int = 0;\n    while (i < {n}) {{\n        var j: int = 0;\n        while (j < {n}) {{\n\
            var a: long = v[i];\n            var b: long = v[j];\n\
            words(a + b); words(a - b); words(a * b); words(a & b); words(a | b);\n\
            out(3, a < b ? -1 : a == b ? 0 : 1);\n\
            if (b != 0) {{ words(a / b); words(a % b); }}\n            j += 1;\n        }}\n        i += 1;\n    }}\n    return 0;\n}}\n",
        n = vs.len()
    );
    let got = debug_all_levels(&src, 2_000_000_000);
    let mut k = 0;
    for a in &vs {
        for b in &vs {
            let mut want: Vec<i128> = Vec::new();
            for r in [a.add(*b), a.add(b.neg()), a.mul(*b), a.tritwise(*b, false), a.tritwise(*b, true)] {
                want.extend(r.0);
            }
            want.push(match a.cmp(*b) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            });
            if b.sign() != 0 {
                let (q, r) = a.divmod(*b);
                want.extend(q.0);
                want.extend(r.0);
            }
            let n = want.len();
            let g: Vec<i128> = got[k..(k + n).min(got.len())].iter().map(|x| *x as i128).collect();
            assert_eq!(g, want, "a = {}, b = {} (sum, difference, product, &, |, order, quotient, remainder)", a.text(), b.text());
            k += n;
        }
    }
    assert_eq!(k, got.len());
}

#[test]
fn shifts_and_conversions() {
    let vs = values();
    let counts = [0i64, 1, -1, 5, -5, 26, -26, 27, -27, 28, 54, -55, 80, -80, 81, -81, 200];
    let mut src = String::from(PRELUDE);
    src += "fn words(v: long) { var p: *int = &v as *int; print_int(p[0]); print(\",\"); print_int(p[1]); print(\",\"); print_int(p[2]); print(\" \"); }\n";
    src += &format!("var v: [{}]long;\nfn main() -> int {{\n", vs.len());
    for (k, x) in vs.iter().enumerate() {
        src += &format!("    v[{k}] = {};\n", x.lit());
    }
    src += "    var i: int = 0;\n";
    src += &format!("    while (i < {}) {{\n        var a: long = v[i];\n", vs.len());
    for c in counts {
        src += &format!("        words(a << {c}); words(a >> {c});\n");
    }
    // narrowing keeps the low trits; a long's truth is "not zero"
    src += "        print_int(a as int); print(\" \"); print_int(a as tryte); print(\" \"); print_int((a as trit) as int); print(\" \");\n";
    src += "        print(a ? \"T \" : \"F \"); print(!a ? \"T \" : \"F \");\n";
    src += "        sign (a) { neg: { print(\"- \"); } zero: { print(\"0 \"); } pos: { print(\"+ \"); } }\n";
    src += "        sign (a <=> 0) { neg: { print(\"- \"); } zero: { print(\"0 \"); } pos: { print(\"+ \"); } }\n";
    src += "        nl();\n        i += 1;\n    }\n    return 0;\n}\n";
    let got = run_all_levels(&src, 1_000_000_000);
    let mut want = String::new();
    for a in &vs {
        for c in counts {
            want += &format!("{} {} ", a.shift(c).words(), a.shift(-c).words());
        }
        let lo = a.0[2];
        let s = match a.sign() {
            -1 => "-",
            0 => "0",
            _ => "+",
        };
        want += &format!(
            "{lo} {} {} {} {} {s} {s} \n",
            ternary::wrap_trits(lo, 9),
            ternary::wrap_trits(lo, 1),
            if a.sign() != 0 { "T" } else { "F" },
            if a.sign() == 0 { "T" } else { "F" }
        );
    }
    for (k, (g, w)) in got.lines().zip(want.lines()).enumerate() {
        assert_eq!(g, w, "a = {}", vs[k].text());
    }
    assert_eq!(got.lines().count(), want.lines().count());
}

#[test]
fn longs_in_memory_calls_and_assignments() {
    // fields, arrays, pointers and globals; parameters (three slots each,
    // some on the stack) and results; compound assignment; ?:; mixed with
    // ints; text both ways; and a long kept across calls in a leaf
    let src = r#"
        import "console.tri";
        import "long.tri";
        struct P { t: tryte; x: long; n: int; }
        var gp: P;
        var ga: [4]long;
        var gl: long = 1000000000000000000;
        fn show(v: long) { var b: [41]char; ltoa(v, &b[0]); print(&b[0]); print(" "); }
        fn mix(a: int, x: long, b: int, y: long, c: int) -> long { return x * a + y * b + c; }
        fn twice(p: *long) { *p = *p * 2; }
        fn fact(n: int) -> long { var r: long = 1; while (n > 1) { r *= n; n -= 1; } return r; }
        fn main() -> int {
            gp.t = 'A'; gp.x = -77; gp.n = 9;
            gp.x += 7000000000000000000000;
            show(gp.x); print_int(gp.t); print(" "); print_int(gp.n); print("\n");
            var i: int = 0;
            while (i < 4) { ga[i] = fact(10 + 10 * i); i += 1; }
            show(ga[0]); show(ga[1]); show(ga[2]); show(ga[3]); print("\n");
            var p: *long = &ga[1];
            twice(p); p[1] -= 1; ga[3] /= 1000; ga[0] %= 1000;
            show(ga[0]); show(ga[1]); show(ga[2]); show(ga[3]); print("\n");
            show(mix(2, gl, 3, -gl, 4)); show(mix(-1, 5, 0, 6, -7)); print("\n");
            var x: long = 5;
            var y: int = 3;
            x = x * y + 1;                      // long and int
            x <<= 2; x >>= 1; x |= 1; x &= 0t111;
            show(x); show(y > 2 ? x : 0); show(y > 5 ? x : -1); print("\n");
            show(atol("  -123456789012345678901234567890")); show(atol("+42")); print("\n");
            show(gl); show(fact(30) / fact(28)); print("\n");
            return 0;
        }
    "#;
    let got = run_all_levels(src, 400_000_000);
    let f = |n: u128| (1..=n).product::<u128>();
    let bits = || L::from_mag(false, 16).shift(2).shift(-1).tritwise(L::from_mag(false, 1), true).tritwise(L::from_mag(false, 13), false).text();
    let want = format!(
        "6999999999999999999923 65 9\n{} {} {} {} \n{} {} {} {} \n{} {} \n{} {} {} \n{} {} \n{} {} \n",
        f(10),
        f(20),
        f(30),
        L::from_mag(false, 1).mul(L::from_mag(false, f(30))).mul(L::from_mag(false, 31 * 32 * 33 * 34 * 35 * 36 * 37 * 38 * 39 * 40)).text(),
        f(10) % 1000,
        2 * f(20),
        f(30) - 1,
        L::from_mag(false, f(30)).mul(L::from_mag(false, 31 * 32 * 33 * 34 * 35 * 36 * 37 * 38 * 39 * 40)).divmod(L::from_mag(false, 1000)).0.text(),
        -1000000000000000000i128 + 4,
        -5 - 7,
        // ((5*3+1) << 2) >> 1, then | 1 and & 0t111 (tritwise)
        bits(),
        bits(),
        -1,
        "-123456789012345678901234567890",
        42,
        1000000000000000000u128,
        30 * 29,
    );
    assert_eq!(got, want);
}

#[test]
fn reals_and_longs() {
    // a long to the nearest real and back (ties to even); a real out of
    // range saturates, a NaN gives 0; mixed arithmetic is real
    let src = r#"
        import "console.tri";
        import "long.tri";
        fn show(v: long) { var b: [41]char; ltoa(v, &b[0]); print(&b[0]); print(" "); }
        fn rl(x: real, l: long, k: int, y: real) -> long { return l * k + (x as long) - (y as long); }
        fn main() -> int {
            show(rl(2.0, 1000000000000000000000, 3, 7.0));
            var big: long = 0t1000000000000000000000000000000000000000000000000000000000000; // 3^60
            var x: real = big as real;
            show(x as long);
            show((x * 3.0) as long - big * 3);
            show(2.5 as long); show(3.5 as long); show(-2.5 as long); show(-1e10 as long);
            show(1e300 as long); show(-1e300 as long);
            var z: real = 0.0;
            show((z / z) as long);
            var l: long = 123456789012345678901234567890;
            var r: real = l;                   // implicit, as for an int
            print(r == 1.2345678901234567890123456789e29 ? "eq " : "ne ");
            print(l + 0.5 > 1e29 ? "gt " : "le ");
            show((r - 1e29) as long);
            print("\n");
            return 0;
        }
    "#;
    let got = run_all_levels(src, 100_000_000);
    let max = L([WMAX, WMAX, WMAX]);
    let want = format!(
        "2999999999999999999995 {} 0 2 4 -2 -10000000000 {} {} 0 eq gt 23456789012345678901234567890 \n",
        3u128.pow(60),
        max.text(),
        max.neg().text()
    );
    assert_eq!(got, want);
}

fn compile_err(src: &str) -> String {
    match build_source("test.tri", src, None, &Options::rom()) {
        Ok(_) => panic!("expected a compile error: {src}"),
        Err(e) => e,
    }
}

#[test]
fn long_type_rules() {
    let cases = [
        ("fn main() -> int { var l: long = 5; var i: int = l; return i; }", "expected int, found long (use `as` to narrow)"),
        ("fn main() -> int { var l: long = 1.5; return 0; }", "expected long, found real (use `as` to convert)"),
        ("fn main() -> int { var l: long = 5; l = l << l; return 0; }", "cannot apply Shl to long and long"),
        ("fn main() -> int { var a: [3]int; var l: long = 1; var p: *int = &a[0]; p = p + l; return 0; }", "cannot apply Add to *int and long"),
        ("fn main() -> int { var l: long = 5; l = l / 0; return 0; }", "division by zero"),
        ("const C: long = 5;\nfn main() -> int { return 0; }", "a constant of type long is not supported"),
        ("fn main() -> int { var l: long = 5; switch (l) { case 1: { } } return 0; }", "expected an integer, found long"),
        ("fn main() -> int { var a: [3]int; var l: long = 1; return a[l]; }", "expected an integer, found long"),
        ("fn main() -> int { return 1000000000000000000000; }", "expected int, found long (use `as` to narrow)"),
        ("fn main() -> int { var i: int = 0; i += 100000000000000; return i; }", "cannot apply Add to int and long"),
        ("fn main() -> int { var l: long = 1; var p: *int = l as *int; return 0; }", "cannot convert long to *int"),
        ("fn main() -> int { var l: long = 1; return l < null; }", "cannot compare long with null"),
        // past 81 trits
        ("fn main() -> int { var l: long = 1000000000000000000000000000000000000000; return 0; }", "bad number `1000000000000000000000000000000000000000`"),
        ("fn main() -> int { var l: long = 0t1000000000000000000000000000000000000000000000000000000000000000000000000000000000; return 0; }", "bad 0t literal: ternary literal does not fit"),
        ("fn main() -> int { var l: long = 0x10000000000000000000000000000000000; return 0; }", "bad 0x literal: hexadecimal literal does not fit"),
        ("fn main(n: long) -> int { return 0; }", "`main` takes at most two parameters"),
    ];
    for (src, want) in cases {
        let e = compile_err(src);
        assert!(e.contains(want), "{src}\n  gave: {e}\n  want: {want}");
    }
    // the largest literals fit: 81 trits, (3^81 - 1) / 2, and 39 decimal digits
    let ok = "import \"console.tri\";\nimport \"long.tri\";\nfn main() -> int { \
        var a: long = 0t111111111111111111111111111111111111111111111111111111111111111111111111111111111; \
        var b: long = 221713244121518884974124815309574946401; \
        var c: [41]char; ltoa(a - b, &c[0]); print(&c[0]); return 0; }";
    let b = build_source("ok.tri", ok, None, &Options::rom()).unwrap_or_else(|e| panic!("{e}"));
    let r = run_rom(&b.image, 10_000_000, "");
    assert_eq!(r.serial, "0");
}
