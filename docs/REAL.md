# real27 and real81: balanced-ternary floating point

**Status: v1 (2026-10-08).** This is the standard for Setun-11̅'s two floating-point formats, written to stand alone so that any ternary machine or library can implement it. The FPU ([`SETUN26_ISA.md`](SETUN26_ISA.md) §14, [`CPU.md`](CPU.md) §6.11), the `setun-real` library (`crates/real`) and Trine's `real` ([`TRINE.md`](TRINE.md) Part II §7) implement it, and §8 describes the conformance suite that checks them.

## Contents

1. [Goals](#1-goals)
2. [Notation](#2-notation)
3. [Formats](#3-formats)
4. [Kinds of value](#4-kinds-of-value)
5. [Rounding](#5-rounding)
6. [Operations](#6-operations)
7. [Exceptions](#7-exceptions)
8. [Conformance](#8-conformance)
9. [Differences from IEEE 754](#9-differences-from-ieee-754)
10. [Implementations](#10-implementations)

## 1. Goals

- **Balanced throughout.** The significand is a balanced ternary integer and carries the sign, so negation is exact, there is no sign field and no negative zero.
- **Rounding is mostly free.** In balanced ternary, dropping trits rounds to nearest. Only results with a factor 2 in the denominator (division, square root, remainder, decimal input) can tie.
- **Two sizes that nest.** real27 fills one word; real81 fills three, and its first word is a real27.
- **Everything is defined.** Every operation on every pair of encodings has one result and one set of flags. There is no implementation-defined behaviour.

## 2. Notation

- A *trit* is −1, 0 or +1, written `T` (or 1̅), `0`, `1`. Trit positions are numbered from 1, most significant first.
- An *n-trit balanced integer* lies in −(3^n − 1)/2 … (3^n − 1)/2.
- `M_max(n)` = (3^n − 1)/2, the largest n-trit significand. `M_norm(n)` = (3^(n−1) − 1)/2, the largest one whose top trit is 0.
- E_max = 9841 and E_min = −9841, the largest and smallest 9-trit integers.

## 3. Formats

| Format | Size | Exponent E | Significand M | n | Precision |
|---|---|---|---|---|---|
| **real27** | 27 trits (one word) | trits 1–9 | trits 10–27 | 18 | about 28.5 bits (IEEE binary32: 24) |
| **real81** | 81 trits (three words, the first most significant) | trits 1–9 | trits 10–81 | 72 | about 114 bits (IEEE binary128: 113) |

- **E** is a 9-trit balanced integer, −9841 … 9841. **M** is an n-trit balanced integer.
- **The value** of a finite encoding is **M · 3^(E − n)**: the significand is a fraction m = M · 3^(−n) with the point just left of its top trit, so |m| < 1/2, and the value is m · 3^E.
- **The first word of a real81 is a real27**: the same E and M's top 18 trits. Dropping the other 54 trits rounds to nearest (balanced truncation), so the first word read alone is the real81 rounded to real27, except that an exact half at trit 18 has no tie-breaking (§5). The conversion of §6.12 rounds correctly.
- **Interchange.** A real27 is one word; a real81 is three words in memory order, the word with E first. In trytes, the most significant tryte comes first (the machine's big-endian order).

Examples (real27, written as E then M in base 27, an overlined digit negative):

| Value | Encoding | Note |
|---|---|---|
| 1 | `001 900000` | E = 1, M = 3^17 (one +1 trit at the top): m = 1/3 |
| −1 | `001 9̅00000` | the significand negated |
| 3 | `002 900000` | powers of 3 are exact |
| 1/3 | `000 900000` | |
| 1/2 | `000 DDDDDD` | 0.111…1₃: never exact; neither is 0.1 |

## 4. Kinds of value

| Kind | Encoding | Value |
|---|---|---|
| **zero** | M = 0, E ≠ E_max | 0. Operations write it with E = 0. There is no −0. |
| **normal** | E_min ≤ E < E_max, top trit of M non-zero | M · 3^(E − n), with 1/6 < \|m\| < 1/2 |
| **subnormal** | E = E_min, top trit of M zero, M ≠ 0 | M · 3^(E_min − n): gradual underflow |
| **±∞** | E = E_max, M = ±3^(n−1) (only the top trit set) | infinity, signed like M |
| **NaN** | E = E_max, any other M | not a number. M is its **payload**; the canonical NaN has M = 0. All NaNs are quiet. |

- **Unnormalized encodings** (E > E_min, top trit of M zero, M ≠ 0) are read by their value, as a normal number. No operation produces one.
- **Range.** The largest finite value is M_max · 3^(E_max − 1 − n), just under ½ · 3^9840 ≈ 3.7 · 10^4694. The smallest positive normal is (M_norm + 1) · 3^(E_min − n), just over ⅙ · 3^−9841 ≈ 7.4 · 10^−4697; the smallest positive subnormal is 3^(E_min − n), about 2 · 10^−4730 (real81).
- **Order.** −∞ < every finite value < +∞. A NaN is unordered with everything, itself included.

## 5. Rounding

Every operation whose exact result is not representable rounds it **to nearest**, and an exact tie goes to the **even significand**. There is no other rounding mode.

Precisely, for an exact non-zero result x and a format with n trits:

1. Let **E** be the smallest integer with |x| ≤ 3^E / 2.
2. If E ≥ E_max, the result **overflows**: ±∞ (signed like x), with OF and IX.
3. Otherwise let E = max(E, E_min) and **M** = x · 3^(n − E) rounded to the nearest integer, ties to even. If |M| > M_max(n) (only possible when |x| is exactly 3^E / 2, a tie between the largest significand and a value at E + 1 three times as far), M = ±M_max(n).
4. The result is M · 3^(E − n), with **IX** if it differs from x. If M = 0 the result is zero; if the result is zero or subnormal and differs from x, **UF** is raised too.

Consequences:

- **Ties need a 2 in the denominator.** Sums, differences, products, scalings and conversions from integers are of the form k · 3^j and never tie. Quotients, square roots of non-squares (never exactly halfway, §6.6), remainders' quotients and decimal fractions can.
- **Overflow** happens exactly when |x| > 3^(E_max − 1) / 2; at that value itself the result is the largest finite number.
- **Underflow** is detected after rounding: UF is raised only for an inexact zero or subnormal result.

## 6. Operations

In the tables, *a* and *b* are the operands, *n* the result format's trits, and "round(x)" is §5. Every operation not listed with a flag raises none.

### 6.1 NaN operands

Unless an operation says otherwise, **a NaN operand gives that NaN as the result, unchanged, with no flag**; when both are NaN, the first. (Subtraction a − b with b a NaN gives b itself, not its negation.)

### 6.2 Addition and subtraction

| Case | a + b |
|---|---|
| ∞ + ∞ of the same sign | that ∞ |
| ∞ + ∞ of opposite signs | canonical NaN, **IV** |
| ∞ + finite | the ∞ |
| 0 + 0 | 0 |
| 0 + b | round(b) |
| otherwise | round(a + b) |

a − b is a + (−b). An exact cancellation gives zero with no flag.

### 6.3 Multiplication

| Case | a · b |
|---|---|
| ∞ · 0, 0 · ∞ | canonical NaN, **IV** |
| ∞ · non-zero, non-zero · ∞ | ∞ with the product of the signs |
| 0 · finite | 0 |
| otherwise | round(a · b) |

### 6.4 Division

| Case | a / b |
|---|---|
| ∞ / ∞, 0 / 0 | canonical NaN, **IV** |
| ∞ / finite | ∞ with the product of the signs |
| finite / ∞, 0 / non-zero | 0 |
| non-zero / 0 | ∞ signed like a, **DZ** |
| otherwise | round(a / b) |

### 6.5 Remainder

The IEEE remainder: **r = a − b · q** with q = a / b rounded to the nearest integer, ties to even. The result is always exact (no IX).

| Case | rem(a, b) |
|---|---|
| a = ∞, or b = 0 | canonical NaN, **IV** |
| b = ∞ | round(a) |
| a = 0 | 0 |
| otherwise | r |

### 6.6 Square root

| Case | √a |
|---|---|
| NaN, 0 | a |
| +∞ | +∞ |
| −∞, a < 0 | canonical NaN, **IV** |
| otherwise | round(√a) |

The square root of a representable number is never exactly halfway between two neighbours (that would need a factor 4 in the operand's denominator), so it never ties.

### 6.7 Minimum and maximum

min(a, b) is the smaller and max(a, b) the larger; a **NaN loses to a number** (min(NaN, 3) = 3), and of two NaNs the first wins. When a and b are equal, the result is a. No flags.

### 6.8 Negation and absolute value

−a negates M (exact; zero stays zero, a NaN's payload is negated, ∞ changes sign). |a| is −a when M < 0 and a is not a NaN, else a. No flags.

### 6.9 Comparison

compare(a, b) gives **less**, **equal**, **greater** or **unordered** (either operand a NaN), by the values (−∞ < finite < +∞). It raises no flag. (The FPU's `FCMP` sets S to −1, 0, +1 and C to 0, or S = 0 and C = +1 when unordered.)

### 6.10 Rounding to an integral value

round-to-nearest (ties to even) and round-toward-zero of a, as a real. NaN, ±∞ and 0 are returned unchanged. The result is exact, and **IX** is raised when it differs from a. (A value whose exponent is n or more is already an integer.)

### 6.11 Scaling and the exponent

- **scale(a, k)** = round(a · 3^k), for any integer k (implementations may clamp |k| to 3 · E_max, which changes no result). NaN, ±∞ and 0 are returned unchanged.
- **exponent(a)**: the E with |a| ≤ 3^E / 2 smallest, i.e. the exponent a would have written normalized (below E_min for subnormals); 0 for zero; E_max with **IV** for ∞ and NaN.
- **classify(a)**: 0 zero, ±1 subnormal, ±2 normal, ±3 infinite (signed like the value), 4 NaN.

### 6.12 Conversions

| From | To | Result |
|---|---|---|
| integer (27 trits) | real81 or real27 | exact in real81 (27 ≤ 72); rounded in real27 |
| real | integer (27 trits) | the nearest integer, ties to even, with **IX** if inexact; out of range or ±∞: ±(3^27 − 1)/2 with **IV**; NaN: 0 with **IV** |
| real81 | real27 | round to 18 trits (§5). ±∞ and 0 stay; a NaN keeps its payload's top 18 trits (truncated toward zero), or becomes the canonical NaN if those would read as ∞ |
| real27 | real81 | exact: the word is the real81's first word, the rest 0 |

### 6.13 Decimal text

- **Input** `[+|-]digits[.digits][(e|E)[+|-]digits]` (at least one digit; `_` may separate digits) is converted **correctly rounded**: the exact decimal value, round(x) by §5 once, ties to even. So `0.5` and `2.5` are exact ties and round to the even significand. A value beyond the range overflows (∞, OF and IX) or underflows (0 or a subnormal, UF and IX).
- **Output** with d significant digits is the decimal value of d digits nearest to the real (ties to even), in plain notation for decimal exponents −5 … 14 and scientific (`1.5e20`) otherwise, without trailing zeros; ∞ and NaN are written `inf`, `-inf` and `nan`. (Printing enough digits — 36 for real81, 10 for real27 — gives text that reads back to the same real.)

### 6.14 Constants

An implementation may provide constants; when it does, each must be correctly rounded. The FPU's `FCONST` provides 0, 1, π, e, ln 3, log₃ e and log₃ 10.

## 7. Exceptions

| Flag | Raised by | Masked result |
|---|---|---|
| **IV** invalid | ∞ − ∞, 0 · ∞, 0/0, ∞/∞, √ of a negative, remainder of ∞ or by 0, a conversion to integer out of range or of a NaN, the exponent of ∞ or NaN | the canonical NaN (conversions: the largest integer of the right sign, or 0 for NaN; exponent: E_max) |
| **DZ** divide by zero | finite non-zero / 0 | ∞ signed like the dividend |
| **OF** overflow | a rounded result beyond the largest finite value (§5) | ±∞, with IX |
| **UF** underflow | a zero or subnormal result that is inexact | that result, with IX |
| **IX** inexact | a rounded result that differs from the exact one | the rounded result |

- Flags are **sticky**: once raised they stay until software clears them.
- An implementation that **traps** must do so before writing any result, and must report which exception (the FPU: vector −11, FCAUSE as [`CPU.md`](CPU.md) §6.11 describes). With every exception masked (the default), the masked results above are delivered.

## 8. Conformance

An implementation conforms when it gives every result and every flag of the **conformance vectors**.

- **The vectors** are `crates/real/tests/vectors.txt` (about 14,000), made by `tools/gen_realvectors.py`: an independent reference that computes each case in exact rational arithmetic from this document, sharing no code with any implementation.
- **Format**: one vector per line, `op operands = result flags`. A real is `E:M` (M the 72-trit significand as stored in a real81; a real27 result is a real81 whose low 54 trits are 0); an integer is a decimal; decimal text is in double quotes. Flags are `IV DZ OF UF IX` joined by `+`, or `-`. A comparison's result is `lt`, `eq`, `gt` or `un`.
- **Operations covered**: `add sub mul div rem min max cmp sqrt neg abs rnd trunc to27 scale exp class cvtif cvtfi dec`.
- **Operands**: every pair of special values (zeros, ±∞, canonical and payload NaNs, ±1, 3, 1/3, 2, 1/2, −7/2, 10, the largest finite, the smallest normal, subnormals), 300 random pairs over all exponents (with an emphasis near 0, near overflow and in the subnormal range), cancelling pairs, integers, and exact halves (2k + 1)/2 for the ties of division and remainder; decimal strings including ties (`0.5`, `2.5`), overflow and underflow.
- **The runners**: `crates/real/tests/conformance.rs` checks the library (its fast fixed-width paths and its exact bignum paths separately); `crates/trine/tests/fpu_conformance.rs` runs each vector as the FPU instruction (`FADD` … `FCLASS`, `FCVTIF`, `FCVTFI`, `FTOW`, `FSCL`, `FEXP`) in a ROM program and checks the stored result and FSR flags (for `FCMP`, its S and C).

To regenerate the vectors: `python tools/gen_realvectors.py > crates/real/tests/vectors.txt` (deterministic; about two seconds).

## 9. Differences from IEEE 754

| Topic | IEEE 754 | real27 / real81 |
|---|---|---|
| Radix | 2 (or 10) | 3, balanced |
| Sign | a separate bit; +0 and −0 | in the significand; one zero |
| Hidden bit | the leading 1 is implicit | none: the top trit is stored (it can be +1 or −1) |
| Rounding modes | five | to nearest, ties to even, only |
| Ties | frequent in binary arithmetic | only with a factor 2 in the denominator |
| NaNs | quiet and signalling | quiet only; the payload is the significand |
| Exponent | biased | balanced; the formats share one 9-trit exponent |
| Formats | binary32/64/128 unrelated in layout | real81's first word is a real27 |
| min/max with NaN | (varies by revision) | a NaN loses to a number |
| Remainder | IEEE remainder | the same |

## 10. Implementations

| Implementation | Where | Conformance |
|---|---|---|
| `setun-real`, the reference library | `crates/real` (Rust: fast fixed-width paths, exact bignum fallbacks) | all vectors, both paths |
| The FPU | `crates/core` (interpreter and both JITs call `setun-real`) | all vectors, as instructions |
| Trine's `real` | the host compiler and `tric` (literals: §6.13 input) | literals converted as §6.13; arithmetic is the FPU's |
| `math.tri` | `crates/trine/lib/math.tri` | elementary functions built on the FPU (not covered by this standard) |
