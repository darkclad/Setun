# Roadmap

Target: **Setun-386**, a balanced-ternary machine sized like a 386, where every
size is a power of 3: 9-trit trytes, 27-trit words, a 3^27 address space,
9|9|9 paging, 3 rings, 3^18 default RAM, and a 640×480 framebuffer with 3^9
colours. Then come a compiler, a kernel and a window system, all written for
the machine itself.

ISA: [`SETUN386_ISA.md`](SETUN386_ISA.md) (v0.4 accepted; v0.5 draft adds the Setun-387 FPU).

## Milestones

1. **`ternary` crate** ✅: `Tern<N>`, 9-trit `Tryte`, 27-trit `Word`, trit fields, base-27 notation; the `0t`/`0z`/`0x` literal notation ([`NOTATION.md`](NOTATION.md))
2. **ISA spec** ✅ v0.4, with v0.5 (FPU) in draft
3. **Emulator core** (`setun-core`, Rust) ✅
   - CPU: decode, ALU, branches, traps, rings, `MODE`/control registers
   - MMU: 2-level 9|9|9 walk, faults, instruction fetch across pages (TLB cache: later, as an optimization)
   - devices: SYSCTL, TPIC, timer, RTC, keyboard, mouse, serial, disk (T1B2 image, DMA), display (text + 3^9-colour framebuffer)
4. **Front ends** ✅: native (`setun`: minifb window, stdio serial, headless PNG screenshots) and **WASM** (`web/`: canvas, keyboard/mouse, serial console, front-panel lamps; boots Trinux from a disk built into the module)
5. **Assembler and disassembler** ✅: text syntax with form selection and branch relaxation, round-trip-safe disassembler, `setun-asm` CLI
6. **Compiler** for **Trine** ✅ ([`LANGUAGE.md`](LANGUAGE.md)): `trine build/run`, stdlib (ports, rt, console, keymap, real, math); `examples/mandel.tri` renders the Mandelbrot set
7. **BIOS** ✅ ([`bios/bios.tri`](../bios/bios.tri), [`BOOT.md`](BOOT.md)): machine check, RTC date, disk boot with the signature `0zB007AB13`, monitor shell
8. **Kernel: Trinux**, modelled on Linux with POSIX interfaces ([`KERNEL.md`](KERNEL.md)) ✅
   - K1 boot, paging, triddy allocator, timer; K2 preemptive user processes; K3 COW fork, execve (TEF), wait4, page faults, initramfs
   - K4 VFS, devfs, tty line discipline, pipes; K5 signals, process groups, job control; K6 tfs root on disk, buffer cache, mounts, /proc
   - K7 tlibc, a POSIX-style sh (functions, eval, here-documents, local, aliases, line editing), core and text utilities, an editor
   - K8 window system: trwm display server, ptys, poll, framebuffer mmap, terminal/clock/colors/paint; Cyrillic font and a Russian layout
   - users and groups: login, su, passwd, useradd/userdel, permission checks, modular password hashing
9. **Setun-387 FPU** ✅ (ISA §14): real27/real81 balanced-ternary floating point with ±∞, NaN and gradual underflow, 27 registers; emulator (`setun-real`), assembler, Trine's `real` type, lazy FPU switching in Trinux, `math.tri`, and `/bin/calc`

## Later

- A Setun-70 emulator as a Trinux user program
- Emulator speed ([`PERFORMANCE.md`](PERFORMANCE.md)): a block interpreter, a Cranelift JIT and fixed-width FPU arithmetic (11–69× faster, bit-identical)
- More of POSIX where it is cheap: `SIGTTOU`/`TOSTOP`, `select`, more `/proc`
