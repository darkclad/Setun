//! The same programs on the Setun and compiled natively for the host:
//! the overhead of the emulator (JIT included) on compute-bound code.
//! The native versions follow the Trine source line by line, with the
//! same fixed-point arithmetic, truncating `/` and rounding `modr`.

use setun_core::Machine;
use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

/// modr: the remainder of rounding division (balanced; m is odd here).
fn modr(a: i64, m: i64) -> i64 {
    let r = a.rem_euclid(m);
    if r > m / 2 { r - m } else { r }
}

fn mandel_native(fb: &mut [i16]) {
    const W: i64 = 640;
    const H: i64 = 480;
    const ONE: i64 = 19683;
    const MAX_ITER: i64 = 27;
    let rgb = |r: i64, g: i64, b: i64| (r * 729 + g * 27 + b) as i16;
    let colour = |n: i64| {
        if n == MAX_ITER {
            return rgb(-13, -13, -13);
        }
        let t = n * 26 / MAX_ITER;
        rgb(t - 13, 13 - (2 * t - 26).abs(), 13 - t)
    };
    for y in 0..H {
        let ci = (y - H / 2) * (ONE * 12 / 5) / H;
        for x in 0..W {
            let cr = (x - W * 11 / 15) * (ONE * 3) / W;
            let (mut zr, mut zi, mut n) = (0i64, 0i64, 0);
            while n < MAX_ITER {
                let zr2 = zr * zr / ONE;
                let zi2 = zi * zi / ONE;
                if zr2 + zi2 > 4 * ONE {
                    break;
                }
                zi = 2 * zr * zi / ONE + ci;
                zr = zr2 - zi2 + cr;
                n += 1;
            }
            fb[(y * W + x) as usize] = colour(n);
        }
    }
}

fn sieve_sort_native() -> i64 {
    let mut sieve = vec![0i8; 20000];
    let n = 20000;
    for s in sieve.iter_mut().take(n).skip(2) {
        *s = 1;
    }
    let mut count = 0;
    for i in 2..n {
        if sieve[i] != 0 {
            count += 1;
            let mut j = i * i;
            while j < n {
                sieve[j] = 0;
                j += i;
            }
        }
    }
    let mut nums = vec![0i64; 1500];
    let mut seed = 12345i64;
    for v in nums.iter_mut() {
        seed = modr(seed * 1103 + 12345, 1000003);
        *v = seed;
    }
    for i in 1..1500 {
        let v = nums[i];
        let mut j = i as i64 - 1;
        while j >= 0 && nums[j as usize] > v {
            nums[j as usize + 1] = nums[j as usize];
            j -= 1;
        }
        nums[(j + 1) as usize] = v;
    }
    let bad = (1..1500).filter(|&i| nums[i - 1] > nums[i]).count() as i64;
    modr(count * 1000 + bad + nums[750] % 1000, 9841)
}

/// Best of `reps` timings of `f`, in seconds.
fn best<T>(reps: u32, mut f: impl FnMut() -> T) -> (f64, T) {
    let mut best = f64::MAX;
    let mut out = None;
    for _ in 0..reps {
        let t = Instant::now();
        let r = black_box(f());
        best = best.min(t.elapsed().as_secs_f64());
        out = Some(r);
    }
    (best, out.unwrap())
}

/// Run a ROM program to its halt or power-off, `reps` times on fresh
/// machines: (best seconds, instructions, the machine of the last run).
fn emulated(path: &Path, reps: u32) -> (f64, u64, Machine) {
    let b = trine::build_file(path, &trine::Options::rom()).unwrap_or_else(|e| panic!("{e}"));
    let mut best = f64::MAX;
    let mut last = None;
    for _ in 0..reps {
        let mut m = Machine::new(729, 1_790_000_000);
        m.load_rom(&b.image);
        let t = Instant::now();
        while !m.powered_off && !m.cpu.halted {
            m.run(1_000_000);
        }
        best = best.min(t.elapsed().as_secs_f64());
        last = Some(m);
    }
    let m = last.unwrap();
    let n = m.cpu.cr(setun_core::isa::cr::CYCLE).value() as u64;
    (best, n, m)
}

pub fn run(root: &Path) {
    println!("{:<12} {:>11} {:>11} {:>12} {:>9}", "program", "native", "Setun", "Setun insns", "overhead");

    let mut fb = vec![0i16; 640 * 480];
    let (tn, _) = best(20, || mandel_native(&mut fb));
    let (te, n, m) = emulated(&root.join("examples/mandel.tri"), 5);
    assert_eq!(m.mem.framebuffer()[..640 * 480], fb[..], "mandel: the same picture");
    println!("{:<12} {:>9.2}ms {:>9.2}ms {:>12} {:>8.1}x", "mandel", tn * 1e3, te * 1e3, n, te / tn);

    let (tn, want) = best(50, sieve_sort_native);
    let path = root.join("crates/bench/programs/sieve_sort.tri");
    let (te, n, _) = emulated(&path, 5);
    let b = trine::build_file(&path, &trine::Options::rom()).unwrap();
    let got = trine::run_rom(&b.image, 100_000_000, "").outcome;
    assert_eq!(got, trine::Outcome::Exit(want), "sieve_sort: the same result");
    println!("{:<12} {:>9.2}ms {:>9.2}ms {:>12} {:>8.1}x", "sieve+sort", tn * 1e3, te * 1e3, n, te / tn);
    println!("(best of several runs; the Setun times include compiling hot code)");
}
