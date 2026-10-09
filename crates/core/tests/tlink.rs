//! The ternary link card `TLINK` (docs/TNET.md §1), including every error.

use setun_core::devices::{block, irq, tlink_error as error, TLINK_MAX_FRAME, TLINK_QUEUE};
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
const LINK: i64 = 8;
const ERROR: i64 = 9;
const DROPS: i64 = 10;
const ADDR: i64 = 11;

fn out(m: &mut Machine, r: i64, v: i64) {
    m.dev.port_out(27 * block::TLINK + r, Word::new(v).unwrap());
}

fn inp(m: &mut Machine, r: i64) -> i64 {
    m.dev.port_in(27 * block::TLINK + r).value()
}

fn cmd(m: &mut Machine, c: i64) -> i64 {
    out(m, CMD, c);
    m.dev.service(&mut m.mem);
    inp(m, ERROR)
}

fn machine() -> Machine {
    let mut m = Machine::new(27, 1_790_000_000);
    m.dev.tlink.link = true;
    m
}

/// A frame of trytes over the whole range, negative ones included.
fn frame(len: usize, seed: i64) -> Vec<i16> {
    (0..len as i64).map(|i| ((i * 7919 + seed * 131 + 9841).rem_euclid(19683) - 9841) as i16).collect()
}

fn poke(m: &mut Machine, at: i64, data: &[i16]) {
    for (i, &t) in data.iter().enumerate() {
        m.mem.write_tryte(at + i as i64, Tryte::new(t as i64).unwrap()).unwrap();
    }
}

fn peek(m: &Machine, at: i64, n: usize) -> Vec<i16> {
    (0..n).map(|i| m.mem.read_tryte(at + i as i64).unwrap().value() as i16).collect()
}

#[test]
fn link_and_address() {
    let mut m = Machine::new(27, 0);
    assert_eq!((inp(&mut m, LINK), inp(&mut m, ADDR)), (0, 0), "nothing until a front end attaches a link");
    m.dev.tlink.link = true;
    m.dev.tlink.addr = -3_000_000_000;
    assert_eq!((inp(&mut m, LINK), inp(&mut m, ADDR)), (1, -3_000_000_000));
    out(&mut m, ADDR, 5);
    assert_eq!(inp(&mut m, ADDR), -3_000_000_000, "the address is read-only");
    m.dev.reset();
    assert_eq!((inp(&mut m, LINK), inp(&mut m, ADDR)), (1, -3_000_000_000), "reset keeps the link and the address");
}

#[test]
fn send_moves_any_trytes_at_any_alignment() {
    let mut m = machine();
    let f = frame(TLINK_MAX_FRAME, 3);
    assert!(f.iter().any(|&t| t < 0) && f.iter().any(|&t| t > 255), "not octets");
    let at = frame_start(5) + 1;
    poke(&mut m, at, &f);
    out(&mut m, TX_ADDR, at);
    out(&mut m, TX_LEN, TLINK_MAX_FRAME as i64);
    assert_eq!(cmd(&mut m, 1), error::NONE);
    assert_eq!(inp(&mut m, STATUS), 0);
    out(&mut m, TX_LEN, 1);
    assert_eq!(cmd(&mut m, 1), error::NONE, "a one-tryte frame");
    assert_eq!(m.dev.tlink_take_tx(), vec![f.clone(), f[..1].to_vec()]);
    assert!(m.dev.tlink_take_tx().is_empty());
}

#[test]
fn receive_copies_and_pops_in_order_with_irq() {
    let mut m = machine();
    out(&mut m, CTRL, 1);
    m.dev.port_out(27 * block::TPIC + 3, Word::new(irq::TLINK as i64).unwrap());
    let (a, b) = (frame(9, 1), frame(TLINK_MAX_FRAME, 2));
    m.dev.tlink_receive(&a);
    m.dev.tlink_receive(&b);
    assert_eq!(m.dev.pending_vector(), Some(1 + irq::TLINK as i64));
    assert_eq!((inp(&mut m, RX_COUNT), inp(&mut m, RX_LEN)), (2, 9));
    let at = frame_start(6) - 4;
    out(&mut m, RX_ADDR, at);
    assert_eq!(cmd(&mut m, -1), error::NONE);
    assert_eq!(peek(&m, at, 9), a);
    assert_eq!((inp(&mut m, RX_COUNT), inp(&mut m, RX_LEN)), (1, TLINK_MAX_FRAME as i64));
    assert_eq!(cmd(&mut m, 0), error::NONE);
    assert_eq!(inp(&mut m, RX_COUNT), 0);
}

#[test]
fn no_irq_unless_enabled() {
    let mut m = machine();
    m.dev.port_out(27 * block::TPIC + 3, Word::new(irq::TLINK as i64).unwrap());
    m.dev.tlink_receive(&frame(20, 0));
    assert_eq!(m.dev.pending_vector(), None);
    assert_eq!(inp(&mut m, RX_COUNT), 1);
}

#[test]
fn send_errors() {
    let mut m = machine();
    let at = frame_start(5);
    poke(&mut m, at, &frame(100, 0));
    out(&mut m, TX_ADDR, at);
    for len in [0, -1, TLINK_MAX_FRAME as i64 + 1] {
        out(&mut m, TX_LEN, len);
        assert_eq!(cmd(&mut m, 1), error::BAD_LENGTH, "length {len}");
        assert_eq!(inp(&mut m, STATUS), -1);
    }
    out(&mut m, TX_LEN, 9);
    out(&mut m, TX_ADDR, -10 * 19683);
    assert_eq!(cmd(&mut m, 1), error::BUS, "no memory there");
    out(&mut m, TX_ADDR, at);
    for _ in 0..TLINK_QUEUE {
        assert_eq!(cmd(&mut m, 1), error::NONE);
    }
    assert_eq!(cmd(&mut m, 1), error::TX_FULL);
    assert_eq!(m.dev.tlink_take_tx().len(), TLINK_QUEUE, "the refused frame was not queued");
    m.dev.tlink.link = false;
    assert_eq!(cmd(&mut m, 1), error::NO_LINK);
    m.dev.tlink.link = true;
    assert_eq!(cmd(&mut m, 2), error::BAD_CMD);
    assert_eq!(cmd(&mut m, 1), error::NONE, "and it works again");
    assert_eq!(inp(&mut m, ERROR), error::NONE);
}

#[test]
fn receive_errors() {
    let mut m = machine();
    assert_eq!(cmd(&mut m, -1), error::NOTHING);
    assert_eq!(cmd(&mut m, 0), error::NOTHING);
    // frames the card never takes: empty, too long, not trytes, no link
    m.dev.tlink_receive(&[]);
    m.dev.tlink_receive(&frame(TLINK_MAX_FRAME + 1, 0));
    m.dev.tlink_receive(&[0, 9842, 0]);
    m.dev.tlink.link = false;
    m.dev.tlink_receive(&frame(9, 0));
    m.dev.tlink.link = true;
    assert_eq!(inp(&mut m, RX_COUNT), 0);
    // a full queue counts drops
    for k in 0..TLINK_QUEUE + 2 {
        m.dev.tlink_receive(&frame(9, k as i64));
    }
    assert_eq!((inp(&mut m, RX_COUNT), inp(&mut m, DROPS)), (TLINK_QUEUE as i64, 2));
    // a bus error leaves the frame queued
    out(&mut m, RX_ADDR, -10 * 19683);
    assert_eq!(cmd(&mut m, -1), error::BUS);
    assert_eq!(inp(&mut m, RX_COUNT), TLINK_QUEUE as i64);
    out(&mut m, RX_ADDR, frame_start(4));
    assert_eq!(cmd(&mut m, -1), error::NONE);
    assert_eq!(peek(&m, frame_start(4), 9), frame(9, 0), "the oldest first");
}
