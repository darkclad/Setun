//! Users and groups: login, su, passwd, useradd/userdel, id, chown, and the
//! kernel's permission checks, each with the ways it must refuse.

mod common;
#[allow(unused_imports)]
use common::{has as assert_has, lacks as assert_lacks};

/// Boot the default disk, log in as root at the first prompt, and type each
/// line at a "$ " prompt. A line starting with `~` is sent as it is at the
/// next prompt ending in ": " (login, password), once the output has been quiet
/// for 3 M steps. Stops when the lines are used up and login asks again.
fn session(lines: &[&str], max_steps: u64) -> String {
    let disk = trinux::build_disk(&trinux::kernel_dir()).unwrap_or_else(|e| panic!("{e}"));
    let mut m = trinux::boot_trytes(&disk.trytes, 729, 0, "");
    let mut out = String::new();
    let (mut next, mut steps, mut idle) = (0, 0u64, 0u64);
    let mut logged_in = false;
    while steps < max_steps && !m.powered_off {
        m.run(1_000_000);
        steps += 1_000_000;
        idle += 1_000_000;
        let fresh = m.dev.serial.take_output();
        if !fresh.is_empty() {
            idle = 0;
        }
        out.push_str(&fresh);
        if out.contains("Kernel panic") {
            break;
        }
        if !logged_in && out.ends_with("login: ") {
            m.dev.serial_input("root\n");
            continue;
        }
        if !logged_in && out.ends_with("Password: ") {
            m.dev.serial_input("root\n");
            logged_in = true;
            continue;
        }
        let Some(line) = lines.get(next) else {
            if out.contains("init: login session exited") && out.ends_with("login: ") {
                break;
            }
            continue;
        };
        if let Some(raw) = line.strip_prefix('~') {
            if idle >= 3_000_000 && out.ends_with(": ") {
                m.dev.serial_input(raw);
                next += 1;
                idle = 0;
            }
        } else if out.ends_with("$ ") {
            m.dev.serial_input(&format!("{line}\n"));
            next += 1;
            idle = 0;
        }
    }
    out
}



#[test]
fn accounts_and_identities() {
    let out = session(
        &[
            "id; whoami; groups",
            "id user; groups user",
            "ls -l /bin/su /bin/passwd /etc/shadow /etc/passwd",
            "grep Uid /proc/$$/status",
            "useradd -G wheel alice; id alice; ls -l /home",
            "passwd alice",
            "~secret\n",
            "~secret\n",
            "grep -c '^alice:\\$md5\\$' /etc/shadow",
            "su alice",
            "id; whoami; pwd; echo $HOME $USER",
            "grep Uid /proc/$$/status",
            "echo mine > /tmp/a.txt; ls -l /tmp/a.txt",
            "passwd",                                     // alice changes her own (passwd is set-user-ID)
            "~secret\n",
            "~newer\n",
            "~newer\n",
            "exit",
            "su - alice -c pwd",
            "chown user:wheel /tmp/a.txt; ls -l /tmp/a.txt; chgrp root /tmp/a.txt; ls -l /tmp/a.txt",
            "userdel -r alice; id alice; ls /home",
            "exit",
        ],
        900_000_000,
    );
    assert_has(
        &out,
        &[
            "uid=0(root) gid=0(root) groups=0(root),10(wheel)\nroot\nroot wheel\n",
            "uid=1000(user) gid=1000(user) groups=1000(user),10(wheel)\nuser wheel\n",
            "-rwsr-xr-x 1 root     root    ",
            "-rw------- 1 root     root         134 Jan  1  1970 /etc/shadow\n",
            "Uid:\t0\t0\t0\n",
            "uid=1001(alice) gid=1001(alice) groups=1001(alice),10(wheel)\n",
            "drwxr-xr-x 2 alice    alice   ",
            "New password: \nRetype new password: \npasswd: password updated successfully\n",
            "$ grep -c '^alice:\\$md5\\$' /etc/shadow\n1\n",
            "uid=1001(alice) gid=1001(alice) groups=1001(alice),10(wheel)\nalice\n/root\n/home/alice alice\n",
            "Uid:\t1001\t1001\t1001\n",
            "-rw-r--r-- 1 alice    alice          5 Sep 21 14:13 /tmp/a.txt\n",
            "Changing password for alice.\nCurrent password: \nNew password: \nRetype new password: \npasswd: password updated successfully\n",
            "$ su - alice -c pwd\n/home/alice\n",
            "-rw-r--r-- 1 user     wheel          5 Sep 21 14:13 /tmp/a.txt\n-rw-r--r-- 1 user     root           5 Sep 21 14:13 /tmp/a.txt\n",
            "id: 'alice': no such user\nuser\n",
        ],
    );
}

#[test]
fn permissions_are_enforced() {
    let out = session(
        &[
            "echo root-only > /tmp/r; useradd alice",
            "su alice",
            "cat /etc/shadow; echo st=$?",
            "echo x > /etc/motd; echo st=$?",
            "ls /root; echo st=$?",
            "touch /etc/new; echo st=$?",
            "rm /tmp/r; echo st=$?",                     // /tmp is sticky: not hers
            "chmod 600 /etc/motd; echo st=$?",
            "chown alice /etc/motd; echo st=$?",
            "kill -9 1; echo st=$?",
            "mount -t tmpfs none /mnt; echo st=$?",
            "/sbin/reboot; echo st=$?",
            "useradd bob; echo st=$?",
            "passwd user; echo st=$?",
            "login -f root; echo st=$?",
            "cat /etc/motd > /dev/null; echo can-read=$?",
            "exit",
            "cat /tmp/r",
            "exit",
        ],
        700_000_000,
    );
    assert_has(
        &out,
        &[
            "cat: /etc/shadow: Permission denied\nst=1\n",
            "sh: /etc/motd: Permission denied\nst=1\n",
            "ls: /root: Permission denied\nst=1\n",
            "touch: /etc/new: Permission denied\nst=1\n",
            "rm: /tmp/r: Operation not permitted\nst=1\n",
            "chmod: /etc/motd: Operation not permitted\nst=1\n",
            "chown: /etc/motd: Operation not permitted\nst=1\n",
            "sh: kill: 1: Operation not permitted\nst=1\n",
            "mount: /mnt: Operation not permitted\nst=1\n",
            "reboot: Operation not permitted\nst=1\n",
            "useradd: only root may add or remove users\nst=1\n",
            "passwd: You may not change the password for user.\nst=1\n",
            "login: -f: Permission denied\nst=1\n",
            "can-read=0\n",
            "$ cat /tmp/r\nroot-only\n",
        ],
    );
}

#[test]
fn logging_in_and_failing_to() {
    let out = session(
        &[
            "su user -c id",                              // root needs no password
            "su - user -c 'echo $HOME'",
            "useradd carol; echo locked",
            "su alice; echo st=$?",                       // no such user
            "exit",                                       // ends root's session: login asks again
            "~nosuch\n",
            "~whatever\n",
            "~user\n",
            "~wrong\n",
            "~user\n",
            "~user\n",
            "id; pwd; ls -l /dev/fb0",
            "su -c id",                                   // root's password, wrongly
            "~nope\n",
            "su carol -c id",                             // a locked account
            "~anything\n",
            "su -c id",
            "~root\n",
            "exit",
            "~a\n",                                       // three failures end the login
            "~b\n",
            "~a\n",
            "~b\n",
            "~a\n",
            "~b\n",
            "~root\n",
            "~root\n",
            "echo back-as-root",
            "exit",
        ],
        900_000_000,
    );
    assert_has(
        &out,
        &[
            "$ su user -c id\nuid=1000(user) gid=1000(user) groups=1000(user),10(wheel)\n",
            "/home/user\n",
            "su: user alice does not exist\nst=1\n",
            "login: nosuch\nPassword: \nLogin incorrect\nlogin: user\nPassword: \nLogin incorrect\nlogin: user\nPassword: \n",
            "uid=1000(user) gid=1000(user) groups=1000(user),10(wheel)\n/home/user\ncrw-rw---- 1 user     user    ",
            "Password: \nsu: Authentication failure\n",
            "$ su -c id\nPassword: \nuid=0(root) gid=0(root) groups=0(root),10(wheel)\n",
            "Password: \nLogin incorrect\ninit: login session exited with 1\nlogin: root\n",
            "back-as-root\n",
        ],
    );
    assert_lacks(&out, &["uid=1003(carol)"]);
}
