//! Build Trinux disk images: the loader (sector 0) and the kernel image
//! (sectors 1..K). See docs/KERNEL.md §2 and docs/BOOT.md.

use setun_core::memory::word_to_trytes;
use setun_core::PAGE;
use std::path::{Path, PathBuf};
use ternary::Word;
use trine::{Kind, Options};

/// Kernel load address: the first word of frame 4.
pub const KERNEL_ORG: i64 = 4 * PAGE - 9840;
/// Kernel globals: frames 31..57.
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
    pub files: usize,
}

/// The initramfs header sector (kernel/initramfs.tri); data follows it.
pub const INITRAMFS_SECTOR: usize = 28;
pub const INITRAMFS_MAGIC: i64 = 31_857_270_902;
/// TEF executable magic (kernel/exec.tri).
pub const TEF_MAGIC: i64 = 31_857_270_901;

/// The repository's `user/` directory.
pub fn user_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../user")
}

/// The default initramfs: (path in the file system, Trine source).
pub fn default_files() -> Vec<(String, PathBuf)> {
    let u = user_dir();
    let mut v: Vec<(String, PathBuf)> = vec![("/sbin/init".into(), u.join("init.tri"))];
    for prog in ["sh", "echo", "cat", "ls", "mkdir", "rm", "true", "false", "k3test"] {
        v.push((format!("/bin/{prog}"), u.join(format!("bin/{prog}.tri"))));
    }
    v.push(("/bin/hello".into(), u.join("hello.tri")));
    v.push(("/bin/k2_hello".into(), u.join("k2_hello.tri")));
    v.push(("/etc/motd".into(), u.join("etc/motd")));
    v
}

/// The bytes of an initramfs file: `.tri` sources are compiled to TEF,
/// anything else is copied as text (one code point per tryte).
pub fn initramfs_file(src: &Path) -> Result<Vec<i16>, String> {
    if src.extension().is_some_and(|e| e == "tri") {
        return tef(src);
    }
    let text = std::fs::read_to_string(src).map_err(|e| format!("{}: {e}", src.display()))?;
    text.chars()
        .map(|c| i16::try_from(c as u32).ok().filter(|&t| t <= 9841).ok_or(format!("{}: {c:?} does not fit a tryte", src.display())))
        .collect()
}

fn push_word(v: &mut Vec<i16>, w: i64) {
    v.extend(word_to_trytes(Word::new(w).unwrap()).iter().map(|t| t.value() as i16));
}

/// Compile a user program into a TEF executable (header + text).
pub fn tef(path: &Path) -> Result<Vec<i16>, String> {
    let b = trine::build_file(path, &Options::user())?;
    let text_end = trine::USER_BASE - 1 + b.image.len() as i64;
    if text_end >= b.data_start - 1 {
        return Err(format!("{}: the code overlaps the globals", path.display()));
    }
    let mut v = Vec::new();
    for w in [
        TEF_MAGIC,
        trine::USER_BASE,
        trine::USER_BASE - 1,
        b.image.len() as i64,
        b.data_start - 1,
        b.data_end - b.data_start,
    ] {
        push_word(&mut v, w);
    }
    v.extend_from_slice(&b.image);
    Ok(v)
}

/// Compile the loader, the kernel and the default initramfs; lay out the disk.
pub fn build_disk(kernel_dir: &Path) -> Result<Disk, String> {
    build_disk_with(kernel_dir, &default_files())
}

pub fn build_disk_with(kernel_dir: &Path, files: &[(String, PathBuf)]) -> Result<Disk, String> {
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

    // initramfs: header at sector 28, file data from sector 29.
    let mut data: Vec<i16> = Vec::new();
    let mut entries: Vec<i16> = Vec::new();
    for (name, src) in files {
        let chars: Vec<i64> = name.chars().map(|c| c as i64).collect();
        if chars.len() > 26 {
            return Err(format!("initramfs path too long: {name}"));
        }
        let file = initramfs_file(src)?;
        let mut n: Vec<i16> = chars.iter().map(|&c| c as i16).collect();
        n.resize(27, 0);
        entries.extend_from_slice(&n);
        push_word(&mut entries, file.len() as i64);
        push_word(&mut entries, data.len() as i64);
        data.extend_from_slice(&file);
        data.resize(data.len().div_ceil(3) * 3, 0);
    }
    let data_sectors = data.len().div_ceil(page).max(1);
    let mut header = Vec::new();
    push_word(&mut header, INITRAMFS_MAGIC);
    push_word(&mut header, files.len() as i64);
    push_word(&mut header, data_sectors as i64);
    header.extend_from_slice(&entries);
    if header.len() > page {
        return Err("too many files for the initramfs header".into());
    }
    trytes.resize(INITRAMFS_SECTOR * page, 0);
    header.resize(page, 0);
    trytes.extend_from_slice(&header);
    data.resize(data_sectors * page, 0);
    trytes.extend_from_slice(&data);
    Ok(Disk { trytes, kernel_sectors: sectors, files: files.len() })
}

/// Boot a disk on a fresh machine with the BIOS; run `steps` instructions.
pub fn boot(disk: &Disk, ram_frames: i64, steps: u64) -> setun_core::Machine {
    boot_with_input(disk, ram_frames, steps, "")
}

/// Boot with `input` waiting on the serial console (the kernel's tty reads it).
pub fn boot_with_input(disk: &Disk, ram_frames: i64, steps: u64, input: &str) -> setun_core::Machine {
    let bios = trine::bios_rom().expect("the BIOS compiles");
    let mut m = setun_core::Machine::new(ram_frames, 1_790_000_000);
    m.load_rom(&bios);
    m.dev.disks[0].attach(Box::new(setun_core::devices::MemDisk::from_trytes(&disk.trytes)));
    m.dev.serial_input(input);
    m.run(steps);
    m
}
