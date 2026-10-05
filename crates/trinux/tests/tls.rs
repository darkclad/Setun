//! TLS (docs/NETWORK.md §13): certificates checked by lib/x509.tri, here
//! through /bin/x509, against chains made by Python's `cryptography`
//! (tests/fixtures/tls, see its README).

mod common;
use common::*;

const ROOTS: &str = include_str!("fixtures/tls/roots.pem");

const CHAINS: &[(&str, &str)] = &[
    ("web", include_str!("fixtures/tls/web.pem")),
    ("rsa", include_str!("fixtures/tls/rsa.pem")),
    ("ed", include_str!("fixtures/tls/ed.pem")),
    ("p384", include_str!("fixtures/tls/p384.pem")),
    ("missing", include_str!("fixtures/tls/missing.pem")),
    ("expired", include_str!("fixtures/tls/expired.pem")),
    ("notyet", include_str!("fixtures/tls/notyet.pem")),
    ("self", include_str!("fixtures/tls/self.pem")),
    ("noca", include_str!("fixtures/tls/noca.pem")),
    ("deep", include_str!("fixtures/tls/deep.pem")),
    ("badsig", include_str!("fixtures/tls/badsig.pem")),
    ("crit", include_str!("fixtures/tls/crit.pem")),
    ("client", include_str!("fixtures/tls/client.pem")),
    ("impostor", include_str!("fixtures/tls/impostor.pem")),
];

const SLOW: u64 = 3_000_000_000; // a few signatures checked in emulated time

fn with_fixtures() -> Session {
    let mut s = Session::boot();
    s.cmd("mkdir -p /tmp/tls");
    s.put_file("/tmp/tls/roots.pem", ROOTS);
    for (name, pem) in CHAINS {
        s.put_file(&format!("/tmp/tls/{name}.pem"), pem);
    }
    s
}

fn x509(s: &mut Session, args: &str) -> String {
    s.cmd_max(&format!("x509 -C /tmp/tls/roots.pem {args}; echo status $?"), SLOW)
}

#[test]
fn certificate_chains_that_hold() {
    let mut s = with_fixtures();
    let o = x509(&mut s, "-n web.test /tmp/tls/web.pem");
    assert_eq!(
        o,
        "0: web.test (issued by Setun Test CA 1)\n1: Setun Test CA 1 (issued by Setun Test Root RSA)\nverify: ok\nstatus 0\n"
    );
    // the other names it holds: one wildcard label, an address, an absolute name
    for host in ["a.web.test", "WWW.Web.Test", "203.0.113.8", "web.test."] {
        has(&x509(&mut s, &format!("-n {host} /tmp/tls/web.pem")), &["verify: ok\nstatus 0"]);
    }
    // an RSA, an Ed25519 and a P-384 server key under the P-384 root, and the RSA one
    has(&x509(&mut s, "-n rsa.test /tmp/tls/rsa.pem"), &["0: rsa.test (issued by Setun Test Root EC)\nverify: ok\nstatus 0"]);
    has(&x509(&mut s, "-n ed.test /tmp/tls/ed.pem"), &["verify: ok\nstatus 0"]);
    has(&x509(&mut s, "-n p384.test /tmp/tls/p384.pem"), &["verify: ok\nstatus 0"]);
    // without -n the name is not checked; -t moves the clock (2019-06-01 for the expired one)
    has(&x509(&mut s, "/tmp/tls/web.pem"), &["verify: ok\nstatus 0"]);
    has(&x509(&mut s, "-t 1559347200 /tmp/tls/expired.pem"), &["verify: ok\nstatus 0"]);
}

#[test]
fn certificate_chains_that_do_not() {
    let mut s = with_fixtures();
    for host in ["a.b.web.test", "other.test", "203.0.113.9", "test", "xweb.test"] {
        has(&x509(&mut s, &format!("-n {host} /tmp/tls/web.pem")), &["verify: certificate does not match the host name\nstatus 1"]);
    }
    for (file, why) in [
        ("missing", "unable to get local issuer certificate"),
        ("expired", "certificate has expired"),
        ("notyet", "certificate is not yet valid"),
        ("self", "self-signed certificate"),
        ("noca", "invalid CA certificate"),
        ("deep", "invalid CA certificate"), // CA 1 allows no CA below it
        ("badsig", "certificate signature failure"),
        ("crit", "unhandled critical extension"),
        ("client", "unsupported certificate purpose"),
        ("impostor", "certificate signature failure"), // the root's name, another key
    ] {
        has(&x509(&mut s, &format!("-n web.test /tmp/tls/{file}.pem")), &[&format!("verify: {why}\nstatus 1")]);
    }
    // a good chain at a time before it began
    has(&x509(&mut s, "-t 0 /tmp/tls/web.pem"), &["verify: certificate is not yet valid\nstatus 1"]);
    // the system store does not know the test roots
    has(&s.cmd_max("x509 /tmp/tls/web.pem; echo status $?", SLOW), &["verify: unable to get local issuer certificate\nstatus 1"]);
}

#[test]
fn certificate_files_that_are_broken() {
    let mut s = with_fixtures();
    let web = CHAINS[0].1;
    let first_end = web.find("-----END CERTIFICATE-----").unwrap();
    // a certificate cut short, base64 that is not, no END line, nothing at all
    let cut: String = {
        let lines: Vec<&str> = web[..first_end].lines().collect();
        let mut v: Vec<&str> = lines[..lines.len() - 3].to_vec();
        v.push("-----END CERTIFICATE-----");
        v.join("\n") + "\n"
    };
    s.put_file("/tmp/tls/cut.pem", &cut);
    s.put_file("/tmp/tls/junk.pem", "-----BEGIN CERTIFICATE-----\nnot*base64!\n-----END CERTIFICATE-----\n");
    s.put_file("/tmp/tls/open.pem", &web[..first_end]);
    s.put_file("/tmp/tls/empty.pem", "just text\n");
    for (file, why) in [
        ("cut", "malformed certificate"),
        ("junk", "malformed certificate"),
        ("open", "malformed certificate"),
        ("empty", "no certificates"),
        ("nosuch", "cannot read the certificate file"),
    ] {
        has(&x509(&mut s, &format!("/tmp/tls/{file}.pem")), &[&format!("x509: /tmp/tls/{file}.pem: {why}\nstatus 2")]);
    }
    // a broken file of roots: the good ones still count
    s.cmd(&format!("cat /tmp/tls/junk.pem /tmp/tls/roots.pem /tmp/tls/cut.pem > /tmp/tls/mixed.pem"));
    has(&s.cmd("x509 -l -C /tmp/tls/mixed.pem"), &["Setun Test Root RSA\nSetun Test Root EC\n"]);
    has(&s.cmd("x509 -C /tmp/nosuch -l; echo status $?"), &["x509: cannot read /tmp/nosuch\nstatus 2"]);
    has(&s.cmd("x509; echo status $?; x509 -l /tmp/tls/web.pem; echo status $?"), &["usage: x509", "status 2\nusage: x509", "status 2"]);
}

#[test]
fn the_system_trust_store() {
    let mut s = Session::boot();
    let o = s.cmd_max("x509 -l", SLOW);
    assert_eq!(o.lines().count(), 27, "{o}");
    has(&o, &["ISRG Root X1\nISRG Root X2\nGTS Root R1\n", "Amazon Root CA 1\n", "Entrust Root Certification Authority - G2\n"]);
}

// ---- wget over TLS, against OpenSSL's s_server on the host -------------------------------------

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

const KEYS: &[(&str, &str)] = &[
    ("web", include_str!("fixtures/tls/web.key")),
    ("rsa", include_str!("fixtures/tls/rsa.key")),
    ("ed", include_str!("fixtures/tls/ed.key")),
    ("p384", include_str!("fixtures/tls/p384.key")),
    ("expired", include_str!("fixtures/tls/expired.key")),
];

/// `openssl s_server -WWW` serving a scratch directory with one of the
/// fixture chains, killed when dropped.
struct TlsServer {
    child: Child,
    port: u16,
    dir: PathBuf,
}

impl Drop for TlsServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn have_openssl() -> bool {
    let ok = Command::new("openssl").arg("version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok();
    if !ok {
        eprintln!("no openssl on this host: skipped");
    }
    ok
}

/// Serve with chain `name` (the leaf, the rest sent along); `opts` replace
/// the default TLS 1.3 / ChaCha20 settings when given.
fn tls_server(name: &str, opts: &[&str]) -> TlsServer {
    let pem = CHAINS.iter().find(|c| c.0 == name).unwrap().1;
    let key = KEYS.iter().find(|c| c.0 == name).unwrap().1;
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("setun-tls-{}-{name}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let end = "-----END CERTIFICATE-----\n";
    let cut = pem.find(end).unwrap() + end.len();
    std::fs::write(dir.join("cert.pem"), &pem[..cut]).unwrap();
    std::fs::write(dir.join("chain.pem"), &pem[cut..]).unwrap();
    std::fs::write(dir.join("key.pem"), key).unwrap();
    std::fs::write(dir.join("hello.txt"), "привет по TLS\n").unwrap();
    let big: String = (1..=20000).map(|i| format!("{i}\n")).collect();
    std::fs::write(dir.join("big"), big).unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut c = Command::new("openssl");
    c.current_dir(&dir).args(["s_server", "-quiet", "-WWW", "-cert", "cert.pem", "-key", "key.pem"]);
    c.args(["-accept", &format!("127.0.0.1:{port}")]);
    if pem.len() > cut {
        c.args(["-cert_chain", "chain.pem"]);
    }
    if opts.is_empty() {
        c.args(["-tls1_3", "-ciphersuites", "TLS_CHACHA20_POLY1305_SHA256"]);
    } else {
        c.args(opts);
    }
    let child = c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let srv = TlsServer { child, port, dir };
    // listening yet?
    for _ in 0..500 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return srv;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("openssl s_server did not start");
}

/// A Trinux with the test roots, where web.test and the others are the host.
fn tls_session() -> Session {
    let mut s = Session::boot();
    s.cmd("mkdir -p /tmp/tls");
    s.put_file("/tmp/tls/roots.pem", ROOTS);
    s.net.alias([203, 0, 113, 8], "127.0.0.1".parse().unwrap());
    for name in ["web.test", "other.test", "rsa.test", "ed.test", "p384.test"] {
        s.net.dns_name(name, [203, 0, 113, 8]);
    }
    s
}

fn wget(s: &mut Session, args: &str) -> String {
    s.cmd_max(&format!("wget {args}; echo status $?"), SLOW)
}

const CA: &str = "--ca-certificate=/tmp/tls/roots.pem";

#[test]
fn wget_fetches_over_tls() {
    if !have_openssl() {
        return;
    }
    let mut s = tls_session();
    let srv = tls_server("web", &[]);
    let p = srv.port;
    let o = wget(&mut s, &format!("{CA} -O - https://web.test:{p}/hello.txt"));
    has(&o, &[
        "Connecting to web.test (203.0.113.8)... connected.\nTLS 1.3, TLS_CHACHA20_POLY1305_SHA256, ecdsa_secp256r1_sha256\n",
        "HTTP request sent, awaiting response... 200 ok\n",
        "привет по TLS\n",
        "status 0",
    ]);
    // by address (no SNI; the certificate names it too), and something bigger than a record
    has(&wget(&mut s, &format!("{CA} -q -O - https://203.0.113.8:{p}/hello.txt")), &["привет по TLS\nstatus 0"]);
    let o = wget(&mut s, &format!("{CA} -q -O /tmp/big https://web.test:{p}/big; wc -l /tmp/big; tail -n 1 /tmp/big"));
    has(&o, &["20000 /tmp/big\n20000\nstatus 0"]);
}

#[test]
fn wget_tls_with_each_kind_of_server_key() {
    if !have_openssl() {
        return;
    }
    let mut s = tls_session();
    for (name, scheme) in [("rsa", "rsa_pss_rsae_sha"), ("ed", "ed25519"), ("p384", "ecdsa_secp384r1_sha384")] {
        let srv = tls_server(name, &[]);
        let o = wget(&mut s, &format!("{CA} -O - https://{name}.test:{}/hello.txt", srv.port));
        has(&o, &[&format!("TLS_CHACHA20_POLY1305_SHA256, {scheme}"), "привет по TLS\n", "status 0"]);
    }
}

#[test]
fn wget_refuses_what_it_cannot_trust() {
    if !have_openssl() {
        return;
    }
    let mut s = tls_session();
    let srv = tls_server("web", &[]);
    let p = srv.port;
    let o = wget(&mut s, &format!("{CA} https://other.test:{p}/hello.txt"));
    has(&o, &["wget: cannot verify other.test's certificate, issued by 'Setun Test CA 1':\n  certificate does not match the host name\nTo connect to other.test insecurely, use `--no-check-certificate'.\nstatus 5"]);
    // only the system roots, which do not know the test CA
    has(&wget(&mut s, &format!("https://web.test:{p}/hello.txt")), &["  unable to get local issuer certificate\n", "status 5"]);
    // unless told not to check
    let o = wget(&mut s, &format!("--no-check-certificate -O - https://other.test:{p}/hello.txt"));
    has(&o, &["WARNING: the certificate of other.test was not checked.\n", "привет по TLS\n", "status 0"]);
    drop(srv);
    let srv = tls_server("expired", &[]);
    has(&wget(&mut s, &format!("{CA} https://web.test:{}/hello.txt", srv.port)), &["  certificate has expired\n", "status 5"]);
    has(&wget(&mut s, &format!("{CA} --ca-certificate=/tmp/nosuch https://web.test:{}/", srv.port)), &["wget: cannot read /tmp/nosuch\nstatus 1"]);
}

#[test]
fn wget_tls_with_servers_it_cannot_agree_with() {
    if !have_openssl() {
        return;
    }
    let mut s = tls_session();
    // TLS 1.2 only; AES-GCM only
    let srv = tls_server("web", &["-tls1_2"]);
    has(&wget(&mut s, &format!("{CA} https://web.test:{}/", srv.port)), &["wget: TLS: the server sent an alert: protocol version\nstatus 4"]);
    let srv = tls_server("web", &["-tls1_3", "-ciphersuites", "TLS_AES_128_GCM_SHA256"]);
    has(&wget(&mut s, &format!("{CA} https://web.test:{}/", srv.port)), &["wget: TLS: the server sent an alert: handshake failure\nstatus 4"]);
    // a client certificate: asked for (none is sent, fine) and required (refused)
    let srv = tls_server("web", &["-tls1_3", "-ciphersuites", "TLS_CHACHA20_POLY1305_SHA256", "-verify", "1"]);
    has(&wget(&mut s, &format!("{CA} -q -O - https://web.test:{}/hello.txt", srv.port)), &["привет по TLS\nstatus 0"]);
    let srv = tls_server("web", &["-tls1_3", "-ciphersuites", "TLS_CHACHA20_POLY1305_SHA256", "-Verify", "1"]);
    has(&wget(&mut s, &format!("{CA} https://web.test:{}/hello.txt", srv.port)), &["wget: TLS: the server sent an alert: certificate required\nstatus 4"]);
    drop(srv);
    // not TLS at all
    let plain = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let pp = plain.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((mut c, _)) = plain.accept() {
            use std::io::Write;
            let _ = c.write_all(b"HTTP/1.0 400 Bad Request\r\n\r\n");
        }
    });
    has(&wget(&mut s, &format!("{CA} https://web.test:{pp}/")), &["wget: TLS: not a TLS server\nstatus 4"]);
}

#[test]
fn tls_records_changed_on_the_way_are_refused() {
    if !have_openssl() {
        return;
    }
    let mut s = tls_session();
    let srv = tls_server("web", &[]);
    // the last octet of the server's first big segment: inside a protected record
    let mut done = false;
    s.net.set_mangle(move |d, f| {
        if d != setun_net::Dir::ToGuest || done {
            return;
        }
        let Some(r) = setun_net::tcp_payload(f) else { return };
        if r.len() > 300 {
            f[r.end - 1] ^= 0x40;
            setun_net::fix_checksums(f);
            done = true;
        }
    });
    has(&wget(&mut s, &format!("{CA} https://web.test:{}/hello.txt", srv.port)), &["wget: TLS: bad record MAC\nstatus 4"]);
}

/// Real sites, through the host's network (run by hand: `--ignored`).
#[test]
#[ignore]
fn wget_real_https_sites() {
    let mut s = Session::boot();
    // the test machine's clock starts in the past; certificates are newer
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let us = s.m.dev.now_us();
    s.m.dev.rtc.write(0, now, us);
    for site in ["example.com", "www.google.com", "letsencrypt.org", "github.com", "www.cloudflare.com", "en.wikipedia.org", "www.amazon.com"] {
        let o = s.cmd_max(&format!("wget -O /dev/null https://{site}/; echo status $?"), 6_000_000_000);
        println!("---- {site}\n{o}");
    }
}
