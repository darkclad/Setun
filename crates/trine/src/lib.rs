//! Trine: the systems language of Setun-26 (`docs/LANGUAGE.md`).
//!
//! Pipeline: [`lexer`] → [`parser`] → [`compile`] (type checking + code
//! generation to assembly text) → `setun-asm` → a T1B2 image. The driver adds
//! the startup code (`crt0`), resolves `import`s (relative files first, then
//! the built-in standard library) and lays out memory.

pub mod ast;
pub mod compile;
mod peephole;
pub mod lexer;
pub mod parser;

use setun_core::Machine;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// A compile error at a source position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub pos: lexer::Pos,
    pub msg: String,
}

/// The standard library, embedded so programs build anywhere.
pub const STDLIB: &[(&str, &str)] = &[
    ("ports.tri", include_str!("../lib/ports.tri")),
    ("rt.tri", include_str!("../lib/rt.tri")),
    ("console.tri", include_str!("../lib/console.tri")),
    ("keymap.tri", include_str!("../lib/keymap.tri")),
    ("real.tri", include_str!("../lib/real.tri")),
    ("math.tri", include_str!("../lib/math.tri")),
];

/// What the build produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A full BIOS ROM image (3^10 trytes), code at the reset vector.
    Rom,
    /// A one-sector boot disk image: the BIOS loads it into frame 1 and
    /// jumps to `BOOT_ENTRY` (see `docs/BOOT.md`).
    Boot,
    /// The program's words from `org − 1` (e.g. a kernel image for a loader).
    Flat,
    /// A user program for Trinux: flat image, unprivileged startup code that
    /// ends with the `exit` system call (93).
    User,
}

/// Trinux user-space layout (docs/KERNEL.md §3.1).
pub const USER_BASE: i64 = -847_288_609_443; // −3^25
pub const USER_DATA: i64 = USER_BASE + 27 * 19_683;
pub const USER_STACK: i64 = -387_420_492; // −3^18 − 3

/// Memory layout of a compiled program.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Word address of the first instruction (`_start`).
    pub org: i64,
    /// Word address of the first global.
    pub data: i64,
    /// Initial stack pointer (the stack grows down).
    pub stack: i64,
    pub kind: Kind,
    /// Optimize the generated code (on by default; off for comparisons).
    pub optimize: bool,
}

/// Boot protocol (docs/BOOT.md): sector 0 of disk 0 is loaded into frame 1.
pub const BOOT_FRAME_START: i64 = 9_842;
/// Entry point: the centre of frame 1.
pub const BOOT_ENTRY: i64 = 19_683;
/// Word at the end of the boot sector that marks it bootable.
/// `0zB007AB13` in base 27: BOOTABLE, with 7 = T, 1 = L and 3 = E.
pub const BOOT_MAGIC: i64 = 115_067_810_199;
pub const BOOT_MAGIC_ADDR: i64 = 29_523;

impl Options {
    /// A ROM program: code at the reset vector −3^10, globals from the start
    /// of RAM frame 1 upward, stack below them in frame 0.
    pub fn rom() -> Self {
        Options { org: -59_049, data: 9_843, stack: 9_840, kind: Kind::Rom, optimize: true }
    }

    /// The BIOS: like `rom()`, but frame 1 stays free for the boot sector;
    /// the stack is in frame 2 and globals start in frame 3.
    pub fn bios() -> Self {
        Options { org: -59_049, data: 49_209, stack: 49_206, kind: Kind::Rom, optimize: true }
    }

    /// A boot sector program: code from the centre of frame 1, stack in
    /// frame 2, globals from frame 3.
    pub fn boot() -> Self {
        Options { org: BOOT_ENTRY, data: 49_209, stack: 49_206, kind: Kind::Boot, optimize: true }
    }

    /// A Trinux user program: code at `USER_BASE`, globals 27 pages higher,
    /// stack just below −3^18.
    pub fn user() -> Self {
        Options { org: USER_BASE, data: USER_DATA, stack: USER_STACK, kind: Kind::User, optimize: true }
    }
}

/// The result of a build.
pub struct Build {
    pub asm: String,
    pub program: setun_asm::text::Program,
    /// T1B2 trytes: a ROM image, or the program's words from `org − 1`.
    pub image: Vec<i16>,
    /// Globals occupy word addresses `data_start .. data_end` (exclusive).
    pub data_start: i64,
    pub data_end: i64,
}

struct Loader {
    names: Vec<String>,
    seen: HashSet<String>,
    items: Vec<ast::Item>,
}

impl Loader {
    fn load(&mut self, name: String, src: &str, dir: Option<PathBuf>) -> Result<(), String> {
        let file = self.names.len() as u16;
        self.names.push(name);
        let toks = lexer::lex(src, file).map_err(|e| self.fmt(&e))?;
        let items = parser::Parser::new(toks).program().map_err(|e| self.fmt(&e))?;
        let mut own = Vec::new();
        for it in items {
            if let ast::Item::Import(path, pos) = &it {
                let local = dir.as_ref().map(|d| d.join(path)).filter(|p| p.is_file());
                let (key, text, sub) = if let Some(p) = local {
                    let key = std::fs::canonicalize(&p).unwrap_or(p.clone()).to_string_lossy().into_owned();
                    let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                    (key, text, p.parent().map(Path::to_path_buf))
                } else if let Some((_, text)) = STDLIB.iter().find(|(n, _)| n == path) {
                    (format!("<std>/{path}"), text.to_string(), None)
                } else {
                    return Err(self.fmt(&Error { pos: *pos, msg: format!("cannot find `{path}`") }));
                };
                if self.seen.insert(key.clone()) {
                    self.load(key, &text, sub)?;
                }
            } else {
                own.push(it);
            }
        }
        self.items.extend(own);
        Ok(())
    }

    fn fmt(&self, e: &Error) -> String {
        let name = self.names.get(e.pos.file as usize).map_or("?", |s| s.as_str());
        format!("{name}:{}:{}: {}", e.pos.line, e.pos.col, e.msg)
    }
}

/// Tags crt0 writes to `SYSCTL.DEBUG` before the exit code / fault details.
pub const EXIT_TAG: i64 = -999_999_001;
pub const FAULT_TAG: i64 = -999_999_002;

fn crt0(o: &Options, data_start: i64, data_end: i64) -> String {
    format!(
        "\
; ---- crt0: generated by trine ------------------------------------------------
        .org {org}
_start: mov   r-1, r1                 ; boot arguments (disk, RAM pages) for main
        mov   r-2, r2
        li    sp, {stack}
        la    r1, _trine_vectors + 363
        mtc   itb, r1
        li    r1, {data_start}
        li    r2, {data_end}
_trine_zero:
        cmp   r1, r2
        bge   _trine_zeroed
        stw   r0, [r1]
        add   r1, r1, 3
        b     _trine_zero
_trine_zeroed:
        call  _trine_init
        mov   r1, r-1
        mov   r2, r-2
        call  main
        mov   r2, r1
        li    r1, {exit_tag}
        out   r1, 3                   ; SYSCTL.DEBUG: exit tag
        out   r2, 3                   ; exit code
        li    r1, -1
        out   r1, 2                   ; SYSCTL.POWER: off
_trine_halt:
        hlt
        b     _trine_halt
_trine_fault:
        li    r1, {fault_tag}
        out   r1, 3                   ; fault tag
        mfc   r1, fcause
        ext   r1, r1, 23, 5
        out   r1, 3                   ; vector
        mfc   r1, epc
        out   r1, 3
        mfc   r1, faddr
        out   r1, 3
        li    r1, -1
        out   r1, 2
        b     _trine_halt
; ---- program -----------------------------------------------------------------
",
        org = o.org,
        stack = o.stack,
        exit_tag = EXIT_TAG,
        fault_tag = FAULT_TAG
    )
}

/// Startup code for Trinux user programs: no privileged instructions.
fn crt0_user(o: &Options, data_start: i64, data_end: i64, has_environ: bool) -> String {
    let environ = if has_environ { "        stw   r-3, [environ]          ; envp, for getenv\n" } else { "" };
    format!(
        "\
; ---- crt0 (user): generated by trine ------------------------------------------
        .org {org}
_start: mov   r-1, r1                 ; argc, argv, envp (set up by the kernel, as is sp)
        mov   r-2, r2
        mov   r-3, r3
        li    r1, {data_start}
        li    r2, {data_end}
_trine_zero:
        cmp   r1, r2
        bge   _trine_zeroed
        stw   r0, [r1]
        add   r1, r1, 3
        b     _trine_zero
_trine_zeroed:
        call  _trine_init
{environ}        mov   r1, r-1
        mov   r2, r-2
        call  main
        sys   93                      ; exit(main's result)
_trine_halt:
        b     _trine_halt
; ---- program -----------------------------------------------------------------
",
        org = o.org
    )
}

const VECTORS: &str = "\
; ---- fault vector table (centre a multiple of 729) -----------------------------
        .align 729
        .zero 122
_trine_vectors:
        .fill 243, _trine_fault
";

fn finish(loader: Loader, opts: &Options) -> Result<Build, String> {
    let mut c = compile::Compiler::new();
    c.optimize = opts.optimize;
    let out = c
        .compile(&loader.items, &compile::Layout { data_org: opts.data })
        .map_err(|e| if e.pos == lexer::Pos::default() { e.msg.clone() } else { loader.fmt(&e) })?;
    let asm = if opts.kind == Kind::User {
        crt0_user(opts, out.data_start, out.data_end, out.has_environ) + &out.text
    } else {
        crt0(opts, out.data_start, out.data_end) + &out.text + VECTORS
    };
    let program = setun_asm::text::assemble(&asm).map_err(|e| format!("internal assembler error: {e}"))?;
    let image = match opts.kind {
        Kind::Rom => setun_asm::rom_image(&program.segments)?,
        Kind::Boot => boot_sector(&program.segments)?,
        Kind::Flat | Kind::User => match program.segments.as_slice() {
            [s] if s.origin == opts.org => setun_asm::words_to_trytes(&s.words),
            _ => return Err("a flat image must be one segment starting at its origin".into()),
        },
    };
    Ok(Build { asm, program, image, data_start: out.data_start, data_end: out.data_end })
}

/// One boot sector (3^9 trytes) holding the program at its load address in
/// frame 1, with the boot magic in its last word.
fn boot_sector(segments: &[setun_asm::text::Segment]) -> Result<Vec<i16>, String> {
    let page = setun_core::PAGE as usize;
    let mut sector = vec![0i16; page];
    let limit = (BOOT_MAGIC_ADDR - 1 - BOOT_FRAME_START) as usize; // the magic's first tryte
    for s in segments {
        let first = s.origin - 1 - BOOT_FRAME_START;
        let t = setun_asm::words_to_trytes(&s.words);
        if first < 0 || first as usize + t.len() > limit {
            return Err(format!(
                "the boot program ({} words at {}) does not fit in the boot sector (addresses {}..{})",
                s.words.len(),
                s.origin,
                BOOT_FRAME_START,
                BOOT_MAGIC_ADDR - 2
            ));
        }
        sector[first as usize..first as usize + t.len()].copy_from_slice(&t);
    }
    let magic = setun_core::memory::word_to_trytes(ternary::Word::new(BOOT_MAGIC).unwrap());
    for (i, t) in magic.iter().enumerate() {
        sector[limit + i] = t.value() as i16;
    }
    Ok(sector)
}

/// Build a program from source text. Imports resolve against the standard
/// library (and `dir`, when given).
pub fn build_source(name: &str, src: &str, dir: Option<&Path>, opts: &Options) -> Result<Build, String> {
    let mut l = Loader { names: Vec::new(), seen: HashSet::new(), items: Vec::new() };
    l.load(name.to_string(), src, dir.map(Path::to_path_buf))?;
    finish(l, opts)
}

/// The BIOS source (`bios/bios.tri`).
pub const BIOS_SOURCE: &str = include_str!("../../../bios/bios.tri");

/// Compile the BIOS into a ROM image.
pub fn bios_rom() -> Result<Vec<i16>, String> {
    build_source("bios/bios.tri", BIOS_SOURCE, None, &Options::bios()).map(|b| b.image)
}

/// Build a program from a file.
pub fn build_file(path: &Path, opts: &Options) -> Result<Build, String> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    build_source(&path.display().to_string(), &src, path.parent(), opts)
}

/// How a program run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// `main` returned this value.
    Exit(i64),
    /// A CPU fault: vector, EPC, FADDR.
    Fault { vector: i64, epc: i64, faddr: i64 },
    /// Still running after the step limit.
    Running,
}

pub struct Run {
    pub outcome: Outcome,
    pub serial: String,
    pub steps: u64,
    pub machine: Machine,
}

/// Run a ROM image on a fresh machine (3^6 pages of RAM) for up to
/// `max_steps` instructions, feeding `input` to the serial console.
pub fn run_rom(image: &[i16], max_steps: u64, input: &str) -> Run {
    run_machine(image, None, max_steps, input)
}

/// Like [`run_rom`], with an optional disk image (T1B2 trytes) as disk 0.
pub fn run_machine(rom: &[i16], disk: Option<&[i16]>, max_steps: u64, input: &str) -> Run {
    let mut m = Machine::new(729, 1_790_000_000);
    m.load_rom(rom);
    if let Some(d) = disk {
        m.dev.disks[0].attach(Box::new(setun_core::devices::MemDisk::from_trytes(d)));
    }
    m.dev.serial_input(input);
    let mut steps = 0;
    while steps < max_steps && !m.powered_off {
        m.step();
        steps += 1;
    }
    let log = &m.dev.debug_log;
    let outcome = match log.as_slice() {
        [.., EXIT_TAG, code] if m.powered_off => Outcome::Exit(*code),
        [.., FAULT_TAG, v, epc, faddr] if m.powered_off => Outcome::Fault { vector: *v, epc: *epc, faddr: *faddr },
        _ => Outcome::Running,
    };
    let serial = m.dev.serial.take_output();
    Run { outcome, serial, steps, machine: m }
}
