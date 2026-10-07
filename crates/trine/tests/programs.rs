//! Trine programs compiled and run on the emulator.

use trine::{build_source, run_rom, Options, Outcome};

fn run_with(src: &str, input: &str) -> (Outcome, String) {
    let b = build_source("test.tri", src, None, &Options::rom()).unwrap_or_else(|e| panic!("compile error: {e}"));
    let r = run_rom(&b.image, 20_000_000, input);
    (r.outcome, r.serial)
}

fn exit(src: &str) -> i64 {
    match run_with(src, "").0 {
        Outcome::Exit(c) => c,
        o => panic!("program did not exit: {o:?}"),
    }
}

fn compile_err(src: &str) -> String {
    match build_source("test.tri", src, None, &Options::rom()) {
        Ok(_) => panic!("expected a compile error"),
        Err(e) => e,
    }
}

#[test]
fn ternary_literals() {
    // base 3: 1, 0, and T or an overlined 1 (mixed is fine); base 27: 0-9, A-D,
    // negative digits overlined, letters in either case; hex for interop
    assert_eq!(exit("fn main() -> int { return 0t1T0; }"), 6);
    assert_eq!(exit("fn main() -> int { return 0t11\u{305}0 - 0t1T0; }"), 0);
    assert_eq!(exit("fn main() -> int { return 0tT11\u{305}; }"), -9 + 3 - 1);
    assert_eq!(exit("fn main() -> int { return 0z1D\u{305}; }"), 27 - 13);
    assert_eq!(exit("fn main() -> int { return 0zdd + 0zDD\u{305}; }"), 13 * 27 + 13 + 13 * 27 - 13);
    assert_eq!(exit("fn main() -> int { var t: tryte = 0zD\u{305}D\u{305}D\u{305}; return t; }"), -9841);
    assert_eq!(exit("fn main() -> int { return 0x1F + -0x10; }"), 15);
    assert_eq!(exit("fn main() -> int { return 0zB007AB13 % 1000; }"), 199);
    assert_eq!(exit("fn main() -> int { return 0t1 - 1; }"), 0, "0t1 minus 1, with a space");
    assert_eq!(exit("fn main() -> int { return 0t1 -1; }"), 0, "a space before the operator is enough");
}

#[test]
fn ternary_literals_refused() {
    let e = compile_err("fn main() -> int { return 0t+-0; }");
    assert!(e.contains("0t digits are 1, 0 and T (the old + and - are gone)"), "{e}");
    // old-notation digits after a valid prefix: 0t0+0 was 3, never 0 + 0
    for src in ["0t0+-", "0t0+0", "0t1-1", "0t1T+x"] {
        let e = compile_err(&format!("fn main() -> int {{ var x: int = 1; return {src}; }}"));
        assert!(e.contains("put a space before the operator"), "{src}: {e}");
    }
    let e = compile_err("fn main() -> int { return 0zMMm; }");
    assert!(e.contains("bad 0z literal: invalid ternary digit 'M'"), "{e}");
    let e = compile_err("fn main() -> int { return 0zE; }");
    assert!(e.contains("invalid ternary digit 'E'"), "{e}");
    let e = compile_err("fn main() -> int { return 0zE\u{305}; }");
    assert!(e.contains("'E' cannot be overlined"), "{e}");
    let e = compile_err("fn main() -> int { return 0z_\u{305}1; }");
    assert!(e.contains("'_' cannot be overlined"), "{e}");
    let e = compile_err("fn main() -> int { return 0z10\u{305}; }");
    assert!(e.contains("'0' cannot be overlined"), "{e}");
    let e = compile_err("fn main() -> int { return 0t1T\u{305}; }");
    assert!(e.contains("'T' cannot be overlined"), "{e}");
    let e = compile_err("fn main() -> int { return 0z1\u{305}\u{305}; }");
    assert!(e.contains("'1' has two overlines"), "{e}");
    let e = compile_err("fn main() -> int { return 0t2; }");
    assert!(e.contains("invalid ternary digit '2'"), "{e}");
    let e = compile_err("fn main() -> int { return 0xZ; }");
    assert!(e.contains("invalid hexadecimal digit 'Z'"), "{e}");
    let e = compile_err("fn main() -> int { return 0x5\u{305}; }");
    assert!(e.contains("hexadecimal digits cannot be overlined"), "{e}");
    // values that do not fit their type: constants are range-checked, and a
    // variable of a narrower type will not take an int literal that is too big
    let e = compile_err("const T: tryte = 0z1000;\nfn main() -> int { return T; }");
    assert!(e.contains("19683 does not fit in tryte"), "{e}");
    let e = compile_err("const T: trit = 0t1T;\nfn main() -> int { return T; }");
    assert!(e.contains("2 does not fit in trit"), "{e}");
    let e = compile_err("fn main() -> int { var t: tryte = 0z1000; return t; }");
    assert!(e.contains("expected tryte, found int"), "{e}");
    let e = compile_err("fn main() -> int { return 0z1000000000; }");
    assert!(e.contains("does not fit in a word"), "{e}");
}

#[test]
fn reals() {
    let src = r#"
        import "console.tri";
        import "real.tri";
        struct P { x: real; tag: int; y: real; }
        var g: real = 2.5;
        const HALF: real = 0.5;
        fn half(x: real) -> real { return x / 2; }
        fn hyp(a: real, n: int, b: real) -> real { return sqrt(a * a + b * b) * n; }
        fn show(x: real) { var b: [24]char; print(rtoa(x, 12, &b[0])); print(" "); }
        fn main() -> int {
            var a: real = 1.5;
            var b: real = a * 4 - 1;               // 5: ints convert
            var arr: [3]real;
            arr[1] = b + HALF;                     // 5.5
            var p: P;
            p.x = 3.0; p.y = 4.0; p.tag = 7;
            var h: real = hyp(p.x, 2, p.y);        // 10, mixed int and real arguments
            b += 1;                                // 6
            b *= 2;                                // 12
            var q: *real = &arr[1];
            *q = *q * 2;                           // 11
            show(h); show(b); show(arr[1]); show(half(g)); show(-a); show(1.0 / 3);
            show(10 % 3.0); show(7.0 | 2.5); show(7.0 & 2.5); show(a > 1 ? a : 0);
            show(abs(-2.25)); show(round(2.5)); show(round(3.5)); show(trunc(-2.7)); show(rem(7.0, 2));
            show(scale3(1.0, 4)); show(exponent3(81.0) as real); show(real27(0.1) - 0.1);
            show(fconst(3)); show((2.5 as int) as real); show((-2.5 as int) as real); show(3.7 as tryte);
            return (h + half(g) + arr[1]) as int;  // 10 + 1.25 + 11 = 22.25 → 22
        }
    "#;
    let (o, serial) = run_with(src, "");
    assert_eq!(o, Outcome::Exit(22));
    assert_eq!(
        serial,
        "10 12 11 1.25 -1.5 0.333333333333 1 7 2.5 1.5 2.25 2 4 -2 -1 81 5 2.58117479171e-10 2.71828182846 2 -2 4 "
    );
}

#[test]
fn real_comparisons_and_nan() {
    // a NaN is unordered: == < <= > >= are false and != is true; as a
    // condition it counts as non-zero, as in C
    let src = r#"
        fn main() -> int {
            var n: real = 0.0 / 0;
            var one: real = 1.0;
            var r: int = 0;
            if (n == n) { r = r + 1; }
            if (n != n) { r = r + 10; }
            if (n < one) { r = r + 100; }
            if (n >= one) { r = r + 1000; }
            if (n) { r = r + 10000; }
            if (one < 2.0 && one <= 1 && one >= 1.0 && one > 0 && one == 1 && one != 2) { r = r + 100000; }
            var v: int = (n == n) + 2 * (n != n) + 4 * (one < 2.0);
            sign (n <=> one) { neg: r = r - 1; zero: r = r + 1000000; pos: r = r - 1; }
            return r + 10000000 * v;
        }
    "#;
    assert_eq!(exit(src), 10 + 10000 + 100000 + 1000000 + 10000000 * 6);
}

#[test]
fn reals_refused() {
    let e = compile_err("fn main() -> int { var x: int = 2.5; return x; }");
    assert!(e.contains("expected int, found real (use `as` to convert)"), "{e}");
    let e = compile_err("fn main() -> int { var x: real = 2.5; switch (x) { case 1: return 1; } return 0; }");
    assert!(e.contains("expected an integer, found real"), "{e}");
    let e = compile_err("fn main() -> int { var x: real = 2.5; x = x << 1; return 0; }");
    assert!(e.contains("cannot apply Shl"), "{e}");
    let e = compile_err("fn main() -> int { var x: real = 2.5; var p: *int = null; p = p + x; return 0; }");
    assert!(e.contains("expected a real, found *int"), "{e}");
    let e = compile_err("fn f(a: int) -> int { return a; }\nfn main() -> int { return f(1.5); }");
    assert!(e.contains("expected int, found real"), "{e}");
    let e = compile_err("fn f() { return 1.5; }\nfn main() -> int { return 0; }");
    assert!(e.contains("a void function cannot return a value"), "{e}");
    let e = compile_err("fn main() -> int { var x: real = fconst(9); return 0; }");
    assert!(e.contains("fconst takes 0..6"), "{e}");
    let e = compile_err("fn main() -> int { var x: real = 1.5x; return 0; }");
    assert!(e.contains("bad number `1.5x`"), "{e}");
    let e = compile_err("fn main() -> int { var x: real = 2.0; var p: *real = &x; return p as real; }");
    assert!(e.contains("cannot convert *real to real"), "{e}");
    let e = compile_err("fn f(a: real, b: real, c: real, d: real, e: real, f: real, g: real, h: real, i: real, j: real) {}\nfn main() -> int { return 0; }");
    assert!(e.contains("at most 9 reals"), "{e}");
    let e = compile_err("fn main() -> int { var x: real = 2.0; x <<= 1; return 0; }");
    assert!(e.contains("cannot apply Shl to a real"), "{e}");
}

#[test]
fn return_value() {
    assert_eq!(exit("fn main() -> int { return 42; }"), 42);
    assert_eq!(exit("fn main() -> int { return -7; }"), -7);
}

#[test]
fn arithmetic_and_precedence() {
    assert_eq!(exit("fn main() -> int { return 2 + 3 * 4 - 10 / 3; }"), 11);
    assert_eq!(exit("fn main() -> int { var a: int = 7; var b: int = -2; return a / b * 100 + a % b; }"), -299);
    assert_eq!(exit("fn main() -> int { var x: int = 5; return -x + ~x; }"), -10, "~x == -x");
    assert_eq!(exit("fn main() -> int { var x: int = 2; return x << 3; }"), 54, "shift counts trits");
    assert_eq!(exit("fn main() -> int { var x: int = 100; return x >> 2; }"), 11, ">> rounds: 100/9");
    assert_eq!(exit("fn main() -> int { return divr(8, 3) * 10 + modr(8, 3); }"), 29);
}

#[test]
fn tritwise_operators() {
    // 0t10T & 0t110 = min per trit = 0t10T   ;  | = max = 0t110
    assert_eq!(exit("fn main() -> int { var a: int = 0t10T; return a & 0tT11; }"), {
        // min(+,-)=-, min(0,+)=0, min(-,+)=- → 0tT0T
        -9 - 1
    });
    assert_eq!(exit("fn main() -> int { var a: int = 0t10T; return a | 0tT11; }"), 9 + 3 + 1);
    assert_eq!(exit("fn main() -> int { return tmul(0t1T0, 0tTT1); }"), -9 + 3);
}

#[test]
fn comparisons_and_logic() {
    let src = "fn main() -> int {
        var a: int = 3; var b: int = 5;
        return (a < b) * 1000 + (a == b) * 100 + (a != b) * 10 + (a <= 3 && b >= 6 || !(a > b));
    }";
    assert_eq!(exit(src), 1011);
    assert_eq!(exit("fn main() -> int { var a: int = 9; return (a <=> 3) * 100 + (a <=> 9) * 10 + (2 <=> a); }"), 99);
}

#[test]
fn control_flow() {
    let src = "fn main() -> int {
        var s: int = 0;
        for (var i: int = 1; i <= 10; i += 1) {
            if (i == 5) { continue; }
            if (i == 9) { break; }
            s += i;
        }
        var n: int = 0;
        while (n < 3) { n += 1; }
        loop { n += 10; if (n > 50) { break; } }
        return s * 1000 + n;
    }";
    assert_eq!(exit(src), 31 * 1000 + 53);
}

#[test]
fn sign_statement_is_three_way() {
    let src = "fn classify(a: int, b: int) -> int {
        sign (a <=> b) { neg: return -1; zero: return 0; pos: return 1; }
        return 99;
    }
    fn main() -> int {
        var r: int = classify(1, 2) * 100 + classify(2, 2) * 10 + classify(3, 2);
        sign (r) { neg: r = r * 2; pos: r = r * 3; }
        return r;
    }";
    assert_eq!(exit(src), (-100 + 1) * 2);
}

#[test]
fn switch_statement() {
    let src = "fn f(c: char) -> int {
        switch (c) {
            case 'a', 'e': return 1;
            case 'z': return 26;
            default: return 0;
        }
        return -1;
    }
    fn main() -> int { return f('a') * 100 + f('z') + f('q') * 1000 + f('e') * 10000; }";
    assert_eq!(exit(src), 10126);
}

#[test]
fn functions_recursion_and_many_args() {
    let src = "fn fib(n: int) -> int { if (n < 2) { return n; } return fib(n - 1) + fib(n - 2); }
    fn sum8(a: int, b: int, c: int, d: int, e: int, f: int, g: int, h: int) -> int {
        return a + 2*b + 3*c + 4*d + 5*e + 6*f + 7*g + 8*h;
    }
    fn main() -> int { return fib(15) * 1000 + sum8(1, 1, 1, 1, 1, 1, 1, 2); }";
    assert_eq!(exit(src), 610 * 1000 + 44);
}

#[test]
fn pointers_arrays_and_strings() {
    let src = "import \"rt.tri\";
    fn main() -> int {
        var a: [5]int = { 10, 20, 30 };
        var p: *int = &a[1];
        *p = 21;
        p = p + 2;
        *p = 4;
        var s: *char = \"Привет\";
        var buf: [8]char = \"hi\";
        return a[0] + a[1] + a[2] + a[3] + a[4] + (p - &a[0]) * 1000 + strlen(s) * 100000 + strlen(&buf[0]) * 10000000;
    }";
    assert_eq!(exit(src), 10 + 21 + 30 + 4 + 3000 + 600_000 + 20_000_000);
}

#[test]
fn structs_and_layout() {
    let src = "struct Entry { tag: tryte; name: [4]tryte; id: int; next: *Entry; }
    var table: [3]Entry;
    fn main() -> int {
        table[0].tag = 'x';
        table[0].id = 77;
        table[1].next = &table[0];
        var e: *Entry = table[1].next;
        e.name[3] = 'q';
        var local: Entry = { 'a', \"bcd\", 5, null };
        return sizeof(Entry) * 1000000 + e.id * 1000 + (table[0].name[3] == 'q') * 100 + local.id + (local.name[1] == 'c') * 10;
    }";
    // tag, name[4] → 5 trytes → 2 slots; id slot; next slot → 12 trytes.
    assert_eq!(exit(src), 12 * 1_000_000 + 77_000 + 100 + 5 + 10);
}

#[test]
fn trytes_wrap_and_casts() {
    let src = "fn main() -> int {
        var t: tryte = 9841;
        t += 1;                            // wraps to -9841
        var big: int = 19683 + 5;
        var w: tryte = big as tryte;       // low 9 trits: 5
        var tr: trit = 7 as trit;          // lowest trit of 7 = +1
        return t * 1000 + w * 10 + tr;
    }";
    assert_eq!(exit(src), -9841 * 1000 + 50 + 1);
}

#[test]
fn globals_constants_and_initialisers() {
    let src = "const N: int = 4;
    const MSG: *char = \"ok\";
    var counter: int = 100;
    var squares: [N]int = { 0, 1, 4, 9 };
    struct P { x: int; y: int; }
    var origin: P = { 3, 4 };
    var zeroed: [10]int;
    fn bump() { counter += 1; }
    fn main() -> int {
        bump(); bump();
        return counter + squares[3] * 1000 + origin.y * 100000 + zeroed[9] + (MSG[1] == 'k') * 10000000;
    }";
    assert_eq!(exit(src), 102 + 9000 + 400_000 + 10_000_000);
}

#[test]
fn function_pointers() {
    let src = "fn twice(x: int) -> int { return 2 * x; }
    fn thrice(x: int) -> int { return 3 * x; }
    fn apply(f: fn(int) -> int, v: int) -> int { return f(v); }
    fn main() -> int {
        var ops: [2]fn(int) -> int = { twice, thrice };
        return apply(ops[0], 5) * 100 + apply(&thrice, 7);
    }";
    assert_eq!(exit(src), 1021);
}

#[test]
fn serial_console_and_stdlib() {
    let src = "import \"console.tri\";
    fn main() -> int {
        puts(\"Привет, \");
        put_int(-1234);
        putc('\\n');
        var buf: [10]char;
        puts(itoa27(9841, &buf[0]));
        puts(itoa27(-9841 + 27 * 5, &buf[0]));
        var b3: [10]char;
        puts(itoa3(-5, 4, &b3[0]));
        var c: char = getc();
        return c;
    }";
    let (o, serial) = run_with(src, "Z");
    assert_eq!(o, Outcome::Exit('Z' as i64));
    // overlined digits are negative code points in the machine, and the
    // character plus U+0305 on the host's side of the serial port
    assert_eq!(serial, "Привет, -1234\n000000DDD000000D\u{305}8\u{305}D\u{305}01\u{305}11");
}

#[test]
fn text_screen_output() {
    let src = "import \"console.tri\";
    fn main() -> int { screen_clear(); screen_puts(\"Trine on Setun-11̅\"); return screen_col; }";
    let b = build_source("t.tri", src, None, &Options::rom()).unwrap();
    let r = run_rom(&b.image, 5_000_000, "");
    assert_eq!(r.outcome, Outcome::Exit(17), "17 cells: the overlined 1 is one");
    let fb = r.machine.mem.framebuffer();
    // an overlined character is its negated code point, one cell
    let line: String = (0..17)
        .map(|i| match fb[3 * i] as i64 {
            c if c < 0 => format!("{}\u{305}", char::from_u32(-c as u32).unwrap()),
            c => char::from_u32(c as u32).unwrap().to_string(),
        })
        .collect();
    assert_eq!(line, "Trine on Setun-11\u{305}");
    assert_eq!(fb[3 * 16], -('1' as i16), "the last 1 is overlined, not 1 + a stray U+0305");
}

#[test]
fn interrupt_handler_counts_timer_ticks() {
    let src = "import \"ports.tri\";
    @align(729) var itb: [243]int;
    var ticks: int = 0;
    interrupt fn on_timer() {
        ticks += 1;
        out(TPIC_EOI, in(TPIC_CLAIM));
    }
    fn main() -> int {
        itb[121 + 1] = &on_timer as int;        // vector +1 = IRQ 0
        mtc(ITB, &itb[121] as int);
        out(TPIC_UNMASK, IRQ_TIMER);
        out(TIMER_PERIOD, 1000);
        out(TIMER_CTRL, 1);
        mtc(MODE, mfc(MODE) + 3);               // IE trit := +1
        while (ticks < 5) { hlt(); }
        return ticks;
    }";
    assert_eq!(exit(src), 5);
}

#[test]
fn faults_are_reported() {
    let (o, _) = run_with("fn main() -> int { var p: *int = 1 as *int; return *p; }", "");
    assert!(matches!(o, Outcome::Fault { vector: -5, faddr: 1, .. }), "{o:?}");
}

#[test]
fn inline_asm_and_extern() {
    let src = "extern fn triple(x: int) -> int;
    fn main() -> int {
        asm { \"        b skip_it\" \"triple: mul r1, r1, 3\" \"        ret\" \"skip_it:\" }
        return triple(14);
    }";
    assert_eq!(exit(src), 42);
}

#[test]
fn compile_errors_are_reported_with_positions() {
    let e = compile_err("fn main() -> int { return x; }");
    assert!(e.contains("test.tri:1:27: unknown name `x`"), "{e}");
    let e = compile_err("fn main() -> int {\n  var t: tryte = 0;\n  var i: int = 5;\n  t = i;\n  return 0;\n}");
    assert!(e.contains("4:7") && e.contains("use `as` to narrow"), "{e}");
    assert!(compile_err("fn main() -> int { return 1 }").contains("expected `;`"));
    assert!(compile_err("fn f() {} fn main() -> int { return f(1); }").contains("expected 0 argument"));
    assert!(compile_err("fn main() -> int { var a: int = 1; var a: int = 2; return a; }").contains("already declared"));
    assert!(compile_err("fn main() -> int { break; return 0; }").contains("outside a loop"));
    assert!(compile_err("struct S { s: S; } fn main() -> int { return 0; }").contains("contains itself"));
    assert!(compile_err("fn main() -> int { var p: *int = 5; return 0; }").contains("expected *int, found int"));
    assert!(compile_err("fn helper() {}").contains("no `fn main()`"));
    assert!(compile_err("import \"nowhere.tri\"; fn main() -> int { return 0; }").contains("cannot find"));
    assert!(compile_err("fn main() -> int { return 'ab'; }").contains("exactly one character"));
}

#[test]
fn math_library() {
    let src = r#"
        import "console.tri";
        import "math.tri";
        var fails: int;
        fn check(name: *char, cond: int) {
            if (!cond) { print("FAIL "); print(name); print("\n"); fails += 1; }
        }
        fn near(a: real, b: real) -> int { return abs(a - b) <= 1e-30 * (abs(b) > 1 ? abs(b) : 1.0); }
        fn show(x: real) { var b: [24]char; print(rtoa(x, 12, &b[0])); print(" "); }
        fn main() -> int {
            var p: real = pi();
            // identities to 30 digits
            check("exp(1)", near(exp(1.0), e()));
            check("exp(ln)", near(exp(ln(12345.678)), 12345.678));
            check("ln(exp)", near(ln(exp(-3.75)), -3.75));
            check("ln 10", near(ln(10.0), 2.302585092994045684017991454684364));
            check("exp 10", near(exp(10.0), 22026.46579480671651695790064528424));
            check("ln small", near(ln(1e-200), -460.5170185988091368035982909368728));
            check("sin pi/6", near(sin(p / 6), 0.5));
            check("cos pi/3", near(cos(p / 3), 0.5));
            check("sin2+cos2", near(sin(2.5) * sin(2.5) + cos(2.5) * cos(2.5), 1.0));
            check("sin big", near(sin(1000.0), 0.8268795405320025602558874291092192));
            check("tan pi/4", near(tan(p / 4), 1.0));
            check("atan 1", near(4 * atan(1.0), p));
            check("atan big", near(atan(1e10), p / 2 - 1e-10));
            check("asin", near(asin(0.5), p / 6));
            check("acos", near(acos(-1.0), p));
            check("pow half", near(pow(2.0, 0.5), sqrt(2.0)));
            check("pow frac", near(pow(10.0, 0.25) * pow(10.0, 0.75), 10.0));
            check("sinh", near(sinh(1.0), 1.175201193643801456882381850595601));
            check("sinh small", near(sinh(1e-5), 0.0000100000000001666666666675));
            check("tanh", near(tanh(0.5), 0.4621171572600097585023184836436725));
            check("hypot", hypot(3.0, 4.0) == 5.0);
            // exact by construction
            check("log3 81", log3(81.0) == 4.0);
            check("log3 1/27", log3(1.0 / 27) == -3.0);
            check("log10 1000", log10(1000.0) == 3.0);
            check("log10 0.001", log10(0.001) == -3.0);
            check("pow int", pow(-2.0, 3.0) == -8.0 && pow(3.0, -2.0) == 1.0 / 9);
            check("pow 0", pow(0.0, 0.0) == 1.0);
            // atan2 quadrants
            check("atan2 q2", near(atan2(1.0, -1.0), 3 * p / 4));
            check("atan2 q3", near(atan2(-1.0, -1.0), -3 * p / 4));
            check("atan2 axes", atan2(1.0, 0.0) == p / 2 && atan2(0.0, -1.0) == p && atan2(0.0, 0.0) == 0.0);
            // special values, with their flags
            set_fsr(19683 * 121);
            var n: real = ln(-1.0);
            check("ln(-1) is NaN", n != n && fsr() % 243 == 81);
            set_fsr(19683 * 121);
            check("ln(0) is -inf", fclass(ln(0.0)) == -3 && fsr() % 243 == 27);
            set_fsr(19683 * 121);
            check("exp overflows", fclass(exp(20000.0)) == 3 && fsr() % 27 == 10);
            check("exp underflows", exp(-20000.0) == 0.0);
            check("exp(-inf)", exp(-1.0 / 0.0) == 0.0);
            n = sin(1.0 / 0.0);
            check("sin(inf) is NaN", n != n);
            n = asin(2.0);
            check("asin(2) is NaN", n != n);
            n = pow(-8.0, 1.0 / 3);
            check("pow(-8, 1/3) is NaN", n != n);
            n = 0.0 / 0.0;
            check("NaN goes through", exp(n) != exp(n) && cos(n) != cos(n) && atan2(n, 1.0) != atan2(n, 1.0));
            check("hypot(inf, NaN)", fclass(hypot(-1.0 / 0.0, n)) == 3);
            check("atan2 infinities", near(atan2(1.0 / 0.0, -1.0 / 0.0), 3 * p / 4));
            show(exp(1.0)); show(ln(2.0)); show(sin(1.0)); show(atan2(-2.0, 3.0)); show(pow(2.0, 100.0)); show(cosh(-2.0));
            return fails;
        }
    "#;
    let (o, serial) = run_with(src, "");
    assert_eq!(o, Outcome::Exit(0), "{serial}");
    assert_eq!(serial, "2.71828182846 0.69314718056 0.841470984808 -0.588002603548 1.26765060023e+30 3.76219569108 ");
}

#[test]
fn binary_builtins() {
    assert_eq!(exit("fn main() -> int { return bxor(0xdeadbeef, 0x0f0f1234); }"), 0xdead_beef ^ 0x0f0f_1234);
    assert_eq!(exit("fn main() -> int { var a: int = 0xdeadbeef; return band(a, 0xff00ff00) + bor(a, 1) - bandn(a, 0xffff); }"),
        (0xdead_beef & 0xff00_ff00) + (0xdead_beef | 1) - (0xdead_beef & !0xffff));
    assert_eq!(exit("fn main() -> int { return bsh(5, 3) * 1000 + bsh(-7, -1); }"), 40_000 - 4);
    assert_eq!(exit("fn main() -> int { var a: int = 0xdeadbeef; return bext(a, 8, 8); }"), 0xbe);
    assert_eq!(exit("fn main() -> int { var a: int = 0x12345678; return brot(a, 8, 32) - 0x34567812; }"), 0);
    assert_eq!(exit("fn main() -> int { var a: int = 0xffffffff; return bext(a + 7, 0, 32); }"), 6); // mod 2^32
    assert_eq!(exit("fn main() -> int { return bxor(-1, 12345); }"), -12346);
}

#[test]
fn binary_builtins_check_their_fields() {
    assert!(compile_err("fn main() -> int { return bext(5, 0, 42); }").contains("`bext` needs a bit position in 0..80 and a length in 1..41"));
    assert!(compile_err("fn main() -> int { return brot(5, 32, 32); }").contains("`brot` needs a bit position in 0..len-1"));
    assert!(compile_err("fn main() -> int { var n: int = 3; return brot(5, n, 32); }").contains("constant"));
    assert!(compile_err("fn main() -> int { return bxor(1); }").contains("argument"));
    assert!(compile_err("fn main() -> int { var r: real = 1.0; return band(r, 1); }").contains("int"));
}

#[test]
fn unreached_functions_are_left_out() {
    // kept: what main calls, a function only passed as a pointer, one only
    // in a global initialiser, one named only in inline assembly, one with
    // inline assembly of its own (its labels are used through `extern fn`),
    // an interrupt handler and what each of them calls; left out: the rest,
    // after their bodies are still checked
    let src = "extern fn quad(x: int) -> int;
    fn used(x: int) -> int { return helper(x) + 1; }
    fn helper(x: int) -> int { return x * 2; }
    fn by_pointer(x: int) -> int { return x + 100; }
    fn in_table(x: int) -> int { return x + 1000; }
    fn in_asm_text(x: int) -> int { return x + 10000; }
    fn asm_lib() { asm { \"        b quad_end\" \"quad: mul r1, r1, 4\" \"        ret\" \"quad_end:\" } }
    interrupt fn on_irq() { never_called_from_main(); }
    fn never_called_from_main() { }
    fn unused_leaf(x: int) -> int { return x * 777; }
    fn unused_caller() -> int { return unused_leaf(3) + unused_too(); }
    fn unused_too() -> int { return 5; }
    var table: [1]fn(int) -> int = { in_table };
    fn apply(f: fn(int) -> int, v: int) -> int { return f(v); }
    fn main() -> int {
        var r: int = used(1) + apply(&by_pointer, 0) + table[0](0) + quad(1);
        asm { \"        li r1, 0\" \"        call in_asm_text\" }
        return r;
    }";
    let b = build_source("test.tri", src, None, &Options::rom()).unwrap_or_else(|e| panic!("{e}"));
    let labels: Vec<&str> = b.asm.lines().filter_map(|l| l.strip_suffix(':')).collect();
    for f in ["main", "used", "helper", "by_pointer", "in_table", "in_asm_text", "asm_lib", "on_irq", "never_called_from_main", "apply"] {
        assert!(labels.contains(&f), "{f} was left out");
    }
    for f in ["unused_leaf", "unused_caller", "unused_too"] {
        assert!(!labels.contains(&f), "{f} was kept");
    }
    assert_eq!(exit(src), 3 + 100 + 1000 + 4);
    // unoptimized output keeps them all (it is what tric matches)
    let plain = build_source("test.tri", src, None, &Options { optimize: false, ..Options::rom() }).unwrap();
    assert!(plain.asm.lines().any(|l| l == "unused_leaf:"), "unoptimized output keeps every function");
    // a function nothing calls is still checked
    let e = compile_err("fn unused() -> int { return nope; }\nfn main() -> int { return 0; }");
    assert!(e.contains("unknown name `nope`"), "{e}");
}

#[test]
fn kleene_truth_values() {
    // true +1, unknown 0, false -1; & and | are min and max, ~ negates:
    // Kleene's tables, checked at run time on variables (no folding)
    let src = "fn and3(a: trit, b: trit) -> trit { return a & b; }
    fn or3(a: trit, b: trit) -> trit { return a | b; }
    fn not3(a: trit) -> trit { return ~a; }
    fn main() -> int {
        var v: [3]trit = { false, unknown, true };
        var r: int = 0;
        var i: int = 0;
        while (i < 3) {
            var j: int = 0;
            while (j < 3) {
                var want_and: trit = v[i < j ? i : j];
                var want_or: trit = v[i > j ? i : j];
                if (and3(v[i], v[j]) != want_and) { r = r + 1; }
                if (or3(v[i], v[j]) != want_or) { r = r + 10; }
                j += 1;
            }
            if (not3(v[i]) != v[2 - i]) { r = r + 100; }
            i += 1;
        }
        var t: trit = ~true;
        if (t != false) { r = r + 1000; }
        if ((false as int) != -1 || (unknown as int) != 0 || (true as int) != 1) { r = r + 10000; }
        // binary truth: b_true and b_false are ints; any non-zero value holds
        var n: int = 0;
        while (b_true) { n += 1; if (n == 3) { break; } }
        if (b_false || !b_true || n != 3) { r = r + 100000; }
        // a trit tested on purpose: as int, compared, or three ways
        var k: trit = false;
        if (k as int) { r = r + 0; } else { r = r + 1000000; }
        if (k == true) { r = r + 1000000; }
        sign (k) { neg: { r = r + 0; } zero: { r = r + 1000000; } pos: { r = r + 1000000; } }
        return r;
    }";
    assert_eq!(exit(src), 0);
}

#[test]
fn kleene_truth_values_refused() {
    let cases: [(&str, &str); 11] = [
        ("fn main() -> int { if (true) { return 1; } return 0; }", "a trit is not a condition"),
        ("fn main() -> int { while (false) { } return 0; }", "a trit is not a condition"),
        ("fn main() -> int { var t: trit = unknown; if (t) { return 1; } return 0; }", "sign (t)"),
        ("fn main() -> int { var t: trit = 1; return !t; }", "a trit is not a condition"),
        ("fn main() -> int { var t: trit = 1; return t && 1; }", "a trit is not a condition"),
        ("fn main() -> int { var t: trit = 1; return t ? 2 : 3; }", "a trit is not a condition"),
        ("fn main() -> int { var a: int = 1; if (a <=> 2) { return 1; } return 0; }", "use `a != b`"),
        ("fn main() -> int { var done: int = false; return done; }", "binary truth is `b_true`/`b_false`"),
        ("fn f() -> int { return true; } fn main() -> int { return f(); }", "binary truth is `b_true`/`b_false`"),
        ("fn f(x: int) -> int { return x; } fn main() -> int { return f(unknown); }", "found a truth value"),
        ("fn main() -> int { var unknown: int = 1; return unknown; }", ""),
    ];
    for (src, want) in cases {
        let e = compile_err(src);
        assert!(e.contains(want), "{src}\n  gave: {e}");
    }
}

/// MRG, TCP, TCN, TREV and STWM as builtins (TERNARY_WORLD A2, A3).
const IDIOMS: &str = "var page: [4]int;
fn main() -> int {
    var old: int = 2222333444555;
    var mask: int = -1000000007;
    var src: int = 3141592653589;
    var m: int = mrg(old, mask, src);
    var z: int = mrg(old, 0, src);                 // a zero mask keeps old
    var c: int = tcp(src, 0) * 1000000 + tcn(src, 9) * 1000 + tcp(-1, 1);
    var r: int = trev(src, 0) % 1000003 + trev(mask, 13) % 1009;
    page[1] = 5555;
    stwm(&page[1], 777, 1);                        // trit 27: take value's
    stwm(&page[1], 0, -9);                         // the low trits cleared where the mask is -1
    return 0;
}";

#[test]
fn ternary_idiom_builtins() {
    let b = build_source("test.tri", IDIOMS, None, &Options::rom()).unwrap_or_else(|e| panic!("{e}"));
    let r = run_rom(&b.image, 20_000_000, "");
    assert!(matches!(r.outcome, Outcome::Exit(0)), "{:?}", r.outcome);
    // the locals are gone by now: recompute and compare through a second
    // program that returns each value in turn
    let w = |v: i64| ternary::Word::new(v).unwrap();
    let (old, mask, src) = (w(2222333444555), w(-1000000007), w(3141592653589));
    let cases: [(&str, i64); 6] = [
        ("mrg(old, mask, src)", old.tritwise_merge(mask, src).value()),
        ("mrg(old, 0, src)", old.value()),
        ("tcp(src, 0)", src.count_pos_low(27) as i64),
        ("tcn(src, 9)", src.count_neg_low(9) as i64),
        ("trev(mask, 13)", mask.reverse_low(13).value()),
        ("mrg(page_after(), 0, 0)", w(5555).tritwise_merge(w(1), w(777)).tritwise_merge(w(-9), w(0)).value()),
    ];
    for (e, want) in cases {
        let src_text = format!(
            "var page: [4]int;
            fn page_after() -> int {{ page[1] = 5555; stwm(&page[1], 777, 1); stwm(&page[1], 0, -9); return page[1]; }}
            fn main() -> int {{
                var old: int = 2222333444555; var mask: int = -1000000007; var src: int = 3141592653589;
                var v: int = {e};
                return v % 9841;
            }}"
        );
        assert_eq!(exit(&src_text), want % 9841, "{e}");
    }
    // refused
    assert!(compile_err("fn main() -> int { var x: int = 1; stwm(x, 1, 1); return 0; }").contains("`stwm` needs a pointer to an int"));
    assert!(compile_err("fn main() -> int { return mrg(1, 2); }").contains("argument"));
}

#[test]
fn cas3_and_swap_builtins() {
    // cas3 swaps only when *p is the expected value and always returns the
    // old one; swap exchanges
    let src = "var lock: int;
    fn main() -> int {
        var r: int = 0;
        lock = 0;
        if (cas3(&lock, 0, 1) != 0 || lock != 1) { r += 1; }       // free: taken
        if (cas3(&lock, 0, 1) != 1 || lock != 1) { r += 10; }      // held: not taken
        if (swap(&lock, -1) != 1 || lock != -1) { r += 100; }      // held, with waiters
        if (cas3(&lock, 5, 7) != -1 || lock != -1) { r += 1000; }  // larger expected: no
        if (swap(&lock, 0) != -1 || lock != 0) { r += 10000; }     // released
        return r;
    }";
    assert_eq!(exit(src), 0);
    assert!(compile_err("fn main() -> int { var x: int = 1; return cas3(x, 0, 1); }").contains("`cas3` needs a pointer to an int"));
    assert!(compile_err("fn main() -> int { var x: tryte = 1; return swap(&x, 0); }").contains("`swap` needs a pointer to an int"));
}

/// Vectors (ISA §15) from Trine: every builtin on tryte and word lanes,
/// checked against the same thing done lane by lane.
const VECTORS: &str = "@align(9) var a: [9]tryte = { 1, -2, 3, 9841, -9841, 0, 100, -100, 13 };
@align(9) var b: [9]tryte = { 5, 5, -5, 1, -1, 7, -300, 4, 0t1T1 };
@align(9) var d: [9]tryte;
@align(9) var wa: [3]int = { 123456789, -5, 3812798742000 };
@align(9) var wb: [3]int = { -1, 99999, 3 };
@align(9) var wd: [3]int;

fn lane(f: int, x: int, y: int) -> int {
    if (f == 0) { return (x + y) as tryte; }
    if (f == 1) { return (x - y) as tryte; }
    if (f == 2) { return x & y; }
    if (f == 3) { return x | y; }
    if (f == 4) { return tmul(x, y); }
    if (f == 5) { return tsat(x, y); }
    if (f == 6) { return cons(x, y); }
    if (f == 7) { return -x; }
    if (f == 8) { return abs(x); }
    return sgn(x);
}

fn run(f: int) {
    if (f == 0) { vadd(d, a, b); vaddw(wd, wa, wb); }
    if (f == 1) { vsub(d, a, b); vsubw(wd, wa, wb); }
    if (f == 2) { vmin(d, a, b); vminw(wd, wa, wb); }
    if (f == 3) { vmax(d, a, b); vmaxw(wd, wa, wb); }
    if (f == 4) { vtmul(d, a, b); vtmulw(wd, wa, wb); }
    if (f == 5) { vtsat(d, a, b); vtsatw(wd, wa, wb); }
    if (f == 6) { vcons(d, a, b); vconsw(wd, wa, wb); }
    if (f == 7) { vneg(d, a); vnegw(wd, wa); }
    if (f == 8) { vabs(d, a); vabsw(wd, wa); }
    if (f == 9) { vsgn(d, a); vsgnw(wd, wa); }
}

fn main() -> int {
    var r: int = 0;
    var f: int = 0;
    while (f < 10) {
        {
            run(f);
            var i: int = 0;
            while (i < 9) { if (d[i] != lane(f, a[i], b[i])) { r += 1; } i += 1; }
            i = 0;
            while (i < 3) {
                var want: int = f == 0 ? wa[i] + wb[i] : f == 1 ? wa[i] - wb[i] : f == 7 ? -wa[i] : f == 8 ? abs(wa[i]) : f == 9 ? sgn(wa[i])
                    : f == 2 ? wa[i] & wb[i] : f == 3 ? wa[i] | wb[i] : f == 4 ? tmul(wa[i], wb[i]) : f == 5 ? tsat(wa[i], wb[i]) : cons(wa[i], wb[i]);
                if (wd[i] != want) { r += 10; }
                i += 1;
            }
        }
        f += 1;
    }
    // reductions: the lanes' sum, and the dot product with one weight trit per lane
    var s: int = 0;
    var dot: int = 0;
    var j: int = 0;
    while (j < 9) { s += a[j]; dot += a[j] * trit(b[j], 27); j += 1; }
    if (vsum(a) != s) { r += 100; }
    if (vdotw(a, b) != dot) { r += 1000; }
    // fills, and a merge: the mask's +1 lanes take the source, -1 clear, 0 keep
    vbcst(d, -77);
    j = 0;
    while (j < 9) { if (d[j] != -77) { r += 10000; } j += 1; }
    vbcstw(wd, 5);
    if (wd[0] != 5 || wd[1] != 5 || wd[2] != 5) { r += 100000; }
    vbcst(wd, 0t1T0);
    vmrg(d, wd, a);
    j = 0;
    while (j < 9) { if (d[j] != mrg(-77, 0t1T0, a[j])) { r += 1000000; } j += 1; }
    return r;
}
";

#[test]
fn vector_builtins() {
    assert_eq!(exit(VECTORS), 0);
    let o = |src: &str| {
        let b = build_source("test.tri", src, None, &Options { optimize: false, ..Options::rom() }).unwrap();
        run_rom(&b.image, 20_000_000, "").outcome
    };
    assert!(matches!(o(VECTORS), Outcome::Exit(0)), "unoptimized");
    // a vector off a multiple of 9 traps -5 at its address
    let src = "@align(9) var pad: [3]int; var v: [3]int; fn main() -> int { vbcstw(&v[0], 1); return 0; }";
    let src2 = "@align(9) var pad: [4]int; var v: [3]int; fn main() -> int { vbcstw(&v[0], 1); return 0; }";
    assert!(matches!(o(src), Outcome::Exit(0)), "pad of 3 words lands v on a multiple of 9: {:?}", o(src));
    assert!(matches!(o(src2), Outcome::Fault { vector: -5, .. }), "{:?}", o(src2));
    for (src, want) in [
        ("fn main() -> int { var x: int; vadd(x, x, x); return 0; }", "`vadd` needs a pointer to a vector"),
        ("fn main() -> int { var x: [3]int; vneg(x, 5); return 0; }", "`vneg` needs a pointer to a vector"),
        ("fn main() -> int { var x: [3]int; return vsum(x, x); }", "argument"),
        ("fn main() -> int { var x: [3]int; vadd(x, x); return 0; }", "argument"),
        ("fn main() -> int { var x: [3]int; var p: *[3]int = &x; vbcst(x, p); return 0; }", "integer"),
        ("fn main() -> int { var x: [3]trit; vabs(x, x); return 0; }", "`vabs` needs a pointer to a vector"),
    ] {
        let e = compile_err(src);
        assert!(e.contains(want), "{src}: {e}");
    }
}

#[test]
fn characters_above_one_tryte() {
    // A6: a character above U+2671 is 3 trytes in a string; a character
    // literal cannot hold one
    let src = "fn main() -> int { var s: *char = \"a日😀\"; var n: int = 0; while (s[n] != 0) { n += 1; } return n * 10 + (s[1] == -2) + (s[4] == -2); }";
    assert_eq!(exit(src), 72);
    assert!(compile_err("fn main() -> int { return '日'; }").contains("takes three trytes"));
    assert!(compile_err("fn main() -> int { return '日\u{305}'; }").contains("takes three trytes"));
    assert!(compile_err("fn main() -> int { return 'ab'; }").contains("exactly one character"));
}
