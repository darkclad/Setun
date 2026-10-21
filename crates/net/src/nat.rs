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
//! IPv6 works the same way (docs/IPV6.md §5): a SYN to an address off
//! fd00::/64, or to fd00::2 (the host's ::1), goes out through a host
//! IPv6 socket, and its frames are rewritten to and from fd00::2.
//!
//! **DNS**: 10.0.2.3:53 and [fd00::3]:53 answer A and AAAA queries with the
//! host's resolver (on a thread), other types with no records, and broken
//! queries with FORMERR.

use smoltcp::wire::Ipv4Address;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream, ToSocketAddrs};
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

/// A DNS query waiting for the resolver: who asked, whether on the IPv6
/// server, the query, and where the addresses will come.
pub(crate) type PendingDns = (smoltcp::wire::IpEndpoint, bool, Vec<u8>, Receiver<Vec<IpAddr>>);

pub(crate) struct Flow {
    pub gport: u16,
    pub dst: IpAddr,
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
    pub aliases: HashMap<IpAddr, IpAddr>,
    /// Names DNS answers itself (tests), before asking the host.
    pub names: HashMap<String, Vec<IpAddr>>,
    /// Queries waiting for the resolver.
    pub pending_dns: Vec<PendingDns>,
    pub connect_timeout: Duration,
}

/// An Ethernet frame's IP layer: (IPv6?, where the transport header
/// starts, the protocol). IPv6 with extension headers is not carried.
pub(crate) fn ip_layer(f: &[u8]) -> Option<(bool, usize, u8)> {
    if f.len() >= 34 && f[12] == 0x08 && f[13] == 0x00 && f[14] >> 4 == 4 {
        let ihl = (f[14] & 15) as usize * 4;
        if ihl < 20 || 14 + ihl > f.len() {
            return None;
        }
        return Some((false, 14 + ihl, f[23]));
    }
    if f.len() >= 54 && is_v6(f) && f[14] >> 4 == 6 {
        return Some((true, 54, f[20]));
    }
    None
}

pub(crate) fn is_v6(f: &[u8]) -> bool {
    f.len() >= 14 && f[12] == 0x86 && f[13] == 0xdd
}

/// Where a TCP header starts in an Ethernet/IPv4 or IPv6 frame: (IPv6?, offset).
pub(crate) fn tcp_offsets(f: &[u8]) -> Option<(bool, usize)> {
    let (v6, t, proto) = ip_layer(f)?;
    (proto == 6 && t + 20 <= f.len()).then_some((v6, t))
}

fn v6_at(f: &[u8], at: usize) -> IpAddr {
    let mut o = [0u8; 16];
    o.copy_from_slice(&f[at..at + 16]);
    IpAddr::V6(Ipv6Addr::from(o))
}

pub(crate) fn ip_src(f: &[u8]) -> IpAddr {
    if is_v6(f) { v6_at(f, 22) } else { IpAddr::V4(Ipv4Addr::new(f[26], f[27], f[28], f[29])) }
}

pub(crate) fn ip_dst(f: &[u8]) -> IpAddr {
    if is_v6(f) { v6_at(f, 38) } else { IpAddr::V4(Ipv4Addr::new(f[30], f[31], f[32], f[33])) }
}

fn set_addr(f: &mut [u8], at: usize, a: IpAddr) {
    match a {
        IpAddr::V4(x) => f[at..at + 4].copy_from_slice(&x.octets()),
        IpAddr::V6(x) => f[at..at + 16].copy_from_slice(&x.octets()),
    }
}

pub(crate) fn set_src(f: &mut [u8], a: IpAddr) {
    set_addr(f, if is_v6(f) { 22 } else { 26 }, a);
}

pub(crate) fn set_dst(f: &mut [u8], a: IpAddr) {
    set_addr(f, if is_v6(f) { 38 } else { 30 }, a);
}

/// The gateway's address in the frame's family.
pub(crate) fn gateway_for(v6: bool) -> IpAddr {
    if v6 { IpAddr::V6(crate::GATEWAY6) } else { IpAddr::V4(crate::GATEWAY) }
}

/// The guest's own link, which the NAT leaves alone: 10.0.2.0/24, or
/// fd00::/64, link-local and multicast addresses.
pub(crate) fn on_link(a: IpAddr) -> bool {
    match a {
        IpAddr::V4(x) => x.octets()[..3] == [10, 0, 2],
        IpAddr::V6(x) => {
            let s = x.segments();
            (s[0] == 0xfd00 && s[1] == 0 && s[2] == 0 && s[3] == 0) || s[0] & 0xffc0 == 0xfe80 || x.is_multicast()
        }
    }
}

pub(crate) fn port_at(f: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([f[at], f[at + 1]])
}

pub(crate) fn set_port(f: &mut [u8], at: usize, p: u16) {
    f[at..at + 2].copy_from_slice(&p.to_be_bytes());
}

/// Recompute the IPv4 header checksum (IPv6 has none).
pub(crate) fn fix_ip_checksum(f: &mut [u8]) {
    if is_v6(f) {
        return;
    }
    let ihl = (f[14] & 15) as usize * 4;
    f[24] = 0;
    f[25] = 0;
    let c = crate::checksum(&[&f[14..14 + ihl]]);
    f[24..26].copy_from_slice(&c.to_be_bytes());
}

impl Nat {
    pub fn new() -> Nat {
        let mut n = Nat { next_standin: STANDIN_FIRST, connect_timeout: Duration::from_secs(10), ..Default::default() };
        n.aliases.insert(IpAddr::V4(crate::GATEWAY), IpAddr::V4(Ipv4Addr::LOCALHOST));
        n.aliases.insert(IpAddr::V6(crate::GATEWAY6), IpAddr::V6(Ipv6Addr::LOCALHOST));
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
    pub fn host_for(&self, dst: IpAddr) -> IpAddr {
        self.aliases.get(&dst).copied().unwrap_or(dst)
    }

    pub fn start_connect(&self, to: SocketAddr) -> Receiver<std::io::Result<TcpStream>> {
        let (tx, rx) = channel();
        let timeout = self.connect_timeout;
        std::thread::spawn(move || {
            let _ = tx.send(TcpStream::connect_timeout(&to, timeout));
        });
        rx
    }

    /// Resolve a name on a thread: its addresses of both families.
    pub fn start_resolve(&mut self, name: String) -> Receiver<Vec<IpAddr>> {
        let (tx, rx) = channel();
        if let Some(a) = self.names.get(&name.to_ascii_lowercase()) {
            let _ = tx.send(a.clone());
            return rx;
        }
        std::thread::spawn(move || {
            let addrs = (name.as_str(), 0)
                .to_socket_addrs()
                .map(|it| {
                    let mut v: Vec<IpAddr> = Vec::new();
                    for a in it {
                        if !v.contains(&a.ip()) {
                            v.push(a.ip());
                        }
                    }
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

/// An answer to `q` (the whole query) from the name's addresses: up to 8
/// A records for an A query, AAAA records for an AAAA query, none for other
/// types. A name with no addresses at all is NXDOMAIN (rcode 3); one with
/// addresses of the other family only is an empty answer.
pub(crate) fn answer(q: &[u8], parsed: &Query, addrs: &[IpAddr]) -> Vec<u8> {
    let want: Vec<&IpAddr> = addrs
        .iter()
        .filter(|a| match parsed.qtype {
            1 => a.is_ipv4(),
            28 => a.is_ipv6(),
            _ => false,
        })
        .take(8)
        .collect();
    let address_query = parsed.qtype == 1 || parsed.qtype == 28;
    let rcode = if address_query && addrs.is_empty() { 3 } else { 0 };
    let mut r = vec![q[0], q[1], 0x80 | (q[2] & 0x01), 0x80 | rcode, 0, 1, 0, want.len() as u8, 0, 0, 0, 0];
    r.extend(&parsed.question);
    for a in want {
        match a {
            IpAddr::V4(x) => {
                r.extend([0xc0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4]);
                r.extend(x.octets());
            }
            IpAddr::V6(x) => {
                r.extend([0xc0, 12, 0, 28, 0, 1, 0, 0, 0, 60, 0, 16]);
                r.extend(x.octets());
            }
        }
    }
    r
}

/// FORMERR for a query too broken to answer (its id, if it has one).
pub(crate) fn format_error(q: &[u8]) -> Vec<u8> {
    let (a, b) = (*q.first().unwrap_or(&0), *q.get(1).unwrap_or(&0));
    vec![a, b, 0x80, 0x81, 0, 0, 0, 0, 0, 0, 0, 0]
}

// ---- UDP -----------------------------------------------------------------------------------

/// A UDP conversation the guest started: its datagrams go out of a host
/// socket of their own, and whatever comes back to that socket is the reply.
pub(crate) struct UdpFlow {
    pub gport: u16,
    /// The guest's own address, which replies go to.
    pub guest: IpAddr,
    pub dst: IpAddr,
    pub dport: u16,
    pub sock: std::net::UdpSocket,
    pub guest_mac: [u8; 6],
    pub last: std::time::Instant,
}

/// How long a quiet UDP conversation is kept.
pub(crate) const UDP_IDLE: Duration = Duration::from_secs(60);
pub(crate) const UDP_FLOWS: usize = 81;

/// Where the parts of an Ethernet/IPv4 or IPv6 UDP frame are: (UDP header
/// start, payload start, payload length). None if it is not one, or an
/// IPv4 fragment.
pub(crate) fn udp_offsets(f: &[u8]) -> Option<(usize, usize, usize)> {
    let (v6, u, proto) = ip_layer(f)?;
    if proto != 17 {
        return None;
    }
    if !v6 && u16::from_be_bytes([f[20], f[21]]) & 0x3fff != 0 {
        return None;
    }
    if u + 8 > f.len() {
        return None;
    }
    let ulen = u16::from_be_bytes([f[u + 4], f[u + 5]]) as usize;
    if ulen < 8 || u + ulen > f.len() {
        return None;
    }
    Some((u, u + 8, ulen - 8))
}

/// An Ethernet/IPv4 or IPv6 UDP frame to the guest, checksums and all.
pub(crate) fn udp_frame(to_mac: [u8; 6], src: IpAddr, sport: u16, dst: IpAddr, dport: u16, data: &[u8]) -> Vec<u8> {
    let (IpAddr::V4(src), IpAddr::V4(dst)) = (src, dst) else { return udp6_frame(to_mac, src, sport, dst, dport, data) };
    let mut f = Vec::with_capacity(42 + data.len());
    f.extend_from_slice(&to_mac);
    f.extend_from_slice(&crate::GATEWAY_MAC);
    f.extend_from_slice(&[0x08, 0x00]);
    let total = 20 + 8 + data.len();
    f.extend_from_slice(&[0x45, 0, (total >> 8) as u8, total as u8, 0, 0, 0x40, 0, 64, 17, 0, 0]);
    f.extend_from_slice(&src.octets());
    f.extend_from_slice(&dst.octets());
    let ulen = 8 + data.len();
    f.extend_from_slice(&sport.to_be_bytes());
    f.extend_from_slice(&dport.to_be_bytes());
    f.extend_from_slice(&(ulen as u16).to_be_bytes());
    f.extend_from_slice(&[0, 0]);
    f.extend_from_slice(data);
    fix_ip_checksum(&mut f);
    // the UDP checksum over the pseudo-header; 0 goes as 0xFFFF
    let c = crate::checksum(&[&f[26..34], &[0, 17], &(ulen as u16).to_be_bytes(), &f[34..]]);
    f[40..42].copy_from_slice(&(if c == 0 { 0xffff } else { c }).to_be_bytes());
    f
}

fn udp6_frame(to_mac: [u8; 6], src: IpAddr, sport: u16, dst: IpAddr, dport: u16, data: &[u8]) -> Vec<u8> {
    let (IpAddr::V6(src), IpAddr::V6(dst)) = (src, dst) else { panic!("mixed families") };
    let ulen = 8 + data.len();
    let mut u = Vec::with_capacity(ulen);
    u.extend_from_slice(&sport.to_be_bytes());
    u.extend_from_slice(&dport.to_be_bytes());
    u.extend_from_slice(&(ulen as u16).to_be_bytes());
    u.extend_from_slice(&[0, 0]);
    u.extend_from_slice(data);
    let c = crate::checksum(&[&src.octets(), &dst.octets(), &(ulen as u32).to_be_bytes(), &[0, 0, 0, 17], &u]);
    u[6..8].copy_from_slice(&(if c == 0 { 0xffff } else { c }).to_be_bytes());
    let mut f = Vec::with_capacity(54 + ulen);
    f.extend_from_slice(&to_mac);
    f.extend_from_slice(&crate::GATEWAY_MAC);
    f.extend_from_slice(&[0x86, 0xdd, 0x60, 0, 0, 0]);
    f.extend_from_slice(&(ulen as u16).to_be_bytes());
    f.extend_from_slice(&[17, 64]);
    f.extend_from_slice(&src.octets());
    f.extend_from_slice(&dst.octets());
    f.extend_from_slice(&u);
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(name: &str, qtype: u16) -> Vec<u8> {
        let mut q = vec![0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        for l in name.split('.') {
            q.push(l.len() as u8);
            q.extend(l.as_bytes());
        }
        q.extend([0, (qtype >> 8) as u8, qtype as u8, 0, 1]);
        q
    }

    fn both() -> Vec<IpAddr> {
        vec!["192.0.2.7".parse().unwrap(), "2001:db8::7".parse().unwrap(), "2001:db8::8".parse().unwrap()]
    }

    #[test]
    fn aaaa_answers_carry_the_ipv6_addresses() {
        let q = query("example.test", 28);
        let r = answer(&q, &parse_query(&q).unwrap(), &both());
        assert_eq!(&r[..12], &[0x12, 0x34, 0x81, 0x80, 0, 1, 0, 2, 0, 0, 0, 0]);
        let rr = &r[12 + q.len() - 12..];
        assert_eq!(&rr[..12], &[0xc0, 12, 0, 28, 0, 1, 0, 0, 0, 60, 0, 16]);
        assert_eq!(&rr[12..28], &"2001:db8::7".parse::<Ipv6Addr>().unwrap().octets());
        assert_eq!(rr.len(), 2 * 28);
    }

    #[test]
    fn a_answers_keep_to_ipv4() {
        let q = query("example.test", 1);
        let r = answer(&q, &parse_query(&q).unwrap(), &both());
        assert_eq!(r[7], 1);
        assert_eq!(&r[r.len() - 4..], &[192, 0, 2, 7]);
    }

    #[test]
    fn no_record_of_the_type_is_empty_and_no_name_is_nxdomain() {
        let q = query("v4only.test", 28);
        let r = answer(&q, &parse_query(&q).unwrap(), &["192.0.2.1".parse().unwrap()]);
        assert_eq!((r[3] & 15, r[7]), (0, 0), "an empty answer, not NXDOMAIN");
        let r = answer(&q, &parse_query(&q).unwrap(), &[]);
        assert_eq!(r[3] & 15, 3, "NXDOMAIN");
        let mx = query("example.test", 15);
        let r = answer(&mx, &parse_query(&mx).unwrap(), &both());
        assert_eq!((r[3] & 15, r[7]), (0, 0), "other types: no records");
    }

    #[test]
    fn at_most_eight_records() {
        let many: Vec<IpAddr> = (1..=12).map(|i| format!("2001:db8::{i}").parse().unwrap()).collect();
        let q = query("many.test", 28);
        assert_eq!(answer(&q, &parse_query(&q).unwrap(), &many)[7], 8);
    }

    #[test]
    fn the_link_is_left_alone() {
        for a in ["10.0.2.2", "10.0.2.99", "fd00::2", "fd00::99", "fe80::1", "ff02::1"] {
            assert!(on_link(a.parse().unwrap()), "{a}");
        }
        for a in ["10.0.3.1", "8.8.8.8", "fd01::1", "2001:db8::1", "::1"] {
            assert!(!on_link(a.parse().unwrap()), "{a}");
        }
    }

    #[test]
    fn udp6_frames_check_out() {
        let f = udp6_frame([2; 6], "2001:db8::1".parse().unwrap(), 53, "fd00::5".parse().unwrap(), 4000, b"reply");
        assert_eq!(f.len(), 54 + 8 + 5);
        assert_eq!(&f[12..14], &[0x86, 0xdd]);
        let c = crate::checksum(&[&f[22..54], &13u32.to_be_bytes(), &[0, 0, 0, 17], &f[54..]]);
        assert_eq!(c, 0, "the checksum adds up");
    }

    #[test]
    fn odd_parts_carry_their_last_octet() {
        // RFC 1071's example, split at odd places: the same sum as in one piece
        let data = [0x00u8, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7];
        let whole = crate::checksum(&[&data]);
        assert_eq!(whole, !0xddf2u16);
        assert_eq!(crate::checksum(&[&data[..3], &data[3..5], &data[5..]]), whole);
        assert_eq!(crate::checksum(&[&data[..7]]), crate::checksum(&[&data[..7], &[0]]), "a lone last octet is padded");
    }
}
