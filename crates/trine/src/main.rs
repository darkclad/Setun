//! `trine`: compile and run Trine programs.
//!
//! ```text
//! trine build FILE.tri [-o OUT.t1b2] [-S OUT.s] [--boot | --bios | --user] [--no-opt] [--data N] [--stack N]
//! trine run   FILE.tri [--boot] [--steps N] [--input TEXT]
//! ```

use std::path::PathBuf;
use std::process::exit;
use trine::{bios_rom, build_file, run_machine, Kind, Options, Outcome};

fn usage() -> ! {
    eprintln!(
        "usage:\n  trine build FILE.tri [-o OUT.t1b2] [-S OUT.s] [--boot | --bios | --user] [--no-opt] [--data N] [--stack N]\n  trine run   FILE.tri [--boot] [--steps N] [--input TEXT]\n\n\
         build    compile to a ROM image (run it with `setun --rom OUT.t1b2`)\n\
         \x20        --boot: a bootable one-sector disk image instead (`setun --disk OUT.t1b2`)\n\
         \x20        --bios: the BIOS memory layout (frame 1 left free for the boot sector)\n\
         \x20        --user: a Trinux program's layout (its flat image; mkdisk makes the TEF)\n\
         \x20        --no-opt: without the optimizer\n\
         run      compile and run on an emulated machine without a window; prints the\n\
         \x20        serial output and how the program ended. --boot boots it through the BIOS\n\
         -S F     also write the generated assembly to F"
    );
    exit(2)
}

fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("{msg}");
    exit(1)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, rest) = match args.split_first() {
        Some((c, r)) => (c.as_str(), r),
        None => usage(),
    };
    let mut input: Option<PathBuf> = None;
    let (mut out, mut asm_out, mut steps, mut text) = (None, None, 100_000_000u64, String::new());
    let mut opts = Options::rom();
    let (mut data, mut stack) = (None, None);
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().unwrap_or_else(|| usage());
        match a.as_str() {
            "-o" => out = Some(PathBuf::from(val())),
            "-S" => asm_out = Some(PathBuf::from(val())),
            "--steps" => steps = val().parse().unwrap_or_else(|_| usage()),
            "--input" => text = val().replace("\\n", "\n"),
            "--boot" => opts = Options::boot(),
            "--bios" => opts = Options::bios(),
            "--user" => opts = Options { optimize: opts.optimize, ..Options::user() },
            "--no-opt" => opts.optimize = false,
            "--data" => data = Some(val().parse().unwrap_or_else(|_| usage())),
            "--stack" => stack = Some(val().parse().unwrap_or_else(|_| usage())),
            _ if input.is_none() && !a.starts_with('-') => input = Some(PathBuf::from(a)),
            _ => usage(),
        }
    }
    opts.data = data.unwrap_or(opts.data);
    opts.stack = stack.unwrap_or(opts.stack);
    let input = input.unwrap_or_else(|| usage());
    let build = build_file(&input, &opts).unwrap_or_else(|e| fail(e));
    if let Some(p) = &asm_out {
        std::fs::write(p, &build.asm).unwrap_or_else(|e| fail(format!("{}: {e}", p.display())));
    }
    match cmd {
        "build" => {
            let out = out.unwrap_or_else(|| input.with_extension("t1b2"));
            let bytes: Vec<u8> = build.image.iter().flat_map(|t| t.to_le_bytes()).collect();
            std::fs::write(&out, bytes).unwrap_or_else(|e| fail(format!("{}: {e}", out.display())));
            let words: usize = build.program.segments.iter().map(|s| s.words.len()).sum();
            let what = match opts.kind {
                Kind::Boot => "boot disk",
                Kind::User => "Trinux program image",
                _ => "ROM",
            };
            eprintln!("{} → {} ({what}, {words} words)", input.display(), out.display());
        }
        "run" => {
            let r = if opts.kind == Kind::Boot {
                let bios = bios_rom().unwrap_or_else(|e| fail(format!("BIOS: {e}")));
                run_machine(&bios, Some(&build.image), steps, &text)
            } else {
                run_machine(&build.image, None, steps, &text)
            };
            print!("{}", r.serial);
            match r.outcome {
                Outcome::Exit(code) => {
                    eprintln!("[exit {code} after {} instructions]", r.steps);
                    exit(code.clamp(0, 255) as i32)
                }
                Outcome::Fault { vector, epc, faddr } => {
                    eprintln!("[CPU fault: vector {vector} at {epc}, address {faddr}]");
                    exit(101)
                }
                Outcome::Running => eprintln!("[still running after {} instructions]", r.steps),
            }
        }
        _ => usage(),
    }
}
