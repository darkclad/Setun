# Setun-11̅ CPU Manual

This is the programmer's manual for the Setun-11̅ CPU: what the machine is, how its numbers, registers, instructions, memory, traps and devices work, and how to program it in assembly. It is organised by topic and meant to be read as a reference.

The normative specification is [`SETUN26_ISA.md`](SETUN26_ISA.md) (ISA v0.7). The emulator in `crates/core` implements it, and where the two differ this manual describes what the emulator does and says so. Other documents cover the rest of the system: [`NOTATION.md`](NOTATION.md) (number notation), [`ASSEMBLER.md`](ASSEMBLER.md), [`BOOT.md`](BOOT.md), [`LANGUAGE.md`](LANGUAGE.md) (the Trine language), [`KERNEL.md`](KERNEL.md) (Trinux), [`NETWORK.md`](NETWORK.md) and [`PERFORMANCE.md`](PERFORMANCE.md).

## Contents

1. [Overview](#1-overview)
2. [Balanced ternary for programmers](#2-balanced-ternary-for-programmers)
3. [Data types the hardware knows](#3-data-types-the-hardware-knows)
4. [Registers](#4-registers)
5. [Instruction formats and encoding](#5-instruction-formats-and-encoding)
6. [Instruction reference](#6-instruction-reference)
7. [Memory and the MMU](#7-memory-and-the-mmu)
8. [Privilege rings and IOPL](#8-privilege-rings-and-iopl)
9. [Traps and interrupts](#9-traps-and-interrupts)
10. [Devices and I/O ports](#10-devices-and-io-ports)
11. [Reset and boot](#11-reset-and-boot)
12. [The assembler](#12-the-assembler)
13. [Calling convention](#13-calling-convention)
14. [Programming examples](#14-programming-examples)
15. [The emulator](#15-the-emulator)
16. [Performance](#16-performance)
- [Appendix A. Opcode map](#appendix-a-opcode-map)
- [Appendix B. ALU functions](#appendix-b-alu-functions)
- [Appendix C. Branch conditions](#appendix-c-branch-conditions)
- [Appendix D. Vector table](#appendix-d-vector-table)
- [Appendix E. Port map](#appendix-e-port-map)

---

## 1. Overview

Setun-11̅ is a balanced-ternary computer. Its digits are trits with the values −1, 0 and +1, and every number is signed by construction. The name is the number 26 written in balanced base 27: `1` `1̅` = 27 − 1. The machine is sized like an Intel 386: 27 general registers, paging with privilege rings, port I/O, a vectored interrupt controller, an on-chip floating-point unit and a small SIMD unit. It keeps three ideas from the Setun lineage: balanced arithmetic, the tritwise logic operations and the 3-way branch. It is not compatible with the Setun or Setun-70.

**Everything is a power of 3.** Every width, size and count in the design is a power of 3, so there are no binary-shaped quantities such as 24-trit addresses or 6-trit fields.

| Quantity | Value | Notes |
|---|---|---|
| trit | 1 | −1, 0, +1 |
| tryte | 3² = 9 trits | smallest addressable unit; ±9 841 |
| word | 3 trytes = 3³ = 27 trits | register, pointer and instruction unit; ±3 812 798 742 493 |
| address space | 3^27 trytes | a virtual address is one word |
| address split | 9 \| 9 \| 9 trits | two page-table levels and an offset |
| page / frame | 3^9 = 19 683 trytes | also one disk sector |
| default RAM | 3^9 frames = 3^18 = 387 420 489 trytes | set with `setun --ram FRAMES` |
| general registers | 3³ = 27 | `r-13` … `r13`, `r0` reads 0 |
| FPU registers | 27 × 81 trits | `f-13` … `f13`, `f0` reads 0 |
| vector registers | 27 × 81 trits | `v-13` … `v13`, `v0` reads 0 |
| control registers | 27 numbers, 11 assigned | `cr-13` … `cr13` |
| privilege rings | 3 | −1 user, 0 drivers, +1 kernel |
| interrupt vectors | 3^5 = 243 | numbered −121 … 121 |
| hardware IRQ lines | 3^4 = 81 | IRQ 0 … 80 |
| I/O ports | 3^9 = 19 683 | blocks of 27 per device |
| colours | 3^9 = 19 683 | 27 levels per channel |
| instruction length | 1 or 2 words | 27 or 54 trits |

The machine resets straight into its native mode: ring +1, paging off, interrupts off, at `PC = −59 049` in the BIOS ROM (§11).

## 2. Balanced ternary for programmers

### 2.1 Trits, trytes and words

A balanced-ternary number with n trits holds every integer from −(3^n − 1)/2 to +(3^n − 1)/2. There is exactly one zero and no unused patterns.

| Trits | Range | Where it appears |
|---|---|---|
| 3 | ±13 | a register field; `q3` immediates; one base-27 digit |
| 5 | ±121 | a vector number |
| 6 | ±364 | `imm6` immediates (`LIQ`, `BRQ`) |
| 9 | ±9 841 | a tryte; a port number; `SYS n`; `BR3` offsets |
| 18 | ±193 710 244 | a frame number; a real27 significand |
| 27 | ±3 812 798 742 493 | a word |
| 54 | about ±2.9·10^25 | the full product of two words (`MULH`:`MUL`) |
| 81 | about ±2.2·10^38 | three words; one vector or FPU register |

**Trit positions** are numbered from 1, most significant first. In a word, trit 1 has weight 3^26 and trit 27 has weight 3^0; trit k has weight 3^(27−k). `x[a:b]` is the field from trit a to trit b, read as a balanced number. In a tryte, trit 1 has weight 3^8 and trit 9 weight 1.

The value of a word made of three trytes t0 (most significant), t1, t2 is `t0·3^18 + t1·3^9 + t2`.

### 2.2 Properties worth knowing

- **There are no unsigned numbers.** Every value is signed, so loads need no sign- or zero-extension variants and there is a single kind of comparison.
- **The sign of a number is the sign of its leading non-zero trit.**
- **Negation is exact and costs nothing:** flip every trit. `NEG` is also the tritwise NOT. No value overflows when negated, and `ABS` never overflows.
- **Dropping low trits rounds to nearest.** Shifting right by k trits (`SH` with a negative count, or reading a high field) gives the integer nearest to v / 3^k. A tie is impossible, because 3^k is odd. Example: 14 is `0t11̅1̅1̅`; dropping the last trit gives `0t11̅1̅` = 5, and 14 / 3 = 4.67.
- **Overflow wraps modulo 3^27** into the balanced range, and the carry trit `C` records it (§4.5).
- Integer division comes in two kinds: `DIV`/`MOD` truncate toward zero (as in C), and `DIVR`/`MODR` round to nearest, which is the natural division of balanced ternary (§6.2).

### 2.3 Notation

The project writes numbers four ways. The full rules are in [`NOTATION.md`](NOTATION.md) and ISA §1.1.

| Prefix | Form | Example | Use |
|---|---|---|---|
| none | decimal | `42`, `-757`, `1_000` | most values |
| `0t` | balanced ternary, digits `1`, `0`, `1̅` (ASCII alias `T` for `1̅`) | `0t11̅0` = `0t1T0` = 6 | trit patterns, masks, attributes |
| `0z` | balanced base 27, one digit per 3 trits | `0zDD5̅`, `0zB007AB13` | compact view of trytes and words |
| `0x` | hexadecimal, a plain value | `0x1F`, `-0x10` | constants from binary systems |

**A negative digit is the positive digit with a bar over it**, as on the Setun. Negating a number toggles the bar on every digit.

The base-27 digits are:

| digit | `D̅` `C̅` `B̅` `A̅` `9̅` … `1̅` | `0` | `1` … `9` `A` `B` `C` `D` |
|---|---|---|---|
| value | −13 −12 −11 −10 −9 … −1 | 0 | +1 … +9 +10 +11 +12 +13 |

- Letters may be written in either case. Case carries no sign; the bar does.
- A tryte is exactly 3 digits (`0zD̅D̅D̅` … `0zDDD`), and a word is 9 digits. A 3-trit register field is one digit, which makes encodings easy to read (§5.4).
- Examples: `0z010` = 27; `0z1D̅` = 27 − 13 = 14; `0zDDD` = 9 841; `0zB007AB13` = 115 067 810 199, the boot signature.
- In host source files the bar is U+0305 COMBINING OVERLINE after the digit. `_` and spaces may separate digits.
- `0x` shows no trits. There is no unsigned wrap: `0xFFFF` does not fit a tryte, so it is an error, not −1.
- Refused literals include a bar on `0` (there is no −0), on `T` or on a letter beyond `D`; two bars on one digit; the letters `E`–`Z` as digits; and a value that does not fit the target.

**Inside the machine** an overlined character is the negated code point: overlined `1` is −49 (§3.2).

### 2.4 Words in memory: centre addressing and alignment

Addresses count trytes. **A word is addressed by its centre tryte**:

- the word at address A occupies trytes A−1, A and A+1;
- the order is big-endian: the most significant tryte is at A−1;
- a word is **aligned** when A is a multiple of 3, and **word accesses must be aligned** (otherwise vector −5, misaligned access);
- tryte accesses (`LDT`, `STT`) may use any address.

Pages, frames and tables are centred the same way. Frame f covers physical addresses `19683·f − 9841 … 19683·f + 9841`; its centre is `19683·f`. A page therefore runs from offset −9 841 to +9 841 around its address, and an aligned word never crosses a page. The general rule: **aligned ⇔ the address is a multiple of the size**. A 9-tryte vector is aligned when its address, the centre of its first word, is a multiple of 9.

One consequence for strings and byte-like data: when a label names a word address, the first tryte stored there is at `label − 1`. The assembler's `.string` and `.tryte` directives pack trytes three to a word starting at that tryte.

## 3. Data types the hardware knows

### 3.1 Integers

| Type | Size | Range | Instructions |
|---|---|---|---|
| tryte | 1 tryte | ±9 841 | `LDT` loads the value unchanged; `STT` stores the low 9 trits of a register |
| word | 3 trytes | ±3 812 798 742 493 | every integer instruction |
| multi-word | n words | | `ADD`/`ADC` and `SUB`/`SBC` chain through `C`; `MUL` and `MULH` give the 54-trit product |

Trine's `int` and pointers are one word; `sizeof` counts trytes (LANGUAGE.md).

### 3.2 Characters

A tryte holds a Unicode code point directly.

- **A positive tryte is a code point U+0000 … U+2671** (9 841). That covers ASCII, Latin-1 and Latin Extended, Greek, Cyrillic, Hebrew, Arabic, Devanagari, general punctuation, arrows, box drawing, blocks and geometric shapes.
- **A tryte below −32 is an overlined character**: the negated code point. Overlined `1` is −49 and overlined `D` is −68. One overlined character is still one tryte and one screen cell.
- **−1 and −4 … −32 mean nothing.** −1 is also what C-style readers return for end of file.
- **A character above U+2671 takes three trytes:** a lead of −2 (or −3 for the overlined character), then two trailing trytes, both in −9 841 … −33. With n = code point − 9 842, they are −(33 + n div 9 809) and −(33 + n mod 9 809). Every Unicode scalar value fits.
  - A trailing tryte is never 0, a newline, an ASCII character or a lead, so NUL-terminated strings, `strlen` (which counts trytes), searching for an ASCII character and splitting lines all keep working.
  - Decoders refuse a lead −1 or −4 … −32, a lead without two trailing trytes, a code point above U+10FFFF and a surrogate. There is no overlong form.
  - The encoding is not self-synchronising: text is decoded forward from a known start.

The keyboard, the serial console, text mode, the assembler's `.string` and Trine's `char` all use this encoding.

### 3.3 Floating point: real27 and real81

A floating-point value is m · 3^E. **The significand carries the sign**, so there is no sign field and no −0.

| Format | Size | Exponent E | Significand M | Precision |
|---|---|---|---|---|
| real27 | 1 word | trits 1–9 (one tryte) | trits 10–27: 18 trits | about 28.5 bits |
| real81 | 3 words | trits 1–9 of the first word | the other 72 trits, most significant first | about 114 bits |

- M is an integer of n trits (n = 18 or 72) and m = M · 3^−n, so |m| < 1/2.
- Both formats share the exponent tryte (−9 841 … 9 841). The first word of a real81 *is* a real27 holding the top 18 trits of M, which is the real81 rounded to real27.
- The FPU registers hold real81 values. Details are in §6.11.

Examples (real27, written as the exponent tryte and then the 6 significand digits, base 27): 1 = `001 900000`; −1 = `001 9̅00000`; 3 = `002 900000`; ⅓ = `000 900000`. Powers of 3 are exact; ½ and 0.1 are not.

### 3.4 Vectors

A vector register holds 81 trits, viewed as **9 tryte lanes** or **3 word lanes**. Word lane k is word k; tryte lane j is tryte `j mod 3` of word `j div 3`, most significant first. In memory a vector is simply its 9 trytes in address order, and its address is the centre of its first word, a multiple of 9. See §6.12.

### 3.5 Binary data

The binary operations (§6.2.5) read a word's value as a two's-complement integer of unlimited width: 5 is …000101, −1 is all ones. The ISA fixes no binary width; `BEXT` and `BROT` take the width as an operand. Octets from binary systems are stored one per tryte as 0 … 255 (the network card does this).

### 3.6 Pixels and text cells

A graphics pixel is one tryte: trits 1–3 red, 4–6 green, 7–9 blue, each −13 … +13 (27 levels). In base 27 a pixel is its three colour digits: `0zDDD` white, `0zD̅D̅D̅` black, `0zDD̅D̅` red, `0z000` 50 % grey. A text cell is one word: character, attribute, reserved. See §10.10.

## 4. Registers

### 4.1 General registers

There are 27 general registers of 27 trits, `r-13` … `r13`. A register number is one 3-trit field, so it is one base-27 digit.

| Register | Hardware role | ABI role (Trine, Trinux) |
|---|---|---|
| `r0` (`zero`) | **always reads 0**; writes are discarded | zero |
| `r1` | – | first argument; return value |
| `r2`–`r6` | – | arguments 2–6 |
| `r7`–`r10` | – | caller-saved temporaries |
| `r11` (`fp`) | – | frame pointer, callee-saved |
| `r12` (`lr`) | – | return address, set by `call`/`jal` |
| `r13` (`sp`) | **stack pointer**: swapped with `KSP` on every ring change | stack pointer, word-aligned, grows toward negative addresses |
| `r-1`–`r-9` | – | callee-saved; Trine keeps register locals here |
| `r-10`–`r-13` | – | caller-saved temporaries, assembler scratch |

The assembler accepts the aliases `zero`, `fp`, `lr` and `sp`. `PC` is a separate register and always a multiple of 3; a jump to a misaligned address faults on the next fetch.

### 4.2 FPU registers

`f-13` … `f13` hold 81 trits each, one real81 value. `f0` reads as zero and ignores writes. The status and control register `FSR` is described in §6.11.2.

### 4.3 Vector registers

`v-13` … `v13` hold 81 trits each (9 tryte lanes or 3 word lanes). `v0` reads as zero and ignores writes.

### 4.4 Control registers

Control registers are `cr-13` … `cr13`. `MFC rd, cr` reads one and `MTC cr, ra` writes one; **both require ring +1**.

| CR | Name | Contents | Access |
|---|---|---|---|
| 0 | `MODE` | status and flags (§4.5) | read/write; also changed by traps and `IRET` |
| 1 | `PTB` | physical address of the level-1 page table (§7.3) | read/write |
| 2 | `FADDR` | faulting virtual address | read/write; set on every trap |
| 3 | `FCAUSE` | cause of the last trap (§9.3) | read/write; set on every trap |
| 4 | `ITB` | interrupt table base: the address of the table's centre, vector 0 (§9.1) | read/write |
| 5 | `EPC` | PC saved on trap entry | read/write |
| 6 | `EMODE` | MODE saved on trap entry | read/write |
| 7 | `KSP` | the other stack pointer, swapped with `r13` on a ring change | read/write |
| 8 | `SCRATCH` | free for the kernel | read/write |
| 9 | `CPUID` | read-only: trit 1 = +1 (the FPU is present); the version in trits 19–27 | read only |
| 10 | `CYCLE` | read-only count of instructions executed | read only |
| −13 … −1, 11 … 13 | – | reserved; read as 0 | read only |

- `MTC` to any register other than 0 … 8 raises vector −1 (illegal instruction). That covers `CPUID`, `CYCLE` and the reserved numbers.
- **`CPUID`** reads 3^26 + 7 = 2 541 865 828 336 in the emulator: trit 1 is +1 and the version field holds 7 (ISA v0.7). The BIOS prints it as "ISA v0.7".
- **`CYCLE`** counts executed instructions, including ones that end in a fault. Delivering an interrupt is not counted. It is the emulator's clock as well: device time advances 100 ns per instruction by default (§15).
- The ISA asks for `PTB` to be a multiple of 3^10 and `ITB` a multiple of 729. The emulator does not check either.

### 4.5 MODE

`MODE` (`cr0`) is one word of flag trits. The weight column gives the value each trit contributes when it is +1, which is how to build a MODE value as a constant.

| Trit | Weight | Field | Values | Reset |
|---|---|---|---|---|
| 27 | 1 | `RING` | current ring: −1 user, 0 drivers, +1 kernel | +1 |
| 26 | 3 | `IE` | +1 maskable interrupts enabled; 0 disabled; −1 reserved (acts as disabled) | 0 |
| 25 | 9 | `PG` | +1 paging on; 0 off | 0 |
| 24 | 27 | `FE` | +1 FPU enabled; 0: every FPU instruction raises vector −12 | +1 |
| 23 | 81 | `IOPL` | lowest ring allowed to use port I/O | 0 |
| 22 | 243 | `TF` | +1 single-step trap after each instruction | 0 |
| 21 | 729 | `S` | sign of the last result: −1, 0, +1 | 0 |
| 20 | 2 187 | `C` | carry trit of the last add, subtract or multiply | 0 |
| 19 | 6 561 | `VE` | +1 vector unit enabled; 0: every vector instruction raises vector −13 | +1 |
| 1–18 | – | – | reserved | 0 |

So MODE after reset is 1 + 27 + 6 561 = 6 589.

- **S** replaces the binary sign and zero flags. Branches test it (§6.7), and `BR3` dispatches on all three of its values.
- **C** is non-zero exactly when an `ADD`, `SUB`, `ADC`, `SBC` or `MUL` overflowed. It feeds `ADC` and `SBC`. After `FCMP`, C = +1 means *unordered*.
- S and C are saved with the rest of MODE in `EMODE` on every trap and restored by `IRET`.
- The ISA says the reserved trits read as 0. The emulator stores whatever `MTC MODE` writes there and returns it on `MFC`; software should write zeros.

## 5. Instruction formats and encoding

### 5.1 Words and fields

An instruction is 1 or 2 words, word-aligned. The first word always has the same layout:

```
word 1, trit:  1 ........ 9   10 ....... 18   19-21   22-24   25-27
               [     A     ] [      B     ] [  X  ] [  Y  ] [  Z  ]
               major opcode   function       three 3-trit operand fields
word 2:        [                     imm27                          ]
```

- `A` (one tryte) is the major opcode and `B` (one tryte) the function within it.
- `X`, `Y` and `Z` are three 3-trit fields, each a register number −13 … 13 or a small immediate.
- So a 1-word instruction is a full three-operand operation, and a 2-word instruction adds a 27-trit immediate that reaches any constant or address.
- **An all-zero word is `ILL`** (major 0, function 0), so running into zeroed memory traps at once.

### 5.2 Instruction length

The first trit of `A` sets the length:

| `A[1]` | A (decimal) | Meaning |
|---|---|---|
| 0 | −3 280 … 3 280 | 1-word instruction |
| + | 3 281 … 9 841 | 2-word instruction; the majors are numbered 6 561 + n |
| − | −9 841 … −3 281 | FPU instruction; then `A[2]` sets the length: 0 one word, + two words, − reserved (`ILL`) |

### 5.3 Formats

| Format | Word 1 operands | Word 2 | Used by |
|---|---|---|---|
| R | `X` = rd, `Y` = ra, `Z` = rb | – | `ALU`, `MEMR`, `VALU`, `VMEM`, `VMOV`, `FALU`, `FCVT`, SYS group |
| Q | `X` = rd, `Y` = ra, `Z` = q3 (−13 … 13) | – | `ALUQ`, `MEM`, `IO`, `JALR`, `FMEM` |
| S | `X` = rd or cond, `Y`:`Z` = imm6 (trits 22–27, −364 … 364) | – | `LIQ`, `BRQ` |
| S9 | `X`:`Y`:`Z` = imm9 (trits 19–27, ±9 841) | – | `SYS n` |
| I | `X` = rd, `Y` = ra | imm27 | `ALUI`, `MEMD`, `FMEMD` |
| J | `X` = rd or cond, `Y` unused | imm27 | `LI`, `LEA`, `JAL`, `BR`, `IOI`, `FLI` |
| B3 | unused | `[offN : 9][offP : 9][0 : 9]` | `BR3` |
| F | `X` = rd, `Y` = ra | `[pos : 9][len : 9][0 : 9]` | `FLD` (`EXT`, `INS`, `BEXT`, `BROT`) |

Immediate rules:

- **Branch and jump offsets count words** and are relative to the address of the branch instruction itself: the target is `PC + 3·off`.
- **`MEM` and `FMEM` scale `q3` by the access size** (1 for a tryte, 3 for a word, 9 for a real81). `MEMD`, `FMEMD` and `LEA` offsets count trytes.
- `JALR` scales `q3` by 3: the target is `ra + 3·q3`.
- `VMEM` scales `q3` by 9.
- The CPU ignores the third tryte of the `B3` and `F` second words, which should be 0.

### 5.4 Reading an encoding

Because each operand field is 3 trits, the base-27 form of an instruction word shows the fields directly: three digits of A, three of B, then one digit each for X, Y and Z. Some words from the assembler's listing:

| Source | Encoding (base 27) | Reading |
|---|---|---|
| `add r1, r2, r3` | `0z001001123` | A = 1 (`ALU`), B = 1 (`ADD`), X Y Z = 1 2 3 |
| `add r1, r2, 5` | `0z002001125` | A = 2 (`ALUQ`), q3 = 5 |
| `add r1, r2, 500` | `0z900001120 0z00000018̅D̅` | A = 6 561 (`ALUI`), imm27 = 500 |
| `ldw r1, [sp+6]` | `0z0030021D2` | A = 3 (`MEM`), B = 2 (`LDW`), X = 1, Y = 13, q3 = 2 words |
| `li r1, 42` | `0z00600012C̅` | A = 6 (`LIQ`), imm6 = 42 = `0z1C̅` |
| `mrg r1, r2, r3` | `0z001011123` | B = 28 = `0z011` |
| `fadd f1, f2, f3` | `0z9̅00001123` | A = −6 561 (`FALU`) |
| `sys 64` | `0z00000402A` | B = 4, imm9 = 64 = `0z02A` |

## 6. Instruction reference

### 6.1 Conventions

- Syntax is the assembler's (lower case; mnemonics are case-insensitive). `rd`, `ra`, `rb` are general registers, `fd`, `fa`, `fb` FPU registers, `vd`, `va`, `vb` vector registers. `b` is `rb`, a `q3` constant or an imm27 constant, depending on the form.
- **S and C columns:** "S" means S := sign of the result; "C" means C is set as described; "–" means unchanged.
- **Faults.** Any instruction can fault while being fetched: vector −5 if `PC` is not a multiple of 3, −3 or −4 if the page is not present or not executable, −10 if nothing is at the physical address. A 2-word instruction's second word is fetched separately and can fault on its own (with `FADDR` = its address). Any reserved major or function raises −1 (`ILL`). The tables list only the faults beyond these.
- **"Mem"** in a fault column means the memory faults: −5 misaligned word access, −3 page not present, −4 protection, −10 bus error.
- On every fault the instruction has no effect: registers, memory and flags are as before, and `EPC` points at it (except `SYS` and `BRK`, §9.2).

### 6.2 Integer ALU: ALU, ALUQ, ALUI

Three majors share one function table (B):

| Major | Form | Syntax | Operation |
|---|---|---|---|
| 1 `ALU` | R | `op rd, ra, rb` | `rd := ra ⊕ rb` |
| 2 `ALUQ` | Q | `op rd, ra, q` | `rd := ra ⊕ q`, q in −13 … 13 |
| 6 561 `ALUI` | I | `op rd, ra, imm` | `rd := ra ⊕ imm27` |

The assembler picks `ALUQ` when the constant is known and fits ±13, `ALUI` otherwise; `op.l` forces `ALUI`. Unary functions use the R form with `rb` ignored and are written `op rd, ra`.

**Every ALU function sets S from its result**, except `CMP`, which sets S from the comparison, and writes no register. `ADD`, `SUB`, `ADC`, `SBC` and `MUL` also set C; every other function leaves C unchanged. `B = 0`, negative B and B above 31 raise `ILL`.

#### 6.2.1 Arithmetic

| B | Syntax | Operation | S | C | Faults |
|---|---|---|---|---|---|
| 1 | `add rd, ra, b` | `rd := ra + b`, wrapped modulo 3^27 | S | carry trit | – |
| 2 | `sub rd, ra, b` | `rd := ra − b` | S | carry trit | – |
| 3 | `adc rd, ra, b` | `rd := ra + b + C` | S | carry trit | – |
| 4 | `sbc rd, ra, b` | `rd := ra − b + C` | S | carry trit | – |
| 5 | `mul rd, ra, b` | low 27 trits of the product | S | sign of the high 27 trits | – |
| 6 | `mulh rd, ra, b` | high 27 trits of the 54-trit product | S | – | – |
| 7 | `div rd, ra, b` | quotient truncated toward zero | S | – | −6 if b = 0 |
| 8 | `mod rd, ra, b` | remainder of `div` (sign of the dividend) | S | – | −6 if b = 0 |
| 9 | `divr rd, ra, b` | quotient rounded to nearest | S | – | −6 if b = 0 |
| 10 | `modr rd, ra, b` | balanced remainder, \|r\| ≤ \|b\|/2 | S | – | −6 if b = 0 |
| 11 | `min rd, ra, b` | tritwise minimum (§6.2.2) | S | – | – |
| 12 | `max rd, ra, b` | tritwise maximum | S | – | – |
| 19 | `neg rd, ra` (also `not`) | `rd := −ra`, the tritwise NOT | S | – | – |
| 20 | `sgn rd, ra` | `rd := sign(ra)` | S | – | – |
| 21 | `abs rd, ra` | `rd :=` \|`ra`\| | S | – | – |

- **The carry trit.** The exact sum of two words lies within ±(3^27 − 1). If it exceeds the word range, the result wraps by 3^27 and C is +1 (positive overflow) or −1 (negative overflow); otherwise C is 0. A three-word add is `add` on the low words, then `adc` on the middle and `adc` on the high words.
- **Multi-word subtraction** is `sub` on the low words, then `sbc` on the middle and high ones. `SUB` leaves C so that the exact difference is `result + C·3^27`, and `SBC` adds it: `rd := ra − b + C`. (The specification had `ra − b − C` until 2026-10-08, which broke the chain.)
- **The product.** For words a and b the exact product is `hi·3^27 + lo`, with `lo` = `mul a, b` and `hi` = `mulh a, b`, both balanced words. `mul` sets C to the sign of `hi`, so C ≠ 0 exactly when the product did not fit.
- **Two divisions.** `div`/`mod` follow C: −7 `div` 2 = −3, −7 `mod` 2 = −1. `divr`/`modr` round to nearest: 8 `divr` 3 = 3, 8 `modr` 3 = −1. When the exact quotient is a half (b even), the emulator chooses the remainder +|b|/2: 7 `divr` 2 = 3, −7 `divr` 2 = −4 (the ISA does not specify ties). No quotient overflows.
- `mov rd, ra` is `add rd, ra, r0`.

#### 6.2.2 Tritwise logic

These act on each of the 27 trit positions independently.

| B | Syntax | Operation |
|---|---|---|
| 11 | `min rd, ra, b` | minimum of the two trits (ternary "AND") |
| 12 | `max rd, ra, b` | maximum (ternary "OR") |
| 13 | `tmul rd, ra, b` | product of the two trits (Setun-70 `SMT`) |
| 14 | `tsat rd, ra, b` | sign of the sum of the two trits, "threshold add" (Setun-70 `SAT`) |
| 15 | `cons rd, ra, b` | consensus: the trit if both agree, else 0 |
| 19 | `neg rd, ra` | flips every trit |

All set S and leave C. Truth table (a, b → result):

| a | b | `min` | `max` | `tmul` | `tsat` | `cons` |
|---|---|---|---|---|---|---|
| − | − | − | − | + | − | − |
| − | 0 | − | 0 | 0 | − | 0 |
| − | + | − | + | − | 0 | 0 |
| 0 | − | − | 0 | 0 | − | 0 |
| 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 0 | + | 0 | + | 0 | + | 0 |
| + | − | − | + | − | 0 | 0 |
| + | 0 | 0 | + | 0 | + | 0 |
| + | + | + | + | + | + | + |

With the Kleene reading +1 true, 0 unknown, −1 false, `min` and `max` are Kleene AND and OR, and `neg` is NOT.

#### 6.2.3 Shifts, comparison and trit queries

| B | Syntax | Operation | S | C |
|---|---|---|---|---|
| 16 | `sh rd, ra, b` | `rd := ra · 3^b`: left for b > 0 (high trits fall off), right for b < 0 (low trits fall off, rounding to nearest) | S | – |
| 17 | `cmp ra, b` | only `S := sign(ra − b)`, computed exactly; no register is written | S | – |
| 18 | `trit rd, ra, b` | `rd :=` trit number b (1 … 27) of `ra`: −1, 0 or +1 | S | – |
| 22 | `tlz rd, ra` | number of leading zero trits of `ra`, 0 … 27 | S | – |

- `cmp` is never fooled by overflow, unlike a `sub` whose result wrapped. Its X field is ignored; the assembler writes 0 there.
- `sh` with |b| ≥ 27 gives 0. The emulator clamps the count to ±40.
- `trit` with b outside 1 … 27 returns 0 in the emulator (the ISA does not say).

#### 6.2.4 Ternary idioms (v0.7)

| B | Syntax | Operation | S | Faults |
|---|---|---|---|---|
| 28 | `mrg rd, ra, b` | trit-mask merge: for each trit i, `ra[i]` = 0 keeps `rd[i]`, +1 takes `b[i]`, −1 gives 0. **Reads `rd`.** | S | – |
| 29 | `tcp rd, ra, b` | number of +1 trits among the low b trits of `ra` (b = 0: all 27) | S | −1 if b is outside 0 … 27 |
| 30 | `tcn rd, ra, b` | number of −1 trits among the low b trits of `ra` (b = 0: all 27) | S | −1 if b is outside 0 … 27 |
| 31 | `trev rd, ra, b` | the low b trits of `ra` in reverse order, the trits above them 0 (b = 0: all 27) | S | −1 if b is outside 0 … 27 |

- `mrg` replaces six ALU operations and a temporary. The merged trits are disjoint, so there are no carries. With an all-zero mask nothing changes and S comes from the old value of `rd`.
- `tcp + tcn` counts the non-zero trits and `tcp − tcn` is the trit balance; together they replace a binary popcount. `trev` is the trit-reversal permutation of a radix-3 FFT.

#### 6.2.5 Binary operations

These read each operand's value as a two's-complement integer of unlimited width (§3.5).

| B | Syntax | Operation | S |
|---|---|---|---|
| 23 | `band rd, ra, b` | binary AND | S |
| 24 | `bor rd, ra, b` | binary OR | S |
| 25 | `bxor rd, ra, b` | binary exclusive OR | S |
| 26 | `bandn rd, ra, b` | `ra` AND NOT `b` | S |
| 27 | `bsh rd, ra, b` | `ra · 2^b` for b > 0 (wrapping modulo 3^27); `⌊ra / 2^−b⌋` for b < 0, rounding toward −∞ like an arithmetic shift; b limited to ±81 | S |

- The exact results of the four logical operations lie within ±2^42; one outside the word range wraps modulo 3^27, as `add` does. Values below 2^41 never wrap, so 32-bit data is always exact.
- None of them changes C.
- Recipes: a 32-bit sum is `add` followed by `bext rd, rd, 0, 32`. A 32-bit right rotation by n is `brot` left by 32 − n.

### 6.3 Trit and bit fields: FLD

`FLD` (A = 6 569) is a 2-word instruction whose second word holds `[pos : 9][len : 9][0 : 9]`. All four functions set S from the result and leave C.

| B | Syntax | Operation | Faults |
|---|---|---|---|
| 1 | `ext rd, ra, pos, len` | `rd := ra[pos : pos+len−1]`, read as a balanced number | −1 unless 1 ≤ pos, 1 ≤ len, pos + len − 1 ≤ 27 |
| 2 | `ins rd, ra, pos, len` | `rd[pos : pos+len−1] :=` the low `len` trits of `ra`; the rest of `rd` is kept (**reads `rd`**) | as `ext` |
| 3 | `bext rd, ra, pos, len` | `rd :=` bits pos … pos+len−1 of `ra` (bit 0 the lowest), as a number 0 … 2^len − 1 | −1 unless 0 ≤ pos ≤ 80 and 1 ≤ len ≤ 41 |
| 4 | `brot rd, ra, pos, len` | `rd :=` the low `len` bits of `ra`, rotated left by `pos` within them | −1 unless 0 ≤ pos < len ≤ 41 |

Trit fields use the 1-based, most-significant-first numbering; bit fields count from bit 0 at the low end. The assembler checks both ranges. Example: `ext r1, r1, 23, 5` extracts the vector number from `FCAUSE`.

### 6.4 Constants and addresses: LIQ, LI, LEA

| Major | Syntax | Operation | Flags |
|---|---|---|---|
| 6 `LIQ` | `li rd, n` (\|n\| ≤ 364) | `rd := imm6` | – |
| 6 563 `LI` | `li rd, n`, `li.l rd, n`, `la rd, label` | `rd := imm27`: any constant or address | – |
| 6 564 `LEA` | `lea rd, target` | `rd := PC + imm27`; the offset counts trytes from this instruction; the assembler computes it from `target` | – |

- `li` picks `LIQ` when the value is a constant known before layout and fits ±364; otherwise `LI`. `mov rd, n` with a constant is the same as `li`.
- `la rd, label` is always `LI` (2 words), so code size does not depend on where the label lands.
- `lea` gives position-independent addresses.

### 6.5 Memory: MEM and MEMD

| Major | Form | Address |
|---|---|---|
| 3 `MEM` | Q | `ra + size·q3`, where size is 1 for `LDT`/`STT` and 3 for `LDW`/`STW`/`SWAP` |
| 6 562 `MEMD` | I | `ra + imm27` (trytes) |

| B | Syntax | Operation | Flags | Faults |
|---|---|---|---|---|
| 1 | `ldt rd, [ra ± off]` | load a tryte; the value is kept as is | – | −3, −4, −10 |
| 2 | `ldw rd, [ra ± off]` | load a word; the address must be a multiple of 3 | – | Mem |
| 3 | `stt rd, [ra ± off]` | store the low 9 trits of `rd` | – | −3, −4, −10 |
| 4 | `stw rd, [ra ± off]` | store `rd`; the address must be a multiple of 3 | – | Mem |
| 5 | `swap rd, [ra ± off]` | atomically exchange `rd` and the word at the address | – | Mem |

- **In assembly the offset is always in trytes.** `ldw r1, [sp+6]` reads the word at `sp + 6`; the assembler encodes it as `MEM` with q3 = 2. It chooses the 1-word form when the offset is a known multiple of the access size and the quotient fits ±13, otherwise `MEMD`. `[label]` means `[r0 + label]`.
- A word store that fails changes nothing; there are no partial writes.
- `swap` needs a writable page; the old value is read first, then the new one stored.
- Loads and stores do not touch S or C.

### 6.6 Read-modify-write: MEMR

`MEMR` (A = 8) is R format. There is no offset: the address is the value of `ra`, computed beforehand. It must be a multiple of 3 (vector −5) and the page must be writable (vector −4). A fault leaves memory and `rd` unchanged.

| B | Syntax | Operation | S | Faults |
|---|---|---|---|---|
| 1 | `stwm rd, [ra], rb` | the word at `ra` := `MRG`(that word, mask `rb`, source `rd`): for each trit, `rb` = 0 keeps the memory trit, +1 takes `rd`'s, −1 clears it. One atomic read-modify-write. | – | Mem |
| 2 | `cas3 rd, [ra], rb` | `old` := the word at `ra`; **S := sign(old − rd)**, exact; if S = 0 the word := `rb`; then `rd := old`. Atomic. | S | Mem |

- `cas3` is a write access even when the comparison fails (like x86 `CMPXCHG`), so it needs a writable page either way.
- A `br3` right after `cas3` sees three outcomes: the word was smaller than expected, equal (and swapped), or larger.
- A mutex fits in one trit: 0 free, +1 held, −1 held with waiters (§14.4).
- Uses of `stwm`: changing a page-table entry's flags (`V`, `AD`, `RING`) in one store, device control words, flag words shared between kernel and drivers.

### 6.7 Branches: BRQ, BR, BR3

| Major | Form | Syntax | Operation | Reach |
|---|---|---|---|---|
| 7 `BRQ` | S | `bxx target` | if cond holds: `PC += 3·imm6` | ±364 words |
| 6 566 `BR` | J | `bxx target` (`bxx.l` forces it) | if cond holds: `PC += 3·imm27` | the whole address space |
| 6 567 `BR3` | B3 | `br3 neg, pos` | S < 0: `PC += 3·offN`; S > 0: `PC += 3·offP`; S = 0: fall through to the next instruction (PC + 6) | ±9 841 words |

Offsets are relative to the branch instruction itself. Branches do not change any flags.

**The condition** is a 3-trit field in X. Each position stands for one value of a flag: `cond[1]` ↔ −1, `cond[2]` ↔ 0, `cond[3]` ↔ +1.

- A **+** trit means "branch if **S** has this value".
- A **−** trit means "branch if **C** has this value".
- `000` never branches. Mixing + and − trits in one condition raises `ILL`.

| Mnemonic | cond | Value | Branch if |
|---|---|---|---|
| `blt` | `+00` | 9 | S < 0 |
| `beq` | `0+0` | 3 | S = 0 |
| `bgt` | `00+` | 1 | S > 0 |
| `ble` | `++0` | 12 | S ≤ 0 |
| `bge` | `0++` | 4 | S ≥ 0 |
| `bne` | `+0+` | 10 | S ≠ 0 |
| `b` | `+++` | 13 | always |
| `bov` | `−0−` | −10 | C ≠ 0 (overflow; after `fcmp`: unordered) |
| `bnov` | `0−0` | −3 | C = 0 |
| `bnever` | `000` | 0 | never |

The assembler uses `BRQ` when the target is within ±364 words and relaxes it to `BR` automatically when it is not. **`br3` is the Setun's three-way jump**: after `cmp a, b` one instruction dispatches on less, equal and greater.

### 6.8 Jumps and calls: JAL, JALR

| Major | Form | Syntax | Operation |
|---|---|---|---|
| 6 565 `JAL` | J | `jal rd, target`, `call target` | `rd := PC + 6`; `PC := PC + 3·imm27` |
| 5 `JALR` | Q | `jalr rd, ra[, q]`, `ret` | `rd := PC + 3`; `PC := ra + 3·q` |

- `call f` is `jal r12, f`. `ret` is `jalr r0, r12, 0`.
- `jalr` computes the target before writing `rd`, so `jalr r12, r12` works.
- Jumps do not change flags. A target that is not a multiple of 3 faults (−5) when the next instruction is fetched, with `EPC` and `FADDR` = the bad address.

### 6.9 Port I/O: IO, IOI

Port I/O requires `RING ≥ MODE.IOPL`, otherwise vector −2. **A port transfers a whole word.**

| Major | Form | Syntax | Port |
|---|---|---|---|
| 4 `IO` | Q | `in rd, [ra ± q]`, `out rd, [ra ± q]` | `ra + q`, q within ±13 |
| 6 568 `IOI` | J | `in rd, port`, `out rd, port` | imm27 |

| B | Operation |
|---|---|
| 1 `IN` | `rd := port` |
| 2 `OUT` | `port := rd` |

The 1-word form suits the port layout: keep a device's base port in a register and use q to pick one of its 27 registers (§10.1). Reading a port with no device returns 0, and writing one does nothing; neither faults.

### 6.10 System group: SYS

Major 0, 1-word. Functions marked "+1" raise vector −2 outside ring +1.

| B | Syntax | Ring | Operation |
|---|---|---|---|
| 0 | `ill` | any | illegal-instruction trap (vector −1) |
| 1 | `nop` | any | no operation |
| 2 | `hlt` | +1 | wait for an interrupt. `PC` already points at the next instruction, which is where the handler returns to. |
| 3 | `brk` | any | breakpoint trap (vector −7), `EPC` = the next instruction |
| 4 | `sys n` | any | system call: vector 0, n (the 9 operand trits, ±9 841) in `FCAUSE[1:18]`, `EPC` = the next instruction |
| 5 | `iret` | +1 | return from a trap (§9.4) |
| 6 | `mfc rd, cr` | +1 | `rd := cr`; the CR number is in the Y field |
| 7 | `mtc cr, ra` | +1 | `cr := ra`; the CR number is in the X field. −1 for CRs other than 0 … 8. |
| 8 | `tlbinv ra` | +1 | invalidate the translation of virtual address `ra` |
| 9 | `tlbflush` | +1 | invalidate all translations except global ones |
| 10 | `fence` | any | order memory and I/O; a no-op in the emulator |
| other | – | – | `ILL` |

- `hlt` with `IE` = 0 stops the CPU until a reset, since no interrupt can be delivered.
- In the assembler the CR is named: `mode ptb faddr fcause itb epc emode ksp scratch cpuid cycle`, or `cr-13` … `cr13`.
- In the emulator `tlbinv` flushes the whole TLB, and the TLB is kept coherent with page-table writes anyway (§7.5). Portable software still issues `tlbinv`/`tlbflush` where the ISA requires them.

### 6.11 The floating-point unit

The FPU is part of the CPU (`CPUID` trit 1 is always +1). It owns the third of the opcode space whose first trit is −.

#### 6.11.1 Values

The formats are in §3.3. With E_max = 9 841 and E_min = −9 841:

| Kind | Encoding | Meaning |
|---|---|---|
| zero | M = 0 and E ≠ E_max | 0; results always write E = 0 |
| normal | top trit of M non-zero, E_min ≤ E < E_max | m · 3^E, 1/6 < \|m\| < 1/2 |
| subnormal | E = E_min, top trit of M zero, M ≠ 0 | gradual underflow |
| ±∞ | E = E_max, M = ±3^(n−1) (only the top trit set) | infinity, signed like M |
| NaN | E = E_max, any other M | not a number; the canonical NaN has M = 0. All NaNs are quiet. |

The largest finite value is just under ½ · 3^9840 ≈ 10^4694; the smallest normal is about ⅙ · 3^−9841 ≈ 10^−4696.

#### 6.11.2 Registers and FSR

- `f-13` … `f13` hold real81 values; `f0` reads as zero and ignores writes.
- `FSR` is one word, read with `fmfs` and written with `fmts` in any ring:

| Trits | Weights | Field | Meaning |
|---|---|---|---|
| 23–27 | 81, 27, 9, 3, 1 | flags `IV DZ OF UF IX` | sticky: +1 once the exception has happened; cleared only by writing FSR |
| 14–18 | 3^13 … 3^9 | masks `IV DZ OF UF IX` | +1 masked (the default result is delivered); 0 unmasked (trap) |
| others | – | – | reserved; `fmts` ignores them and they read as 0 |

At reset all masks are +1 and all flags 0: FSR = 2 381 643.

`MODE.FE` (+1 at reset) enables the unit. With `FE` = 0, every FPU instruction raises vector −12 before doing anything, which lets a kernel switch FPU state lazily. Reserved FPU majors raise −1 whether or not the unit is enabled.

#### 6.11.3 Rounding and exceptions

- **Every result is rounded to nearest**; there is no rounding-mode register. Exact sums, products, scalings and integer conversions are never halfway between two neighbours in balanced ternary. Division, square root, remainder and decimal conversion can land exactly halfway; those **ties go to the even significand**. A rounded result raises IX.

| Flag | Raised when | Masked result |
|---|---|---|
| IV invalid | ∞ − ∞, 0 · ∞, 0 / 0, ∞ / ∞, √ of a negative, `frem` by 0 or of ∞, a conversion to integer out of range or of NaN | the canonical NaN; for integers the largest word of the right sign, or 0 for NaN |
| DZ divide by zero | finite non-zero / 0 | ∞ signed like the dividend |
| OF overflow | the rounded exponent would exceed 9 840 | ±∞ (and IX) |
| UF underflow | the result is subnormal or flushed to 0, and inexact | the subnormal or 0 (and IX) |
| IX inexact | the result was rounded | the rounded result |

- **NaN in, NaN out:** an operation with a NaN operand returns that NaN (the first if both are) and raises nothing.
- **An unmasked exception traps** to vector −11 before any result is written. The flag is set in FSR, `EPC` holds the instruction, and the exception number (1 IV, 2 DZ, 3 OF, 4 UF, 5 IX) is in `FCAUSE[1:18]`. If several are raised at once, the first unmasked one in the order IV, DZ, OF, UF, IX is reported.

#### 6.11.4 Encoding

| A | Base 27 | Mnemonic | Format | Operation |
|---|---|---|---|---|
| −6 561 | `9̅00` | `FALU` | R | `fd := fa ⊕ fb` |
| −6 560 | `9̅01` | `FCVT` | R | conversions, moves, constants, FSR |
| −6 559 | `9̅02` | `FMEM` | Q | load or store at `ra + size·q3` |
| −4 374 | `6̅00` | `FMEMD` | I | load or store at `ra + imm27` (trytes) |
| −4 373 | `6̅01` | `FLI` | J | `fd := imm27` read as a real27 |

#### 6.11.5 FALU functions

None of these changes S or C except `fcmp`. Faults: −12 if `FE` = 0, −11 on an unmasked exception.

| B | Syntax | Operation | Exceptions |
|---|---|---|---|
| 1 | `fadd fd, fa, fb` | fa + fb | IV OF UF IX |
| 2 | `fsub fd, fa, fb` | fa − fb | IV OF UF IX |
| 3 | `fmul fd, fa, fb` | fa · fb | IV OF UF IX |
| 4 | `fdiv fd, fa, fb` | fa / fb | IV DZ OF UF IX |
| 5 | `frem fd, fa, fb` | fa − fb · q with q = fa / fb rounded to the nearest integer, ties to even: the IEEE remainder, exact | IV |
| 6 | `fmin fd, fa, fb` | the smaller; a NaN operand loses to a number | – |
| 7 | `fmax fd, fa, fb` | the larger | – |
| 8 | `fsqrt fd, fa` | √fa | IV IX |
| 9 | `fneg fd, fa` | −fa; exact, never traps; a NaN's payload is negated | – |
| 10 | `fabs fd, fa` | \|fa\| | – |
| 11 | `frnd fd, fa` | fa rounded to the nearest integer, ties to even | IX |
| 12 | `ftrunc fd, fa` | fa rounded toward zero to an integer | IX |
| 13 | `fcmp fa, fb` | S := −1, 0, +1 for fa <, =, > fb and C := 0; unordered (a NaN): S := 0, C := +1. No register is written. | – |

After `fcmp` the ordinary branches test the result, and `bov` means unordered.

#### 6.11.6 FCVT functions

| B | Syntax | Operation | Exceptions |
|---|---|---|---|
| 1 | `fcvtif fd, ra` | integer → real; exact | – |
| 2 | `fcvtfi rd, fa` | real → integer, rounded to nearest, ties to even | IV IX |
| 3 | `ftow rd, fa` | fa rounded to real27, as a raw word in `rd` | IX; UF or OF at the ends of the range |
| 4 | `ffrw fd, ra` | the real27 word in `ra`, as a real; exact | – |
| 5 | `fexp rd, fa` | the exponent E of fa written normalized; 0 for zero; E_max and IV for ∞ or NaN | IV |
| 6 | `fscl fd, fa, rb` | fa · 3^rb; exact unless it overflows or underflows | OF UF IX |
| 7 | `fmfs rd` | `rd := FSR` | – |
| 8 | `fmts ra` | `FSR := ra`; only the flag and mask trits are taken | – |
| 9 | `fclass rd, fa` | 0 zero, ±1 subnormal, ±2 normal, ±3 infinite (signed like the value), 4 NaN | – |
| 10 | `fconst fd, k` | a constant; k (in the Y field) is 0 zero, 1 one, 2 π, 3 e, 4 ln 3, 5 log₃ e, 6 log₃ 10, each correctly rounded. Other k raise −1. | – |
| 11 | `fmov fd, fa` | `fd := fa`; a NaN keeps its payload | – |

In assembly `fconst` takes a name (`zero`, `one`, `pi`, `e`, `ln3`, `log3e`, `log3_10`) or a number 0 … 6. The ISA says `fclass` signs a NaN like the value; the emulator always returns +4.

#### 6.11.7 FMEM, FMEMD and FLI

| B | Syntax | Operation | Size |
|---|---|---|---|
| 1 | `flds fd, [ra ± off]` | load a real27 into `fd` | 3 trytes |
| 2 | `fldd fd, [ra ± off]` | load a real81 | 9 trytes |
| −1 | `fsts fd, [ra ± off]` | store `fd` rounded to real27 (raises IX, and UF or OF at the ends of the range) | 3 trytes |
| −2 | `fstd fd, [ra ± off]` | store `fd` as a real81 | 9 trytes |

- Addresses must be multiples of 3 (a real81 needs only word alignment). Faults: Mem, −12, −11.
- The 1-word `FMEM` scales q3 by the size (3 or 9 trytes); the assembler uses it when the tryte offset is a multiple of the size within ±13 sizes, and `FMEMD` otherwise.
- A real81 store that spans two pages checks both before writing, so it is all or nothing.
- `fli fd, x` (2 words) loads any real27 constant: `fli f1, 2.5`. An integer operand is converted.

### 6.12 The vector unit

The vector unit works on the 81-trit `v` registers as 9 tryte lanes or 3 word lanes (§3.4). All three majors are 1-word R-format instructions. **Vector instructions never change S or C.** With `MODE.VE` = 0 every vector instruction raises vector −13 and changes nothing; reset sets `VE` = +1 and zeroes the registers.

#### 6.12.1 VALU (major 9)

| B | Syntax | Operation |
|---|---|---|
| 1 … 27 | `vadd vd, va, vb`, … | ALU function B (§6.2) on each of the 9 tryte lanes, wrapping within the lane |
| 28 … 54 | `vaddw vd, va, vb`, … | ALU function B − 27 on each of the 3 word lanes |
| 55 | `vmrg vd, vm, vs` | `vd := MRG(vd, vm, vs)` over all 81 trits: the mask's 0 keeps `vd`, +1 takes `vs`, −1 clears |
| 56 | `vsum rd, va` | `rd` (a general register) := the exact sum of `va`'s 9 tryte lanes |
| 57 | `vdotw rd, va, vb` | `rd` := Σ `va.lane[j]` × (the lowest trit of `vb.lane[j]`), over the 9 tryte lanes |
| other | – | `ILL` |

- **Only the lane-wise ALU functions are valid**, in either range: `add`, `sub`, `min`, `max`, `tmul`, `tsat`, `cons` (three operands) and `neg`, `abs`, `sgn` (two). The mnemonic is `v` + the ALU name, with a `w` suffix for word lanes: `vadd`, `vtmul`, `vnegw`, `vsubw`. Any other function (`mul`, `div`, shifts, carries, `cmp`, the binary operations) raises −1.
- `vdotw` takes one weight trit per lane, so each product is a pass, a negation or a zero: the inner loop of a ternary-weight network.

#### 6.12.2 VMEM (major 10)

| B | Syntax | Operation | Faults |
|---|---|---|---|
| 1 | `vld vd, [ra ± off]` | load 9 trytes | Mem |
| 2 | `vst vd, [ra ± off]` | store 9 trytes; all three words are checked before any is written | Mem |

- The address is `ra + 9·q3`; in assembly the offset is in trytes, a multiple of 9 within ±117.
- **The address must be a multiple of 9** (vector −5 otherwise). It is the centre of the vector's first word, so the vector occupies trytes `addr − 1 … addr + 7` (words at addr, addr + 3, addr + 6). Trine passes the second tryte of a `[9]tryte` array for this reason.

#### 6.12.3 VMOV (major 11)

| B | Syntax | Operation |
|---|---|---|
| 1 | `vget rd, va, j` | `rd :=` tryte lane j (0 … 8) of `va` |
| 2 | `vput vd, ra, j` | tryte lane j of `vd` := the low tryte of `ra` |
| 3 | `vgetw rd, va, k` | `rd :=` word lane k (0 … 2) of `va` |
| 4 | `vputw vd, ra, k` | word lane k of `vd` := `ra` |
| 5 | `vbcst vd, ra` | every tryte lane := the low tryte of `ra` |
| 6 | `vbcstw vd, ra` | every word lane := `ra` |
| other | – | `ILL`; so is a lane number out of range |

The lane number is in the Z field.

### 6.13 Pseudo-instructions

| Written | Expands to |
|---|---|
| `li rd, n` | `LIQ` if n is a known constant within ±364, otherwise `LI` |
| `la rd, label` | `LI` (always 2 words) |
| `mov rd, ra` | `add rd, ra, r0` |
| `mov rd, n` | as `li rd, n` |
| `not rd, ra` | `neg rd, ra` |
| `cmp ra, b` | `ALU`/`ALUQ`/`ALUI` function `CMP` with rd = 0 |
| `call f` | `jal r12, f` |
| `ret` | `jalr r0, r12, 0` |
| `push ra` | `sub sp, sp, 3` then `stw ra, [sp]` |
| `pop rd` | `ldw rd, [sp]` then `add sp, sp, 3` |
| `b target` | branch with condition "always" |
| `op.l …` | the long (2-word) form of `op` |

## 7. Memory and the MMU

### 7.1 Physical memory map

Physical memory is divided into **frames of 3^9 = 19 683 trytes**. Frame f is centred on `19683·f` and covers `19683·f ± 9841`. Frame numbers are 18 trits, so the physical address space is the full 27 trits.

| Frames | Physical addresses | Contents |
|---|---|---|
| 0 … N−1 | −9 841 upward | **RAM**. The default N is 3^9 frames = 3^18 trytes; `setun --ram FRAMES` sets it, and `SYSCTL.RAM_FRAMES` reports it. RAM reads 0 until written. |
| −3 … −1 | −68 890 … −9 842 | **BIOS ROM**, 3^10 trytes. Writes raise a bus error (−10). |
| −81 … −55 | −1 604 164 … −1 072 724 | **framebuffer**, 27 frames = 3^12 trytes; ordinary read/write memory that the display shows (§10.10) |
| every other frame | – | nothing; an access raises a bus error (vector −10) |

Rule of thumb: **non-negative frames are RAM, negative frames are devices.** Device DMA (disks, network card) uses physical addresses and bypasses the MMU.

### 7.2 Virtual addresses: 9 | 9 | 9

With `MODE.PG` = 0, virtual and physical addresses are the same. With `PG` = +1, every address the CPU uses (instruction fetch, loads, stores, the vector-table read) is virtual. A virtual address is a whole word, so there is no canonical-form rule:

```
  trit:  1 ......... 9   10 ........ 18   19 ........ 27
        [  L1 index   ] [   L2 index   ] [    offset    ]
          3^9 entries      3^9 entries      −9841 … 9841
```

There are two table levels, as on the 386.

- **A page-table entry is one word**, and a table has 3^9 entries × 3 trytes = 3^10 trytes, that is 3 contiguous frames.
- Entry i (−9 841 … 9 841) of a table centred at T is the word at `T + 3·i`.
- The level-1 table is centred at `PTB`. A level-2 table is centred at the centre of the frame its level-1 entry names, so it occupies that frame and its two neighbours.
- The ISA requires every table to sit at a multiple of 3^10; the emulator does not check this.
- A minimal process needs a level-1 and one level-2 table: 6 pages, about 118 000 trytes.

### 7.3 Page-table entries

A **zero entry means "not present"**, so zeroed memory is a valid empty table. The weight column gives each field's contribution, so an entry is `19683·FRAME + 6561·V + 2187·X + 729·RING + 243·AD + 81·G + 27·SW1 + 9·SW2`.

| Trits | Weight | Field | Values |
|---|---|---|---|
| 1–18 | 3^9 | `FRAME` | physical frame number (balanced) |
| 19 | 6 561 | `V` | 0 not present, +1 read/write, −1 read-only |
| 20 | 2 187 | `X` | 0 no execute, +1 execute, −1 execute-only (not readable) |
| 21 | 729 | `RING` | lowest ring that may access the page: −1 user, 0 drivers, +1 kernel only |
| 22 | 243 | `AD` | set by hardware: 0 untouched, −1 accessed, +1 written (dirty) |
| 23 | 81 | `G` | +1 global: kept by `tlbflush` (for kernel mappings) |
| 24–25 | 27, 9 | `SW` | 2 trits free for the operating system (Trinux marks copy-on-write here) |
| 26–27 | 3, 1 | – | reserved, must be 0 |

### 7.4 The walk

1. `E1 :=` the word at `PTB + 3·a[1:9]`.
2. `E2 :=` the word at `19683·E1.FRAME + 3·a[10:18]`.
3. Physical address `:= 19683·E2.FRAME + a[19:27]`.

Both entries are checked, so the permission is the **more restrictive of the two levels**:

| Check | Fault |
|---|---|
| `V = 0` at either level | vector −3, page fault |
| a write needs `V = +1` | vector −4, protection |
| an instruction fetch needs `X ≠ 0` | vector −4 |
| a data read needs `X ≠ −1` | vector −4 |
| `MODE.RING ≥ RING` at both levels | vector −4 |
| a table entry at a physical address with nothing behind it | vector −10 |

On a fault `FADDR` receives the virtual address and `FCAUSE` the vector, the access type (−1 read, 0 fetch, +1 write) and the ring. The hardware updates `AD` in the level-2 entry only: an access makes an untouched entry −1, and a write makes it +1.

- There is no separate "supervisor may not touch user pages" rule: ring +1 passes every `RING` check.
- **Instruction fetch across pages.** A 2-word instruction whose first word is the last word of a page continues on the next page. Its second word is translated separately; if that fetch faults, `FADDR` holds the second word's address and the instruction has no effect. Aligned word accesses never cross a page, but a real81 or vector access can, and the stores check every word before writing.

### 7.5 TLB

The ISA leaves the TLB architecturally invisible and asks software to maintain it:

- after changing a page-table entry, `tlbinv` with the virtual address;
- after writing `PTB`, `tlbflush`, which keeps `G` entries;
- writing `PTB` does not flush implicitly.

The emulator is stricter than this. Its software TLB (1 024 entries per access kind) watches every frame holding page-table entries it has used, and any write to one (a CPU store or disk DMA) flushes it. Writing `PTB` flushes it too, and `tlbinv` flushes all of it. Translations are therefore never stale in the emulator, but code written for the ISA must still issue the instructions.

## 8. Privilege rings and IOPL

There are 3 rings on one trit: **−1 user, 0 drivers, +1 kernel**. Every check has the form "current ring ≥ required ring".

| Resource | Requirement | Fault otherwise |
|---|---|---|
| `mfc`, `mtc`, `iret`, `hlt`, `tlbinv`, `tlbflush` | ring +1 | −2 |
| `in`, `out` | `RING ≥ MODE.IOPL` (reset value 0: drivers and kernel) | −2 |
| a page | `RING ≥` the `RING` trit at both levels | −4 |
| FPU, vector unit | `MODE.FE`, `MODE.VE` = +1 (set by the kernel) | −12, −13 |

- **Only a trap or an interrupt raises the ring, and every trap enters ring +1.** A kernel dispatches to driver-ring code in software.
- **Only `iret` lowers it**, by restoring `EMODE`.
- Each change of ring swaps `r13` with `KSP`, so user code and the kernel have separate stacks without any software help.
- `IOPL` lets the kernel give port I/O to ring 0 (the default), to user code (−1) or to nobody but itself (+1).

## 9. Traps and interrupts

### 9.1 The vector table

There are **243 = 3^5 vectors, numbered −121 … 121**. Each entry is one word holding a handler address, at `ITB + 3·v`. The table is 729 trytes centred on `ITB`. With paging on, `ITB` is a virtual address. Faults are negative, the system call is at the centre, and the outside world is positive.

| Vector | Cause | `EPC` holds |
|---|---|---|
| **0** | system call (`sys n`) | the next instruction |
| −1 | illegal or reserved instruction | the instruction |
| −2 | privilege violation (ring or `IOPL`) | the instruction |
| −3 | page fault (not present) | the instruction |
| −4 | protection fault | the instruction |
| −5 | misaligned access | the instruction |
| −6 | divide by zero | the instruction |
| −7 | breakpoint (`brk`) | the next instruction |
| −8 | single step (`TF`) | the next instruction |
| −9 | double fault (a fault while reading a vector-table entry) | the original trap's `EPC` |
| −10 | bus error (no device at the physical address) | the instruction |
| −11 | floating-point exception (unmasked) | the instruction |
| −12 | FPU not available (`MODE.FE` = 0) | the instruction |
| −13 | vector unit not available (`MODE.VE` = 0) | the instruction |
| −14 … −26 | reserved CPU exceptions | |
| −27 … −121 | free for the operating system | |
| **+1 … +81** | hardware IRQ 0 … 80 (vector = IRQ + `TPIC.VECBASE`, default +1) | the next instruction |
| +82 … +121 | free for the operating system | |

A misaligned instruction fetch is a vector −5 fault with `EPC` and `FADDR` both the bad PC.

### 9.2 Entry

When an instruction faults, executes `sys` or `brk`, completes with `TF` set, or an interrupt is taken, the CPU:

1. sets `FCAUSE` (§9.3) and `FADDR` (the faulting virtual address, or 0 when the trap is not a memory access), `EMODE := MODE` and `EPC :=` the address from the table above;
2. if `MODE.RING < +1`, swaps `r13 ↔ KSP` (switches to the kernel stack);
3. sets `MODE.RING := +1`, `IE := 0`, `TF := 0`;
4. loads `PC :=` the word at `ITB + 3·vector`.

S, C, `FE`, `VE`, `PG` and `IOPL` keep their values inside the handler. The faulting instruction has had no effect.

### 9.3 FCAUSE and FADDR

| Trits | Weight | Field |
|---|---|---|
| 23–27 | 1 | the vector, −121 … 121 |
| 22 | 243 | access type: −1 read, 0 fetch, +1 write |
| 21 | 729 | the ring the CPU was in |
| 1–18 | 19 683 | extra information: the `sys` number, or the FPU exception (1 IV, 2 DZ, 3 OF, 4 UF, 5 IX) |

So `FCAUSE = 19683·info + 729·ring + 243·access + vector`, and `ext r1, r1, 23, 5` extracts the vector. For traps that are not memory accesses (system calls, interrupts, illegal instructions and so on) the emulator writes −1 into the access trit and 0 into `FADDR`.

### 9.4 IRET

1. `MODE := EMODE`; `PC := EPC`.
2. If the new `RING < +1`, swap `r13 ↔ KSP`.

A kernel that wants nested traps must save `EPC`, `EMODE` (and `FADDR`/`FCAUSE` if it needs them) before re-enabling interrupts. Returning to the instruction after a fault simply means adding 3 or 6 to `EPC` before `iret`.

### 9.5 Interrupts

- The CPU checks for an interrupt **before each instruction**, and only while `MODE.IE` = +1. The interrupt controller (§10.3) supplies the vector of the highest-priority pending, unmasked line.
- Taking an interrupt does not acknowledge it. The handler reads `TPIC.CLAIM`, which returns the line and marks it in service, and writes the line to `TPIC.EOI` when done. A line that is never claimed stays pending and interrupts again as soon as `IE` returns to +1.
- `hlt` waits for an interrupt. With `IE` = 0 it waits until reset.

### 9.6 Single step

When `TF` is +1 at the start of an instruction and the instruction completes without a fault, vector −8 is raised afterwards with `EPC` = the next instruction. Trap entry clears `TF`, and `iret` restores it from `EMODE`, so after `iret` exactly one instruction runs before the next step trap. `hlt` does not step-trap.

### 9.7 Double and triple faults

- If reading the vector-table entry faults (for example `ITB` points at unmapped memory), the CPU delivers vector −9 instead, with the original `EPC`. In the emulator the second entry overwrites `EMODE` with the MODE of the first (already ring +1) and `FCAUSE` with vector −9, so the original MODE is lost.
- If reading the double-fault entry also faults, the CPU resets (a "triple fault"). In the emulator this resets the CPU alone; devices and memory keep their state.

## 10. Devices and I/O ports

### 10.1 Port addressing

- There are **3^9 = 19 683 ports**, so a port number is one tryte (−9 841 … 9 841).
- Each device has a **block of 27 ports, centred**: block b holds ports `27·b − 13 … 27·b + 13`, and the device register at offset r is port `27·b + r`.
- So `li r3, 162` followed by `in r1, [r3+2]` reads register 2 of block 6.
- Every port transfers a word. There are 729 blocks; negative blocks are reserved for expansion buses.
- Unassigned ports read 0 and ignore writes. Registers not listed below behave the same way.

| Block | Base port | Device | IRQ |
|---|---|---|---|
| 0 | 0 | system control (`SYSCTL`) | – |
| 1 | 27 | interrupt controller (`TPIC`) | – |
| 2 | 54 | interval timer | 0 |
| 3 | 81 | real-time clock | 5 |
| 4 | 108 | keyboard | 1 |
| 5 | 135 | mouse | 2 |
| 6 | 162 | serial console | 3 |
| 7 | 189 | disk 0 | 4 |
| 8 | 216 | disk 1 | 7 |
| 9 | 243 | display controller | 6 |
| 10 | 270 | network card (`TNIC`) | 8 |
| 11 | 297 | random-number generator (`TRNG`) | – |

Devices do not raise interrupts unless their own `CTRL` enables them (the timer and RTC always raise theirs), and every line is masked in the `TPIC` at reset.

**Time.** Device time is emulated and counted in instructions: 100 ns per instruction by default (10 MIPS), set with `setun --mips N`. The timer, the RTC's sub-second part and the display's frame counter all follow it, which keeps the machine deterministic.

### 10.2 SYSCTL (block 0, port 0)

| Offset | Register | Access |
|---|---|---|
| 0 | `SYSID`: 26 (`0t100T`, "11̅" in base 27) | read |
| 1 | `RAM_FRAMES`: RAM size in frames | read |
| 2 | `POWER`: write +1 to reset (CPU and devices; RAM, disks and the clock are kept), −1 to power off | write |
| 3 | `DEBUG`: each word written goes to the host log | write |
| 4 | `BOOTINFO` | read; the emulator returns 0 |

### 10.3 TPIC interrupt controller (block 1, port 27)

81 IRQ lines, 0 … 80. **A lower number has higher priority.** All lines are masked at reset.

| Offset | Register | Access |
|---|---|---|
| 0 | `VECBASE`: the vector of IRQ 0, default +1 | read/write |
| 1 | `CLAIM`: returns the highest-priority pending, unmasked line that may interrupt, clears its pending state and marks it in service; −1 if there is none | read |
| 2 | `EOI`: write a line number to end its service | write |
| 3 | `UNMASK`: write a line number | write |
| 4 | `MASK`: write a line number | write |
| 5 | `SELECT`: a line number for `STATE` | read/write |
| 6 | `STATE` of the selected line: −1 masked, 0 idle, +1 pending | read |

A device raising its line makes it pending. A line interrupts the CPU only while no line of equal or higher priority (lower or equal number) is in service. The interrupt vector is `VECBASE + line`.

### 10.4 Interval timer (block 2, port 54, IRQ 0)

A 1 MHz time base.

| Offset | Register | Access |
|---|---|---|
| 0 | `CTRL`: −1 one-shot, 0 stopped, +1 periodic. Writing it also reloads `COUNT` from `PERIOD`. | read/write |
| 1 | `PERIOD` in µs (negative values store 0) | read/write |
| 2 | `COUNT`: µs remaining | read/write |
| 3 | `UPTIME`: µs since power-on, wrapping as a word after about 44 days | read |

When `COUNT` reaches 0 the timer raises IRQ 0. A one-shot timer then stops; a periodic one reloads and keeps its phase, and several missed periods raise the IRQ once. A period of 0 never fires.

### 10.5 Real-time clock (block 3, port 81, IRQ 5)

| Offset | Register | Access |
|---|---|---|
| 0 | `SECONDS`: Unix time. A word holds ±3.8·10^12 seconds, so there is no year-2038 problem. Writing it also zeroes `MICROS`. | read/write |
| 1 | `MICROS`: microseconds within the second | read |
| 2 | `ALARM`: Unix time, or 0 for off. When `SECONDS` reaches it, IRQ 5 is raised and `ALARM` returns to 0. | read/write |

The front end sets the clock from the host at power-on.

### 10.6 Keyboard (block 4, port 108, IRQ 1)

| Offset | Register | Access |
|---|---|---|
| 0 | `DATA`: the next key event, **+code on press, −code on release**, 0 if the queue is empty. Codes are USB HID usage IDs. | read |
| 1 | `COUNT`: queued events | read |
| 2 | `CTRL`: write +1 IRQ on each event, 0 off, −1 flush the queue; reads 1 or 0 | read/write |
| 3 | `LEDS`: stored, for the host | read/write |

It is a queued event device with an interrupt, not a PS/2 emulation.

### 10.7 Mouse (block 5, port 135, IRQ 2)

| Offset | Register | Access |
|---|---|---|
| 0 | `DX`: accumulated motion; reading clears it | read |
| 1 | `DY`: likewise | read |
| 2 | `BUTTONS` = `L + 3·M + 9·R` (1 = down). Reading pops the oldest queued button change and latches the position where it happened; with none queued it is the current state. | read |
| 3 | `WHEEL`: accumulated steps; reading clears it | read |
| 4 | `CTRL`: +1 IRQ on motion, button or wheel changes | read/write |
| 5 | `ABS_X`: absolute position in framebuffer pixels (or the latched position after a `BUTTONS` read that popped a change) | read |
| 6 | `ABS_Y` | read |
| 7 | `PENDING`: button changes still queued (a FIFO of 27), so a quick press and release between two reads is not lost | read |

### 10.8 Serial console (block 6, port 162, IRQ 3)

| Offset | Register | Access |
|---|---|---|
| 0 | `DATA`: one tryte of text (§3.2). Reading returns the next received tryte, or 0 if there is none. Writing sends one. | read/write |
| 1 | `RX_COUNT`: received trytes waiting | read |
| 2 | `TX_READY`: +1 ready, 0 busy (the emulator is always ready) | read |
| 3 | `CTRL`: +1 IRQ on receive | read/write |

The host side carries an overlined character as the character followed by U+0305, and a 3-tryte character as the one Unicode character, both ways.

### 10.9 Disks (block 7, port 189, IRQ 4; block 8, port 216, IRQ 7)

A sector is **one page, 3^9 trytes**. The two disks are identical; the ISA lists disk 1 as reserved, and the emulator implements it as a second disk.

| Offset | Register | Access |
|---|---|---|
| 0 | `CMD`: writing starts the operation: −1 read, +1 write, 0 flush. Ignored while busy. | read/write |
| 1 | `STATUS`: −1 error, 0 idle, +1 busy | read |
| 2 | `LBA`: first sector | read/write |
| 3 | `COUNT`: sectors | read/write |
| 4 | `DMA`: physical address of the first tryte of the first frame | read/write |
| 5 | `SECTORS`: capacity (0: no disk) | read |
| 6 | `ERROR`: 0 none, 1 no media, 2 LBA out of range, 3 DMA address not page-aligned, 4 bus error during DMA, 5 host I/O error, 6 bad command | read |
| 7 | `CTRL`: +1 IRQ on completion | read/write |

- DMA uses physical addresses and bypasses the MMU. **"Page-aligned" means the lowest tryte of a frame**, `19683·f − 9841`, not its centre; consecutive sectors go to consecutive frames. The BIOS reads sector 0 to address 9 842, the start of frame 1.
- In the emulator a command completes before the next instruction runs. Software should still poll `STATUS` or wait for the IRQ.
- Disk images on the host use the T1B2 format: one tryte per little-endian `i16`, so a sector is 39 366 bytes.

### 10.10 Display (block 9, port 243, IRQ 6)

| Offset | Register | Access |
|---|---|---|
| 0 | `MODE`: −1 text 80×30 (reset), 0 graphics 640×480, +1 graphics 800×600 | read/write |
| 1 | `FB_BASE`: physical address of the first framebuffer tryte, −1 604 164 | read |
| 2 | `WIDTH`: pixels, or 80 cells in text mode | read |
| 3 | `HEIGHT`: pixels, or 30 cells in text mode | read |
| 4 | `PITCH`: trytes per line: 640, 800, or 240 in text mode (80 one-word cells) | read |
| 5 | `FRAME`: vsync counter, 60 per emulated second | read |
| 6 | `CTRL`: +1 IRQ on vsync | read/write |
| 7 | `CURSOR`: text cell index of the cursor | read/write |
| 8 | `HWC_X`: hardware mouse cursor, pixels | read/write |
| 9 | `HWC_Y` | read/write |
| 10 | `HWC_ON`: +1 show it | read/write |

**Graphics modes: one tryte per pixel, 3^9 colours.** Pixel (x, y) is the tryte at `FB_BASE + y·PITCH + x`. Trits 1–3 are red, 4–6 green and 7–9 blue, each a balanced value v from −13 to +13 shown at sRGB intensity `round(255·(v + 13)/26)`.

| Pixel (base 27) | Colour |
|---|---|
| `DDD` | white |
| `D̅D̅D̅` | black |
| `DD̅D̅` | red |
| `D̅DD̅` | green |
| `D̅D̅D` | blue |
| `000` | 50 % grey (a zeroed framebuffer is mid-grey) |

Negating a pixel inverts its colour. 800×600 needs 480 000 trytes, which fits the 3^12-tryte region.

**Text mode: 80×30 cells, one word per cell**, drawn with an 8×8 font with doubled rows into 640×480. Cell k (row-major, k = 80·row + col) occupies trytes `FB_BASE + 3k` (character), `FB_BASE + 3k + 1` (attribute) and `FB_BASE + 3k + 2` (reserved); as a word it is at address `FB_BASE + 3k + 1`, with the character in its most significant tryte.

- **Character:** a code point (§3.2). Codes up to 32 show blank; a tryte below −32 shows the character one row lower under a bar; characters without a glyph show as a hollow box. The font covers Latin, Greek, box drawing, blocks and Cyrillic U+0400 … U+045F.
- **Attribute:** foreground red, green, blue in trits 1–3, background in trits 4–6, each trit −1 = 0 %, 0 = 50 %, +1 = 100 % (27 colours each); trits 7, 8, 9 = +1 turn on underline, blink and bold. Example: `0t111TTT000` is white on black.
- The cursor cell shows a blinking bar in its two bottom rows.

### 10.11 Network card TNIC (block 10, port 270, IRQ 8)

Ethernet II frames without the FCS, 14 … 1 514 octets, **one octet (0 … 255) per tryte**. DMA uses physical tryte addresses of any alignment and bypasses the MMU. Every command completes before the next instruction. Details: [`NETWORK.md`](NETWORK.md) §1.

| Offset | Register | Access |
|---|---|---|
| 0 | `CMD`: +1 send `TX_LEN` octets from `TX_ADDR`; −1 copy the oldest received frame to `RX_ADDR` and drop it from the queue; 0 drop it without copying | write |
| 1 | `STATUS`: 0 ok, −1 the last command failed | read |
| 2 | `TX_ADDR` | read/write |
| 3 | `TX_LEN` | read/write |
| 4 | `RX_ADDR` | read/write |
| 5 | `RX_LEN`: length of the oldest received frame, 0 if none | read |
| 6 | `RX_COUNT`: frames queued (at most 27) | read |
| 7 | `CTRL`: +1 IRQ 8 when a frame arrives | read/write |
| 8 | `MAC_HI`: MAC octets 0 … 2 as `o0·65536 + o1·256 + o2` | read |
| 9 | `MAC_LO`: octets 3 … 5 | read |
| 10 | `LINK`: +1 a network is attached | read |
| 11 | `ERROR`: 0 none, 1 no link, 2 bad length, 3 a tryte outside 0 … 255, 4 bus error during DMA, 5 nothing received, 6 bad command, 7 transmit queue (27 frames) full | read |
| 12 | `DROPS`: received frames lost to a full queue | read |

The default MAC address is 52:54:00:12:34:56.

### 10.12 Random-number generator TRNG (block 11, port 297)

| Offset | Register | Access |
|---|---|---|
| 0 | `DATA`: the next random octet, 0 … 255 | read |
| 1 | `SOURCE`: +1 seeded from the host's entropy, 0 a fixed seed | read |

The octets are a ChaCha20 key stream (the RFC 8439 block function) under a 256-bit seed from the front end. Without one the seed is zero and every run gives the same numbers. Details: [`NETWORK.md`](NETWORK.md) §8.

## 11. Reset and boot

### 11.1 Reset state

Power-on, `SYSCTL.POWER` = +1 and a triple fault all reset the CPU:

| State | After reset |
|---|---|
| `PC` | −59 049 (−3^10, the centre of ROM frame −3) |
| general registers | 0 |
| control registers | 0, except `CPUID` |
| `MODE` | `RING` = +1, `FE` = +1, `VE` = +1, everything else 0 (paging off, interrupts off, `IOPL` = 0, S = C = 0) |
| FPU | all `f` registers 0; FSR = all masks set, no flags |
| vector unit | all `v` registers 0 |
| TLB | empty |

Power-on and `POWER` = +1 also reset the devices: every `TPIC` line masked with `VECBASE` = +1, the timer stopped, the display in text mode, and the disk and network registers cleared. RAM, the disk media, the clock and the input queues are kept.

### 11.2 The BIOS

The BIOS ([`bios/bios.tri`](../bios/bios.tri)) is a Trine program in ROM frames −3 … −1. Its startup code sets `sp`, installs a vector table that reports CPU faults (`ITB`), zeroes its globals and calls `main`, which:

1. reads the RAM size from `SYSCTL`, tests frame 0 and the last frame, reads the clock and asks both disks for their size;
2. shows the results on the text screen and copies them to the serial console;
3. reads **disk 0, sector 0** by DMA into **frame 1** (physical 9 842 … 29 524);
4. checks that the **last word of frame 1** (address 29 523) holds the **boot signature `0zB007AB13`** (115 067 810 199);
5. jumps to the **centre of frame 1, address 19 683**, with `r1` = the boot disk (0) and `r2` = the number of RAM frames, still in ring +1 with paging and interrupts off.

If there is no disk, the read fails or the signature is missing, it says why and starts a monitor on the keyboard and serial console (`help`, `info`, `time`, `dump A [N]`, `boot`, `reset`, `off`).

BIOS memory use: frame 0 is free (the RAM test writes there), frame 1 holds the boot sector, frame 2 the BIOS stack, frame 3 onward its globals.

### 11.3 The boot sector

| Sector trytes | Address | Contents |
|---|---|---|
| 0 … 9 839 | 9 842 … 19 681 | free for the boot program's data |
| 9 840 … 19 679 | 19 682 … 29 521 | code, entered at 19 683 |
| 19 680 … 19 682 | 29 522 … 29 524 | the signature word |

The boot program may use all of RAM. The BIOS never runs again after the jump, though its `ITB` stays set until the program changes it. `trine build --boot prog.tri -o prog.disk` makes such a disk image; [`BOOT.md`](BOOT.md) has the full protocol and [`bios/boot_hello.tri`](../bios/boot_hello.tri) an example. Trinux's loader then reads the kernel, builds page tables, sets `PTB`, turns on `PG` and enters the kernel ([`KERNEL.md`](KERNEL.md)).

## 12. The assembler

The assembler is `crates/asm` (`setun_asm::text`), with a disassembler (`setun_asm::disasm`) and the `setun-asm` command-line tool. [`ASSEMBLER.md`](ASSEMBLER.md) is its guide, and [`crates/demo/src/demo.s`](../crates/demo/src/demo.s) a complete program. This section summarises it; the instruction syntax is in §6.

### 12.1 Lines and values

```
label:  mnemonic operand, operand, …   ; comment
NAME = expression                      ; constant (also .equ NAME, expression)
```

- Labels end with `:`; a line may carry several. Mnemonics are case-insensitive. `;` starts a comment outside quotes.
- Values: decimal (`1_000`), `0t`, `0z` and `0x` literals (§2.3), characters `'A'`, `'Ж'`, `'\n'` (escapes `\n \t \r \0 \\ \' \"`), `$` (the current instruction's address), labels and constants.
- Expressions: `+ - * / ( )` on integers; `/` truncates. Unary minus.
- Registers: `r-13` … `r13`, `zero`, `fp`, `lr`, `sp`; `f-13` … `f13`; `v-13` … `v13`.
- Memory operands: `[ra]`, `[ra + expr]`, `[ra - expr]`, or `[expr]` (base `r0`). Offsets are in trytes.
- A label is a word address (a multiple of 3); a string's first tryte is at `label − 1`.

### 12.2 Short and long forms

Where an instruction has a 1-word and a 2-word encoding, the assembler picks the short one when it fits, and `.l` on the mnemonic forces the long one.

| Written | Short form when | Long form |
|---|---|---|
| any ALU op with a constant, `cmp ra, n` | the constant fits ±13 | `ALUI` |
| `li rd, n`, `mov rd, n` | the constant fits ±364 | `LI` |
| `ldt ldw stt stw swap`, `flds fldd fsts fstd` | the offset is a multiple of the access size and offset/size fits ±13 | `MEMD`, `FMEMD` |
| a branch | the target is within ±364 words | `BR`; relaxed automatically when a short branch turns out to be out of range |

A value that depends on a label is not known before layout, so it always gets the long form.

### 12.3 Directives

| Directive | Effect |
|---|---|
| `.org addr` | start a segment at a word address (a multiple of 3) |
| `.word v, …` | one word per value (labels allowed) |
| `.tryte v, …` | trytes packed 3 per word; consecutive `.tryte` lines pack as one run |
| `.string "text"` | one tryte per character (3 for characters above U+2671) plus a terminating 0, packed 3 per word |
| `.zero n` | n zero words |
| `.fill n, v` | n words of v |
| `.align n` | zero words up to the next multiple of n (a positive multiple of 3); `.align 9` for vectors |
| `.real27 x, …` / `.real81 x, …` | one or three words per real, written as a decimal (`2.5`, `-1e-3`) or an integer |
| `.equ NAME, v` / `NAME = v` | a constant |

### 12.4 Command line

```
setun-asm build prog.s -o prog.t1b2 [--listing]       # one segment's trytes
setun-asm build demo.s -o demo.t1b2 --rom --listing   # a full 3^10-tryte BIOS ROM image
setun-asm dis demo.t1b2 --rom                         # listing
setun-asm dis prog.t1b2 --origin N [--source]         # N = word address of the first word
setun --rom demo.t1b2                                 # run a ROM image
```

Every line the disassembler prints reassembles to the same words; anything that would not (data, non-canonical encodings) comes out as `.word`, and `.l` marks long forms that were not needed. Images use the T1B2 format. Trinux also has its own native assembler, `tas` ([`SELFHOST.md`](SELFHOST.md)).

## 13. Calling convention

Trine and Trinux use one convention, which matches the ISA's suggested register roles, so assembly and Trine code can call each other ([`LANGUAGE.md`](LANGUAGE.md) §11).

| Item | Rule |
|---|---|
| Arguments | `r1`–`r6`, left to right; more are pushed on the stack, the last one first |
| Return value | `r1`; a `real` in `f1` |
| Real arguments | `f1`–`f9`, in order among the reals (integer arguments still take `r1`–`r6` in order among themselves) |
| Caller-saved | `r1`–`r10`, `r-10`–`r-13`; all FPU registers |
| Callee-saved | `r-1`–`r-9`, `r11` (`fp`), `r13` (`sp`). Trine keeps register locals in `r-1`–`r-9`, so assembly called from Trine must preserve them. |
| Return address | `r12` (`lr`), set by `call`/`jal`; a function that calls others saves it |
| Stack | grows toward negative addresses; always word-aligned; `push`/`pop` move `sp` by 3 |
| Frame | `fp` points at the saved `fp`; locals sit below it |
| Structs and arrays | passed as pointers |

**System calls (Trinux).** `sys n` with the arguments in `r1`–`r6`; the result comes back in `r1`, and a failure returns −errno. The numbers are Linux's generic table (`asm-generic/unistd.h`), for example `read` 63, `write` 64, `exit` 93, `futex` 98. User programs start with argc, argv and envp in `r1`–`r3` and `sp` set by the kernel ([`KERNEL.md`](KERNEL.md) §5).

**Interrupt handlers** must preserve every register they use (Trine's `interrupt fn` does this and ends with `iret`), and must claim and acknowledge their IRQ at the `TPIC`.

## 14. Programming examples

These fragments use the assembler's syntax and the calling convention of §13. They were assembled and run on the emulator.

### 14.1 A loop

The sum 1 + 2 + … + n. `sub` sets S, so the loop needs no separate compare.

```
; r1 := 1 + 2 + ... + r2   (r2 >= 0)
sum:    li    r1, 0
        cmp   r2, 0
        ble   done            ; nothing to add
loop:   add   r1, r1, r2
        sub   r2, r2, 1       ; S := sign(r2)
        bgt   loop
done:   ret
```

### 14.2 A three-way branch

One `cmp` and one `br3` decide less, equal and greater.

```
; r1 := -1, 0 or +1 as r1 <, =, > r2
cmp3:   cmp   r1, r2          ; S := sign(r1 - r2), exact
        br3   less, more      ; S < 0 -> less, S > 0 -> more
        li    r1, 0           ; S = 0 falls through
        ret
less:   li    r1, -1
        ret
more:   li    r1, 1
        ret
```

### 14.3 A masked store

Make a page read-only by setting the `V` trit (trit 19, weight 3^8) of its page-table entry to −1, leaving every other trit (frame, `X`, `RING`, `AD`, the software trits) as it is. One atomic `stwm` replaces a load, two tritwise operations and a store.

```
; r1 = address of the page-table entry, r4 = the virtual address it maps
        li     r2, 0t1_0000_0000       ; mask: +1 in trit 19 only
        li     r3, -0t1_0000_0000      ; source: -1 in trit 19
        stwm   r3, [r1], r2            ; V := -1, all other trits kept
        tlbinv r4                      ; drop the old translation
```

### 14.4 A three-state lock with CAS3

The lock word is one trit: 0 free, +1 held, −1 held with waiters. `cas3` sets S to the sign of (old value − expected value), so `br3` would even tell "held" (+1) from "held with waiters" (−1); here both go to the slow path.

```
; r1 = address of the lock word (a multiple of 3, in a writable page)
mtx_lock:
        li    r3, 1
        li    r2, 0
        cas3  r2, [r1], r3      ; if the word is 0, it becomes +1
        br3   slow, slow        ; S != 0: someone holds it
        ret                     ; S = 0: acquired
slow:   li    r2, -1
        swap  r2, [r1]          ; mark "held with waiters"
        cmp   r2, 0
        bne   slow              ; still held: a real kernel sleeps here (futex)
        ret                     ; it was free: we hold it (as -1)

mtx_unlock:
        li    r2, 0
        swap  r2, [r1]          ; release
        cmp   r2, 0
        blt   wake              ; old value -1: there are waiters
        ret
wake:   ; wake one waiter (Trinux: futex, system call 98)
        ret
```

Trinux's `mtx_lock` and `mtx_unlock` in tlibc follow this scheme and sleep with `futex` instead of spinning.

### 14.5 A ternary dot product with the vector unit

`vdotw` multiplies 9 tryte lanes by 9 weight trits and sums the products in one instruction.

```
; r1 := sum over j = 0..8 of x[j] * w[j], where w[j] is -1, 0 or +1
; r2 = address of x, r3 = address of w (each a multiple of 9)
dot9:   vld   v1, [r2]
        vld   v2, [r3]
        vdotw r1, v1, v2
        ret

        .align 9
xs:     .tryte 1, 2, 3, 4, 5, 6, 7, 8, 9
ws:     .tryte 1, -1, 0, 1, -1, 0, 1, -1, 0     ; dot9(xs, ws) = -3
```

`.align 9` makes `xs` a multiple of 9, and 9 trytes later `ws` is one too. The general form for longer vectors loops over 9-tryte chunks and accumulates `vdotw` results with `add`.

## 15. The emulator

The machine exists as the emulator in `crates/core`; there is no hardware. It is written in Rust and runs the same core in three front ends:

- **Native window** (`setun`, `crates/native`): the display, keyboard, mouse and the serial console on stdin/stdout. Without `--rom` it boots the built-in BIOS. Options include `--disk FILE`, `--ram FRAMES`, `--mips N`, `--headless N` (run N instructions without a window and print the serial output and text screen) and `--net` (attach a network).
- **Console** (`setun --console`): no window; the terminal is the serial console (Ctrl-] leaves). Without `--disk` it boots a Trinux disk built from the source tree.
- **Browser** (`crates/web`): the core compiled to WebAssembly, with the display in a canvas.

**Determinism.** Time is counted in instructions (100 ns each by default), so the same disk and the same input always give the same machine state after the same number of steps. Every optimisation must preserve that state exactly. `setun-bench` (`crates/bench`) runs fixed workloads and hashes the whole machine (registers, control registers, FPU, all of RAM, the framebuffer, device time and serial output) against the golden hashes in `crates/bench/golden.txt`. `crates/core/tests/fast_path.rs` runs random programs through the fast and the reference paths and compares the whole state at every checkpoint.

**How it runs code** (details in [`PERFORMANCE.md`](PERFORMANCE.md)):

| Layer | What it does |
|---|---|
| Reference path | `Machine::step`: full decode and page walk for every instruction. Everything else must match it. |
| Batches | devices report how many steps remain until their next event; the CPU runs that many without touching them. Port I/O, `iret`, `mtc` and single-stepping take the single-step path. |
| Block interpreter | instructions decoded once into basic blocks (up to 256 instructions, ending at a branch, jump or a generic instruction), chained to their successors; a software TLB and per-page fetch and data windows skip most translation work. Writes to code or page tables drop stale blocks. |
| Cranelift JIT | native builds compile a block run 300 times, with the blocks reachable from it in the same page (up to 32), to machine code on background threads. Guest registers live in host registers; S and C are kept lazily. |
| WebAssembly JIT | the browser build compiles the same regions to small wasm modules (`crates/core/src/wjit.rs`), which the page compiles and links. |
| FPU | `setun-real` computes every result exactly and rounds once; fast paths in 256-bit integers cover the common operations. |

Compiled and interpreted execution give identical state, so when compiled code arrives does not change behaviour. `SETUN_JIT_OFF=1` runs the interpreter alone.

## 16. Performance

All figures were measured on 2026-10-07 on one core of a Ryzen 9 5950X.

**Against native code.** `setun-bench --vs-native` runs Rust ports of Trine programs, written line by line from the Trine source with results checked equal, against the same programs on the Setun-11̅ (best of 3 runs):

| Program | Native | Setun-11̅ | Overhead |
|---|---:|---:|---:|
| Mandelbrot (fixed-point arithmetic, framebuffer) | 9.6 ms | 74.8–77.7 ms | 7.8–8.1× |
| sieve + insertion sort (tryte array, word array) | 0.15 ms | 8.4–8.8 ms | 56–59× |

Mandelbrot is arithmetic in registers, where compiled guest code is fast. The sieve and sort are memory loops, where the host compiler still produces tighter code than Trine.

**`setun-bench` workloads:**

| Workload | Instructions | Time | Speed |
|---|---:|---:|---:|
| int (sieve, sort, strings) | 6.6M | 0.01 s | 557 MIPS |
| fpu (`math.tri`) | 1.2M | 0.02 s | 66 MIPS |
| mandel (fixed point, framebuffer) | 61.5M | 0.08 s | 825 MIPS |
| boot (Trinux to the shell) | 3.9M | 0.02 s | 309 MIPS |
| shell (`md5sum`, `sort`, `calc`, `ls` with paging) | 14.9M | 0.06 s | 269 MIPS |

**Cost per guest instruction**, in host cycles: in compiled code a compare costs about 0.4, an add 3–4 and a load or store about 4.5; interpreted instructions cost 8–40. An FPU add takes about 20–30 ns, nearly all of it the exact arithmetic.

**Browser.** The WebAssembly JIT runs about 288 MIPS under node.

Profiling methods, the per-instruction table and the history of these numbers are in [`PERFORMANCE.md`](PERFORMANCE.md).

---

## Appendix A. Opcode map

| A | Base 27 | Mnemonic | Length | Format | Operation |
|---|---|---|---|---|---|
| 0 | `000` | SYS group | 1 | R/S9 | system and control (§6.10) |
| 1 | `001` | `ALU` | 1 | R | `rd := ra ⊕ rb` (§6.2) |
| 2 | `002` | `ALUQ` | 1 | Q | `rd := ra ⊕ q3` |
| 3 | `003` | `MEM` | 1 | Q | load/store at `ra + size·q3` (§6.5) |
| 4 | `004` | `IO` | 1 | Q | port I/O at `ra + q3` (§6.9) |
| 5 | `005` | `JALR` | 1 | Q | `rd := PC + 3; PC := ra + 3·q3` (§6.8) |
| 6 | `006` | `LIQ` | 1 | S | `rd := imm6` (§6.4) |
| 7 | `007` | `BRQ` | 1 | S | conditional branch, ±364 words (§6.7) |
| 8 | `008` | `MEMR` | 1 | R | `STWM`, `CAS3` (§6.6) |
| 9 | `009` | `VALU` | 1 | R | vector arithmetic (§6.12) |
| 10 | `00A` | `VMEM` | 1 | R | vector load/store at `ra + 9·q3` |
| 11 | `00B` | `VMOV` | 1 | R | vector lane moves |
| 6 561 | `900` | `ALUI` | 2 | I | `rd := ra ⊕ imm27` |
| 6 562 | `901` | `MEMD` | 2 | I | load/store at `ra + imm27` |
| 6 563 | `902` | `LI` | 2 | J | `rd := imm27` |
| 6 564 | `903` | `LEA` | 2 | J | `rd := PC + imm27` |
| 6 565 | `904` | `JAL` | 2 | J | `rd := PC + 6; PC := PC + 3·imm27` |
| 6 566 | `905` | `BR` | 2 | J | conditional branch, any distance |
| 6 567 | `906` | `BR3` | 2 | B3 | three-way branch on S |
| 6 568 | `907` | `IOI` | 2 | J | port I/O at `imm27` |
| 6 569 | `908` | `FLD` | 2 | F | `EXT`, `INS`, `BEXT`, `BROT` (§6.3) |
| −6 561 | `9̅00` | `FALU` | 1 | R | FPU arithmetic (§6.11) |
| −6 560 | `9̅01` | `FCVT` | 1 | R | FPU conversions, moves, constants, FSR |
| −6 559 | `9̅02` | `FMEM` | 1 | Q | FPU load/store at `ra + size·q3` |
| −4 374 | `6̅00` | `FMEMD` | 2 | I | FPU load/store at `ra + imm27` |
| −4 373 | `6̅01` | `FLI` | 2 | J | `fd := imm27` as a real27 |
| other | | – | | | reserved: `ILL` (vector −1) |

Function numbers within the other majors:

| Major | B = 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| SYS (0 = `ILL`) | `NOP` | `HLT` | `BRK` | `SYS` | `IRET` | `MFC` | `MTC` | `TLBINV` | `TLBFLUSH` | `FENCE` | | | |
| MEM, MEMD | `LDT` | `LDW` | `STT` | `STW` | `SWAP` | | | | | | | | |
| MEMR | `STWM` | `CAS3` | | | | | | | | | | | |
| IO, IOI | `IN` | `OUT` | | | | | | | | | | | |
| FLD | `EXT` | `INS` | `BEXT` | `BROT` | | | | | | | | | |
| VMEM | `VLD` | `VST` | | | | | | | | | | | |
| VMOV | `VGET` | `VPUT` | `VGETW` | `VPUTW` | `VBCST` | `VBCSTW` | | | | | | | |
| FALU | `FADD` | `FSUB` | `FMUL` | `FDIV` | `FREM` | `FMIN` | `FMAX` | `FSQRT` | `FNEG` | `FABS` | `FRND` | `FTRUNC` | `FCMP` |
| FCVT | `FCVTIF` | `FCVTFI` | `FTOW` | `FFRW` | `FEXP` | `FSCL` | `FMFS` | `FMTS` | `FCLASS` | `FCONST` | `FMOV` | | |

`FMEM`/`FMEMD` functions are ± the size in words: 1 `FLDS`, 2 `FLDD`, −1 `FSTS`, −2 `FSTD`. `VALU` functions are 1 … 27 (tryte lanes), 28 … 54 (word lanes), 55 `VMRG`, 56 `VSUM`, 57 `VDOTW`.

## Appendix B. ALU functions

For `ALU`, `ALUQ` and `ALUI`. All set S (`CMP` from the comparison); "C" marks the functions that also set C.

| B | Base 27 | Mnemonic | Operation | C | Faults |
|---|---|---|---|---|---|
| 1 | `001` | `add` | ra + b | C | |
| 2 | `002` | `sub` | ra − b | C | |
| 3 | `003` | `adc` | ra + b + C | C | |
| 4 | `004` | `sbc` | ra − b + C | C | |
| 5 | `005` | `mul` | low 27 trits of ra · b | C | |
| 6 | `006` | `mulh` | high 27 trits of ra · b | | |
| 7 | `007` | `div` | quotient, truncated | | −6 |
| 8 | `008` | `mod` | remainder of `div` | | −6 |
| 9 | `009` | `divr` | quotient, rounded to nearest | | −6 |
| 10 | `00A` | `modr` | balanced remainder | | −6 |
| 11 | `00B` | `min` | tritwise minimum | | |
| 12 | `00C` | `max` | tritwise maximum | | |
| 13 | `00D` | `tmul` | tritwise product | | |
| 14 | `01D̅` | `tsat` | tritwise sign of sum | | |
| 15 | `01C̅` | `cons` | tritwise consensus | | |
| 16 | `01B̅` | `sh` | ra · 3^b | | |
| 17 | `01A̅` | `cmp` | S := sign(ra − b); no write | | |
| 18 | `019̅` | `trit` | trit b of ra | | |
| 19 | `018̅` | `neg` / `not` | −ra | | |
| 20 | `017̅` | `sgn` | sign(ra) | | |
| 21 | `016̅` | `abs` | \|ra\| | | |
| 22 | `015̅` | `tlz` | leading zero trits | | |
| 23 | `014̅` | `band` | binary AND | | |
| 24 | `013̅` | `bor` | binary OR | | |
| 25 | `012̅` | `bxor` | binary XOR | | |
| 26 | `011̅` | `bandn` | ra AND NOT b | | |
| 27 | `010` | `bsh` | ra · 2^b or ⌊ra / 2^−b⌋ | | |
| 28 | `011` | `mrg` | trit-mask merge into rd | | |
| 29 | `012` | `tcp` | count of +1 trits | | −1 if b ∉ 0 … 27 |
| 30 | `013` | `tcn` | count of −1 trits | | −1 if b ∉ 0 … 27 |
| 31 | `014` | `trev` | reversed low trits | | −1 if b ∉ 0 … 27 |

## Appendix C. Branch conditions

The cond field (X) of `BRQ` and `BR`. Positions: `cond[1]` ↔ −1, `cond[2]` ↔ 0, `cond[3]` ↔ +1; + trits test S, − trits test C.

| Mnemonic | cond (base 3) | Value | Base 27 digit | Condition |
|---|---|---|---|---|
| `blt` | `0t100` | 9 | `9` | S < 0 |
| `beq` | `0t010` | 3 | `3` | S = 0 |
| `bgt` | `0t001` | 1 | `1` | S > 0 |
| `ble` | `0t110` | 12 | `C` | S ≤ 0 |
| `bge` | `0t011` | 4 | `4` | S ≥ 0 |
| `bne` | `0t101` | 10 | `A` | S ≠ 0 |
| `b` | `0t111` | 13 | `D` | always |
| `bov` | `0t1̅01̅` | −10 | `A̅` | C ≠ 0 |
| `bnov` | `0t01̅0` | −3 | `3̅` | C = 0 |
| `bnever` | `0t000` | 0 | `0` | never |
| – | mixed + and − | | | `ILL` |

Other patterns of a single sign are valid too: for example `0t1̅00` (−9) branches when C = −1. `BR3` takes no condition: it always dispatches on S.

## Appendix D. Vector table

| Vector | Cause | `EPC` | `FADDR` | `FCAUSE[1:18]` |
|---|---|---|---|---|
| −121 … −27 | free for the operating system | | | |
| −26 … −14 | reserved CPU exceptions | | | |
| −13 | vector unit not available | instruction | 0 | 0 |
| −12 | FPU not available | instruction | 0 | 0 |
| −11 | floating-point exception | instruction | 0 | 1 IV, 2 DZ, 3 OF, 4 UF, 5 IX |
| −10 | bus error | instruction | address | 0 |
| −9 | double fault | original trap's | 0 | 0 |
| −8 | single step | next instruction | 0 | 0 |
| −7 | breakpoint (`brk`) | next instruction | 0 | 0 |
| −6 | divide by zero | instruction | 0 | 0 |
| −5 | misaligned access | instruction | address | 0 |
| −4 | protection fault | instruction | address | 0 |
| −3 | page fault | instruction | address | 0 |
| −2 | privilege violation | instruction | 0 | 0 |
| −1 | illegal instruction | instruction | 0 | 0 |
| 0 | system call (`sys n`) | next instruction | 0 | n |
| +1 … +81 | IRQ 0 … 80 (with `VECBASE` = +1) | next instruction | 0 | 0 |
| +82 … +121 | free for the operating system | | | |

Handler address: the word at `ITB + 3·vector`.

## Appendix E. Port map

| Block | Ports | Base | Device | IRQ | Vector (default) | Registers (offset) |
|---|---|---|---|---|---|---|
| 0 | −13 … 13 | 0 | `SYSCTL` | – | – | 0 SYSID, 1 RAM_FRAMES, 2 POWER, 3 DEBUG, 4 BOOTINFO |
| 1 | 14 … 40 | 27 | `TPIC` | – | – | 0 VECBASE, 1 CLAIM, 2 EOI, 3 UNMASK, 4 MASK, 5 SELECT, 6 STATE |
| 2 | 41 … 67 | 54 | timer | 0 | +1 | 0 CTRL, 1 PERIOD, 2 COUNT, 3 UPTIME |
| 3 | 68 … 94 | 81 | RTC | 5 | +6 | 0 SECONDS, 1 MICROS, 2 ALARM |
| 4 | 95 … 121 | 108 | keyboard | 1 | +2 | 0 DATA, 1 COUNT, 2 CTRL, 3 LEDS |
| 5 | 122 … 148 | 135 | mouse | 2 | +3 | 0 DX, 1 DY, 2 BUTTONS, 3 WHEEL, 4 CTRL, 5 ABS_X, 6 ABS_Y, 7 PENDING |
| 6 | 149 … 175 | 162 | serial console | 3 | +4 | 0 DATA, 1 RX_COUNT, 2 TX_READY, 3 CTRL |
| 7 | 176 … 202 | 189 | disk 0 | 4 | +5 | 0 CMD, 1 STATUS, 2 LBA, 3 COUNT, 4 DMA, 5 SECTORS, 6 ERROR, 7 CTRL |
| 8 | 203 … 229 | 216 | disk 1 | 7 | +8 | as disk 0 |
| 9 | 230 … 256 | 243 | display | 6 | +7 | 0 MODE, 1 FB_BASE, 2 WIDTH, 3 HEIGHT, 4 PITCH, 5 FRAME, 6 CTRL, 7 CURSOR, 8 HWC_X, 9 HWC_Y, 10 HWC_ON |
| 10 | 257 … 283 | 270 | `TNIC` | 8 | +9 | 0 CMD, 1 STATUS, 2 TX_ADDR, 3 TX_LEN, 4 RX_ADDR, 5 RX_LEN, 6 RX_COUNT, 7 CTRL, 8 MAC_HI, 9 MAC_LO, 10 LINK, 11 ERROR, 12 DROPS |
| 11 | 284 … 310 | 297 | `TRNG` | – | – | 0 DATA, 1 SOURCE |
| −364 … −1, 12 … 364 | | | unassigned (negative blocks reserved for expansion buses) | | | read 0, writes ignored |
