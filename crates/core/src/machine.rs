//! A complete Setun-386: CPU + memory + devices + emulated time.

use crate::cpu::{Cpu, Step};
use crate::devices::{Devices, Power};
use crate::io::Io;
use crate::memory::{frame_start, Memory, ROM_FIRST_FRAME, ROM_FRAMES};
use crate::PAGE;
use ternary::Tryte;

/// Default emulated speed: 100 ns per instruction (10 MIPS, i386DX-33 class).
pub const DEFAULT_NS_PER_INSTRUCTION: u64 = 100;

pub struct Machine {
    pub cpu: Cpu,
    pub mem: Memory,
    pub dev: Devices,
    pub ns_per_instruction: u64,
    pub powered_off: bool,
}

impl Machine {
    /// `ram_frames` pages of RAM; `unix_time_s` seeds the real-time clock.
    pub fn new(ram_frames: i64, unix_time_s: i64) -> Self {
        Machine {
            cpu: Cpu::new(),
            mem: Memory::new(ram_frames),
            dev: Devices::new(ram_frames, unix_time_s),
            ns_per_instruction: DEFAULT_NS_PER_INSTRUCTION,
            powered_off: false,
        }
    }

    /// Load the BIOS image (up to 3^10 trytes) into ROM frames −3..−1,
    /// starting at the lowest ROM address.
    pub fn load_rom(&mut self, trytes: &[i16]) {
        assert!(trytes.len() as i64 <= ROM_FRAMES * PAGE, "ROM image too large");
        let base = frame_start(ROM_FIRST_FRAME);
        for (i, &t) in trytes.iter().enumerate() {
            self.mem.poke(base + i as i64, Tryte::new(t as i64).expect("tryte out of range"));
        }
    }

    /// Execute one instruction and let emulated time and devices move on.
    pub fn step(&mut self) -> Step {
        if self.powered_off {
            return Step::Halted;
        }
        let s = self.cpu.step(&mut self.mem, &mut self.dev);
        self.dev.advance_ns(self.ns_per_instruction);
        self.dev.service(&mut self.mem);
        match self.dev.take_power() {
            Some(Power::Reset) => self.reset(),
            Some(Power::Off) => self.powered_off = true,
            None => {}
        }
        s
    }

    /// Run up to `max` steps; stops early on power-off. Returns steps taken.
    /// A halted CPU still lets time pass, so timers can wake it.
    ///
    /// The result is exactly that of `max` calls to [`Machine::step`], but
    /// between device events the CPU runs in batches without touching the
    /// devices, and a halted CPU skips ahead to the next event.
    pub fn run(&mut self, max: u64) -> u64 {
        let mut n = 0;
        while n < max {
            if self.powered_off {
                return n;
            }
            let budget = (max - n).min(self.dev.steps_until_event(self.ns_per_instruction));
            let deliverable = self.cpu.mode(crate::isa::mode::IE) == 1 && self.dev.pending_vector().is_some();
            if budget > 1 && !deliverable {
                let k = self.cpu.run_batch(&mut self.mem, &mut self.dev, budget);
                if k > 0 {
                    // one advance for the whole batch: no event fell inside it
                    self.dev.advance_ns(k * self.ns_per_instruction);
                    n += k;
                    continue;
                }
            }
            // an interrupt to take, port I/O, single-stepping, or an event now
            self.step();
            n += 1;
        }
        max
    }

    /// Reset the CPU and devices (RAM, disks and the clock are kept).
    pub fn reset(&mut self) {
        self.cpu.reset();
        self.dev.reset();
    }
}
