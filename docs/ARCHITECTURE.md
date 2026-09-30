# Setun-70 architecture digest

Primary source: Brusentsov & Zhogolev, *Algorithmic description of the structure
and instruction set of the Setun-70 computer*, MSU Computing Center report
No. 27-VT (417), 1970. The ALGOL text (OCR'd by S. Maslovski) is in
[`reference/Setun70.alg`](reference/Setun70.alg). The overview paper by
Brusentsov, Zhogolev and Maslov (1971) is in `reference/` (Russian). This file
summarizes both. **Where this digest and the ALGOL disagree, the ALGOL wins.**

## Data

| Unit | Trits | Range | Notes |
|---|---|---|---|
| trit | 1 | −1, 0, +1 | written `-`, `0`, `+` |
| tryte ("слог", syllable) | 6 | −364..364 | smallest addressable unit |
| word | 18 | ±193 710 244 | one stack cell = 3 trytes, big-endian |
| double word | 36 | | `S:Y` pair for products, shifts, normalization |

Trit arrays are 1-based and MSB-first: in `k[1:6]`, `k[1]` is the most significant trit.

## Memory

- A page is 81 trytes, addressed −40..+40.
- There are 27 first-level pages, numbered −13..+13. Pages −4..+4 are RAM (9 pages). The other 18 are ROM, which is not writable (`W=S` checks `|page| ≤ 4`).
- Second-level memory: 3 page channels `f[-1..1]`, each up to 6561 pages (the 8-trit `q` register). The prototype had one 243-page drum.
- **Page registers** `h[-1], h[0], h[+1]` (3 trits each) choose which 3 pages address trytes can reach.

## Instruction stream (POLIZ)

A program is a stream of trytes `k[1:6]`, fetched from page `ch`, address `ca`:

- `k[1:2] = 00` → **operation**, with `k[3]` giving the group:
  - `0`: basic operations B1..B27, selected by `k[4:6]` (−13..13 → B1..B27)
  - `+`: special (privileged) operations S1..S27
  - `−`: **macro-operation** (software trap, in effect a system call)
- otherwise → **address tryte**: push a word read from page `h[k2]`, offset `k[3:6]`, length `k1+2` trytes (`k1` = 0 → 1 tryte, +1 → 2, −1 → 3). Missing low trytes are zero-filled. A 2-tryte access through `h[0]` is impossible because it would encode as `00…`, an operation.

## Processor state

| Register | Size | Meaning |
|---|---|---|
| `c1` | 1 | mode: −1 user, 0 macro-op, +1 interrupt/system |
| `ch`, `ca` | 3, 4 | program page, program address |
| `cb`, `cc` | 12, 12 | 2 saved copies of `c[1:8]`, so `c` is a 3-deep mode/PC stack |
| `ph`, `pa` | 2, 3 | stack page (−4..4, RAM only) and top-word index (−13..13) |
| `pb` | 5 | saved `ph:pa` while in interrupt mode (swapped by `EXP`) |
| `h[3]` | 3 each | page registers |
| `e` | 6 | small adder: exponents, indices |
| `R` | 18 | multiplier; shown on front-panel lamps |
| `Y` | 18 | low half of `S:Y` |
| `v[1:9]` | 9 | pending interrupt requests |
| `u[3]`, `g[3]`, `a[3]` | 4, 7, 1 | I/O channel control, data (binary, 7 bits) and "act" flags |
| `q[3]` | 8 | page number for each second-level channel |

The stack word at index `pa` occupies trytes `3·pa−1 .. 3·pa+1` of page `ph`, so
`pa` = −13..13 covers exactly one page. **T** is the word at `pa` (top) and
**S** the word at `pa−1`. `t` is T's first tryte.

## Modes and control transfer

- **Boot**: `c1 = +1`, `ch = 12`, `ca = −40` (a ROM page), `ph = pa = 0`, `Y = 0`.
- **Allowed operations**: user mode allows basic ops and macro-ops. Macro-op mode allows basic and special ops. System mode allows basic and special ops, with interrupts and macro-ops disabled.
- **Macro-op** (only from user mode; system mode halts the machine!): push `c` to `cb` and `cb` to `cc`, set `c1 = 0`, `ch = ca = −13`, push `t = m[−13, ka]`. The **trap table is page −13**. Return with `RMC`.
- **Interrupt** `w`: the same shift of `c`, then `c1 = +1`, `ch = 13`, `ca = −13`, swap `ph:pa ↔ pb`, push `t = m[13, w]`. The **vector table is page 13**:
  - `w` = −40..−32: pending requests `v[1..9]` (timer, I/O ×3, drum ×3, console button, program page exhausted)
  - `w` = −31: stack underflow (op with `pa = −13`)
  - `w` = −30: stack overflow (push with `pa = 13`)
  - `w` = −29: operand crosses the page end
  - `w` = −28: operation not allowed in this mode
- Interrupt requests are checked before each fetch, but only when `c1 ≠ +1`.
- When `ca` passes +40 outside system mode, `v[9]` is set. The OS must then move execution to the next page. **Programs cannot span pages without system software.**
- Branches (`C=T`, `CLT`/`CET`/`CGT`, `COT`) set `ca := t`, so they stay within the current page.

## Basic operations (k = `000` + k[4:6])

| # | Mnemonic | Effect (↓ = pop one word) |
|---|---|---|
| B1 | LST | `S:Y := S:Y · 3^t` (shift) ↓ |
| B2 | COT | if `\|S\| ≥ 3^17/2` then `ca := t` ↓ (overflow branch) |
| B3 | XNN | normalize T:Y, adjust `e` |
| B4–B6 | E-1, E=0, E+1 | `e` −1 / :=0 / +1 |
| B7 | T-E | `T := T − e·3^12` |
| B8 | E=T | `e := t` ↓ |
| B9 | T+E | `T := T + e·3^12` |
| B10–B12 | CLT, CET, CGT | if S <0 / =0 / >0 then `ca := t` ↓ |
| B13 | T=C | `T := 0; t := ca` (overwrites T; `pa` unchanged) |
| B14 | R=T | `R := T` ↓ |
| B15 | C=T | `ca := t` ↓ |
| B16 | T=W | T is replaced by the operand its address tryte `t` points to (indirect load) |
| B17 | YFT | `Y := 0` ↓ |
| B18 | W=S | store S at the address tryte `t` ↓ |
| B19 | SMT | `S := S ⊙ T` (tritwise multiply) ↓ |
| B20 | Y=T | `Y := T` ↓ |
| B21 | SAT | `S := sign(S + T)` tritwise ↓ |
| B22 | S-T | `S := S − T` ↓ |
| B23 | TDN | `T := −T` |
| B24 | S+T | `S := S + T` ↓ |
| B25 | LBT | `S:Y := S·3^18 + T·R·9` ↓ |
| B26 | L*T | `R := S; S:Y := T·R·9` ↓ |
| B27 | LHT | `S:Y := S:Y + T·R·9` ↓ |

Special operations S1..S27 cover I/O channel read/write/control (`CG*`, `LG*`,
`LU*`), second-level page seek and copy (`LQ*`, `CF*`, `LF*`), stack pointer
access (`CP`, `EXP`, `LP`), the `c` stack (`CMC`, `RMC`, `LMC`) and page registers
(`LH*`). See `reference/Описание_наборa_инструкций_Сетунь_70.pdf` and the ALGOL.

## I/O

- 3 character channels, each with up to 8 devices, selected through `u[i, 3:4]`.
- `u[i,1]` is the direction: +1 output, −1 input.
- A channel runs asynchronously and raises `v[i+3]` when a character has been transferred.
- The character code is a 7-bit paper-tape row: bits 1–6 carry the non-zero trits and bit 7 gives the sign. This mapping is lossy; see the 1971 paper, §3.
- The prototype had a Consul-254 typewriter as console, a photoelectric tape reader and a PERFOM-30 punch.

## Open questions / spec ambiguities to resolve during implementation

- Overflow semantics of `S+T` etc. We assume results wrap mod 3^18, which is consistent with `COT` testing the guard trit.
- `WATCH` (timer) period and drum seek timing are unspecified, so they will be configurable.
- The exact semantics of `XNN` when T:Y = 0.
- The 1975 "structural" revision (return stack) is only described in SORUCOM-2011 and Maslovski's `structural` branch.
