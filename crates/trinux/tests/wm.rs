//! Trinux K8: the window system, driven like a user would: the mouse and
//! keyboard devices of the emulated machine, and the framebuffer read back
//! (including the terminal window's text, matched glyph by glyph).

use setun_core::Machine;

const W: usize = 640;
// Colours, as trwm and the apps define them (r*729 + g*27 + b).
const fn rgb(r: i64, g: i64, b: i64) -> i64 {
    r * 729 + g * 27 + b
}
const DESK: i64 = rgb(-6, -1, 3);
const PANEL: i64 = rgb(-9, -9, -8);
const TITLE_ON: i64 = rgb(-10, -6, 8);
const TITLE_OFF: i64 = rgb(-4, -4, -3);
const RED: i64 = rgb(13, -13, -13);
const WHITE: i64 = 9841;

// Window geometry (trwm cascades windows in creation order).
const TERM_X: i64 = 24;
const TERM_Y: i64 = 42;
const PAINT_X: i64 = 52;
const PAINT_Y: i64 = 66;

struct Session {
    m: Machine,
    out: String,
    mark: usize, // where the output stood when the last action began
}

impl Session {
    fn boot() -> Session {
        Session::boot_as("root", "root")
    }

    /// Boot and log in as `user` with `password`.
    fn boot_as(user: &str, password: &str) -> Session {
        let disk = trinux::build_disk(&trinux::kernel_dir()).unwrap_or_else(|e| panic!("{e}"));
        let mut s = Session { m: trinux::boot_trytes(&disk.trytes, 729, 0, ""), out: String::new(), mark: 0 };
        s.wait_for("login: ", 60);
        s.m.dev.serial_input(&format!("{user}\n"));
        s.wait_for("Password: ", 30);
        s.m.dev.serial_input(&format!("{password}\n"));
        s.wait_for("$ ", 60);
        s
    }

    fn run(&mut self, millions: u64) {
        for _ in 0..millions {
            self.m.run(1_000_000);
            self.out.push_str(&self.m.dev.serial.take_output());
        }
    }

    /// Run until the serial output contains `what` (at most `max` million steps).
    fn wait_for(&mut self, what: &str, max: u64) {
        let start = self.out.len().saturating_sub(what.len());
        for _ in 0..max {
            if self.out[start.min(self.out.len())..].contains(what) || self.out.ends_with(what) {
                return;
            }
            self.run(1);
        }
        panic!("timed out waiting for {what:?}:\n{}", self.out);
    }

    /// Wait for text to appear after the start of the last action.
    fn expect(&mut self, what: &str, max: u64) {
        for _ in 0..max {
            if self.out[self.mark..].contains(what) {
                return;
            }
            self.run(1);
        }
        panic!("timed out waiting for {what:?}:\n{}", self.out);
    }

    fn console(&mut self, line: &str) {
        self.mark = self.out.len();
        self.m.dev.serial_input(&format!("{line}\n"));
    }

    fn pixel(&self, x: i64, y: i64) -> i64 {
        self.m.mem.framebuffer()[y as usize * W + x as usize] as i64
    }

    fn click(&mut self, x: i64, y: i64) {
        self.mark = self.out.len();
        self.m.dev.mouse_abs(x, y);
        self.run(2);
        self.m.dev.mouse_buttons(true, false, false);
        self.run(2);
        self.m.dev.mouse_buttons(false, false, false);
        self.run(4);
    }

    fn drag(&mut self, from: (i64, i64), to: (i64, i64)) {
        self.mark = self.out.len();
        self.m.dev.mouse_abs(from.0, from.1);
        self.run(2);
        self.m.dev.mouse_buttons(true, false, false);
        self.run(2);
        self.m.dev.mouse_abs(to.0, to.1);
        self.run(3);
        self.m.dev.mouse_buttons(false, false, false);
        self.run(4);
    }

    fn key(&mut self, code: i64, shift: bool, ctrl: bool) {
        if shift {
            self.m.dev.key(225, true);
        }
        if ctrl {
            self.m.dev.key(224, true);
        }
        self.m.dev.key(code, true);
        self.m.dev.key(code, false);
        if ctrl {
            self.m.dev.key(224, false);
        }
        if shift {
            self.m.dev.key(225, false);
        }
        self.run(1);
    }

    fn type_text(&mut self, s: &str) {
        for c in s.chars() {
            let (code, shift) = hid(c);
            self.key(code, shift, false);
        }
    }

    /// The terminal window's text, recognised from the screen (`rows` rows).
    /// Letters drawn alike (Latin A, Cyrillic А) read as the first in
    /// `glyph_order`; compare with `canonical`.
    fn term_text(&self, rows: usize) -> Vec<String> {
        let font = trinux::font8x8_table();
        (0..rows)
            .map(|r| {
                (0..76)
                    .map(|c| {
                        let (x0, y0) = (TERM_X + 8 * c as i64, TERM_Y + 9 * r as i64);
                        let cell: Vec<i16> = (0..8)
                            .map(|y| (0..8).map(|x| (self.pixel(x0 + x, y0 + y) == WHITE) as i16 * (1 << x)).sum())
                            .collect();
                        glyph_order().find(|&i| font[i * 8..i * 8 + 8] == cell[..]).map(trinux::font_char).unwrap_or('?')
                    })
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }
}

/// Font glyphs in the order the screen reader tries them: ASCII, Latin-1,
/// then Cyrillic.
fn glyph_order() -> impl Iterator<Item = usize> {
    (32..127).chain(161..256).chain(256..trinux::FONT_GLYPHS)
}

/// How `term_text` reads `s` back: each character as the first glyph that
/// looks the same, and characters past the font as '?'.
fn canonical(s: &str) -> String {
    let font = trinux::font8x8_table();
    s.chars()
        .map(|c| match trinux::font_index(c as u32) {
            Some(i) => {
                let g = &font[i * 8..i * 8 + 8];
                glyph_order().find(|&j| &font[j * 8..j * 8 + 8] == g).map(trinux::font_char).unwrap_or('?')
            }
            None => '?',
        })
        .collect()
}

/// USB HID usage code for a character, and whether it needs shift.
fn hid(c: char) -> (i64, bool) {
    match c {
        'a'..='z' => (4 + (c as i64 - 'a' as i64), false),
        'A'..='Z' => (4 + (c as i64 - 'A' as i64), true),
        '1'..='9' => (30 + (c as i64 - '1' as i64), false),
        '0' => (39, false),
        '\n' => (40, false),
        ' ' => (44, false),
        '-' => (45, false),
        '=' => (46, false),
        '/' => (56, false),
        '.' => (55, false),
        '>' => (55, true),
        '|' => (49, true),
        '$' => (33, true),
        '?' => (56, true),
        '\'' => (52, false),
        _ => panic!("no key for {c:?}"),
    }
}

#[test]
fn k8_window_system_end_to_end() {
    let mut s = Session::boot();
    s.console("trwm term paint &");
    s.wait_for("trwm: window 1 'Paint'", 200);
    s.run(40);
    // the desktop, the panel, and the focused Paint window's title bar
    assert_eq!(s.pixel(600, 460), DESK);
    assert_eq!(s.pixel(2, 2), PANEL);
    assert_eq!(s.pixel(PAINT_X + 120, PAINT_Y - 8), TITLE_ON);
    assert_eq!(s.pixel(TERM_X + 500, TERM_Y - 8), TITLE_OFF);
    // painting: a drag across the canvas leaves red dots (red is preselected)
    s.drag((PAINT_X + 100, PAINT_Y + 100), (PAINT_X + 140, PAINT_Y + 100));
    assert_eq!(s.pixel(PAINT_X + 100, PAINT_Y + 100), RED, "the dot where the drag started");
    assert_eq!(s.pixel(PAINT_X + 140, PAINT_Y + 100), RED, "the dot where it ended");
    assert_eq!(s.pixel(PAINT_X + 120, PAINT_Y + 130), WHITE, "untouched canvas");
    // raise the terminal by its title bar and type into its shell
    s.click(TERM_X + 500, TERM_Y - 8);
    assert_eq!(s.pixel(TERM_X + 500, TERM_Y - 8), TITLE_ON);
    s.type_text("echo hi > /tmp/w\n");
    s.run(60);
    s.type_text("echo $$ | wc -c\n");
    s.run(60);
    let text = s.term_text(6);
    assert!(text.iter().any(|l| l == "$ echo hi > /tmp/w"), "terminal: {text:#?}");
    assert!(text.iter().any(|l| l.trim() == "2"), "the pipeline's output in the window: {text:#?}");
    // the console sees what the window's shell wrote
    s.console("cat /tmp/w");
    s.expect("hi\n", 60);
    // ^C in the window interrupts its foreground job (the pty's line discipline)
    s.type_text("sleep 100\n");
    s.run(30);
    s.key(6, false, true); // ^C
    s.run(40);
    s.type_text("echo after\n");
    s.run(40);
    let text = s.term_text(12);
    assert!(text.iter().any(|l| l == "$ sleep 100"), "{text:#?}");
    assert!(text.iter().any(|l| l == "^C"), "{text:#?}");
    assert!(text.iter().any(|l| l == "after"), "{text:#?}");
    // drag the terminal down by its title bar: Paint, hidden behind it, shows again
    s.drag((TERM_X + 500, TERM_Y - 8), (TERM_X + 500, TERM_Y - 8 + 250));
    s.run(20);
    assert_eq!(s.pixel(TERM_X + 500, TERM_Y - 8 + 250), TITLE_ON, "the moved title bar");
    assert_eq!(s.pixel(PAINT_X + 120, PAINT_Y - 8), TITLE_OFF, "Paint's title bar, uncovered");
    assert_eq!(s.pixel(PAINT_X + 100, PAINT_Y + 100), RED, "Paint kept its pixels");
    // close Paint with its close box, then end the session with [Exit]
    s.click(PAINT_X + 243 - 7, PAINT_Y - 8);
    s.expect("trwm: window 1 closed", 60);
    s.click(340, 8);
    s.expect("trwm: exit", 60);
    s.run(30);
    assert_eq!(s.m.dev.display.mode(), -1, "the console is back in text mode");
    s.console("echo console-ok");
    s.expect("console-ok\n", 60);
}

// HID usage codes used below
const ALT: i64 = 226;
const SHIFT: i64 = 225;
const CAPS: i64 = 57;

impl Session {
    /// Press keys one at a time (no modifiers).
    fn keys(&mut self, codes: &[i64]) {
        for &c in codes {
            self.key(c, false, false);
        }
    }

    /// Alt+Shift, pressing `first` before the other one.
    fn switch_layout(&mut self, first: i64) {
        let second = if first == ALT { SHIFT } else { ALT };
        self.m.dev.key(first, true);
        self.m.dev.key(second, true);
        self.m.dev.key(second, false);
        self.m.dev.key(first, false);
        self.run(1);
    }
}

// g h b d t n on a US keyboard are п р и в е т in the Russian layout
const PRIVET: [i64; 6] = [10, 11, 5, 7, 23, 17];

#[test]
fn the_console_keyboard_has_a_russian_layout() {
    let mut s = Session::boot();
    s.mark = s.out.len();
    s.type_text("echo ");
    s.switch_layout(ALT);
    s.keys(&PRIVET);
    s.key(44, false, false);
    s.key(19, true, false); // Shift+P: З
    s.key(32, true, false); // Shift+3: №
    s.keys(&[CAPS, 10, CAPS]); // Caps Lock: П
    s.keys(&[53, 56]); // ` /: ё .
    s.m.dev.key(ALT, true); // Alt with a letter does not switch, and types the letter
    s.key(10, false, false);
    s.m.dev.key(ALT, false);
    s.switch_layout(SHIFT); // back to US, the other key first
    s.type_text(" ok\n");
    s.expect("echo привет З№Пё.п ok\nпривет З№Пё.п ok\n", 60);
    // Ctrl+letter is a control code by key position in either layout
    s.switch_layout(ALT);
    s.keys(&[4, 22, 7]); // ф ы в
    s.key(6, false, true); // Ctrl+C (the key under с)
    s.expect("фыв^C\n$ ", 60);
    // keys the layout has nothing for type nothing (F1), and Enter still works
    s.keys(&[58, 40]);
    s.run(10);
    s.switch_layout(ALT);
    s.console("echo back-in-us");
    s.expect("back-in-us\n", 60);
}

#[test]
fn a_normal_user_runs_the_window_system() {
    // login hands the console devices to whoever logs in
    let mut s = Session::boot_as("user", "user");
    s.console("ls -l /dev/fb0 /dev/mouse /dev/kbd");
    s.expect("crw-rw---- 1 user     user    ", 60);
    s.console("trwm clock &");
    s.wait_for("trwm: window 0 'Clock'", 200);
    s.run(40);
    assert_eq!(s.pixel(2, 2), PANEL, "the panel is drawn");
    s.click(340, 8); // [Exit]
    s.expect("trwm: exit", 60);
}

#[test]
fn arrow_keys_edit_the_shell_line() {
    let mut s = Session::boot();
    // the console: Left goes through the kernel's keymap as ESC [ D
    s.mark = s.out.len();
    s.type_text("echo helo");
    s.key(80, false, false); // Left
    s.type_text("l\n");
    s.expect("\nhello\n", 60);
    s.key(82, false, false); // Up: the line again
    s.type_text("\n");
    s.expect("\u{1b}[K\nhello\n", 60);
    // the terminal window: the keys come through trwm and the pty
    s.console("trwm term &");
    s.wait_for("trwm: window 0 'Terminal'", 200);
    s.run(40);
    s.click(TERM_X + 500, TERM_Y - 8);
    s.type_text("echo one\n");
    s.run(40);
    s.key(82, false, false); // Up
    s.key(80, false, false); // Left, Left: before "ne"
    s.key(80, false, false);
    s.type_text("x\n");
    s.run(60);
    let text = s.term_text(5);
    assert!(text.contains(&"oxne".to_string()), "{text:#?}");
}

#[test]
fn the_dead_key_types_overlined_digits() {
    const BACKQUOTE: i64 = 53;
    let mut s = Session::boot();
    s.mark = s.out.len();
    s.type_text("echo '0z"); // quoted: the shell runs `...` as a command
    s.keys(&[BACKQUOTE, 30]); // ` 1: an overlined 1 (the code point -'1')
    s.key(7, true, false); // D
    s.keys(&[BACKQUOTE, 44]); // ` Space: a plain backquote
    s.keys(&[BACKQUOTE, BACKQUOTE]); // ` `: a plain backquote too
    s.keys(&[BACKQUOTE, 55]); // ` .: the backquote, then the dot
    s.keys(&[BACKQUOTE, 80, 79]); // ` then an arrow: the backquote is dropped (Left, then Right back)
    s.keys(&[BACKQUOTE, 7]); // ` d: letters work too
    s.type_text("'\n");
    // the serial port shows an overlined character as itself plus U+0305
    s.expect("\n0z1\u{305}D```.d\u{305}\n", 60);
    // in the Russian layout the key is ё, not a dead key
    s.switch_layout(ALT);
    s.keys(&[BACKQUOTE]);
    s.switch_layout(ALT);
    s.type_text(" x\n");
    s.expect("ё x\n", 60);
}

#[test]
fn trwm_switches_layout_and_shows_it() {
    let mut s = Session::boot();
    s.console("trwm term &");
    s.wait_for("trwm: window 0 'Terminal'", 200);
    s.run(40);
    s.click(TERM_X + 500, TERM_Y - 8);
    let font = trinux::font8x8_table();
    let panel_text = |s: &Session| -> String {
        (0..2)
            .map(|k| {
                let x0 = W as i64 - 22 + 8 * k;
                let cell: Vec<i16> = (0..8).map(|y| (0..8).map(|x| (s.pixel(x0 + x, 4 + y) == WHITE) as i16 * (1 << x)).sum()).collect();
                (b'A'..=b'Z').find(|&c| font[c as usize * 8..c as usize * 8 + 8] == cell[..]).map(|c| c as char).unwrap_or('?')
            })
            .collect()
    };
    assert_eq!(panel_text(&s), "EN");
    s.type_text("echo ");
    s.switch_layout(ALT);
    s.expect("trwm: layout RU", 30);
    s.run(10);
    assert_eq!(panel_text(&s), "RU", "the panel shows the layout");
    s.keys(&PRIVET);
    s.switch_layout(SHIFT);
    s.expect("trwm: layout EN", 30);
    s.type_text("\n");
    s.run(60);
    assert_eq!(panel_text(&s), "EN");
    let text = s.term_text(4);
    assert!(text.contains(&canonical("$ echo привет")), "{text:#?}");
    assert!(text.contains(&canonical("привет")), "{text:#?}");
}

#[test]
fn the_terminal_window_shows_cyrillic() {
    let mut s = Session::boot();
    s.console("trwm term &");
    s.wait_for("trwm: window 0 'Terminal'", 200);
    s.run(40);
    s.click(TERM_X + 500, TERM_Y - 8);
    s.type_text("cat /etc/pangrams\n");
    s.run(120);
    let text = s.term_text(7);
    let file = std::fs::read_to_string(trinux::user_dir().join("etc/pangrams")).unwrap();
    for line in file.lines() {
        let want = canonical(line);
        assert!(text.contains(&want), "missing {want:?} in the terminal: {text:#?}");
    }
    // the letters only Cyrillic has really are read back as Cyrillic
    assert!(text.iter().any(|l| l.contains("Съешь") || l.contains("Cъeшь")), "{text:#?}");
    assert!(text.iter().any(|l| l.contains('Ж') && l.contains('Щ') && l.contains('Ю') && l.contains('Я')), "{text:#?}");
    // characters past the font are drawn as '?'
    assert!(text.iter().any(|l| l.starts_with("? ? are past the font.")), "{text:#?}");
}

/// The pid trwm logged for the window titled `title`.
fn window_pid(out: &str, title: &str) -> i64 {
    let tag = format!("'{title}' (pid ");
    let at = out.rfind(&tag).unwrap_or_else(|| panic!("no window {title:?} in:\n{out}")) + tag.len();
    out[at..].split(')').next().unwrap().parse().unwrap()
}

#[test]
fn k8_window_system_refusals() {
    let mut s = Session::boot();
    s.console("trwm clock &");
    s.wait_for("trwm: window 0 'Clock'", 200);
    // a second server cannot have the display
    s.console("trwm");
    s.expect("trwm: /dev/fb0: Device or resource busy", 60);
    s.click(340, 8); // [Exit]
    s.expect("trwm: exit", 60);
    s.run(20);
    // clients that break the protocol are cut off; the others keep working
    s.console(
        "trwm /usr/lib/trwtest/nocreate /usr/lib/trwtest/unknown /usr/lib/trwtest/huge /usr/lib/trwtest/bigwin          /usr/lib/trwtest/crash /bin/nope clock &",
    );
    s.expect("trwm: started", 200);
    s.run(150);
    for want in [
        "bad request (no window yet)",
        "bad request (unknown op)",
        "bad request (bad length)",
        "bad request (bad CREATE)",
        "trwm: /bin/nope: No such file or directory",
        "'Crash' (pid",
        "'Clock' (pid",
    ] {
        assert!(s.out.contains(want), "missing {want:?} in:\n{}", s.out);
    }
    // the crashing client dies of SIGSEGV after a second, and its window goes
    s.run(30);
    assert!(s.out.contains("killed by signal 11"), "{}", s.out);
    assert!(s.out.contains("closed (exited)") || s.out.contains("window 4 closed"), "{}", s.out);
    // killing a client from the console removes its window
    let pid = window_pid(&s.out, "Clock");
    s.console(&format!("kill -9 {pid}"));
    s.expect("closed", 100);
    // the server still answers: [Exit] ends the session cleanly
    s.click(340, 8);
    s.expect("trwm: exit", 60);
    s.run(20);
    assert_eq!(s.m.dev.display.mode(), -1);
}

#[test]
fn k8_kernel_interfaces() {
    let mut s = Session::boot();
    s.console("k8test");
    s.expect("k8test: all passed", 600);
    assert!(!s.out.contains("FAILED"), "{}", s.out);
    let n = s.out.matches(" ok\n").count();
    assert!(n >= 45, "only {n} checks:\n{}", s.out);
}

/// A press, a move and a release 3 ms apart (as a browser delivers a quick
/// drag between two frames, while clients keep the kernel busy): the mouse's
/// FIFO of button changes keeps the press and the release apart.
#[test]
fn k8_a_quick_drag_is_not_lost() {
    let mut s = Session::boot();
    s.console("trwm term clock colors paint &");
    s.wait_for("trwm: window 3 'Paint'", 300);
    s.run(80);
    // Paint is the fourth window: content at (108, 114), title bar above it
    assert_eq!(s.pixel(223, 105), TITLE_ON);
    s.m.dev.mouse_abs(223, 105);
    s.m.run(30_000);
    s.m.dev.mouse_buttons(true, false, false);
    s.m.run(30_000);
    s.m.dev.mouse_abs(331, 252);
    s.m.run(30_000);
    s.m.dev.mouse_buttons(false, false, false);
    s.run(10);
    assert_eq!(s.pixel(331, 252), TITLE_ON, "the title bar moved with the drag");
    assert_ne!(s.pixel(223, 105), TITLE_ON, "and left its old place");
}
