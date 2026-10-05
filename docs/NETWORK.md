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
- Later (N7): outbound connections (NAT to host sockets) and DNS at `10.0.2.3`.

The native front end: `setun --console --net 2323:23` (repeatable `--net HOST:GUEST`). Then `telnet localhost 2323` reaches Trinux's `telnetd`. The web build has no network yet: `LINK` reads 0.

## 3. The kernel stack (`kernel/net/`)

The kernel runs with interrupts off and is never preempted, so the stack needs no locks.

- Received frames are processed in the IRQ 8 handler: the handler drains the card's queue and runs each frame up the stack.
- Timers (ARP retries, TCP retransmission, TIME-WAIT) run on the timer tick (HZ = 100).
- Sends build the frame in a kernel buffer and DMA it out at once.

The plan (as built, Ethernet shares `arp.tri`, ICMP is in `ip.tri`, and UDP and the interfaces' ioctls are in `socket.tri`; see §7):

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
| N6 | SSH: a random-number device, the crypto library, `sshd` (§8) ✅ | RFC test vectors; the host's OpenSSH logs in, runs commands and a pty session, moves 120 KB each way; refusals of other algorithms, wrong passwords, a non-SSH-2 client, an overlong line and a tampered packet |
| N7 | outbound NAT and DNS | |

**SSH and binary operations**: SHA-2 and ChaCha20 need binary XOR, AND and
rotate. They are done in software, with no change to the ISA (§8): rotates
and shifts are plain arithmetic, and XOR and AND go through 256×256 tables.
Binary-integer instructions could still come later, as a speed-up.

## 7. As built (N1–N5)

### Running it

```
cargo run --release -p setun-native -- --console --net 2323:23
telnet localhost 2323          # from another terminal: Trinux's login prompt
```

- `--net` attaches the gateway. `HOSTPORT:GUESTPORT` adds a port forward, and `ADDR:HOSTPORT:GUESTPORT` listens on another host address. A bare `--net on` attaches the network without forwards.
- Forwards listen on 127.0.0.1 unless an address is given, so the machine is not reachable from the LAN by accident. Trinux's accounts are `root`/`root` and `user`/`user`.
- When `eth0` is up at boot (the card reports a link), init starts `/sbin/telnetd` next to the console login.

### Kernel

| File | Lines | What |
|---|---|---|
| `drivers/tnic.tri` | ~90 | probe, MAC, IRQ 8 receive loop, send |
| `net/net.tri` | ~330 | octet helpers, checksums, `lo`/`eth0`, the 27 counters, the loopback queue, `net_bh`, `net_sleep` |
| `net/arp.tri` | ~220 | Ethernet, the ARP cache, 9 waiting packets |
| `net/ip.tri` | ~120 | IPv4 in and out, ICMP echo, destination unreachable |
| `net/socket.tri` | ~820 | sockets, rings, the system calls, interface ioctls, UDP, raw ICMP |
| `net/tcp.tri` | ~720 | TCP |

- **Loopback** packets wait in a one-page queue (9 slots of 3^7) until `net_bh()` delivers them. It runs at the end of every network system call, the card's interrupt and the timer tick.
  - A reply is therefore never built while the packet it answers is still in `txbuf`.
  - `net_sleep()` delivers the queue before sleeping. If anything was delivered it returns at once, so a wake-up that came from that delivery is not lost and the caller checks its condition again.
- **ARP**: a packet for a next hop that isn't resolved yet waits in one of 9 slots of 3^7 (one page). ARP asks once a second, three times, then drops the waiting packets and counts `arp_failed`.
- **TCP** keeps things simple where a virtual link allows, and the tests check each choice:
  - segments are accepted in order only; a gap gets a duplicate ACK;
  - a timeout resends from `snd_una` (go-back-N);
  - RTO follows RFC 6298 in 10 ms ticks, from 200 ms to 60 s, 5 SYN retries and 9 data retries;
  - a zero-window probe goes out on the persist timer;
  - RFC 5961's exact-RST and challenge-ACK rules apply;
  - `TIME-WAIT` lasts 2 s; an orphaned `FIN-WAIT-2` lasts 60 s.
- Closing a connection that still has unread data sends RST, and so does data arriving at an orphaned socket (both as Linux does).
- `SO_REUSEADDR`, as on Linux: a stream socket may share a port with sockets that are not listening, such as connections still in `TIME-WAIT` or old ones a restarted server left behind.
- A listener's backlog counts children still in the handshake and children waiting for `accept`. A SYN past the limit is dropped, and the client's retry gets in once there is room.
- `/proc/net/stat` lists one counter per line (`ip_csum_errors`, `tcp_retransmits`, `udp_no_port`, …). The negative tests read it.
- `/proc/net/tcp` and `/proc/net/udp` show Linux's first columns plus the owner's uid. `/proc/net/dev` and `/proc/net/arp` use Linux's layouts.
- `struct ifreq` is `{ name: [27]char; value: int; value2: int; }`. `SIOCGIFHWADDR` returns octets 0..2 in `value` and 3..5 in `value2`, because 48 bits do not fit a 27-trit word.

### User space

- `user/lib/socket.tri`:
  - the calls, `inet_aton`/`inet_ntoa`, and `resolve_host` (dotted quad, `/etc/hosts`, `localhost`);
  - `struct ifreq`;
  - UTF-8 helpers. A character followed by U+0305 decodes to the negative (overlined) tryte, and an overlined tryte encodes the same way, as on the serial console.
- `nc [-lubzv] [-p port] [-w secs] [host] port`. When stdin ends, a TCP connection is half-closed and nc prints until the other side closes; a UDP client waits one more second for replies.
- `ping [-c n] [-s size] [-W secs] host` is set-user-ID root and drops root once its raw socket is open.
- `ifconfig [iface [addr] [netmask m] [gw g] [up|down]]`
- `netstat [-tualn]`
- `telnetd [-p port] [-1]`:
  - each connection gets a pty with `/bin/login` and a relay process;
  - the server offers `WILL ECHO` and `WILL SGA` and reads and drops every other command, including `NAWS`;
  - CR LF and CR NUL become CR, and `\n` goes out as CR LF;
  - when the client hangs up, the session gets `SIGHUP`.
- `login` hands the console's devices to the user only when it runs on the console, not on a pty.

### Tests

- `crates/core/tests/nic.rs`: the card's registers, DMA, and every `ERROR` code.
- `crates/net/tests/wire.rs`: the gateway answers hand-made frames.
- `crates/trinux/tests/net.rs` boots Trinux with the gateway attached and covers:
  - pings both ways; ARP failure; no route; no link;
  - `ifconfig` settings and refusals, as root and as a user;
  - 15 kinds of broken frame, each dropped and counted;
  - UDP through the gateway and on loopback; ICMP port unreachable;
  - `nettest`: 98 checks of the socket calls and their errors on loopback, including 100 000 octets through the 19 683-octet window, backlog retry, RST on close, `TIME-WAIT` and `SO_REUSEADDR`, and the socket limit;
  - TCP against smoltcp in both directions; RST on a closed port; a reset in the middle of a connection;
  - a link that drops every fourth frame each way;
  - a forward from a real host socket;
  - telnet: two sessions at once, a refused password, Ctrl-C, logout, and a client hang-up.
  - TCP bulk from the gateway into Trinux: 108 894 octets arrive with the right MD5.

## 8. SSH (N6)

```
cargo run --release -p setun-native -- --console --net 2222:22
ssh -p 2222 root@localhost          # password root; or user/user
```

When `eth0` is up at boot, init starts `/sbin/sshd` next to `telnetd`. Without `--disk`, `--console` boots a fresh disk every time, which means a new host key every time, so a client warns about a changed key. Use a disk file (`mkdisk -o trinux.disk`, `--disk trinux.disk`) to keep the key.

### Randomness: the `TRNG` device (ISA §8.2, block 11, port 297)

| Offset | Register |
|---|---|
| 0 | `DATA` (read): the next random octet, 0..255 |
| 1 | `SOURCE` (read-only): +1 seeded from the host's entropy, 0 a fixed seed |

- The octets are a ChaCha20 key stream (RFC 8439 block function) under a 256-bit seed.
- `setun` seeds it from the operating system: 256 bits through the standard library's randomly keyed hasher, so no extra dependency is needed.
- Without a seed (in tests, and in the web build) the key is all zeros. A run is then reproducible, and the kernel says at boot that the numbers are not for keys.
- Trinux exposes the device as `/dev/random`, `/dev/urandom` (1,8 and 1,9) and `getrandom(2)` (278).

### Crypto in Trine (`user/lib/`)

| File | What | Cost (instructions) |
|---|---|---|
| `bin32.tri` | 32-bit binary arithmetic: add, shift and rotate are plain arithmetic on words; XOR and AND go through two 256×256 tables made at start-up on the heap | tables: about 1.3 M once |
| `sha2.tri` | SHA-256; SHA-512 with each 64-bit value as two 32-bit halves | about 50 k per SHA-256 block, 0.2 M per SHA-512 block |
| `chacha.tri` | ChaCha20 (64-bit counter and nonce, as SSH uses it) and Poly1305 (TweetNaCl's 17 limbs of 8 bits) | about 0.1 M per 64 octets |
| `c25519.tri` | X25519 and Ed25519 signing, after TweetNaCl | X25519 ≈ 38 M, an Ed25519 signature ≈ 65 M |
| `base64.tri` | RFC 4648, for public keys and fingerprints | |

- The field elements of 2^255 − 19 are 16 **balanced** limbs of 16 bits. Carries use `divr` (round to nearest), so a limb ends in −2^15..2^15, and every intermediate value stays far inside a 27-trit word:
  - products under 2^34;
  - sums of 16 products under 2^38;
  - carried before the top half is folded down times 38.
- `/bin/cryptotest` checks the vectors of FIPS 180-4, RFC 7748, RFC 8032 and RFC 8439, plus values from another implementation (Python's `cryptography`).

### `sshd`

| | |
|---|---|
| Key exchange | `curve25519-sha256` (also `@libssh.org`), RFC 8731; strict KEX (`kex-strict-s-v00@openssh.com`, the Terrapin fix): sequence numbers restart after NEWKEYS and nothing else may come during the first exchange; re-keying when the client asks |
| Host key | `ssh-ed25519`, made on first start: `/etc/ssh/ssh_host_ed25519_key` (the 32-octet seed in hex, mode 0600) and `.pub` in OpenSSH's format; the boot log shows its `SHA256:` fingerprint |
| Cipher | `chacha20-poly1305@openssh.com`. The tag is checked before anything is decrypted; a bad tag ends the connection |
| Login | `password`, checked against `/etc/shadow` like `login(1)`. Empty passwords are never accepted; an unknown user is refused the same way as a wrong password; after 6 failures the server disconnects |
| Sessions | one `session` channel per connection; `pty-req`, `shell`, `exec`, `window-change`; the exit code goes back as `exit-status` (or `exit-signal`) |
| Windows | 3^10 octets in, 3^9 per packet; output waits for the client's window |

- Each connection runs in its own process. The child that runs the shell or command becomes the user (groups, gid, uid), changes to the home directory, and gets `HOME`, `USER`, `LOGNAME`, `SHELL`, `PATH` and `TERM`. A shell is a login shell (`-sh`) and prints `/etc/motd` first.
- Text is UTF-8 on the wire. A character followed by U+0305 becomes the overlined (negative) tryte, as on the serial console. On a pty, `\n` goes out as CR LF.
- Trine has no bounds checks, so every buffer the client can reach has an explicit limit:
  - packet length ≤ 35 000;
  - KEXINIT ≤ 4096;
  - a version line ≤ 255 characters, with at most 20 lines before it;
  - names and commands bounded by their buffers;
  - channel data decoded in pieces of 2187 octets.
- Not yet supported: public-key login; `env`, `subsystem` (sftp) and port forwarding requests, which get `CHANNEL_FAILURE`; and more than one channel per connection.

### What a login costs

- A key exchange takes about 140 M instructions: an X25519 key pair, the shared secret, and an Ed25519 signature.
- `--console` runs a busy guest as fast as the host allows (about 200–400 MIPS with the JIT), so that is under a second.
- At the window front end's default 10 MIPS it would be about 14 s.

### Tests (`crates/trinux/tests/ssh.rs`; skipped without an `ssh` on the host, `SETUN_SSH` picks one)

They drive the host's real OpenSSH client (10.2 here) through a port forward, with the password given by `SSH_ASKPASS`. They cover:
- commands with exit codes, UTF-8 output and the user's identity;
- a `-tt` interactive session (motd, UTF-8 typed and echoed, CR LF, exit code);
- 108 894 octets out and 120 000 in, through both windows;
- `Ciphers`, `KexAlgorithms` and `HostKeyAlgorithms` the server doesn't offer;
- 7 wrong passwords (disconnected at 6), an unknown user, a missing command (127);
- an SSH-1.5 client and an overlong line before the version;
- a packet altered in flight with its TCP checksum fixed up, which SSH's MAC catches.
