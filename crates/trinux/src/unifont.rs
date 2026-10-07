//! /usr/share/unifont: GNU Unifont (fonts/unifont, OFL 1.1 or GPL 2+ with
//! the font embedding exception) for planes 0 and 1, packed for the
//! machine. gfx.tri reads it a glyph at a time.
//!
//! Layout, in trytes:
//! - 0..3: `U`, `F`, 1 (the version);
//! - 3..: a directory of 512 words (3 trytes each, high first), one per
//!   page of 256 code points (U+0000..U+1FFFF): the offset of the page's
//!   record, or 0 if the font has nothing in that page;
//! - a page record: 256 trytes, each glyph's width (0 none, 1 = 8 pixels,
//!   2 = 16 pixels), then the glyphs present, in code point order. A glyph
//!   is 16 rows, top first, each row's pixels leftmost first, as a bit
//!   stream cut into 14-bit pieces (2^14 <= 3^9), each stored as its value
//!   minus 8192: 10 trytes for an 8-pixel glyph, 19 for a 16-pixel one.

use std::io::{BufRead, BufReader};
use std::path::Path;

pub const PAGES: usize = 512;
pub const HEADER: usize = 3;
pub const NARROW_TRYTES: usize = 10;
pub const WIDE_TRYTES: usize = 19;

/// The font's source files.
pub fn sources() -> Vec<std::path::PathBuf> {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fonts/unifont");
    vec![d.join("unifont-18.0.01.hex.gz"), d.join("unifont_upper-18.0.01.hex.gz")]
}

/// Glyphs by code point: (16 or 8 pixels wide, 16 rows of bits, leftmost
/// pixel as the highest bit of the row).
pub fn load(files: &[std::path::PathBuf]) -> Result<std::collections::BTreeMap<u32, (u8, [u16; 16])>, String> {
    let mut glyphs = std::collections::BTreeMap::new();
    for f in files {
        let file = std::fs::File::open(f).map_err(|e| format!("{}: {e}", f.display()))?;
        for line in BufReader::new(flate2::read::GzDecoder::new(file)).lines() {
            let line = line.map_err(|e| format!("{}: {e}", f.display()))?;
            let Some((cp, hex)) = line.trim().split_once(':') else { continue };
            let cp = u32::from_str_radix(cp, 16).map_err(|e| format!("{}: {e}", f.display()))?;
            if cp >= (PAGES as u32) << 8 {
                continue;
            }
            let wide = match hex.len() {
                32 => false,
                64 => true,
                n => return Err(format!("{}: U+{cp:04X}: {n} hex digits", f.display())),
            };
            let per = if wide { 4 } else { 2 };
            let mut rows = [0u16; 16];
            for (r, row) in rows.iter_mut().enumerate() {
                let v = u16::from_str_radix(&hex[r * per..(r + 1) * per], 16).map_err(|e| format!("{}: {e}", f.display()))?;
                *row = if wide { v } else { v << 8 };
            }
            glyphs.insert(cp, (if wide { 16 } else { 8 }, rows));
        }
    }
    Ok(glyphs)
}

/// The packed file.
pub fn build(glyphs: &std::collections::BTreeMap<u32, (u8, [u16; 16])>) -> Vec<i16> {
    let mut out: Vec<i16> = vec!['U' as i16, 'F' as i16, 1];
    out.resize(HEADER + 3 * PAGES, 0);
    for page in 0..PAGES as u32 {
        let in_page: Vec<_> = glyphs.range(page << 8..(page + 1) << 8).collect();
        if in_page.is_empty() {
            continue;
        }
        let at = out.len() as i64;
        let w = ternary::Word::new(at).unwrap();
        let d = HEADER + 3 * page as usize;
        out[d] = w.field(1..=9) as i16;
        out[d + 1] = w.field(10..=18) as i16;
        out[d + 2] = w.field(19..=27) as i16;
        let widths = out.len();
        out.resize(widths + 256, 0);
        for (&cp, &(width, rows)) in in_page {
            out[widths + (cp & 255) as usize] = if width == 16 { 2 } else { 1 };
            let mut bits: Vec<bool> = Vec::new();
            for row in rows {
                for k in 0..width {
                    bits.push(row & (0x8000 >> k) != 0);
                }
            }
            let n = if width == 16 { WIDE_TRYTES } else { NARROW_TRYTES };
            bits.resize(n * 14, false);
            for chunk in bits.chunks(14) {
                let v = chunk.iter().fold(0i16, |a, &b| a * 2 + b as i16);
                out.push(v - 8192);
            }
        }
    }
    out
}

/// A glyph read back from the packed file, as gfx.tri reads it (for tests).
pub fn glyph(file: &[i16], cp: u32) -> Option<(u8, [u16; 16])> {
    let page = (cp >> 8) as usize;
    if page >= PAGES {
        return None;
    }
    let d = HEADER + 3 * page;
    let at = (file[d] as i64 * 19_683 + file[d + 1] as i64) * 19_683 + file[d + 2] as i64;
    if at == 0 {
        return None;
    }
    let at = at as usize;
    let lo = (cp & 255) as usize;
    let size = |w: i16| match w {
        1 => NARROW_TRYTES,
        2 => WIDE_TRYTES,
        _ => 0,
    };
    let w = file[at + lo];
    if w == 0 {
        return None;
    }
    let off = at + 256 + (0..lo).map(|j| size(file[at + j])).sum::<usize>();
    let width = if w == 2 { 16 } else { 8 };
    let bits: Vec<bool> = file[off..off + size(w)]
        .iter()
        .flat_map(|&t| {
            let v = t + 8192;
            (0..14).rev().map(move |k| v >> k & 1 != 0)
        })
        .collect();
    let mut rows = [0u16; 16];
    for (r, row) in rows.iter_mut().enumerate() {
        for k in 0..width {
            if bits[r * width as usize + k as usize] {
                *row |= 0x8000 >> k;
            }
        }
    }
    Some((width, rows))
}
