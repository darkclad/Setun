//! setun-bench: emulator speed on fixed workloads, plus a hash of the whole
//! machine state after each one.
//!
//!   setun-bench            run all workloads, compare with golden.txt
//!   setun-bench --record   run them and write golden.txt
//!   setun-bench NAME...    run only these workloads
//!
//!   setun-bench --profile FILE.tri   where a ROM program spends its time
//!
//! SETUN_JIT_OFF=1 runs every workload on the interpreter alone.
//!
//! The emulator is deterministic (time is counted in instructions), so an
//! optimisation that does not change behaviour must reproduce every hash
//! and step count exactly.

mod micro;
mod profile;
mod vs_native;

use setun_core::Machine;
use std::path::{Path, PathBuf};
use std::time::Instant;

const CHUNK: u64 = 1_000_000;

/// The shell workload's script.
const SHELL_SCRIPT: &str = "md5sum /bin/calc; sort /etc/pangrams | wc; calc 'atan(1)*4'; ls -l /bin | wc -l";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn golden_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("golden.txt")
}

/// FNV-1a over i64 values.
struct Hash(u64);

impl Hash {
    fn add(&mut self, v: i64) {
        for b in v.to_le_bytes() {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x100_0000_01b3);
        }
    }
}

/// Everything observable: registers, control registers, FPU, RAM, the
/// framebuffer, emulated time and the serial output so far.
fn state_hash(m: &Machine, serial: &str) -> u64 {
    let mut h = Hash(0xcbf2_9ce4_8422_2325);
    for r in -13..=13 {
        h.add(m.cpu.reg(r).value());
        let (w0, w1, w2) = {
            let w = m.cpu.freg(r).to_words();
            (w[0], w[1], w[2])
        };
        h.add(w0);
        h.add(w1);
        h.add(w2);
        h.add(m.cpu.cr(r).value());
    }
    h.add(m.cpu.pc.value());
    h.add(m.cpu.fsr().value());
    h.add(m.cpu.halted as i64);
    let page = setun_core::PAGE;
    for frame in 0..m.mem.ram_frames() {
        let base = frame * page;
        let mut a = base - 9840;
        while a <= base + 9840 {
            h.add(m.mem.read_word(a).map(|w| w.value()).unwrap_or(0));
            a += 3;
        }
    }
    for &t in m.mem.framebuffer() {
        h.add(t as i64);
    }
    h.add(m.dev.now_us());
    for c in serial.chars() {
        h.add(c as i64);
    }
    h.0
}

struct Outcome {
    /// For ROM programs: a hash of what they produce (debug log with the
    /// exit code, serial output, framebuffer), which must not change when
    /// only the compiler's code generation does. 0 for Trinux workloads.
    result: u64,
    jit: [u64; 4],
    name: &'static str,
    steps: u64,
    instructions: u64,
    secs: f64,
    hash: u64,
}

fn instructions(m: &Machine) -> u64 {
    m.cpu.cr(setun_core::isa::cr::CYCLE).value() as u64
}

/// Run a ROM program until it powers off or halts (or `cap` steps).
fn rom(name: &'static str, path: &Path, cap: u64) -> Outcome {
    let b = trine::build_file(path, &trine::Options::rom()).unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut m = Machine::new(729, 1_790_000_000);
    m.load_rom(&b.image);
    if std::env::var_os("SETUN_JIT_OFF").is_some() {
        m.cpu.set_jit_threshold(u32::MAX);
    }
    let t = Instant::now();
    let mut steps = 0;
    // until it powers off, or halts for good (mandel ends in HLT)
    while steps < cap && !m.powered_off && !m.cpu.halted {
        steps += m.run(CHUNK.min(cap - steps));
    }
    let secs = t.elapsed().as_secs_f64();
    let serial = m.dev.serial.take_output();
    let mut r = Hash(0xcbf2_9ce4_8422_2325);
    for &v in &m.dev.debug_log {
        r.add(v);
    }
    for c in serial.chars() {
        r.add(c as i64);
    }
    // the 27 frames the framebuffer had before ISA v0.8, then only what is
    // drawn past them, so a program's outcome does not depend on the size
    let fb = m.mem.framebuffer();
    let old = 27 * setun_core::PAGE as usize;
    for &t in &fb[..old] {
        r.add(t as i64);
    }
    for &t in fb[old..].iter().filter(|&&t| t != 0) {
        r.add(t as i64);
    }
    let mut out = Outcome { result: r.0, jit: m.cpu.jit_stats(), name, steps, instructions: instructions(&m), secs, hash: 0 };
    out.hash = state_hash(&m, &serial);
    out
}

/// Trinux: boot to the shell (logging in as root), run a script, then idle.
fn trinux() -> Vec<Outcome> {
    let disk = trinux::build_disk(&trinux::kernel_dir()).expect("disk builds");
    let bios = trine::bios_rom().expect("BIOS builds");
    let mut m = Machine::new(729, 1_790_000_000);
    if std::env::var_os("SETUN_JIT_OFF").is_some() {
        m.cpu.set_jit_threshold(u32::MAX);
    }
    m.load_rom(&bios);
    m.dev.disks[0].attach(Box::new(setun_core::devices::MemDisk::from_trytes(&disk.trytes)));
    let mut serial = String::new();
    let mut results = Vec::new();

    // until `prompts` shell prompts have appeared
    let phase = |m: &mut Machine, serial: &mut String, name: &'static str, input: Option<&str>, cap: u64| {
        if let Some(s) = input {
            m.dev.serial_input(s);
        }
        let start_steps_instr = instructions(m);
        let t = Instant::now();
        let mut steps = 0;
        loop {
            steps += m.run(CHUNK);
            serial.push_str(&m.dev.serial.take_output());
            if serial.ends_with("login: ") || serial.ends_with("Password: ") {
                m.dev.serial_input("root\n");
            } else if serial.ends_with("$ ") || steps >= cap {
                break;
            }
        }
        let secs = t.elapsed().as_secs_f64();
        Outcome {
            result: 0,
            jit: m.cpu.jit_stats(),
            name,
            steps,
            instructions: instructions(m) - start_steps_instr,
            secs,
            hash: state_hash(m, serial),
        }
    };
    results.push(phase(&mut m, &mut serial, "boot", None, 3_000_000_000));
    results.push(phase(
        &mut m,
        &mut serial,
        "shell",
        Some(&format!("{SHELL_SCRIPT}\n")),
        3_000_000_000,
    ));
    // the same again: programs exec'd before (their text cached, their
    // code compiled)
    results.push(phase(&mut m, &mut serial, "again", Some(&format!("{SHELL_SCRIPT}\n")), 3_000_000_000));
    // idle at the prompt: mostly HLT, woken by the timer
    let t = Instant::now();
    let i0 = instructions(&m);
    let mut steps = 0;
    while steps < 300_000_000 {
        steps += m.run(CHUNK);
    }
    serial.push_str(&m.dev.serial.take_output());
    results.push(Outcome {
        result: 0,
        jit: m.cpu.jit_stats(),
        name: "idle",
        steps,
        instructions: instructions(&m) - i0,
        secs: t.elapsed().as_secs_f64(),
        hash: state_hash(&m, &serial),
    });
    results
}

/// Print the source location (with inlined frames) of each RVA in this
/// executable, for reading profiles.
fn resolve(rvas: &[String]) {
    extern "system" {
        fn GetModuleHandleW(name: *const u16) -> usize;
    }
    let base = unsafe { GetModuleHandleW(std::ptr::null()) };
    for r in rvas {
        let rva = usize::from_str_radix(r.trim_start_matches("0x"), 16).expect("hex RVA");
        let mut frames = Vec::new();
        backtrace::resolve((base + rva) as *mut std::ffi::c_void, |s| {
            let name = s.name().map(|n| format!("{n:#}")).unwrap_or_default();
            let at = match (s.filename(), s.lineno()) {
                (Some(f), Some(l)) => format!("{}:{l}", f.file_name().unwrap().to_string_lossy()),
                _ => String::new(),
            };
            frames.push(format!("{name} @ {at}"));
        });
        println!("{r}: {}", frames.join("  <=  "));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--resolve") {
        resolve(&args[1..]);
        return;
    }
    if args.first().map(String::as_str) == Some("--profile-trinux") {
        let script = args.get(1).map_or(SHELL_SCRIPT, String::as_str);
        profile::trinux(&format!("{}\n", script.trim_end()));
        return;
    }
    if args.first().map(String::as_str) == Some("--profile") {
        profile::run(Path::new(&args[1]));
        return;
    }
    if args.iter().any(|a| a == "--vs-native") {
        vs_native::run(&root());
        return;
    }
    if args.iter().any(|a| a == "--micro") {
        micro::run();
        return;
    }
    let record = args.iter().any(|a| a == "--record");
    let only: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let want = |n: &str| only.is_empty() || only.iter().any(|o| *o == n);

    let r = root();
    let mut all = Vec::new();
    if want("int") {
        all.push(rom("int", &r.join("crates/bench/programs/int.tri"), 2_000_000_000));
    }
    if want("fpu") {
        all.push(rom("fpu", &r.join("crates/bench/programs/fpu.tri"), 2_000_000_000));
    }
    if want("mandel") {
        all.push(rom("mandel", &r.join("examples/mandel.tri"), 2_000_000_000));
    }
    if want("boot") || want("shell") || want("again") || want("idle") {
        all.extend(trinux());
    }

    let golden = std::fs::read_to_string(golden_path()).unwrap_or_default();
    let expect = |name: &str| -> Option<(u64, u64)> {
        golden.lines().find_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            (f.len() == 3 && f[0] == name).then(|| (f[1].parse().unwrap(), u64::from_str_radix(f[2], 16).unwrap()))
        })
    };
    println!("{:<8} {:>12} {:>12} {:>8} {:>9}  {:<16} check", "workload", "steps", "instructions", "secs", "MIPS", "hash");
    let mut bad = 0;
    let mut lines = String::new();
    for o in &all {
        let mips = o.steps as f64 / o.secs / 1e6;
        let check = match expect(o.name) {
            _ if record => "recorded",
            Some((s, h)) if s == o.steps && h == o.hash => "ok",
            Some(_) => {
                bad += 1;
                "MISMATCH"
            }
            None => "no golden",
        };
        println!(
            "{:<8} {:>12} {:>12} {:>8.2} {:>9.1}  {:016x} {check}   jit {:?}",
            o.name, o.steps, o.instructions, o.secs, mips, o.hash, o.jit
        );
        lines.push_str(&format!("{} {} {:016x}\n", o.name, o.steps, o.hash));
    }
    if record {
        std::fs::write(golden_path(), lines).expect("write golden.txt");
    }
    // what the ROM programs produce: independent of code generation
    let outcomes_path = golden_path().with_file_name("outcomes.txt");
    let known = std::fs::read_to_string(&outcomes_path).unwrap_or_default();
    let mut out_lines = String::new();
    for o in all.iter().filter(|o| o.result != 0) {
        let line = format!("{} {:016x}", o.name, o.result);
        let status = match known.lines().find(|l| l.starts_with(&format!("{} ", o.name))) {
            Some(l) if l == line => "result ok",
            Some(_) => {
                bad += 1;
                "RESULT MISMATCH"
            }
            None => "result: none recorded",
        };
        println!("{line}  {status}");
        out_lines.push_str(&line);
        out_lines.push(char::from(10));
    }
    if args.iter().any(|a| a == "--record-outcomes") {
        std::fs::write(&outcomes_path, out_lines).expect("write outcomes.txt");
    }
    #[cfg(feature = "opstats")]
    {
        let c = setun_core::cpu::opstats::report();
        let total: u64 = c.iter().map(|e| e.1).sum();
        for (k, n) in c.iter().take(30) {
            println!("{:>6.2}%  {k}", 100.0 * *n as f64 / total as f64);
        }
    }
    if bad > 0 {
        std::process::exit(1);
    }
}
