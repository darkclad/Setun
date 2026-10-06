# Ternary World Roadmap

As of 2026-10-06, with the decisions of that day. Copied from the shared doc. The milestone plan for Setun-11̅ itself is in [`ROADMAP.md`](ROADMAP.md).

## Purpose and scope

Inside the ternary world everything is ternary-native, from the transistor to the file format; binary exists only at one border, behind adapters. This roadmap lists every layer that has to be redesigned, what the ternary version looks like, and where Setun-11̅ already stands.

Guiding principles:

1. **Balanced ternary everywhere.** Digits are −1, 0, +1. Ranges are symmetric, sign is free, rounding is truncation.
2. **Powers of 3 for every size.** Trit, tryte (9 trits), word (27 trits), page, block, address space.
3. **Use the third state, don't waste it.** A value with two states usually hides a third (unknown, pending, contended). If it truly has two, pack several into trits.
4. **Binary only at the border.** One adapter layer converts. Nothing inside depends on a binary format or a power of 2.
5. **Bottom-up, tested.** A layer is specified before the layer above relies on it, and every milestone ships with negative tests.

The sections run bottom-up. Each has a lead, a table of components (the ternary-native design, its gain over binary, Setun-11̅ status), and open questions. The physical layer is out of reach for an emulator project, so it is tracked as a requirements list, not a build plan.

## Map of major points

Setun-11̅ is ternary from the ISA up; silicon and native links remain. Eight layers stack from the number system up to software. Tooling (§10) checks all of them, and one border layer (§9) is the only place binary is allowed in.

| § | Layer | State |
|---|---|---|
| 8 | Software stack | Core done: Trinux and self-hosting done; three-way idioms next |
| 7 | Security and cryptography | Binary side done: border crypto done; lattice and GF(3) designs ahead |
| 6 | Interconnect, I/O and networking | Binary side done: binary network stack done; native ternary links ahead |
| 5 | Data formats | Binary side done: text and executables done; float standard and codecs ahead |
| 4 | Memory and storage | Core done: addressing, paging and tfs done; ECC and caches ahead |
| 3 | CPU and ISA | Core done: core ISA and FPU done; trit masks, atomics, vectors ahead |
| 2 | Physical layer and circuits | Research only: requirements only; the emulator is the machine |
| 1 | Number and logic foundations | Core done: balanced arithmetic and Kleene logic done; trit type ahead |
| 9 | Binary border (beside all layers) | One layer of adapters: octets, text, integers, floats, hashes, gateway |
| 10 | Tooling and verification (under all layers) | Reference interpreter, fuzzing, conformance |

## 1. Number and logic foundations

The arithmetic is done: Setun-11̅ has balanced integers, tritwise logic and native rounding (ISA v0.6). What is missing is a three-valued logic that programmers and tools use as fluently as Boolean algebra.

| Component | Ternary-native design | Gain over binary | Status |
| --- | --- | --- | --- |
| Digits and notation | −1/0/+1; overlined digits; `0t` base-3 and `0z` base-27 literals | Sign and digit in one notation; negation toggles the bars | Done |
| Integers | Tryte ±9,841, word ±3.8×10¹²; no unsigned types | Symmetric ranges, no sign extension, one compare instruction | Done |
| Division | `DIVR` rounds to nearest; `MODR` remainder with \|r\| ≤ \|b\|/2 | Rounding falls out of the arithmetic | Done |
| Logic primitives | `MIN` (and), `MAX` (or), `NEG` (not), `TMUL`, `TSAT`, `CONS` (consensus) | Kleene logic in one instruction; consensus has no binary equivalent | Done |
| Truth type in languages | A `trit` type: false / unknown / true, with Kleene `and`, `or`, `not`. A plain `if` on a trit is refused; a three-arm form (true / unknown / false) compiles to one `BR3` | NULL, `Option` and "not known yet" without a separate flag | Planned |
| Logic algebra and minimization | Canonical forms and minimizers (ternary Karnaugh maps, Quine–McCluskey) for 3-valued functions | Compilers and chip design need them; the binary versions don't apply | Research |
| Packing two-state data | A tlibc flag-set module first: 3 flags in 2 trits, 19 flags in 12 trits (99.9% efficient) | Recovers the 0.415 bit lost per flag stored alone | Planned |

Decided 2026-10-06:

- Trine gets a `trit` type with Kleene `and`, `or`, `not`. An `if` on a trit does not compile; the program must say what unknown does, in a three-arm form.
- Packed flags start as a tlibc library. They become a language feature only if they see heavy use.

## 2. Physical layer and circuits

This layer is a requirements list: the emulator is the reference machine, and any hardware must match it state for state. The premise is a ternary switch as fast and frugal as a binary transistor; everything here assumes it.

| Component | Ternary-native design | Gain over binary | Status |
| --- | --- | --- | --- |
| Switching device | Three stable states per device, for example multi-threshold carbon-nanotube FETs | The premise of the whole stack | Research |
| Signal levels | Balanced −V / 0 / +V, with 0 at ground | DC-balanced wires; the cost is half the noise margin at the same swing | Research |
| Gate library | Standard cells for `NEG`, `MIN`, `MAX`, `CONS`, `TSAT`, a 3-way multiplexer, half and full adders | One cell per ISA logic operation | Research |
| Adders | Balanced full adder: sum and carry each in {−1, 0, +1}; ternary carry-lookahead | Carry chains about 37% shorter for the same range | Research |
| Multipliers | Partial products are only 0, +x or −x | No Booth recoding; 27 partial products instead of 43 | Research |
| Storage cells | Ternary latch, flip-flop and SRAM cell with three stable states | Registers and caches hold 1.585 bits per cell | Research |
| Clock and static power | A two-phase clock is fine; the middle level must not draw static current | If the 0 state leaks, ternary loses its power case | Research |

Decided 2026-10-06:

- No device family is picked. The roadmap watches CNTFET, multi-threshold CMOS and others, and the emulator stays the reference until one is real.
- No FPGA prototype: the emulator is the only machine. Hardware work is ongoing research, not a phase.

## 3. CPU architecture and ISA

The Setun-11̅ core is ternary-native already: one sign trit replaces the zero and sign flags, and every branch can go three ways. The gaps are the instructions that make the third state pay off in everyday code: masked writes, three-state atomics and vectors.

| Component | Ternary-native design | Gain over binary | Status |
| --- | --- | --- | --- |
| Status and branches | `S` = sign of the last result; `BR3` jumps on <, = or > | One compare drives a three-way branch; no Z/N/V flag zoo | Done |
| Carry | Carry trit `C`, non-zero exactly on overflow; `ADC`/`SBC` | Overflow detection and multi-word arithmetic from one trit | Done |
| Privilege | 3 rings on one trit: user, drivers, kernel | Driver ring for free, without a binary 4-ring waste | Done |
| Floating point | real27 and real81 sharing a 9-trit exponent; no sign trit, no −0; truncation rounds to nearest | No guard/sticky bits; reading the first word of a real81 gives the rounded real27 | Done |
| Trit-mask merge | An ALU merge of a source into a register under a mask (0 keeps, +1 takes the source, −1 clears), plus a masked word store for device registers and shared flags | Sets, clears and keeps fields in one write; binary needs AND then OR | Planned |
| Three-state atomics | Lock word free / held / contended; compare-and-swap that reports <, = or > | Futex-style locks without a second waiter flag | Planned |
| Vectors | 27 new 81-trit vector registers, saved lazily like the FPU, as 9 tryte lanes or 3 word lanes, with `MIN`, `MAX`, `TMUL` per lane | Text, pixels and small integers in parallel | Planned |
| Ternary-weight dot product | `TMUL` + add over lanes for weights in {−1, 0, +1} | Ternary neural networks (BitNet b1.58 style) run with no multiplier at all | Planned |

Decided 2026-10-06:

- The trit-mask merge comes in two forms: a register merge under the ALU major, and a masked word store that writes memory directly.
- Vectors get 27 new 81-trit registers, switched lazily like the FPU registers.

## 4. Memory and storage

Addressing and paging are done and are powers of 3 throughout. Still to design: error correction, caches, and the persistent storage cell. Ternary is stronger than binary at error correction, so ECC is the first item to add.

| Component | Ternary-native design | Gain over binary | Status |
| --- | --- | --- | --- |
| Address space | 3^27 trytes, addressed by the centre; aligned means the address is a multiple of the size | No canonical-form rule; alignment is one divisibility test | Done |
| Paging | 9 \| 9 \| 9 split, 3^9-tryte pages, one-word page-table entries with a 3-valued `RING` field | Each level indexes by one tryte, with no shifting or masking | Done |
| Physical allocator | "triddy": a buddy allocator that splits blocks in 3 | Free blocks stay powers of 3 | Done |
| Memory ECC | Shortened ternary Hamming code [31,27]: one 27-trit word + 4 check trits corrects any 1 trit error; perfect ternary Golay [11,6,5] where 2-error correction is needed. Specified for hardware only | 15% overhead for single correction (binary SECDED on 64 bits: 12.5%); Golay has no binary equal | Planned |
| Caches | Lines of 27 or 81 trytes; 3-way or 9-way sets; set counts a power of 3, indexed by address trits | The index is a trit field, never a modulo | Planned |
| Disk layout | Sector = 1 block = 3^9 trytes; `tfs` with 81-tryte directory entries, 243 per block | The file system needs no binary sizes | Done |
| Storage cell | A 3-level flash or resistive cell holding exactly one trit | Today's multi-level flash already stores levels; 3 levels give the widest margin per trit | Research |

Decided 2026-10-06:

- ECC is written down as a specification for future hardware. The emulator stays error-free and does not model check trits.
- `tfs` gets no checksums for now.

## 5. Data formats

Formats are where a ternary world either stays ternary or quietly turns into binary carried in trits. The rule: every format counts in trytes and stores signed values in balanced form, never in an offset or a zigzag encoding.

| Format | Ternary-native design | Gain over binary | Status |
| --- | --- | --- | --- |
| Text | One Unicode code point per tryte; an overlined character is the negated code point | Fixed width, no UTF-8 decoding, overline for free | Done |
| Text beyond U+2671 | A lead tryte in the unused −1…−32 range followed by 2 trytes (3^18 values) covers all of Unicode; built now in tlibc, the UTF-8 border and the font path | CJK and emoji without giving up one tryte per common character | Planned |
| Floating point standard | real27 / real81 written up as a standalone standard: formats, exceptions, text conversion, a test suite | A ternary IEEE 754 that other ternary machines can adopt | Partial |
| Images | 27 levels per channel, 3^9 colours per tryte; a lossless file format whose prediction residuals are stored balanced | Residuals are signed by nature, so balanced storage needs no offset | Planned |
| Audio | Balanced samples of one word (3 trytes, about 42.8 bits). A tryte (14.3 bits) falls short of 16-bit audio, and 2 trytes is not a power of 3 | Silence is 0 with no DC offset; the range is symmetric | Planned |
| Compression and serialization | Entropy coding in base 3; varints in balanced base 27; type tags in trits | Signed varints need no zigzag trick | Planned |
| Executables | TEF, the Trinux executable format | Already tryte-native | Done |

Decided 2026-10-06:

- Code points beyond U+2671 (CJK, emoji) are implemented now: 3 trytes per character, a lead tryte in −1…−32 and 2 trytes of code point.
- A timestamp is one word of seconds around the 1970 epoch (±120,000 years, no 2038 problem). Sub-second time is a `long` (81 trits): seconds in the top word, a base-3 fraction of a second in the lower 54 trits.

## 6. Interconnect, I/O and networking

Today every Setun-11̅ network packet is binary: `TNIC` carries one octet per tryte to a binary Ethernet. A native stack only pays off once two ternary machines talk, so its first milestone is two emulators linked trit to trit.

| Component | Ternary-native design | Gain over binary | Status |
| --- | --- | --- | --- |
| Port I/O | A port number is one tryte; a port transfers a word | Already native | Done |
| On-chip buses | 27 lines per word instead of 43 | About 37% fewer wires, where most delay and power go | Research |
| Chip-to-chip links | Balanced 3-level signalling, one trit per symbol | Industry PAM3 (GDDR7, 100BASE-T1) maps 3 bits onto 2 symbols; native trits need no mapping | Research |
| Peripheral bus | A ternary serial bus with balanced line coding; the 0 symbol doubles as idle and frame marker | DC-free by construction; framing without bit stuffing | Planned |
| Link layer | Ternary frames, 27-trit station addresses, ternary checksums | No octet packing inside the ternary network | Planned |
| Network and transport | A native protocol: 27-trit or 81-trit host addresses, word-sized sequence numbers and windows | 3^27 ≈ 7.6×10¹² addresses in one word, about 1,800 times as many as IPv4 | Planned |
| Binary networking | IPv4, TCP, UDP, DNS, SSH, TLS 1.3 and HTTP through the `setun-net` gateway | Reaches the binary internet today | Done |

Decided 2026-10-06:

- The native stack keeps BSD sockets, with a new ternary address family, so `ssh`, `wget` and `httpd` run over both stacks.
- Native networking comes before IPv6. IPv6 is added only if the binary side needs it.

## 7. Security and cryptography

Cryptography is the one layer where "design our own" is dangerous: a cipher is trusted only after years of public attack. So the plan has two tracks: binary standard crypto stays for the border, and the ternary-native track starts with lattice schemes that are already standardized and happen to be ternary-friendly.

| Component | Ternary-native design | Gain over binary | Status |
| --- | --- | --- | --- |
| Border crypto | SHA-2, ChaCha20-Poly1305, AES-GCM, X25519, Ed25519, ECDSA, RSA, on the ISA v0.6 binary operations | Talks to the real internet today | Done |
| Random numbers | `TRNG` device and `/dev/urandom`; converting bits to trits by rejection (8 bits below 243 give 5 trits) | Unbiased trits, which naive bits mod 3 is not | Partial |
| Post-quantum public key | Lattice schemes whose secrets are small balanced numbers: NTRU uses exactly −1, 0, +1; ML-KEM and ML-DSA use small coefficients around 0. ML-KEM comes first; NTRU follows | Standard algorithms, and the native number format fits them | Planned |
| Symmetric cipher | A design over trits: add mod 3^27, trit rotations, `TMUL` and consensus as the non-linear layer. Research only, never for real secrets | No binary operations inside the ternary world | Research |
| Hash | A sponge over trits | IOTA's ternary hash Curl-P was broken in 2017, a warning that a new design needs public review; research only | Research |

Decided 2026-10-06:

- A ternary cipher and hash are research toys, built and published for study. Real secrets always use standard algorithms.
- ML-KEM (FIPS 203) is the first native public-key target, because it also works with binary peers.

## 8. Software stack

The system software exists and builds itself: Trinux, tlibc, Trine and the `tas`/`tric` toolchain. The next step is to make programs think in trits, not just run on them: three-way control flow in the language, trit operations in place of bit tricks, and data structures that branch by 3.

| Component | Ternary-native design | Gain over binary | Status |
| --- | --- | --- | --- |
| Operating system | Trinux: POSIX interfaces, a tryte is the "byte" (`CHAR_BIT` = 9), text in code points | Unix programs port with few changes | Done |
| Language and compiler | Trine; `tric` compiles itself to a fixed point | The ecosystem grows from inside the machine | Done |
| Three-way control flow | `match` on a comparison with `<`, `=`, `>` arms, compiled to one `BR3` | The C convention (<0, 0, >0) becomes a first-class branch | Planned |
| Trit operations in place of bit tricks | Count of + and − trits, leading-zero trits, trit reversal, packed flag sets | Replaces popcount, clz and bitmask idioms | Planned |
| Data structures | 3-ary heaps; B-trees with 27 or 81 keys per node; hash tables of 3^k buckets indexed by trits; ternary search tries | Wider fan-out means shallower trees | Planned |
| Algorithms | Radix sort in base 27; radix-3 FFT and number-theoretic transforms | Natural digit size, no conversions | Planned |
| Optimizer in `tric` | Port the host optimizer (register locals, peephole) into the self-hosted compiler | Optimized builds made on the machine itself | Planned |

Decided 2026-10-06:

- The library keeps binary search for sorted arrays. Splitting by 3 costs about 1.26× the compares, so ternary fan-out is used only in wide nodes (B-trees, tries).

## 9. The binary border

The border is one thin layer of adapters, and nothing above it knows binary exists. Each adapter has one conversion rule, and data crossing it is either exact or flagged as rounded.

| Crossing | Rule | Exact? | Status |
| --- | --- | --- | --- |
| Binary operations | `BAND`, `BOR`, `BXOR`, `BANDN`, `BSH`, `BEXT`, `BROT` for data that comes from binary systems | Yes | Done |
| Octets | One octet per tryte on the wire (`TNIC`) | Yes | Done |
| Text | Code points ↔ UTF-8 at the border; an overlined character ↔ the character + U+0305 | Yes | Done |
| Hashes and signatures | Computed over the binary form (text hashed as UTF-8), so digests match other systems | Yes | Done |
| Integers | Two's complement ↔ balanced. A word holds any 32-bit integer; a `long` (3 words, 81 trits) holds any 64-bit one, and even any 128-bit one | Yes | Partial |
| Floating point | IEEE 754 ↔ real27/real81. Binary fractions are not finite in base 3 (1/2 = 0.111… in ternary), so this direction always rounds | No: keep IEEE values as bit patterns when exactness matters | Planned |
| Gateway | `setun-net`: the emulator's user-mode NAT to host sockets, DNS and the internet | Yes | Done |

Decided 2026-10-06:

- A 64-bit integer from binary lands in a `long`: 3 words, 81 trits, already in ISA §10 and planned for Trine. Power-of-3 sizes go tryte, word, long; there is no two-word type.

## 10. Tooling and verification

The emulator's reference interpreter is the definition of the machine, and every faster path must match it state for state. What is missing is conformance suites that let a second implementation (an FPGA, another emulator) prove it is the same machine.

| Component | Ternary-native design | Gain | Status |
| --- | --- | --- | --- |
| Reference and fast paths | Golden state hashes; a fast-vs-reference fuzz test; directed tests, each shown to catch its bug | JIT and optimizer cannot drift from the spec | Done |
| Negative tests | Every milestone ships tests of what must be refused: malformed literals, broken packets, bad syscalls | Bugs at the edges surface before users find them | Done |
| Conformance suites | Separate suites for the ISA, the real27/real81 standard and each native protocol | A second implementation can be checked without reading the emulator | Planned |
| Fault injection | Flip trits on links and in packets to test checksums; memory faults wait for hardware ECC | Proves the error handling actually works | Planned |
| Ternary EDA | A hardware description language with a balanced trit type; a gate-level simulator for ternary cells | Needed before any ternary silicon or FPGA work | Research |

## Phased roadmap

Ternary idioms come first; hardware stays research. Phases run in order, and each gate must pass before the next phase starts.

| Phase | Sections | Work | Gate to pass |
|---|---|---|---|
| Done (October 2026) | – | Balanced arithmetic, ISA v0.6, emulator with JIT, BIOS; Trinux, binary network stack to TLS 1.3, self-hosting S1–S3 | – |
| **A · Ternary idioms** (next) | 1, 3, 5, 8 | `trit` type and three-arm `if` in Trine; trit-mask merge and masked store; trit counts, packed flags, 3-ary heaps, wide B-trees; `tric` optimizer; vector registers, three-state atomics; text past U+2671 in 3 trytes | Programs branch with `BR3` and write with masks; negative tests pass |
| B · Formats, reliability | 4, 5, 9, 10 | real27/real81 standard with a conformance suite; ECC as a hardware spec; balanced varints, word audio, image formats; `long` and timestamps in Trine | Conformance suites published for the ISA, floats and formats |
| C · Native networking | 6, 7 | Two emulators linked trit to trit; ternary link layer and protocol; BSD sockets over both stacks; ML-KEM first, then NTRU | `ssh` between two Setun machines over the native stack |
| Ongoing research (alongside every phase) | 1, 2, 7 | Ternary cipher and hash (study only), logic minimization; devices (CNTFET, multi-threshold CMOS), ternary HDL, gate simulator | – |

Phase A comes first because it is software only and makes every later layer think in trits. There is no hardware phase: the emulator is the only machine, and devices, HDL and circuits stay ongoing research (decided 2026-10-06).
