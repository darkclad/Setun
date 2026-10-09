//! TNET in Trinux (docs/TNET.md): the TLINK driver and the TL link layer,
//! against frames made and checked on the host by setun-net (itself
//! checked against the conformance vectors).

use setun_net::tnet::{flip_trit, frame, parse, TL_ECHO, TL_ECHO_REPLY};

const ME: i64 = 1; // 0.0.1
const PEER: i64 = 2; // 0.0.2

fn boot(addr: i64) -> setun_core::Machine {
    let disk = trinux::build_disk(&trinux::kernel_dir()).unwrap_or_else(|e| panic!("{e}")).trytes;
    let mut m = trinux::boot_trytes(&disk, 729, 0, "");
    m.dev.tlink.link = true;
    m.dev.tlink.addr = addr;
    m
}

/// Run until the output ends with `until` (or the step budget runs out).
fn run_until(m: &mut setun_core::Machine, out: &mut String, until: &str, max: u64) {
    let mut steps = 0;
    while steps < max && !out.ends_with(until) {
        m.run(1_000_000);
        steps += 1_000_000;
        out.push_str(&m.dev.serial.take_output());
    }
}

fn login(m: &mut setun_core::Machine) -> String {
    let mut out = String::new();
    run_until(m, &mut out, "login: ", 400_000_000);
    m.dev.serial_input("root\n");
    run_until(m, &mut out, "Password: ", 50_000_000);
    m.dev.serial_input("root\n");
    run_until(m, &mut out, "$ ", 200_000_000);
    out
}

fn shell(m: &mut setun_core::Machine, line: &str) -> String {
    let mut out = String::new();
    m.dev.serial_input(&format!("{line}\n"));
    run_until(m, &mut out, "$ ", 300_000_000);
    out
}

/// Give the machine a frame and let it answer; the frames it sent.
fn exchange(m: &mut setun_core::Machine, f: &[i16]) -> Vec<Vec<i16>> {
    m.dev.tlink_receive(f);
    m.run(5_000_000);
    m.dev.tlink_take_tx()
}

fn counters(m: &mut setun_core::Machine) -> String {
    shell(m, "cat /proc/net/tlink")
}

#[test]
fn the_link_comes_up_with_its_station_address() {
    let mut m = boot(3i64.pow(18) * 4 - 3 * 19683 + 7); // trytes 4, -3, 7
    let out = login(&mut m);
    assert!(out.contains("tl0: TLINK at port 324, IRQ 9, station 4.-3.7, link up"), "{out}");
    let c = counters(&mut m);
    assert!(c.contains("station 4.-3.7 up\nrx_frames 0\n"), "{c}");
    let ls = shell(&mut m, "ls /proc/net");
    assert!(ls.contains("arp\ndev\nstat\ntcp\ntlink\nudp\n"), "{ls}");
}

#[test]
fn no_link_no_address() {
    let disk = trinux::build_disk(&trinux::kernel_dir()).unwrap().trytes;
    let mut m = trinux::boot_trytes(&disk, 729, 0, "");
    let out = login(&mut m);
    assert!(out.contains("tl0: TLINK at port 324, IRQ 9, station 0.0.0, no link"), "{out}");
}

#[test]
fn an_address_without_a_link_is_down() {
    let disk = trinux::build_disk(&trinux::kernel_dir()).unwrap().trytes;
    let mut m = trinux::boot_trytes(&disk, 729, 0, "");
    m.dev.tlink.addr = 5;
    let out = login(&mut m);
    assert!(out.contains("station 0.0.5, no link"), "{out}");
    let c = counters(&mut m);
    assert!(c.contains("station 0.0.5 down
"), "{c}");
}

#[test]
fn echo_comes_back_and_bad_frames_are_dropped_and_counted() {
    let mut m = boot(ME);
    login(&mut m);
    // an echo request with trytes over the whole range: the reply carries them back
    let payload: Vec<i16> = vec![0, 1, -1, 9841, -9841, 255, 256, 1000, -5000];
    let reply = exchange(&mut m, &frame(ME, PEER, TL_ECHO, &payload));
    if reply.len() != 1 {
        panic!("no reply: {}", counters(&mut m));
    }
    assert_eq!(parse(&reply[0]), Some((PEER, ME, TL_ECHO_REPLY, payload.clone())), "a reply with a good check");
    // to the broadcast address: answered too
    let reply = exchange(&mut m, &frame(0, PEER, TL_ECHO, &[7]));
    assert_eq!(parse(&reply[0]), Some((PEER, ME, TL_ECHO_REPLY, vec![7])));
    // a long one: the largest payload
    let big: Vec<i16> = (0..2178).map(|i| ((i * 37) % 19683 - 9841) as i16).collect();
    let reply = exchange(&mut m, &frame(ME, PEER, TL_ECHO, &big));
    assert_eq!(parse(&reply[0]), Some((PEER, ME, TL_ECHO_REPLY, big)));
    // every one of these is dropped without a reply
    let good = frame(ME, PEER, TL_ECHO, &[1, 2, 3]);
    let mut corrupt = Vec::new();
    for k in [0, 26, 40, 63, 80, 99] {
        for e in [1, -1] {
            let mut g = good.clone();
            flip_trit(&mut g, k, e);
            corrupt.push(g);
        }
    }
    for g in &corrupt {
        assert!(exchange(&mut m, g).is_empty(), "answered a corrupted frame");
    }
    assert!(exchange(&mut m, &good[..8]).is_empty(), "short");
    assert!(exchange(&mut m, &frame(PEER + 1, PEER, TL_ECHO, &[1])).is_empty(), "for another station");
    assert!(exchange(&mut m, &frame(ME, PEER, 9, &[1])).is_empty(), "unknown type");
    assert!(exchange(&mut m, &frame(ME, PEER, TL_ECHO_REPLY, &[1])).is_empty(), "a reply is not answered");
    assert!(exchange(&mut m, &frame(ME, PEER, 1, &[1, 2, 3])).is_empty(), "a TNP packet shorter than its header");
    let c = counters(&mut m);
    assert!(
        c.contains(&format!(
            "rx_frames {}\nrx_trytes",
            3 + corrupt.len() + 5
        )),
        "{c}"
    );
    for want in [
        format!("bad_check {}\n", corrupt.len()),
        "short 1\n".into(),
        "not_ours 1\n".into(),
        "bad_type 1\n".into(),
        "tx_frames 3\n".into(),
        "echo_replies 1\n".into(),
        "tnp_dropped 1\n".into(),
    ] {
        assert!(c.contains(&want), "missing {want:?} in {c}");
    }
}

#[test]
fn two_machines_on_one_wire() {
    // both boot; frames between them cross the wire with its faults
    use setun_net::tnet::Wire;
    let mut a = boot(ME);
    let mut b = boot(PEER);
    Wire::attach(&mut a, &mut b, ME, PEER);
    login(&mut a);
    login(&mut b);
    let mut wire = Wire::default();
    // the host plays a third station that only listens: inject an echo to B
    // "from A" on the wire itself, then let B's reply cross to A
    b.dev.tlink_receive(&frame(PEER, ME, TL_ECHO, &[5, -5]));
    for _ in 0..20 {
        a.run(1_000_000);
        b.run(1_000_000);
        wire.pump(&mut a, &mut b);
    }
    assert_eq!(wire.log.len(), 1, "{:?}", wire.log);
    assert!(!wire.log[0].0, "B sent it");
    assert_eq!(parse(&wire.log[0].1), Some((ME, PEER, TL_ECHO_REPLY, vec![5, -5])));
    let c = counters(&mut a);
    assert!(c.contains("echo_replies 1\n"), "{c}");
    // a flipped trit on the way: A drops it and counts it
    wire.ba.flips.push((1, 30, 1));
    b.dev.tlink_receive(&frame(PEER, ME, TL_ECHO, &[6]));
    for _ in 0..20 {
        a.run(1_000_000);
        b.run(1_000_000);
        wire.pump(&mut a, &mut b);
    }
    let c = counters(&mut a);
    assert!(c.contains("echo_replies 1\n") && c.contains("bad_check 1\n"), "{c}");
}

// ---- C2: TNP, TCMP and TSP between two machines -------------------------------------------

use setun_net::tnet::{tnp_packet, Wire, TL_TNP};

/// A TNP packet to me from the peer, in a TL frame.
fn tnp_frame(proto: i16, payload: &[i16]) -> Vec<i16> {
    frame(ME, PEER, TL_TNP, &tnp_packet(ME, PEER, proto, payload))
}

/// The TNP packets in the frames a machine sent: (dst, src, proto, payload).
fn tnp_of(frames: &[Vec<i16>]) -> Vec<(i64, i64, i16, Vec<i16>)> {
    frames
        .iter()
        .filter_map(|f| parse(f))
        .filter(|p| p.2 == TL_TNP)
        .map(|(_, _, _, p)| {
            use setun_net::tnet::trytes_word;
            (trytes_word(&p[0..3]), trytes_word(&p[3..6]), p[6], p[9..9 + p[8] as usize].to_vec())
        })
        .collect()
}

#[test]
fn tcmp_echo_and_tnp_refusals() {
    let mut m = boot(ME);
    login(&mut m);
    let data: Vec<i16> = vec![1, 7, 2, 9841, -9841, 0, 4444];
    let out = exchange(&mut m, &tnp_frame(3, &data));
    let mut want = data.clone();
    want[0] = 2;
    assert_eq!(tnp_of(&out), vec![(PEER, ME, 3, want)], "an echo reply with the same data");
    // dropped: hop limit 0, for another host, a length past the frame, too short
    let mut p = tnp_packet(ME, PEER, 3, &[1, 0, 0]);
    p[7] = 0;
    assert!(exchange(&mut m, &frame(ME, PEER, TL_TNP, &p)).is_empty(), "hop limit 0");
    assert!(exchange(&mut m, &frame(ME, PEER, TL_TNP, &tnp_packet(9, PEER, 3, &[1, 0, 0]))).is_empty(), "not ours");
    let mut p = tnp_packet(ME, PEER, 3, &[1, 0, 0]);
    p[8] = 50;
    assert!(exchange(&mut m, &frame(ME, PEER, TL_TNP, &p)).is_empty(), "length past the frame");
    assert!(exchange(&mut m, &frame(ME, PEER, TL_TNP, &[0, 0, 1])).is_empty(), "shorter than a header");
    // an unknown protocol is answered "unreachable" with the packet's start
    let out = exchange(&mut m, &tnp_frame(9, &[1, 2, 3]));
    let got = tnp_of(&out);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!((got[0].0, got[0].2, got[0].3[0], got[0].3[1]), (PEER, 3, 3, 1), "{got:?}");
    assert_eq!(&got[0].3[3..], &tnp_packet(ME, PEER, 9, &[1, 2, 3])[..], "it quotes the packet");
    // a TSP opening to a closed port is answered with a reset
    let syn: Vec<i16> = vec![200, 77, 0, 0, 5, 0, 0, 0, 1, 100];
    let out = exchange(&mut m, &tnp_frame(1, &syn));
    let got = tnp_of(&out);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!((got[0].2, got[0].3[0], got[0].3[1]), (1, 77, 200), "ports swapped");
    assert_eq!(got[0].3[8], 3 + 9, "RST + ACK: {:?}", got[0].3);
    assert_eq!(&got[0].3[5..8], &[0, 0, 6], "acknowledges the opening");
    // a bad control tryte is dropped silently
    let bad: Vec<i16> = vec![200, 77, 0, 0, 5, 0, 0, 0, -3, 100];
    assert!(exchange(&mut m, &tnp_frame(1, &bad)).is_empty(), "ACK of -1");
}

/// Two booted machines on one wire.
struct Pair {
    a: setun_core::Machine,
    b: setun_core::Machine,
    wire: Wire,
}

impl Pair {
    fn new() -> Pair {
        let mut a = boot(ME);
        let mut b = boot(PEER);
        Wire::attach(&mut a, &mut b, ME, PEER);
        login(&mut a);
        login(&mut b);
        Pair { a, b, wire: Wire::default() }
    }

    /// Type a line on one machine and run both until that one's output
    /// ends with `until` (its prompt by default).
    fn on(&mut self, on_a: bool, line: &str, until: &str, max: u64) -> String {
        let mut out = String::new();
        if on_a { self.a.dev.serial_input(&format!("{line}\n")); } else { self.b.dev.serial_input(&format!("{line}\n")); }
        let mut steps = 0;
        while steps < max && !out.ends_with(until) {
            self.a.run(500_000);
            self.b.run(500_000);
            self.wire.pump(&mut self.a, &mut self.b);
            steps += 500_000;
            let (mine, other) = if on_a { (&mut self.a, &mut self.b) } else { (&mut self.b, &mut self.a) };
            out.push_str(&mine.dev.serial.take_output());
            other.dev.serial.take_output();
        }
        out
    }

    fn a(&mut self, line: &str) -> String { self.on(true, line, "$ ", 2_000_000_000) }
    fn b(&mut self, line: &str) -> String { self.on(false, line, "$ ", 2_000_000_000) }
}

#[test]
fn tping_both_ways() {
    let mut p = Pair::new();
    let out = p.a("tping -c 2 0.0.2");
    assert!(out.contains("TPING 0.0.2 (0.0.2) 27 trytes of data."), "{out}");
    assert!(out.contains("27 trytes from 0.0.2: seq=1 time=") && out.contains("27 trytes from 0.0.2: seq=2 time="), "{out}");
    assert!(out.contains("2 packets transmitted, 2 received, 0% packet loss"), "{out}");
    assert!(!out.contains("DATA DIFFER"), "{out}");
    let out = p.b("tping -c 1 -s 2166 0.0.1");
    assert!(out.contains("2166 trytes from 0.0.1: seq=1") && out.contains("1 received"), "the largest echo: {out}");
    let out = p.a("tping -c 1 -W 1 0.0.9; echo status $?");
    assert!(out.contains("1 packets transmitted, 0 received, 100% packet loss\nstatus 1"), "nobody there: {out}");
    let out = p.a("tping 10.0.2.2; echo status $?");
    assert!(out.contains("tping: 10.0.2.2: no TNET address\nstatus 2"), "{out}");
    let out = p.a("tping -s 2167 0.0.2; echo status $?");
    assert!(out.contains("usage: tping") && out.contains("status 2"), "{out}");
}

#[test]
fn tsp_streams_through_nc() {
    let mut p = Pair::new();
    // text crosses as code points, no UTF-8 in between
    p.b("nc -t -l 77 > /tmp/got &");
    let out = p.a("echo 'Сетунь over TNET 11̅' | nc 0.0.2 77; echo status $?");
    assert!(out.contains("status 0"), "{out}");
    let out = p.b("wait; cat /tmp/got");
    assert!(out.contains("Сетунь over TNET 11̅\n"), "{out}");
    // a file of many segments, with frames lost and corrupted on the way
    p.wire.ab.drops = vec![3, 7];
    p.wire.ab.flips = vec![(5, 100, 1), (9, 40, -1)];
    p.wire.ba.drops = vec![2];
    p.b("nc -t -l 78 > /tmp/big &");
    let out = p.a("nc 0.0.2 78 < /etc/ssl/cert.pem; md5sum /etc/ssl/cert.pem");
    let sum_a = out.lines().find(|l| l.ends_with("  /etc/ssl/cert.pem")).map(|l| l[..32].to_string()).unwrap_or_default();
    let out_b = p.b("wait; md5sum /tmp/big; wc -c /tmp/big");
    let sum_b = out_b.lines().find(|l| l.ends_with("  /tmp/big")).map(|l| l[..32].to_string()).unwrap_or_default();
    assert!(!sum_a.is_empty() && sum_a == sum_b, "a: {out}\nb: {out_b}");
    let size: usize = out_b
        .lines()
        .find(|l| l.ends_with(" /tmp/big") && !l.starts_with("wait") && !l.contains("  /tmp/big"))
        .and_then(|l| l.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    assert!(size > 10 * 2159, "many segments: {size}");
    let c = p.b("cat /proc/net/tlink");
    assert!(c.contains("bad_check 2\n"), "two corrupted frames: {c}");
    // refused
    let out = p.a("nc 0.0.2 99; echo status $?");
    assert!(out.contains("Connection refused") && out.contains("status 1"), "{out}");
}

#[test]
fn gate_c_ssh_between_setun_machines() {
    let mut p = Pair::new();
    // sshd makes its host key on the first boot: wait until it listens
    let mut log = String::new();
    for _ in 0..40 {
        log = p.b("sleep 2; cat /var/log/sshd.log");
        if log.contains("listening on port 22") {
            break;
        }
    }
    assert!(log.contains("listening on port 22"), "{log}");
    let out = p.on(true, "ssh user@0.0.2 'uname -a; echo $SSH_CONNECTION'", "password: ", 3_000_000_000);
    assert!(out.contains("user@0.0.2's password: "), "{out}");
    let out = p.on(true, "user", "$ ", 3_000_000_000);
    assert!(out.contains("Trinux setun 0.1 #8 K8 balanced ternary setun11̅"), "{out}");
    let log = p.b("cat /var/log/sshd.log");
    assert!(log.contains("connection from 0.0.1\n"), "sshd saw a TNET peer: {log}");
    let known = p.a("cat /root/.ssh/known_hosts");
    assert!(known.contains("0.0.2 ssh-ed25519 "), "{known}");
    // the wire carried only TL frames with good checks, TNP inside
    assert!(p.wire.log.iter().all(|(_, f)| parse(f).is_some()), "every frame checks");
    assert!(p.wire.log.len() > 20, "{} frames", p.wire.log.len());
}

/// A TSP segment from the host (port 200) to port `dport`.
fn tsp_seg(dport: i16, seq: i64, ack: i64, ctl: i16, data: &[i16]) -> Vec<i16> {
    use setun_net::tnet::word_trytes;
    let mut s = vec![200, dport];
    s.extend(word_trytes(seq));
    s.extend(word_trytes(ack));
    s.push(ctl);
    s.push(9841);
    s.extend_from_slice(data);
    tnp_frame(1, &s)
}

/// The TSP segments in the frames a machine sent: (seq, ack, ctl, data).
fn tsp_of(frames: &[Vec<i16>]) -> Vec<(i64, i64, i16, Vec<i16>)> {
    use setun_net::tnet::trytes_word;
    tnp_of(frames).into_iter().filter(|p| p.2 == 1).map(|(_, _, _, t)| (trytes_word(&t[2..5]), trytes_word(&t[5..8]), t[8], t[10..].to_vec())).collect()
}

#[test]
fn tsp_sequence_numbers_wrap_modulo_3_27() {
    // the host opens a connection whose data crosses the end of the word's
    // range: 3^27 wraps to -3^27 as the machine's own arithmetic does
    const W: i64 = (7_625_597_484_987 - 1) / 2;
    let wrap = |v: i64| (v + W).rem_euclid(2 * W + 1) - W;
    let mut m = boot(ME);
    login(&mut m);
    shell(&mut m, "nc -t -l 50 > /tmp/w &");
    let seq0 = W - 1;
    let r = tsp_of(&exchange(&mut m, &tsp_seg(50, seq0, 0, 1, &[])));
    assert_eq!(r.len(), 1, "a SYN-ACK: {r:?}");
    assert_eq!((r[0].1, r[0].2), (W, 1 + 3), "acknowledges the opening at {W}: {r:?}");
    let iss = r[0].0;
    // five trytes from W: the second is at -W
    let data: Vec<i16> = vec![104, 101, 108, 108, 111];
    let r = tsp_of(&exchange(&mut m, &tsp_seg(50, W, wrap(iss + 1), 3, &data)));
    assert!(r.iter().any(|s| s.1 == wrap(W + 5) && s.1 == -W + 4), "the ACK has wrapped: {r:?}");
    let r = tsp_of(&exchange(&mut m, &tsp_seg(50, wrap(W + 5), wrap(iss + 1), -1 + 3, &[])));
    assert!(r.iter().any(|s| s.1 == wrap(W + 6)), "the closing acknowledged: {r:?}");
    let out = shell(&mut m, "wait; cat /tmp/w; echo");
    assert!(out.contains("hello\n"), "{out}");
}

#[test]
fn a_tsp_opening_is_not_taken_by_a_tcp_listener() {
    let mut p = Pair::new();
    p.b("nc -l 81 > /dev/null &");
    let out = p.a("nc 0.0.2 81; echo status $?");
    assert!(out.contains("Connection refused") && out.contains("status 1"), "only TCP listens on 81: {out}");
}

#[test]
fn tdp_to_a_closed_port_is_unreachable() {
    let mut m = boot(ME);
    login(&mut m);
    let out = exchange(&mut m, &tnp_frame(2, &[200, 90, 1, 2, 3]));
    let got = tnp_of(&out);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!((got[0].2, got[0].3[0], got[0].3[1]), (3, 3, 2), "TCMP port unreachable: {got:?}");
    assert!(exchange(&mut m, &tnp_frame(2, &[200])).is_empty(), "shorter than a TDP header");
    assert!(exchange(&mut m, &tnp_frame(2, &[0, 90, 1])).is_empty(), "port 0");
}

#[test]
fn tdp_datagrams_through_nc() {
    let mut p = Pair::new();
    p.b("nc -t -u -l 90 < /dev/null > /tmp/d &");
    let out = p.a("echo 'датаграмма через TDP 0t1̅01' | nc -u -w 1 0.0.2 90; echo status $?");
    assert!(out.contains("status 0"), "{out}");
    let out = p.b("sleep 1; kill %1; cat /tmp/d");
    assert!(out.contains("датаграмма через TDP 0t1̅01\n"), "{out}");
}
