//! `mkdisk`: build a bootable Trinux disk image.
//!
//! ```text
//! mkdisk [-o trinux.disk] [--kernel-dir DIR]
//! mkdisk --update trinux.disk      the loader and kernel of a fresh build,
//!                                  the files of the disk kept
//! setun --disk trinux.disk
//! ```

use std::path::PathBuf;

fn main() {
    let mut out = PathBuf::from("trinux.disk");
    let mut dir = trinux::kernel_dir();
    let mut update = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-o" => out = args.next().map(PathBuf::from).unwrap_or_else(|| usage()),
            "--kernel-dir" => dir = args.next().map(PathBuf::from).unwrap_or_else(|| usage()),
            "--update" => update = Some(args.next().map(PathBuf::from).unwrap_or_else(|| usage())),
            _ => usage(),
        }
    }
    let disk = trinux::build_disk(&dir).unwrap_or_else(|e| {
        eprintln!("mkdisk: {e}");
        std::process::exit(1)
    });
    if let Some(path) = update {
        return update_disk(&path, &disk);
    }
    let bytes: Vec<u8> = disk.trytes.iter().flat_map(|t| t.to_le_bytes()).collect();
    std::fs::write(&out, bytes).unwrap_or_else(|e| {
        eprintln!("mkdisk: {}: {e}", out.display());
        std::process::exit(1)
    });
    let fs = trinux::tfs_of(&disk.trytes).expect("the volume just built");
    eprintln!(
        "{}: loader + kernel ({} sector(s)) + tfs root at sector {} ({} files, {} of {} blocks free), {} sectors in all",
        out.display(),
        disk.kernel_sectors,
        trinux::TFS_START,
        disk.files,
        fs.sb(trinux::tfs::SB_FREE_BLOCKS),
        fs.nblocks(),
        disk.trytes.len() / setun_core::PAGE as usize
    );
}

/// `--update`: the boot area of `fresh` into the disk at `path`, written
/// beside it and renamed over it, so a failure leaves the old disk whole.
fn update_disk(path: &PathBuf, fresh: &trinux::Disk) {
    let fail = |e: String| -> ! {
        eprintln!("mkdisk: {}: {e}", path.display());
        std::process::exit(1)
    };
    let bytes = std::fs::read(path).unwrap_or_else(|e| fail(e.to_string()));
    if bytes.len() % 2 != 0 {
        fail("an odd number of bytes: not a .t1b2 image".into());
    }
    let mut trytes: Vec<i16> = bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    trinux::update_boot(&mut trytes, &fresh.trytes).unwrap_or_else(|e| fail(e));
    let tmp = path.with_extension("update");
    let out: Vec<u8> = trytes.iter().flat_map(|t| t.to_le_bytes()).collect();
    std::fs::write(&tmp, out).unwrap_or_else(|e| fail(e.to_string()));
    std::fs::rename(&tmp, path).unwrap_or_else(|e| fail(e.to_string()));
    eprintln!("{}: loader + kernel ({} sector(s)) replaced, files kept", path.display(), fresh.kernel_sectors);
}

fn usage() -> ! {
    eprintln!("usage: mkdisk [-o trinux.disk] [--kernel-dir DIR]\n       mkdisk --update trinux.disk [--kernel-dir DIR]");
    std::process::exit(2)
}
