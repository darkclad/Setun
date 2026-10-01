# Roadmap

Target: **Setun-386**, a balanced-ternary machine sized like a 386, where every
size is a power of 3: 9-trit trytes, 27-trit words, a 3^27 address space,
9|9|9 paging, 3 rings, 3^18 default RAM, and a 640×480 framebuffer with 3^9
colours. Then come a compiler, a kernel and a window system, all written for
the machine itself.

ISA: [`SETUN386_ISA.md`](SETUN386_ISA.md) (v0.4, accepted).

## Milestones

1. **`ternary` crate** ✅: `Tern<N>`, 9-trit `Tryte`, 27-trit `Word`, trit fields, base-27 notation
2. **ISA spec**: `SETUN386_ISA.md` ✅ v0.4
3. **Emulator core** (`setun-core`, Rust)
   - CPU: decode, ALU, branches, traps, rings, `MODE`/control registers ✅
   - MMU: 2-level 9|9|9 walk, faults, instruction fetch across pages ✅ (TLB cache: later, as an optimization)
   - devices: SYSCTL, TPIC, timer, RTC, keyboard, mouse, serial, disk (T1B2 image, DMA), display (text + 3^9-colour framebuffer) ✅
   - tests ✅: 20 CPU/MMU tests and 13 device tests (timer-interrupt program, ROM boot, DMA)
4. **Front ends** ✅: native (`setun`: minifb window, stdio serial, headless PNG screenshots) and **WASM** (`web/`: canvas, keyboard/mouse, serial console, front-panel lamps; boots Trinux from a disk built into the module)
5. **Assembler and disassembler** ✅: text syntax with form selection and branch relaxation, round-trip-safe disassembler, `setun-asm` CLI; the demo ROM is now `demo.s`
6. **Compiler** for **Trine** ✅ ([`LANGUAGE.md`](LANGUAGE.md) v0.2): `trine build/run`, stdlib (ports, rt, console), 19 end-to-end tests; `examples/mandel.tri` renders the Mandelbrot set
7. **BIOS** ✅ ([`bios/bios.tri`](../bios/bios.tri), [`BOOT.md`](BOOT.md)): machine check, RTC date, disk boot with signature, monitor shell; `setun` boots it by default; `trine build --boot` makes boot disks
8. **Kernel: Trinux**, modelled on Linux with POSIX interfaces ([`KERNEL.md`](KERNEL.md)): K1 ✅ boot, paging, triddy allocator, timer; K2 ✅ preemptive user processes; K3 ✅ COW fork, execve (TEF), wait4, page faults, initramfs, /sbin/init; K4 ✅ VFS, devfs, tty line discipline, pipes, dup2, /bin/sh with coreutils; K5 ✅ signals, process groups, job control, ^C/^Z; K6 ✅ tfs root on disk, buffer cache, mounts, /proc, ps; K7 ✅ tlibc modules, a POSIX-style sh (control flow, expansions, variables), core utilities; K8 ✅ window system: trwm display server, ptys, poll, framebuffer mmap, terminal/clock/colors/paint
9. **Window system**: framebuffer compositor, mouse cursor, windows, a terminal app
10. Later: a "Setun-387" floating-point unit, and a Setun-70 emulator as a user program
