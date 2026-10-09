# TNET: the native ternary network

**Status (2026-10-09): C1–C3 built, so Gate C has passed: `ssh` runs between two Setun machines over TNET. C4 and C5 too: `ssh` makes post-quantum key exchanges (ML-KEM-768, and Streamlined NTRU Prime, each with X25519), with OpenSSH and between Setun machines. Conformance vectors cover TCRC-18, TL frames and the station's exchanges for TNP, TCMP, TDP and TSP (§7). Phase C of [`TERNARY_WORLD.md`](TERNARY_WORLD.md).** Setun-11̅ machines talk to binary networks through `TNIC`, one octet per tryte ([`NETWORK.md`](NETWORK.md)). TNET is how two Setun machines talk to each other with nothing binary in between: frames of trytes, ternary addresses and checksums, a stream protocol with word-sized sequence numbers, and the same BSD sockets over both. Gate C: **`ssh` from one Setun machine to another over TNET**.

| Layer | Name | Section | Milestone |
|---|---|---|---|
| card | `TLINK`, block 12 | §1 | C1 ✅ |
| link | TL frames with a TCRC-18 check | §2 | C1 ✅ |
| network | TNP: 27-trit host addresses | §3 | C2 ✅ |
| transport | TSP streams, TDP datagrams | §4 | C2 ✅ |
| sockets | `AF_TNP` beside `AF_INET` | §5 | C2–C3 ✅ |
| key exchange | ML-KEM-768 and sntrup761 in `ssh` | §6 | C4–C5 ✅ |

Sizes follow the power-of-3 rule throughout, since none of it has to meet a binary standard.

## 1. The card: `TLINK` (ISA §8, block 12)

| Block | Base port | Device | IRQ |
|---|---|---|---|
| 12 | 324 | ternary link (`TLINK`) | 9 |

`TLINK` moves **frames of trytes**, any value −9841 … 9841, between memory and a point-to-point link. It works like `TNIC` with no octet rule: a frame is 1 … 2187 (3^7) trytes, DMA uses physical tryte addresses, and the card only exchanges frames with queues of 27 that a front end connects to the other machine.

| Offset | Register |
|---|---|
| 0 | `CMD`: +1 send the frame at `TX_ADDR` (`TX_LEN` trytes); −1 copy the oldest received frame to `RX_ADDR` and drop it; 0 drop it without copying. Each command completes before the next instruction. |
| 1 | `STATUS`: 0 the last command succeeded, −1 it failed |
| 2 | `TX_ADDR` |
| 3 | `TX_LEN` |
| 4 | `RX_ADDR` |
| 5 | `RX_LEN` (read-only): the oldest received frame's length, 0 if none |
| 6 | `RX_COUNT` (read-only): frames queued, at most 27 |
| 7 | `CTRL`: +1 raise IRQ 9 when a frame arrives |
| 8 | `LINK` (read-only): +1 a link is attached, 0 none |
| 9 | `ERROR`: 0 none, 1 no link, 2 bad length, 4 bus error during DMA, 5 nothing received, 6 bad command, 7 the transmit queue is full |
| 10 | `DROPS` (read-only): frames lost because the receive queue was full |
| 11 | `ADDR` (read-only): the station address the front end gave the card (§2), 0 if none |

The card does not check frames: the link layer does (§2), so a corrupted frame reaches the driver, which counts and drops it.

**The wire.** A front end joins two cards: in one process (two machines stepped in turn, as the tests do) or between processes over UDP (`setun --tlink listen:PORT`, `--tlink connect:HOST:PORT`), one datagram per frame, each tryte as a 16-bit little-endian integer. Either side can inject faults: flip chosen trits, or a trit with a given probability, and drop or duplicate frames.

## 2. The link layer: TL frames

A TL frame, in trytes:

| Trytes | Field |
|---|---|
| 3 | destination: a 27-trit station address (one word); 0 is broadcast |
| 3 | source |
| 1 | type: 1 TNP (§3); 2 link echo request, 3 link echo reply (the payload comes back unchanged); others are dropped |
| n | payload, 0 … 2178 trytes |
| 2 | check: TCRC-18 of everything before it |

**TCRC-18** is a cyclic redundancy check over GF(3). Eighteen trits of +1 and then the frame's trits, most significant first, are read as a polynomial, multiplied by x^18 and divided by

  g(x) = x^18 + x^13 + x − 1,

and the 18-trit remainder, balanced, is the check (two trytes, the higher first). The +1 start plays the role of a binary CRC's all-ones initial value: without it a zero tryte added in front of a frame (a broadcast address is all zeros) would not change the check.

A receiver drops a frame shorter than 9 trytes, one whose check fails, one for another station (neither its address nor 0), and one of an unknown type, and counts each. g is primitive over GF(3) (the order of x is 3^18 − 1), so TCRC-18 detects every error in one or two trits of a frame, every burst of at most 18 trits, and all but 3^−18 of the rest. Over GF(3) a single wrong trit is caught whether it is off by one level or by two.

A station address and the TNP host address are the same 27-trit number: on a link there is nothing to resolve.

## 3. The network: TNP

A TNP packet is the payload of a TL frame of type 1:

| Trytes | Field |
|---|---|
| 3 | destination host address |
| 3 | source host address |
| 1 | protocol: 1 TSP (§4.1), 2 TDP (§4.2), 3 TCMP (§3.1) |
| 1 | hop limit: 27 when sent; a router lowers it and drops the packet at 0 |
| 1 | payload length, 0 … 2169 |
| n | payload |

- A **host address** is 27 trits: written as three trytes in decimal, dotted (`0.0.1`, `1.-3.0`). 0 is "any" in a socket address and never a host's; it is the broadcast station address on a link (§2).
- A receiver drops a packet shorter than its header, one whose length runs past the frame, one for another host, and one with hop limit 0 or an unknown protocol (answered by TCMP "unreachable"), and counts each. There are no routers yet: every host is on the link.
- TNP has no check of its own; the TL check covers each hop.

### 3.1 TCMP: control messages

| Trytes | Field |
|---|---|
| 1 | type: 1 echo request, 2 echo reply, 3 unreachable |
| 1 | for echo: an identifier; for unreachable: 1 no such protocol, 2 no such port |
| 1 | for echo: a sequence number |
| n | for echo: data, returned unchanged; for unreachable: the start of the packet that failed (its TNP header and up to 27 trytes) |

`tping ADDR` sends echo requests and prints the replies.

## 4. Transport

Ports are one tryte, 1 … 9841 (0 is "any" when binding). Below 243 (3^5) only root may bind.

### 4.1 TSP: streams

TSP is a reliable, ordered stream of trytes in both directions, like TCP, and its header is 10 trytes:

| Trytes | Field |
|---|---|
| 1 | source port |
| 1 | destination port |
| 3 | sequence number: of the first payload tryte (or of the opening or the closing) |
| 3 | acknowledgement number: the next tryte expected |
| 1 | control trits (below) |
| 1 | window: how many trytes the sender will accept, 0 … 9841 |

The control tryte's trits, counted from the least significant:

| Trit | Meaning |
|---|---|
| 1 | **connection**: +1 opening (TCP's SYN), −1 closing (FIN), 0 neither. One trit holds both, since a segment never opens and closes at once. |
| 2 | +1 the acknowledgement number is valid (ACK) |
| 3 | +1 reset (RST) |
| 4 … 9 | 0 |

- **Sequence numbers** are words and wrap modulo 3^27; "a before b" means that a − b, wrapped into the word's range, is negative. The opening and the closing each count as one tryte.
- **The connection** follows TCP's states (RFC 9293): opening, opening-acknowledged, established, closing in either order, time-wait for 2 seconds. An opening to a port nobody listens on is answered with a reset.
- **Retransmission**: a segment not acknowledged in time is sent again, with the timeout doubling each time (from 1 s, at most 27 s; 9 tries, then the connection fails with `ETIMEDOUT`).
- **Segments** carry at most 2159 trytes (a TL frame of 2187 less 9, 9 and 10 trytes of headers). A segment's length is the TNP length less 10.
- Any tryte is payload. A program that writes octets (0 … 255), as `ssh` does, has them carried as they are.

### 4.2 TDP: datagrams

A TDP header is the source and destination ports (2 trytes); the payload follows, at most 2167 trytes. A datagram for a port nobody has bound is answered with TCMP "port unreachable".

## 5. Sockets

- `socket(AF_TNP, SOCK_STREAM, 0)` is TSP, `SOCK_DGRAM` TDP, and `SOCK_RAW` with protocol 3 TCMP (root only). `AF_TNP` is 27.
- `struct sockaddr_tn { tn_family; tn_port; tn_addr; }`, one word each: the same size as `sockaddr_in`, so code that handles both checks the family.
- Every call works as for `AF_INET` (NETWORK.md §4), with the same errors; `read` and `write` move trytes of any value.
- **Names**: `/etc/hosts` holds TNP addresses next to IPv4 ones; a line's address decides its family (`0.0.2 setun-b`). The resolver in tlibc (`tn_aton`, `tn_ntoa`, and `host_lookup` returning the family) gives TNP addresses first when a name has both.
- `ssh`, `scp`, `sftp`, `nc` and `tping` use either family; `sshd` listens on both, and `init` starts it when only the ternary link is up.

### 5.1 How it is built

TSP runs on TCP's state machine (`kernel/net/tcp.tri`): `tcp_segment()` takes a segment of either family once its header is read, and each socket's family decides the header (`tsp.tri` builds and parses TSP's), the sequence arithmetic (for TSP plain word arithmetic, which wraps modulo 3^27 by itself) and the way out (`tnp_send`). Ports are separate per family, so TCP and TSP can each use port 22.

## 6. Post-quantum key exchange

**C4 (built).** `ssh` and `sshd` offer `mlkem768x25519-sha256` first, then `curve25519-sha256`; the client's order decides (RFC 4253 §7.1). The exchange is OpenSSH's: the client sends its ML-KEM-768 encapsulation key (1184 octets) and its X25519 key (32); the server checks the encapsulation key (FIPS 203 §7.2), encapsulates, and returns the ciphertext (1088) and its X25519 key; K = SHA-256(ML-KEM key ‖ X25519 key), encoded as a string. sshd logs which exchange it made.

- `user/lib/sha3.tri`: SHA3-256, SHA3-512, SHAKE128 and SHAKE256 (FIPS 202). Keccak's 64-bit lanes are pairs of 32-bit halves, as SHA-512's words are.
- `user/lib/mlkem.tri`: ML-KEM-768 (FIPS 203) with plain arithmetic modulo 3329 (no Montgomery tricks; a word holds every product). Each operation takes about a second of emulated time.
- The reference is `tools/mlkem.py`, written from the standard; `tools/check_mlkem.py` checks it against OpenSSL 3.5 (key generation from the seed, and decapsulation of OpenSSL's ciphertexts), and `tools/gen_pqvectors.py` turns it into the vectors of `/bin/pqtest`.
- Tests: `/bin/pqtest` (SHA-3 and SHAKE at every block edge, ML-KEM key generation, encapsulation, decapsulation, implicit rejection, the key check); the host's OpenSSH logs in to Trinux's sshd with the hybrid forced, with curve25519 forced, with its own default (it picks the hybrid) and with its order reversed (it gets curve25519); and Gate C's two Setun machines agree on the hybrid over TNET.

**C5 (built).** `sntrup761x25519-sha512` (also under its `@openssh.com` name) comes second in the list. Its client sends an sntrup761 public key (1158 octets) and an X25519 key; the server encapsulates and returns the ciphertext (1039) and its X25519 key; K = SHA-512(sntrup key ‖ X25519 key), and the exchange hash and the key derivation are SHA-512 too, so the session id is 64 octets (and a public-key login signs over all 64).

- `user/lib/sntrup.tri`: Streamlined NTRU Prime in the ring Z[x]/(x^761 − x − 1), q = 4591. Its small polynomials are balanced trits: coefficients −1, 0, +1, exactly 286 of them non-zero in a short one. Key generation inverts g modulo 3 and 3f modulo q with the reference code's divstep iteration (about 15 s emulated); encapsulation takes about 2 s, decapsulation about 4.
- `tools/sntrup761.py` is the reference, written from the specification; its inverses use the extended Euclidean algorithm instead, so the vectors of `/bin/pqtest` check two methods against each other. OpenSSL has no sntrup761, so the check against another implementation is the handshake with OpenSSH: forced under both names, and with a public-key login over the 64-octet session id. `tools/check_sntrup761.py` checks the reference's own round trips.
- `ssh -o KexAlgorithms=LIST` chooses what the Trinux client offers; between the two Setun machines of Gate C the client makes all three exchanges, so its sntrup761 key generation and decapsulation are tested too.

## 7. Conformance

- **The vectors** are `crates/net/tests/tnet_vectors.txt`, made by `tools/gen_tnetvectors.py` from a model of this document (polynomial long division over GF(3), a trit at a time, sharing no code with any implementation): TCRC-18 of edge and random messages, whole TL frames, and corrupted frames that must fail the check. Lines: `crc T… = C`, `frame D S Y : P… = F…`, `bad F…`.
- **Exchanges** (`ex NAME : F => G ; H …`): a frame given to a station (0.0.1, listening on no port) and exactly the frames it must send back, or none. They cover link echoes (to the station and to broadcast), TCMP echo, "unreachable" for an unknown protocol and for a closed TDP port (quoting the TNP header and 27 trytes), TSP's resets to a closed port (for an opening, with sequence numbers at both ends of the word's range; for an acknowledgement; for data with a closing), and the frames that must be dropped silently: a reply, another station, an unknown type, a bad check, a short frame, hop limit 0, another host, a TNP length past the frame, a short TNP, TCMP or TSP header, a reset, an acknowledgement trit of −1, control trits beyond the third, and port 0.
- They are checked against the host's implementation (`setun-net`, the `crc`, `frame` and `bad` lines), and the exchanges against a booted Trinux (`crates/trinux/tests/tnet.rs`).
