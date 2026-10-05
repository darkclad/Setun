//! scp and sftp in Trinux (docs/NETWORK.md §12), against Trinux's own
//! sshd on loopback, logging in with a key made by Trinux's ssh-keygen.

mod common;
use common::*;

const LONG: u64 = 30_000_000_000;

fn with_key(s: &mut Session) {
    let o = s.cmd_max("ssh-keygen -C copier > /dev/null; cat /root/.ssh/id_ed25519.pub >> /root/.ssh/authorized_keys; chmod 600 /root/.ssh/authorized_keys; echo ready", LONG);
    has(&o, &["ready\n"]);
}

/// A shell command that writes these lines to a file (Trinux has no printf).
fn write_lines(path: &str, lines: &[&str]) -> String {
    let mut c = format!("echo '{}' > {path}", lines[0]);
    for l in &lines[1..] {
        c.push_str(&format!("; echo '{l}' >> {path}"));
    }
    c
}

fn md5s(o: &str) -> Vec<String> {
    o.lines().filter(|l| l.contains(" /")).map(|l| l.split(' ').next().unwrap().to_string()).collect()
}

#[test]
fn scp_copies_both_ways() {
    let mut s = Session::boot();
    with_key(&mut s);
    s.cmd("echo 'Сетунь: троичная машина' > /tmp/ru.txt; seq 1 2000 > /tmp/nums; mkdir /tmp/in");
    let o = s.cmd_max("scp /tmp/ru.txt /tmp/nums localhost:/tmp/in; echo status $?", LONG);
    has(&o, &["Warning: Permanently added 'localhost' (ED25519)", "/tmp/ru.txt -> /tmp/in/ru.txt\n", "/tmp/nums -> /tmp/in/nums\n", "status 0"]);
    let o = s.cmd_max("scp -q localhost:/tmp/in/ru.txt /tmp/ru.back; scp -q root@localhost:/tmp/in/nums /tmp/nums.back; echo status $?; md5sum /tmp/ru.txt /tmp/in/ru.txt /tmp/ru.back /tmp/nums /tmp/nums.back", LONG);
    has(&o, &["status 0"]);
    let sums = md5s(&o);
    assert!(sums.len() == 5 && sums[0] == sums[1] && sums[1] == sums[2] && sums[3] == sums[4], "the same characters arrive both ways:\n{o}");
    let o = s.cmd("cat /tmp/ru.back; wc -c /tmp/in/ru.txt");
    has(&o, &["Сетунь: троичная машина\n", "24 /tmp/in/ru.txt"]);
    // the refusals
    let o = s.cmd_max("scp localhost:/nope /tmp/x; echo status $?; scp /nope localhost:/tmp/x; echo status $?", LONG);
    has(&o, &["scp: /nope: No such file or directory\nstatus 1", "scp: /nope: No such file or directory\nstatus 1"]);
    let o = s.cmd_max("scp /tmp/ru.txt /tmp/nums localhost:/tmp/in/nums; echo status $?; scp /tmp/nums /tmp/other; echo status $?", LONG);
    has(&o, &["scp: /tmp/in/nums: Not a directory\nstatus 1", "scp: copies must go between this machine and one other\nstatus 1"]);
}

#[test]
fn sftp_in_batch() {
    let mut s = Session::boot();
    with_key(&mut s);
    s.cmd("seq 1 3000 > /tmp/nums; cd /tmp");
    let batch = ["pwd", "mkdir work", "cd work", "put /tmp/nums", "ls", "get nums /tmp/nums.back", "rename nums n2", "-rm nosuch", "rm n2", "cd ..", "rmdir work", "bye"];
    s.cmd(&write_lines("/tmp/b", &batch));
    let o = s.cmd_max("sftp -b /tmp/b localhost; echo status $?", LONG);
    has(
        &o,
        &[
            "sftp> pwd\nRemote working directory: /root\n",
            "sftp> put /tmp/nums\nUploading /tmp/nums to /root/work/nums\n",
            " nums\n",
            "sftp> get nums /tmp/nums.back\nFetching /root/work/nums to /tmp/nums.back\n",
            "sftp> -rm nosuch\nCouldn't delete file: No such file or directory\n",
            "sftp> bye\nstatus 0",
        ],
    );
    let o = s.cmd("md5sum /tmp/nums /tmp/nums.back; ls /root");
    let sums = md5s(&o);
    assert!(sums.len() == 2 && sums[0] == sums[1], "{o}");
    assert!(!o.contains("work"), "the directory is gone: {o}");
    // without "-", a failure ends the batch with status 1
    s.cmd(&write_lines("/tmp/b2", &["rm nosuch", "pwd"]));
    let o = s.cmd_max("sftp -b /tmp/b2 localhost; echo status $?", LONG);
    has(&o, &["Couldn't delete file: No such file or directory\nstatus 1"]);
    assert!(!o.contains("Remote working directory"), "it stopped: {o}");
}
