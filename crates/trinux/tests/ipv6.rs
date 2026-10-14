//! IPv6 (docs/IPV6.md): autoconfiguration, Neighbor Discovery, ICMPv6,
//! and the conformance vectors. (Emulated time: 10 000 000 steps a second.)

mod common;
use common::*;
use setun_net::ip6::icmp6_frame;
use std::net::Ipv6Addr as Ipv6Address;

const S_MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
const P_MAC: [u8; 6] = [0x02, 0, 0, 0, 0, 0x77];
const GW_MAC: [u8; 6] = [0x52, 0x55, 0x0a, 0x00, 0x02, 0x02];

fn a6(s: &str) -> Ipv6Address {
    s.parse().unwrap()
}

fn solicited(a: Ipv6Address) -> Ipv6Address {
    let o = a.octets();
    Ipv6Address::new(0xff02, 0, 0, 0, 0, 1, 0xff00 | u16::from(o[13]), u16::from(o[14]) << 8 | u16::from(o[15]))
}

fn mcast_mac(a: Ipv6Address) -> [u8; 6] {
    let o = a.octets();
    [0x33, 0x33, o[12], o[13], o[14], o[15]]
}

/// A Neighbor Solicitation from the peer for `target`, with its link-layer address.
fn ns_from_peer(src: Ipv6Address, target: Ipv6Address) -> Vec<u8> {
    let mut m = vec![135, 0, 0, 0, 0, 0, 0, 0];
    m.extend_from_slice(&target.octets());
    m.extend_from_slice(&[1, 1]);
    m.extend_from_slice(&P_MAC);
    icmp6_frame(mcast_mac(solicited(target)), P_MAC, src, solicited(target), 255, &m)
}

/// A Neighbor Advertisement for `target` with flags, from `src` to `dst`.
fn na(src_mac: [u8; 6], src: Ipv6Address, dst: Ipv6Address, target: Ipv6Address, flags: u8, lla: [u8; 6]) -> Vec<u8> {
    let mut m = vec![136, 0, 0, 0, flags, 0, 0, 0];
    m.extend_from_slice(&target.octets());
    m.extend_from_slice(&[2, 1]);
    m.extend_from_slice(&lla);
    let dmac = if dst.octets()[0] == 0xff { mcast_mac(dst) } else { S_MAC };
    icmp6_frame(dmac, src_mac, src, dst, 255, &m)
}

/// A Router Advertisement's message from the gateway: router lifetime
/// `life`, and one prefix /64 with flags, valid and preferred lifetimes.
fn ra_msg(life: u16, prefix: Ipv6Address, flags: u8, valid: u32, pref: u32) -> Vec<u8> {
    let mut m = vec![134, 0, 0, 0, 64, 0];
    m.extend_from_slice(&life.to_be_bytes());
    m.extend_from_slice(&[0; 8]);
    m.extend_from_slice(&[1, 1]);
    m.extend_from_slice(&GW_MAC);
    m.extend_from_slice(&[3, 4, 64, flags]);
    m.extend_from_slice(&valid.to_be_bytes());
    m.extend_from_slice(&pref.to_be_bytes());
    m.extend_from_slice(&[0; 4]);
    m.extend_from_slice(&prefix.octets());
    m
}

fn ra_frame(m: &[u8]) -> Vec<u8> {
    icmp6_frame(mcast_mac(a6("ff02::1")), GW_MAC, setun_net::GATEWAY6_LL, a6("ff02::1"), 255, m)
}

fn ra(life: u16, prefix: Ipv6Address, flags: u8, valid: u32, pref: u32) -> Vec<u8> {
    ra_frame(&ra_msg(life, prefix, flags, valid, pref))
}

fn s_ll() -> Ipv6Address {
    a6("fe80::5054:ff:fe12:3456")
}

fn s_gl() -> Ipv6Address {
    setun_net::GUEST6
}

/// Booted, with both addresses of eth0 configured.
fn boot6() -> Session {
    let mut s = Session::boot();
    let mut tries = 0;
    while !s.out.contains("eth0: inet6 fd00::5054:ff:fe12:3456 scope global\n") {
        tries += 1;
        assert!(tries < 100, "no global address:\n{}", s.out);
        s.idle(10_000_000);
    }
    s
}

fn v6_frames(fs: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
    fs.into_iter().filter(|f| f.len() >= 14 && f[12] == 0x86 && f[13] == 0xdd).collect()
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn hexs(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[test]
fn autoconfigures_and_pings() {
    let mut s = boot6();
    has(&s.out, &["eth0: inet6 fe80::5054:ff:fe12:3456 scope link\n", "eth0: inet6 fd00::5054:ff:fe12:3456 scope global\n"]);
    let o = s.cmd("cat /proc/net/if_inet6");
    assert_eq!(
        o,
        "00000000000000000000000000000001 01 80 10 80       lo\n\
         fe80000000000000505400fffe123456 02 40 20 80     eth0\n\
         fd00000000000000505400fffe123456 02 40 00 00     eth0\n"
    );
    let o = s.cmd("cat /proc/net/neigh6");
    has(&o, &["fe80::5055:aff:fe00:202                  52:55:0a:00:02:02 STALE      yes default\n"]);
    let o = s.cmd("ping6 -c 2 fd00::2");
    has(&o, &["PING fd00::2(fd00::2) 56 data bytes\n", "64 bytes from fd00::2: icmp_seq=2 time=", "2 packets transmitted, 2 received, 0% packet loss"]);
    // smoltcp drops the reply that made it solicit our link-local address, so the first may go
    let o = s.cmd("ping6 -c 2 fe80::5055:aff:fe00:202");
    has(&o, &["64 bytes from fe80::5055:aff:fe00:202: icmp_seq=2 time="]);
    let o = s.cmd("ping6 -c 1 -s 1400 fd00::2");
    has(&o, &["1408 bytes from fd00::2: icmp_seq=1", "1 received"]);
    for me in ["::1", "fd00::5054:ff:fe12:3456", "fe80::5054:ff:fe12:3456"] {
        let o = s.cmd(&format!("ping6 -c 1 {me}"));
        has(&o, &[&format!("64 bytes from {me}: icmp_seq=1"), "1 received"]);
    }
    let o = s.cmd("cat /proc/net/stat6");
    has(&o, &["rdnss fd00::3\n", "mtu 1500\n", "dad_failed 0\n", "icmp6_bad 0\n", "nd_bad 0\n"]);
    assert!(!o.contains("nd_ra_in 0\n"), "{o}");
    let o = s.cmd("ls /proc/net");
    has(&o, &["if_inet6", "neigh6", "stat6"]);
}

#[test]
fn ping6_failures() {
    let mut s = boot6();
    let o = s.cmd("ping6 nonsense; echo rc=$?");
    has(&o, &["ping6: nonsense: Name or service not known\n", "rc=2"]);
    let o = s.cmd("ping6 -c 0 ::1; echo rc=$?");
    has(&o, &["usage: ping6", "rc=2"]);
    let o = s.cmd("ping6 -c 1 fd00::99; echo rc=$?");
    has(&o, &["1 packets transmitted, 0 received, 100% packet loss", "rc=1"]);
    has(&s.cmd("cat /proc/net/neigh6"), &["fd00::99                                 00:00:00:00:00:00 INCOMPLETE no\n"]);
    s.idle(50_000_000); // three solicitations, a second apart
    assert!(!s.cmd("cat /proc/net/neigh6").contains("fd00::99"));
    has(&s.cmd("cat /proc/net/stat6"), &["nd_resolve_failed 1\n"]);
    // a router lifetime of 0 takes the default router away
    s.net.inject(&mut s.m, &ra(0, a6("fd00::"), 0xc0, 86400, 14400));
    s.idle(10_000_000);
    assert!(!s.cmd("cat /proc/net/neigh6").contains("default"));
    let o = s.cmd("ping6 -c 1 2001:db8::1; echo rc=$?");
    has(&o, &["ping6: sendto: Network is unreachable", "rc=2"]);
    let o = s.cmd("ping6 -c 1 fd00::2"); // still on the link
    has(&o, &["1 received"]);
}

/// The text lines of the vectors, checked by /bin/ip6vec inside Trinux.
#[test]
fn text_vectors() {
    let mut s = Session::boot();
    let o = s.cmd("ip6vec");
    assert!(o.contains("ip6vec: 37 checked, 0 failed"), "{o}");
}

/// The exchange vectors of docs/IPV6.md §7: each frame given to the
/// station, and exactly the frames it must send back.
#[test]
fn the_exchange_vectors() {
    let text = std::fs::read_to_string(trinux::user_dir().join("share/ipv6vectors.txt")).unwrap();
    let mut s = boot6();
    s.net.set_capture(true);
    let (p_gl, p_ll) = (a6("fd00::77"), a6("fe80::77"));
    // the peer becomes a neighbour (each solicitation is answered)
    for (p, me) in [(p_gl, s_gl()), (p_ll, s_ll())] {
        s.net.inject(&mut s.m, &ns_from_peer(p, me));
        s.idle(10_000_000);
    }
    let answers = v6_frames(s.net.captured());
    assert_eq!(answers.len(), 2, "the solicitations were answered");
    let mut n = 0;
    for line in text.lines().filter_map(|l| l.strip_prefix("ex ")) {
        let (name, rest) = line.split_once(" : ").unwrap();
        let (input, output) = rest.split_once(" =>").unwrap();
        let want: Vec<Vec<u8>> = output.split(" ; ").map(str::trim).filter(|f| !f.is_empty()).map(hex).collect();
        // keep the peer REACHABLE, so a reply never waits on Neighbor Discovery
        for (p, me) in [(p_gl, s_gl()), (p_ll, s_ll())] {
            s.net.inject(&mut s.m, &na(P_MAC, p, me, p, 0x60, P_MAC));
        }
        s.idle(2_000_000);
        assert!(v6_frames(s.net.captured()).is_empty(), "priming before {name}");
        s.net.inject(&mut s.m, &hex(input));
        s.idle(20_000_000); // 200 ms: the error limit refills
        let got = v6_frames(s.net.captured());
        assert_eq!(got.len(), want.len(), "vector {name}: {} frames, want {}", got.len(), want.len());
        for (k, (g, w)) in got.iter().zip(&want).enumerate() {
            assert_eq!(hexs(g), hexs(w), "vector {name}, frame {k}");
        }
        n += 1;
    }
    assert!(n >= 60, "{n} vectors");
    has(&s.cmd("cat /proc/net/stat6"), &["ip6_frag_dropped 1\n"]);
}

/// Duplicate address detection fails both ways it can (§3): someone
/// answers for the tentative address, or solicits it at the same time.
#[test]
fn duplicate_addresses_are_never_used() {
    let mut s = boot6();
    s.net.set_capture(true);
    // a new prefix: its address is tentative while DAD runs, and a defender answers
    s.net.inject(&mut s.m, &ra(1800, a6("fd01::"), 0xc0, 86400, 14400));
    s.idle(1_000_000);
    let probe = v6_frames(s.net.captured());
    let dad = probe.iter().find(|f| f[54] == 135).expect("a DAD solicitation");
    assert_eq!(&dad[22..38], &[0u8; 16], "DAD comes from ::");
    assert_eq!(dad[21], 255, "hop limit 255");
    let tent = a6("fd01::5054:ff:fe12:3456");
    assert_eq!(&dad[62..78], &tent.octets(), "for the new address");
    // while tentative it answers nothing but Neighbor Discovery (RFC 4862 §5.4)
    let mut e = vec![128, 0, 0, 0, 0, 1, 0, 1];
    e.extend_from_slice(b"to a tentative address");
    s.net.inject(&mut s.m, &icmp6_frame(S_MAC, P_MAC, a6("fd00::77"), tent, 64, &e));
    s.idle(1_000_000);
    assert!(v6_frames(s.net.captured()).is_empty(), "no echo reply, no solicitation");
    assert!(s.cmd("cat /proc/net/if_inet6").contains("fd01000000000000505400fffe123456 02 40 00 40     eth0\n"), "tentative");
    s.net.inject(&mut s.m, &na(P_MAC, tent, a6("ff02::1"), tent, 0x20, P_MAC));
    s.idle(20_000_000);
    has(&s.out, &["eth0: duplicate address fd01::5054:ff:fe12:3456\n"]);
    has(&s.cmd("cat /proc/net/if_inet6"), &["fd01000000000000505400fffe123456 02 40 00 48     eth0\n"]);
    has(&s.cmd("cat /proc/net/stat6"), &["dad_failed 1\n"]);
    // it is never answered for
    s.net.captured();
    s.net.inject(&mut s.m, &ns_from_peer(a6("fd00::77"), tent));
    s.idle(10_000_000);
    assert!(v6_frames(s.net.captured()).iter().all(|f| f[54] != 136), "no advertisement for a duplicate");
    // a simultaneous DAD (a solicitation from :: for the same tentative address)
    s.net.inject(&mut s.m, &ra(1800, a6("fd02::"), 0xc0, 86400, 14400));
    s.idle(1_000_000);
    let t2 = a6("fd02::5054:ff:fe12:3456");
    let mut m = vec![135, 0, 0, 0, 0, 0, 0, 0];
    m.extend_from_slice(&t2.octets());
    s.net.inject(&mut s.m, &icmp6_frame(mcast_mac(solicited(t2)), P_MAC, Ipv6Address::UNSPECIFIED, solicited(t2), 255, &m));
    s.idle(20_000_000);
    has(&s.out, &["eth0: duplicate address fd02::5054:ff:fe12:3456\n"]);
    has(&s.cmd("cat /proc/net/stat6"), &["dad_failed 2\n"]);
    // a duplicate stays one: the prefix advertised again does not revive it
    s.net.inject(&mut s.m, &ra(1800, a6("fd01::"), 0xc0, 86400, 14400));
    s.idle(20_000_000);
    has(&s.cmd("cat /proc/net/if_inet6"), &["fd01000000000000505400fffe123456 02 40 00 48     eth0\n"]);
    assert!(!s.out.contains("inet6 fd01::5054:ff:fe12:3456 scope global"), "never preferred");
}

/// Prefix lifetimes (RFC 4862 §5.5.3): preferred, then deprecated, then
/// gone; a prefix that is not autonomous, not /64, or link-local makes no
/// address; and the slots run out after three.
#[test]
fn prefix_lifetimes_and_refusals() {
    let mut s = boot6();
    s.net.inject(&mut s.m, &ra(1800, a6("fd04::"), 0xc0, 40, 20));
    s.idle(15_000_000);
    has(&s.cmd("cat /proc/net/if_inet6"), &["fd04000000000000505400fffe123456 02 40 00 00     eth0\n"]);
    s.idle(200_000_000); // past the preferred lifetime
    has(&s.cmd("cat /proc/net/if_inet6"), &["fd04000000000000505400fffe123456 02 40 00 20     eth0\n"]);
    s.idle(200_000_000); // past the valid lifetime
    assert!(!s.cmd("cat /proc/net/if_inet6").contains("fd04"));
    for (pfx, flags, valid, pref) in [("fd05::", 0x80, 600, 300), ("fe80::", 0xc0, 600, 300), ("fd06::", 0xc0, 300, 600), ("fd07::", 0xc0, 0, 0)] {
        s.net.inject(&mut s.m, &ra(1800, a6(pfx), flags, valid, pref));
    }
    let mut short = ra_msg(1800, a6("fd08::"), 0xc0, 600, 300);
    short[16 + 8 + 2] = 48; // the prefix option's length field: /48
    s.net.inject(&mut s.m, &ra_frame(&short));
    s.idle(20_000_000);
    let o = s.cmd("cat /proc/net/if_inet6");
    for p in ["fd05", "fd06", "fd07", "fd08"] {
        assert!(!o.contains(&format!("{p}0000")), "{p} makes no address:\n{o}");
    }
    assert_eq!(o.matches("eth0").count(), 2, "{o}");
    // three global slots at most
    for p in ["fd09::", "fd0a::", "fd0b::"] {
        s.net.inject(&mut s.m, &ra(1800, a6(p), 0xc0, 600, 300));
    }
    s.idle(20_000_000);
    let o = s.cmd("cat /proc/net/if_inet6");
    assert!(o.contains("fd09") && o.contains("fd0a") && !o.contains("fd0b"), "{o}");
}

/// Neighbour unreachability detection (RFC 4861 §7.3): a neighbour that
/// stops answering goes DELAY, then PROBE with unicast solicitations,
/// then away.
#[test]
fn unreachable_neighbours_are_probed_then_dropped() {
    let mut s = boot6();
    s.net.set_capture(true);
    let p = a6("fd00::77");
    s.net.inject(&mut s.m, &ns_from_peer(p, s_gl()));
    s.idle(10_000_000);
    // answering it used the new STALE entry, which makes it DELAY (RFC 4861 §7.3.3)
    has(&s.cmd("cat /proc/net/neigh6"), &["fd00::77                                 02:00:00:00:00:77 DELAY      no\n"]);
    s.net.captured();
    s.send("ping6 -c 1 -W 1 fd00::77\n");
    s.idle(20_000_000);
    let sent = v6_frames(s.net.captured());
    assert!(sent.iter().any(|f| f[54] == 128 && f[0..6] == P_MAC), "the echo went straight to the peer");
    s.wait_prompt(300_000_000);
    has(&s.cmd("cat /proc/net/neigh6"), &["fd00::77                                 02:00:00:00:00:77 DELAY "]);
    s.idle(100_000_000); // 5 s of DELAY, then 3 probes a second apart
    let probes: Vec<_> = v6_frames(s.net.captured()).into_iter().filter(|f| f[54] == 135 && f[62..78] == p.octets()).collect();
    assert_eq!(probes.len(), 3, "three unicast probes");
    assert!(probes.iter().all(|f| f[0..6] == P_MAC), "unicast to the cached address");
    assert!(!s.cmd("cat /proc/net/neigh6").contains("fd00::77"));
    // a solicited advertisement makes it REACHABLE again
    s.net.inject(&mut s.m, &ns_from_peer(p, s_gl()));
    s.idle(10_000_000);
    s.net.inject(&mut s.m, &na(P_MAC, p, s_gl(), p, 0x60, P_MAC));
    s.idle(10_000_000);
    has(&s.cmd("cat /proc/net/neigh6"), &["fd00::77                                 02:00:00:00:00:77 REACHABLE"]);
}

/// ICMPv6 errors (§2): a burst of 9, then one per 100 ms.
#[test]
fn icmp6_errors_are_rate_limited() {
    let mut s = boot6();
    s.net.set_capture(true);
    let p = a6("fd00::77");
    s.net.inject(&mut s.m, &ns_from_peer(p, s_gl()));
    s.idle(10_000_000);
    s.net.captured();
    // packets with an unknown next header, each worth a Parameter Problem
    let mut bad = vec![0x52, 0x54, 0x00, 0x12, 0x34, 0x56, 2, 0, 0, 0, 0, 0x77, 0x86, 0xdd, 0x60, 0, 0, 0, 0, 4, 253, 64];
    bad.extend_from_slice(&p.octets());
    bad.extend_from_slice(&s_gl().octets());
    bad.extend_from_slice(b"oops");
    for _ in 0..20 {
        s.net.inject(&mut s.m, &bad);
    }
    s.idle(200_000);
    let errs = v6_frames(s.net.captured()).into_iter().filter(|f| f[54] == 4).count();
    assert_eq!(errs, 9, "a burst of nine");
    has(&s.cmd("cat /proc/net/stat6"), &["icmp6_rate_limited 11\n", "icmp6_errors_sent 9\n"]);
    s.idle(10_000_000);
    s.net.captured();
    s.net.inject(&mut s.m, &bad);
    s.idle(200_000);
    assert_eq!(v6_frames(s.net.captured()).into_iter().filter(|f| f[54] == 4).count(), 1, "refilled");
}
