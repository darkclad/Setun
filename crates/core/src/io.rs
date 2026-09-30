//! Port I/O and interrupt interface between the CPU and the devices.

use ternary::Word;

/// What the CPU sees of the device side: 3^9 word-wide ports and the
/// interrupt controller's "is an interrupt waiting" line.
pub trait Io {
    fn port_in(&mut self, port: i64) -> Word;
    fn port_out(&mut self, port: i64, value: Word);

    /// Vector of the highest-priority pending, unmasked interrupt, if any.
    /// Only consulted while `MODE.IE = +1`.
    fn pending_vector(&mut self) -> Option<i64> {
        None
    }
}

/// No devices: reads return 0, writes are dropped.
#[derive(Default)]
pub struct NullIo;

impl Io for NullIo {
    fn port_in(&mut self, _port: i64) -> Word {
        Word::ZERO
    }

    fn port_out(&mut self, _port: i64, _value: Word) {}
}
