//! Emulator speed: time the demo ROM's graphics sweep.
fn main() {
    let mut m = setun_demo::machine(729, 0);
    m.run(200_000);
    m.dev.key(43, true);
    let n = 5_000_000;
    let t = std::time::Instant::now();
    m.run(n);
    let s = t.elapsed().as_secs_f64();
    println!("{n} instructions in {s:.2} s = {:.1} MIPS", n as f64 / s / 1e6);
}
