//! /sbin/httpd (docs/NETWORK.md §15), reached from the host through a port
//! forward, with requests written by hand so every byte is known.

mod common;
use common::*;
use std::io::{Read, Write};

/// Send `req` to the forwarded port; the whole answer (the server closes).
fn http(s: &mut Session, port: u16, req: &[u8]) -> Vec<u8> {
    let req = req.to_vec();
    let t = std::thread::spawn(move || {
        let mut c = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.set_read_timeout(Some(std::time::Duration::from_secs(120))).unwrap();
        c.write_all(&req).unwrap();
        if req.is_empty() {
            let _ = c.shutdown(std::net::Shutdown::Write); // says nothing and leaves
        }
        let mut out = Vec::new();
        let _ = c.read_to_end(&mut out);
        out
    });
    let start = std::time::Instant::now();
    while !t.is_finished() {
        assert!(start.elapsed().as_secs() < 150, "no answer; the console ends:\n{}", &s.out[s.out.len().saturating_sub(1500)..]);
        s.step();
    }
    t.join().unwrap()
}

fn get(s: &mut Session, port: u16, path: &str) -> String {
    String::from_utf8_lossy(&http(s, port, format!("GET {path} HTTP/1.1\r\nHost: trinux\r\n\r\n").as_bytes())).into_owned()
}

fn status(a: &str) -> &str {
    a.split("\r\n").next().unwrap_or("")
}

fn header<'a>(a: &'a str, name: &str) -> Option<&'a str> {
    let head = a.split("\r\n\r\n").next()?;
    head.split("\r\n").skip(1).find_map(|l| {
        let (n, v) = l.split_once(':')?;
        n.eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

fn body(a: &str) -> &str {
    a.split_once("\r\n\r\n").map(|x| x.1).unwrap_or("")
}

fn forward(s: &mut Session) -> u16 {
    s.net.forward("127.0.0.1:0".parse().unwrap(), 80).unwrap().port()
}

fn wait_httpd(s: &mut Session) {
    let mut tries = 0;
    while !s.cmd("cat /var/log/httpd.log").contains("httpd: serving /var/www on port 80") {
        tries += 1;
        assert!(tries < 100, "httpd did not start:\n{}", s.out);
        s.idle(10_000_000);
    }
}

#[test]
fn httpd_serves_pages_files_and_directories() {
    let mut s = Session::boot();
    wait_httpd(&mut s);
    let p = forward(&mut s);
    let a = get(&mut s, p, "/");
    assert_eq!(status(&a), "HTTP/1.1 200 OK", "{a}");
    assert_eq!(header(&a, "Content-Type"), Some("text/html; charset=utf-8"));
    assert_eq!(header(&a, "Connection"), Some("close"));
    assert!(header(&a, "Date").is_some_and(|d| d.ends_with(" GMT")), "{a}");
    let b = body(&a);
    assert!(b.contains("Привет из троичного мира!") && b.contains("Trinux on Setun-11\u{305}"), "{b}");
    assert_eq!(header(&a, "Content-Length").unwrap().parse::<usize>().unwrap(), b.len());
    // HEAD: the same headers, no body
    let h = String::from_utf8(http(&mut s, p, b"HEAD /index.html HTTP/1.1\r\nHost: x\r\n\r\n")).unwrap();
    assert_eq!(header(&h, "Content-Length"), header(&a, "Content-Length"));
    assert_eq!(body(&h), "");
    // a directory: redirected to its slash, then listed in order; a file in
    // it with a Russian name, asked for percent-encoded
    s.cmd("mkdir -p /var/www/docs/sub; echo 'файл с текстом' > /var/www/docs/файл.txt; echo plain > /var/www/docs/b.txt; echo '<x>' > '/var/www/docs/a&b.txt'");
    let r = get(&mut s, p, "/docs?x=1");
    assert_eq!(status(&r), "HTTP/1.1 301 Moved Permanently");
    assert_eq!(header(&r, "Location"), Some("/docs/"));
    let l = get(&mut s, p, "/docs/");
    assert_eq!(status(&l), "HTTP/1.1 200 OK");
    has(body(&l), &["<title>Index of /docs/</title>", "<li><a href=\"a&amp;b.txt\">a&amp;b.txt</a>\n<li><a href=\"b.txt\">b.txt</a>\n<li><a href=\"sub/\">sub/</a>\n<li><a href=\"файл.txt\">файл.txt</a>\n</ul>"]);
    let f = get(&mut s, p, "/docs/%D1%84%D0%B0%D0%B9%D0%BB.txt");
    assert_eq!(status(&f), "HTTP/1.1 200 OK", "{f}");
    assert_eq!(header(&f, "Content-Type"), Some("text/plain; charset=utf-8"));
    assert_eq!(body(&f), "файл с текстом\n");
    // ASCII only: still text (by its name); . and harmless .. are folded
    let g = get(&mut s, p, "/docs/./sub/../b.txt");
    assert_eq!((status(&g), body(&g)), ("HTTP/1.1 200 OK", "plain\n"));
    // the absolute form, and HTTP/1.0 without a Host
    let g = String::from_utf8(http(&mut s, p, b"GET http://trinux/docs/b.txt HTTP/1.1\r\nHost: trinux\r\n\r\n")).unwrap();
    assert_eq!(body(&g), "plain\n");
    let g = String::from_utf8(http(&mut s, p, b"GET /docs/b.txt HTTP/1.0\r\n\r\n")).unwrap();
    assert_eq!((status(&g), body(&g)), ("HTTP/1.1 200 OK", "plain\n"));
    // the log, in the Common Log Format
    let log = s.log("httpd");
    has(&log, &["10.0.2.2 - - [", "] \"GET / HTTP/1.1\" 200 ", "\"HEAD /index.html HTTP/1.1\" 200 -\n", "\"GET /docs?x=1 HTTP/1.1\" 301 "]);
}

#[test]
fn httpd_refuses_what_it_should() {
    let mut s = Session::boot();
    wait_httpd(&mut s);
    let p = forward(&mut s);
    for (path, want) in [
        ("/nosuch", "404 Not Found"),
        ("/../etc/passwd", "403 Forbidden"),
        ("/%2e%2e/etc/passwd", "403 Forbidden"),
        ("/docs/../../etc/shadow", "403 Forbidden"),
        ("/a%zz", "400 Bad Request"),
        ("/a%00b", "400 Bad Request"),
        ("/%C0%AF", "400 Bad Request"),            // an overlong slash
        ("/%F0%9F%98%80", "400 Bad Request"),      // a character beyond a tryte
        ("/%D1", "400 Bad Request"),               // cut short
        ("relative", "400 Bad Request"),
    ] {
        let a = get(&mut s, p, path);
        assert_eq!(status(&a), format!("HTTP/1.1 {want}"), "{path}: {a}");
    }
    let r = |s: &mut Session, req: &[u8]| String::from_utf8_lossy(&http(s, p, req)).into_owned();
    let a = r(&mut s, b"POST / HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\n\r\n");
    assert_eq!(status(&a), "HTTP/1.1 405 Method Not Allowed");
    assert_eq!(header(&a, "Allow"), Some("GET, HEAD"));
    assert_eq!(status(&r(&mut s, b"GET / HTTP/1.1\r\n\r\n")), "HTTP/1.1 400 Bad Request"); // no Host
    assert_eq!(status(&r(&mut s, b"GET / HTTP/2.0\r\nHost: x\r\n\r\n")), "HTTP/1.1 505 HTTP Version Not Supported");
    assert_eq!(status(&r(&mut s, b"GARBAGE\r\n\r\n")), "HTTP/1.1 400 Bad Request");
    assert_eq!(status(&r(&mut s, b"GET /  HTTP/1.1\r\nHost: x\r\n\r\n")), "HTTP/1.1 400 Bad Request");
    let long = format!("GET /{} HTTP/1.1\r\nHost: x\r\n\r\n", "a".repeat(2100));
    assert_eq!(status(&r(&mut s, long.as_bytes())), "HTTP/1.1 414 URI Too Long");
    let big = format!("GET / HTTP/1.1\r\nHost: x\r\nX-Big: {}\r\n\r\n", "b".repeat(7000));
    assert_eq!(status(&r(&mut s, big.as_bytes())), "HTTP/1.1 431 Request Header Fields Too Large");
    // a client that says nothing and leaves: no answer, no harm
    assert_eq!(http(&mut s, p, b""), b"");
    // a client that starts and stops: a 408 after 27 s (emulated)
    let slow = http(&mut s, p, b"GET / HT");
    assert_eq!(status(&String::from_utf8_lossy(&slow)), "HTTP/1.1 408 Request Timeout");
    // and the server still serves
    assert_eq!(status(&get(&mut s, p, "/")), "HTTP/1.1 200 OK");
    // bad options
    let o = s.cmd("httpd -r /nosuch -p 81; echo status $?; httpd -p 0; echo status $?; httpd -x; echo status $?");
    has(&o, &["httpd: /nosuch: not a directory\nstatus 1", "httpd: bad port\nstatus 2", "usage: httpd [-s [-c cert] [-k key]] [-p port] [-r root] [-t secs] [-1]\nstatus 2"]);
}

// ---- HTTPS: httpd -s against OpenSSL's s_client, and against our own wget ------------------

use std::process::{Command, Stdio};

fn have_openssl() -> bool {
    Command::new("openssl").arg("version").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok()
}

/// Start `httpd -s` (making its key and certificate the first time), forward
/// a host port to 443, and fetch the certificate to a host file. No request
/// deadline: an idle guest runs far ahead of the host's real time.
fn https_server(s: &mut Session) -> (u16, std::path::PathBuf) {
    s.cmd("httpd -s -t 0 2> /tmp/https.log &");
    let mut tries = 0;
    while !s.cmd("cat /tmp/https.log").contains("httpd: serving HTTPS from /var/www on port 443") {
        tries += 1;
        assert!(tries < 300, "httpd -s did not start:\n{}", s.cmd("cat /tmp/https.log"));
        s.idle(10_000_000);
    }
    let port = s.net.forward("127.0.0.1:0".parse().unwrap(), 443).unwrap().port();
    let dir = std::env::temp_dir().join(format!("setun-https-{}-{port}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let cert = s.cmd_max("cat /etc/ssl/httpd.crt", 100_000_000).replace("\r\n", "\n");
    assert!(cert.starts_with("-----BEGIN CERTIFICATE-----"), "{cert}");
    std::fs::write(dir.join("ca.pem"), cert).unwrap();
    (port, dir)
}

/// `openssl s_client` sending `req`: (exit code, everything it printed).
fn s_client(s: &mut Session, port: u16, dir: &std::path::Path, extra: &[&str], req: &str) -> (i32, String) {
    let mut c = Command::new("openssl");
    c.current_dir(dir).args(["s_client", "-connect", &format!("127.0.0.1:{port}"), "-servername", "localhost", "-CAfile", "ca.pem"]);
    c.args(["-verify_return_error", "-verify_hostname", "localhost", "-ign_eof", "-tls1_3"]);
    c.args(extra);
    let (code, out, err) = run_host(s, &mut c, Some(req.as_bytes().to_vec()));
    (code, out + &err)
}

const GET: &str = "GET / HTTP/1.1\r\nHost: localhost\r\n\r\n";

#[test]
fn httpd_serves_https_to_openssl() {
    if !have_openssl() {
        return;
    }
    let mut s = Session::boot();
    let (p, dir) = https_server(&mut s);
    has(&s.cmd("cat /tmp/https.log"), &["httpd: made a P-256 key and a self-signed certificate for setun, localhost, 127.0.0.1 and 10.0.2.15 in /etc/ssl/httpd.crt\n"]);
    for (extra, cipher, group) in [
        (&[][..], "TLS_CHACHA20_POLY1305_SHA256", "X25519,"),
        (&["-ciphersuites", "TLS_AES_128_GCM_SHA256"][..], "TLS_AES_128_GCM_SHA256", "X25519,"),
        (&["-ciphersuites", "TLS_AES_256_GCM_SHA384", "-groups", "P-256"][..], "TLS_AES_256_GCM_SHA384", "ECDH, prime256v1"),
        (&["-groups", "X448:P-384"][..], "TLS_CHACHA20_POLY1305_SHA256", "ECDH, secp384r1"), // a HelloRetryRequest
    ] {
        let (code, out) = s_client(&mut s, p, &dir, extra, GET);
        assert_eq!(code, 0, "{extra:?}: {out}");
        has(&out, &[
            "Verify return code: 0 (ok)",
            &format!("New, TLSv1.3, Cipher is {cipher}"),
            "HTTP/1.1 200 OK\r\n",
            "Привет из троичного мира!",
        ]);
        assert!(out.contains(&format!("Peer Temp Key: {group}")), "{extra:?}: group {group} in\n{out}");
    }
    // the log names each request
    has(&s.cmd("cat /tmp/https.log"), &["10.0.2.2 - - [", "\"GET / HTTP/1.1\" 200 "]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn https_between_trinux_programs_and_refusals() {
    if !have_openssl() {
        return;
    }
    let mut s = Session::boot();
    let (p, dir) = https_server(&mut s);
    // our wget and our httpd, over loopback, the certificate checked
    let o = s.cmd_max("wget --ca-certificate=/etc/ssl/httpd.crt -O - https://localhost/; echo status $?", 3_000_000_000);
    has(&o, &["TLS 1.3, TLS_CHACHA20_POLY1305_SHA256, x25519, ecdsa_secp256r1_sha256\n", "Привет из троичного мира!", "status 0"]);
    let o = s.cmd_max("wget --ca-certificate=/etc/ssl/httpd.crt -q -O - https://127.0.0.1/nosuch; echo status $?", 3_000_000_000);
    has(&o, &["wget: HTTP/1.1 404 Not Found\nstatus 8"]);
    // what the server must refuse
    for (extra, want) in [
        (&["-groups", "X448"][..], "alert handshake failure"),
        (&["-sigalgs", "rsa_pss_rsae_sha256"][..], "alert handshake failure"),
        (&["-ciphersuites", "TLS_AES_128_CCM_SHA256"][..], "alert handshake failure"),
    ] {
        let (code, out) = s_client(&mut s, p, &dir, extra, GET);
        assert_ne!(code, 0, "{extra:?}: {out}");
        has(&out, &[want]);
    }
    let mut c = Command::new("openssl");
    c.current_dir(&dir).args(["s_client", "-connect", &format!("127.0.0.1:{p}"), "-tls1_2"]);
    let (code, out, err) = run_host(&mut s, &mut c, Some(GET.as_bytes().to_vec()));
    assert_ne!(code, 0);
    has(&(out + &err), &["alert protocol version"]);
    // plain HTTP to the HTTPS port; a client that never speaks (to a
    // server with the usual deadline)
    s.cmd("httpd -s -p 4443 2> /tmp/https2.log &");
    let mut tries = 0;
    while !s.cmd("cat /tmp/https2.log").contains("on port 4443") {
        tries += 1;
        assert!(tries < 100);
        s.idle(10_000_000);
    }
    let p2 = s.net.forward("127.0.0.1:0".parse().unwrap(), 4443).unwrap().port();
    let t = std::thread::spawn(move || {
        use std::io::{Read, Write};
        let mut c = std::net::TcpStream::connect(("127.0.0.1", p)).unwrap();
        c.write_all(GET.as_bytes()).unwrap();
        let mut v = Vec::new();
        let _ = c.read_to_end(&mut v);
        let mut quiet = std::net::TcpStream::connect(("127.0.0.1", p2)).unwrap();
        let mut w = Vec::new();
        let _ = quiet.read_to_end(&mut w); // the server gives up after 27 s
        (v, w)
    });
    while !t.is_finished() {
        s.step();
    }
    let (plain, quiet) = t.join().unwrap();
    assert!(plain.starts_with(&[21, 3, 3, 0, 2, 2]), "an alert record: {plain:?}"); // fatal
    assert!(quiet.is_empty());
    has(&s.cmd("cat /tmp/https.log"), &[" TLS: not a TLS client\n", " TLS: the client does not speak TLS 1.3\n"]);
    has(&s.cmd("cat /tmp/https2.log"), &[" TLS: the peer was too slow\n"]);
    // a key and a certificate that do not belong together; files that are not there
    s.cmd_max("certgen /tmp/other.key /tmp/other.crt other", 3_000_000_000);
    let o = s.cmd("httpd -s -p 4443 -k /tmp/other.key -c /etc/ssl/httpd.crt; echo status $?; httpd -s -p 4443 -k /tmp/nokey -c /tmp/other.crt; echo status $?; httpd -k /tmp/other.key; echo status $?");
    has(&o, &[
        "/etc/ssl/httpd.crt: the certificate is not for the key in /tmp/other.key\nstatus 1",
        "/tmp/nokey: not a P-256 or P-384 private key\nstatus 1",
        "httpd: -c and -k go with -s\nstatus 2",
    ]);
    let _ = std::fs::remove_dir_all(&dir);
}
