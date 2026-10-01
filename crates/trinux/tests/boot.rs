//! Trinux on the emulator: K1 (boot, paging, timer) and K2 (user processes).

use setun_core::isa::{cr, mode};
use std::path::{Path, PathBuf};

fn boot_log(programs: &[PathBuf], steps: u64) -> (String, setun_core::Machine) {
    let disk = trinux::build_disk_with(&trinux::kernel_dir(), programs).unwrap_or_else(|e| panic!("{e}"));
    let mut m = trinux::boot(&disk, 729, steps);
    (m.dev.serial.take_output(), m)
}

/// Write a user program to a temp file next to a copy of user/lib.
fn temp_program(name: &str, src: &str) -> PathBuf {
    let stem = name.trim_end_matches(".tri");
    let dir = std::env::temp_dir().join(format!("trinux-user-{}-{stem}", std::process::id()));
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::copy(trinux::user_dir().join("lib/tlibc.tri"), dir.join("lib/tlibc.tri")).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, src).unwrap();
    p
}

#[test]
fn k1_kernel_boots_with_paging_and_timer() {
    let (out, m) = boot_log(&trinux::default_programs(), 60_000_000);
    for line in [
        "Trinux loader: kernel",
        "Trinux version 0.1 (trine) on Setun-386, balanced ternary",
        "Booted from disk 0, RAM 729 pages",
        "Traps: 243 vectors",
        "Memory: 689 pages free (frames 40..728), triddy orders 0..9",
        "mm: triddy self-test OK",
        "Paging: 9|9|9, kernel identity-mapped in L1 slots 0..+1, 759 pages",
        "Timer: 100 Hz",
    ] {
        assert!(out.contains(line), "missing {line:?} in:\n{out}");
    }
    assert!(!out.contains("panic"), "{out}");
    assert!(out.contains("[    0."), "printk uptime stamps: {out}");
    assert_eq!(m.cpu.mode(mode::PG), 1);
    assert!(m.cpu.cr(cr::PTB).value() % 59_049 == 0, "PTB is a multiple of 3^10");
}

#[test]
fn k2_two_processes_interleave_under_the_timer() {
    let (out, m) = boot_log(&trinux::default_programs(), 60_000_000);
    assert!(out.contains("init: started user program 0 as pid 1"), "{out}");
    assert!(out.contains("init: started user program 1 as pid 2"), "{out}");
    for pid in [1, 2] {
        for step in 0..5 {
            assert!(out.contains(&format!("pid {pid}: step {step}\n")), "pid {pid} step {step}:\n{out}");
        }
        assert!(out.contains(&format!("[pid {pid}] exited with code {}", 10 * pid)), "{out}");
    }
    // Preemption: pid 2 printed before pid 1 finished.
    let first_of_2 = out.find("pid 2: step 0").unwrap();
    let last_of_1 = out.find("pid 1: step 4").unwrap();
    assert!(first_of_2 < last_of_1, "no interleaving:\n{out}");
    assert!(out.contains("Idle: no user processes left"), "{out}");
    // Idle in ring +1 with interrupts on, the kernel's own page table active.
    assert_eq!(m.cpu.ring(), 1);
    assert_eq!(m.cpu.mode(mode::IE), 1);
}

#[test]
fn k2_user_faults_kill_the_process_not_the_kernel() {
    let bad = temp_program(
        "null.tri",
        "import \"lib/tlibc.tri\";\nfn main() -> int { puts(\"about to crash\\n\"); var p: *int = null; return *p; }\n",
    );
    let good = trinux::user_dir().join("k2_hello.tri");
    let (out, _) = boot_log(&[bad, good], 60_000_000);
    assert!(out.contains("about to crash"), "{out}");
    assert!(out.contains("[pid 1] protection fault at"), "{out}");
    assert!(out.contains("address 0 - killed"), "{out}");
    assert!(out.contains("[pid 2] exited with code 20"), "the other process carries on:\n{out}");
    assert!(!out.contains("Kernel panic"), "{out}");
}

#[test]
fn k2_syscall_errors_use_linux_errno() {
    let src = "import \"lib/tlibc.tri\";
    fn main() -> int {
        // A kernel address is not a valid user buffer: EFAULT (14).
        var r: int = write(1, 1000 as *char, 3);
        if (r != -1 || errno != 14) { return 1; }
        // A bad descriptor: EBADF (9).
        r = write(7, \"x\", 1);
        if (r != -1 || errno != 9) { return 2; }
        // An unknown system call: -ENOSYS (38).
        if (syscall(9000) != -38) { return 3; }
        return 77;
    }";
    let (out, _) = boot_log(&[temp_program("errno.tri", src)], 60_000_000);
    assert!(out.contains("[pid 1] exited with code 77"), "{out}");
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
