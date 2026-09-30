//! Demo ROM for the Setun-386 front ends.
//!
//! Boots from the reset vector (paging off, ring +1) and shows:
//! * a text-mode screen with machine facts, the 27 text colours, the RAM size
//!   (from `SYSCTL`) and a live Unix clock (from the RTC);
//! * keyboard and serial echo into a typing area (with Shift, Enter,
//!   Backspace), mirrored to the serial console;
//! * Tab: a 640×480 graphics screen sweeping through the 3^9-colour space;
//!   any key returns;
//! * a fault handler that reports CPU faults on the bottom line.
//!
//! Written with the `setun-asm` builder; the text assembler comes later.

use setun_asm::{words_to_trytes, Asm};
use setun_core::isa::{alu, cond, cr};
use setun_core::memory::{frame_start, ROM_FIRST_FRAME, ROM_FRAMES};
use setun_core::{Machine, PAGE};
use ternary::{Trit, Tryte, Word};

pub const ENTRY: i64 = -59_049;
/// Interrupt table: a multiple of 729 inside ROM frame −1.
pub const ITB: i64 = -29_160;
pub const FB_BASE: i64 = -1_604_164;
const STACK_TOP: i64 = 9_000;

// Global registers.
const CUR: i64 = -1; // cursor cell
const SHIFT: i64 = -2; // Shift held (0/1)
const FB: i64 = -3; // framebuffer base
const ATTR: i64 = -4; // attribute for typed text
const KBD: i64 = -5; // keyboard block base port
const SER: i64 = -6; // serial block base port
const DISP: i64 = -7; // display block base port

const PROMPT_CELL: i64 = 14 * 80;
const TYPE_START: i64 = PROMPT_CELL + 2;
const TYPE_END: i64 = 29 * 80;
const CLOCK_CELL: i64 = 8 * 80 + 13;

/// Text attribute: fg/bg one trit per channel (−1 = 0 %, 0 = 50 %, +1 = 100 %).
pub fn attr(fg: [i64; 3], bg: [i64; 3]) -> i64 {
    let t: Vec<Trit> = fg.iter().chain(bg.iter()).map(|&v| Trit::from_i64(v)).collect();
    Tryte::from_trits(&[&t[..], &[Trit::Z; 3]].concat()).value()
}

const WHITE: [i64; 3] = [1, 1, 1];
const BLACK: [i64; 3] = [-1, -1, -1];

fn a_title() -> i64 {
    attr(WHITE, [-1, -1, 1])
}
fn a_body() -> i64 {
    attr(WHITE, BLACK)
}
fn a_yellow() -> i64 {
    attr([1, 1, -1], BLACK)
}
fn a_cyan() -> i64 {
    attr([-1, 1, 1], BLACK)
}
fn a_green() -> i64 {
    attr([-1, 1, -1], BLACK)
}
fn a_fault() -> i64 {
    attr(WHITE, [1, -1, -1])
}

/// HID usage → character, unshifted then shifted (57 entries each).
fn keymap() -> Vec<i64> {
    let mut plain = vec![0i64; 57];
    let mut shift = vec![0i64; 57];
    for (i, c) in ('a'..='z').enumerate() {
        plain[4 + i] = c as i64;
        shift[4 + i] = c.to_ascii_uppercase() as i64;
    }
    for (i, (p, s)) in "1234567890".chars().zip("!@#$%^&*()".chars()).enumerate() {
        plain[30 + i] = p as i64;
        shift[30 + i] = s as i64;
    }
    let rest = [
        (44, ' ', ' '),
        (45, '-', '_'),
        (46, '=', '+'),
        (47, '[', '{'),
        (48, ']', '}'),
        (49, '\\', '|'),
        (51, ';', ':'),
        (52, '\'', '"'),
        (53, '`', '~'),
        (54, ',', '<'),
        (55, '.', '>'),
        (56, '/', '?'),
    ];
    for (i, p, s) in rest {
        plain[i] = p as i64;
        shift[i] = s as i64;
    }
    [plain, shift].concat()
}

fn puts_at(a: &mut Asm, label: &str, cell: i64, attr: i64) {
    a.la(1, label).subq(1, 1, 1).li(2, cell).li(3, attr).call("puts");
}

/// `r5 := FB + 3·reg` (address of a text cell).
fn cell_addr(a: &mut Asm, reg: i64) {
    a.add(5, reg, reg).add(5, 5, reg).add(5, 5, FB);
}

/// Assemble the main program (at `ENTRY`).
pub fn program() -> Asm {
    let mut a = Asm::new(ENTRY);

    // ---- entry ---------------------------------------------------------
    a.label("entry");
    a.li(13, STACK_TOP);
    a.li(FB, FB_BASE).li(KBD, 108).li(SER, 162).li(DISP, 243);
    a.li(ATTR, a_green()).li(SHIFT, 0);
    a.li(1, ITB).mtc(cr::ITB, 1);
    a.la(1, "s_banner").subq(1, 1, 1).call("serial_puts");
    a.call("draw_screen");

    // ---- main loop -----------------------------------------------------
    a.label("main");
    a.in_port(1, 81).li(2, CLOCK_CELL).li(3, a_body()).call("print_num");
    a.inp(1, KBD, 1).cmpq(1, 0).brq(cond::EQ, "check_serial");
    a.inp(1, KBD, 0).cmpq(1, 0).brq(cond::LT, "key_release");
    a.cmpi(1, 225).brq(cond::EQ, "shift_on");
    a.cmpi(1, 229).brq(cond::EQ, "shift_on");
    a.cmpi(1, 43).brq(cond::EQ, "do_gfx");
    a.cmpi(1, 41).brq(cond::EQ, "do_redraw");
    a.cmpi(1, 40).brq(cond::EQ, "key_enter");
    a.cmpi(1, 42).brq(cond::EQ, "key_bs");
    a.cmpi(1, 56).brq(cond::GT, "main");
    a.la(3, "keymap").subq(3, 3, 1).add(3, 3, 1);
    a.cmpq(SHIFT, 0).brq(cond::EQ, "km_lookup").addi(3, 3, 57);
    a.label("km_lookup").ldt(1, 3, 0).cmpq(1, 0).brq(cond::EQ, "main");
    a.call("echo").jmp("main");
    a.label("key_enter").li(1, 10).call("echo").jmp("main");
    a.label("key_bs").li(1, 8).call("echo").jmp("main");
    a.label("shift_on").li(SHIFT, 1).jmp("main");
    a.label("key_release").sub(1, 0, 1);
    a.cmpi(1, 225).brq(cond::EQ, "shift_off");
    a.cmpi(1, 229).brq(cond::EQ, "shift_off");
    a.jmp("main");
    a.label("shift_off").li(SHIFT, 0).jmp("main");
    a.label("do_gfx").call("graphics").call("draw_screen").jmp("main");
    a.label("do_redraw").call("draw_screen").jmp("main");
    a.label("check_serial");
    a.inp(1, SER, 1).cmpq(1, 0).brq(cond::EQ, "main");
    a.inp(1, SER, 0).call("echo").jmp("main");

    // ---- draw_screen ---------------------------------------------------
    a.label("draw_screen").push_reg(12);
    a.li(1, -1).out(1, DISP, 0);
    a.mov(5, FB).li(6, 30 * 80).li(7, 32).li(8, a_body());
    a.label("ds_fill").stt(7, 5, 0).stt(8, 5, 1).addq(5, 5, 3).subq(6, 6, 1).brq(cond::GT, "ds_fill");
    a.mov(5, FB).li(6, 80).li(8, a_title());
    a.label("ds_bar").stt(7, 5, 0).stt(8, 5, 1).addq(5, 5, 3).subq(6, 6, 1).brq(cond::GT, "ds_bar");
    puts_at(&mut a, "s_title", 2, a_title());
    puts_at(&mut a, "s_sub", 2 * 80 + 2, a_yellow());
    puts_at(&mut a, "s_l4", 4 * 80 + 2, a_body());
    puts_at(&mut a, "s_l5", 5 * 80 + 2, a_body());
    puts_at(&mut a, "s_ram", 7 * 80 + 2, a_cyan());
    a.in_port(1, 1).li(3, a_body()).call("print_num");
    a.la(1, "s_ram2").subq(1, 1, 1).li(3, a_cyan()).call("puts");
    puts_at(&mut a, "s_clock", 8 * 80 + 2, a_cyan());
    puts_at(&mut a, "s_pal", 10 * 80 + 2, a_cyan());
    // 27 colour swatches: bg field = i (weight 27), two cells each.
    a.li(6, -13).li(7, 32);
    a.label("ds_pal").alui(alu::MUL, 3, 6, 27);
    cell_addr(&mut a, 2);
    a.stt(7, 5, 0).stt(3, 5, 1).stt(7, 5, 3).stt(3, 5, 4).addq(2, 2, 2);
    a.addq(6, 6, 1).cmpq(6, 13).brq(cond::LE, "ds_pal");
    puts_at(&mut a, "s_keys", 12 * 80 + 2, a_cyan());
    a.call("clear_typing");
    a.pop_reg(12).ret();

    // ---- clear_typing: blank rows 14..29, prompt, cursor -----------------
    a.label("clear_typing");
    a.li(2, PROMPT_CELL);
    cell_addr(&mut a, 2);
    a.li(6, TYPE_END - PROMPT_CELL + 80).li(7, 32).li(8, a_body());
    a.label("ct_loop").stt(7, 5, 0).stt(8, 5, 1).addq(5, 5, 3).subq(6, 6, 1).brq(cond::GT, "ct_loop");
    cell_addr(&mut a, 2);
    a.li(7, '>' as i64).li(8, a_yellow()).stt(7, 5, 0).stt(8, 5, 1);
    a.li(CUR, TYPE_START).out(CUR, DISP, 7);
    a.ret();

    // ---- puts: r1 = string (tryte addr), r2 = cell, r3 = attr ------------
    //      returns r2 = next cell; clobbers r4, r5.
    a.label("puts");
    cell_addr(&mut a, 2);
    a.label("puts_loop").ldt(4, 1, 0).cmpq(4, 0).brq(cond::EQ, "puts_done");
    a.stt(4, 5, 0).stt(3, 5, 1).addq(5, 5, 3).addq(1, 1, 1).addq(2, 2, 1).jmp("puts_loop");
    a.label("puts_done").ret();

    // ---- print_num: r1 = value, r2 = cell, r3 = attr ---------------------
    //      returns r2 = next cell; clobbers r1, r4..r7.
    a.label("print_num");
    cell_addr(&mut a, 2);
    a.cmpq(1, 0).brq(cond::GE, "pn_pos");
    a.li(4, '-' as i64).stt(4, 5, 0).stt(3, 5, 1).addq(5, 5, 3).addq(2, 2, 1).sub(1, 0, 1);
    a.label("pn_pos").li(6, 0);
    a.label("pn_div").alui(alu::MOD, 7, 1, 10).alui(alu::DIV, 1, 1, 10).push_reg(7).addq(6, 6, 1);
    a.cmpq(1, 0).brq(cond::GT, "pn_div");
    a.label("pn_out").pop_reg(7).addi(7, 7, '0' as i64);
    a.stt(7, 5, 0).stt(3, 5, 1).addq(5, 5, 3).addq(2, 2, 1).subq(6, 6, 1).brq(cond::GT, "pn_out");
    a.ret();

    // ---- serial_puts: r1 = string -----------------------------------------
    a.label("serial_puts").ldt(4, 1, 0).cmpq(4, 0).brq(cond::EQ, "sp_done");
    a.out(4, SER, 0).addq(1, 1, 1).jmp("serial_puts");
    a.label("sp_done").ret();

    // ---- echo: r1 = character ---------------------------------------------
    a.label("echo").push_reg(12);
    a.cmpq(1, 10).brq(cond::EQ, "echo_nl");
    a.cmpq(1, 13).brq(cond::EQ, "echo_nl");
    a.cmpq(1, 8).brq(cond::EQ, "echo_bs");
    a.cmpi(1, 127).brq(cond::EQ, "echo_bs");
    a.out(1, SER, 0);
    cell_addr(&mut a, CUR);
    a.stt(1, 5, 0).stt(ATTR, 5, 1).addq(CUR, CUR, 1).jmp("echo_wrap");
    a.label("echo_nl").li(2, 10).out(2, SER, 0);
    a.alui(alu::DIV, 2, CUR, 80).addq(2, 2, 1).alui(alu::MUL, CUR, 2, 80).addq(CUR, CUR, 2);
    a.jmp("echo_wrap");
    a.label("echo_bs").li(2, TYPE_START).cmp(CUR, 2).brq(cond::LE, "echo_cur");
    a.subq(CUR, CUR, 1);
    cell_addr(&mut a, CUR);
    a.li(4, 32).stt(4, 5, 0).li(2, 8).out(2, SER, 0).jmp("echo_cur");
    a.label("echo_wrap").li(2, TYPE_END).cmp(CUR, 2).brq(cond::LT, "echo_cur");
    a.call("clear_typing");
    a.label("echo_cur").out(CUR, DISP, 7).pop_reg(12).ret();

    // ---- graphics: sweep the 3^9-colour cube, wait for a key ---------------
    a.label("graphics").out(0, DISP, 0).mov(9, FB).li(2, 0);
    a.label("gy").alui(alu::MUL, 5, 2, 27).alui(alu::DIV, 5, 5, 480).subq(5, 5, 13).alui(alu::MUL, 5, 5, 27);
    a.li(3, 0);
    a.label("gx").alui(alu::MUL, 6, 3, 27).alui(alu::DIV, 6, 6, 640).subq(6, 6, 13).alui(alu::MUL, 6, 6, 729);
    a.add(7, 3, 2).alui(alu::MUL, 7, 7, 27).alui(alu::DIV, 7, 7, 1119).subq(7, 7, 13);
    a.add(6, 6, 5).add(6, 6, 7).stt(6, 9, 0).addq(9, 9, 1);
    a.addq(3, 3, 1).cmpi(3, 640).brq(cond::LT, "gx");
    a.addq(2, 2, 1).cmpi(2, 480).brq(cond::LT, "gy");
    a.label("gwait").inp(1, SER, 1).cmpq(1, 0).brq(cond::GT, "gserial");
    a.inp(1, KBD, 1).cmpq(1, 0).brq(cond::EQ, "gwait");
    a.inp(1, KBD, 0).cmpq(1, 0).brq(cond::LE, "gwait");
    a.ret();
    a.label("gserial").inp(1, SER, 0).ret();

    // ---- fault handler -------------------------------------------------------
    a.label("fault").li(13, STACK_TOP).li(FB, FB_BASE).li(1, -1).li(2, 243).out(1, 2, 0);
    a.mfc(1, cr::FCAUSE).ext(1, 1, 23, 5).push_reg(1);
    puts_at(&mut a, "s_fault", 29 * 80 + 2, a_fault());
    a.pop_reg(1).call("print_num");
    a.label("fault_halt").hlt().jmp("fault_halt");

    // ---- data ------------------------------------------------------------------
    let strings = [
        ("s_banner", "Setun-386 demo ROM ready. Type here or in the window.\n"),
        ("s_title", "SETUN-386 \u{b7} balanced ternary computer"),
        ("s_sub", "Every size is a power of 3.  After Brusentsov's Setun, MSU 1958/1970."),
        ("s_l4", "tryte = 9 trits    word = 27 trits    address space = 3^27 trytes"),
        ("s_l5", "page = 3^9 trytes  27 registers       rings: -1 user, 0 drivers, +1 kernel"),
        ("s_ram", "RAM: "),
        ("s_ram2", " pages \u{d7} 3^9 trytes"),
        ("s_clock", "Unix time: "),
        ("s_pal", "27 text colours: "),
        ("s_keys", "Type to echo (keyboard or serial).  Tab = 3^9-colour graphics.  Esc = redraw."),
        ("s_fault", "CPU fault, vector "),
    ];
    for (l, s) in strings {
        a.label(l).string(s);
    }
    a.label("keymap").trytes(&keymap());
    a
}

/// The full 3-frame ROM image (3^10 trytes).
pub fn rom() -> Vec<i16> {
    let (words, labels) = program().assemble_with_labels().expect("demo ROM assembles");
    let rom_start = frame_start(ROM_FIRST_FRAME);
    let mut rom = vec![0i16; (ROM_FRAMES * PAGE) as usize];
    let place = |rom: &mut Vec<i16>, centre: i64, words: &[Word]| {
        let first = (centre - 1 - rom_start) as usize;
        let t = words_to_trytes(words);
        rom[first..first + t.len()].copy_from_slice(&t);
    };
    let end = ENTRY + 3 * words.len() as i64;
    assert!(end < ITB - 363, "program overlaps the vector table");
    place(&mut rom, ENTRY, &words);
    let fault = Word::new(labels["fault"]).unwrap();
    place(&mut rom, ITB - 3 * 121, &vec![fault; 243]);
    rom
}

/// A machine with the demo ROM loaded, ready to boot.
pub fn machine(ram_frames: i64, unix_time_s: i64) -> Machine {
    let mut m = Machine::new(ram_frames, unix_time_s);
    m.load_rom(&rom());
    m
}

/// Read the text screen as strings (one per row), for tests and headless runs.
pub fn screen_text(m: &Machine) -> Vec<String> {
    let fb = m.mem.framebuffer();
    (0..30)
        .map(|row| {
            (0..80)
                .map(|col| {
                    let c = fb[3 * (row * 80 + col)] as i64;
                    u32::try_from(c).ok().and_then(char::from_u32).filter(|c| *c >= ' ').unwrap_or(' ')
                })
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn booted() -> Machine {
        let mut m = machine(729, 1_790_000_000);
        m.run(200_000);
        m
    }

    #[test]
    fn boots_to_the_text_screen() {
        let mut m = booted();
        let s = screen_text(&m);
        assert!(s[0].contains("SETUN-386"), "{:?}", s[0]);
        assert!(s[7].contains("RAM: 729 pages"), "{:?}", s[7]);
        assert!(s[8].starts_with("  Unix time: 17900000"), "{:?}", s[8]);
        assert_eq!(s[14], ">");
        assert_eq!(m.dev.serial.take_output(), "Setun-386 demo ROM ready. Type here or in the window.\n");
        assert_eq!(m.dev.display.mode(), -1);
    }

    #[test]
    fn keyboard_and_serial_echo() {
        let mut m = booted();
        for (code, shift) in [(11, true), (8, false), (15, false), (15, false), (18, false), (44, false)] {
            if shift {
                m.dev.key(225, true);
            }
            m.dev.key(code, true);
            m.dev.key(code, false);
            if shift {
                m.dev.key(225, false);
            }
            m.run(20_000);
        }
        m.dev.serial_input("Ж!");
        m.run(50_000);
        let s = screen_text(&m);
        assert_eq!(s[14], "> Hello Ж!");
        assert!(m.dev.serial.take_output().ends_with("Hello Ж!"));
        // Backspace and Enter.
        m.dev.key(42, true);
        m.run(20_000);
        m.dev.key(40, true);
        m.run(20_000);
        m.dev.serial_input("ok");
        m.run(40_000);
        let s = screen_text(&m);
        assert_eq!(s[14], "> Hello Ж");
        assert_eq!(s[15], "  ok");
    }

    #[test]
    fn tab_shows_graphics_and_a_key_returns() {
        let mut m = booted();
        m.dev.key(43, true);
        m.run(8_000_000);
        assert_eq!(m.dev.display.mode(), 0);
        let fb = m.mem.framebuffer();
        let px = |x: usize, y: usize| fb[y * 640 + x] as i64;
        assert_eq!(Tryte::new(px(0, 0)).unwrap().to_b27(), "mmm", "top-left is black");
        assert_eq!(Tryte::new(px(639, 479)).unwrap().to_b27(), "MMM", "bottom-right is white");
        m.dev.key(44, true);
        m.run(200_000);
        assert_eq!(m.dev.display.mode(), -1);
        assert!(screen_text(&m)[0].contains("SETUN-386"));
    }

    #[test]
    fn faults_are_reported_on_screen() {
        let mut m = booted();
        // Jump somewhere illegal: all-zero RAM executes as ILL.
        m.cpu.pc = Word::new(3 * 1000).unwrap();
        m.run(50_000);
        assert!(screen_text(&m)[29].contains("CPU fault, vector -1"), "{:?}", screen_text(&m)[29]);
    }
}
