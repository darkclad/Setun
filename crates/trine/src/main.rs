//! `trine`: compile and run Trine programs.
//!
//! ```text
//! trine build FILE.tri [-o OUT.t1b2] [-S OUT.s]     a ROM image for `setun --rom`
//! trine run FILE.tri [--steps N] [--input TEXT]     run headless, print serial output
//! ```

use std::path::PathBuf;
use std::process::exit;
use trine::{build_file, run_rom, Options, Outcome};

fn usage() -> ! {
    eprintln!(
        "usage:\n  trine build FILE.tri [-o OUT.t1b2] [-S OUT.s]\n  trine run FILE.tri [--steps N] [--input TEXT]\n\n\
         build   compile to a BIOS ROM image (run it with `setun --rom OUT.t1b2`)\n\
         run     compile and run on an emulated machine without a window; prints the\n\
         \x20       serial output and how the program ended"
    );
    exit(2)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, rest) = match args.split_first() {
        Some((c, r)) => (c.as_str(), r),
        None => usage(),
    };
    let mut input: Option<PathBuf> = None;
    let (mut out, mut asm_out, mut steps, mut text) = (None, None, 100_000_000u64, String::new());
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().unwrap_or_else(|| usage());
        match a.as_str() {
            "-o" => out = Some(PathBuf::from(val())),
            "-S" => asm_out = Some(PathBuf::from(val())),
            "--steps" => steps = val().parse().unwrap_or_else(|_| usage()),
            "--input" => text = val().replace("\\n", "\n"),
            _ if input.is_none() && !a.starts_with('-') => input = Some(PathBuf::from(a)),
            _ => usage(),
        }
    }
    let input = input.unwrap_or_else(|| usage());
    let build = build_file(&input, &Options::rom()).unwrap_or_else(|e| {
        eprintln!("{e}");
        exit(1)
    });
    if let Some(p) = &asm_out {
        std::fs::write(p, &build.asm).unwrap_or_else(|e| {
            eprintln!("{}: {e}", p.display());
            exit(1)
        });
    }
    match cmd {
        "build" => {
            let out = out.unwrap_or_else(|| input.with_extension("t1b2"));
            let bytes: Vec<u8> = build.image.iter().flat_map(|t| t.to_le_bytes()).collect();
            std::fs::write(&out, bytes).unwrap_or_else(|e| {
                eprintln!("{}: {e}", out.display());
                exit(1)
            });
            let words: usize = build.program.segments.iter().map(|s| s.words.len()).sum();
            eprintln!("{} → {} ({words} words)", input.display(), out.display());
        }
        "run" => {
            let r = run_rom(&build.image, steps, &text);
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
