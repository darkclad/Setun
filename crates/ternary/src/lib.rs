//! Balanced-ternary fixed-width integers.
//!
//! A balanced-ternary digit ("trit") is one of −1, 0, +1. An `N`-trit number
//! covers exactly the integers in `[-(3^N-1)/2, (3^N-1)/2]`, so we store it as
//! a plain `i64` and derive trits on demand. That keeps arithmetic cheap while
//! still letting the emulator address individual trits and trit fields the way
//! the 1970 Setun-70 ALGOL specification does.
//!
//! Conventions borrowed from the spec:
//! * trit positions are **1-based and most-significant first**: in a tryte
//!   `k[1:6]`, `k[1]` has weight 3^5 and `k[6]` has weight 3^0;
//! * text form writes trits as `+`, `0`, `-` (e.g. `+-0` = 6);
//! * nonary (base-9) form uses the Setun digits `W X Y Z 0 1 2 3 4` for −4..+4,
//!   one digit per trit pair (e.g. the opcode tryte `000---` is `0ZW`).

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

/// Page offset / 4-trit address, −40..=40.
pub type Addr = Tern<4>;
/// Page number field, −13..=13.
pub type PageNo = Tern<3>;
/// 6-trit tryte ("слог", syllable): the smallest addressable unit, −364..=364.
pub type Tryte = Tern<6>;
/// 18-trit machine word (three trytes): one stack cell.
pub type Word = Tern<18>;
/// 36-trit double word, e.g. the `S:Y` pair.
pub type DWord = Tern<36>;

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

    /// Change width, keeping the low trits (sign-correct in balanced ternary:
    /// widening never changes the value).
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

    /// Trit-by-trit product (Setun-70 `SMT`).
    pub fn tritwise_mul(self, rhs: Self) -> Self {
        let (a, b) = (self.trits(), rhs.trits());
        let mut out = [Trit::Z; N];
        for i in 0..N {
            out[i] = a[i] * b[i];
        }
        Self::from_trits(&out)
    }

    /// Trit-by-trit "threshold addition" (Setun-70 `SAT`): each result trit is
    /// the sign of the sum of the two input trits.
    pub fn tritwise_sat(self, rhs: Self) -> Self {
        let (a, b) = (self.trits(), rhs.trits());
        let mut out = [Trit::Z; N];
        for i in 0..N {
            out[i] = Trit::from_i64(a[i] as i64 + b[i] as i64);
        }
        Self::from_trits(&out)
    }

    /// Nonary text: one Setun digit (W X Y Z 0 1 2 3 4) per trit pair. An odd
    /// width gets an implicit leading zero trit.
    pub fn to_nonary(self) -> String {
        let mut t: Vec<Trit> = self.trits().to_vec();
        if N % 2 == 1 {
            t.insert(0, Trit::Z);
        }
        t.chunks(2)
            .map(|p| NONARY[(p[0] as i64 * 3 + p[1] as i64 + 4) as usize])
            .collect()
    }

    /// Parse nonary text (see [`Tern::to_nonary`]).
    pub fn from_nonary(s: &str) -> Result<Self, ParseError> {
        let mut v: i128 = 0;
        let mut digits = 0;
        for c in s.chars().filter(|c| !c.is_whitespace() && *c != '_') {
            let d = NONARY
                .iter()
                .position(|&n| n == c.to_ascii_uppercase())
                .ok_or(ParseError::BadChar(c))? as i128
                - 4;
            v = v * 9 + d;
            digits += 1;
        }
        if digits == 0 {
            return Err(ParseError::Empty);
        }
        Self::new(v as i64)
            .filter(|_| v.abs() <= Self::MAX.0 as i128)
            .ok_or(ParseError::Overflow)
    }
}

const NONARY: [char; 9] = ['W', 'X', 'Y', 'Z', '0', '1', '2', '3', '4'];

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
        write!(f, "Tern<{N}>({self} = {})", self.0)
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
        assert_eq!(Tryte::MAX.value(), 364);
        assert_eq!(Tryte::MIN.value(), -364);
        assert_eq!(Addr::MAX.value(), 40);
        assert_eq!(PageNo::MAX.value(), 13);
        assert_eq!(Word::MAX.value(), 193_710_244);
        assert!(Tryte::new(365).is_none());
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
        for v in -364..=364 {
            let x = Tryte::new(v).unwrap();
            assert_eq!(Tryte::from_trits(&x.trits()), x);
            assert_eq!(x.to_string().parse::<Tryte>().unwrap(), x);
            assert_eq!(Tryte::from_nonary(&x.to_nonary()).unwrap(), x);
            assert_eq!((-x).value(), -v);
        }
    }

    #[test]
    fn opcode_nonary_matches_instruction_table() {
        // (trits, nonary) pairs from Maslovski's Setun-70 instruction table.
        for (tr, nn) in [
            ("000---", "0ZW"), // B1  LST
            ("000000", "000"), // B14 R=T
            ("0000++", "004"), // B18 W=S
            ("000+--", "01W"), // B19 SMT
            ("000+++", "014"), // B27 LHT
            ("00+---", "02W"), // S1  CG1
            ("00++++", "044"), // S27 LG3
        ] {
            assert_eq!(t::<6>(tr).to_nonary(), nn, "{tr}");
            assert_eq!(Tryte::from_nonary(nn).unwrap(), t::<6>(tr));
        }
    }

    #[test]
    fn fields_are_one_based_msb_first() {
        let k = t::<6>("+-0+0-");
        assert_eq!(k.trit(1), Trit::P);
        assert_eq!(k.trit(6), Trit::N);
        assert_eq!(k.field(1..=2), 2); // "+-"
        assert_eq!(k.field(3..=6), t::<4>("0+0-").value());
        let k2 = k.with_field(1..=2, 0);
        assert_eq!(k2.to_string(), "000+0-");
        assert_eq!(k2.with_field(1..=2, 2), k);
    }

    #[test]
    fn wrap_and_shift() {
        // 364 + 1 wraps to -364 in a tryte.
        assert_eq!(Tryte::MAX.wrapping_add(Tryte::new(1).unwrap()), Tryte::MIN);
        // Dropping low trits rounds to nearest.
        assert_eq!(t::<6>("000+-").shift(-1).to_string(), "00000+"); // 2 -> 1
        assert_eq!(t::<6>("0000+").shift(-1).value(), 0); // 1 -> 0
        assert_eq!(t::<6>("0000-+").shift(-1).value(), -1); // -2 -> -1
        assert_eq!(t::<6>("+00000").shift(1).value(), 0); // high trit falls off
        for v in -364..=364i64 {
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
    }

    #[test]
    fn resize_preserves_value() {
        let x = Tryte::new(-300).unwrap();
        let w: Word = x.resize();
        assert_eq!(w.value(), -300);
        assert_eq!(w.resize::<6>(), x);
    }
}
