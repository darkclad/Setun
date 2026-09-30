//! Setun-386 emulator core, implementing `docs/SETUN386_ISA.md` v0.4.
//!
//! * [`memory`]: physical memory map (RAM, BIOS ROM, framebuffer).
//! * [`cpu`]: registers, decode/execute, traps and the 9|9|9 MMU.
//! * [`isa`]: opcode numbers and instruction encoders, shared with the
//!   assembler and the tests.
//! * [`io`]: the port-I/O and interrupt interface that devices implement.
//! * [`devices`]: the standard devices (ISA §8).
//! * [`machine`]: CPU + memory + devices + emulated time.

pub mod cpu;
pub mod devices;
pub mod io;
pub mod isa;
pub mod machine;
pub mod memory;

pub use cpu::{Cpu, Step};
pub use devices::Devices;
pub use io::{Io, NullIo};
pub use machine::Machine;
pub use memory::Memory;

/// Trytes per page / frame (3^9).
pub const PAGE: i64 = 19_683;
/// Half a page: page offsets run −HALF_PAGE..=HALF_PAGE.
pub const HALF_PAGE: i64 = 9_841;
