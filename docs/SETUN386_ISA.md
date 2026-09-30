# Setun-386 Instruction Set Architecture

**Status: DRAFT v0.1, under design review. Nothing is implemented yet.**
Items marked **⚠ REVIEW** differ from, or add to, the original brief.

The Setun-386 is a balanced-ternary machine sized like an Intel 386. Its
memory, register file, paging, privilege rings and I/O are rich enough for a
kernel with processes and a windowing GUI. Like the 386, which boots as an
8086, it boots in **Setun-70 compatibility mode**. The BIOS then switches it
to **native mode**.

---

## 1. Data

| Name | Trytes | Trits | Range | ≈ bits |
|---|---|---|---|---|
| trit | – | 1 | −1, 0, +1 | 1.58 |
| **tryte** | 1 | 6 | −364..364 | 9.5 |
| duo | 2 | 12 | ±265 720 | 19 |
| triple | 3 | 18 | ±193 710 244 | 28.5 |
| **word** | 4 | 24 | ±141 214 768 240 | 38 |

- The **tryte is the smallest addressable unit**. Addresses count trytes.
- **Big-endian**: a multi-tryte value is stored most significant tryte first, and its address is the address of that first tryte. This matches the Setun-70.
- **No alignment requirement.** Any access may start at any tryte, and may cross a page boundary (see §5.5). **⚠ REVIEW**, because 3^7 is not divisible by 4.
- **There are no unsigned numbers.** Every value is a balanced-ternary signed integer. So loads need no sign- or zero-extension variants, and there is one comparison, not signed and unsigned versions. Negation is exact (flip every trit), and truncating low trits rounds to nearest.
- Trit positions follow the Setun convention: **1-based, MSB first**. In a word, `w[1]` has weight 3^23 and `w[24]` has weight 3^0. `w[a:b]` is a field.
- Text notation: trits `+ 0 -`. Nonary (base 9, one digit per trit pair) uses `W X Y Z 0 1 2 3 4` for −4..+4. For example the tryte `000---` is `0ZW`.

## 2. Registers

### 2.1 General registers

There are 27 general registers of 24 trits, numbered with a balanced 3-trit index `r-13 … r0 … r13`.

| Register | Hardware role | Suggested ABI role |
|---|---|---|
| `r0` | **always reads 0**; writes are discarded | zero |
| `r1`–`r6` | – | arguments; `r1` = return value |
| `r7`–`r10` | – | caller-saved temporaries |
| `r11` | – | frame pointer `fp` |
| `r12` | – | link register `lr` (return address from `JAL`) |
| `r13` | **stack pointer**: swapped with `KSP` on ring change (§7.3) | `sp`; the stack grows toward negative addresses |
| `r-1`–`r-9` | – | callee-saved |
| `r-10`–`r-13` | – | caller-saved temporaries, assembler scratch |

`PC` is a separate 24-trit register. It is not a general register.

### 2.2 MODE: status and flags (control register `cr0`)

| Trit | Field | Values |
|---|---|---|
| 24 | `RING` | current ring: −1 user, 0 drivers, +1 kernel |
| 23 | `IE` | +1 maskable interrupts enabled, 0 disabled, −1 reserved |
| 22 | `PG` | +1 paging on, 0 off |
| 21 | `CM` | +1 Setun-70 compatibility mode, 0 native |
| 20 | `IOPL` | lowest ring allowed to use `IN`/`OUT` (reset value 0) |
| 19 | `TF` | +1 single-step trap after each instruction |
| 18 | `S` | **sign of the last result**: −1, 0, +1 |
| 17 | `C` | **carry trit** out of the last add/subtract/multiply **⚠ REVIEW** |
| 1–16 | – | reserved, read as 0 |

**S** replaces the binary sign and zero flags, as the brief asks.

**C is a proposed addition.** In balanced ternary the carry out of an addition is itself a trit, and **C ≠ 0 exactly when the result overflowed**. One trit therefore gives both an overflow flag and multi-precision carry (`ADC`/`SBC`).

Because S and C live in MODE, they are saved automatically on a trap.

### 2.3 Control registers

Control registers are numbered `cr-13..cr13`, accessed with `MFC`/`MTC`, and **require ring +1**.

| CR | Name | Contents |
|---|---|---|
| 0 | `MODE` | §2.2 |
| 1 | `PTB` | page-table base: physical address of the level-1 table, a multiple of 2187 (§5) |
| 2 | `FADDR` | faulting virtual address |
| 3 | `FCAUSE` | `[19:24]` vector number, `[18]` access type (−1 read, 0 fetch, +1 write), `[17]` ring of the access, `[1:12]` extra info (for example the `SYS` number) |
| 4 | `ITB` | interrupt table base (§7) |
| 5 | `EPC` | PC saved on trap entry |
| 6 | `EMODE` | MODE saved on trap entry |
| 7 | `KSP` | the other stack pointer, swapped with `r13` on ring change |
| 8 | `SCRATCH` | free for the kernel (trap-entry save slot) |
| 9 | `CPUID` | read-only: ISA version and feature trits |
| 10 | `CYCLE` | read-only instruction counter (wraps) |
| −13..−1 | – | reserved for future virtual Setun-70 (V70) state (§9.4) |
| others | – | reserved: reads 0, writes trap |

## 3. Instruction encoding

Instructions are **1 or 2 words** (4 or 8 trytes) and may start at any tryte address. Word 1 always begins with a **2-tryte opcode field**: major tryte `A` = `w[1:6]`, function tryte `B` = `w[7:12]`.

**Length rule.** The first trit of the instruction, `A[1]`, gives the length:

| `A[1]` | Nonary first digit of A | Meaning |
|---|---|---|
| 0 | `Z` `0` `1` | 1-word instruction |
| + | `2` `3` `4` | 2-word instruction: word 2 is a 24-trit immediate |
| − | `W` `X` `Y` | reserved (a future "Setun-387" coprocessor). Traps as illegal in v1. |

**An all-zero word is `ILL`**, so running into zeroed memory traps at once.

### 3.1 Formats (word 1)

```
trit:     1 ..... 6   7 ..... 12   13-15  16-18  19-21  22-24
  R :   [    A    ] [     B     ] [ rd ] [ ra ] [ rb ] [ f3 ]
  I :   [    A    ] [     B     ] [ rd ] [ ra ] [   imm6    ]
  M :   [    A    ] [     B     ] [ rd ] [       imm9       ]
  B3:   [    A    ] [     B     ] [   offN6   ] [   offP6   ]
  L :   word 1 in R/I/M layout (short immediate ignored), word 2 = imm24
```

- Register fields are balanced 3-trit indices from −13 to 13.
- `imm6` is ±364, `imm9` ±9841, `imm24` a full word.
- In `M`-format branches the `rd` field holds a 3-trit condition.
- **Branch and `JAL` offsets are in trytes and relative to the address of the branch instruction itself.**

### 3.2 Opcode map (major tryte A)

| A | Mnemonic | Format | Operation |
|---|---|---|---|
| `000` | SYS group | by B | system and control (§3.6) |
| `001` | ALU | R | `rd := ra ⊕ rb` (§3.3) |
| `002` | ALUI | I | `rd := ra ⊕ imm6` |
| `003` | MEM | I | load/store at `ra + imm6` (§3.4) |
| `004` | `LI` | M | `rd := imm9` (B = `000`) |
| `01W` | `JAL` | M | `rd := PC+4; PC := PC + imm9` |
| `01X` | `JALR` | I | `rd := PC+4; PC := ra + imm6` |
| `01Y` | `BR` | M | if `cond(rd-field)` then `PC := PC + imm9` (§3.5) |
| `01Z` | `BR3` | B3 | **3-way**: S<0 → `PC += offN`; S>0 → `PC += offP`; S=0 → next instruction |
| `010` | IO group | M / I | port I/O (§3.7) |
| `011` | FLD group | I | trit-field extract/insert (§3.3) |
| `200` | ALUI.L | L | `rd := ra ⊕ imm24` |
| `201` | MEM.L | L | load/store at `ra + imm24` |
| `202` | `LI.L` | L | `rd := imm24` |
| `203` | `JAL.L` | L | `rd := PC+8; PC := PC + imm24` |
| `204` | `BR.L` | L | conditional branch, 24-trit offset |
| `205` | `BR3.L` | L | 3-way branch; word 2 = `offN[1:12]`, `offP[13:24]` |
| other | – | – | reserved → `ILL` trap |

Most of the 3^12 opcode space is unused on purpose. There is room for floating point, vector and graphics operations later.

### 3.3 ALU functions (B, for ALU / ALUI / ALUI.L)

**Every ALU operation sets `S := sign(result)`.** Arithmetic operations also set `C`.

| B | Mnemonic | Operation |
|---|---|---|
| `001` | `ADD` | `rd := ra + rb`, C = carry trit |
| `002` | `SUB` | `rd := ra − rb` |
| `003` | `ADC` | `rd := ra + rb + C` |
| `004` | `SBC` | `rd := ra − rb − C` |
| `01W` | `MUL` | `rd :=` low 24 trits of `ra·rb`; C ≠ 0 if the product overflowed |
| `01X` | `MULH` | `rd :=` high 24 trits of the 48-trit product |
| `01Y` | `DIV` | quotient **truncated toward zero** (C semantics) |
| `01Z` | `MOD` | remainder of `DIV` (sign of the dividend) |
| `010` | `DIVR` | quotient **rounded to nearest** (natural in balanced ternary) **⚠ REVIEW** |
| `011` | `MODR` | balanced remainder, `\|r\| ≤ \|rb\|/2` |
| `012` | `MIN` | tritwise minimum (ternary "AND") |
| `013` | `MAX` | tritwise maximum (ternary "OR") |
| `014` | `TMUL` | tritwise product (Setun-70 `SMT`) |
| `02W` | `TSAT` | tritwise threshold add, sign(a+b) (Setun-70 `SAT`) |
| `02X` | `CONS` | tritwise consensus: a if a = b, else 0 |
| `02Y` | `SH` | `rd := ra · 3^rb`: shifts left if rb > 0; if rb < 0, shifts right, dropping trits (rounds) |
| `020` | `CMP` | `S := sign(ra − rb)` computed **exactly** (never fooled by overflow); `rd` not written |
| `021` | `SGN` | `rd := sign(ra)` |
| `022` | `ABS` | `rd := \|ra\|` |
| `023` | `TLZ` | number of leading zero trits of `ra` (0..24), used for normalization |
| `024` | `TRIT` | `rd :=` trit number `rb` (1..24) of `ra` |
| `000`, negative | – | reserved → `ILL` |

Tritwise NOT is plain negation: `SUB rd, r0, ra`.

**FLD group** (A = `011`, I format, `imm6 = [pos−13 : 3 trits][len−13 : 3 trits]`):

| B | Mnemonic | Operation |
|---|---|---|
| `001` | `EXT` | `rd :=` field `ra[pos : pos+len−1]`, read as a balanced number |
| `002` | `INS` | `rd[pos : pos+len−1] :=` the low `len` trits of `ra` |

### 3.4 Memory functions (B, for MEM / MEM.L)

The address is `ra + imm`. `ST*` stores the low trytes of `rd`.

| B | Mnemonic | Operation |
|---|---|---|
| `001`/`002`/`003`/`004` | `LD1`/`LD2`/`LD3`/`LD4` | load 1/2/3/4 trytes; the value is preserved (no extension variants needed) |
| `01W`/`01X`/`01Y`/`01Z` | `ST1`/`ST2`/`ST3`/`ST4` | store the low 1/2/3/4 trytes of `rd` |
| `010` | `SWAP` | atomically exchange `rd` with the word at the address |

`LD3`/`ST3` (18 trits) handle page-table entries and Setun-70 words.

### 3.5 Branch conditions (the 3-trit `cond` in the rd field)

Each trit position belongs to one sign value: `cond[1]` ↔ −1, `cond[2]` ↔ 0, `cond[3]` ↔ +1.

- A **+** trit means "branch if **S** has that value".
- A **−** trit means "branch if **C** has that value".
- Positions that are + are OR-ed together, and so are positions that are −.

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
| `000` | – | never (a 1-word no-op) |

Combining + and − trits in one condition is reserved.

`BR3` is the true 3-way branch of the Setun and Setun-70. After `CMP a, b`, one instruction dispatches on less / equal / greater.

### 3.6 SYS group (A = `000`)

| B | Mnemonic | Ring | Operation |
|---|---|---|---|
| `000` | `ILL` | any | illegal instruction trap (vector 0) |
| `001` | `NOP` | any | no operation |
| `002` | `HLT` | +1 | wait for an interrupt |
| `003` | `BRK` | any | breakpoint trap (vector 6) |
| `004` | `SYS imm9` | any | system call: vector 27, with imm9 placed in `FCAUSE[1:12]` |
| `01W` | `IRET` | +1 | return from trap (§7.3) |
| `01X` | `MFC rd, cr` | +1 | `rd := cr[imm6]` |
| `01Y` | `MTC cr, ra` | +1 | `cr[imm6] := ra` |
| `01Z` | `TLBINV ra` | +1 | invalidate the translation for virtual address `ra` |
| `010` | `TLBFLUSH` | +1 | invalidate all translations |
| `011` | `FENCE` | any | order memory and I/O (a no-op in the v1 emulator) |

### 3.7 IO group (A = `010`)

All four operations require `RING ≥ MODE.IOPL`, otherwise vector 1 is raised. **Ports transfer whole words.**

| B | Mnemonic | Format | Operation |
|---|---|---|---|
| `001` | `IN rd, port` | M | `rd := port[imm9]` |
| `002` | `OUT rd, port` | M | `port[imm9] := rd` |
| `003` | `INR rd, ra, imm6` | I | `rd := port[ra + imm6]` |
| `004` | `OUTR rd, ra, imm6` | I | `port[ra + imm6] := rd` |

### 3.8 Pseudo-instructions (assembler)

| Pseudo | Expands to |
|---|---|
| `MOV rd, ra` | `ADD rd, ra, r0` |
| `NEG rd, ra` / `NOT rd, ra` | `SUB rd, r0, ra` |
| `LI rd, big` | `LI` or `LI.L`, chosen by size |
| `CALL f` | `JAL r12, f` |
| `RET` | `JALR r0, r12, 0` |
| `PUSH ra` | `ADDI r13, r13, -4` then `ST4 ra, [r13]` |
| `POP rd` | `LD4 rd, [r13]` then `ADDI r13, r13, 4` |

## 4. Physical memory map

Physical addresses are balanced integers. Memory is organized in **frames of 3^7 = 2187 trytes**. Frame `f` is **centered** on `2187·f` and covers `2187·f − 1093 … 2187·f + 1093`. (Balanced addressing makes every page centered, just as Setun-70 pages run from −40 to +40.)

| Frames | Physical addresses | Contents |
|---|---|---|
| `0 … N−1` | from −1 093 upward | **RAM**. The default is N = 6561 frames = 3^15 trytes (14 348 907), configurable. |
| **0** | −1 093 … +1 093 | **Setun-70 window**: in compat mode this *is* the Setun-70 memory (§9) |
| −9 … −1 | −20 776 … −1 094 | **BIOS ROM**, 3^9 trytes (§9.2) |
| −486 … −244 | −1 063 975 … −532 535 | **framebuffer**, 243 frames = 531 441 trytes (§8) |
| all other negative frames | – | reserved for memory-mapped devices; reading or writing them raises a bus error (vector 10) |

Rule of thumb: **non-negative frames are RAM, negative frames are devices.**

PTE frame numbers are 13 trits wide, so the physical address space is 20 trits (±1.74·10^9 trytes). RAM can grow to 797 161 frames.

## 5. MMU

### 5.1 Pages and virtual addresses

Pages are 3^7 = 2187 trytes and centered, like frames. There is a 2-level page walk. Each table is **exactly one page**: 729 entries of 3 trytes.

```
virtual address (24-trit word)
  trit:  1 .... 5   6 ...... 11   12 ..... 17   18 ....... 24
        [ 00000 ] [ L1 index  ] [  L2 index  ] [   offset   ]
                   6 trits,      6 trits,       7 trits,
                   −364..364     −364..364      −1093..1093
```

**⚠ REVIEW: canonical 19-trit virtual addresses.** Two levels of page-sized tables translate 6 + 6 + 7 = **19 trits**, which is 3^19 ≈ 1.16·10^9 trytes, or about 1.4 GB of binary storage. That is the same order as the 386's 4 GB.

With paging on, the top 5 trits of a virtual address must be 0. In balanced ternary, widening a 19-trit number just leaves the top trits 0, so no sign-extension rule is needed. A non-canonical address raises a protection fault (vector 3).

The full 3^24 space would need a third level, the analogue of the 386 → x86-64 path. The spare `A[1] = −` opcode space and `CPUID` feature trits leave room for that.

### 5.2 Page-table entry (3 trytes = 18 trits)

A **zero entry is "not present"**, so zeroed memory is a valid empty table.

| Trits | Field | Values |
|---|---|---|
| 1–13 | `FRAME` | physical frame number (balanced) |
| 14 | `V` | 0 not present, +1 read/write, −1 read-only |
| 15 | `X` | 0 no execute, +1 execute, −1 execute-only (not readable) |
| 16 | `RING` | lowest ring that may access the page: −1 user, 0 drivers, +1 kernel only |
| 17 | `AD` | set by hardware: 0 untouched, −1 accessed, +1 written (dirty) |
| 18 | `SW` | free for the OS (for example copy-on-write) |

Entry `i` (−364..364) of a table centered at `T` occupies trytes `T + 3i − 1 … T + 3i + 1`. The 729 entries fill the page exactly, the same trick the Setun-70 stack uses (`3·pa−1 … 3·pa+1`).

### 5.3 Walk

1. `L1 = PTB + 3·VA[6:11]`, where `PTB` is a frame center. If `V = 0`, raise a page fault.
2. `L2 = 2187·L1.FRAME + 3·VA[12:17]`. If `V = 0`, raise a page fault.
3. `PA = 2187·L2.FRAME + VA[18:24]`.

The permissions checked are the **more restrictive** of L1 and L2:

- write needs `V = +1` at both levels;
- instruction fetch needs `X ≠ 0`;
- a data read needs `X ≠ −1`;
- `MODE.RING ≥ RING` must hold at both levels.

Violations raise vector 3 (protection) or vector 2 (not present). `FADDR` receives the virtual address and `FCAUSE` the access type and ring.

The hardware updates `AD` in the L2 entry.

### 5.4 TLB

The TLB is not architecturally visible. After changing a PTE, software uses `TLBINV`; after writing `PTB`, `TLBFLUSH`. Writing `PTB` does **not** flush implicitly.

### 5.5 Accesses that cross a page

A multi-tryte access, or an instruction, that runs past the end of a page is translated one page at a time. If the second page faults, `FADDR` holds the address of the first tryte on the faulting page, and the instruction has no effect. This is the same as unaligned accesses on the 386.

## 6. Privilege rings

There are 3 rings on one trit: **−1 user, 0 drivers, +1 kernel**. Every check is a plain trit comparison, "current ring ≥ required ring".

| Resource | Requirement |
|---|---|
| `MFC`, `MTC`, `IRET`, `HLT`, `TLB*` | ring +1 |
| `IN`, `OUT`, `INR`, `OUTR` | `RING ≥ MODE.IOPL` (default 0: drivers and kernel) |
| a page | `RING ≥ PTE.RING` |

Only a trap or interrupt raises the ring. Only `IRET` lowers it.

## 7. Traps and interrupts

### 7.1 Vector table

`ITB` points to **243 (3^5) vectors** numbered 0..242. Each entry is one word holding a handler address, at `ITB + 4·v`. The table is 972 trytes and fits in one page. With paging on, `ITB` is a virtual address.

| Vector | Cause | `EPC` holds |
|---|---|---|
| 0 | illegal or reserved instruction | the instruction |
| 1 | privilege violation (ring or `IOPL`) | the instruction |
| 2 | page fault (not present) | the instruction |
| 3 | protection fault (permissions, non-canonical address) | the instruction |
| 4 | divide by zero | the instruction |
| 6 | breakpoint (`BRK`) | the next instruction |
| 7 | single step (`TF`) | the next instruction |
| 8 | double fault (a fault while fetching a vector) | – |
| 10 | bus error (no device at the physical address) | the instruction |
| 5, 9, 11–26 | reserved CPU exceptions | |
| **27** | **system call** (`SYS`) | the next instruction |
| 28–80 | reserved | |
| **81–161** | **hardware IRQ 0–80** from the interrupt controller | the next instruction |
| 162–242 | free for OS use | |

A fault while handling a double fault resets the machine (a "triple fault", as on the 386).

### 7.2 Entry

1. `EMODE := MODE`, `EPC :=` the address from the table above, and `FADDR`/`FCAUSE` are set.
2. If `MODE.RING < +1`: **swap `r13 ↔ KSP`** (switch to the kernel stack).
3. `MODE.RING := +1; IE := 0; TF := 0; CM := 0`.
4. `PC := word at ITB + 4·vector`.

**⚠ REVIEW**: every trap enters the kernel (ring +1). Driver-ring handlers are reached by the kernel dispatching to them in software. This is simpler than 386 gates.

### 7.3 `IRET`

1. `MODE := EMODE; PC := EPC`.
2. If the new `RING < +1`: swap `r13 ↔ KSP`.

The swap works in both directions: while a user program runs, `KSP` holds the kernel stack pointer, and while the kernel runs, it holds the user's.

A kernel must save `EPC` and `EMODE` before re-enabling interrupts if it wants nested traps.

## 8. Devices

### 8.1 I/O port map

There are 3^10 = 59 049 ports, numbered in balanced form −29 524..29 524. Each device gets a block of 81 ports. Every port holds a word. All standard devices sit inside the `imm9` range, so a single `IN`/`OUT` reaches them. Negative ports are reserved for expansion buses.

| Ports | Device | IRQ |
|---|---|---|
| 0–80 | system control (`SYSCTL`) | – |
| 81–161 | interrupt controller (`TPIC`) | – |
| 162–242 | interval timer | 0 |
| 243–323 | real-time clock | 5 |
| 324–404 | keyboard | 1 |
| 405–485 | mouse | 2 |
| 486–566 | serial debug console | 3 |
| 567–647 | disk 0 (648–728: disk 1, reserved) | 4 |
| 729–809 | display controller | 6 |
| 810+ | reserved | |

### 8.2 Registers (offsets from the block base)

Ternary control values are used throughout. For example, timer mode is −1 one-shot, 0 stopped, +1 periodic.

**SYSCTL (0)**

| Offset | Register |
|---|---|
| +0 | `SYSID` (read-only) |
| +1 | `RAM_FRAMES` (read-only) |
| +2 | `POWER`: write +1 to reset, −1 to power off |
| +3 | `DEBUG`: write a tryte and it appears in the host log |
| +4 | `BOOTINFO` (read-only): boot device, host flags |

**TPIC (81), interrupt controller.** There are 81 IRQ lines (0–80), and a lower number means higher priority.

| Offset | Register |
|---|---|
| +0 | `VECBASE`: vector of IRQ 0, default 81 |
| +1 | `CLAIM` (read): returns the highest-priority pending unmasked IRQ and marks it in service, or −1 if there is none |
| +2 | `EOI` (write): the IRQ number |
| +3 | `UNMASK` (write): the IRQ number |
| +4 | `MASK` (write): the IRQ number |
| +5 | `SELECT` |
| +6 | `STATE` of the selected IRQ: −1 masked, 0 idle, +1 pending |

All IRQs are masked at reset.

**Timer (162).** 1 MHz time base.

| Offset | Register |
|---|---|
| +0 | `CTRL`: −1 one-shot, 0 stop, +1 periodic |
| +1 | `PERIOD` (µs) |
| +2 | `COUNT` (µs remaining) |
| +3 | `UPTIME` (µs since reset; about 39 hours before it wraps) |

**RTC (243)**

| Offset | Register |
|---|---|
| +0 | `SECONDS`: Unix time. A 24-trit word holds ±1.4·10^11 seconds, so there is no 2038 problem. |
| +1 | `MICROS` |
| +2 | `ALARM`: Unix time, or 0 for off. Raises IRQ 5. |

**Keyboard (324)**

| Offset | Register |
|---|---|
| +0 | `DATA`: the next key event, **+code on press, −code on release**, 0 if the queue is empty. Codes are USB HID usage IDs (1..231), which fit in one tryte. |
| +1 | `COUNT`: number of queued events |
| +2 | `CTRL`: +1 IRQ on, 0 off, −1 flush |
| +3 | `LEDS` |

"PS/2-style" means a queued event device with an interrupt. It is not bit-compatible with PS/2.

**Mouse (405)**

| Offset | Register |
|---|---|
| +0 | `DX` (read clears) |
| +1 | `DY` (read clears) |
| +2 | `BUTTONS`: trit 24 left, 23 right, 22 middle; +1 = down |
| +3 | `WHEEL` |
| +4 | `CTRL` |
| +5 | `ABS_X` (absolute position, handy in the browser) |
| +6 | `ABS_Y` |

Balanced ternary represents deltas naturally.

**Serial console (486)**

| Offset | Register |
|---|---|
| +0 | `DATA`: one tryte character (§8.4) |
| +1 | `STATUS`: trit 24 = receive ready, trit 23 = transmit ready |
| +2 | `CTRL`: +1 IRQ on receive |

The host side is stdin/stdout in the native build and a console panel in the browser.

**Disk 0 (567).** A sector is **one page, 2187 trytes**. DMA uses **physical** addresses and bypasses the MMU.

| Offset | Register |
|---|---|
| +0 | `CMD`: writing starts the operation: −1 read, +1 write, 0 flush |
| +1 | `STATUS`: −1 error, 0 idle, +1 busy |
| +2 | `LBA` |
| +3 | `COUNT` (sectors) |
| +4 | `DMA` (physical address) |
| +5 | `SECTORS` (read-only capacity) |
| +6 | `ERROR` |
| +7 | `CTRL`: +1 IRQ on completion |

**Display (729)**

| Offset | Register |
|---|---|
| +0 | `MODE`: **−1 text 80×30**, 0 = 640×480, +1 = 800×600 |
| +1 | `FB_BASE` (read-only physical address) |
| +2 | `WIDTH` |
| +3 | `HEIGHT` |
| +4 | `PITCH` (trytes per line) |
| +5 | `FRAME` (vsync counter) |
| +6 | `CTRL`: +1 IRQ on vsync |
| +7 | `CURSOR` (text cell index) |
| +8 | `HWC_X` (hardware mouse cursor) |
| +9 | `HWC_Y` |
| +10 | `HWC_ON` |

### 8.3 Framebuffer formats

- **Graphics mode: one tryte per pixel, 729 colours.** The three nonary digits of the pixel are **R, G, B**. Each digit −4..+4 gives an intensity of `(d+4)/8`, stored as `round(255·(d+4)/8)` in sRGB. So a pixel written in nonary reads directly as its colour:
  - `444` white
  - `WWW` black
  - `4WW` red
  - `W4W` green
  - `WW4` blue
  - `000` 50% grey. Note that a zeroed framebuffer shows mid-grey.
- Pixel `(x, y)` is at `FB_BASE + y·PITCH + x`. The top-left pixel is `(0, 0)`.
- **Text mode (**⚠ REVIEW**, added for the BIOS and early kernel): 80×30 cells of 2 trytes**, character then attribute, rendered with an 8×16 font to 640×480.
  - The attribute tryte is **foreground = trits 1–3, background = trits 4–6**.
  - Each trit is one of R, G, B at 0%, 50% or 100%, giving **27 colours from one trit per channel**.

### 8.4 Tryte character set (TCS-1) **⚠ REVIEW**

| Codes | Contents |
|---|---|
| 0 | NUL |
| 1–127 | ASCII |
| 128–255 | Latin-1 |
| 256–364 | box-drawing and UI glyphs |
| −1 … −66 | Cyrillic (А–Я, Ё, а–я, ё) |
| other negatives | reserved |

Full Unicode can come later as a variable-length "UTF-T".

### 8.5 Host file format for tryte data (T3B4)

Disk images, ROM images and memory dumps are stored as **3 trytes per 4 bytes**. Each 18-trit group `v` is written as a little-endian `u32` of `v + 193 710 244`. That fits because 3^18 < 2^32, and it wastes only 10.8% (storing one tryte per `i16` would waste 40%).

One sector (2187 trytes) is 2916 bytes.

## 9. Setun-70 compatibility mode

### 9.1 The perfect fit

The Setun-70's first-level memory is 27 pages × 81 trytes = **2187 = 3^7 trytes, exactly one Setun-386 frame**.

In compat mode, Setun-70 address (page `p`, offset `a`) is **physical `81·p + a`**, which falls in −1093..+1093 = **frame 0**. No base register or translation is needed.

- Setun-70 RAM pages −4..4 are physical −364..+364.
- Its "ROM" pages are ordinary RAM in frame 0, protected by the Setun-70 ISA itself: `W=S` and `COPYF` refuse pages with `|p| > 4`.

### 9.2 Reset and boot flow

1. **Reset**: the hardware copies ROM frame −1 into frame 0 and enters compat mode (`MODE.CM = +1`). The Setun-70 registers take their 1970 reset values (`c1 = +1, ch = 12, ca = −40`), so execution starts at physical 12·81 − 40 = **932**. The display is in text mode.
2. **Compat BIOS stage** (Setun-70 code): banner on the console channel, a check of the Setun-70 memory, then the mode switch.
3. **Switch to native**: every Setun-70 operation code is already taken, so the switch is a **Setun-70 I/O device**, the same way the 386's A20 line hung off the keyboard controller.
   - Select device +4 on character channel +1 for output (`LU3`).
   - Write the tryte `++++++` (`LG3`).
   - The CPU leaves compat mode with `RING = +1`, `PG = 0`, `IE = 0` and `PC =` **physical −20 776** (the first tryte of ROM frame −9). **⚠ REVIEW**
4. **Native BIOS** (ROM frames −9..−2):
   - set up `ITB`;
   - read the RAM size from `SYSCTL`;
   - clear the text screen and run a self-test;
   - load **disk sector 0 into frame 1**, and jump to its center with `r1` = boot disk and `r2` = RAM frame count (the analogue of the PC's 0x7C00 boot sector).

### 9.3 Compat devices

The Setun-70 channel model maps onto native devices:

| Setun-70 | Native device |
|---|---|
| character channel −1 | serial console (TCS-1 ↔ the Setun-70 7-bit code) |
| character channel 0 | keyboard (in) and text screen (out) |
| channel +1, device +4 | `SYSCTL` mode switch |
| drum channel −1 | disk 0: drum page `q` = trytes `81·q …` of the disk image, so a compat program can load from disk |
| timer interrupt `v[1]` | interval timer |

### 9.4 Later: virtual Setun-70 (V70)

This is the analogue of the 386's virtual-8086 mode. A native kernel `IRET`s into a task with `EMODE.CM = +1`, and that task's virtual page 0 becomes its Setun-70 memory. Setun-70 special operations and macro-operations trap to the native kernel for emulation. Control registers `cr-13..cr-1` expose the compat state. This is not in v1.

## 10. Notes for the compiler

| Type | Size |
|---|---|
| `char` | 1 tryte |
| `short` | 2 trytes |
| `int`, `long`, pointers | 1 word (4 trytes) |

- `sizeof` counts trytes.
- There is no `unsigned`.
- Integer division truncates (`DIV`/`MOD`).
- Tritwise operators map to `MIN`/`MAX`/`TMUL`/`TSAT`/`CONS`.
- Comparisons are `CMP` followed by `BR`/`BR3`. `switch` on sign compiles to a single `BR3`.

## 11. Example

Print "Hi" on the serial console, then halt:

```
        LI    r1, 'H'
wait1:  IN    r2, 487          ; serial STATUS
        TRIT  r2, r2, 23       ; tx-ready trit (ALUI form: rb = imm)
        BR    BLE, wait1       ; loop while tx-ready ≤ 0 (the TRIT result set S)
        OUT   r1, 486          ; serial DATA
        LI    r1, 'i'
wait2:  IN    r2, 487
        TRIT  r2, r2, 23
        BR    BLE, wait2
        OUT   r1, 486
        HLT
```

## 12. Summary of review points

1. **No alignment requirement**, and accesses may cross pages (§1, §5.5), because a 3^7-tryte page cannot hold a whole number of 4-tryte words. The alternative is an 18-trit (3-tryte) word, which fits pages exactly but gives a smaller address space.
2. **PTEs are 3 trytes**, so each table is exactly one page of 729 entries (§5.2).
3. **2-level paging gives a 19-trit canonical virtual address** (≈1.16G trytes). The full 3^24 space needs a 3rd level later (§5.1). The physical address space is 20 trits.
4. A **carry trit `C`** is added next to `S` (§2.2).
5. Both **truncating and rounding** division are provided (§3.3).
6. The **Setun-70 memory is physical frame 0**, and the **mode switch is a Setun-70 I/O device** (§9).
7. Every trap enters **ring +1** (§7.2).
8. **Text mode** is added to the display (§8.3).
9. The **TCS-1 character set** and the **T3B4 file format** are new proposals (§8.4, §8.5).
10. **Ports transfer words**, not trytes (§3.7).
