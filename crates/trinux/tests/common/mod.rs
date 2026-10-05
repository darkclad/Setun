//! A booted Trinux behind the setun-net gateway, for the network tests.
#![allow(dead_code)]

use setun_core::Machine;
use setun_net::Network;

/// A booted Trinux with a network, logged in as root.
pub struct Session {
    pub m: Machine,
    pub net: Network,
    pub out: String,
    /// Where `expect` starts looking.
    pub mark: usize,
    /// Echo every UDP datagram that reaches the gateway's port 7 back to its sender.
    pub udp_echo: Option<setun_net::UdpId>,
}

pub const SLICE: u64 = 20_000; // 2 ms of emulated time between network polls

impl Session {
    pub fn boot() -> Session {
        Self::boot_with(true)
    }

    pub fn boot_with(link: bool) -> Session {
        let disk = trinux::build_disk(&trinux::kernel_dir()).unwrap_or_else(|e| panic!("{e}"));
        Self::boot_disk(&disk.trytes, link)
    }

    /// Boot from this disk image (logged in as root).
    pub fn boot_disk(disk: &[i16], link: bool) -> Session {
        Self::boot_disk_ram(disk, link, 729)
    }

    /// `boot_disk` on a machine with this many frames of RAM.
    pub fn boot_disk_ram(disk: &[i16], link: bool, frames: i64) -> Session {
        let mut m = trinux::boot_trytes(disk, frames, 0, "");
        let net = Network::attach(&mut m);
        if !link {
            m.dev.nic.link = false;
        }
        let mut s = Session { m, net, out: String::new(), mark: 0, udp_echo: None };
        s.wait_for("login: ", 400_000_000);
        s.send("root\n");
        s.wait_for("Password: ", 100_000_000);
        s.send("root\n");
        s.wait_prompt(200_000_000);
        if link {
            // let sshd finish starting (it makes its host key) before a test begins
            let mut tries = 0;
            while !s.cmd("cat /var/log/sshd.log 2>/dev/null").contains("sshd: listening on port 22") {
                tries += 1;
                assert!(tries < 300, "sshd did not start:\n{}", s.out);
                s.idle(10_000_000);
            }
        }
        s
    }

    /// What a daemon has logged so far (/var/log/NAME.log).
    pub fn log(&mut self, name: &str) -> String {
        self.cmd(&format!("cat /var/log/{name}.log"))
    }

    pub fn step(&mut self) {
        if self.net.waiting_on_host() > 0 {
            // the host works in real time: keep the guest's clock near it
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        self.m.run(SLICE);
        self.net.poll(&mut self.m);
        if let Some(u) = self.udp_echo {
            while let Some((port, data)) = self.net.udp_recv(u) {
                self.net.udp_send(u, port, &data);
            }
        }
        let fresh = self.m.dev.serial.take_output();
        self.out.push_str(&fresh);
        assert!(!self.out.contains("Kernel panic"), "kernel panic:\n{}", self.out);
    }

    /// Run until `pred` holds on the output after the mark; panics after `max` steps.
    pub fn until(&mut self, what: &str, max: u64, pred: impl Fn(&str) -> bool) {
        let mut n = 0;
        while !pred(&self.out[self.mark..]) {
            assert!(n < max, "timed out waiting for {what:?}; output:\n{}", self.out);
            self.step();
            n += SLICE;
        }
    }

    pub fn wait_for(&mut self, s: &str, max: u64) {
        let s2 = s.to_string();
        self.until(s, max, move |o| o.contains(&s2));
        self.mark = self.out.len();
    }

    pub fn wait_prompt(&mut self, max: u64) {
        self.until("$ ", max, |o| o.ends_with("$ "));
        self.mark = self.out.len();
    }

    /// Run for `n` steps (polling the network).
    pub fn idle(&mut self, n: u64) {
        let mut k = 0;
        while k < n {
            self.step();
            k += SLICE;
        }
    }

    pub fn send(&mut self, s: &str) {
        self.m.dev.serial_input(s);
    }

    /// Type a command, wait for the next prompt, return what it printed.
    pub fn cmd(&mut self, line: &str) -> String {
        self.cmd_max(line, 600_000_000)
    }

    pub fn cmd_max(&mut self, line: &str, max: u64) -> String {
        let start = self.out.len();
        self.send(&format!("{line}\n"));
        self.until(line, max, |o| o.ends_with("$ ") && o.len() > line.len() + 1);
        let text = self.out[start..].to_string();
        self.mark = self.out.len();
        // drop the echoed command line and the prompt
        let body = text.splitn(2, '\n').nth(1).unwrap_or("");
        body.strip_suffix("$ ").unwrap_or(body).to_string()
    }

    /// Write a text file in Trinux (through a here-document, typed a line
    /// at a time so the terminal's input buffer never overflows).
    pub fn put_file(&mut self, path: &str, text: &str) {
        assert!(!text.lines().any(|l| l == "EOF"));
        let start = self.out.len();
        let mut line = format!("cat > {path} <<'EOF'");
        for next in text.lines().chain(["EOF"]) {
            self.mark = self.out.len();
            self.send(&format!("{line}\n"));
            self.until(&line, 100_000_000, |o| o.ends_with("> "));
            line = next.to_string();
        }
        self.mark = self.out.len();
        self.send("EOF
");
        self.until("EOF", 200_000_000, |o| o.ends_with("$ "));
        let o = self.out[start..].to_string();
        self.mark = self.out.len();
        assert!(!o.contains("sh:") && !o.contains("cat:"), "{o}");
    }
}

pub fn has(out: &str, wants: &[&str]) {
    for w in wants {
        assert!(out.contains(w), "missing {w:?} in:\n{out}");
    }
}


impl Session {
    /// Type a line without waiting for a prompt.
    pub fn type_line(&mut self, line: &str) {
        self.send(&format!("{line}\n"));
    }

    pub fn run_until(&mut self, what: &str, max: u64, mut done: impl FnMut(&mut Session) -> bool) {
        let mut n = 0;
        while !done(self) {
            assert!(n < max, "timed out: {what}; output:\n{}", self.out);
            self.step();
            n += SLICE;
        }
    }
}

pub fn stat(s: &mut Session, name: &str) -> u64 {
    let o = s.cmd("cat /proc/net/stat");
    let line = o.lines().find(|l| l.starts_with(&format!("{name} "))).unwrap_or_else(|| panic!("no {name} in {o}"));
    line.split(' ').nth(1).unwrap().parse().unwrap()
}

/// Run a host program (an ssh, sftp or scp client) while the machine runs,
/// until it exits: (status, stdout, stderr). Its output is drained as it
/// comes, so a full pipe never stops it.
pub fn run_host(s: &mut Session, cmd: &mut std::process::Command, input: Option<Vec<u8>>) -> (i32, String, String) {
    use std::process::Stdio;
    cmd.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("run the host program");
    if let Some(data) = input {
        let mut stdin = child.stdin.take().unwrap();
        std::thread::spawn(move || {
            use std::io::Write;
            let _ = stdin.write_all(&data);
        });
    }
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
            return (st.code().unwrap_or(-1), String::from_utf8_lossy(&out).into_owned(), String::from_utf8_lossy(&err).into_owned());
        }
        if start.elapsed().as_secs() > 180 {
            let _ = child.kill();
            let tail: String = s.out.chars().rev().take(1500).collect::<Vec<_>>().into_iter().rev().collect();
            panic!("the host program did not finish; the console ends:\n{tail}");
        }
        s.step();
    }
}
