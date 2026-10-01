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
        let disk = trinux::build_disk(&trinux::kernel_dir()).unwrap_or_else(|e| panic!("{e}"));
        let mut s = Session { m: trinux::boot_trytes(&disk.trytes, 729, 0, ""), out: String::new(), mark: 0 };
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
        panic!("timed out waiting for {what:?}:
{}", self.out);
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
                        (32u8..127).find(|&ch| font[ch as usize * 8..ch as usize * 8 + 8] == cell[..]).map(|ch| ch as char).unwrap_or('?')
                    })
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }
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

/// The pid trwm logged for the window titled `title`.
fn window_pid(out: &str, title: &str) -> i64 {
    let tag = format!("'{title}' (pid ");
    let at = out.rfind(&tag).unwrap_or_else(|| panic!("no window {title:?} in:
{out}")) + tag.len();
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
        assert!(s.out.contains(want), "missing {want:?} in:
{}", s.out);
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
