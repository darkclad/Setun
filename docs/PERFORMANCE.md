# Emulator performance

The goal is to get as close as possible to one bare core of the host. The
first step, a fast interpreter, is done; a JIT compiler is the next one.

## Rule: the fast path changes nothing

Time on the Setun-386 is counted in instructions (100 ns each by default),
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
| **Software TLB** | 1024 entries per access kind. Entries are cached only once the accessed (or dirty) trit is set, so hits never update page tables. Frames holding page-table entries the TLB has used are watched; any write to one (a CPU store or disk DMA) flushes the TLB, as does writing `PTB`. The TLB is therefore invisible. |
| **Fetch and data windows** | The current code page and the last read and written data pages keep their translation (and, for data, the RAM frame itself), so most accesses skip the TLB and index the frame directly. They are cleared on any change of translation or ring. |
| **RAM as words** | Each RAM frame holds 6561 word values, so aligned word loads and stores are single machine accesses; tryte access extracts or replaces one tryte. ROM and the framebuffer stay tryte arrays. |
| **Arithmetic** | Word sums wrap with one compare in `i64`; `MUL` takes its quotient estimate from `f64` operands and the low half from wrapping 64-bit arithmetic, with no 128-bit helpers. |

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

Per guest instruction now (TSC cycles): CMP/LI 8, ADD 11–13, LDW 13, STW
19, MUL 31, a generic instruction 42, a block boundary about 20.

## Numbers

`cargo run --release -p setun-bench` on a Ryzen 9 5950X core (MIPS = guest
steps per second):

| Workload | Start | Now | |
|---|---:|---:|---:|
| int (sieve, sort, strings) | 21.8 | 222 | 10× |
| mandel (fixed point, framebuffer) | 19.4 | 239 | 12× |
| boot (Trinux to the shell) | 13.1 | 198 | 15× |
| shell (md5sum, sort, calc, ls with paging) | 6.3 | 216 | 34× |
| idle (at the prompt) | 20.6 | 3660 | 178× |
| fpu (math.tri) | 7.8 | 12 | 1.6× |

The browser build (WebAssembly, measured in Node) boots Trinux at 134
MIPS, up from 11.

Against native code on the same core, the same Mandelbrot algorithm runs
about 138× slower on the Setun (312× at the start). That gap is two
factors: about 19 host cycles per guest instruction, and about 7× more
guest instructions than native code needs (Trine's stack-based code).

## Next

- **FPU:** `setun-real` does every operation with big integers. Fixed-width
  128/256-bit arithmetic would make FPU-heavy code several times faster.
- **JIT:** translate hot code to native x86-64 with Cranelift. Guest
  registers live in a context struct, the TLB lookup is inlined, and faults
  return to the interpreter. The same equivalence tests apply.
- **Guest code:** a register-allocating optimiser in Trine would remove most
  of the stack traffic, which makes up nearly half of all instructions.
