# Roadmap

Target: **Setun-11̅** (Setun-26), a balanced-ternary machine sized like a 386, where every
size is a power of 3: 9-trit trytes, 27-trit words, a 3^27 address space,
9|9|9 paging, 3 rings, 3^18 default RAM, and a 640×480 framebuffer with 3^9
colours. Then come a compiler, a kernel and a window system, all written for
the machine itself.

ISA: [`SETUN26_ISA.md`](SETUN26_ISA.md) (v0.4 accepted; v0.5 draft adds floating-point instructions, §14).

Beyond this machine: [`TERNARY_WORLD.md`](TERNARY_WORLD.md) lists what a whole ternary-native world needs, layer by layer, with the decisions taken and a phased plan.

## Milestones

1. **`ternary` crate** ✅: `Tern<N>`, 9-trit `Tryte`, 27-trit `Word`, trit fields, base-27 notation; the `0t`/`0z`/`0x` literal notation ([`NOTATION.md`](NOTATION.md))
2. **ISA spec** ✅ v0.4, with v0.5 (FPU) in draft
3. **Emulator core** (`setun-core`, Rust) ✅
   - CPU: decode, ALU, branches, traps, rings, `MODE`/control registers
   - MMU: 2-level 9|9|9 walk, faults, instruction fetch across pages
   - devices: SYSCTL, TPIC, timer, RTC, keyboard, mouse, serial, disk (T1B2 image, DMA), display (text + 3^9-colour framebuffer)
4. **Front ends** ✅: native (`setun`: minifb window, stdio serial, headless PNG screenshots) and **WASM** (`web/`: canvas, keyboard/mouse, serial console, front-panel lamps; boots Trinux from a disk built into the module)
5. **Assembler and disassembler** ✅: text syntax with form selection and branch relaxation, round-trip-safe disassembler, `setun-asm` CLI
6. **Compiler** for **Trine** ✅ ([`LANGUAGE.md`](LANGUAGE.md)): `trine build/run`, stdlib (ports, rt, console, keymap, real, math), an optimizer (§16); `examples/mandel.tri` renders the Mandelbrot set
7. **BIOS** ✅ ([`bios/bios.tri`](../bios/bios.tri), [`BOOT.md`](BOOT.md)): machine check, RTC date, disk boot with the signature `0zB007AB13`, monitor shell
8. **Kernel: Trinux**, modelled on Linux with POSIX interfaces ([`KERNEL.md`](KERNEL.md)) ✅
   - K1 boot, paging, triddy allocator, timer; K2 preemptive user processes; K3 COW fork, execve (TEF), wait4, page faults, initramfs
   - K4 VFS, devfs, tty line discipline, pipes; K5 signals, process groups, job control; K6 tfs root on disk, buffer cache, mounts, /proc
   - K7 tlibc, a POSIX-style sh (functions, eval, here-documents, local, aliases, line editing), core and text utilities, an editor
   - K8 window system: trwm display server, ptys, poll, framebuffer mmap, terminal/clock/colors/paint; Cyrillic font and a Russian layout
   - users and groups: login, su, passwd, useradd/userdel, permission checks, modular password hashing
9. **Floating-point unit** ✅ (ISA §14): real27/real81 balanced-ternary floating point with ±∞, NaN and gradual underflow, 27 registers; emulator (`setun-real`), assembler, Trine's `real` type, lazy FPU switching in Trinux, `math.tri`, and `/bin/calc`

10. **Performance** ([`PERFORMANCE.md`](PERFORMANCE.md)): every step keeps the machine state bit-identical to the reference interpreter (golden state hashes, a fast-vs-reference fuzz test, directed tests each shown to catch its bug)
    - **Emulator** ✅: batched execution up to the next device event, an invisible software TLB, a basic-block interpreter with chaining, RAM stored as words, a Cranelift JIT for hot regions (compiled on background threads, registers in host registers), and fixed-width FPU arithmetic
    - **Trine optimizer** ✅: register locals, direct operands, a liveness-driven peephole pass; optimized and plain builds must give the same results
    - Result so far: the Mandelbrot benchmark went from 312× slower than the same code compiled natively to about 7.8×

11. **Networking** ([`NETWORK.md`](NETWORK.md)): the `TNIC` Ethernet card (one octet per tryte), `setun-net` (a user-mode gateway on smoltcp with port forwards, `setun --net`), and in Trinux: ARP, IPv4, ICMP, UDP, TCP (in order, go-back-N retransmission, RFC 6298 timers), BSD sockets with Linux's calls, `/proc/net`; `ifconfig`, `ping`, `nc`, `netstat`, `telnetd`
    - N1–N5 ✅: tested against smoltcp both ways, over a lossy link, through a real host socket, and with hand-made broken packets; `nettest` checks the socket calls and their errors
    - N6 ✅ SSH: a random-number device (`TRNG`, block 11), `/dev/urandom` and `getrandom`; SHA-256/512, ChaCha20-Poly1305, X25519 and Ed25519 in Trine (binary XOR/AND by table, no ISA change); `sshd` with curve25519-sha256, ssh-ed25519, chacha20-poly1305@openssh.com, strict KEX, password login, pty and exec sessions; tested with the host's OpenSSH (NETWORK.md §8)
    - N7 ✅ out to the world: outbound TCP through a NAT to host sockets, DNS at 10.0.2.3 (host resolver), a resolver in tlibc, `host`, `wget` (NETWORK.md §9)
    - N8 ✅ public-key login for `sshd` (ssh-ed25519, `~/.ssh/authorized_keys`); an `ssh` client and `ssh-keygen` in Trinux, sharing the transport with `sshd` (NETWORK.md §10)
    - N9 ✅ copying files: an SFTP v3 server behind sshd's `sftp` subsystem, used by the host's `sftp` and `scp` (NETWORK.md §11); Trinux's `ssh` client checked against a real OpenSSH server
    - N10 ✅ `scp` and `sftp` clients in Trinux, on a shared client library (NETWORK.md §12)
    - N11 ✅ UDP through the NAT, `clock_settime`, `ntpdate` (NETWORK.md §13)
    - N12 ✅ `scp -r` and `sftp put -r`/`get -r` (NETWORK.md §12)
    - N13 ✅ TLS 1.3 for `wget` (ChaCha20-Poly1305, X25519; ECDSA P-256/P-384, RSA, Ed25519 certificates checked to a 27-root store), `x509`; bignums, SHA-384, HMAC/HKDF (NETWORK.md §14)
    - ISA v0.6 ✅ binary operations (`BAND` `BOR` `BXOR` `BANDN` `BSH`, `BEXT`/`BROT` with the width as an operand; ISA §3.3.1): SHA-2/ChaCha20 without tables
    - N14 ✅ TLS for more servers: AES-128/256-GCM, P-256/P-384 key exchange after a HelloRetryRequest (cookies too); public-key crypto 2.4–4× faster (product-scanning Montgomery)
    - N15 ✅ `wget` speaks HTTP/1.1: redirects, chunked bodies, `-S`, `--max-redirect`
    - N16 ✅ `httpd`: static files over HTTP/1.1 and HTTPS (TLS 1.3 server side, ECDSA signing with RFC 6979 nonces, `certgen` self-signed certificates)
    - IPv6 ([`IPV6.md`](IPV6.md)): V1 ✅ autoconfiguration, Neighbor Discovery, ICMPv6, `ping6`, conformance vectors; V2 UDP/TCP and `AF_INET6` sockets; V3 the gateway's NAT and AAAA; V4 names and brackets in the programs
    - next ideas: `curl`-like POST

12. **Self-hosting** ([`SELFHOST.md`](SELFHOST.md)): Trinux builds its own programs
    - S1 ✅ `tas`, the assembler in Trine: the host assembler's executables, word for word
    - S2 ✅ `tric`, the compiler in Trine: the host compiler's unoptimized assembly, character for character (exact real literals too), and its error messages
    - S3 ✅ the fixed point: `tric` builds itself, and that build builds the same file again
    - ✅ the optimizer in `tric`: all four levels of the host's (`-O1`..`-O4`), each matching the host's assembly

## Next: performance

In rough order of payoff for effort (estimates from the benchmarks):

| # | Work | Where | Expected |
|---|---|---|---|
| P1 | ✅ Word-at-a-time `memcpy`, `memset`, `memmove`, `memcmp` (from 18 trytes, when both ends sit alike in their words); `strlen`/`strcmp` stay tryte-wise (a zero tryte in a word costs as much to find as loading them) | Trine runtime | int 6.77M → 6.58M; the bigger runtime cost every program size until unreached functions were left out (below) |
| P2 | ~~Division by a constant as a multiplication by its reciprocal~~ | JIT | not needed: Cranelift already multiplies (measured with `setun-bench --micro`) |
| P3 | ~~`char` and tryte locals in registers~~ | Trine | dropped: built and measured, 0.04% on the text tools (their strings go through arrays and pointers, not `char` locals) |
| P4 | ✅ Leaf functions use six more caller-saved registers, unsaved | Trine | done |
| P5 | ~~Staying native across calls and returns~~ | JIT | dropped: at most about 5% measured (the calls that matter are inlined now) |
| P6 | ✅ Tryte loads and stores, FMOV/FLDD/FLDS/FSTD inline; FCMP and the tritwise ALU functions by direct helpers | JIT | done; FADD/FMUL/FDIV still go through the interpreter's code |
| P7 | ✅ Inlining (cost model), `a[i ± k]` folded into the address, local value numbering, `if (c) break/continue` as one jump, loops with a register test rotated | Trine | done: sieve+sort 13.3M → 5.5M instructions, mandel 70.9M → 67.7M |
| P8 | ~~Two-tier compilation~~: IR per instruction cut by a fifth ✅; tiers dropped: region size (4–32 blocks) and acceptance thresholds made no measurable difference, compiles run in the background | JIT | — |
| P9 | ✅ A WebAssembly JIT for the browser build: regions as small wasm modules the page compiles and links into the function table; tested with the native fast-path tests compiled to wasm (`crates/wtest`) | Web | Trinux under node 108 → 288 MIPS |
| P10 | ✅ Fewer interpreted instructions: unreached functions left out of every program (shell workload 18.9M → 14.4M instructions); regions from 300 runs and 4 inline instructions (were 1000 and 16), compile threads on one shared queue (host cycles −15%) | Trine, JIT | what is left interpreted: cold code, and hot code waiting for its compile (about 2.5M of 28M instructions) |

## Later

- A Setun-70 emulator as a Trinux user program
- More of POSIX where it is cheap: `SIGTTOU`/`TOSTOP`, `select`, more `/proc`
