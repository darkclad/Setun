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
