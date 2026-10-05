//! Random-number generator `TRNG` (ISA §8.2, block 11).
//!
//! Registers: 0 `DATA` (read: the next random octet, 0..255), 1 `SOURCE`
//! (read-only: +1 seeded from the host's entropy, 0 from a fixed seed).
//!
//! The octets come from ChaCha20 (RFC 8439 block function) keyed by a
//! 256-bit seed. A front end seeds it from the operating system; without
//! that the seed is all zeros, so a test run is reproducible.

#[derive(Clone)]
pub struct Rng {
    key: [u32; 8],
    counter: u64,
    block: [u8; 64],
    used: usize,
    /// Seeded from the host's entropy (not a fixed seed).
    pub from_host: bool,
}

impl Default for Rng {
    fn default() -> Self {
        Rng { key: [0; 8], counter: 0, block: [0; 64], used: 64, from_host: false }
    }
}

fn quarter(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(16);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(12);
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(8);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(7);
}

impl Rng {
    /// Key the generator; `from_host` says whether the seed is real entropy.
    pub fn seed(&mut self, seed: [u8; 32], from_host: bool) {
        for (k, c) in self.key.iter_mut().zip(seed.chunks_exact(4)) {
            *k = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
        }
        self.counter = 0;
        self.used = 64;
        self.from_host = from_host;
    }

    fn refill(&mut self) {
        let mut s = [0u32; 16];
        s[..4].copy_from_slice(&[0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574]);
        s[4..12].copy_from_slice(&self.key);
        s[12] = self.counter as u32;
        s[13] = (self.counter >> 32) as u32;
        let init = s;
        for _ in 0..10 {
            quarter(&mut s, 0, 4, 8, 12);
            quarter(&mut s, 1, 5, 9, 13);
            quarter(&mut s, 2, 6, 10, 14);
            quarter(&mut s, 3, 7, 11, 15);
            quarter(&mut s, 0, 5, 10, 15);
            quarter(&mut s, 1, 6, 11, 12);
            quarter(&mut s, 2, 7, 8, 13);
            quarter(&mut s, 3, 4, 9, 14);
        }
        for (i, w) in s.iter().enumerate() {
            self.block[4 * i..4 * i + 4].copy_from_slice(&w.wrapping_add(init[i]).to_le_bytes());
        }
        self.counter += 1;
        self.used = 0;
    }

    pub fn next_octet(&mut self) -> u8 {
        if self.used == 64 {
            self.refill();
        }
        let b = self.block[self.used];
        self.used += 1;
        b
    }

    pub fn read(&mut self, r: i64) -> i64 {
        match r {
            0 => self.next_octet() as i64,
            1 => self.from_host as i64,
            _ => 0,
        }
    }
}

/// 256 bits from the host operating system, through the standard library's
/// randomly keyed hasher (it is seeded from the OS on first use per thread
/// and per `RandomState`), so no extra dependency is needed.
#[cfg(not(target_arch = "wasm32"))]
pub fn host_entropy() -> [u8; 32] {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut out = [0u8; 32];
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    for (i, chunk) in out.chunks_exact_mut(8).enumerate() {
        let mut h = RandomState::new().build_hasher();
        h.write_u128(t);
        h.write_usize(i);
        h.write_u32(std::process::id());
        chunk.copy_from_slice(&h.finish().to_le_bytes());
    }
    out
}
