//! Trinux K6: the tfs root on disk, the buffer cache, mounts, /proc and ps.
//! Files must survive a reboot; damaged disks must give errors, not crashes.

use std::path::PathBuf;
use trinux::tfs::{self, Tfs};
use trinux::{Root, TFS_START};

const PAGE: usize = 19_683;

/// Boot `disk` and type each line at the shell prompt (as in boot.rs;
/// `~` sends raw input after 0.5 s). Stops at a panic or power-off.
/// Returns the transcript and the machine (whose disk can be read back).
fn session(disk: &[i16], lines: &[&str], max_steps: u64) -> (String, setun_core::Machine) {
    let mut m = trinux::boot_trytes(disk, 729, 0, "");
    let mut out = String::new();
    let (mut next, mut steps, mut since_input) = (0, 0, 0);
    while steps < max_steps && !m.powered_off {
        m.run(1_000_000);
        steps += 1_000_000;
        since_input += 1_000_000;
        out.push_str(&m.dev.serial.take_output());
        if out.contains("Kernel panic") {
            break;
        }
        let Some(line) = lines.get(next) else { continue };
        if let Some(raw) = line.strip_prefix('~') {
            if since_input >= 5_000_000 {
                m.dev.serial_input(raw);
                next += 1;
                since_input = 0;
            }
        } else if out.ends_with("$ ") {
            m.dev.serial_input(&format!("{line}\n"));
            next += 1;
            since_input = 0;
        }
    }
    (out, m)
}

fn default_disk() -> Vec<i16> {
    trinux::build_disk(&trinux::kernel_dir()).unwrap_or_else(|e| panic!("{e}")).trytes
}

/// The default disk with its tfs volume changed by `f`.
fn damaged_disk(f: impl FnOnce(&mut Tfs)) -> Vec<i16> {
    let mut disk = default_disk();
    let mut fs = trinux::tfs_of(&disk).unwrap();
    f(&mut fs);
    let start = TFS_START * PAGE;
    disk[start..start + fs.trytes.len()].copy_from_slice(&fs.trytes);
    disk
}

fn assert_has(out: &str, wants: &[&str]) {
    for w in wants {
        assert!(out.contains(w), "missing {w:?} in:\n{out}");
    }
}

fn assert_clean(disk: &[i16]) {
    let fs = trinux::tfs_of(disk).unwrap();
    assert_eq!(fs.fsck(), Vec::<String>::new(), "fsck after the session");
}

// ---- persistence -------------------------------------------------------------------------

#[test]
fn k6_files_survive_a_reboot() {
    let (out, mut m) = session(
        &default_disk(),
        &["echo persist > /home/note", "mkdir /home/docs", "echo deep > /home/docs/d.txt", "mv /home/note /home/docs/note", "poweroff"],
        200_000_000,
    );
    assert_has(&out, &["reboot: Power down"]);
    assert!(m.powered_off);
    let disk = trinux::disk_contents(&mut m);
    // The host reads what the kernel wrote, and the volume is consistent and clean.
    let fs = trinux::tfs_of(&disk).unwrap();
    assert_eq!(fs.read_text("/home/docs/note").unwrap(), "persist\n");
    assert_eq!(fs.read_text("/home/docs/d.txt").unwrap(), "deep\n");
    assert!(fs.lookup("/home/note").is_err());
    assert_eq!(fs.sb(tfs::SB_STATE), 1, "cleanly unmounted");
    assert_clean(&disk);
    // A second boot of the same disk sees the files.
    let (out, _) = session(&disk, &["cat /home/docs/note /home/docs/d.txt", "ls /home/docs", "exit"], 150_000_000);
    assert!(!out.contains("not cleanly unmounted"), "{out}");
    assert_has(&out, &["$ cat /home/docs/note /home/docs/d.txt\npersist\ndeep\n", "$ ls /home/docs\nd.txt\nnote\n"]);
}

#[test]
fn k6_unsynced_shutdown_is_detected() {
    // Pull the plug without sync or poweroff: the superblock still says "mounted".
    let (out, mut m) = session(&default_disk(), &["echo lost > /home/x", "true"], 100_000_000);
    assert_has(&out, &["$ true\n$ "]);
    let disk = trinux::disk_contents(&mut m);
    let (out, _) = session(&disk, &["exit"], 60_000_000);
    assert_has(&out, &["tfs: volume was not cleanly unmounted (run fsck)", "VFS: Mounted root (tfs filesystem)"]);
}

#[test]
fn k6_sync_and_the_flush_daemon_write_through() {
    // After `sync`, the data is on the disk even without a clean shutdown.
    let (_, mut m) = session(&default_disk(), &["echo synced > /home/s", "sync", "true"], 100_000_000);
    let disk = trinux::disk_contents(&mut m);
    assert_eq!(trinux::tfs_of(&disk).unwrap().read_text("/home/s").unwrap(), "synced\n");
    // Without sync, the idle loop flushes within 5 s of emulated time.
    let (_, mut m) = session(&default_disk(), &["echo later > /home/l", "sleep 7", "true"], 200_000_000);
    let disk = trinux::disk_contents(&mut m);
    assert_eq!(trinux::tfs_of(&disk).unwrap().read_text("/home/l").unwrap(), "later\n");
}

// ---- the file-system scenarios, then fsck --------------------------------------------------

#[test]
fn k6_fstest_passes_and_leaves_a_clean_volume() {
    let (out, mut m) = session(&default_disk(), &["fstest", "poweroff"], 900_000_000);
    assert!(!out.contains("FAILED"), "{out}");
    assert_has(&out, &["fstest: all passed", "reboot: Power down"]);
    let n = out.matches(" ok\n").count();
    assert!(n >= 85, "only {n} checks ran:\n{out}");
    assert_clean(&trinux::disk_contents(&mut m));
}

#[test]
fn k6_full_volume_gives_enospc_and_recovers() {
    let files = trinux::default_files();
    let disk = trinux::build_disk_root(&trinux::kernel_dir(), &files, &Root::Tfs { blocks: 81, inodes: 243 })
        .unwrap_or_else(|e| panic!("{e}"))
        .trytes;
    let (out, mut m) = session(&disk, &["fstest enospc", "poweroff"], 900_000_000);
    assert!(!out.contains("FAILED"), "{out}");
    assert_has(
        &out,
        &[
            "fstest: ENOSPC when the volume is full ok",
            "fstest: ENOSPC: mkdir on a full volume ok",
            "fstest: the file holds what was written ok",
            "fstest: every block comes back ok",
            "fstest: ENOSPC when the inode table is full ok",
            "fstest: every inode comes back ok",
            "fstest: all passed",
        ],
    );
    assert_clean(&trinux::disk_contents(&mut m));
}

// ---- damaged disks ---------------------------------------------------------------------------

#[test]
fn k6_bad_superblock_without_initramfs_panics() {
    let disk = damaged_disk(|fs| fs.set_word(0, tfs::SB_MAGIC, 7));
    let (out, _) = session(&disk, &[], 40_000_000);
    assert_has(&out, &["Kernel panic - not syncing: VFS: Unable to mount root fs"]);
}

#[test]
fn k6_inconsistent_superblock_is_refused() {
    let disk = damaged_disk(|fs| fs.set_word(0, tfs::SB_NBLOCKS, 100_000));
    let (out, _) = session(&disk, &[], 40_000_000);
    assert_has(&out, &["tfs: inconsistent superblock, not mounting", "VFS: Unable to mount root fs"]);
    let disk = damaged_disk(|fs| fs.set_word(0, tfs::SB_FREE_INODES, -5));
    let (out, _) = session(&disk, &[], 40_000_000);
    assert_has(&out, &["tfs: inconsistent superblock, not mounting"]);
}

#[test]
fn k6_initramfs_root_still_boots() {
    let disk = trinux::build_disk_root(&trinux::kernel_dir(), &trinux::default_files(), &Root::Initramfs)
        .unwrap_or_else(|e| panic!("{e}"))
        .trytes;
    let (out, _) = session(&disk, &["echo ramfs root", "ls /proc/1", "exit"], 100_000_000);
    let count = format!("initramfs: {} files", trinux::default_files().len());
    assert_has(&out, &[&count, "VFS: Mounted root (ramfs from the initramfs)", "ramfs root\n", "cmdline\nstat\nstatus\n"]);
}

#[test]
fn k6_damaged_files_give_eio_and_the_system_carries_on() {
    let disk = damaged_disk(|fs| {
        // /etc/motd: a block pointer far outside the volume
        let motd = fs.lookup("/etc/motd").unwrap();
        fs.set_inode_word(motd, tfs::I_DIRECT, 99_999);
        // /etc/badmode: an inode with a mode that is neither file nor directory
        let bad = fs.write_file("/etc/badmode", &[65, 10], 0o644).unwrap();
        fs.set_inode_word(bad, tfs::I_MODE, 12_345);
        // /etc/badent: a directory entry naming an inode beyond the table
        fs.write_file("/etc/badent", &[66, 10], 0o644).unwrap();
        let etc = fs.lookup("/etc").unwrap();
        let blk = fs.inode_word(etc, tfs::I_DIRECT) as usize;
        let slot = fs.entries(etc).unwrap().into_iter().find(|e| e.1 == "badent").unwrap().0;
        fs.set_word(blk, slot * tfs::DIRENT_TRYTES / 3, 5000);
    });
    let (out, _) = session(
        &disk,
        &["cat /etc/motd", "cat /etc/badmode", "cat /etc/badent", "ls /etc", "echo still alive", "exit"],
        150_000_000,
    );
    assert_has(
        &out,
        &[
            "tfs: bad block 99999 in inode",
            "cat: /etc/motd: Input/output error\n",
            "tfs: bad mode 12345 in inode",
            "cat: /etc/badmode: Input/output error\n",
            "tfs: bad entry inode 5000",
            "cat: /etc/badent: Input/output error\n",
            "$ ls /etc\nbadent\nbadmode\nmotd\n",
            "$ echo still alive\nstill alive\n",
            "init: /bin/sh exited with 0",
        ],
    );
}

// ---- /proc and ps ---------------------------------------------------------------------------

#[test]
fn k6_proc_and_ps() {
    let (out, _) = session(
        &default_disk(),
        &["sleep 30 &", "ps", "cat /proc/3/status", "cat /proc/3/cmdline; echo", "ls /proc", "cat /proc/mounts", "df /", "kill %1", "exit"],
        200_000_000,
    );
    assert_has(
        &out,
        &[
            "  PID  PPID  PGID S   TIME CMD\n    1     0     1 S   0:00 /sbin/init\n    2     1     2 S   0:00 sh\n    3     2     3 S   0:00 sleep 30\n",
            " R   0:00 ps\n",
            "Name:\tsleep\nState:\tS (sleeping)\nPid:\t3\nPPid:\t2\nPGid:\t3\nSid:\t1\n",
            "$ cat /proc/3/cmdline; echo\nsleep\u{0}30\u{0}\n",
            "$ ls /proc\n1\n2\n3\n",
            "meminfo\nmounts\nuptime\n",
            "/dev/hda / tfs rw 0 0\ndevfs /dev ramfs rw 0 0\nproc /proc proc rw 0 0\ntmpfs /tmp ramfs rw 0 0\n",
            " Blocks    Used   Avail  Inodes   IFree Mounted on\n     480",
        ],
    );
}

// ---- mkdisk refusals ---------------------------------------------------------------------------

#[test]
fn mkdisk_refuses_what_does_not_fit() {
    let dir = std::env::temp_dir().join(format!("trinux-mkdisk-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let big = dir.join("big.txt");
    std::fs::write(&big, "x".repeat(3 * PAGE)).unwrap();
    let files: Vec<(String, PathBuf)> = vec![("/big".into(), big.clone())];
    let e = trinux::build_tfs(&files, 10, 243).unwrap_err();
    assert!(e.contains("out of blocks"), "{e}");
    let long = format!("/{}", "n".repeat(78));
    let e = trinux::build_tfs(&[(long, big.clone())], 486, 243).unwrap_err();
    assert!(e.contains("bad name"), "{e}");
    let e = trinux::build_tfs(&files, 4, 243).unwrap_err();
    assert!(e.contains("not a usable volume size"), "{e}");
    let missing: Vec<(String, PathBuf)> = vec![("/x".into(), dir.join("missing.txt"))];
    assert!(trinux::build_tfs(&missing, 486, 243).is_err());
    std::fs::remove_dir_all(&dir).ok();
}
