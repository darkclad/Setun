//! Ternary link card `TLINK` (ISA §8.2, docs/TNET.md §1).
//!
//! Frames are 1..2187 (3^7) trytes of any value: nothing binary on the
//! wire. DMA uses physical tryte addresses of any alignment. The card does
//! not check frames (the link layer's TCRC-18 does); it only moves them
//! between memory and two queues, which a front end joins to another
//! machine's card, so each machine stays deterministic.
//!
//! Registers: 0 `CMD` (+1 send, −1 receive into `RX_ADDR`, 0 drop the oldest
//! received frame), 1 `STATUS`, 2 `TX_ADDR`, 3 `TX_LEN`, 4 `RX_ADDR`,
//! 5 `RX_LEN`, 6 `RX_COUNT`, 7 `CTRL`, 8 `LINK`, 9 `ERROR`, 10 `DROPS`,
//! 11 `ADDR`.

use crate::memory::Memory;
use std::collections::VecDeque;
use ternary::Tryte;

/// Frames each queue holds.
pub const TLINK_QUEUE: usize = 27;
/// Longest frame, in trytes.
pub const TLINK_MAX_FRAME: usize = 2187;

/// `ERROR` codes (the same numbers as `TNIC`'s, without "not an octet").
pub mod error {
    pub const NONE: i64 = 0;
    pub const NO_LINK: i64 = 1;
    pub const BAD_LENGTH: i64 = 2;
    pub const BUS: i64 = 4;
    pub const NOTHING: i64 = 5;
    pub const BAD_CMD: i64 = 6;
    pub const TX_FULL: i64 = 7;
}

#[derive(Default)]
pub struct Tlink {
    /// A link is attached (set by the front end).
    pub link: bool,
    /// The station address (set by the front end; read-only for the guest).
    pub addr: i64,
    rx: VecDeque<Vec<i16>>,
    tx: VecDeque<Vec<i16>>,
    pending: Option<i64>,
    status: i64,
    tx_addr: i64,
    tx_len: i64,
    rx_addr: i64,
    error: i64,
    drops: i64,
    irq_on: bool,
}

impl Tlink {
    /// Reset the registers; the link and the queues are kept.
    pub fn reset(&mut self) {
        *self = Tlink {
            link: self.link,
            addr: self.addr,
            rx: std::mem::take(&mut self.rx),
            tx: std::mem::take(&mut self.tx),
            drops: self.drops,
            ..Tlink::default()
        };
    }

    pub fn read(&self, r: i64) -> i64 {
        match r {
            0 => self.pending.unwrap_or(0),
            1 => self.status,
            2 => self.tx_addr,
            3 => self.tx_len,
            4 => self.rx_addr,
            5 => self.rx.front().map_or(0, |f| f.len() as i64),
            6 => self.rx.len() as i64,
            7 => self.irq_on as i64,
            8 => self.link as i64,
            9 => self.error,
            10 => self.drops,
            11 => self.addr,
            _ => 0,
        }
    }

    pub fn write(&mut self, r: i64, v: i64) {
        match r {
            0 => self.pending = Some(v),
            2 => self.tx_addr = v,
            3 => self.tx_len = v,
            4 => self.rx_addr = v,
            7 => self.irq_on = v == 1,
            _ => {}
        }
    }

    /// Perform a pending command (it completes before the next instruction).
    pub fn service(&mut self, mem: &mut Memory) {
        let Some(cmd) = self.pending.take() else {
            return;
        };
        self.error = self.run(cmd, mem);
        self.status = if self.error == error::NONE { 0 } else { -1 };
    }

    fn run(&mut self, cmd: i64, mem: &mut Memory) -> i64 {
        match cmd {
            1 => {
                if !self.link {
                    return error::NO_LINK;
                }
                if self.tx_len < 1 || self.tx_len > TLINK_MAX_FRAME as i64 {
                    return error::BAD_LENGTH;
                }
                if self.tx.len() >= TLINK_QUEUE {
                    return error::TX_FULL;
                }
                let mut frame = Vec::with_capacity(self.tx_len as usize);
                for i in 0..self.tx_len {
                    match mem.read_tryte(self.tx_addr + i) {
                        Ok(t) => frame.push(t.value() as i16),
                        Err(_) => return error::BUS,
                    }
                }
                self.tx.push_back(frame);
                error::NONE
            }
            -1 => {
                let Some(frame) = self.rx.front() else {
                    return error::NOTHING;
                };
                for (i, &t) in frame.iter().enumerate() {
                    if mem.write_tryte(self.rx_addr + i as i64, Tryte::new(t as i64).unwrap()).is_err() {
                        return error::BUS; // the frame stays queued
                    }
                }
                self.rx.pop_front();
                error::NONE
            }
            0 => {
                if self.rx.pop_front().is_none() {
                    return error::NOTHING;
                }
                error::NONE
            }
            _ => error::BAD_CMD,
        }
    }

    /// A frame arrives from the link. True if the IRQ should be raised.
    /// A frame of a bad length, or with a value that is not a tryte, is not
    /// delivered.
    pub fn receive(&mut self, frame: &[i16]) -> bool {
        if !self.link || frame.is_empty() || frame.len() > TLINK_MAX_FRAME || frame.iter().any(|&t| !(-9841..=9841).contains(&t)) {
            return false;
        }
        if self.rx.len() >= TLINK_QUEUE {
            self.drops += 1;
            return false;
        }
        self.rx.push_back(frame.to_vec());
        self.irq_on
    }

    /// Frames the guest has sent, oldest first.
    pub fn take_tx(&mut self) -> Vec<Vec<i16>> {
        self.tx.drain(..).collect()
    }
}
