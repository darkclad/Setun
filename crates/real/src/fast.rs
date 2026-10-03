//! Fast paths in fixed-width (256-bit) integers for the common cases: every
//! function returns `None` when its operands are out of its range (far
//! apart exponents, results near overflow or in subnormal territory), and
//! the caller then takes the exact big-integer path. Where both apply they
//! give identical results: each rounds the exact value once, correctly.

use crate::{Real, E_MAX, E_MIN, IX, M_MAX, M_NORM, N81};
use std::cmp::Ordering;
use ethnum::{I256, U256};
use std::sync::OnceLock;

/// 3^k for k = 0..=161 (3^161 < 2^256).
fn pow3_256() -> &'static [U256; 162] {
    static T: OnceLock<[U256; 162]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [U256::ONE; 162];
        for k in 1..162 {
            t[k] = t[k - 1] * 3;
        }
        t
    })
}

fn p3(k: u32) -> U256 {
    pow3_256()[k as usize]
}

/// 3^k for k = 0..=80 as i128, at compile time.
const POW3_128: [i128; 81] = {
    let mut t = [1i128; 81];
    let mut k = 1;
    while k < 81 {
        t[k] = t[k - 1] * 3;
        k += 1;
    }
    t
};

/// Magnitudes up to this keep 2a + 1 and a · 3 in range.
const A_LIMIT_BITS: u32 = 250;

fn bits(a: U256) -> u32 {
    256 - a.leading_zeros()
}

/// The smallest t with |a| ≤ (3^t − 1)/2, i.e. 3^t ≥ 2a + 1.
fn trit_len(a: U256) -> u32 {
    if a == U256::ZERO {
        return 0;
    }
    let target = (a << 1u32) + U256::ONE;
    // 3^t >= target: start just below from the bit length, then step up
    let mut t = ((bits(target) - 1) as u64 * 10_000 / 15_850) as usize;
    let p = pow3_256();
    while p[t] < target {
        t += 1;
    }
    t as u32
}

/// Round a / d to the nearest integer, ties to even; `sticky`: the true
/// value is a little larger. Returns (q, inexact).
fn round_div(a: U256, d: U256, sticky: bool) -> (U256, bool) {
    let (q, r) = if a.high() == &0 && d.high() == &0 {
        let (a, d) = (*a.low(), *d.low());
        (U256::from(a / d), U256::from(a % d))
    } else {
        let q = a / d;
        (q, a - mulu(q, d))
    };
    let twice = r << 1u32;
    let up = match twice.cmp(&d) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => sticky || (q & U256::ONE) == U256::ONE,
    };
    (if up { q + U256::ONE } else { q }, r != U256::ZERO || sticky)
}

/// Round num · 3^p to `n` significand trits (as a real81). `sticky`: the
/// true magnitude is a little more than |num| (num ≠ 0 then).
pub(crate) fn round_int(num: I256, p: i64, sticky: bool, n: u32) -> Option<(Real, u8)> {
    if num == I256::ZERO {
        return if sticky { None } else { Some((Real::ZERO, 0)) };
    }
    let a = num.unsigned_abs();
    if bits(a) > A_LIMIT_BITS {
        return None;
    }
    // the exponent is exactly p + t (see Real::round): stay clear of the ends
    let t = trit_len(a);
    let e = p + t as i64;
    if e <= E_MIN + 1 || e >= E_MAX - 1 {
        return None;
    }
    let k = n as i64 - t as i64;
    let (q, inexact) = if k >= 0 {
        (mulu(a, p3(k as u32)), sticky)
    } else {
        round_div(a, p3((-k) as u32), sticky)
    };
    let max = U256::from(((POW3_128[n as usize] - 1) / 2) as u128);
    let q = q.min(max);
    let m = q.as_i128() * POW3_128[(N81 - n) as usize];
    Some((Real { e, m: if num < I256::ZERO { -m } else { m } }, if inexact { IX } else { 0 }))
}

/// a · b for 128-bit a, b: the full 256-bit product, from 64-bit pieces.
#[inline]
fn mul128(a: u128, b: u128) -> U256 {
    let (a1, a0) = (a >> 64, a as u64 as u128);
    let (b1, b0) = (b >> 64, b as u64 as u128);
    let (p00, p01, p10, p11) = (a0 * b0, a0 * b1, a1 * b0, a1 * b1);
    let mid = (p00 >> 64) + (p01 as u64 as u128) + (p10 as u64 as u128);
    let lo = (p00 as u64 as u128) | (mid << 64);
    let hi = p11 + (p01 >> 64) + (p10 >> 64) + (mid >> 64);
    U256::from_words(hi, lo)
}

/// a · b where the caller knows the product fits in 256 bits.
#[inline]
fn mulu(a: U256, b: U256) -> U256 {
    let ((ah, al), (bh, bl)) = (a.into_words(), b.into_words());
    match (ah, bh) {
        (0, 0) => mul128(al, bl),
        (0, _) => mul128(al, bl) + (mul128(al, bh) << 128u32),
        (_, 0) => mul128(al, bl) + (mul128(ah, bl) << 128u32),
        _ => a * b,
    }
}

/// m · 3^k in 256 bits, if it stays within the limit.
fn scaled(m: i128, k: u32) -> Option<I256> {
    if k > 161 {
        return None;
    }
    let mag = m.unsigned_abs();
    let p = p3(k);
    let bm = 128 - mag.leading_zeros();
    if bm + bits(p) > A_LIMIT_BITS {
        return None;
    }
    // p < 2^(250 − bm) and the product fits: multiply by p's 128-bit halves
    let (ph, pl) = p.into_words();
    let v = mul128(mag, pl) + (mul128(mag, ph) << 128u32);
    Some(if m < 0 { -v.as_i256() } else { v.as_i256() })
}

// ---- in i128 ----------------------------------------------------------------
//
// Rounding to nearest in balanced ternary is dropping trits (no ties for a
// power of 3), and dropping k trits and then j more is dropping k + j: so
// a sum or product can be rounded in steps, each in i128, and still be
// rounded once, exactly. These return None where they would have to look
// further (cancellation after an inexact step, subnormals, the exponent
// ends); the 256-bit paths below then take over.

/// (hi:lo) / d and the remainder, for hi < d: one hardware division.
#[inline(always)]
fn div_2by1(hi: u64, lo: u64, d: u64) -> (u64, u64) {
    #[cfg(target_arch = "x86_64")]
    {
        let (q, r): (u64, u64);
        // SAFETY: hi < d, so the quotient fits in 64 bits and DIV cannot fault
        unsafe {
            std::arch::asm!("div {d}", d = in(reg) d, inout("rax") lo => q, inout("rdx") hi => r, options(pure, nomem, nostack));
        }
        (q, r)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let n = (hi as u128) << 64 | lo as u128;
        ((n / d as u128) as u64, (n % d as u128) as u64)
    }
}

/// n / d and n % d for a 64-bit d: two hardware divisions at most, where
/// i128 division is a library call.
#[inline(always)]
fn div_u64(n: u128, d: u64) -> (u128, u64) {
    let (hi, lo) = ((n >> 64) as u64, n as u64);
    if hi < d {
        let (q, r) = div_2by1(hi, lo, d);
        return (q as u128, r);
    }
    let (qh, rh) = (hi / d, hi % d);
    let (ql, r) = div_2by1(rh, lo, d);
    ((qh as u128) << 64 | ql as u128, r)
}

/// round(v / 3^k) for k ≤ 80, and whether that was inexact.
#[inline(always)]
fn drop_trits(v: i128, k: u32) -> (i128, bool) {
    if k == 0 {
        return (v, false);
    }
    let p = POW3_128[k as usize];
    let (q, r) = if k <= 40 {
        // 3^40 < 2^64
        let (q, r) = div_u64(v.unsigned_abs(), p as u64);
        if v < 0 { (-(q as i128), -(r as i128)) } else { (q as i128, r as i128) }
    } else {
        let q = v / p;
        (q, v - q * p)
    };
    let half = p / 2; // (p − 1) / 2: p is odd
    if r > half {
        (q + 1, true)
    } else if r < -half {
        (q - 1, true)
    } else {
        (q, r != 0)
    }
}

fn finish(m: i128, e: i64, inexact: bool) -> Option<(Real, u8)> {
    if e <= E_MIN + 1 || e >= E_MAX - 1 {
        return None;
    }
    Some((Real { e, m }, if inexact { IX } else { 0 }))
}

/// a + b: the smaller's trits below the larger's last one are dropped
/// first, then the sum is renormalized.
fn add_i128(a: Real, b: Real) -> Option<(Real, u8)> {
    let (big, small) = if a.e >= b.e { (a, b) } else { (b, a) };
    let d = (big.e - small.e) as u64;
    if d > 80 {
        return None;
    }
    let (s, mut inexact) = drop_trits(small.m, d as u32);
    let (mut m, mut e) = (big.m + s, big.e);
    if m.abs() > M_MAX {
        let (q, ix) = drop_trits(m, 1);
        (m, e) = (q, e + 1);
        inexact |= ix;
    } else if m.abs() <= M_NORM {
        // leading trits cancelled: exact so far, shift up; else the trits
        // dropped would be needed
        if inexact || m == 0 {
            return None;
        }
        while m.abs() <= M_NORM {
            (m, e) = (m * 3, e - 1);
        }
    }
    finish(m, e, inexact)
}

/// a · b: each significand as two balanced base-3^36 digits, the four
/// partial products in i128, and the 143-odd-trit product dropped to 72
/// trits in two steps of 36 (or 35, 34 when it is a trit or two shorter).
fn mul_i128(a: Real, b: Real) -> Option<(Real, u8)> {
    const B: i128 = POW3_128[36];
    let split = |m: i128| {
        let h = drop_trits(m, 36).0;
        (h, m - h * B)
    };
    let ((ah, al), (bh, bl)) = (split(a.m), split(b.m));
    let (p2, p1, p0) = (ah * bh, ah * bl + al * bh, al * bl);
    let (q0, ix0) = drop_trits(p0, 36);
    let p1 = p1 + q0;
    // dropped 36 + k trits in all: e = a.e + b.e − 36 + k
    let (q1, ix1) = drop_trits(p1, 36);
    let m = p2 + q1;
    if m.abs() > M_NORM {
        return finish(m, a.e + b.e, ix0 || ix1);
    }
    let (q1, ix1) = drop_trits(p1, 35);
    let m = p2 * 3 + q1;
    if m.abs() > M_NORM {
        return finish(m, a.e + b.e - 1, ix0 || ix1);
    }
    let (q1, ix1) = drop_trits(p1, 34);
    let m = p2 * 9 + q1;
    if m.abs() > M_NORM {
        return finish(m, a.e + b.e - 2, ix0 || ix1);
    }
    None
}

/// Sum of two finite non-zero values whose exponents are not too far apart.
pub(crate) fn add(a: Real, b: Real) -> Option<(Real, u8)> {
    if let Some(r) = add_i128(a, b) {
        return Some(r);
    }
    let (pa, pb) = (a.e - N81 as i64, b.e - N81 as i64);
    if (pa - pb).abs() > 80 {
        return None;
    }
    let p = pa.min(pb);
    let num = scaled(a.m, (pa - p) as u32)?.checked_add(scaled(b.m, (pb - p) as u32)?)?;
    round_int(num, p, false, N81)
}

/// Product of two finite non-zero values.
pub(crate) fn mul(a: Real, b: Real) -> Option<(Real, u8)> {
    if let Some(r) = mul_i128(a, b) {
        return Some(r);
    }
    let mag = mul128(a.m.unsigned_abs(), b.m.unsigned_abs()).as_i256();
    let num = if (a.m < 0) != (b.m < 0) { -mag } else { mag };
    round_int(num, a.e - N81 as i64 + b.e - N81 as i64, false, N81)
}

/// Quotient of two finite non-zero values.
pub(crate) fn div(a: Real, b: Real) -> Option<(Real, u8)> {
    let (x, y) = (U256::from(a.m.unsigned_abs()), U256::from(b.m.unsigned_abs()));
    let p = (a.e - N81 as i64) - (b.e - N81 as i64);
    // the smallest e with 2x · 3^(p−e) ≤ y (no ties: x/y·3^k is never ½)
    let fits = |e: i64| -> Option<bool> {
        let k = p - e;
        if k.unsigned_abs() > 120 {
            return None;
        }
        Some(if k >= 0 { mulu(x << 1u32, p3(k as u32)) <= y } else { x << 1u32 <= mulu(y, p3((-k) as u32)) })
    };
    let mut e = p + trit_len(x) as i64 - trit_len(y) as i64;
    let mut guard = 0;
    while !fits(e)? {
        e += 1;
        guard += 1;
        if guard > 4 {
            return None;
        }
    }
    while fits(e - 1)? {
        e -= 1;
        guard += 1;
        if guard > 8 {
            return None;
        }
    }
    if e <= E_MIN + 1 || e >= E_MAX - 1 {
        return None;
    }
    // M = round(x / y · 3^(p + n − e))
    let k = p + N81 as i64 - e;
    let (q, inexact) = if k >= 0 {
        if k > 161 || bits(x) + (k as u32 * 8).div_ceil(5) > A_LIMIT_BITS {
            return None;
        }
        round_div(mulu(x, p3(k as u32)), y, false)
    } else {
        if -k > 161 || bits(y) + ((-k) as u32 * 8).div_ceil(5) > A_LIMIT_BITS {
            return None;
        }
        round_div(x, mulu(y, p3((-k) as u32)), false)
    };
    let max = U256::from(crate::M_MAX as u128);
    let m = q.min(max).as_i128();
    let neg = (a.m < 0) != (b.m < 0);
    Some((Real { e, m: if neg { -m } else { m } }, if inexact { IX } else { 0 }))
}

/// Integer square root (floor) of a 256-bit value.
fn isqrt(x: U256) -> U256 {
    if x < U256::from(2u32) {
        return x;
    }
    // start above the root, then Newton down
    let b = bits(x);
    let mut r = U256::ONE << b.div_ceil(2);
    loop {
        let next = (r + x / r) >> 1u32;
        if next >= r {
            return r;
        }
        r = next;
    }
}

/// Square root of a finite positive value, rounded exactly: the integer
/// root gives a candidate, and an integer test on the halfway points
/// settles it. There are no ties: that would need 4x = ((2q ± 1)·3^s)², an
/// odd number equal to a multiple of 4.
pub(crate) fn sqrt(a: Real) -> Option<(Real, u8)> {
    let m = a.m.unsigned_abs();
    let p = a.e - N81 as i64;
    // X = M · 3^d with p − d even and the root at least N81 + 2 trits long
    let t = trit_len(U256::from(m)) as i64;
    let want = 2 * (N81 as i64 + 2) - t + 1;
    let d = if (p - want).rem_euclid(2) == 0 { want } else { want + 1 };
    if !(0..=161).contains(&d) {
        return None;
    }
    let x = U256::from(m).checked_mul(p3(d as u32))?;
    if bits(x) > A_LIMIT_BITS - 4 {
        return None;
    }
    let root = isqrt(x);
    let tr = trit_len(root);
    let e = (p - d) / 2 + tr as i64;
    if e <= E_MIN + 1 || e >= E_MAX - 1 || tr < N81 + 1 {
        return None;
    }
    let s = tr - N81;
    let unit = p3(s);
    let (mut q, _) = round_div(root, unit, root * root != x);
    // sqrt(x) must lie in ((q − ½)·3^s, (q + ½)·3^s): compare squares of
    // the doubled bounds with 4x
    let four_x = x << 2u32;
    let two = U256::from(2u32);
    while (two * q + U256::ONE) * unit * ((two * q + U256::ONE) * unit) <= four_x {
        q += U256::ONE;
    }
    while q > U256::ZERO && (two * q - U256::ONE) * unit * ((two * q - U256::ONE) * unit) > four_x {
        q -= U256::ONE;
    }
    let exact = q * unit * (q * unit) == x;
    let max = U256::from(crate::M_MAX as u128);
    Some((Real { e, m: q.min(max).as_i128() }, if exact { 0 } else { IX }))
}

/// An integer as a real81 (always exact).
pub(crate) fn from_i64(v: i64) -> Option<Real> {
    if v == 0 {
        return Some(Real::ZERO);
    }
    // t trits (|v| ≤ (3^t − 1)/2, t ≤ 40 < 72): exactly M = v · 3^(72 − t), E = t
    let a = v.unsigned_abs() as i128;
    let t = (1..=40).find(|&t| a <= (POW3_128[t] - 1) / 2)?;
    Some(Real { e: t as i64, m: v as i128 * POW3_128[N81 as usize - t] })
}

/// v · 3^k for a finite non-zero value (also renormalizes, k = 0).
pub(crate) fn scale(a: Real, k: i64) -> Option<(Real, u8)> {
    round_int(I256::from(a.m), a.e - N81 as i64 + k, false, N81)
}

/// Round to `n` significand trits (real27: n = 18).
pub(crate) fn narrow(a: Real, n: u32) -> Option<(Real, u8)> {
    round_int(I256::from(a.m), a.e - N81 as i64, false, n)
}

/// The exponent written normalized, for a finite non-zero value.
pub(crate) fn exponent(a: Real) -> i64 {
    a.e - N81 as i64 + trit_len(U256::from(a.m.unsigned_abs())) as i64
}

/// Round to an integer value (to nearest, ties to even, or toward zero).
pub(crate) fn round_to_int(a: Real, toward_zero: bool) -> Option<(Real, u8)> {
    let p = a.e - N81 as i64;
    if p >= 0 {
        return round_int(I256::from(a.m), p, false, N81);
    }
    if -p > 161 {
        return None;
    }
    let mag = U256::from(a.m.unsigned_abs());
    let unit = p3((-p) as u32);
    let (q, inexact) = if toward_zero {
        let q = mag / unit;
        (q, q * unit != mag)
    } else {
        round_div(mag, unit, false)
    };
    let q = if a.m < 0 { -q.as_i256() } else { q.as_i256() };
    let (v, _) = round_int(q, 0, false, N81)?;
    Some((v, if inexact { IX } else { 0 }))
}

/// Round to the nearest integer, ties to even, for a finite non-zero value:
/// `Some(Some(v))` when it fits a word, `Some(None)` when it is out of range.
pub(crate) fn to_int(a: Real, word_max: i64) -> Option<(Option<i64>, bool)> {
    let p = a.e - N81 as i64;
    let mag = U256::from(a.m.unsigned_abs());
    let (q, inexact) = if p >= 0 {
        if p > 40 {
            return Some((None, false)); // |v| ≥ 3^40 · 1/2 is far beyond a word
        }
        (mag.checked_mul(p3(p as u32))?, false)
    } else if -p > 161 {
        (U256::ZERO, true) // below a half
    } else {
        round_div(mag, p3((-p) as u32), false)
    };
    if q > U256::from(word_max as u128) {
        return Some((None, inexact));
    }
    let v = q.as_i128() as i64;
    Some((Some(if a.m < 0 { -v } else { v }), inexact))
}

/// Order of two finite values (the caller handles NaN, ∞ and zero), when
/// their exponents are close enough for 256 bits.
pub(crate) fn cmp(a: Real, b: Real) -> Option<Ordering> {
    let (sa, sb) = (a.m.signum(), b.m.signum());
    if sa != sb {
        return Some(sa.cmp(&sb));
    }
    // close exponents: scale the one with the larger up, in i128
    // (|M| < 2^114, 3^8 < 2^13)
    let d = a.e - b.e;
    if d.abs() <= 8 {
        let (x, y) = if d >= 0 { (a.m * POW3_128[d as usize], b.m) } else { (a.m, b.m * POW3_128[(-d) as usize]) };
        return Some(x.cmp(&y));
    }
    let (pa, pb) = (a.e - N81 as i64, b.e - N81 as i64);
    if (pa - pb).abs() > 80 {
        return None;
    }
    let p = pa.min(pb);
    Some(scaled(a.m, (pa - p) as u32)?.cmp(&scaled(b.m, (pb - p) as u32)?))
}

#[cfg(test)]
mod timing {
    use super::*;
    use std::hint::black_box;
    use std::time::Instant;

    fn t(name: &str, n: u32, mut f: impl FnMut(u32)) {
        let s = Instant::now();
        for k in 0..n {
            f(k);
        }
        println!("{name:<12} {:.1} ns", s.elapsed().as_nanos() as f64 / n as f64);
    }

    #[test]
    #[ignore]
    fn pieces() {
        let n = 300_000;
        let a = U256::from(crate::M_MAX as u128) * 3u128;
        t("pow3_256", n, |k| { black_box(p3(black_box(k % 100))); });
        t("trit_len", n, |k| { black_box(trit_len(black_box(a + k as u128))); });
        t("scaled", n, |k| { black_box(scaled(black_box(crate::M_MAX - k as i128), 3)); });
        t("i256 mul", n, |k| { black_box(I256::from(black_box(crate::M_MAX - k as i128)).checked_mul(I256::from(1_000_003i128))); });
        t("round_div/3", n, |k| { black_box(round_div(black_box(a + k as u128), U256::from(3u32), false)); });
        t("round_int", n, |k| { black_box(round_int(black_box(I256::from(crate::M_MAX * 3 - k as i128)), 0, false, N81)); });
        let x = Real { e: 3, m: crate::M_MAX - 5 };
        let y = Real { e: 2, m: crate::M_MAX / 2 + 7 };
        t("fast add", n, |_| { black_box(add(black_box(x), black_box(y))); });
        t("Real add", n, |_| { black_box(black_box(x).add(black_box(y))); });
        t("kind", n, |_| { black_box(black_box(x).kind()); });
    }
}
