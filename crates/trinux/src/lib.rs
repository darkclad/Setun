//! Build Trinux disk images: the loader (sector 0) and the kernel image
//! (sectors 1..K). See docs/KERNEL.md §2 and docs/BOOT.md.

use setun_core::memory::word_to_trytes;
use setun_core::PAGE;
use std::path::{Path, PathBuf};
use ternary::Word;
use trine::{Kind, Options};

/// Kernel load address: the first word of frame 4.
pub const KERNEL_ORG: i64 = 4 * PAGE - 9840;
/// Kernel globals: frames 31..39.
pub const KERNEL_DATA: i64 = 31 * PAGE - 9840;
/// Kernel boot stack: the top word of frame 3.
pub const KERNEL_STACK: i64 = 3 * PAGE + 9840;
pub const KERNEL_MAX_SECTORS: usize = 27;
/// Words in the loader sector that mkdisk fills in (see kernel/boot/loader.tri).
pub const LOADER_KERNEL_SECTORS: i64 = 29_520;
pub const LOADER_KERNEL_ENTRY: i64 = 29_517;

pub fn kernel_options() -> Options {
    Options { org: KERNEL_ORG, data: KERNEL_DATA, stack: KERNEL_STACK, kind: Kind::Flat }
}

/// The repository's `kernel/` directory.
pub fn kernel_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../kernel")
}

pub struct Disk {
    /// T1B2 trytes, whole sectors.
    pub trytes: Vec<i16>,
    pub kernel_sectors: usize,
    pub programs: usize,
}

/// Sector holding the boot-module table (kernel/module.tri).
pub const MODULE_TABLE_SECTOR: usize = 28;
pub const MODULE_MAGIC: i64 = 31_857_270_900;

/// The repository's `user/` directory.
pub fn user_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../user")
}

/// The user programs a default disk starts (K2: two copies of the demo).
pub fn default_programs() -> Vec<PathBuf> {
    vec![user_dir().join("k2_hello.tri"), user_dir().join("k2_hello.tri")]
}

/// A user program as page images: page `first_page` onward, laid out so the
/// image starts at `USER_BASE − 1`.
fn program_pages(path: &Path) -> Result<Vec<i16>, String> {
    let image = trine::build_file(path, &Options::user())?.image;
    let page = PAGE as usize;
    let lead = (trine::USER_BASE - 1 - (trine::USER_BASE / PAGE * PAGE - 9841)) as usize; // 9840
    let mut pages = vec![0i16; lead];
    pages.extend_from_slice(&image);
    let n = pages.len().div_ceil(page);
    if n > 9 {
        return Err(format!("{}: {n} pages of code; at most 9 fit below the globals", path.display()));
    }
    pages.resize(n * page, 0);
    Ok(pages)
}

/// Compile the loader, the kernel and `programs`, and lay out the disk.
pub fn build_disk(kernel_dir: &Path) -> Result<Disk, String> {
    build_disk_with(kernel_dir, &default_programs())
}

pub fn build_disk_with(kernel_dir: &Path, programs: &[PathBuf]) -> Result<Disk, String> {
    let loader = trine::build_file(&kernel_dir.join("boot/loader.tri"), &Options::boot())?.image;
    let kernel = trine::build_file(&kernel_dir.join("main.tri"), &kernel_options())?.image;
    let page = PAGE as usize;
    let sectors = kernel.len().div_ceil(page);
    if sectors > KERNEL_MAX_SECTORS {
        return Err(format!("the kernel needs {sectors} sectors; at most {KERNEL_MAX_SECTORS} fit below the kernel data"));
    }
    let mut sector0 = loader;
    for (addr, value) in [(LOADER_KERNEL_SECTORS, sectors as i64), (LOADER_KERNEL_ENTRY, KERNEL_ORG)] {
        let first = (addr - 1 - trine::BOOT_FRAME_START) as usize;
        if sector0[first..first + 3].iter().any(|&t| t != 0) {
            return Err("the loader is too large: it overlaps the kernel descriptor words".into());
        }
        for (i, t) in word_to_trytes(Word::new(value).unwrap()).iter().enumerate() {
            sector0[first + i] = t.value() as i16;
        }
    }
    let mut trytes = sector0;
    trytes.extend_from_slice(&kernel);
    trytes.resize((1 + sectors) * page, 0);

    // Boot modules: the table at sector 28, programs from sector 29.
    trytes.resize((MODULE_TABLE_SECTOR + 1) * page, 0);
    let first_page = trine::USER_BASE / PAGE;
    let mut table = vec![MODULE_MAGIC, programs.len() as i64];
    for p in programs {
        let pages = program_pages(p)?;
        let start = trytes.len() / page;
        table.extend([start as i64, (pages.len() / page) as i64, first_page]);
        trytes.extend_from_slice(&pages);
    }
    if table.len() * 3 > page {
        return Err("too many boot programs for one table sector".into());
    }
    let t0 = MODULE_TABLE_SECTOR * page;
    for (j, &w) in table.iter().enumerate() {
        for (i, t) in word_to_trytes(Word::new(w).unwrap()).iter().enumerate() {
            trytes[t0 + 3 * j + i] = t.value() as i16;
        }
    }
    Ok(Disk { trytes, kernel_sectors: sectors, programs: programs.len() })
}

/// Boot a disk on a fresh machine with the BIOS; run `steps` instructions.
pub fn boot(disk: &Disk, ram_frames: i64, steps: u64) -> setun_core::Machine {
    let bios = trine::bios_rom().expect("the BIOS compiles");
    let mut m = setun_core::Machine::new(ram_frames, 1_790_000_000);
    m.load_rom(&bios);
    m.dev.disks[0].attach(Box::new(setun_core::devices::MemDisk::from_trytes(&disk.trytes)));
    m.run(steps);
    m
}
