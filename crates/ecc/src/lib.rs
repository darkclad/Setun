//! The reference encoders and decoders for Setun-11̅'s error-correcting
//! codes (docs/ECC.md): linear codes over GF(3) on balanced trits.
//!
//! - [`sec`]: [31,27,3], a shortened ternary Hamming code. It corrects any
//!   one wrong trit in a word.
//! - [`secded`]: [33,27,4]. It corrects any one wrong trit and detects any two.
//! - [`Golay`]: the perfect ternary Golay code [11,6,5]. It corrects any two
//!   wrong trits in 6 trits of data.
//!
//! The word codes are systematic: a codeword is the data trits, then the
//! check trits. Each column of the parity-check matrix is a nonzero
//! balanced number (its trits, least significant first), so the syndrome,
//! read as a balanced number, names the wrong trit by its column and gives
//! the error by its sign. The Golay code is cyclic, decoded by a table.

/// A trit: −1, 0 or +1.
pub type Trit = i8;

/// What a decoder found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decoded {
    /// No error: the data as stored.
    Clean(Vec<Trit>),
    /// The errors at these positions (codeword order) were corrected.
    Corrected(Vec<Trit>, Vec<usize>),
    /// More errors than the code corrects: the data cannot be trusted.
    Uncorrectable,
}

fn add3(a: Trit, b: Trit) -> Trit {
    match a + b {
        2 => -1,
        -2 => 1,
        s => s,
    }
}

/// The balanced trits of `v`, least significant first.
pub fn trits_of(mut v: i64, n: usize) -> Vec<Trit> {
    let mut t = vec![0; n];
    for x in t.iter_mut() {
        let r = v.rem_euclid(3);
        let d = if r == 2 { -1 } else { r };
        *x = d as Trit;
        v = (v - d) / 3;
    }
    assert_eq!(v, 0, "the value needs more than {n} trits");
    t
}

/// The value of balanced trits, least significant first.
pub fn value_of(t: &[Trit]) -> i64 {
    t.iter().rev().fold(0, |v, &d| v * 3 + d as i64)
}

/// A systematic single-error code: k data columns and r check columns.
pub struct WordCode {
    pub k: usize,
    pub r: usize,
    /// The positive values of the data columns, in data order.
    pub columns: Vec<i64>,
}

impl WordCode {
    pub fn n(&self) -> usize {
        self.k + self.r
    }

    /// The check trits: −(the sum of data[j] × column j).
    pub fn checks(&self, data: &[Trit]) -> Vec<Trit> {
        assert_eq!(data.len(), self.k);
        let mut s = vec![0; self.r];
        for (&d, &c) in data.iter().zip(&self.columns) {
            if d != 0 {
                for (x, &h) in s.iter_mut().zip(&trits_of(c, self.r)) {
                    *x = add3(*x, d * h);
                }
            }
        }
        s.iter().map(|&x| -x).collect()
    }

    pub fn encode(&self, data: &[Trit]) -> Vec<Trit> {
        let mut w = data.to_vec();
        w.extend(self.checks(data));
        w
    }

    /// The syndrome of a stored word, as a balanced number: 0 if it is a
    /// codeword, ±column of the trit in error after one error.
    pub fn syndrome(&self, word: &[Trit]) -> i64 {
        assert_eq!(word.len(), self.n());
        let mut s = self.checks(&word[..self.k]);
        for (x, &p) in s.iter_mut().zip(&word[self.k..]) {
            *x = add3(*x, -p);
        }
        -value_of(&s)
    }

    /// The column of each position: data columns, then 1, 3, 9, … for the
    /// check trits.
    pub fn column(&self, pos: usize) -> i64 {
        if pos < self.k { self.columns[pos] } else { 3i64.pow((pos - self.k) as u32) }
    }

    pub fn decode(&self, word: &[Trit]) -> Decoded {
        let s = self.syndrome(word);
        if s == 0 {
            return Decoded::Clean(word[..self.k].to_vec());
        }
        let Some(pos) = (0..self.n()).find(|&p| self.column(p) == s.abs()) else {
            return Decoded::Uncorrectable;
        };
        let mut w = word.to_vec();
        w[pos] = add3(w[pos], -(s.signum() as Trit));
        Decoded::Corrected(w[..self.k].to_vec(), vec![pos])
    }
}

/// [31,27,3]: the data columns are the 27 smallest positive numbers that
/// are not powers of 3, in 4 trits.
pub fn sec() -> WordCode {
    let columns = (1..).filter(|&v: &i64| !is_power_of_3(v)).take(27).collect();
    WordCode { k: 27, r: 4, columns }
}

fn is_power_of_3(mut v: i64) -> bool {
    while v % 3 == 0 {
        v /= 3;
    }
    v == 1
}

/// The data columns of [`secded`]: 33 points of PG(5,3) with no three on
/// a line (a cap), so no three columns are dependent and the distance is 4.
/// With the 6 unit check columns, the cap allows at most one column of
/// weight 3 for each 3 of the 6 check trits (two on the same trits are on a
/// line with a unit column): 20 columns of weight 3, then 7 of weight 4.
/// That is the least total weight, 88, so the fewest gates in the checker.
pub const SECDED_COLUMNS: [i64; 27] = [
    7, 16, 17, 31, 32, 39, 57, 62, 71, 75, 83, 96, 109, 117, 153, 161, 174, 231, 239, 253, 261, 271, 273, 322,
    327, 330, 351,
];

/// [33,27,4]: corrects one wrong trit and detects two.
pub fn secded() -> WordCode {
    WordCode { k: 27, r: 6, columns: SECDED_COLUMNS.to_vec() }
}

/// The ternary Golay code [11,6,5], cyclic, with generator
/// g(x) = x^5 + x^4 − x^3 + x^2 − 1. A codeword is 6 data trits then 5
/// check trits: c(x) = d(x)·x^5 − (d(x)·x^5 mod g(x)), coefficients of x^10
/// first.
pub struct Golay {
    table: std::collections::HashMap<Vec<Trit>, Vec<Trit>>,
}

pub const GOLAY_G: [Trit; 6] = [1, 1, -1, 1, 0, -1]; // x^5 … x^0

/// The remainder of a polynomial (highest coefficient first) mod g(x),
/// as 5 trits (x^4 first).
fn golay_rem(p: &[Trit]) -> Vec<Trit> {
    let mut r = p.to_vec();
    for i in 0..r.len().saturating_sub(5) {
        let c = r[i];
        if c != 0 {
            for (j, &g) in GOLAY_G.iter().enumerate() {
                r[i + j] = add3(r[i + j], -c * g);
            }
        }
    }
    r[r.len() - 5..].to_vec()
}

impl Golay {
    pub fn new() -> Golay {
        // Every error of weight 0, 1 or 2, by its syndrome. The code is
        // perfect: these 1 + 22 + 220 = 243 = 3^5 errors have 243 different
        // syndromes.
        let mut table = std::collections::HashMap::new();
        let mut put = |e: Vec<Trit>| {
            let old = table.insert(golay_rem(&e), e);
            assert!(old.is_none(), "two errors share a syndrome");
        };
        put(vec![0; 11]);
        for i in 0..11 {
            for a in [1, -1] {
                let mut e = vec![0; 11];
                e[i] = a;
                put(e.clone());
                for j in i + 1..11 {
                    for b in [1, -1] {
                        let mut e2 = e.clone();
                        e2[j] = b;
                        put(e2);
                    }
                }
            }
        }
        Golay { table }
    }

    pub fn encode(&self, data: &[Trit]) -> Vec<Trit> {
        assert_eq!(data.len(), 6);
        let mut c = data.to_vec();
        c.extend([0; 5]);
        let r = golay_rem(&c);
        for (x, &y) in c[6..].iter_mut().zip(&r) {
            *x = -y;
        }
        c
    }

    pub fn decode(&self, word: &[Trit]) -> Decoded {
        assert_eq!(word.len(), 11);
        let e = &self.table[&golay_rem(word)];
        let at: Vec<usize> = (0..11).filter(|&i| e[i] != 0).collect();
        let fixed: Vec<Trit> = word.iter().zip(e).map(|(&w, &x)| add3(w, -x)).collect();
        if at.is_empty() {
            Decoded::Clean(fixed[..6].to_vec())
        } else {
            Decoded::Corrected(fixed[..6].to_vec(), at)
        }
    }
}

impl Default for Golay {
    fn default() -> Self {
        Golay::new()
    }
}
