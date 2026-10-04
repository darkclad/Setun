//! Trinux on the emulator: K1 (boot, paging, timer), K2 (user processes),
//! K3 (fork, execve, wait4, copy on write, initramfs), K4 (VFS, tty, pipes,
//! the shell).

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

/// Boot the default disk and type each line at the shell prompt, waiting
/// for "$ " before the next one. An entry starting with `~` is typed as is
/// (no newline) 0.5 s of emulated time after the previous one, without
/// waiting for a prompt: that is how ^C and ^Z arrive. Returns the console
/// transcript.
/// The first login: answer "login: " and "Password: " as root (password
/// "root"). True if it typed something.
fn auto_login(m: &mut setun_core::Machine, out: &str, logged_in: &mut bool) -> bool {
    if out.ends_with("login: ") {
        m.dev.serial_input("root\n");
        return true;
    }
    if out.ends_with("Password: ") {
        m.dev.serial_input("root\n");
        *logged_in = true;
        return true;
    }
    false
}

fn shell(lines: &[&str], max_steps: u64) -> String {
    let disk = trinux::build_disk(&trinux::kernel_dir()).unwrap_or_else(|e| panic!("{e}"));
    let mut m = trinux::boot(&disk, 729, 0);
    let mut out = String::new();
    let mut next = 0;
    let mut steps = 0;
    let mut since_input = 0;
    let mut logged_in = false;
    while steps < max_steps {
        m.run(1_000_000);
        steps += 1_000_000;
        since_input += 1_000_000;
        out.push_str(&m.dev.serial.take_output());
        if out.contains("Kernel panic") {
            break;
        }
        if !logged_in && auto_login(&mut m, &out, &mut logged_in) {
            continue;
        }
        let Some(line) = lines.get(next) else {
            if out.contains("init: login session exited") && out.ends_with("login: ") {
                break; // the session ended and login asks again
            }
            continue;
        };
        if let Some(raw) = line.strip_prefix('~') {
            if since_input >= 5_000_000 {
                m.dev.serial_input(raw);
                next += 1;
                since_input = 0;
            }
        } else if out.ends_with("$ ") {
            m.dev.serial_input(&format!("{line}\n"));
            next += 1;
            since_input = 0;
        }
    }
    out
}

#[test]
fn k1_kernel_boots_with_paging_and_timer() {
    let (out, m) = boot_log(&trinux::default_files(), 30_000_000);
    for line in [
        "Trinux loader: kernel",
        "Trinux version 0.1 (trine) on Setun-11̅, balanced ternary",
        "Booted from disk 0, RAM 729 pages",
        "Traps: 243 vectors",
        "Memory: 671 pages free (frames 58..728), triddy orders 0..9",
        "mm: triddy self-test OK",
        "Paging: 9|9|9, kernel identity-mapped in L1 slots 0..+1, 759 pages",
        "VFS: Mounted root (tfs filesystem): 486 blocks",
        "VFS: devfs on /dev, proc on /proc, tmpfs on /tmp, console tty",
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
    // Each line is two writes, so a preemption can split "parent 3" itself.
    let last_parent = out.rfind("parent ").unwrap_or_else(|| panic!("{out}"));
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

#[test]
fn k3_k3test_forks_execs_and_waits() {
    let out = shell(&["k3test first", "exit"], 400_000_000);
    for line in [
        "hello: pid 4, parent 3, argc 3: [/bin/hello] [first] [ternary]",
        "k3test: /bin/hello exited with 3",
        "k3test: child saw 43, parent still has 100", // copy on write (555 % 256 = 43)
        "killed by signal 11",
        "k3test: child killed by signal 11",
        "k3test: ECHILD ok",
        "k3test: 5 fork/exec/wait cycles, no memory leaked",
        "k3test: heap ok, heap[999] = 42",
        "k3test: done",
        "init: login session exited with 0",
    ] {
        assert!(out.contains(line), "missing {line:?} in:\n{out}");
    }
    assert!(!out.contains("LEAK"), "{out}");
}

#[test]
fn k4_shell_pipes_echo_into_cat() {
    let out = shell(&["echo hi | cat", "exit 5"], 150_000_000);
    assert!(out.contains("init: pid 1, starting /bin/login\nlogin: root\nPassword: \n"), "{out}");
    assert!(out.contains("$ echo hi | cat\nhi\n$ exit 5\n"), "{out}");
    // the session's end is reported, and login asks again (init lives on)
    assert!(out.contains("init: login session exited with 5\nlogin: "), "{out}");
    assert!(!out.contains("Attempted to kill init"), "{out}");
}

#[test]
fn k4_shell_files_dirs_and_status() {
    let out = shell(
        &[
            "pwd; cd /tmp; pwd",
            "echo one > f; echo two >> f; cat f",
            "cat < f | cat | cat",
            "mkdir -p a/b && cd a/b && pwd && cd ../.. && ls",
            "ls /",
            "ls -l /bin/true /etc",
            "cat /etc/motd",
            "false; echo status $?",
            "false || echo or-ran && echo and-ran",
            "true && false && echo never; echo $?",
            r#"echo 'single  $?' "double $?" back\\slash"#,
            "nosuch; echo $?",
            "cat /nope",
            "cat /nope 2>&1 >/dev/null | cat",
            "rm f; cat f; rm -d a/b; ls a",
            "cd /dev; ls",
            "exit",
        ],
        600_000_000,
    );
    for want in [
        "$ pwd; cd /tmp; pwd\n/root\n/tmp\n",                     // a login starts at home
        "cat f\none\ntwo\n",
        "cat < f | cat | cat\none\ntwo\n",
        "&& ls\n/tmp/a/b\na\nf\n",
        "$ ls /\nbin\ndev\netc\nhome\nmnt\nproc\nroot\nsbin\ntmp\n",
        "$ ls -l /bin/true /etc\n-rwxr-xr-x 1 root     root    ",
        "/etc:\n-rw-r--r-- 1 root     root          44 group\n",
        "-rw-r--r-- 1 root     root          64 motd\n",
        "-rw------- 1 root     root         134 shadow\n",
        "Welcome to Trinux on Setun-11̅.\nEverything here is a power of 3.\n",
        "status 1\n",
        "or-ran\nand-ran\n",
        "echo never; echo $?\n1\n",
        "single  $? double 0 back\\slash\n",
        "sh: nosuch: not found\n127\n",
        "cat: /nope: No such file or directory\n",
        "/dev/null | cat\ncat: /nope: No such file or directory\n",
        "cat: f: No such file or directory\n",
        "$ cd /dev; ls\nconsole\nfb0\nkbd\nmouse\nnull\nptmx\npts\ntty\nttyS0\nzero\n",
        "init: login session exited with 0",
    ] {
        assert!(out.contains(want), "missing {want:?} in:\n{out}");
    }
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

#[test]
fn k5_ctrl_c_interrupts_sleep() {
    let out = shell(&["sleep 100", "~\u{3}", "echo $?", "exit"], 100_000_000);
    assert!(out.contains("$ sleep 100\n^C\n$ echo $?\n130\n"), "{out}");
    assert!(out.contains("init: login session exited with 0"), "{out}");
}

#[test]
fn k5_signal_semantics() {
    let out = shell(&["sigtest", "exit"], 300_000_000);
    for check in [
        "handler for SIGUSR1",
        "blocked signal is pending",
        "delivered on unblock",
        "nested handlers unwind",
        "SIG_IGN",
        "SIGKILL cannot be caught",
        "SIGPIPE kills the writer",
        "SIGINT ends a sleeping child",
        "SIGTERM ignored",
        "SIGKILL after an ignored SIGTERM",
        "nanosleep interrupted with EINTR",
        "SA_RESTART resumes read",
        "WUNTRACED sees SIGSTOP",
        "SIGCONT then SIGTERM",
        "getpgid of the new group",
        "kill(-pgrp)",
        "group leader terminated",
        "ESRCH for an empty group",
    ] {
        assert!(out.contains(&format!("sigtest: {check} ok\n")), "missing {check:?} in:\n{out}");
    }
    assert!(out.contains("sigtest: done"), "{out}");
    assert!(!out.contains("FAILED"), "{out}");
}

#[test]
fn k5_job_control() {
    let out = shell(
        &[
            "sleep 50 &",
            "jobs",
            "kill %1",
            "true",
            "sleep 100",
            "~\u{1a}",
            "jobs",
            "bg",
            "jobs",
            "fg",
            "~\u{3}",
            "echo $?",
            "cat | cat",
            "~\u{3}",
            "~abc",
            "~\u{3}",
            "echo after",
            "cat &",
            "~true\n",                                     // give cat 0.5 s to reach its read

            "kill -9 %1",
            "true",
            "exit 3",
        ],
        400_000_000,
    );
    for want in [
        "$ sleep 50 &\n[1] 3\n",
        "$ jobs\n[1]  Running                 sleep 50 &\n",
        "[1]  Terminated              sleep 50\n$ ",
        "$ sleep 100\n^Z\n[1]  Stopped                 sleep 100\n",
        "$ jobs\n[1]  Stopped                 sleep 100\n",
        "$ bg\n[1] sleep 100 &\n$ jobs\n[1]  Running                 sleep 100 &\n$ fg\nsleep 100\n^C\n",
        "$ echo $?\n130\n",
        "$ cat | cat\n^C\n$ ",
        "$ abc^C\n$ echo after\nafter\n",                       // ^C at the prompt drops the line
        "[1]  Killed                  cat\n",
        "init: login session exited with 3",
    ] {
        assert!(out.contains(want), "missing {want:?} in:\n{out}");
    }
    // SIGTTIN for a background read: reported once cat reaches its read,
    // before or after the next prompt (that depends on how fast cat gets
    // there), but before it is killed
    let (bg, stop, kill) = (out.find("$ cat &\n[1] 7\n"), out.find("[1]  Stopped                 cat\n"), out.find("$ kill -9 %1"));
    assert!(matches!((bg, stop, kill), (Some(b), Some(s), Some(k)) if b < s && s < k), "cat stopped for its read:\n{out}");
}

#[test]
fn fpu_lazy_switching() {
    let out = shell(&["fputest", "echo st=$?", "grep FPU /proc/1/status", "exit"], 400_000_000);
    for check in [
        "first use",
        "inexact sets IX",
        "registers survive switching (parent)",
        "registers survive switching (child)",
        "fork copies the FPU state",
        "execve resets the FPU",
        "handler starts with a clean FPU",
        "interrupted FPU state restored",
        "unmasked divide by zero is SIGFPE",
        "masked divide by zero is infinity",
        "unmasked invalid sqrt is SIGFPE",
        "parent unaffected",
    ] {
        assert!(out.contains(&format!("fputest: {check} ok\n")), "missing {check:?} in:\n{out}");
    }
    assert!(out.contains("fputest: pi = 3.14159265359\nfputest: done\n"), "{out}");
    assert!(out.contains("st=0\n"), "{out}");
    assert!(out.contains("FPU:\tunused\n"), "init never touched the FPU:\n{out}");
    assert!(!out.contains("FAILED") && !out.contains("Kernel panic"), "{out}");
}
