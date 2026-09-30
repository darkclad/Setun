# Roadmap

Target: **Setun-386**, a balanced-ternary machine sized like a 386, where every
size is a power of 3: 9-trit trytes, 27-trit words, a 3^27 address space,
9|9|9 paging, 3 rings, 3^18 default RAM, and a 640×480 framebuffer with 3^9
colours. Then come a compiler, a kernel and a window system, all written for
the machine itself.

ISA: [`SETUN386_ISA.md`](SETUN386_ISA.md) (v0.4, accepted).

## Milestones

1. **`ternary` crate**: `Tern<N>`, 9-trit `Tryte`, 27-trit `Word`, trit fields, base-27 notation
2. **ISA spec**: `SETUN386_ISA.md` ✅ v0.4
3. **Emulator core** (`setun-core`, Rust)
   - CPU: decode, ALU, branches, traps, rings, `MODE`/control registers
   - MMU: 2-level 9|9|9 walk, TLB, faults, instruction fetch across pages
   - devices: SYSCTL, TPIC, timer, RTC, keyboard, mouse, serial, disk (T1B2 image, DMA), display (text + 3^9-colour framebuffer)
   - tests: per-instruction tests and MMU walk tests
4. **Front ends**: native (window + framebuffer + serial on stdio) and **WASM** (canvas framebuffer, keyboard/mouse events, console panel)
5. **Assembler and disassembler**
6. **Compiler**: a small C-like language → Setun-386 assembly (`char` = tryte = a Unicode character, `int` = pointer = word, no `unsigned`)
7. **BIOS**: vectors, RAM size, text console, disk boot sector
8. **Kernel**: paging, processes and a scheduler, syscalls, drivers in ring 0, a disk file system
9. **Window system**: framebuffer compositor, mouse cursor, windows, a terminal app
10. Later: a "Setun-387" floating-point unit, and a Setun-70 emulator as a user program
