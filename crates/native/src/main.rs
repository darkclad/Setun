//! Setun-26 native front end.
//!
//! A window showing the display (text or graphics mode), keyboard and mouse
//! input, and the serial console on stdin/stdout. Without `--rom` it boots
//! the built-in BIOS (from bios/bios.tri); `--demo` boots the old demo ROM.
//! `--console` runs without a window, the terminal as the serial console
//! (for ssh); without `--disk` it boots a Trinux disk built from the source
//! tree.
//!
//! ```text
//! setun [--rom FILE.t1b2] [--disk FILE.t1b2] [--ram FRAMES] [--mips N]
//!       [--scale N] [--headless INSTRUCTIONS] [--keys TEXT] [--screenshot FILE.png]
//! setun --console [--disk FILE.t1b2] [--ram FRAMES]
//! ```

use minifb::{Key, KeyRepeat, MouseButton, MouseMode, Scale, Window, WindowOptions};
use setun_core::devices::FileDisk;
use setun_core::machine::Machine;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::mpsc;

mod console;

struct Args {
    rom: Option<PathBuf>,
    demo: bool,
    disk: Option<PathBuf>,
    ram_frames: i64,
    mips: f64,
    scale: usize,
    headless: Option<u64>,
    keys: String,
    press: Vec<i64>,
    screenshot: Option<PathBuf>,
    console: bool,
    /// `--net`: attach a network, with these port forwards (host, guest).
    net: Option<Vec<(std::net::SocketAddr, u16)>>,
    /// `--tlink`: a ternary link to another setun over UDP, and
    /// `--tlink-addr`: this station's address (docs/TNET.md §1).
    tlink: Option<String>,
    tlink_addr: i64,
}

/// What the machine is connected to, polled after every slice of time.
pub struct Links {
    net: Option<setun_net::Network>,
    tlink: Option<setun_net::tnet::UdpLink>,
}

impl Links {
    /// Is anything attached (so waits are kept short and traffic flows)?
    pub fn active(&self) -> bool {
        self.net.is_some() || self.tlink.is_some()
    }

    pub fn poll(&mut self, m: &mut Machine) {
        if let Some(n) = self.net.as_mut() {
            n.poll(m);
        }
        if let Some(t) = self.tlink.as_mut() {
            t.poll(m);
        }
    }
}

fn usage() -> ! {
    eprintln!(
        "usage: setun [--rom FILE.t1b2 | --demo] [--disk FILE.t1b2] [--ram FRAMES] [--mips N] [--scale N]\n\
         \x20            [--headless INSTRUCTIONS] [--keys TEXT] [--screenshot FILE.png]\n\
         \x20      setun --console [--disk FILE.t1b2] [--ram FRAMES]\n\
         \n\
         --ram FRAMES     RAM in 3^9-tryte pages (default 19683 = 3^18 trytes)\n\
         --mips N         emulated speed in millions of instructions per second (default 10)\n\
         --headless N     run N instructions without a window, print serial output and the text screen\n\
         --keys TEXT      (headless) feed TEXT to the serial console after booting\n\
         --press A,B,..   (headless) press these USB HID keys after booting\n\
         --screenshot F   (headless) save the final screen as a PNG\n\
         --console        no window: this terminal is the serial console (Ctrl-] leaves);\n\
         \x20                without --disk, a Trinux disk built from the source tree\n\
         --net [ADDR:]HOST:GUEST  attach the user-mode network, forwarding a host port (repeatable; 'on': none)\n\
         --tlink listen:PORT | connect:HOST:PORT  a ternary link to another setun over UDP\n\
         --tlink-addr A.B.C   this station's TNET address, three trytes (default 0.0.1)"
    );
    std::process::exit(2)
}

fn parse_args() -> Args {
    let mut a = Args {
        rom: None,
        demo: false,
        disk: None,
        ram_frames: setun_core::memory::DEFAULT_RAM_FRAMES,
        mips: 10.0,
        scale: 2,
        headless: None,
        keys: String::new(),
        press: Vec::new(),
        screenshot: None,
        console: false,
        net: None,
        tlink: None,
        tlink_addr: 1,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut val = || it.next().unwrap_or_else(|| usage());
        match flag.as_str() {
            "--rom" => a.rom = Some(val().into()),
            "--demo" => a.demo = true,
            "--disk" => a.disk = Some(val().into()),
            "--ram" => a.ram_frames = val().parse().unwrap_or_else(|_| usage()),
            "--mips" => a.mips = val().parse().unwrap_or_else(|_| usage()),
            "--scale" => a.scale = val().parse().unwrap_or_else(|_| usage()),
            "--headless" => a.headless = Some(val().parse().unwrap_or_else(|_| usage())),
            "--keys" => a.keys = val(),
            "--press" => {
                a.press = val().split(',').map(|k| k.trim().parse().unwrap_or_else(|_| usage())).collect()
            }
            "--screenshot" => a.screenshot = Some(val().into()),
            "--console" => a.console = true,
            "--net" => {
                let spec = val();
                let fwds = a.net.get_or_insert_with(Vec::new);
                if spec != "on" {
                    fwds.push(parse_forward(&spec).unwrap_or_else(|| usage()));
                }
            }
            "--tlink" => a.tlink = Some(val()),
            "--tlink-addr" => a.tlink_addr = setun_net::tnet::parse_addr(&val()).unwrap_or_else(|| usage()),
            _ => usage(),
        }
    }
    a
}

/// `[ADDR:]HOSTPORT:GUESTPORT`.
fn parse_forward(spec: &str) -> Option<(std::net::SocketAddr, u16)> {
    let (host, guest) = spec.rsplit_once(':')?;
    let guest: u16 = guest.parse().ok()?;
    let host = if host.contains(':') { host.parse().ok()? } else { format!("127.0.0.1:{host}").parse().ok()? };
    Some((host, guest))
}

/// Attach what `--net` and `--tlink` asked for.
fn attach_links(m: &mut Machine, args: &Args) -> Links {
    let tlink = args.tlink.as_ref().map(|spec| match setun_net::tnet::UdpLink::open(spec) {
        Ok(l) => {
            m.dev.tlink.link = true;
            m.dev.tlink.addr = args.tlink_addr;
            eprintln!("setun: ternary link {spec}, station {}", setun_net::tnet::addr_str(args.tlink_addr));
            l
        }
        Err(e) => {
            eprintln!("setun: --tlink {spec}: {e}");
            std::process::exit(1)
        }
    });
    Links { net: attach_net(m, args), tlink }
}

/// Attach the network `--net` asked for, if any.
fn attach_net(m: &mut Machine, args: &Args) -> Option<setun_net::Network> {
    let fwds = args.net.as_ref()?;
    let mut net = setun_net::Network::attach(m);
    for &(host, guest) in fwds {
        match net.forward(host, guest) {
            Ok(a) => eprintln!("setun: {a} -> guest port {guest}"),
            Err(e) => {
                eprintln!("setun: cannot listen on {host}: {e}");
                std::process::exit(1)
            }
        }
    }
    Some(net)
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Read a T1B2 file: one tryte per little-endian i16.
fn read_t1b2(path: &PathBuf) -> std::io::Result<Vec<i16>> {
    let bytes = std::fs::read(path)?;
    Ok(bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect())
}

fn build_machine(args: &Args) -> Machine {
    let mut m = match &args.rom {
        Some(path) => {
            let rom = read_t1b2(path).unwrap_or_else(|e| {
                eprintln!("setun: cannot read ROM {}: {e}", path.display());
                std::process::exit(1)
            });
            let mut m = Machine::new(args.ram_frames, unix_now());
            m.load_rom(&rom);
            m
        }
        None if args.demo => setun_demo::machine(args.ram_frames, unix_now()),
        None => {
            let rom = trine::bios_rom().unwrap_or_else(|e| {
                eprintln!("setun: the built-in BIOS does not compile: {e}");
                std::process::exit(1)
            });
            let mut m = Machine::new(args.ram_frames, unix_now());
            m.load_rom(&rom);
            m
        }
    };
    m.dev.rng.seed(setun_core::devices::host_entropy(), true);
    if let Some(path) = &args.disk {
        match FileDisk::open(path) {
            Ok(d) => m.dev.disks[0].attach(Box::new(d)),
            Err(e) => {
                eprintln!("setun: cannot open disk {}: {e}", path.display());
                std::process::exit(1)
            }
        }
    }
    m
}

fn save_png(m: &Machine, path: &PathBuf) {
    let mut px = Vec::new();
    let (w, h) = m.dev.display.render(&m.mem, &mut px);
    let rgb: Vec<u8> = px.iter().flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8]).collect();
    let file = std::fs::File::create(path).expect("create screenshot");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().and_then(|mut wr| wr.write_image_data(&rgb)).expect("write screenshot");
}

fn headless(mut m: Machine, n: u64, args: &Args) {
    m.run(n / 2);
    if !args.keys.is_empty() {
        m.dev.serial_input(&args.keys.replace("\\n", "\n"));
    }
    for &k in &args.press {
        m.dev.key(k, true);
        m.dev.key(k, false);
    }
    m.run(n - n / 2);
    print!("{}", m.dev.serial.take_output());
    println!("\n---- screen ----");
    for line in setun_demo::screen_text(&m) {
        println!("{line}");
    }
    if let Some(p) = &args.screenshot {
        save_png(&m, p);
        println!("---- screenshot: {} ----", p.display());
    }
}

/// minifb key → USB HID usage ID.
fn hid(k: Key) -> Option<i64> {
    use Key::*;
    let n = k as i64;
    Some(match k {
        A | B | C | D | E | F | G | H | I | J | K | L | M | N | O | P | Q | R | S | T | U | V | W | X | Y | Z => {
            4 + (n - A as i64)
        }
        Key1 | Key2 | Key3 | Key4 | Key5 | Key6 | Key7 | Key8 | Key9 => 30 + (n - Key1 as i64),
        Key0 => 39,
        Enter => 40,
        Escape => 41,
        Backspace => 42,
        Tab => 43,
        Space => 44,
        Minus => 45,
        Equal => 46,
        LeftBracket => 47,
        RightBracket => 48,
        Backslash => 49,
        Semicolon => 51,
        Apostrophe => 52,
        Backquote => 53,
        Comma => 54,
        Period => 55,
        Slash => 56,
        CapsLock => 57,
        F1 | F2 | F3 | F4 | F5 | F6 | F7 | F8 | F9 | F10 | F11 | F12 => 58 + (n - F1 as i64),
        Insert => 73,
        Home => 74,
        PageUp => 75,
        Delete => 76,
        End => 77,
        PageDown => 78,
        Right => 79,
        Left => 80,
        Down => 81,
        Up => 82,
        LeftCtrl => 224,
        LeftShift => 225,
        LeftAlt => 226,
        LeftSuper => 227,
        RightCtrl => 228,
        RightShift => 229,
        RightAlt => 230,
        RightSuper => 231,
        _ => return None,
    })
}

fn main() {
    let args = parse_args();
    if args.console && args.disk.is_none() && args.rom.is_none() && !args.demo {
        let disk = trinux::build_disk(&trinux::kernel_dir()).unwrap_or_else(|e| {
            eprintln!("setun: cannot build the Trinux disk (run from the source tree, or give --disk): {e}");
            std::process::exit(1)
        });
        let mut m = build_machine(&args);
        m.dev.disks[0].attach(Box::new(setun_core::devices::MemDisk::from_trytes(&disk.trytes)));
        let links = attach_links(&mut m, &args);
        return console::run(m, links);
    }
    let mut m = build_machine(&args);
    let mut links = attach_links(&mut m, &args);
    if args.console {
        return console::run(m, links);
    }
    if let Some(n) = args.headless {
        return headless(m, n, &args);
    }

    // Serial console: stdin lines → the machine.
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if tx.send(line + "\n").is_err() {
                break;
            }
        }
    });

    let scale = match args.scale {
        1 => Scale::X1,
        4 => Scale::X4,
        _ => Scale::X2,
    };
    let mut window = Window::new(
        "Setun-11̅",
        640,
        480,
        WindowOptions { scale, ..WindowOptions::default() },
    )
    .unwrap_or_else(|e| {
        eprintln!("setun: cannot open window: {e}");
        std::process::exit(1)
    });
    window.set_target_fps(60);

    let per_frame = (args.mips * 1e6 / 60.0) as u64;
    let mut px = Vec::new();
    let mut buttons = (false, false, false);
    let mut stdout = std::io::stdout();
    let mut frames = 0u64;
    let mut t0 = std::time::Instant::now();
    let start = std::time::Instant::now();

    while window.is_open() && !m.powered_off {
        // Input.
        for k in window.get_keys_pressed(KeyRepeat::Yes) {
            if let Some(c) = hid(k) {
                m.dev.key(c, true);
            }
        }
        for k in window.get_keys_released() {
            if let Some(c) = hid(k) {
                m.dev.key(c, false);
            }
        }
        let (bw, bh) = m.dev.display.pixel_size();
        if let Some((x, y)) = window.get_unscaled_mouse_pos(MouseMode::Discard) {
            let (ww, wh) = window.get_size();
            let bx = (x as f64 * bw as f64 / ww.max(1) as f64) as i64;
            let by = (y as f64 * bh as f64 / wh.max(1) as f64) as i64;
            m.dev.mouse_abs(bx, by);
        }
        let b = (
            window.get_mouse_down(MouseButton::Left),
            window.get_mouse_down(MouseButton::Middle),
            window.get_mouse_down(MouseButton::Right),
        );
        if b != buttons {
            m.dev.mouse_buttons(b.0, b.1, b.2);
            buttons = b;
        }
        if let Some((_, dy)) = window.get_scroll_wheel() {
            m.dev.mouse_wheel(-dy.signum() as i64);
        }
        while let Ok(line) = rx.try_recv() {
            m.dev.serial_input(&line);
        }

        // Emulate one video frame's worth of instructions.
        m.run(per_frame);
        links.poll(&mut m);

        let out = m.dev.serial.take_output();
        if !out.is_empty() {
            print!("{out}");
            let _ = stdout.flush();
        }
        m.dev.display.set_wall_ms(Some(start.elapsed().as_millis() as u64));
        let (w, h) = m.dev.display.render(&m.mem, &mut px);
        if window.update_with_buffer(&px, w, h).is_err() {
            break;
        }

        frames += 1;
        if frames.is_multiple_of(60) {
            let fps = 60.0 / t0.elapsed().as_secs_f64();
            window.set_title(&format!("Setun-11̅ — {:.0} fps, {:.1} MIPS", fps, fps * per_frame as f64 / 1e6));
            t0 = std::time::Instant::now();
        }
    }
}
