//! The gateway without a machine (`Network::new`, `exchange`), as a relay
//! for a card in a browser uses it, and its public-only mode: the host
//! behind 10.0.2.2 and private addresses are refused, public ones are not.

use setun_core::devices::DEFAULT_MAC;
use setun_net::{Network, GATEWAY, GATEWAY_MAC, GUEST};
use smoltcp::phy::ChecksumCapabilities;
use smoltcp::wire::*;
use std::net::{Ipv4Addr, TcpListener, UdpSocket};

fn eth(ethertype: EthernetProtocol, dst: [u8; 6], payload_len: usize, fill: impl FnOnce(&mut [u8])) -> Vec<u8> {
    let repr = EthernetRepr { src_addr: EthernetAddress(DEFAULT_MAC), dst_addr: EthernetAddress(dst), ethertype };
    let mut buf = vec![0u8; repr.buffer_len() + payload_len];
    let mut f = EthernetFrame::new_unchecked(&mut buf[..]);
    repr.emit(&mut f);
    fill(f.payload_mut());
    buf
}

fn arp_request() -> Vec<u8> {
    let arp = ArpRepr::EthernetIpv4 {
        operation: ArpOperation::Request,
        source_hardware_addr: EthernetAddress(DEFAULT_MAC),
        source_protocol_addr: GUEST,
        target_hardware_addr: EthernetAddress([0; 6]),
        target_protocol_addr: GATEWAY,
    };
    eth(EthernetProtocol::Arp, [0xff; 6], arp.buffer_len(), |p| arp.emit(&mut ArpPacket::new_unchecked(p)))
}

fn ipv4(dst: Ipv4Address, proto: IpProtocol, payload_len: usize, fill: impl FnOnce(&mut [u8], &Ipv4Repr)) -> Vec<u8> {
    let ip = Ipv4Repr { src_addr: GUEST, dst_addr: dst, next_header: proto, payload_len, hop_limit: 64 };
    eth(EthernetProtocol::Ipv4, GATEWAY_MAC, ip.buffer_len() + payload_len, |p| {
        let mut pkt = Ipv4Packet::new_unchecked(p);
        ip.emit(&mut pkt, &ChecksumCapabilities::default());
        fill(pkt.payload_mut(), &ip);
    })
}

fn syn(dst: Ipv4Address, dport: u16) -> Vec<u8> {
    let tcp = TcpRepr {
        src_port: 40001,
        dst_port: dport,
        control: TcpControl::Syn,
        seq_number: TcpSeqNumber(1000),
        ack_number: None,
        window_len: 19683,
        window_scale: None,
        max_seg_size: Some(1460),
        sack_permitted: false,
        sack_ranges: [None, None, None],
        timestamp: None,
        payload: &[],
    };
    ipv4(dst, IpProtocol::Tcp, tcp.buffer_len(), |p, ip| {
        tcp.emit(&mut TcpPacket::new_unchecked(p), &IpAddress::Ipv4(ip.src_addr), &IpAddress::Ipv4(ip.dst_addr), &ChecksumCapabilities::default())
    })
}

fn udp(dst: Ipv4Address, dport: u16, data: &[u8]) -> Vec<u8> {
    let u = UdpRepr { src_port: 40002, dst_port: dport };
    ipv4(dst, IpProtocol::Udp, u.header_len() + data.len(), |p, ip| {
        u.emit(
            &mut UdpPacket::new_unchecked(p),
            &IpAddress::Ipv4(ip.src_addr),
            &IpAddress::Ipv4(ip.dst_addr),
            data.len(),
            |b| b.copy_from_slice(data),
            &ChecksumCapabilities::default(),
        )
    })
}

/// The TCP control flags of the guest-bound segments from `src`.
fn tcp_flags_from(frames: &[Vec<u8>], src: Ipv4Address) -> Vec<u8> {
    let mut out = Vec::new();
    for f in frames {
        let Ok(e) = EthernetFrame::new_checked(&f[..]) else { continue };
        if e.ethertype() != EthernetProtocol::Ipv4 {
            continue;
        }
        let Ok(ip) = Ipv4Packet::new_checked(e.payload()) else { continue };
        if ip.next_header() != IpProtocol::Tcp || ip.src_addr() != src {
            continue;
        }
        let Ok(t) = TcpPacket::new_checked(ip.payload()) else { continue };
        out.push((t.rst() as u8) << 2 | (t.syn() as u8) << 1 | t.ack() as u8);
    }
    out
}

/// Run the gateway on a wall clock until `done` says so (or two seconds).
fn run(net: &mut Network, first: Vec<Vec<u8>>, mut done: impl FnMut(&[Vec<u8>]) -> bool) -> Vec<Vec<u8>> {
    let t0 = std::time::Instant::now();
    let mut all = Vec::new();
    let mut input = first;
    while t0.elapsed() < std::time::Duration::from_secs(2) {
        let us = t0.elapsed().as_micros() as i64;
        all.extend(net.exchange(us, std::mem::take(&mut input)));
        if done(&all) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    all
}

#[test]
fn the_gateway_works_without_a_machine() {
    let mut net = Network::new(0);
    let out = net.exchange(0, vec![arp_request()]);
    let reply = out.iter().find_map(|f| {
        let e = EthernetFrame::new_checked(&f[..]).ok()?;
        (e.ethertype() == EthernetProtocol::Arp).then(|| ArpRepr::parse(&ArpPacket::new_checked(e.payload()).ok()?).ok())?
    });
    match reply {
        Some(ArpRepr::EthernetIpv4 { operation: ArpOperation::Reply, source_protocol_addr, source_hardware_addr, .. }) => {
            assert_eq!(source_protocol_addr, GATEWAY);
            assert_eq!(source_hardware_addr, EthernetAddress(GATEWAY_MAC));
        }
        other => panic!("no ARP reply: {other:?}"),
    }
}

#[test]
fn the_host_is_reached_as_10_0_2_2_unless_public_only() {
    let host = TcpListener::bind("127.0.0.1:0").unwrap();
    host.set_nonblocking(true).unwrap();
    let port = host.local_addr().unwrap().port();
    // normally: the SYN to 10.0.2.2 becomes a connection to the host's 127.0.0.1
    let mut net = Network::new(0);
    let out = run(&mut net, vec![arp_request(), syn(GATEWAY, port)], |o| tcp_flags_from(o, GATEWAY).contains(&0b011));
    assert!(tcp_flags_from(&out, GATEWAY).contains(&0b011), "SYN-ACK from the host's side");
    assert!(host.accept().is_ok(), "the host got the connection");
    // public only: a reset, and the host hears nothing
    let mut net = Network::new(0);
    net.set_public_only(true);
    let out = run(&mut net, vec![arp_request(), syn(GATEWAY, port)], |o| tcp_flags_from(o, GATEWAY).iter().any(|f| f & 0b100 != 0));
    let flags = tcp_flags_from(&out, GATEWAY);
    assert!(flags.iter().any(|f| f & 0b100 != 0), "RST, got {flags:?}");
    assert!(!flags.contains(&0b011), "no SYN-ACK");
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(host.accept().is_err(), "the host heard nothing");
}

#[test]
fn private_destinations_are_refused_when_public_only() {
    for dst in [[192, 168, 1, 1], [10, 1, 2, 3], [172, 20, 0, 1], [127, 0, 0, 1], [169, 254, 1, 1], [100, 64, 0, 1]] {
        let a = Ipv4Address::new(dst[0], dst[1], dst[2], dst[3]);
        let mut net = Network::new(0);
        net.set_public_only(true);
        let out = run(&mut net, vec![arp_request(), syn(a, 22)], |o| tcp_flags_from(o, a).iter().any(|f| f & 0b100 != 0));
        let flags = tcp_flags_from(&out, a);
        assert!(flags.iter().any(|f| f & 0b100 != 0), "{a}: RST, got {flags:?}");
    }
}

#[test]
fn udp_to_the_host_is_dropped_when_public_only() {
    let host = UdpSocket::bind("127.0.0.1:0").unwrap();
    host.set_nonblocking(true).unwrap();
    let port = host.local_addr().unwrap().port();
    let mut buf = [0u8; 64];
    // normally it arrives
    let mut net = Network::new(0);
    run(&mut net, vec![arp_request(), udp(GATEWAY, port, b"tri")], |_| false);
    let got = host.recv_from(&mut buf);
    assert!(matches!(got, Ok((3, _))), "the host got the datagram: {got:?}");
    assert_eq!(net.stats.udp_refused, 0);
    // public only: dropped and counted
    let mut net = Network::new(0);
    net.set_public_only(true);
    run(&mut net, vec![arp_request(), udp(GATEWAY, port, b"tri"), udp(Ipv4Address::new(192, 168, 1, 1), 53, b"x")], |_| false);
    assert!(host.recv_from(&mut buf).is_err(), "the host heard nothing");
    assert_eq!(net.stats.udp_refused, 2);
}

#[test]
fn which_addresses_are_public() {
    use setun_net::is_public;
    for a in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111", "::ffff:8.8.8.8"] {
        assert!(is_public(a.parse().unwrap()), "{a} is public");
    }
    for a in [
        "10.0.2.2", "192.168.1.2", "172.16.0.1", "172.31.255.255", "127.0.0.1", "0.0.0.0", "169.254.0.1", "100.64.0.1",
        "100.127.255.255", "224.0.0.1", "255.255.255.255", "240.0.0.1", "192.0.2.1", "198.18.0.1", "::1", "::", "fd00::2",
        "fe80::1", "ff02::1", "2001:db8::1", "::ffff:192.168.1.1", "64:ff9b::a00:1",
    ] {
        assert!(!is_public(a.parse().unwrap()), "{a} is not public");
    }
    assert!(is_public(Ipv4Addr::new(172, 32, 0, 1).into()), "172.32/16 is outside 172.16/12");
}
