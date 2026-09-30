# Setun-386

A balanced-ternary computer sized like an Intel 386, with an emulator, BIOS,
compiler, kernel and window system. Like the 386 booting as an 8086, it starts
in **Setun-70 compatibility mode**, the ternary two-stack machine built at
Moscow State University by N. P. Brusentsov and E. A. Zhogolev (1967–1970).
The BIOS then switches it to native mode.

- 24-trit words (4 six-trit trytes, ≈38 bits), with the tryte as the smallest addressable unit
- 27 general registers, a load/store ISA, a sign-trit flag, and Setun ternary operations (tritwise multiply, threshold add, 3-way branch)
- 3 privilege rings on one trit, 2-level paging with 2187-tryte pages
- 3^15-tryte default RAM, 3^10 I/O ports, 243 interrupt vectors
- 640×480 framebuffer with one tryte per pixel (729 colours = 9³)

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
