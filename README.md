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
- 3 privilege rings on one trit, 243 interrupt vectors, 3^9 I/O ports (a port number is one tryte)
- 640×480 framebuffer, one tryte per pixel = 3^9 colours (27 levels per channel)
- a tryte holds a Unicode code point directly

## Docs

- [`docs/SETUN386_ISA.md`](docs/SETUN386_ISA.md): the ISA (v0.4, accepted)
- [`docs/ROADMAP.md`](docs/ROADMAP.md): build order and milestones
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and [`docs/reference/`](docs/reference/): the historical Setun-70 (heritage and background only)

## Layout

| Path | What |
|---|---|
| `crates/ternary` | balanced-ternary fixed-width integers (`Trit`, `Tryte`, `Word`, `Tern<N>`) |
| `crates/core` | emulator core (`setun-core`): CPU, traps, 9\|9\|9 MMU, memory map, devices (interrupt controller, timer, RTC, keyboard, mouse, serial, DMA disks, display renderer), `Machine` |

## Build

```
cargo test
```

## Credits

The Setun-70 reference material in `docs/reference/` was collected by Stanislav
Maslovski ([smaslovski/Setun70](https://github.com/smaslovski/Setun70), GPL-3).
It is included for study only.
