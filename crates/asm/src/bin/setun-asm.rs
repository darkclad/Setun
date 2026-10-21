//! Setun-11̅ assembler and disassembler.
//!
//! ```text
//! setun-asm build IN.s [-o OUT.t1b2] [--rom] [--listing]
//! setun-asm dis IN.t1b2 [--rom | --origin N] [--source]
//! ```
//!
//! Images are T1B2: one tryte per little-endian i16. `--rom` builds or reads
//! the full 3-frame BIOS ROM image that `setun --rom` loads.

use setun_asm::{disasm, rom_image, text, words_to_trytes};
use setun_core::memory::{frame_start, ROM_FIRST_FRAME};
use std::path::{Path, PathBuf};
use std::process::exit;
use ternary::{pow3, Word};

fn usage() -> ! {
    eprintln!(
        "usage:\n  setun-asm build IN.s [-o OUT.t1b2] [--rom] [--listing]\n  setun-asm dis IN.t1b2 [--rom | --origin N] [--source]\n\n\
         build   assemble; --rom writes the full 3^10-tryte BIOS ROM image, otherwise\n\
         \x20       the single segment's words (first tryte = origin - 1)\n\
         dis     disassemble; --rom assumes the ROM layout, --origin gives the word\n\
         \x20       address of the first word; --source prints reassemblable source"
    );
    exit(2)
}

fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("setun-asm: {msg}");
    exit(1)
}

fn write_t1b2(path: &Path, trytes: &[i16]) {
    let bytes: Vec<u8> = trytes.iter().flat_map(|t| t.to_le_bytes()).collect();
    std::fs::write(path, bytes).unwrap_or_else(|e| fail(format!("{}: {e}", path.display())));
}

fn read_t1b2(path: &Path) -> Vec<i16> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| fail(format!("{}: {e}", path.display())));
    if !bytes.len().is_multiple_of(2) {
        fail(format!("{}: odd length, not a T1B2 image", path.display()));
    }
    bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
}

fn build(args: &[String]) {
    let mut input = None;
    let (mut out, mut rom, mut listing) = (None, false, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" => out = Some(PathBuf::from(it.next().unwrap_or_else(|| usage()))),
            "--rom" => rom = true,
            "--listing" => listing = true,
            _ if input.is_none() && !a.starts_with('-') => input = Some(PathBuf::from(a)),
            _ => usage(),
        }
    }
    let input = input.unwrap_or_else(|| usage());
    let src = std::fs::read_to_string(&input).unwrap_or_else(|e| fail(format!("{}: {e}", input.display())));
    let p = text::assemble(&src).unwrap_or_else(|e| fail(format!("{}: {e}", input.display())));
    if listing {
        print!("{}", p.listing_text(&src));
    }
    let trytes = if rom {
        rom_image(&p.segments).unwrap_or_else(|e| fail(e))
    } else {
        match p.segments.as_slice() {
            [s] => words_to_trytes(&s.words),
            [] => Vec::new(),
            _ => fail("several .org segments: use --rom, or keep one segment"),
        }
    };
    let out = out.unwrap_or_else(|| input.with_extension("t1b2"));
    write_t1b2(&out, &trytes);
    let words: usize = p.segments.iter().map(|s| s.words.len()).sum();
    eprintln!(
        "{} → {}: {} segment(s), {} words, {} symbols",
        input.display(),
        out.display(),
        p.segments.len(),
        words,
        p.symbols.len()
    );
}

fn dis(args: &[String]) {
    let mut input = None;
    let (mut origin, mut rom, mut source) = (None, false, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--rom" => rom = true,
            "--source" => source = true,
            "--origin" => origin = Some(it.next().and_then(|v| v.parse::<i64>().ok()).unwrap_or_else(|| usage())),
            _ if input.is_none() && !a.starts_with('-') => input = Some(PathBuf::from(a)),
            _ => usage(),
        }
    }
    let input = input.unwrap_or_else(|| usage());
    let trytes = read_t1b2(&input);
    let origin = match (rom, origin) {
        (true, _) => frame_start(ROM_FIRST_FRAME) + 1,
        (false, Some(o)) => o,
        (false, None) => fail("give --rom or --origin N"),
    };
    if origin.rem_euclid(3) != 0 {
        fail(format!("origin {origin} is not word-aligned"));
    }
    let words: Vec<Word> = trytes
        .chunks(3)
        .map(|c| {
            let t = |i: usize| c.get(i).copied().unwrap_or(0) as i128;
            Word::wrap(t(0) * pow3(18) + t(1) * pow3(9) + t(2))
        })
        .collect();
    // Skip runs of 9+ zero words (empty ROM, padding).
    let mut i = 0;
    while i < words.len() {
        let start = i;
        let mut zeros = 0;
        while i < words.len() {
            if words[i] == Word::ZERO {
                zeros += 1;
                if zeros >= 9 {
                    break;
                }
            } else {
                zeros = 0;
            }
            i += 1;
        }
        let end = if zeros >= 9 { i + 1 - zeros } else { i };
        if end > start {
            let at = origin + 3 * start as i64;
            if source {
                print!("{}", disasm::disasm_source(&words[start..end], at));
            } else {
                print!("{}", disasm::disasm(&words[start..end], at));
            }
        }
        while i < words.len() && words[i] == Word::ZERO {
            i += 1;
        }
        if !source && i > end && i < words.len() {
            println!("{:>14}  … {} zero words", origin + 3 * end as i64, i - end);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("build") => build(&args[1..]),
        Some("dis") => dis(&args[1..]),
        _ => usage(),
    }
}
