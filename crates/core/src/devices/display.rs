//! Display controller and renderer (ISA §8.2, §8.3).
//!
//! Registers: 0 `MODE`: graphics at the VESA sizes 0 = 640×480,
//! +1 = 800×600, +2 = 1024×768, +3 = 1280×1024, +4 = 1600×1200,
//! +5 = 1920×1080, +6 = 1920×1200, and text at the same sizes in 8×16
//! cells, −1 (80×30) to −7 (240×75): text mode −(m + 1) fills the screen of
//! graphics mode m. Other values leave the mode as it is.
//! 1 `FB_BASE`, 2 `WIDTH`, 3 `HEIGHT`, 4 `PITCH`, 5 `FRAME`, 6 `CTRL`,
//! 7 `CURSOR`, 8 `HWC_X`, 9 `HWC_Y`, 10 `HWC_ON`, 11 `PREFERRED`: the
//! graphics mode of the monitor's own size (read-only; the front end sets
//! it, as a monitor reports its size), 0 = 640×480 unless told.
//!
//! In text mode `WIDTH`/`HEIGHT` count cells and `PITCH` is three trytes a
//! cell (one word). Rendering happens host-side into 0xRRGGBB pixels.

use crate::memory::{frame_start, Memory, FB_FIRST_FRAME};
use std::sync::OnceLock;
use ternary::{wrap_trits, Tryte};

/// Microseconds per video frame (60 Hz).
pub const FRAME_US: i64 = 16_667;

/// Text-mode blink half-period in host milliseconds.
pub const BLINK_MS: u64 = 500;

/// The largest `MODE`: graphics at 1920×1200.
pub const MAX_MODE: i64 = 6;
/// The smallest: text at 1920×1200 (240×75 cells).
pub const MIN_MODE: i64 = -MAX_MODE - 1;

pub struct Display {
    mode: i64,
    frame: i64,
    irq_on: bool,
    cursor: i64,
    hwc: (i64, i64, i64),
    preferred: i64,
    next_vsync_us: i64,
    /// Host wall-clock time in milliseconds, set by the front end before
    /// rendering. Blinking follows it when present, so the cursor blinks at
    /// a steady rate however fast the emulator runs; without it (headless,
    /// tests) blinking follows emulated frames.
    wall_ms: Option<u64>,
}

impl Default for Display {
    fn default() -> Self {
        Self::new()
    }
}

impl Display {
    pub fn new() -> Self {
        Display { mode: -1, frame: 0, irq_on: false, cursor: 0, hwc: (0, 0, 0), preferred: 0, next_vsync_us: FRAME_US, wall_ms: None }
    }

    /// Physical address of the first framebuffer tryte.
    pub fn fb_base() -> i64 {
        frame_start(FB_FIRST_FRAME)
    }

    pub fn mode(&self) -> i64 {
        self.mode
    }

    /// The monitor's size, as graphics mode `m` (0..=MAX_MODE), for
    /// `PREFERRED`: the front end's part, before the machine boots.
    pub fn set_preferred(&mut self, m: i64) {
        self.preferred = m.clamp(0, MAX_MODE);
    }

    pub fn preferred(&self) -> i64 {
        self.preferred
    }

    /// The graphics mode with `m`'s pixels: `m` itself, or for text mode m
    /// the one it fills.
    fn screen_of(m: i64) -> i64 {
        if m < 0 {
            -m - 1
        } else {
            m
        }
    }

    /// Text cells of text mode `m` (−1..=MIN_MODE): 8×16 pixels each.
    pub fn text_size(m: i64) -> (usize, usize) {
        let (w, h) = Self::mode_size(Self::screen_of(m));
        (w / 8, h / 16)
    }

    /// (width, height, pitch) as the registers report them.
    pub fn geometry(&self) -> (i64, i64, i64) {
        match self.mode {
            m if m < 0 => {
                let (c, r) = Self::text_size(m);
                (c as i64, r as i64, 3 * c as i64)
            }
            m => {
                let (w, h) = Self::mode_size(m);
                (w as i64, h as i64, w as i64)
            }
        }
    }

    /// The pixels of graphics mode `m` (0..=6).
    pub fn mode_size(m: i64) -> (usize, usize) {
        match m {
            1 => (800, 600),
            2 => (1024, 768),
            3 => (1280, 1024),
            4 => (1600, 1200),
            5 => (1920, 1080),
            6 => (1920, 1200),
            _ => (640, 480),
        }
    }

    /// Output size in pixels.
    pub fn pixel_size(&self) -> (usize, usize) {
        Self::mode_size(Self::screen_of(self.mode))
    }

    /// Advance to `now_us`; true on a vsync with the IRQ enabled.
    /// Microseconds of advance until the next vsync interrupt, if enabled.
    /// (Without the interrupt, frames are counted the same in one go.)
    pub fn us_until_vsync_irq(&self, now_us: i64) -> Option<i64> {
        self.irq_on.then(|| (self.next_vsync_us - now_us).max(1))
    }

    pub fn advance(&mut self, now_us: i64) -> bool {
        let mut fire = false;
        while now_us >= self.next_vsync_us {
            self.frame += 1;
            self.next_vsync_us += FRAME_US;
            fire = self.irq_on;
        }
        fire
    }

    pub fn read(&self, r: i64) -> i64 {
        let (w, h, p) = self.geometry();
        match r {
            0 => self.mode,
            1 => Self::fb_base(),
            2 => w,
            3 => h,
            4 => p,
            5 => self.frame,
            6 => self.irq_on as i64,
            7 => self.cursor,
            8 => self.hwc.0,
            9 => self.hwc.1,
            10 => self.hwc.2,
            11 => self.preferred,
            _ => 0,
        }
    }

    pub fn write(&mut self, r: i64, v: i64) {
        match r {
            0 if (MIN_MODE..=MAX_MODE).contains(&v) => self.mode = v,
            6 => self.irq_on = v == 1,
            7 => self.cursor = v,
            8 => self.hwc.0 = v,
            9 => self.hwc.1 = v,
            10 => self.hwc.2 = v,
            _ => {}
        }
    }

    /// Tie blinking to the host clock (`Some(ms)`) or to emulated frames (`None`).
    pub fn set_wall_ms(&mut self, ms: Option<u64>) {
        self.wall_ms = ms;
    }

    /// Blink phase: half a second on, half a second off.
    fn blink_off(&self) -> bool {
        match self.wall_ms {
            Some(ms) => (ms / BLINK_MS) % 2 == 1,
            None => (self.frame / 30) % 2 == 1,
        }
    }

    /// Render the current screen into `out` (0xRRGGBB, row-major).
    /// Returns (width, height) in pixels.
    pub fn render(&self, mem: &Memory, out: &mut Vec<u32>) -> (usize, usize) {
        let (w, h) = self.pixel_size();
        out.clear();
        out.resize(w * h, 0);
        let fb = mem.framebuffer();
        if self.mode < 0 {
            self.render_text(fb, out, w);
        } else {
            let pitch = self.geometry().2 as usize;
            let pal = palette();
            for y in 0..h {
                let row = &fb[y * pitch..y * pitch + w];
                for (o, &t) in out[y * w..(y + 1) * w].iter_mut().zip(row) {
                    *o = pal[(t as i32 + 9841) as usize];
                }
            }
        }
        if self.hwc.2 == 1 {
            draw_arrow(out, w, h, self.hwc.0, self.hwc.1);
        }
        (w, h)
    }

    fn render_text(&self, fb: &[i16], out: &mut [u32], w: usize) {
        let blink_off = self.blink_off();
        let (cols, rows) = Self::text_size(self.mode);
        for row in 0..rows {
            for col in 0..cols {
                let cell = row * cols + col;
                let ch = fb[3 * cell] as i64;
                let at = Tryte::new(fb[3 * cell + 1] as i64).unwrap_or(Tryte::ZERO).trits();
                let fg = trit_rgb(at[0] as i64, at[1] as i64, at[2] as i64);
                let bg = trit_rgb(at[3] as i64, at[4] as i64, at[5] as i64);
                let (underline, blink, bold) = (at[6] as i64 == 1, at[7] as i64 == 1, at[8] as i64 == 1);
                let glyph = if blink && blink_off { [0u8; 8] } else { glyph(ch) };
                let cursor_here = cell as i64 == self.cursor && !blink_off;
                for gy in 0..16 {
                    let mut bits = glyph[gy / 2];
                    if bold {
                        bits |= bits << 1;
                    }
                    let solid = (underline && gy == 15) || (cursor_here && gy >= 14);
                    let o = (row * 16 + gy) * w + col * 8;
                    for gx in 0..8 {
                        let on = solid || bits & (1 << gx) != 0;
                        out[o + gx] = if on { fg } else { bg };
                    }
                }
            }
        }
    }
}

/// pixel_rgb for every tryte value, computed once (indexed by value + 9841).
fn palette() -> &'static [u32] {
    static PAL: OnceLock<Vec<u32>> = OnceLock::new();
    PAL.get_or_init(|| (-9841..=9841).map(pixel_rgb).collect())
}

/// Colour of a graphics pixel tryte: trits 1–3 R, 4–6 G, 7–9 B, each
/// −13..13 → 27 levels.
pub fn pixel_rgb(t: i64) -> u32 {
    let v = t as i128;
    let b = wrap_trits(v, 3);
    let v = (v - b) / 27;
    let g = wrap_trits(v, 3);
    let r = (v - g) / 27;
    let lvl = |c: i128| (((c + 13) * 255 + 13) / 26) as u32;
    (lvl(r) << 16) | (lvl(g) << 8) | lvl(b)
}

/// Text-mode colour: one trit per channel (0%, 50%, 100%).
pub fn trit_rgb(r: i64, g: i64, b: i64) -> u32 {
    let lvl = |c: i64| match c {
        -1 => 0u32,
        0 => 128,
        _ => 255,
    };
    (lvl(r) << 16) | (lvl(g) << 8) | lvl(b)
}

/// 8×8 glyph for a character tryte (a Unicode code point); bit 0 is the
/// leftmost pixel. Unknown characters show as a hollow box. A negative
/// tryte is the character overlined (see `font::overlined`).
fn glyph(ch: i64) -> [u8; 8] {
    static TABLE: OnceLock<Vec<Option<[u8; 8]>>> = OnceLock::new();
    const BOX: [u8; 8] = [0x00, 0x7E, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00];
    if ch < -32 {
        return super::overlined(glyph(-ch));
    }
    if ch <= 32 {
        return [0; 8];
    }
    let table = TABLE.get_or_init(|| (0..=9841u32).map(super::glyph8x8).collect());
    table[ch as usize].unwrap_or(BOX)
}

/// Hardware mouse cursor: a 1-pixel-outlined arrow.
fn draw_arrow(out: &mut [u32], w: usize, h: usize, x0: i64, y0: i64) {
    const ARROW: [&str; 12] = [
        "X.........",
        "XX........",
        "XoX.......",
        "XooX......",
        "XoooX.....",
        "XooooX....",
        "XoooooX...",
        "XooooooX..",
        "XoooXXXXX.",
        "XooX......",
        "XoX.......",
        "XX........",
    ];
    for (dy, line) in ARROW.iter().enumerate() {
        for (dx, c) in line.bytes().enumerate() {
            let (x, y) = (x0 + dx as i64, y0 + dy as i64);
            if c == b'.' || x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                continue;
            }
            out[y as usize * w + x as usize] = if c == b'X' { 0x000000 } else { 0xFFFFFF };
        }
    }
}
