//! Turn crates/core/tests/fast_path.rs into plain functions: each
//! `#[test] fn name()` becomes `pub fn name()`, listed in `TESTS`.

use std::path::PathBuf;

fn main() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fast_path.rs");
    println!("cargo:rerun-if-changed={}", src.display());
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        println!("cargo:rustc-link-arg-cdylib=--export-table");
        println!("cargo:rustc-link-arg-cdylib=--growable-table");
    }
    let text = std::fs::read_to_string(&src).expect("fast_path.rs");
    let mut out = String::new();
    let mut names = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(l) = lines.next() {
        if l.starts_with("//!") {
            continue; // (an inner doc comment cannot sit in an included file)
        }
        if l.trim() == "#[test]" {
            let f = lines.next().expect("a function after #[test]");
            let name = f.trim_start_matches("fn ").split('(').next().unwrap().to_string();
            out += &format!("pub {f}\n");
            names.push(name);
            continue;
        }
        out += l;
        out.push('\n');
    }
    out += "\npub static TESTS: &[(&str, fn())] = &[\n";
    for n in &names {
        out += &format!("    (\"{n}\", {n}),\n");
    }
    out += "];\n";
    let dst = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("fast_path_tests.rs");
    std::fs::write(dst, out).unwrap();
}
