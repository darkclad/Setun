//! Outbound connections and DNS (docs/NETWORK.md §9): Trinux reaching
//! servers on the host through the gateway's NAT, and names through
//! 10.0.2.3. The servers here are real host sockets in the test.

mod common;
use common::*;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};

/// A tiny host-side server, polled between slices of emulation: it reads
/// a request up to `end`, then answers with `reply(request)` and closes.
struct HostServer {
    listener: TcpListener,
    open: Vec<(TcpStream, Vec<u8>)>,
    pub requests: Vec<String>,
    end: &'static [u8],
    reply: fn(&str) -> Vec<u8>,
}

impl HostServer {
    fn new(end: &'static [u8], reply: fn(&str) -> Vec<u8>) -> HostServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        HostServer { listener, open: Vec::new(), requests: Vec::new(), end, reply }
    }

    fn port(&self) -> u16 {
        self.listener.local_addr().unwrap().port()
    }

    fn poll(&mut self) {
        while let Ok((c, _)) = self.listener.accept() {
            c.set_nonblocking(true).unwrap();
            self.open.push((c, Vec::new()));
        }
        let mut k = 0;
        while k < self.open.len() {
            let (c, buf) = &mut self.open[k];
            let mut b = [0u8; 4096];
            let done = match c.read(&mut b) {
                Ok(0) => true,
                Ok(n) => {
                    buf.extend_from_slice(&b[..n]);
                    buf.windows(self.end.len()).any(|w| w == self.end)
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => false,
                Err(_) => true,
            };
            if done {
                let req = String::from_utf8_lossy(buf).into_owned();
                let answer = (self.reply)(&req);
                c.set_nonblocking(false).unwrap();
                let _ = c.write_all(&answer);
                let _ = c.shutdown(std::net::Shutdown::Both);
                self.requests.push(req);
                self.open.remove(k);
            } else {
                k += 1;
            }
        }
    }
}

/// Type a command and wait for its prompt, serving the host side meanwhile.
fn cmd_serving(s: &mut Session, srv: &mut HostServer, line: &str) -> String {
    let start = s.out.len();
    s.send(&format!("{line}\n"));
    let mut n = 0u64;
    loop {
        srv.poll();
        let o = &s.out[start..];
        if o.len() > line.len() + 1 && o.ends_with("$ ") {
            break;
        }
        assert!(n < 2_000_000_000, "timed out on {line:?}:\n{}", s.out);
        s.step();
        n += SLICE;
    }
    let text = s.out[start..].to_string();
    s.mark = s.out.len();
    let body = text.splitn(2, '\n').nth(1).unwrap_or("").to_string();
    body.strip_suffix("$ ").unwrap_or(&body).to_string()
}

fn echo_server() -> HostServer {
    HostServer::new(b"\n", |req| format!("the host got: {req}").into_bytes())
}

fn http_server() -> HostServer {
    HostServer::new(b"\r\n\r\n", |req| {
        let path = req.split(' ').nth(1).unwrap_or("/");
        match path {
            "/hello.txt" => "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\n\r\nпривет из хоста\n".as_bytes().to_vec(),
            "/big" => {
                let body: String = (1..=5000).map(|i| format!("{i}\n")).collect();
                format!("HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
            }
            _ => b"HTTP/1.0 404 Not Found\r\n\r\nnope\n".to_vec(),
        }
    })
}

#[test]
fn trinux_connects_out_to_the_host() {
    let mut s = Session::boot();
    let mut srv = echo_server();
    let p = srv.port();
    let o = cmd_serving(&mut s, &mut srv, &format!("echo hi from trinux | nc 10.0.2.2 {p}; echo status $?"));
    has(&o, &["the host got: hi from trinux\nstatus 0"]);
    // an address that stands for the host, as an outside server would
    s.net.alias([203, 0, 113, 7], "127.0.0.1".parse().unwrap());
    let o = cmd_serving(&mut s, &mut srv, &format!("echo outside | nc -v 203.0.113.7 {p}"));
    has(&o, &[&format!("Connection to 203.0.113.7 port {p} [tcp] succeeded!"), "the host got: outside\n"]);
    assert_eq!(srv.requests, vec!["hi from trinux\n", "outside\n"]);
    // a port nobody listens on
    let closed = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let o = s.cmd(&format!("nc 10.0.2.2 {closed}; echo status $?"));
    has(&o, &["nc: connect: Connection refused\nstatus 1"]);
    // with the NAT turned off, the gateway refuses at once
    s.net.set_outbound(false);
    let o = cmd_serving(&mut s, &mut srv, &format!("nc 10.0.2.2 {p}; echo status $?"));
    has(&o, &["nc: connect: Connection refused\nstatus 1"]);
    assert_eq!(srv.requests.len(), 2, "nothing reached the host");
}

#[test]
fn dns_through_10_0_2_3() {
    let mut s = Session::boot();
    s.net.dns_name("web.test", [203, 0, 113, 7]);
    let o = s.cmd("cat /etc/resolv.conf; host web.test; echo status $?");
    has(&o, &["nameserver 10.0.2.3\n", "web.test has address 203.0.113.7\nstatus 0"]);
    // the host's own resolver answers the rest
    let o = s.cmd("host localhost");
    has(&o, &["localhost has address 127.0.0.1\n"]);
    let o = s.cmd("host nosuch.invalid; echo status $?");
    has(&o, &["Host nosuch.invalid not found: 3(NXDOMAIN)\nstatus 1"]);
    // refusals and a server that is not there
    let o = s.cmd("host 'bad..name'; host -s 10.0.2.99 web.test; echo status $?");
    has(&o, &["host: 'bad..name' is not a legal name\n", ";; connection timed out; no servers could be reached\nstatus 1"]);
    // a query too short to be one: FORMERR (12 octets), not silence
    let o = s.cmd("echo junk | nc -b -u 10.0.2.3 53 | wc -c");
    assert!(o.lines().any(|l| l.trim() == "12"), "{o}");
    // names work everywhere resolve_host is used
    let o = s.cmd("ping -c 1 web.test; echo status $?");
    has(&o, &["PING web.test (203.0.113.7)"]);
}

#[test]
fn wget_over_http() {
    let mut s = Session::boot();
    let mut srv = http_server();
    let p = srv.port();
    s.net.alias([203, 0, 113, 8], "127.0.0.1".parse().unwrap());
    s.net.dns_name("web.test", [203, 0, 113, 8]);
    let o = cmd_serving(&mut s, &mut srv, &format!("wget -q -O - http://web.test:{p}/hello.txt"));
    assert_eq!(o, "привет из хоста\n");
    assert!(srv.requests[0].starts_with("GET /hello.txt HTTP/1.0\r\nHost: web.test\r\n"), "{:?}", srv.requests);
    let o = cmd_serving(&mut s, &mut srv, &format!("cd /tmp; wget http://10.0.2.2:{p}/big; tail -n 1 big; wc -l big"));
    has(&o, &["Saved 23893 octets to 'big'", "\n5000\n", "5000 big"]);
    let o = cmd_serving(&mut s, &mut srv, &format!("wget http://web.test:{p}/missing; echo status $?"));
    has(&o, &["wget: HTTP/1.0 404 Not Found\nstatus 8"]);
    let o = s.cmd("wget https://web.test/; echo status $?; wget http://nosuch.invalid/; echo status $?");
    has(&o, &["wget: https is not supported (there is no TLS)\nstatus 1", "wget: unable to resolve host address 'nosuch.invalid'\nstatus 4"]);
}

// ---- UDP through the NAT, and NTP -----------------------------------------------------------

/// Type a command and wait for its prompt, calling `serve` between slices.
fn cmd_polling(s: &mut Session, line: &str, mut serve: impl FnMut()) -> String {
    let start = s.out.len();
    s.send(&format!("{line}\n"));
    let mut n = 0u64;
    loop {
        serve();
        let o = &s.out[start..];
        if o.len() > line.len() + 1 && o.ends_with("$ ") {
            break;
        }
        assert!(n < 2_000_000_000, "timed out on {line:?}:\n{}", s.out);
        s.step();
        n += SLICE;
    }
    let text = s.out[start..].to_string();
    s.mark = s.out.len();
    let body = text.splitn(2, '\n').nth(1).unwrap_or("").to_string();
    body.strip_suffix("$ ").unwrap_or(&body).to_string()
}

fn udp_socket() -> std::net::UdpSocket {
    let u = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    u.set_nonblocking(true).unwrap();
    u
}

/// The seconds of 2030-01-01 00:00:00 UTC, in NTP's era.
const NTP_2030: u32 = (1_893_456_000u64 + 2_208_988_800u64) as u32;

/// Answer one NTP request on u: `mode` 4 and stratum 2 normally; `broken`
/// echoes the wrong originate time; `kiss` answers with stratum 0.
fn ntp_answer(u: &std::net::UdpSocket, broken: bool, kiss: bool) {
    let mut b = [0u8; 100];
    while let Ok((n, from)) = u.recv_from(&mut b) {
        if n < 48 {
            continue;
        }
        let mut r = [0u8; 48];
        r[0] = (4 << 3) | 4;
        r[1] = if kiss { 0 } else { 2 };
        r[24..32].copy_from_slice(&b[40..48]);
        if broken {
            r[24] ^= 1;
        }
        r[40..44].copy_from_slice(&NTP_2030.to_be_bytes());
        let _ = u.send_to(&r, from);
    }
}

#[test]
fn udp_out_through_the_nat() {
    let mut s = Session::boot();
    let echo = udp_socket();
    let p = echo.local_addr().unwrap().port();
    let mut serve = || {
        let mut b = [0u8; 2048];
        while let Ok((n, from)) = echo.recv_from(&mut b) {
            let mut r = b"echo: ".to_vec();
            r.extend_from_slice(&b[..n]);
            let _ = echo.send_to(&r, from);
        }
    };
    let o = cmd_polling(&mut s, &format!("echo 'hi over udp' | nc -u 10.0.2.2 {p}"), &mut serve);
    has(&o, &["echo: hi over udp\n"]);
    s.net.alias([203, 0, 113, 9], "127.0.0.1".parse().unwrap());
    let o = cmd_polling(&mut s, &format!("echo 'Сетунь' | nc -u 203.0.113.9 {p}"), &mut serve);
    has(&o, &["echo: Сетунь\n"]);
    assert!(s.net.stats.udp_out >= 2 && s.net.stats.udp_in >= 2);
    // the DNS server and test ports stay where they were
    let o = s.cmd("host localhost");
    has(&o, &["localhost has address 127.0.0.1"]);
    // with the NAT off, nothing goes out
    s.net.set_outbound(false);
    let before = s.net.stats.udp_out;
    let o = cmd_polling(&mut s, &format!("echo lost | nc -u 10.0.2.2 {p}; echo done"), &mut serve);
    has(&o, &["done\n"]);
    assert!(!o.contains("echo: lost"));
    assert_eq!(s.net.stats.udp_out, before);
}

#[test]
fn ntpdate_sets_the_clock() {
    let mut s = Session::boot();
    let good = udp_socket();
    let pg = good.local_addr().unwrap().port();
    let broken = udp_socket();
    let pb = broken.local_addr().unwrap().port();
    let kiss = udp_socket();
    let pk = kiss.local_addr().unwrap().port();
    let mut serve = || {
        ntp_answer(&good, false, false);
        ntp_answer(&broken, true, false);
        ntp_answer(&kiss, false, true);
    };
    let o = cmd_polling(&mut s, &format!("ntpdate -q -p {pg} 10.0.2.2; echo status $?; date"), &mut serve);
    has(&o, &["server 10.0.2.2, stratum 2, offset ", " s: 2030-01-01 00:00:00 UTC\nstatus 0\n"]);
    assert!(!o.contains("2030\n") && !o.contains("Jan  1"), "-q leaves the clock alone: {o}");
    let o = cmd_polling(&mut s, &format!("su user -c 'ntpdate -p {pg} 10.0.2.2'; echo status $?"), &mut serve);
    has(&o, &["ntpdate: cannot set the clock: Operation not permitted\nstatus 2"]);
    let o = cmd_polling(&mut s, &format!("ntpdate -p {pg} 10.0.2.2; echo status $?; date +%Y"), &mut serve);
    has(&o, &["ntpdate: the clock is set\nstatus 0\n2030\n"]);
    let o = cmd_polling(&mut s, &format!("ntpdate -q -p {pb} 10.0.2.2; echo status $?"), &mut serve);
    has(&o, &["ntpdate: no server suitable for synchronization found\nstatus 1"]);
    let o = cmd_polling(&mut s, &format!("ntpdate -q -p {pk} 10.0.2.2; echo status $?"), &mut serve);
    has(&o, &["ntpdate: the server is not synchronised (kiss-o'-death)\nstatus 1"]);
    let o = s.cmd("ntpdate -q nosuch.invalid; echo status $?; ntpdate; echo status $?");
    has(&o, &["ntpdate: nosuch.invalid: Name or service not known\nstatus 1", "usage: ntpdate [-q] [-p port] server\nstatus 2"]);
}
