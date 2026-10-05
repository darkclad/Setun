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
    has(&o, &["httpd: /nosuch: not a directory\nstatus 1", "httpd: bad port\nstatus 2", "usage: httpd [-p port] [-r root] [-1]\nstatus 2"]);
}
