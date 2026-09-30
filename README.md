# Setun-70

An emulator, BIOS and operating system for the **Setun-70**, the balanced-ternary
two-stack computer built at Moscow State University by N. P. Brusentsov and
E. A. Zhogolev (1967–1970).

- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md): the machine, digested from the 1970 ALGOL specification
- [`docs/ROADMAP.md`](docs/ROADMAP.md): milestones and planned extensions
- [`docs/reference/`](docs/reference/): primary sources (the ALGOL spec, the 1971 overview paper, the 2006 history paper, instruction tables)

## Layout

| Path | What |
|---|---|
| `crates/ternary` | balanced-ternary fixed-width integers (`Trit`, `Tryte`, `Word`, …) |

## Build

```
cargo test
```

## Credits

The reference material in `docs/reference/` was collected by Stanislav Maslovski
for his Fortran emulator ([smaslovski/Setun70](https://github.com/smaslovski/Setun70), GPL-3).
His files are included for study; this project's code is written independently
from the 1970 specification.
