//! TNET on the host (docs/TNET.md): the conformance vectors against
//! setun-net's TCRC-18 and TL frames, and the wire's faults.

use setun_net::tnet::{check_trytes, flip_trit, frame, parse, tcrc18};

fn nums(s: &str) -> Vec<i64> {
    s.split_whitespace().map(|t| t.parse().unwrap()).collect()
}

fn trytes(s: &str) -> Vec<i16> {
    nums(s).into_iter().map(|v| v as i16).collect()
}

#[test]
fn the_conformance_vectors() {
    let text = include_str!("tnet_vectors.txt");
    let (mut crcs, mut frames, mut bad) = (0, 0, 0);
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        if let Some(rest) = line.strip_prefix("crc ") {
            let (m, c) = rest.rsplit_once("= ").unwrap();
            assert_eq!(tcrc18(&trytes(m)), c.trim().parse::<i64>().unwrap(), "{line}");
            crcs += 1;
        } else if let Some(rest) = line.strip_prefix("frame ") {
            let (head, f) = rest.split_once(" = ").unwrap();
            let (dsy, pl) = head.split_once(" : ").map(|(a, b)| (a, b.trim())).unwrap_or((head.trim_end_matches(" :"), ""));
            let h = nums(dsy);
            let (pl, f) = (trytes(pl), trytes(f));
            assert_eq!(frame(h[0], h[1], h[2] as i16, &pl), f, "{line}");
            assert_eq!(parse(&f), Some((h[0], h[1], h[2] as i16, pl)), "{line}");
            frames += 1;
        } else if let Some(rest) = line.strip_prefix("bad ") {
            assert_eq!(parse(&trytes(rest)), None, "{line}");
            bad += 1;
        } else {
            panic!("unknown vector {line}");
        }
    }
    assert!(crcs > 20 && frames >= 4 && bad > 20, "{crcs} {frames} {bad}");
}

#[test]
fn every_single_trit_error_is_caught() {
    // beyond the vectors' sample: every trit of a frame, both errors
    let f = frame(5, -7, 2, &[1, -9841, 9841, 0, 42]);
    for k in 0..f.len() * 9 {
        for e in [1, -1] {
            let mut g = f.clone();
            flip_trit(&mut g, k, e);
            assert_ne!(g, f);
            assert_eq!(parse(&g), None, "trit {k} by {e}");
        }
    }
}

#[test]
fn every_double_error_in_a_short_frame_is_caught() {
    let f = frame(1, 2, 3, &[100]);
    let n = f.len() * 9;
    for i in 0..n {
        for j in i + 1..n {
            for (e1, e2) in [(1, 1), (1, -1), (-1, 1), (-1, -1)] {
                let mut g = f.clone();
                flip_trit(&mut g, i, e1);
                flip_trit(&mut g, j, e2);
                assert_eq!(parse(&g), None, "trits {i}, {j}");
            }
        }
    }
}

#[test]
fn every_burst_up_to_18_trits_is_caught() {
    let f = frame(9, 10, 1, &[3, 1, 4, 1, 5, 9, 2, 6]);
    let n = f.len() * 9;
    let mut seed = 99u64;
    for start in 0..n - 18 {
        for len in [2, 9, 17, 18] {
            // a burst: its first and last trits wrong, the ones between at random
            let mut g = f.clone();
            for k in start..start + len {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let e = if k == start || k == start + len - 1 { 1 + (seed >> 40) as i64 % 2 } else { (seed >> 40) as i64 % 3 };
                if e != 0 {
                    flip_trit(&mut g, k, if e == 2 { -1 } else { e });
                }
            }
            assert_eq!(parse(&g), None, "burst at {start}, {len}");
        }
    }
}

#[test]
fn checks_are_two_trytes_and_frames_refuse_short_ones() {
    assert_eq!(check_trytes(&[]).len(), 2);
    assert_eq!(parse(&[0; 8]), None, "shorter than a header and a check");
    let f = frame(0, 4, 2, &[1]);
    let mut g = vec![0];
    g.extend(&f);
    assert_eq!(parse(&g), None, "a zero tryte in front");
}

#[test]
fn udp_link_between_two_machines() {
    use setun_core::devices::block;
    use setun_core::{Io, Machine};
    use setun_net::tnet::UdpLink;
    use ternary::{Tryte, Word};
    let mut a = Machine::new(27, 0);
    let mut b = Machine::new(27, 0);
    a.dev.tlink.link = true;
    b.dev.tlink.link = true;
    // b listens on a free port; a connects to it
    let probe = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let mut lb = UdpLink::open(&format!("listen:{port}")).unwrap();
    let mut la = UdpLink::open(&format!("connect:127.0.0.1:{port}")).unwrap();
    let send = |m: &mut Machine, f: &[i16]| {
        let at = 19683 * 3;
        for (i, &t) in f.iter().enumerate() {
            m.mem.write_tryte(at + i as i64, Tryte::new(t as i64).unwrap()).unwrap();
        }
        let p = |r: i64| 27 * block::TLINK + r;
        m.dev.port_out(p(2), Word::new(at).unwrap());
        m.dev.port_out(p(3), Word::new(f.len() as i64).unwrap());
        m.dev.port_out(p(0), Word::new(1).unwrap());
        m.dev.service(&mut m.mem);
    };
    let fa = frame(2, 1, 2, &[9841, -9841, 0, 300]);
    send(&mut a, &fa);
    la.poll(&mut a);
    let mut got = Vec::new();
    for _ in 0..200 {
        lb.poll(&mut b);
        if b.dev.tlink.read(6) > 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(b.dev.tlink.read(6), 1, "b got a's frame");
    // and back: b now knows its peer
    let fb = frame(1, 2, 3, &[-1]);
    send(&mut b, &fb);
    lb.poll(&mut b);
    for _ in 0..200 {
        la.poll(&mut a);
        if a.dev.tlink.read(6) > 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(a.dev.tlink.read(6), 1, "a got b's answer");
    got.push(a.dev.tlink.read(5));
    assert_eq!(got, vec![fb.len() as i64]);
    assert!(UdpLink::open("sideways:1").is_err());
    assert!(UdpLink::open("listen:x").is_err());
}

#[test]
fn addresses_in_text() {
    use setun_net::tnet::{addr_str, parse_addr};
    assert_eq!(parse_addr("0.0.1"), Some(1));
    assert_eq!(parse_addr("4.-3.7"), Some(4 * 387_420_489 - 3 * 19683 + 7));
    assert_eq!(addr_str(4 * 387_420_489 - 3 * 19683 + 7), "4.-3.7");
    for bad in ["1.2", "1.2.3.4", "9842.0.0", "a.b.c", "", "1..2"] {
        assert_eq!(parse_addr(bad), None, "{bad}");
    }
}
