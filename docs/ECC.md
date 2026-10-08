# Error correction for Setun-11̅ memory

This is a specification for hardware. The emulator stays error-free and
does not model check trits (decided 2026-10-06). The reference encoders and
decoders are in [`crates/ecc`](../crates/ecc), and their tests prove every
claim below by going through every error pattern.

## Contents

1. [Ternary errors](#1-ternary-errors)
2. [How the codes are written](#2-how-the-codes-are-written)
3. [SEC: the [31,27,3] word code](#3-sec-the-31273-word-code)
4. [SEC-DED: the [33,27,4] word code](#4-sec-ded-the-33274-word-code)
5. [Golay: the [11,6,5] code](#5-golay-the-1165-code)
6. [What the memory does with the result](#6-what-the-memory-does-with-the-result)
7. [Cost against binary](#7-cost-against-binary)
8. [Known answers](#8-known-answers)
9. [How it is tested](#9-how-it-is-tested)

## 1. Ternary errors

A cell that holds a trit can fail in two ways: it reads one level away
(−1 ↔ 0 ↔ +1) or two levels away (−1 ↔ +1). Either way, the trit read is the
trit stored plus an error e ∈ {+1, −1} in GF(3), where +1 + +1 = −1 and
−1 + −1 = +1. A code over GF(3) corrects both kinds alike: it knows the
position of the wrong trit and the value to subtract.

The codes are linear: what a decoder does depends only on the error, not on
the data. That is why a test that tries every error pattern of a weight
proves the claim for every one of the 3^27 words.

## 2. How the codes are written

The word codes are systematic: a codeword is the 27 data trits d₀…d₂₆, then
r check trits p₀…p_{r−1}. The parity-check matrix is H = [A | I]: column j
of A belongs to data trit j, and column i of I to check trit i.

Each column is written as a positive balanced number, its trits least
significant first. Check trit i has the column 3^i. Data trit j has the
column a_j given in the code's table. Then:

- **Encoding:** p = −Σ d_j · a_j, added trit by trit in GF(3) (no carries).
- **Syndrome:** S = Σ c_j · h_j over the stored word, again trit by trit,
  read as a balanced number.
- **Decoding:** S = 0: no error. S = ±h_j for some position j: trit j is
  off by ±1; subtract that sign. Any other S: more errors than the code
  corrects.

So the syndrome, read as a number, names the wrong trit by its column, and
its sign is the error. A hardware decoder compares S with the 31 or 33
column values; no table is needed.

## 3. SEC: the [31,27,3] word code

A shortened ternary Hamming code: 27 data trits and 4 check trits. It
corrects any one wrong trit.

A column is a nonzero vector of 4 trits, and two columns must not be
multiples of each other, so the columns are distinct positive numbers from
1 to 40 (the 40 points of PG(3,3)). The check trits take 1, 3, 9 and 27.
The data columns are the 27 smallest of the rest: every number from 2 to 31
that is not a power of 3.

| Data trit | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Column | 2 | 4 | 5 | 6 | 7 | 8 | 10 | 11 | 12 | 13 | 14 | 15 | 16 | 17 |

| Data trit | 14 | 15 | 16 | 17 | 18 | 19 | 20 | 21 | 22 | 23 | 24 | 25 | 26 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Column | 18 | 19 | 20 | 21 | 22 | 23 | 24 | 25 | 26 | 28 | 29 | 30 | 31 |

Its distance is 3, not more: data trits 1 and 2 (columns 4 and 5, which are
++00 and −−+0 written least significant first) add up to 00+0, the column
of check trit 2. So two errors can look like one, and SEC then corrects
the wrong trit: errors of +1 at data trits 1 and 2 are "corrected" at check
trit 2. That is why memory uses SEC-DED.

## 4. SEC-DED: the [33,27,4] word code

27 data trits and 6 check trits. It corrects any one wrong trit and detects
any two. **This is the code for memory words.**

Distance 4 needs every three columns to be independent. Seen as points of
PG(5,3), the 33 columns must form a cap: no three of them on a line. The
line through two points p and q holds p + q and p − q as well, so a cap's
columns never satisfy a_i ± a_j = ±a_k.

The data columns:

| Data trit | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Column | 7 | 16 | 17 | 31 | 32 | 39 | 57 | 62 | 71 | 75 | 83 | 96 | 109 | 117 |

| Data trit | 14 | 15 | 16 | 17 | 18 | 19 | 20 | 21 | 22 | 23 | 24 | 25 | 26 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Column | 153 | 161 | 174 | 231 | 239 | 253 | 261 | 271 | 273 | 322 | 327 | 330 | 351 |

The checker's cost is the total weight of the columns (each nonzero trit is
one GF(3) adder input). This choice has the least possible weight:

- A column of weight 1 is a check column, and one of weight 2 lies on the
  line through two check columns. So every data column has weight 3 or more.
- Two columns of weight 3 on the same three trits lie on a line with a
  check column. With 6 check trits there are C(6,3) = 20 sets of three, so
  at most 20 columns have weight 3.
- So 27 columns weigh at least 20 · 3 + 7 · 4 = 88. These have 20 columns
  of weight 3 and 7 of weight 4: 88.

A plain greedy search (each positive number in order, kept if it leaves
the set a cap) gets stuck at 32 points. These came from a randomized search
that prefers light columns, and the test checks the cap property directly.

## 5. Golay: the [11,6,5] code

The ternary Golay code protects 6 data trits with 5 check trits and
corrects any two wrong trits. It is perfect: the 3^5 = 243 syndromes are
exactly the 1 + 22 + 220 errors of weight 0, 1 and 2, so a table of 243
entries decodes it.

It is cyclic, with generator g(x) = x⁵ + x⁴ − x³ + x² − 1, a factor of
x¹¹ − 1 over GF(3). A codeword lists the coefficients of x¹⁰ down to x⁰:
the 6 data trits, then the 5 check trits of
c(x) = d(x)·x⁵ − (d(x)·x⁵ mod g(x)). Its weight distribution is the Golay
code's: 1 word of weight 0, 132 of weight 5, 132 of 6, 330 of 8, 110 of 9
and 24 of 11.

A perfect code always decodes to the nearest codeword. Three errors are
never reported clean, but they are miscorrected. Golay suits short fields
that must survive two errors: a boot record's header, the mode trits of a
page-table entry, or a disk sector's address. No memory word uses it.

## 6. What the memory does with the result

Each word is stored with its check trits: 33 trits for 27. On every read:

| Result | The memory |
|---|---|
| Clean | returns the word |
| Corrected | returns the corrected word, writes it back (scrubbing), and counts the event |
| Uncorrectable | returns the word as read and raises a machine check |

A machine check would need an ISA trap vector and a status port. They are
left out on purpose until there is hardware: today's ISA and emulator have
none.

## 7. Cost against binary

A 27-trit word holds log₂ 3^27 ≈ 42.8 bits.

| Code | Data | Check | Overhead | Corrects | Detects |
|---|---|---|---|---|---|
| Ternary SEC [31,27,3] | 27 trits | 4 trits | 14.8% | 1 | — |
| Ternary SEC-DED [33,27,4] | 27 trits | 6 trits | 22.2% | 1 | 2 |
| Binary SEC-DED (50,43) | 43 bits | 7 bits | 16.3% | 1 | 2 |
| Binary SEC-DED (72,64) | 64 bits | 8 bits | 12.5% | 1 | 2 |
| Ternary Golay [11,6,5] | 6 trits | 5 trits | 83% | 2 | — |

Per word, ternary SEC-DED costs more than binary: distance 4 needs a cap
of 33 points, and the largest cap in PG(4,3) has 20, so 5 check trits
cannot do it. Ternary correction gains elsewhere: one check trit
covers both error values, so a cell that drifts two levels is corrected as
cheaply as one that drifts one. A code over a whole line of words would cut
the overhead (a cap of 88 points in PG(6,3) would give [88,81,4], 8.6%);
that waits for a cache design.

## 8. Known answers

Trits least significant first; e₀ is data trit 0 = +1 and the rest 0;
the ramp is data trit j = +1, 0, −1 for j mod 3 = 0, 1, 2.

| Code | Data | Check trits |
|---|---|---|
| SEC | e₀ | +1 −1 0 0 |
| SEC | ramp | −1 −1 0 +1 |
| SEC-DED | e₀ | −1 +1 −1 0 0 0 |
| SEC-DED | ramp | 0 −1 +1 0 0 +1 |

Golay, data then check trits, coefficients of x¹⁰ first:

| Data | Codeword |
|---|---|
| +1 0 0 0 0 0 | +1 0 0 0 0 0 −1 −1 +1 −1 0 |
| +1 0 −1 +1 +1 0 | +1 0 −1 +1 +1 0 −1 −1 +1 +1 +1 |

## 9. How it is tested

[`crates/ecc/tests/codes.rs`](../crates/ecc/tests/codes.rs), with data of
all zeros, all +1, all −1 and random words:

- SEC: all 62 single errors are corrected; the columns are 31 different
  points; a codeword of weight 3 exists; a double error is miscorrected as
  §3 says.
- SEC-DED: all 66 single errors are corrected; all 2,112 double errors are
  reported uncorrectable; the columns form a cap, with 20 of weight 3 on
  different trits and 7 of weight 4.
- Golay: all 729 codewords have the Golay weight distribution; a rotation
  of a codeword is a codeword; every error of weight 1 or 2 on every
  codeword is corrected (729 × 242 cases); three errors are never reported
  clean.
- The known answers above.

Each part of the codec has a mutation that fails these tests: GF(3)
addition, the correction's sign, a SEC-DED column, the check trits' sign,
the syndrome's sign, the Golay generator, the Golay correction, the check
columns, and the SEC column choice.
