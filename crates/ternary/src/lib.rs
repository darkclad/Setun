//! Balanced-ternary fixed-width integers.
//!
//! A balanced-ternary digit ("trit") is one of −1, 0, +1. An `N`-trit number
//! covers exactly the integers in `[-(3^N-1)/2, (3^N-1)/2]`, so we store it as
//! a plain `i64` and derive trits on demand. That keeps arithmetic cheap while
//! still letting the emulator address individual trits and trit fields.
//!
//! Conventions (see `docs/SETUN386_ISA.md` §1):
//! * trit positions are **1-based and most-significant first**: in a tryte
//!   `x[1:9]`, `x[1]` has weight 3^8 and `x[9]` has weight 3^0;
//! * text form writes trits as `+`, `0`, `-` (e.g. `+-0` = 6);
//! * base-27 form uses one digit per 3 trits: `0`, `A`..`M` = +1..+13,
//!   `a`..`m` = −1..−13 (negation swaps case; e.g. `MMM` = 9841).

use std::fmt;
use std::ops::{Neg, RangeInclusive};
use std::str::FromStr;

/// A single balanced-ternary digit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(i8)]
pub enum Trit {
    N = -1,
    Z = 0,
    P = 1,
}

impl Trit {
    pub const fn value(self) -> i8 {
        self as i8
    }

    pub const fn from_i64(v: i64) -> Trit {
        if v < 0 {
            Trit::N
        } else if v > 0 {
            Trit::P
        } else {
            Trit::Z
        }
    }

    pub const fn to_char(self) -> char {
        match self {
            Trit::N => '-',
            Trit::Z => '0',
            Trit::P => '+',
        }
    }

    pub fn from_char(c: char) -> Option<Trit> {
        match c {
            '-' | '−' | 'T' | 't' => Some(Trit::N),
            '0' => Some(Trit::Z),
            '+' | '1' => Some(Trit::P),
            _ => None,
        }
    }
}

impl Neg for Trit {
    type Output = Trit;
    fn neg(self) -> Trit {
        Trit::from_i64(-(self as i64))
    }
}

impl std::ops::Mul for Trit {
    type Output = Trit;
    fn mul(self, rhs: Trit) -> Trit {
        Trit::from_i64(self as i64 * rhs as i64)
    }
}

/// 3^n as i128 (n ≤ 80).
pub const fn pow3(n: u32) -> i128 {
    let mut r: i128 = 1;
    let mut i = 0;
    while i < n {
        r *= 3;
        i += 1;
    }
    r
}

/// Round `v / 3^k` to the nearest integer. In balanced ternary this is exactly
/// "drop the low k trits": ties are impossible because 3^k is odd.
pub fn div_pow3_round(v: i128, k: u32) -> i128 {
    let d = pow3(k);
    (v + (d - 1) / 2).div_euclid(d)
}

/// Wrap `v` into the balanced range of an `n`-trit number, i.e. keep the low
/// `n` trits and drop the rest.
pub fn wrap_trits(v: i128, n: u32) -> i128 {
    let m = pow3(n);
    let h = (m - 1) / 2;
    (v + h).rem_euclid(m) - h
}

/// An `N`-trit balanced-ternary integer (`N` ≤ 39 so it fits in an `i64`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Tern<const N: usize>(i64);

/// 3-trit field: a register index or one base-27 digit, −13..=13.
pub type Tri = Tern<3>;
/// 9-trit tryte: the smallest addressable unit, −9841..=9841.
pub type Tryte = Tern<9>;
/// 27-trit word: register, pointer and instruction unit (3 trytes).
pub type Word = Tern<27>;

impl<const N: usize> Tern<N> {
    const WIDTH_OK: () = assert!(N >= 1 && N <= 39, "Tern<N> supports 1..=39 trits");

    /// Number of trits.
    pub const WIDTH: usize = N;
    /// 3^N.
    pub const MODULUS: i128 = pow3(N as u32);
    /// Largest representable value, (3^N−1)/2.
    pub const MAX: Self = Tern(((pow3(N as u32) - 1) / 2) as i64);
    /// Smallest representable value, −(3^N−1)/2.
    pub const MIN: Self = Tern(-((pow3(N as u32) - 1) / 2) as i64);
    pub const ZERO: Self = Tern(0);

    /// Exact construction; `None` if `v` does not fit in `N` trits.
    pub const fn new(v: i64) -> Option<Self> {
        #[allow(clippy::let_unit_value)]
        let _ = Self::WIDTH_OK;
        if v >= Self::MIN.0 && v <= Self::MAX.0 {
            Some(Tern(v))
        } else {
            None
        }
    }

    /// Construction that keeps only the low `N` trits of `v`.
    pub fn wrap(v: i128) -> Self {
        #[allow(clippy::let_unit_value)]
        let _ = Self::WIDTH_OK;
        Tern(wrap_trits(v, N as u32) as i64)
    }

    pub const fn value(self) -> i64 {
        self.0
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// Sign as a trit (the value of the leading non-zero trit).
    pub const fn sign(self) -> Trit {
        Trit::from_i64(self.0)
    }

    /// All trits, most significant first (`[0]` is the spec's `x[1]`).
    pub fn trits(self) -> [Trit; N] {
        let mut out = [Trit::Z; N];
        let mut v = self.0;
        for slot in out.iter_mut().rev() {
            let r = v.rem_euclid(3);
            let t = if r == 2 { -1 } else { r };
            *slot = Trit::from_i64(t);
            v = (v - t) / 3;
        }
        debug_assert_eq!(v, 0);
        out
    }

    /// Build from trits, most significant first. Shorter input is
    /// zero-extended on the left; longer input keeps the low `N` trits.
    pub fn from_trits(trits: &[Trit]) -> Self {
        let v = trits.iter().fold(0i128, |acc, &t| acc * 3 + t as i128);
        Self::wrap(v)
    }

    /// Trit at 1-based, MSB-first position `i` (spec notation `x[i]`).
    pub fn trit(self, i: usize) -> Trit {
        assert!((1..=N).contains(&i), "trit index {i} out of 1..={N}");
        self.trits()[i - 1]
    }

    /// Value of the trit field `x[a:b]` (1-based, inclusive, MSB first),
    /// read as a balanced-ternary number.
    pub fn field(self, r: RangeInclusive<usize>) -> i64 {
        let (a, b) = (*r.start(), *r.end());
        assert!(1 <= a && a <= b && b <= N, "field {a}:{b} out of 1..={N}");
        self.trits()[a - 1..b]
            .iter()
            .fold(0i64, |acc, &t| acc * 3 + t as i64)
    }

    /// Replace the trit field `x[a:b]` with the low trits of `v`.
    pub fn with_field(self, r: RangeInclusive<usize>, v: i64) -> Self {
        let (a, b) = (*r.start(), *r.end());
        assert!(1 <= a && a <= b && b <= N, "field {a}:{b} out of 1..={N}");
        let width = b - a + 1;
        let shift = (N - b) as u32;
        let old = self.field(a..=b) as i128;
        let new = wrap_trits(v as i128, width as u32);
        Self::wrap(self.0 as i128 + (new - old) * pow3(shift))
    }

    /// Change width, keeping the low trits (widening never changes the value).
    pub fn resize<const M: usize>(self) -> Tern<M> {
        Tern::<M>::wrap(self.0 as i128)
    }

    pub fn wrapping_add(self, rhs: Self) -> Self {
        Self::wrap(self.0 as i128 + rhs.0 as i128)
    }

    pub fn wrapping_sub(self, rhs: Self) -> Self {
        Self::wrap(self.0 as i128 - rhs.0 as i128)
    }

    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        Self::new(self.0.checked_add(rhs.0)?)
    }

    /// Full-precision product.
    pub fn mul_wide(self, rhs: Self) -> i128 {
        self.0 as i128 * rhs.0 as i128
    }

    /// Multiply by 3^t: shift left for t > 0 (high trits fall off), right for
    /// t < 0 (low trits fall off, which rounds to nearest).
    pub fn shift(self, t: i32) -> Self {
        if t >= 0 {
            Self::wrap(self.0 as i128 * pow3(t as u32))
        } else {
            Self::wrap(div_pow3_round(self.0 as i128, (-t) as u32))
        }
    }

    /// Trit-by-trit product (`TMUL`, the Setun-70 `SMT`).
    pub fn tritwise_mul(self, rhs: Self) -> Self {
        self.tritwise(rhs, |a, b| a * b)
    }

    /// Trit-by-trit "threshold addition" (`TSAT`, the Setun-70 `SAT`): each
    /// result trit is the sign of the sum of the two input trits.
    pub fn tritwise_sat(self, rhs: Self) -> Self {
        self.tritwise(rhs, |a, b| Trit::from_i64(a as i64 + b as i64))
    }

    /// Trit-by-trit minimum (ternary "AND").
    pub fn tritwise_min(self, rhs: Self) -> Self {
        self.tritwise(rhs, |a, b| a.min(b))
    }

    /// Trit-by-trit maximum (ternary "OR").
    pub fn tritwise_max(self, rhs: Self) -> Self {
        self.tritwise(rhs, |a, b| a.max(b))
    }

    /// Trit-by-trit consensus: the common trit where both agree, else 0.
    pub fn tritwise_cons(self, rhs: Self) -> Self {
        self.tritwise(rhs, |a, b| if a == b { a } else { Trit::Z })
    }

    fn tritwise(self, rhs: Self, f: impl Fn(Trit, Trit) -> Trit) -> Self {
        let (a, b) = (self.trits(), rhs.trits());
        let mut out = [Trit::Z; N];
        for i in 0..N {
            out[i] = f(a[i], b[i]);
        }
        Self::from_trits(&out)
    }

    /// Number of leading zero trits (0..=N).
    pub fn leading_zero_trits(self) -> u32 {
        self.trits().iter().take_while(|&&t| t == Trit::Z).count() as u32
    }

    /// Base-27 text: one digit per 3 trits (`0`, `A`..`M` = +1..+13,
    /// `a`..`m` = −1..−13). A width that is not a multiple of 3 gets implicit
    /// leading zero trits.
    pub fn to_b27(self) -> String {
        let mut t: Vec<Trit> = self.trits().to_vec();
        while !t.len().is_multiple_of(3) {
            t.insert(0, Trit::Z);
        }
        t.chunks(3)
            .map(|d| b27_digit(d[0] as i64 * 9 + d[1] as i64 * 3 + d[2] as i64))
            .collect()
    }

    /// Parse base-27 text (see [`Tern::to_b27`]). `_` and whitespace are ignored.
    pub fn from_b27(s: &str) -> Result<Self, ParseError> {
        let mut v: i128 = 0;
        let mut digits = 0;
        for c in s.chars().filter(|c| !c.is_whitespace() && *c != '_') {
            v = v * 27 + b27_value(c).ok_or(ParseError::BadChar(c))? as i128;
            digits += 1;
            if v.abs() > Self::MAX.0 as i128 {
                return Err(ParseError::Overflow);
            }
        }
        if digits == 0 {
            return Err(ParseError::Empty);
        }
        Ok(Tern(v as i64))
    }
}

/// Split a full product into `(high, low)` halves of `N` trits each, so that
/// `p = high·3^N + low` with both halves balanced (`MULH` / `MUL`).
pub fn split_product<const N: usize>(p: i128) -> (Tern<N>, Tern<N>) {
    let lo = wrap_trits(p, N as u32);
    let hi = (p - lo) / pow3(N as u32);
    (Tern::<N>::wrap(hi), Tern::<N>(lo as i64))
}

fn b27_digit(v: i64) -> char {
    match v {
        0 => '0',
        1..=13 => (b'A' + (v - 1) as u8) as char,
        -13..=-1 => (b'a' + (-v - 1) as u8) as char,
        _ => unreachable!("base-27 digit out of range: {v}"),
    }
}

fn b27_value(c: char) -> Option<i64> {
    match c {
        '0' => Some(0),
        'A'..='M' => Some(c as i64 - 'A' as i64 + 1),
        'a'..='m' => Some(-(c as i64 - 'a' as i64 + 1)),
        _ => None,
    }
}

impl<const N: usize> Neg for Tern<N> {
    type Output = Self;
    /// Negation is exact: flip every trit.
    fn neg(self) -> Self {
        Tern(-self.0)
    }
}

impl<const N: usize> fmt::Display for Tern<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.trits().iter().try_for_each(|t| write!(f, "{}", t.to_char()))
    }
}

impl<const N: usize> fmt::Debug for Tern<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Tern<{N}>({} = {})", self.to_b27(), self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Empty,
    BadChar(char),
    Overflow,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Empty => write!(f, "empty ternary literal"),
            ParseError::BadChar(c) => write!(f, "invalid ternary digit {c:?}"),
            ParseError::Overflow => write!(f, "ternary literal does not fit"),
        }
    }
}

impl std::error::Error for ParseError {}

impl<const N: usize> FromStr for Tern<N> {
    type Err = ParseError;

    /// Parse trit text like `+-0` (MSB first). Up to `N` trits; shorter input
    /// is zero-extended on the left. `_` and whitespace are ignored.
    fn from_str(s: &str) -> Result<Self, ParseError> {
        let mut trits = Vec::new();
        for c in s.chars().filter(|c| !c.is_whitespace() && *c != '_') {
            trits.push(Trit::from_char(c).ok_or(ParseError::BadChar(c))?);
        }
        if trits.is_empty() {
            return Err(ParseError::Empty);
        }
        if trits.len() > N {
            return Err(ParseError::Overflow);
        }
        Ok(Self::from_trits(&trits))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t<const N: usize>(s: &str) -> Tern<N> {
        s.parse().unwrap()
    }

    #[test]
    fn ranges() {
        assert_eq!(Tri::MAX.value(), 13);
        assert_eq!(Tryte::MAX.value(), 9841);
        assert_eq!(Tryte::MIN.value(), -9841);
        assert_eq!(Word::MAX.value(), 3_812_798_742_493);
        assert!(Tryte::new(9842).is_none());
    }

    #[test]
    fn brusentsov_examples() {
        // From Brusentsov & Ramil Alvarez (2006), section 3.
        assert_eq!(t::<3>("+++").value(), 13);
        assert_eq!(t::<3>("+-+").value(), 7);
        assert_eq!(t::<3>("+-0").value(), 6);
        assert_eq!(t::<3>("-+0").value(), -6);
        assert_eq!((-t::<3>("+-+")).to_string(), "-+-");
    }

    #[test]
    fn every_tryte_roundtrips() {
        for v in -9841..=9841 {
            let x = Tryte::new(v).unwrap();
            assert_eq!(Tryte::from_trits(&x.trits()), x);
            assert_eq!(x.to_string().parse::<Tryte>().unwrap(), x);
            assert_eq!(Tryte::from_b27(&x.to_b27()).unwrap(), x);
            assert_eq!((-x).value(), -v);
        }
    }

    #[test]
    fn base27_notation() {
        assert_eq!(Tryte::new(1).unwrap().to_b27(), "00A");
        assert_eq!(Tryte::new(-1).unwrap().to_b27(), "00a");
        assert_eq!(Tryte::new(27).unwrap().to_b27(), "0A0");
        assert_eq!(Tryte::MAX.to_b27(), "MMM");
        assert_eq!(Tryte::MIN.to_b27(), "mmm");
        // Negation swaps case; a pixel's digits are R, G, B.
        let red = Tryte::from_b27("Mmm").unwrap();
        assert_eq!((-red).to_b27(), "mMM");
        assert_eq!(Word::from_b27("MMMMMMMMM").unwrap(), Word::MAX);
        assert_eq!(Tern::<4>::new(40).unwrap().to_b27(), "AM"); // 1·27 + 13
        assert!(Tryte::from_b27("AMMM").is_err());
        assert!(Tryte::from_b27("0N0").is_err());
    }

    #[test]
    fn product_split() {
        let cases = [
            (Word::MAX, Word::MAX),
            (Word::MIN, Word::MAX),
            (Word::new(12_345).unwrap(), Word::new(-678).unwrap()),
        ];
        for (a, b) in cases {
            let p = a.mul_wide(b);
            let (hi, lo) = split_product::<27>(p);
            assert_eq!(hi.value() as i128 * pow3(27) + lo.value() as i128, p);
        }
    }

    #[test]
    fn fields_are_one_based_msb_first() {
        let k = t::<9>("+-0+0-00+");
        assert_eq!(k.trit(1), Trit::P);
        assert_eq!(k.trit(6), Trit::N);
        assert_eq!(k.trit(9), Trit::P);
        assert_eq!(k.field(1..=2), 2); // "+-"
        assert_eq!(k.field(3..=6), t::<4>("0+0-").value());
        let k2 = k.with_field(1..=2, 0);
        assert_eq!(k2.to_string(), "000+0-00+");
        assert_eq!(k2.with_field(1..=2, 2), k);
    }

    #[test]
    fn wrap_and_shift() {
        // MAX + 1 wraps to MIN.
        assert_eq!(Tryte::MAX.wrapping_add(Tryte::new(1).unwrap()), Tryte::MIN);
        // Dropping low trits rounds to nearest.
        assert_eq!(t::<6>("000+-").shift(-1).to_string(), "00000+"); // 2 -> 1
        assert_eq!(t::<6>("0000+").shift(-1).value(), 0); // 1 -> 0
        assert_eq!(t::<6>("0000-+").shift(-1).value(), -1); // -2 -> -1
        assert_eq!(t::<6>("+00000").shift(1).value(), 0); // high trit falls off
        for v in -9841..=9841i64 {
            let x = Tryte::new(v).unwrap();
            let r = x.shift(-2).value() as f64;
            assert!((r - v as f64 / 9.0).abs() <= 0.5, "{v}");
        }
    }

    #[test]
    fn tritwise_ops() {
        let a = t::<6>("+0-+0-");
        let b = t::<6>("++--00");
        assert_eq!(a.tritwise_mul(b).to_string(), "+0+-00");
        assert_eq!(a.tritwise_sat(b).to_string(), "++-00-");
        assert_eq!(a.tritwise_min(b).to_string(), "+0--0-");
        assert_eq!(a.tritwise_max(b).to_string(), "++-+00");
        assert_eq!(a.tritwise_cons(b).to_string(), "+0-000");
        assert_eq!(t::<9>("000+-0000").leading_zero_trits(), 3);
        assert_eq!(Tryte::ZERO.leading_zero_trits(), 9);
    }

    #[test]
    fn resize_preserves_value() {
        let x = Tryte::new(-3000).unwrap();
        let w: Word = x.resize();
        assert_eq!(w.value(), -3000);
        assert_eq!(w.resize::<9>(), x);
    }
}
