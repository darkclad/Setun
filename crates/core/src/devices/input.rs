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

/// Mouse: accumulated motion, buttons, wheel and absolute position, plus a
/// FIFO of button changes (each with the position where it happened), so
/// a press and release between two reads are not lost.
#[derive(Default)]
pub struct Mouse {
    dx: i64,
    dy: i64,
    buttons: i64,
    wheel: i64,
    irq_on: bool,
    pub abs_x: i64,
    pub abs_y: i64,
    changes: std::collections::VecDeque<(i64, i64, i64)>,
    latched: Option<(i64, i64)>,
}

/// Button changes the FIFO holds; older ones are dropped.
const MOUSE_FIFO: usize = 27;
/// U+0305 COMBINING OVERLINE, as a code point.
const OVERLINE: i64 = 0x305;

impl Mouse {
    pub fn moved(&mut self, dx: i64, dy: i64) -> bool {
        self.dx += dx;
        self.dy += dy;
        if dx != 0 || dy != 0 {
            self.latched = None; // the position registers follow the pointer again
        }
        self.irq_on && (dx != 0 || dy != 0)
    }

    /// `BUTTONS = L + 3·M + 9·R`.
    pub fn buttons(&mut self, left: bool, middle: bool, right: bool) -> bool {
        let b = left as i64 + 3 * middle as i64 + 9 * right as i64;
        let changed = b != self.buttons;
        self.buttons = b;
        if changed {
            if self.changes.len() == MOUSE_FIFO {
                self.changes.pop_front();
            }
            self.changes.push_back((self.abs_x, self.abs_y, b));
        }
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
            // BUTTONS pops the oldest queued change and latches its position
            // for ABS_X/ABS_Y; with none queued it is the current state.
            2 => match self.changes.pop_front() {
                Some((x, y, b)) => {
                    self.latched = Some((x, y));
                    b
                }
                None => {
                    self.latched = None;
                    self.buttons
                }
            },
            3 => std::mem::take(&mut self.wheel),
            4 => self.irq_on as i64,
            5 => self.latched.map_or(self.abs_x, |p| p.0),
            6 => self.latched.map_or(self.abs_y, |p| p.1),
            7 => self.changes.len() as i64,
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
    /// A character from the host. The machine writes an overlined character
    /// as its negated code point, the host as the character followed by
    /// U+0305 COMBINING OVERLINE: an overline arriving after a printable
    /// character still waiting in the queue negates it (otherwise it is
    /// dropped).
    pub fn receive(&mut self, c: i64) -> bool {
        if c == OVERLINE {
            if let Some(last) = self.rx.back_mut() {
                if *last >= 33 {
                    *last = -*last;
                }
            }
            return false;
        }
        self.rx.push_back(c);
        self.irq_on
    }

    /// Drain transmitted output as a host string: a negative code point
    /// (an overlined character) becomes the character and U+0305.
    pub fn take_output(&mut self) -> String {
        let mut s = String::new();
        for c in self.tx.drain(..) {
            let ch = |v: i64| u32::try_from(v).ok().and_then(char::from_u32).unwrap_or('\u{FFFD}');
            if c < -32 {
                s.push(ch(-c));
                s.push('\u{305}');
            } else if c < 0 {
                s.push('\u{FFFD}'); // an overlined control character means nothing
            } else {
                s.push(ch(c));
            }
        }
        s
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
