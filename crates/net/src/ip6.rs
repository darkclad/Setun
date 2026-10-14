//! The gateway's IPv6 side (docs/IPV6.md §5): its addresses, and the
//! Router Advertisements it sends, which smoltcp does not do as a router.

use smoltcp::wire::Ipv6Address;

/// The link's prefix, fd00::/64 (QEMU's layout on a unique-local prefix).
pub const PREFIX6: Ipv6Address = Ipv6Address::new(0xfd00, 0, 0, 0, 0, 0, 0, 0);
/// The gateway: router, NAT, and stand-in for the host's ::1.
pub const GATEWAY6: Ipv6Address = Ipv6Address::new(0xfd00, 0, 0, 0, 0, 0, 0, 2);
/// DNS, a second address of the gateway.
pub const DNS6: Ipv6Address = Ipv6Address::new(0xfd00, 0, 0, 0, 0, 0, 0, 3);
/// The gateway's link-local address: its MAC 52:55:0a:00:02:02 as a modified EUI-64.
pub const GATEWAY6_LL: Ipv6Address = Ipv6Address::new(0xfe80, 0, 0, 0, 0x5055, 0x0aff, 0xfe00, 0x0202);
/// The address Trinux forms from its MAC 52:54:00:12:34:56.
pub const GUEST6: Ipv6Address = Ipv6Address::new(0xfd00, 0, 0, 0, 0x5054, 0x00ff, 0xfe12, 0x3456);

/// Seconds between unsolicited advertisements.
pub const RA_INTERVAL_S: i64 = 200;

/// The ones'-complement sum of 16-bit words, folded and complemented.
pub fn checksum(parts: &[&[u8]]) -> u16 {
    let mut sum: u32 = 0;
    for p in parts {
        for c in p.chunks(2) {
            sum += u32::from(c[0]) << 8 | u32::from(*c.get(1).unwrap_or(&0));
        }
    }
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// An Ethernet frame carrying an ICMPv6 message from `src` to `dst` (hop
/// limit `hop`), with its checksum filled in.
pub fn icmp6_frame(dst_mac: [u8; 6], src_mac: [u8; 6], src: Ipv6Address, dst: Ipv6Address, hop: u8, msg: &[u8]) -> Vec<u8> {
    let mut m = msg.to_vec();
    m[2] = 0;
    m[3] = 0;
    let len = (m.len() as u32).to_be_bytes();
    let c = checksum(&[&src.octets(), &dst.octets(), &len, &[0, 0, 0, 58], &m]);
    m[2..4].copy_from_slice(&c.to_be_bytes());
    let mut f = Vec::with_capacity(14 + 40 + m.len());
    f.extend_from_slice(&dst_mac);
    f.extend_from_slice(&src_mac);
    f.extend_from_slice(&[0x86, 0xdd]);
    f.extend_from_slice(&[0x60, 0, 0, 0]);
    f.extend_from_slice(&(m.len() as u16).to_be_bytes());
    f.push(58);
    f.push(hop);
    f.extend_from_slice(&src.octets());
    f.extend_from_slice(&dst.octets());
    f.extend_from_slice(&m);
    f
}

/// The gateway's Router Advertisement to all nodes: router lifetime 1800 s,
/// the prefix fd00::/64 (on-link, autonomous, valid 86400 s, preferred
/// 14400 s), MTU 1500, RDNSS fd00::3.
pub fn router_advertisement(gw_mac: [u8; 6]) -> Vec<u8> {
    let mut m = vec![134, 0, 0, 0, 64, 0];
    m.extend_from_slice(&1800u16.to_be_bytes());
    m.extend_from_slice(&[0; 8]); // reachable time, retransmission timer: unspecified
    m.extend_from_slice(&[1, 1]);
    m.extend_from_slice(&gw_mac);
    m.extend_from_slice(&[5, 1, 0, 0]);
    m.extend_from_slice(&1500u32.to_be_bytes());
    m.extend_from_slice(&[3, 4, 64, 0xc0]);
    m.extend_from_slice(&86400u32.to_be_bytes());
    m.extend_from_slice(&14400u32.to_be_bytes());
    m.extend_from_slice(&[0; 4]);
    m.extend_from_slice(&PREFIX6.octets());
    m.extend_from_slice(&[25, 3, 0, 0]);
    m.extend_from_slice(&1800u32.to_be_bytes());
    m.extend_from_slice(&DNS6.octets());
    icmp6_frame([0x33, 0x33, 0, 0, 0, 1], gw_mac, GATEWAY6_LL, Ipv6Address::new(0xff02, 0, 0, 0, 0, 0, 0, 1), 255, &m)
}

/// Is this frame a Router Solicitation (an ICMPv6 type 133 right after the
/// IPv6 header)?
pub fn is_router_solicitation(f: &[u8]) -> bool {
    f.len() >= 14 + 40 + 8 && f[12] == 0x86 && f[13] == 0xdd && f[14 + 6] == 58 && f[14 + 40] == 133
}
