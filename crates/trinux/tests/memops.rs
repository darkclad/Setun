//! The kernel's memory routines (the assembly in kernel/mm.tri), lifted
//! into a ROM program and checked against copies done a tryte at a time:
//! every length up to 40 and every alignment of the source and the
//! destination, overlapping both ways, and nothing written outside.

use trine::{build_source, run_rom, Options, Outcome};

/// The `asm { ... }` block of kernel/mm.tri, as a Trine function.
fn kernel_asm() -> String {
    let src = std::fs::read_to_string(trinux::kernel_dir().join("mm.tri")).expect("mm.tri");
    let start = src.find("fn _mm_asm() {").expect("the assembly block");
    let end = start + src[start..].find("\n}\n").expect("its end") + 3;
    src[start..end].to_string()
}

fn run(driver: &str) -> Outcome {
    let src = format!(
        "{driver}\nextern fn zero_words(p: *int, n: int);\nextern fn copy_words(dst: *int, src: *int, n: int);\n\
         extern fn copy_trytes(dst: *tryte, src: *tryte, n: int);\nextern fn zero_trytes(dst: *tryte, n: int);\n{}",
        kernel_asm()
    );
    let b = build_source("memops.tri", &src, None, &Options::rom()).unwrap_or_else(|e| panic!("{e}"));
    run_rom(&b.image, 400_000_000, "").outcome
}

#[test]
fn copy_trytes_matches_a_tryte_at_a_time() {
    // for each (dst offset, src offset, n): fill, copy, compare with the
    // reference (forward, a tryte at a time, as the routine promises);
    // returns the number of mismatches
    let driver = r#"
        var a: [400]tryte;
        var b: [400]tryte;
        fn fill() {
            var i: int = 0;
            while (i < 400) { a[i] = ((i * 37 + 11) % 19683 - 9841) as tryte; b[i] = a[i]; i += 1; }
        }
        fn main() -> int {
            var bad: int = 0;
            var n: int = 0;
            while (n <= 40) {
                var d: int = 100;
                while (d < 106) {
                    var s: int = 90;
                    while (s < 118) {                       // below, overlapping and above d
                        fill();
                        copy_trytes(&a[d], &a[s], n);
                        var k: int = 0;
                        while (k < n) { b[d + k] = b[s + k]; k += 1; }
                        var i: int = 0;
                        while (i < 400) { if (a[i] != b[i]) { bad += 1; } i += 1; }
                        s += 1;
                    }
                    d += 1;
                }
                n += 1;
            }
            return bad;
        }
    "#;
    assert_eq!(run(driver), Outcome::Exit(0));
}

#[test]
fn word_routines_match_a_word_at_a_time() {
    // zero_words and copy_words for every count up to 30 (the unrolled
    // part and its tail), copy_words overlapping both ways
    let driver = r#"
        var a: [120]int;
        var b: [120]int;
        fn fill() {
            var i: int = 0;
            while (i < 120) { a[i] = i * 1000003 - 7; b[i] = a[i]; i += 1; }
        }
        fn check() -> int {
            var bad: int = 0;
            var i: int = 0;
            while (i < 120) { if (a[i] != b[i]) { bad += 1; } i += 1; }
            return bad;
        }
        fn main() -> int {
            var bad: int = 0;
            var n: int = 0;
            while (n <= 30) {
                fill();
                zero_words(&a[40], n);
                var k: int = 0;
                while (k < n) { b[40 + k] = 0; k += 1; }
                bad += check();
                var s: int = 30;
                while (s <= 50) {
                    fill();
                    copy_words(&a[40], &a[s], n);
                    k = 0;
                    while (k < n) { b[40 + k] = b[s + k]; k += 1; }
                    bad += check();
                    s += 5;
                }
                n += 1;
            }
            // nothing at all for counts of 0 and below
            fill();
            zero_words(&a[40], -3);
            copy_words(&a[40], &a[60], 0);
            bad += check();
            return bad;
        }
    "#;
    assert_eq!(run(driver), Outcome::Exit(0));
}
