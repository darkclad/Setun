//! Trinux building its own programs (docs/SELFHOST.md): the assembler `tas`,
//! written in Trine, must turn the host compiler's assembly into exactly the
//! executables the host's own assembler made.

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

#[test]
fn tas_matches_the_host_assembler() {
    let progs = ["echo", "cat", "edit", "calc", "tas"];
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

