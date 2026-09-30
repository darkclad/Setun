# Setun-386

A balanced-ternary computer sized like an Intel 386, with an emulator, BIOS,
compiler, kernel and window system. Like the 386 booting as an 8086, it starts
in **Setun-70 compatibility mode**, the ternary two-stack machine built at
Moscow State University by N. P. Brusentsov and E. A. Zhogolev (1967–1970).
The BIOS then switches it to native mode.

**Everything is a power of 3:**

- 9-trit tryte (3²), the smallest addressable unit
- 27-trit word = register = pointer (3³)
- 3^27-tryte address space (3^3^3), paged 9|9|9 with 3^9-tryte pages
- 27 general registers, a load/store 3-operand ISA, a sign trit and a carry trit, and Setun ternary operations (tritwise multiply, threshold add, 3-way branch)
- 3 privilege rings on one trit, 243 interrupt vectors, 3^9 I/O ports (a port number is one tryte)
- 640×480 framebuffer, one tryte per pixel = 3^9 colours (27 levels per channel)
- a tryte holds a Unicode code point directly

## Docs

- [`docs/SETUN386_ISA.md`](docs/SETUN386_ISA.md): the native ISA (draft, under review)
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md): the Setun-70 (compatibility mode), digested from the 1970 ALGOL specification
- [`docs/ROADMAP.md`](docs/ROADMAP.md): build order and milestones
- [`docs/reference/`](docs/reference/): Setun-70 primary sources

## Layout

| Path | What |
|---|---|
| `crates/ternary` | balanced-ternary fixed-width integers (`Trit`, `Tryte`, `Word`, …) |

## Build

```
cargo test
```

## Credits

The Setun-70 reference material in `docs/reference/` was collected by Stanislav
Maslovski for his Fortran emulator ([smaslovski/Setun70](https://github.com/smaslovski/Setun70), GPL-3).
It is included for study; this project's code is written independently.
