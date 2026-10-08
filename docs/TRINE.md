# The Trine Programming Language

Trine is the systems programming language of Setun-11̅. The BIOS, the
Trinux kernel, its drivers, the window system, the C library (tlibc) and
the user programs are written in it, and so is `tric`, the compiler that
builds Trine programs inside Trinux.

This manual has two parts. Part I is a tutorial: it introduces the language
through short programs. Part II is the reference: it states every rule,
limit and builtin, and the messages the compiler gives. The appendices
collect the grammar and the tables for quick lookup.

The authority is the compiler in `crates/trine` (the host compiler,
`trine`) and its port `user/bin/tric.tri` (the self-hosted compiler,
`tric`), which produces the same output. `docs/LANGUAGE.md` is the compact
design reference.

## Contents

- [Part I. Tutorial](#part-i-tutorial)
  - [1. Hello, world](#1-hello-world)
  - [2. Variables and types](#2-variables-and-types)
  - [3. Functions](#3-functions)
  - [4. Control flow](#4-control-flow)
  - [5. Pointers and arrays](#5-pointers-and-arrays)
  - [6. Structs](#6-structs)
  - [7. Strings and characters](#7-strings-and-characters)
  - [8. Ternary features](#8-ternary-features)
  - [9. Reals](#9-reals)
  - [10. Calling into assembly](#10-calling-into-assembly)
  - [11. Building and running](#11-building-and-running)
- [Part II. Reference](#part-ii-reference)
  - [1. Lexical structure](#1-lexical-structure)
  - [2. Types](#2-types)
  - [3. Expressions](#3-expressions)
  - [4. Statements](#4-statements)
  - [5. Declarations](#5-declarations)
  - [6. Builtins](#6-builtins)
  - [7. Reals](#7-reals)
  - [8. Program kinds and memory layout](#8-program-kinds-and-memory-layout)
  - [9. Calling convention and inline assembly](#9-calling-convention-and-inline-assembly)
  - [10. The standard library](#10-the-standard-library)
  - [11. The compilers](#11-the-compilers)
  - [12. Differences from C](#12-differences-from-c)
- [Appendix A. Grammar](#appendix-a-grammar)
- [Appendix B. Keywords](#appendix-b-keywords)
- [Appendix C. Operators](#appendix-c-operators)
- [Appendix D. Builtins](#appendix-d-builtins)

---

# Part I. Tutorial

## 1. Hello, world

Trine programs are built for one of two environments: the bare machine,
where the program is the ROM, and Trinux, where it is a user program.

### A ROM program

```trine
import "console.tri";

fn main() -> int {
    puts("Привет, Setun!\n");
    return 0;
}
```

Save it as `hello.tri` and run it:

```text
$ trine run hello.tri
Привет, Setun!
[exit 0 after 230 instructions]
```

`trine run` compiles the program into a ROM image, runs it on an emulated
machine without a window, prints what the program wrote to the serial
console, and reports how it ended. `import "console.tri"` brings in the
standard library's console module, which defines `puts`. When `main`
returns, its value becomes the exit code and the machine powers off.

The program text is UTF-8. Cyrillic text needs no special handling: one
character of `"Привет"` is one tryte (the machine's 9-trit byte), because a
tryte holds Unicode code points up to U+2671 directly.

### A Trinux program

Inside Trinux, programs use tlibc, the C library written in Trine:

```trine
import "/usr/src/lib/tlibc.tri";

fn main(argc: int, argv: **char) -> int {
    puts("Hello from ");
    puts(argv[0]);
    puts("\n");
    return 0;
}
```

```text
$ tric -o hello hello.tri
$ ./hello
Hello from ./hello
```

`main` receives the argument count and the argument vector. Its return
value is the process's exit status. tlibc's `puts` does not add a newline.
On the host, the same program is built with `trine build --user` (with the
import path pointing at `user/lib/tlibc.tri`); Section 11 covers the
options.

## 2. Variables and types

Declarations put the name first and the type after a colon:

```trine
var counter: int = 0;           // a global, in RAM
const LIMIT: int = 27;          // a compile-time constant, no storage

fn main() -> int {
    var t: tryte = 100;         // 9 trits: -9841 .. 9841
    var flag: trit = 1;         // -1, 0 or +1
    var n: int = t + flag;      // trit and tryte widen to int
    counter = n * LIMIT;
    return counter;
}
```

The integer types are `trit` (one trit), `tryte` (9 trits; `char` is the
same type) and `int` (27 trits, the machine word). All are balanced
signed: there are no unsigned types. A smaller type converts to a larger
one implicitly. The other way needs `as`, which keeps the low trits:

```trine
fn main() -> int {
    var big: int = 19683 + 5;
    var t: tryte = big as tryte;    // 5: the low 9 trits
    var s: trit = 7 as trit;        // +1: the lowest trit of 7
    return t * 10 + s;
}
```

A constant that fits may be assigned to a smaller type without `as`
(`var t: tryte = 100;`). Integer arithmetic wraps modulo 3^27 and is never
undefined.

A local without an initialiser has an unspecified value. A global without
one is zero.

## 3. Functions

```trine
fn add(a: int, b: int) -> int {
    return a + b;
}

fn reset(p: *int) {             // no `->`: returns nothing
    *p = 0;
}

fn fib(n: int) -> int {
    if (n < 2) { return n; }
    return fib(n - 1) + fib(n - 2);
}

fn main() -> int {
    var x: int = add(2, 3);
    reset(&x);
    return fib(15) + x;
}
```

Functions may be called before their definition. Parameters are passed by
value; arrays and structs are passed as pointers. A function returns a
scalar, a `real`, or nothing.

A function is also a value of a function type, which is how callbacks
work:

```trine
fn twice(x: int) -> int { return 2 * x; }
fn thrice(x: int) -> int { return 3 * x; }
fn apply(f: fn(int) -> int, v: int) -> int { return f(v); }

fn main() -> int {
    var ops: [2]fn(int) -> int = { twice, thrice };
    return apply(ops[0], 5) * 100 + apply(&thrice, 7);   // 1021
}
```

## 4. Control flow

`if`, `while` and `for` look like C's. `loop` is an endless loop, left
with `break` or `return`:

```trine
fn main() -> int {
    var s: int = 0;
    for (var i: int = 1; i <= 10; i += 1) {
        if (i == 5) { continue; }
        if (i == 9) { break; }
        s += i;
    }
    var n: int = 0;
    while (n < 3) { n += 1; }
    loop {
        n += 10;
        if (n > 50) { break; }
    }
    return s * 1000 + n;            // 31053
}
```

A condition is binary: zero is false and anything else is true. It may be
an integer, a pointer or a real, but not a `trit` (Section 8 explains why).

### Three-way branches: `sign`

Setun's native jump is three-way. The `sign` statement branches on the
sign of an integer expression:

```trine
fn classify(a: int, b: int) -> int {
    sign (a <=> b) {
        neg:  return -1;
        zero: return 0;
        pos:  return 1;
    }
    return 99;
}

fn main() -> int {
    return classify(1, 2) * 100 + classify(2, 2) * 10 + classify(3, 2);
}
```

With `a <=> b` as the expression, `sign` compiles to one compare and one
`BR3`. Each arm is optional. `neg`, `zero` and `pos` are labels only here;
elsewhere they are ordinary names.

### `switch`

```trine
fn kind(c: char) -> int {
    switch (c) {
        case 'a', 'e', 'i', 'o', 'u': return 1;
        case ' ': return 2;
        default: return 0;
    }
    return -1;
}

fn main() -> int { return kind('e') * 10 + kind(' '); }
```

A `case` takes one or more constants. There is no fall-through: after an
arm's statements, control leaves the `switch`.

## 5. Pointers and arrays

```trine
import "rt.tri";

fn main() -> int {
    var a: [5]int = { 10, 20, 30 };     // a[3] and a[4] are 0
    var p: *int = &a[1];
    *p = 21;
    p = p + 2;                          // advances by 2 elements
    *p = 4;
    var buf: [8]char = "hi";
    return a[1] + a[3] + (p - &a[0]) * 1000 + strlen(&buf[0]) * 10000;
}
```

`*T` is a pointer to `T` and `[N]T` an array of `N` elements. Types read
left to right: `*[4]int` is a pointer to an array, `[4]*int` an array of
pointers. An array used as a value becomes a pointer to its first element.
Pointer arithmetic counts elements; the difference of two pointers of the
same type is a number of elements. `null` is the null pointer.

Sizes are counted in trytes: `sizeof(int)` is 3, `sizeof(tryte)` is 1.

## 6. Structs

```trine
struct Entry {
    tag: tryte;
    name: [4]tryte;
    id: int;
    next: *Entry;
}

var table: [3]Entry;

fn main() -> int {
    table[0].id = 77;
    table[1].next = &table[0];
    var e: *Entry = table[1].next;
    var local: Entry = { 'a', "bcd", 5, null };
    return e.id + local.id + sizeof(Entry);     // 77 + 5 + 12
}
```

There is no `->`: `.` looks through a pointer to a struct. Structs cannot
be assigned, passed or returned as a whole; copy them field by field, or
pass pointers. Section II.2.3 gives the layout rules (`sizeof(Entry)` is 12
here).

## 7. Strings and characters

A character literal is a Unicode code point of type `tryte`. A string
literal is a NUL-terminated array of trytes; its value is a `*tryte` to
the first character.

```trine
fn count(s: *char, c: char) -> int {
    var n: int = 0;
    while (*s != 0) {
        if (*s == c) { n += 1; }
        s = s + 1;
    }
    return n;
}

fn main() -> int {
    return count("Привет, мир", 'и');     // 2
}
```

Escapes are `\n \t \r \e \0 \\ \' \"` (`\e` is ESC, for terminal
sequences).

Two encodings go beyond one code point per tryte:

- **Overlined characters.** A character followed by U+0305 COMBINING
  OVERLINE is one tryte holding the negated code point. `'1̅'` is −49, and
  in `"Setun-11̅"` the last tryte is −49. This is how the machine writes
  negative digits.
- **Characters above U+2671** take three trytes in a string (ISA §8.4):
  `"日本"` is 6 trytes plus the NUL. A character literal cannot hold one;
  `'日'` is an error. tlibc's `tstr_next`, `tstr_chars` and `tchar_put`
  read and write such characters.

## 8. Ternary features

### Balanced literals

Besides decimal and hexadecimal, Trine reads balanced ternary (`0t`) and
balanced base 27 (`0z`). A negative digit is written with an overline
(U+0305); in base 3, `T` is an ASCII alias for 1̅:

```trine
fn main() -> int {
    var a: int = 0t1T0;         // 9 - 3 + 0 = 6
    var b: int = 0t11̅0;         // the same, with an overline
    var c: int = 0z1D̅;          // 27 - 13 = 14
    var d: tryte = 0zDDD;       // 9841, the largest tryte
    return a + b + c + d;
}
```

### Tritwise operators

`&` is the tritwise minimum and `|` the tritwise maximum, the ternary AND
and OR. `~x` negates every trit, which in balanced ternary is the same as
`-x`. There is no `^`: ternary has no single exclusive or, so the tritwise
product, threshold sum and consensus are builtins (`tmul`, `tsat`,
`cons`). Shifts count trits:

```trine
fn main() -> int {
    var a: int = 0t10T;
    var lo: int = a & 0tT11;        // 0tT0T = -10
    var hi: int = a | 0tT11;        // 0t111 = 13
    var x: int = 2 << 3;            // 2 * 27 = 54
    var y: int = 100;
    return lo + hi + x + (y >> 2);  // 100 >> 2 = 11: rounds 100/9
}
```

`x >> n` drops `n` trits, which rounds to the nearest integer rather than
toward minus infinity.

### Three-way comparison

`a <=> b` is a `trit`: −1, 0 or +1. It is computed exactly and is never
fooled by overflow.

### Kleene truth values

`true`, `unknown` and `false` are the trits +1, 0 and −1. With them `&`,
`|` and `~` are Kleene's three-valued AND, OR and NOT:

```trine
fn implies(a: trit, b: trit) -> trit { return ~a | b; }

fn main() -> int {
    var u: trit = unknown & true;   // unknown
    var f: trit = unknown & false;  // false
    var r: trit = implies(unknown, unknown);
    var n: int = 0;
    sign (r) {
        neg:  n = 1;
        zero: n = 2;                // taken: unknown implies unknown
        pos:  n = 3;
    }
    if (u == unknown && f == false) { n += 10; }
    return n;                       // 12
}
```

Conditions stay binary. A `trit` is refused as a condition, because tested
the binary way `false` (−1) would count as true. Compare it (`t ==
true`), branch three ways with `sign`, or convert it (`t as int`). For
binary truth there are `TRUE` (the `int` 1) and `FALSE` (the `int` 0).
The three Kleene names never become an `int` implicitly: `var done: int =
false;` is an error.

## 9. Reals

`real` is the FPU's real81: 81 trits, a 72-trit significand (about 34
decimal digits) and a power of 3 as the exponent.

```trine
import "console.tri";
import "real.tri";
import "math.tri";

fn show(x: real) {
    var b: [24]char;
    puts(rtoa(x, 12, &b[0]));
    puts("\n");
}

fn main() -> int {
    var a: real = 1.5;
    var b: real = a * 4 - 1;        // integers convert: 5
    show(sqrt(a * a + b * b));
    show(sin(pi() / 6));
    show(1.0 / 3);                  // exact in ternary
    return (b + 0.25) as int;       // rounds to nearest: 5
}
```

An integer converts to `real` implicitly. `x as int` rounds a real to the
nearest integer, ties to even. Division by zero and other exceptions do
not trap by default; they produce infinities or NaN and set flags in
`fsr()`.

## 10. Calling into assembly

`asm { ... }` pastes strings into the generated assembly. The usual
pattern defines a routine in an `asm` block, jumps around it, and declares
it with `extern fn`:

```trine
extern fn triple(x: int) -> int;

fn _asm_lib() {
    asm {
        "        b     triple_end"
        "triple: mul   r1, r1, 3"
        "        ret"
        "triple_end:"
    }
}

fn main() -> int { return triple(14); }     // 42
```

The routine follows the calling convention: arguments in `r1`…`r6`, the
result in `r1`. Global variables are visible to assembly by name.
Section II.9 has the full rules.

## 11. Building and running

```text
trine build FILE.tri [-o OUT.t1b2] [-S OUT.s] [--boot | --bios | --user]
                     [--no-opt | -O0..-O4] [--data N] [--stack N]
trine run   FILE.tri [--boot] [--steps N] [--input TEXT]
```

| Option | Effect |
|---|---|
| `build` | compile to an image; the default output is `FILE.t1b2`. A ROM image runs with `setun --rom OUT.t1b2` |
| `run` | compile and run on an emulated machine without a window: print the serial output, then how the program ended |
| `--boot` | a bootable one-sector disk image (`setun --disk OUT.t1b2`); `run --boot` boots it through the BIOS |
| `--bios` | the BIOS memory layout (RAM frame 1 left free for the boot sector) |
| `--user` | a Trinux program's layout; the output is its flat image, which `mkdisk` packages |
| `-S F` | also write the generated assembly to `F` (with `build` or `run`) |
| `-O0` … `-O4` | the optimization level; the default is `-O4` |
| `--no-opt` | the same as `-O0` (an explicit `-On` takes precedence) |
| `--data N`, `--stack N` | override the first global's word address and the initial stack pointer |
| `--steps N` | stop `run` after N instructions (default 100 000 000) |
| `--input TEXT` | feed TEXT to the serial console; `\n` in it becomes a newline |

`trine run` prints one of:

```text
[exit 42 after 42 instructions]
[CPU fault: vector -6 at -58872, address 0]
[still running after 100000000 instructions]
```

Its own exit status is `main`'s value clamped to 0…255, 101 after a CPU
fault, 1 after a compile error and 2 after a usage error.

Inside Trinux, `tric` compiles the same language with the same results
(Section II.11.3).

---

# Part II. Reference

## 1. Lexical structure

### 1.1 Source text

A source file is UTF-8 text, conventionally with the extension `.tri`.
Whitespace (any Unicode whitespace) separates tokens and is otherwise
ignored. Positions in messages count lines and characters from 1.

### 1.2 Comments

`//` starts a comment that runs to the end of the line. `/*` starts one
that runs to the next `*/`; block comments do not nest. An unclosed block
comment is an error: `unterminated /* comment`.

### 1.3 Identifiers

An identifier starts with a letter or `_` and continues with letters,
digits and `_`. "Letter" and "digit" are Unicode's: `счёт` is a valid
name. Case matters. A keyword cannot be an identifier.

Some names are refused for globals, functions and constants: `sp`, `lr`,
`fp`, `zero`, `r` followed by an integer (`r1`, `r-3`), and any name
starting with `_trine` (`` `sp` is a reserved name ``). They would read as
registers or collide with the startup code in the generated assembly.

The names of builtins (Section 6) are not reserved. A function, global or
local of the same name hides the builtin.

### 1.4 Keywords

```text
fn var const struct return if else while for loop break continue sign
switch case default as sizeof import extern interrupt asm true false
unknown TRUE FALSE null void trit tryte int char real
```

`neg`, `zero` and `pos` are contextual: they are `sign` arm labels when
followed by `:` inside `sign { }`, and ordinary identifiers everywhere
else. `trit` is a type keyword, but `trit(` starts a call of the builtin
`trit`.

### 1.5 Punctuation

```text
<<= >>= <=> -> << >> <= >= == != && || += -= *= /= %= &= |=
+ - * / % < > = ! ~ & | ? : ; , . ( ) [ ] { } @
```

The longest match wins. Any other character is an error: ``unexpected
character `…` ``.

### 1.6 Integer literals

| Form | Example | Value |
|---|---|---|
| decimal | `42`, `1_000` | 42, 1000 |
| balanced ternary | `0t1T0`, `0t11̅0` | 6 |
| balanced base 27 | `0z1D̅`, `0zdd` | 14, 364 |
| hexadecimal | `0x1F` | 31 |

- `_` may separate digits in every form.
- **`0t`**: the digits `1`, `0`, and `T` or `1̅` (a `1` followed by
  U+0305) for −1, most significant first. `T` and `1̅` may be mixed.
- **`0z`**: the digits `0`–`9` and `A`–`D` (either case) for 0…13; a
  negative digit is the positive one followed by U+0305.
- **`0x`**: plain hexadecimal. Nothing wraps; a negative value is written
  with unary minus (`-0x10`).
- After `0t`, `0z` or `0x` the whole run of letters, digits, `_` and
  overlines is the literal. A malformed one is an error that says what is
  wrong, for example `bad 0z literal: invalid ternary digit 'E'`, `'0'
  cannot be overlined`, `'1' has two overlines`, `bad 0t literal: empty
  ternary literal`, `invalid hexadecimal digit 'Z'`, `hexadecimal digits
  cannot be overlined`.
- A `0t` literal directly followed by `+` or `-` is refused (``a 0t literal
  followed by + or -: put a space before the operator``), because `+` and
  `-` were once base-3 digits. `0t1 - 1` and `0t1 -1` are fine.
- A letter right after a decimal number is an error: ``bad number `1x` ``.
- A literal that does not fit in a word is refused where it is used:
  `constant N does not fit in a word`. A constant must also fit the type it
  initialises (`19683 does not fit in tryte`).

### 1.7 Real literals

A real literal is digits, then a point and digits, or an exponent, or
both: `2.5`, `1e10`, `6.02e-23`, `1_000.5`. `1.` and `.5` are not real
literals. `_` may appear in the digits before the exponent but not in it.
The value is the decimal's exact value rounded once to the nearest
`real` (ties to even); a value beyond the range becomes ±∞ and a tiny one
a subnormal or 0. A letter right after the literal is an error (``bad
number `1.5x` ``).

### 1.8 Character literals

`'A'`, `'Ж'`, `'\n'`, `'1̅'`. The value is the code point, of type
`tryte`; an overlined character (followed by U+0305) is its negated code
point. Errors:

- `a character literal holds exactly one character` (`'ab'`, `''`);
- `a character above U+2671 takes three trytes: use a string` (`'日'`,
  also overlined);
- `unterminated literal` (a missing quote, or a newline inside);
- ``unknown escape `\q` ``.

### 1.9 String literals

`"text"`. Escapes as for characters:

| Escape | Value |
|---|---|
| `\n` | 10, newline |
| `\t` | 9, tab |
| `\r` | 13, carriage return |
| `\e` | 27, escape |
| `\0` | 0 |
| `\\` `\'` `\"` | the character |

A string is stored as trytes followed by a 0 tryte:

- a code point up to U+2671 (9841) is one tryte;
- a character followed by U+0305 is one tryte holding the negated code
  point (`Setun-11̅` ends in −49); an overline on a control character or a
  space is an error (`U+0020 cannot be overlined`), as are two overlines
  (`two overlines on one character`) and an overline with nothing before it;
- a character above U+2671 is three trytes: −2 (−3 when overlined), then
  two trytes in −9841…−33 (ISA §8.4). `"a日😀"` is 7 trytes plus the NUL.

A string cannot span lines. Identical strings in one program share
storage. Strings are placed with the code (in the ROM, for a ROM
program); treat them as read-only.

## 2. Types

### 2.1 The types

| Type | Size (trytes) | Values | Word-addressed |
|---|---|---|---|
| `trit` | 1 | −1, 0, +1 (stored in a tryte) | no |
| `tryte`, `char` | 1 | −9841 … 9841 | no |
| `int` | 3 | −3 812 798 742 493 … 3 812 798 742 493 | yes |
| `real` | 9 | real81 (Section 7) | yes |
| `*T` | 3 | an address | yes |
| `fn(A, B) -> R` | 3 | a function's address | yes |
| `[N]T` | N · sizeof(T) | N elements | as `T` |
| struct `S` | a multiple of 3 | fields | yes |
| `void` | 0 | none | |

- `char` is another name for `tryte`, not a separate type.
- `void` is allowed only as a return type (written by omitting `->`, or
  as `-> void`) and as a pointer target, `*void`. `a variable cannot be
  void`; `arrays of void are not allowed`.
- A function type without `->` returns `void`.
- An array length is a constant integer expression and must be positive
  (`array length must be positive, not 0`).
- `null` has a type of its own that converts to any pointer or function
  type.
- A type name that is not a struct is ``unknown type `Foo` ``.

### 2.2 Addresses and alignment

A word occupies three trytes and is addressed by its centre: the word at
`A` is the trytes `A−1`, `A`, `A+1`, and a word access needs `A` to be a
multiple of 3. Trine keeps this rule, so a pointer is always the hardware
address used to access the object.

| Object | Its address | Alignment |
|---|---|---|
| `trit`, `tryte` | the tryte | none |
| `int`, `real`, pointer, function pointer | the centre of its first word | a multiple of 3 |
| array of trytes or trits | its first tryte | the storage starts a word |
| array of word-addressed elements | the address of element 0 | a multiple of 3 |
| struct | the centre of its first word | a multiple of 3 |

The compiler allocates every global and every local in whole words, so a
tryte array also starts at the first tryte of a word.

`bytes(p)` gives the first tryte of a word-addressed object (`p − 1`, as a
`*tryte`), for code that copies memory tryte by tryte.

### 2.3 Struct layout

Fields are placed in declaration order, in word slots of three trytes:

- A word-addressed field (`int`, `real`, pointer, struct, array of those)
  starts a fresh slot.
- A tryte-sized field (`trit`, `tryte`, an array of them) takes the next
  free tryte; a tryte array can run across slots.
- The size is rounded up to whole slots, and is at least 3.

A field's offset is counted from the struct's address (the centre of its
first slot), so the first tryte of the struct is at offset −1:

```text
struct Entry { tag: tryte; name: [4]tryte; id: int; }

slot:      0             1             2
offset:   -1   0  +1  |  2   3   4  |  5   6   7
field:    tag n0  n1  | n2  n3  pad | id id  id      sizeof = 9, id at offset 6
```

Errors: ``struct `S` contains itself`` (a field of the struct's own type,
directly or through other structs; a pointer is fine), ``field `x`
appears twice``, ``field `x` cannot be void``, ``struct `S` is defined
twice``.

Struct layout is computed before any constant is declared, so an array
length inside a struct must be a literal expression or use `sizeof`; a
named `const` there gives `a constant integer expression is required
here`.

### 2.4 Truth types

- **Binary truth** is an `int`: comparisons, `!`, `&&` and `||` give 1 or
  0, and `TRUE` and `FALSE` are the `int` constants 1 and 0.
- **Kleene truth** is a `trit`: `true` +1, `unknown` 0, `false` −1.
- A condition (`if`, `while`, `for`, `?:`, `!`, `&&`, `||`) takes an
  integer, a pointer, a function pointer or a real, and holds when the
  value is non-zero (a NaN counts as non-zero). A `trit` is refused, and so
  is a `<=>` expression (Section 4.3).

### 2.5 Conversions

**Implicit conversions** happen where a value is assigned, initialises a
variable, is passed as an argument, or is returned. The rule
(`check_assign`) accepts:

| To | From |
|---|---|
| the same type | always (an array first becomes a pointer to its element) |
| `int` | `trit`, `tryte` |
| `tryte` | `trit`; an integer constant in −9841…9841 |
| `trit` | an integer constant in −1…1 |
| a pointer or function type | `null` |
| `*T` | `*void`, and `*void` from any pointer |
| `real` | any integer (where a real is expected) |

Everything else is an error:

- `expected tryte, found int (use `as` to narrow)` for an integer that
  does not fit implicitly;
- `expected int, found real (use `as` to convert)` for a real where an
  integer is expected;
- `expected *int, found int` and similar for other mismatches;
- `expected a real, found *int` where a real is expected.

A Kleene expression (built only from `true`, `unknown` and `false` with
`~`, `&`, `|` and parentheses) is refused where an `int` or `tryte` is
expected: `expected int, found a truth value (a trit): binary truth is
`TRUE`/`FALSE`, or convert with `as``. A `trit` variable still
widens to `int` implicitly.

**Explicit conversions** use `as`:

| From | To | Result |
|---|---|---|
| integer | `tryte` | the low 9 trits (balanced wrapping: `9842 as tryte` is −9841) |
| integer | `trit` | the lowest trit |
| integer | `int` | the value |
| integer, pointer, function | pointer or function type | the same address |
| pointer, function | integer | the address (narrowed as above for `tryte`/`trit`) |
| integer | `real` | exact |
| `real` | integer | rounded to nearest, ties to even; out of range or NaN saturates and raises IV (Section 7); then narrowed |

Other conversions are ``cannot convert *real to real`` and the like. Use
`sgn(x)` for a sign, not `x as trit`.

## 3. Expressions

### 3.1 Precedence

From the highest to the lowest:

| Level | Operators | Associativity |
|---|---|---|
| 1 | `f(args)` `a[i]` `s.f` | left |
| 2 | prefix `-x` `~x` `!x` `*p` `&x` | right |
| 3 | `x as T` | left |
| 4 | `*` `/` `%` | left |
| 5 | `+` `-` | left |
| 6 | `<<` `>>` | left |
| 7 | `&` | left |
| 8 | `\|` | left |
| 9 | `<=>` | left |
| 10 | `<` `<=` `>` `>=` | left |
| 11 | `==` `!=` | left |
| 12 | `&&` | left |
| 13 | `\|\|` | left |
| 14 | `c ? a : b` | right |
| 15 | `=` `+=` `-=` `*=` `/=` `%=` `<<=` `>>=` `&=` `\|=` | right |

- `as` binds after the prefix operators: `&x as int` is `(&x) as int` and
  `-x as tryte` is `(-x) as tryte`. It binds before the binary operators:
  `a * b as tryte` is `a * (b as tryte)`.
- `&` and `|` bind tighter than comparisons, unlike C: `x & 1 == 0` is
  `(x & 1) == 0`.
- `sizeof(T)` is a primary expression and takes a type.
- In `c ? a : b`, `a` may be any expression including an assignment; `b`
  is a conditional expression.

### 3.2 Evaluation order

Operands and function arguments are evaluated left to right. A plain
assignment evaluates its right side before the address of its left side;
a compound assignment computes the address first. `&&` and `||` evaluate
their right operand only when needed.

### 3.3 Arithmetic

`+ - * / %` on integers give an `int`, with these exceptions: `*` of two
`trit`s is a `trit`, and `-x` of a `trit` is a `trit`.

- **Overflow wraps** modulo 3^27, balanced (the result stays in the word
  range). It never traps.
- **`/`** truncates toward zero and **`%`** has the sign of the dividend
  (C rules). `divr(a, b)` rounds to nearest and `modr(a, b)` gives the
  balanced remainder.
- Division by a constant zero is a compile error (`division by zero`);
  division by zero at run time raises CPU vector −6.
- **Unary `-`** is exact (there is no most-negative value). **`~x` is the
  same operation as `-x`**: flipping every trit negates the number.
- Mixing a `real` into `+ - * / %` makes the operation real (Section 7).
- Any other operand type gives `cannot apply Add to int and *int` (the
  operator is named by its internal name: `Add`, `Sub`, `Mul`, `Div`,
  `Mod`, `Shl`, `Shr`, `Min`, `Max`).

### 3.4 Pointer arithmetic

- `p + n`, `n + p` and `p - n`, with `p` a `*T` and `n` an integer, move by
  `n · sizeof(T)` trytes and keep the type `*T`.
- `p - q` of two pointers of the same type is the number of elements
  between them, an `int`; pointers of different types give ``cannot subtract
  *tryte from *int``.
- Pointer arithmetic wraps like integer arithmetic.

### 3.5 Tritwise operators and shifts

| Operator | Meaning |
|---|---|
| `a & b` | per trit, the minimum (ternary AND) |
| `a \| b` | per trit, the maximum (ternary OR) |
| `~a` | per trit, the negation; equal to `-a` |
| `a << n` | `a · 3^n` |
| `a >> n` | `a` with `n` trits dropped: `a / 3^n` rounded to nearest |

`&` and `|` of two `trit`s are `trit`s; otherwise the result is an `int`.
On reals, `&` and `|` are the minimum and maximum (Section 7). Shifts take
integers only.

### 3.6 Comparisons

- `<`, `<=`, `>`, `>=`, `==` and `!=` give the `int` 1 or 0.
- `a <=> b` gives the `trit` −1, 0 or +1: the sign of `a − b`, computed
  exactly.
- Allowed operands: two integers, two pointers (of any pointee types), two
  function values or `null`, a pointer and the constant 0, or reals and
  integers mixed (Section 7). Otherwise ``cannot compare int with *int`` or
  ``cannot compare *int with a number``.

### 3.7 Logical operators

`!x`, `a && b` and `a || b` take conditions (Section 2.4) and give the
`int` 1 or 0. `&&` and `||` short-circuit. A `trit` operand is refused:
`a trit is not a condition (`false` is −1, which would hold): use `sign
(t) { neg: ... zero: ... pos: ... }`, or compare, as in `t == true``.

### 3.8 Conditional expression

`c ? a : b` evaluates `c` as a condition, then one of `a` and `b`. Its type:

- if either branch is a `real`, `real` (an integer branch is converted);
- if both have the same type, that type;
- if both are integers, `int`;
- a pointer and `null`, the pointer type;
- otherwise ``the branches have different types: int and null``.

### 3.9 Assignment

`lhs = rhs` stores `rhs` into the variable, element, field or `*p` named by
`lhs`, after the implicit conversion of Section 2.5, and has the stored
value. Assignment is an expression and associates to the right.

- Only scalars and reals can be assigned: ``cannot assign a whole [3]int;
  copy it field by field``.
- An expression without an address cannot be assigned: `this expression
  has no address (it is not a variable, element or field)`.

`lhs op= rhs` (`+= -= *= /= %= <<= >>= &= |=`) applies `op` to the current
value and `rhs`, and stores the result. On a `real`, only `+= -= *= /= %=
&= |=` are allowed (``cannot apply Shl to a real``).

On a `tryte` or `trit` variable, compound assignment narrows the result as
`as` would, without needing `as`, at every optimization level: a `tryte`
keeps the low 9 trits (`t += 1` on a `tryte` holding 9841 gives −9841), a
`trit` its low trit (`k += 1` on a `trit` holding 1 gives −1, since 2 is
1·3 − 1). The right side may be any integer expression (`t += x` with `x`
an `int`).

### 3.10 Address, dereference, indexing, fields

- `&x` is the address of a variable, element or field, of type `*T`. `&f`
  of a function is the function value itself, of type `fn(...) -> R`.
- `*p` is the object `p` points to. ``cannot dereference a *void``;
  ``cannot dereference int``.
- `a[i]` is element `i` of an array or of the memory a pointer points to;
  `i` must be an integer. There is no bounds check. ``cannot index int``;
  ``cannot index a *void``.
- `s.f` is field `f` of a struct, or of the struct a pointer points to (one
  level). ``int has no fields``; ``struct P has no field `z` ``.
- An array or struct used as a value evaluates to its address; an array
  then has the type of a pointer to its element.

### 3.11 `sizeof`

`sizeof(T)` is the size of type `T` in trytes, an `int` constant. It
takes a type, not an expression.

### 3.12 Calls

`f(args)` calls a function, a function value, or a pointer to a function.
The number of arguments must match (`expected 2 argument(s), found 1`),
and each argument converts to its parameter's type by the rule of Section
2.5. ``cannot call int``. A call to a builtin is described in Section 6.

### 3.13 Constant folding

These expressions are evaluated at compile time when every operand is a
constant: integer, character and truth literals, named integer constants,
unary `-` and `~` and `!`, binary `+ - * / % << < <= > >= == != <=>`,
`as` to `int`, `tryte`, `trit` or a pointer type, and `sizeof`. A folded
value outside the word range is an error (`constant 3812798742494 does
not fit in a word`) rather than wrapping. A constant integer expression is
required for array lengths, `const` initialisers, `case` values,
attributes and some builtin arguments; elsewhere a non-constant gives `a
constant integer expression is required here`.

The type of a folded constant is `tryte` for a character literal, the
declared type for a named constant, the target type for a cast, `trit`
for `<=>` and Kleene expressions, and `int` otherwise (`'a' + 1` is an
`int`).

## 4. Statements

### 4.1 Blocks and scope

`{ ... }` is a block. A local's scope runs from its declaration to the end
of the enclosing block. The body of `if`, `while`, `for`, `loop`, each
`sign` arm and each `switch` arm is a scope of its own. A name may be
declared again in an inner block, hiding the outer one; declaring it twice
in one block is ``a is already declared in this block``. Locals hide
globals.

### 4.2 Variable declarations

```text
var NAME: TYPE;
var NAME: TYPE = INITIALISER;
```

A local's initialiser cannot see the variable it initialises. A local
without an initialiser has an unspecified value. Initialisers:

- an expression, for a scalar or real;
- a string literal, for an array of trytes: the characters, then zeros to
  the end (if the string fills the array exactly, there is no terminating
  zero); a longer string is ``string of 3 characters does not fit in
  [2]char``;
- `{ a, b, ... }`, for an array or a struct: element by element or field by
  field, nested for nested aggregates, with a trailing comma allowed;
  missing elements and fields are zero. Too many: ``3 initialisers for an
  array of 2``, ``3 initialisers for a struct with 2 fields``. A list for a
  scalar: ``a list cannot initialise int``; a scalar for an aggregate:
  ``[2]int must be initialised with { … }``.

`{ ... }` is only an initialiser; elsewhere it is ``a `{ … }` list is only
allowed as an initialiser``.

### 4.3 `if`

```text
if (CONDITION) STATEMENT [else STATEMENT]
```

`else` binds to the nearest `if`. The condition follows Section 2.4. A
`trit` is refused, and so is `if (a <=> b)`: ``<=>` gives a trit, which is
not a condition: use `a != b` (or `sign (a <=> b) { ... }`)``.

### 4.4 `while`, `loop`, `for`

```text
while (CONDITION) STATEMENT
loop STATEMENT
for ([var-declaration | expression ;] [CONDITION] ; [expression]) STATEMENT
```

- `loop` repeats forever; leave it with `break` or `return`.
- In `for`, the first part is a `var` declaration (scoped to the loop) or
  an expression, or empty. A missing condition is always true: `for (;;)`
  is an endless loop. `continue` in a `for` runs the step expression.

### 4.5 `sign`

```text
sign (EXPRESSION) {
    neg:  STATEMENTS
    zero: STATEMENTS
    pos:  STATEMENTS
}
```

Branches on the sign of an integer expression: the `neg` arm for a
negative value, `zero` for 0, `pos` for a positive one. Arms may come in
any order; each is optional and may appear once (`this arm appears
twice`). An arm runs to the next arm label or the closing brace; control
then leaves the statement.

When the expression is `a <=> b`, the operands are compared directly
(exactly, with reals allowed; an unordered real comparison takes the
`zero` arm). Otherwise the expression must be an integer (`expected an
integer, found real`). The statement compiles to a compare and one `BR3`.

### 4.6 `switch`

```text
switch (EXPRESSION) {
    case C1, C2: STATEMENTS
    case C3: STATEMENTS
    default: STATEMENTS
}
```

- The expression must be an integer.
- Each `case` lists one or more constant integer expressions. A value may
  appear once in the whole statement (`case 1 appears twice`); there may be
  one `default` (``two `default` arms``).
- The arm whose case matches runs; if none matches, `default` runs, if
  present. There is no fall-through.
- `break` leaves the `switch`; `continue` continues the enclosing loop.

### 4.7 `break`, `continue`, `return`

- `break` leaves the innermost loop or `switch` (``break` outside a loop or
  switch``).
- `continue` starts the next round of the innermost loop (``continue`
  outside a loop``).
- `return;` leaves a `void` function; `return e;` returns `e`, converted to
  the return type. Errors: `this function must return a int`, `a void
  function cannot return a value`. Reaching the end of a function without
  `return` returns an unspecified value.

### 4.8 `asm`

```text
asm { "line" "line" ... }
```

Each string is one line of assembly, pasted into the output verbatim.
Section 9.3 gives the rules.

### 4.9 Expression statements

Any expression followed by `;` is a statement; its value is discarded.

## 5. Declarations

### 5.1 Programs

A program is one compilation unit: the main file and everything it
imports, transitively. Top-level items are imports, global variables,
constants, structs, functions and extern declarations, in any order.
Struct names are known everywhere, and functions can be called before
their definition. Constants must be declared before they are used in other
declarations (an array length, another constant).

### 5.2 Functions

```text
[interrupt] fn NAME(P1: T1, P2: T2, ...) [-> RETURN] { BODY }
```

- A parameter is a scalar (an integer, a pointer, a function) or a `real`.
  ``parameter of type S must be passed as a pointer``.
- Without reals, any number of parameters is allowed: the first six travel
  in registers, the rest on the stack.
- With reals, at most 9 reals and 6 others: ``a function with real
  parameters takes at most 9 reals and 6 other parameters``.
- The return type is a scalar, a `real` or nothing. ``cannot return S by
  value; return a pointer``.
- A function may be declared with `extern fn` and defined later; the two
  must have the same type, and a name may have only one definition
  (`` `f` is already defined ``).
- Parameters are local variables of the function.

### 5.3 `main`

Every program defines `fn main()` (otherwise `no `fn main()` defined`).
It takes at most two integer or pointer parameters: ``main` takes at most
two parameters: (disk, ram) or (argc, argv)``.

| Program kind | `main`'s parameters | When `main` returns |
|---|---|---|
| ROM, BIOS, boot sector | `(disk, ram)`: what the BIOS passes in `r1`, `r2` (the boot disk and the RAM size in pages) | its value goes to `SYSCTL.DEBUG` after an exit tag, and the machine powers off |
| Trinux user program | `(argc, argv)`, set up by the kernel | its value is passed to the `exit` system call (93) |

If `main` is declared without a return type, the exit value is
unspecified.

### 5.4 Interrupt functions

```text
interrupt fn NAME() { BODY }
```

An interrupt function takes no parameters and returns nothing (`an
interrupt fn takes no parameters and returns nothing`). It saves `r1`…
`r10` and `r-10`…`r-13` on entry, keeps the usual frame (which saves `lr`
and `fp`) and the callee-saved registers it uses, and ends with `IRET`.
The f and v registers are not saved. Its address is `&name`, ready for a
vector table:

```trine
import "ports.tri";

@align(729) var itb: [243]int;
var ticks: int = 0;

interrupt fn on_timer() {
    ticks += 1;
    out(TPIC_EOI, in(TPIC_CLAIM));
}

fn main() -> int {
    itb[121 + 1] = &on_timer as int;        // vector +1 = IRQ 0; the table is centred
    mtc(ITB, &itb[121] as int);
    out(TPIC_UNMASK, IRQ_TIMER);
    out(TIMER_PERIOD, 1000);
    out(TIMER_CTRL, 1);
    mtc(MODE, mfc(MODE) + 3);               // IE trit := +1
    while (ticks < 5) { hlt(); }
    return ticks;
}
```

### 5.5 Global variables

```text
[@align(N)] [@at(ADDR)] var NAME: TYPE [= INITIALISER];
```

- Globals live in RAM at addresses the compiler assigns, in declaration
  order, from the layout's data address upward (Section 8).
- All globals are zeroed at startup. Then the initialisers run, in
  declaration order, as code in `_trine_init`. An initialiser is an
  ordinary expression and may call functions.
- `@align(N)` aligns the global's address (the centre of its first word)
  to a multiple of `N`, which must be a positive multiple of 3
  (`@align(4) must be a positive multiple of 3`).
- `@at(ADDR)` places the global at a fixed address without allocating
  storage, for memory-mapped devices. The address is used as written.
- Both take constant integer expressions. Attributes apply only to `var`
  declarations (``attributes apply to `var` declarations``); others are
  ``unknown attribute `@foo` ``.

### 5.6 Constants

```text
const NAME: TYPE = CONSTANT;
```

A constant has no storage. Allowed forms:

| Type | Initialiser |
|---|---|
| `int`, `tryte`, `trit`, pointer | a constant integer expression that fits the type |
| `*tryte` (`*char`) | a string literal |
| `real` | a real literal, an integer constant expression, another real constant, or one of these negated |

A constant of another type is ``a constant of type S is not supported``.
An integer constant used in an expression has its declared type.

### 5.7 Structs

```text
struct NAME { FIELD: TYPE; FIELD: TYPE; ... }
```

A struct may contain pointers to itself. Section 2.3 gives the layout.

### 5.8 Extern declarations

```text
extern fn NAME(PARAMS) [-> RETURN];
extern var NAME: TYPE;
```

They name a function or variable defined elsewhere: in an `asm` block, or
in the same program. The name is used as an assembler label; a missing
definition is reported by the assembler (``internal assembler error: line
49: undefined symbol `ticks` ``).

### 5.9 Imports

```text
import "FILE";
```

- An import is resolved against the directory of the importing file
  first, then against the standard library (`ports.tri`, `rt.tri`,
  `console.tri`, `keymap.tri`, `real.tri`, `math.tri`). Otherwise ``cannot
  find `nowhere.tri` ``. An absolute path is used as it is.
- Each file is loaded once, identified by its canonical path (a standard
  library file by its name); a second import of the same file does
  nothing.
- Imports load depth first: a file's own items follow everything it
  imports.
- All top-level names share one namespace. There is no privacy: by
  convention a library prefixes its internal names with `_` (as `math.tri`
  does), but nothing stops other files from using them, and two files
  defining the same name collide (`` `x` is already defined ``).

## 6. Builtins

Builtins look like function calls and compile to one instruction or a
few. They are not values (`&sgn` is an error), and a function, global or
local of the same name replaces them. "Integer" below means `int`,
`tryte` or `trit`. A wrong argument count gives `` `mrg` takes 3
argument(s), found 2 ``; a non-integer where an integer is needed gives
`expected an integer, found real`. The instructions are those of
`docs/SETUN26_ISA.md`.

### 6.1 Arithmetic and trit operations

| Builtin | Result | Meaning | Instruction |
|---|---|---|---|
| `sgn(x)` | `trit` | the sign of `x` | `SGN` |
| `abs(x)` | `int` | `\|x\|` (exact: there is no overflow) | `ABS` |
| `tlz(x)` | `int` | the number of leading zero trits, 0…27 | `TLZ` |
| `divr(a, b)` | `int` | `a / b` rounded to nearest | `DIVR` |
| `modr(a, b)` | `int` | the balanced remainder, `\|r\| ≤ \|b\|/2` | `MODR` |
| `mulh(a, b)` | `int` | the high 27 trits of the 54-trit product | `MULH` |
| `tmul(a, b)` | `int` | the tritwise product | `TMUL` |
| `tsat(a, b)` | `int` | the tritwise threshold sum, per trit sign(a + b) | `TSAT` |
| `cons(a, b)` | `int` | the tritwise consensus: per trit, `a` if `a = b`, else 0 | `CONS` |
| `trit(x, i)` | `trit` | trit `i` of `x`, 1 the most significant, 27 the lowest; `i` a constant in 1…27 (`trit position must be 1..27`) | `TRIT` |
| `ext(x, pos, len)` | `int` | trits `pos`…`pos+len−1` of `x`, as a balanced number; `pos`, `len` constants | `EXT` |
| `ins(x, v, pos, len)` | `int` | `x` with trits `pos`…`pos+len−1` replaced by the low `len` trits of `v`; `pos`, `len` constants | `INS` |

`ext` and `ins` fields are checked by the assembler (``trit field 0+3
outside 1..27``). `x as tryte` is `ext(x, 19, 9)` and `x as trit` is
`ext(x, 27, 1)`.

### 6.2 Binary operations

For data from binary systems: a word's value is read as a two's-complement
integer of unlimited width (ISA §3.3.1).

| Builtin | Result | Meaning | Instruction |
|---|---|---|---|
| `band(a, b)` | `int` | binary AND | `BAND` |
| `bor(a, b)` | `int` | binary OR | `BOR` |
| `bxor(a, b)` | `int` | binary exclusive OR | `BXOR` |
| `bandn(a, b)` | `int` | `a` AND NOT `b` | `BANDN` |
| `bsh(x, n)` | `int` | `x · 2^n`, or `⌊x / 2^−n⌋` for `n < 0` | `BSH` |
| `bext(x, pos, len)` | `int` | bits `pos`…`pos+len−1` of `x` (bit 0 the lowest), as 0…2^len−1 | `BEXT` |
| `brot(x, n, len)` | `int` | the low `len` bits of `x`, rotated left by `n` | `BROT` |

`bext` and `brot` take constant `pos`/`n` and `len`: for `bext`, `pos` in
0…80; for `brot`, `n` in 0…len−1; `len` in 1…41 (``bext` needs a bit
position in 0..80 and a length in 1..41``).

### 6.3 Ternary idioms

| Builtin | Result | Meaning | Instruction |
|---|---|---|---|
| `mrg(old, mask, src)` | `int` | per trit: mask 0 keeps `old`'s trit, +1 takes `src`'s, −1 gives 0 | `MRG` |
| `stwm(p, value, mask)` | `void` | `*p := mrg(*p, mask, value)` as one atomic store; `p` is an `*int` | `STWM` |
| `tcp(x, n)` | `int` | the number of +1 trits among the low `n` trits of `x` (`n` = 0: all 27) | `TCP` |
| `tcn(x, n)` | `int` | the number of −1 trits among the low `n` trits (`n` = 0: all 27) | `TCN` |
| `trev(x, n)` | `int` | the low `n` trits of `x` reversed, the trits above them 0 (`n` = 0: all 27) | `TREV` |
| `cas3(p, expected, new)` | `int` | the old `*p`; `*p := new` if it equalled `expected`; atomic; `p` is an `*int` | `CAS3` |
| `swap(p, v)` | `int` | the old `*p`, and `*p := v`; atomic; `p` is an `*int` | `SWAP` |

- `stwm`, `cas3` and `swap` need a pointer to an `int`: `` `cas3` needs a
  pointer to an int ``. The address must be word-aligned (else vector −5)
  and the page writable (else vector −4).
- `tcp`, `tcn` and `trev` with `n` outside 0…27 raise an illegal-instruction
  fault at run time.

### 6.4 I/O and control

| Builtin | Result | Meaning | Instruction |
|---|---|---|---|
| `in(port)` | `int` | read a device port | `IN` (immediate port when `port` is constant, else register) |
| `out(port, v)` | `void` | write a device port | `OUT` |
| `mfc(REG)` | `int` | read a control register | `MFC` |
| `mtc(REG, v)` | `void` | write a control register; `v` a word value | `MTC` |
| `hlt()` | `void` | halt until an interrupt | `HLT` |
| `brk()` | `void` | breakpoint trap | `BRK` |
| `tlbflush()` | `void` | invalidate all non-global translations | `TLBFLUSH` |
| `tlbinv(a)` | `void` | invalidate the translation of address `a` | `TLBINV` |
| `fence()` | `void` | memory fence | `FENCE` |
| `syscall(n, a1, …, a6)` | `int` | system call `n` (a constant) with up to 6 arguments in `r1`…`r6`; the result from `r1` | `SYS n` |
| `bytes(p)` | `*tryte` | the first tryte of the object `p` points to: `p − 1` | `SUB` |

- `REG` is a control register's name, in either case: `MODE`, `PTB`,
  `FADDR`, `FCAUSE`, `ITB`, `EPC`, `EMODE`, `KSP`, `SCRATCH`, `CPUID`,
  `CYCLE`. Others: ``unknown control register `FOO`; one of MODE, PTB, …``.
- `MFC`, `MTC`, `HLT`, `TLBINV` and `TLBFLUSH` are privileged (ring +1).
- `ports.tri` names every device port (Section 10).
- `syscall` without a number or with more than 6 arguments: `` `syscall`
  takes a constant number and up to 6 arguments ``.

### 6.5 Reals

| Builtin | Result | Meaning | Instruction |
|---|---|---|---|
| `abs(x)` | `real` | `\|x\|`, for a real `x` | `FABS` |
| `sqrt(x)` | `real` | square root | `FSQRT` |
| `trunc(x)` | `real` | rounded toward zero to an integral value | `FTRUNC` |
| `round(x)` | `real` | rounded to nearest, ties to even | `FRND` |
| `rem(a, b)` | `real` | the IEEE remainder, the same as `a % b` | `FREM` |
| `scale3(x, k)` | `real` | `x · 3^k`, `k` an integer (exact unless it overflows or underflows) | `FSCL` |
| `exponent3(x)` | `int` | the exponent E of `x` normalized (0 for zero) | `FEXP` |
| `fclass(x)` | `int` | 0 zero, ±1 subnormal, ±2 normal, ±3 infinite, 4 NaN (signed like `x`) | `FCLASS` |
| `real27(x)` | `real` | `x` rounded to real27 precision (18 trits) | `FTOW`, `FFRW` |
| `fconst(k)` | `real` | constant `k` (0…6): 0, 1, π, e, ln 3, log₃ e, log₃ 10 | `FCONST` |
| `fsr()` | `int` | the FPU status register | `FMFS` |
| `set_fsr(v)` | `void` | write the FPU status register | `FMTS` |

Integer arguments convert to `real`. `fconst` needs a constant in range
(`fconst takes 0..6: zero, one, pi, e, ln 3, log3 e, log3 10`).

### 6.6 Vectors

The vector builtins work memory to memory on 81-trit vectors: 9 tryte
lanes, or 3 word lanes for the names ending in `w`.

| Builtin | Result | Meaning | Instructions |
|---|---|---|---|
| `vadd(d, a, b)` `vsub` `vmin` `vmax` `vtmul` `vtsat` `vcons` | `void` | `d := a ⊕ b` on each tryte lane | `VLD`, `VALU`, `VST` |
| `vaddw(d, a, b)` `vsubw` `vminw` `vmaxw` `vtmulw` `vtsatw` `vconsw` | `void` | the same on word lanes | `VLD`, `VALU`, `VST` |
| `vneg(d, a)` `vabs` `vsgn` | `void` | `d :=` the operation on each tryte lane of `a` | `VLD`, `VALU`, `VST` |
| `vnegw(d, a)` `vabsw` `vsgnw` | `void` | the same on word lanes | `VLD`, `VALU`, `VST` |
| `vmrg(d, mask, src)` | `void` | `d := mrg(d, mask, src)` over all 81 trits | `VLD`, `VMRG`, `VST` |
| `vbcst(d, x)` | `void` | every tryte lane of `d` := `x` | `VBCST`, `VST` |
| `vbcstw(d, x)` | `void` | every word lane of `d` := `x` | `VBCSTW`, `VST` |
| `vsum(a)` | `int` | the sum of `a`'s tryte lanes | `VLD`, `VSUM` |
| `vdotw(a, w)` | `int` | Σ `a[j]` × the lowest trit of `w[j]`, over the tryte lanes | `VLD`, `VDOTW` |

**The alignment rule.** Each vector argument is a pointer to `int` or to
`tryte` (an array of either decays to one), otherwise `` `vadd` needs a
pointer to a vector ([3]int or [9]tryte) ``. The vector's address, the
centre of its first word, must be a multiple of 9, or the access traps
(vector −5). For an `*int` that is the pointer itself; for a `*tryte` it
is the pointer plus 1, which the compiler adds. Declare vectors
`@align(9)`: a `[3]int` or `[9]tryte` global so declared is a valid
vector. Locals are only word-aligned, so vectors are globals or heap
memory. The builtins use `v1`…`v3`.

```trine
@align(9) var a: [9]tryte = { 1, -2, 3, 9841, -9841, 0, 100, -100, 13 };
@align(9) var d: [9]tryte;
@align(9) var w: [3]int;

fn main() -> int {
    vabs(d, a);                 // d[j] = abs(a[j])
    vbcstw(w, 5);               // w = { 5, 5, 5 }
    return vsum(d) + w[2];
}
```

## 7. Reals

### 7.1 Format

`real` is the FPU's real81 (ISA §14): three words, holding a value
`m · 3^E`. The exponent `E` is one tryte (−9841…9841); the significand is
72 trits and carries the sign, so there is no −0. The largest finite value
is just under ½ · 3^9840 (about 10^4694); there are subnormals, ±∞ and
NaN. All NaNs are quiet.

### 7.2 Literals and constants

Real literals are written in decimal (Section 1.7) and rounded once to
the nearest real81, ties to even. Powers of 3 and their fractions are
exact (`1.0 / 3`), and 0.1 is not. `const` reals are allowed (Section
5.6), and real globals take any initialiser expression.

### 7.3 Operations

- `+ - * / %` with a real operand are real operations; an integer operand
  converts exactly. `%` is the IEEE remainder `a − b · q`, `q` the quotient
  rounded to nearest (ties to even), not C's `fmod`: `10 % 3.0` is 1 and
  `7.0 % 2` is −1.
- `|` and `&` on reals are the maximum and minimum (a NaN operand loses to
  a number).
- Unary `-` negates exactly.
- `<<`, `>>`, `sign` on a plain real, `switch` on a real and the integer
  builtins are refused.
- Every result is rounded to nearest. A result that had to be rounded
  sets IX.

### 7.4 Comparisons and NaN

The comparisons compare reals with reals and integers. A comparison with a
NaN is unordered: `==`, `<`, `<=`, `>`, `>=` are false and `!=` is true.
`a <=> b` and `sign (a <=> b)` give 0 for an unordered pair. As a
condition, a real is true when non-zero, so a NaN is true.

### 7.5 Exceptions and `fsr`

The FPU signals invalid (IV), divide by zero (DZ), overflow (OF),
underflow (UF) and inexact (IX). All are masked at reset, so an exception
produces its default result (NaN, ±∞, a subnormal or 0, the rounded
value) and sets its sticky flag in the FPU status register:

| FSR trits | Field |
|---|---|
| 23–27 | flags `IV DZ OF UF IX` (+1 once raised; cleared only by writing FSR) |
| 14–18 | masks `IV DZ OF UF IX` (+1 masked; 0 makes it trap to vector −11) |

`trit(fsr(), 24)` reads the DZ flag; `set_fsr(v)` writes the register.

### 7.6 Conversions

- integer → `real`: implicit and exact.
- `real` → integer: `as int` (or `as tryte`, `as trit`), rounded to
  nearest with ties to even; a NaN gives 0 and an out-of-range value the
  largest word of its sign, both raising IV. `as tryte` and `as trit` then
  keep the low trits.
- `real27(x)` rounds to the one-word format's precision and back.
- Pointers and reals do not convert.

### 7.7 Reals and the ABI

Real parameters travel in `f1`…`f9` and a real result in `f1`; a call
with real arguments takes at most 9 reals and 6 other arguments (`a call
with real arguments takes at most 9 reals and 6 other arguments`).

## 8. Program kinds and memory layout

### 8.1 Layouts

Addresses are word addresses. A RAM frame (page) is 3^9 trytes; frame 0
covers −9841…9841 and frame 1 9842…29524.

| Kind | Option | Code (`.org`) | Globals from | Initial stack |
|---|---|---|---|---|
| ROM | (default) | −59 049 (the reset vector, −3^10) | 9843 (frame 1) | 9840 (frame 0, growing down) |
| BIOS | `--bios` | −59 049 | 49 209 (frame 3) | 49 206 (frame 2) |
| boot sector | `--boot` | 19 683 (the centre of frame 1) | 49 209 | 49 206 |
| Trinux user program | `--user` | −847 288 609 443 (−3^25) | −847 288 078 002 (27 pages higher) | set by the kernel |

- **ROM**: the image is a full ROM (3^10 trytes). Code, string literals and
  real constants live in the ROM; globals are in RAM.
- **BIOS**: like ROM, with frame 1 left free for the boot sector.
- **Boot sector**: the image is one sector (3^9 trytes) holding the program
  at its load address, with the boot signature in its last word (address
  29 523). The BIOS loads it into frame 1 and jumps to 19 683. A program
  that does not fit is refused: `the boot program (N words at A) does not
  fit in the boot sector (addresses 9842..29521)`.
- **User**: the image is the program's words from `org − 1`; `mkdisk` (on
  the host) or `tas` (in Trinux) wraps it into a Trinux executable.
- `--data` and `--stack` override the globals' start and the initial
  stack.

### 8.2 Startup code

The compiler adds startup code, `crt0`, in front of the program.

For ROM, BIOS and boot programs it:

1. saves `r1` and `r2` (the boot arguments) for `main`;
2. sets `sp`;
3. installs a vector table in which every vector reports a CPU fault;
4. zeroes the globals;
5. calls `_trine_init`, which runs the global initialisers;
6. calls `main(r1, r2)`;
7. writes the exit tag and `main`'s value to `SYSCTL.DEBUG` (port 3) and
   powers the machine off.

A fault writes the fault tag, the vector, `EPC` and `FADDR` to
`SYSCTL.DEBUG` and powers off; `trine run` reports it as a CPU fault. A
program that handles interrupts installs its own vector table (Section
5.4).

For a Trinux user program, crt0 uses no privileged instruction: it saves
`argc`, `argv` and `envp` (the kernel sets them and `sp`), zeroes the
globals, runs `_trine_init`, stores `envp` into the global `environ` if
the program declares one (tlibc does), calls `main(argc, argv)`, and
passes the result to system call 93, `exit`.

### 8.3 Frames

The stack grows toward negative addresses and stays word-aligned. A
function's frame holds the saved `lr` and caller's `fp` above `fp`, `fp`
pointing at the saved `fp`, and locals and saved registers below it.
Locals are allocated in whole words.

## 9. Calling convention and inline assembly

### 9.1 Registers

| Register | Role |
|---|---|
| `r0` | always zero |
| `r1`…`r6` | integer arguments, left to right; `r1` the result |
| `r1`…`r10`, `r-10`…`r-13` | caller-saved |
| `r-1`…`r-9` | callee-saved; compiled code keeps locals in them |
| `r11` (`fp`) | frame pointer, callee-saved |
| `r12` (`lr`) | return address, set by `CALL`/`JAL` |
| `r13` (`sp`) | stack pointer, callee-saved |
| `f1`…`f9` | real arguments, in order among the reals; `f1` the real result |
| `f1`…`f13`, `f-1`…`f-13` | caller-saved |
| `v1`…`v3` | used by the vector builtins |

### 9.2 Arguments and results

- Integer, pointer and function arguments take `r1`…`r6` in order. More
  are passed on the stack: the seventh at the lowest address, just above
  the return address (`[fp+6]` in the callee, the eighth at `[fp+9]`, and
  so on). The caller removes them.
- Reals take `f1`…`f9` in order among the reals, while the other
  arguments take `r1`…`r6` in order among themselves. A function with real
  parameters has no stack arguments.
- Structs and arrays are passed as pointers.
- The result is in `r1`, or `f1` for a real.

This is the ISA's suggested ABI, so Trine and assembly can call each
other in both directions. Assembly called from Trine must preserve
`r-1`…`r-9`, `fp` and `sp`.

### 9.3 Inline assembly

- `asm { "…" "…" }` pastes each string as one line of the output,
  unchanged. Lines need no indentation; the assembler syntax is in
  `docs/ASSEMBLER.md`.
- Trine locals and parameters are not visible by name. Global variables
  are: each global is an assembler constant holding its address, so
  `ldw r1, [ticks]` reads `ticks`. Functions are labels.
- Labels defined in an `asm` block can be named by `extern fn` or `extern
  var` declarations. Avoid the compiler's label names (`.L…`, `.Lstr…`,
  `.Lreal…`) and `_trine…`.
- A function containing `asm` keeps all its locals in memory, uses the
  plain frame, and is not touched by the optimizer; it is never inlined,
  and is always kept in the output (it may define labels).
- The compiler does not check or analyse the strings. The code must leave
  callee-saved registers, `fp` and `sp` as it found them.

## 10. The standard library

The six modules are embedded in the compiler and found by `import`
without a path (Section 5.9). In Trinux they are in `/usr/lib/trine`.

### 10.1 `rt.tri`: memory and strings

| Function | Description |
|---|---|
| `memset(p: *tryte, value: tryte, n: int)` | set `n` trytes to `value` (a word at a time from 18 trytes on) |
| `memcpy(dst: *tryte, src: *tryte, n: int)` | copy `n` trytes (by words when both ends sit alike in their words) |
| `strlen(s: *char) -> int` | the number of trytes before the 0 |
| `strcmp(a: *char, b: *char) -> trit` | three-way comparison: −1, 0 or +1 |
| `itoa(v: int, buf: *char) -> *char` | decimal text of `v` into `buf` (at least 15 trytes); returns `buf` |
| `b27_digit(d: int) -> char` | a base-27 digit −13…13 as a character (a negative one overlined) |
| `itoa27(v: int, buf: *char) -> *char` | `v` as 9 base-27 digits plus a 0 (10 trytes) |
| `itoa3(v: int, n: int, buf: *char) -> *char` | the low `n` trits of `v` (n ≤ 27) as `1`, `0`, `1̅`; `buf` holds n + 1 |

### 10.2 `console.tri`: serial console and text screen

Imports `ports.tri` and `rt.tri`.

| Item | Description |
|---|---|
| `putc(c: char)` | write a character to the serial port, waiting until it is ready |
| `puts(s: *char)` | write a string to the serial port |
| `put_int(v: int)` | write `v` in decimal to the serial port |
| `getc() -> char` | the next character from the serial console, or 0 if none is waiting |
| `const COLS: int = 80`, `const ROWS: int = 30` | the text screen's size |
| `attr(fr: trit, fg: trit, fb: trit, br: trit, bg: trit, bb: trit) -> tryte` | an attribute tryte from foreground and background R, G, B trits |
| `var screen_row: int`, `var screen_col: int` | the cursor position |
| `var screen_attr: tryte` | the current attribute (initially white on black, `0t111TTT000`) |
| `cell(i: int) -> *tryte` | the framebuffer cell `i` (character, then attribute) |
| `screen_clear()` | select text mode, clear the screen, home the cursor |
| `screen_scroll()` | scroll the screen up one row |
| `screen_putc(c: char)` | write a character at the cursor (newline, wrapping and scrolling) |
| `screen_puts(s: *char)` | write a string to the screen |
| `print(s: *char)` | write to both the serial port and the screen |
| `print_int(v: int)` | `v` in decimal, to both |

### 10.3 `real.tri`: decimal text for reals

| Function | Description |
|---|---|
| `rtoa(x: real, digits: int, buf: *char) -> *char` | `x` with up to `digits` (1…12) significant digits, like `%g`: `3.14159`, `1e+100`, `inf`, `nan`; `buf` holds 24 trytes |
| `strtor(s: *char, end: **char) -> real` | parse `[-]d[.d][e[-]d]` after leading blanks; `*end` is set to the first character not used, if `end` is not null |
| `atof(s: *char) -> real` | `strtor(s, null)` |
| `pow10(k: int) -> real` | 10^k for any integer `k` |
| `floor(x: real) -> real` | rounded down to an integer |
| `ceil(x: real) -> real` | rounded up to an integer |
| `rtoa_put(buf: *char, at: int, s: *char) -> int` | helper of `rtoa`: copy `s` into `buf` at `at`, return the new position |

### 10.4 `math.tri`: elementary functions

Imports `real.tri`. Accurate to about 30 significant digits for ordinary
arguments. Reductions work in powers of 3 (`exponent3`, `scale3`), so
`log3` of a power of 3 is exact. Special values follow C: `ln(0)` is −∞
(DZ), `ln(-1)` is NaN (IV), a large `exp` overflows to ∞ (OF), and a NaN
passes through.

| Function | Description |
|---|---|
| `exp(x)`, `ln(x)`, `log3(x)`, `log10(x)` | exponential and logarithms |
| `pow(x, y)` | `x^y` |
| `sin(x)`, `cos(x)`, `tan(x)` | trigonometric functions |
| `asin(x)`, `acos(x)`, `atan(x)`, `atan2(y, x)` | inverse trigonometric functions |
| `sinh(x)`, `cosh(x)`, `tanh(x)` | hyperbolic functions |
| `hypot(x, y)` | `√(x² + y²)` |
| `pi()`, `e()` | the constants, from `fconst` |

All take and return `real`. The names starting with `_` (`_math_nan`,
`_fabs`, `_ln_mant`, `_ipow`, `_reduce`, …) are internal.

### 10.5 `ports.tri`: device registers

Named `int` constants for every device port of ISA §8 (port = 27 · block
+ register): `SYSCTL_*`, `TPIC_*` (interrupt controller), `TIMER_*`,
`RTC_*`, `KBD_*`, `MOUSE_*`, `SERIAL_*`, `DISK_*` (disk 0; disk 1 is +27),
`DISPLAY_*`, `NIC_*`, `RNG_*`; the IRQ numbers `IRQ_TIMER` … `IRQ_NIC`
(vector = IRQ + 1); and `FB_BASE` (−1 604 164, the first framebuffer
tryte) and `PAGE` (19 683).

### 10.6 `keymap.tri`: keyboard layouts

USB HID usage codes to characters, in a US and a Russian (ЙЦУКЕН) layout.
Alt+Shift switches layouts, Caps Lock flips letter case, Ctrl+letter gives
a control code, navigation keys give VT100 sequences, and Right Alt with a
digit or letter types it overlined. Each program that imports it keeps its
own keyboard state (`km_shift`, `km_ctrl`, `km_alt`, `km_caps`,
`km_layout`, `km_altgr`, `km_seq`).

| Item | Description |
|---|---|
| `const KM_US: int = 0`, `const KM_RU: int = 1` | the layouts |
| `keymap_set_layout(l: int)` | select a layout |
| `keymap_layout_name() -> *char` | `"EN"` or `"RU"` |
| `keymap_event(ev: int) -> int` | a key event (+code press, −code release): the character it types, or 0 (then `km_seq` holds its escape sequence, if any) |
| `keymap_char(code: int) -> int` | the character key `code` types now, or 0 |
| `keymap_sequence(code: int) -> *char` | the escape sequence a key types, or null |
| `keymap_overline(c: int) -> int` | `c` overlined (a digit or letter), else `c` |
| `km_cyr(c: int, upper: int) -> int` | helper: a Cyrillic letter or its capital |

### 10.7 tlibc

Trinux programs use tlibc (`user/lib/tlibc.tri`, in Trinux
`/usr/src/lib/tlibc.tri`) and the other libraries in `user/lib`: system
call wrappers, stdio helpers, strings, memory allocation, signals and
more. tlibc imports `rt.tri`. It is described with Trinux in
`docs/TRINUX.md`.

## 11. The compilers

### 11.1 The host compiler, `trine`

`crates/trine` compiles a program in these stages:

1. **Loading** (`lib.rs`): the main file and its imports, depth first,
   each once.
2. **Lexer** (`lexer.rs`): tokens with positions.
3. **Parser** (`parser.rs`): recursive descent to a syntax tree.
4. **Checker and code generator** (`compile.rs`): one pass over the tree
   that checks types and writes Setun-11̅ assembly text. Struct names and
   layouts come first, then constants, function signatures and globals in
   order, then the function bodies, then `_trine_init`. Every expression
   leaves its value in `r1` (a real in `f1`).
5. **Peephole** (`peephole.rs`): rewrites each function's assembly.
6. **Startup code** (`crt0`) and, for the bare machine, the fault vector
   table are added.
7. **Assembler** (`setun-asm`): the text becomes an image (ROM, boot
   sector or flat).

The library API: `build_source`, `build_file`, `Options::rom()`,
`bios()`, `boot()`, `user()`, `Options::level`, and `run_rom` /
`run_machine` to run an image.

### 11.2 Optimization levels

The levels are cumulative. `-O4` is the default; `-O0` (or `--no-opt`)
gives the plain code. Every access to memory that has an address (a
global, anything through a pointer, a local whose address is taken)
happens as written and in order at every level, so there is no need for
`volatile`.

| Level | Adds |
|---|---|
| 1 | the peephole pass (copy forwarding, retargeting results, dead-code removal, local value numbering), which skips functions with inline assembly; `&&`, `\|\|` and `!` in conditions jump straight to their targets; `if (c) break;` and `if (c) continue;` as one jump; loops on cheap tests rotated (the test at the bottom) |
| 2 | `int` and pointer locals and parameters in registers (`r-1`…`r-9`, and in a leaf function also the caller-saved `r7`…`r10`, `r-10`…`r-13`) when their address is never taken; real locals in `f-1`…`f-9` in a leaf function; direct operands; loads and stores through simple addresses; compound assignment in place; a frame set up with one stack adjustment |
| 3 | left operands held in `r4`…`r6` and `f3`…`f9` instead of the stack; direct calls with up to six integer arguments passed straight in registers |
| 4 | inlining of small functions; functions not reachable from `main` left out |

Details:

- A local whose address is taken (`&x` anywhere in the function) stays in
  memory. A function with inline assembly keeps all locals in memory.
- **Inlining** (level 4) takes a function whose size, plus that of what it
  inlines in turn, is at most 60 (statements and expressions counted), with
  at most six parameters, no reals, no inline assembly, not an interrupt
  function and not recursive. Each function may grow by at most 200 this
  way. Outside loops only tiny bodies without locals are inlined. A leaf
  function (one whose calls are all builtins or inlinable) inlines all its
  calls.
- **Reachability** (level 4): the kept functions are those reachable from
  `main`, the interrupt functions, functions containing inline assembly,
  anything named by a global or constant initialiser or in inline
  assembly text, and what these name in turn. The others are still checked,
  then dropped.
- `crates/trine/tests/optimizer.rs` compiles programs at every level and
  requires the same results.

### 11.3 The self-hosted compiler, `tric`

`tric` (`user/bin/tric.tri` and `user/bin/tric/*.tri`) is the compiler
ported to Trine. It runs in Trinux and compiles the same language.

```text
tric [-S] [-v] [-O level] [-o OUT] FILE.tri
```

| Option | Effect |
|---|---|
| (none) | compile, then run `/bin/tas` to assemble a Trinux executable; the default output is `a.out` |
| `-S` | write the assembly instead; the default output is `a.s` |
| `-o OUT` | the output file |
| `-O0` … `-O4` | the optimization level; **the default is `-O0`** |
| `-v` | report the program's size (expressions, statements, items, names, code trytes) and the heap the compile took |

- `tric` writes the same assembly as `trine build --user` at the same
  level, character for character: the same instructions, label numbers,
  registers, pools and global equates. The tests compare them
  (`crates/trinux/tests/selfhost.rs`).
- Its errors are the host's, at the same line and column; it exits 1 and
  leaves no output file.
- Imports resolve as on the host: next to the importing file, then the
  standard library in `/usr/lib/trine`.
- `tric` builds itself to a fixed point (`docs/SELFHOST.md`).
- One difference: `tric` refuses a number that does not fit in a word
  when it reads it; the host reads it and refuses it later.
- While assembling, `tric` writes the assembly to `OUT.tmp.s` beside the
  output and removes it afterwards.

### 11.4 Messages

A compile error is one line:

```text
FILE:LINE:COLUMN: message
```

`FILE` is the path as given for the main file, the canonical path for an
imported file, and `<std>/NAME` for a standard library module. Lines and
columns count from 1, columns in characters. The few errors with no
position (``no `fn main()` defined``, the rule on `main`'s parameters) are
printed without one. An error found by the assembler is reported as
`internal assembler error: line N: …`, `N` being a line of the generated
assembly (write it with `-S` to look). Only the first error is reported.

## 12. Differences from C

| Topic | C | Trine |
|---|---|---|
| Declarations | `int *p[4];` | `var p: [4]*int;` (name first, types read left to right) |
| Integer types | signed and unsigned, several widths | `trit`, `tryte` (`char`), `int`; all balanced signed |
| Overflow | undefined for signed types | wraps modulo 3^27 |
| `char` | a byte | one Unicode code point up to U+2671 (larger ones are 3 trytes in strings) |
| Truth | `0` false, else true | conditions are binary; `true`/`unknown`/`false` are the trits +1/0/−1; a trit is not a condition |
| `&`, `\|` | bitwise AND, OR | tritwise minimum, maximum; bind tighter than comparisons |
| `^` | exclusive or | none (`tmul`, `tsat`, `cons`; `bxor` for binary data) |
| `~x` | bitwise NOT | tritwise NOT, equal to `-x` |
| Shifts | bits; `>>` floors or is implementation-defined | trits; `>>` rounds to nearest |
| Three-way compare | none | `a <=> b`, a `trit`, exact |
| Three-way branch | none | `sign (e) { neg: zero: pos: }` |
| `->` | member through a pointer | none; `.` looks through a pointer |
| `switch` | falls through; `case` takes one value | no fall-through; `case a, b:` |
| Casts | `(T)x` | `x as T` |
| `sizeof` | bytes; types and expressions | trytes; types only |
| Pointers | byte addresses | tryte addresses; a word is addressed by its centre tryte |
| Struct values | assigned, passed, returned | pointers only; no struct assignment |
| Preprocessor | `#include`, `#define`, `#if` | none; `import` loads a file once; `const` |
| Compilation | separate units, linker | one unit; `extern` for assembly |
| `volatile` | needed for devices | not needed: accesses to addressable memory happen as written |
| `goto`, labels | yes | no |
| `do … while` | yes | no (`loop { … if (!c) break; }`) |
| Variadic functions | yes | no |
| `long`, `float`, `double` | yes | `real` (real81) only; no 81-trit integer |
| Unsigned arithmetic | yes | none; `band`/`bor`/`bxor`/`bsh`/`bext`/`brot` for binary data |
| Floating `%` | `fmod` (truncating) | IEEE remainder (rounding) |
| Initialiser for an aggregate | `{ … }` | `{ … }`; a string for a tryte array |
| Globals' initialisers | constants | any expression, run at startup in order |
| Builtins | library functions | instruction-level builtins (`in`, `out`, `mfc`, `cas3`, `vadd`, …) |
| Interrupt handlers | compiler extensions | `interrupt fn` |

---

# Appendix A. Grammar

EBNF, checked against `crates/trine/src/parser.rs`. Terminals are in
quotes; `IDENT`, `INT`, `REAL`, `CHAR` and `STRING` are tokens (Section
II.1).

```text
program     = { item } ;
item        = import | { attr } var | const | struct | func | extern ;
import      = "import" STRING ";" ;
attr        = "@" IDENT "(" expr ")" ;             (* IDENT is align or at *)
var         = "var" IDENT ":" type [ "=" init ] ";" ;
const       = "const" IDENT ":" type "=" expr ";" ;
struct      = "struct" IDENT "{" { IDENT ":" type ";" } "}" ;
func        = [ "interrupt" ] "fn" signature block ;
extern      = "extern" ( "fn" signature | "var" IDENT ":" type ) ";" ;
signature   = IDENT "(" [ param { "," param } ] ")" [ "->" type ] ;
param       = IDENT ":" type ;

type        = "int" | "tryte" | "char" | "trit" | "real" | "void" | IDENT
            | "*" type
            | "[" expr "]" type
            | "fn" "(" [ type { "," type } ] ")" [ "->" type ] ;

init        = expr | "{" [ init { "," init } [ "," ] ] "}" ;

block       = "{" { stmt } "}" ;
stmt        = var
            | block
            | "if" "(" expr ")" stmt [ "else" stmt ]
            | "while" "(" expr ")" stmt
            | "loop" stmt
            | "for" "(" ( var | expr ";" | ";" ) [ expr ] ";" [ expr ] ")" stmt
            | "sign" "(" expr ")" "{" { arm ":" { stmt } } "}"
            | "switch" "(" expr ")" "{" { ( "case" expr { "," expr } | "default" ) ":" { stmt } } "}"
            | "break" ";"
            | "continue" ";"
            | "return" [ expr ] ";"
            | "asm" "{" { STRING } "}"
            | expr ";" ;
arm         = "neg" | "zero" | "pos" ;                (* contextual *)

expr        = cond [ assign_op expr ] ;
assign_op   = "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "<<=" | ">>=" | "&=" | "|=" ;
cond        = or [ "?" expr ":" cond ] ;
or          = and { "||" and } ;
and         = eq { "&&" eq } ;
eq          = rel { ( "==" | "!=" ) rel } ;
rel         = cmp3 { ( "<" | "<=" | ">" | ">=" ) cmp3 } ;
cmp3        = max { "<=>" max } ;
max         = min { "|" min } ;
min         = shift { "&" shift } ;
shift       = add { ( "<<" | ">>" ) add } ;
add         = mul { ( "+" | "-" ) mul } ;
mul         = cast { ( "*" | "/" | "%" ) cast } ;
cast        = prefix { "as" type } ;
prefix      = ( "-" | "~" | "!" | "*" | "&" ) prefix | postfix ;
postfix     = primary { "(" [ expr { "," expr } ] ")" | "[" expr "]" | "." IDENT } ;
primary     = INT | REAL | CHAR | STRING
            | "true" | "unknown" | "false" | "TRUE" | "FALSE" | "null"
            | IDENT
            | "trit"                                  (* only before "(" *)
            | "sizeof" "(" type ")"
            | "(" expr ")" ;
```

Notes:

- Attributes may precede only a `var` declaration; several may be given.
- `extern` declarations take no attributes.
- In a `sign` body the statements of an arm run to the next arm label or
  `}`; in a `switch` body, to the next `case`, `default` or `}`.

# Appendix B. Keywords

| Keyword | Use |
|---|---|
| `fn` | function declaration; function type |
| `var` | variable declaration |
| `const` | constant declaration |
| `struct` | struct declaration |
| `return` | return from a function |
| `if`, `else` | conditional statement |
| `while`, `for`, `loop` | loops |
| `break`, `continue` | loop control |
| `sign` | three-way branch |
| `switch`, `case`, `default` | multi-way branch |
| `as` | conversion |
| `sizeof` | size of a type |
| `import` | load a file |
| `extern` | external function or variable |
| `interrupt` | interrupt function |
| `asm` | inline assembly |
| `true`, `unknown`, `false` | the Kleene truth values, `trit` +1, 0, −1 |
| `TRUE`, `FALSE` | the binary truth values, `int` 1, 0 |
| `null` | the null pointer |
| `void`, `trit`, `tryte`, `int`, `char`, `real` | types |

Contextual: `neg`, `zero`, `pos` (arm labels inside `sign`). Attribute
names after `@`: `align`, `at`.

# Appendix C. Operators

| Operator | Operands | Result | Notes |
|---|---|---|---|
| `f(…)` | function, function value, pointer to function | return type | arguments left to right |
| `a[i]` | array or pointer; integer | element | no bounds check |
| `s.f` | struct or pointer to struct | field | |
| `-x`, `~x` | integer or real | `int` (`trit` for a trit; `real`) | equal for integers |
| `!x` | condition | `int` 1/0 | trit refused |
| `*p` | pointer | pointee | `*void` refused |
| `&x` | lvalue or function | `*T` (function: its type) | |
| `x as T` | see Section II.2.5 | `T` | |
| `*` | integers or reals | `int` (`trit` × `trit` = `trit`; `real`) | wraps |
| `/` | integers or reals | `int` or `real` | truncates toward zero |
| `%` | integers or reals | `int` or `real` | sign of dividend; real: IEEE remainder |
| `+`, `-` | integers, reals, pointer and integer, two pointers (`-`) | `int`, `real`, pointer, `int` | pointers scale by element size |
| `<<`, `>>` | integers | `int` | trits; `>>` rounds |
| `&`, `\|` | integers or reals | `int` (`trit` for two trits; `real`) | min, max |
| `<=>` | comparable pair | `trit` | exact; unordered reals give 0 |
| `<` `<=` `>` `>=` `==` `!=` | comparable pair | `int` 1/0 | NaN: only `!=` holds |
| `&&`, `\|\|` | conditions | `int` 1/0 | short-circuit; trit refused |
| `?:` | condition, two values | see Section II.3.8 | |
| `=` | lvalue, value | the stored value | scalars and reals only |
| `op=` | lvalue, value | the stored value | tryte-sized targets narrow (Section II.3.9) |

# Appendix D. Builtins

| Builtin | Arguments | Result | Instruction |
|---|---|---|---|
| `sgn` | x | `trit` | `SGN` |
| `abs` | x (integer or real) | `int` or `real` | `ABS`, `FABS` |
| `tlz` | x | `int` | `TLZ` |
| `divr`, `modr` | a, b | `int` | `DIVR`, `MODR` |
| `mulh` | a, b | `int` | `MULH` |
| `tmul`, `tsat`, `cons` | a, b | `int` | `TMUL`, `TSAT`, `CONS` |
| `trit` | x, i (const 1…27) | `trit` | `TRIT` |
| `ext` | x, pos, len (const) | `int` | `EXT` |
| `ins` | x, v, pos, len (const) | `int` | `INS` |
| `band`, `bor`, `bxor`, `bandn` | a, b | `int` | `BAND`, `BOR`, `BXOR`, `BANDN` |
| `bsh` | x, n | `int` | `BSH` |
| `bext` | x, pos (0…80), len (1…41), const | `int` | `BEXT` |
| `brot` | x, n (0…len−1), len (1…41), const | `int` | `BROT` |
| `mrg` | old, mask, src | `int` | `MRG` |
| `stwm` | p: `*int`, value, mask | `void` | `STWM` |
| `tcp`, `tcn`, `trev` | x, n | `int` | `TCP`, `TCN`, `TREV` |
| `cas3` | p: `*int`, expected, new | `int` | `CAS3` |
| `swap` | p: `*int`, v | `int` | `SWAP` |
| `bytes` | p (pointer) | `*tryte` | `SUB` |
| `in` | port | `int` | `IN` |
| `out` | port, v | `void` | `OUT` |
| `mfc` | REG | `int` | `MFC` |
| `mtc` | REG, v | `void` | `MTC` |
| `hlt`, `brk`, `tlbflush`, `fence` | none | `void` | as named |
| `tlbinv` | a | `void` | `TLBINV` |
| `syscall` | n (const), up to 6 values | `int` | `SYS n` |
| `sqrt`, `trunc`, `round` | x | `real` | `FSQRT`, `FTRUNC`, `FRND` |
| `rem` | a, b | `real` | `FREM` |
| `scale3` | x, k (integer) | `real` | `FSCL` |
| `exponent3` | x | `int` | `FEXP` |
| `fclass` | x | `int` | `FCLASS` |
| `real27` | x | `real` | `FTOW`, `FFRW` |
| `fconst` | k (const 0…6) | `real` | `FCONST` |
| `fsr` | none | `int` | `FMFS` |
| `set_fsr` | v | `void` | `FMTS` |
| `vadd` `vsub` `vmin` `vmax` `vtmul` `vtsat` `vcons` (and `…w`) | d, a, b | `void` | `VLD`, `VALU`, `VST` |
| `vneg` `vabs` `vsgn` (and `…w`) | d, a | `void` | `VLD`, `VALU`, `VST` |
| `vmrg` | d, mask, src | `void` | `VLD`, `VMRG`, `VST` |
| `vbcst`, `vbcstw` | d, x | `void` | `VBCST`, `VBCSTW`, `VST` |
| `vsum` | a | `int` | `VLD`, `VSUM` |
| `vdotw` | a, w | `int` | `VLD`, `VDOTW` |
