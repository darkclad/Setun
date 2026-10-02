//! Balanced-ternary fixed-width integers.
//!
//! A balanced-ternary digit ("trit") is one of −1, 0, +1. An `N`-trit number
//! covers exactly the integers in `[-(3^N-1)/2, (3^N-1)/2]`, so we store it as
//! a plain `i64` and derive trits on demand. That keeps arithmetic cheap while
//! still letting the emulator address individual trits and trit fields.
//!
//! Conventions (see `docs/SETUN26_ISA.md` §1):
//! * trit positions are **1-based and most-significant first**: in a tryte
//!   `x[1:9]`, `x[1]` has weight 3^8 and `x[9]` has weight 3^0;
//! * **base 3** writes trits as `1`, `0` and `1̅` (a `1` with U+0305
//!   COMBINING OVERLINE), most significant first, e.g. `11̅0` = 6; `T` is
//!   an ASCII alias for `1̅`;
//! * **base 27** writes one digit per 3 trits: `0`–`9` and `A`–`D` (either
//!   case) are 0..13, and a negative digit is the same digit overlined,
//!   `1̅`…`9̅`, `A̅`…`D̅` = −1..−13. So `DDD` = 9841, `D̅D̅D̅` = −9841, and
//!   negating a number toggles the overline on every digit.
//!
//! An overline belongs to the digit before it: `0` takes none, nor does
//! anything that is not a digit, and a digit takes at most one. `_` and
//! whitespace may separate digits.

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

    /// The canonical text: `1`, `0`, or `1` with an overline.
    pub const fn to_str(self) -> &'static str {
        match self {
            Trit::N => "1\u{305}",
            Trit::Z => "0",
            Trit::P => "1",
        }
    }

    /// The ASCII text: `1`, `0` or `T`.
    pub const fn to_ascii(self) -> char {
        match self {
            Trit::N => 'T',
            Trit::Z => '0',
            Trit::P => '1',
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

const POW3: [i128; 81] = {
    let mut t = [1i128; 81];
    let mut i = 1;
    while i < 81 {
        t[i] = t[i - 1] * 3;
        i += 1;
    }
    t
};

const POW3_I64: [i64; 40] = {
    let mut t = [1i64; 40];
    let mut i = 1;
    while i < 40 {
        t[i] = t[i - 1] * 3;
        i += 1;
    }
    t
};

/// 3^n as i128 (n ≤ 80), from a table.
#[inline]
pub const fn pow3(n: u32) -> i128 {
    POW3[n as usize]
}

/// Round `v / 3^k` to the nearest integer. In balanced ternary this is exactly
/// "drop the low k trits": ties are impossible because 3^k is odd.
#[inline]
pub fn div_pow3_round(v: i128, k: u32) -> i128 {
    // Fast path: 128-bit division is a slow software routine.
    if k <= 39 && (-(1i128 << 62)..(1i128 << 62)).contains(&v) {
        let d = POW3_I64[k as usize];
        return (v as i64 + (d - 1) / 2).div_euclid(d) as i128;
    }
    let d = pow3(k);
    (v + (d - 1) / 2).div_euclid(d)
}

/// Wrap `v` into the balanced range of an `n`-trit number, i.e. keep the low
/// `n` trits and drop the rest.
#[inline]
pub fn wrap_trits(v: i128, n: u32) -> i128 {
    if n <= 39 && (-(1i128 << 62)..(1i128 << 62)).contains(&v) {
        let m = POW3_I64[n as usize];
        let h = (m - 1) / 2;
        return ((v as i64 + h).rem_euclid(m) - h) as i128;
    }
    let m = pow3(n);
    let h = (m - 1) / 2;
    (v + h).rem_euclid(m) - h
}

/// An `N`-trit balanced-ternary integer (`N` ≤ 39 so it fits in an `i64`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
#[repr(transparent)]
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
        Trit::from_i64(self.field_unchecked(i, i))
    }

    /// Value of the trit field `x[a:b]` (1-based, inclusive, MSB first),
    /// read as a balanced-ternary number.
    pub fn field(self, r: RangeInclusive<usize>) -> i64 {
        let (a, b) = (*r.start(), *r.end());
        assert!(1 <= a && a <= b && b <= N, "field {a}:{b} out of 1..={N}");
        self.field_unchecked(a, b)
    }

    /// Constant-time field extraction: dropping the low `N − b` trits is
    /// exact rounding in balanced ternary, then keep `b − a + 1` trits.
    fn field_unchecked(self, a: usize, b: usize) -> i64 {
        let shifted = div_pow3_round(self.0 as i128, (N - b) as u32);
        wrap_trits(shifted, (b - a + 1) as u32) as i64
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

    /// The base-27 digits, −13..=13, most significant first: one per 3
    /// trits (a width that is not a multiple of 3 gets implicit leading zero
    /// trits).
    pub fn b27_digits(self) -> Vec<i64> {
        let mut t: Vec<Trit> = self.trits().to_vec();
        while !t.len().is_multiple_of(3) {
            t.insert(0, Trit::Z);
        }
        t.chunks(3).map(|d| d[0] as i64 * 9 + d[1] as i64 * 3 + d[2] as i64).collect()
    }

    /// Base-27 text: `0`–`9`, `A`–`D` for 0..13, and those digits followed by
    /// U+0305 COMBINING OVERLINE for −1..−13.
    pub fn to_b27(self) -> String {
        self.b27_digits().into_iter().map(b27_text).collect()
    }

    /// Parse base-27 text (see [`Tern::to_b27`]; letters in either case).
    pub fn from_b27(s: &str) -> Result<Self, ParseError> {
        let mut v: i128 = 0;
        for (c, over) in digit_marks(s)? {
            let d = b27_value(c).ok_or(if over { ParseError::BadOverline(c) } else { ParseError::BadChar(c) })?;
            if over && d == 0 {
                return Err(ParseError::BadOverline(c));
            }
            v = v * 27 + if over { -d } else { d } as i128;
            if v.abs() > Self::MAX.0 as i128 {
                return Err(ParseError::Overflow);
            }
        }
        Ok(Tern(v as i64))
    }

    /// The trits as ASCII text, with `T` for −1.
    pub fn to_ascii(self) -> String {
        self.trits().iter().map(|t| t.to_ascii()).collect()
    }
}

/// U+0305 COMBINING OVERLINE: the mark of a negative digit.
pub const OVERLINE: char = '\u{305}';

/// Host text as the machine's code points, one per tryte: a character
/// followed by U+0305 is the character overlined, written as its negated
/// code point (`1` + U+0305 is −`'1'` = −49; docs/NOTATION.md). An overline
/// with nothing before it, two overlines, an overline on a control
/// character or a space, and a character above 9841 are errors.
pub fn code_points(s: &str) -> Result<Vec<i64>, String> {
    let mut out: Vec<i64> = Vec::new();
    let mut barred = false;
    for c in s.chars() {
        if c == OVERLINE {
            match out.last_mut() {
                None => return Err("an overline with no character before it".into()),
                Some(_) if barred => return Err("two overlines on one character".into()),
                Some(&mut p) if p <= 32 => return Err(format!("U+{p:04X} cannot be overlined")),
                Some(p) => *p = -*p,
            }
            barred = true;
        } else {
            if c as i64 > 9841 {
                return Err(format!("character U+{:X} does not fit in one tryte", c as i64));
            }
            out.push(c as i64);
            barred = false;
        }
    }
    Ok(out)
}

/// The value of a prefixed literal's digits, as the assembler and the Trine
/// compiler read them: `t` balanced ternary, `z` balanced base 27, `x` plain
/// hexadecimal (for values from binary systems; a sign is written with unary
/// minus, and nothing wraps). Range checks against a type are the caller's.
pub fn parse_prefixed(base: char, digits: &str) -> Result<i64, String> {
    match base {
        't' => digits.parse::<Tern<39>>().map(|t| t.value()).map_err(|e| e.to_string()),
        'z' => Tern::<39>::from_b27(digits).map(|t| t.value()).map_err(|e| e.to_string()),
        'x' => {
            if digits.contains(OVERLINE) {
                return Err("hexadecimal digits cannot be overlined".into());
            }
            let h: String = digits.chars().filter(|&c| c != '_').collect();
            if h.is_empty() {
                return Err("empty hexadecimal literal".into());
            }
            if let Some(c) = h.chars().find(|c| !c.is_ascii_hexdigit()) {
                return Err(format!("invalid hexadecimal digit {c:?}"));
            }
            i64::from_str_radix(&h, 16).map_err(|_| "hexadecimal literal does not fit".into())
        }
        _ => Err(format!("unknown literal prefix 0{base}")),
    }
}

/// Split digit text into (character, overlined) pairs, skipping `_` and
/// whitespace. An overline must follow a character that can take one; a
/// second overline, or one with nothing before it, is an error.
fn digit_marks(s: &str) -> Result<Vec<(char, bool)>, ParseError> {
    let mut out = Vec::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == OVERLINE {
            return Err(ParseError::StrayOverline);
        }
        let over = it.peek() == Some(&OVERLINE);
        if over {
            it.next();
            if it.peek() == Some(&OVERLINE) {
                return Err(ParseError::DoubleOverline(c));
            }
        }
        if c.is_whitespace() || c == '_' {
            if over {
                return Err(ParseError::BadOverline(c));
            }
            continue;
        }
        out.push((c, over));
    }
    if out.is_empty() {
        return Err(ParseError::Empty);
    }
    Ok(out)
}

fn b27_text(v: i64) -> String {
    let d = b27_char(v.abs());
    if v < 0 { format!("{d}{OVERLINE}") } else { d.to_string() }
}

/// The digit for 0..=13.
fn b27_char(v: i64) -> char {
    match v {
        0..=9 => (b'0' + v as u8) as char,
        10..=13 => (b'A' + (v - 10) as u8) as char,
        _ => unreachable!("base-27 digit out of range: {v}"),
    }
}

/// The value of an un-overlined base-27 digit, 0..=13.
fn b27_value(c: char) -> Option<i64> {
    match c {
        '0'..='9' => Some(c as i64 - '0' as i64),
        'A'..='D' => Some(c as i64 - 'A' as i64 + 10),
        'a'..='d' => Some(c as i64 - 'a' as i64 + 10),
        _ => None,
    }
}

/// Split a full product into `(high, low)` halves of `N` trits each, so that
/// `p = high·3^N + low` with both halves balanced (`MULH` / `MUL`).
pub fn split_product<const N: usize>(p: i128) -> (Tern<N>, Tern<N>) {
    let lo = wrap_trits(p, N as u32);
    let hi = (p - lo) / pow3(N as u32);
    (Tern::<N>::wrap(hi), Tern::<N>(lo as i64))
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
        self.trits().iter().try_for_each(|t| write!(f, "{}", t.to_str()))
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
    /// An overline on something that cannot take one (`0`, `T`, a separator).
    BadOverline(char),
    /// Two overlines on one digit.
    DoubleOverline(char),
    /// An overline with no digit before it.
    StrayOverline,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Empty => write!(f, "empty ternary literal"),
            ParseError::BadChar(c) => write!(f, "invalid ternary digit {c:?}"),
            ParseError::Overflow => write!(f, "ternary literal does not fit"),
            ParseError::BadOverline(c) => write!(f, "{c:?} cannot be overlined"),
            ParseError::DoubleOverline(c) => write!(f, "{c:?} has two overlines"),
            ParseError::StrayOverline => write!(f, "an overline with no digit before it"),
        }
    }
}

impl std::error::Error for ParseError {}

impl<const N: usize> FromStr for Tern<N> {
    type Err = ParseError;

    /// Parse base-3 text like `11̅0` or `1T0` (MSB first). Up to `N` trits;
    /// shorter input is zero-extended on the left. `T` and an overlined `1`
    /// are the same digit and may be mixed.
    fn from_str(s: &str) -> Result<Self, ParseError> {
        let mut trits = Vec::new();
        for (c, over) in digit_marks(s)? {
            trits.push(match (c, over) {
                ('1', false) => Trit::P,
                ('1', true) | ('T' | 't', false) => Trit::N,
                ('0', false) => Trit::Z,
                ('0' | 'T' | 't', true) => return Err(ParseError::BadOverline(c)),
                (_, true) => return Err(ParseError::BadOverline(c)),
                _ => return Err(ParseError::BadChar(c)),
            });
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
    fn code_points_fold_overlines() {
        assert_eq!(code_points("Setun-11\u{305}").unwrap().last(), Some(&-49));
        assert_eq!(code_points("1\u{305}1").unwrap(), [-49, 49]);
        assert_eq!(code_points("Ж").unwrap(), [0x416]);
        assert!(code_points("\u{305}1").unwrap_err().contains("no character before"));
        assert!(code_points("1\u{305}\u{305}").unwrap_err().contains("two overlines"));
        assert!(code_points(" \u{305}").unwrap_err().contains("cannot be overlined"));
        assert!(code_points("\u{1F600}").unwrap_err().contains("does not fit"));
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
        assert_eq!(t::<3>("111").value(), 13);
        assert_eq!(t::<3>("1T1").value(), 7);
        assert_eq!(t::<3>("1T0").value(), 6);
        assert_eq!(t::<3>("T10").value(), -6);
        assert_eq!((-t::<3>("1T1")).to_ascii(), "T1T");
        assert_eq!((-t::<3>("1T1")).to_string(), "1\u{305}11\u{305}", "canonical: overlined ones");
    }

    #[test]
    fn base3_text_forms() {
        // T and an overlined 1 are the same digit, alone or mixed
        assert_eq!(t::<4>("1T0T").value(), t::<4>("11\u{305}01\u{305}").value());
        assert_eq!(t::<4>("1t01\u{305}").value(), 27 - 9 - 1);
        assert_eq!(t::<9>("1_T 0").value(), 6, "separators");
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
        let o = OVERLINE;
        assert_eq!(Tryte::new(1).unwrap().to_b27(), "001");
        assert_eq!(Tryte::new(-1).unwrap().to_b27(), format!("001{o}"));
        assert_eq!(Tryte::new(13).unwrap().to_b27(), "00D");
        assert_eq!(Tryte::new(10).unwrap().to_b27(), "00A");
        assert_eq!(Tryte::new(27).unwrap().to_b27(), "010");
        assert_eq!(Tryte::MAX.to_b27(), "DDD");
        assert_eq!(Tryte::MIN.to_b27(), format!("D{o}D{o}D{o}"));
        // negation toggles every overline; a pixel's digits are R, G, B
        let red = Tryte::from_b27(&format!("DD{o}D{o}")).unwrap();
        assert_eq!((-red).to_b27(), format!("D{o}DD"));
        assert_eq!(Word::from_b27("DDDDDDDDD").unwrap(), Word::MAX);
        assert_eq!(Tern::<4>::new(40).unwrap().to_b27(), "1D"); // 1·27 + 13
        assert_eq!(Tryte::from_b27("abc").unwrap(), Tryte::from_b27("ABC").unwrap(), "letters in either case");
        assert_eq!(Tryte::from_b27(&format!("d{o}")).unwrap().value(), -13);
        assert_eq!(Tryte::new(5 * 27 - 9).unwrap().to_b27(), format!("059{o}"));
        assert_eq!(Tryte::new(-9841).unwrap().b27_digits(), vec![-13, -13, -13]);
        assert!(Tryte::from_b27("1DDD").is_err());
        assert!(Tryte::from_b27("0E0").is_err());
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
        let k = t::<9>("1T010T001");
        assert_eq!(k.trit(1), Trit::P);
        assert_eq!(k.trit(6), Trit::N);
        assert_eq!(k.trit(9), Trit::P);
        assert_eq!(k.field(1..=2), 2); // "1T"
        assert_eq!(k.field(3..=6), t::<4>("010T").value());
        let k2 = k.with_field(1..=2, 0);
        assert_eq!(k2.to_ascii(), "00010T001");
        assert_eq!(k2.with_field(1..=2, 2), k);
    }

    #[test]
    fn wrap_and_shift() {
        // MAX + 1 wraps to MIN.
        assert_eq!(Tryte::MAX.wrapping_add(Tryte::new(1).unwrap()), Tryte::MIN);
        // Dropping low trits rounds to nearest.
        assert_eq!(t::<6>("0001T").shift(-1).to_ascii(), "000001"); // 2 -> 1
        assert_eq!(t::<6>("00001").shift(-1).value(), 0); // 1 -> 0
        assert_eq!(t::<6>("0000T1").shift(-1).value(), -1); // -2 -> -1
        assert_eq!(t::<6>("100000").shift(1).value(), 0); // high trit falls off
        for v in -9841..=9841i64 {
            let x = Tryte::new(v).unwrap();
            let r = x.shift(-2).value() as f64;
            assert!((r - v as f64 / 9.0).abs() <= 0.5, "{v}");
        }
    }

    #[test]
    fn tritwise_ops() {
        let a = t::<6>("10T10T");
        let b = t::<6>("11TT00");
        assert_eq!(a.tritwise_mul(b).to_ascii(), "101T00");
        assert_eq!(a.tritwise_sat(b).to_ascii(), "11T00T");
        assert_eq!(a.tritwise_min(b).to_ascii(), "10TT0T");
        assert_eq!(a.tritwise_max(b).to_ascii(), "11T100");
        assert_eq!(a.tritwise_cons(b).to_ascii(), "10T000");
        assert_eq!(t::<9>("0001T0000").leading_zero_trits(), 3);
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
