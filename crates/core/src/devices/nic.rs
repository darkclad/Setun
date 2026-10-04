//! Network card `TNIC` (ISA §8.2, docs/NETWORK.md §1).
//!
//! Frames are Ethernet II without the FCS, 14..1514 octets, one octet per
//! tryte. DMA uses physical tryte addresses of any alignment. The card only
//! moves frames between memory and two queues; a front end connects the
//! queues to a network (`setun-net`), so the machine stays deterministic.
//!
//! Registers: 0 `CMD` (+1 send, −1 receive into `RX_ADDR`, 0 drop the oldest
//! received frame), 1 `STATUS`, 2 `TX_ADDR`, 3 `TX_LEN`, 4 `RX_ADDR`,
//! 5 `RX_LEN`, 6 `RX_COUNT`, 7 `CTRL`, 8 `MAC_HI`, 9 `MAC_LO`, 10 `LINK`,
//! 11 `ERROR`, 12 `DROPS`.

use crate::memory::Memory;
use std::collections::VecDeque;
use ternary::Tryte;

/// Frames each queue holds.
pub const NIC_QUEUE: usize = 27;
/// Shortest frame: an Ethernet header.
pub const MIN_FRAME: usize = 14;
/// Longest frame: header + a 1500-octet MTU.
pub const MAX_FRAME: usize = 1514;
/// The card's MAC address unless the front end sets another.
pub const DEFAULT_MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];

/// `ERROR` codes.
pub mod error {
    pub const NONE: i64 = 0;
    pub const NO_LINK: i64 = 1;
    pub const BAD_LENGTH: i64 = 2;
    pub const NOT_OCTET: i64 = 3;
    pub const BUS: i64 = 4;
    pub const NOTHING: i64 = 5;
    pub const BAD_CMD: i64 = 6;
    pub const TX_FULL: i64 = 7;
}

pub struct Nic {
    /// A network is attached (set by the front end).
    pub link: bool,
    pub mac: [u8; 6],
    rx: VecDeque<Vec<u8>>,
    tx: VecDeque<Vec<u8>>,
    pending: Option<i64>,
    status: i64,
    tx_addr: i64,
    tx_len: i64,
    rx_addr: i64,
    error: i64,
    drops: i64,
    irq_on: bool,
}

impl Default for Nic {
    fn default() -> Self {
        Nic {
            link: false,
            mac: DEFAULT_MAC,
            rx: VecDeque::new(),
            tx: VecDeque::new(),
            pending: None,
            status: 0,
            tx_addr: 0,
            tx_len: 0,
            rx_addr: 0,
            error: 0,
            drops: 0,
            irq_on: false,
        }
    }
}

impl Nic {
    /// Reset the registers. The link, the MAC address and the host-side
    /// queues are kept, like the disks' media.
    pub fn reset(&mut self) {
        *self = Nic {
            link: self.link,
            mac: self.mac,
            rx: std::mem::take(&mut self.rx),
            tx: std::mem::take(&mut self.tx),
            drops: self.drops,
            ..Nic::default()
        };
    }

    pub fn read(&self, r: i64) -> i64 {
        let mac = |a: usize| (self.mac[a] as i64) * 65536 + (self.mac[a + 1] as i64) * 256 + self.mac[a + 2] as i64;
        match r {
            0 => self.pending.unwrap_or(0),
            1 => self.status,
            2 => self.tx_addr,
            3 => self.tx_len,
            4 => self.rx_addr,
            5 => self.rx.front().map_or(0, |f| f.len() as i64),
            6 => self.rx.len() as i64,
            7 => self.irq_on as i64,
            8 => mac(0),
            9 => mac(3),
            10 => self.link as i64,
            11 => self.error,
            12 => self.drops,
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
                if self.tx_len < MIN_FRAME as i64 || self.tx_len > MAX_FRAME as i64 {
                    return error::BAD_LENGTH;
                }
                if self.tx.len() >= NIC_QUEUE {
                    return error::TX_FULL;
                }
                let mut frame = Vec::with_capacity(self.tx_len as usize);
                for i in 0..self.tx_len {
                    match mem.read_tryte(self.tx_addr + i) {
                        Ok(t) if (0..=255).contains(&t.value()) => frame.push(t.value() as u8),
                        Ok(_) => return error::NOT_OCTET,
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
                for (i, &b) in frame.iter().enumerate() {
                    if mem.write_tryte(self.rx_addr + i as i64, Tryte::new(b as i64).unwrap()).is_err() {
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

    /// A frame arrives from the network. True if the IRQ should be raised.
    /// Frames of a bad length are not delivered by real hardware either.
    pub fn receive(&mut self, frame: &[u8]) -> bool {
        if !self.link || frame.len() < MIN_FRAME || frame.len() > MAX_FRAME {
            return false;
        }
        if self.rx.len() >= NIC_QUEUE {
            self.drops += 1;
            return false;
        }
        self.rx.push_back(frame.to_vec());
        self.irq_on
    }

    /// Frames the guest has sent, oldest first.
    pub fn take_tx(&mut self) -> Vec<Vec<u8>> {
        self.tx.drain(..).collect()
    }
}
