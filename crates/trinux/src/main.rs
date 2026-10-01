//! `mkdisk`: build a bootable Trinux disk image.
//!
//! ```text
//! mkdisk [-o trinux.disk] [--kernel-dir DIR]
//! setun --disk trinux.disk
//! ```

use std::path::PathBuf;

fn main() {
    let mut out = PathBuf::from("trinux.disk");
    let mut dir = trinux::kernel_dir();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-o" => out = args.next().map(PathBuf::from).unwrap_or_else(|| usage()),
            "--kernel-dir" => dir = args.next().map(PathBuf::from).unwrap_or_else(|| usage()),
            _ => usage(),
        }
    }
    let disk = trinux::build_disk(&dir).unwrap_or_else(|e| {
        eprintln!("mkdisk: {e}");
        std::process::exit(1)
    });
    let bytes: Vec<u8> = disk.trytes.iter().flat_map(|t| t.to_le_bytes()).collect();
    std::fs::write(&out, bytes).unwrap_or_else(|e| {
        eprintln!("mkdisk: {}: {e}", out.display());
        std::process::exit(1)
    });
    eprintln!(
        "{}: loader + kernel ({} sector(s)), {} sectors in all",
        out.display(),
        disk.kernel_sectors,
        disk.trytes.len() / setun_core::PAGE as usize
    );
}

fn usage() -> ! {
    eprintln!("usage: mkdisk [-o trinux.disk] [--kernel-dir DIR]");
    std::process::exit(2)
}
