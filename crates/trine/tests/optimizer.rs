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

#[test]
fn calls_compound_assignments_and_operand_order_the_same_way() {
    // arguments in registers (simple ones after the last call, which may
    // change what earlier ones read), compound assignment in place, and a
    // simple left operand read after the right side only when that cannot
    // change it
    let src = r#"
        import "console.tri";
        var x: int = 10;
        var y: int = 20;
        var gc: char = 'A';
        var arr: [12]int;
        var tr: [6]tryte;
        struct S { a: int; t: tryte; b: int; }
        var gs: S;
        fn put(v: int) { var b: [28]char; itoa(v, &b[0]); print(&b[0]); print(" "); }
        fn bump() -> int { x = x * 2 + 1; y += 3; return 7; }
        fn six(a: int, b: int, c: int, d: int, e: int, f: int) -> int { return ((((a * 3 + b) * 3 + c) * 3 + d) * 3 + e) * 3 + f; }
        fn two(a: int, b: int) -> int { return a * 1000 + b; }
        fn ch(c: char, n: int) -> int { return c * 10 + n; }
        fn main() -> int {
            var r: int = 4;
            var mem: int = 9;
            var pm: *int = &mem;                              // mem stays in memory
            put(two(x, bump()));                              // x read before bump changes it
            put(two(bump(), x));                              // x read after
            put(six(x, y, bump(), x, y, r));                  // simple ones on both sides of a call
            put(six(mem, x, y, mem, x, y));                   // several from memory in a row
            put(six(1, two(x, y), 3, r, two(y, x), mem));      // calls inside, nested
            put(ch(gc, x));                                   // a char from memory
            // compound assignment in place
            x += 5; y -= x; mem *= 3; *pm += 2; x /= 3; y %= 7;
            var i: int = 0;
            while (i < 12) { arr[i] = i * i; i += 1; }
            i = 2;
            arr[i] += 100; arr[i + 3] -= y; arr[7] *= r; arr[i * 2] %= 5;
            gs.a += 9; gs.b -= gs.a; gs.t += 9000; gs.t += 9000; // tryte overflow wraps
            tr[1] = 9800 as tryte; tr[1] += 100; tr[2] -= 9841;
            var p: *int = &arr[0];
            p += 3; p -= 1;                                    // pointer steps scale
            put(x + y * 3 + mem + arr[2] + arr[5] + arr[7] + arr[4] + gs.a + gs.b + gs.t + tr[1] + tr[2] + *p);
            // the left operand after the right side, only when safe
            put(x + bump());                                  // bump changes x: x first
            put(100 - bump());
            put(r + (r = 5));                                 // the right side assigns r
            put(mem + y * 2 + r);
            return (x + y + r + mem) % 9841;
        }
    "#;
    same_both_ways_src("calls.tri", src, 50_000_000);
}

#[test]
fn temporaries_in_registers_the_same_way() {
    // integer and real temporaries held in registers (r4-r6, f3-f13)
    // instead of the stack: nested deeper than there are registers, with a
    // call on the right (which must not find them changed), real stores to
    // every kind of place, a simple real variable on the right, comparisons
    let src = r#"
        import "console.tri";
        var gx: int = 7;
        var gr: real = 1.5;
        var ra: [5]real;
        struct P { a: real; n: int; b: real; }
        var gp: P;
        fn put(v: int) { var b: [28]char; itoa(v, &b[0]); print(&b[0]); print(" "); }
        // f and g use r4..r6 and f3.. themselves (six arguments, temporaries)
        fn six(a: int, b: int, c: int, d: int, e: int, h: int) -> int { return a + b * 2 + c * 3 + d * 4 + e * 5 + h * 6; }
        fn f(v: int) -> int { gx += v; return six(v, v + 1, v * 2, v - 3, gx, 7) % 97 + v * 2 + 1; }
        fn g(v: real) -> real { gr = gr + v * (v - (v * 2.0 - (v + 1.0))); return v * 0.5 + six(1, 2, 3, 4, 5, 6); }
        fn main() -> int {
            var a: int = 3;
            var b: int = 5;
            var m: int = 11;
            var pm: *int = &m;
            // deeper than r4..r6: the stack again from the fourth level
            put((a + b) * ((b - a) * ((a * b + 1) * ((b + 2) * (a - 7) + (m - b)) - (a + m)) + (b * m - a)) - (m + a) * (b - m));
            put((a + b) * f(a) + (b - a) * f(b) - gx);         // calls on the right
            put((a * 2 + gx) - (f(m) + gx));                   // gx changed by f, read in order
            if ((a + b) * 2 > (m - a) * (b - 1)) { put(1); } else { put(2); }
            if ((a * b) == (b * a + m - m)) { put(3); }
            var x: real = 2.0;
            var y: real = 0.75;
            var z: real = (x + y) * ((x - y) * ((x * y + 1.0) * ((y + 2.0) * (x - 7.0) + (x * y - y)) - (x + y)) + (y * x - x));
            put((z * 1000.0) as int);
            var w: real = 0.0;
            w = (x * y + x / y) - (y * y - x * x) * DEEP;
            put((w * 1000.0) as int);
            var u: real = x * g(y) + (y - g(x)) * gr;          // calls on the right
            put((u * 1000.0) as int);
            ra[2] = x * y; gp.a = x - y; gp.b = gp.a * x + y; gr = gr * x;
            ra[3] = ra[2] / y + gp.b;
            put(((ra[2] + ra[3] + gp.a + gp.b + gr) * 100.0) as int);
            if (x * y > y * y + x) { put(4); } else { put(5); }
            if (x - y < gr) { put(6); }
            put(((x * y + y * gr) * 1000.0) as int);
            return (a + b + m) % 9841;
        }
    "#;
    // right-nested 14 deep: more than the eleven f registers
    let mut deep = String::from("(x - y)");
    for k in 0..14 {
        deep = format!("(x {} y * {deep})", if k % 2 == 0 { "+" } else { "-" });
    }
    same_both_ways_src("temps.tri", &src.replace("DEEP", &deep), 50_000_000);
}

#[test]
fn loads_with_any_index_the_same_way() {
    // a load's index may be any integer expression (evaluated into r1):
    // products, nested indexing, calls that change the array or the
    // index's variables, every element type and base, inside expressions
    // whose temporaries sit in registers
    let src = r#"
        import "console.tri";
        var ga: [60]int;
        var gc: [60]char;
        var gr: [20]real;
        var gi: int = 2;
        fn put(v: int) { var b: [28]char; itoa(v, &b[0]); print(&b[0]); print(" "); }
        fn bump(k: int) -> int { gi += 1; ga[k] = ga[k] * 3 + 1; return k + 1; }
        fn main() -> int {
            var la: [30]int;
            var i: int = 0;
            while (i < 60) { ga[i] = i * i - 17 * i; gc[i] = (i * 7 % 26 + 'a') as char; i += 1; }
            i = 0;
            while (i < 30) { la[i] = 1000 - i * 31; i += 1; }
            i = 0;
            while (i < 20) { gr[i] = i as real * 0.75 - 3.0; i += 1; }
            var p: *int = &ga[10];
            var s: int = 0;
            var j: int = 1;
            while (j < 6) {
                s = s * 3 + ga[4 * j + 1] - ga[j * j] + la[2 * j - 1] + gc[j * 5 + 2] + p[j * 3 - 2];
                s = s % 1000003 + ga[la[j] % 50 + 1] + ga[ga[j + 2] % 7 + 20];   // nested
                s += ga[bump(j * 2) + gi];                                      // a call in the index
                s = (s + (ga[j * 2] + la[j + 3]) * (gc[j * 3] - ga[j * 4 + 1])) % 999983;
                j += 1;
            }
            put(s);
            var r: real = 0.0;
            j = 0;
            while (j < 6) { r = r * 0.5 + gr[j * 3 + 1] * gr[(j * 2) % 7] - gr[j + 2 * 1]; j += 1; }
            put((r * 1000.0) as int);
            return s % 9841;
        }
    "#;
    same_both_ways_src("anyindex.tri", src, 50_000_000);
}

#[test]
fn real_locals_in_registers_the_same_way() {
    // real locals of leaf functions live in f-1..f-9: more than nine (the
    // rest in memory), scopes that end and free theirs, parameters,
    // compound assignment, comparisons, an address taken; and a function
    // that calls (not a leaf) keeping its reals across calls to leaves
    // that use f-1..f-9 themselves
    let src = r#"
        import "console.tri";
        fn put(v: int) { var b: [28]char; itoa(v, &b[0]); print(&b[0]); print(" "); }
        fn leaf(x: real, y: real) -> real {
            var a: real = x * 2.0;
            var b: real = y - 1.0;
            var c: real = a * b;
            var d: real = c + x;
            var e: real = d / 3.0;
            var f: real = e - a;
            var g: real = f * f;
            var h: real = g + b;
            var i: real = h - c;
            var j: real = i + d;                   // the tenth and eleventh: memory
            var k: real = j * e;
            a += k; b -= j * 0.5; c *= 1.5; d /= 2.0;
            if (a > b) { var t: real = a; a = b; b = t; }
            { var u: real = c + d; c = u * 0.25; }
            { var v: real = d - c; d = v + 1.0; }
            var n: int = 0;
            var s: real = 0.0;
            while (s < a + 10.0 && n < 50) { s += b * 0.125 + 0.5; n += 1; }
            return a + b + c + d + e + f + g + h + i + j + k + s + n;
        }
        fn addr_taken(x: real) -> real {
            var r: real = x + 1.0;
            var p: *real = &r;
            *p = *p * 3.0;
            return r;
        }
        fn caller(x: real) -> real {
            var keep1: real = x * 7.0;              // not a leaf: in memory,
            var keep2: real = x - 0.25;             // and must survive the calls
            var r: real = leaf(x, keep1) + leaf(keep2, x);
            return r + keep1 * keep2 + addr_taken(keep2);
        }
        fn main() -> int {
            var t: int = 0;
            var q: int = 1;
            while (q < 6) {
                t = (t * 3 + (caller(q as real * 0.7) * 100.0) as int) % 1000003;
                q += 1;
            }
            put(t);
            return t % 9841;
        }
    "#;
    same_both_ways_src("reals.tri", src, 80_000_000);
}

#[test]
fn leaf_registers_and_inlined_calls_the_same_way() {
    // a leaf function also keeps locals in r7 and r8 (a seventh parameter
    // among them, with an eighth in memory loaded past it); temporaries held
    // across calls a leaf inlines, plain ones and ones whose body uses a
    // builtin; early returns out of inlined bodies (jumps to the next line)
    let src = r#"
        import "console.tri";
        fn put(v: int) { var b: [28]char; itoa(v, &b[0]); print(&b[0]); print(" "); }
        fn lo(x: int) -> int { if (x < 0) { return 0; } return x % 4096; }
        fn hi(x: int) -> int { return lo(x / 4096) * 7 + 1; }
        fn sg(x: int) -> int { return sgn(x) * 5 + abs(x) % 13; }
        fn nine(a: int, b: int, c: int, d: int, e: int, f: int, g: int, h: int, i: int) -> int {
            var p: *int = &h;                          // h in memory, after g in r7
            *p = *p + 1;
            var s: int = a + b * 2 + c * 3 + d * 5 + e * 7 + f * 11 + g * 13 + h * 17 + i * 19;
            var t: int = s % 1000;
            var u: int = t * 3 + g;
            var v: int = u - a + i;
            var w: int = v * 2 + e;
            var k: int = 0;
            while (k < 9) {
                s = s * 3 + lo(s) + hi(t) * 1000 + lo(u) * hi(v) + sg(w - s);
                t = (t + lo(s) * hi(u + k) - sg(s) * lo(w)) % 100003;
                u = (u + hi(t) + g) % 99991;
                v = (v * 3 + lo(t + u) + h) % 77777;
                w = (w + sg(v - u) + i) % 88883;
                s = s % 999983;
                k += 1;
            }
            return s + t + u + v + w + g + h;
        }
        fn main() -> int {
            var r: int = 0;
            var q: int = 1;
            while (q < 7) {
                r = (r * 3 + nine(q, -q, q * 2, 5, -7, q + 3, 100 - q, q * q, -9)) % 1000003;
                q += 1;
            }
            put(r);
            return r % 9841;
        }
    "#;
    same_both_ways_src("leafregs.tri", src, 80_000_000);
}

#[test]
fn temporaries_not_held_across_an_inlined_syscall() {
    // `syscall` takes r1..r6, so a temporary must not wait in r4..r6 across
    // a call whose inlined body makes one (a ROM cannot run it: read the code)
    let src = r#"
        fn sc(x: int) -> int { return syscall(1, x, 2, 3, 4, 5); }
        fn leafy(a: int, b: int) -> int { return a * 7 + sc(b) * 3; }
        fn main() -> int { return leafy(1, 2); }
    "#;
    let b = build_source("sc.tri", src, None, &opts(true)).unwrap();
    let body: Vec<&str> = b.asm.lines().skip_while(|l| !l.starts_with("leafy:")).take_while(|l| !l.starts_with("main:")).collect();
    let sys = body.iter().position(|l| l.trim_start().starts_with("sys ")).expect("the syscall inlined");
    for l in &body[sys + 1..] {
        assert!(!["r4", "r5", "r6"].iter().any(|r| l.contains(r)), "r4..r6 read after the syscall: {l}\n{}", body.join("\n"));
    }
}

#[test]
fn shared_parameter_registers_the_same_way() {
    // an inlined parameter shares the register of a local passed to it
    // unless the body assigns the parameter, or a later argument changes
    // the local; passed on to a nested inlined call; locals declared after
    // the call (the register must stay the caller's); constants on the left
    let src = r#"
        import "console.tri";
        fn put(v: int) { var b: [28]char; itoa(v, &b[0]); print(&b[0]); print(" "); }
        fn twice(x: int) -> int { return 2 * x + 1; }
        fn mix(x: int, y: int) -> int { return twice(x) * 7 + 3 * twice(y) - x; }
        fn bump(x: int) -> int { x += 5; x = x * 3; return x; }        // assigns its parameter
        fn pair(x: int, y: int) -> int { return x * 1000 + y; }
        fn work(n: int) -> int {
            var a: int = n;
            var s: int = 0;
            var k: int = 0;
            while (k < 12) {
                s = s * 3 + mix(a, k) + bump(a) + a;                   // a unchanged by bump
                s = s % 99991 + pair(a, a = a + 1) + 7 * a;            // a later argument changes a
                var t: int = twice(k) + 100 * twice(a);
                var u: int = mix(t, s) - t;
                s = (s + u + 2 * t + (5 + a) * 3) % 1000003;
                k += 1;
            }
            return s + a;
        }
        fn main() -> int {
            var r: int = 0;
            var q: int = 1;
            while (q < 6) { r = (r * 3 + work(q * 11)) % 1000003; q += 1; }
            put(r);
            return r % 9841;
        }
    "#;
    same_both_ways_src("sharedparams.tri", src, 80_000_000);
}
