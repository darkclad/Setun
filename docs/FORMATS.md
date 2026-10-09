# Ternary data formats

**Status: v1 (2026-10-08).** The formats Trinux programs use for numbers, structured values, sound and pictures. The rule for all of them (TERNARY_WORLD.md §5): **they count in trytes and keep signed values balanced**, never in an offset or a zigzag encoding. Binary formats are converted at the border (WAV, PPM), never carried inside.

| Format | For | Library | Tools |
|---|---|---|---|
| [tvarint](#1-tvarint-integers-of-any-size) | integers of any size | `user/lib/tvarint.tri` | |
| [TTV](#2-ttv-tagged-values) | structured values | `user/lib/ttv.tri` | `ttv` |
| [TAU](#3-tau-sound) | sound | `user/lib/tau.tri` | `wav2tau`, `tau2wav`, `tone`, `tauinfo` |
| [TIM](#4-tim-pictures) | pictures | `user/lib/tim.tri` | `ppm2tim`, `tim2ppm`, `timinfo`, `timview` |

Multi-tryte numbers (words) are stored most significant tryte first, as the machine stores them. "A word" in a file is three trytes.

## 1. tvarint: integers of any size

A **tvarint** is a balanced integer in one or more trytes. Each tryte is one trit of control and 8 trits of payload:

| Trit 1 (the control trit) | Trits 2–9 |
|---|---|
| **+1**: another tryte follows | an 8-trit balanced group, −3280 … 3280 |
| **0**: this is the last tryte | the last group |
| **−1**: not part of a tvarint: free for the format that contains it (TIM uses it for runs, §4.3) | |

- The groups are the value's **balanced base-3^8 digits, least significant first**: v = g₀ + g₁ · 6561 + g₂ · 6561² + …. Signed values need no zigzag: −5 is the single tryte −5.
- **The encoding is canonical**: as few trytes as the value needs, so the last group is non-zero unless the value is 0, which is the single tryte 0. A decoder refuses a longer encoding, a control trit of −1 inside a tvarint, a truncated one, and a value too large for the type it reads (an `int`: 27 trits, at most 4 trytes; a `long`: 81 trits, at most 11).
- **Sizes**: |v| ≤ 3280 takes 1 tryte, ≤ 21,523,360 2, ≤ 141,214,768,240 3 (a word needs at most 4).

| Value | Trytes |
|---|---|
| 0 | `0` |
| 7 | `7` |
| −3280 | `−3280` |
| 3281 | `6561 − 3280` then `1`: trytes 3281 and 1 (control +1 is 6561 in the tryte's value) |
| −10,000 | groups 3122, −2: trytes 9683, −2 |

(A tryte's value is control · 6561 + group.)

Library (`tvarint.tri`): `tvar_put(buf, v) -> n` and `tvar_put_long(buf, v) -> n` write one and return its length; `tvar_get(buf, n, *v) -> used` and `tvar_get_long(buf, n, *v) -> used` read one and return the trytes used, or −1 if it is not a valid tvarint of that size.

## 2. TTV: tagged values

**TTV** (trit-tagged values) serializes structured data, as CBOR or MessagePack do for binary systems. A value is one **tag tryte** and its payload:

| Tag | Type | Payload |
|---|---|---|
| 0 | null | none |
| 1 | truth | one tryte: −1 false, 0 unknown, +1 true (Kleene) |
| 2 | integer | a tvarint (any size) |
| 3 | real | 9 trytes: a real81 ([`REAL.md`](REAL.md)) |
| 4 | text | a tvarint length (in trytes), then the text (code points, [`TERNARY_WORLD.md`](TERNARY_WORLD.md) §5) |
| 5 | octets | a tvarint length, then one octet (0 … 255) per tryte: binary data from the border |
| 6 | list | a tvarint count, then that many values |
| 7 | map | a tvarint count, then that many key-value pairs (keys are values of any type) |

Other tags are refused, and so are a truth other than −1, 0, +1, a negative length or count, one past the end of the data, and an octet outside 0 … 255. A file read as one value must end with it. A reader may refuse an integer past 81 trits (a `long`). A truth is a first-class type, so `unknown` survives a round trip, which no binary format offers.

### 2.1 Text notation

TTV values are written and read as text by the `ttv` tool and by `ttvtext.tri` (`ttv_format`, `ttv_parse`): `null`, `true`, `unknown`, `false`; integers in decimal; reals with a point or an exponent (`2.5`, `1e+20`), printed as Trine's `rtoa` does with 12 significant digits (plain for decimal exponents −5 … 11, otherwise `d.ddde±N`), with `.0` added when that shows neither; `"text"` with `\"`, `\\` and `\n` escaped; `x"0aff"` octets in lower-case hexadecimal; `[a, b]` lists and `{key: value}` maps, with `, ` and `: ` between items. Spaces between items are optional when reading; printing writes exactly this form. Text read and printed again is unchanged, and a value printed and read again is the same value, except a real that needs more than 12 significant digits: its text is the nearest 12-digit decimal (the trytes of a TTV file are always exact; the notation is for people).

Library (`ttv.tri`, and `ttvtext.tri` for the notation): a writer (`ttv_null`, `ttv_truth`, `ttv_int`, `ttv_long`, `ttv_real`, `ttv_text`, `ttv_octets`, `ttv_list`, `ttv_map` append to a buffer) and a reader (`ttv_kind`, `ttv_read_*`, `ttv_skip`), each checking bounds. The `ttv` tool prints a TTV file in a readable notation (`ttv file`) and makes one from that notation (`ttv -c text`).

## 3. TAU: sound

A TAU file is a header of 6 words and then the samples:

| Word | Field |
|---|---|
| 0 | magic: the trytes `T A U` (84 · 3^18 + 65 · 3^9 + 85) |
| 1 | version: 1 |
| 2 | sample rate, in Hz |
| 3 | channels, 1 … 9 |
| 4 | frames (samples per channel) |
| 5 | reserved, 0 (a reader refuses anything else) |

- **A sample is one word**: a balanced 27-trit value, full scale ±3,812,798,742,493 (about 42.8 bits). Silence is 0 with no offset, and the range is symmetric. Frames follow one another, the channels of a frame interleaved.
- **From and to WAV** (PCM at the border): a 16-bit sample s is stored as s · 3^16, an 8-bit one as (s − 128) · 3^21, a 24-bit one as s · 3^10; all exact, so WAV → TAU → WAV gives back the same samples. TAU → WAV rounds to the nearest (16 bits unless asked otherwise) and saturates.

Library (`tau.tri`): `tau_header`, `tau_read_header` (which checks it), `tau_sample`, `tau_set_sample`. Tools: `wav2tau in.wav out.tau`, `tau2wav [-b 8|16|24] in.tau out.wav`, `tone [-r rate] [-a amplitude] freq seconds out.tau` (a sine from `math.tri`), `tauinfo file`.

## 4. TIM: pictures

### 4.1 The header

8 words:

| Word | Field |
|---|---|
| 0 | magic: the trytes `T I M` |
| 1 | version: 1 |
| 2 | width, 1 … 19,683 |
| 3 | height, 1 … 19,683 |
| 4 | channels: 1 (grey) or 3 (red, green, blue) |
| 5 | depth: trits per sample, 1 … 9 (3 is the display's 27 levels) |
| 6 | coding: 0 raw, 1 predicted (§4.3) |
| 7 | reserved, 0 (a reader refuses anything else) |

- **A sample is balanced**: depth d gives −(3^d − 1)/2 … (3^d − 1)/2, with 0 the middle of the range (mid-grey, not black). Samples go row by row, top to bottom, left to right, the channels of a pixel together.
- **Raw coding** stores one sample per tryte. A sample outside its depth's range is refused, in either coding, and so is a file longer or shorter than its samples.

### 4.2 Depths and the display

The framebuffer's pixel is one tryte of three 3-trit channels (27 levels each: 0z000 is mid-grey, 0zDDD white). A depth-3 RGB TIM is exactly that. A deeper picture is shown by rounding each sample to 3 trits, which balanced rounding does by dropping trits.

Binary pictures enter at the border: an 8-bit PPM (P6) or PGM (P5) channel value v becomes v − 128 at depth 6 (−364 … 364 holds −128 … 127 exactly), and back.

### 4.3 Predicted coding

Each sample is predicted from its neighbours in the same channel, and the **balanced residual** r = x − p is stored. The predictor is LOCO-I's median edge detector, with a the sample to the left, b the one above and c the one above-left (missing neighbours are 0):

- p = min(a, b) if c ≥ max(a, b); max(a, b) if c ≤ min(a, b); a + b − c otherwise.

Residuals are signed by nature, which is why balanced storage needs no offset. The residuals, in sample order, are written as **tvarints**, except that a **run of zero residuals** is one tryte with control trit −1 and the run length (1 … 3280) in its 8 payload trits, the state that a tvarint never uses. A zero residual is always part of a run (a tvarint 0 is refused), and a run of 0 is refused. An encoder makes each run as long as it can, up to 3280, so the encoding of a picture is unique. A flat area costs one tryte per 3280 samples; an edge costs one or two trytes per sample.

Library (`tim.tri`): `tim_read` / `tim_write` (either coding), `tim_predict`. Tools: `ppm2tim [-r] in.ppm out.tim` (`-r` raw), `tim2ppm in.tim out.ppm`, `timinfo file` (size, depth, coding, the space it saves), `timview file` (draws it on the framebuffer until Enter).

## 5. Conformance

An implementation conforms when it gives every **conformance vector**.

- **The vectors** are `user/share/fmtvectors.txt` (on a Trinux disk, `/usr/share/fmtvectors.txt`), made by `tools/gen_fmtvectors.py`: a model of this document in Python, sharing no code with the libraries.
- **What they cover**:
  - tvarints at the edge of every length, both signs, words and longs, and the encodings that must be refused (a zero last group, a control trit of −1, truncation, past 81 trits, past a word for an `int`);
  - TTV values of every type in their text notation (§2.1), including `unknown`, longs, exact reals, Cyrillic text, escapes, octets and nesting, and malformed values (an unknown tag, a truth of 2, lengths past the data, octets past 255, data after the value);
  - TIM pictures of every depth, both codings, flat areas, edges, noise and long runs, and malformed files (each bad header field, a short or long file, an empty run, a zero residual);
  - TAU files with extreme samples, and malformed headers (magic, version, rate, channels, frames, reserved word, a short file).
- **Format**: one vector per line: `tvar V = T…`, `tvar! T…`, `tvari! T…`, `ttv N = T…`, `ttv> N = T…` (printing only), `ttv! T…`, `tim W H C D K : S… = T…`, `tim! T…`, `tau R C F : S… = T…`, `tau! T…` (described in the file's header).
- **The checker** `/bin/fmtvec [file]` runs them against the Trinux libraries; `crates/trinux/tests/formats.rs` runs it, and a file of wrong vectors to see that it complains.

To regenerate: `python tools/gen_fmtvectors.py > user/share/fmtvectors.txt`.
