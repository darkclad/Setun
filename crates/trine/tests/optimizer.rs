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

#[test]
fn arrays_and_fields_the_same_way() {
    let src = r#"
        import "console.tri";
        struct P { tag: tryte; x: int; name: [5]char; y: int; r: real; }
        var garr: [20]int;
        var gch: [12]char;
        var gp: P;
        var greal: [4]real;
        fn put(v: int) { var b: [28]char; itoa(v, &b[0]); print(&b[0]); print(" "); }
        fn fill(p: *P, v: int) { p.x = v; p.y = v * 2; p.tag = (v % 9) as tryte; p.name[1] = 'q'; }
        fn main() -> int {
            var larr: [15]int;
            var lch: [9]char;
            var lp: P;
            var i: int = 0;
            while (i < 15) { larr[i] = i * 7 - 20; garr[i] = larr[i] + larr[14 - i]; i += 1; }
            var sum: int = 0;
            var j: int = 0;
            while (j < 15) { sum = sum + garr[j] * 3 - larr[j]; j += 1; }
            put(sum);
            // tryte arrays, constant and computed indexes
            var k: int = 0;
            while (k < 9) { lch[k] = ('a' + k) as char; gch[k + 2] = lch[k]; k += 1; }
            gch[0] = 'X';
            put(gch[0] + gch[5] + lch[8]);
            // a pointer in a register: indexing, negative offsets, deref
            var pt: *int = &garr[10];
            put(pt[-3] + pt[2] + *pt);
            *pt = 999;
            pt[1] = pt[-1] + 1;
            put(garr[10] + garr[11]);
            // struct fields: global, local, through a pointer
            fill(&gp, 7);
            fill(&lp, 11);
            var q: *P = &lp;
            q.x = q.x + gp.y;
            put(lp.x + lp.y + gp.tag + q.tag + gp.name[1]);
            gp.r = 2.5;
            greal[2] = gp.r * 2.0;
            put((greal[2] + gp.r) as int);
            return sum % 9841;
        }
    "#;
    same_both_ways_src("arrays.tri", src, 50_000_000);
}

#[test]
fn inlining_keeps_meaning() {
    let src = r#"
        import "console.tri";
        var g: int = 1000;
        var order: int;
        fn put(v: int) { var b: [28]char; itoa(v, &b[0]); print(&b[0]); print(" "); }
        fn uses_g(x: int) -> int { return x + g; }               // sees the global g
        fn first_odd(a: int, b: int) -> int {                    // early returns, loops
            var i: int = a;
            while (i < b) {
                if (i % 2 == 0) { i += 1; continue; }
                if (i > 50) { break; }
                return i;
            }
            return -1;
        }
        fn via_ptr(x: int) -> int { var t: int = x; var p: *int = &t; *p = *p * 3; return t; }
        fn sq(x: int) -> int { return x * x; }
        fn sum_sq(a: int, b: int) -> int { return sq(a) + sq(b); }      // nested
        fn fact(n: int) -> int { if (n <= 1) { return 1; } return n * fact(n - 1); }
        fn tick(v: int) -> int { order = order * 10 + v; return v; }
        fn three(a: int, b: int, c: int) -> int { return a * 100 + b * 10 + c; }
        fn main() -> int {
            var total: int = 0;
            var k: int = 0;
            while (k < 6) {
                var g: int = 5;                                  // shadows the global
                total = total + uses_g(k) + g;
                total = total + first_odd(k * 9, 60) + via_ptr(k) + sum_sq(k, k + 1);
                k += 1;
            }
            put(total);
            put(fact(7));
            var r: int = 0;
            var j: int = 0;
            while (j < 2) { r = r + three(tick(1), tick(2), tick(3)); j += 1; }
            put(r);
            put(order);
            return total % 1000;
        }
    "#;
    same_both_ways_src("inline.tri", src, 50_000_000);
}

#[test]
fn builds_are_reproducible() {
    // inlining compiles the callee's body at every call; nothing it caches
    // may depend on hash order or on where a copy happens to live
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for f in ["user/bin/cat.tri", "user/bin/sort.tri", "user/bin/login.tri"] {
        let p = root.join(f);
        let first = build_file(&p, &Options::user()).unwrap_or_else(|e| panic!("{e}")).asm;
        for _ in 0..4 {
            let again = build_file(&p, &Options::user()).unwrap_or_else(|e| panic!("{e}")).asm;
            assert!(again == first, "{f}: two builds differ");
        }
    }
}

#[test]
fn indexes_with_constant_offsets_the_same_way() {
    // a[i + k], a[k + i], a[i - k] and nested ones fold k into the
    // displacement: loads and stores, word, tryte and real elements,
    // global, local and pointer bases; the values all differ, and the sums
    // weigh by position, so any element off by one shows
    let src = r#"
        import "console.tri";
        var gw: [40]int;
        var gc: [40]char;
        var gr: [12]real;
        fn put(v: int) { var b: [28]char; itoa(v, &b[0]); print(&b[0]); print(" "); }
        fn main() -> int {
            var lw: [40]int;
            var lc: [40]char;
            var i: int = 0;
            while (i < 40) { gw[i] = i * i - 37 * i; lw[i] = 3 * i + 1000; gc[i] = (i * 7 % 26 + 'a') as char; lc[i] = (i % 5 + 'A') as char; i += 1; }
            var s: int = 0;
            i = 5;
            while (i < 30) {
                s = s * 3 + gw[i + 1] - gw[i - 1] + gw[2 + i] + gw[i + 1 + 2] - gw[i - 3 + 1];
                s = s % 1000003 + lw[i + 4] - lw[i - 5] + gc[i + 3] - gc[i - 2] + lc[i + 1];
                lw[i + 1] = gw[i - 1] + i;                  // stores with offsets
                gw[i - 2] = lw[i + 1] * 2;
                lc[i + 2] = gc[i - 1];
                gc[i + 1] = (gc[i + 1] + 1) as char;
                i += 1;
            }
            put(s);
            // a pointer in a register, with offsets either way
            var p: *int = &gw[20];
            var j: int = 0;
            var t: int = 0;
            while (j < 8) { t = t * 3 + p[j + 1] - p[j - 4] + p[3 - j]; p[j - 2] = t % 977; j += 1; }
            put(t % 1000003);
            // reals
            var k: int = 0;
            while (k < 12) { gr[k] = k as real * 1.5; k += 1; }
            k = 1;
            var r: real = 0.0;
            while (k < 10) { r = r * 2.0 + gr[k + 1] - gr[k - 1]; gr[k + 2] = r; k += 1; }
            put(r as int);
            var w: int = 0;
            i = 0;
            while (i < 40) { w = (w * 3 + gw[i] + lw[i] + gc[i] + lc[i]) % 1000003; i += 1; }
            put(w);
            return w % 9841;
        }
    "#;
    same_both_ways_src("offsets.tri", src, 80_000_000);
}

#[test]
fn loops_and_jumps_the_same_way() {
    // rotated loops (the test at the bottom) and `if (c) break/continue`
    // as one jump: loops that never run, continue in a for (the step still
    // runs), break/continue with an else, break inside a switch inside a
    // loop, nested loops, a for without a test, a test that reads memory
    let src = r#"
        import "console.tri";
        var g: int = 3;
        fn put(v: int) { var b: [28]char; itoa(v, &b[0]); print(&b[0]); print(" "); }
        fn main() -> int {
            var s: int = 0;
            var i: int = 0;
            while (i < 0) { s += 1000; }                      // never runs
            for (i = 5; i < 5; i += 1) { s += 1000; }         // never runs
            i = 0;
            while (i < 30) {
                i += 1;
                if (i % 3 == 0) continue;
                if (i > 25) break;
                s = s * 3 + i;
            }
            put(s % 1000003);
            // continue in the round where the test must end the loop
            var c: int = 0;
            var cs: int = 0;
            while (c < 10) { c += 1; if (c % 2 == 0) continue; cs += c; }
            put(cs);
            var t: int = 0;
            for (var j: int = 0; j < 40; j += 2) {
                if (j % 6 == 0) { continue; }
                if (j == 34) { break; } else { t = t * 3 + j; }
                t += 1;
            }
            put(t % 1000003);
            var u: int = 0;
            var k: int = 0;
            while (k != 20) {
                k += 1;
                switch (k % 4) {
                    case 0: u += 1; break;
                    case 1: if (k > 10) break; u += 10;
                    default: u += 100;
                }
                if (k == 17) continue; else u = u * 2 % 999983;
            }
            put(u);
            var n: int = 0;
            var a: int = 0;
            while (a < 6) {
                var b: int = a;
                while (b > 0) { if (b == 2) break; n = n * 3 + b; b -= 1; }
                a += 1;
            }
            put(n % 1000003);
            var m: int = 0;
            for (;;) { m += 1; if (m == 7) break; }
            put(m);
            while (g < 50) { g = g * 2 + 1; }                // the test reads memory
            put(g);
            return (s + cs + t + u + n + m + g) % 9841;
        }
    "#;
    same_both_ways_src("loops.tri", src, 50_000_000);
}
