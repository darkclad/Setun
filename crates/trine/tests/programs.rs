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
    // 0t+0- & 0t++0 = min per trit = 0t+0-   ;  | = max = 0t++0
    assert_eq!(exit("fn main() -> int { var a: int = 0t+0-; return a & 0t-++; }"), {
        // min(+,-)=-, min(0,+)=0, min(-,+)=- → 0t-0-
        -9 - 1
    });
    assert_eq!(exit("fn main() -> int { var a: int = 0t+0-; return a | 0t-++; }"), 9 + 3 + 1);
    assert_eq!(exit("fn main() -> int { return tmul(0t+-0, 0t--+); }"), -9 + 3);
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
        var c: char = getc();
        return c;
    }";
    let (o, serial) = run_with(src, "Z");
    assert_eq!(o, Outcome::Exit('Z' as i64));
    assert_eq!(serial, "Привет, -1234\n000000MMM");
}

#[test]
fn text_screen_output() {
    let src = "import \"console.tri\";
    fn main() -> int { screen_clear(); screen_puts(\"Trine on Setun-386\"); return screen_col; }";
    let b = build_source("t.tri", src, None, &Options::rom()).unwrap();
    let r = run_rom(&b.image, 5_000_000, "");
    assert_eq!(r.outcome, Outcome::Exit(18));
    let fb = r.machine.mem.framebuffer();
    let line: String = (0..18).map(|i| char::from_u32(fb[3 * i] as u32).unwrap()).collect();
    assert_eq!(line, "Trine on Setun-386");
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
