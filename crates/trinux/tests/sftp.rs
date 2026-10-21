//! Copying files with the host's sftp and scp (docs/NETWORK.md §11):
//! /usr/lib/sftp-server behind sshd's "sftp" subsystem. Skipped without an
//! OpenSSH client and ssh-keygen on the host.

mod common;
use common::*;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// A scratch directory on the host, and a key Trinux's root will accept.
struct Host {
    dir: PathBuf,
    key: String,
    port: u16,
}

fn setup(s: &mut Session, name: &str) -> Option<Host> {
    let ok = |p: &str| Command::new(p).arg("-h").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok();
    if !ok("sftp") || !ok("ssh-keygen") {
        eprintln!("no sftp or ssh-keygen on this host: skipped");
        return None;
    }
    let dir = std::env::temp_dir().join(format!("setun-sftp-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let key = dir.join("id");
    let st = Command::new("ssh-keygen").stdin(Stdio::null()).args(["-q", "-t", "ed25519", "-N", "", "-C", "sftp-test", "-f"]).arg(&key).status().unwrap();
    assert!(st.success());
    let public = std::fs::read_to_string(dir.join("id.pub")).unwrap();
    s.cmd(&format!("mkdir /root/.ssh; echo '{}' > /root/.ssh/authorized_keys; chmod 600 /root/.ssh/authorized_keys", public.trim()));
    let port = s.net.forward("127.0.0.1:0".parse().unwrap(), 22).unwrap().port();
    Some(Host { dir, key: key.display().to_string(), port })
}

fn ssh_opts(h: &Host) -> Vec<String> {
    ["-i", &h.key, "-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null", "-o", "LogLevel=ERROR", "-o", "IdentitiesOnly=yes"]
        .iter()
        .map(|x| x.to_string())
        .collect()
}

fn sftp_batch(s: &mut Session, h: &Host, user: &str, batch: &str) -> (i32, String, String) {
    let file = h.dir.join("batch");
    std::fs::write(&file, batch).unwrap();
    let mut c = Command::new("sftp");
    c.current_dir(&h.dir).args(["-P", &h.port.to_string(), "-b"]).arg(&file).args(ssh_opts(h)).arg(format!("{user}@127.0.0.1"));
    run_host(s, &mut c, None)
}

#[test]
fn sftp_puts_gets_and_manages_files() {
    let mut s = Session::boot();
    let Some(h) = setup(&mut s, "sftp") else { return };
    let all_octets: Vec<u8> = (0..=255u8).chain(0..=255u8).collect();
    std::fs::write(h.dir.join("bin.dat"), &all_octets).unwrap();
    let text = "Сетунь: троичная машина\nline two\n";
    std::fs::write(h.dir.join("text.txt"), text).unwrap();
    let (code, out, err) = sftp_batch(
        &mut s,
        &h,
        "root",
        "mkdir /tmp/x\ncd /tmp/x\nput bin.dat\nput text.txt\nls -l\nget bin.dat bin.back\nget text.txt text.back\nget /etc/motd motd\nrename text.txt words.txt\nls\n",
    );
    assert_eq!(code, 0, "stdout:\n{out}\nstderr:\n{err}");
    assert!(out.contains("bin.dat") && out.contains("text.txt") && out.contains("words.txt"), "{out}");
    assert_eq!(std::fs::read(h.dir.join("bin.back")).unwrap(), all_octets, "512 octets, every value, both ways");
    assert_eq!(std::fs::read_to_string(h.dir.join("text.back")).unwrap(), text, "UTF-8 text both ways");
    let motd = std::fs::read_to_string(h.dir.join("motd")).unwrap();
    assert!(motd.contains("Setun-1\u{305}1") || motd.contains("Setun-11\u{305}"), "characters beyond 255 come out as UTF-8: {motd:?}");
    // in Trinux: the text became characters, the octets stayed octets
    let o = s.cmd("wc -c /tmp/x/bin.dat /tmp/x/words.txt; head -n 1 /tmp/x/words.txt");
    has(&o, &["512 /tmp/x/bin.dat", "33 /tmp/x/words.txt", "Сетунь: троичная машина\n"]);
    // tidy up, and the errors the protocol reports
    let (code, _, err) = sftp_batch(&mut s, &h, "root", "rm /tmp/x/bin.dat\nrm /tmp/x/words.txt\nrmdir /tmp/x\n-get /nonexistent here\n-rename /etc/motd /etc/passwd\n-rmdir /etc\n");
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("\"/nonexistent\" not found") && err.contains("remote rmdir \"/etc\": Failure"), "{err}");
    let o = s.cmd("ls /tmp/x; echo status $?");
    has(&o, &["No such file or directory\nstatus 1"]);
    let log = s.log("sshd");
    has(&log, &["sftp for root"]);
}

#[test]
fn scp_both_ways_and_refusals() {
    let mut s = Session::boot();
    let Some(h) = setup(&mut s, "scp") else { return };
    let body: String = (1..=3000).map(|i| format!("{i}\n")).collect();
    std::fs::write(h.dir.join("numbers"), &body).unwrap();
    let scp = |s: &mut Session, args: &[&str]| {
        let mut c = Command::new("scp");
        c.current_dir(&h.dir).args(["-P", &h.port.to_string()]).args(ssh_opts(&h)).args(args);
        run_host(s, &mut c, None)
    };
    let (code, _, err) = scp(&mut s, &["numbers", "root@127.0.0.1:/tmp/numbers"]);
    assert_eq!(code, 0, "{err}");
    let o = s.cmd("wc -l /tmp/numbers; tail -n 1 /tmp/numbers");
    has(&o, &["3000 /tmp/numbers", "\n3000\n"]);
    let (code, _, err) = scp(&mut s, &["root@127.0.0.1:/tmp/numbers", "copy"]);
    assert_eq!(code, 0, "{err}");
    assert!(std::fs::read_to_string(h.dir.join("copy")).unwrap() == body);
    // nothing there, and somewhere a user may not write
    let (code, _, err) = scp(&mut s, &["root@127.0.0.1:/nope", "x"]);
    assert_ne!(code, 0);
    assert!(err.contains("No such file"), "{err}");
    s.cmd("mkdir -p /home/user/.ssh 2>/dev/null; mkdir /home/user/.ssh; cp /root/.ssh/authorized_keys /home/user/.ssh/; chown user /home/user/.ssh /home/user/.ssh/authorized_keys");
    let (code, _, err) = scp(&mut s, &["numbers", "user@127.0.0.1:/root/numbers"]);
    assert_ne!(code, 0);
    assert!(err.contains("ermission denied"), "{err}");
}

#[test]
fn the_hosts_scp_r_into_trinux_and_back() {
    let mut s = Session::boot();
    let Some(h) = setup(&mut s, "scpr") else { return };
    let src = h.dir.join("tree");
    std::fs::create_dir_all(src.join("sub/deeper")).unwrap();
    std::fs::create_dir_all(src.join("empty")).unwrap();
    std::fs::write(src.join("a.txt"), "Сетунь: троичная машина\n").unwrap();
    std::fs::write(src.join("sub/bin.dat"), (0..=255u8).collect::<Vec<u8>>()).unwrap();
    std::fs::write(src.join("sub/deeper/n"), (1..=2000).map(|i| format!("{i}\n")).collect::<String>()).unwrap();
    let scp = |s: &mut Session, args: &[&str]| {
        let mut c = Command::new("scp");
        c.current_dir(&h.dir).args(["-r", "-P", &h.port.to_string()]).args(ssh_opts(&h)).args(args);
        run_host(s, &mut c, None)
    };
    let (code, _, err) = scp(&mut s, &["tree", "root@127.0.0.1:/tmp/hosttree"]);
    assert_eq!(code, 0, "{err}");
    let o = s.cmd("ls /tmp/hosttree /tmp/hosttree/sub; head -n 1 /tmp/hosttree/a.txt; wc -c /tmp/hosttree/sub/bin.dat");
    has(&o, &["a.txt\nempty\nsub\n", "Сетунь: троичная машина\n", "256 /tmp/hosttree/sub/bin.dat"]);
    let (code, _, err) = scp(&mut s, &["root@127.0.0.1:/tmp/hosttree", "back"]);
    assert_eq!(code, 0, "{err}");
    for f in ["a.txt", "sub/bin.dat", "sub/deeper/n"] {
        assert_eq!(std::fs::read(src.join(f)).unwrap(), std::fs::read(h.dir.join("back").join(f)).unwrap(), "{f}");
    }
    assert!(h.dir.join("back/empty").is_dir());
}
