//! The network card `TNIC` (docs/NETWORK.md §1), including every error.

use setun_core::devices::{block, irq, nic_error as error, DEFAULT_MAC, MAX_FRAME, NIC_QUEUE};
use setun_core::memory::frame_start;
use setun_core::{Io, Machine};
use ternary::{Tryte, Word};

const CMD: i64 = 0;
const STATUS: i64 = 1;
const TX_ADDR: i64 = 2;
const TX_LEN: i64 = 3;
const RX_ADDR: i64 = 4;
const RX_LEN: i64 = 5;
const RX_COUNT: i64 = 6;
const CTRL: i64 = 7;
const MAC_HI: i64 = 8;
const MAC_LO: i64 = 9;
const LINK: i64 = 10;
const ERROR: i64 = 11;
const DROPS: i64 = 12;

fn out(m: &mut Machine, r: i64, v: i64) {
    m.dev.port_out(27 * block::NIC + r, Word::new(v).unwrap());
}

fn inp(m: &mut Machine, r: i64) -> i64 {
    m.dev.port_in(27 * block::NIC + r).value()
}

/// Write a register and let the card act, as after an `OUT` instruction.
fn cmd(m: &mut Machine, c: i64) -> i64 {
    out(m, CMD, c);
    m.dev.service(&mut m.mem);
    inp(m, ERROR)
}

fn machine() -> Machine {
    let mut m = Machine::new(27, 1_790_000_000);
    m.dev.nic.link = true;
    m
}

fn frame(len: usize, seed: u8) -> Vec<u8> {
    (0..len).map(|i| (i as u8).wrapping_mul(7).wrapping_add(seed)).collect()
}

fn poke_octets(m: &mut Machine, at: i64, data: &[u8]) {
    for (i, &b) in data.iter().enumerate() {
        m.mem.write_tryte(at + i as i64, Tryte::new(b as i64).unwrap()).unwrap();
    }
}

fn peek_octets(m: &Machine, at: i64, n: usize) -> Vec<u8> {
    (0..n).map(|i| m.mem.read_tryte(at + i as i64).unwrap().value() as u8).collect()
}

#[test]
fn mac_and_link() {
    let mut m = Machine::new(27, 0);
    let mac = DEFAULT_MAC.map(|b| b as i64);
    assert_eq!(inp(&mut m, MAC_HI), mac[0] * 65536 + mac[1] * 256 + mac[2]);
    assert_eq!(inp(&mut m, MAC_LO), mac[3] * 65536 + mac[4] * 256 + mac[5]);
    assert_eq!(inp(&mut m, LINK), 0, "no network until a front end attaches one");
    m.dev.nic.link = true;
    assert_eq!(inp(&mut m, LINK), 1);
}

#[test]
fn send_moves_octets_at_any_alignment() {
    let mut m = machine();
    let f = frame(60, 3);
    let at = frame_start(5) + 1; // not word-aligned
    poke_octets(&mut m, at, &f);
    out(&mut m, TX_ADDR, at);
    out(&mut m, TX_LEN, 60);
    assert_eq!(cmd(&mut m, 1), error::NONE);
    assert_eq!(inp(&mut m, STATUS), 0);
    assert_eq!(m.dev.net_take_tx(), vec![f]);
    assert!(m.dev.net_take_tx().is_empty(), "taking empties the queue");
}

#[test]
fn receive_copies_and_pops_in_order_with_irq() {
    let mut m = machine();
    out(&mut m, CTRL, 1);
    m.dev.port_out(27 * block::TPIC + 3, Word::new(irq::NIC as i64).unwrap()); // unmask
    assert_eq!(inp(&mut m, RX_LEN), 0);
    let (a, b) = (frame(14, 1), frame(MAX_FRAME, 2));
    m.dev.net_receive(&a);
    m.dev.net_receive(&b);
    assert_eq!(m.dev.pending_vector(), Some(1 + irq::NIC as i64));
    assert_eq!((inp(&mut m, RX_COUNT), inp(&mut m, RX_LEN)), (2, 14));

    let at = frame_start(6) - 4;
    out(&mut m, RX_ADDR, at);
    assert_eq!(cmd(&mut m, -1), error::NONE);
    assert_eq!(peek_octets(&m, at, 14), a);
    assert_eq!((inp(&mut m, RX_COUNT), inp(&mut m, RX_LEN)), (1, MAX_FRAME as i64));

    assert_eq!(cmd(&mut m, 0), error::NONE, "drop without copying");
    assert_eq!(inp(&mut m, RX_COUNT), 0);
    assert_eq!(peek_octets(&m, at, 14), a, "the dropped frame was not copied");
}

#[test]
fn no_irq_unless_enabled() {
    let mut m = machine();
    m.dev.port_out(27 * block::TPIC + 3, Word::new(irq::NIC as i64).unwrap());
    m.dev.net_receive(&frame(20, 0));
    assert_eq!(m.dev.pending_vector(), None, "CTRL is 0 at reset");
    assert_eq!(inp(&mut m, RX_COUNT), 1, "the frame is still queued");
}

// ---- errors ------------------------------------------------------------------

#[test]
fn send_errors() {
    let mut m = machine();
    let at = frame_start(5);
    poke_octets(&mut m, at, &frame(100, 0));
    out(&mut m, TX_ADDR, at);

    for len in [0, 13, MAX_FRAME as i64 + 1, -60] {
        out(&mut m, TX_LEN, len);
        assert_eq!(cmd(&mut m, 1), error::BAD_LENGTH, "length {len}");
        assert_eq!(inp(&mut m, STATUS), -1);
    }

    out(&mut m, TX_LEN, 60);
    m.mem.write_tryte(at + 30, Tryte::new(256).unwrap()).unwrap();
    assert_eq!(cmd(&mut m, 1), error::NOT_OCTET);
    m.mem.write_tryte(at + 30, Tryte::new(-1).unwrap()).unwrap();
    assert_eq!(cmd(&mut m, 1), error::NOT_OCTET, "negative trytes are not octets either");
    m.mem.write_tryte(at + 30, Tryte::new(9).unwrap()).unwrap();

    out(&mut m, TX_ADDR, frame_start(27) - 10); // runs past the end of RAM
    assert_eq!(cmd(&mut m, 1), error::BUS);
    assert!(m.dev.net_take_tx().is_empty(), "nothing failed was sent");

    out(&mut m, TX_ADDR, at);
    assert_eq!(cmd(&mut m, 1), error::NONE, "a good send clears the error");
    assert_eq!(inp(&mut m, STATUS), 0);

    m.dev.nic.link = false;
    assert_eq!(cmd(&mut m, 1), error::NO_LINK);
    m.dev.nic.link = true;

    assert_eq!(cmd(&mut m, 2), error::BAD_CMD);
    assert_eq!(cmd(&mut m, -5), error::BAD_CMD);
}

#[test]
fn transmit_queue_full() {
    let mut m = machine();
    let at = frame_start(5);
    poke_octets(&mut m, at, &frame(60, 0));
    out(&mut m, TX_ADDR, at);
    out(&mut m, TX_LEN, 60);
    for _ in 0..NIC_QUEUE {
        assert_eq!(cmd(&mut m, 1), error::NONE);
    }
    assert_eq!(cmd(&mut m, 1), error::TX_FULL);
    assert_eq!(m.dev.net_take_tx().len(), NIC_QUEUE);
    assert_eq!(cmd(&mut m, 1), error::NONE, "room again once the host drains it");
}

#[test]
fn receive_errors() {
    let mut m = machine();
    out(&mut m, RX_ADDR, frame_start(5));
    assert_eq!(cmd(&mut m, -1), error::NOTHING);
    assert_eq!(cmd(&mut m, 0), error::NOTHING);

    let f = frame(64, 5);
    m.dev.net_receive(&f);
    out(&mut m, RX_ADDR, frame_start(-1)); // ROM
    assert_eq!(cmd(&mut m, -1), error::BUS);
    assert_eq!(inp(&mut m, RX_COUNT), 1, "a failed receive keeps the frame");
    out(&mut m, RX_ADDR, frame_start(5));
    assert_eq!(cmd(&mut m, -1), error::NONE);
    assert_eq!(peek_octets(&m, frame_start(5), 64), f);
}

#[test]
fn bad_frames_from_the_network_are_not_delivered() {
    let mut m = machine();
    m.dev.net_receive(&frame(13, 0));
    m.dev.net_receive(&frame(MAX_FRAME + 1, 0));
    assert_eq!(inp(&mut m, RX_COUNT), 0);
    m.dev.nic.link = false;
    m.dev.net_receive(&frame(60, 0));
    assert_eq!(inp(&mut m, RX_COUNT), 0, "no link, no frames");
}

#[test]
fn receive_queue_overflow_counts_drops() {
    let mut m = machine();
    for k in 0..NIC_QUEUE + 5 {
        m.dev.net_receive(&frame(60, k as u8));
    }
    assert_eq!(inp(&mut m, RX_COUNT), NIC_QUEUE as i64);
    assert_eq!(inp(&mut m, DROPS), 5);
    out(&mut m, RX_ADDR, frame_start(5));
    assert_eq!(cmd(&mut m, -1), error::NONE);
    assert_eq!(peek_octets(&m, frame_start(5), 60), frame(60, 0), "the oldest frames are kept");
}

#[test]
fn reset_keeps_link_mac_and_queued_frames() {
    let mut m = machine();
    out(&mut m, CTRL, 1);
    m.dev.net_receive(&frame(60, 0));
    m.reset();
    assert_eq!(inp(&mut m, CTRL), 0);
    assert_eq!(inp(&mut m, LINK), 1);
    assert_eq!(inp(&mut m, RX_COUNT), 1);
}

// ---- the random-number generator (block 11) ----------------------------------------------

#[test]
fn rng_is_chacha20_and_reproducible_by_default() {
    let mut m = Machine::new(27, 0);
    let rd = |m: &mut Machine, r: i64| m.dev.port_in(27 * block::RNG + r).value();
    assert_eq!(rd(&mut m, 1), 0, "SOURCE: a fixed seed");
    let first: Vec<i64> = (0..16).map(|_| rd(&mut m, 0)).collect();
    // RFC 8439 A.1, test vector 1: all-zero key and nonce, block 0
    let want = [0x76, 0xb8, 0xe0, 0xad, 0xa0, 0xf1, 0x3d, 0x90, 0x40, 0x5d, 0x6a, 0xe5, 0x53, 0x86, 0xbd, 0x28];
    assert_eq!(first, want.map(|b| b as i64).to_vec());
    // the next block continues the stream
    for _ in 16..64 {
        rd(&mut m, 0);
    }
    let second: Vec<i64> = (0..4).map(|_| rd(&mut m, 0)).collect();
    assert_eq!(second, vec![0x9f, 0x07, 0xe7, 0xbe]); // RFC 8439 A.1 vector 2 (block counter 1)
    m.dev.rng.seed(setun_core::devices::host_entropy(), true);
    assert_eq!(rd(&mut m, 1), 1);
    let a: Vec<i64> = (0..32).map(|_| rd(&mut m, 0)).collect();
    assert!(a.iter().all(|&v| (0..=255).contains(&v)));
    assert_ne!(a[..16], want.map(|b| b as i64)[..], "a host seed changes the stream");
    m.reset();
    assert_eq!(rd(&mut m, 1), 1, "reset keeps the seed");
}
