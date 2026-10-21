//! The ssh client and ssh-keygen in Trinux, against Trinux's own sshd on
//! loopback (docs/NETWORK.md §10).

mod common;
use common::*;

const LONG: u64 = 20_000_000_000;

/// Type a line, answer the password prompt, wait for the shell's prompt.
fn with_password(s: &mut Session, line: &str, password: &str) -> String {
    let start = s.out.len();
    s.send(&format!("{line}\n"));
    s.until("the password prompt", LONG, |o| o.contains("'s password: "));
    s.send(&format!("{password}\n"));
    s.until("the prompt", LONG, |o| o.ends_with("$ ") && o.contains("'s password: ") && !o.trim_end().ends_with("password:"));
    let o = s.out[start..].to_string();
    s.mark = s.out.len();
    o
}

#[test]
fn ssh_by_password_and_known_hosts() {
    let mut s = Session::boot();
    let o = with_password(&mut s, "ssh -i /nokey root@localhost 'echo inner $USER; exit 4'; echo status $?", "root");
    has(&o, &["root@localhost's password: ", "Warning: Permanently added 'localhost' (ED25519) to the list of known hosts.\n", "inner root\nstatus 4\n"]);
    let o = s.cmd("cat /root/.ssh/known_hosts");
    assert!(o.contains("localhost ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAA"), "{o}");
    // the second time the key is known: no warning
    let o = with_password(&mut s, "ssh -i /nokey -p 22 root@localhost uname -m", "root");
    assert!(o.contains("setun11\u{305}\n") && !o.contains("Warning"), "{o}");
    // three wrong passwords
    let start = s.out.len();
    s.send("ssh -i /nokey user@127.0.0.1 true; echo status $?\n");
    for _ in 0..3 {
        s.until("a password prompt", LONG, |o| o.ends_with("'s password: "));
        s.send("wrong\n");
        s.idle(1_000_000);
    }
    s.until("the end", LONG, |o| o.contains("status "));
    s.wait_prompt(LONG);
    let o = s.out[start..].to_string();
    has(&o, &["Permission denied, please try again.\n", "user@127.0.0.1: Permission denied (publickey,password).\nstatus 255"]);
}

#[test]
fn ssh_by_key_and_a_changed_host_key() {
    let mut s = Session::boot();
    let o = s.cmd_max("ssh-keygen -C tester; cat /root/.ssh/id_ed25519.pub >> /root/.ssh/authorized_keys; chmod 600 /root/.ssh/authorized_keys; ssh-keygen -l -f /root/.ssh/id_ed25519.pub", LONG);
    has(&o, &["Generating public/private ed25519 key pair.\n", "Your identification has been saved in /root/.ssh/id_ed25519\n", "The key fingerprint is:\nSHA256:", " tester\n", "256 SHA256:"]);
    let o = s.cmd_max("ls -l /root/.ssh/id_ed25519; ssh-keygen; echo status $?", LONG);
    has(&o, &["-rw------- 1 root     root", "ssh-keygen: /root/.ssh/id_ed25519 already exists\nstatus 1"]);
    let o = s.cmd_max("ssh localhost 'echo by key; id -u 2>/dev/null; whoami'; echo status $?", LONG);
    assert!(!o.contains("password"), "no password asked: {o}");
    has(&o, &["by key\n", "root\nstatus 0"]);
    has(&s.log("sshd"), &["accepted publickey for root"]);
    // someone else's key on record for localhost
    let o = s.cmd_max("sed 's/^/localhost /' /root/.ssh/id_ed25519.pub > /root/.ssh/known_hosts; ssh localhost true; echo status $?", LONG);
    has(&o, &["WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!", "Host key verification failed.\nstatus 255"]);
    // nobody there, and no such host
    let o = s.cmd("ssh -p 2222 localhost true; echo status $?; ssh nosuch.invalid true; echo status $?");
    has(&o, &["ssh: connect to host localhost port 2222: Connection refused\nstatus 255", "ssh: Could not resolve hostname nosuch.invalid: Name or service not known\nstatus 255"]);
}

#[test]
fn an_interactive_ssh_session() {
    let mut s = Session::boot();
    s.send("ssh -i /nokey user@localhost\n");
    s.until("the password prompt", LONG, |o| o.contains("'s password: "));
    s.send("user\n");
    s.until("the remote motd and prompt", LONG, |o| o.contains("Welcome to Trinux") && o.ends_with("$ "));
    s.mark = s.out.len();
    s.send("echo deep inside $USER\r");
    s.until("the remote command", LONG, |o| o.contains("deep inside user"));
    s.mark = s.out.len();
    s.send("exit\r");
    // the remote echo, the remote shell's "exit", then our own prompt
    s.until("back home", LONG, |o| o.matches("exit").count() >= 2 && o.ends_with("$ "));
    s.mark = s.out.len();
    let o = s.cmd("echo back as $USER");
    has(&o, &["back as root\n"]);
    // "~." leaves without the remote shell's help
    s.send("ssh -i /nokey user@localhost\n");
    s.until("the password prompt", LONG, |o| o.contains("'s password: "));
    s.send("user\n");
    s.until("the remote prompt", LONG, |o| o.contains("Welcome to Trinux") && o.ends_with("$ "));
    s.mark = s.out.len();
    s.send("\r~.");
    s.until("closed", LONG, |o| o.contains("Connection to localhost closed."));
    s.until("back home", LONG, |o| o.ends_with("$ "));
    s.mark = s.out.len();
    let o = s.cmd("echo still $USER");
    has(&o, &["still root\n"]);
}

/// IPv6 by name (docs/IPV6.md, V4's gate): a name with only an AAAA record,
/// from the gateway's DNS, for Trinux's own global address; ssh and scp to
/// it, and scp with the address itself in brackets.
#[test]
fn ssh_and_scp_by_name_over_ipv6() {
    let mut s = Session::boot();
    let mut tries = 0;
    while !s.out.contains("eth0: inet6 fd00::5054:ff:fe12:3456 scope global\n") {
        tries += 1;
        assert!(tries < 100, "no global address");
        s.idle(10_000_000);
    }
    s.net.dns_name6("trinux6.test", setun_net::GUEST6);
    let o = with_password(&mut s, "ssh -i /nokey root@trinux6.test 'netstat -t | grep ESTAB'; echo status $?", "root");
    has(&o, &["root@trinux6.test's password: ", "status 0\n"]);
    assert!(o.contains("tcp6") && o.contains("fd00::5054:ff:fe12:3456:22"), "{o}");
    has(&s.log("sshd"), &["connection from fd00::5054:ff:fe12:3456\n"]);
    s.cmd("echo copied over IPv6 > /tmp/src");
    let o = with_password(&mut s, "scp -i /nokey /tmp/src 'root@[fd00::5054:ff:fe12:3456]:/tmp/dst'; echo status $?", "root");
    has(&o, &["status 0\n"]);
    has(&s.cmd("cat /tmp/dst"), &["copied over IPv6\n"]);
    let o = with_password(&mut s, "scp -i /nokey root@trinux6.test:/tmp/dst /tmp/back; echo status $?", "root");
    has(&o, &["status 0\n"]);
    has(&s.cmd("cat /tmp/back"), &["copied over IPv6\n"]);
}
