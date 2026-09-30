# Roadmap

Target: **Setun-386**, a balanced-ternary machine sized like a 386 (24-trit
words, paging, 3 rings, 640×480 framebuffer). It boots in **Setun-70
compatibility mode** and the BIOS switches it to native mode. Then come a
compiler, a kernel and a window system, all written for the machine itself.

- ISA: [`SETUN386_ISA.md`](SETUN386_ISA.md) (draft, under review)
- Compatibility mode: [`ARCHITECTURE.md`](ARCHITECTURE.md), the Setun-70 as specified in 1970

## Milestones

1. **`ternary` crate**: trits, trytes, fields, nonary notation ✅
2. **ISA spec**: `SETUN386_ISA.md` 🟡 draft, waiting for design review
3. **Emulator core** (`setun-core`, Rust)
   - native CPU: decode, ALU, branches, traps, rings, `MODE`/control registers
   - MMU: 2-level walk, TLB, faults, accesses that cross pages
   - Setun-70 compat CPU (the 1970 ALGOL semantics) and the mode switch
   - devices: SYSCTL, TPIC, timer, RTC, keyboard, mouse, serial, disk (T3B4 image, DMA), display (text + 729-colour framebuffer)
   - tests: per-instruction tests, MMU walk tests, the Setun-70 spec cases
4. **Front ends**: native (window + framebuffer + serial on stdio) and **WASM** (canvas framebuffer, keyboard/mouse events, console panel)
5. **Assemblers**: Setun-386 native assembler/disassembler, plus a Setun-70 assembler for the compat BIOS stage
6. **Compiler**: a small C-like language → Setun-386 assembly (`char` = tryte, `int` = word, no `unsigned`)
7. **BIOS**: compat stage in Setun-70 code → mode switch → native BIOS (vectors, RAM size, text console, disk boot sector)
8. **Kernel**: paging, processes and a scheduler, syscalls, drivers in ring 0, a disk file system
9. **Window system**: framebuffer compositor, mouse cursor, windows, a terminal app
10. Later: virtual Setun-70 (V70) tasks, a 3rd page-table level (full 3^24 virtual addresses), a "Setun-387" floating-point unit
