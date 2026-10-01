//! Physical memory map (ISA §4).
//!
//! Frame `f` is centred on `PAGE·f` and covers `PAGE·f ± HALF_PAGE`.
//! Non-negative frames are RAM, negative frames are devices:
//!
//! | frames      | contents                          |
//! |-------------|-----------------------------------|
//! | 0 … N−1     | RAM (allocated lazily, reads 0)   |
//! | −3 … −1     | BIOS ROM (read-only)              |
//! | −81 … −55   | framebuffer (27 frames = 3^12)    |
//! | other       | bus error                         |

use crate::{HALF_PAGE, PAGE};
use ternary::{div_pow3_round, pow3, wrap_trits, Tryte, Word};

pub const ROM_FIRST_FRAME: i64 = -3;
pub const ROM_FRAMES: i64 = 3;
pub const FB_FIRST_FRAME: i64 = -81;
pub const FB_FRAMES: i64 = 27;
/// Default RAM: 3^9 frames = 3^18 trytes.
pub const DEFAULT_RAM_FRAMES: i64 = 19_683;

/// Lowest physical address of a frame.
pub const fn frame_start(frame: i64) -> i64 {
    frame * PAGE - HALF_PAGE
}

/// Access to a physical address with nothing behind it (or a ROM write).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BusError;

pub struct Memory {
    ram: Vec<Option<Box<[i16]>>>,
    rom: Vec<i16>,
    fb: Vec<i16>,
}

enum Region {
    Ram(usize),
    Rom,
    Fb,
}

impl Memory {
    /// Memory with `ram_frames` frames of RAM (frames `0..ram_frames`).
    pub fn new(ram_frames: i64) -> Self {
        assert!(ram_frames >= 1, "need at least one RAM frame");
        Memory {
            ram: (0..ram_frames).map(|_| None).collect(),
            rom: vec![0; (ROM_FRAMES * PAGE) as usize],
            fb: vec![0; (FB_FRAMES * PAGE) as usize],
        }
    }

    pub fn ram_frames(&self) -> i64 {
        self.ram.len() as i64
    }

    fn locate(&self, pa: i64) -> Result<(Region, usize), BusError> {
        let frame = div_pow3_round(pa as i128, 9) as i64;
        let idx = (pa - frame * PAGE + HALF_PAGE) as usize;
        if (0..self.ram_frames()).contains(&frame) {
            Ok((Region::Ram(frame as usize), idx))
        } else if (ROM_FIRST_FRAME..ROM_FIRST_FRAME + ROM_FRAMES).contains(&frame) {
            Ok((Region::Rom, (frame - ROM_FIRST_FRAME) as usize * PAGE as usize + idx))
        } else if (FB_FIRST_FRAME..FB_FIRST_FRAME + FB_FRAMES).contains(&frame) {
            Ok((Region::Fb, (frame - FB_FIRST_FRAME) as usize * PAGE as usize + idx))
        } else {
            Err(BusError)
        }
    }

    pub fn read_tryte(&self, pa: i64) -> Result<Tryte, BusError> {
        let v = match self.locate(pa)? {
            (Region::Ram(f), i) => self.ram[f].as_ref().map_or(0, |fr| fr[i]),
            (Region::Rom, i) => self.rom[i],
            (Region::Fb, i) => self.fb[i],
        };
        Ok(Tryte::new(v as i64).expect("stored tryte in range"))
    }

    pub fn write_tryte(&mut self, pa: i64, t: Tryte) -> Result<(), BusError> {
        let v = t.value() as i16;
        match self.locate(pa)? {
            (Region::Ram(f), i) => {
                let fr = self.ram[f].get_or_insert_with(|| vec![0; PAGE as usize].into_boxed_slice());
                fr[i] = v;
            }
            (Region::Rom, _) => return Err(BusError),
            (Region::Fb, i) => self.fb[i] = v,
        }
        Ok(())
    }

    /// Word at centre address `pa` (trytes `pa−1, pa, pa+1`, big-endian).
    /// Alignment is the caller's business.
    pub fn read_word(&self, pa: i64) -> Result<Word, BusError> {
        // Fast path: when all three trytes are in one frame, a single lookup
        // covers them (always true for aligned words).
        if let Ok((region, i)) = self.locate(pa) {
            let off = i % PAGE as usize;
            if off >= 1 && off + 1 < PAGE as usize {
                let s: &[i16] = match region {
                    Region::Ram(f) => match &self.ram[f] {
                        Some(fr) => fr,
                        None => return Ok(Word::ZERO),
                    },
                    Region::Rom => &self.rom,
                    Region::Fb => &self.fb,
                };
                let v = s[i - 1] as i64 * 387_420_489 + s[i] as i64 * 19_683 + s[i + 1] as i64;
                return Ok(Word::new(v).expect("word from trytes"));
            }
        }
        let t0 = self.read_tryte(pa - 1)?.value() as i128;
        let t1 = self.read_tryte(pa)?.value() as i128;
        let t2 = self.read_tryte(pa + 1)?.value() as i128;
        Ok(Word::wrap(t0 * pow3(18) + t1 * pow3(9) + t2))
    }

    pub fn write_word(&mut self, pa: i64, w: Word) -> Result<(), BusError> {
        let [t0, t1, t2] = word_to_trytes(w);
        // Check all three first so a failing word write has no effect.
        for a in [pa - 1, pa, pa + 1] {
            self.locate(a)?;
        }
        self.write_tryte(pa - 1, t0)?;
        self.write_tryte(pa, t1)?;
        self.write_tryte(pa + 1, t2)
    }

    /// Store words at consecutive word addresses starting at `pa` (host-side
    /// loading, may target ROM).
    pub fn load_words(&mut self, pa: i64, words: &[Word]) {
        for (k, &w) in words.iter().enumerate() {
            let a = pa + 3 * k as i64;
            for (j, t) in word_to_trytes(w).into_iter().enumerate() {
                self.poke(a - 1 + j as i64, t);
            }
        }
    }

    /// Host-side write that ignores ROM protection (used to load the BIOS).
    pub fn poke(&mut self, pa: i64, t: Tryte) {
        match self.locate(pa).expect("poke outside the memory map") {
            (Region::Rom, i) => self.rom[i] = t.value() as i16,
            _ => self.write_tryte(pa, t).unwrap(),
        }
    }

    /// Raw framebuffer trytes (27 frames), for the display device.
    pub fn framebuffer(&self) -> &[i16] {
        &self.fb
    }
}

/// Split a word into its three trytes, most significant first.
pub fn word_to_trytes(w: Word) -> [Tryte; 3] {
    let v = w.value() as i128;
    let t2 = wrap_trits(v, 9);
    let v1 = (v - t2) / pow3(9);
    let t1 = wrap_trits(v1, 9);
    let t0 = (v1 - t1) / pow3(9);
    [t0, t1, t2].map(|t| Tryte::new(t as i64).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_boundaries() {
        let mut m = Memory::new(3);
        // RAM frames 0..=2: −9841 ..= 2·19683 + 9841.
        assert!(m.read_tryte(-9841).is_ok());
        assert!(m.read_tryte(-9842).is_ok()); // frame −1 is ROM ...
        assert_eq!(frame_start(-3), -68_890);
        assert!(m.read_tryte(-68_890).is_ok()); // ... ROM starts here
        assert!(m.read_tryte(-68_891).is_err()); // frame −4: nothing
        assert!(m.read_tryte(2 * 19683 + 9841).is_ok());
        assert!(m.read_tryte(3 * 19683 - 9841).is_err()); // frame 3: no RAM
        assert_eq!(frame_start(FB_FIRST_FRAME), -1_604_164);
        assert!(m.write_tryte(-1_604_164, Tryte::MAX).is_ok());
        assert_eq!(m.write_tryte(-59_049, Tryte::MAX), Err(BusError)); // ROM
    }

    #[test]
    fn words_roundtrip_and_are_big_endian() {
        let mut m = Memory::new(1);
        let w = Word::from_b27("ABC123D98").unwrap();
        m.write_word(300, w).unwrap();
        assert_eq!(m.read_word(300).unwrap(), w);
        assert_eq!(m.read_tryte(299).unwrap().to_b27(), "ABC");
        assert_eq!(m.read_tryte(301).unwrap().to_b27(), "D98");
        for v in [Word::MAX, Word::MIN, Word::ZERO, Word::new(-1).unwrap()] {
            let t = word_to_trytes(v);
            m.write_word(0, v).unwrap();
            assert_eq!(m.read_word(0).unwrap(), v, "{t:?}");
        }
    }
}
