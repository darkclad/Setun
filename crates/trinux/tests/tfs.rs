//! The host-side tfs (crates/trinux/src/tfs.rs): format, files, fsck, and
//! the ways it must refuse or report damage.

use trinux::tfs::*;

fn text(s: &str) -> Vec<i16> {
    s.chars().map(|c| c as u32 as i16).collect()
}

#[test]
fn fresh_volume_is_clean() {
    let fs = Tfs::format(486, 729).unwrap();
    assert_eq!(fs.fsck(), Vec::<String>::new());
    assert_eq!(fs.sb(SB_NINODES), 729);
    assert_eq!(fs.sb(SB_DATA), 2 + 1 + 3);
    assert_eq!(fs.sb(SB_FREE_BLOCKS), 486 - 6 - 1); // the root's directory block
    assert_eq!(fs.sb(SB_FREE_INODES), 728);
    let names: Vec<String> = fs.entries(ROOT).unwrap().into_iter().map(|e| e.1).collect();
    assert_eq!(names, [".", ".."]);
}

#[test]
fn files_and_directories_round_trip() {
    let mut fs = Tfs::format(486, 243).unwrap();
    fs.write_file("/etc/motd", &text("hello\n"), 0o644).unwrap();
    // 20 blocks: past the 15 direct pointers into the single-indirect block
    let big: Vec<i16> = (0..20 * BLOCK).map(|i| (i % 19_683) as i16 - 9_841).collect();
    fs.write_file("/data/big", &big, 0o600).unwrap();
    fs.mkdir_p("/a/b/c").unwrap();
    assert_eq!(fs.read_text("/etc/motd").unwrap(), "hello\n");
    assert_eq!(fs.read_file("/data/big").unwrap(), big);
    let big_ino = fs.lookup("/data/big").unwrap();
    assert_eq!(fs.inode_word(big_ino, I_BLOCKS), 21, "20 data blocks + 1 indirect");
    assert_eq!(fs.inode_word(fs.lookup("/a").unwrap(), I_NLINK), 3);
    assert_eq!(fs.inode_word(ROOT, I_NLINK), 5, "., .., and the .. of etc, data, a");
    assert_eq!(fs.fsck(), Vec::<String>::new());
    // a name of exactly NAME_MAX characters fits
    let long = "n".repeat(NAME_MAX);
    fs.write_file(&format!("/{long}"), &[], 0o644).unwrap();
    assert!(fs.lookup(&format!("/{long}")).is_ok());
    assert_eq!(fs.fsck(), Vec::<String>::new());
}

#[test]
fn directories_grow_past_one_block() {
    let mut fs = Tfs::format(486, 729).unwrap();
    for i in 0..300 {
        fs.write_file(&format!("/many/f{i}"), &[], 0o644).unwrap();
    }
    let d = fs.lookup("/many").unwrap();
    assert_eq!(fs.entries(d).unwrap().len(), 302);
    assert_eq!(fs.inode_word(d, I_BLOCKS), 2);
    assert!(fs.lookup("/many/f299").is_ok());
    assert_eq!(fs.fsck(), Vec::<String>::new());
}

// ---- refusals --------------------------------------------------------------------------

#[test]
fn refuses_bad_names_and_duplicates() {
    let mut fs = Tfs::format(100, 243).unwrap();
    let too_long = "x".repeat(NAME_MAX + 1);
    assert!(fs.write_file(&format!("/{too_long}"), &[], 0o644).unwrap_err().contains("bad name"));
    fs.write_file("/f", &[], 0o644).unwrap();
    assert!(fs.write_file("/f", &[], 0o644).unwrap_err().contains("exists"));
    assert!(fs.write_file("relative", &[], 0o644).is_err());
    assert!(fs.lookup("/f/g").unwrap_err().contains("not a directory"));
    assert!(fs.lookup("/nope").unwrap_err().contains("no such file"));
    assert_eq!(fs.fsck(), Vec::<String>::new());
}

#[test]
fn reports_full_volume_and_inode_table() {
    let mut fs = Tfs::format(20, 243).unwrap();
    let free = fs.sb(SB_FREE_BLOCKS) as usize;
    let e = fs.write_file("/big", &vec![1; (free + 1) * BLOCK], 0o644).unwrap_err();
    assert!(e.contains("out of blocks"), "{e}");
    let mut fs = Tfs::format(100, 243).unwrap();
    for i in 0..242 {
        fs.write_file(&format!("/f{i}"), &[], 0o644).unwrap();
    }
    assert!(fs.write_file("/one-more", &[], 0o644).unwrap_err().contains("out of inodes"));
}

#[test]
fn refuses_unusable_geometry() {
    assert!(Tfs::format(5, 243).is_err(), "no room for data");
    assert!(Tfs::format(486, 20_000).is_err(), "more inodes than one map block");
    assert!(Tfs::format(3usize.pow(13) + 1, 243).is_err(), "beyond 3^13 blocks");
}

#[test]
fn open_rejects_damaged_superblocks() {
    let fs = Tfs::format(100, 243).unwrap();
    assert!(Tfs::open(fs.trytes[..100].to_vec()).unwrap_err().contains("no superblock"));
    let mut bad = Tfs { trytes: fs.trytes.clone() };
    bad.set_word(0, SB_MAGIC, 12345);
    assert!(Tfs::open(bad.trytes).unwrap_err().contains("bad magic"));
    let mut bad = Tfs { trytes: fs.trytes.clone() };
    bad.set_word(0, SB_NBLOCKS, 1000);
    assert!(Tfs::open(bad.trytes).unwrap_err().contains("inconsistent"));
    let mut bad = Tfs { trytes: fs.trytes.clone() };
    bad.set_word(0, SB_DATA, 100);
    assert!(Tfs::open(bad.trytes).unwrap_err().contains("inconsistent"));
    assert!(Tfs::open(fs.trytes).is_ok());
}

// ---- fsck finds damage -----------------------------------------------------------------

fn sample() -> Tfs {
    let mut fs = Tfs::format(100, 243).unwrap();
    fs.write_file("/a", &text("aaa"), 0o644).unwrap();
    fs.write_file("/d/b", &text("bbb"), 0o644).unwrap();
    assert_eq!(fs.fsck(), Vec::<String>::new());
    fs
}

fn assert_finds(fs: &Tfs, what: &str) {
    let errs = fs.fsck();
    assert!(errs.iter().any(|e| e.contains(what)), "expected {what:?} in {errs:?}");
}

#[test]
fn fsck_finds_a_block_pointer_out_of_range() {
    let mut fs = sample();
    let a = fs.lookup("/a").unwrap();
    fs.set_inode_word(a, I_DIRECT, 99_999);
    assert_finds(&fs, "out of range");
    assert!(fs.read_file("/a").unwrap_err().contains("bad block"));
}

#[test]
fn fsck_finds_a_block_used_twice() {
    let mut fs = sample();
    let a = fs.lookup("/a").unwrap();
    let b = fs.lookup("/d/b").unwrap();
    let blk = fs.inode_word(a, I_DIRECT);
    let old = fs.inode_word(b, I_DIRECT);
    fs.set_inode_word(b, I_DIRECT, blk);
    assert_finds(&fs, "used by inodes");
    assert!(old != blk);
    assert_finds(&fs, "marked used but unowned");
}

#[test]
fn fsck_finds_wrong_link_counts_and_free_counts() {
    let mut fs = sample();
    let a = fs.lookup("/a").unwrap();
    fs.set_inode_word(a, I_NLINK, 2);
    assert_finds(&fs, "nlink 2 but 1 links");
    let mut fs = sample();
    let n = fs.sb(SB_FREE_BLOCKS);
    fs.set_word(0, SB_FREE_BLOCKS, n + 1);
    assert_finds(&fs, "free blocks, superblock says");
    let mut fs = sample();
    let n = fs.sb(SB_FREE_INODES);
    fs.set_word(0, SB_FREE_INODES, n - 1);
    assert_finds(&fs, "free inodes, superblock says");
}

#[test]
fn fsck_finds_unreachable_inodes_and_bad_entries() {
    // An allocated inode no directory names (a leak, as after a crash).
    let mut fs = sample();
    let d = fs.lookup("/d").unwrap();
    let data_blk = fs.inode_word(d, I_DIRECT) as usize;
    // Clear the entry "b" (slot 2) in /d: inode b becomes unreachable.
    for k in 0..3 {
        fs.set_tryte(data_blk, 2 * DIRENT_TRYTES + k, 0);
    }
    fs.set_inode_word(d, I_NLINK, 2);
    assert_finds(&fs, "allocated but unreachable");
    // An entry naming an inode beyond the table.
    let mut fs = sample();
    let d = fs.lookup("/d").unwrap();
    let data_blk = fs.inode_word(d, I_DIRECT) as usize;
    fs.set_word(data_blk, 2 * DIRENT_TRYTES / 3, 5000);
    assert_finds(&fs, "has inode 5000");
}
