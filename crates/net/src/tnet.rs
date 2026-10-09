//! TNET on the host (docs/TNET.md): TCRC-18, TL frames, and the wire that
//! joins two machines' `TLINK` cards, with fault injection for tests.

use setun_core::Machine;

/// g(x) = x^18 + x^13 + x − 1: x^18 ≡ −x^13 − x + 1.
const FOLD: i64 = -(3i64.pow(13)) - 3 + 1;
const TOP: i64 = 3i64.pow(17);
/// 18 trits of +1, the initial value.
const INIT: i64 = (3i64.pow(18) - 1) / 2;

/// The trits of a tryte, most significant first.
fn tryte_trits(v: i64) -> [i64; 9] {
    let mut t = [0; 9];
    let mut v = v;
    for k in (0..9).rev() {
        let d = (v + 1).rem_euclid(3) - 1;
        t[k] = d;
        v = (v - d) / 3;
    }
    t
}

/// GF(3) addition, trit by trit, of two balanced numbers.
fn gf3_add(a: i64, b: i64) -> i64 {
    let (mut a, mut b, mut out, mut w) = (a, b, 0, 1);
    while a != 0 || b != 0 {
        let (x, y) = ((a + 1).rem_euclid(3) - 1, (b + 1).rem_euclid(3) - 1);
        out += ((x + y + 1).rem_euclid(3) - 1) * w;
        a = (a - x) / 3;
        b = (b - y) / 3;
        w *= 3;
    }
    out
}

fn step(r: i64, t: i64) -> i64 {
    let top = (r + if r >= 0 { TOP / 2 } else { -(TOP / 2) }) / TOP;
    let low = r - top * TOP;
    gf3_add(gf3_add(low * 3, t), top * FOLD)
}

/// TCRC-18 of a message of trytes: the 18-trit remainder, balanced.
pub fn tcrc18(msg: &[i16]) -> i64 {
    let mut r = INIT;
    for &v in msg {
        for t in tryte_trits(v as i64) {
            r = step(r, t);
        }
    }
    for _ in 0..18 {
        r = step(r, 0);
    }
    r
}

/// The check's two trytes, the higher first.
pub fn check_trytes(msg: &[i16]) -> [i16; 2] {
    let c = tcrc18(msg);
    let lo = (c + 9841).rem_euclid(19683) - 9841;
    [((c - lo) / 19683) as i16, lo as i16]
}

/// A word as three trytes, the most significant first.
pub fn word_trytes(v: i64) -> [i16; 3] {
    let t2 = (v + 9841).rem_euclid(19683) - 9841;
    let v1 = (v - t2) / 19683;
    let t1 = (v1 + 9841).rem_euclid(19683) - 9841;
    [((v1 - t1) / 19683) as i16, t1 as i16, t2 as i16]
}

pub fn trytes_word(t: &[i16]) -> i64 {
    t[0] as i64 * 387_420_489 + t[1] as i64 * 19683 + t[2] as i64
}

pub const TL_TNP: i16 = 1;
pub const TL_ECHO: i16 = 2;
pub const TL_ECHO_REPLY: i16 = 3;

/// A whole TL frame.
pub fn frame(dst: i64, src: i64, typ: i16, payload: &[i16]) -> Vec<i16> {
    let mut f = Vec::with_capacity(9 + payload.len());
    f.extend(word_trytes(dst));
    f.extend(word_trytes(src));
    f.push(typ);
    f.extend_from_slice(payload);
    let c = check_trytes(&f);
    f.extend(c);
    f
}

/// A received frame, if its check is right: (dst, src, type, payload).
pub fn parse(f: &[i16]) -> Option<(i64, i64, i16, Vec<i16>)> {
    if f.len() < 9 || check_trytes(&f[..f.len() - 2]) != [f[f.len() - 2], f[f.len() - 1]] {
        return None;
    }
    Some((trytes_word(&f[0..3]), trytes_word(&f[3..6]), f[6], f[7..f.len() - 2].to_vec()))
}

/// Flip trit `k` (0 = the most significant of the first tryte) of a frame by
/// `e` (+1 or −1, in GF(3)).
pub fn flip_trit(f: &mut [i16], k: usize, e: i64) {
    let (i, b) = (k / 9, k % 9);
    let mut t = tryte_trits(f[i] as i64);
    t[b] = (t[b] + e + 1).rem_euclid(3) - 1;
    f[i] = t.iter().fold(0, |v, &d| v * 3 + d) as i16;
}

/// What the wire does to frames in one direction.
#[derive(Default, Clone)]
pub struct Faults {
    /// Flip trit k of the n-th frame (counting from 0) by e.
    pub flips: Vec<(usize, usize, i64)>,
    /// Drop the n-th frames.
    pub drops: Vec<usize>,
    /// Duplicate the n-th frames.
    pub dups: Vec<usize>,
}

/// Two machines' TLINK cards joined by a wire.
#[derive(Default)]
pub struct Wire {
    /// Faults for frames from a to b, and from b to a.
    pub ab: Faults,
    pub ba: Faults,
    sent_ab: usize,
    sent_ba: usize,
    /// Every frame that crossed, as sent (before faults): (from a, frame).
    pub log: Vec<(bool, Vec<i16>)>,
}

impl Wire {
    /// Attach both cards, with station addresses.
    pub fn attach(a: &mut Machine, b: &mut Machine, addr_a: i64, addr_b: i64) {
        a.dev.tlink.link = true;
        a.dev.tlink.addr = addr_a;
        b.dev.tlink.link = true;
        b.dev.tlink.addr = addr_b;
    }

    fn deliver(faults: &Faults, n: usize, mut f: Vec<i16>, to: &mut Machine) {
        if faults.drops.contains(&n) {
            return;
        }
        for &(m, k, e) in &faults.flips {
            if m == n && k < f.len() * 9 {
                flip_trit(&mut f, k, e);
            }
        }
        if faults.dups.contains(&n) {
            to.dev.tlink_receive(&f);
        }
        to.dev.tlink_receive(&f);
    }

    /// Move the frames each card has sent to the other.
    pub fn pump(&mut self, a: &mut Machine, b: &mut Machine) {
        for f in a.dev.tlink_take_tx() {
            self.log.push((true, f.clone()));
            Self::deliver(&self.ab, self.sent_ab, f, b);
            self.sent_ab += 1;
        }
        for f in b.dev.tlink_take_tx() {
            self.log.push((false, f.clone()));
            Self::deliver(&self.ba, self.sent_ba, f, a);
            self.sent_ba += 1;
        }
    }
}

/// "a.b.c": three trytes in decimal, each −9841 … 9841.
pub fn parse_addr(s: &str) -> Option<i64> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let mut v = 0i64;
    for p in parts {
        let t: i64 = p.parse().ok()?;
        if !(-9841..=9841).contains(&t) {
            return None;
        }
        v = v * 19683 + t;
    }
    Some(v)
}

/// The dotted form of an address.
pub fn addr_str(a: i64) -> String {
    let t = word_trytes(a);
    format!("{}.{}.{}", t[0], t[1], t[2])
}

/// A `TLINK` joined to another setun process over UDP (docs/TNET.md §1):
/// one datagram per frame, each tryte a 16-bit little-endian integer.
/// `listen:PORT` waits for the peer and answers whoever sent the last
/// frame; `connect:HOST:PORT` sends to that address.
pub struct UdpLink {
    sock: std::net::UdpSocket,
    peer: Option<std::net::SocketAddr>,
}

impl UdpLink {
    pub fn open(spec: &str) -> std::io::Result<UdpLink> {
        use std::io::{Error, ErrorKind};
        let bad = || Error::new(ErrorKind::InvalidInput, "expected listen:PORT or connect:HOST:PORT");
        let (sock, peer) = if let Some(port) = spec.strip_prefix("listen:") {
            let port: u16 = port.parse().map_err(|_| bad())?;
            (std::net::UdpSocket::bind(("0.0.0.0", port))?, None)
        } else if let Some(target) = spec.strip_prefix("connect:") {
            use std::net::ToSocketAddrs;
            let peer = target.to_socket_addrs()?.next().ok_or_else(bad)?;
            (std::net::UdpSocket::bind(("0.0.0.0", 0))?, Some(peer))
        } else {
            return Err(bad());
        };
        sock.set_nonblocking(true)?;
        Ok(UdpLink { sock, peer })
    }

    /// Frames out to the peer, and in from it.
    pub fn poll(&mut self, m: &mut Machine) {
        for f in m.dev.tlink_take_tx() {
            if let Some(p) = self.peer {
                let bytes: Vec<u8> = f.iter().flat_map(|t| t.to_le_bytes()).collect();
                let _ = self.sock.send_to(&bytes, p);
            }
        }
        let mut buf = [0u8; 2 * 2187 + 2];
        while let Ok((n, from)) = self.sock.recv_from(&mut buf) {
            if self.peer.is_none() {
                self.peer = Some(from);
            }
            if n % 2 == 0 {
                let f: Vec<i16> = buf[..n].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
                m.dev.tlink_receive(&f);
            }
        }
    }
}

/// A TNP packet (docs/TNET.md §3): header and payload, hop limit 27.
pub fn tnp_packet(dst: i64, src: i64, proto: i16, payload: &[i16]) -> Vec<i16> {
    let mut p = Vec::with_capacity(9 + payload.len());
    p.extend(word_trytes(dst));
    p.extend(word_trytes(src));
    p.push(proto);
    p.push(27);
    p.push(payload.len() as i16);
    p.extend_from_slice(payload);
    p
}
