//! Setun-386 browser front end: plain `extern "C"` WebAssembly exports, driven
//! by `web/index.html` (no wasm-bindgen needed).
//!
//! The page calls `setun_boot`, then once per animation frame: input calls,
//! `setun_run`, `setun_render` (RGBA pixels in WASM memory) and
//! `setun_serial_take` (UTF-8 output).
//!
//! By default the machine runs the BIOS and boots Trinux from a disk built at
//! compile time (build.rs: the same disk `mkdisk` makes). The demo ROM is the
//! other choice.

use setun_core::devices::MemDisk;
use setun_core::machine::Machine;
use setun_core::PAGE;
use std::cell::RefCell;

/// The BIOS ROM and the Trinux disk (zero-run encoded), from build.rs.
static BIOS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/bios.t1b2"));
static DISK: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/trinux.rle"));

pub const KIND_TRINUX: u32 = 0;
pub const KIND_DEMO: u32 = 1;

struct Emu {
    m: Machine,
    px: Vec<u32>,
    rgba: Vec<u8>,
    size: (usize, usize),
    serial: Vec<u8>,
    ram_frames: i64,
    kind: u32,
}

thread_local! {
    static EMU: RefCell<Option<Emu>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Emu) -> R) -> R {
    EMU.with(|e| f(e.borrow_mut().as_mut().expect("setun_boot first")))
}

fn trytes_le(b: &[u8]) -> Vec<i16> {
    b.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect()
}

/// Undo build.rs's encoding: "TDSK", the tryte count, then chunks of
/// (zero run, literal count, literals).
pub fn decode_disk(b: &[u8]) -> Vec<i16> {
    assert_eq!(&b[..4], b"TDSK", "not an encoded disk");
    let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize;
    let total = u32_at(4);
    let mut out = Vec::with_capacity(total);
    let mut i = 8;
    while out.len() < total {
        let zeros = u32_at(i);
        let lits = u32_at(i + 4);
        i += 8;
        out.resize(out.len() + zeros, 0);
        out.extend(trytes_le(&b[i..i + 2 * lits]));
        i += 2 * lits;
    }
    out
}

/// The pristine Trinux disk.
pub fn trinux_disk() -> Vec<i16> {
    decode_disk(DISK)
}

fn new_machine(kind: u32, ram: i64, unix: i64, disk: Option<Box<dyn setun_core::devices::BlockStore>>) -> Machine {
    if kind == KIND_DEMO {
        return setun_demo::machine(ram, unix);
    }
    let mut m = Machine::new(ram, unix);
    m.load_rom(&trytes_le(BIOS));
    let store = disk.unwrap_or_else(|| Box::new(MemDisk::from_trytes(&trinux_disk())));
    m.dev.disks[0].attach(store);
    m
}

/// Start a machine: `kind` 0 boots Trinux, 1 runs the demo ROM.
/// `unix_time_s` seeds the clock.
#[no_mangle]
pub extern "C" fn setun_boot(kind: u32, ram_frames: u32, unix_time_s: f64) {
    let ram = ram_frames.clamp(1, 19_683) as i64;
    let kind = if kind == KIND_DEMO { KIND_DEMO } else { KIND_TRINUX };
    EMU.with(|e| {
        *e.borrow_mut() = Some(Emu {
            m: new_machine(kind, ram, unix_time_s as i64, None),
            px: Vec::new(),
            rgba: Vec::new(),
            size: (640, 480),
            serial: Vec::new(),
            ram_frames: ram,
            kind,
        })
    });
}

/// Power-cycle: a fresh machine with a re-read clock. Trinux keeps its disk,
/// so files written before survive (as on real hardware).
#[no_mangle]
pub extern "C" fn setun_power_cycle(unix_time_s: f64) {
    with(|e| {
        let disk = e.m.dev.disks[0].detach();
        e.m = new_machine(e.kind, e.ram_frames, unix_time_s as i64, disk);
    });
}

/// Power-cycle with the disk as it was built (a fresh Trinux install).
#[no_mangle]
pub extern "C" fn setun_fresh_disk(unix_time_s: f64) {
    with(|e| e.m = new_machine(e.kind, e.ram_frames, unix_time_s as i64, None));
}

/// 0 = Trinux, 1 = the demo ROM.
#[no_mangle]
pub extern "C" fn setun_kind() -> u32 {
    with(|e| e.kind)
}

/// Sectors on the boot disk (0 for the demo).
#[no_mangle]
pub extern "C" fn setun_disk_sectors() -> u32 {
    with(|e| e.m.dev.disks[0].read(5) as u32)
}

const _: () = assert!(PAGE == 19_683);

/// Run `n` instructions. Returns 1 if the machine has powered itself off.
#[no_mangle]
pub extern "C" fn setun_run(n: u32) -> u32 {
    with(|e| {
        e.m.run(n as u64);
        e.m.powered_off as u32
    })
}

/// Render the display into RGBA bytes; returns a pointer to them.
#[no_mangle]
pub extern "C" fn setun_render() -> *const u8 {
    with(|e| {
        e.size = e.m.dev.display.render(&e.m.mem, &mut e.px);
        e.rgba.resize(e.px.len() * 4, 255);
        for (o, &p) in e.rgba.chunks_exact_mut(4).zip(&e.px) {
            o[0] = (p >> 16) as u8;
            o[1] = (p >> 8) as u8;
            o[2] = p as u8;
        }
        e.rgba.as_ptr()
    })
}

#[no_mangle]
pub extern "C" fn setun_width() -> u32 {
    with(|e| e.size.0 as u32)
}

#[no_mangle]
pub extern "C" fn setun_height() -> u32 {
    with(|e| e.size.1 as u32)
}

/// A key event: USB HID usage `code`.
#[no_mangle]
pub extern "C" fn setun_key(code: i32, pressed: i32) {
    with(|e| e.m.dev.key(code as i64, pressed != 0));
}

/// Mouse position in framebuffer pixels and buttons (bit 0 left, 1 middle, 2 right).
#[no_mangle]
pub extern "C" fn setun_mouse(x: i32, y: i32, buttons: i32) {
    with(|e| {
        e.m.dev.mouse_abs(x as i64, y as i64);
        e.m.dev.mouse_buttons(buttons & 1 != 0, buttons & 2 != 0, buttons & 4 != 0);
    });
}

#[no_mangle]
pub extern "C" fn setun_wheel(steps: i32) {
    with(|e| e.m.dev.mouse_wheel(steps as i64));
}

/// One character (Unicode code point) typed on the serial console.
#[no_mangle]
pub extern "C" fn setun_serial_char(code_point: u32) {
    if let Some(c) = char::from_u32(code_point) {
        with(|e| e.m.dev.serial_input(c.encode_utf8(&mut [0u8; 4])));
    }
}

/// Collect pending serial output as UTF-8; returns its length in bytes.
/// Read it at `setun_serial_ptr()`.
#[no_mangle]
pub extern "C" fn setun_serial_take() -> u32 {
    with(|e| {
        e.serial = e.m.dev.serial.take_output().into_bytes();
        e.serial.len() as u32
    })
}

#[no_mangle]
pub extern "C" fn setun_serial_ptr() -> *const u8 {
    with(|e| e.serial.as_ptr())
}

/// Program counter (for the front-panel lamps).
#[no_mangle]
pub extern "C" fn setun_pc() -> f64 {
    with(|e| e.m.cpu.pc.value() as f64)
}

/// General register `r` (−13..=13).
#[no_mangle]
pub extern "C" fn setun_reg(r: i32) -> f64 {
    with(|e| e.m.cpu.reg((r as i64).clamp(-13, 13)).value() as f64)
}

/// Current ring (−1, 0, +1) and whether the CPU is halted (1/0).
#[no_mangle]
pub extern "C" fn setun_ring() -> i32 {
    with(|e| e.m.cpu.ring() as i32)
}

#[no_mangle]
pub extern "C" fn setun_halted() -> i32 {
    with(|e| e.m.cpu.halted as i32)
}

/// Display mode: −1 text, 0 = 640×480, +1 = 800×600.
#[no_mangle]
pub extern "C" fn setun_display_mode() -> i32 {
    with(|e| e.m.dev.display.mode() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output() -> String {
        setun_serial_take();
        with(|e| String::from_utf8(e.serial.clone()).unwrap())
    }

    fn run_until(want: &str, max_millions: u32) -> String {
        let mut all = String::new();
        for _ in 0..max_millions {
            setun_run(1_000_000);
            all += &output();
            if all.contains(want) {
                return all;
            }
        }
        panic!("no {want:?} in:\n{all}");
    }

    /// Wait for the login prompt and sign in as root.
    fn login() {
        run_until("login: ", 80);
        type_line("root\n");
        run_until("Password: ", 20);
        type_line("root\n");
        run_until("$ ", 40);
    }

    fn type_line(s: &str) {
        for c in s.chars() {
            setun_serial_char(c as u32);
        }
    }

    #[test]
    fn the_disk_round_trips_and_boots_trinux() {
        // the embedded disk decodes to the disk mkdisk builds
        let built = trinux_disk();
        assert_eq!(built.len(), 729 * PAGE as usize);
        assert_eq!(&built[..19_683], &decode_disk(DISK)[..19_683]);
        setun_boot(KIND_TRINUX, 729, 1_790_000_000.0);
        assert_eq!(setun_kind(), KIND_TRINUX);
        assert_eq!(setun_disk_sectors(), 729);
        login();
        type_line("echo hi from the web > /home/w; cat /home/w\n");
        let out = run_until("hi from the web\n", 40);
        assert!(out.contains("cat /home/w"), "{out}");
        // power-cycling keeps the disk: the file is still there
        type_line("sync\n");
        run_until("$ ", 20);
        setun_power_cycle(1_790_000_100.0);
        login();
        type_line("cat /home/w\n");
        run_until("hi from the web\n", 40);
        // a fresh disk does not have it
        setun_fresh_disk(1_790_000_200.0);
        login();
        type_line("cat /home/w\n");
        run_until("No such file or directory", 40);
    }

    #[test]
    fn the_demo_rom_is_still_there() {
        setun_boot(KIND_DEMO, 729, 1_790_000_000.0);
        assert_eq!(setun_run(300_000), 0);
        let ptr = setun_render();
        assert!(!ptr.is_null());
        assert_eq!((setun_width(), setun_height()), (640, 480));
        assert!(output().starts_with("Setun-386 demo ROM ready"));
        for c in "Привет".chars() {
            setun_serial_char(c as u32);
        }
        setun_run(100_000);
        assert_eq!(output(), "Привет");
        setun_key(43, 1);
        setun_run(10_000);
        assert_eq!(setun_display_mode(), 0, "Tab switched to graphics");
        assert_eq!(setun_ring(), 1);
        assert_eq!(setun_disk_sectors(), 0);
    }
}
