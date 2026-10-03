use setun_real::Real;
use std::time::Instant;
fn main() {
    let a = Real::from_i64(3).div(Real::from_i64(7)).0;
    let b = Real::from_i64(5).div(Real::from_i64(11)).0;
    let n = 2_000_000;
    for (name, f) in [
        ("add", (|x: Real, y: Real| x.add(y)) as fn(Real, Real) -> (Real, u8)),
        ("mul", |x, y| x.mul(y)),
        ("div", |x, y| x.div(y)),
        ("sqrt", |x, _| x.sqrt()),
    ] {
        let t = Instant::now();
        let mut acc = a;
        let mut fl = 0;
        for i in 0..n {
            let (r, f2) = f(if i % 2 == 0 { a } else { acc }, b);
            acc = if r.e.abs() > 50 { a } else { r };
            fl |= f2;
        }
        println!("{name}: {:.1} ns ({:?} {fl})", t.elapsed().as_nanos() as f64 / n as f64, acc.e);
    }
}
