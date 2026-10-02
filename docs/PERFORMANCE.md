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
| **Software TLB** | 1024 entries per access kind. Entries are cached only once the accessed (or dirty) trit is set, so hits never update page tables. Frames holding page-table entries the TLB has used are watched by `Memory`; any write to one (a CPU store or disk DMA) flushes the TLB, as does writing `PTB`. The TLB is therefore invisible. |
| **Decoded-instruction cache** | Each physical word is decoded once into a compact form, with dedicated handlers for ADD/SUB/CMP, the other ALU ops, loads and stores, branches, jumps, `LI` and `LEA`. Everything else (SYS, I/O, FPU, field ops) reuses `Cpu::execute`. Frames with decoded code are watched the same way, so self-modifying code, program loading and DMA drop the stale entries. |
| **Fetch and data windows** | The current code page and the last read and written data pages keep their translation, so sequential work skips the TLB lookup. They are cleared on any change of translation or ring. |
| **Arithmetic** | Word sums wrap with one compare in `i64`; `MUL` splits its 54-trit product from an `f64` quotient estimate plus exact fix-ups, avoiding 128-bit division. |
| **Memory** | Word reads and writes to RAM take one frame lookup. |

The instruction mix explains the choices: loads and stores are 40–45% of
all instructions (Trine's code generator keeps values on the stack), ADD,
SUB and CMP about 35%, branches about 8%. Build with
`--features opstats` to print the mix for each workload.

## Numbers

`cargo run --release -p setun-bench` on a Ryzen 9 5950X core (MIPS = guest
steps per second):

| Workload | Before | Now | |
|---|---:|---:|---:|
| int (sieve, sort, strings) | 21.8 | 123 | 5.6× |
| mandel (fixed point, framebuffer) | 19.4 | 98 | 5.1× |
| boot (Trinux to the shell) | 13.1 | 99 | 7.6× |
| shell (md5sum, sort, calc, ls with paging) | 6.3 | 105 | 16.6× |
| idle (at the prompt) | 20.6 | 2000 | 97× |
| fpu (math.tri) | 7.8 | 11 | 1.4× |

The browser build (WebAssembly, measured in Node) boots Trinux at 99 MIPS,
up from 11.

## Next

- **FPU:** `setun-real` does every operation with big integers. Fixed-width
  128/256-bit arithmetic would make FPU-heavy code several times faster.
- **JIT:** translate hot code to native x86-64 with Cranelift. Guest
  registers live in a context struct, the TLB lookup is inlined, and faults
  return to the interpreter. The same equivalence tests apply.
- **Guest code:** a register-allocating optimiser in Trine would remove most
  of the stack traffic, which makes up nearly half of all instructions.
