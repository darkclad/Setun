//! Build Trinux disk images: the loader (sector 0), the kernel image
//! (sectors 1..K) and the root file system: a tfs volume from sector 243
//! (the default), or an initramfs at sector 28. See docs/KERNEL.md §2, §19
//! and docs/BOOT.md.

pub mod tfs;
pub mod unifont;

use setun_core::memory::word_to_trytes;
use setun_core::PAGE;
use std::path::{Path, PathBuf};
use ternary::Word;
use trine::{Kind, Options};

/// Kernel load address: the first word of frame 4.
pub const KERNEL_ORG: i64 = 4 * PAGE - 9840;
/// Kernel globals: frames 31..57.
pub const KERNEL_DATA: i64 = 31 * PAGE - 9840;
/// First address past the globals (the triddy allocator owns frame 58 on).
pub const KERNEL_DATA_END: i64 = 58 * PAGE - 9841;
/// Kernel boot stack: the top word of frame 3.
pub const KERNEL_STACK: i64 = 3 * PAGE + 9840;
pub const KERNEL_MAX_SECTORS: usize = 27;
/// Words in the loader sector that mkdisk fills in (see kernel/boot/loader.tri).
pub const LOADER_KERNEL_SECTORS: i64 = 29_520;
pub const LOADER_KERNEL_ENTRY: i64 = 29_517;

pub fn kernel_options() -> Options {
    Options { org: KERNEL_ORG, data: KERNEL_DATA, stack: KERNEL_STACK, kind: Kind::Flat, optimize: true, level: None }
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

/// The tfs root volume starts here on the boot disk (kernel/params.tri).
pub const TFS_START: usize = 243;
/// Default disk: 3^6 sectors, so the volume has 486 blocks.
pub const DISK_SECTORS: usize = 729;
pub const TFS_INODES: usize = 729;

/// How to lay out the root file system.
#[derive(Clone, Debug)]
pub enum Root {
    /// A tfs volume of `blocks` blocks and `inodes` inodes at TFS_START.
    Tfs { blocks: usize, inodes: usize },
    /// The K3 initramfs (a ramfs root, nothing persists).
    Initramfs,
}

impl Default for Root {
    fn default() -> Self {
        Root::Tfs { blocks: DISK_SECTORS - TFS_START, inodes: TFS_INODES }
    }
}

/// Directories every tfs root gets (mount points and the like).
pub const ROOT_DIRS: [&str; 10] = ["/dev", "/proc", "/tmp", "/home", "/mnt", "/etc", "/root", "/home/user", "/var", "/var/log"];

/// Owners and modes that differ from the defaults (root:root, 0755 for
/// programs and directories, 0644 for text): the password hashes are root's
/// alone, su and passwd run set-user-ID root, and each user owns a home.
pub const SPECIAL_MODES: [(&str, i64, i64, i64); 7] = [
    ("/etc/shadow", 0o600, 0, 0),
    ("/bin/su", 0o4755, 0, 0),
    ("/bin/passwd", 0o4755, 0, 0),
    ("/bin/ping", 0o4755, 0, 0),
    ("/bin/tping", 0o4755, 0, 0),
    ("/root", 0o700, 0, 0),
    ("/home/user", 0o755, 1000, 1000),
];

/// The initramfs header sector (kernel/initramfs.tri); data follows it.
pub const INITRAMFS_SECTOR: usize = 28;
pub const INITRAMFS_MAGIC: i64 = 115_067_810_202;
/// TEF executable magic (kernel/exec.tri).
pub const TEF_MAGIC: i64 = 115_067_810_201;

/// The repository's `user/` directory.
pub fn user_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../user")
}

/// A generated file rather than a source: the 8x8 font for the window system.
pub const FONT_SOURCE: &str = "<font8x8>";

/// Another: GNU Unifont, packed (unifont.rs).
pub const UNIFONT_SOURCE: &str = "<unifont>";

/// The packed Unifont, built once.
pub fn unifont_file() -> Result<Vec<i16>, String> {
    static FILE: std::sync::OnceLock<Result<Vec<i16>, String>> = std::sync::OnceLock::new();
    FILE.get_or_init(|| unifont::load(&unifont::sources()).map(|g| unifont::build(&g))).clone()
}

/// Glyphs in the font file: code points 0..255, then the Cyrillic block
/// U+0400..U+045F as glyphs 256..351, then № as glyph 352.
pub const FONT_GLYPHS: usize = 353;

/// The font file's glyph for a code point, if it has one.
pub fn font_index(cp: u32) -> Option<usize> {
    use setun_core::devices::{CYRILLIC_FIRST, CYRILLIC_LAST};
    match cp {
        0..=255 => Some(cp as usize),
        CYRILLIC_FIRST..=CYRILLIC_LAST => Some(256 + (cp - CYRILLIC_FIRST) as usize),
        setun_core::devices::NUMERO => Some(352),
        _ => None,
    }
}

/// The code point of font file glyph `i` (the inverse of `font_index`).
pub fn font_char(i: usize) -> char {
    let cp = match i {
        0..=255 => i as u32,
        352 => setun_core::devices::NUMERO,
        _ => setun_core::devices::CYRILLIC_FIRST + (i - 256) as u32,
    };
    char::from_u32(cp).unwrap_or('?')
}

/// The window system's font: FONT_GLYPHS glyphs, eight rows each, one tryte
/// per row (bit 0 = leftmost pixel), from the same font as the emulator's
/// text mode.
pub fn font8x8_table() -> Vec<i16> {
    let mut v = Vec::with_capacity(FONT_GLYPHS * 8);
    for i in 0..FONT_GLYPHS {
        let cp = font_char(i) as u32;
        let g = setun_core::devices::glyph8x8(cp).unwrap_or([0; 8]);
        v.extend(g.iter().map(|&b| b as i16));
    }
    v
}

/// The default root file system: (path in the file system, source).
pub fn default_files() -> Vec<(String, PathBuf)> {
    let u = user_dir();
    let mut v: Vec<(String, PathBuf)> = vec![("/sbin/init".into(), u.join("init.tri"))];
    for prog in [
        "sh", "echo", "cat", "ls", "mkdir", "rm", "true", "false", "sleep", "ps", "mount", "umount", "sync", "mv", "touch",
        "df", "head", "tail", "wc", "grep", "sort", "uniq", "cp", "ln", "chmod", "k3test", "sigtest", "fstest", "libctest", "mtxtest", "vectest", "chartest", "dstest",
        "fdcheck", "trwm", "term", "clock", "colors", "paint", "k8test", "edit", "login", "su", "passwd", "fputest", "calc",
        "nc", "ping", "netstat", "nettest", "cryptotest", "pktest", "fmttest", "fmtvec", "tping", "pqtest", "x509", "certgen", "tas", "tric", "host", "wget", "ssh", "ssh-keygen", "scp", "sftp", "ntpdate",
    ] {
        v.push((format!("/bin/{prog}"), u.join(format!("bin/{prog}.tri"))));
    }
    // One multi-call program under many names (hard links on tfs).
    // The ternary data formats (docs/FORMATS.md).
    for applet in ["ttv", "wav2tau", "tau2wav", "tone", "tauinfo", "ppm2tim", "tim2ppm", "timinfo", "timview"] {
        v.push((format!("/bin/{applet}"), u.join("bin/media.tri")));
    }
    for applet in [
        "rmdir", "basename", "dirname", "env", "printenv", "uname", "date", "clear", "seq", "tee", "yes", "kill", "test", "[",
        "id", "whoami", "groups", "chown", "chgrp", "stat", "time",
    ] {
        v.push((format!("/bin/{applet}"), u.join("bin/misc.tri")));
    }
    v.push(("/sbin/ifconfig".into(), u.join("bin/ifconfig.tri")));
    v.push(("/sbin/telnetd".into(), u.join("bin/telnetd.tri")));
    v.push(("/sbin/sshd".into(), u.join("bin/sshd.tri")));
    v.push(("/sbin/httpd".into(), u.join("bin/httpd.tri")));
    v.push(("/var/www/index.html".into(), u.join("var/www/index.html")));
    v.push(("/usr/lib/sftp-server".into(), u.join("bin/sftp-server.tri")));
    for name in ["useradd", "userdel"] {
        v.push((format!("/sbin/{name}"), u.join("bin/useradm.tri")));
    }
    // The text and file tools, likewise one program.
    for tool in ["sed", "tr", "cut", "od", "diff", "find", "xargs", "md5sum"] {
        v.push((format!("/bin/{tool}"), u.join("bin/tools.tri")));
    }
    // One test client under several names; the name picks how it misbehaves.
    for name in ["nocreate", "unknown", "huge", "crash", "bigwin"] {
        v.push((format!("/usr/lib/trwtest/{name}"), u.join("bin/badclient.tri")));
    }
    for name in ["reboot", "poweroff", "halt"] {
        v.push((format!("/sbin/{name}"), u.join("bin/reboot.tri")));
    }
    v.push(("/bin/hello".into(), u.join("hello.tri")));
    v.push(("/bin/k2_hello".into(), u.join("k2_hello.tri")));
    v.push(("/etc/motd".into(), u.join("etc/motd")));
    v.push(("/etc/pangrams".into(), u.join("etc/pangrams")));
    for db in ["passwd", "group", "shadow", "hosts", "resolv.conf"] {
        v.push((format!("/etc/{db}"), u.join(format!("etc/{db}"))));
    }
    v.push(("/etc/ssl/cert.pem".into(), u.join("etc/ssl/cert.pem")));
    v.push(("/usr/share/fmtvectors.txt".into(), u.join("share/fmtvectors.txt")));
    v.push(("/usr/share/font8x8".into(), PathBuf::from(FONT_SOURCE)));
    v.push(("/usr/share/unifont".into(), PathBuf::from(UNIFONT_SOURCE)));
    v
}

/// Is the file at `name` a program compiled from `src` (a `.tri` source
/// put at a name that is not itself a `.tri`)?
fn compiled(name: &str, src: &Path) -> bool {
    src.extension().is_some_and(|e| e == "tri") && !name.ends_with(".tri")
}

/// The trytes of the file at `name` on a disk: a `.tri` source is compiled
/// to TEF, unless it goes in as a source (a name ending in `.tri`); a
/// `.wav`, `.ppm`, `.pgm` or `.bin` file is copied octet for octet; anything
/// else is copied as text (one code point per tryte).
pub fn disk_file(name: &str, src: &Path) -> Result<Vec<i16>, String> {
    if src == Path::new(FONT_SOURCE) {
        return Ok(font8x8_table());
    }
    if src == Path::new(UNIFONT_SOURCE) {
        return unifont_file();
    }
    if compiled(name, src) {
        return tef(src);
    }
    // binary files from the border: one octet per tryte
    if src.extension().is_some_and(|e| ["wav", "ppm", "pgm", "bin"].iter().any(|b| e == *b)) {
        let bytes = std::fs::read(src).map_err(|e| format!("{}: {e}", src.display()))?;
        return Ok(bytes.into_iter().map(|b| b as i16).collect());
    }
    let text = std::fs::read_to_string(src).map_err(|e| format!("{}: {e}", src.display()))?;
    // a character followed by U+0305 is stored overlined (negated)
    let cps = ternary::code_points(&text).map_err(|m| format!("{}: {m}", src.display()))?;
    Ok(cps.into_iter().map(|t| t as i16).collect())
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

/// Compile the loader, the kernel and the default files; lay out the disk.
/// The disk also carries the Trine sources `tric` needs (docs/SELFHOST.md).
pub fn build_disk(kernel_dir: &Path) -> Result<Disk, String> {
    let mut files = default_files();
    files.extend(source_files());
    build_disk_with(kernel_dir, &files)
}

/// Sources for building programs in Trinux: the libraries in /usr/src/lib,
/// the standard library in /usr/lib/trine, the compiler's own sources (so
/// it can rebuild itself) and /usr/src/hello.tri.
pub fn source_files() -> Vec<(String, PathBuf)> {
    let u = user_dir();
    let tree = |d: &Path| -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(d)
            .map(|r| r.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "tri")).collect())
            .unwrap_or_default();
        v.sort();
        v
    };
    let name = |p: &Path| p.file_name().unwrap().to_string_lossy().into_owned();
    let mut v = Vec::new();
    for p in tree(&u.join("lib")) {
        v.push((format!("/usr/src/lib/{}", name(&p)), p));
    }
    for p in tree(&u.join("../crates/trine/lib")) {
        v.push((format!("/usr/lib/trine/{}", name(&p)), p));
    }
    // the long runtime, which the compiler appends to code that uses it
    v.push(("/usr/lib/trine/long.s".into(), u.join("../crates/trine/lib/long.s")));
    v.push(("/usr/src/bin/tric.tri".into(), u.join("bin/tric.tri")));
    for p in tree(&u.join("bin/tric")) {
        v.push((format!("/usr/src/bin/tric/{}", name(&p)), p));
    }
    v.push(("/usr/src/hello.tri".into(), u.join("hello.tri")));
    v
}

/// The default layout (a tfs root) with these files.
pub fn build_disk_with(kernel_dir: &Path, files: &[(String, PathBuf)]) -> Result<Disk, String> {
    build_disk_root(kernel_dir, files, &Root::default())
}

/// A tfs volume holding `files` (sources compiled as for the initramfs).
pub fn build_tfs(files: &[(String, PathBuf)], blocks: usize, inodes: usize) -> Result<tfs::Tfs, String> {
    let mut fs = tfs::Tfs::format(blocks, inodes)?;
    for d in ROOT_DIRS {
        fs.mkdir_p(d)?;
    }
    // The same source under several names becomes hard links to one file.
    // (a source and the program compiled from it are different files)
    let mut written: Vec<(&PathBuf, String)> = Vec::new();
    for (name, src) in files {
        if let Some((_, first)) = written.iter().find(|(p, n)| *p == src && compiled(n, p) == compiled(name, src)) {
            fs.link(first, name).map_err(|e| format!("{name}: {e}"))?;
            continue;
        }
        let data = disk_file(name, src)?;
        let perm = if compiled(name, src) { 0o755 } else { 0o644 };
        fs.write_file(name, &data, perm).map_err(|e| format!("{name}: {e}"))?;
        written.push((src, name.clone()));
    }
    for (path, perm, uid, gid) in SPECIAL_MODES {
        if fs.lookup(path).is_ok() {
            fs.set_owner_mode(path, perm, uid, gid)?;
        }
    }
    Ok(fs)
}

pub fn build_disk_root(kernel_dir: &Path, files: &[(String, PathBuf)], root: &Root) -> Result<Disk, String> {
    let mut disk = build_boot_area(kernel_dir, files, root)?;
    if let Root::Tfs { blocks, inodes } = root {
        let fs = build_tfs(files, *blocks, *inodes)?;
        disk.trytes.resize(TFS_START * PAGE as usize, 0);
        disk.trytes.extend_from_slice(&fs.trytes);
    }
    Ok(disk)
}

fn build_boot_area(kernel_dir: &Path, files: &[(String, PathBuf)], root: &Root) -> Result<Disk, String> {
    let loader = trine::build_file(&kernel_dir.join("boot/loader.tri"), &Options::boot())?.image;
    let kb = trine::build_file(&kernel_dir.join("main.tri"), &kernel_options())?;
    if kb.data_end > KERNEL_DATA_END {
        return Err(format!("the kernel globals end at {}, past frame 57 ({KERNEL_DATA_END})", kb.data_end));
    }
    let kernel = kb.image;
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
    if matches!(root, Root::Tfs { .. }) {
        return Ok(Disk { trytes, kernel_sectors: sectors, files: files.len() });
    }

    // initramfs: header at sector 28, file data from sector 29.
    let mut data: Vec<i16> = Vec::new();
    let mut entries: Vec<i16> = Vec::new();
    for (name, src) in files {
        let chars: Vec<i64> = name.chars().map(|c| c as i64).collect();
        if chars.len() > 26 {
            return Err(format!("initramfs path too long: {name}"));
        }
        let file = disk_file(name, src)?;
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
    if trytes.len() > TFS_START * page {
        return Err("the initramfs does not fit below sector 243".into());
    }
    Ok(Disk { trytes, kernel_sectors: sectors, files: files.len() })
}

/// Everything on the machine's disk 0 (to boot it again, or inspect it).
pub fn disk_contents(m: &mut setun_core::Machine) -> Vec<i16> {
    let mut store = m.dev.disks[0].detach().expect("a disk in drive 0");
    let mut out = vec![0i16; store.sectors() as usize * PAGE as usize];
    for (lba, chunk) in out.chunks_mut(PAGE as usize).enumerate() {
        assert!(store.read(lba as i64, chunk));
    }
    m.dev.disks[0].attach(store);
    out
}

/// The tfs volume inside a disk image.
pub fn tfs_of(trytes: &[i16]) -> Result<tfs::Tfs, String> {
    let start = TFS_START * PAGE as usize;
    if trytes.len() <= start {
        return Err("no tfs volume on this disk".into());
    }
    tfs::Tfs::open(trytes[start..].to_vec())
}

/// Boot a disk on a fresh machine with the BIOS; run `steps` instructions.
pub fn boot(disk: &Disk, ram_frames: i64, steps: u64) -> setun_core::Machine {
    boot_with_input(disk, ram_frames, steps, "")
}

/// Boot with `input` waiting on the serial console (the kernel's tty reads it).
pub fn boot_with_input(disk: &Disk, ram_frames: i64, steps: u64, input: &str) -> setun_core::Machine {
    boot_trytes(&disk.trytes, ram_frames, steps, input)
}

/// Boot raw disk trytes (for example a disk a previous run wrote to).
pub fn boot_trytes(trytes: &[i16], ram_frames: i64, steps: u64, input: &str) -> setun_core::Machine {
    let bios = trine::bios_rom().expect("the BIOS compiles");
    let mut m = setun_core::Machine::new(ram_frames, 1_790_000_000);
    m.load_rom(&bios);
    m.dev.disks[0].attach(Box::new(setun_core::devices::MemDisk::from_trytes(trytes)));
    m.dev.serial_input(input);
    m.run(steps);
    m
}
