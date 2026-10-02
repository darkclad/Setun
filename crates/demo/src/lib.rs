//! Demo ROM for the Setun-26 front ends, written in assembly (`demo.s`).
//!
//! Boots from the reset vector (paging off, ring +1) and shows:
//! * a text-mode screen with machine facts, the 27 text colours, the RAM size
//!   (from `SYSCTL`) and a live Unix clock (from the RTC);
//! * keyboard and serial echo into a typing area (with Shift, Enter,
//!   Backspace), mirrored to the serial console;
//! * Tab: a 640×480 graphics screen sweeping through the 3^9-colour space;
//!   any key returns;
//! * a fault handler that reports CPU faults on the bottom line.

use setun_core::Machine;
use std::sync::OnceLock;

/// The demo ROM's assembly source.
pub const SOURCE: &str = include_str!("demo.s");

/// The full 3-frame ROM image (3^10 trytes), assembled once.
pub fn rom() -> Vec<i16> {
    static ROM: OnceLock<Vec<i16>> = OnceLock::new();
    ROM.get_or_init(|| {
        let p = setun_asm::text::assemble(SOURCE).unwrap_or_else(|e| panic!("demo.s: {e}"));
        setun_asm::rom_image(&p.segments).unwrap_or_else(|e| panic!("demo.s: {e}"))
    })
    .clone()
}

/// A machine with the demo ROM loaded, ready to boot.
pub fn machine(ram_frames: i64, unix_time_s: i64) -> Machine {
    let mut m = Machine::new(ram_frames, unix_time_s);
    m.load_rom(&rom());
    m
}

/// Read the text screen as strings (one per row), for tests and headless runs.
/// An overlined character comes out as the character and U+0305, as on serial.
pub fn screen_text(m: &Machine) -> Vec<String> {
    let fb = m.mem.framebuffer();
    (0..30)
        .map(|row| {
            (0..80)
                .map(|col| {
                    let c = fb[3 * (row * 80 + col)] as i64;
                    let ch = |c: i64| u32::try_from(c).ok().and_then(char::from_u32).filter(|c| *c > ' ');
                    match c {
                        c if c < 0 => ch(-c).map_or(" ".into(), |ch| format!("{ch}\u{305}")),
                        c => ch(c).unwrap_or(' ').to_string(),
                    }
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
    use ternary::{Tryte, Word};

    #[test]
    fn source_layout() {
        let p = setun_asm::text::assemble(SOURCE).unwrap();
        let (code, table) = (&p.segments[0], &p.segments[1]);
        assert_eq!(code.origin, -59_049, "entry at the reset PC");
        assert!(code.origin + 3 * code.words.len() as i64 <= table.origin, "code stays below the vector table");
        assert_eq!(table.words.len(), 243);
        assert!(table.words.iter().all(|w| w.value() == p.symbols["fault"]));
    }

    fn booted() -> Machine {
        let mut m = machine(729, 1_790_000_000);
        m.run(200_000);
        m
    }

    #[test]
    fn boots_to_the_text_screen() {
        let mut m = booted();
        let s = screen_text(&m);
        assert!(s[0].contains("SETUN-11̅"), "{:?}", s[0]);
        assert!(s[7].contains("RAM: 729 pages"), "{:?}", s[7]);
        assert!(s[8].starts_with("  Unix time: 17900000"), "{:?}", s[8]);
        assert_eq!(s[14], ">");
        assert_eq!(m.dev.serial.take_output(), "Setun-11̅ demo ROM ready. Type here or in the window.\n");
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
        assert_eq!(Tryte::new(px(0, 0)).unwrap().to_b27(), "D\u{305}D\u{305}D\u{305}", "top-left is black");
        assert_eq!(Tryte::new(px(639, 479)).unwrap().to_b27(), "DDD", "bottom-right is white");
        m.dev.key(44, true);
        m.run(200_000);
        assert_eq!(m.dev.display.mode(), -1);
        assert!(screen_text(&m)[0].contains("SETUN-11̅"));
    }

    #[test]
    fn faults_are_reported_on_screen() {
        let mut m = booted();
        // Jump somewhere illegal: all-zero RAM executes as ILL.
        m.cpu.pc = Word::new(3 * 1000).unwrap();
        m.run(50_000);
        assert!(screen_text(&m)[29].contains("CPU fault, vector -1"), "{:?}", screen_text(&m)[29]);
    }

    #[test]
    fn disassembly_reassembles_the_whole_rom() {
        let p = setun_asm::text::assemble(SOURCE).unwrap();
        for s in &p.segments {
            let text = setun_asm::disasm::disasm_source(&s.words, s.origin);
            let again = setun_asm::text::assemble(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
            assert_eq!(again.segments[0].words, s.words);
        }
    }
}
