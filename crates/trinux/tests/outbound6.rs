//! IPv6 through the gateway (docs/IPV6.md §5, V3): Trinux reaching servers
//! on the host's ::1 through fd00::2 and the NAT, and the host reaching
//! Trinux through a port forward over IPv6. The servers here are real host
//! sockets in the test.

mod common;
use common::*;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};

/// A host-side server on [::1]: it reads a request up to `end`, answers
/// `reply(request)` and closes. `EchoServer::new()` answers a line with
/// "the host got: LINE".
struct EchoServer {
    listener: TcpListener,
    open: Vec<(TcpStream, Vec<u8>)>,
    requests: Vec<String>,
    end: &'static [u8],
    reply: fn(&str) -> Vec<u8>,
}

impl EchoServer {
    fn new() -> EchoServer {
        Self::speaking(b"\n", |req| format!("the host got: {req}").into_bytes())
    }

    fn speaking(end: &'static [u8], reply: fn(&str) -> Vec<u8>) -> EchoServer {
        let listener = TcpListener::bind("[::1]:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        EchoServer { listener, open: Vec::new(), requests: Vec::new(), end, reply }
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
                c.set_nonblocking(false).unwrap();
                let _ = c.write_all(&(self.reply)(&req));
                let _ = c.shutdown(std::net::Shutdown::Both);
                self.requests.push(req);
                self.open.remove(k);
            } else {
                k += 1;
            }
        }
    }
}

/// Type a command and wait for its prompt, running `serve` meanwhile.
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

/// Booted, with the global IPv6 address configured.
fn boot6() -> Session {
    let mut s = Session::boot();
    let mut tries = 0;
    while !s.out.contains("eth0: inet6 fd00::5054:ff:fe12:3456 scope global\n") {
        tries += 1;
        assert!(tries < 100, "no global address:\n{}", s.out);
        s.idle(10_000_000);
    }
    s
}

#[test]
fn trinux_connects_out_over_ipv6() {
    let mut s = boot6();
    let mut srv = EchoServer::new();
    let p = srv.port();
    // fd00::2 stands for the host's ::1
    let o = cmd_polling(&mut s, &format!("echo hi over IPv6 | nc fd00::2 {p}; echo status $?"), || srv.poll());
    has(&o, &["the host got: hi over IPv6\nstatus 0"]);
    // an outside address that stands for the host, as a real server would
    s.net.alias6("2001:db8::7".parse().unwrap(), "::1".parse().unwrap());
    let o = cmd_polling(&mut s, &format!("echo outside | nc -v 2001:db8::7 {p}"), || srv.poll());
    has(&o, &[&format!("Connection to 2001:db8::7 port {p} [tcp] succeeded!"), "the host got: outside\n"]);
    assert_eq!(srv.requests, vec!["hi over IPv6\n", "outside\n"]);
    assert!(s.net.outbound_flows() <= 2);
    // a port nobody listens on
    let closed = TcpListener::bind("[::1]:0").unwrap().local_addr().unwrap().port();
    let o = s.cmd(&format!("nc fd00::2 {closed}; echo status $?"));
    has(&o, &["nc: connect: Connection refused\nstatus 1"]);
    // on the link, the NAT stays out of the way: fd00::99 is nobody, so no host connection
    let before = s.net.outbound_flows();
    let o = s.cmd(&format!("nc -w 2 fd00::99 {p}; echo status $?"));
    has(&o, &["status 1"]);
    assert_eq!(s.net.outbound_flows(), before, "nothing went to the host");
    // with the NAT off, the gateway refuses at once
    s.net.set_outbound(false);
    let o = cmd_polling(&mut s, &format!("nc fd00::2 {p}; echo status $?"), || srv.poll());
    has(&o, &["nc: connect: Connection refused\nstatus 1"]);
    assert_eq!(srv.requests.len(), 2, "nothing more reached the host");
}

#[test]
fn udp_out_over_ipv6() {
    let mut s = boot6();
    let echo = UdpSocket::bind("[::1]:0").unwrap();
    echo.set_nonblocking(true).unwrap();
    let p = echo.local_addr().unwrap().port();
    let mut froms = Vec::new();
    let mut serve = || {
        let mut b = [0u8; 2048];
        while let Ok((n, from)) = echo.recv_from(&mut b) {
            let mut r = b"echo: ".to_vec();
            r.extend_from_slice(&b[..n]);
            let _ = echo.send_to(&r, from);
            froms.push(from);
        }
    };
    let o = cmd_polling(&mut s, &format!("echo 'datagram over IPv6' | nc -u fd00::2 {p}"), &mut serve);
    has(&o, &["echo: datagram over IPv6\n"]);
    s.net.alias6("2001:db8::9".parse().unwrap(), "::1".parse().unwrap());
    let o = cmd_polling(&mut s, &format!("echo 'Сетунь' | nc -u 2001:db8::9 {p}"), &mut serve);
    has(&o, &["echo: Сетунь\n"]);
    assert!(froms.iter().all(|f| f.is_ipv6()), "{froms:?}");
    assert!(s.net.stats.udp_out >= 2 && s.net.stats.udp_in >= 2);
    assert!(s.net.udp_flows() >= 2);
}

/// A port forward that reaches Trinux over IPv6 (`--net6`): the host side
/// on ::1 or 127.0.0.1, the guest side always IPv6.
#[test]
fn a_port_forward_over_ipv6() {
    let mut s = boot6();
    for host in ["[::1]:0", "127.0.0.1:0"] {
        let a = s.net.forward6(host.parse().unwrap(), 23).unwrap();
        let mut c = TcpStream::connect(a).unwrap();
        c.set_nonblocking(true).unwrap();
        let mut got = Vec::new();
        let mut n = 0;
        while !String::from_utf8_lossy(&got).contains("login: ") {
            let mut b = [0u8; 512];
            if let Ok(k) = c.read(&mut b) {
                got.extend_from_slice(&b[..k]);
            }
            s.step();
            n += 1;
            assert!(n < 20_000, "no login prompt from {host}: {got:?}");
        }
        drop(c);
        s.idle(10_000_000);
    }
    let log = s.log("telnetd");
    assert_eq!(log.matches("telnetd: connection from fd00::2\n").count(), 2, "{log}");
    assert_eq!(s.net.open_forwards(), 0);
}

// ---- V4: names and brackets (docs/IPV6.md §6) -------------------------------------------

/// DNS's AAAA answers through `host` (over an IPv4 and an IPv6 server),
/// names in nc and ping6, and /etc/hosts' ::1.
#[test]
fn names_of_either_family() {
    let mut s = boot6();
    s.net.dns_name6("six.test", "2001:db8::7".parse().unwrap());
    s.net.dns_name("both.test", [203, 0, 113, 7]);
    s.net.dns_name6("both.test", "2001:db8::8".parse().unwrap());
    s.net.dns_name("four.test", [203, 0, 113, 9]);
    let o = s.cmd("host six.test; echo status $?");
    has(&o, &["six.test has IPv6 address 2001:db8::7\nstatus 0"]);
    let o = s.cmd("host both.test");
    has(&o, &["both.test has address 203.0.113.7\nboth.test has IPv6 address 2001:db8::8\n"]);
    let o = s.cmd("host four.test");
    assert_eq!(o, "four.test has address 203.0.113.9\n");
    // the same server over IPv6
    let o = s.cmd("host -s fd00::3 six.test; echo status $?");
    has(&o, &["six.test has IPv6 address 2001:db8::7\nstatus 0"]);
    let o = s.cmd("host -s fd00::99 six.test; echo status $?");
    has(&o, &[";; connection timed out; no servers could be reached\nstatus 1"]);
    let o = s.cmd("host -s nonsense six.test; echo status $?");
    has(&o, &["usage: host", "status 2"]);
    let o = s.cmd("host nosuch.invalid; echo status $?");
    has(&o, &["Host nosuch.invalid not found: 3(NXDOMAIN)\nstatus 1"]);
    // a name with only an AAAA record reaches the host through the NAT
    s.net.alias6("2001:db8::7".parse().unwrap(), "::1".parse().unwrap());
    let mut srv = EchoServer::new();
    let p = srv.port();
    let o = cmd_polling(&mut s, &format!("echo by name | nc six.test {p}; echo status $?"), || srv.poll());
    has(&o, &["the host got: by name\nstatus 0"]);
    // -4 refuses a name that has IPv6 only; -6 refuses one with IPv4 only
    let o = s.cmd(&format!("nc -4 six.test {p}; echo status $?"));
    has(&o, &["nc: six.test: Name or service not known\nstatus 1"]);
    let o = s.cmd(&format!("nc -6 four.test {p}; echo status $?"));
    has(&o, &["nc: four.test: Name or service not known\nstatus 1"]);
    // localhost: IPv4 first, ::1 with -6 (/etc/hosts)
    s.cmd("nc -6 -l 2400 > /tmp/six &");
    s.idle(10_000_000);
    s.cmd("echo over ::1 | nc -6 localhost 2400; wait");
    has(&s.cmd("cat /tmp/six"), &["over ::1\n"]);
    // without -6, localhost is IPv4: only an IPv4 listener hears it
    s.cmd("nc -l 2401 > /tmp/four &");
    s.idle(10_000_000);
    s.cmd("echo over 127.0.0.1 | nc localhost 2401; wait");
    has(&s.cmd("cat /tmp/four"), &["over 127.0.0.1\n"]);
    // a nameserver given as an IPv6 address in /etc/resolv.conf
    s.cmd("cp /etc/resolv.conf /tmp/resolv; echo 'nameserver fd00::3' > /etc/resolv.conf");
    let o = s.cmd("host six.test; ping6 -c 1 six.test | head -n 1");
    has(&o, &["six.test has IPv6 address 2001:db8::7\n", "PING six.test(2001:db8::7) 56 data bytes\n"]);
    s.cmd("echo 'nameserver fd00::99' > /etc/resolv.conf");
    has(&s.cmd("host six.test; echo status $?"), &[";; connection timed out; no servers could be reached\nstatus 1"]);
    s.cmd("cp /tmp/resolv /etc/resolv.conf");
    let o = s.cmd("ping6 -c 1 ip6-localhost");
    has(&o, &["PING ip6-localhost(::1) 56 data bytes", "64 bytes from ::1: icmp_seq=1"]);
    let o = s.cmd("ping6 -c 1 four.test; echo status $?");
    has(&o, &["ping6: four.test: Name or service not known\nstatus 2"]);
}

fn http(req: &str) -> Vec<u8> {
    let path = req.split(' ').nth(1).unwrap_or("/");
    match path {
        "/hello.txt" => "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\n\r\nпривет по IPv6\n".as_bytes().to_vec(),
        "/moved" => b"HTTP/1.1 301 Moved Permanently\r\nLocation: /hello.txt\r\nContent-Length: 0\r\n\r\n".to_vec(),
        _ => b"HTTP/1.0 404 Not Found\r\n\r\nnope\n".to_vec(),
    }
}

/// wget with an IPv6 address in brackets (RFC 3986 §3.2.2), and with a
/// name that has IPv6 only.
#[test]
fn wget_over_ipv6() {
    let mut s = boot6();
    let mut srv = EchoServer::speaking(b"\r\n\r\n", http);
    let p = srv.port();
    let o = cmd_polling(&mut s, &format!("wget -O - http://[fd00::2]:{p}/hello.txt; echo status $?"), || srv.poll());
    has(&o, &["Connecting to fd00::2 (fd00::2)... connected.", "200 OK\nпривет по IPv6\n", "status 0"]);
    assert!(srv.requests[0].contains(&format!("\r\nHost: [fd00::2]:{p}\r\n")), "{}", srv.requests[0]);
    // a redirect to a path keeps the brackets
    let o = cmd_polling(&mut s, &format!("wget -q -O - http://[fd00::2]:{p}/moved; echo status $?"), || srv.poll());
    has(&o, &["привет по IPv6\nstatus 0"]);
    assert!(srv.requests[2].starts_with("GET /hello.txt ") && srv.requests[2].contains(&format!("Host: [fd00::2]:{p}\r\n")), "{}", srv.requests[2]);
    // by name
    s.net.dns_name6("web6.test", "2001:db8::80".parse().unwrap());
    s.net.alias6("2001:db8::80".parse().unwrap(), "::1".parse().unwrap());
    let o = cmd_polling(&mut s, &format!("wget -q -O - http://web6.test:{p}/hello.txt; echo status $?"), || srv.poll());
    has(&o, &["привет по IPv6\nstatus 0"]);
    assert!(srv.requests[3].contains(&format!("Host: web6.test:{p}\r\n")));
    // malformed brackets
    let o = s.cmd("wget http://[fd00::2/x; echo status $?");
    has(&o, &["wget: no ']' after an IPv6 address\n"]);
    let o = s.cmd("wget http://[not-an-address]/; echo status $?");
    has(&o, &["wget: bad IPv6 address in the URL\n"]);
    assert_eq!(srv.requests.len(), 4);
}

/// What is not a host name never reaches DNS (the host's resolver might
/// take a bracketed address as one).
#[test]
fn literals_are_not_names() {
    let mut s = boot6();
    let o = s.cmd("host '[fd00::2]'; host 'a:b'; host 'web/x'; echo status $?");
    has(&o, &["host: '[fd00::2]' is not a legal name\n", "host: 'a:b' is not a legal name\n", "host: 'web/x' is not a legal name\n"]);
    let o = s.cmd("nc '[fd00::2]' 22; echo status $?");
    has(&o, &["nc: [fd00::2]: Name or service not known\nstatus 1"]);
}
