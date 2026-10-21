//! Exhaustive checks of the codes in docs/ECC.md. The codes are linear, so
//! what a decoder does depends only on the error, not on the data: going
//! through every error pattern of a weight proves the claim for all data.
//! Random data on top checks the encoders' arithmetic.

use setun_ecc::*;

fn rng(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

fn random_trits(n: usize, seed: &mut u64) -> Vec<Trit> {
    (0..n).map(|_| (rng(seed) % 3) as Trit - 1).collect()
}

fn datas(k: usize) -> Vec<Vec<Trit>> {
    let mut seed = 0x5e7u64;
    let mut v = vec![vec![0; k], vec![1; k], vec![-1; k]];
    v.extend((0..60).map(|_| random_trits(k, &mut seed)));
    v
}

fn add3(a: Trit, b: Trit) -> Trit {
    match a + b {
        2 => -1,
        -2 => 1,
        s => s,
    }
}

fn hit(w: &[Trit], errs: &[(usize, Trit)]) -> Vec<Trit> {
    let mut w = w.to_vec();
    for &(p, e) in errs {
        w[p] = add3(w[p], e);
    }
    w
}

#[test]
fn balanced_numbers() {
    for v in -364..=364 {
        assert_eq!(value_of(&trits_of(v, 6)), v);
    }
    assert_eq!(trits_of(5, 3), vec![-1, -1, 1]);
}

#[test]
fn sec_columns() {
    let c = sec();
    assert_eq!(&c.columns[..8], &[2, 4, 5, 6, 7, 8, 10, 11]);
    assert_eq!(*c.columns.last().unwrap(), 31);
    // 31 different points of PG(3,3): any two columns independent
    let mut all: Vec<i64> = (0..31).map(|p| c.column(p)).collect();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), 31);
    assert!(all.iter().all(|&v| (1..=40).contains(&v)));
}

#[test]
fn sec_corrects_every_single_error() {
    let c = sec();
    for d in datas(27) {
        let w = c.encode(&d);
        assert_eq!(w.len(), 31);
        assert_eq!(c.syndrome(&w), 0);
        assert_eq!(c.decode(&w), Decoded::Clean(d.clone()));
        for p in 0..31 {
            for e in [1, -1] {
                let bad = hit(&w, &[(p, e)]);
                assert_eq!(c.syndrome(&bad), e as i64 * c.column(p));
                assert_eq!(c.decode(&bad), Decoded::Corrected(d.clone(), vec![p]), "{p} {e}");
            }
        }
    }
}

#[test]
fn sec_has_distance_3_exactly() {
    // columns 1 and 2 are 4 and 5; trit by trit, 4 + 5 is 9, check trit 2:
    // the codeword has 3 nonzero trits
    let c = sec();
    let mut d = vec![0; 27];
    d[1] = 1;
    d[2] = 1;
    let w = c.encode(&d);
    assert_eq!(w.iter().filter(|&&t| t != 0).count(), 3);
    assert_eq!(w[29], -1);
}

#[test]
fn secded_columns_are_a_cap() {
    let c = secded();
    assert_eq!(c.n(), 33);
    // least weight: one column of weight 3 for each 3 of the 6 check trits,
    // then 7 of weight 4
    let weight = |v: i64| trits_of(v, 6).iter().filter(|&&t| t != 0).count();
    let mut supports: Vec<Vec<usize>> = c
        .columns
        .iter()
        .filter(|&&v| weight(v) == 3)
        .map(|&v| (0..6).filter(|&i| trits_of(v, 6)[i] != 0).collect())
        .collect();
    supports.sort();
    supports.dedup();
    assert_eq!(supports.len(), 20);
    assert_eq!(c.columns.iter().filter(|&&v| weight(v) == 4).count(), 7);
    let cols: Vec<i64> = (0..33).map(|p| c.column(p)).collect();
    for (i, &a) in cols.iter().enumerate() {
        for &b in &cols[i + 1..] {
            for s in [1, -1] {
                let x = value_of(
                    &trits_of(a, 6).iter().zip(&trits_of(b, 6)).map(|(&p, &q)| add3(p, s * q)).collect::<Vec<_>>(),
                );
                assert!(!cols.contains(&x.abs()), "{a} {b} {s}: three columns on a line");
            }
        }
    }
}


#[test]
fn secded_corrects_one_and_detects_two() {
    let c = secded();
    let mut doubles = 0;
    for (n, d) in datas(27).into_iter().enumerate() {
        let w = c.encode(&d);
        assert_eq!(c.decode(&w), Decoded::Clean(d.clone()));
        for p in 0..33 {
            for e in [1, -1] {
                assert_eq!(c.decode(&hit(&w, &[(p, e)])), Decoded::Corrected(d.clone(), vec![p]));
                // every double error, for the first few words
                if n < 4 {
                    for q in p + 1..33 {
                        for f in [1, -1] {
                            assert_eq!(c.decode(&hit(&w, &[(p, e), (q, f)])), Decoded::Uncorrectable, "{p} {q}");
                            doubles += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(doubles, 4 * 33 * 32 / 2 * 4);
}

#[test]
fn sec_miscorrects_some_double_errors() {
    // the reason for SEC-DED: a single-error code turns two errors into a
    // third. Errors of +1 at columns 4 and 5 look like +1 at check trit 2.
    let c = sec();
    let w = c.encode(&[0; 27]);
    let bad = hit(&w, &[(1, 1), (2, 1)]);
    let mut wrong = vec![0; 27];
    wrong[1] = 1;
    wrong[2] = 1;
    assert_eq!(c.decode(&bad), Decoded::Corrected(wrong, vec![29]));
}

fn all_data6() -> Vec<Vec<Trit>> {
    (0..729).map(|v| trits_of(v - 364, 6)).collect()
}

#[test]
fn golay_is_the_golay_code() {
    let g = Golay::new();
    let mut weights = [0usize; 12];
    for d in all_data6() {
        let w = g.encode(&d);
        assert_eq!(&w[..6], &d[..]);
        weights[w.iter().filter(|&&t| t != 0).count()] += 1;
    }
    // the ternary Golay code's weight distribution
    assert_eq!(weights, [1, 0, 0, 0, 0, 132, 132, 0, 330, 110, 0, 24]);
}

#[test]
fn golay_is_cyclic() {
    let g = Golay::new();
    for d in all_data6().into_iter().step_by(7) {
        let w = g.encode(&d);
        let mut r = w.clone();
        r.rotate_left(1);
        assert_eq!(g.encode(&r[..6]), r, "a rotation of a codeword is one");
    }
}

#[test]
fn golay_corrects_every_error_of_weight_2() {
    let g = Golay::new();
    let mut n = 0;
    for d in all_data6() {
        let w = g.encode(&d);
        assert_eq!(g.decode(&w), Decoded::Clean(d.clone()));
        for p in 0..11 {
            for e in [1, -1] {
                assert_eq!(g.decode(&hit(&w, &[(p, e)])), Decoded::Corrected(d.clone(), vec![p]));
                for q in p + 1..11 {
                    for f in [1, -1] {
                        assert_eq!(g.decode(&hit(&w, &[(p, e), (q, f)])), Decoded::Corrected(d.clone(), vec![p, q]));
                        n += 1;
                    }
                }
            }
        }
    }
    assert_eq!(n, 729 * 220);
}

#[test]
fn golay_three_errors_are_never_reported_clean() {
    // a perfect code decodes every word to the nearest codeword: three
    // errors land 2 away from another codeword and are miscorrected, never
    // passed as clean (the distance is 5)
    let g = Golay::new();
    let d = vec![1, 0, -1, 1, 1, 0];
    let w = g.encode(&d);
    for p in 0..11 {
        for q in p + 1..11 {
            for r in q + 1..11 {
                let bad = hit(&w, &[(p, 1), (q, -1), (r, 1)]);
                match g.decode(&bad) {
                    Decoded::Corrected(x, at) => assert!(x != d || at.len() <= 2),
                    other => panic!("{other:?}"),
                }
            }
        }
    }
}

#[test]
fn known_answers() {
    // the vectors of docs/ECC.md, for other implementations
    let mut e0 = vec![0; 27];
    e0[0] = 1;
    let ramp: Vec<Trit> = (0..27).map(|i| [1, 0, -1][i % 3]).collect();
    assert_eq!(&sec().encode(&e0)[27..], &[1, -1, 0, 0]);
    assert_eq!(&sec().encode(&ramp)[27..], &[-1, -1, 0, 1]);
    assert_eq!(&secded().encode(&e0)[27..], &[-1, 1, -1, 0, 0, 0]);
    assert_eq!(&secded().encode(&ramp)[27..], &[0, -1, 1, 0, 0, 1]);
    let g = Golay::new();
    assert_eq!(g.encode(&[1, 0, 0, 0, 0, 0]), vec![1, 0, 0, 0, 0, 0, -1, -1, 1, -1, 0]);
    assert_eq!(g.encode(&[1, 0, -1, 1, 1, 0]), vec![1, 0, -1, 1, 1, 0, -1, -1, 1, 1, 1]);
}
