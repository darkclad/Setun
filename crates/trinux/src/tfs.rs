//! tfs, the ternary file system: a host-side implementation for `mkdisk`
//! (format and populate) and for tests (read back what the kernel wrote,
//! check consistency, corrupt on purpose). The kernel side is
//! `kernel/fs/tfs.tri`; docs/KERNEL.md §19 describes the format.
//!
//! A block is one disk sector, 3^9 trytes = 6561 words. Block numbers are
//! relative to the start of the volume.
//!
//! | Block | Contents |
//! |---|---|
//! | 0 | superblock (words, see `SB_*`) |
//! | 1 | inode map: one tryte per inode, 0 free / 1 used |
//! | 2 .. 2+B | block map: one tryte per block, 0 free / 1 used / -1 bad |
//! | then | inode table: 243 inodes of 27 words per block |
//! | then | data blocks |
//!
//! Inode (27 words): mode, nlink, uid, gid, size (trytes), atime, mtime,
//! ctime, blocks (allocated data + indirect), direct[15], single indirect,
//! double indirect, generation. Inode numbers start at 1 (the root).
//!
//! A directory is a file of 81-tryte entries: a word with the inode number
//! (0 = free slot), then the name in 78 trytes, NUL-terminated when shorter.
//! Every directory starts with "." and "..".

use setun_core::memory::word_to_trytes;
use setun_core::PAGE;
use ternary::Word;

pub const MAGIC: i64 = 115_067_810_204;
pub const VERSION: i64 = 1;
pub const BLOCK: usize = PAGE as usize;
pub const WORDS_PER_BLOCK: usize = BLOCK / 3;
pub const INODE_WORDS: usize = 27;
pub const INODES_PER_BLOCK: usize = WORDS_PER_BLOCK / INODE_WORDS; // 243
pub const DIRENT_TRYTES: usize = 81;
pub const DIRENTS_PER_BLOCK: usize = BLOCK / DIRENT_TRYTES; // 243
pub const NAME_MAX: usize = 77;
pub const NDIRECT: usize = 15;
pub const ROOT: i64 = 1;

pub const S_IFDIR: i64 = 16384;
pub const S_IFREG: i64 = 32768;

// Superblock words.
pub const SB_MAGIC: usize = 0;
pub const SB_VERSION: usize = 1;
pub const SB_NBLOCKS: usize = 2;
pub const SB_NINODES: usize = 3;
pub const SB_IMAP: usize = 4;
pub const SB_BMAP: usize = 5;
pub const SB_BMAP_BLOCKS: usize = 6;
pub const SB_ITABLE: usize = 7;
pub const SB_ITABLE_BLOCKS: usize = 8;
pub const SB_DATA: usize = 9;
pub const SB_FREE_BLOCKS: usize = 10;
pub const SB_FREE_INODES: usize = 11;
pub const SB_ROOT: usize = 12;
pub const SB_STATE: usize = 13; // 1 clean, 0 mounted (or not cleanly unmounted)

// Inode words.
pub const I_MODE: usize = 0;
pub const I_NLINK: usize = 1;
pub const I_UID: usize = 2;
pub const I_GID: usize = 3;
pub const I_SIZE: usize = 4;
pub const I_MTIME: usize = 6;
pub const I_BLOCKS: usize = 8;
pub const I_DIRECT: usize = 9;
pub const I_SINGLE: usize = 24;
pub const I_DOUBLE: usize = 25;

pub struct Tfs {
    /// The whole volume, `nblocks * BLOCK` trytes.
    pub trytes: Vec<i16>,
}

impl std::fmt::Debug for Tfs {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Tfs({} trytes)", self.trytes.len())
    }
}

fn err<T>(m: impl Into<String>) -> Result<T, String> {
    Err(m.into())
}

impl Tfs {
    // ---- raw access ----------------------------------------------------------------

    pub fn word(&self, blk: usize, j: usize) -> i64 {
        let b = blk * BLOCK + 3 * j;
        let t = &self.trytes[b..b + 3];
        t[0] as i64 * 387_420_489 + t[1] as i64 * 19_683 + t[2] as i64
    }

    pub fn set_word(&mut self, blk: usize, j: usize, v: i64) {
        let b = blk * BLOCK + 3 * j;
        for (k, t) in word_to_trytes(Word::new(v).expect("word")).iter().enumerate() {
            self.trytes[b + k] = t.value() as i16;
        }
    }

    pub fn tryte(&self, blk: usize, i: usize) -> i16 {
        self.trytes[blk * BLOCK + i]
    }

    pub fn set_tryte(&mut self, blk: usize, i: usize, v: i16) {
        self.trytes[blk * BLOCK + i] = v;
    }

    pub fn sb(&self, f: usize) -> i64 {
        self.word(0, f)
    }

    fn set_sb(&mut self, f: usize, v: i64) {
        self.set_word(0, f, v);
    }

    pub fn nblocks(&self) -> usize {
        self.sb(SB_NBLOCKS) as usize
    }

    pub fn ninodes(&self) -> usize {
        self.sb(SB_NINODES) as usize
    }

    fn inode_pos(&self, ino: i64) -> (usize, usize) {
        let i = (ino - 1) as usize;
        (self.sb(SB_ITABLE) as usize + i / INODES_PER_BLOCK, (i % INODES_PER_BLOCK) * INODE_WORDS)
    }

    pub fn inode_word(&self, ino: i64, f: usize) -> i64 {
        let (b, w) = self.inode_pos(ino);
        self.word(b, w + f)
    }

    pub fn set_inode_word(&mut self, ino: i64, f: usize, v: i64) {
        let (b, w) = self.inode_pos(ino);
        self.set_word(b, w + f, v);
    }

    fn imap(&self, ino: i64) -> i16 {
        self.tryte(self.sb(SB_IMAP) as usize, (ino - 1) as usize)
    }

    fn bmap_entry(&self, blk: usize) -> i16 {
        let bm = self.sb(SB_BMAP) as usize;
        self.tryte(bm + blk / BLOCK, blk % BLOCK)
    }

    fn set_bmap(&mut self, blk: usize, v: i16) {
        let bm = self.sb(SB_BMAP) as usize;
        self.set_tryte(bm + blk / BLOCK, blk % BLOCK, v);
    }

    // ---- format and open -------------------------------------------------------------

    /// A fresh volume with an empty root directory. `ninodes` is rounded up
    /// to a whole number of inode-table blocks.
    pub fn format(nblocks: usize, ninodes: usize) -> Result<Tfs, String> {
        let ninodes = ninodes.max(1).div_ceil(INODES_PER_BLOCK) * INODES_PER_BLOCK;
        if ninodes > BLOCK {
            return err(format!("tfs: at most {BLOCK} inodes"));
        }
        let bmap_blocks = nblocks.div_ceil(BLOCK);
        let itable_blocks = ninodes / INODES_PER_BLOCK;
        let data = 2 + bmap_blocks + itable_blocks;
        if nblocks < data + 2 || nblocks as i64 > 3i64.pow(13) {
            return err(format!("tfs: {nblocks} blocks is not a usable volume size"));
        }
        let mut fs = Tfs { trytes: vec![0; nblocks * BLOCK] };
        for (f, v) in [
            (SB_MAGIC, MAGIC),
            (SB_VERSION, VERSION),
            (SB_NBLOCKS, nblocks as i64),
            (SB_NINODES, ninodes as i64),
            (SB_IMAP, 1),
            (SB_BMAP, 2),
            (SB_BMAP_BLOCKS, bmap_blocks as i64),
            (SB_ITABLE, 2 + bmap_blocks as i64),
            (SB_ITABLE_BLOCKS, itable_blocks as i64),
            (SB_DATA, data as i64),
            (SB_FREE_BLOCKS, (nblocks - data) as i64),
            (SB_FREE_INODES, ninodes as i64),
            (SB_ROOT, ROOT),
            (SB_STATE, 1),
        ] {
            fs.set_sb(f, v);
        }
        for b in 0..data {
            fs.set_bmap(b, 1);
        }
        let root = fs.alloc_inode()?;
        assert_eq!(root, ROOT);
        fs.set_inode_word(root, I_MODE, S_IFDIR + 0o755);
        fs.set_inode_word(root, I_NLINK, 2);
        fs.add_entry(root, ".", root)?;
        fs.add_entry(root, "..", root)?;
        Ok(fs)
    }

    /// Check the superblock of an existing volume.
    pub fn open(trytes: Vec<i16>) -> Result<Tfs, String> {
        if trytes.len() < BLOCK {
            return err("tfs: no superblock");
        }
        let fs = Tfs { trytes };
        if fs.sb(SB_MAGIC) != MAGIC {
            return err("tfs: bad magic");
        }
        let n = fs.nblocks();
        if n * BLOCK > fs.trytes.len() || fs.sb(SB_DATA) as usize >= n || fs.ninodes() == 0 {
            return err("tfs: inconsistent superblock");
        }
        Ok(fs)
    }

    // ---- allocation ------------------------------------------------------------------

    fn alloc_inode(&mut self) -> Result<i64, String> {
        let im = self.sb(SB_IMAP) as usize;
        for i in 0..self.ninodes() {
            if self.tryte(im, i) == 0 {
                self.set_tryte(im, i, 1);
                let ino = i as i64 + 1;
                for f in 0..INODE_WORDS {
                    self.set_inode_word(ino, f, 0);
                }
                let free = self.sb(SB_FREE_INODES);
                self.set_sb(SB_FREE_INODES, free - 1);
                return Ok(ino);
            }
        }
        err("tfs: out of inodes")
    }

    fn alloc_block(&mut self) -> Result<usize, String> {
        for b in self.sb(SB_DATA) as usize..self.nblocks() {
            if self.bmap_entry(b) == 0 {
                self.set_bmap(b, 1);
                self.trytes[b * BLOCK..(b + 1) * BLOCK].fill(0);
                let free = self.sb(SB_FREE_BLOCKS);
                self.set_sb(SB_FREE_BLOCKS, free - 1);
                return Ok(b);
            }
        }
        err("tfs: out of blocks")
    }

    /// The block holding logical block `lbn` of `ino`, allocating when asked.
    fn bmap(&mut self, ino: i64, lbn: usize, alloc: bool) -> Result<usize, String> {
        let new = |fs: &mut Tfs, ino: i64| -> Result<usize, String> {
            let b = fs.alloc_block()?;
            let n = fs.inode_word(ino, I_BLOCKS);
            fs.set_inode_word(ino, I_BLOCKS, n + 1);
            Ok(b)
        };
        if lbn < NDIRECT {
            let mut b = self.inode_word(ino, I_DIRECT + lbn) as usize;
            if b == 0 && alloc {
                b = new(self, ino)?;
                self.set_inode_word(ino, I_DIRECT + lbn, b as i64);
            }
            return Ok(b);
        }
        let lbn = lbn - NDIRECT;
        let (slot, inner) = if lbn < WORDS_PER_BLOCK {
            (I_SINGLE, None)
        } else {
            (I_DOUBLE, Some(lbn - WORDS_PER_BLOCK))
        };
        let mut ind = self.inode_word(ino, slot) as usize;
        if ind == 0 {
            if !alloc {
                return Ok(0);
            }
            ind = new(self, ino)?;
            self.set_inode_word(ino, slot, ind as i64);
        }
        let (ind, idx) = match inner {
            None => (ind, lbn),
            Some(l) => {
                let i1 = l / WORDS_PER_BLOCK;
                if i1 >= WORDS_PER_BLOCK {
                    return err("tfs: file too large");
                }
                let mut ind2 = self.word(ind, i1) as usize;
                if ind2 == 0 {
                    if !alloc {
                        return Ok(0);
                    }
                    ind2 = new(self, ino)?;
                    self.set_word(ind, i1, ind2 as i64);
                }
                (ind2, l % WORDS_PER_BLOCK)
            }
        };
        let mut b = self.word(ind, idx) as usize;
        if b == 0 && alloc {
            b = new(self, ino)?;
            self.set_word(ind, idx, b as i64);
        }
        Ok(b)
    }

    // ---- file data -------------------------------------------------------------------

    pub fn read_inode(&self, ino: i64) -> Result<Vec<i16>, String> {
        let size = self.inode_word(ino, I_SIZE) as usize;
        let mut out = vec![0i16; size];
        for lbn in 0..size.div_ceil(BLOCK) {
            let b = self.clone_bmap(ino, lbn)?;
            let n = (size - lbn * BLOCK).min(BLOCK);
            if b != 0 {
                out[lbn * BLOCK..lbn * BLOCK + n].copy_from_slice(&self.trytes[b * BLOCK..b * BLOCK + n]);
            }
        }
        Ok(out)
    }

    /// bmap without allocation, on a shared reference.
    fn clone_bmap(&self, ino: i64, lbn: usize) -> Result<usize, String> {
        let b = if lbn < NDIRECT {
            self.inode_word(ino, I_DIRECT + lbn) as usize
        } else if lbn - NDIRECT < WORDS_PER_BLOCK {
            let ind = self.inode_word(ino, I_SINGLE) as usize;
            if ind == 0 {
                0
            } else {
                self.check_block(ind)?;
                self.word(ind, lbn - NDIRECT) as usize
            }
        } else {
            let l = lbn - NDIRECT - WORDS_PER_BLOCK;
            let ind = self.inode_word(ino, I_DOUBLE) as usize;
            if ind == 0 {
                0
            } else {
                self.check_block(ind)?;
                let ind2 = self.word(ind, l / WORDS_PER_BLOCK) as usize;
                if ind2 == 0 {
                    0
                } else {
                    self.check_block(ind2)?;
                    self.word(ind2, l % WORDS_PER_BLOCK) as usize
                }
            }
        };
        if b != 0 {
            self.check_block(b)?;
        }
        Ok(b)
    }

    fn check_block(&self, b: usize) -> Result<(), String> {
        if b < self.sb(SB_DATA) as usize || b >= self.nblocks() {
            return err(format!("tfs: bad block {b}"));
        }
        Ok(())
    }

    fn write_inode_data(&mut self, ino: i64, data: &[i16]) -> Result<(), String> {
        for (lbn, chunk) in data.chunks(BLOCK).enumerate() {
            let b = self.bmap(ino, lbn, true)?;
            self.trytes[b * BLOCK..b * BLOCK + chunk.len()].copy_from_slice(chunk);
        }
        self.set_inode_word(ino, I_SIZE, data.len() as i64);
        Ok(())
    }

    // ---- directories -----------------------------------------------------------------

    /// (slot, name, inode) for every used entry of directory `dir`.
    pub fn entries(&self, dir: i64) -> Result<Vec<(usize, String, i64)>, String> {
        let data = self.read_inode(dir)?;
        let mut v = Vec::new();
        for (slot, e) in data.chunks(DIRENT_TRYTES).enumerate() {
            if e.len() < DIRENT_TRYTES {
                break;
            }
            let ino = e[0] as i64 * 387_420_489 + e[1] as i64 * 19_683 + e[2] as i64;
            if ino == 0 {
                continue;
            }
            let name: String = e[3..].iter().take_while(|&&c| c != 0).map(|&c| char::from_u32(c as u32).unwrap_or('?')).collect();
            v.push((slot, name, ino));
        }
        Ok(v)
    }

    pub fn dir_lookup(&self, dir: i64, name: &str) -> Result<Option<i64>, String> {
        Ok(self.entries(dir)?.into_iter().find(|(_, n, _)| n == name).map(|(_, _, i)| i))
    }

    fn check_name(name: &str) -> Result<(), String> {
        let n = name.chars().count();
        if n == 0 || n > NAME_MAX || name.contains('/') {
            return err(format!("tfs: bad name {name:?} (1..{NAME_MAX} characters)"));
        }
        Ok(())
    }

    fn add_entry(&mut self, dir: i64, name: &str, ino: i64) -> Result<(), String> {
        Self::check_name(name)?;
        let chars: Vec<i16> = name.chars().map(|c| c as u32 as i16).collect();
        let size = self.inode_word(dir, I_SIZE) as usize;
        let slots = size / DIRENT_TRYTES;
        let used: Vec<usize> = self.entries(dir)?.iter().map(|e| e.0).collect();
        let slot = (0..slots).find(|s| !used.contains(s)).unwrap_or(slots);
        let lbn = slot / DIRENTS_PER_BLOCK;
        let b = self.bmap(dir, lbn, true)?;
        let off = b * BLOCK + (slot % DIRENTS_PER_BLOCK) * DIRENT_TRYTES;
        for (k, t) in word_to_trytes(Word::new(ino).unwrap()).iter().enumerate() {
            self.trytes[off + k] = t.value() as i16;
        }
        let e = &mut self.trytes[off + 3..off + DIRENT_TRYTES];
        e.fill(0);
        e[..chars.len()].copy_from_slice(&chars);
        if slot == slots {
            self.set_inode_word(dir, I_SIZE, ((slot + 1) * DIRENT_TRYTES) as i64);
        }
        Ok(())
    }

    /// The inode at an absolute path.
    pub fn lookup(&self, path: &str) -> Result<i64, String> {
        let mut cur = ROOT;
        for part in path.split('/').filter(|p| !p.is_empty()) {
            if self.inode_word(cur, I_MODE) / 4096 % 16 * 4096 != S_IFDIR {
                return err(format!("tfs: {path}: not a directory"));
            }
            cur = self.dir_lookup(cur, part)?.ok_or(format!("tfs: {path}: no such file"))?;
        }
        Ok(cur)
    }

    pub fn mkdir_p(&mut self, path: &str) -> Result<i64, String> {
        let mut cur = ROOT;
        for part in path.split('/').filter(|p| !p.is_empty()) {
            cur = match self.dir_lookup(cur, part)? {
                Some(i) => i,
                None => {
                    Self::check_name(part)?;
                    let d = self.alloc_inode()?;
                    self.set_inode_word(d, I_MODE, S_IFDIR + 0o755);
                    self.set_inode_word(d, I_NLINK, 2);
                    self.add_entry(d, ".", d)?;
                    self.add_entry(d, "..", cur)?;
                    self.add_entry(cur, part, d)?;
                    let n = self.inode_word(cur, I_NLINK);
                    self.set_inode_word(cur, I_NLINK, n + 1);
                    d
                }
            };
        }
        Ok(cur)
    }

    /// Create (or fail on an existing) regular file with `data`.
    pub fn write_file(&mut self, path: &str, data: &[i16], perm: i64) -> Result<i64, String> {
        let (dir, name) = path.rsplit_once('/').ok_or(format!("tfs: {path}: not absolute"))?;
        Self::check_name(name)?;
        let d = self.mkdir_p(dir)?;
        if self.dir_lookup(d, name)?.is_some() {
            return err(format!("tfs: {path}: exists"));
        }
        let ino = self.alloc_inode()?;
        self.set_inode_word(ino, I_MODE, S_IFREG + perm);
        self.set_inode_word(ino, I_NLINK, 1);
        self.add_entry(d, name, ino)?;
        self.write_inode_data(ino, data)?;
        Ok(ino)
    }

    /// Set a file's permission bits (type kept), owner and group.
    pub fn set_owner_mode(&mut self, path: &str, perm: i64, uid: i64, gid: i64) -> Result<(), String> {
        let ino = self.lookup(path)?;
        let mode = self.inode_word(ino, I_MODE);
        self.set_inode_word(ino, I_MODE, mode / 4096 * 4096 + perm);
        self.set_inode_word(ino, I_UID, uid);
        self.set_inode_word(ino, I_GID, gid);
        Ok(())
    }

    /// A second name for an existing file (a hard link).
    pub fn link(&mut self, existing: &str, path: &str) -> Result<(), String> {
        let ino = self.lookup(existing)?;
        if self.inode_word(ino, I_MODE) / 4096 % 16 * 4096 == S_IFDIR {
            return err(format!("tfs: {existing}: cannot link a directory"));
        }
        let (dir, name) = path.rsplit_once('/').ok_or(format!("tfs: {path}: not absolute"))?;
        Self::check_name(name)?;
        let d = self.mkdir_p(dir)?;
        if self.dir_lookup(d, name)?.is_some() {
            return err(format!("tfs: {path}: exists"));
        }
        self.add_entry(d, name, ino)?;
        let n = self.inode_word(ino, I_NLINK);
        self.set_inode_word(ino, I_NLINK, n + 1);
        Ok(())
    }

    pub fn read_file(&self, path: &str) -> Result<Vec<i16>, String> {
        self.read_inode(self.lookup(path)?)
    }

    pub fn read_text(&self, path: &str) -> Result<String, String> {
        Ok(self.read_file(path)?.iter().map(|&c| char::from_u32(c as u32).unwrap_or('?')).collect())
    }

    // ---- consistency check -----------------------------------------------------------

    /// fsck: walk the tree from the root and compare with the maps, the free
    /// counts and the link counts. Returns the problems found (empty = clean).
    pub fn fsck(&self) -> Vec<String> {
        let mut errs = Vec::new();
        let n = self.nblocks();
        let data = self.sb(SB_DATA) as usize;
        let mut owner = vec![0i64; n];
        let mut links = vec![0i64; self.ninodes() + 1];
        let mut seen = vec![false; self.ninodes() + 1];
        let claim = |b: usize, ino: i64, errs: &mut Vec<String>, owner: &mut Vec<i64>| {
            if b < data || b >= n {
                errs.push(format!("inode {ino}: block {b} out of range"));
            } else if owner[b] != 0 {
                errs.push(format!("block {b} used by inodes {} and {ino}", owner[b]));
            } else {
                owner[b] = ino;
            }
        };
        let mut stack = vec![ROOT];
        links[ROOT as usize] += 1; // the root's own ".." is itself; count the implicit reference
        while let Some(ino) = stack.pop() {
            if ino < 1 || ino as usize > self.ninodes() {
                errs.push(format!("inode number {ino} out of range"));
                continue;
            }
            if seen[ino as usize] {
                continue;
            }
            seen[ino as usize] = true;
            if self.imap(ino) != 1 {
                errs.push(format!("inode {ino} in use but free in the inode map"));
            }
            let mode = self.inode_word(ino, I_MODE);
            let size = self.inode_word(ino, I_SIZE) as usize;
            let mut count = 0;
            for lbn in 0..NDIRECT {
                let b = self.inode_word(ino, I_DIRECT + lbn) as usize;
                if b != 0 {
                    claim(b, ino, &mut errs, &mut owner);
                    count += 1;
                }
            }
            for (slot, double) in [(I_SINGLE, false), (I_DOUBLE, true)] {
                let ind = self.inode_word(ino, slot) as usize;
                if ind == 0 {
                    continue;
                }
                claim(ind, ino, &mut errs, &mut owner);
                count += 1;
                if ind < data || ind >= n {
                    continue;
                }
                for j in 0..WORDS_PER_BLOCK {
                    let b = self.word(ind, j) as usize;
                    if b == 0 {
                        continue;
                    }
                    claim(b, ino, &mut errs, &mut owner);
                    count += 1;
                    if double && b >= data && b < n {
                        for k in 0..WORDS_PER_BLOCK {
                            let b2 = self.word(b, k) as usize;
                            if b2 != 0 {
                                claim(b2, ino, &mut errs, &mut owner);
                                count += 1;
                            }
                        }
                    }
                }
            }
            if count != self.inode_word(ino, I_BLOCKS) {
                errs.push(format!("inode {ino}: {count} blocks, but the inode says {}", self.inode_word(ino, I_BLOCKS)));
            }
            if size > (count as usize) * BLOCK && mode / 4096 % 16 * 4096 == S_IFDIR {
                errs.push(format!("directory {ino}: size {size} beyond its blocks"));
            }
            if mode / 4096 % 16 * 4096 == S_IFDIR {
                match self.entries(ino) {
                    Err(e) => errs.push(format!("directory {ino}: {e}")),
                    Ok(es) => {
                        if es.first().map(|e| e.1.as_str()) != Some(".") || es.get(1).map(|e| e.1.as_str()) != Some("..") {
                            errs.push(format!("directory {ino}: missing . or .."));
                        }
                        for (_, name, child) in es {
                            if child < 1 || child as usize > self.ninodes() {
                                errs.push(format!("directory {ino}: entry {name:?} has inode {child}"));
                                continue;
                            }
                            if name != "." && name != ".." {
                                links[child as usize] += 1;
                                stack.push(child);
                                if self.inode_word(child, I_MODE) / 4096 % 16 * 4096 == S_IFDIR {
                                    links[ino as usize] += 1; // the child's ".."
                                    links[child as usize] += 1; // the child's "."
                                }
                            }
                        }
                    }
                }
            }
        }
        links[ROOT as usize] += 1; // the root's "."
        for ino in 1..=self.ninodes() as i64 {
            let used = self.imap(ino) == 1;
            if used && !seen[ino as usize] {
                errs.push(format!("inode {ino} allocated but unreachable"));
            }
            if seen[ino as usize] && self.inode_word(ino, I_NLINK) != links[ino as usize] {
                errs.push(format!("inode {ino}: nlink {} but {} links", self.inode_word(ino, I_NLINK), links[ino as usize]));
            }
        }
        let mut free_blocks = 0;
        for (b, &o) in owner.iter().enumerate().take(n).skip(data) {
            let m = self.bmap_entry(b);
            if o != 0 && m != 1 {
                errs.push(format!("block {b} used by inode {o} but not marked"));
            }
            if o == 0 && m == 1 {
                errs.push(format!("block {b} marked used but unowned"));
            }
            if m == 0 {
                free_blocks += 1;
            }
        }
        if free_blocks != self.sb(SB_FREE_BLOCKS) {
            errs.push(format!("{free_blocks} free blocks, superblock says {}", self.sb(SB_FREE_BLOCKS)));
        }
        let free_inodes = (1..=self.ninodes() as i64).filter(|&i| self.imap(i) == 0).count() as i64;
        if free_inodes != self.sb(SB_FREE_INODES) {
            errs.push(format!("{free_inodes} free inodes, superblock says {}", self.sb(SB_FREE_INODES)));
        }
        errs
    }
}
