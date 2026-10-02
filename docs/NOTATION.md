# Number notation

How Setun-11̅ numbers are written: in source code, on the screen, in dumps and over the serial port. The normative digit tables are in ISA §1.1 ([`SETUN26_ISA.md`](SETUN26_ISA.md)). This page gives the summary and the reasons behind it.

## Summary

| Prefix | Form | Example | Use for |
|---|---|---|---|
| none | decimal | `42`, `-757`, `1_000` | most values |
| `0t` | balanced ternary: `1`, `0`, `1̅` (`T` = ASCII alias for `1̅`) | `0t1T0` = `0t11̅0` = 6 | trit patterns, masks, attributes |
| `0z` | balanced base 27: `0`–`9`, `A`–`D` (either case) = 0…13; negative digits overlined `1̅`…`D̅` = −1…−13 | `0zDD5̅`, `0zB007AB13` | compact view of trytes and words |
| `0x` | hexadecimal, a value only | `0x1F`, `-0x10` | constants from binary systems (code points, file formats, colours) |

- **A negative digit is the positive digit with a bar over it**, as on the Setun. Negating a number toggles the bar on every digit.
- **One `0z` digit is exactly 3 trits:** a tryte is 3 digits (`0zD̅D̅D̅` … `0zDDD` = ±9 841), and a word is 9 digits (±3 812 798 742 493).
- **`0x` does not show trits.** `0x41` is simply 65. There is no unsigned wrap: `0xFFFF` does not fit a tryte (max `0x2671`), so it is an error, not −1. A negative hex value is written with unary minus.
- `_` may separate digits, e.g. `0t1T0_01T_TT1`.

## Writing the bar

| Where | Form of an overlined character |
|---|---|
| Host source files (assembler, Trine) | the character followed by U+0305 COMBINING OVERLINE |
| Inside the machine | the **negated code point**: overlined `1` is −`'1'` = −49, one tryte |
| Screen and window system | the glyph one row lower, under a bar |
| Serial port | converted to and from character + U+0305, both ways |
| Keyboard (US layout) | the dead key `` ` `` and then the character; `` ` `` Space or `` ` `` `` ` `` types a plain backquote |

Code points −1…−32 (overlined control characters) mean nothing: they show blank, and the serial port sends U+FFFD for them.

## Refused literals

Each of these is a compile or assembly error with a specific message:

- a bar on `0` (there is no −0), on `T`, on a letter beyond `D`, or on a separator;
- two bars on one digit, or a bar with no digit before it;
- `E`–`Z` as `0z` digits, `2` as a `0t` digit, and the old `+`/`-` base-3 digits;
- a `0t` literal directly followed by `+` or `-`: `0t0+0` was 3 in the old notation, so it is refused rather than read as `0 + 0`. Write `0t1 - 1`;
- an overlined hex digit, or a non-hex character after `0x`;
- an empty literal (`0t`, `0z`, `0x`);
- a value that does not fit the target type.

## Output

- `od -t` dumps three `0z` digits per tryte, and `od -3` nine trits per tryte.
- `itoa27`, `itoa3` and `b27_digit` in `rt.tri`, `printk`, the BIOS dump, the disassembler and the web page readout use the same digits.

## Why this notation

**Balanced, not `0`/`1`/`2`.** A trit is −1, 0 or +1. `0t012` would read as ordinary unbalanced ternary (= 5) and has no trit worth 2. The digits have to match what is in the registers.

**`T`/`1̅`, not `+`/`-`.** `+` and `-` are operators, so `0t+-0 - 0t-+` is hard for people to read and easy for a lexer to get wrong. `1`, `0`, `T` keep the literal a single alphanumeric token. `T` is the usual balanced-ternary convention (Knuth); `1̅` is what Setun publications printed.

**Base 27, not 9 or 18.** A "hex" digit is useful only if it covers a whole number of trits, the way one hex digit is 4 bits. So the base must be a power of 3:

| Base | Trits per digit | Digits per tryte | Digits per word |
|---|---|---|---|
| 3 | 1 | 9 | 27 |
| 9 | 2 | 4½ ✗ | 13½ ✗ |
| 27 | 3 | 3 ✓ | 9 ✓ |

18 is not a power of 3, and base 9 splits a tryte in the middle of a digit, like octal on an 8-bit byte.

**Overlined digits, not letters.** Balanced ternary has no unsigned reading: the trit pattern already is the signed value. So an aligned base-27 digit must itself be signed, and needs 13 negative symbols. Earlier versions used `A`–`M` / `a`–`m` with case as the sign; it worked but was hard to read. With the bar, positive digits look like hex (`0`–`9`, `A`–`D`) and the sign is visible on each digit.

## Prior art

- **Setun (Brusentsov, 1958):** balanced base 9 externally, with negative digits as letters `W X Y Z` = −4…−1 (Cyrillic `Ж Х У Ц`), and negative digits printed upside down or overlined. The 9-trit command, the same size as our tryte, was written as base-9 digit pairs around one lone trit (`k y1 y2 x1 y3 y4`), following the instruction's fields.
- **Setun-70 software (Ramil Alvarez):** balanced base 9 with `Q W E R` for −1…−4, because those keys sit under 1–4 on the keyboard.
- **Douglas W. Jones (heptavintimal):** unbalanced base 27, `0`–`9` plus letters without `I L O U W Y`. Read as balanced, zero is `D`, which is why we did not adopt it.

Sources: [Сетунь (Wikipedia)](https://ru.wikipedia.org/wiki/%D0%A1%D0%B5%D1%82%D1%83%D0%BD%D1%8C_(%D0%BA%D0%BE%D0%BC%D0%BF%D1%8C%D1%8E%D1%82%D0%B5%D1%80)), [Девятеричная симметричная система](https://trinary.computer/kb/bcdb3d94-3134-4fcd-8bec-c5777fadf03d), [Balanced ternary (Wikipedia)](https://en.wikipedia.org/wiki/Balanced_ternary), [Jones: Heptavintimal](https://homepage.cs.uiowa.edu/~jones/ternary/hept.shtml).
