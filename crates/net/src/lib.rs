//! `setun-net`: a user-mode network behind the Setun-26 network card
//! (docs/NETWORK.md §2), in the manner of QEMU's "slirp".
//!
//! The network is the guest's gateway at 10.0.2.2 on 10.0.2.0/24, built on
//! smoltcp, an independent TCP/IP stack. It answers ARP and ping, and it
//! makes TCP connections to the guest:
//!
//! - **port forwards** tie a host listener to a guest port and copy the
//!   bytes of each accepted connection both ways;
//! - **test connections** are the same without a host socket, so tests
//!   stay deterministic;
//! - **listeners** on the gateway accept connections the guest makes;
//! - **outbound connections** from the guest reach the outside world through
//!   host sockets (a NAT), and 10.0.2.3 answers DNS (`nat.rs`).
//!
//! Time is the emulator's clock. A loss filter can drop chosen frames.

mod nat;
pub use nat::DNS;

use setun_core::Machine;
use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{self, ChecksumCapabilities, Device, DeviceCapabilities, Medium};
use smoltcp::socket::{icmp, tcp, udp};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr, IpEndpoint, Ipv4Address};
use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};

pub use smoltcp::socket::tcp::State;

/// The gateway's address.
pub const GATEWAY: Ipv4Address = Ipv4Address::new(10, 0, 2, 2);
/// The address the guest is expected to use.
pub const GUEST: Ipv4Address = Ipv4Address::new(10, 0, 2, 15);
/// The gateway's MAC address.
pub const GATEWAY_MAC: [u8; 6] = [0x52, 0x55, 0x0a, 0x00, 0x02, 0x02];

const TCP_BUFFER: usize = 65536;

/// Which way a frame is travelling, for the loss filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    ToGuest,
    FromGuest,
}

/// A TCP connection between the gateway and the guest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnId(usize);

/// A gateway port accepting the guest's connections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListenerId(usize);

/// A UDP port on the gateway.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UdpId(usize);

struct Conn {
    handle: SocketHandle,
    /// The host side of a forwarded connection.
    host: Option<TcpStream>,
    /// Bytes from the guest not yet written to the host socket.
    to_host: Vec<u8>,
    host_eof: bool,
    /// The guest closed its side, so the host socket's sending side was shut.
    host_wr_shut: bool,
    /// Bytes received from the guest (test connections).
    received: Vec<u8>,
    closed: bool,
}

struct Listener {
    port: u16,
    handle: SocketHandle,
    accepted: VecDeque<ConnId>,
}

struct Forward {
    listener: TcpListener,
    guest_port: u16,
}

/// Counters, for tests and the front end's status line.
#[derive(Debug, Default, Clone, Copy)]
pub struct Stats {
    pub frames_to_guest: u64,
    pub frames_from_guest: u64,
    pub dropped_to_guest: u64,
    pub dropped_from_guest: u64,
}

/// The frame queues smoltcp sees as its network device.
#[derive(Default)]
struct Wire {
    /// Frames from the guest, for smoltcp.
    inbound: VecDeque<Vec<u8>>,
    /// Frames from smoltcp, for the guest.
    outbound: VecDeque<Vec<u8>>,
}

impl Device for Wire {
    type RxToken<'a> = RxToken;
    type TxToken<'a> = TxToken<'a>;

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = setun_core::devices::MAX_FRAME;
        caps.medium = Medium::Ethernet;
        caps.checksum = ChecksumCapabilities::default(); // compute and verify every checksum
        caps
    }

    fn receive(&mut self, _now: Instant) -> Option<(RxToken, TxToken<'_>)> {
        let frame = self.inbound.pop_front()?;
        Some((RxToken(frame), TxToken(&mut self.outbound)))
    }

    fn transmit(&mut self, _now: Instant) -> Option<TxToken<'_>> {
        Some(TxToken(&mut self.outbound))
    }
}

struct RxToken(Vec<u8>);

impl phy::RxToken for RxToken {
    fn consume<R, F: FnOnce(&[u8]) -> R>(self, f: F) -> R {
        f(&self.0)
    }
}

struct TxToken<'a>(&'a mut VecDeque<Vec<u8>>);

impl phy::TxToken for TxToken<'_> {
    fn consume<R, F: FnOnce(&mut [u8]) -> R>(self, len: usize, f: F) -> R {
        let mut buf = vec![0; len];
        let r = f(&mut buf);
        self.0.push_back(buf);
        r
    }
}

/// A frame filter: return true to drop the frame.
pub type LossFilter = Box<dyn FnMut(Dir, &[u8]) -> bool + Send>;
/// A frame editor, for tests that need a broken packet the checksums accept.
pub type Mangler = Box<dyn FnMut(Dir, &mut Vec<u8>) + Send>;

pub struct Network {
    iface: Interface,
    wire: Wire,
    sockets: SocketSet<'static>,
    conns: Vec<Conn>,
    listeners: Vec<Listener>,
    udps: Vec<SocketHandle>,
    forwards: Vec<Forward>,
    icmp: SocketHandle,
    next_port: u16,
    loss: Option<LossFilter>,
    mangle: Option<Mangler>,
    nat: nat::Nat,
    dns: SocketHandle,
    outbound: bool,
    pub stats: Stats,
}

fn now(m: &Machine) -> Instant {
    Instant::from_micros(m.dev.now_us())
}

impl Network {
    /// A network attached to `m`'s card (its `LINK` reads +1 from now on).
    pub fn attach(m: &mut Machine) -> Self {
        m.dev.nic.link = true;
        let mut wire = Wire::default();
        let mut config = Config::new(HardwareAddress::Ethernet(EthernetAddress(GATEWAY_MAC)));
        config.random_seed = 0x5e7u64; // reproducible initial sequence numbers
        let mut iface = Interface::new(config, &mut wire, now(m));
        iface.update_ip_addrs(|a| {
            a.push(IpCidr::new(IpAddress::Ipv4(GATEWAY), 24)).unwrap();
            a.push(IpCidr::new(IpAddress::Ipv4(DNS), 24)).unwrap();
        });
        let mut sockets = SocketSet::new(Vec::new());
        let icmp = sockets.add(icmp::Socket::new(
            icmp::PacketBuffer::new(vec![icmp::PacketMetadata::EMPTY; 27], vec![0; 27 * 1514]),
            icmp::PacketBuffer::new(vec![icmp::PacketMetadata::EMPTY; 27], vec![0; 27 * 1514]),
        ));
        sockets.get_mut::<icmp::Socket>(icmp).bind(icmp::Endpoint::Ident(0x5e7)).unwrap();
        let mut d = udp::Socket::new(
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 27], vec![0; 27 * 1514]),
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 27], vec![0; 27 * 1514]),
        );
        d.bind(smoltcp::wire::IpListenEndpoint { addr: Some(IpAddress::Ipv4(DNS)), port: 53 }).unwrap();
        let dns = sockets.add(d);
        Network {
            iface,
            wire,
            sockets,
            conns: Vec::new(),
            listeners: Vec::new(),
            udps: Vec::new(),
            forwards: Vec::new(),
            icmp,
            next_port: 49152,
            loss: None,
            mangle: None,
            nat: nat::Nat::new(),
            dns,
            outbound: true,
            stats: Stats::default(),
        }
    }

    /// Let the guest connect out through host sockets (on by default).
    pub fn set_outbound(&mut self, on: bool) {
        self.outbound = on;
    }

    /// Make guest address `seen` stand for host address `host` (10.0.2.2
    /// stands for 127.0.0.1 from the start).
    pub fn alias(&mut self, seen: [u8; 4], host: IpAddr) {
        self.nat.aliases.insert(Ipv4Address::from(seen), host);
    }

    /// Let DNS answer `name` with `addr` without asking the host.
    pub fn dns_name(&mut self, name: &str, addr: [u8; 4]) {
        self.nat.names.insert(name.to_ascii_lowercase(), Ipv4Addr::from(addr));
    }

    /// How long a host connection may take before the guest is refused.
    pub fn set_connect_timeout(&mut self, d: std::time::Duration) {
        self.nat.connect_timeout = d;
    }

    /// Connections made through the NAT so far.
    pub fn outbound_flows(&self) -> usize {
        self.nat.flows.len()
    }

    /// Host connections or DNS lookups still under way. They take real
    /// time, so a front end that runs the guest faster than real time
    /// should slow down while this is non-zero (or the guest gives up first).
    pub fn waiting_on_host(&self) -> usize {
        self.nat.flows.iter().filter(|f| matches!(f.state, nat::FlowState::Connecting(..))).count() + self.nat.pending_dns.len()
    }

    /// Drop the frames `f` returns true for.
    pub fn set_loss(&mut self, f: impl FnMut(Dir, &[u8]) -> bool + Send + 'static) {
        self.loss = Some(Box::new(f));
    }

    pub fn clear_loss(&mut self) {
        self.loss = None;
    }

    /// Edit frames in flight (see [`tcp_payload`] and [`fix_checksums`]).
    pub fn set_mangle(&mut self, f: impl FnMut(Dir, &mut Vec<u8>) + Send + 'static) {
        self.mangle = Some(Box::new(f));
    }

    pub fn clear_mangle(&mut self) {
        self.mangle = None;
    }

    /// Exchange frames with the card and move data on every connection.
    /// Call it often: after each slice of emulation.
    pub fn poll(&mut self, m: &mut Machine) {
        for mut f in m.dev.net_take_tx() {
            if let Some(g) = self.mangle.as_mut() {
                g(Dir::FromGuest, &mut f);
            }
            if self.loss.as_mut().is_some_and(|l| l(Dir::FromGuest, &f)) {
                self.stats.dropped_from_guest += 1;
                continue;
            }
            self.stats.frames_from_guest += 1;
            if let Some(f) = self.nat_in(f) {
                self.wire.inbound.push_back(f);
            }
        }
        self.nat_connects();
        self.accept_forwards();
        let t = now(m);
        self.iface.poll(t, &mut self.wire, &mut self.sockets);
        self.pump();
        self.dns_poll();
        self.iface.poll(t, &mut self.wire, &mut self.sockets);
        while let Some(mut f) = self.wire.outbound.pop_front() {
            self.nat_out(&mut f);
            if let Some(g) = self.mangle.as_mut() {
                g(Dir::ToGuest, &mut f);
            }
            if self.loss.as_mut().is_some_and(|l| l(Dir::ToGuest, &f)) {
                self.stats.dropped_to_guest += 1;
                continue;
            }
            self.stats.frames_to_guest += 1;
            m.dev.net_receive(&f);
        }
        self.nat_prune(); // after the frames that still needed the flows (a refusal's RST)
    }

    /// How long (µs of emulated time) until smoltcp has a timer to run, if
    /// it has one: a front end may sleep that long when the guest is idle.
    pub fn poll_delay_us(&mut self, m: &Machine) -> Option<i64> {
        self.iface.poll_delay(now(m), &self.sockets).map(|d| d.total_micros() as i64)
    }

    fn ephemeral_port(&mut self) -> u16 {
        let p = self.next_port;
        self.next_port = if p == 65535 { 49152 } else { p + 1 };
        p
    }

    fn tcp_socket() -> tcp::Socket<'static> {
        let mut s = tcp::Socket::new(
            tcp::SocketBuffer::new(vec![0; TCP_BUFFER]),
            tcp::SocketBuffer::new(vec![0; TCP_BUFFER]),
        );
        s.set_nagle_enabled(false);
        s.set_ack_delay(None);
        s
    }

    fn add_conn(&mut self, handle: SocketHandle, host: Option<TcpStream>) -> ConnId {
        self.conns.push(Conn {
            handle,
            host,
            to_host: Vec::new(),
            host_eof: false,
            host_wr_shut: false,
            received: Vec::new(),
            closed: false,
        });
        ConnId(self.conns.len() - 1)
    }

    // ---- test connections: the gateway connects to the guest ----------------------

    /// Start a TCP connection from the gateway to `guest_port` on the guest.
    pub fn connect(&mut self, guest_port: u16) -> ConnId {
        let mut s = Self::tcp_socket();
        let local = self.ephemeral_port();
        s.connect(self.iface.context(), IpEndpoint::new(IpAddress::Ipv4(GUEST), guest_port), local)
            .expect("connect");
        let h = self.sockets.add(s);
        self.add_conn(h, None)
    }

    fn tcp(&mut self, c: ConnId) -> &mut tcp::Socket<'static> {
        let h = self.conns[c.0].handle;
        self.sockets.get_mut::<tcp::Socket>(h)
    }

    pub fn state(&mut self, c: ConnId) -> State {
        self.tcp(c).state()
    }

    /// Queue bytes for the guest; returns how many fit in the send buffer.
    pub fn send(&mut self, c: ConnId, data: &[u8]) -> usize {
        self.tcp(c).send_slice(data).unwrap_or(0)
    }

    /// Bytes from the guest received so far, taken.
    pub fn recv(&mut self, c: ConnId) -> Vec<u8> {
        self.drain(c);
        std::mem::take(&mut self.conns[c.0].received)
    }

    /// Bytes from the guest received so far, left in place.
    pub fn peek(&mut self, c: ConnId) -> &[u8] {
        self.drain(c);
        &self.conns[c.0].received
    }

    /// True once the guest has closed its side (or the connection is gone)
    /// and every byte has been read.
    pub fn guest_closed(&mut self, c: ConnId) -> bool {
        self.drain(c);
        let s = self.tcp(c);
        let after_fin = matches!(s.state(), State::CloseWait | State::LastAck | State::Closing | State::TimeWait | State::Closed);
        after_fin && s.recv_queue() == 0
    }

    /// Bytes queued for the guest that it has not acknowledged yet.
    pub fn unacked(&mut self, c: ConnId) -> usize {
        self.tcp(c).send_queue()
    }

    fn drain(&mut self, c: ConnId) {
        if self.conns[c.0].host.is_some() {
            return;
        }
        let mut buf = [0u8; 4096];
        loop {
            let n = self.tcp(c).recv_slice(&mut buf).unwrap_or(0);
            if n == 0 {
                break;
            }
            self.conns[c.0].received.extend_from_slice(&buf[..n]);
        }
    }

    /// Close the gateway's side (FIN).
    pub fn close(&mut self, c: ConnId) {
        self.tcp(c).close();
    }

    /// Reset the connection (RST).
    pub fn abort(&mut self, c: ConnId) {
        self.tcp(c).abort();
    }

    // ---- listeners: the guest connects to the gateway ----------------------------

    /// Accept connections from the guest to `10.0.2.2:port`.
    pub fn listen(&mut self, port: u16) -> ListenerId {
        let h = self.listen_socket(port);
        self.listeners.push(Listener { port, handle: h, accepted: VecDeque::new() });
        ListenerId(self.listeners.len() - 1)
    }

    fn listen_socket(&mut self, port: u16) -> SocketHandle {
        let mut s = Self::tcp_socket();
        s.listen(port).expect("listen");
        self.sockets.add(s)
    }

    /// The next connection the guest made to this listener that has finished
    /// its handshake, if any.
    pub fn accept(&mut self, l: ListenerId) -> Option<ConnId> {
        let ready = self.listeners[l.0].accepted.iter().position(|&c| {
            let h = self.conns[c.0].handle;
            !matches!(self.sockets.get::<tcp::Socket>(h).state(), State::SynReceived)
        })?;
        self.listeners[l.0].accepted.remove(ready)
    }

    // ---- UDP ---------------------------------------------------------------------

    /// A UDP socket on the gateway's `port`.
    pub fn udp_bind(&mut self, port: u16) -> UdpId {
        let mut s = udp::Socket::new(
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 27], vec![0; 27 * 1514]),
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 27], vec![0; 27 * 1514]),
        );
        s.bind(port).expect("bind");
        self.udps.push(self.sockets.add(s));
        UdpId(self.udps.len() - 1)
    }

    /// Send a datagram to the guest's `port`.
    pub fn udp_send(&mut self, u: UdpId, guest_port: u16, data: &[u8]) {
        let h = self.udps[u.0];
        self.sockets
            .get_mut::<udp::Socket>(h)
            .send_slice(data, IpEndpoint::new(IpAddress::Ipv4(GUEST), guest_port))
            .expect("udp send");
    }

    /// The next datagram received, with the sender's port.
    pub fn udp_recv(&mut self, u: UdpId) -> Option<(u16, Vec<u8>)> {
        let h = self.udps[u.0];
        let s = self.sockets.get_mut::<udp::Socket>(h);
        s.recv().ok().map(|(d, meta)| (meta.endpoint.port, d.to_vec()))
    }

    // ---- ping --------------------------------------------------------------------

    /// Send an ICMP echo request to the guest.
    pub fn ping(&mut self, seq: u16, payload: &[u8]) {
        use smoltcp::wire::{Icmpv4Packet, Icmpv4Repr};
        let repr = Icmpv4Repr::EchoRequest { ident: 0x5e7, seq_no: seq, data: payload };
        let s = self.sockets.get_mut::<icmp::Socket>(self.icmp);
        let buf = s.send(repr.buffer_len(), IpAddress::Ipv4(GUEST)).expect("icmp send");
        let mut pkt = Icmpv4Packet::new_unchecked(buf);
        repr.emit(&mut pkt, &ChecksumCapabilities::default());
    }

    /// Echo replies received from the guest: (sequence number, payload).
    pub fn ping_replies(&mut self) -> Vec<(u16, Vec<u8>)> {
        use smoltcp::wire::{Icmpv4Packet, Icmpv4Repr};
        let mut out = Vec::new();
        let s = self.sockets.get_mut::<icmp::Socket>(self.icmp);
        while let Ok((data, _from)) = s.recv() {
            let Ok(pkt) = Icmpv4Packet::new_checked(data) else { continue };
            if let Ok(Icmpv4Repr::EchoReply { seq_no, data, .. }) = Icmpv4Repr::parse(&pkt, &ChecksumCapabilities::default()) {
                out.push((seq_no, data.to_vec()));
            }
        }
        out
    }

    // ---- port forwards: host sockets ---------------------------------------------

    /// Forward connections accepted on the host's `addr` to `guest_port`.
    /// Returns the bound address (port 0 lets the system choose).
    pub fn forward(&mut self, addr: SocketAddr, guest_port: u16) -> std::io::Result<SocketAddr> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        let bound = listener.local_addr()?;
        self.forwards.push(Forward { listener, guest_port });
        Ok(bound)
    }

    fn accept_forwards(&mut self) {
        let mut new = Vec::new();
        for f in &self.forwards {
            while let Ok((stream, _)) = f.listener.accept() {
                let _ = stream.set_nonblocking(true);
                let _ = stream.set_nodelay(true);
                new.push((stream, f.guest_port));
            }
        }
        for (stream, port) in new {
            let c = self.connect(port);
            self.conns[c.0].host = Some(stream);
        }
    }

    /// Move bytes between forwarded connections and their host sockets, and
    /// hand out connections the guest made to the listeners.
    fn pump(&mut self) {
        for k in 0..self.listeners.len() {
            let h = self.listeners[k].handle;
            let s = self.sockets.get_mut::<tcp::Socket>(h);
            if s.state() != State::Listen && s.state() != State::Closed {
                let port = self.listeners[k].port;
                let id = self.add_conn(h, None);
                self.listeners[k].accepted.push_back(id);
                self.listeners[k].handle = self.listen_socket(port);
            }
        }
        for k in 0..self.conns.len() {
            if self.conns[k].closed || self.conns[k].host.is_none() {
                continue;
            }
            let h = self.conns[k].handle;
            let conn = &mut self.conns[k];
            let s = self.sockets.get_mut::<tcp::Socket>(h);
            let host = conn.host.as_mut().unwrap();
            // host → guest
            if !conn.host_eof && s.may_send() {
                let mut buf = [0u8; 4096];
                while s.send_capacity() - s.send_queue() > 0 {
                    let room = (s.send_capacity() - s.send_queue()).min(buf.len());
                    match host.read(&mut buf[..room]) {
                        Ok(0) => {
                            conn.host_eof = true;
                            s.close();
                            break;
                        }
                        Ok(n) => {
                            s.send_slice(&buf[..n]).ok();
                        }
                        Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                        Err(_) => {
                            conn.host_eof = true;
                            s.abort();
                            break;
                        }
                    }
                }
            }
            // guest → host
            let mut buf = [0u8; 4096];
            while let Ok(n) = s.recv_slice(&mut buf) {
                if n == 0 {
                    break;
                }
                conn.to_host.extend_from_slice(&buf[..n]);
            }
            while !conn.to_host.is_empty() {
                match host.write(&conn.to_host) {
                    Ok(n) if n > 0 => {
                        conn.to_host.drain(..n);
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                    _ => {
                        conn.to_host.clear();
                        s.abort();
                        break;
                    }
                }
            }
            // the guest's FIN half-closes the host socket; the host's EOF closes
            // ours (above); the connection is over when both are done
            let after_fin = matches!(s.state(), State::CloseWait | State::LastAck | State::Closing | State::TimeWait | State::Closed);
            if after_fin && s.recv_queue() == 0 && conn.to_host.is_empty() && !conn.host_wr_shut {
                let _ = host.shutdown(std::net::Shutdown::Write);
                conn.host_wr_shut = true;
            }
            let reset = s.state() == State::Closed && !(conn.host_eof && conn.host_wr_shut);
            let finished = conn.host_eof && conn.host_wr_shut && matches!(s.state(), State::Closed | State::TimeWait);
            if reset || finished {
                let _ = host.shutdown(std::net::Shutdown::Both);
                conn.closed = true;
                conn.host = None;
            }
        }
    }

    /// Forwarded connections still open.
    pub fn open_forwards(&self) -> usize {
        self.conns.iter().filter(|c| c.host.is_some()).count()
    }
}

/// Where the TCP payload of an Ethernet/IPv4/TCP frame lies, if it is one.
pub fn tcp_payload(f: &[u8]) -> Option<std::ops::Range<usize>> {
    if f.len() < 54 || f[12] != 0x08 || f[13] != 0x00 || f[23] != 6 {
        return None;
    }
    let ihl = (f[14] & 15) as usize * 4;
    let total = u16::from_be_bytes([f[16], f[17]]) as usize;
    let t = 14 + ihl;
    let doff = (f[t + 12] >> 4) as usize * 4;
    let start = t + doff;
    let end = 14 + total;
    (start <= end && end <= f.len()).then_some(start..end)
}

/// Recompute the TCP checksum of an Ethernet/IPv4/TCP frame after an edit.
pub fn fix_checksums(f: &mut [u8]) {
    let ihl = (f[14] & 15) as usize * 4;
    let total = u16::from_be_bytes([f[16], f[17]]) as usize;
    let t = 14 + ihl;
    let len = total - ihl;
    f[t + 16] = 0;
    f[t + 17] = 0;
    let mut sum: u32 = 0;
    for c in f[26..34].chunks(2) {
        sum += u16::from_be_bytes([c[0], c[1]]) as u32;
    }
    sum += 6 + len as u32;
    for c in f[t..t + len].chunks(2) {
        sum += (c[0] as u32) << 8 | *c.get(1).unwrap_or(&0) as u32;
    }
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    let c = !(sum as u16);
    f[t + 16] = (c >> 8) as u8;
    f[t + 17] = c as u8;
}

// ---- the NAT and DNS (nat.rs) ---------------------------------------------------------------

impl Network {
    /// A frame from the guest: rewritten if it belongs to an outbound
    /// connection, held (None) while that connection is being made.
    fn nat_in(&mut self, mut f: Vec<u8>) -> Option<Vec<u8>> {
        let Some((_, t)) = nat::tcp_offsets(&f) else { return Some(f) };
        let dst = nat::ip_dst(&f);
        let sport = nat::port_at(&f, t);
        let dport = nat::port_at(&f, t + 2);
        let flags = f[t + 13];
        let syn = flags & 2 != 0 && flags & 16 == 0;
        if let Some(k) = self.nat.flows.iter().position(|x| x.gport == sport && x.dst == dst && x.dport == dport) {
            let over = self.nat.flows[k].conn.is_none_or(|c| self.conns[c].closed);
            match &mut self.nat.flows[k].state {
                nat::FlowState::Connecting(_, held) => {
                    if syn {
                        *held = f; // the newest copy of the SYN waits
                    }
                    return None;
                }
                nat::FlowState::Open if syn && over => {
                    self.nat.flows.remove(k); // the same ports again: a new connection
                }
                nat::FlowState::Open => {
                    let standin = self.nat.flows[k].standin;
                    Self::to_standin(&mut f, t, standin);
                    return Some(f);
                }
            }
        }
        if !syn || !self.outbound {
            return Some(f);
        }
        let outside = !(dst.octets()[0] == 10 && dst.octets()[1] == 0 && dst.octets()[2] == 2);
        let to_host = dst == GATEWAY && dport < nat::STANDIN_FIRST && !self.listeners.iter().any(|l| l.port == dport);
        if !outside && !to_host {
            return Some(f);
        }
        let standin = self.nat.standin();
        let rx = self.nat.start_connect(SocketAddr::new(self.nat.host_for(dst), dport));
        self.nat.flows.push(nat::Flow { gport: sport, dst, dport, standin, state: nat::FlowState::Connecting(rx, f), conn: None });
        None
    }

    fn to_standin(f: &mut [u8], t: usize, standin: u16) {
        nat::set_ip(f, 30, GATEWAY);
        nat::set_port(f, t + 2, standin);
        nat::fix_ip_checksum(f);
        fix_checksums(f);
    }

    /// Finish host connections that have come through (or failed).
    fn nat_connects(&mut self) {
        for k in 0..self.nat.flows.len() {
            let nat::FlowState::Connecting(rx, _) = &self.nat.flows[k].state else { continue };
            let result = match rx.try_recv() {
                Ok(r) => r.ok(),
                Err(std::sync::mpsc::TryRecvError::Empty) => continue,
                Err(_) => None,
            };
            let standin = self.nat.flows[k].standin;
            if let Some(stream) = result {
                let _ = stream.set_nonblocking(true);
                let _ = stream.set_nodelay(true);
                let mut sock = Self::tcp_socket();
                sock.listen(smoltcp::wire::IpListenEndpoint { addr: Some(IpAddress::Ipv4(GATEWAY)), port: standin }).expect("listen");
                let h = self.sockets.add(sock);
                let c = self.add_conn(h, Some(stream));
                self.nat.flows[k].conn = Some(c.0);
            }
            // refused or timed out: nobody listens, so smoltcp answers with RST
            let st = std::mem::replace(&mut self.nat.flows[k].state, nat::FlowState::Open);
            if let nat::FlowState::Connecting(_, mut held) = st {
                if let Some((_, t)) = nat::tcp_offsets(&held) {
                    Self::to_standin(&mut held, t, standin);
                    self.wire.inbound.push_back(held);
                }
            }
        }
    }

    /// A frame for the guest: from a stand-in port, it must come from the
    /// address the guest connected to.
    fn nat_out(&mut self, f: &mut [u8]) {
        let Some((_, t)) = nat::tcp_offsets(f) else { return };
        if nat::ip_src(f) != GATEWAY {
            return;
        }
        let sport = nat::port_at(f, t);
        let dport = nat::port_at(f, t + 2);
        let Some(fl) = self.nat.flows.iter().find(|x| x.standin == sport && x.gport == dport) else { return };
        nat::set_ip(f, 26, fl.dst);
        nat::set_port(f, t, fl.dport);
        nat::fix_ip_checksum(f);
        fix_checksums(f);
    }

    /// Forget outbound connections that are over (and their sockets).
    fn nat_prune(&mut self) {
        let mut k = 0;
        while k < self.nat.flows.len() {
            let gone = match self.nat.flows[k].conn {
                Some(c) => {
                    let h = self.conns[c].handle;
                    self.conns[c].closed && self.sockets.get::<tcp::Socket>(h).state() == State::Closed
                }
                None => matches!(self.nat.flows[k].state, nat::FlowState::Open),
            };
            if gone {
                if let Some(c) = self.nat.flows[k].conn {
                    self.sockets.remove(self.conns[c].handle);
                }
                self.nat.flows.remove(k);
            } else {
                k += 1;
            }
        }
    }

    /// Answer DNS queries to 10.0.2.3.
    fn dns_poll(&mut self) {
        let mut queries = Vec::new();
        {
            let s = self.sockets.get_mut::<udp::Socket>(self.dns);
            while let Ok((data, meta)) = s.recv() {
                queries.push((meta.endpoint, data.to_vec()));
            }
        }
        let mut replies = Vec::new();
        for (from, q) in queries {
            match nat::parse_query(&q) {
                None => replies.push((from, nat::format_error(&q))),
                Some(p) if p.qtype == 1 => {
                    let rx = self.nat.start_resolve(p.name.clone());
                    self.nat.pending_dns.push((from.port, q, rx));
                }
                Some(p) => replies.push((from, nat::answer(&q, &p, &[]))),
            }
        }
        let mut k = 0;
        while k < self.nat.pending_dns.len() {
            match self.nat.pending_dns[k].2.try_recv() {
                Err(std::sync::mpsc::TryRecvError::Empty) => k += 1,
                r => {
                    let (port, q, _) = self.nat.pending_dns.remove(k);
                    let addrs = r.unwrap_or_default();
                    let p = nat::parse_query(&q).expect("parsed before");
                    replies.push((IpEndpoint::new(IpAddress::Ipv4(GUEST), port), nat::answer(&q, &p, &addrs)));
                }
            }
        }
        let s = self.sockets.get_mut::<udp::Socket>(self.dns);
        for (to, r) in replies {
            let _ = s.send_slice(&r, to);
        }
    }
}
