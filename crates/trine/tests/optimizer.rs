//! The optimizer must not change what a program does: each program here is
//! compiled with and without optimization, and both runs must end the same
//! way with the same serial output, debug log and framebuffer.

use std::path::Path;
use trine::{build_file, build_source, run_rom, Options, Outcome};

fn opts(optimize: bool) -> Options {
    Options { optimize, ..Options::rom() }
}

/// (outcome, serial, debug log, framebuffer hash, steps)
fn run(image: &[i16], steps: u64) -> (Outcome, String, Vec<i64>, u64, u64) {
    let r = run_rom(image, steps, "");
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &t in r.machine.mem.framebuffer() {
        h = (h ^ t as u16 as u64).wrapping_mul(0x100_0000_01b3);
    }
    (r.outcome, r.serial, r.machine.dev.debug_log.clone(), h, r.steps)
}

fn same_both_ways_src(name: &str, src: &str, steps: u64) -> Outcome {
    let a = build_source(name, src, None, &opts(false)).unwrap_or_else(|e| panic!("{name}: {e}"));
    let b = build_source(name, src, None, &opts(true)).unwrap_or_else(|e| panic!("{name}: {e}"));
    let (ra, rb) = (run(&a.image, steps), run(&b.image, steps));
    assert_eq!((&ra.0, &ra.1, &ra.2, ra.3), (&rb.0, &rb.1, &rb.2, rb.3), "{name}: optimized run differs");
    assert!(rb.4 <= ra.4, "{name}: the optimized program took more steps ({} vs {})", rb.4, ra.4);
    rb.0
}

fn same_both_ways_file(path: &Path, steps: u64) {
    let a = build_file(path, &opts(false)).unwrap_or_else(|e| panic!("{e}"));
    let b = build_file(path, &opts(true)).unwrap_or_else(|e| panic!("{e}"));
    let (ra, rb) = (run(&a.image, steps), run(&b.image, steps));
    assert_eq!((&ra.0, &ra.1, &ra.2, ra.3), (&rb.0, &rb.1, &rb.2, rb.3), "{}: optimized run differs", path.display());
}

#[test]
fn example_and_benchmark_programs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    same_both_ways_file(&root.join("examples/hello.tri"), 1_000_000);
    same_both_ways_file(&root.join("crates/bench/programs/int.tri"), 200_000_000);
    same_both_ways_file(&root.join("crates/bench/programs/fpu.tri"), 50_000_000);
    same_both_ways_file(&root.join("examples/mandel.tri"), 400_000_000);
}

#[test]
fn register_allocation_torture() {
    let src = r#"
        import "console.tri";
        var g: int = 5;
        var tbl: [10]int;

        fn put(v: int) { var b: [28]char; itoa(v, &b[0]); print(&b[0]); print(" "); }

        // more parameters than registers for arguments, and recursion
        fn many(a: int, b: int, c: int, d: int, e: int, f: int, h: int, i: int) -> int {
            if (a <= 0) { return b + c + d + e + f + h + i; }
            var keep: int = a * 1000 + i;                 // must survive the call
            var r: int = many(a - 1, b + 1, c, d, e, f, h, i + 2);
            return keep - r;
        }

        fn twice(x: int) -> int { return x + x; }

        fn main() -> int {
            // twelve locals: some spill to memory
            var l1: int = 1; var l2: int = 2; var l3: int = 3; var l4: int = 4;
            var l5: int = 5; var l6: int = 6; var l7: int = 7; var l8: int = 8;
            var l9: int = 9; var l10: int = 10; var l11: int = 11; var l12: int = 12;
            put(l1 + l2 * l3 - l4 / l2 + l5 % l3 + (l6 | l7) + (l8 & l9) + l10 * l11 - l12);
            // address taken: stays in memory and sees writes through the pointer
            var a: int = 40;
            var pa: *int = &a;
            *pa = *pa + 2;
            put(a);
            // shadowing, and registers reused by a later scope
            var s: int = 100;
            {
                var s: int = 7;
                var t: int = s * 3;
                put(s + t);
            }
            { var u: int = 9; put(s + u); }
            // compound assignments in every flavour
            var x: int = 1000;
            x += 7; x -= 3; x *= 3; x /= 2; x %= 997; x <<= 2; x >>= 1; x |= 0t1T1; x &= 0t11111111;
            put(x);
            // a pointer in a register: scaled arithmetic and compound assignment
            var i: int = 0;
            while (i < 10) { tbl[i] = i * i; i += 1; }
            var p: *int = &tbl[0];
            p += 3;
            put(*p);
            p = p + 2;
            put(*p + p[1]);
            var q: *int = &tbl[9];
            put(q - p);
            // calls keep register locals (callee-saved)
            var acc: int = 0;
            var k: int = 0;
            while (k < 5) { acc = acc + twice(k) + g; k += 1; }
            put(acc);
            put(many(3, 1, 2, 3, 4, 5, 6, 7));
            // a function pointer held in a register
            var fp: fn(int) -> int = &twice;
            put(fp(21));
            // comparisons with register operands on both sides
            var lo: int = -5;
            var hi: int = 5;
            var n: int = 0;
            if (lo < hi) { n += 1; }
            if (hi > lo && lo != hi) { n += 10; }
            if (lo >= hi || lo == lo) { n += 100; }
            put(n);
            return l12 + acc;
        }
    "#;
    let out = same_both_ways_src("torture.tri", src, 50_000_000);
    assert_eq!(out, Outcome::Exit(12 + 45), "the program ran to the end");
}

#[test]
fn conditions_short_circuit_the_same_way() {
    let src = r#"
        import "console.tri";
        var calls: int;
        fn t(v: int) -> int { calls = calls * 3 + 1; return v; }   // records the order
        fn put(v: int) { var b: [28]char; itoa(v, &b[0]); print(&b[0]); print(" "); }
        fn main() -> int {
            var hits: int = 0;
            var i: int = -3;
            while (i <= 3) {
                if (t(i) > 0 && t(i * 2) < 5 || !t(i + 3)) { hits += 1; }
                if (!(i < 0 || i == 2) && (t(i) != 1 || i > 2)) { hits += 10; }
                if (i != 0 && !(i > 1 && i < 3)) { hits += 100; }
                i += 1;
            }
            put(hits);
            put(calls);
            var n: int = 0;
            var k: int = 0;
            while (k < 20 && (n < 7 || k % 2 == 0) && !(k == 13)) { n += 1; k += 1; }
            put(n);
            // reals: a NaN makes every comparison false except !=
            var z: real = 0.0;
            var nan: real = z / z;
            var one: real = 1.0;
            var r: int = 0;
            if (nan < one || nan > one || nan == one) { r += 1; }
            if (nan != one && !(nan == nan)) { r += 10; }
            if (!(nan < one) && one < 2.0) { r += 100; }
            if (one == one && (nan >= one || one > 0.5)) { r += 1000; }
            put(r);
            return hits % 1000;
        }
    "#;
    same_both_ways_src("conditions.tri", src, 50_000_000);
}
