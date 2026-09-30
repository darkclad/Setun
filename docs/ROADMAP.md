# Roadmap

Target: a Setun-70 emulator in Rust (native + WASM front panel), then a ROM
BIOS, then an OS, all written for the ternary machine itself.

## Machine profiles

The core implements the 1970 ISA exactly (**`classic`** profile). Our own
extensions live behind a **`setun70x`** profile so faithful programs keep
running unchanged. The candidate extensions below are to be pinned down one by one:

| Extension | Why | Faithful to the design? |
|---|---|---|
| Wider page registers (`h`, `ph`, `ch` 3 → 6 trits: 729 pages ≈ 59k trytes) | room for an OS + user programs | yes: the 1971 paper says memory grows by lengthening the page registers |
| Drum/disk channels backed by host image files | persistence, a file system | yes: same `LQ/CF/LF` protocol |
| Host-terminal console on channel 1 (UTF-8 ↔ tryte charset) | usable shell | device-level only |
| 1975 return stack | structured programming, DSSP-style OS | historical revision |
| Debug trap / trace hooks | tooling | host-only, invisible to programs |

## Milestones

1. **`ternary` crate**: trits, trytes, words, fields, nonary notation ✅
2. **`setun-core`**: CPU from the ALGOL spec (fetch/decode, B1–B27, S1–S27, macro-ops, interrupts, modes), memory with a RAM/ROM split, drum and I/O channels as host-side devices. Tests: one per opcode against the ALGOL semantics.
3. **`setun-asm`**: POLIZ assembler and disassembler (labels, page directives, nonary and trit literals, macro-op names).
4. **`setun-cli`**: run images, single-step, trace, dump pages; front panel in the terminal (TUI).
5. **ROM BIOS** (ROM pages): interrupt vectors (page 13), stack page extension on overflow and underflow, program page chaining (`v[9]`), console driver, drum boot loader, machine-code monitor.
6. **OS**: macro-op syscall table (page −13), drum paging, a small drum file system, timer-driven tasks, and a DSSP/Forth-style interactive shell.
7. **`setun-web`**: WASM build with a browser front panel (R-register lamps, stack view, typewriter console).
