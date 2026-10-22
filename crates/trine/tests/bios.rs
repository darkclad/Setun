//! The BIOS (bios/bios.tri) on the emulator.

use trine::{bios_rom, build_source, run_machine, Options, Outcome};

fn bios() -> Vec<i16> {
    bios_rom().unwrap_or_else(|e| panic!("BIOS does not compile: {e}"))
}

fn screen(m: &setun_core::Machine) -> Vec<String> {
    let fb = m.mem.framebuffer();
    (0..30)
        .map(|r| {
            (0..80)
                .map(|c| {
                    // an overlined character is its negated code point: show it with U+0305
                    let v = fb[3 * (r * 80 + c)] as i64;
                    let ch = u32::try_from(v.abs()).ok().and_then(char::from_u32).filter(|c| *c > ' ');
                    match ch {
                        Some(ch) if v < 0 => format!("{ch}\u{305}"),
                        Some(ch) => ch.to_string(),
                        None => " ".into(),
                    }
                })
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

#[test]
fn boots_a_disk() {
    let src = include_str!("../../../bios/boot_hello.tri");
    let disk = build_source("boot_hello.tri", src, None, &Options::boot()).unwrap_or_else(|e| panic!("{e}")).image;
    assert_eq!(disk.len(), 19_683, "one sector");
    let r = run_machine(&bios(), Some(&disk), 30_000_000, "");
    assert_eq!(r.outcome, Outcome::Exit(42), "serial:\n{}", r.serial);
    assert!(r.serial.contains("SETUN-11̅ BIOS 1.0"), "{}", r.serial);
    assert!(r.serial.contains("729 pages × 3^9 trytes = 14348907 trytes   test OK"), "{}", r.serial);
    assert!(r.serial.contains("Clock   2026-09-21 14:13:20 UTC"), "{}", r.serial);
    assert!(r.serial.contains("Disk 0  1 sector (19683 trytes)"), "{}", r.serial);
    assert!(r.serial.contains("disk 0 ... booting"), "{}", r.serial);
    assert!(r.serial.contains("Hello from the boot sector! Booted from disk 0, 729 pages of RAM."), "{}", r.serial);
    let s = screen(&r.machine);
    assert!(s[0].contains("SETUN-11̅ BIOS 1.0"), "{:?}", s[0]);
}

#[test]
fn banner_shows_setun_11_overlined() {
    let r = run_machine(&bios(), None, 30_000_000, "off\n");
    let banner: String = r.serial.lines().take(8).collect::<Vec<_>>().join("\n");
    assert!(banner.contains("SETUN-11\u{305} BIOS"), "{banner}");
    assert!(banner.contains("Setun-11\u{305}, ISA v0.8, ring"), "{banner}");
    // the old names are gone
    assert!(!r.serial.contains("386") && !r.serial.contains("387"), "{}", r.serial);
    // not the base-3 alias, and never a plain 11 without the bar
    for bad in ["Setun-1T", "SETUN-1T"] {
        assert!(!r.serial.contains(bad), "{bad} in {}", r.serial);
    }
    for (i, _) in r.serial.match_indices("ETUN-11").chain(r.serial.match_indices("etun-11")) {
        assert_eq!(r.serial[i + 7..].chars().next(), Some('\u{305}'), "11 without an overline at {i}: {}", r.serial);
    }
    // on the screen the second 1 is one cell holding −'1', which draws with a bar
    let fb = r.machine.mem.framebuffer();
    let row0: Vec<i64> = (0..80).map(|c| fb[3 * c] as i64).collect();
    let at = row0.windows(7).position(|w| w == [83, 69, 84, 85, 78, 45, 49]).expect("SETUN-1 on row 0");
    assert_eq!(row0[at + 7], -49, "{row0:?}");
}

#[test]
fn monitor_without_a_disk() {
    let input = "help\ntime\ndump -59049 2\nfrob\nboot\noff\n";
    let r = run_machine(&bios(), None, 60_000_000, input);
    let out = &r.serial;
    assert!(out.contains("disk 0 ... no disk"), "{out}");
    assert!(out.contains("Monitor ready; type help."), "{out}");
    assert!(out.contains("dump A [N]"), "help listed: {out}");
    assert!(out.contains("-59049  "), "dump printed the reset vector: {out}");
    assert!(out.contains("unknown command; type help"), "{out}");
    assert!(r.machine.powered_off, "`off` powered the machine down:\n{out}");
}

#[test]
fn refuses_a_disk_without_the_boot_signature() {
    let blank = vec![0i16; 19_683];
    let r = run_machine(&bios(), Some(&blank), 20_000_000, "off\n");
    assert!(r.serial.contains("not bootable (no boot signature)"), "{}", r.serial);
    assert!(r.machine.powered_off);
}

#[test]
fn keyboard_input_reaches_the_monitor() {
    let mut m = setun_core::Machine::new(729, 1_790_000_000);
    m.load_rom(&bios());
    m.run(3_000_000);
    // "off" + Enter typed on the keyboard (HID usages).
    for code in [18, 9, 9, 40] {
        m.dev.key(code, true);
        m.run(200_000);
        m.dev.key(code, false);
        m.run(200_000);
    }
    m.run(2_000_000);
    assert!(m.powered_off, "{}", m.dev.serial.take_output());
}
