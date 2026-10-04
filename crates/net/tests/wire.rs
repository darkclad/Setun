//! The gateway answers frames a guest sends through the card. The "guest"
//! here is the test itself, writing frames into memory and issuing `CMD`.

use setun_core::devices::{block, DEFAULT_MAC};
use setun_core::memory::frame_start;
use setun_core::{Io, Machine};
use setun_net::{Dir, Network, GATEWAY, GATEWAY_MAC, GUEST};
use smoltcp::phy::ChecksumCapabilities;
use smoltcp::wire::*;
use ternary::{Tryte, Word};

fn send(m: &mut Machine, frame: &[u8]) {
    let at = frame_start(5);
    for (i, &b) in frame.iter().enumerate() {
        m.mem.write_tryte(at + i as i64, Tryte::new(b as i64).unwrap()).unwrap();
    }
    let p = |r: i64| 27 * block::NIC + r;
    m.dev.port_out(p(2), Word::new(at).unwrap());
    m.dev.port_out(p(3), Word::new(frame.len() as i64).unwrap());
    m.dev.port_out(p(0), Word::new(1).unwrap());
    m.dev.service(&mut m.mem);
    assert_eq!(m.dev.port_in(p(11)).value(), 0, "send failed");
}

fn receive_all(m: &mut Machine) -> Vec<Vec<u8>> {
    let p = |r: i64| 27 * block::NIC + r;
    let mut out = Vec::new();
    loop {
        let len = m.dev.port_in(p(5)).value();
        if len == 0 {
            return out;
        }
        let at = frame_start(6);
        m.dev.port_out(p(4), Word::new(at).unwrap());
        m.dev.port_out(p(0), Word::new(-1).unwrap());
        m.dev.service(&mut m.mem);
        out.push((0..len).map(|i| m.mem.read_tryte(at + i).unwrap().value() as u8).collect());
    }
}

fn eth(dst: [u8; 6], ethertype: EthernetProtocol, payload_len: usize, fill: impl FnOnce(&mut [u8])) -> Vec<u8> {
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
    eth([0xff; 6], EthernetProtocol::Arp, arp.buffer_len(), |p| arp.emit(&mut ArpPacket::new_unchecked(p)))
}

fn ping_request(seq: u16, corrupt: bool) -> Vec<u8> {
    let data = b"ternary";
    let icmp = Icmpv4Repr::EchoRequest { ident: 7, seq_no: seq, data };
    let ip = Ipv4Repr {
        src_addr: GUEST,
        dst_addr: GATEWAY,
        next_header: IpProtocol::Icmp,
        payload_len: icmp.buffer_len(),
        hop_limit: 64,
    };
    let caps = ChecksumCapabilities::default();
    let mut f = eth(GATEWAY_MAC, EthernetProtocol::Ipv4, ip.buffer_len() + icmp.buffer_len(), |p| {
        let mut pkt = Ipv4Packet::new_unchecked(p);
        ip.emit(&mut pkt, &caps);
        icmp.emit(&mut Icmpv4Packet::new_unchecked(pkt.payload_mut()), &caps);
    });
    if corrupt {
        let n = f.len();
        f[n - 1] ^= 1; // the ICMP checksum no longer matches
    }
    f
}

#[test]
fn arp_and_ping_through_the_card() {
    let mut m = Machine::new(27, 0);
    let mut net = Network::attach(&mut m);
    send(&mut m, &arp_request());
    send(&mut m, &ping_request(1, false));
    net.poll(&mut m);
    let frames = receive_all(&mut m);
    let mut arp_reply = false;
    let mut echo_reply = false;
    for f in &frames {
        let e = EthernetFrame::new_checked(&f[..]).unwrap();
        assert_eq!(e.dst_addr(), EthernetAddress(DEFAULT_MAC));
        match e.ethertype() {
            EthernetProtocol::Arp => {
                let a = ArpRepr::parse(&ArpPacket::new_checked(e.payload()).unwrap()).unwrap();
                if let ArpRepr::EthernetIpv4 { operation: ArpOperation::Reply, source_hardware_addr, .. } = a {
                    assert_eq!(source_hardware_addr, EthernetAddress(GATEWAY_MAC));
                    arp_reply = true;
                }
            }
            EthernetProtocol::Ipv4 => {
                let ip = Ipv4Packet::new_checked(e.payload()).unwrap();
                let icmp = Icmpv4Packet::new_checked(ip.payload()).unwrap();
                if let Icmpv4Repr::EchoReply { seq_no: 1, data, .. } =
                    Icmpv4Repr::parse(&icmp, &ChecksumCapabilities::default()).unwrap()
                {
                    assert_eq!(data, b"ternary");
                    echo_reply = true;
                }
            }
            _ => {}
        }
    }
    assert!(arp_reply, "ARP reply from the gateway in {frames:?}");
    assert!(echo_reply, "echo reply from the gateway: {} frames {:?}", frames.len(), frames.iter().map(|f| f.len()).collect::<Vec<_>>());
    assert_eq!(net.stats.frames_from_guest, 2);
}

#[test]
fn a_corrupt_ping_gets_no_answer() {
    let mut m = Machine::new(27, 0);
    let mut net = Network::attach(&mut m);
    send(&mut m, &arp_request());
    net.poll(&mut m);
    receive_all(&mut m);
    send(&mut m, &ping_request(2, true));
    net.poll(&mut m);
    assert!(receive_all(&mut m).is_empty(), "a bad checksum is dropped");
}

#[test]
fn the_loss_filter_drops_frames() {
    let mut m = Machine::new(27, 0);
    let mut net = Network::attach(&mut m);
    net.set_loss(|d, _| d == Dir::FromGuest);
    send(&mut m, &arp_request());
    net.poll(&mut m);
    assert!(receive_all(&mut m).is_empty());
    assert_eq!(net.stats.dropped_from_guest, 1);
    net.clear_loss();
    send(&mut m, &arp_request());
    net.poll(&mut m);
    assert_eq!(receive_all(&mut m).len(), 1);
}
