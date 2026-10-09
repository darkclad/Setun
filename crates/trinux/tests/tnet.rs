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
    assert!(exchange(&mut m, &frame(ME, PEER, 1, &[1, 2, 3])).is_empty(), "TNP waits for C2");
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
