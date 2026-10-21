//! SSH (docs/NETWORK.md §8): Trinux's sshd against the host's OpenSSH
//! client, through a port forward. Skipped when the host has no `ssh`
//! (set SETUN_SSH to choose one).

mod common;
use common::*;
use std::process::{Command, Stdio};

/// The host's OpenSSH client, if there is one.
fn ssh_client() -> Option<String> {
    let cand = std::env::var("SETUN_SSH").unwrap_or_else(|_| "ssh".into());
    Command::new(&cand).arg("-V").stdin(Stdio::null()).stderr(Stdio::null()).status().ok().filter(|s| s.success()).map(|_| cand)
}

/// A script that answers ssh's password prompt.
fn askpass(password: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("setun-askpass-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(format!("askpass-{password}.sh"));
    std::fs::write(&p, format!("#!/bin/sh\necho '{password}'\n")).unwrap();
    p
}

/// Run `ssh user@trinux command` against the session's forward, stepping
/// the machine until the client exits: (status, stdout, stderr).
fn run_ssh(s: &mut Session, port: u16, user: &str, password: &str, extra: &[&str], command: &str) -> (i32, String, String) {
    run_ssh_input(s, port, user, password, extra, command, None)
}

fn run_ssh_input(
    s: &mut Session,
    port: u16,
    user: &str,
    password: &str,
    extra: &[&str],
    command: &str,
    input: Option<Vec<u8>>,
) -> (i32, String, String) {
    let ssh = ssh_client().expect("an ssh client");
    let ask = askpass(password);
    let mut cmd = Command::new(ssh);
    // ssh takes the first value of each option: the test's own go first
    cmd.args(extra)
        .args(["-p", &port.to_string()])
        .args(["-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null", "-o", "LogLevel=ERROR"])
        .args(["-o", "PreferredAuthentications=password", "-o", "PubkeyAuthentication=no", "-o", "NumberOfPasswordPrompts=1"])
        .arg(format!("{user}@127.0.0.1"))
        .args(if command.is_empty() { vec![] } else { vec![command] })
        .env("SSH_ASKPASS", &ask)
        .env("SSH_ASKPASS_REQUIRE", "force")
        .env("DISPLAY", ":0")
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("run ssh");
    if let Some(data) = input {
        let mut stdin = child.stdin.take().unwrap();
        std::thread::spawn(move || {
            use std::io::Write;
            let _ = stdin.write_all(&data);
        });
    }
    // drain the client's output as it comes, or a full pipe stops it
    let reader = |mut r: Box<dyn std::io::Read + Send>| {
        std::thread::spawn(move || {
            let mut v = Vec::new();
            let _ = r.read_to_end(&mut v);
            v
        })
    };
    let out_t = reader(Box::new(child.stdout.take().unwrap()));
    let err_t = reader(Box::new(child.stderr.take().unwrap()));
    let start = std::time::Instant::now();
    loop {
        if let Some(st) = child.try_wait().unwrap() {
            let out = out_t.join().unwrap();
            let err = err_t.join().unwrap();
            return (
                st.code().unwrap_or(-1),
                String::from_utf8_lossy(&out).into_owned(),
                String::from_utf8_lossy(&err).into_owned(),
            );
        }
        if start.elapsed().as_secs() > 120 {
            // what the machine says about it, then give up
            let state = s.cmd("netstat -a; ps; cat /proc/net/stat | grep tcp");
            let _ = child.kill();
            let tail: String = s.out.chars().rev().take(1500).collect::<Vec<_>>().into_iter().rev().collect();
            panic!("ssh did not finish; the machine:\n{state}\nthe console ends:\n{tail}");
        }
        s.step();
    }
}

fn booted() -> Option<(Session, u16)> {
    if ssh_client().is_none() {
        eprintln!("no ssh client on this host: skipped");
        return None;
    }
    let mut s = Session::boot();
    let port = s.net.forward("127.0.0.1:0".parse().unwrap(), 22).unwrap().port();
    Some((s, port))
}

#[test]
fn openssh_logs_in_and_runs_commands() {
    let Some((mut s, port)) = booted() else { return };
    has(&s.log("sshd"), &["sshd: host key ED25519 SHA256:"]);
    let (code, out, err) = run_ssh(&mut s, port, "root", "root", &[], "echo привет from $USER; id; exit 3");
    assert_eq!(code, 3, "stdout:\n{out}\nstderr:\n{err}\nconsole:\n{}", s.out);
    assert_eq!(out, "привет from root\nuid=0(root) gid=0(root) groups=0(root),10(wheel)\n");
    let (code, out, _) = run_ssh(&mut s, port, "user", "user", &[], "whoami; pwd; ls /root");
    assert_eq!(code, 1);
    assert_eq!(out, "user\n/home/user\n");
    // a wrong password
    let (code, _, err) = run_ssh(&mut s, port, "user", "nope", &[], "true");
    assert_eq!(code, 255, "{err}");
    assert!(err.contains("Permission denied"), "{err}");
    has(&s.log("sshd"), &["accepted password for root", "failed password for user"]);
}

/// ML-KEM-768 with X25519 (docs/TNET.md §6) against OpenSSH, which also
/// knows curve25519: forced each way, and OpenSSH's own report of what was
/// agreed.
#[test]
fn post_quantum_key_exchange_with_openssh() {
    let Some((mut s, port)) = booted() else { return };
    let (code, out, err) = run_ssh(&mut s, port, "root", "root", &["-v", "-oKexAlgorithms=mlkem768x25519-sha256"], "echo pq ok");
    assert_eq!((code, out.as_str()), (0, "pq ok\n"), "stderr:\n{err}");
    assert!(err.contains("kex: algorithm: mlkem768x25519-sha256"), "{err}");
    let (code, out, err) = run_ssh(&mut s, port, "root", "root", &["-v", "-oKexAlgorithms=curve25519-sha256"], "echo classic ok");
    assert_eq!((code, out.as_str()), (0, "classic ok\n"), "stderr:\n{err}");
    assert!(err.contains("kex: algorithm: curve25519-sha256"), "{err}");
    // the client's order decides, not the server's
    let (code, _, err) = run_ssh(&mut s, port, "root", "root", &["-v", "-oKexAlgorithms=curve25519-sha256,mlkem768x25519-sha256"], "true");
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("kex: algorithm: curve25519-sha256"), "{err}");
    has(&s.log("sshd"), &["key exchange mlkem768x25519-sha256", "key exchange curve25519-sha256"]);
    // OpenSSH's default list puts the hybrid first: that is what it gets
    let (code, _, err) = run_ssh(&mut s, port, "root", "root", &["-v"], "true");
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("kex: algorithm: mlkem768x25519-sha256"), "{err}");
    // Streamlined NTRU Prime with X25519, hashed with SHA-512 (a 64-octet
    // session id, which the public-key login signs over), by both names
    for name in ["sntrup761x25519-sha512", "sntrup761x25519-sha512@openssh.com"] {
        let opt = format!("-oKexAlgorithms={name}");
        let (code, out, err) = run_ssh(&mut s, port, "root", "root", &["-v", &opt], "echo ntru ok");
        assert_eq!((code, out.as_str()), (0, "ntru ok\n"), "stderr:\n{err}");
        assert!(err.contains(&format!("kex: algorithm: {name}")), "{err}");
    }
    has(&s.log("sshd"), &["key exchange sntrup761x25519-sha512"]);
    // a key exchange the server does not have
    let (code, _, err) = run_ssh(&mut s, port, "root", "root", &["-v", "-oKexAlgorithms=diffie-hellman-group14-sha256"], "true");
    assert_eq!(code, 255, "{err}");
    assert!(err.contains("no matching key exchange method found. Their offer: mlkem768x25519-sha256,sntrup761x25519-sha512,sntrup761x25519-sha512@openssh.com,curve25519-sha256,"), "{err}");
}

#[test]
fn an_interactive_session_on_a_pty() {
    let Some((mut s, port)) = booted() else { return };
    let typed = "echo TERM=$TERM\necho жук\nexit 7\n".as_bytes().to_vec();
    let (code, out, err) = run_ssh_input(&mut s, port, "root", "root", &["-tt"], "", Some(typed));
    assert_eq!(code, 7, "stdout:\n{out}\nstderr:\n{err}");
    assert!(out.contains("Welcome to Trinux"), "the motd: {out}");
    assert!(out.contains("TERM="), "{out}");
    assert!(out.contains("жук\r\n"), "UTF-8 both ways, CR LF on the pty: {out:?}");
    has(&s.log("sshd"), &["shell for root"]);
}

#[test]
fn large_transfers_both_ways_through_the_windows() {
    let Some((mut s, port)) = booted() else { return };
    let (code, out, err) = run_ssh(&mut s, port, "root", "root", &[], "seq 1 30000 | tail -n 1");
    assert_eq!((code, out.as_str()), (0, "30000\n"), "{err}");
    let (code, out, err) = run_ssh(&mut s, port, "root", "root", &[], "seq 1 20000");
    let want: String = (1..=20000).map(|i| format!("{i}\n")).collect();
    assert_eq!(code, 0, "{err}");
    assert!(out == want, "{} octets of output arrive whole ({} received)", want.len(), out.len());
    let data: Vec<u8> = (0..120_000u32).map(|i| b'a' + (i % 26) as u8).collect();
    let (code, out, err) = run_ssh_input(&mut s, port, "root", "root", &[], "wc -c", Some(data));
    assert_eq!((code, out.trim()), (0, "120000"), "{err}\n{}", s.out.lines().filter(|l| l.contains("sshd")).collect::<Vec<_>>().join("\n"));
}

#[test]
fn refusals() {
    let Some((mut s, port)) = booted() else { return };
    for (opt, says) in [
        ("Ciphers=aes128-ctr", "no matching cipher"),
        ("KexAlgorithms=diffie-hellman-group14-sha256", "no matching key exchange method"),
        ("HostKeyAlgorithms=rsa-sha2-512", "no matching host key type"),
    ] {
        let (code, _, err) = run_ssh(&mut s, port, "root", "root", &["-o", opt, "-o", "LogLevel=INFO"], "true");
        assert_eq!(code, 255, "{opt}: {err}");
        assert!(err.contains(says), "{opt}: {err}");
    }
    // seven wrong passwords: the server stops at six
    let (code, _, err) = run_ssh(&mut s, port, "user", "nope", &["-o", "NumberOfPasswordPrompts=7"], "true");
    assert_eq!(code, 255);
    assert!(err.contains("Too many authentication failures"), "{err}");
    // an unknown user is refused the same way as a wrong password
    let (code, _, err) = run_ssh(&mut s, port, "nobody", "x", &[], "true");
    assert_eq!(code, 255);
    assert!(err.contains("Permission denied"), "{err}");
    // a command that is not there
    let (code, _, err) = run_ssh(&mut s, port, "root", "root", &[], "nosuchprogram");
    assert_eq!(code, 127, "{err}");
}

#[test]
fn not_ssh_2() {
    let Some((mut s, _)) = booted() else { return };
    let c = s.net.connect(22);
    s.run_until("connected", 100_000_000, |s| s.net.state(c) == setun_net::State::Established);
    s.net.send(c, b"SSH-1.5-ancient\r\n");
    s.run_until("the server hangs up", 300_000_000, |s| s.net.guest_closed(c));
    let got = String::from_utf8(s.net.recv(c)).unwrap();
    assert_eq!(got, "SSH-2.0-TrinuxSSH_0.1\r\nProtocol mismatch.\r\n");
    has(&s.log("sshd"), &["not SSH 2 from 10.0.2.2"]);
    // a line longer than the 255 RFC 4253 allows, before any version
    let c = s.net.connect(22);
    s.run_until("connected", 100_000_000, |s| s.net.state(c) == setun_net::State::Established);
    let mut long = vec![b'x'; 300];
    long.extend(b"\r\nSSH-2.0-late\r\n");
    s.net.send(c, &long);
    s.run_until("the server hangs up", 300_000_000, |s| s.net.guest_closed(c));
    has(&s.log("sshd"), &["an overlong line before the version from 10.0.2.2"]);
}

#[test]
fn a_tampered_packet_ends_the_connection() {
    let Some((mut s, port)) = booted() else { return };
    // from the fifth data segment the client sends (past NEWKEYS), flip the
    // last octet; the TCP checksum is made right again, so only SSH's MAC can tell
    let mut seen = 0;
    s.net.set_mangle(move |d, f| {
        if d != setun_net::Dir::ToGuest {
            return;
        }
        let Some(r) = setun_net::tcp_payload(f) else { return };
        if r.is_empty() {
            return;
        }
        seen += 1;
        if seen >= 5 {
            f[r.end - 1] ^= 0x40;
            setun_net::fix_checksums(f);
        }
    });
    let (code, _, err) = run_ssh(&mut s, port, "root", "root", &[], "true");
    assert_eq!(code, 255, "{err}");
    has(&s.log("sshd"), &["disconnecting: message authentication failed"]);
}

/// A new key pair from the host's ssh-keygen: (private key path, public line).
fn keygen(kind: &str, name: &str) -> Option<(String, String)> {
    let dir = std::env::temp_dir().join(format!("setun-keys-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    let key = dir.join(name);
    let _ = std::fs::remove_file(&key);
    let _ = std::fs::remove_file(key.with_extension("pub"));
    let ok = Command::new("ssh-keygen").stdin(Stdio::null())
        .args(["-q", "-t", kind, "-N", "", "-C", name, "-f"])
        .arg(&key)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?
        .success();
    if !ok {
        return None;
    }
    let public = std::fs::read_to_string(format!("{}.pub", key.display())).ok()?;
    Some((key.display().to_string(), public.trim().to_string()))
}

#[test]
fn public_key_logins() {
    let Some((mut s, port)) = booted() else { return };
    let Some((key, public)) = keygen("ed25519", "trusted") else {
        eprintln!("no ssh-keygen on this host: skipped");
        return;
    };
    let (other, _) = keygen("ed25519", "stranger").unwrap();
    let (rsa, rsa_public) = keygen("rsa", "rsa").unwrap();
    s.cmd(&format!("mkdir /root/.ssh; echo '{rsa_public}' > /root/.ssh/authorized_keys; echo '{public}' >> /root/.ssh/authorized_keys; chmod 600 /root/.ssh/authorized_keys"));
    let with_key = |k: &str| -> Vec<String> {
        ["-i", k, "-o", "IdentitiesOnly=yes", "-o", "PubkeyAuthentication=yes", "-o", "PreferredAuthentications=publickey"]
            .iter()
            .map(|x| x.to_string())
            .collect()
    };
    let opts = with_key(&key);
    let o: Vec<&str> = opts.iter().map(|x| x.as_str()).collect();
    let (code, out, err) = run_ssh(&mut s, port, "root", "unused", &o, "echo by key; whoami");
    assert_eq!((code, out.as_str()), (0, "by key\nroot\n"), "{err}");
    has(&s.log("sshd"), &["accepted publickey for root"]);
    // after sntrup761x25519-sha512 the session id the key signs is 64 octets
    let mut o2 = vec!["-oKexAlgorithms=sntrup761x25519-sha512"];
    o2.extend(o.iter());
    let (code, out, err) = run_ssh(&mut s, port, "root", "unused", &o2, "echo by key after ntru");
    assert_eq!((code, out.as_str()), (0, "by key after ntru\n"), "{err}");
    // a key that is not in the file
    let opts = with_key(&other);
    let o: Vec<&str> = opts.iter().map(|x| x.as_str()).collect();
    let (code, _, err) = run_ssh(&mut s, port, "root", "unused", &o, "true");
    assert_eq!(code, 255);
    assert!(err.contains("Permission denied (publickey,password)"), "{err}");
    has(&s.log("sshd"), &["key not authorized for root"]);
    // an RSA key, even one listed: only ssh-ed25519
    let opts = with_key(&rsa);
    let o: Vec<&str> = opts.iter().map(|x| x.as_str()).collect();
    let (code, _, err) = run_ssh(&mut s, port, "root", "unused", &o, "true");
    assert_eq!(code, 255, "{err}");
    // the right key for the wrong user
    let opts = with_key(&key);
    let o: Vec<&str> = opts.iter().map(|x| x.as_str()).collect();
    let (code, _, _) = run_ssh(&mut s, port, "user", "unused", &o, "true");
    assert_eq!(code, 255);
    // a file others may write is not trusted
    s.cmd("chmod 666 /root/.ssh/authorized_keys");
    let (code, _, _) = run_ssh(&mut s, port, "root", "unused", &o, "true");
    assert_eq!(code, 255);
    has(&s.log("sshd"), &["ignored (group or others may write it): /root/.ssh/authorized_keys"]);
}

/// IPv6 end to end (docs/IPV6.md, V3's gate): the host's OpenSSH on ::1, a
/// `--net6` forward, and Trinux's sshd answering over IPv6.
#[test]
fn openssh_logs_in_over_ipv6() {
    if ssh_client().is_none() {
        eprintln!("no ssh client on this host: skipped");
        return;
    }
    let mut s = Session::boot();
    let port = s.net.forward6("[::1]:0".parse().unwrap(), 22).unwrap().port();
    let (code, out, err) = run_ssh(&mut s, port, "root", "root", &["-o", "HostName=::1"], "echo over IPv6; netstat -t | grep tcp6");
    assert_eq!(code, 0, "stdout:\n{out}\nstderr:\n{err}\nconsole:\n{}", s.out);
    assert!(out.starts_with("over IPv6\ntcp6 "), "{out}");
    assert!(out.contains(":22") && out.contains("fd00::2:") && out.contains("ESTABLISHED"), "{out}");
    has(&s.log("sshd"), &["connection from fd00::2\n", "accepted password for root"]);
}
