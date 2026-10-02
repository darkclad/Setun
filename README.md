# Setun-386

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
- the Setun-387 FPU: 27 real registers, real27 (9-trit exponent, 18-trit mantissa) and real81 (72-trit mantissa), where 1/3 is exact and 1/10 is not
- 3 privilege rings on one trit, 243 interrupt vectors, 3^9 I/O ports (a port number is one tryte)
- 640×480 framebuffer, one tryte per pixel = 3^9 colours (27 levels per channel)
- a tryte holds a Unicode code point directly

![Trinux running its window system: a terminal, a clock, all 3^9 colours and a paint program](docs/screenshots/trinux-k8.png)

*Trinux on the emulator: `trwm term clock colors paint`. The Colors window shows every one of the 3^9 pixel colours once.*

The font covers Latin, Greek and the Cyrillic block. Here `cat /etc/pangrams` runs on the text console: ([screenshot](docs/screenshots/cyrillic-console.png))

## Docs

- [`docs/SETUN386_ISA.md`](docs/SETUN386_ISA.md): the ISA (v0.4 accepted; v0.5 draft adds the Setun-387 floating-point unit)
- [`docs/PERFORMANCE.md`](docs/PERFORMANCE.md): how the emulator runs fast without changing behaviour, and the benchmarks
- [`docs/LANGUAGE.md`](docs/LANGUAGE.md): Trine, the systems language (v0.2, implemented)
- [`docs/BOOT.md`](docs/BOOT.md): the boot protocol
- [`docs/KERNEL.md`](docs/KERNEL.md): Trinux, the Linux-modelled POSIX kernel (K1–K8 running: a POSIX-like system with a scripting shell, core utilities and a window system with a terminal)
- [`docs/ROADMAP.md`](docs/ROADMAP.md): build order and milestones
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and [`docs/reference/`](docs/reference/): the historical Setun-70 (heritage and background only)

## Layout

| Path | What |
|---|---|
| `crates/ternary` | balanced-ternary fixed-width integers (`Trit`, `Tryte`, `Word`, `Tern<N>`) |
| `crates/core` | emulator core (`setun-core`): CPU, traps, 9\|9\|9 MMU, memory map, devices (interrupt controller, timer, RTC, keyboard, mouse, serial, DMA disks, display renderer), `Machine` |
| `crates/asm` | assembler, disassembler and the `setun-asm` tool ([`docs/ASSEMBLER.md`](docs/ASSEMBLER.md)) |
| `crates/demo` | demo ROM in assembly ([`demo.s`](crates/demo/src/demo.s)): text screen, keyboard/serial echo, 3^9-colour graphics |
| `crates/native` | native front end, binary `setun`: window, keyboard/mouse, serial on stdio |
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
sh web/build.sh                                # rebuild web/setun.wasm (BIOS + Trinux disk inside)
python -m http.server 8386 --directory web     # then open http://localhost:8386
```

The browser page boots Trinux from the built-in disk (3^7 pages of RAM). Log in
as `user` (password `user`) or `root` (password `root`), then type at the
prompt, or press **Window system** for `trwm term clock colors paint`.
Power-cycle keeps the disk, so files written in the browser survive a reboot;
**Fresh disk** restores it. The old demo ROM is still under **Boot**.

Speed: about 20 MIPS native and 16 MIPS in the browser for the demo ROM; Trinux
(paging, user mode) runs at about 4-5 MIPS in a browser tab. The clock can be
paced or left at Max.

## Credits

The Setun-70 reference material in `docs/reference/` was collected by Stanislav
Maslovski ([smaslovski/Setun70](https://github.com/smaslovski/Setun70), GPL-3).
It is included for study only.
