use super::*;
use std::cmp::Ordering;

fn r(s: &str) -> Real {
    Real::from_decimal(s).unwrap().0
}

fn i(v: i64) -> Real {
    Real::from_i64(v)
}

/// A small deterministic generator of f64 test values over a wide range.
fn values() -> Vec<f64> {
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut out = vec![1.0, -1.0, 0.5, 3.0, 1.0 / 3.0, 1e-30, -7.25e40, 123456.789, 2f64.sqrt()];
    for _ in 0..300 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let mant = (x >> 11) as f64 / (1u64 << 53) as f64 + 0.5;
        let exp = ((x >> 3) % 120) as i32 - 60;
        let sign = if x & 1 == 0 { 1.0 } else { -1.0 };
        out.push(sign * mant * 10f64.powi(exp));
    }
    out
}

fn from_f64(v: f64) -> Real {
    r(&format!("{v:e}"))
}

fn close(got: Real, want: f64, what: &str) {
    let g = got.to_f64();
    let tol = want.abs() * 4e-16 + f64::MIN_POSITIVE;
    assert!((g - want).abs() <= tol, "{what}: got {g:e}, want {want:e}");
}

#[test]
fn encodings() {
    // 1 = E 1, m = 1/3: in base 27, `001 900000`
    let one = i(1);
    assert_eq!(one, Real { e: 1, m: TOP });
    let w = one.to_word27().0;
    assert_eq!(w, (3i128.pow(18) + 3i128.pow(17)) as i64, "E = 1 in the top tryte, then M");
    assert_eq!(Real::from_word27(w), one);
    assert_eq!(i(-1).m, -TOP);
    assert_eq!(i(3), Real { e: 2, m: TOP });
    assert_eq!(r("0.3333333333333333333333333333333333333").e, 0, "1/3 is near 3^0 · 1/3");
    // real81 words round-trip, for many values
    for v in values() {
        let x = from_f64(v);
        assert_eq!(Real::from_words(x.to_words()), x, "{v}");
    }
    for x in [Real::INF, Real::NEG_INF, Real::NAN, Real::ZERO, Real { e: E_MIN, m: 5 }] {
        assert_eq!(Real::from_words(x.to_words()), x);
    }
}

#[test]
fn kinds() {
    assert_eq!(Real::ZERO.kind(), Kind::Zero);
    assert_eq!(i(5).kind(), Kind::Normal);
    assert_eq!(Real::INF.kind(), Kind::Inf);
    assert_eq!(Real::NEG_INF.kind(), Kind::Inf);
    assert_eq!(Real::NAN.kind(), Kind::NaN);
    assert_eq!(Real { e: E_MAX, m: 42 }.kind(), Kind::NaN, "a NaN with a payload");
    assert_eq!(Real { e: E_MIN, m: 1 }.kind(), Kind::Subnormal);
    assert_eq!(Real { e: 7, m: 0 }.kind(), Kind::Zero, "zero with any exponent but E_MAX");
    assert_eq!([i(-5).class(), Real::ZERO.class(), Real::NAN.class(), Real::NEG_INF.class()], [-2, 0, 4, -3]);
    assert_eq!(Real { e: E_MIN, m: -1 }.class(), -1);
}

#[test]
fn integers_are_exact() {
    for v in [0, 1, -1, 2, 13, -9841, 123_456_789, WORD_MAX, -WORD_MAX] {
        let x = i(v);
        assert_eq!(x.to_i64(), (v, 0), "{v}");
    }
    assert_eq!(r("2.5").to_i64(), (2, IX), "ties to even");
    assert_eq!(r("3.5").to_i64(), (4, IX));
    assert_eq!(r("-2.5").to_i64(), (-2, IX));
    assert_eq!(r("2.4").to_i64(), (2, IX));
    assert_eq!(r("1e20").to_i64(), (WORD_MAX, IV), "out of range saturates");
    assert_eq!(r("-1e20").to_i64(), (-WORD_MAX, IV));
    assert_eq!(Real::NAN.to_i64(), (0, IV));
    assert_eq!(Real::NEG_INF.to_i64(), (-WORD_MAX, IV));
}

#[test]
fn arithmetic_matches_f64() {
    let vs = values();
    for (k, &a) in vs.iter().enumerate() {
        let b = vs[(k * 7 + 3) % vs.len()];
        let (x, y) = (from_f64(a), from_f64(b));
        close(x.add(y).0, a + b, &format!("{a:e} + {b:e}"));
        close(x.sub(y).0, a - b, &format!("{a:e} - {b:e}"));
        close(x.mul(y).0, a * b, &format!("{a:e} * {b:e}"));
        close(x.div(y).0, a / b, &format!("{a:e} / {b:e}"));
        close(x.abs().sqrt().0, a.abs().sqrt(), &format!("sqrt {a:e}"));
        assert_eq!(x.cmp(y), a.partial_cmp(&b), "cmp {a:e} {b:e}");
    }
}

#[test]
fn exact_results_raise_nothing() {
    // powers of 3 and their sums and products are exact
    assert_eq!(i(9).mul(i(27)), (i(243), 0));
    assert_eq!(i(1).div(i(3)).1, 0, "1/3 is exact in ternary");
    assert_eq!(i(1).div(i(3)).0.mul(i(3)), (i(1), 0));
    assert_eq!(i(1).div(i(2)).1, IX, "1/2 is not");
    assert_eq!(i(9).sqrt(), (i(3), 0));
    assert_eq!(i(2).sqrt().1, IX);
    assert_eq!(i(5).add(i(-5)), (Real::ZERO, 0), "no -0: x - x is the one zero");
    assert_eq!(Real::from_decimal("0.1").unwrap().1, IX, "0.1 is not exact");
    // a far smaller addend is below half an ulp: the larger stays, inexactly
    assert_eq!(i(1).add(r("1e-300")), (i(1), IX));
}

#[test]
fn ties_go_to_even() {
    // 11/2 = 5.5 is exactly halfway between two 72-trit neighbours
    let (q, f) = i(11).div(i(2));
    assert_eq!(f, IX);
    assert_eq!(q.m % 2, 0, "the even significand");
    let up = Real { e: q.e, m: q.m + 1 };
    let down = Real { e: q.e, m: q.m - 1 };
    let d_up = up.sub(r("5.5")).0.abs();
    let d_down = r("5.5").sub(down).0.abs();
    assert!(d_up.cmp(d_down) != Some(Ordering::Equal) || q.m % 2 == 0);
    // the same tie from the decimal side
    assert_eq!(r("5.5"), q);
    // round to integer
    assert_eq!(r("2.5").round_int(false).0, i(2));
    assert_eq!(r("3.5").round_int(false).0, i(4));
    assert_eq!(r("-0.5").round_int(false).0, Real::ZERO);
    assert_eq!(r("2.7").round_int(true), (i(2), IX));
    assert_eq!(r("-2.7").round_int(true), (i(-2), IX));
    assert_eq!(i(4).round_int(true), (i(4), 0));
}

#[test]
fn remainder_is_ieee_and_exact() {
    assert_eq!(i(7).rem(i(2)), (i(-1), 0), "7/2 = 3.5 rounds to 4");
    assert_eq!(i(5).rem(i(2)), (i(1), 0), "5/2 = 2.5 rounds to 2");
    assert_eq!(i(10).rem(i(3)), (i(1), 0));
    let third = i(1).div(i(3)).0; // 16/3 rem 1 = 1/3, exact in ternary
    assert_eq!(i(16).div(i(3)).0.rem(i(1)), (third, 0));
    assert_eq!(i(1).rem(Real::ZERO), (Real::NAN, IV));
    assert_eq!(Real::INF.rem(i(2)), (Real::NAN, IV));
    assert_eq!(i(3).rem(Real::INF), (i(3), 0));
}

#[test]
fn specials() {
    assert_eq!(Real::INF.sub(Real::INF), (Real::NAN, IV));
    assert_eq!(Real::INF.add(Real::INF), (Real::INF, 0));
    assert_eq!(Real::ZERO.mul(Real::INF), (Real::NAN, IV));
    assert_eq!(Real::ZERO.div(Real::ZERO), (Real::NAN, IV));
    assert_eq!(Real::INF.div(Real::INF), (Real::NAN, IV));
    assert_eq!(i(1).div(Real::ZERO), (Real::INF, DZ));
    assert_eq!(i(-3).div(Real::ZERO), (Real::NEG_INF, DZ));
    assert_eq!(i(-1).sqrt(), (Real::NAN, IV));
    assert_eq!(Real::NEG_INF.sqrt(), (Real::NAN, IV));
    assert_eq!(i(5).div(Real::INF), (Real::ZERO, 0));
    // a NaN passes through untouched, payload and all, raising nothing
    let p = Real { e: E_MAX, m: 12345 };
    assert_eq!(p.add(i(1)), (p, 0));
    assert_eq!(i(1).mul(p), (p, 0));
    assert_eq!(p.add(Real::NAN), (p, 0), "the first NaN wins");
    assert_eq!(p.neg(), Real { e: E_MAX, m: -12345 });
    assert_eq!(p.cmp(i(1)), None);
    assert_eq!(i(1).min(p), i(1), "a NaN loses to a number");
    assert_eq!(p.max(i(-7)), i(-7));
}

#[test]
fn overflow_and_underflow() {
    let big = Real { e: 9840, m: TOP };
    assert_eq!(big.mul(i(27)), (Real::INF, OF | IX));
    assert_eq!(big.neg().mul(i(27)), (Real::NEG_INF, OF | IX));
    assert_eq!(big.scale(5), (Real::INF, OF | IX));
    // gradual underflow: the smallest normal, divided by 3, is exact
    let small = Real { e: E_MIN, m: TOP };
    assert_eq!(small.div(i(3)), (Real { e: E_MIN, m: TOP / 3 }, 0));
    assert_eq!(small.div(i(3)).0.kind(), Kind::Subnormal);
    // and an inexact tiny result raises UF
    let (t, f) = small.div(i(4));
    assert_eq!(t.kind(), Kind::Subnormal);
    assert_eq!(f, UF | IX);
    assert_eq!(small.mul(small), (Real::ZERO, UF | IX), "flushes to zero");
    // subnormals work in arithmetic
    let sub = Real { e: E_MIN, m: 5 };
    assert_eq!(sub.add(sub), (Real { e: E_MIN, m: 10 }, 0));
    assert_eq!(sub.mul(i(3)).0, Real { e: E_MIN, m: 15 });
    assert_eq!(sub.exponent().0, E_MIN - 72 + 3, "5 is three trits (1TT): its exponent written normalized");
}

#[test]
fn comparisons() {
    let order = [Real::NEG_INF, r("-1e100"), i(-1), r("-1e-300"), Real::ZERO, Real { e: E_MIN, m: 1 }, r("0.5"), i(1), r("1e100"), Real::INF];
    for (a, x) in order.iter().enumerate() {
        for (b, y) in order.iter().enumerate() {
            assert_eq!(x.cmp(*y), Some(a.cmp(&b)), "{a} vs {b}");
        }
    }
    assert_eq!(Real { e: 3, m: 0 }.cmp(Real::ZERO), Some(Ordering::Equal), "every zero is the zero");
}

#[test]
fn real27_rounding() {
    let (third, f) = i(1).div(i(3)).0.to_real27();
    assert_eq!(f, 0);
    assert_eq!(third, i(1).div(i(3)).0, "1/3 fits in 18 trits");
    let (tenth, f) = r("0.1").to_real27();
    assert_eq!(f, IX);
    assert!((tenth.to_f64() - 0.1).abs() < 0.1 * 2.6e-9, "18 trits ≈ 28.5 bits");
    // the first word of a real81 is that value in real27, rounded (no tie here)
    for v in values() {
        let x = from_f64(v);
        assert_eq!(Real::from_word27(x.to_words()[0]), x.to_real27().0, "{v}");
    }
    assert_eq!(Real::INF.to_word27(), (Real::INF.to_words()[0], 0));
    assert_eq!(Real::from_word27(Real::NAN.to_word27().0), Real::NAN);
    // one shared exponent: narrowing never overflows, it only loses
    // precision; the largest real81 becomes the largest real27
    let max27 = (3i128.pow(18) - 1) / 2 * 3i128.pow(54);
    assert_eq!(Real { e: 9840, m: M_MAX }.to_real27(), (Real { e: 9840, m: max27 }, IX));
}

#[test]
fn decimal_text() {
    assert_eq!(r("3.25").to_decimal(10), "3.25");
    assert_eq!(r("-0.001").to_decimal(10), "-0.001");
    assert_eq!(r("1e100").to_decimal(5), "1e100");
    assert_eq!(r("123456").to_decimal(3), "123000");
    assert_eq!(r("2.5e-20").to_decimal(4), "2.5e-20");
    assert_eq!(r("1_000.5").to_decimal(8), "1000.5");
    assert_eq!(Real::INF.to_decimal(5), "inf");
    assert_eq!(Real::NAN.to_decimal(5), "nan");
    assert_eq!(Real::ZERO.to_decimal(5), "0");
    assert_eq!(r("0.1").to_decimal(30), "0.1", "nearest to 30 digits is still 0.1");
    assert_eq!(Real::from_decimal("1e5000").unwrap(), (Real::INF, OF | IX));
    assert_eq!(Real::from_decimal("1e-5000").unwrap(), (Real::ZERO, UF | IX));
    for bad in ["", ".", "1.2.3", "1e", "abc", "--1", "1e+"] {
        assert!(Real::from_decimal(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn constants() {
    let c = |k| Real::constant(k).unwrap();
    assert_eq!(c(0), Real::ZERO);
    assert_eq!(c(1), i(1));
    assert_eq!(c(2).to_decimal(33), "3.1415926535897932384626433832795", "(a trailing 0 is trimmed)");
    assert_eq!(c(3).to_decimal(33), "2.71828182845904523536028747135266");
    assert_eq!(c(4).to_decimal(30), "1.09861228866810969139524523692");
    assert_eq!(c(5).to_decimal(15), "0.910239226626837");
    assert_eq!(c(6).to_decimal(15), "2.09590327428938");
    assert!(Real::constant(7).is_none() && Real::constant(-1).is_none());
    // ln 3 · log₃ e = 1, to within an ulp
    let one = c(4).mul(c(5)).0;
    assert!(one.sub(i(1)).0.abs().cmp(r("1e-33")) == Some(Ordering::Less));
}

/// Random values over the whole range: normal, short significands,
/// unnormalized encodings, subnormals at E_MIN, exponents near E_MAX.
fn wide_values(seed: u64, count: usize) -> Vec<Real> {
    let mut x = seed | 1;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let r = next();
        let e = match r % 8 {
            0 => E_MIN,
            1 => E_MAX - 1 - (next() % 5) as i64,
            2 => E_MIN + (next() % 5) as i64,
            3 => (next() % 400) as i64 - 200,
            _ => (next() % 40) as i64 - 20,
        };
        let full = (next() as i128) << 50 ^ next() as i128;
        let m = match (r >> 8) % 4 {
            0 => full % M_MAX,                       // anything, maybe unnormalized
            1 => (full % (M_MAX - M_NORM)).abs() + M_NORM + 1, // normalized
            2 => (next() % 1000) as i128 - 500,      // tiny significand
            _ => -((full % (M_MAX - M_NORM)).abs() + M_NORM + 1),
        };
        if m != 0 {
            out.push(Real { e, m });
        }
    }
    out
}

#[test]
fn fixed_width_fast_paths_agree_with_big_integers() {
    let vals = wide_values(0x51ed_270b_57a1_0c05, 1500);
    let mut fast_hits = 0;
    for (k, &a) in vals.iter().enumerate() {
        let b = vals[(k * 7919 + 1) % vals.len()];
        for (x, y) in [(a, b), (a, a), (a, a.neg()), (a, Real { e: a.e - 1, m: b.m })] {
            assert_eq!(x.add_with(y, true), x.add_with(y, false), "add {x:?} {y:?}");
            assert_eq!(x.mul_with(y, true), x.mul_with(y, false), "mul {x:?} {y:?}");
            assert_eq!(x.div_with(y, true), x.div_with(y, false), "div {x:?} {y:?}");
            if fast::add(x, y).is_some() {
                fast_hits += 1;
            }
        }
        let p = a.abs();
        assert_eq!(p.sqrt_with(true), p.sqrt_with(false), "sqrt {p:?}");
    }
    for v in [0, 1, -1, 2, WORD_MAX, -WORD_MAX, 123_456_789, -42] {
        assert_eq!(Some(Real::from_i64(v)), Some(Real::round81(&BigInt::from(v), &BigInt::one(), 0, false).0));
    }
    // ordinary values really do take the fast path
    assert!(fast_hits > 3000, "fast path used {fast_hits} times");
    let one = Real::from_i64(1);
    let three = Real::from_i64(3);
    assert!(fast::add(one, three).is_some() && fast::mul(one, three).is_some());
    assert!(fast::div(one, three).is_some() && fast::sqrt(three).is_some());
}

#[test]
fn fixed_width_conversions_agree_with_big_integers() {
    for a in wide_values(0x2bd7_a6a6_e99c_2ddc, 3000) {
        let (m, p) = a.exact();
        // scaling and narrowing
        for k in [-7, 0, 5, 100, -3000] {
            if let Some(r) = fast::scale(a, k) {
                assert_eq!(r, Real::round81(&m, &BigInt::one(), p + k, false), "scale {a:?} {k}");
            }
        }
        if let Some(r) = fast::narrow(a, N27) {
            assert_eq!(r, Real::round(&m, &BigInt::one(), p, false, N27), "real27 {a:?}");
        }
        assert_eq!(fast::exponent(a), p + trit_len(&m) as i64, "exponent {a:?}");
        // rounding to integers
        for tz in [false, true] {
            if let Some(r) = fast::round_to_int(a, tz) {
                let expect = if p >= 0 {
                    Real::round81(&m, &BigInt::one(), p, false)
                } else {
                    let den = big_pow3((-p) as u32);
                    let (q, inexact) = if tz {
                        let (q, rm) = m.div_rem(&den);
                        (q, !rm.is_zero())
                    } else {
                        round_div(&m, &den, false)
                    };
                    (Real::round81(&q, &BigInt::one(), 0, false).0, if inexact { IX } else { 0 })
                };
                assert_eq!(r, expect, "round_int {a:?} {tz}");
            }
        }
        if let Some((v, inexact)) = fast::to_int(a, WORD_MAX) {
            let (q, ix) = if p >= 0 { (m.clone() * big_pow3(p as u32), false) } else { round_div(&m, &big_pow3((-p) as u32), false) };
            match q.to_i64() {
                Some(w) if w.abs() <= WORD_MAX => assert_eq!((v, inexact), (Some(w), ix), "to_int {a:?}"),
                _ => assert_eq!(v, None, "to_int out of range {a:?}"),
            }
        }
    }
}

#[test]
fn word_split_matches_division() {
    let mut vals: Vec<i128> = vec![0, 1, -1, M_MAX, -M_MAX, TOP, -TOP, C27, -C27, C27 / 2, -(C27 / 2), C27 / 2 + 1];
    for a in wide_values(0x7f4a_7c15_9e37_79b9, 4000) {
        vals.push(a.m);
        vals.push(a.m / 7 + 1);
    }
    for v in vals {
        let (q, lo) = split27(v);
        assert_eq!((q, lo as i128), bal_split(v, C27), "{v}");
        let r = Real { e: 17, m: v.clamp(-M_MAX, M_MAX) };
        assert_eq!(Real::from_words(r.to_words()), r);
    }
    for e in [E_MIN, -1, 0, 1, E_MAX] {
        let r = Real { e, m: -M_MAX };
        assert_eq!(Real::from_words(r.to_words()), r);
    }
}

#[test]
#[ignore]
fn time_ops() {
    use std::time::Instant;
    let a = Real::from_i64(123_456_789).div(Real::from_i64(7)).0;
    let b = Real::from_i64(987_654_321).div(Real::from_i64(13)).0;
    let n = 200_000;
    let t = Instant::now();
    let mut x = a;
    for _ in 0..n { x = std::hint::black_box(x.add(b).0); }
    println!("add  {:.1} ns", t.elapsed().as_nanos() as f64 / n as f64);
    let t = Instant::now();
    for _ in 0..n { x = std::hint::black_box(a.mul(b).0); }
    println!("mul  {:.1} ns", t.elapsed().as_nanos() as f64 / n as f64);
    let t = Instant::now();
    for _ in 0..n { x = std::hint::black_box(a.div(b).0); }
    println!("div  {:.1} ns", t.elapsed().as_nanos() as f64 / n as f64);
    let t = Instant::now();
    for _ in 0..n { std::hint::black_box(a.cmp(b)); }
    println!("cmp  {:.1} ns", t.elapsed().as_nanos() as f64 / n as f64);
    let big = ethnum::U256::from(u128::MAX) * 12345u128;
    let t = Instant::now();
    let mut acc = ethnum::U256::ZERO;
    for k in 0..n { acc += std::hint::black_box(big + k as u128) / ethnum::U256::from(3u32); }
    println!("u256/3 {:.1} ns", t.elapsed().as_nanos() as f64 / n as f64);
    let t = Instant::now();
    for k in 0..n { acc += std::hint::black_box(big + k as u128) / ethnum::U256::from(pow3(72) as u128); }
    println!("u256/3^72 {:.1} ns {}", t.elapsed().as_nanos() as f64 / n as f64, acc & ethnum::U256::ONE);
    let _ = x;
}

#[test]
fn fast_comparison_agrees_with_big_integers() {
    let vals = wide_values(0x1234_5678_9abc_def1, 2000);
    for (k, &a) in vals.iter().enumerate() {
        let b = vals[(k * 31 + 7) % vals.len()];
        for (x, y) in [(a, b), (a, a), (a, Real { e: a.e + 1, m: a.m / 3 }), (a, a.neg())] {
            if let Some(o) = fast::cmp(x, y) {
                let (mx, px) = x.exact();
                let (my, py) = y.exact();
                let p = px.min(py);
                let big = (mx * big_pow3((px - p) as u32)).cmp(&(my * big_pow3((py - p) as u32)));
                assert_eq!(o, big, "{x:?} {y:?}");
            }
        }
    }
}
