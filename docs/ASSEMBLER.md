# Setun-386 assembly language

The assembler lives in `crates/asm` (`setun_asm::text`), along with the
disassembler (`setun_asm::disasm`) and the `setun-asm` command-line tool. The demo
ROM, [`crates/demo/src/demo.s`](../crates/demo/src/demo.s), is a complete
example program.

## Lines

```
label:  mnemonic operand, operand, …   ; comment
NAME = expression                      ; constant (also: .equ NAME, expression)
```

- Labels end with `:`, and a line may carry several.
- Mnemonics are case-insensitive.
- `;` starts a comment, except inside quotes.

## Values

| Syntax | Meaning |
|---|---|
| `42`, `-7`, `1_000` | decimal |
| `0t1T0`, `0t11̅0` | balanced ternary: `1`, `0`, and `T` or `1̅` for −1, most significant first = 6 |
| `0zDD5̅` | balanced base 27: `0`–`9`, `A`–`D` = 0…13 (either case), negative digits overlined with U+0305 (ISA §1.1) |
| `0x1F` | hexadecimal, for values from binary systems (no wrapping; use unary minus) |
| `'A'`, `'Ж'`, `'\n'` | a Unicode character (escapes: `\n \t \r \0 \\ \' \"`) |
| `$` | address of the current instruction |
| `label`, `NAME` | a label's address or a constant |
| `+ - * / ( )` | integer arithmetic (`/` truncates) |

**Registers:** `r-13` … `r13`. Aliases: `zero` = r0, `fp` = r11, `lr` = r12, `sp` = r13.

**Addresses** are word addresses, where a word's address is its centre tryte and a multiple of 3. A string's first tryte is at `label − 1`.

## Automatic form selection

Where an instruction has a short (1-word) and a long (2-word) encoding, the assembler picks the short one when it fits. Adding `.l` to the mnemonic forces the long one.

| Written | Short form when | Long form |
|---|---|---|
| `add r1, r2, imm` (any ALU op), `cmp r1, imm` | the constant fits ±13 | ALUI, 27-bit immediate |
| `li r1, imm` | the constant fits ±364 | LI |
| `ldw r1, [r2+off]` (`ldt stt stw swap`) | `off` is a multiple of the access size and `off/size` fits ±13 | MEMD with `off` in trytes |
| `blt label` (any branch) | the target is within ±364 words | BR. **Relaxed automatically**: a short branch that turns out to be out of range becomes long. |

A value that depends on a label is not known before layout, so it always gets the long form.

## Instructions

| Group | Syntax |
|---|---|
| ALU, 3 operands | `add sub adc sbc mul mulh div mod divr modr min max tmul tsat cons sh trit  rd, ra, rb\|imm` |
| ALU, 2 operands | `neg not sgn abs tlz  rd, ra` · `cmp ra, rb\|imm` · `mov rd, ra\|imm` |
| constants | `li rd, value` · `la rd, label` (always 2 words) · `lea rd, target` (PC-relative) |
| memory | `ldt ldw stt stw swap  rd, [ra ± off]` or `[label]` (offsets are always in **trytes**) |
| port I/O | `in rd, [ra ± q]` (q ±13) · `in rd, port` · `out` likewise |
| branches | `b blt beq bgt ble bge bne bov bnov bnever  target` · `br3 negative, positive` |
| calls | `call target` · `jal rd, target` · `jalr rd, ra[, q]` · `ret` |
| stack | `push r` · `pop r` (each 2 instructions, 3 trytes per word) |
| trit fields | `ext rd, ra, pos, len` · `ins rd, ra, pos, len` |
| system | `nop hlt brk iret tlbflush fence ill` · `tlbinv ra` · `sys n` · `mfc rd, creg` · `mtc creg, ra` |
| FPU arithmetic (ISA §14) | `fadd fsub fmul fdiv frem fmin fmax  fd, fa, fb` · `fsqrt fneg fabs frnd ftrunc fmov  fd, fa` · `fcmp fa, fb` |
| FPU conversions | `fcvtif fd, ra` · `fcvtfi rd, fa` · `ftow rd, fa` · `ffrw fd, ra` · `fexp rd, fa` · `fscl fd, fa, rb` · `fclass rd, fa` |
| FPU state and constants | `fmfs rd` · `fmts ra` · `fconst fd, zero\|one\|pi\|e\|ln3\|log3e\|log3_10` (or 0…6) · `fli fd, real` (2 words: any real27, e.g. `fli f1, 2.5`) |
| FPU memory | `flds fldd fsts fstd  fd, [ra ± off]` (real27 = 1 word, real81 = 3 words; the 1-word form when off is a multiple of the size within ±13 sizes) |

**Control register names:** `mode ptb faddr fcause itb epc emode ksp scratch cpuid cycle`, or `cr-13` … `cr13`.

**FPU registers** are `f-13` … `f13`; `f0` reads as zero. After `fcmp`, the usual branches test the result (`blt`, `beq`, …), and `bov` means *unordered* (a NaN was compared).

## Directives

| Directive | Effect |
|---|---|
| `.org addr` | start a segment at a word address (a multiple of 3) |
| `.word v, …` | one word per value (labels allowed) |
| `.tryte v, …` | trytes packed 3 per word. Consecutive `.tryte` lines pack as one run. |
| `.string "text"` | one tryte per character plus a terminating 0 |
| `.zero n` | `n` zero words |
| `.fill n, v` | `n` words of `v` |
| `.real27 x, …` / `.real81 x, …` | one word / three words per real, written as a decimal (`2.5`, `-1e-3`) or an integer |
| `.equ NAME, v` / `NAME = v` | constant |

## Disassembly

Every line the disassembler prints reassembles to exactly the same words. Anything that would not (data, non-canonical encodings) comes out as `.word`. It prints `.l` wherever the long form was used although the short one would fit.

## Command line

```
setun-asm build demo.s -o demo.t1b2 --rom --listing   # full BIOS ROM image
setun-asm build prog.s -o prog.t1b2                   # one segment's trytes
setun-asm dis demo.t1b2 --rom                         # listing
setun-asm dis demo.t1b2 --rom --source > again.s      # reassemblable source
setun --rom demo.t1b2                                 # run it
```

Images use the T1B2 format: one tryte per little-endian `i16` (ISA §8.5).
