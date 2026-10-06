# Trine: a C-like language for Setun-11̅

**Status: v0.2, implemented by the v1 compiler (`crates/trine`, binary `trine`).**
§14 records the design decisions; §15 lists the changes from v0.1.

Trine is the systems language for Setun-11̅: the language of the BIOS,
kernel, drivers, window system and applications. It is a small language in
the spirit of C: statically typed, with pointers and manual memory, compiled
ahead of time to Setun-11̅ assembly, and suited to bare metal. Its
arithmetic is balanced ternary all the way down.

The name means "threefold". Source files use `.tri`. **⚠ REVIEW**

## 1. Goals and non-goals

**Goals**
- **Enough for an OS:** pointers, structs, arrays, function pointers, interrupt handlers, port I/O, control registers and inline assembly.
- **Honest about ternary:** there are no unsigned types, comparisons can be three-way, operators are tritwise, and shifts count trits.
- **Simple to compile:** one compilation unit, no preprocessor, and a declaration syntax that parses without C's declarator puzzles.
- **Predictable:** every access to memory that has an address (globals, anything through a pointer, locals whose address is taken) happens in the output as written, in order, so there is no need for `volatile`. The optimizer (§16) only keeps other locals in registers and tidies register moves.

**Not in v1:** 81-trit `long`, generics, a preprocessor or macros, variadic functions, and passing or returning structs by value.

## 2. Lexical structure

- **Comments:** `// to end of line` and `/* block */`, which do not nest.
- **Identifiers:** `[A-Za-z_][A-Za-z0-9_]*`. Case matters.
- **Integer literals**, the same forms as the assembler:
  - `42` and `1_000` (decimal)
  - `0t1T0` = `0t11̅0` = 6 (balanced ternary: `1`, `0`, and `T` or an overlined `1` for −1, most significant first)
  - `0zDD5̅` (balanced base 27: `0`–`9` and `A`–`D` = 0…13 in either case; a negative digit is overlined with U+0305 COMBINING OVERLINE; see ISA §1.1)
  - `0x1F` (hexadecimal, for values from binary systems; nothing wraps, so a sign is written with unary minus)
  - A malformed literal is an error that says what is wrong (a bar on `0`, two bars, `E` as a digit, the old `+`/`-` base-3 digits…), and a constant must fit its type.
- **Real literals:** `2.5`, `1e10`, `6.02e-23`, `1_000.5`: digits with a point and digits, an exponent, or both (`1.` and `.5` are not reals). A letter right after a number is an error. The value is rounded to the nearest `real`.
- **Character literals:** `'A'`, `'Ж'`, `'\n'`. The value is the Unicode code point, of type `tryte`. Escapes are `\n \t \r \e \0 \\ \' \"` (`\e` is ESC, 27, for terminal escape sequences).
- **String literals:** `"Привет"`. A NUL-terminated array of trytes, one character per tryte. Its type is `*tryte`, pointing at the first character.
- **Keywords:** `fn var const struct return if else while for loop break continue sign switch case default as sizeof import extern interrupt asm true false unknown b_true b_false null void trit tryte int char real`
- `neg`, `zero` and `pos` are **contextual**: they are arm labels only when followed by `:` inside `sign { }`, and ordinary names everywhere else.

## 3. Types

| Type | Size (trytes) | Values | Notes |
|---|---|---|---|
| `trit` | 1 | −1, 0, +1 | three-state flags, signs. Stored in a tryte. |
| `tryte`, alias `char` | 1 | −9 841 … 9 841 | a `char` holds one Unicode code point |
| `int` | 3 | ±3 812 798 742 493 | the machine word: 27 trits |
| `real` | 9 | ±3^9840, 71-trit precision, ±∞, NaN | the FPU's real81 (ISA §14): about 34 decimal digits |
| `*T` | 3 | an address | a pointer is a word |
| `[N]T` | N·sizeof(T), rounded up (§4) | | fixed-size array |
| `struct S` | a multiple of 3 | | §4 |
| `fn(A, B) -> R` | 3 | | function pointer |
| `void` | – | | only as a return type or `*void` |

- **There are no unsigned types.** Every integer is balanced signed, so negation is exact and `abs` never overflows.
- **Truth values are ternary (Kleene logic):** `true`, `unknown` and `false` are the `trit`s +1, 0 and −1, so `&`, `|` and `~` (min, max, negation) are Kleene's AND, OR and NOT: `unknown & false` is `false`, `unknown | false` is `unknown`. `&`, `|` and `*` of two trits, and `~` of a trit, are trits. The three names never become an `int` implicitly (`var done: int = false` is refused); `t as int` is the explicit form.
- **Conditions are binary:** `if`, `while`, `for`, `?:`, `!`, `&&` and `||` take an integer, a pointer or a real, true when non-zero. The binary truth values are `b_true` = 1 and `b_false` = 0, both `int`s. **A `trit` is refused as a condition**, because tested that way `false` (−1) would hold: branch three ways with `sign (t) { neg: … zero: … pos: … }`, compare (`t == true`), or convert (`t as int`). `if (a <=> b)` is refused likewise; the fix is `a != b`. `null` is a pointer 0.
- **Conversions:** implicit only when a value always fits (`trit` → `tryte` → `int`). Everything else needs `as`.
  - Narrowing keeps the low trits: `x as tryte` is `x` wrapped to 9 trits, and `x as trit` is its lowest trit. This is balanced wrapping, so `9842 as tryte` = −9841.
  - Use `sgn(x)` for the sign.
  - `as` also converts between pointer types, and between pointers and `int`.
- **Overflow wraps** modulo 3^27 (balanced) and is never undefined.
- **Reals:** an integer converts to `real` implicitly; `x as int` rounds a real to the nearest integer (ties to even) and saturates, with the IV flag, on NaN or out of range. Arithmetic `+ - * / %` follows the FPU (round to nearest; `%` is the exact IEEE remainder), and `|`/`&` on reals are max/min. Comparisons with a NaN are unordered: every one is false except `!=`. A real is true when it is non-zero, so a NaN is true. Exceptions are masked by default and only set the sticky flags in `fsr()`.

## 4. Memory layout and addresses ⚠ REVIEW

Setun-11̅ addresses a multi-tryte value by its **centre tryte**: the word at address `A` occupies `A−1, A, A+1`. Trine keeps this rule, so **a pointer is always the hardware address used to access the object**.

| Object | Its address | Alignment |
|---|---|---|
| `trit`, `tryte`, `char` | the tryte itself | none |
| `int`, pointer, function pointer | the centre of the word | a multiple of 3 |
| array of trytes (e.g. a string) | its first tryte | none |
| array of words or structs | the address of element 0 | a multiple of 3 |
| `struct` | the centre of its first word | a multiple of 3 |

**Structs** are laid out in word *slots*:
- Each slot has three tryte positions: offsets −1, 0 and +1 from the slot centre.
- Fields go in declaration order. A word field takes a whole fresh slot, at offset `3j`.
- Tryte fields fill the next free positions. A tryte array can run across slots.
- The struct's size is a whole number of slots.

```
struct Entry { tag: tryte; name: [4]tryte; id: int; }

slot:      0             1             2
offset:   -1   0  +1  |  2   3   4  |  5   6   7
field:    tag n0  n1  | n2  n3  pad | id id  id      sizeof = 9, id at offset 6
```

- `p + n` on a `*T` advances by `n · sizeof(T)` trytes.
- `&arr[i]` = `&arr[0] + i · sizeof(T)`.
- `bytes(p)` gives the first tryte of a word-addressed object (`p − 1` as `*tryte`), for code that copies memory tryte by tryte.
- This is the one place where centre addressing shows through. Everywhere else, pointers just work.

## 5. Declarations

The declaration syntax is **name first**, in the style of Go and Rust rather than C. It is easier to read and to parse: `*[4]int` is a pointer to an array, and `[4]*int` is an array of pointers. **⚠ REVIEW**

```
var count: int = 0;                  // global or local variable
var buf: [80]char;                   // zero-initialised
const WIDTH: int = 640;              // compile-time constant (no storage)
const GREETING: *tryte = "Hello";

struct Point { x: int; y: int; }
var origin: Point = { 0, 0 };

fn add(a: int, b: int) -> int { return a + b; }
fn reset() { ... }                   // no `->` means void

extern fn puts(s: *tryte);           // defined in assembly or elsewhere
extern var ticks: int;
```

- **Locals** may be declared anywhere in a block. Their scope runs from the declaration to the end of the block. An uninitialised local has an unspecified value; an uninitialised global is zero.
- **Initialisers:** `{ … }` for arrays and structs, and string literals for `[N]char`.
- **Placement attributes** (for the BIOS and kernel):
  - `@align(729) var itb: [243]int;` aligns a global.
  - `@at(-1604164) var fb: [307200]tryte;` places a global at a fixed address without allocating it (memory-mapped devices).

## 6. Expressions

Precedence, from highest to lowest:

| Level | Operators | Notes |
|---|---|---|
| 1 | `f(x)` `a[i]` `s.f` `p.f` | `.` looks through pointers automatically, so there is no `->` |
| 2 | `-x` `~x` `!x` `*p` `&x` `sizeof(T)`, then `x as T` | `~` is tritwise NOT, which in balanced ternary **equals `-x`**. `as` applies after prefix operators: `&x as int` is `(&x) as int`. |
| 3 | `*` `/` `%` | `/` truncates toward zero and `%` has the sign of the dividend (C rules). `divr`/`modr` round (§8). |
| 4 | `+` `-` | pointer ± integer scales by sizeof |
| 5 | `<<` `>>` | shift by **trits**: `x << n` = x·3^n; `x >> n` drops n trits, **rounding to nearest** |
| 6 | `&` | tritwise **minimum** (ternary AND) |
| 7 | `\|` | tritwise **maximum** (ternary OR) |
| 8 | `<=>` | **three-way compare**: a `trit`, −1 / 0 / +1. Exact, never fooled by overflow. |
| 9 | `<` `<=` `>` `>=` | `int` 1 or 0 |
| 10 | `==` `!=` | `int` 1 or 0 |
| 11 | `&&` | short-circuit |
| 12 | `\|\|` | short-circuit |
| 13 | `c ? a : b` | |
| 14 | `=` `+=` `-=` `*=` `/=` `%=` `<<=` `>>=` `&=` `\|=` | assignment is an expression, right-associative |

**Evaluation order:** operands left to right, and function arguments left to right.

There is **no binary `^`**. Ternary has no single "exclusive or", so the tritwise product, threshold add and consensus are builtins (§8).

## 7. Statements

```
if (x > 0) { ... } else if (x == 0) { ... } else { ... }
while (n > 0) { n -= 1; }
for (var i: int = 0; i < 10; i += 1) { ... }
loop { ... if (done) break; }        // endless loop
break; continue; return e;

sign (a <=> b) {                     // three-way branch: a single CMP + BR3
    neg:  lesser();
    zero: same();
    pos:  greater();
}

switch (c) {                         // no fall-through; `case` takes constants
    case 'a', 'b': vowel_like();
    case 10: newline();
    default: other();
}

asm { "hlt" }                        // inline assembly, §9
```

`sign (e)` takes any integer expression and branches on its sign. With `e = a <=> b`, or `e = a - b` when it cannot overflow, it compiles to one `CMP` and one `BR3`, the Setun's native three-way jump. Every arm is optional.

## 8. Builtins

These are not functions: each compiles to one instruction or a few.

| Builtin | Meaning | Instruction |
|---|---|---|
| `sgn(x)` `abs(x)` | sign, absolute value | `SGN`, `ABS` |
| `divr(a, b)` `modr(a, b)` | rounding division, balanced remainder | `DIVR`, `MODR` |
| `tmul(a, b)` `tsat(a, b)` `cons(a, b)` | tritwise product / threshold add / consensus | `TMUL`, `TSAT`, `CONS` |
| `trit(x, i)` | trit `i` (1 = most significant … 27) | `TRIT` |
| `ext(x, pos, len)` `ins(x, v, pos, len)` | read / write a trit field (constant `pos`, `len`) | `EXT`, `INS` |
| `tlz(x)` | leading zero trits | `TLZ` |
| `band(a, b)` `bor(a, b)` `bxor(a, b)` `bandn(a, b)` | binary AND / OR / XOR / a AND NOT b, on two's-complement values (ISA §3.3.1) | `BAND` … `BANDN` |
| `bsh(x, n)` | x·2^n, or ⌊x / 2^−n⌋ for n < 0 | `BSH` |
| `bext(x, pos, len)` `brot(x, n, len)` | bits pos..pos+len−1; the low len bits rotated left by n (constant `pos`/`n`, `len`) | `BEXT`, `BROT` |
| `mulh(a, b)` | high word of the 54-trit product | `MULH` |
| `bytes(p)` | first tryte of a word-addressed object | `SUB 1` |
| `in(port)` `out(port, v)` | port I/O | `IN`, `OUT` |
| `mfc(REG)` `mtc(REG, v)` | control registers (`MODE`, `PTB`, `ITB`, …) | `MFC`, `MTC` |
| `hlt()` `brk()` `tlbflush()` `tlbinv(a)` `fence()` | system | as named |
| `syscall(n, …)` | trap to the kernel, arguments in r1–r6 | `SYS n` |
| `abs(x)` `sqrt(x)` | on a `real` | `FABS`, `FSQRT` |
| `trunc(x)` `round(x)` | to an integral real: toward zero / to nearest even | `FTRUNC`, `FRND` |
| `rem(a, b)` | IEEE remainder (the same as `%`) | `FREM` |
| `scale3(x, k)` `exponent3(x)` | x·3^k; the exponent E of x | `FSCL`, `FEXP` |
| `fconst(k)` | 0 zero, 1 one, 2 π, 3 e, 4 ln 3, 5 log₃ e, 6 log₃ 10 (constant `k`) | `FCONST` |
| `fclass(x)` | −3 −∞, −2 normal, −1 subnormal, 0 zero, … 3 +∞, 4 NaN | `FCLASS` |
| `real27(x)` | x rounded to real27 precision (17 trits) | `FTOW` + `FFRW` |
| `fsr()` `set_fsr(v)` | the FPU status register (flags, masks; ISA §14.4) | `FMFS`, `FMTS` |

## 9. Low-level features

**Interrupt and trap handlers:**

```
interrupt fn on_timer() {
    ticks += 1;
    out(TPIC_EOI, in(TPIC_CLAIM));
}
```

An `interrupt fn` takes no arguments. The compiler saves every register the handler uses, including `lr`, and ends with `IRET` instead of `RET`. The handler's address is `&on_timer`, ready to store in the vector table:

```
@align(729) var itb: [243]int;
itb[121 + 1] = &on_timer as int;     // vector +1 = IRQ 0; the table is centred
mtc(ITB, &itb[121] as int);
```

**Inline assembly:** `asm { "line1" "line2" }`. The strings are pasted into the output verbatim. Trine locals are not visible inside; pass values in globals, or use a small `extern fn` written in assembly.

**Entry point:** `fn main()`. The compiler generates startup code (`crt0`) at the reset vector. In order, it:
1. sets `sp`;
2. installs a vector table that reports CPU faults;
3. zeroes the globals;
4. runs the global initialisers;
5. calls `main`.

When `main` returns, its value goes to `SYSCTL.DEBUG` and the machine powers off. `trine run` reads that value back as the program's exit code.

**Where things live:** code and string literals in ROM; globals in RAM (by default from address 9843 upward, with the stack below them). Global initialisers run as generated code at start-up, so ROM programs get ordinary writable globals.

## 10. Modules

```
import "console.tri";
```

`import` includes a file once, textually (a second import of the same file does nothing), and the whole program compiles as one unit. Symbol names are global. A module keeps a name private by prefixing it with `_`, which hides it from other files. **⚠ REVIEW**

## 11. Calling convention (ABI)

| | |
|---|---|
| Arguments | `r1`–`r6`, left to right; more are pushed on the stack, the last one first |
| Return value | `r1`; a `real` in `f1` |
| Real arguments | `f1`–`f9`, in order among the reals (the integer arguments still take `r1`–`r6` in order among themselves); a function with reals takes at most 9 reals and 6 others |
| FPU | `f1`–`f13`, `f−1`–`f−13` are caller-saved |
| Caller-saved | `r1`–`r10`, `r-10`–`r-13` |
| Callee-saved | `r-1`–`r-9`, `r11` (`fp`), `r13` (`sp`). Compiled code keeps locals in `r-1`–`r-9`, so assembly called from Trine must preserve them (the kernel's `switch_to` saves them with the stack). |
| Return address | `r12` (`lr`), set by `CALL`/`JAL` |
| Stack | grows toward negative addresses, always word-aligned |
| Frame | `fp` points at the saved `fp`; locals sit below it |

Structs and arrays are passed as pointers. This is the same convention as the ISA's suggested ABI, so assembly and Trine can call each other.

## 12. Standard library (v1, small)

| Module | Contents |
|---|---|
| `rt.tri` | `memset`, `memcpy`, `strlen`, `strcmp`, `itoa` (decimal), `itoa27` (a word as 9 base-27 digits, negative ones overlined), `itoa3` (base 3), `b27_digit` |
| `console.tri` | serial `putc`/`puts`/`getc`; a text-mode screen writer with cursor, colours and scrolling |
| `real.tri` | `rtoa(x, digits, buf)` (decimal text, like `%g`), `strtor`, `atof`, `pow10`, `floor`, `ceil` |
| `math.tri` | `exp ln log3 log10 pow sin cos tan asin acos atan atan2 sinh cosh tanh hypot pi e`, from the FPU's `+ − × ÷ √` to about 30 digits; ternary range reduction (`exponent3`/`scale3`), so `log3` of a power of 3 is exact; C's special values and flags |
| `ports.tri` | named constants for every device register in ISA §8 |
| (generated) | startup code, see §9 |

## 13. Examples

**Hello, serial:**

```
import "ports.tri";

fn putc(c: char) {
    while (in(SERIAL_TX_READY) <= 0) {}
    out(SERIAL_DATA, c);
}

fn puts(s: *char) {
    while (*s != 0) { putc(*s); s = s + 1; }
}

fn main() {
    puts("Привет, Setun!\n");
}
```

**Three-way search:**

```
fn find(a: *int, n: int, key: int) -> int {
    var lo: int = 0;
    var hi: int = n - 1;
    while (lo <= hi) {
        var mid: int = (lo + hi) / 2;
        sign (a[mid] <=> key) {
            neg:  lo = mid + 1;
            zero: return mid;
            pos:  hi = mid - 1;
        }
    }
    return -1;
}
```

**Pixels (27 levels per channel):**

```
const FB: *tryte = -1604164 as *tryte;

fn rgb(r: int, g: int, b: int) -> tryte {       // each −13..13
    return (r * 729 + g * 27 + b) as tryte;
}

fn plot(x: int, y: int, c: tryte) { FB[y * 640 + x] = c; }
```

## 14. Design decisions (accepted 2026-09-30)

1. **The name "Trine"** and the `.tri` extension.
2. **Name-first declarations** (`var x: int`, `fn f(a: int) -> int`, `*[4]int`) instead of C's declarator syntax.
3. **Addresses and layout (§4):** a pointer is the hardware address, so word objects are addressed by their centre; structs are laid out in word slots; `bytes(p)` gives a tryte view.
4. **Operators:** `&` and `|` are tritwise min and max, there is no `^`, `~x == -x`, shifts count trits with `>>` rounding, and `<=>` returns a trit.
5. **The `sign` statement** as the language's 3-way branch.
6. **C division semantics** for `/` and `%`, with rounding division available through builtins.
7. **One compilation unit** with textual `import`, and `_` for private names.
8. **No `volatile`:** every access to addressable memory happens as written; the optimizer (§16) never removes or reorders one.
9. **What v1 leaves out:** floats, `long`, struct values, varargs, generics and macros.

## 15. Changes from v0.1

- **`ovf()` was removed.** Address arithmetic in the generated code overwrites the carry trit, so the builtin could not be reliable. It may return with an optimising compiler.
- **`neg`, `zero` and `pos` became contextual** instead of keywords, so they stay usable as variable names.
- **`as` binds after prefix operators.**
- **Globals live in RAM** and are filled in by generated start-up code (§9).
- **`trit(x, i)` parses as the builtin** even though `trit` is a type keyword.
- **Compound assignment narrows implicitly:** `t += 1` on a `tryte` wraps, while `t = t + 1` still needs `as tryte`.

## 16. Optimization

`trine` optimizes by default (`Options::optimize`; off gives the plain code, for comparisons):

- **Register locals:** word scalars (`int`, pointers) declared in a function, and its parameters, live in the callee-saved registers `r-1`–`r-9` when their address is never taken (`&x` anywhere in the function keeps every `x` in memory). Registers are handed out at the declaration and freed at the end of the block; a function saves the ones it uses in its prologue. Functions with inline assembly keep all locals in memory.
- **Direct operands:** arithmetic and comparisons take register locals, constants and simple loads as operands (`mul r1, r-6, r-6`) instead of pushing and popping intermediates.
- **Unreached functions left out:** only the functions reachable from `main` are kept, with interrupt handlers, functions holding inline assembly (whose labels `extern fn`s may name), and anything named by a global initialiser or in inline assembly text. The rest are still checked, then dropped: every program carries the whole runtime and the libraries it imports, and smaller programs load faster (the Trinux shell workload ran 24% fewer instructions). The plain output keeps them all, as `tric` does.
- **Peephole:** a pass over each function's assembly, driven by register liveness (the S/C flags count as a register), forwards copies into their uses, computes results straight into their final register and removes dead side-effect-free instructions. Loads, stores, calls and anything it does not recognize are left alone.

The effect on the benchmarks (`crates/bench`): the Mandelbrot inner loop went from about 70 instructions to 22; across whole programs 39–69% fewer instructions run (`docs/PERFORMANCE.md`). `crates/trine/tests/optimizer.rs` compiles programs both ways and requires the same results.

## Appendix: grammar (EBNF)

```
program     = { import | decl } ;
import      = "import" STRING ";" ;
decl        = [ attr ] ( var | const | struct | func | extern ) ;
attr        = "@align" "(" expr ")" | "@at" "(" expr ")" ;
var         = "var" IDENT ":" type [ "=" init ] ";" ;
const       = "const" IDENT ":" type "=" expr ";" ;
struct      = "struct" IDENT "{" { IDENT ":" type ";" } "}" ;
func        = [ "interrupt" ] "fn" IDENT "(" [ param { "," param } ] ")" [ "->" type ] block ;
extern      = "extern" ( "fn" IDENT "(" [ param { "," param } ] ")" [ "->" type ]
                       | "var" IDENT ":" type ) ";" ;
param       = IDENT ":" type ;
type        = "int" | "tryte" | "char" | "trit" | "void" | IDENT
            | "*" type | "[" expr "]" type | "fn" "(" [ type { "," type } ] ")" [ "->" type ] ;
init        = expr | "{" [ init { "," init } ] "}" ;
block       = "{" { stmt } "}" ;
stmt        = var | block | "if" "(" expr ")" stmt [ "else" stmt ]
            | "while" "(" expr ")" stmt | "loop" stmt
            | "for" "(" [ var | expr ";" ] [ expr ] ";" [ expr ] ")" stmt
            | "sign" "(" expr ")" "{" { ( "neg" | "zero" | "pos" ) ":" { stmt } } "}"
            | "switch" "(" expr ")" "{" { ( "case" expr { "," expr } | "default" ) ":" { stmt } } "}"
            | "break" ";" | "continue" ";" | "return" [ expr ] ";"
            | "asm" "{" { STRING } "}" | expr ";" ;
expr        = assignment ;   (* precedence as in §6 *)
```
