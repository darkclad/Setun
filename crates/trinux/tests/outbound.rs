//! Outbound connections and DNS (docs/NETWORK.md §9): Trinux reaching
//! servers on the host through the gateway's NAT, and names through
//! 10.0.2.3. The servers here are real host sockets in the test.

mod common;
use common::*;
use std::net::TcpListener;

/// A command with the host server polled meanwhile.
fn cmd_srv(s: &mut Session, srv: &mut HostServer, line: &str) -> String {
    s.cmd_with(line, || srv.poll())
}

fn echo_server() -> HostServer {
    HostServer::new("127.0.0.1:0", b"\n", |req| format!("the host got: {req}").into_bytes())
}

fn http_server() -> HostServer {
    HostServer::new("127.0.0.1:0", b"\r\n\r\n", |req| {
        let path = req.split(' ').nth(1).unwrap_or("/");
        match path {
            "/hello.txt" => "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\n\r\nпривет из хоста\n".as_bytes().to_vec(),
            "/big" => {
                let body: String = (1..=5000).map(|i| format!("{i}\n")).collect();
                format!("HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
            }
            // redirects: to a path, relative, absolute (from the Host header), in a loop, without Location
            "/moved" => b"HTTP/1.1 301 Moved Permanently\r\nLocation: /hello.txt\r\nContent-Length: 0\r\n\r\n".to_vec(),
            "/rel/a" => b"HTTP/1.1 302 Found\r\nlocation:   b?x=1\r\n\r\n".to_vec(),
            "/rel/b?x=1" => b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nrel b\n".to_vec(),
            "/abs" => {
                let host = req.lines().find_map(|l| l.strip_prefix("Host: ")).unwrap_or("").to_string();
                format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: http://{host}/moved\r\n\r\n").into_bytes()
            }
            "/loop" => b"HTTP/1.1 302 Found\r\nLocation: /loop\r\n\r\n".to_vec(),
            "/304" => b"HTTP/1.1 304 Not Modified\r\n\r\n".to_vec(),
            // chunked: sizes in hex with an extension, a trailer; then a bad size
            "/chunked" => b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n5;ext=1\r\npedia\r\nE\r\n in\r\n\r\nchunks.\r\n0\r\nX-Trailer: yes\r\n\r\nafter the end\n".to_vec(),
            "/badchunk" => b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\nzz\r\nmore\r\n0\r\n\r\n".to_vec(),
            "/short" => b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nonly this\n".to_vec(),
            "/garbage" => b"SSH-2.0-OpenSSH_10.2\r\n\r\n".to_vec(),
            "/longhead" => {
                let csp: String = "x".repeat(5000);
                format!("HTTP/1.1 200 OK\r\nContent-Security-Policy: {csp}\r\nContent-Length: 3\r\n\r\nok\nignored").into_bytes()
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
    let o = cmd_srv(&mut s, &mut srv, &format!("echo hi from trinux | nc 10.0.2.2 {p}; echo status $?"));
    has(&o, &["the host got: hi from trinux\nstatus 0"]);
    // an address that stands for the host, as an outside server would
    s.net.alias([203, 0, 113, 7], "127.0.0.1".parse().unwrap());
    let o = cmd_srv(&mut s, &mut srv, &format!("echo outside | nc -v 203.0.113.7 {p}"));
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
    let o = cmd_srv(&mut s, &mut srv, &format!("nc 10.0.2.2 {p}; echo status $?"));
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
    let o = cmd_srv(&mut s, &mut srv, &format!("wget -q -O - http://web.test:{p}/hello.txt"));
    assert_eq!(o, "привет из хоста\n");
    assert!(srv.requests[0].starts_with(&format!("GET /hello.txt HTTP/1.1\r\nHost: web.test:{p}\r\n")), "{:?}", srv.requests);
    let o = cmd_srv(&mut s, &mut srv, &format!("cd /tmp; wget http://10.0.2.2:{p}/big; tail -n 1 big; wc -l big"));
    has(&o, &["Saved 23893 octets to 'big'", "\n5000\n", "5000 big"]);
    let o = cmd_srv(&mut s, &mut srv, &format!("wget http://web.test:{p}/missing; echo status $?"));
    has(&o, &["wget: HTTP/1.0 404 Not Found\nstatus 8"]);
    let o = s.cmd("wget ftp://web.test/; echo status $?; wget --bogus http://web.test/; echo status $?; wget http://nosuch.invalid/; echo status $?");
    has(&o, &["wget: only http:// and https:// URLs\nstatus 1\nusage: wget", "status 1\n", "wget: unable to resolve host address 'nosuch.invalid'\nstatus 4"]);
}

#[test]
fn wget_follows_redirects_and_reads_chunks() {
    let mut s = Session::boot();
    let mut srv = http_server();
    let p = srv.port();
    s.net.alias([203, 0, 113, 8], "127.0.0.1".parse().unwrap());
    s.net.dns_name("web.test", [203, 0, 113, 8]);
    let o = cmd_srv(&mut s, &mut srv, &format!("wget -O - http://web.test:{p}/abs; echo status $?"));
    has(&o, &[
        &format!("Location: http://web.test:{p}/moved [following]\n"),
        "Location: /hello.txt [following]\n",
        "HTTP request sent, awaiting response... 200 OK\nпривет из хоста\n",
        "status 0",
    ]);
    // a relative target, with a query; saved under the final name
    let o = cmd_srv(&mut s, &mut srv, &format!("cd /tmp; wget http://web.test:{p}/rel/a; echo status $?; cat b"));
    has(&o, &["Location: b?x=1 [following]", "Saved 6 octets to 'b'\nstatus 0\nrel b\n"]);
    assert!(srv.requests.iter().any(|r| r.starts_with("GET /rel/b?x=1 HTTP/1.1\r\n")), "{:?}", srv.requests);
    // a loop gives up after 20, or as many as asked; none at all with 0
    let o = cmd_srv(&mut s, &mut srv, &format!("wget -q http://web.test:{p}/loop; echo status $?"));
    has(&o, &["wget: 20 redirections exceeded.\nstatus 8"]);
    let o = cmd_srv(&mut s, &mut srv, &format!("wget -q --max-redirect=2 http://web.test:{p}/loop; echo status $?"));
    has(&o, &["wget: 2 redirections exceeded.\nstatus 8"]);
    let o = cmd_srv(&mut s, &mut srv, &format!("wget -q --max-redirect=0 http://web.test:{p}/moved; echo status $?"));
    has(&o, &["wget: 0 redirections exceeded.\nstatus 8"]);
    // a 3xx without a Location is an answer like any other
    let o = cmd_srv(&mut s, &mut srv, &format!("wget -q http://web.test:{p}/304; echo status $?"));
    has(&o, &["wget: HTTP/1.1 304 Not Modified\nstatus 8"]);
    // chunked: extensions and trailers handled, nothing after the last chunk
    let o = cmd_srv(&mut s, &mut srv, &format!("wget -q -O - http://web.test:{p}/chunked; echo; echo status $?"));
    has(&o, &["Wikipedia in\r\n\r\nchunks.\nstatus 0"]);
    let o = cmd_srv(&mut s, &mut srv, &format!("wget -q -O /tmp/x http://web.test:{p}/badchunk; echo status $?"));
    has(&o, &["wget: a malformed chunk\nstatus 7"]);
    // a body cut short; an answer that is not HTTP; a 5 KB header
    let o = cmd_srv(&mut s, &mut srv, &format!("wget -q -O /tmp/x http://web.test:{p}/short; echo status $?"));
    has(&o, &["wget: the connection closed before the end of the body\nstatus 4"]);
    let o = cmd_srv(&mut s, &mut srv, &format!("wget -q -O /tmp/x http://web.test:{p}/garbage; echo status $?"));
    has(&o, &["wget: not an HTTP answer\nstatus 7"]);
    let o = cmd_srv(&mut s, &mut srv, &format!("wget -q -O - http://web.test:{p}/longhead; echo status $?"));
    has(&o, &["ok\nstatus 0"]);
    // -S: the headers as they came
    let o = cmd_srv(&mut s, &mut srv, &format!("wget -S -O /tmp/x http://web.test:{p}/moved"));
    has(&o, &["  HTTP/1.1 301 Moved Permanently\n  Location: /hello.txt\n  Content-Length: 0\n", "  HTTP/1.0 200 OK\n  Content-Type: text/plain\n"]);
}

// ---- UDP through the NAT, and NTP -----------------------------------------------------------

/// Type a command and wait for its prompt, calling `serve` between slices.
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
    let o = s.cmd_with(&format!("echo 'hi over udp' | nc -u 10.0.2.2 {p}"), &mut serve);
    has(&o, &["echo: hi over udp\n"]);
    s.net.alias([203, 0, 113, 9], "127.0.0.1".parse().unwrap());
    let o = s.cmd_with(&format!("echo 'Сетунь' | nc -u 203.0.113.9 {p}"), &mut serve);
    has(&o, &["echo: Сетунь\n"]);
    assert!(s.net.stats.udp_out >= 2 && s.net.stats.udp_in >= 2);
    // the DNS server and test ports stay where they were
    let o = s.cmd("host localhost");
    has(&o, &["localhost has address 127.0.0.1"]);
    // with the NAT off, nothing goes out
    s.net.set_outbound(false);
    let before = s.net.stats.udp_out;
    let o = s.cmd_with(&format!("echo lost | nc -u 10.0.2.2 {p}; echo done"), &mut serve);
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
    let o = s.cmd_with(&format!("ntpdate -q -p {pg} 10.0.2.2; echo status $?; date"), &mut serve);
    has(&o, &["server 10.0.2.2, stratum 2, offset ", " s: 2030-01-01 00:00:00 UTC\nstatus 0\n"]);
    assert!(!o.contains("2030\n") && !o.contains("Jan  1"), "-q leaves the clock alone: {o}");
    let o = s.cmd_with(&format!("su user -c 'ntpdate -p {pg} 10.0.2.2'; echo status $?"), &mut serve);
    has(&o, &["ntpdate: cannot set the clock: Operation not permitted\nstatus 2"]);
    let o = s.cmd_with(&format!("ntpdate -p {pg} 10.0.2.2; echo status $?; date +%Y"), &mut serve);
    has(&o, &["ntpdate: the clock is set\nstatus 0\n2030\n"]);
    let o = s.cmd_with(&format!("ntpdate -q -p {pb} 10.0.2.2; echo status $?"), &mut serve);
    has(&o, &["ntpdate: no server suitable for synchronization found\nstatus 1"]);
    let o = s.cmd_with(&format!("ntpdate -q -p {pk} 10.0.2.2; echo status $?"), &mut serve);
    has(&o, &["ntpdate: the server is not synchronised (kiss-o'-death)\nstatus 1"]);
    let o = s.cmd("ntpdate -q nosuch.invalid; echo status $?; ntpdate; echo status $?");
    has(&o, &["ntpdate: nosuch.invalid: Name or service not known\nstatus 1", "usage: ntpdate [-q] [-p port] server\nstatus 2"]);
}
