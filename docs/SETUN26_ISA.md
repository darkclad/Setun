# Setun-11̅ Instruction Set Architecture

Setun-11̅ (Setun-26): 11̅ is 26 in balanced base 27, one less than 3³.

**Status: v0.8 (2026-10-22): v0.4 (accepted 2026-09-30) plus floating-point instructions (§14), binary operations (§3.3.1), the ternary idioms and vector unit (§3.3, §3.4.1, §15), and the VESA display sizes up to 1920×1200 (§8.3), all implemented.** §13 records the decisions. [`CPU.md`](CPU.md) is the programmer's manual built on this specification.

Setun-11̅ is a balanced-ternary machine sized like an Intel 386: rich
enough for a kernel with processes and a windowing GUI. It is a new CPU
designed from scratch. From the Setun lineage it keeps balanced arithmetic, the
tritwise multiply and threshold-add operations, and the 3-way branch. It has no
hardware Setun-70 compatibility mode. A Setun-70 emulator may one day run on it
as an ordinary program.

## 0. Design principle: everything is a power of 3

Every width, size and count is a power of 3. Binary thinking such as a 24-trit
address (24 = 2³·3) or a 6-trit unit (6 = 2·3) is out.

| Quantity | Value | |
|---|---|---|
| tryte | 3² = 9 trits | the smallest addressable unit |
| word = register = pointer | 3 trytes = 3³ = 27 trits | |
| address space | 3^27 trytes | 3^3^3 |
| address split | 9 \| 9 \| 9 trits | |
| page | 3^9 trytes | |
| general registers | 3³ = 27 | |
| rings | 3 | on one trit |
| interrupt vectors | 3^5 = 243 | |
| I/O ports | 3^9 = 19 683 | a port number is one tryte |
| colours | 3^9 | 27 levels per channel |

Version history:
- v0.1: 24-trit words.
- v0.2: 6-trit trytes and 18-trit words.
- v0.3: the pure-ternary revision.
- v0.4: Setun-70 compatibility mode dropped, default RAM 3^18.
- v0.5: renamed Setun-11̅ (formerly Setun-386, with a separate "Setun-387" FPU); floating-point instructions (§14), the overlined digit notation (§1.1), and the boot signature `0zB007AB13`.
- v0.6: binary operations for data from binary systems (§3.3.1): `BAND`, `BOR`, `BXOR`, `BANDN`, `BSH`, `BEXT`, `BROT`.
- v0.7: `SBC` adds the carry (`ra − b + C`, so `SUB`/`SBC` chain; §3.3); ternary idioms: the trit-mask merge `MRG` and the masked store `STWM` (major 8, §3.4.1), trit counting and reversal `TCP`, `TCN`, `TREV` (§3.3), the three-way compare-and-swap `CAS3` (§3.4.1), and the vector unit (majors 9–11, `MODE.VE`, vector −13; §15).
- v0.8: the VESA display sizes 1024×768, 1280×1024, 1600×1200, 1920×1080 and 1920×1200 (`MODE` +2 … +6); `MODE` keeps its value when written one it does not have (it took the sign before); the framebuffer grows to 243 frames = 3^14 trytes and moves to frames −324 … −82 (`FB_BASE` −6 387 133), so frames −81 … −55 are a hole now (§4, §8.3).

---

## 1. Data

| Name | Trytes | Trits | Range | ≈ bits | 386 analogue |
|---|---|---|---|---|---|
| trit | – | 1 | −1, 0, +1 | 1.58 | bit |
| **tryte** | 1 | 9 | −9 841..9 841 | 14.3 | byte |
| **word** | 3 | 27 | ±3 812 798 742 493 | 42.8 | dword (register, pointer) |

- The **tryte** is the smallest addressable unit, and addresses count trytes.
- The **word** is the register width, the pointer width and the instruction unit.
- **Addressed by the centre.**
  - The word at address `A` occupies trytes `A−1, A, A+1`.
  - Inside a word the order is big-endian: the most significant tryte is at `A−1`.
  - Pages, tables and frames are centred the same way, which balanced ternary forces anyway: a page runs from −9841 to +9841 around its address.
  - Result: **aligned ⇔ the address is a multiple of the size.**
- **Alignment is required** for word accesses (address a multiple of 3), otherwise vector −5 is raised. Aligned words never cross a page.
- **There are no unsigned numbers.**
  - Every value is balanced signed, so loads need no extension variants, and there is one comparison.
  - Negation is exact: flip every trit.
  - Dropping low trits rounds to nearest.
- **Trit positions** are 1-based and most significant first, in the Setun convention. In a word, `x[1]` has weight 3^26 and `x[27]` has weight 3^0. `x[a:b]` is a field.

### 1.1 Notation

A negative digit is the positive one with a bar over it, as on the Setun. Negating a number toggles the bar on every digit.

**Base 3, prefix `0t`:**
- digits `1`, `0` and `1̅` (−1), most significant first; `0t11̅0` = 6;
- `T` is an ASCII alias for `1̅`, so `0t1T0` is the same literal. The two may be mixed in one literal.

**Base 27 ("heptavigesimal"), prefix `0z`:** one digit is 3 trits.

| digit | `D̅` `C̅` `B̅` `A̅` `9̅` … `2̅` `1̅` | `0` | `1` `2` … `9` `A` `B` `C` `D` |
|---|---|---|---|
| value | −13 −12 −11 −10 −9 … −2 −1 | 0 | +1 +2 … +9 +10 +11 +12 +13 |

- Letters may be written in either case (`0zabc` = `0zABC`). Case carries no sign; the bar does.
- A tryte is 3 digits, from `0zD̅D̅D̅` to `0zDDD` (±9841). A word is 9 digits.
- Examples:
  - `001` = 1
  - `001̅` = −1
  - `010` = 27
  - `1D̅` = 27 − 13 = 14
  - `DDD` = +9841, the largest tryte
  - `0zB007AB13` = 115 067 810 199, the boot signature ("BOOTABLE")

**Writing the bar:**
- **In host source files** (Trine, assembler, Rust tests): the digit followed by U+0305 COMBINING OVERLINE, in UTF-8. `_` and spaces may separate digits.
- **In the machine:** an overlined character is the **negated code point**: overlined `1` is −49, overlined `D` is −68. So an overlined digit is still one tryte, one glyph and one screen cell, and "overline" is the same negation as on the number itself. Only code points below −32 are overlined characters. −2 and −3 lead a character above U+2671 (§8.4); −1 and −4…−32 mean nothing (−1 is also what C-style readers return for end of file).
  - **Screen:** the glyph moved down one row, under a bar.
  - **Serial port:** the character followed by U+0305, both ways.
  - **Keyboard:** Right Alt (AltGr) held with the character.
- **Refused in literals:**
  - a bar on `0` (there is no −0), on `T`, on a letter beyond `D`, or on a separator;
  - two bars on one digit, or a bar with no digit before it;
  - the letters `E`–`Z` as digits, and the old `+`/`-` base-3 digits;
  - a value that does not fit the target type.

**Hexadecimal, prefix `0x`:** for values that come from binary systems. It is plain hexadecimal: a sign is written with unary minus, and nothing wraps.

**Dumps** show `0z` digits per tryte (`od -t`), and base 3 on request (`od -3`).

## 2. Registers

### 2.1 General registers

There are 27 general registers of 27 trits, `r-13 … r0 … r13`. A register index is one base-27 digit.

| Register | Hardware role | Suggested ABI role |
|---|---|---|
| `r0` | **always reads 0**; writes are discarded | zero |
| `r1`–`r6` | – | arguments; `r1` = return value |
| `r7`–`r10` | – | caller-saved temporaries |
| `r11` | – | frame pointer `fp` |
| `r12` | – | link register `lr` |
| `r13` | **stack pointer**: swapped with `KSP` on a ring change (§7.3) | `sp`; word-aligned, grows toward negative addresses |
| `r-1`–`r-9` | – | callee-saved |
| `r-10`–`r-13` | – | caller-saved temporaries, assembler scratch |

`PC` is separate and always word-aligned (a multiple of 3).

### 2.2 MODE: status and flags (`cr0`)

| Trit | Field | Values |
|---|---|---|
| 27 | `RING` | current ring: −1 user, 0 drivers, +1 kernel |
| 26 | `IE` | +1 maskable interrupts enabled, 0 disabled, −1 reserved |
| 25 | `PG` | +1 paging on, 0 off |
| 24 | `FE` | +1 the floating-point unit is enabled; 0: FPU instructions raise vector −12 (§14.2) |
| 23 | `IOPL` | lowest ring allowed to use port I/O (reset value 0) |
| 22 | `TF` | +1 single-step trap after each instruction |
| 21 | `S` | **sign of the last result**: −1, 0, +1 |
| 20 | `C` | **carry trit** of the last add/subtract/multiply |
| 19 | `VE` | +1 the vector unit is enabled; 0: vector instructions raise vector −13 (§15.1) |
| 1–18 | – | reserved, read as 0 |

- **S** replaces the binary sign and zero flags.
- **C** is non-zero exactly when a result overflowed. It also feeds `ADC`/`SBC` for multi-word arithmetic.
- **Multi-word arithmetic:** `SUB` sets C so that the exact difference is `rd + C·3^27`, and `SBC` *adds* C, so `SUB` then `SBC` chains a subtraction exactly as `ADD` then `ADC` chains an addition. (Until 2026-10-08 the specification had `SBC` subtract C, which broke the chain; nothing used it.)
- S and C are saved with MODE on every trap.

### 2.3 Control registers

Control registers are `cr-13..cr13`. They are accessed with `MFC`/`MTC` and **require ring +1**.

| CR | Name | Contents |
|---|---|---|
| 0 | `MODE` | §2.2 |
| 1 | `PTB` | physical address of the level-1 page table, a multiple of 3^10 (§5) |
| 2 | `FADDR` | faulting virtual address |
| 3 | `FCAUSE` | `[23:27]` vector (−121..121), `[22]` access type (−1 read, 0 fetch, +1 write), `[21]` ring of the access, `[1:18]` extra info (for example the `SYS` number) |
| 4 | `ITB` | interrupt table base, a multiple of 729 (§7.1) |
| 5 | `EPC` | PC saved on trap entry |
| 6 | `EMODE` | MODE saved on trap entry |
| 7 | `KSP` | the other stack pointer, swapped with `r13` on a ring change |
| 8 | `SCRATCH` | free for the kernel |
| 9 | `CPUID` | read-only: the ISA version in trits 19–27, and feature trits; trit 1 = +1: the FPU is present (always +1 on Setun-11̅; not reused) |
| 10 | `CYCLE` | read-only instruction counter |
| others | – | reserved: reads return 0. Writes to these, and to the read-only `CPUID`/`CYCLE`, raise `ILL` (vector −1). |

## 3. Instruction encoding

- An instruction is **1 or 2 words** (27 or 54 trits), word-aligned.
- Word 1 = **major opcode tryte `A`** + **function tryte `B`** + **9 operand trits**, which is exactly three 3-trit register fields.
- So **1-word instructions are full 3-operand RISC operations**.
- 2-word instructions add a **27-trit immediate**, which reaches any address or constant in a single instruction.

**The length is set by the first trit, `A[1]`:**

| `A[1]` | A (decimal) | Meaning |
|---|---|---|
| 0 | −3280..3280 | 1-word instruction |
| + | 3281..9841 | 2-word instruction (majors are numbered `6561 + n`) |
| − | −9841..−3281 | **floating-point instructions** (§14) |

**An all-zero word is `ILL`**, so running into zeroed memory traps at once.

### 3.1 Formats

```
word 1 trit:   1 ....... 9   10 ...... 18   19-21  22-24  25-27
               [    A     ] [     B      ] [  X  ] [  Y  ] [  Z  ]
word 2:                                                           [ imm27 ]

 1-word  R  :  A B rd ra rb        rd := ra ⊕ rb
         Q  :  A B rd ra q3        q3 = small immediate −13..13
         S  :  A B rd imm6         imm6 = −364..364
 2-word  I  :  A B rd ra | imm27
         J  :  A B rd  –  | imm27
         B3 :  A B  –  –  | offN9 ++ offP9 ++ 0⁹
```

**Branch and jump offsets count words and are relative to the branch instruction itself.** The target is `PC + 3·off`.

### 3.2 Opcode map (major tryte A)

| A | Mnemonic | Format | Operation |
|---|---|---|---|
| **1-word** | | | |
| 0 | SYS group | R/S | system and control (§3.6) |
| 1 | ALU | R | `rd := ra ⊕ rb` (§3.3) |
| 2 | ALUQ | Q | `rd := ra ⊕ q3` (e.g. `ADDQ r13, r13, -3`) |
| 3 | MEM | Q | load or store at `ra + size·q3`: the offset is scaled by the access size (§3.4) |
| 4 | IO | Q | port I/O at port `ra + q3` (§3.7) |
| 5 | `JALR` | Q | `rd := PC + 3; PC := ra + 3·q3` (so `RET` = `JALR r0, r12, 0`) |
| 6 | `LIQ` | S | `rd := imm6` |
| 7 | `BRQ` | S | the `rd` field holds `cond`: if it holds, `PC += 3·imm6` (§3.5) |
| 8 | MEMR | R | read-modify-write memory operations at the address in `ra` (§3.4.1) |
| 9 | VALU | R | vector arithmetic on v registers (§15) |
| 10 | VMEM | R | vector load and store at `ra + 9·q3` (§15) |
| 11 | VMOV | R | vector lane moves and broadcasts (§15) |
| **2-word** | | | |
| 6561 | ALUI | I | `rd := ra ⊕ imm27` |
| 6562 | MEMD | I | load or store at `ra + imm27` (trytes) |
| 6563 | `LI` | J | `rd := imm27`: any constant or address |
| 6564 | `LEA` | J | `rd := PC + imm27` (trytes): PC-relative addresses |
| 6565 | `JAL` | J | `rd := PC + 6; PC := PC + 3·imm27` |
| 6566 | `BR` | J | the `rd` field holds `cond`: if it holds, `PC += 3·imm27` |
| 6567 | `BR3` | B3 | **3-way branch**: S<0 → `PC += 3·offN`; S>0 → `PC += 3·offP`; S=0 → next instruction |
| 6568 | IOI | J | port I/O at port `imm27` |
| 6569 | FLD | I | trit fields: `imm27 = [pos : 9][len : 9][0 : 9]` (§3.3) |
| other | – | – | reserved → `ILL` trap |

### 3.3 ALU functions (B, for ALU / ALUQ / ALUI)

- **Every ALU operation sets `S := sign(result)`.**
- `ADD`, `SUB`, `ADC`, `SBC` and `MUL` also set `C`. Every other operation, including `MULH`, leaves `C` unchanged.
- `b` is `rb`, `q3` or `imm27`, depending on the form.

| B | Mnemonic | Operation |
|---|---|---|
| 1 | `ADD` | `rd := ra + b`, C = carry trit |
| 2 | `SUB` | `rd := ra − b` |
| 3 | `ADC` | `rd := ra + b + C` |
| 4 | `SBC` | `rd := ra − b + C`, C = carry trit (C from the previous `SUB`/`SBC`: the difference's carry is added) |
| 5 | `MUL` | low 27 trits of the product; C := the sign of the high 27 trits (so C ≠ 0 exactly on overflow) |
| 6 | `MULH` | high 27 trits of the 54-trit product |
| 7 | `DIV` | quotient truncated toward zero (C semantics) |
| 8 | `MOD` | remainder of `DIV` |
| 9 | `DIVR` | quotient rounded to nearest (the natural balanced-ternary division) |
| 10 | `MODR` | balanced remainder, `\|r\| ≤ \|b\|/2`; on a tie (even `b`, `\|r\|` = `\|b\|/2`) the remainder is +`\|b\|/2` |
| 11 | `MIN` | tritwise minimum (ternary "AND") |
| 12 | `MAX` | tritwise maximum (ternary "OR") |
| 13 | `TMUL` | tritwise product (Setun-70 `SMT`) |
| 14 | `TSAT` | tritwise threshold add, sign(a+b) (Setun-70 `SAT`) |
| 15 | `CONS` | tritwise consensus: a if a = b, else 0 |
| 16 | `SH` | `rd := ra · 3^b`: left for b > 0 (trits shifted out are lost), right (dropping trits, which rounds) for b < 0; `\|b\|` ≥ 27 gives 0 |
| 17 | `CMP` | only `S := sign(ra − b)`, computed **exactly** (never fooled by overflow); `rd` is not written |
| 18 | `TRIT` | `rd :=` trit number `b` (1..27) of `ra`; 0 for any other `b` |
| 19 | `NEG` | `rd := −ra`, which is also the tritwise NOT |
| 20 | `SGN` | `rd := sign(ra)` |
| 21 | `ABS` | `rd := \|ra\|` |
| 22 | `TLZ` | number of leading zero trits of `ra` (0..27) |
| 23 | `BAND` | binary AND (§3.3.1) |
| 24 | `BOR` | binary OR |
| 25 | `BXOR` | binary exclusive OR |
| 26 | `BANDN` | `ra` AND NOT `b` |
| 27 | `BSH` | `ra · 2^b` for b > 0, `⌊ra / 2^−b⌋` for b < 0; a `b` beyond ±81 acts as ±81 |
| 28 | `MRG` | trit-mask merge: per trit i, `ra[i]` = 0 keeps `rd[i]`, +1 takes `b[i]`, −1 gives 0. **Reads `rd`.** |
| 29 | `TCP` | number of +1 trits among the low `b` trits of `ra` (`b` = 0: all 27) |
| 30 | `TCN` | number of −1 trits among the low `b` trits of `ra` (`b` = 0: all 27) |
| 31 | `TREV` | the low `b` trits of `ra` in reverse order, the trits above them 0 (`b` = 0: all 27) |

`B = 0` and negative values are reserved and raise `ILL`. `MOV rd, ra` is `ADD rd, ra, r0`. In every group a reserved function raises `ILL` before its operands are used, so no alignment or page check comes first (port I/O alone checks `IOPL` first, §3.7).

- `MRG` replaces six ALU operations (`TMUL`, `SUB`, `MAX`, `TMUL`, `ADD` and a temporary): the merged trits are disjoint, so there are no carries. With an all-zero mask nothing changes, and S comes from the old value.
- `TCP`, `TCN` and `TREV` raise `ILL` when `b` is outside 0…27. `TCP + TCN` counts the non-zero trits and `TCP − TCN` is the trit balance (the ternary popcount); `TREV` is the trit-reversal permutation of a radix-3 FFT.

**FLD** (A = 6569):

| B | Mnemonic | Operation |
|---|---|---|
| 1 | `EXT` | `rd := ra[pos : pos+len−1]`, read as a balanced number |
| 2 | `INS` | `rd[pos : pos+len−1] :=` the low `len` trits of `ra` |
| 3 | `BEXT` | `rd :=` bits `pos .. pos+len−1` of `ra` (bit 0 the lowest), as a number 0..2^len−1; 0 ≤ pos ≤ 80 |
| 4 | `BROT` | `rd :=` the low `len` bits of `ra`, rotated left by `pos` within them; 0 ≤ pos < len |

For `EXT` and `INS`, the field must lie in the word: pos ≥ 1, len ≥ 1 and pos + len − 1 ≤ 27. For `BEXT` and `BROT`, 1 ≤ len ≤ 41 (2^41 − 1 is the widest field a word holds). Other fields raise `ILL`. All four set S from the value written to `rd` and leave C unchanged.

#### 3.3.1 Binary operations

Data from binary systems (octets, 32-bit words of SHA-2 and ChaCha20, checksums) needs AND, XOR and rotations. The binary operations read a word's **value** as a two's-complement integer of unlimited width: 5 is …000101, −1 is all ones, −6 is …111010. There is no binary register width, so no power of 2 is built into the machine: the program names the width where one matters, in `BEXT` and `BROT`.

- `BAND`, `BOR`, `BXOR` and `BANDN` compute the exact integer result. It lies within ±2^42; one that is outside the word range wraps modulo 3^27, as `ADD` does. Values below 2^41 never wrap, so 32-bit data is always exact.
- `BSH` multiplies by a power of 2 (wrapping modulo 3^27) or divides by one, rounding toward −∞ like an arithmetic shift.
- A 32-bit sum is `ADD` then `BEXT rd, rd, 0, 32`. A 32-bit right rotation by n is `BROT` left by 32 − n.
- All seven set S from the result and leave C unchanged.

### 3.4 Memory functions (B, for MEM and MEMD)

| B | Mnemonic | Operation |
|---|---|---|
| 1 | `LDT` | load a tryte. The value is preserved, so no extension variants are needed. |
| 2 | `LDW` | load a word (the address must be a multiple of 3) |
| 3 | `STT` | store the low tryte of `rd` |
| 4 | `STW` | store `rd` |
| 5 | `SWAP` | atomically exchange `rd` with the word at the address |

In the 1-word MEM form the offset is `q3` **elements**: `LDW r1, [r13 + 2]` reads the word at `r13 + 6`.

#### 3.4.1 Read-modify-write (MEMR: A = 8, R format)

| B | Mnemonic | Operation |
|---|---|---|
| 1 | `STWM rd, [ra], rb` | the word at `ra` := `MRG`(that word, mask `rb`, `rd`), as one atomic read-modify-write: per trit, `rb` = 0 keeps the memory trit, +1 takes `rd`'s, −1 clears it |
| 2 | `CAS3 rd, [ra], rb` | `old` := the word at `ra`; **S := sign(`old` − `rd`)**, exact; if S = 0 the word := `rb`; then `rd` := `old`. Atomic. A `BR3` right after sees three outcomes: smaller than expected, equal (swapped), larger |

- There is no offset (the R format holds three registers and no immediate): the address is computed first. It must be a multiple of 3 (vector −5), and the page must be writable (vector −4). Both are write accesses, checked before anything is read: a trap leaves memory and `rd` unchanged, and `FCAUSE` reports a write. `STWM` leaves S unchanged. `CAS3` needs a writable page even when the comparison fails (it is a write access either way, like x86's `CMPXCHG`).
- A mutex fits one trit: 0 free, +1 held, −1 held with waiters. Lock is `CAS3` from 0 to +1, else `SWAP` in −1 and sleep until the swap finds 0; unlock is `SWAP` in 0, waking a waiter if the old value was −1 (Trinux: `mtx_lock`, `mtx_unlock` in tlibc, over `futex`).
- Uses: page-table entry flags (`V`, `AD`, `RING`) set or cleared in one store, device control registers, flag words shared between the kernel and drivers.
- Other functions are reserved and raise `ILL`.

### 3.5 Branch conditions (the 3-trit `cond` in the rd field)

Each trit position belongs to one sign value: `cond[1]` ↔ −1, `cond[2]` ↔ 0, `cond[3]` ↔ +1.

- A **+** trit means "branch if **S** has that value".
- A **−** trit means "branch if **C** has that value".
- Mixing + and − trits in one condition is reserved.

| cond | Alias | Meaning |
|---|---|---|
| `+00` | `BLT` | S < 0 |
| `0+0` | `BEQ` | S = 0 |
| `00+` | `BGT` | S > 0 |
| `++0` | `BLE` | S ≤ 0 |
| `0++` | `BGE` | S ≥ 0 |
| `+0+` | `BNE` | S ≠ 0 |
| `+++` | `B` | always |
| `-0-` | `BOV` | C ≠ 0 (overflow) |
| `0-0` | `BNOV` | C = 0 |
| `000` | – | never |

- `BRQ` reaches ±364 words.
- `BR` reaches the whole address space.
- **`BR3`** is the Setun's 3-way jump. After `CMP a, b`, one instruction dispatches on less / equal / greater.

### 3.6 SYS group (A = 0, 1-word)

| B | Mnemonic | Ring | Operation |
|---|---|---|---|
| 0 | `ILL` | any | illegal instruction trap (vector −1) |
| 1 | `NOP` | any | no operation |
| 2 | `HLT` | +1 | wait for an interrupt; `PC` already points at the next instruction, which is where the handler returns to |
| 3 | `BRK` | any | breakpoint trap (vector −7) |
| 4 | `SYS n` | any | system call: vector 0, with `n` = the 9 operand trits (±9841) placed in `FCAUSE[1:18]` |
| 5 | `IRET` | +1 | return from trap (§7.3) |
| 6 | `MFC rd, cr` | +1 | `rd := cr[ra-field]` |
| 7 | `MTC cr, ra` | +1 | `cr[rd-field] := ra` |
| 8 | `TLBINV ra` | +1 | invalidate the translation for virtual address `ra` |
| 9 | `TLBFLUSH` | +1 | invalidate all translations |
| 10 | `FENCE` | any | order memory and I/O (a no-op in the v1 emulator) |

### 3.7 Port I/O (IO: A = 4; IOI: A = 6568)

Port I/O requires `RING ≥ MODE.IOPL`, otherwise vector −2 is raised. **A port transfers a word.**

| B | Mnemonic | Operation |
|---|---|---|
| 1 | `IN rd, port` | `rd := port[p]` |
| 2 | `OUT rd, port` | `port[p] := rd` |

The IO form's port is `ra + q3`: put a device's base port in a register and use `q3` to pick one of its 27 registers.

### 3.8 Pseudo-instructions (assembler)

| Pseudo | Expands to |
|---|---|
| `LI rd, n` | `LIQ` if \|n\| ≤ 364, otherwise `LI` |
| `MOV rd, ra` | `ADD rd, ra, r0` |
| `CALL f` | `JAL r12, f` |
| `RET` | `JALR r0, r12, 0` |
| `PUSH ra` | `ADDQ r13, r13, -3` then `STW ra, [r13]` |
| `POP rd` | `LDW rd, [r13]` then `ADDQ r13, r13, 3` |
| `NOT rd, ra` | `NEG rd, ra` |

## 4. Physical memory map

Memory is divided into **frames of 3^9 = 19 683 trytes**. Frame `f` is centred on `19683·f` and covers `19683·f ± 9841`. The physical address space is the full 27 trits, because frame numbers are 18 trits.

| Frames | Physical addresses | Contents |
|---|---|---|
| `0 … N−1` | from −9 841 upward | **RAM**. The default is **N = 3^9 frames = 3^18 = 9^9 trytes** (387 420 489 trytes ≈ 691 MB), a "page of pages". Configurable from 3^15 up to host RAM; the browser (WASM) build is capped at 3^18. |
| −3 … −1 | −68 890 … −9 842 | **BIOS ROM**, 3^10 trytes (§9.2) |
| −324 … −82 | −6 387 133 … −1 604 165 | **framebuffer**, 243 frames = 3^14 trytes (§8.3) |
| other negative frames | – | reserved for devices; accessing them raises a bus error (vector −10) |

Rule of thumb: **non-negative frames are RAM, negative frames are devices.**

## 5. MMU

### 5.1 Virtual addresses: 9 | 9 | 9

A virtual address is a whole word, **27 trits = 3^3^3**. It needs no canonical-form rule.

```
  trit:  1 ........ 9   10 ....... 18   19 ....... 27
        [  L1 index  ] [  L2 index   ] [   offset    ]
         3^9 entries     3^9 entries     −9841..9841
```

- There are 2 table levels, like the 386.
- **A page-table entry is one word.**
- Each table has 3^9 entries × 3 trytes = **3^10 trytes = 3 contiguous pages**, and must sit at a multiple of 3^10.
- Entry `i` (−9841..9841) of a table centred at `T` is the word at `T + 3·i`.

**(memory cost)**: a minimal process needs an L1 table plus one L2 table, 6 pages or about 118k trytes. That is under 1% of the default RAM. A sparse "small L1" option can come later if it matters.

### 5.2 Page-table entry (1 word = 27 trits = frame 18 + flags 9)

A **zero entry means "not present"**, so zeroed memory is a valid empty table.

| Trits | Field | Values |
|---|---|---|
| 1–18 | `FRAME` | physical frame number (balanced) |
| 19 | `V` | 0 not present, +1 read/write, −1 read-only |
| 20 | `X` | 0 no execute, +1 execute, −1 execute-only (not readable) |
| 21 | `RING` | lowest ring that may access the page: −1 user, 0 drivers, +1 kernel only |
| 22 | `AD` | set by hardware: 0 untouched, −1 accessed, +1 written (dirty) |
| 23 | `G` | +1 global: kept by `TLBFLUSH` (for kernel mappings) |
| 24–25 | `SW` | 2 trits free for the OS (for example copy-on-write) |
| 26–27 | – | reserved, must be 0 |

### 5.3 Walk

1. `E1 = word at PTB + 3·a[1:9]`.
2. `E2 = word at 19683·E1.FRAME + 3·a[10:18]`.
3. `PA = 19683·E2.FRAME + a[19:27]`.

If `V = 0` at either level, a page fault (vector −3) is raised. The permissions checked are the **more restrictive of the two levels**. The levels are checked in order: `E1` completely (present, then permissions) before `E2` is read, so an `E1` that forbids the access raises −4 even when `E2` is absent.

- a write needs `V = +1`;
- an instruction fetch needs `X ≠ 0`;
- a data read needs `X ≠ −1`;
- `MODE.RING ≥ RING` must hold.

A violation raises vector −4. `FADDR` receives the virtual address and `FCAUSE` the access type and ring. The hardware updates `AD` in E2 after both levels pass: a write sets +1; a read or fetch sets −1 if it was 0. An instruction whose later access faults keeps the `AD` updates of its earlier accesses (a `SWAP` on a read-only page marks it accessed).

### 5.4 TLB

The TLB is not architecturally visible.
- After changing a PTE, software uses `TLBINV`.
- After writing `PTB`, `TLBFLUSH`, which keeps `G` entries.
- Writing `PTB` does not flush implicitly.

### 5.5 Instruction fetch across pages

A 2-word instruction in the last word of a page continues on the next page. Its second word is translated separately. If that fetch faults, `FADDR` holds the second word's address and the instruction has no effect.

## 6. Privilege rings

There are 3 rings on one trit: **−1 user, 0 drivers, +1 kernel**. Every check is "current ring ≥ required ring".

| Resource | Requirement |
|---|---|
| `MFC`, `MTC`, `IRET`, `HLT`, `TLBINV`, `TLBFLUSH` | ring +1 |
| `IN`, `OUT` | `RING ≥ MODE.IOPL` (default 0: drivers and kernel) |
| a page | `RING ≥ RING` at both levels |

Only a trap or interrupt raises the ring. Only `IRET` lowers it.

## 7. Traps and interrupts

### 7.1 Vector table

There are **243 = 3^5 vectors, numbered balanced −121..121**. Each entry is one word holding a handler address, at `ITB + 3·v`. The table is 729 trytes, centred on `ITB`. With paging on, `ITB` is a virtual address.

| Vector | Cause | `EPC` holds |
|---|---|---|
| **0** | **system call** (`SYS`) | the next instruction |
| −1 | illegal or reserved instruction | the instruction |
| −2 | privilege violation (ring or `IOPL`) | the instruction |
| −3 | page fault (not present) | the instruction |
| −4 | protection fault | the instruction |
| −5 | misaligned access | the instruction |
| −6 | divide by zero | the instruction |
| −7 | breakpoint (`BRK`) | the next instruction |
| −8 | single step (`TF`) | the next instruction |
| −9 | double fault (a fault while fetching a vector) | – |
| −10 | bus error (no device at the physical address) | the instruction |
| −11 | floating-point exception (an unmasked FPU exception, §14.4) | the instruction |
| −12 | FPU not available (`MODE.FE` = 0) | the instruction |
| −13 | vector unit not available (`MODE.VE` = 0) | the instruction |
| −14 … −26 | reserved CPU exceptions | |
| −27 … −121 | free for OS use | |
| **+1 … +81** | **hardware IRQ 0–80** (81 = 3^4 lines; vector = IRQ + 1) | the next instruction |
| +82 … +121 | free for OS use | |

Faults are negative, the system call sits at the centre, and the outside world is positive. A fault while handling a double fault resets the machine (a "triple fault").

### 7.2 Entry

1. `EMODE := MODE`, `EPC :=` the address from the table above, and `FADDR`/`FCAUSE` are set. `FCAUSE`'s vector and ring are always defined, and its info field for `SYS`; the access trit and `FADDR` only for the memory faults (−3, −4, −5, −10). For other traps their values are the implementation's (the reference emulator writes −1 and 0).
2. If `MODE.RING < +1`: **swap `r13 ↔ KSP`** (switch to the kernel stack).
3. `MODE.RING := +1; IE := 0; TF := 0`.
4. `PC := word at ITB + 3·vector`.

every trap enters ring +1. The kernel dispatches to driver-ring code in software.

### 7.3 `IRET`

1. `MODE := EMODE; PC := EPC`.
2. If the new `RING < +1`: swap `r13 ↔ KSP`.

A kernel must save `EPC`/`EMODE` before re-enabling interrupts if it wants nested traps.

## 8. Devices

### 8.1 I/O port map (3^9 ports instead of 3^10)

- There are **3^9 = 19 683 ports**, so **a port number is exactly one tryte** (−9841..9841).
- Each device gets a **block of 27 ports, centred**: block `b` holds ports `27·b − 13 … 27·b + 13`.
- The device register at offset `r` is port `27·b + r`, which suits the `IN rd, [ra + q3]` form.
- There are 729 possible blocks. Negative blocks are reserved for expansion buses.

| Block | Base port | Device | IRQ |
|---|---|---|---|
| 0 | 0 | system control (`SYSCTL`) | – |
| 1 | 27 | interrupt controller (`TPIC`) | – |
| 2 | 54 | interval timer | 0 |
| 3 | 81 | real-time clock | 5 |
| 4 | 108 | keyboard | 1 |
| 5 | 135 | mouse | 2 |
| 6 | 162 | serial debug console | 3 |
| 7 | 189 | disk 0 | 4 |
| 8 | 216 | disk 1 (reserved) | 7 |
| 9 | 243 | display controller | 6 |
| 10 | 270 | network card (`TNIC`) | 8 |
| 11 | 297 | random-number generator (`TRNG`) | – |
| 12 | 324 | ternary link (`TLINK`) | 9 |

### 8.2 Registers (offset from the block base)

Control values are ternary wherever that is natural.

**SYSCTL (0)**

| Offset | Register |
|---|---|
| 0 | `SYSID` (read-only): 26 (`0t100T`) |
| 1 | `RAM_FRAMES` (read-only) |
| 2 | `POWER`: write +1 to reset, −1 to power off |
| 3 | `DEBUG`: write a tryte and it appears in the host log |
| 4 | `BOOTINFO` (read-only) |

**TPIC (27), interrupt controller.** There are 81 IRQ lines (0–80), and a lower number means higher priority. All IRQs are masked at reset.

| Offset | Register |
|---|---|
| 0 | `VECBASE`: vector of IRQ 0, default +1 |
| 1 | `CLAIM` (read): returns the highest-priority pending unmasked IRQ and marks it in service, or −1 if there is none. A line interrupts the CPU only while no line of equal or higher priority is in service. |
| 2 | `EOI` (write): the IRQ number |
| 3 | `UNMASK` (write): the IRQ number |
| 4 | `MASK` (write): the IRQ number |
| 5 | `SELECT` |
| 6 | `STATE` of the selected IRQ: −1 masked, 0 idle, +1 pending |

**Timer (54).** 1 MHz time base. Writing `CTRL` reloads `COUNT` from `PERIOD`. A periodic timer keeps its phase, and several missed periods raise the IRQ once.

| Offset | Register |
|---|---|
| 0 | `CTRL`: −1 one-shot, 0 stop, +1 periodic |
| 1 | `PERIOD` (µs) |
| 2 | `COUNT` (µs remaining) |
| 3 | `UPTIME` (µs since reset; wraps after about 44 days) |

**RTC (81)**

| Offset | Register |
|---|---|
| 0 | `SECONDS`: Unix time. A word holds ±3.8·10^12 seconds, so there is no year-2038 problem. Writing it also zeroes `MICROS`. |
| 1 | `MICROS` |
| 2 | `ALARM`: Unix time, or 0 for off. Raises IRQ 5. |

**Keyboard (108)**

| Offset | Register |
|---|---|
| 0 | `DATA`: the next key event, **+code on press, −code on release**, 0 if the queue is empty. Codes are USB HID usage IDs. |
| 1 | `COUNT`: number of queued events |
| 2 | `CTRL`: +1 IRQ on, 0 off, −1 flush |
| 3 | `LEDS` |

"PS/2-style" means a queued event device with an interrupt. It is not bit-compatible with PS/2.

**Mouse (135)**

| Offset | Register |
|---|---|
| 0 | `DX` (reading clears it) |
| 1 | `DY` (reading clears it) |
| 2 | `BUTTONS` = `L + 3·M + 9·R` (1 = down). Reading pops the oldest queued button change and latches the position where it happened (see 7); with none queued it is the current state. |
| 3 | `WHEEL` |
| 4 | `CTRL` |
| 5 | `ABS_X` (absolute position, for the browser; after a `BUTTONS` read that popped a change, that change's position) |
| 6 | `ABS_Y` |
| 7 | `PENDING`: button changes still queued (a FIFO of 27), so a quick press and release between two reads is not lost |

**Serial console (162)**

| Offset | Register |
|---|---|
| 0 | `DATA`: one tryte character (§8.4). Reading returns 0 if nothing has been received. |
| 1 | `RX_COUNT` |
| 2 | `TX_READY`: +1 ready, 0 busy |
| 3 | `CTRL`: +1 IRQ on receive |

**Disk 0 (189).** A sector is **one page, 3^9 trytes**. DMA uses **physical, page-aligned** addresses and bypasses the MMU.

| Offset | Register |
|---|---|
| 0 | `CMD`: writing starts the operation: −1 read, +1 write, 0 flush |
| 1 | `STATUS`: −1 error, 0 idle, +1 busy |
| 2 | `LBA` |
| 3 | `COUNT` (sectors) |
| 4 | `DMA` (physical address) |
| 5 | `SECTORS` (read-only capacity) |
| 6 | `ERROR`: 0 none, 1 no media, 2 LBA out of range, 3 DMA address not page-aligned, 4 bus error during DMA, 5 host I/O error, 6 bad command |
| 7 | `CTRL`: +1 IRQ on completion |

**Display (243)**

| Offset | Register |
|---|---|
| 0 | `MODE`: −1 text 80×30, 0 = 640×480, +1 = 800×600, +2 = 1024×768, +3 = 1280×1024, +4 = 1600×1200, +5 = 1920×1080, +6 = 1920×1200; writing any other value leaves the mode as it is |
| 1 | `FB_BASE` (read-only physical address of the first tryte) |
| 2 | `WIDTH` |
| 3 | `HEIGHT` |
| 4 | `PITCH` (trytes per line). In text mode `WIDTH`/`HEIGHT` count cells (80×30) and `PITCH` is 240 (80 one-word cells). |
| 5 | `FRAME` (vsync counter) |
| 6 | `CTRL`: +1 IRQ on vsync |
| 7 | `CURSOR` (text cell index) |
| 8 | `HWC_X` (hardware mouse cursor) |
| 9 | `HWC_Y` |
| 10 | `HWC_ON` |

**Network card (270), `TNIC`.** Ethernet II frames without the FCS, 14..1514 octets, **one octet (0..255) per tryte**. DMA uses physical tryte addresses of any alignment and bypasses the MMU. Every command completes before the next instruction. Details and rationale: [`NETWORK.md`](NETWORK.md) §1.

| Offset | Register |
|---|---|
| 0 | `CMD`: +1 send `TX_LEN` octets from `TX_ADDR`; −1 copy the oldest received frame to `RX_ADDR` and drop it from the queue; 0 drop it without copying |
| 1 | `STATUS`: 0 ok, −1 the last command failed |
| 2 | `TX_ADDR` |
| 3 | `TX_LEN` |
| 4 | `RX_ADDR` |
| 5 | `RX_LEN` (read-only): length of the oldest received frame, 0 if none |
| 6 | `RX_COUNT` (read-only): frames queued (at most 27) |
| 7 | `CTRL`: +1 IRQ 8 when a frame arrives |
| 8 | `MAC_HI` (read-only): MAC octets 0..2 as `o0·65536 + o1·256 + o2` |
| 9 | `MAC_LO` (read-only): octets 3..5 |
| 10 | `LINK` (read-only): +1 a network is attached |
| 11 | `ERROR`: 0 none, 1 no link, 2 bad length, 3 a tryte outside 0..255, 4 bus error during DMA, 5 nothing received, 6 bad command, 7 transmit queue (27 frames) full |
| 12 | `DROPS` (read-only): received frames lost to a full queue |

**Ternary link (324), `TLINK`.** Frames of 1..2187 (3^7) trytes of **any value**, to and from a point-to-point link to another Setun machine; the card checks nothing (the link layer's TCRC-18 does). DMA and commands work as for `TNIC`. Details: [`TNET.md`](TNET.md) §1.

| Offset | Register |
|---|---|
| 0 | `CMD`: +1 send `TX_LEN` trytes from `TX_ADDR`; −1 copy the oldest received frame to `RX_ADDR` and drop it; 0 drop it without copying |
| 1 | `STATUS`: 0 ok, −1 the last command failed |
| 2–4 | `TX_ADDR`, `TX_LEN`, `RX_ADDR` |
| 5 | `RX_LEN` (read-only) |
| 6 | `RX_COUNT` (read-only, at most 27) |
| 7 | `CTRL`: +1 IRQ 9 when a frame arrives |
| 8 | `LINK` (read-only): +1 a link is attached |
| 9 | `ERROR`: 0 none, 1 no link, 2 bad length, 4 bus error during DMA, 5 nothing received, 6 bad command, 7 transmit queue full |
| 10 | `DROPS` (read-only) |
| 11 | `ADDR` (read-only): the station address the front end gave the card |

**Random-number generator (297), `TRNG`.** A ChaCha20 key stream (RFC 8439 block function) under a 256-bit seed from the front end; with none, the seed is zero and every run gives the same numbers. Details: [`NETWORK.md`](NETWORK.md) §8.

| Offset | Register |
|---|---|
| 0 | `DATA` (read): the next random octet, 0..255 |
| 1 | `SOURCE` (read-only): +1 seeded from the host's entropy, 0 a fixed seed |

### 8.3 Framebuffer formats

- **Graphics: one tryte per pixel, 3^9 = 19 683 colours.**
  - Trits 1–3 are **R**, 4–6 are **G** and 7–9 are **B**. Each is a balanced −13..+13 value, giving **27 = 3³ levels per channel**.
  - The intensity is `(v+13)/26`, shown as `round(255·(v+13)/26)` in sRGB.
- In base 27, **a pixel is its three colour digits**:
  - `DDD` white
  - `D̅D̅D̅` black
  - `DD̅D̅` red
  - `D̅DD̅` green
  - `D̅D̅D` blue
  - `000` 50% grey (so a zeroed framebuffer is mid-grey)
  - `DD0` yellow-ish
  - Negating a pixel inverts its colour.
- Pixel `(x, y)` is the tryte at `FB_BASE + y·PITCH + x`.
- The largest mode, 1920×1200, needs 2 304 000 trytes, which fit the 3^14-tryte region (4 782 969).
- **Text mode: 80×30 cells, one word per cell** (character, attribute, reserved), rendered with an 8×16 font to 640×480. Cell `k` occupies trytes `FB_BASE + 3k` (character), `FB_BASE + 3k + 1` (attribute) and `FB_BASE + 3k + 2` (reserved). The v1 emulator's font is 8×8 with doubled rows. It covers Latin, Greek, box drawing, blocks and the Cyrillic block U+0400..U+045F (Russian, Ukrainian, Belarusian, Serbian, Macedonian; drawn for the machine in the same style, see `crates/core/src/devices/font.rs`). Other characters show as a hollow box.
  - The **attribute tryte** is foreground RGB in trits 1–3 and background RGB in trits 4–6, each trit 0%, 50% or 100%, so 27 colours each. Trits 7–9 are style: underline, blink, bold.

### 8.4 Characters: a tryte holds Unicode directly

- A 9-trit tryte covers ±9841.
- **A positive tryte is a Unicode code point, U+0000..U+2671.** That covers ASCII, Latin-1, Latin Extended, Greek, **Cyrillic**, Hebrew, Arabic, Devanagari, general punctuation, arrows, box drawing, blocks and geometric shapes.
- **A negative tryte below −32 is an overlined character** (§1.1).
- **A character above U+2671 is three trytes:** the lead −2 (−3 when overlined), then two trailing trytes, both in −9841…−33: with n = code point − 9842, they are −(33 + n div 9809) and −(33 + n mod 9809). Every Unicode scalar value fits (9809² is far beyond U+10FFFF).
  - A trailing tryte is never 0, a newline, any ASCII character or a lead, so NUL-terminated strings, `strlen` (which counts trytes), `strchr` for ASCII and line splitting all keep working. A tool that reads trytes one at a time sees two overlined characters.
  - Malformed, refused by every decoder (or shown as U+FFFD): a lead −1 or −4…−32, a lead not followed by two trailing trytes, a code point above U+10FFFF, a surrogate (U+D800…U+DFFF). There is no overlong form: the encoding starts at U+2672.
  - The form is not self-synchronizing; text is decoded forward from a known start.
- For nearly all text, one character is one tryte, and the serial console, keyboard, text mode and the compiler's `char` all use this encoding. The serial port carries a 3-tryte character as the one Unicode character, both ways.

### 8.5 Host file format for tryte data (T1B2)

Disk images, ROM images and memory dumps store **one tryte per little-endian `i16`**. That works because 3^9 = 19 683 < 2^15, and it wastes 10.9%.

One sector is 39 366 bytes.

## 9. Reset and boot

1. **Reset state**:
   - native mode, `RING = +1`, `PG = 0`, `IE = 0`;
   - all general and control registers 0;
   - every TPIC IRQ masked;
   - display in text mode;
   - **`PC = −59 049`** (−3^10, the centre of ROM frame −3).
2. **BIOS** (ROM frames −3..−1, 3^10 trytes of native code):
   - set `ITB`;
   - read the RAM size from `SYSCTL`;
   - clear the text screen and run a self-test;
   - load **disk sector 0 into frame 1**, check the boot signature `0zB007AB13` in its last word, then jump to its centre (physical 19 683 = 3^9) with `r1` = boot disk and `r2` = RAM frame count. This is the analogue of the PC's 0x7C00 boot sector. The full protocol is in [`BOOT.md`](BOOT.md).
3. **Boot sector / loader**: loads the kernel, builds page tables, sets `PTB`, turns on `PG`, and enters the kernel.

## 10. Notes for the compiler

| C type | Size |
|---|---|
| `char` | 1 tryte: one Unicode character (§8.4) |
| `int`, pointers | 1 word (27 trits, ±3.8·10^12) |
| `long` | 3 words (81 trits), in software via `ADC`/`MULH` (optional) |

- `sizeof` counts trytes.
- There is no `unsigned`.
- Integer division truncates (`DIV`/`MOD`).
- Tritwise operators map to `MIN`/`MAX`/`TMUL`/`TSAT`/`CONS`.
- Struct fields are aligned to their own size.
- A 3-way `switch` on the sign of a comparison compiles to a single `BR3`.

## 11. Example

Print "Hi" on the serial console, then halt (a ROM at the reset address; `setun-asm build hi.s --rom`, then `setun --rom hi.rom`):

```
        .org -59049
        li    r1, 'H'          ; 72
        call  putc
        li    r1, 'i'          ; 105
        call  putc
        hlt

putc:   li    r3, 162          ; serial block base
wait:   in    r2, [r3 + 2]     ; TX_READY
        cmp   r2, 0
        ble   wait             ; wait until TX_READY > 0
        out   r1, [r3 + 0]     ; DATA
        ret
```

[`CPU.md`](CPU.md) §14 has more examples.

## 12. Setun-11̅ vs i386 at a glance

| | i386 | Setun-11̅ |
|---|---|---|
| smallest unit | byte = 2³ bits | tryte = 3² trits (≈ 14.3 bits) |
| register / pointer | 32 = 2^5 bits | 27 = 3³ trits (≈ 42.8 bits) |
| general registers | 8 | 27 |
| address space | 2^32 bytes | 3^27 trytes (≈ 1.4·10^13 bytes equivalent) |
| paging | 4 KiB pages, 10\|10\|12 | 3^9-tryte pages (≈ 35 KB), 9\|9\|9 |
| rings | 4 | 3 on one trit |
| flags | many bits | S and C trits |
| I/O ports | 2^16 | 3^9 |
| interrupt vectors | 256 | 243 |
| colours (VGA-class) | 256 | 19 683 |
| reset | real mode (8086) | native, ring +1, paging off |

## 13. Decisions

1. **The pure-ternary sizes** (§0): the tryte is 9 trits, the word, register and pointer are 27 trits, addresses are 3^27, and the split is 9|9|9.
2. **Centre addressing** and **required alignment** (§1). Only 2-word instructions can cross a page.
3. **Encoding**: 1-word instructions are 3-operand; 2-word instructions carry a 27-trit immediate (§3). Major opcodes are numbered in decimal; 2-word majors are `6561 + n`.
4. **Page tables are 3 pages each** (3^9 entries), costing about 118k trytes per process at minimum (§5.1).
5. **Ports are 3^9, not 3^10**, so a port number is one tryte (§8.1).
6. **Vectors are numbered −121..121**: faults negative, the system call at 0, IRQs positive (§7.1).
7. **Characters: a tryte is a Unicode code point** (U+0000..U+2671, with negative leads for the rest) (§8.4).
8. **Colours: 3^9, with 27 levels per channel.** A pixel's three base-27 digits are R, G, B (§8.3).
9. **No Setun-70 compatibility mode**: a new CPU from scratch that resets straight into native mode (§9). **Default RAM is 3^18 = 9^9 trytes** (§4).
10. A **carry trit C**, **two kinds of division**, **every trap enters ring +1**, and **text mode**. These carry over from v0.2.
11. **Floating-point instructions** (§14): real27 and real81 with one shared 9-trit exponent, a signed balanced significand (no sign trit, no −0), 27 flat registers, ±∞/NaN/subnormals, round to nearest with ties to even, and a "not available" trap for lazy context switching.
12. **Binary operations** (§3.3.1): values read as two's-complement integers of unlimited width, so the ISA fixes no binary width; `BEXT` and `BROT` take the width as an operand. Added in v0.6 for crypto: SHA-2 and ChaCha20 had needed 256×256 tables.

## 14. The floating-point unit

The formats, rounding, operations and exceptions are specified on their own in [`REAL.md`](REAL.md), the real27/real81 standard, with its conformance vectors; this section gives the FPU that implements them.

The i386 needed a separate 387 coprocessor; the 486DX built it in, and so does this machine. The FPU is part of the CPU and of this ISA, not an option: `CPUID` trit 1 is always +1. It owns the third of the opcode space whose first trit is −.

### 14.1 Formats

A floating-point value is **m · 3^E**, with E an integer and m a balanced fraction. **The significand carries the sign:** negating a value negates its significand, so there is **no sign field and no −0**.

| Format | Size | Exponent E | Significand M | Precision |
|---|---|---|---|---|
| **real27** | 1 word | trits 1–9 (a tryte) | trits 10–27: 18 trits | 18 trits ≈ 28.5 bits (IEEE single: 24) |
| **real81** | 3 words (81 trits) | trits 1–9 of word 0 | the other 72 trits, most significant first | 72 trits ≈ 114 bits (IEEE quad: 113) |

- **Value:** M is an integer with n trits (n = 18 or 72), and m = M · 3^−n. So m is a fraction with the point just left of M's top trit, and |m| < 1/2.
- **Both formats share the exponent**, which is one tryte, −9841…9841. The first word of a real81 *is* a real27: E and the top 18 trits of M. Dropping trits in balanced ternary rounds to nearest, so reading only the first word gives the real81 rounded to real27 (see §14.3 for the one exception, exact halves).
- **Kinds of value**, with E_max = 9841 and E_min = −9841:

| Kind | Encoding | Meaning |
|---|---|---|
| zero | M = 0 and E ≠ E_max | 0. Results always write E = 0. |
| normal | top trit of M non-zero, E_min ≤ E < E_max | m · 3^E, with 1/6 < \|m\| < 1/2 |
| subnormal | E = E_min, top trit of M zero, M ≠ 0 | gradual underflow: m · 3^E_min |
| ±∞ | E = E_max, M = ±3^(n−1) (only the top trit set) | infinity, signed like M |
| NaN | E = E_max, any other M | not a number; the canonical NaN has M = 0 ("an infinity with no direction"). M is its payload. All NaNs are quiet. |

- **Range:** the largest real is just under ½ · 3^9840 ≈ 10^4694, and the smallest normal is about ⅙ · 3^−9841 ≈ 10^−4696. Subnormals reach 3^−(9841+n).
- **Examples (real27, base 27, E then M):**
  - 1 = `001 900000` (E = 1, m = 1/3: M is a single +1 trit at the top, and the first base-27 digit of M is the trits 1 0 0 = 9)
  - −1 = `001 9̅00000`
  - ½ ≈ `000 DDDDDD` (m = 0.111…1: ½ itself needs infinitely many 1s, so it is never exact)
  - 3 = `002 900000`, ⅓ = `000 900000`: powers of 3 are exact, and 0.1 is not

### 14.2 Registers

- **f−13 … f13**: 27 registers of 81 trits, each holding a real81. **f0 reads as zero, and writes to it are ignored**, as with r0.
- **FSR** (status and control, one word), read and written with `FMFS`/`FMTS` in any ring:

| Trits | Field | Meaning |
|---|---|---|
| 23–27 | flags `IV DZ OF UF IX` | sticky: +1 once the exception has happened; cleared only by writing FSR |
| 14–18 | masks `IV DZ OF UF IX` | +1 masked (the default value results, §14.4); 0 unmasked (trap) |
| others | – | reserved, read as 0 |

- **At reset** all masks are +1 and all flags are 0.
- **MODE trit 24 is `FE`** (FPU enabled), +1 at reset. When it is 0, every FPU instruction raises **vector −12 (FPU not available)** before doing anything. A kernel clears it on a context switch and saves or restores the registers on the first FPU instruction of the next task: **lazy switching**, as on the 386 with `CR0.TS`.

### 14.3 Rounding

- **Every result is rounded to nearest.** There is no rounding-mode register.
- **No ties in the common case:** in balanced ternary, dropping trits *is* rounding to nearest. Exact results of add, subtract, multiply, scale and conversions from integers have a power of 3 in the denominator, so they are never exactly halfway between two neighbours.
- **The exception: exact halves.** Division, square root, remainder and conversions from decimal can land exactly halfway (11 ÷ 2 = 5.5 lies between 5 and 6 when both are representable). **Ties go to the even significand** (M even), as in IEEE.
- A result that had to be rounded raises **IX** (inexact).

### 14.4 Exceptions

| Flag | When | Masked result |
|---|---|---|
| **IV** invalid | ∞ − ∞, 0 · ∞, 0 / 0, ∞ / ∞, √ of a negative, `FREM` by 0 or of ∞, a conversion to integer out of range or of NaN | the canonical NaN (integers: the largest word of the right sign, or 0 for NaN) |
| **DZ** divide by zero | finite non-zero / 0 | ∞ signed like the dividend |
| **OF** overflow | the rounded result's E would exceed 9840 | ±∞ (and IX) |
| **UF** underflow | the result is subnormal (or flushed to 0) and inexact | the subnormal or 0 (and IX) |
| **IX** inexact | the result was rounded | the rounded result |

- **NaN in, NaN out:** an operation with a NaN operand returns that NaN (the first one if both are) and raises nothing. Comparisons with NaN are *unordered*.
- **An unmasked exception traps** to **vector −11 (floating-point exception)** before anything is written. EPC holds the instruction, the flag is set, and FCAUSE[1:18] holds the exception (1 IV, 2 DZ, 3 OF, 4 UF, 5 IX; the field a system call's number uses).

### 14.5 Encoding

FPU majors have `A[1] = −`. Then `A[2]` sets the length: 0 means 1 word, + means 2 words, and − is reserved (`ILL`). The fields are the integer formats' (§3.1). The register fields name f registers or r registers as each instruction says.

| A | Mnemonic | Format | Operation |
|---|---|---|---|
| **1-word** | | | |
| −6561 | FALU | R | `fd := fa ⊕ fb` (functions below) |
| −6560 | FCVT | R | conversions, moves, constants and FSR (below) |
| −6559 | FMEM | Q | load or store at `ra + size·q3`: B = 1 `FLDS` (real27 → fd), 2 `FLDD` (real81 → fd), −1 `FSTS` (fd rounded → real27), −2 `FSTD` (fd → real81). The size is 3 or 9 trytes; addresses must be word-aligned. |
| **2-word** | | | |
| −4374 | FMEMD | I | as FMEM at `ra + imm27` (trytes) |
| −4373 | `FLI` | J | `fd := imm27` read as a real27 (any single-precision constant in one instruction) |
| other | – | – | reserved → `ILL` |

**FALU functions (B):**

| B | Mnemonic | Operation |
|---|---|---|
| 1 | `FADD` | fa + fb |
| 2 | `FSUB` | fa − fb |
| 3 | `FMUL` | fa · fb |
| 4 | `FDIV` | fa / fb |
| 5 | `FREM` | fa − fb · q, q = fa / fb rounded to the nearest integer (ties to even): the IEEE remainder, exact |
| 6 | `FMIN` | the smaller (a NaN operand loses to a number) |
| 7 | `FMAX` | the larger |
| 8 | `FSQRT` | √fa |
| 9 | `FNEG` | −fa (exact, never traps; a NaN's payload is negated) |
| 10 | `FABS` | \|fa\| |
| 11 | `FRND` | fa rounded to the nearest integer, ties to even |
| 12 | `FTRUNC` | fa rounded toward zero to an integer |
| 13 | `FCMP` | compare: MODE.S := −1, 0 or +1 for fa <, =, > fb, and C := 0. Unordered (a NaN): S := 0 and C := +1. fd is not written. |

**FCVT functions (B)**, with fd/rd, fa/ra and rb as each says:

| B | Mnemonic | Operation |
|---|---|---|
| 1 | `FCVTIF fd, ra` | integer → real (exact: 27 trits fit in 72) |
| 2 | `FCVTFI rd, fa` | real → integer, rounded to nearest, ties to even (IV out of range) |
| 3 | `FTOW rd, fa` | fa rounded to real27, as a raw word in rd |
| 4 | `FFRW fd, ra` | the real27 in ra, as a real (exact) |
| 5 | `FEXP rd, fa` | the exponent E of fa normalized (0 for zero; IV and E_max for ∞/NaN) |
| 6 | `FSCL fd, fa, rb` | fa · 3^rb (exact unless it overflows or underflows) |
| 7 | `FMFS rd` | rd := FSR |
| 8 | `FMTS ra` | FSR := ra (reserved trits ignored) |
| 9 | `FCLASS rd, fa` | 0 zero, ±1 subnormal, ±2 normal, ±3 infinite (signed like the value), 4 NaN |
| 10 | `FCONST fd, k` | a constant, k in the ra field: 0 zero, 1 one, 2 π, 3 e, 4 ln 3, 5 log₃ e, 6 log₃ 10 (each correctly rounded) |
| 11 | `FMOV fd, fa` | fd := fa (a NaN keeps its payload) |

**Assembler forms:** the mnemonic with operands in field order, e.g. `fadd f1, f2, f3`, `fldd f4, [r13]`, `fldd f4, [r2+12]` (the offset always in trytes; the assembler picks `FMEM` or `FMEMD`), `fli f1, 2.5`, `fconst f2, pi`. Directives: `.real27 x` emits a word, and `.real81 x` emits three, with x written as a decimal or base-27 number.

**Trine:** the type `real` is a real81 (§ LANGUAGE.md).

## 15. The vector unit

A SIMD unit on 81-trit registers: one register is **9 tryte lanes or 3 word lanes**, so the same instruction adds nine trytes (text, pixels, ternary weights) or three words.

### 15.1 Registers and enable

- `v−13 … v13`, 3 words each; `v0` reads as zero and ignores writes. Word lane k is word k; tryte lane j is tryte `j mod 3` of word `j div 3`, the most significant first. In memory that is simply the 9 trytes in address order.
- `MODE.VE` (trit 19): with `VE` = 0, every vector instruction raises vector **−13** (vector unit not available) and changes nothing. Reset sets `VE` = 1 and zeroes the registers. An OS switches the registers lazily, as for the FPU (Trinux: docs/KERNEL.md).

### 15.2 Encoding

All three majors are 1-word R-format instructions (§3.1).

| A | Mnemonic | Operation |
|---|---|---|
| 9 | VALU | lane-wise arithmetic and reductions (functions below) |
| 10 | VMEM | B = 1 `VLD vd, [ra + 9·q3]`, B = 2 `VST vd, [ra + 9·q3]`: 9 trytes at an address that must be a **multiple of 9** (vector −5 otherwise). `VST` checks all three words before writing any, so a store straddling into a read-only page changes nothing. |
| 11 | VMOV | lane moves (below) |

**VALU functions (B):**

| B | Mnemonic | Operation |
|---|---|---|
| 1–27 | `VADD vd, va, vb`, … | the ALU function B (§3.3) on 9 tryte lanes, wrapping within each lane |
| 28–54 | `VADDW vd, va, vb`, … | the ALU function B − 27 on 3 word lanes |
| 55 | `VMRG vd, vm, vs` | `vd := MRG(vd, vm, vs)` on the 81 trits: per trit, the mask's 0 keeps `vd`, +1 takes `vs`, −1 clears |
| 56 | `VSUM rd, va` | `rd` (a general register) := the sum of `va`'s 9 tryte lanes (exact) |
| 57 | `VDOTW rd, va, vb` | `rd` := Σ `va.lane[j]` × the lowest trit of `vb.lane[j]`: one weight trit per lane, so each product is a pass, a negation or a zero (the inner loop of a ternary-weight network) |
| other | – | `ILL` |

Only the lane-wise ALU functions are valid in either range: `ADD`, `SUB`, `MIN`, `MAX`, `TMUL`, `TSAT`, `CONS` (`vd, va, vb`) and `NEG`, `ABS`, `SGN` (`vd, va`). Any other function (`MUL`, `DIV`, shifts, carries, `CMP`, the binary operations) raises `ILL`. Vector instructions do not touch `MODE.S` or `C`.

**VMOV functions (B):**

| B | Mnemonic | Operation |
|---|---|---|
| 1 | `VGET rd, va, j` | `rd` := tryte lane j (0..8) of `va` |
| 2 | `VPUT vd, ra, j` | tryte lane j of `vd` := the low tryte of `ra` |
| 3 | `VGETW rd, va, k` | `rd` := word lane k (0..2) |
| 4 | `VPUTW vd, ra, k` | word lane k of `vd` := `ra` |
| 5 | `VBCST vd, ra` | every tryte lane := the low tryte of `ra` |
| 6 | `VBCSTW vd, ra` | every word lane := `ra` |
| other | – | `ILL`; so is a lane number out of range |

**Assembler forms:** `vadd v1, v2, v3`, `vnegw v4, v5`, `vld v1, [r2+18]` (the offset in trytes, a multiple of 9 within ±117), `vget r1, v2, 8`. The disassembler prints the same.

**Trine:** memory-to-memory builtins (LANGUAGE.md §8): `vadd(d, a, b)` and the rest, on `@align(9)` arrays.

## 16. Conformance

An implementation conforms when every **conformance vector** gives the stated state after one instruction.

- **The vectors** are `crates/core/tests/isa_vectors.txt` (about 3,900), made by `tools/gen_isavectors.py`: a model of this document written in Python, sharing no code with the emulator. They cover every ALU function in all three forms (R, Q, I) with edge and random operands, the trit fields, every load and store in both address forms, `STWM` and `CAS3`, every branch condition against every S and C, `BR3`, `JAL`, `JALR`, `LEA`, `LI`, `LIQ`, the whole SYS group in each ring (with `MFC`/`MTC` on every control register, `IRET` to each ring, single step), port I/O against `IOPL`, reserved encodings, the vector unit, and the MMU: each combination of `V`, `X` and `RING` for loads, stores and fetches from both rings, the `AD` trit, and refusals at the first level.
- **Every trap** is reached: vectors 0, −1 … −8, −10, −12, −13, with `EPC`, `EMODE`, the stack swap, `FCAUSE` (vector, ring, and for memory faults the access) and `FADDR`.
- **Format**: one vector per line, `name | before | code | after`. The machine is described in the file's header: RAM of 27 frames, `PC` = 0, `ITB` = 14580 with a table whose entry for vector v is 30000 + 3v, and port p reading 3p + 1. A state lists registers (`rN=`, `vN=`), the MODE fields by name, control registers (`crN=`) and physical trytes (`tA=`); the state after lists only what changed, with `pc=`, and for a trap `trap=`, `fring=`, and where defined `facc=`, `faddr=`, `finfo=`.
- **The runner** `crates/core/tests/isa_conformance.rs` runs each vector through the interpreter's single step and again through its block-decoded fast path, and compares every register, the MODE fields, the control registers and the memory around every tryte the vector names.
- Floating-point arithmetic has its own vectors (`REAL.md` §8), run as FPU instructions; here only the "FPU off" trap.

To regenerate: `python tools/gen_isavectors.py > crates/core/tests/isa_vectors.txt` (deterministic, under a second).
