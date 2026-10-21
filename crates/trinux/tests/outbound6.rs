//! IPv6 through the gateway (docs/IPV6.md §5, V3): Trinux reaching servers
//! on the host's ::1 through fd00::2 and the NAT, and the host reaching
//! Trinux through a port forward over IPv6. The servers here are real host
//! sockets in the test.

mod common;
use common::*;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};

/// A host-side line server on [::1]: it reads a line, answers "the host
/// got: LINE" and closes.
struct EchoServer {
    listener: TcpListener,
    open: Vec<(TcpStream, Vec<u8>)>,
    requests: Vec<String>,
}

impl EchoServer {
    fn new() -> EchoServer {
        let listener = TcpListener::bind("[::1]:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        EchoServer { listener, open: Vec::new(), requests: Vec::new() }
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
                    buf.contains(&b'\n')
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => false,
                Err(_) => true,
            };
            if done {
                let req = String::from_utf8_lossy(buf).into_owned();
                c.set_nonblocking(false).unwrap();
                let _ = c.write_all(format!("the host got: {req}").as_bytes());
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
