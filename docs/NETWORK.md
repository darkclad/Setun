# Networking

Goal: the Setun-11̅ on a real network. Trinux gets a TCP/IP stack of its own,
written in Trine like the rest of the kernel. The first payoff is logging in
from another machine (`telnet`, and later `ssh`); after that, talking to the
outside world.

There are four layers, and each one gets tested on its own:

1. **The network card (`TNIC`)**, an emulated device in `setun-core` (§1)
2. **The host side (`setun-net`)**, a user-mode network behind the card, the way QEMU's "slirp" works (§2)
3. **The kernel stack**: Ethernet, ARP, IPv4, ICMP, UDP and TCP, with sockets (§3, §4)
4. **User space**: tlibc socket calls and the `ifconfig`, `ping`, `nc`, `netstat` and `telnetd` programs (§5)

## 0. Octets on a ternary machine

The wire carries 8-bit octets. On the Setun-11̅, **one tryte holds one octet** (0..255),
the same rule as one character per tryte. That applies to frames in memory,
to what the card moves by DMA, and to the data read from and written to a socket.

- A socket carries octets. `send`/`write` of a tryte outside 0..255 fails with `EINVAL` before anything is sent.
- Text is Unicode code points, not UTF-8. Programs that put text on the
  network (`telnetd`, `nc`) convert to and from UTF-8 themselves, with the tlibc helpers `utf8_encode`/`utf8_decode`.
- Address structures follow the Trinux rule for system structures: **one word per field** (§4.2).
- "Network byte order" has no meaning inside a word, so `htons`, `htonl`,
  `ntohs` and `ntohl` return their argument unchanged. The kernel writes the
  octets in network order when it builds a packet.

Sizes follow the power-of-3 rule wherever the protocols leave the choice to us:

- 27-frame receive queues;
- one-page (3^9 = 19 683 octet) socket buffers;
- 27 sockets;
- a 27-entry ARP cache.

What the protocols fix stays as it is: 1500-octet MTU, 16-bit ports, 32-bit addresses and sequence numbers.

## 1. The network card (ISA §8, block 10)

| Block | Base port | Device | IRQ |
|---|---|---|---|
| 10 | 270 | network card (`TNIC`) | 8 |

Frames are Ethernet II frames without the FCS, 14..1514 octets, one octet per
tryte. DMA uses **physical** tryte addresses with any alignment and bypasses the MMU, like the disk.

| Offset | Register |
|---|---|
| 0 | `CMD`: **+1** send the frame at `TX_ADDR` (`TX_LEN` octets); **−1** copy the oldest received frame to `RX_ADDR` and drop it from the queue; **0** drop the oldest received frame without copying. Each command completes before the next instruction. |
| 1 | `STATUS`: 0 the last command succeeded, −1 it failed (see `ERROR`) |
| 2 | `TX_ADDR` |
| 3 | `TX_LEN` |
| 4 | `RX_ADDR` |
| 5 | `RX_LEN` (read-only): length of the oldest received frame, 0 if none |
| 6 | `RX_COUNT` (read-only): frames queued, at most 27 |
| 7 | `CTRL`: +1 raise IRQ 8 when a frame arrives |
| 8 | `MAC_HI` (read-only): octets 0..2 of the MAC address, as `o0·65536 + o1·256 + o2` |
| 9 | `MAC_LO` (read-only): octets 3..5 |
| 10 | `LINK` (read-only): +1 a network is attached, 0 none |
| 11 | `ERROR`: 0 none, 1 no link, 2 bad length, 3 a tryte outside 0..255 in the frame, 4 bus error during DMA, 5 nothing received, 6 bad command, 7 the transmit queue is full |
| 12 | `DROPS` (read-only): frames lost because the receive queue was full |

- The default MAC address is `52:54:00:12:34:56`. It is locally administered, the same address QEMU gives its first card.
- Frames for any address are received; the kernel filters them.
- The host takes transmitted frames from a queue of 27. If the host hasn't drained it, a send fails with error 7 and the kernel counts a drop. TCP recovers by retransmitting, as it would on a real card.

## 2. The host side: `setun-net`

The emulator stays deterministic: the card only exchanges frames with queues, and a front end moves them. `setun-net` is the network that sits behind the card.

- It is the guest's **gateway and router** at `10.0.2.2`, with MAC `52:55:0a:00:02:02`, on `10.0.2.0/24`; the guest is `10.0.2.15`. These are QEMU's addresses.
- It answers ARP and ping, built on `smoltcp`, an independent TCP/IP stack. Every test that crosses the card therefore checks Trinux against someone else's TCP.
- **Port forwards**: a host listener (for example `127.0.0.1:2323`) is tied to a guest port (23). Each accepted connection becomes a smoltcp TCP connection from `10.0.2.2` to the guest, and the bytes are copied both ways.
- **Test connections**: the same thing without a host socket, for deterministic tests: `connect(guest_port)`, `send`, `recv`, `state`.
- **Loss injection** for tests: drop chosen frames in either direction.
- Time is the emulator's clock, so a test run is reproducible.
- Later (N6): outbound connections (NAT to host sockets) and DNS at `10.0.2.3`.

The native front end: `setun --console --net 2323:23` (repeatable `--net HOST:GUEST`). Then `telnet localhost 2323` reaches Trinux's `telnetd`. The web build has no network yet: `LINK` reads 0.

## 3. The kernel stack (`kernel/net/`)

The kernel runs with interrupts off and is never preempted, so the stack needs no locks.

- Received frames are processed in the IRQ 8 handler: the handler drains the card's queue and runs each frame up the stack.
- Timers (ARP retries, TCP retransmission, TIME-WAIT) run on the timer tick (HZ = 100).
- Sends build the frame in a kernel buffer and DMA it out at once.

| File | Contents |
|---|---|
| `drivers/tnic.tri` | card driver: probe, MAC, receive loop, send, counters |
| `net/netdev.tri` | interfaces `lo` (127.0.0.1/8) and `eth0`: address, netmask, gateway, flags, counters; routing is "on-link or the default gateway" |
| `net/eth.tri`, `net/arp.tri` | Ethernet II; ARP cache of 27 entries with request retries, and one queued packet per unresolved entry |
| `net/ip.tri` | IPv4: header checks (version, IHL, total length, checksum, TTL); no fragmentation (fragments are dropped and counted, and DF is set on everything sent) |
| `net/icmp.tri` | echo request/reply, port unreachable, raw ICMP sockets for `ping` |
| `net/udp.tri` | UDP with checksums |
| `net/tcp.tri` | TCP: the RFC 9293 state machine, MSS 1460, a one-page receive window (19 683 < 65 535, so no window scaling), retransmission with RFC 6298 RTO and backoff, delayed ACK off, Nagle off, RST handling, TIME-WAIT of 2·MSL = 2 s, no SACK or timestamps |
| `net/socket.tri` | socket table (27), socket inodes (`I_SOCK`) behind file descriptors, the system calls, poll |
| `fs/procfs.tri` | `/proc/net/dev`, `/proc/net/arp`, `/proc/net/tcp`, `/proc/net/udp` |

- Checksums are the ones'-complement sum of 16-bit words. That is plain addition with an end-around carry, so it needs no binary operations.
- At boot, if the card reports a link, `eth0` gets the slirp defaults: 10.0.2.15/24, gateway 10.0.2.2. `ifconfig` can change them.

## 4. Sockets

### 4.1 System calls (Linux generic numbers)

| Call | Nr | Notes |
|---|---|---|
| `socket` | 198 | `AF_INET` (2): `SOCK_STREAM` (1), `SOCK_DGRAM` (2), `SOCK_RAW` (3) with `IPPROTO_ICMP` (root only); `SOCK_NONBLOCK`, `SOCK_CLOEXEC` |
| `bind` | 200 | `EADDRINUSE`, `EACCES` below port 1024 unless root |
| `listen` | 201 | backlog clamped to 27 |
| `accept` / `accept4` | 202 / 242 | |
| `connect` | 203 | `ECONNREFUSED`, `ETIMEDOUT`, `EINPROGRESS` when non-blocking |
| `getsockname` / `getpeername` | 204 / 205 | |
| `sendto` / `recvfrom` | 206 / 207 | `MSG_DONTWAIT`, `MSG_PEEK` |
| `setsockopt` / `getsockopt` | 208 / 209 | `SO_REUSEADDR`, `SO_ERROR`, `SO_TYPE`, `SO_KEEPALIVE` (accepted), `TCP_NODELAY` (accepted) |
| `shutdown` | 210 | |

`read`, `write`, `close`, `fcntl(O_NONBLOCK)`, `fstat` (`S_IFSOCK`) and `ppoll` work on sockets. A write on a connection whose peer has closed raises `SIGPIPE` and returns `EPIPE`.

### 4.2 Structures (one word per field)

```
struct sockaddr_in {          // sizeof = 9 trytes; addrlen is checked against it
    sin_family: int;          // AF_INET
    sin_port: int;            // 0..65535
    sin_addr: int;            // 0..2^32-1, 10.0.2.15 = 10·2^24 + 0·2^16 + 2·2^8 + 15
}
```

`struct ifreq` for the interface ioctls is `{ name: [27]char; addr: int; }`. The ioctls are `SIOCGIFADDR`/`SIOCSIFADDR`, `SIOCGIFNETMASK`/`SIOCSIFNETMASK`, `SIOCGIFFLAGS`/`SIOCSIFFLAGS`, `SIOCGIFHWADDR`, plus Trinux's `SIOCGIFGW`/`SIOCSIFGW` for the default gateway (Linux keeps that in a routing table).

## 5. User space

- **tlibc** (`user/lib/socket.tri`): the calls above, `inet_aton`/`inet_ntoa`, `htons` & co., `utf8_encode`/`utf8_decode`, `gethostbyname` from `/etc/hosts`.
- **`/sbin/ifconfig`**: show the interfaces and set address, mask and gateway.
- **`/bin/ping`**: setuid root, raw ICMP.
- **`/bin/nc`**: connect or listen (`-l`), text by default and `-b` for raw octets.
- **`/bin/netstat`**: reads `/proc/net/tcp`.
- **`/sbin/telnetd`**: RFC 854 with `WILL ECHO` and `WILL SUPPRESS-GO-AHEAD`. It reads and ignores `NAWS` and other options, and runs `/bin/login` on a pty, one process per connection. init starts it when `eth0` is up.

## 6. Milestones

Each step comes with negative tests, the same as the kernel milestones.

| # | Work | Tests |
|---|---|---|
| N1 | `TNIC` device; `setun-net` (ARP, ping, forwards, test connections, loss); `setun --net` | device registers and DMA, every `ERROR` code, queue overflow and `DROPS` |
| N2 | driver, `lo`/`eth0`, Ethernet, ARP, IPv4, ICMP echo; `ifconfig`, `ping`, `/proc/net/dev` | ping the gateway and 127.0.0.1; the gateway pings Trinux; frames that are short, have bad checksums, the wrong version, bad lengths, or are fragments are dropped and counted |
| N3 | sockets, UDP, raw ICMP; tlibc | a UDP echo through the gateway and on loopback; `bind` conflicts, bad `addrlen`, bad family, privileged ports |
| N4 | TCP | a guest server with a smoltcp client; a guest client to a host-side listener; loopback; RST on a closed port; retransmission under loss; full buffers and flow control; every close path; malformed segments |
| N5 | `nc`, `netstat`, `telnetd`; docs | log in through `setun --net` with a real host socket; many sessions at once |
| N6 | outbound NAT and DNS; then SSH (crypto: see below) | |

**SSH**: an `sshd` needs SHA-2, ChaCha20, X25519 and Ed25519. X25519 and Ed25519's
field arithmetic are plain modular arithmetic, which a ternary machine does well.
SHA-2 and ChaCha20 need binary XOR, AND and rotate. They can be done bit by bit
(md5 does that) or with binary-integer instructions added to the ISA. That
choice belongs with N6.
