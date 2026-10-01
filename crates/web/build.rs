//! Build the BIOS ROM and the Trinux boot disk (the same as `mkdisk`) for the
//! browser, and embed them: the ROM as T1B2 trytes, the disk zero-run encoded
//! (see `decode_disk` in lib.rs), because most of its 3^6 sectors are empty.

use std::io::Write;
use std::path::PathBuf;

fn main() {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for dir in ["kernel", "user", "bios", "crates/trine/lib", "crates/trine/src", "crates/trinux/src", "crates/asm/src"] {
        println!("cargo:rerun-if-changed={}", root.join(dir).display());
    }

    let rom = trine::bios_rom().expect("the BIOS compiles");
    write_trytes(&out.join("bios.t1b2"), &rom);

    let disk = trinux::build_disk(&trinux::kernel_dir()).expect("the Trinux disk builds");
    let mut f = std::fs::File::create(out.join("trinux.rle")).unwrap();
    f.write_all(&encode(&disk.trytes)).unwrap();
}

fn write_trytes(path: &std::path::Path, t: &[i16]) {
    let bytes: Vec<u8> = t.iter().flat_map(|v| v.to_le_bytes()).collect();
    std::fs::write(path, bytes).unwrap();
}

/// "TDSK", the tryte count (u32), then chunks: a run of zeros (u32), a count
/// of literal trytes (u32), the literals (i16 each). Little-endian throughout.
fn encode(t: &[i16]) -> Vec<u8> {
    let mut out = b"TDSK".to_vec();
    out.extend((t.len() as u32).to_le_bytes());
    let mut i = 0;
    while i < t.len() {
        let z = i;
        while i < t.len() && t[i] == 0 {
            i += 1;
        }
        let s = i;
        // literals run until 9 zeros in a row (shorter gaps stay literal)
        while i < t.len() && !(t[i] == 0 && t[i..].iter().take(9).all(|&v| v == 0)) {
            i += 1;
        }
        out.extend(((s - z) as u32).to_le_bytes());
        out.extend(((i - s) as u32).to_le_bytes());
        out.extend(t[s..i].iter().flat_map(|v| v.to_le_bytes()));
    }
    out
}
