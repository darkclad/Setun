//! Trinux on the emulator: K1 (boot, paging, timer), K2 (user processes),
//! K3 (fork, execve, wait4, copy on write, initramfs).

use setun_core::isa::{cr, mode};
use std::path::{Path, PathBuf};

fn boot_log(files: &[(String, PathBuf)], steps: u64) -> (String, setun_core::Machine) {
    let disk = trinux::build_disk_with(&trinux::kernel_dir(), files).unwrap_or_else(|e| panic!("{e}"));
    let mut m = trinux::boot(&disk, 729, steps);
    (m.dev.serial.take_output(), m)
}

/// A disk whose /sbin/init is `src` (with user/lib next to it), plus /bin/hello.
fn with_init(test: &str, src: &str) -> Vec<(String, PathBuf)> {
    let dir = std::env::temp_dir().join(format!("trinux-user-{}-{test}", std::process::id()));
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::copy(trinux::user_dir().join("lib/tlibc.tri"), dir.join("lib/tlibc.tri")).unwrap();
    let p = dir.join("init.tri");
    std::fs::write(&p, src).unwrap();
    vec![("/sbin/init".into(), p), ("/bin/hello".into(), trinux::user_dir().join("hello.tri"))]
}

#[test]
fn k1_kernel_boots_with_paging_and_timer() {
    let (out, m) = boot_log(&trinux::default_files(), 30_000_000);
    for line in [
        "Trinux loader: kernel",
        "Trinux version 0.1 (trine) on Setun-386, balanced ternary",
        "Booted from disk 0, RAM 729 pages",
        "Traps: 243 vectors",
        "Memory: 689 pages free (frames 40..728), triddy orders 0..9",
        "mm: triddy self-test OK",
        "Paging: 9|9|9, kernel identity-mapped in L1 slots 0..+1, 759 pages",
        "initramfs: 3 files",
        "init: started /sbin/init as pid 1",
        "Timer: 100 Hz",
    ] {
        assert!(out.contains(line), "missing {line:?} in:\n{out}");
    }
    assert!(out.contains("[    0."), "printk uptime stamps: {out}");
    assert_eq!(m.cpu.mode(mode::PG), 1);
    assert!(m.cpu.cr(cr::PTB).value() % 59_049 == 0, "PTB is a multiple of 3^10");
}

#[test]
fn k3_init_forks_execs_and_waits() {
    let (out, _) = boot_log(&trinux::default_files(), 200_000_000);
    for line in [
        "init: pid 1, argv[0] = /sbin/init",
        "hello: pid 2, parent 1, argc 3: [/bin/hello] [first] [ternary]",
        "init: /bin/hello exited with 3",
        "init: child saw 43, parent still has 100",              // copy on write (555 % 256 = 43)
        "killed by signal 11",
        "init: child killed by signal 11",
        "init: ECHILD ok",
        "init: 5 fork/exec/wait cycles, no memory leaked",
        "init: heap ok, heap[999] = 42",
        "init: done",
        "Kernel panic - not syncing: Attempted to kill init! exit status 0",
    ] {
        assert!(out.contains(line), "missing {line:?} in:\n{out}");
    }
    assert!(!out.contains("LEAK"), "{out}");
}

#[test]
fn k2_processes_interleave_under_the_timer() {
    let src = "import \"lib/tlibc.tri\";
    fn work(tag: *char) {
        var i: int = 0;
        while (i < 4) {
            puts(tag); put_int(i); puts(\"\\n\");
            var spin: int = 0; while (spin < 15000) { spin += 1; }
            i += 1;
        }
    }
    fn main(argc: int, argv: **char) -> int {
        var pid: int = fork();
        if (pid == 0) { work(\"child \"); return 7; }
        work(\"parent \");
        var st: int;
        waitpid(pid, &st, 0);
        puts(\"child exit \"); put_int(WEXITSTATUS(st)); puts(\"\\n\");
        return 0;
    }";
    let (out, _) = boot_log(&with_init("interleave", src), 100_000_000);
    let first_child = out.find("child 0").unwrap_or_else(|| panic!("{out}"));
    let last_parent = out.find("parent 3").unwrap_or_else(|| panic!("{out}"));
    assert!(first_child < last_parent, "no preemption:\n{out}");
    assert!(out.contains("child exit 7"), "{out}");
}

#[test]
fn k2_syscall_errors_use_linux_errno() {
    let src = "import \"lib/tlibc.tri\";
    fn main(argc: int, argv: **char) -> int {
        if (write(1, 1000 as *char, 3) != -1 || errno != 14) { return 1; }   // EFAULT
        if (write(7, \"x\", 1) != -1 || errno != 9) { return 2; }           // EBADF
        if (syscall(9000) != -38) { return 3; }                             // ENOSYS
        if (execve(\"/no/such\", null, null) != -1 || errno != 2) { return 4; }   // ENOENT
        var st: int;
        if (waitpid(-1, &st, 0) != -1 || errno != 10) { return 5; }         // ECHILD
        return 77;
    }";
    let (out, _) = boot_log(&with_init("errno", src), 60_000_000);
    assert!(out.contains("Attempted to kill init! exit status 19712"), "77 * 256:\n{out}");
}

#[test]
fn kernel_fault_panics_with_details() {
    // A copy of the kernel whose main dereferences a misaligned pointer.
    let dir = std::env::temp_dir().join(format!("trinux-panic-{}", std::process::id()));
    copy_dir(&trinux::kernel_dir(), &dir);
    std::fs::write(
        dir.join("main.tri"),
        "import \"arch.tri\";\nfn main(d: int, f: int) -> int { traps_init(); var p: *int = 1 as *int; return *p; }\n",
    )
    .unwrap();
    let disk = trinux::build_disk_with(&dir, &[]).unwrap_or_else(|e| panic!("{e}"));
    let mut m = trinux::boot(&disk, 729, 20_000_000);
    let out = m.dev.serial.take_output();
    std::fs::remove_dir_all(&dir).ok();
    assert!(out.contains("trap: misaligned access (vector -5)"), "{out}");
    assert!(out.contains("address 1"), "{out}");
    assert!(out.contains("Kernel panic: unhandled trap in the kernel"), "{out}");
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let p = e.path();
        if p.is_dir() {
            copy_dir(&p, &to.join(e.file_name()));
        } else {
            std::fs::copy(&p, to.join(e.file_name())).unwrap();
        }
    }
}
