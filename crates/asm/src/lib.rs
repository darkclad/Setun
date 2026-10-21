//! The Setun-11̅ assembler: assembly text to words (`text`), words back to
//! text (`disasm`), and the helpers that lay assembled words out as trytes
//! for ROM images and disk sectors. The language is described in
//! docs/ASSEMBLER.md; the `setun-asm` binary is its command line.

pub mod disasm;
pub mod text;

use ternary::{pow3, Word};

/// Pack trytes big-endian into words (padded with zero trytes).
pub fn pack_trytes(trytes: &[i64]) -> Vec<Word> {
    trytes
        .chunks(3)
        .map(|c| {
            let t = |i: usize| c.get(i).copied().unwrap_or(0) as i128;
            Word::wrap(t(0) * pow3(18) + t(1) * pow3(9) + t(2))
        })
        .collect()
}

/// Build the full 3-frame BIOS ROM image (3^10 trytes, starting at the
/// lowest ROM address) from assembled segments, which must all lie in ROM.
pub fn rom_image(segments: &[text::Segment]) -> Result<Vec<i16>, String> {
    use setun_core::memory::{frame_start, ROM_FIRST_FRAME, ROM_FRAMES};
    let start = frame_start(ROM_FIRST_FRAME);
    let len = (ROM_FRAMES * setun_core::PAGE) as usize;
    let mut rom = vec![0i16; len];
    for s in segments {
        let first = s.origin - 1 - start;
        let t = words_to_trytes(&s.words);
        if first < 0 || first as usize + t.len() > len {
            return Err(format!(
                "segment at {} ({} words) is outside ROM ({}..={})",
                s.origin,
                s.words.len(),
                start,
                start + len as i64 - 1
            ));
        }
        rom[first as usize..first as usize + t.len()].copy_from_slice(&t);
    }
    Ok(rom)
}

/// Convert assembled words into trytes (for ROM images and disk sectors).
pub fn words_to_trytes(words: &[Word]) -> Vec<i16> {
    words
        .iter()
        .flat_map(|&w| setun_core::memory::word_to_trytes(w).map(|t| t.value() as i16))
        .collect()
}
