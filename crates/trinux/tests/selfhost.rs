//! Trinux building its own programs (docs/SELFHOST.md): the assembler `tas`
//! and the compiler `tric`, written in Trine. `tas` must turn the host
//! compiler's assembly into exactly the executables the host's own assembler
//! made; `tric` must write exactly the host compiler's unoptimized assembly,
//! and its error messages; and `tric` must build itself to a fixed point.

mod common;
use common::*;
use trinux::Root;

/// A disk with the default files and, under /usr/src, the host compiler's
/// assembly for these programs (a bigger volume, to hold it).
fn disk_with_sources(progs: &[&str], extra: &[(&str, &str)]) -> Vec<i16> {
    let dir = std::env::temp_dir().join(format!("setun-selfhost-{}-{}", std::process::id(), progs.join("-").len()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut files = trinux::default_files();
    let user = trinux::user_dir();
    for p in progs {
        let b = trine::build_file(&user.join(format!("bin/{p}.tri")), &trine::Options::user()).unwrap_or_else(|e| panic!("{p}: {e}"));
        let path = dir.join(format!("{p}.s"));
        std::fs::write(&path, &b.asm).unwrap();
        files.push((format!("/usr/src/{p}.s"), path));
    }
    for (name, text) in extra {
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        files.push((format!("/usr/src/{name}"), path));
    }
    trinux::build_disk_root(&trinux::kernel_dir(), &files, &Root::Tfs { blocks: 1944, inodes: 729 })
        .unwrap_or_else(|e| panic!("{e}"))
        .trytes
}

fn md5_of(o: &str, path: &str) -> String {
    o.lines().find(|l| l.ends_with(path)).unwrap_or_else(|| panic!("no md5 of {path} in:\n{o}")).split_whitespace().next().unwrap().to_string()
}

/// Assemble /usr/src/P.s in Trinux; it must equal /bin/P.
fn same_as_host(s: &mut Session, p: &str) {
    let o = s.cmd_max(&format!("tas -o /tmp/{p} /usr/src/{p}.s; echo status $?; md5sum /tmp/{p} /bin/{p}"), 60_000_000_000);
    assert!(o.contains("status 0"), "{p}: {o}");
    assert_eq!(md5_of(&o, &format!("/tmp/{p}")), md5_of(&o, &format!("/bin/{p}")), "{p}: tas and the host differ:\n{o}");
}

/// A disk with the Trine sources: the programs' and the library's under
/// /usr/src (as in the repository: bin/, lib/), the standard library in
/// /usr/lib/trine, and for each program the host compiler's unoptimized
/// assembly as /usr/src/P.s.
fn disk_with_trine(progs: &[&str], extra: &[(&str, &str)]) -> Vec<i16> {
    // (one directory per call: tests run in parallel)
    let first = extra.first().map_or("", |e| e.0);
    let dir = std::env::temp_dir().join(format!("setun-tric-{}-{}-{first}", std::process::id(), progs.join("-")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut files = trinux::default_files();
    let user = trinux::user_dir();
    // (a target named `.tri` goes onto the disk as a source, not compiled)
    let add = |files: &mut Vec<(String, std::path::PathBuf)>, target: String, src: &std::path::Path| {
        files.push((target, src.to_path_buf()));
    };
    let tree = |d: &std::path::Path| -> Vec<std::path::PathBuf> {
        let mut v: Vec<_> = std::fs::read_dir(d).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|x| x == "tri")).collect();
        v.sort();
        v
    };
    for p in tree(&user.join("lib")) {
        add(&mut files, format!("/usr/src/lib/{}", p.file_name().unwrap().to_str().unwrap()), &p);
    }
    for p in tree(&user.join("bin/tric")) {
        add(&mut files, format!("/usr/src/bin/tric/{}", p.file_name().unwrap().to_str().unwrap()), &p);
    }
    for p in tree(&user.join("../crates/trine/lib")) {
        add(&mut files, format!("/usr/lib/trine/{}", p.file_name().unwrap().to_str().unwrap()), &p);
    }
    let opts = trine::Options { optimize: false, ..trine::Options::user() };
    for p in progs {
        let src = user.join(format!("bin/{p}.tri"));
        add(&mut files, format!("/usr/src/bin/{p}.tri"), &src);
        let b = trine::build_file(&src, &opts).unwrap_or_else(|e| panic!("{p}: {e}"));
        let path = dir.join(format!("{p}.s"));
        std::fs::write(&path, &b.asm).unwrap();
        files.push((format!("/usr/src/{p}.s"), path));
    }
    for (name, text) in extra {
        let path = dir.join(format!("{name}.txt"));
        std::fs::write(&path, text).unwrap();
        files.push((format!("/usr/src/{name}"), path));
    }
    trinux::build_disk_root(&trinux::kernel_dir(), &files, &Root::Tfs { blocks: 1944, inodes: 729 })
        .unwrap_or_else(|e| panic!("{e}"))
        .trytes
}

/// Compile /usr/src/bin/P.tri in Trinux: the assembly must be the host's.
fn tric_same_as_host(s: &mut Session, p: &str) {
    tric_compare(s, p, &format!("/usr/src/bin/{p}.tri"));
}

/// Compile `src` in Trinux: the assembly must be /usr/src/P.s.
fn tric_compare(s: &mut Session, p: &str, src: &str) {
    // (on the disk: a big program's assembly does not fit in a RAM file in /tmp)
    let o = s.cmd_max(&format!("mkdir -p /usr/obj; tric -v -S -o /usr/obj/{p}.s {src}; echo status $?; md5sum /usr/obj/{p}.s /usr/src/{p}.s"), 200_000_000_000);
    eprintln!("{p}: {o}");
    assert!(o.contains("status 0"), "{p}: {o}");
    let (a, b) = (md5_of(&o, &format!("/usr/obj/{p}.s")), md5_of(&o, &format!("/usr/src/{p}.s")));
    if a != b {
        let d = s.cmd_max(&format!("diff /usr/src/{p}.s /usr/obj/{p}.s | head -20"), 20_000_000_000);
        panic!("{p}: tric and the host differ:\n{d}");
    }
}

/// tric's optimization levels (TERNARY_WORLD phase A): at -O1 (the
/// peephole pass, conditions jumping straight to their targets, rotated
/// loops) and -O2 (register locals, direct operands, the fast frame) tric
/// writes exactly the host's assembly at that level.
fn tric_at_level(level: u8, progs: &[&str]) -> Session {
    let opts = trine::Options { level: Some(level), ..trine::Options::user() };
    let user = trinux::user_dir();
    let want: Vec<(String, String)> = progs
        .iter()
        .map(|p| {
            let b = trine::build_file(&user.join(format!("bin/{p}.tri")), &opts).unwrap_or_else(|e| panic!("{p}: {e}"));
            (format!("{p}.O{level}.s"), b.asm)
        })
        .collect();
    let extra: Vec<(&str, &str)> = want.iter().map(|(n, a)| (n.as_str(), a.as_str())).collect();
    let disk = disk_with_trine(progs, &extra);
    let mut s = Session::boot_disk(&disk, false);
    for p in progs {
        let o = s.cmd_max(
            &format!("mkdir -p /usr/obj; tric -O{level} -S -o /usr/obj/{p}.s /usr/src/bin/{p}.tri; echo status $?; md5sum /usr/obj/{p}.s /usr/src/{p}.O{level}.s"),
            200_000_000_000,
        );
        assert!(o.contains("status 0"), "{p}: {o}");
        let (a, b) = (md5_of(&o, &format!("/usr/obj/{p}.s")), md5_of(&o, &format!("/usr/src/{p}.O{level}.s")));
        if a != b {
            let d = s.cmd_max(&format!("diff /usr/src/{p}.O{level}.s /usr/obj/{p}.s | head -n 30"), 20_000_000_000);
            panic!("{p}: tric -O{level} and the host differ:\n{d}");
        }
    }
    s
}

#[test]
fn tric_matches_the_host_at_level_1() {
    tric_at_level(1, &["echo", "cat", "edit", "calc", "dstest", "vectest", "chartest", "mtxtest", "o1test", "sh", "tric"]);
}

#[test]
fn tric_matches_the_host_at_level_2() {
    tric_at_level(2, &["echo", "cat", "edit", "calc", "dstest", "vectest", "chartest", "mtxtest", "o1test", "o2test", "sh", "tric"]);
}

#[test]
fn tric_matches_the_host_at_level_3() {
    tric_at_level(3, &["echo", "cat", "edit", "calc", "dstest", "vectest", "chartest", "mtxtest", "o1test", "o2test", "o3test", "sh", "tric"]);
}

/// Level 4 is everything (the host's default build): inlining, and the
/// functions nothing reaches left out.
#[test]
fn tric_matches_the_host_at_level_4() {
    let mut s = tric_at_level(4, &["echo", "cat", "edit", "calc", "dstest", "vectest", "chartest", "mtxtest", "o1test", "o2test", "o3test", "sh", "tric"]);
    has(&s.cmd("tric -O5 -S -o /tmp/x.s /usr/src/bin/echo.tri; echo status $?"), &["usage: tric", "status 2"]);
}

/// Real literals: the decimal is converted exactly and rounded once, ties to
/// even, as the host does; subnormals, overflow to ∞, the far cut-offs,
/// negated constants and integers as reals.
const REALS: &str = "\
const A: real = 1.5;
const B: real = -A;
const C: real = 7;
const D: real = -C;
const N: int = -12;
const E: real = N;
const BIG: real = 1e300;
const TINY: real = 1e-300;
const SUB: real = 2.5e-4700;
const HUGE: real = 1e5000;
const OVER: real = 1e20001;
const UNDER: real = 1e-20001;
const ZERO: real = 0.0;
fn main() -> int {
    var x: real = 0.1 + 0.2 + A + B + C + D + E + BIG + TINY + SUB + HUGE + OVER + UNDER + ZERO;
    x = x * 3.14159265358979323846264338327950288 + 2.718281828459045e0 - 1e-5 / 6.02214076e23;
    x = x + 0.5 + 1.0 + 1_000.000_1 + 9.99999999999999999999e-1 + 123456789012345678901234567890.123;
    x = x + 0.3333333333333333333333333333333333333 + 1e-4695 + 5e-4696 + 3e+20 + 0e5 + 4.5e-1;
    return x as int;
}
";

#[test]
fn tric_converts_reals_as_the_host_does() {
    let opts = trine::Options { optimize: false, ..trine::Options::user() };
    let want = trine::build_source("/usr/src/reals.tri", REALS, None, &opts).unwrap_or_else(|e| panic!("{e}")).asm;
    let disk = disk_with_trine(&["calc", "fputest"], &[("reals.tri", REALS), ("reals.s", &want)]);
    let mut s = Session::boot_disk(&disk, false);
    tric_compare(&mut s, "reals", "/usr/src/reals.tri");
    for p in ["calc", "fputest"] {
        tric_same_as_host(&mut s, p);
    }
}

#[test]
fn tric_matches_the_host_compiler() {
    let progs = ["echo", "cat", "sh", "edit", "tas", "tric", "libctest", "wget"];
    let disk = disk_with_trine(&progs, &[]);
    let mut s = Session::boot_disk(&disk, false);
    for p in progs {
        tric_same_as_host(&mut s, p);
    }
}

/// The fixed point: tric (built by the host) builds tric; that one builds
/// tric again, and the two are the same program, the one the host's own
/// unoptimized assembly makes. What the second builds runs.
#[test]
fn tric_builds_itself() {
    let disk = disk_with_trine(&["tric", "echo"], &[]);
    let mut s = Session::boot_disk(&disk, false);
    let o = s.cmd_max(
        "mkdir -p /usr/obj; cd /usr/obj; tric -o tric2 /usr/src/bin/tric.tri; echo status $?; \
         ./tric2 -o tric3 /usr/src/bin/tric.tri; echo status $?; tas -o tric.host /usr/src/tric.s; md5sum tric2 tric3 tric.host; ls",
        400_000_000_000,
    );
    assert_eq!(o.matches("status 0").count(), 2, "{o}");
    let (a, b, c) = (md5_of(&o, " tric2"), md5_of(&o, " tric3"), md5_of(&o, " tric.host"));
    assert_eq!(a, b, "not a fixed point:\n{o}");
    assert_eq!(a, c, "not the host's:\n{o}");
    assert!(!o.contains(".tmp.s"), "the assembly was left behind:\n{o}");
    has(&s.cmd_max("./tric3 -o echo3 /usr/src/bin/echo.tri && ./echo3 built by tric3", 100_000_000_000), &["built by tric3\n"]);
}

/// The stock disk has what a user needs to build a program: tric, tas and
/// the sources, with imports found next to the file.
#[test]
fn tric_on_the_default_disk() {
    let mut s = Session::boot_with(false);
    let o = s.cmd_max("cd /usr/src; tric -o /root/hello hello.tri; echo status $?; /root/hello one two; ls /root; df", 60_000_000_000);
    eprintln!("{o}");
    has(&o, &["status 0", "argc 3: [/root/hello] [one] [two]"]);
    assert!(!o.contains("hello.tmp.s"), "the assembly was left behind:\n{o}");
}

/// Errors: for each bad program, tric says what the host compiler says,
/// at the same place, exits 1 and leaves no output behind.
#[test]
fn tric_reports_errors_as_the_host_does() {
    let bad: &[(&str, &str)] = &[
        ("unknown.tri", "fn main() -> int { return x; }\n"),
        ("twice.tri", "fn main() -> int {\n    var a: int = 1;\n    var a: int = 2;\n    return a;\n}\n"),
        ("types.tri", "fn main() -> int { return \"s\"; }\n"),
        ("syntax.tri", "fn main() -> int { return 1 +; }\n"),
        ("charlit.tri", "fn main() -> int { var c: char = 'ab'; return 0; }\n"),
        ("args.tri", "fn f(a: int) -> int { return a; }\nfn main() -> int { return f(1, 2); }\n"),
        ("field.tri", "struct s { a: int; a: int; }\nfn main() -> int { return 0; }\n"),
        ("break.tri", "fn main() -> int { break; }\n"),
        ("hex.tri", "fn main() -> int { return 0x; }\n"),
        ("import.tri", "import \"nosuch.tri\";\nfn main() -> int { return 0; }\n"),
        ("nomain.tri", "fn helper() {}\n"),
        ("init.tri", "fn main() -> int { var x: [3]int = {1, 2, 3, 4}; return x[0]; }\n"),
        ("case.tri", "fn main() -> int { switch (1) { case 1: return 1; case 2, 1: return 2; } return 0; }\n"),
        ("fits.tri", "const c: tryte = 20000;\nfn main() -> int { return 0; }\n"),
        ("ptr.tri", "fn main() -> int { var p: *int; var q: *char; return p - q; }\n"),
        ("sizeof.tri", "fn main() -> int { return sizeof(nosuch); }\n"),
        ("comment.tri", "fn main() -> int { /* open\n"),
        ("ternary.tri", "fn main() -> int { return 0t1+1; }\n"),
        ("mainargs.tri", "fn main(a: int, b: int, c: int) -> int { return 0; }\n"),
        ("reserved.tri", "var r1: int;\nfn main() -> int { return 0; }\n"),
        ("creg.tri", "fn main() -> int { return mfc(foo); }\n"),
        ("cond.tri", "struct s { a: int; }\nvar v: s;\nfn main() -> int { if (v) { return 1; } return 0; }\n"),
        ("deref.tri", "fn main() -> int { var p: *void; return *p; }\n"),
        ("noaddr.tri", "fn main() -> int { var p: *int = &5; return 0; }\n"),
        ("block.tri", "fn main() -> int {\n"),
        ("escape.tri", "fn main() -> int { return '\\q'; }\n"),
        // Kleene truth values (TERNARY_WORLD A1)
        ("tritcond.tri", "fn main() -> int { var t: trit = unknown; if (t) { return 1; } return 0; }\n"),
        ("truecond.tri", "fn main() -> int { while (true) { } return 0; }\n"),
        ("cmp3cond.tri", "fn main() -> int { var a: int = 1; if (a <=> 2) { return 1; } return 0; }\n"),
        ("truthint.tri", "fn main() -> int { var done: int = false; return done; }\n"),
        ("truthret.tri", "fn f() -> int { return ~true | unknown; }\nfn main() -> int { return f(); }\n"),
        ("unknownid.tri", "fn main() -> int { var unknown: int = 1; return unknown; }\n"),
        // ternary idiom builtins (A2, A3)
        ("stwmptr.tri", "fn main() -> int { var x: int = 1; stwm(x, 1, 1); return 0; }\n"),
        ("mrgargs.tri", "fn main() -> int { return mrg(1, 2); }\n"),
        ("cas3ptr.tri", "fn main() -> int { var x: int = 1; return cas3(x, 0, 1); }\n"),
        ("swapptr.tri", "fn main() -> int { var x: tryte = 1; return swap(&x, 0); }\n"),
        // vector builtins (A5)
        ("vecptr.tri", "fn main() -> int { var x: int; vadd(x, x, x); return 0; }\n"),
        ("vectrit.tri", "fn main() -> int { var x: [3]trit; vabsw(x, x); return 0; }\n"),
        ("vecargs.tri", "fn main() -> int { var x: [3]int; return vsum(x, x); }\n"),
        ("vecfill.tri", "fn main() -> int { var x: [3]int; vbcst(x, x); return 0; }\n"),
        // text beyond U+2671 (A6)
        ("bigchar.tri", "fn main() -> int { return '日'; }\n"),
        ("bigchar2.tri", "fn main() -> int { return '日本'; }\n"),
    ];
    let disk = disk_with_trine(&[], bad);
    let mut s = Session::boot_disk(&disk, false);
    s.cmd("mkdir /usr/obj");
    let opts = trine::Options { optimize: false, ..trine::Options::user() };
    for (name, text) in bad {
        let want = match trine::build_source(&format!("/usr/src/{name}"), text, None, &opts) {
            Err(e) => e,
            Ok(_) => panic!("{name}: the host compiles it"),
        };
        let o = s.cmd_max(&format!("tric -S -o /usr/obj/bad.s /usr/src/{name}; echo status $?; ls /usr/obj"), 20_000_000_000);
        has(&o, &[want.as_str(), "status 1"]);
        assert!(!o.contains("bad.s"), "{name}: output left behind:\n{o}");
    }
    has(&s.cmd("tric; echo status $?; tric /nosuch.tri; echo status $?"), &["usage: tric [-S] [-O level] [-o out] file.tri\nstatus 2", "tric: /nosuch.tri: No such file or directory\nstatus 1"]);
}

#[test]
fn tas_matches_the_host_assembler() {
    let progs = ["echo", "cat", "edit", "calc", "tas", "mtxtest", "vectest", "chartest", "dstest"];
    let disk = disk_with_sources(&progs, &[]);
    let mut s = Session::boot_disk(&disk, false);
    for p in progs {
        same_as_host(&mut s, p);
    }
    // and what it made runs
    has(&s.cmd("/tmp/echo Привет, троичный мир; echo 'x y' | /tmp/cat"), &["Привет, троичный мир\nx y\n"]);
}

#[test]
fn tas_assembles_the_largest_programs() {
    let progs = ["sh", "wget"];
    let disk = disk_with_sources(&progs, &[]);
    let mut s = Session::boot_disk(&disk, false);
    for p in progs {
        same_as_host(&mut s, p);
    }
}

#[test]
fn tas_reports_errors_with_their_lines() {
    let bad = [
        ("unknown.s", "        .org -847288609443\n        frob r1, r2\n", "tas: line 2: unknown instruction `frob`"),
        ("undef.s", "        .org -847288609443\n        call nowhere\n", "tas: line 2: undefined symbol `nowhere`"),
        ("twice.s", "        .org -847288609443\na:      nop\na:      nop\n", "tas: line 3: defined twice:"),
        ("operands.s", "        .org -847288609443\n        add r1, r2\n", "tas: line 2: bad operands"),
        ("register.s", "        .org -847288609443\n        add r1, 5, r2\n", "tas: line 2: register expected"),
        ("range.s", "        .org -847288609443\n        ldw r1, [r2+5]\n        .tryte 20000\n", "tas: line 3: tryte out of range: `20000`"),
        ("align.s", "        .org -847288609444\n", "tas: line 1: .org is not word-aligned: `-847288609444`"),
        ("field.s", "        .org -847288609443\n        ext r1, r2, 20, 9\n", "tas: line 2: trit field out of 1..27"),
        ("string.s", "        .org -847288609443\n        .string \"open\n", "tas: line 2: unterminated string"),
        ("real.s", "        .org -847288609443\n        .real81 1.5\n", "tas: line 2: real literals are not supported by tas"),
        ("nodata.s", "        .org -847288609443\n        nop\n", "tas: line 0: no _trine_data_start/_trine_data_end"),
        ("origin.s", "        .org 0\n        nop\n", "tas: line 0: the program must start at the user origin, not `0`"),
        ("escape.s", "        .org -847288609443\n        .tryte '\\q'\n", "tas: line 2: unknown escape"),
        // quoted operands alone on a line (once dropped): they assemble, so the
        // error is the missing data symbols at the end
        ("tryte.s", "        .org -847288609443\n        .tryte 'A'\n        .tryte '\\''\n        .word '\\n', -'1'\n", "tas: line 0: no _trine_data_start/_trine_data_end"),
        ("mfc.s", "        .org -847288609443\n        mfc r1, nosuch\n", "tas: line 2: unknown control register `nosuch`"),
        ("vlane.s", "        .org -847288609443\n        vget r1, v2, 9\n", "tas: line 2: lane out of range: `9`"),
        ("vwlane.s", "        .org -847288609443\n        vputw v1, r2, 3\n", "tas: line 2: lane out of range: `3`"),
        ("valign.s", "        .org -847288609443\n        vld v1, [r2+3]\n", "tas: line 2: offset is not a multiple of 9: `3`"),
        ("voff.s", "        .org -847288609443\n        vst v1, [r2+126]\n", "tas: line 2: offset/9 out of range: `14`"),
        ("vreg.s", "        .org -847288609443\n        vadd v1, v2, r3\n", "tas: line 2: vector register expected"),
        ("vsum.s", "        .org -847288609443\n        vsum v1, v2\n", "tas: line 2: register expected"),
        ("vmul.s", "        .org -847288609443\n        vmul v1, v2, v3\n", "tas: line 2: unknown instruction `vmul`"),
        ("vneg.s", "        .org -847288609443\n        vnegw v1, v2, v3\n", "tas: line 2: bad operands"),
    ];
    let extra: Vec<(&str, &str)> = bad.iter().map(|(n, t, _)| (*n, *t)).collect();
    let disk = disk_with_sources(&[], &extra);
    let mut s = Session::boot_disk(&disk, false);
    for (name, _, want) in bad {
        let o = s.cmd_max(&format!("tas -o /tmp/x /usr/src/{name}; echo status $?"), 2_000_000_000);
        has(&o, &[want, "status 1"]);
    }
    has(&s.cmd("tas; echo status $?; tas /nosuch.s; echo status $?"), &["usage: tas [-r] [-o out] file.s\nstatus 2", "tas: /nosuch.s: No such file or directory\nstatus 1"]);
    // -r: the bare image, no header (19 words of nops = 57 trytes)
    let o = s.cmd("echo '        .org 0' > /tmp/r.s; echo '        .zero 19' >> /tmp/r.s; tas -r -o /tmp/r /tmp/r.s; echo status $?; wc -c /tmp/r");
    has(&o, &["status 0", "57 /tmp/r"]);
}


/// Kleene truth values (TERNARY_WORLD A1): true, unknown, false as trits,
/// b_true and b_false, & | ~ on trits, a trit tested on purpose.
const KLEENE: &str = "fn and3(a: trit, b: trit) -> trit { return a & b; }
fn or3(a: trit, b: trit) -> trit { return a | b; }
fn not3(a: trit) -> trit { return ~a; }
fn main() -> int {
    var v: [3]trit = { false, unknown, true };
    var r: int = 0;
    var i: int = 0;
    while (i < 3) {
        if (and3(v[i], true) != v[i] || or3(v[i], false) != v[i]) { r += 1; }
        if (not3(v[i]) != v[2 - i]) { r += 10; }
        i += 1;
    }
    var t: trit = ~true & unknown;
    var n: int = 0;
    while (b_true) { n += 1; if (n == 3 || b_false) { break; } }
    sign (t) { neg: { r += 0; } zero: { r += 100; } pos: { r += 100; } }
    if ((t as int) == -1 && t == false) { return r; }
    return r + 1000;
}
";

/// The ternary idiom builtins (TERNARY_WORLD A2, A3).
const IDIOMS: &str = "var page: [4]int;
fn main() -> int {
    var old: int = 2222333444555;
    var m: int = mrg(old, -1000000007, 3141592653589);
    var c: int = tcp(m, 0) + tcn(m, 9) + trev(old, 13);
    stwm(&page[1], m, -9);
    c += cas3(&page[0], 0, 1) + swap(&page[0], -1);
    return c;
}
";

#[test]
fn tric_compiles_truth_values_as_the_host_does() {
    let opts = trine::Options { optimize: false, ..trine::Options::user() };
    let want = trine::build_source("/usr/src/kleene.tri", KLEENE, None, &opts).unwrap_or_else(|e| panic!("{e}")).asm;
    let want2 = trine::build_source("/usr/src/idioms.tri", IDIOMS, None, &opts).unwrap_or_else(|e| panic!("{e}")).asm;
    let disk = disk_with_trine(&["vectest", "chartest", "dstest"], &[("kleene.tri", KLEENE), ("kleene.s", &want), ("idioms.tri", IDIOMS), ("idioms.s", &want2)]);
    let mut s = Session::boot_disk(&disk, false);
    tric_compare(&mut s, "kleene", "/usr/src/kleene.tri");
    tric_compare(&mut s, "idioms", "/usr/src/idioms.tri");
    tric_same_as_host(&mut s, "vectest"); // the vector builtins (A5)
    tric_same_as_host(&mut s, "chartest"); // 3-tryte characters in literals (A6)
    tric_same_as_host(&mut s, "dstest"); // flags, 3-ary heap, B-tree (phase A)
}

/// (A debugging aid, not run by default.) `TRIC_DBG=file.tri
/// [TRIC_LEVEL=n] cargo test -p trinux --test selfhost tric_debug_level --
/// --ignored`: tric -On and the host's assembly of the file (which may
/// import ../lib/...), written to dbg_tric.s and dbg_host.s in the temp
/// directory, for diffing.
#[test]
#[ignore]
fn tric_debug_level() {
    let Ok(path) = std::env::var("TRIC_DBG") else { return };
    let level: u8 = std::env::var("TRIC_LEVEL").ok().and_then(|l| l.parse().ok()).unwrap_or(4);
    let src = std::fs::read_to_string(&path).unwrap();
    let opts = trine::Options { level: Some(level), ..trine::Options::user() };
    let want = trine::build_source("/usr/src/bin/dbg.tri", &src, Some(&trinux::user_dir().join("bin")), &opts).unwrap_or_else(|e| panic!("{e}")).asm;
    let disk = disk_with_trine(&[], &[("dbgsrc", &src), ("dbg.s", &want)]);
    let mut s = Session::boot_disk(&disk, false);
    let o = s.cmd_max(&format!("cp /usr/src/dbgsrc /usr/src/bin/dbg.tri; tric -O{level} -S -o /tmp/dbg.s /usr/src/bin/dbg.tri; echo status $?; echo ====; cat /tmp/dbg.s"), 20_000_000_000);
    let (head, tric_out) = o.split_once("====").unwrap_or((&o, ""));
    std::fs::write(std::env::temp_dir().join("dbg_tric.s"), tric_out.trim_start()).unwrap();
    std::fs::write(std::env::temp_dir().join("dbg_host.s"), &want).unwrap();
    println!("{head}");
}
