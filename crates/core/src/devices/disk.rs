//! Block disk with DMA (ISA §8.2). A sector is one page (3^9 trytes).
//!
//! Registers: 0 `CMD` (−1 read, +1 write, 0 flush; writing starts it),
//! 1 `STATUS` (−1 error, 0 idle, +1 busy), 2 `LBA`, 3 `COUNT`, 4 `DMA`,
//! 5 `SECTORS`, 6 `ERROR`, 7 `CTRL` (+1 IRQ on completion).

use crate::memory::{frame_start, Memory};
use crate::{HALF_PAGE, PAGE};
use ternary::Tryte;

/// `ERROR` codes.
pub mod error {
    pub const NONE: i64 = 0;
    pub const NO_MEDIA: i64 = 1;
    pub const LBA_RANGE: i64 = 2;
    pub const DMA_ALIGN: i64 = 3;
    pub const BUS: i64 = 4;
    pub const IO: i64 = 5;
    pub const BAD_CMD: i64 = 6;
}

/// Backing storage for a disk: sectors of `PAGE` trytes.
pub trait BlockStore {
    fn sectors(&self) -> i64;
    fn read(&mut self, lba: i64, buf: &mut [i16]) -> bool;
    fn write(&mut self, lba: i64, buf: &[i16]) -> bool;
    fn flush(&mut self) -> bool {
        true
    }
}

/// An in-memory disk.
pub struct MemDisk {
    data: Vec<i16>,
}

impl MemDisk {
    pub fn new(sectors: i64) -> Self {
        MemDisk { data: vec![0; (sectors * PAGE) as usize] }
    }

    /// Build from raw trytes (padded to whole sectors).
    pub fn from_trytes(trytes: &[i16]) -> Self {
        let sectors = (trytes.len() as i64 + PAGE - 1) / PAGE;
        let mut d = Self::new(sectors.max(1));
        d.data[..trytes.len()].copy_from_slice(trytes);
        d
    }

    pub fn trytes(&self) -> &[i16] {
        &self.data
    }
}

impl BlockStore for MemDisk {
    fn sectors(&self) -> i64 {
        self.data.len() as i64 / PAGE
    }

    fn read(&mut self, lba: i64, buf: &mut [i16]) -> bool {
        let s = (lba * PAGE) as usize;
        buf.copy_from_slice(&self.data[s..s + PAGE as usize]);
        true
    }

    fn write(&mut self, lba: i64, buf: &[i16]) -> bool {
        let s = (lba * PAGE) as usize;
        self.data[s..s + PAGE as usize].copy_from_slice(buf);
        true
    }
}

/// A disk image file in T1B2 format: one tryte per little-endian `i16`
/// (ISA §8.5), so a sector is 39 366 bytes.
#[cfg(not(target_arch = "wasm32"))]
pub struct FileDisk {
    file: std::fs::File,
    sectors: i64,
}

#[cfg(not(target_arch = "wasm32"))]
impl FileDisk {
    const SECTOR_BYTES: u64 = 2 * PAGE as u64;

    /// Open an existing image read/write.
    pub fn open(path: &std::path::Path) -> std::io::Result<Self> {
        let file = std::fs::OpenOptions::new().read(true).write(true).open(path)?;
        let sectors = (file.metadata()?.len() / Self::SECTOR_BYTES) as i64;
        Ok(FileDisk { file, sectors })
    }

    /// Create (or truncate) a zero-filled image of `sectors` sectors.
    pub fn create(path: &std::path::Path, sectors: i64) -> std::io::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        file.set_len(sectors as u64 * Self::SECTOR_BYTES)?;
        Ok(FileDisk { file, sectors })
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl BlockStore for FileDisk {
    fn sectors(&self) -> i64 {
        self.sectors
    }

    fn read(&mut self, lba: i64, buf: &mut [i16]) -> bool {
        use std::io::{Read, Seek, SeekFrom};
        let mut bytes = vec![0u8; Self::SECTOR_BYTES as usize];
        let ok = self.file.seek(SeekFrom::Start(lba as u64 * Self::SECTOR_BYTES)).is_ok()
            && self.file.read_exact(&mut bytes).is_ok();
        if ok {
            for (t, b) in buf.iter_mut().zip(bytes.chunks_exact(2)) {
                *t = i16::from_le_bytes([b[0], b[1]]);
            }
        }
        ok
    }

    fn write(&mut self, lba: i64, buf: &[i16]) -> bool {
        use std::io::{Seek, SeekFrom, Write};
        let bytes: Vec<u8> = buf.iter().flat_map(|t| t.to_le_bytes()).collect();
        self.file.seek(SeekFrom::Start(lba as u64 * Self::SECTOR_BYTES)).is_ok()
            && self.file.write_all(&bytes).is_ok()
    }

    fn flush(&mut self) -> bool {
        self.file.sync_data().is_ok()
    }
}

#[derive(Default)]
pub struct Disk {
    store: Option<Box<dyn BlockStore>>,
    pending: Option<i64>,
    status: i64,
    lba: i64,
    count: i64,
    dma: i64,
    error: i64,
    irq_on: bool,
}

impl Disk {
    pub fn attach(&mut self, store: Box<dyn BlockStore>) {
        self.store = Some(store);
    }

    pub fn detach(&mut self) -> Option<Box<dyn BlockStore>> {
        self.store.take()
    }

    pub fn reset(&mut self) {
        let store = self.store.take();
        *self = Disk { store, ..Disk::default() };
    }

    pub fn read(&self, r: i64) -> i64 {
        match r {
            0 => self.pending.unwrap_or(0),
            1 => self.status,
            2 => self.lba,
            3 => self.count,
            4 => self.dma,
            5 => self.store.as_ref().map_or(0, |s| s.sectors()),
            6 => self.error,
            7 => self.irq_on as i64,
            _ => 0,
        }
    }

    pub fn write(&mut self, r: i64, v: i64) {
        match r {
            0 if self.status != 1 => {
                self.pending = Some(v);
                self.status = 1;
            }
            2 => self.lba = v,
            3 => self.count = v,
            4 => self.dma = v,
            7 => self.irq_on = v == 1,
            _ => {}
        }
    }

    /// Perform a pending command. True if it completed with the IRQ enabled.
    pub fn service(&mut self, mem: &mut Memory) -> bool {
        let Some(cmd) = self.pending.take() else {
            return false;
        };
        self.error = self.run(cmd, mem);
        self.status = if self.error == error::NONE { 0 } else { -1 };
        self.irq_on
    }

    fn run(&mut self, cmd: i64, mem: &mut Memory) -> i64 {
        let Some(store) = self.store.as_mut() else {
            return error::NO_MEDIA;
        };
        if cmd == 0 {
            return if store.flush() { error::NONE } else { error::IO };
        }
        if cmd != 1 && cmd != -1 {
            return error::BAD_CMD;
        }
        if self.lba < 0 || self.count < 0 || self.lba + self.count > store.sectors() {
            return error::LBA_RANGE;
        }
        if (self.dma + HALF_PAGE).rem_euclid(PAGE) != 0 {
            return error::DMA_ALIGN;
        }
        let first_frame = (self.dma + HALF_PAGE) / PAGE;
        let mut buf = vec![0i16; PAGE as usize];
        for k in 0..self.count {
            let base = frame_start(first_frame + k);
            if cmd == -1 {
                if !store.read(self.lba + k, &mut buf) {
                    return error::IO;
                }
                for (i, &t) in buf.iter().enumerate() {
                    let t = Tryte::new(t as i64).unwrap_or(Tryte::ZERO);
                    if mem.write_tryte(base + i as i64, t).is_err() {
                        return error::BUS;
                    }
                }
            } else {
                for (i, slot) in buf.iter_mut().enumerate() {
                    match mem.read_tryte(base + i as i64) {
                        Ok(t) => *slot = t.value() as i16,
                        Err(_) => return error::BUS,
                    }
                }
                if !store.write(self.lba + k, &buf) {
                    return error::IO;
                }
            }
        }
        error::NONE
    }
}
