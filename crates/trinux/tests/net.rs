//! Networking (docs/NETWORK.md): the card driver, ARP, IPv4, ICMP, UDP, TCP
//! and sockets, against the smoltcp gateway of `setun-net`, on loopback,
//! and against hand-made broken packets.

mod common;
use common::*;

// ---- N2: the card, ARP, IPv4, ICMP ------------------------------------------------------

#[test]
fn eth0_comes_up_and_pings_work() {
    let mut s = Session::boot();
    has(&s.out, &["eth0: TNIC at port 270, IRQ 8, MAC 52:54:00:12:34:56, link up, 10.0.2.15/24 via 10.0.2.2"]);
    let o = s.cmd("ifconfig");
    has(
        &o,
        &[
            "eth0: flags=67<UP,BROADCAST,RUNNING>  mtu 1500\n        inet 10.0.2.15  netmask 255.255.255.0  gateway 10.0.2.2\n",
            "        ether 52:54:00:12:34:56\n",
            "lo: flags=73<UP,LOOPBACK,RUNNING>  mtu 19683\n        inet 127.0.0.1  netmask 255.0.0.0\n        inet6 ::1  prefixlen 128  scopeid 0x10<host>\n",
        ],
    );
    let o = s.cmd("ping -c 3 10.0.2.2");
    has(&o, &["PING 10.0.2.2 (10.0.2.2) 56(84) bytes of data.\n", "64 bytes from 10.0.2.2: icmp_seq=3 ttl=64 time=", "3 packets transmitted, 3 received, 0% packet loss"]);
    let o = s.cmd("ping -c 2 -s 1000 gateway");
    has(&o, &["1008 bytes from 10.0.2.2: icmp_seq=2 ttl=64", "2 packets transmitted, 2 received, 0% packet loss"]);
    let o = s.cmd("ping -c 2 localhost");
    has(&o, &["64 bytes from 127.0.0.1: icmp_seq=1 ttl=64", "2 packets transmitted, 2 received"]);
    let o = s.cmd("ping -c 1 10.0.2.15");
    has(&o, &["64 bytes from 10.0.2.15: icmp_seq=1", "1 received"]);
    let o = s.cmd("cat /proc/net/arp");
    has(&o, &["10.0.2.2         0x1         0x2         52:55:0a:00:02:02     *        eth0\n"]);

    // the gateway pings Trinux
    s.net.ping(5, b"from the gateway");
    s.net.ping(6, b"");
    s.idle(2_000_000);
    let replies = s.net.ping_replies();
    assert_eq!(replies, vec![(5, b"from the gateway".to_vec()), (6, vec![])]);

    let o = s.cmd("cat /proc/net/dev");
    has(&o, &["Inter-|   Receive", "    lo: ", "  eth0: "]);
    let o = s.cmd("cat /proc/net/stat");
    has(&o, &["icmp_echo_replies_sent 5\n", "ip_csum_errors 0\n"]);
}

#[test]
fn ping_failures() {
    let mut s = Session::boot();
    // nobody at .99: ARP never answers, so nothing comes back
    let o = s.cmd_max("ping -c 2 10.0.2.99", 900_000_000);
    has(&o, &["2 packets transmitted, 0 received, 100% packet loss"]);
    let o = s.cmd("sleep 3; cat /proc/net/stat");
    has(&o, &["arp_failed 1\n"]);
    let o = s.cmd("ping nosuchhost; echo status $?");
    has(&o, &["ping: nosuchhost: Name or service not known\nstatus 2"]);
    let o = s.cmd("ping -c 0 10.0.2.2; echo status $?");
    has(&o, &["usage: ping", "status 2"]);
    let o = s.cmd("ifconfig eth0 gw 0.0.0.0; ping -c 1 8.8.8.8; echo status $?");
    has(&o, &["ping: sendto: Network is unreachable\nstatus 2"]);
    let o = s.cmd("ifconfig eth0 down; ping -c 1 10.0.2.2; ifconfig eth0 up; ifconfig eth0 gw 10.0.2.2; ping -c 1 10.0.2.2");
    has(&o, &["ping: sendto: Network is unreachable", "1 packets transmitted, 1 received"]);
}

#[test]
fn ifconfig_settings_and_refusals() {
    let mut s = Session::boot();
    let o = s.cmd("ifconfig eth0 10.0.2.20 netmask 255.255.0.0; ifconfig eth0");
    has(&o, &["inet 10.0.2.20  netmask 255.255.0.0  gateway 10.0.2.2"]);
    let o = s.cmd("ifconfig eth0 netmask 255.0.255.0; ifconfig eth0 300.1.1.1; ifconfig eth0 gw 192.168.1.1; ifconfig wlan0; ifconfig lo 10.1.1.1");
    has(
        &o,
        &[
            "ifconfig: netmask: Invalid argument\n",
            "ifconfig: bad address: 300.1.1.1\n",
            "ifconfig: gw: Network is unreachable\n",
            "ifconfig: wlan0: No such device\n",
            "ifconfig: 10.1.1.1: Invalid argument\n",
        ],
    );
    // users may look but not touch
    let o = s.cmd("su user -c 'ifconfig eth0 10.0.2.30; ifconfig eth0 down'");
    has(&o, &["ifconfig: 10.0.2.30: Operation not permitted\n", "ifconfig: down: Operation not permitted\n"]);
}

#[test]
fn no_link_means_no_eth0_traffic() {
    let mut s = Session::boot_with(false);
    has(&s.out, &["eth0: TNIC at port 270, IRQ 8, MAC 52:54:00:12:34:56, no link"]);
    let o = s.cmd("ping -c 1 10.0.2.2; echo status $?");
    has(&o, &["Network is unreachable", "status 2"]);
    let o = s.cmd("ping -c 1 127.0.0.1");
    has(&o, &["1 received"]);
}

// ---- broken packets from the wire ----------------------------------------------------------

/// An IPv4 packet from the gateway to the guest, in an Ethernet frame.
fn ip_frame(proto: u8, payload: &[u8], tweak: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut f = vec![0x52, 0x54, 0x00, 0x12, 0x34, 0x56, 0x52, 0x55, 0x0a, 0x00, 0x02, 0x02, 0x08, 0x00];
    let total = 20 + payload.len();
    let mut ip = vec![0x45, 0, (total >> 8) as u8, total as u8, 0, 1, 0x40, 0, 64, proto, 0, 0, 10, 0, 2, 2, 10, 0, 2, 15];
    let c = checksum(&ip);
    ip[10] = (c >> 8) as u8;
    ip[11] = c as u8;
    f.extend(ip);
    f.extend_from_slice(payload);
    tweak(&mut f);
    f
}

fn checksum(data: &[u8]) -> u16 {
    let mut s: u32 = 0;
    for c in data.chunks(2) {
        s += (c[0] as u32) << 8 | *c.get(1).unwrap_or(&0) as u32;
    }
    while s > 0xffff {
        s = (s & 0xffff) + (s >> 16);
    }
    !(s as u16)
}

fn icmp_echo(seq: u8) -> Vec<u8> {
    let mut p = vec![8, 0, 0, 0, 0, 7, 0, seq, b'x', b'y'];
    let c = checksum(&p);
    p[2] = (c >> 8) as u8;
    p[3] = c as u8;
    p
}


#[test]
fn broken_packets_are_dropped_and_counted() {
    let mut s = Session::boot();
    s.cmd("ping -c 1 10.0.2.2"); // the gateway's MAC is known now
    let before = stat(&mut s, "icmp_echo_replies_sent");
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("ip_csum_errors", ip_frame(1, &icmp_echo(1), |f| f[24] ^= 0xff)),
        ("ip_hdr_errors", ip_frame(1, &icmp_echo(2), |f| f[14] = 0x65)), // version 6
        ("ip_hdr_errors", ip_frame(1, &icmp_echo(3), |f| {
            f[14] = 0x44; // header length 16
            let c = checksum(&f[14..34].iter().enumerate().map(|(i, &b)| if i == 10 || i == 11 { 0 } else { b }).collect::<Vec<_>>());
            f[24] = (c >> 8) as u8;
            f[25] = c as u8;
        })),
        ("ip_hdr_errors", ip_frame(1, &icmp_echo(4), |f| {
            f[17] = 200; // total length past the frame
            f[24] = 0;
            f[25] = 0;
            let c = checksum(&f[14..34]);
            f[24] = (c >> 8) as u8;
            f[25] = c as u8;
        })),
        ("ip_hdr_errors", ip_frame(1, &icmp_echo(5), |f| f.truncate(30))),
        ("ip_frag_dropped", ip_frame(1, &icmp_echo(6), |f| {
            f[20] = 0x20; // More Fragments
            f[24] = 0;
            f[25] = 0;
            let c = checksum(&f[14..34]);
            f[24] = (c >> 8) as u8;
            f[25] = c as u8;
        })),
        ("ip_not_ours", ip_frame(1, &icmp_echo(7), |f| {
            f[33] = 99; // to 10.0.2.99
            f[24] = 0;
            f[25] = 0;
            let c = checksum(&f[14..34]);
            f[24] = (c >> 8) as u8;
            f[25] = c as u8;
        })),
        ("icmp_errors", ip_frame(1, &icmp_echo(8), |f| *f.last_mut().unwrap() ^= 1)),
        ("udp_errors", ip_frame(17, &[0, 7, 0, 9, 0, 9, 0x12, 0x34, b'!'], |_| {})), // bad checksum
        ("udp_errors", ip_frame(17, &[0, 7, 0, 9, 0, 99, 0, 0, b'!'], |_| {})),       // length past the packet
        ("tcp_errors", ip_frame(6, &[0u8; 19], |_| {})),                              // too short
        ("tcp_errors", ip_frame(6, &[0, 7, 0, 23, 0, 0, 0, 1, 0, 0, 0, 0, 0x50, 2, 0x40, 0, 0xde, 0xad, 0, 0], |_| {})),
        ("ip_unknown_proto", ip_frame(99, b"what is this", |_| {})),
        ("ip_hdr_errors", vec![0x52, 0x54, 0x00, 0x12, 0x34, 0x56, 0x52, 0x55, 0x0a, 0x00, 0x02, 0x02, 0x08, 0x00]),
        ("arp_bad", {
            let mut f = vec![0x52, 0x54, 0x00, 0x12, 0x34, 0x56, 0x52, 0x55, 0x0a, 0x00, 0x02, 0x02, 0x08, 0x06];
            f.extend([0, 1, 0x08, 0x00, 6, 4, 0, 9]); // operation 9
            f.extend([0u8; 20]);
            f
        }),
    ];
    for (counter, frame) in cases {
        let was = stat(&mut s, counter);
        s.m.dev.net_receive(&frame);
        s.idle(200_000);
        let now = stat(&mut s, counter);
        assert_eq!(now, was + 1, "{counter} did not count {frame:02x?}");
    }
    assert_eq!(stat(&mut s, "icmp_echo_replies_sent"), before, "no broken echo was answered");
    // and a good one still is
    s.m.dev.net_receive(&ip_frame(1, &icmp_echo(9), |_| {}));
    s.idle(200_000);
    assert_eq!(stat(&mut s, "icmp_echo_replies_sent"), before + 1);
}

// ---- N3: UDP and sockets --------------------------------------------------------------------

#[test]
fn udp_through_the_gateway_and_on_loopback() {
    let mut s = Session::boot();
    s.udp_echo = Some(s.net.udp_bind(7));
    let o = s.cmd("echo 'тернарный UDP' | nc -u 10.0.2.2 7");
    has(&o, &["тернарный UDP\n"]);
    // a datagram from the gateway to a listening nc
    s.send("nc -u -l 5000 > /tmp/u &\n");
    s.wait_prompt(100_000_000);
    s.idle(5_000_000);
    let g = s.net.udp_bind(40000);
    s.net.udp_send(g, 5000, b"hello from smoltcp\n");
    s.idle(5_000_000);
    // and on loopback, to a second listener
    let o = s.cmd("nc -u -l 6000 > /tmp/v & sleep 1; echo 'loop back' | nc -u 127.0.0.1 6000; sleep 1; cat /tmp/u /tmp/v; kill %1 %2");
    has(&o, &["hello from smoltcp\nloop back\n"]);
    // nobody on the port: the sender hears ICMP port unreachable, the counter says so
    s.net.udp_send(g, 5999, b"anyone?");
    s.idle(2_000_000);
    has(&s.cmd("cat /proc/net/stat"), &["udp_no_port 1\n"]);
}

#[test]
fn socket_calls_and_their_errors() {
    let mut s = Session::boot();
    let o = s.cmd_max("nettest", 2_000_000_000);
    assert!(o.contains("nettest: all passed") && !o.contains("FAIL"), "{o}");
    assert!(o.matches(" ok\n").count() >= 95, "{o}");
}

// ---- N4: TCP against another stack (smoltcp, behind the card) ---------------------------------


#[test]
fn tcp_server_in_trinux_client_on_the_gateway() {
    let mut s = Session::boot();
    // in the background, from files
    s.cmd("echo 'и обратно' > /tmp/reply; nc -l 2323 < /tmp/reply > /tmp/got &");
    s.idle(10_000_000);
    let c = s.net.connect(2323);
    s.run_until("established", 100_000_000, |s| s.net.state(c) == setun_net::State::Established);
    s.net.send(c, "привет from smoltcp\n".as_bytes());
    s.run_until("the reply and the guest's FIN", 200_000_000, |s| s.net.guest_closed(c));
    assert_eq!(String::from_utf8(s.net.recv(c)).unwrap(), "и обратно\n");
    let o = s.cmd("netstat -a");
    has(&o, &["tcp        0      0 10.0.2.15:2323          10.0.2.2:49152          FIN_WAIT2\n"]);
    s.net.close(c);
    let o = s.cmd("wait; jobs; cat /tmp/got");
    has(&o, &["привет from smoltcp\n"]);
    assert!(!o.contains("Running"), "nc has finished: {o}");

    // in the foreground, typing
    s.type_line("nc -l 2324");
    s.idle(10_000_000);
    let c = s.net.connect(2324);
    s.run_until("established", 100_000_000, |s| s.net.state(c) == setun_net::State::Established);
    s.net.send(c, b"line one\n");
    s.until("data", 100_000_000, |o| o.contains("line one\n"));
    s.type_line("typed in Trinux");
    s.run_until("typed line", 100_000_000, |s| s.net.peek(c).ends_with(b"typed in Trinux\n"));
    s.net.close(c);
    s.wait_prompt(200_000_000);
}

#[test]
fn tcp_client_in_trinux_server_on_the_gateway() {
    let mut s = Session::boot();
    let l = s.net.listen(8080);
    s.type_line("seq 1 3000 | nc 10.0.2.2 8080; echo nc status $?");
    let mut conn = None;
    s.run_until("a connection", 300_000_000, |s| {
        conn = conn.or_else(|| s.net.accept(l));
        conn.is_some()
    });
    let c = conn.unwrap();
    s.run_until("all of seq and the FIN", 2_000_000_000, |s| s.net.guest_closed(c));
    let want: String = (1..=3000).map(|i| format!("{i}\n")).collect();
    let got = String::from_utf8(s.net.recv(c)).unwrap();
    assert_eq!(got.len(), want.len());
    assert!(got == want, "the data differ");
    s.net.send(c, b"thanks\n");
    s.net.close(c);
    s.wait_prompt(300_000_000);
    has(&s.out, &["thanks\nnc status 0\n"]);
}

#[test]
fn tcp_refusals_and_resets_with_the_gateway() {
    let mut s = Session::boot();
    // a closed port answers RST
    let c = s.net.connect(9999);
    s.idle(20_000_000);
    assert_eq!(s.net.state(c), setun_net::State::Closed);
    assert!(stat(&mut s, "tcp_rst_sent") >= 1);
    // and the gateway refuses Trinux
    let o = s.cmd("nc 10.0.2.2 81; echo status $?");
    has(&o, &["nc: connect: Connection refused\nstatus 1"]);
    // a reset in the middle of a connection
    let l = s.net.listen(8081);
    s.type_line("nc 10.0.2.2 8081; echo status $?");
    let mut conn = None;
    s.run_until("a connection", 300_000_000, |s| {
        conn = conn.or_else(|| s.net.accept(l));
        conn.is_some()
    });
    let c = conn.unwrap();
    s.net.send(c, b"bye\n");
    s.until("data", 100_000_000, |o| o.contains("bye\n"));
    s.net.abort(c);
    s.wait_prompt(200_000_000);
    has(&s.out, &["nc: read: Connection reset by peer\nstatus 1"]);
}

#[test]
fn tcp_survives_a_lossy_link() {
    let mut s = Session::boot();
    s.cmd("ping -c 1 10.0.2.2");
    // drop every fourth frame each way
    let mut n = [0u32; 2];
    s.net.set_loss(move |d, _| {
        let k = if d == setun_net::Dir::ToGuest { 0 } else { 1 };
        n[k] += 1;
        n[k] % 4 == 0
    });
    let l = s.net.listen(8082);
    s.type_line("seq 1 2000 | nc 10.0.2.2 8082; echo nc status $?");
    let mut conn = None;
    s.run_until("a connection", 600_000_000, |s| {
        conn = conn.or_else(|| s.net.accept(l));
        conn.is_some()
    });
    let c = conn.unwrap();
    s.run_until("all the data", 6_000_000_000, |s| s.net.guest_closed(c));
    let want: String = (1..=2000).map(|i| format!("{i}\n")).collect();
    assert!(String::from_utf8(s.net.recv(c)).unwrap() == want, "data differ under loss");
    s.net.close(c);
    s.wait_prompt(1_000_000_000);
    has(&s.out, &["nc status 0\n"]);
    s.net.clear_loss();
    assert!(stat(&mut s, "tcp_retransmits") > 0);
    assert!(s.net.stats.dropped_from_guest > 0 && s.net.stats.dropped_to_guest > 0);
}

#[test]
fn a_port_forward_from_a_real_host_socket() {
    use std::io::{Read, Write};
    let mut s = Session::boot();
    let addr = s.net.forward("127.0.0.1:0".parse().unwrap(), 2325).expect("listen on the host");
    s.cmd("echo 'from Trinux' > /tmp/r; nc -l 2325 < /tmp/r > /tmp/g &");
    s.idle(10_000_000);
    let mut host = std::net::TcpStream::connect(addr).expect("connect to the forward");
    host.set_nonblocking(true).unwrap();
    host.write_all(b"from the host\n").unwrap();
    let mut got = Vec::new();
    s.run_until("the reply through the host socket", 300_000_000, |_| {
        let mut b = [0u8; 256];
        match host.read(&mut b) {
            Ok(0) => true,
            Ok(n) => {
                got.extend_from_slice(&b[..n]);
                false
            }
            Err(_) => false,
        }
    });
    assert_eq!(got, b"from Trinux\n");
    drop(host);
    let o = s.cmd("sleep 1; cat /tmp/g");
    has(&o, &["from the host\n"]);
}

// ---- N5: logging in over telnet -----------------------------------------------------------------

/// A minimal telnet client on a test connection: the text so far, with
/// Telnet commands removed, and the commands seen.
struct Telnet {
    c: setun_net::ConnId,
    text: String,
    raw: Vec<u8>,
    commands: Vec<[u8; 3]>,
}

impl Telnet {
    fn connect(s: &mut Session, port: u16) -> Telnet {
        let c = s.net.connect(port);
        s.run_until("telnet connection", 100_000_000, |s| s.net.state(c) == setun_net::State::Established);
        Telnet { c, text: String::new(), raw: Vec::new(), commands: Vec::new() }
    }

    fn pull(&mut self, s: &mut Session) {
        self.raw.extend(s.net.recv(self.c));
        let mut keep = Vec::new();
        let mut i = 0;
        while i < self.raw.len() {
            if self.raw[i] == 255 {
                if i + 2 >= self.raw.len() {
                    break; // the rest of the command has not arrived
                }
                self.commands.push([255, self.raw[i + 1], self.raw[i + 2]]);
                i += 3;
            } else {
                keep.push(self.raw[i]);
                i += 1;
            }
        }
        self.raw.drain(..i);
        // whole UTF-8 sequences only
        let cut = match std::str::from_utf8(&keep) {
            Ok(_) => keep.len(),
            Err(e) => e.valid_up_to(),
        };
        self.text.push_str(std::str::from_utf8(&keep[..cut]).unwrap());
        let mut rest = keep[cut..].to_vec();
        rest.extend(self.raw.drain(..));
        self.raw = rest;
    }

    fn wait(&mut self, s: &mut Session, what: &str) {
        let mut n = 0;
        loop {
            self.pull(s);
            if self.text.contains(what) {
                return;
            }
            assert!(n < 400_000_000, "telnet: timed out waiting for {what:?}; got:\n{}\nconsole:\n{}", self.text, s.out);
            s.step();
            n += SLICE;
        }
    }

    fn line(&mut self, s: &mut Session, text: &str) {
        let mut b = text.as_bytes().to_vec();
        b.extend(b"\r\n");
        assert_eq!(s.net.send(self.c, &b), b.len());
    }
}

#[test]
fn telnet_logins() {
    let mut s = Session::boot();
    has(&s.out, &["init: eth0 is up, starting /sbin/telnetd"]);
    let mut t = Telnet::connect(&mut s, 23);
    t.wait(&mut s, "login: ");
    assert!(t.commands.contains(&[255, 251, 1]) && t.commands.contains(&[255, 251, 3]), "WILL ECHO, WILL SGA: {:?}", t.commands);
    // the client's answers are swallowed, not typed
    assert_eq!(s.net.send(t.c, &[255, 253, 1, 255, 253, 3, 255, 250, 31, 0, 80, 0, 24, 255, 240]), 15);
    t.line(&mut s, "root");
    t.wait(&mut s, "Password: ");
    t.line(&mut s, "root");
    t.wait(&mut s, "$ ");
    t.line(&mut s, "echo привет, $USER; id; ls -l /proc/self/fd/0 2>&1 | wc -l");
    t.wait(&mut s, "привет, root\r\nuid=0(root) gid=0(root)");
    t.wait(&mut s, "\r\n$ ");
    // a second session at the same time, as an ordinary user
    let mut u = Telnet::connect(&mut s, 23);
    u.wait(&mut s, "login: ");
    u.line(&mut s, "user");
    u.wait(&mut s, "Password: ");
    u.line(&mut s, "wrong");
    u.wait(&mut s, "Login incorrect");
    u.wait(&mut s, "login: ");
    u.line(&mut s, "user");
    u.wait(&mut s, "Password: ");
    u.line(&mut s, "user");
    u.wait(&mut s, "$ ");
    u.line(&mut s, "whoami; netstat");
    u.wait(&mut s, "user\r\n");
    u.wait(&mut s, "10.0.2.15:23            10.0.2.2:");
    // Ctrl-C reaches the session's foreground job
    t.line(&mut s, "sleep 100");
    s.idle(5_000_000);
    s.net.send(t.c, &[3]);
    t.wait(&mut s, "^C");
    t.line(&mut s, "echo after");
    t.wait(&mut s, "after\r\n$ ");
    // logging out closes the connection
    t.line(&mut s, "exit");
    s.run_until("the server's FIN", 300_000_000, |s| s.net.guest_closed(t.c));
    s.net.close(t.c);
    // the client hanging up ends the other session
    s.net.close(u.c);
    s.idle(50_000_000);
    let o = s.cmd("ps");
    assert!(!o.contains("-sh") || o.matches("-sh").count() == 1, "the telnet shells are gone:\n{o}");
    has(&s.log("telnetd"), &["telnetd: connection from 10.0.2.2"]);
}

// ---- crypto for SSH ----------------------------------------------------------------------------

#[test]
fn crypto_test_vectors() {
    let mut s = Session::boot();
    let o = s.cmd_max("cryptotest", 30_000_000_000);
    println!("{o}");
    assert!(o.contains("cryptotest: all passed") && !o.contains("FAIL"), "{o}");
}

#[test]
fn post_quantum_test_vectors() {
    let mut s = Session::boot();
    let o = s.cmd_max("pqtest", 60_000_000_000);
    println!("{o}");
    assert!(o.contains("pqtest: all passed") && !o.contains("FAIL"), "{o}");
}

#[test]
fn public_key_test_vectors() {
    let mut s = Session::boot();
    let o = s.cmd_max("pktest", 200_000_000_000);
    println!("{o}");
    assert!(o.contains("pktest: all passed") && !o.contains("FAIL"), "{o}");
}

#[test]
fn tcp_bulk_from_the_gateway_into_trinux() {
    let mut s = Session::boot();
    s.cmd("nc -l 2400 < /dev/null > /tmp/in &");
    s.idle(10_000_000);
    let c = s.net.connect(2400);
    s.run_until("established", 100_000_000, |s| s.net.state(c) == setun_net::State::Established);
    let data: String = (1..=20000).map(|i| format!("{i}\n")).collect();
    let bytes = data.as_bytes();
    let mut sent = 0;
    s.run_until("all sent", 3_000_000_000, |s| {
        if sent < bytes.len() {
            sent += s.net.send(c, &bytes[sent..]);
        }
        sent == bytes.len() && s.net.unacked(c) == 0
    });
    s.net.close(c);
    let o = s.cmd_max("wait; seq 1 20000 > /tmp/want; wc -c /tmp/in; cmp /tmp/want /tmp/in && echo SAME || md5sum /tmp/want /tmp/in", 3_000_000_000);
    has(&o, &["108894 /tmp/in"]);
    let sums: Vec<&str> = o.lines().filter(|l| l.contains("/tmp/")).map(|l| l.split(' ').next().unwrap()).collect();
    assert!(o.contains("SAME") || (sums.len() == 3 && sums[1] == sums[2]), "the data differ:\n{o}");
}
