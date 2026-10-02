//! Setun-26 floating point (ISA §14): real27 and real81.
//!
//! A value is `m · 3^E` with a balanced fraction `m = M · 3^−n`: the
//! significand M carries the sign, so there is no sign trit and no −0. Both
//! formats share a one-tryte exponent; real81 has n = 72 significand trits,
//! real27 n = 18. A [`Real`] always holds the real81 fields; a real27 value
//! is a `Real` whose low 54 trits are zero.
//!
//! Every operation computes its exact result as a big rational and rounds it
//! once, to nearest. Balanced ternary needs no tie rule for sums and products
//! (dropping trits is rounding to nearest, and their exact values have only
//! 3s in the denominator), but quotients, square roots and decimal
//! conversions can land exactly halfway; those ties go to the even M.

// add, sub, mul… also return the exception flags, so they are not the
// operator traits.
#![allow(clippy::should_implement_trait)]

mod fast;

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};
use std::cmp::Ordering;
use std::sync::OnceLock;

/// Significand trits of real81 and real27.
pub const N81: u32 = 72;
pub const N27: u32 = 18;
/// Exponent range: one balanced tryte. E_MAX marks ∞ and NaN.
pub const E_MAX: i64 = 9841;
pub const E_MIN: i64 = -9841;
/// The largest word, for conversions to integer.
pub const WORD_MAX: i64 = 3_812_798_742_493;

/// Exception flags (FSR order: IV DZ OF UF IX).
pub const IV: u8 = 1;
pub const DZ: u8 = 2;
pub const OF: u8 = 4;
pub const UF: u8 = 8;
pub const IX: u8 = 16;

const fn pow3(n: u32) -> i128 {
    let mut v: i128 = 1;
    let mut k = 0;
    while k < n {
        v *= 3;
        k += 1;
    }
    v
}

/// 3^71: the top trit of a real81 significand, and ±∞'s M.
pub const TOP: i128 = pow3(N81 - 1);
/// The largest real81 significand, (3^72 − 1) / 2.
pub const M_MAX: i128 = (pow3(N81) - 1) / 2;
/// Significands with |M| above this have a non-zero top trit (normalized).
const M_NORM: i128 = (TOP - 1) / 2;

/// The kinds of value (ISA §14.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Zero,
    Normal,
    Subnormal,
    Inf,
    NaN,
}

/// An FPU value: the exponent and 72-trit significand of a real81.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Real {
    pub e: i64,
    pub m: i128,
}

fn big_pow3(n: u32) -> BigInt {
    static CACHE: OnceLock<Vec<BigInt>> = OnceLock::new();
    let c = CACHE.get_or_init(|| {
        let mut v = Vec::with_capacity(400);
        let mut p = BigInt::one();
        for _ in 0..400 {
            v.push(p.clone());
            p *= 3;
        }
        v
    });
    if (n as usize) < c.len() {
        c[n as usize].clone()
    } else {
        num_traits::pow(BigInt::from(3), n as usize)
    }
}

/// Round num/den to the nearest integer, ties to even. `sticky` says the
/// true value is a little larger in magnitude than num/den. Returns the
/// integer and whether it is inexact.
fn round_div(num: &BigInt, den: &BigInt, sticky: bool) -> (BigInt, bool) {
    let neg = num.is_negative();
    let a = num.abs();
    let (q, r): (BigInt, BigInt) = a.div_rem(den);
    let twice: BigInt = &r * 2;
    let up = match twice.cmp(den) {
        Ordering::Greater => true,
        Ordering::Less => false,
        Ordering::Equal => sticky || q.is_odd(),
    };
    let q = if up { q + 1 } else { q };
    let inexact = !r.is_zero() || sticky;
    (if neg { -q } else { q }, inexact)
}

/// Number of trits of |v| (0 for 0): the smallest t with |v| ≤ (3^t − 1)/2.
fn trit_len(v: &BigInt) -> u32 {
    let a = v.abs();
    if a.is_zero() {
        return 0;
    }
    // (3^t - 1)/2 >= a  <=>  3^t >= 2a + 1
    let target: BigInt = a * 2 + 1;
    let mut t = ((target.bits() as f64) / 3f64.log2()) as u32;
    t = t.saturating_sub(1);
    while big_pow3(t) < target {
        t += 1;
    }
    t
}

impl Real {
    pub const ZERO: Real = Real { e: 0, m: 0 };
    pub const NAN: Real = Real { e: E_MAX, m: 0 };
    pub const INF: Real = Real { e: E_MAX, m: TOP };
    pub const NEG_INF: Real = Real { e: E_MAX, m: -TOP };

    pub fn kind(self) -> Kind {
        if self.e == E_MAX {
            return if self.m == TOP || self.m == -TOP { Kind::Inf } else { Kind::NaN };
        }
        if self.m == 0 {
            Kind::Zero
        } else if self.m.abs() <= M_NORM && self.e == E_MIN {
            Kind::Subnormal
        } else {
            Kind::Normal // an unnormalized encoding with E > E_MIN reads by its value
        }
    }

    pub fn is_nan(self) -> bool {
        self.kind() == Kind::NaN
    }

    /// −1, 0 or +1 (0 for zero and NaN).
    pub fn signum(self) -> i64 {
        if self.is_nan() {
            0
        } else {
            self.m.signum() as i64
        }
    }

    /// The exact value of a finite real as (M, p): M · 3^p.
    fn exact(self) -> (BigInt, i64) {
        (BigInt::from(self.m), self.e - N81 as i64)
    }

    /// Round num/den · 3^p to a value with n significand trits (stored as a
    /// real81). `sticky`: the true magnitude is a little more than that.
    fn round(num: &BigInt, den: &BigInt, p: i64, sticky: bool, n: u32) -> (Real, u8) {
        if num.is_zero() && !sticky {
            return (Real::ZERO, 0);
        }
        let n_i = n as i64;
        let max = (pow3(n) - 1) / 2;
        let norm = (pow3(n - 1) - 1) / 2;
        // The exponent comes from the exact value: the smallest E with
        // |v| ≤ 3^E / 2 = (max + ½) · 3^(E−n). Then rounding at E cannot
        // overflow the significand (a tie at max + ½ keeps max: the next
        // value up, at E + 1, is three times as far), and cannot leave it
        // unnormalized, since at E − 1 the value did not fit.
        let a = num.abs();
        let fits = |e: i64| -> bool {
            // 2|num| · 3^p ≤ den · 3^e, with sticky meaning "a little more"
            let k = p - e;
            let two_a: BigInt = &a * 2;
            let ord = if k >= 0 {
                let lhs: BigInt = two_a * big_pow3(k as u32);
                lhs.cmp(den)
            } else {
                let rhs: BigInt = den * big_pow3((-k) as u32);
                two_a.cmp(&rhs)
            };
            ord == Ordering::Less || (ord == Ordering::Equal && !sticky)
        };
        let mut e = p + trit_len(&a) as i64 - trit_len(den) as i64 + 1;
        while !fits(e) {
            e += 1;
        }
        while e > E_MIN && fits(e - 1) {
            e -= 1;
        }
        if e >= E_MAX {
            let inf = if num.is_negative() { Real::NEG_INF } else { Real::INF };
            return (inf, OF | IX);
        }
        let e = e.max(E_MIN);
        // M = round(num/den · 3^(p + n − e))
        let k = p + n_i - e;
        let (mut q, inexact) = if k >= 0 {
            round_div(&(num * big_pow3(k as u32)), den, sticky)
        } else {
            round_div(num, &(den * big_pow3((-k) as u32)), sticky)
        };
        if q.abs() > BigInt::from(max) {
            q = BigInt::from(max) * q.signum(); // the tie at the top of the range
        }
        let mut flags = if inexact { IX } else { 0 };
        let mag = q.abs();
        if mag.is_zero() {
            return (Real::ZERO, flags | if inexact { UF } else { 0 });
        }
        if e == E_MIN && mag <= BigInt::from(norm) && inexact {
            flags |= UF;
        }
        (Real { e, m: q.to_i128().unwrap() * pow3(N81 - n) }, flags)
    }

    fn round81(num: &BigInt, den: &BigInt, p: i64, sticky: bool) -> (Real, u8) {
        Real::round(num, den, p, sticky, N81)
    }

    /// Integers are exact (a word has 27 trits, a real81 72).
    pub fn from_i64(v: i64) -> Real {
        fast::from_i64(v).unwrap_or_else(|| Real::round81(&BigInt::from(v), &BigInt::one(), 0, false).0)
    }

    /// Round to the nearest integer word, ties to even. NaN gives 0 and IV;
    /// out of range (or ±∞) gives the largest word of the right sign and IV.
    pub fn to_i64(self) -> (i64, u8) {
        match self.kind() {
            Kind::NaN => return (0, IV),
            Kind::Inf => return (WORD_MAX * self.m.signum() as i64, IV),
            Kind::Zero => return (0, 0),
            _ => {}
        }
        if let Some((v, inexact)) = fast::to_int(self, WORD_MAX) {
            return match v {
                Some(v) => (v, if inexact { IX } else { 0 }),
                None => (WORD_MAX * self.m.signum() as i64, IV),
            };
        }
        let (m, p) = self.exact();
        let (q, inexact) =
            if p >= 0 { (m * big_pow3(p as u32), false) } else { round_div(&m, &big_pow3((-p) as u32), false) };
        match q.to_i64() {
            Some(v) if v.abs() <= WORD_MAX => (v, if inexact { IX } else { 0 }),
            _ => (WORD_MAX * q.signum().to_i64().unwrap(), IV),
        }
    }

    // ---- arithmetic -----------------------------------------------------------

    fn nan_of(a: Real, b: Real) -> Option<Real> {
        if a.is_nan() {
            Some(a)
        } else if b.is_nan() {
            Some(b)
        } else {
            None
        }
    }

    pub fn neg(self) -> Real {
        if self.kind() == Kind::Zero {
            Real::ZERO
        } else {
            Real { e: self.e, m: -self.m }
        }
    }

    pub fn abs(self) -> Real {
        if self.m < 0 && !self.is_nan() {
            self.neg()
        } else {
            self
        }
    }

    pub fn add(self, b: Real) -> (Real, u8) {
        self.add_with(b, true)
    }

    /// `add`, optionally without the fixed-width fast path (for tests).
    pub fn add_with(self, b: Real, use_fast: bool) -> (Real, u8) {
        if let Some(n) = Real::nan_of(self, b) {
            return (n, 0);
        }
        match (self.kind(), b.kind()) {
            (Kind::Inf, Kind::Inf) => {
                return if self.m == b.m { (self, 0) } else { (Real::NAN, IV) };
            }
            (Kind::Inf, _) => return (self, 0),
            (_, Kind::Inf) => return (b, 0),
            (Kind::Zero, Kind::Zero) => return (Real::ZERO, 0),
            (Kind::Zero, _) => return Real::renorm(b),
            (_, Kind::Zero) => return Real::renorm(self),
            _ => {}
        }
        if use_fast {
            if let Some(r) = fast::add(self, b) {
                return r;
            }
        }
        let (ma, pa) = self.exact();
        let (mb, pb) = b.exact();
        // far apart: the smaller is below half an ulp of the larger
        if (pa - pb).abs() > 2 * N81 as i64 + 6 {
            let (big, small) = if pa > pb { (self, b) } else { (b, self) };
            let (m, p) = big.exact();
            // nudge by the sign of the small one: exact enough to round right
            let num = m * 3 + BigInt::from(small.m.signum());
            let (r, f) = Real::round81(&num, &BigInt::one(), p - 1, false);
            return (r, f | IX);
        }
        let p = pa.min(pb);
        let num = ma * big_pow3((pa - p) as u32) + mb * big_pow3((pb - p) as u32);
        Real::round81(&num, &BigInt::one(), p, false)
    }

    fn renorm(a: Real) -> (Real, u8) {
        if let Some(r) = fast::scale(a, 0) {
            return r;
        }
        let (m, p) = a.exact();
        Real::round81(&m, &BigInt::one(), p, false)
    }

    pub fn sub(self, b: Real) -> (Real, u8) {
        if b.is_nan() && !self.is_nan() {
            return (b, 0);
        }
        self.add(b.neg())
    }

    pub fn mul(self, b: Real) -> (Real, u8) {
        self.mul_with(b, true)
    }

    /// `mul`, optionally without the fast path.
    pub fn mul_with(self, b: Real, use_fast: bool) -> (Real, u8) {
        if let Some(n) = Real::nan_of(self, b) {
            return (n, 0);
        }
        let sign = self.m.signum() * b.m.signum();
        match (self.kind(), b.kind()) {
            (Kind::Inf, Kind::Zero) | (Kind::Zero, Kind::Inf) => return (Real::NAN, IV),
            (Kind::Inf, _) | (_, Kind::Inf) => return (if sign < 0 { Real::NEG_INF } else { Real::INF }, 0),
            (Kind::Zero, _) | (_, Kind::Zero) => return (Real::ZERO, 0),
            _ => {}
        }
        if use_fast {
            if let Some(r) = fast::mul(self, b) {
                return r;
            }
        }
        let (ma, pa) = self.exact();
        let (mb, pb) = b.exact();
        Real::round81(&(ma * mb), &BigInt::one(), pa + pb, false)
    }

    pub fn div(self, b: Real) -> (Real, u8) {
        self.div_with(b, true)
    }

    /// `div`, optionally without the fast path.
    pub fn div_with(self, b: Real, use_fast: bool) -> (Real, u8) {
        if let Some(n) = Real::nan_of(self, b) {
            return (n, 0);
        }
        let sign = self.m.signum() * b.m.signum();
        match (self.kind(), b.kind()) {
            (Kind::Inf, Kind::Inf) | (Kind::Zero, Kind::Zero) => return (Real::NAN, IV),
            (Kind::Inf, _) => return (if sign < 0 { Real::NEG_INF } else { Real::INF }, 0),
            (_, Kind::Inf) => return (Real::ZERO, 0),
            (Kind::Zero, _) => return (Real::ZERO, 0),
            (_, Kind::Zero) => return (if self.m < 0 { Real::NEG_INF } else { Real::INF }, DZ),
            _ => {}
        }
        if use_fast {
            if let Some(r) = fast::div(self, b) {
                return r;
            }
        }
        let (ma, pa) = self.exact();
        let (mb, pb) = b.exact();
        let (num, den) = if mb.is_negative() { (-ma, -mb) } else { (ma, mb) };
        Real::round81(&num, &den, pa - pb, false)
    }

    /// The IEEE remainder: a − b·q with q = a/b rounded to nearest, ties to
    /// even. Always exact.
    pub fn rem(self, b: Real) -> (Real, u8) {
        if let Some(n) = Real::nan_of(self, b) {
            return (n, 0);
        }
        match (self.kind(), b.kind()) {
            (Kind::Inf, _) | (_, Kind::Zero) => return (Real::NAN, IV),
            (_, Kind::Inf) => return Real::renorm(self),
            (Kind::Zero, _) => return (Real::ZERO, 0),
            _ => {}
        }
        let (ma, pa) = self.exact();
        let (mb, pb) = b.exact();
        let p = pa.min(pb);
        let a_int = &ma * big_pow3((pa - p) as u32);
        let b_int = &mb * big_pow3((pb - p) as u32);
        let (num, den) = if b_int.is_negative() { (-a_int.clone(), -b_int.clone()) } else { (a_int.clone(), b_int.clone()) };
        let (q, _) = round_div(&num, &den, false);
        let r = a_int - q * b_int;
        let (v, f) = Real::round81(&r, &BigInt::one(), p, false);
        (v, f & !IX) // exact by construction
    }

    pub fn sqrt(self) -> (Real, u8) {
        self.sqrt_with(true)
    }

    /// `sqrt`, optionally without the fast path.
    pub fn sqrt_with(self, use_fast: bool) -> (Real, u8) {
        match self.kind() {
            Kind::NaN | Kind::Zero => return (self, 0),
            Kind::Inf => return if self.m > 0 { (self, 0) } else { (Real::NAN, IV) },
            _ => {}
        }
        if self.m < 0 {
            return (Real::NAN, IV);
        }
        if use_fast {
            if let Some(r) = fast::sqrt(self) {
                return r;
            }
        }
        let (mut m, mut p) = self.exact();
        // normalize subnormals so the root has enough trits
        let lz = N81 as i64 - trit_len(&m) as i64;
        if lz > 0 {
            m *= big_pow3(lz as u32);
            p -= lz;
        }
        // X = M · 3^d with d ≥ 2n − 72 + 8 and p − d even
        let s = (p - 2 * N81 as i64 - 8).div_euclid(2);
        let d = p - 2 * s;
        let x = m * big_pow3(d as u32);
        let root = x.sqrt();
        let sticky = &root * &root != x;
        Real::round81(&root, &BigInt::one(), s, sticky)
    }

    pub fn cmp(self, b: Real) -> Option<Ordering> {
        if self.is_nan() || b.is_nan() {
            return None;
        }
        let (ka, kb) = (self.kind(), b.kind());
        if ka == Kind::Inf || kb == Kind::Inf || ka == Kind::Zero || kb == Kind::Zero {
            let rank = |r: Real| -> i128 {
                match r.kind() {
                    Kind::Inf => 2 * r.m.signum(),
                    Kind::Zero => 0,
                    _ => r.m.signum(),
                }
            };
            let (x, y) = (rank(self), rank(b));
            if x != y || x.abs() != 1 {
                return Some(x.cmp(&y));
            }
        }
        if let Some(o) = fast::cmp(self, b) {
            return Some(o);
        }
        let (ma, pa) = self.exact();
        let (mb, pb) = b.exact();
        let p = pa.min(pb);
        // far apart and same sign: the exponents decide
        if (pa - pb).abs() > 2 * N81 as i64 + 6 && ma.signum() == mb.signum() {
            let bigger_mag = if pa > pb { Ordering::Greater } else { Ordering::Less };
            return Some(if ma.is_negative() { bigger_mag.reverse() } else { bigger_mag });
        }
        let x = ma * big_pow3((pa - p) as u32);
        let y = mb * big_pow3((pb - p) as u32);
        Some(x.cmp(&y))
    }

    pub fn min(self, b: Real) -> Real {
        match (self.is_nan(), b.is_nan()) {
            (true, true) => self,
            (true, false) => b,
            (false, true) => self,
            _ => if self.cmp(b) == Some(Ordering::Greater) { b } else { self },
        }
    }

    pub fn max(self, b: Real) -> Real {
        match (self.is_nan(), b.is_nan()) {
            (true, true) => self,
            (true, false) => b,
            (false, true) => self,
            _ => if self.cmp(b) == Some(Ordering::Less) { b } else { self },
        }
    }

    /// Round to an integer value: to nearest (ties to even), or toward zero.
    pub fn round_int(self, toward_zero: bool) -> (Real, u8) {
        if matches!(self.kind(), Kind::NaN | Kind::Inf | Kind::Zero) {
            return (self, 0);
        }
        if let Some(r) = fast::round_to_int(self, toward_zero) {
            return r;
        }
        let (m, p) = self.exact();
        if p >= 0 {
            return Real::renorm(self);
        }
        let den = big_pow3((-p) as u32);
        let (q, inexact) = if toward_zero {
            let (q, r) = m.div_rem(&den); // truncates toward zero
            (q, !r.is_zero())
        } else {
            round_div(&m, &den, false)
        };
        let (v, _) = Real::round81(&q, &BigInt::one(), 0, false);
        (v, if inexact { IX } else { 0 })
    }

    /// self · 3^k.
    pub fn scale(self, k: i64) -> (Real, u8) {
        if matches!(self.kind(), Kind::NaN | Kind::Inf | Kind::Zero) {
            return (self, 0);
        }
        let k = k.clamp(-3 * E_MAX, 3 * E_MAX);
        if let Some(r) = fast::scale(self, k) {
            return r;
        }
        let (m, p) = self.exact();
        Real::round81(&m, &BigInt::one(), p + k, false)
    }

    /// The exponent of the value written normalized (for subnormals, below
    /// E_MIN). Zero gives 0; ∞ and NaN give E_MAX and IV.
    pub fn exponent(self) -> (i64, u8) {
        match self.kind() {
            Kind::Zero => (0, 0),
            Kind::Inf | Kind::NaN => (E_MAX, IV),
            _ => (fast::exponent(self), 0),
        }
    }

    /// FCLASS: 0 zero, ±1 subnormal, ±2 normal, ±3 infinite, 4 NaN.
    pub fn class(self) -> i64 {
        let s = self.m.signum() as i64;
        match self.kind() {
            Kind::Zero => 0,
            Kind::Subnormal => s,
            Kind::Normal => 2 * s,
            Kind::Inf => 3 * s,
            Kind::NaN => 4,
        }
    }

    // ---- real27 ---------------------------------------------------------------

    /// Round to real27 precision (18 trits), still as a Real.
    pub fn to_real27(self) -> (Real, u8) {
        match self.kind() {
            Kind::Zero | Kind::Inf => return (self, 0),
            Kind::NaN => {
                // keep the payload's top trits, unless they would read as ∞
                let top = self.m / pow3(N81 - N27);
                let top = if top.abs() == pow3(N27 - 1) { 0 } else { top };
                return (Real { e: E_MAX, m: top * pow3(N81 - N27) }, 0);
            }
            _ => {}
        }
        if let Some(r) = fast::narrow(self, N27) {
            return r;
        }
        let (m, p) = self.exact();
        Real::round(&m, &BigInt::one(), p, false, N27)
    }

    // ---- encodings ------------------------------------------------------------

    /// The three words of a real81, most significant first.
    pub fn to_words(self) -> [i64; 3] {
        let (rest, lo) = split27(self.m);
        let (hi, mid) = split27(rest);
        [(self.e as i128 * pow3(18) + hi) as i64, mid, lo]
    }

    pub fn from_words(w: [i64; 3]) -> Real {
        let h = 193_710_244; // (3^18 − 1) / 2
        let hi = (w[0] + h).rem_euclid(387_420_489) - h;
        let e = (w[0] - hi) / 387_420_489;
        Real { e, m: (hi as i128 * pow3(27) + w[1] as i128) * pow3(27) + w[2] as i128 }
    }

    /// The real27 word for this value (rounded to 18 trits).
    pub fn to_word27(self) -> (i64, u8) {
        let (r, f) = self.to_real27();
        (r.to_words()[0], f)
    }

    pub fn from_word27(w: i64) -> Real {
        Real::from_words([w, 0, 0])
    }

    // ---- decimal ----------------------------------------------------------------

    /// Parse a decimal number: `[-]digits[.digits][e[-]digits]`, `_` allowed
    /// between digits. Correctly rounded (ties to even).
    pub fn from_decimal(s: &str) -> Result<(Real, u8), String> {
        let t: String = s.chars().filter(|&c| c != '_').collect();
        let (neg, body) = match t.strip_prefix('-') {
            Some(b) => (true, b),
            None => (false, t.strip_prefix('+').unwrap_or(&t)),
        };
        let (mant, exp) = match body.find(['e', 'E']) {
            Some(i) => (&body[..i], &body[i + 1..]),
            None => (body, "0"),
        };
        let exp10: i64 = exp.parse().map_err(|_| format!("bad exponent in {s:?}"))?;
        let (int, frac) = match mant.find('.') {
            Some(i) => (&mant[..i], &mant[i + 1..]),
            None => (mant, ""),
        };
        if int.is_empty() && frac.is_empty() {
            return Err(format!("no digits in {s:?}"));
        }
        if !int.chars().chain(frac.chars()).all(|c| c.is_ascii_digit()) {
            return Err(format!("bad number {s:?}"));
        }
        let digits = format!("{int}{frac}");
        let mut num: BigInt = digits.parse().unwrap_or_default();
        if neg {
            num = -num;
        }
        let e10 = exp10 - frac.len() as i64;
        if e10.abs() > 20_000 {
            // beyond any real: overflow or underflow
            return Ok(if num.is_zero() {
                (Real::ZERO, 0)
            } else if e10 > 0 {
                (if neg { Real::NEG_INF } else { Real::INF }, OF | IX)
            } else {
                (Real::ZERO, UF | IX)
            });
        }
        let ten = BigInt::from(10);
        let (num, den) = if e10 >= 0 {
            (num * num_traits::pow(ten, e10 as usize), BigInt::one())
        } else {
            (num, num_traits::pow(ten, (-e10) as usize))
        };
        Ok(Real::round81(&num, &den, 0, false))
    }

    /// Decimal text with `digits` significant digits (rounded to nearest),
    /// in scientific notation when very large or small. ∞ and NaN spell out.
    pub fn to_decimal(self, digits: usize) -> String {
        match self.kind() {
            Kind::NaN => return "nan".into(),
            Kind::Inf => return if self.m > 0 { "inf".into() } else { "-inf".into() },
            Kind::Zero => return "0".into(),
            _ => {}
        }
        let digits = digits.max(1);
        let (m, p) = self.exact();
        let neg = m.is_negative();
        let (num, den) = if p >= 0 { (m.abs() * big_pow3(p as u32), BigInt::one()) } else { (m.abs(), big_pow3((-p) as u32)) };
        // the decimal exponent: 10^x ≤ v < 10^(x+1)
        let est = (num.bits() as f64 - den.bits() as f64) * std::f64::consts::LOG10_2;
        let mut x = est.floor() as i64 - 1;
        let ten = BigInt::from(10);
        let pow10 = |k: i64| num_traits::pow(ten.clone(), k.unsigned_abs() as usize);
        let ge = |k: i64| -> bool {
            // v >= 10^k ?
            if k >= 0 { num >= &den * pow10(k) } else { &num * pow10(k) >= den }
        };
        while ge(x + 1) {
            x += 1;
        }
        while !ge(x) {
            x -= 1;
        }
        // q = round(v · 10^(digits−1−x))
        let sh = digits as i64 - 1 - x;
        let (n2, d2) = if sh >= 0 { (&num * pow10(sh), den.clone()) } else { (num.clone(), &den * pow10(sh)) };
        let (mut q, _) = round_div(&n2, &d2, false);
        if q >= pow10(digits as i64) {
            q /= 10;
            x += 1;
        }
        let s = q.to_string();
        let mut body = if (-5..15).contains(&x) {
            if x >= 0 {
                let x = x as usize;
                if s.len() > x + 1 { format!("{}.{}", &s[..=x], &s[x + 1..]) } else { format!("{s}{}", "0".repeat(x + 1 - s.len())) }
            } else {
                format!("0.{}{s}", "0".repeat((-x - 1) as usize))
            }
        } else {
            let (h, t) = s.split_at(1);
            if t.is_empty() { format!("{h}e{x}") } else { format!("{h}.{t}e{x}") }
        };
        if body.contains('.') && !body.contains('e') {
            while body.ends_with('0') {
                body.pop();
            }
            if body.ends_with('.') {
                body.pop();
            }
        } else if let Some(epos) = body.find('e') {
            let (mant, exp) = body.split_at(epos);
            let mut mant = mant.to_string();
            if mant.contains('.') {
                while mant.ends_with('0') {
                    mant.pop();
                }
                if mant.ends_with('.') {
                    mant.pop();
                }
            }
            body = format!("{mant}{exp}");
        }
        if neg { format!("-{body}") } else { body }
    }

    /// The nearest f64 (for tests and debugging).
    pub fn to_f64(self) -> f64 {
        self.to_decimal(20).parse().unwrap_or(f64::NAN)
    }

    // ---- constants ----------------------------------------------------------------

    /// FCONST k: 0 zero, 1 one, 2 π, 3 e, 4 ln 3, 5 log₃ e, 6 log₃ 10.
    pub fn constant(k: i64) -> Option<Real> {
        static C: OnceLock<[Real; 7]> = OnceLock::new();
        let c = C.get_or_init(|| {
            let s = big_pow3(SCALE);
            let pi = pi_fixed();
            let e = e_fixed();
            let ln3 = ln3_fixed();
            let ln10 = ln10_fixed();
            let at = |num: &BigInt, den: &BigInt| Real::round81(num, den, 0, false).0;
            [
                Real::ZERO,
                Real::from_i64(1),
                at(&pi, &s),
                at(&e, &s),
                at(&ln3, &s),
                at(&s, &ln3),
                at(&ln10, &ln3),
            ]
        });
        c.get(k as usize).copied().filter(|_| k >= 0)
    }
}

/// Balanced (v div m, v mod m).
/// 3^27, and its inverse modulo 2^128 (it is odd).
const C27: i128 = pow3(27);
const C27_INV: i128 = {
    let mut x: i128 = C27; // correct to 3 bits; each step doubles that
    let mut k = 0;
    while k < 7 {
        x = x.wrapping_mul(2i128.wrapping_sub(C27.wrapping_mul(x)));
        k += 1;
    }
    x
};

/// Split v into (v div 3^27, balanced remainder) without 128-bit division:
/// fold the bits down 62 at a time for the remainder (2^62 mod 3^27 is
/// small), then divide exactly by multiplying with 3^27's inverse.
fn split27(v: i128) -> (i128, i64) {
    const R62: i128 = (1i128 << 62) % C27;
    let mut t = v;
    while !(-(1i128 << 62)..(1i128 << 62)).contains(&t) {
        t = (t >> 62) * R62 + (t & ((1i128 << 62) - 1));
    }
    let h = (C27 as i64 - 1) / 2;
    let lo = ((t as i64) % C27 as i64 + C27 as i64 + h) % C27 as i64 - h;
    let q = (v - lo as i128).wrapping_mul(C27_INV);
    (q, lo)
}

#[allow(dead_code)]
fn bal_split(v: i128, m: i128) -> (i128, i128) {
    let h = (m - 1) / 2;
    let lo = (v + h).rem_euclid(m) - h;
    ((v - lo) / m, lo)
}

// ---- constants to 200 trits ------------------------------------------------------------

const SCALE: u32 = 200;

/// atan(1/x) · 3^SCALE
fn atan_inv(x: i64) -> BigInt {
    let s = big_pow3(SCALE);
    let x2 = BigInt::from(x * x);
    let mut power = s / x; // S / x^(2k+1)
    let mut sum = BigInt::zero();
    let mut k: i64 = 0;
    while !power.is_zero() {
        let term = &power / (2 * k + 1);
        if k % 2 == 0 {
            sum += term;
        } else {
            sum -= term;
        }
        power /= &x2;
        k += 1;
    }
    sum
}

/// atanh(1/x) · 3^SCALE
fn atanh_inv(x: i64) -> BigInt {
    let s = big_pow3(SCALE);
    let x2 = BigInt::from(x * x);
    let mut power = s / x;
    let mut sum = BigInt::zero();
    let mut k: i64 = 0;
    while !power.is_zero() {
        sum += &power / (2 * k + 1);
        power /= &x2;
        k += 1;
    }
    sum
}

fn pi_fixed() -> BigInt {
    (atan_inv(5) * 4 - atan_inv(239)) * 4
}

fn e_fixed() -> BigInt {
    let mut term = big_pow3(SCALE);
    let mut sum = BigInt::zero();
    let mut k = 1;
    while !term.is_zero() {
        sum += &term;
        term /= k;
        k += 1;
    }
    sum
}

fn ln3_fixed() -> BigInt {
    atanh_inv(2) * 2 // ln 3 = 2 atanh(1/2)
}

fn ln10_fixed() -> BigInt {
    ln3_fixed() * 2 + atanh_inv(19) * 2 // 10 = 9 · (1 + 1/19) / (1 − 1/19)
}

#[cfg(test)]
mod tests;
