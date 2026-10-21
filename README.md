# Setun-11̅

Setun-11̅ (Setun-26): 11̅ is 26 in balanced base 27, one less than 3³.

A balanced-ternary computer sized like an Intel 386, with an emulator, BIOS,
compiler, kernel and window system. It is a new CPU designed from scratch and
named after the Soviet **Setun** computers (Brusentsov, Moscow State
University, 1958 and 1970). From them it keeps balanced arithmetic, the
tritwise multiply and threshold-add operations, and the 3-way branch.

**Everything is a power of 3:**

- 9-trit tryte (3²), the smallest addressable unit
- 27-trit word = register = pointer (3³)
- 3^27-tryte address space (3^3^3), paged 9|9|9 with 3^9-tryte pages
- default RAM 3^18 = 9^9 trytes (≈ 691 MB equivalent)
- 27 general registers, a load/store 3-operand ISA, a sign trit and a carry trit
- the floating-point unit (ISA §14): 27 real registers, real27 (9-trit exponent, 18-trit mantissa) and real81 (72-trit mantissa), where 1/3 is exact and 1/10 is not
- 3 privilege rings on one trit, 243 interrupt vectors, 3^9 I/O ports (a port number is one tryte)
- 640×480 framebuffer, one tryte per pixel = 3^9 colours (27 levels per channel)
- a tryte holds a Unicode code point directly

![Trinux running its window system: a terminal, a clock, all 3^9 colours and a paint program](docs/screenshots/trinux-k8.png)

*Trinux on the emulator: `trwm term clock colors paint`. The Colors window shows every one of the 3^9 pixel colours once.*

The font covers Latin, Greek and the Cyrillic block. Here `cat /etc/pangrams` runs on the text console: ([screenshot](docs/screenshots/cyrillic-console.png))

## Docs

- [`docs/CPU.md`](docs/CPU.md): **the CPU manual**: registers, every instruction, the MMU, traps, devices and ports, the assembler, the ABI
- [`docs/TRINUX.md`](docs/TRINUX.md): **the Trinux manual**: the shell, every command, files, processes, signals, the window system, networking, system calls and libraries
- [`docs/TRINE.md`](docs/TRINE.md): **the Trine manual**: a tutorial and the complete language reference, builtins, the standard library, the compilers and their optimization levels
- [`docs/REAL.md`](docs/REAL.md): the real27/real81 floating-point standard, with its conformance vectors
- [`docs/FORMATS.md`](docs/FORMATS.md): the ternary data formats: tvarint balanced varints, TTV tagged values, TAU sound, TIM pictures
- [`docs/ECC.md`](docs/ECC.md): error correction for memory: SEC-DED [33,27,4], Hamming [31,27,3] and Golay [11,6,5] over GF(3), with a reference codec
- [`docs/SETUN26_ISA.md`](docs/SETUN26_ISA.md): the normative ISA specification (v0.7: integer, floating-point and vector instructions)
- [`docs/PERFORMANCE.md`](docs/PERFORMANCE.md): how the emulator runs fast without changing behaviour, and the benchmarks
- [`docs/LANGUAGE.md`](docs/LANGUAGE.md): Trine, the compact language reference
- [`docs/SELFHOST.md`](docs/SELFHOST.md): self-hosting: `tas` and `tric`, Trinux's own assembler and compiler (`-O0` to `-O4`, the host's output to the character)
- [`docs/TERNARY_WORLD.md`](docs/TERNARY_WORLD.md): the plan for a more ternary system, phase by phase
- [`docs/BOOT.md`](docs/BOOT.md): the boot protocol
- [`docs/KERNEL.md`](docs/KERNEL.md): Trinux's kernel, milestone by milestone (K1–K8, users, the FPU, networking, shared memory, the vector unit)
- [`docs/TNET.md`](docs/TNET.md): the native ternary network between Setun machines: the `TLINK` card, TL frames with a TCRC-18 check over GF(3), TNP addresses, TSP streams
- [`docs/NETWORK.md`](docs/NETWORK.md): networking: the `TNIC` card, the host-side network, Trinux's TCP/IP stack and sockets, telnet and SSH logins, TLS for `wget`, a web server
- [`docs/ROADMAP.md`](docs/ROADMAP.md): build order and milestones
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and [`docs/reference/`](docs/reference/): the historical Setun-70 (heritage and background only)

## Layout

| Path | What |
|---|---|
| `crates/ternary` | balanced-ternary fixed-width integers (`Trit`, `Tryte`, `Word`, `Tern<N>`) |
| `crates/real` | real27/real81 floating point, correctly rounded, and its conformance vectors ([`docs/REAL.md`](docs/REAL.md)) |
| `crates/ecc` | the reference encoders and decoders of the error-correcting codes ([`docs/ECC.md`](docs/ECC.md)) |
| `crates/core` | emulator core (`setun-core`): CPU, traps, 9\|9\|9 MMU, memory map, devices (interrupt controller, timer, RTC, keyboard, mouse, serial, DMA disks, display renderer), `Machine` |
| `crates/asm` | assembler, disassembler and the `setun-asm` tool ([`docs/ASSEMBLER.md`](docs/ASSEMBLER.md)) |
| `crates/demo` | demo ROM in assembly ([`demo.s`](crates/demo/src/demo.s)): text screen, keyboard/serial echo, 3^9-colour graphics |
| `crates/native` | native front end, binary `setun`: window, keyboard/mouse, serial on stdio, `--net`, `--net6` |
| `crates/net` | `setun-net`: the network behind the card, a user-mode gateway (10.0.2.2) on smoltcp with port forwards |
| `crates/web` + `web/` | browser front end: WebAssembly exports (with the BIOS and the Trinux disk built in) and the front-panel page |
| `crates/trine` | the Trine compiler (binary `trine`) and its standard library (`lib/`) |
| `examples/` | Trine programs, e.g. `mandel.tri` |
| `bios/` | the BIOS (`bios.tri`, built into `setun`) and a sample boot sector |
| `kernel/` | Trinux, the kernel (Trine) |
| `user/` | Trinux user programs and `tlibc` |
| `crates/trinux` | `mkdisk`: builds bootable Trinux disks; boot tests |

## Build and run

```
cargo test
cargo run --release -p setun-native            # window: boots the BIOS (--demo: demo ROM)
cargo run --release -p trine -- build --boot bios/boot_hello.tri -o hello.disk
cargo run --release -p setun-native -- --disk hello.disk     # BIOS boots it
cargo run --release -p setun-native -- --help  # options: --rom, --disk, --ram, --mips, --headless, --screenshot
cargo run --release -p trine -- run examples/hello.tri        # compile + run headless
cargo run --release -p trine -- build examples/mandel.tri -o mandel.t1b2
cargo run --release -p setun-native -- --rom mandel.t1b2      # watch it draw
cargo run --release -p trinux --bin mkdisk -- -o trinux.disk
cargo run --release -p setun-native -- --disk trinux.disk     # boot Trinux to a shell prompt
cargo run --release -p setun-native -- --console              # no window: this terminal is the console (ssh); Ctrl-] leaves
cargo run --release -p setun-native -- --console --net 2323:23 # with a network; then `telnet localhost 2323` logs in
cargo run --release -p setun-native -- --console --net 2222:22 # then `ssh -p 2222 root@localhost` (password root)
cargo run --release -p setun-native -- --console --net 8080:80 # then http://localhost:8080/ is Trinux's httpd
sh web/build.sh                                # rebuild web/setun.wasm (BIOS + Trinux disk inside)
python -m http.server 8026 --directory web     # then open http://localhost:8026
```

The browser page boots Trinux from the built-in disk (3^7 pages of RAM). Log in
as `user` (password `user`) or `root` (password `root`), then type at the
prompt, or press **Window system** for `trwm term clock colors paint`.
Power-cycle keeps the disk, so files written in the browser survive a reboot;
**Fresh disk** restores it. The old demo ROM is still under **Boot**.

Speed: the browser build compiles hot code to WebAssembly as it runs (a JIT,
like the native one), and runs Trinux at a few hundred MIPS at Max (about
290 under node; 110 interpreted). The clock can be paced or left at Max.

## Credits

The Setun-70 reference material in `docs/reference/` was collected by Stanislav
Maslovski ([smaslovski/Setun70](https://github.com/smaslovski/Setun70), GPL-3).
It is included for study only.

## License

MIT, see [LICENSE](LICENSE). This covers everything in the repository except
`docs/reference/`, which keeps its original license (GPL-3, see Credits).
