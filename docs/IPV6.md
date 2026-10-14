# IPv6

**Status (2026-10-14): V1 built: Trinux autoconfigures from the gateway's Router Advertisements, defends and checks its addresses, resolves neighbours, and `ping6` works; the exchange vectors pass (§7). V2 next.** Trinux speaks IPv6 beside IPv4 on the same `TNIC` card ([`NETWORK.md`](NETWORK.md)): a host stack in Trine with Neighbor Discovery, stateless address autoconfiguration, ICMPv6, UDP and TCP, and `AF_INET6` sockets. `setun-net` grows the other side: a router that advertises a prefix, a NAT to the host's IPv6 sockets, and DNS answers with AAAA records. Gate: **the host's OpenSSH logs in to Trinux over IPv6, and Trinux's `wget` and `ssh` reach the host over IPv6.**

| Layer | What | Section | Milestone |
|---|---|---|---|
| addresses | the link's prefix, the interface's addresses, interning | §1 | V1 |
| network | the IPv6 header, extension headers, ICMPv6 | §2 | V1 |
| link | Neighbor Discovery, SLAAC, duplicate detection | §3 | V1 |
| sockets | `AF_INET6`: raw ICMPv6, UDP, TCP | §4 | V2 |
| host side | the router, NAT and DNS in `setun-net` | §5 | V3 |
| user space | `ping6`, `nc -6`, `wget`, `ssh`, `host`, the servers | §6 | V2–V4 |
| conformance | vectors from an independent model | §7 | V1–V4 |

The protocols fix their sizes (128-bit addresses, 16-bit ports, 32-bit sequence numbers), so the power-of-3 rule applies only where they leave the choice: 27 neighbour-cache entries, an 81-entry address table.

## 1. Addresses

The link copies QEMU's IPv6 user network, on a unique-local prefix instead of QEMU's deprecated `fec0::/64` (RFC 3879):

| Address | Who |
|---|---|
| `fd00::/64` | the link's prefix, advertised by the router |
| `fd00::2` | the gateway: router, NAT, and stand-in for the host's `::1` |
| `fd00::3` | DNS, a second address of the gateway |
| `fe80::5055:aff:fe00:202` | the gateway's link-local address (its MAC `52:55:0a:00:02:02` as an EUI-64) |
| `fd00::5054:ff:fe12:3456` | Trinux: the prefix and its MAC `52:54:00:12:34:56` as an EUI-64 (RFC 4862) |
| `fe80::5054:ff:fe12:3456` | Trinux's link-local address |
| `::1` | loopback |

An address is **16 octets, one per tryte**, as everywhere on the wire side ([`NETWORK.md`](NETWORK.md) §0). 128 bits do not fit a 27-trit word. They would fit an 81-trit `long` as a signed number (3^81 / 2 > 2^127), but every `long` operation is a library call.

**Interning.** The kernel keeps a table of 81 IPv6 addresses. Each one has a small integer **handle**, and the same address always gets the same handle. Sockets, TCP and UDP carry an IPv6 address as its handle in the same `int` fields that hold an IPv4 address, so the TCP state machine and the socket lookups compare addresses with `==` for both families.
- Handle 0 is `::`, the unspecified address, just as IPv4's 0 is `INADDR_ANY`.
- The interface's own addresses and `::1` are pinned.
- A full table frees the handles that no socket refers to and that the packet being processed has not taken. A datagram waiting in a socket carries the sender's 16 octets, not a handle, so only sockets pin handles. If every handle is in use, the packet is dropped and counted.

**Text.** RFC 5952: lowercase hex, leading zeros dropped, the longest run of two or more zero groups (the first, on a tie) as `::`. Input also takes upper case, a run of one group as `::`, and an IPv4 tail (`::ffff:1.2.3.4`). With a port: `[fd00::2]:22`.

## 2. The network layer

**The header (RFC 8200).** 40 octets: version 6, traffic class 0 and flow label 0 on everything sent, payload length, next header, hop limit 64 (255 for Neighbor Discovery), source, destination.

Received packets are checked in order. Each failure is counted and drops the packet:
1. at least 40 octets, version 6;
2. the payload length fits the frame (octets past it are padding and ignored);
3. the destination is one of ours: our unicast addresses, all-nodes `ff02::1`, or the solicited-node group of one of our addresses (`ff02::1:ffXX:XXXX`, the last 24 bits);
4. the source is not multicast, and not `::1` on `eth0`;
5. the extension headers, then the upper layer, below.

**Extension headers.** Hop-by-Hop Options (only right after the header), Destination Options and Routing are walked:
- An option's type says what to do with an option we don't know (RFC 8200 §4.2). With 00 skip it; with 01 drop the packet; with 10 drop and send Parameter Problem code 2; with 11 the same, but not for a multicast destination. Pad1 and PadN are known.
- A Routing header with segments left 0 is skipped. Any other is answered with Parameter Problem code 0, pointing at its type.
- A **Fragment** header drops the packet, counted: no reassembly, as with IPv4. Nothing Trinux sends is fragmented, and the link MTU is 1500.
- An unknown next header gets Parameter Problem code 1, pointing at the field that named it.
- No Next Header (59) ends the packet quietly.

**ICMPv6 (RFC 4443).** The checksum covers the pseudo-header (RFC 8200 §8.1: source, destination, upper-layer length as 32 bits, next header).
- Echo requests to a unicast address are answered, and to a multicast one too (as Linux does).
- Errors go back for a closed UDP port (Destination Unreachable code 4), an unknown next header and bad options (Parameter Problem). Never about an ICMPv6 error, a packet to a multicast address (except Parameter Problem code 2), or a packet from `::` or a multicast source. At most a burst of 9, then one per 100 ms (a token bucket).
- An error quotes as much of the offending packet as fits in 1280 octets.
- Received errors go to raw sockets. Destination Unreachable codes 1, 3 and 4 and Parameter Problem abort a TCP connection still in SYN-SENT with the matching errno, as IPv4's do. Packet Too Big is counted and otherwise ignored, since nothing larger than 1500 is ever sent.

**Routing.** On-link for `fe80::/10`, the advertised prefixes and multicast. Everything else goes to the default router, if there is one, or fails with `ENETUNREACH`.

**Source selection** (a short RFC 6724): `::1` for `::1`, the link-local address for link-local and multicast destinations, and the global address otherwise, if it is preferred. A tentative or deprecated address is never chosen.

## 3. The link: Neighbor Discovery and SLAAC

**Ethernet.** EtherType 0x86DD. A multicast destination maps to `33:33` and its last 32 bits (RFC 2464). The card receives every frame, and the kernel keeps unicast frames for our MAC and multicast frames for our groups.

**Neighbor Discovery (RFC 4861).** Every ND message must arrive with hop limit 255, have a valid checksum and code 0, and pass its message's own checks. Anything else is dropped and counted.
- **Neighbour cache:** 27 entries: address, MAC, state, timer, retries, and one packet waiting for resolution (a newer one replaces it, as in the ARP cache).
- **States:**

| State | Meaning | Leaves |
|---|---|---|
| INCOMPLETE | a multicast Neighbor Solicitation is out | NA: REACHABLE; after 3 tries 1 s apart: deleted, and the waiting packet's sender gets `EHOSTUNREACH` |
| REACHABLE | confirmed within 30 s | after 30 s: STALE |
| STALE | usable, unconfirmed | used: DELAY |
| DELAY | waiting 5 s for upper-layer confirmation (TCP progress confirms) | then PROBE |
| PROBE | unicast solicitations, 3 tries 1 s apart | NA: REACHABLE; else deleted |

- **Solicitations:** an NS for one of our addresses (not tentative) gets a solicited NA with the router flag clear and the override flag set. An NS from `::` is DAD: the answer goes to `ff02::1` with the solicited flag clear. The NS's source link-layer option creates or updates a STALE entry.
- **Advertisements:** an NA updates the cache as RFC 4861 §7.2.5 says, including the override flag and a changed MAC. An NA for one of our own tentative addresses means DAD failed.
- **Redirects** are counted and ignored. Trinux is a host and never forwards.

**Address autoconfiguration (RFC 4862).** When the card reports a link:
1. The link-local address is formed and **tentative**. One DAD solicitation goes from `::` to its solicited-node group. With no answer within 1 s it becomes preferred; an answer, or someone else's DAD solicitation for it, makes it `dadfailed` (shown by `ifconfig`, never used), and IPv6 stays down on `eth0`.
2. Up to three Router Solicitations, 4 s apart, from the link-local address with a source link-layer option.
3. A **Router Advertisement** must come from a link-local address with hop limit 255:
   - Its router lifetime makes the sender the default router; 0 removes it.
   - A Prefix Information option with the A flag, a /64 length and a valid lifetime of at least its preferred one forms the prefix and the interface identifier. The address gets its own DAD, then is used until its preferred lifetime runs out (deprecated), and removed when the valid lifetime runs out. A later RA updates the lifetimes, with RFC 4862 §5.5.3e's two-hour rule against shortening the valid lifetime.
   - The MTU option is accepted from 1280 to 1500.
   - RDNSS (RFC 8106) is recorded in `/proc/net/rdnss`. `/etc/resolv.conf` stays as it is, since 10.0.2.3 answers AAAA queries too.
4. Up to 3 global addresses besides the link-local one (unused slots are listed as such).

MLD reports are not sent: the virtual link has no switch that snoops them, and the gateway delivers every multicast frame anyway. This is RFC 8504's one SHOULD-level gap here, and it is said so.

## 4. Sockets: `AF_INET6`

`AF_INET6 = 10`, as in Linux. `struct sockaddr_in6` follows the Trinux rule, one word per field, 20 words:

| Word | Field |
|---|---|
| 0 | `sin6_family` (10) |
| 1 | `sin6_port` |
| 2 | `sin6_flowinfo` (0) |
| 3 … 18 | `sin6_addr`: 16 octets |
| 19 | `sin6_scope_id`: 0, or 2 (`eth0`) for a link-local address |

- SOCK_STREAM (TCP), SOCK_DGRAM (UDP) and SOCK_RAW with `IPPROTO_ICMPV6` (58, root only). Raw sockets came with V1; the others answer `EPROTONOSUPPORT` until V2.
- A raw ICMPv6 socket works as in Linux: the kernel computes the checksum of what is sent, and a receive returns the ICMPv6 message without its IPv6 header.
- **IPv6 sockets are IPv6-only**, as if `IPV6_V6ONLY` were always set (OpenBSD's choice): no IPv4-mapped addresses. A server that wants both families opens two sockets. `setsockopt(IPV6_V6ONLY, 1)` succeeds; setting 0 fails with `ENOPROTOOPT`.
- A link-local address needs a scope: `connect`, `bind` and `sendto` with an `fe80::` address and scope 0 fail with `EINVAL`.
- TCP's MSS is 1440 (1500 − 40 − 20), announced as such.
- UDP checksums are mandatory: a zero checksum is dropped, and one that computes to 0 is sent as 0xFFFF.
- `getsockname`, `getpeername`, `accept` and `recvfrom` write a `sockaddr_in6` and say its length, 60 trytes.
- `/proc/net/tcp6`, `/proc/net/udp6` and `/proc/net/if_inet6` follow Linux's columns, in hex.

## 5. The host side (`setun-net`)

- smoltcp with `proto-ipv6` answers Neighbor Discovery and ping for `fd00::2`, `fd00::3` and the gateway's link-local address.
- **Router Advertisements** are built by `setun-net` itself, which smoltcp does not do as a router. One goes out in answer to each Router Solicitation, and one every 200 s after the first, so a guest that never speaks IPv6 never sees one. Each carries:
  - a router lifetime of 1800 s;
  - the prefix `fd00::/64` with L and A set, valid 86400 s, preferred 14400 s;
  - MTU 1500;
  - RDNSS `fd00::3`.
- **NAT:** a TCP SYN or UDP datagram from the guest to an address off `fd00::/64` goes out through a host IPv6 socket, exactly as IPv4's NAT works (NETWORK.md §9, §13). `fd00::2` stands for the host's `::1`. With no IPv6 route on the host, the guest's SYN is refused, as an unreachable IPv4 address is.
- **DNS** at `10.0.2.3` and `fd00::3` answers AAAA queries from the host's resolver (up to 8 records), and A queries as before.
- **Port forwards to the guest over IPv6:** `--net6 HOST:GUEST` listens on the host like `--net` and reaches the guest's global address. Test connections take a family.
- Tests can inject raw frames and capture what the guest sends (`Network::inject`, `Network::capture`), for the exchange vectors (§7).

## 6. User space

- tlibc: `inet_pton`/`inet_ntop` for both families, `struct sockaddr_in6`, `getaddrinfo`-style `resolve_any` returning AAAA as well.
- **`ping6 [-c count] [-s size] [-W timeout] host`**, a raw ICMPv6 socket, set-user-ID root like `ping`, printed as iputils prints it. It shows no `ttl=`: a raw socket does not see the hop limit without `IPV6_RECVHOPLIMIT`. A link-local address is on eth0.
- **`ip6vec`** checks `inet_pton6` and `inet_ntop6` against the vectors' text lines (§7).
- **`nc -6`/`-4`**. Without either, an address literal decides; a name tries AAAA first, then A (RFC 6724's default preference, since the prefix is unique-local and the NAT carries both).
- **`wget`, `ssh`, `scp`, `sftp`, `host`** take IPv6 literals in brackets where a port can follow. `host` prints AAAA records.
- **`telnetd`, `sshd` and `httpd` listen on both families.**
- `/proc/net/if_inet6` has Linux's columns (in lowercase hex, as Linux writes them); `/proc/net/neigh6` lists the neighbour cache with its states and the default router; `/proc/net/stat6` has the counters, the RDNSS address and the link MTU.
- `ifconfig` shows `inet6` lines with scope and state. `netstat` shows `tcp6` and `udp6` (V2).

## 7. Conformance

As for the ISA, REAL, the formats and TNET: vectors made by an independent model of this specification (`tools/gen_ipv6vectors.py`, plain Python on octet lists, nothing shared with the kernel), in `user/share/ipv6vectors.txt`:
- `text IN = OUT` and `bad IN`: RFC 5952 formatting and RFC 4291 parsing, with addresses that must be refused. `/bin/ip6vec` checks them inside Trinux.
- `ex NAME : F => G ; H …`: an Ethernet frame given to Trinux after autoconfiguration, and exactly the frames it must send back (none: it drops F). They cover echo, every ICMPv6 error and every reason to drop, the extension headers and options, and Neighbor Discovery. `crates/trinux/tests/ipv6.rs` runs them with the gateway's frames captured: before each frame a made-up peer (`fd00::77`, `fe80::77`) is made a REACHABLE neighbour, so no reply waits on resolution, and 200 ms pass after it, so the error limit has refilled.

What changes the station for good (DAD failures, prefixes and their lifetimes, losing the router, neighbour unreachability, the error limit) has tests of its own in the same file. Every milestone has negative tests and mutation checks.

The gateway's smoltcp drops a packet that needs a neighbour it has not resolved, instead of queueing it (RFC 4861 §7.2.2 says it should queue one). So the first `ping6` to its link-local address can go unanswered: answering it is what makes smoltcp solicit Trinux. Trinux queues one packet per neighbour.

## 8. Milestones

| | Contents | Gate |
|---|---|---|
| V1 ✅ | §1–§3 in the kernel; raw ICMPv6 sockets, `ping6`, `ip6vec`; `inject`/`capture` and RAs in `setun-net`; `/proc/net/{if_inet6,neigh6,stat6}`; the vectors | Trinux autoconfigures `fd00::5054:ff:fe12:3456` and pings `fd00::2` |
| V2 | §4: UDP and TCP over IPv6; `nc -6`; `ifconfig` and `netstat` for IPv6; telnetd and httpd on both families | a test connection over IPv6 reaches `telnetd`; UDP both ways |
| V3 | §5: NAT, AAAA, `--net6` | `wget http://[fd00::2]:port/` from a host server on `::1`; the host's OpenSSH logs in over `--net6` |
| V4 | §6: names, brackets, `ssh`/`scp`/`sftp`/`host`, `netstat` | `ssh` from Trinux to the host's sshd on `::1` by name |
