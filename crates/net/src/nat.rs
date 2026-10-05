//! Outbound connections and DNS for the guest (docs/NETWORK.md §9).
//!
//! **TCP**: a SYN the guest sends to an outside address (or to the gateway
//! itself, which stands for the host's 127.0.0.1, as in QEMU) starts a host
//! connection on a thread. When it succeeds, smoltcp gets a listening
//! socket on a local "stand-in" port, and the guest's frames are rewritten
//! on the way in (destination → 10.0.2.2:stand-in) and out (source → the
//! address the guest asked for). The data are then copied like a port
//! forward's. When the host connection fails, the SYN reaches smoltcp with
//! nobody listening, and smoltcp's RST tells the guest "refused".
//!
//! **DNS**: 10.0.2.3:53 answers A queries with the host's resolver (on a
//! thread), AAAA and other types with no records, and broken queries with
//! FORMERR.

use smoltcp::wire::Ipv4Address;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

/// The DNS server the guest is told about.
pub const DNS: Ipv4Address = Ipv4Address::new(10, 0, 2, 3);

/// Stand-in ports: below smoltcp's own ephemeral range (49152 up).
pub(crate) const STANDIN_FIRST: u16 = 40000;
pub(crate) const STANDIN_LAST: u16 = 49151;

pub(crate) enum FlowState {
    /// The host connection is being made; the guest's SYN waits.
    Connecting(Receiver<std::io::Result<TcpStream>>, Vec<u8>),
    /// Connected (or refused): frames are rewritten.
    Open,
}

pub(crate) struct Flow {
    pub gport: u16,
    pub dst: Ipv4Address,
    pub dport: u16,
    pub standin: u16,
    pub state: FlowState,
    /// The connection copying its data (index into Network::conns).
    pub conn: Option<usize>,
}

#[derive(Default)]
pub(crate) struct Nat {
    pub flows: Vec<Flow>,
    pub next_standin: u16,
    /// Addresses the guest sees, and the host address each stands for.
    pub aliases: HashMap<Ipv4Address, IpAddr>,
    /// Names DNS answers itself (tests), before asking the host.
    pub names: HashMap<String, Ipv4Addr>,
    pub pending_dns: Vec<(u16, Vec<u8>, Receiver<Vec<Ipv4Addr>>)>,
    pub connect_timeout: Duration,
}

/// Where the parts of an Ethernet/IPv4/TCP frame are: (IP header start,
/// TCP header start).
pub(crate) fn tcp_offsets(f: &[u8]) -> Option<(usize, usize)> {
    if f.len() < 54 || f[12] != 0x08 || f[13] != 0x00 || f[14] >> 4 != 4 || f[23] != 6 {
        return None;
    }
    let ihl = (f[14] & 15) as usize * 4;
    if ihl < 20 || 14 + ihl + 20 > f.len() {
        return None;
    }
    Some((14, 14 + ihl))
}

pub(crate) fn ip_src(f: &[u8]) -> Ipv4Address {
    Ipv4Address::new(f[26], f[27], f[28], f[29])
}

pub(crate) fn ip_dst(f: &[u8]) -> Ipv4Address {
    Ipv4Address::new(f[30], f[31], f[32], f[33])
}

pub(crate) fn port_at(f: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([f[at], f[at + 1]])
}

pub(crate) fn set_port(f: &mut [u8], at: usize, p: u16) {
    f[at..at + 2].copy_from_slice(&p.to_be_bytes());
}

pub(crate) fn set_ip(f: &mut [u8], at: usize, a: Ipv4Address) {
    f[at..at + 4].copy_from_slice(&a.octets());
}

/// Recompute the IPv4 header checksum.
pub(crate) fn fix_ip_checksum(f: &mut [u8]) {
    let ihl = (f[14] & 15) as usize * 4;
    f[24] = 0;
    f[25] = 0;
    let mut sum: u32 = 0;
    for c in f[14..14 + ihl].chunks(2) {
        sum += u16::from_be_bytes([c[0], c[1]]) as u32;
    }
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    let c = !(sum as u16);
    f[24] = (c >> 8) as u8;
    f[25] = c as u8;
}

impl Nat {
    pub fn new() -> Nat {
        let mut n = Nat { next_standin: STANDIN_FIRST, connect_timeout: Duration::from_secs(10), ..Default::default() };
        n.aliases.insert(crate::GATEWAY, IpAddr::V4(Ipv4Addr::LOCALHOST));
        n
    }

    pub fn standin(&mut self) -> u16 {
        loop {
            let p = self.next_standin;
            self.next_standin = if p == STANDIN_LAST { STANDIN_FIRST } else { p + 1 };
            if !self.flows.iter().any(|f| f.standin == p) {
                return p;
            }
        }
    }

    /// The host address a guest destination stands for.
    pub fn host_for(&self, dst: Ipv4Address) -> IpAddr {
        self.aliases.get(&dst).copied().unwrap_or(IpAddr::V4(Ipv4Addr::from(dst.octets())))
    }

    pub fn start_connect(&self, to: SocketAddr) -> Receiver<std::io::Result<TcpStream>> {
        let (tx, rx) = channel();
        let timeout = self.connect_timeout;
        std::thread::spawn(move || {
            let _ = tx.send(TcpStream::connect_timeout(&to, timeout));
        });
        rx
    }

    /// Resolve a name on a thread.
    pub fn start_resolve(&mut self, name: String) -> Receiver<Vec<Ipv4Addr>> {
        let (tx, rx) = channel();
        if let Some(a) = self.names.get(&name.to_ascii_lowercase()) {
            let _ = tx.send(vec![*a]);
            return rx;
        }
        std::thread::spawn(move || {
            let addrs = (name.as_str(), 0)
                .to_socket_addrs()
                .map(|it| {
                    let mut v: Vec<Ipv4Addr> = it
                        .filter_map(|a| match a.ip() {
                            IpAddr::V4(x) => Some(x),
                            _ => None,
                        })
                        .collect();
                    v.dedup();
                    v
                })
                .unwrap_or_default();
            let _ = tx.send(addrs);
        });
        rx
    }
}

// ---- DNS messages (RFC 1035) -----------------------------------------------------------

pub(crate) struct Query {
    pub name: String,
    pub qtype: u16,
    /// The question section as received (copied into the answer).
    pub question: Vec<u8>,
}

/// Parse a query with one question. None if it is malformed.
pub(crate) fn parse_query(q: &[u8]) -> Option<Query> {
    if q.len() < 12 || q[2] & 0x80 != 0 || u16::from_be_bytes([q[4], q[5]]) != 1 {
        return None;
    }
    let mut i = 12;
    let mut labels = Vec::new();
    loop {
        let len = *q.get(i)? as usize;
        if len == 0 {
            i += 1;
            break;
        }
        if len > 63 {
            return None; // no compression in a question
        }
        let l = q.get(i + 1..i + 1 + len)?;
        labels.push(String::from_utf8(l.to_vec()).ok()?);
        i += 1 + len;
        if i > 12 + 255 {
            return None;
        }
    }
    let qtype = u16::from_be_bytes([*q.get(i)?, *q.get(i + 1)?]);
    let _class = u16::from_be_bytes([*q.get(i + 2)?, *q.get(i + 3)?]);
    if labels.is_empty() {
        return None;
    }
    Some(Query { name: labels.join("."), qtype, question: q[12..i + 4].to_vec() })
}

/// An answer to `q` (the whole query) with these A records; rcode 3
/// (NXDOMAIN) when an A query found nothing.
pub(crate) fn answer(q: &[u8], parsed: &Query, addrs: &[Ipv4Addr]) -> Vec<u8> {
    let a_query = parsed.qtype == 1;
    let n = if a_query { addrs.len().min(8) } else { 0 };
    let rcode = if a_query && n == 0 { 3 } else { 0 };
    let mut r = vec![q[0], q[1], 0x80 | (q[2] & 0x01), 0x80 | rcode, 0, 1, 0, n as u8, 0, 0, 0, 0];
    r.extend(&parsed.question);
    for a in &addrs[..n] {
        r.extend([0xc0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4]);
        r.extend(a.octets());
    }
    r
}

/// FORMERR for a query too broken to answer (its id, if it has one).
pub(crate) fn format_error(q: &[u8]) -> Vec<u8> {
    let (a, b) = (*q.first().unwrap_or(&0), *q.get(1).unwrap_or(&0));
    vec![a, b, 0x80, 0x81, 0, 0, 0, 0, 0, 0, 0, 0]
}
