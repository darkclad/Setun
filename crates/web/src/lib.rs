//! Setun-26 browser front end: plain `extern "C"` WebAssembly exports, driven
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
    /// The page compiles the JIT's modules (`setun_jit_enable`).
    static JIT_ON: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The module `setun_jit_take` last handed out.
    static JIT_OUT: RefCell<(u64, Vec<u8>)> = const { RefCell::new((0, Vec::new())) };
    /// Entropy for the next machine's TRNG, from the page's crypto.getRandomValues.
    static SEED: RefCell<Option<[u8; 32]>> = const { RefCell::new(None) };
}

/// Set word `i` (0..8) of the seed for the next boot or power-cycle. The
/// page passes fresh browser entropy before each; a machine made without it
/// runs its TRNG from the fixed all-zero seed (SOURCE reads 0).
#[no_mangle]
pub extern "C" fn setun_seed_word(i: u32, v: u32) {
    if i >= 8 {
        return;
    }
    SEED.with(|s| {
        let mut s = s.borrow_mut();
        let seed = s.get_or_insert([0; 32]);
        seed[4 * i as usize..4 * i as usize + 4].copy_from_slice(&v.to_le_bytes());
    });
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
    let mut m = new_machine_unseeded(kind, ram, unix, disk);
    // each seed is used once: the page sends a new one for every machine
    if let Some(seed) = SEED.with(|s| s.borrow_mut().take()) {
        m.dev.rng.seed(seed, true);
    }
    m
}

fn new_machine_unseeded(kind: u32, ram: i64, unix: i64, disk: Option<Box<dyn setun_core::devices::BlockStore>>) -> Machine {
    if kind == KIND_DEMO {
        return setun_demo::machine(ram, unix);
    }
    let mut m = Machine::new(ram, unix);
    m.cpu.set_jit_enabled(JIT_ON.with(|j| j.get()));
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

// ---- the JIT ---------------------------------------------------------------------------
//
// Hot code is compiled to small WebAssembly modules that import this
// module's memory and function table. The page compiles them: once it
// calls `setun_jit_enable(1)`, it takes each module with `setun_jit_take`
// (then `setun_jit_ptr`, `setun_jit_id`), compiles and instantiates it in
// the background with `{ env: { memory, table } }`, adds its `run` export to
// `__indirect_function_table`, and reports the index with `setun_jit_done`.

/// The page will compile modules (1) or not (0): the JIT on or off.
#[no_mangle]
pub extern "C" fn setun_jit_enable(on: u32) {
    JIT_ON.with(|j| j.set(on != 0));
    EMU.with(|e| {
        if let Some(e) = e.borrow_mut().as_mut() {
            e.m.cpu.set_jit_enabled(on != 0);
        }
    });
}

/// The next module to compile: its length in bytes (0: none waiting).
#[no_mangle]
pub extern "C" fn setun_jit_take() -> u32 {
    match setun_core::cpu::Cpu::jit_take() {
        Some(r) => JIT_OUT.with(|o| {
            *o.borrow_mut() = r;
            o.borrow().1.len() as u32
        }),
        None => 0,
    }
}

/// The bytes of the module `setun_jit_take` handed out.
#[no_mangle]
pub extern "C" fn setun_jit_ptr() -> *const u8 {
    JIT_OUT.with(|o| o.borrow().1.as_ptr())
}

/// Its request id.
#[no_mangle]
pub extern "C" fn setun_jit_id() -> f64 {
    JIT_OUT.with(|o| o.borrow().0 as f64)
}

/// Request `id` compiled: its `run` is at `index` in the table (0: failed).
#[no_mangle]
pub extern "C" fn setun_jit_done(id: f64, index: u32) {
    setun_core::cpu::Cpu::jit_finished(id as u64, index);
}

/// Run `n` instructions. Returns 1 if the machine has powered itself off.
#[no_mangle]
pub extern "C" fn setun_run(n: u32) -> u32 {
    with(|e| {
        e.m.run(n as u64);
        e.m.powered_off as u32
    })
}

/// Render the display into RGBA bytes; returns a pointer to them.
/// `now_ms` is the page clock (`performance.now()`), which drives blinking.
#[no_mangle]
pub extern "C" fn setun_render(now_ms: f64) -> *const u8 {
    with(|e| {
        e.m.dev.display.set_wall_ms(now_ms.is_finite().then(|| now_ms.max(0.0) as u64));
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

    #[test]
    fn web_page_and_module_carry_the_new_name() {
        let web = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web");
        let page = std::fs::read_to_string(web.join("index.html")).unwrap();
        assert!(page.contains("<title>Setun-11\u{305}</title>"), "page title");
        // the heading draws the bar with CSS over the second 1, and names the machine for screen readers
        assert!(page.contains(r#"aria-label="Setun-11̅">SETUN<span aria-hidden="true">-1<span class="ov">1</span>"#), "heading");
        let wasm = std::fs::read(web.join("setun.wasm")).unwrap();
        for old in ["Setun-386", "SETUN-386", "Setun-387", "SETUN-387", "setun386"] {
            assert!(!page.contains(old), "{old} in index.html");
            assert!(!wasm.windows(old.len()).any(|w| w == old.as_bytes()), "{old} in setun.wasm");
        }
    }

    #[test]
    fn each_machine_takes_the_seed_given_once() {
        let first = |kind: u32| {
            setun_boot(kind, 27, 0.0);
            with(|e| (e.m.dev.rng.from_host, e.m.dev.rng.next_octet(), e.m.dev.rng.next_octet()))
        };
        // no seed: the fixed one, the same every time
        let a = first(KIND_TRINUX);
        assert!(!a.0);
        assert_eq!(first(KIND_TRINUX), a);
        // a seed: from the host, different output; then gone again
        for (i, v) in [7u32, 1, 2, 3, 4, 5, 6, 9].iter().enumerate() {
            setun_seed_word(i as u32, *v);
        }
        setun_seed_word(8, 1); // out of range: ignored
        let b = first(KIND_TRINUX);
        assert!(b.0);
        assert_ne!((b.1, b.2), (a.1, a.2));
        assert_eq!(first(KIND_TRINUX), a);
        // power-cycles take a new one too
        setun_seed_word(0, 99);
        setun_power_cycle(0.0);
        assert!(with(|e| e.m.dev.rng.from_host));
    }

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
        let ptr = setun_render(0.0);
        assert!(!ptr.is_null());
        assert_eq!((setun_width(), setun_height()), (640, 480));
        assert!(output().starts_with("Setun-11̅ demo ROM ready"));
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
