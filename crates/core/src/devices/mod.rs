//! Standard devices and the port bus (ISA §8).
//!
//! Each device owns a centred block of 27 ports: block `b` covers ports
//! `27·b − 13 … 27·b + 13`, and register `r` of the device is port `27·b + r`.
//! Every port transfers a word.

mod disk;
mod display;
mod font;
mod input;
mod nic;
mod pic;
mod timer;

pub use disk::{BlockStore, Disk, MemDisk};
#[cfg(not(target_arch = "wasm32"))]
pub use disk::FileDisk;
pub use display::{Display, FRAME_US};
pub use font::{glyph8x8, overlined, CYRILLIC_FIRST, CYRILLIC_LAST, NUMERO};
pub use input::{Keyboard, Mouse, Serial};
pub use nic::{Nic, DEFAULT_MAC, MAX_FRAME, MIN_FRAME, NIC_QUEUE};
pub use nic::error as nic_error;
pub use pic::Pic;
pub use timer::{Rtc, Timer};

use crate::io::Io;
use crate::memory::Memory;
use ternary::{div_pow3_round, Word};

/// Device block numbers (ISA §8.1).
pub mod block {
    pub const SYSCTL: i64 = 0;
    pub const TPIC: i64 = 1;
    pub const TIMER: i64 = 2;
    pub const RTC: i64 = 3;
    pub const KEYBOARD: i64 = 4;
    pub const MOUSE: i64 = 5;
    pub const SERIAL: i64 = 6;
    pub const DISK0: i64 = 7;
    pub const DISK1: i64 = 8;
    pub const DISPLAY: i64 = 9;
    pub const NIC: i64 = 10;
}

/// IRQ line numbers (ISA §8.1).
pub mod irq {
    pub const TIMER: usize = 0;
    pub const KEYBOARD: usize = 1;
    pub const MOUSE: usize = 2;
    pub const SERIAL: usize = 3;
    pub const DISK0: usize = 4;
    pub const RTC: usize = 5;
    pub const DISPLAY: usize = 6;
    pub const DISK1: usize = 7;
    pub const NIC: usize = 8;
}

/// Value of `SYSCTL.SYSID`: the machine number.
pub const SYSID: i64 = 26; // 0t100T, "11̅" in balanced base 27

/// Power request written to `SYSCTL.POWER`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Power {
    Reset,
    Off,
}

pub fn word(v: i64) -> Word {
    Word::wrap(v as i128)
}

/// All standard devices, wired to the port bus and the interrupt controller.
pub struct Devices {
    pub pic: Pic,
    pub timer: Timer,
    pub rtc: Rtc,
    pub keyboard: Keyboard,
    pub mouse: Mouse,
    pub serial: Serial,
    pub disks: [Disk; 2],
    pub display: Display,
    pub nic: Nic,
    /// Trytes written to `SYSCTL.DEBUG`, for the host log.
    pub debug_log: Vec<i64>,
    ram_frames: i64,
    power: Option<Power>,
    now_us: i64,
    ns_carry: u64,
}

impl Devices {
    /// `unix_time_s` seeds the real-time clock.
    pub fn new(ram_frames: i64, unix_time_s: i64) -> Self {
        Devices {
            pic: Pic::new(),
            timer: Timer::default(),
            rtc: Rtc::new(unix_time_s),
            keyboard: Keyboard::default(),
            mouse: Mouse::default(),
            serial: Serial::default(),
            disks: [Disk::default(), Disk::default()],
            display: Display::new(),
            nic: Nic::default(),
            debug_log: Vec::new(),
            ram_frames,
            power: None,
            now_us: 0,
            ns_carry: 0,
        }
    }

    /// Reset device state (keeps disks, the clock and host-side queues).
    pub fn reset(&mut self) {
        self.pic = Pic::new();
        self.timer = Timer::default();
        self.display = Display::new();
        for d in &mut self.disks {
            d.reset();
        }
        self.nic.reset();
    }

    /// Emulated microseconds since power-on.
    pub fn now_us(&self) -> i64 {
        self.now_us
    }

    /// A pending `SYSCTL.POWER` request, cleared by reading it.
    pub fn take_power(&mut self) -> Option<Power> {
        self.power.take()
    }

    /// Advance emulated time and fire timer, RTC alarm and vsync interrupts.
    pub fn advance_ns(&mut self, ns: u64) {
        let total = self.ns_carry + ns;
        self.ns_carry = total % 1000;
        let us = (total / 1000) as i64;
        if us > 0 {
            self.advance_us(us);
        }
    }

    /// How many steps of `ns` each can pass before a device event (timer,
    /// RTC alarm, vsync interrupt): the event happens during the advance of
    /// the returned step, never earlier. At least 1; `u64::MAX` for none.
    pub fn steps_until_event(&self, ns: u64) -> u64 {
        if ns == 0 {
            return u64::MAX; // time stands still
        }
        let soonest = [
            self.timer.us_until_fire(),
            self.rtc.us_until_alarm(self.now_us),
            self.display.us_until_vsync_irq(self.now_us),
        ]
        .into_iter()
        .flatten()
        .min();
        match soonest {
            None => u64::MAX,
            Some(us) => {
                // the first k with ns_carry + k·ns >= 1000·us
                let need = (us as u64).saturating_mul(1000).saturating_sub(self.ns_carry);
                need.div_ceil(ns.max(1)).max(1)
            }
        }
    }

    pub fn advance_us(&mut self, us: i64) {
        self.now_us += us;
        if self.timer.advance(us) {
            self.pic.raise(irq::TIMER);
        }
        if self.rtc.check_alarm(self.now_us) {
            self.pic.raise(irq::RTC);
        }
        if self.display.advance(self.now_us) {
            self.pic.raise(irq::DISPLAY);
        }
    }

    /// Run pending DMA transfers.
    pub fn service(&mut self, mem: &mut Memory) {
        for (n, line) in [irq::DISK0, irq::DISK1].into_iter().enumerate() {
            if self.disks[n].service(mem) {
                self.pic.raise(line);
            }
        }
        self.nic.service(mem);
    }

    // ---- host-side input -------------------------------------------------

    /// A key event: USB HID usage `code`, pressed or released.
    pub fn key(&mut self, code: i64, pressed: bool) {
        if self.keyboard.push(if pressed { code } else { -code }) {
            self.pic.raise(irq::KEYBOARD);
        }
    }

    /// Relative mouse motion.
    pub fn mouse_move(&mut self, dx: i64, dy: i64) {
        if self.mouse.moved(dx, dy) {
            self.pic.raise(irq::MOUSE);
        }
    }

    /// Absolute mouse position (browser / tablet), in framebuffer pixels.
    pub fn mouse_abs(&mut self, x: i64, y: i64) {
        let (dx, dy) = (x - self.mouse.abs_x, y - self.mouse.abs_y);
        self.mouse.abs_x = x;
        self.mouse.abs_y = y;
        self.mouse_move(dx, dy);
    }

    /// Mouse buttons (left, middle, right) and wheel steps.
    pub fn mouse_buttons(&mut self, left: bool, middle: bool, right: bool) {
        if self.mouse.buttons(left, middle, right) {
            self.pic.raise(irq::MOUSE);
        }
    }

    pub fn mouse_wheel(&mut self, steps: i64) {
        if self.mouse.wheel(steps) {
            self.pic.raise(irq::MOUSE);
        }
    }

    /// Characters typed on the host serial console.
    pub fn serial_input(&mut self, text: &str) {
        for c in text.chars() {
            if self.serial.receive(c as i64) {
                self.pic.raise(irq::SERIAL);
            }
        }
    }

    /// A frame from the network for the card (dropped if there is no link,
    /// it has a bad length, or the receive queue is full).
    pub fn net_receive(&mut self, frame: &[u8]) {
        if self.nic.receive(frame) {
            self.pic.raise(irq::NIC);
        }
    }

    /// Frames the card has sent since the last call.
    pub fn net_take_tx(&mut self) -> Vec<Vec<u8>> {
        self.nic.take_tx()
    }

    // ---- ports -----------------------------------------------------------

    fn sysctl_in(&self, r: i64) -> i64 {
        match r {
            0 => SYSID,
            1 => self.ram_frames,
            _ => 0,
        }
    }

    fn sysctl_out(&mut self, r: i64, v: i64) {
        match r {
            2 if v == 1 => self.power = Some(Power::Reset),
            2 if v == -1 => self.power = Some(Power::Off),
            3 => self.debug_log.push(v),
            _ => {}
        }
    }
}

/// Split a port number into (block, register).
pub fn decode_port(port: i64) -> (i64, i64) {
    let b = div_pow3_round(port as i128, 3) as i64;
    (b, port - 27 * b)
}

impl Io for Devices {
    fn port_in(&mut self, port: i64) -> Word {
        let (b, r) = decode_port(port);
        let v = match b {
            block::SYSCTL => self.sysctl_in(r),
            block::TPIC => self.pic.read(r),
            block::TIMER => self.timer.read(r, self.now_us),
            block::RTC => self.rtc.read(r, self.now_us),
            block::KEYBOARD => self.keyboard.read(r),
            block::MOUSE => self.mouse.read(r),
            block::SERIAL => self.serial.read(r),
            block::DISK0 => self.disks[0].read(r),
            block::DISK1 => self.disks[1].read(r),
            block::DISPLAY => self.display.read(r),
            block::NIC => self.nic.read(r),
            _ => 0,
        };
        word(v)
    }

    fn port_out(&mut self, port: i64, value: Word) {
        let (b, r) = decode_port(port);
        let v = value.value();
        match b {
            block::SYSCTL => self.sysctl_out(r, v),
            block::TPIC => self.pic.write(r, v),
            block::TIMER => self.timer.write(r, v),
            block::RTC => self.rtc.write(r, v, self.now_us),
            block::KEYBOARD => self.keyboard.write(r, v),
            block::MOUSE => self.mouse.write(r, v),
            block::SERIAL => self.serial.write(r, v),
            block::DISK0 => self.disks[0].write(r, v),
            block::DISK1 => self.disks[1].write(r, v),
            block::DISPLAY => self.display.write(r, v),
            block::NIC => self.nic.write(r, v),
            _ => {}
        }
    }

    fn pending_vector(&mut self) -> Option<i64> {
        self.pic.pending_vector()
    }
}
