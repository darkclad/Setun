//! Device tests against docs/SETUN386_ISA.md §8.

use setun_core::devices::{self, block, decode_port, irq, BlockStore, Display, MemDisk};
use setun_core::isa::{self, alu, cond, cr, op, sys};
use setun_core::memory::frame_start;
use setun_core::{Io, Machine, PAGE};
use ternary::{Tryte, Word};

fn port(b: i64, r: i64) -> i64 {
    27 * b + r
}

fn out(m: &mut Machine, b: i64, r: i64, v: i64) {
    m.dev.port_out(port(b, r), Word::new(v).unwrap());
}

fn inp(m: &mut Machine, b: i64, r: i64) -> i64 {
    m.dev.port_in(port(b, r)).value()
}

fn machine() -> Machine {
    Machine::new(27, 1_790_000_000)
}

/// Load words at consecutive word addresses from `origin`.
fn load(m: &mut Machine, origin: i64, words: &[Word]) {
    m.mem.load_words(origin, words);
}

#[test]
fn port_decoding_is_centred() {
    assert_eq!(decode_port(0), (0, 0));
    assert_eq!(decode_port(164), (6, 2));
    assert_eq!(decode_port(13), (0, 13));
    assert_eq!(decode_port(14), (1, -13));
    assert_eq!(decode_port(-14), (-1, 13));
}

#[test]
fn sysctl() {
    let mut m = machine();
    assert_eq!(inp(&mut m, block::SYSCTL, 0), devices::SYSID);
    assert_eq!(inp(&mut m, block::SYSCTL, 1), 27);
    out(&mut m, block::SYSCTL, 3, 'x' as i64);
    assert_eq!(m.dev.debug_log, vec!['x' as i64]);
    assert_eq!(m.cpu.cr(cr::CPUID).value(), 4);
}

#[test]
fn pic_masking_claim_and_priority() {
    let mut m = machine();
    m.dev.pic.raise(5);
    assert_eq!(m.dev.pending_vector(), None, "masked at reset");
    out(&mut m, block::TPIC, 3, 5); // unmask 5
    out(&mut m, block::TPIC, 3, 2);
    out(&mut m, block::TPIC, 3, 0);
    assert_eq!(m.dev.pending_vector(), Some(6), "vector = VECBASE(1) + 5");
    out(&mut m, block::TPIC, 5, 5);
    assert_eq!(inp(&mut m, block::TPIC, 6), 1, "selected line pending");
    m.dev.pic.raise(2);
    assert_eq!(inp(&mut m, block::TPIC, 1), 2, "claim: highest priority first");
    assert_eq!(m.dev.pending_vector(), None, "5 blocked while 2 in service");
    m.dev.pic.raise(0);
    assert_eq!(m.dev.pending_vector(), Some(1), "0 outranks the in-service 2");
    assert_eq!(inp(&mut m, block::TPIC, 1), 0);
    out(&mut m, block::TPIC, 2, 0); // EOI 0
    out(&mut m, block::TPIC, 2, 2); // EOI 2
    assert_eq!(inp(&mut m, block::TPIC, 1), 5);
    assert_eq!(inp(&mut m, block::TPIC, 1), -1, "nothing left");
}

#[test]
fn timer_modes() {
    let mut m = machine();
    out(&mut m, block::TPIC, 3, irq::TIMER as i64);
    out(&mut m, block::TIMER, 1, 1000);
    out(&mut m, block::TIMER, 0, -1); // one-shot
    m.dev.advance_us(999);
    assert_eq!(m.dev.pending_vector(), None);
    m.dev.advance_us(1);
    assert_eq!(m.dev.pending_vector(), Some(1), "fired");
    assert_eq!(inp(&mut m, block::TIMER, 0), 0, "one-shot stops");
    assert_eq!(inp(&mut m, block::TIMER, 3), 1000, "uptime µs");
}

/// A real interrupt-driven program: the kernel idles in HLT, a 1 ms periodic
/// timer interrupt counts ticks in r5.
#[test]
fn timer_interrupt_program() {
    let mut m = machine();
    let q = |b, rd, ra, v| isa::rrr(op::ALUQ, b, rd, ra, v);
    let itb = 3 * 3000;
    let handler = 3 * 4000;
    // Vector table: IRQ 0 → vector +1.
    load(&mut m, itb + 3, &[Word::new(handler).unwrap()]);
    // Handler: r7 = TPIC base; r6 = CLAIM; EOI r6; r5 += 1; IRET.
    load(&mut m, handler, &[
        isa::s6(op::LIQ, 0, 7, 27),
        isa::rrr(op::IO, isa::io::IN, 6, 7, 1),
        isa::rrr(op::IO, isa::io::OUT, 6, 7, 2),
        q(alu::ADD, 5, 5, 1),
        isa::rrr(op::SYS, sys::IRET, 0, 0, 0),
    ]);
    // Kernel at 0.
    let mut k = Vec::new();
    k.extend(isa::wide(op::LI, 0, 1, 0, itb));
    k.push(isa::rrr(op::SYS, sys::MTC, cr::ITB, 1, 0));
    k.push(isa::s6(op::LIQ, 0, 7, 27));
    k.push(isa::rrr(op::IO, isa::io::OUT, 0, 7, 3)); // UNMASK IRQ 0 (r0 = 0)
    k.push(isa::s6(op::LIQ, 0, 7, 54)); // timer block
    k.extend(isa::wide(op::LI, 0, 1, 0, 1000));
    k.push(isa::rrr(op::IO, isa::io::OUT, 1, 7, 1)); // PERIOD = 1000 µs
    k.push(isa::s6(op::LIQ, 0, 1, 1));
    k.push(isa::rrr(op::IO, isa::io::OUT, 1, 7, 0)); // CTRL = periodic
    k.push(isa::rrr(op::SYS, sys::MFC, 1, cr::MODE, 0));
    k.push(q(alu::ADD, 1, 1, 3)); // IE trit (weight 3) := +1
    k.push(isa::rrr(op::SYS, sys::MTC, cr::MODE, 1, 0));
    let idle = k.len() as i64;
    k.push(isa::rrr(op::SYS, sys::HLT, 0, 0, 0));
    k.push(isa::s6(op::BRQ, 0, cond::ALWAYS, idle - k.len() as i64));
    load(&mut m, 0, &k);
    m.cpu.pc = Word::ZERO;

    m.run(55_000); // 5.5 ms at 100 ns/instruction
    assert_eq!(m.cpu.reg(5).value(), 5, "five 1 ms ticks");
}

#[test]
fn rtc_time_and_alarm() {
    let mut m = machine();
    assert_eq!(inp(&mut m, block::RTC, 0), 1_790_000_000);
    m.dev.advance_us(2_500_000);
    assert_eq!(inp(&mut m, block::RTC, 0), 1_790_000_002);
    assert_eq!(inp(&mut m, block::RTC, 1), 500_000);
    out(&mut m, block::RTC, 0, 2_000_000_000);
    assert_eq!(inp(&mut m, block::RTC, 0), 2_000_000_000);
    out(&mut m, block::TPIC, 3, irq::RTC as i64);
    out(&mut m, block::RTC, 2, 2_000_000_001);
    m.dev.advance_us(999_999);
    assert_eq!(m.dev.pending_vector(), None);
    m.dev.advance_us(1);
    assert_eq!(m.dev.pending_vector(), Some(1 + irq::RTC as i64));
    assert_eq!(inp(&mut m, block::RTC, 2), 0, "alarm clears");
}

#[test]
fn keyboard_mouse_serial() {
    let mut m = machine();
    for line in [irq::KEYBOARD, irq::MOUSE, irq::SERIAL] {
        out(&mut m, block::TPIC, 3, line as i64);
    }
    m.dev.key(4, true); // HID 'a', IRQ off: queued silently
    assert_eq!(m.dev.pending_vector(), None);
    out(&mut m, block::KEYBOARD, 2, 1);
    m.dev.key(4, false);
    assert_eq!(m.dev.pending_vector(), Some(1 + irq::KEYBOARD as i64));
    assert_eq!(inp(&mut m, block::KEYBOARD, 1), 2);
    assert_eq!(inp(&mut m, block::KEYBOARD, 0), 4);
    assert_eq!(inp(&mut m, block::KEYBOARD, 0), -4);
    assert_eq!(inp(&mut m, block::KEYBOARD, 0), 0);

    m.dev.mouse_move(5, -3);
    m.dev.mouse_move(2, 1);
    m.dev.mouse_buttons(true, false, true);
    assert_eq!(inp(&mut m, block::MOUSE, 0), 7);
    assert_eq!(inp(&mut m, block::MOUSE, 1), -2);
    assert_eq!(inp(&mut m, block::MOUSE, 0), 0, "read clears");
    assert_eq!(inp(&mut m, block::MOUSE, 2), 10, "L + 9·R");
    m.dev.mouse_abs(100, 50);
    assert_eq!((inp(&mut m, block::MOUSE, 5), inp(&mut m, block::MOUSE, 6)), (100, 50));
    // A press and a release between two reads both stay, each with its position.
    m.dev.mouse_buttons(false, false, false);
    assert_eq!(inp(&mut m, block::MOUSE, 2), 0);
    m.dev.mouse_buttons(true, false, false);
    m.dev.mouse_abs(120, 60);
    m.dev.mouse_buttons(false, false, false);
    assert_eq!(inp(&mut m, block::MOUSE, 7), 2, "two changes pending");
    assert_eq!(inp(&mut m, block::MOUSE, 2), 1, "the press first");
    assert_eq!((inp(&mut m, block::MOUSE, 5), inp(&mut m, block::MOUSE, 6)), (100, 50), "where it was pressed");
    assert_eq!(inp(&mut m, block::MOUSE, 2), 0, "then the release");
    assert_eq!((inp(&mut m, block::MOUSE, 5), inp(&mut m, block::MOUSE, 6)), (120, 60));
    assert_eq!(inp(&mut m, block::MOUSE, 7), 0);
    assert_eq!(inp(&mut m, block::MOUSE, 2), 0, "the current state once drained");
    for _ in 0..40 {
        m.dev.mouse_buttons(true, false, false);
        m.dev.mouse_buttons(false, false, false);
    }
    assert_eq!(inp(&mut m, block::MOUSE, 7), 27, "the FIFO keeps the newest 27 changes");

    m.dev.serial_input("Жa");
    assert_eq!(inp(&mut m, block::SERIAL, 1), 2);
    assert_eq!(inp(&mut m, block::SERIAL, 0), 'Ж' as i64, "a tryte holds the code point");
    out(&mut m, block::SERIAL, 0, 'П' as i64);
    out(&mut m, block::SERIAL, 0, '!' as i64);
    assert_eq!(m.dev.serial.take_output(), "П!");
}

#[test]
fn disk_dma_read_write_and_errors() {
    let mut m = machine();
    let mut img = vec![0i16; 3 * PAGE as usize];
    img[0] = 1234;
    img[PAGE as usize + 5] = -777;
    m.dev.disks[0].attach(Box::new(MemDisk::from_trytes(&img)));
    assert_eq!(inp(&mut m, block::DISK0, 5), 3, "SECTORS");

    // Read sectors 0..=1 into frames 4..=5.
    out(&mut m, block::DISK0, 2, 0);
    out(&mut m, block::DISK0, 3, 2);
    out(&mut m, block::DISK0, 4, frame_start(4));
    out(&mut m, block::DISK0, 7, 1);
    out(&mut m, block::TPIC, 3, irq::DISK0 as i64);
    out(&mut m, block::DISK0, 0, -1);
    assert_eq!(inp(&mut m, block::DISK0, 1), 1, "busy until serviced");
    m.dev.service(&mut m.mem);
    assert_eq!(inp(&mut m, block::DISK0, 1), 0);
    assert_eq!(m.dev.pending_vector(), Some(1 + irq::DISK0 as i64));
    assert_eq!(m.mem.read_tryte(frame_start(4)).unwrap().value(), 1234);
    assert_eq!(m.mem.read_tryte(frame_start(5) + 5).unwrap().value(), -777);

    // Write frame 4 back to sector 2.
    m.mem.write_tryte(frame_start(4) + 9, Tryte::new(42).unwrap()).unwrap();
    out(&mut m, block::DISK0, 2, 2);
    out(&mut m, block::DISK0, 3, 1);
    out(&mut m, block::DISK0, 0, 1);
    m.dev.service(&mut m.mem);
    let store = m.dev.disks[0].detach().unwrap();
    let mut buf = vec![0i16; PAGE as usize];
    let mut store = store;
    store.read(2, &mut buf);
    assert_eq!((buf[0], buf[9]), (1234, 42));
    m.dev.disks[0].attach(store);

    // Errors: misaligned DMA, LBA out of range, write into ROM.
    for (lba, dma, err) in [(0, frame_start(4) + 1, 3), (3, frame_start(4), 2), (0, frame_start(-1), 4)] {
        out(&mut m, block::DISK0, 2, lba);
        out(&mut m, block::DISK0, 3, 1);
        out(&mut m, block::DISK0, 4, dma);
        out(&mut m, block::DISK0, 0, -1);
        m.dev.service(&mut m.mem);
        assert_eq!(inp(&mut m, block::DISK0, 1), -1, "STATUS error");
        assert_eq!(inp(&mut m, block::DISK0, 6), err);
    }
}

#[test]
fn file_disk_roundtrip() {
    use setun_core::devices::FileDisk;
    let path = std::env::temp_dir().join(format!("setun386-test-{}.t1b2", std::process::id()));
    {
        let mut d = FileDisk::create(&path, 2).unwrap();
        let mut buf = vec![0i16; PAGE as usize];
        buf[0] = -9841;
        buf[PAGE as usize - 1] = 9841;
        assert!(d.write(1, &buf));
    }
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 2 * 2 * PAGE as u64, "T1B2: 2 bytes per tryte");
    let mut d = FileDisk::open(&path).unwrap();
    assert_eq!(d.sectors(), 2);
    let mut buf = vec![0i16; PAGE as usize];
    assert!(d.read(1, &mut buf));
    assert_eq!((buf[0], buf[PAGE as usize - 1]), (-9841, 9841));
    drop(d);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn display_registers_and_vsync() {
    let mut m = machine();
    assert_eq!(inp(&mut m, block::DISPLAY, 0), -1, "text mode at reset");
    assert_eq!(
        (inp(&mut m, block::DISPLAY, 2), inp(&mut m, block::DISPLAY, 3), inp(&mut m, block::DISPLAY, 4)),
        (80, 30, 240)
    );
    assert_eq!(inp(&mut m, block::DISPLAY, 1), -1_604_164);
    out(&mut m, block::DISPLAY, 0, 1);
    assert_eq!((inp(&mut m, block::DISPLAY, 2), inp(&mut m, block::DISPLAY, 3)), (800, 600));
    out(&mut m, block::DISPLAY, 6, 1);
    out(&mut m, block::TPIC, 3, irq::DISPLAY as i64);
    m.dev.advance_us(devices::FRAME_US);
    assert_eq!(inp(&mut m, block::DISPLAY, 5), 1);
    assert_eq!(m.dev.pending_vector(), Some(1 + irq::DISPLAY as i64));
}

#[test]
fn graphics_pixels_are_rgb_digits() {
    let mut m = machine();
    out(&mut m, block::DISPLAY, 0, 0); // 640×480
    let base = Display::fb_base();
    let px = |s: &str| Tryte::from_b27(s).unwrap();
    for (x, c) in ["MMM", "mmm", "Mmm", "mMm", "mmM", "000"].iter().enumerate() {
        m.mem.write_tryte(base + 640 * 2 + x as i64, px(c)).unwrap();
    }
    let mut buf = Vec::new();
    assert_eq!(m.dev.display.render(&m.mem, &mut buf), (640, 480));
    let row = &buf[640 * 2..640 * 2 + 6];
    assert_eq!(row, &[0xFFFFFF, 0x000000, 0xFF0000, 0x00FF00, 0x0000FF, 0x808080]);
    assert_eq!(buf[0], 0x808080, "zeroed framebuffer is mid-grey");
}

#[test]
fn text_mode_renders_glyphs_and_colours() {
    let mut m = machine();
    let base = Display::fb_base();
    // Cell (row 1, col 2): 'H', white on blue (fg +++, bg --+).
    let cell = base + 3 * (80 + 2);
    let attr: Tryte = "+++--+000".parse().unwrap();
    m.mem.write_tryte(cell, Tryte::new('H' as i64).unwrap()).unwrap();
    m.mem.write_tryte(cell + 1, attr).unwrap();
    m.dev.display.write(7, -1); // cursor off-screen
    let mut buf = Vec::new();
    m.dev.display.render(&m.mem, &mut buf);
    // font8x8 'H' row 1 is 0x33: pixels 0,1,4,5 lit. Glyph row 1 → scanlines 2..=3.
    let y = 16 + 2;
    let x0 = 2 * 8;
    let line: Vec<u32> = (0..8).map(|x| buf[y * 640 + x0 + x]).collect();
    let (w, b) = (0xFFFFFF, 0x0000FF);
    assert_eq!(line, vec![w, w, b, b, w, w, b, b]);
}

#[test]
fn rom_boots_prints_and_powers_off() {
    // A BIOS at the reset vector: print "OK" on serial, then SYSCTL POWER −1.
    let mut m = machine();
    let mut rom_words = vec![Word::ZERO; 3 * 729]; // ROM prefix up to the entry word
    let entry_index = ((-59_049 - (frame_start(-3) + 1)) / 3) as usize; // word index of −3^10
    let prog = [
        isa::s6(op::LIQ, 0, 7, 162),
        isa::s6(op::LIQ, 0, 1, 'O' as i64),
        isa::rrr(op::IO, isa::io::OUT, 1, 7, 0),
        isa::s6(op::LIQ, 0, 1, 'K' as i64),
        isa::rrr(op::IO, isa::io::OUT, 1, 7, 0),
        isa::s6(op::LIQ, 0, 1, -1),
        isa::rrr(op::IO, isa::io::OUT, 1, 0, 2), // port 0 + 2 = SYSCTL.POWER
    ];
    rom_words.resize(entry_index + prog.len(), Word::ZERO);
    rom_words[entry_index..].copy_from_slice(&prog);
    let trytes: Vec<i16> = rom_words
        .iter()
        .flat_map(|&w| setun_core::memory::word_to_trytes(w).map(|t| t.value() as i16))
        .collect();
    // Word k's first tryte sits at frame_start(−3) + 3k: word centres are
    // multiples of 3, and frame_start(−3) + 1 = −68 889 is one.
    m.load_rom(&trytes);
    let n = m.run(1000);
    assert!(m.powered_off);
    assert_eq!(n, 7);
    assert_eq!(m.dev.serial.take_output(), "OK");
}
