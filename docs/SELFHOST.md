# Self-hosting

Trinux can build its own programs. Two programs, both written in Trine,
take the place of the host's toolchain:

| Program | Does | Matches |
|---|---|---|
| `tas` | assembles `.s` into a Trinux executable (TEF) | `setun-asm`, word for word |
| `tric` | compiles `.tri` into assembly, then runs `tas` | `trine build --user --no-opt`, character for character |

```text
tric [-S] [-v] [-o OUT] FILE.tri      compile (and assemble) a program
tas [-r] [-o OUT] FILE.s              assemble
```

`tric -o hello hello.tri` writes the executable `hello` (default `a.out`);
with `-S` it writes the assembly instead (default `a.s`). `-v` reports the
program's size and the memory the compile took. Imports resolve as on the
host: next to the importing file first, then the standard library, which
lives in `/usr/lib/trine` (`ports.tri`, `rt.tri`, `console.tri`,
`keymap.tri`, `real.tri`, `math.tri`).

The Trinux disk carries what they need: the libraries in `/usr/src/lib`,
the standard library, the compiler's own sources in `/usr/src/bin`, and an
example:

```text
$ cd /usr/src
$ tric -o /root/hello hello.tri
$ /root/hello one two
hello: pid 12, parent 7, argc 3: [/root/hello] [one] [two]
$ tric -o /root/tric bin/tric.tri          # the compiler, rebuilt by itself
```

A program's imports are relative to its file, so a program kept elsewhere
imports the libraries by their full path (`import "/usr/src/lib/tlibc.tri";`).

## The rule: the same output as the host

Neither program is a new design. Each is a port of the host's code, and
the tests hold them to its output:

- `tas` must turn the host compiler's assembly into the same executable
  the host assembler makes (`echo` through `sh` and `wget`, 93k lines,
  and `tas` itself).
- `tric` must write the same assembly as the host compiler with its
  optimizer off: the same instructions, label numbers, string and real
  pools, and global equates. The tests compare `echo`, `cat`, `sh`,
  `edit`, `tas`, `tric`, `libctest`, `wget`, `calc` and `fputest`.
- Its errors are the host's too: for each of 26 bad programs (lexing,
  parsing, types, layout, imports), `tric` gives the host's message at the
  same line and column, exits 1 and leaves no output file behind.

Matching the host to the character keeps the port honest: any difference
is a bug in one of the two, and a diff shows where. It also pins down
details that are easy to get wrong in a port:

- **Look-ahead takes labels.** To find an expression's type, the host
  compiles it and throws the code away (`peek_type`). The labels that pass
  allocated stay allocated, and strings it interned stay in the pool. `tric`
  does the same, and remembers each expression's type as the host does, so
  the label numbers agree.
- **Imports load depth first.** A file's own items follow everything it
  imports, and each file loads once (by its normalized path).
- **Real literals are exact.** `1e-4700` or `0.1` becomes a real81 by the
  host's rule: the decimal's exact value as a big rational, rounded once to
  72 trits, ties to even, with subnormals at the bottom of the exponent
  range and ∞ at the top. `tric` carries a small bignum (base-3^9 limbs,
  long division a trit at a time) for this.

## The fixed point

`tric` builds itself:

```text
tric -o tric2 /usr/src/bin/tric.tri      (tric as the host built it)
./tric2 -o tric3 /usr/src/bin/tric.tri
```

`tric2` and `tric3` are the same file, and both are the file `tas` makes
from the host compiler's unoptimized assembly of `tric`. What `tric3`
compiles runs.

## Memory

A Trinux process is small next to the host, so `tric` keeps little in
memory:

- Each function's code goes to the output file as soon as it is done. The
  host only ever edits the function being compiled: look-ahead truncation,
  and the prologue's `sub sp` inserted once the frame size is known. Only
  the startup code needs the globals' layout, and that is fixed before the
  first function.
- Its tables are sized from the program: the symbol tables after the
  sources are read, the syntax tree in growable arrays.

Compiling `wget` (48k expressions, 1.8M trytes of assembly) takes 5.3M
trytes of heap. It runs on the default machine of 729 pages. Large
assembly goes to the disk, not `/tmp`, which keeps each file in one block
of RAM.

## Not yet

- The optimizer: `tric` is the host compiler with `--no-opt`, so its
  programs run slower than the ones the host builds. Porting the optimizer
  (register locals, inlining, the peephole pass) would be the next step.
- `tric` has its own lexer limits where the host is looser: a number that
  does not fit in a word is refused when it is read, where the host reads
  it and refuses it later.

## Tests

`crates/trinux/tests/selfhost.rs`: `tas_matches_the_host_assembler`,
`tas_assembles_the_largest_programs`, `tas_reports_errors_with_their_lines`,
`tric_matches_the_host_compiler`, `tric_converts_reals_as_the_host_does`,
`tric_reports_errors_as_the_host_does`, `tric_builds_itself`.
