//! `--console`: the machine on the terminal, no window (for ssh). The
//! terminal is put in raw mode and wired to the serial console: keys go in
//! as the bytes a VT terminal sends (arrows as ANSI sequences, Ctrl-C as
//! 3, Backspace as DEL), output comes back with each `\n` made `\r\n`.
//! Emulated time is held to real time, so an idle guest costs nothing and
//! `sleep 1` takes a second; a busy one runs as fast as the host allows.
//! Ctrl-] leaves.

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use setun_core::machine::Machine;
use std::io::Write;
use std::time::{Duration, Instant};

/// Instructions between looks at the keyboard and the clock (well under a
/// millisecond of host time).
const SLICE: u64 = 100_000;

/// What the loop gets from the terminal.
pub enum Input {
    Bytes(Vec<u8>),
    Quit,
}

/// The bytes a VT terminal sends for a key, or `Quit` for Ctrl-].
pub fn key_bytes(k: &KeyEvent) -> Option<Input> {
    if k.kind == KeyEventKind::Release {
        return None;
    }
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let seq = |s: &str| Some(Input::Bytes(s.as_bytes().to_vec()));
    match k.code {
        KeyCode::Char(']') if ctrl => Some(Input::Quit),
        KeyCode::Char(c) if ctrl && c.is_ascii_alphabetic() => Some(Input::Bytes(vec![c.to_ascii_lowercase() as u8 & 0x1f])),
        KeyCode::Char(c) => {
            let mut b = [0u8; 4];
            seq(c.encode_utf8(&mut b))
        }
        KeyCode::Enter => seq("\r"),
        KeyCode::Backspace => seq("\x7f"),
        KeyCode::Tab => seq("\t"),
        KeyCode::Esc => seq("\x1b"),
        KeyCode::Up => seq("\x1b[A"),
        KeyCode::Down => seq("\x1b[B"),
        KeyCode::Right => seq("\x1b[C"),
        KeyCode::Left => seq("\x1b[D"),
        KeyCode::Home => seq("\x1b[H"),
        KeyCode::End => seq("\x1b[F"),
        KeyCode::Delete => seq("\x1b[3~"),
        KeyCode::PageUp => seq("\x1b[5~"),
        KeyCode::PageDown => seq("\x1b[6~"),
        _ => None,
    }
}

/// Serial output for a raw-mode terminal: each `\n` becomes `\r\n`.
pub fn crlf(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        if c == '\n' {
            o.push('\r');
        }
        o.push(c);
    }
    o
}

/// Run until the machine powers off or `input` says quit. `now_us` is the
/// real clock (microseconds); `wait` is called when the guest is ahead of
/// it (with the time to wait). The emulated clock never runs ahead of the
/// real one by more than a slice. `poll` runs after every turn (the network).
pub fn run_loop(
    m: &mut Machine,
    mut input: impl FnMut() -> Vec<Input>,
    out: &mut impl Write,
    mut now_us: impl FnMut() -> i64,
    mut wait: impl FnMut(Duration),
    mut poll: impl FnMut(&mut Machine),
) {
    let base = m.dev.now_us() - now_us();
    while !m.powered_off {
        for i in input() {
            match i {
                Input::Quit => return,
                Input::Bytes(b) => m.dev.serial_input(&String::from_utf8_lossy(&b)),
            }
        }
        let ahead = m.dev.now_us() - (now_us() + base);
        if ahead > 0 {
            wait(Duration::from_micros(ahead.min(10_000) as u64));
        } else {
            m.run(SLICE);
        }
        poll(m);
        let s = m.dev.serial.take_output();
        if !s.is_empty() {
            let _ = out.write_all(crlf(&s).as_bytes());
            let _ = out.flush();
        }
    }
}

/// Raw mode on the real terminal, restored however we leave.
struct Raw;

impl Drop for Raw {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

pub fn run(mut m: Machine, mut links: crate::Links) {
    // emulated speed far above what the host reaches: a busy guest is
    // limited by the host, an idle one by the clock
    m.ns_per_instruction = 1;
    if let Err(e) = crossterm::terminal::enable_raw_mode() {
        eprintln!("setun: --console needs a terminal: {e}");
        std::process::exit(1);
    }
    let _raw = Raw;
    eprint!("Setun-11\u{305} console (Ctrl-] leaves)\r\n");
    let start = Instant::now();
    let mut stdout = std::io::stdout();
    let input = || {
        let mut v = Vec::new();
        while event::poll(Duration::ZERO).unwrap_or(false) {
            if let Ok(Event::Key(k)) = event::read() {
                if let Some(i) = key_bytes(&k) {
                    v.push(i);
                }
            }
        }
        v
    };
    // waiting means watching the keyboard (a key ends the wait at once)
    let wait = |d: Duration| {
        let _ = event::poll(d);
    };
    // with a network or a link, a wait is at most a millisecond, so traffic flows
    let networked = links.active();
    let wait = |d: Duration| wait(if networked { d.min(Duration::from_millis(1)) } else { d });
    let poll = |m: &mut Machine| links.poll(m);
    run_loop(&mut m, input, &mut stdout, || start.elapsed().as_micros() as i64, wait, poll);
    eprint!("\r\nsetun: left the console\r\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventState;

    fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent { code, modifiers: mods, kind: KeyEventKind::Press, state: KeyEventState::NONE }
    }

    fn bytes(i: Option<Input>) -> Vec<u8> {
        match i {
            Some(Input::Bytes(b)) => b,
            Some(Input::Quit) => b"QUIT".to_vec(),
            None => Vec::new(),
        }
    }

    #[test]
    fn keys_are_what_a_vt_terminal_sends() {
        let n = KeyModifiers::NONE;
        let c = KeyModifiers::CONTROL;
        assert_eq!(bytes(key_bytes(&key(KeyCode::Char('a'), n))), b"a");
        assert_eq!(bytes(key_bytes(&key(KeyCode::Char('ж'), n))), "ж".as_bytes());
        assert_eq!(bytes(key_bytes(&key(KeyCode::Char('c'), c))), [3]);
        assert_eq!(bytes(key_bytes(&key(KeyCode::Char('D'), c))), [4]);
        assert_eq!(bytes(key_bytes(&key(KeyCode::Enter, n))), b"\r");
        assert_eq!(bytes(key_bytes(&key(KeyCode::Backspace, n))), [127]);
        assert_eq!(bytes(key_bytes(&key(KeyCode::Up, n))), b"\x1b[A");
        assert_eq!(bytes(key_bytes(&key(KeyCode::Left, n))), b"\x1b[D");
        assert_eq!(bytes(key_bytes(&key(KeyCode::Delete, n))), b"\x1b[3~");
        assert_eq!(bytes(key_bytes(&key(KeyCode::Char(']'), c))), b"QUIT");
        // a key's release (Windows reports them) sends nothing
        let mut up = key(KeyCode::Char('a'), n);
        up.kind = KeyEventKind::Release;
        assert!(key_bytes(&up).is_none());
        assert!(key_bytes(&key(KeyCode::F(5), n)).is_none());
    }

    #[test]
    fn output_lines_end_in_crlf() {
        assert_eq!(crlf("a\nb\n"), "a\r\nb\r\n");
        assert_eq!(crlf("ж\n"), "ж\r\n");
    }

    /// Boot Trinux on the console loop with scripted keys and a fake clock
    /// that advances only when the loop waits: (output, emulated µs at the
    /// end, fake real µs at the end).
    fn session(keys: Vec<(i64, &'static str)>, until_us: i64) -> (String, i64, i64) {
        let disk = trinux::build_disk(&trinux::kernel_dir()).expect("the disk builds");
        let mut m = trinux::boot_trytes(&disk.trytes, 729, 0, "");
        m.ns_per_instruction = 1;
        let clock = std::cell::Cell::new(0i64);
        let mut keys = keys.into_iter().peekable();
        let mut out = Vec::new();
        let mut turns = 0u64;
        run_loop(
            &mut m,
            || {
                let mut v = Vec::new();
                // a loop that never waits never moves this clock: stop it
                turns += 1;
                if turns > 3_000_000 {
                    v.push(Input::Quit);
                }
                while keys.peek().is_some_and(|k| k.0 <= clock.get()) {
                    let (_, s) = keys.next().unwrap();
                    v.push(if s == "QUIT" { Input::Quit } else { Input::Bytes(s.as_bytes().to_vec()) });
                }
                if clock.get() > until_us {
                    v.push(Input::Quit);
                }
                v
            },
            &mut out,
            || clock.get(),
            |d| clock.set(clock.get() + d.as_micros() as i64),
            |_| {},
        );
        (String::from_utf8_lossy(&out).into_owned(), m.dev.now_us(), clock.get())
    }

    #[test]
    fn a_session_logs_in_runs_commands_and_quits() {
        let (out, emulated, real) = session(
            vec![
                (3_000_000, "root\r"),
                (4_000_000, "root\r"),
                (6_000_000, "echo hi there; sleep 2; echo done\r"),
                (12_000_000, "QUIT"),
            ],
            60_000_000,
        );
        assert!(out.contains("login: "), "{out}");
        // blank lines and short lines keep their line ends
        assert!(out.contains("BIOS 1.0\r\n\r\n  CPU") && out.contains("none\r\n\r\n  Boot"), "{out:?}");
        assert!(out.contains("hi there\r\n"), "{out}");
        assert!(out.contains("done\r\n"), "{out}");
        assert!(!out.contains("hi there\n") || out.contains("hi there\r\n"));
        // quit came at 12 s: the clocks agree to within a slice or two
        assert!((12_000_000..12_100_000).contains(&real), "real {real}");
        assert!((emulated - real).abs() < 50_000, "emulated {emulated} vs real {real}");
    }

    #[test]
    fn sleep_takes_real_time() {
        // `sleep 3` must not print its marker before 3 s of the real clock
        // (the marker is in a variable: the echoed command line lacks it)
        let keys = || vec![(3_000_000, "root\r"), (4_000_000, "root\r"), (6_000_000, "x=WOKE; sleep 3; echo $x\r")];
        let (out, _, _) = session(keys(), 8_500_000);
        assert!(!out.contains("WOKE\r\n"), "woke too early: {out}");
        let (out, _, _) = session(keys(), 9_800_000);
        assert!(out.contains("WOKE\r\n"), "{out}");
    }
}
