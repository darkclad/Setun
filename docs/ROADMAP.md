# Roadmap

Target: **Setun-11̅** (Setun-26), a balanced-ternary machine sized like a 386, where every
size is a power of 3: 9-trit trytes, 27-trit words, a 3^27 address space,
9|9|9 paging, 3 rings, 3^18 default RAM, and a 640×480 framebuffer with 3^9
colours. Then come a compiler, a kernel and a window system, all written for
the machine itself.

ISA: [`SETUN26_ISA.md`](SETUN26_ISA.md) (v0.4 accepted; v0.5 draft adds floating-point instructions, §14).

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
    - Result so far: the Mandelbrot benchmark went from 312× slower than the same code compiled natively to about 19×; the Trinux shell workload from 77 s to 0.5 s

11. **Networking** ([`NETWORK.md`](NETWORK.md)): the `TNIC` Ethernet card (one octet per tryte), `setun-net` (a user-mode gateway on smoltcp with port forwards, `setun --net`), and in Trinux: ARP, IPv4, ICMP, UDP, TCP (in order, go-back-N retransmission, RFC 6298 timers), BSD sockets with Linux's calls, `/proc/net`; `ifconfig`, `ping`, `nc`, `netstat`, `telnetd`
    - N1–N5 ✅: tested against smoltcp both ways, over a lossy link, through a real host socket, and with hand-made broken packets; `nettest` checks the socket calls and their errors
    - N6 ✅ SSH: a random-number device (`TRNG`, block 11), `/dev/urandom` and `getrandom`; SHA-256/512, ChaCha20-Poly1305, X25519 and Ed25519 in Trine (binary XOR/AND by table, no ISA change); `sshd` with curve25519-sha256, ssh-ed25519, chacha20-poly1305@openssh.com, strict KEX, password login, pty and exec sessions; tested with the host's OpenSSH (NETWORK.md §8)
    - N7 ✅ out to the world: outbound TCP through a NAT to host sockets, DNS at 10.0.2.3 (host resolver), a resolver in tlibc, `host`, `wget` (NETWORK.md §9)
    - N8 ✅ public-key login for `sshd` (ssh-ed25519, `~/.ssh/authorized_keys`); an `ssh` client and `ssh-keygen` in Trinux, sharing the transport with `sshd` (NETWORK.md §10)
    - N9 ✅ copying files: an SFTP v3 server behind sshd's `sftp` subsystem, used by the host's `sftp` and `scp` (NETWORK.md §11); Trinux's `ssh` client checked against a real OpenSSH server
    - N10 ✅ `scp` and `sftp` clients in Trinux, on a shared client library (NETWORK.md §12)
    - next ideas: IPv6; TLS for `wget`; UDP through the NAT; `scp -r`

## Next: performance

In rough order of payoff for effort (estimates from the benchmarks):

| # | Work | Where | Expected |
|---|---|---|---|
| P1 | Word-at-a-time `memcpy`, `memset`, `strlen`, `strcmp` in the runtime (today one tryte per step) | Trine runtime | 2–3× on string-heavy code (int benchmark, shell tools, kernel copies) |
| P2 | ~~Division by a constant as a multiplication by its reciprocal~~ | JIT | not needed: Cranelift already multiplies (measured with `setun-bench --micro`) |
| P3 | ~~`char` and tryte locals in registers~~ | Trine | dropped: built and measured, 0.04% on the text tools (their strings go through arrays and pointers, not `char` locals) |
| P4 | ✅ Leaf functions use six more caller-saved registers, unsaved | Trine | done |
| P5 | ~~Staying native across calls and returns~~ | JIT | dropped: at most about 5% measured (the calls that matter are inlined now) |
| P6 | Inline tryte loads and stores, and FPU instructions, in compiled code | JIT | strings and floating point |
| P7 | ✅ Inlining (cost model), `a[i ± k]` folded into the address, local value numbering, `if (c) break/continue` as one jump, loops with a register test rotated | Trine | done: sieve+sort 13.3M → 5.5M instructions, mandel 70.9M → 67.7M |
| P8 | ~~Two-tier compilation~~: IR per instruction cut by a fifth ✅; tiers dropped: region size (4–32 blocks) and acceptance thresholds made no measurable difference, compiles run in the background | JIT | — |
| P9 | A WebAssembly JIT for the browser build, which only interprets | Web | several times in the artifact |
| P10 | Fewer interpreted instructions: about a fifth of the shell workload still runs in the block interpreter (cold blocks, and short regions without loops); each new process's code starts cold | JIT, kernel | up to about 1.5× on the shell workload |

## Later

- A Setun-70 emulator as a Trinux user program
- More of POSIX where it is cheap: `SIGTTOU`/`TOSTOP`, `select`, more `/proc`
