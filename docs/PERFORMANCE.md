# Emulator performance

The goal is to get as close as possible to one bare core of the host. There
are three layers: a block interpreter, a JIT compiler for hot code (native
builds), and fast fixed-width arithmetic for the FPU.

## Rule: the fast path changes nothing

Time on the Setun-11̅ is counted in instructions (100 ns each by default),
so the emulator is deterministic: the same disk and the same input always
give the same machine state after the same number of steps. Every
optimisation must keep that state **identical**, not just "working":

- `crates/bench` (`setun-bench`) runs six workloads and hashes the whole
  machine (registers, control registers, FPU, all of RAM, the framebuffer,
  device time and serial output). `golden.txt` holds the hashes from the
  original interpreter; `setun-bench` fails on any mismatch.
- `crates/core/tests/fast_path.rs` runs 300 random programs (mostly well
  formed, so they fault, trap, take interrupts, switch rings, rewrite their
  own code and page tables) through the fast path (`Machine::run`) and the
  reference path (`Machine::step`) and compares the whole state at every
  checkpoint. Directed tests cover unmapping the running code page,
  rewriting an instruction that already ran, switching page tables and
  remapping a data page; each was shown to fail when its safeguard is
  removed.
- `Machine::step` stays the reference: a full decode and page walk every
  time.

## How the fast path works

| Piece | What it does |
|---|---|
| **Batches** (`Machine::run`, `Cpu::run_batch`) | Devices report how many steps remain until their next event (timer, RTC alarm, vsync interrupt). The CPU runs that many steps without touching them, then time advances once. A halted CPU skips straight to the event. Port I/O, `IRET`, `MTC` and single-stepping take the exact single-step path. |
| **Basic blocks** | Instructions are decoded once into blocks that end at a branch, jump, generic instruction, 256 instructions or the frame end. A block runs in a tight loop with the PC in a local; the PC, `CYCLE` and the faulting PC are written back only at the block's end, on a fault (recomputed exactly) or before a generic instruction. ADD/SUB/CMP, the other ALU ops, loads and stores, branches, jumps, `LI` and `LEA` are handled inline; SYS, I/O, FPU and field ops go through `exec_major`/`exec_fpu`. Frames with decoded code are watched by `Memory`, so self-modifying code, program loading and DMA drop the stale blocks, and a store that hits code or a page table ends its block. |
| **Chaining** | Each block remembers the last two blocks it went on to, by target address; a link is used only in the epoch it was made in (bumped by any change of translation or ring, dropped code or a cleared arena). |
| **Software TLB** | 1024 entries per access kind. Entries are cached only once the accessed (or dirty) trit is set, so hits never update page tables. Frames holding page-table entries the TLB has used are watched; any write to one (a CPU store or disk DMA) flushes the TLB, as does writing `PTB`. The TLB is therefore invisible. |
| **Fetch and data windows** | The current code page and the last read and written data pages keep their translation (and, for data, the RAM frame itself), so most accesses skip the TLB and index the frame directly. They are cleared on any change of translation or ring. |
| **RAM as words** | Each RAM frame holds 6561 word values, so aligned word loads and stores are single machine accesses; tryte access extracts or replaces one tryte. ROM and the framebuffer stay tryte arrays. |
| **Arithmetic** | Word sums wrap with one compare in `i64`; `MUL` takes its quotient estimate from `f64` operands and the low half from wrapping 64-bit arithmetic, with no 128-bit helpers. |

## The JIT

Native builds compile hot code with [Cranelift](https://cranelift.dev/)
(`crates/core/src/jit.rs`):

- **Regions.** A block run 300 times becomes the root of a region: the
  blocks reachable from it by static jumps and branches within the same
  page (up to 32). Only regions with a loop or at least four inline
  instructions are compiled; entering and leaving native code costs a load
  and a store per register, which shorter stretches do not repay. The
  common short ones are the return paths of functions (registers restored,
  then JALR), which a region never reaches from its caller. On the shell
  workload (threshold 1000 and 16 instructions before) this cut the
  host's cycles by about 15%: interpreted instructions cost about 37
  cycles each there, compiled ones about 9.
- **Registers in host registers.** Guest registers and the flags are
  Cranelift variables, loaded at entry and written back at every exit. S
  is kept lazily, as a value whose sign it is; C as the trit.
- **Inline:** ADD/SUB/CMP, MUL/MULH/DIV/MOD, NEG/SGN/ABS, LI, LEA, branches,
  BR3, JAL, and word and tryte loads and stores through the CPU's read and
  write windows (raw frame pointers; a write pointer is only handed out for
  frames nothing watches, and every window is dropped when a frame becomes
  watched, so a direct store can never miss an invalidation). FMOV, FLDD,
  FLDS and FSTD (a real81) go through the windows too, after a check that
  the FPU is on.
- **Direct helpers.** The tritwise functions (MIN, MAX, TMUL, TSAT, CONS),
  SH, TRIT and TLZ call a pure function of the two operands, and FCMP one
  that compares two f registers; FSTD gets its three words from one. These
  skip the generic path's flag and register round trip: MIN 12.8 to 9.0 ns,
  FCMP 12.3 to 5.6 ns, FSTD 32 to 14 ns (`setun-bench --micro`).
  Everything else calls the interpreter's own code for that one
  instruction, about 13 ns.
- **Exits.** JALR, generic instructions, faults, a store into code or a page
  table, and too little budget for the next block return to the block
  engine with the exact step count, PC and fault.
- **Background compilation.** Cranelift runs on its own thread; the CPU
  keeps interpreting and installs the code when it arrives, if the block is
  still current. Since native and interpreted execution give identical
  state, when code arrives does not change behaviour.
- **Tests.** The random-program test compiles every block at once
  (`set_jit_threshold(1)`) and must match the reference path; directed
  tests cover carries in both directions, MUL/DIV with a zero-divisor fault
  inside compiled code, and a misaligned load through an open window. Each
  was shown to catch a deliberately broken JIT.

## The FPU

`setun-real` computes every result exactly and rounds once. The big-integer
path stays as the reference; fast paths in 256-bit integers (`ethnum`, with
a hand-written 128×128 multiply) cover add/sub (exponents within 80 trits),
mul, div, sqrt, comparisons, integer conversions, scaling, rounding to
integers and narrowing to real27, and fall back near the ends of the
exponent range. `sqrt` is rounded exactly by an integer test on the halfway
points. Differential tests require identical values and flags against the
big-integer path over the whole range, subnormals and unnormalized
encodings included.

The instruction mix explains the choices: loads and stores are 40–45% of
all instructions (Trine's code generator keeps values on the stack), ADD,
SUB and CMP about 35%, branches about 8%. Build with
`--features opstats` to print the mix for each workload.

## Measuring

- `setun-bench --micro` times tight loops of one instruction kind and
  prints ns and TSC cycles per guest instruction (the TSC counts at the
  3.4 GHz base clock).
- Profiling on Windows: `samply record --save-only --unstable-presymbolicate
  -o p.json.gz target/release/setun-bench ...` (needs Administrator), then
  `setun-bench --resolve <RVA>...` maps hot addresses to source lines.

Per guest instruction now (TSC cycles at 3.4 GHz), interpreted / compiled:
CMP and LI 8 / 0.4, ADD 11–13 / 3–4, MUL 31 / 0.7, LDW 13 / 4.3, STW 19 /
4.5, a taken branch 24 / 1.1. Interpreted only: LDT 33, a generic
instruction 42, FADD 300, FDIV 485, FSQRT 925.

## Numbers

`cargo run --release -p setun-bench` on a Ryzen 9 5950X core. Since the Trine
optimizer the guest runs far fewer instructions, so the fair measure is the
time for the same work:

| Workload | Instructions at start | Now | Time at start | Now |
|---|---:|---:|---:|---:|
| int (sieve, sort, strings) | 49.0M | 30.0M | 2.25 s | 0.09 s |
| mandel (fixed point, framebuffer) | 312.6M | 96.5M | 16.1 s | 0.18 s |
| boot (Trinux to the shell) | 11.9M | 9.6M | 1.07 s | 0.07 s |
| shell (md5sum, sort, calc, ls with paging) | 486M | 162M | 76.9 s | 0.53 s |
| fpu (math.tri) | 3.38M | 2.66M | 0.43 s | 0.09 s |

The JIT compiles on background threads (all cores but one, at most 8),
which take regions from one shared queue, so a long compile holds up no
other (short workloads otherwise wait for code); `SETUN_JIT_OFF=1` measures the interpreter alone. The browser
build (WebAssembly, no JIT) boots Trinux at about 125 MIPS, up from 11.

Against the same code compiled natively for the host (`setun-bench
--vs-native`, Rust ports that follow the Trine source line by line, results
checked equal; best of several runs, Ryzen 9 5950X):

| Program | Native | Setun | Setun instructions | Overhead |
|---|---:|---:|---:|---:|
| mandel (fixed-point arithmetic, framebuffer) | 9.4 ms | 72 ms | 65.3M | 7.7× |
| sieve + insertion sort (tryte array, word array) | 0.2 ms | 7.7 ms | 5.5M | 37× (180–240× before constant index offsets and value numbering) |

Mandelbrot is arithmetic in registers, where compiled Setun code runs at
about 700 MIPS. The sieve and sort are memory loops, where the host compiler
still does better than Trine (the sort's inner loop is 9 Setun instructions,
about half again what the host needs). Both run 95% compiled.

## Next

- **Trine:** more locals in registers (tryte and `char` locals, more than
  nine), word-at-a-time `memcpy`/`strlen` in the runtime, and branch
  inversion (`if (c) break;` still costs a branch around a branch).
- **JIT coverage:** tryte loads and stores, FPU instructions and JALR (call
  returns) still leave or call out; inlining them and following returns
  would keep more code native.
- **A WebAssembly JIT** for the browser build, which has only the
  interpreter.
