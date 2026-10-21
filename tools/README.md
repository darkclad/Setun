# tools/

Generators and reference models, kept apart from the code they check. A
conformance generator is written from the specification, not from the
emulator, kernel or libraries, so a vector file is a second opinion: when a
test fails, one of the two readings of the document is wrong.

The Python tools need Python 3; those that say so need the `cryptography`
package, and `check_mlkem.py` needs `openssl` 3.5 or later on the PATH. Run
them from the repository's root.

## Conformance vectors

| Tool | Writes | Checked by |
|---|---|---|
| `gen_isavectors.py` | `crates/core/tests/isa_vectors.txt`: one instruction each, the state before and after ([ISA](../docs/SETUN26_ISA.md) §16) | `crates/core/tests/isa_conformance.rs` |
| `gen_realvectors.py` | `crates/real/tests/vectors.txt`: real27/real81 operations in exact rational arithmetic ([REAL](../docs/REAL.md) §8) | `crates/real/tests/conformance.rs`, `crates/trine/tests/fpu_conformance.rs` |
| `gen_fmtvectors.py` | `user/share/fmtvectors.txt`: tvarints, TTV text, TIM pictures ([FORMATS](../docs/FORMATS.md) §5) | `/bin/fmtvec` |
| `gen_tnetvectors.py` | `crates/net/tests/tnet_vectors.txt`: TCRC-18 and TL frames ([TNET](../docs/TNET.md) §7) | `crates/net/tests/tnet.rs` |
| `gen_ipv6vectors.py` | `user/share/ipv6vectors.txt`: address text, and the frames the station answers a frame with ([IPV6](../docs/IPV6.md) §7) | `/bin/ip6vec` |

Each prints to standard output, for example:

```
python tools/gen_isavectors.py > crates/core/tests/isa_vectors.txt
```

## Cryptography

| Tool | What it is |
|---|---|
| `mlkem.py` | ML-KEM-768 (FIPS 203) from the standard, the reference for `user/lib/mlkem.tri` |
| `check_mlkem.py` | checks `mlkem.py` against OpenSSL's ML-KEM-768: the same keys from the same seed, and OpenSSL's encapsulations decapsulate to its secret |
| `sntrup761.py` | sntrup761 (Streamlined NTRU Prime, as OpenSSH uses it), the reference for `user/lib/sntrup.tri` |
| `check_sntrup761.py` | self-checks of `sntrup761.py`; the check against another implementation is the OpenSSH handshake in `crates/trinux/tests/ssh.rs` |
| `gen_pqvectors.py` | SHA-3, SHAKE, ML-KEM-768 and sntrup761 vectors, printed as Trine source for the "vectors" block of `user/bin/pqtest.tri` |
| `aesvectors.py` | AES-GCM, ECDH and ECDSA signing vectors (`cryptography`): `python tools/aesvectors.py > user/bin/aesvectors.inc.tri` |
| `pkvectors.py` | ECDSA, RSA and AEAD vectors with fresh keys each run (`cryptography`): `python tools/pkvectors.py user/bin/pkvectors.inc.tri` |

## Generated code and tables

| Tool | What it does |
|---|---|
| `gen_gfmul.py` | unrolls `gf_mul` in `user/lib/c25519.tri` into column sums and patches it in: `python tools/gen_gfmul.py user/lib/c25519.tri` |
| `gen_mont.py` | unrolls the Montgomery passes for P-256 and P-384 in `user/lib/bignum.tri`: `python tools/gen_mont.py user/lib/bignum.tri` |
| `gen_wcwidth.py` | writes `user/lib/wcwidth.tri` from Unicode's `EastAsianWidth.txt`: `python tools/gen_wcwidth.py EastAsianWidth.txt > user/lib/wcwidth.tri` |

## The browser build under node

| Tool | What it does |
|---|---|
| `wtest.mjs` | runs `crates/core/tests/fast_path.rs` compiled to WebAssembly (`crates/wtest`), the JIT's modules compiled as they are queued. Build first: `cargo build --release -p setun-wtest --target wasm32-unknown-unknown`, then `node tools/wtest.mjs [name filter]` |
| `wjit.mjs` | boots Trinux in `web/setun.wasm`, logs in and runs commands with the JIT off and on; the transcripts must match, and the times show the JIT's gain: `node tools/wjit.mjs [web/setun.wasm]` |
| `wnet.mjs` | boots Trinux in `web/setun.wasm` with its card carried to `setun-relay` over a WebSocket, as the page does; checks DNS and HTTP through it, and that a private address and the relay's host are refused: `node tools/wnet.mjs [web/setun.wasm] [ws://127.0.0.1:8027/]` |
