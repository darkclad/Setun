//! Setun-386 browser front end: plain `extern "C"` WebAssembly exports, driven
//! by `web/index.html` (no wasm-bindgen needed).
//!
//! The page calls `setun_init`, then once per animation frame: input calls,
//! `setun_run`, `setun_render` (RGBA pixels in WASM memory) and
//! `setun_serial_take` (UTF-8 output).

use setun_core::machine::Machine;
use std::cell::RefCell;

struct Emu {
    m: Machine,
    px: Vec<u32>,
    rgba: Vec<u8>,
    size: (usize, usize),
    serial: Vec<u8>,
    ram_frames: i64,
}

thread_local! {
    static EMU: RefCell<Option<Emu>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Emu) -> R) -> R {
    EMU.with(|e| f(e.borrow_mut().as_mut().expect("setun_init first")))
}

/// Create a machine with the demo ROM. `unix_time_s` seeds the clock.
#[no_mangle]
pub extern "C" fn setun_init(ram_frames: u32, unix_time_s: f64) {
    let ram = ram_frames.clamp(1, 19_683) as i64;
    let unix = unix_time_s as i64;
    EMU.with(|e| {
        *e.borrow_mut() = Some(Emu {
            m: setun_demo::machine(ram, unix),
            px: Vec::new(),
            rgba: Vec::new(),
            size: (640, 480),
            serial: Vec::new(),
            ram_frames: ram,
        })
    });
}

/// Power-cycle: a fresh machine with the same RAM size and a re-read clock.
#[no_mangle]
pub extern "C" fn setun_power_cycle(unix_time_s: f64) {
    let ram = with(|e| e.ram_frames);
    setun_init(ram as u32, unix_time_s);
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
#[no_mangle]
pub extern "C" fn setun_render() -> *const u8 {
    with(|e| {
        e.size = e.m.dev.display.render(&e.m.mem, &mut e.px);
        e.rgba.clear();
        e.rgba.extend(e.px.iter().flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8, 255]));
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
    fn exports_drive_the_demo() {
        setun_init(729, 1_790_000_000.0);
        assert_eq!(setun_run(300_000), 0);
        let ptr = setun_render();
        assert!(!ptr.is_null());
        assert_eq!((setun_width(), setun_height()), (640, 480));
        let n = setun_serial_take();
        let out = with(|e| String::from_utf8(e.serial.clone()).unwrap());
        assert_eq!(n as usize, out.len());
        assert!(out.starts_with("Setun-386 demo ROM ready"));
        for c in "Привет".chars() {
            setun_serial_char(c as u32);
        }
        setun_run(100_000);
        setun_serial_take();
        assert_eq!(with(|e| String::from_utf8(e.serial.clone()).unwrap()), "Привет");
        setun_key(43, 1);
        setun_run(10_000);
        assert_eq!(setun_display_mode(), 0, "Tab switched to graphics");
        assert_eq!(setun_ring(), 1);
    }
}
