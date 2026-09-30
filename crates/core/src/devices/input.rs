//! Keyboard, mouse and serial console (ISA §8.2).
//!
//! Host-side methods return true when the device wants its IRQ raised.

use std::collections::VecDeque;

/// Queued key events: +code on press, −code on release (USB HID usages).
#[derive(Default)]
pub struct Keyboard {
    queue: VecDeque<i64>,
    irq_on: bool,
    leds: i64,
}

impl Keyboard {
    pub fn push(&mut self, event: i64) -> bool {
        self.queue.push_back(event);
        self.irq_on
    }

    pub fn read(&mut self, r: i64) -> i64 {
        match r {
            0 => self.queue.pop_front().unwrap_or(0),
            1 => self.queue.len() as i64,
            2 => self.irq_on as i64,
            3 => self.leds,
            _ => 0,
        }
    }

    pub fn write(&mut self, r: i64, v: i64) {
        match r {
            2 => match v {
                1 => self.irq_on = true,
                0 => self.irq_on = false,
                -1 => self.queue.clear(),
                _ => {}
            },
            3 => self.leds = v,
            _ => {}
        }
    }
}

/// Mouse: accumulated motion, buttons, wheel and absolute position.
#[derive(Default)]
pub struct Mouse {
    dx: i64,
    dy: i64,
    buttons: i64,
    wheel: i64,
    irq_on: bool,
    pub abs_x: i64,
    pub abs_y: i64,
}

impl Mouse {
    pub fn moved(&mut self, dx: i64, dy: i64) -> bool {
        self.dx += dx;
        self.dy += dy;
        self.irq_on && (dx != 0 || dy != 0)
    }

    /// `BUTTONS = L + 3·M + 9·R`.
    pub fn buttons(&mut self, left: bool, middle: bool, right: bool) -> bool {
        let b = left as i64 + 3 * middle as i64 + 9 * right as i64;
        let changed = b != self.buttons;
        self.buttons = b;
        self.irq_on && changed
    }

    pub fn wheel(&mut self, steps: i64) -> bool {
        self.wheel += steps;
        self.irq_on && steps != 0
    }

    pub fn read(&mut self, r: i64) -> i64 {
        match r {
            0 => std::mem::take(&mut self.dx),
            1 => std::mem::take(&mut self.dy),
            2 => self.buttons,
            3 => std::mem::take(&mut self.wheel),
            4 => self.irq_on as i64,
            5 => self.abs_x,
            6 => self.abs_y,
            _ => 0,
        }
    }

    pub fn write(&mut self, r: i64, v: i64) {
        if r == 4 {
            self.irq_on = v == 1;
        }
    }
}

/// Serial debug console. Characters are trytes holding Unicode code points.
#[derive(Default)]
pub struct Serial {
    rx: VecDeque<i64>,
    /// Characters the machine has written, for the host to drain.
    pub tx: Vec<i64>,
    irq_on: bool,
}

impl Serial {
    pub fn receive(&mut self, c: i64) -> bool {
        self.rx.push_back(c);
        self.irq_on
    }

    /// Drain transmitted output as a host string.
    pub fn take_output(&mut self) -> String {
        self.tx
            .drain(..)
            .map(|c| u32::try_from(c).ok().and_then(char::from_u32).unwrap_or('\u{FFFD}'))
            .collect()
    }

    pub fn read(&mut self, r: i64) -> i64 {
        match r {
            0 => self.rx.pop_front().unwrap_or(0),
            1 => self.rx.len() as i64,
            2 => 1, // always ready to transmit
            3 => self.irq_on as i64,
            _ => 0,
        }
    }

    pub fn write(&mut self, r: i64, v: i64) {
        match r {
            0 => self.tx.push(v),
            3 => self.irq_on = v == 1,
            _ => {}
        }
    }
}
