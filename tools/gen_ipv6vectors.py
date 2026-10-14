#!/usr/bin/env python3
"""IPv6 conformance vectors (docs/IPV6.md §7), from an independent model of
the specification: plain Python on octet lists, nothing shared with the
kernel.

    python tools/gen_ipv6vectors.py > user/share/ipv6vectors.txt

Lines:
    text IN = OUT     inet_pton6 takes IN, and inet_ntop6 gives OUT (RFC 5952)
    bad IN            inet_pton6 refuses IN
    ex NAME : F => G ; H ...
                      the station, given Ethernet frame F (hex), sends exactly
                      G, H, ... (nothing after "=>": it drops F)

The station is Trinux after autoconfiguration: MAC 52:54:00:12:34:56,
fe80::5054:ff:fe12:3456 and fd00::5054:ff:fe12:3456. The peer is
fe80::77 / fd00::77 with MAC 02:00:00:00:00:77; before each exchange the
runner makes it a REACHABLE neighbour, so replies never wait on Neighbor
Discovery.
"""
import ipaddress
import sys

S_MAC = bytes.fromhex('525400123456')
P_MAC = bytes.fromhex('020000000077')
S_LL = ipaddress.IPv6Address('fe80::5054:ff:fe12:3456').packed
S_GL = ipaddress.IPv6Address('fd00::5054:ff:fe12:3456').packed
P_LL = ipaddress.IPv6Address('fe80::77').packed
P_GL = ipaddress.IPv6Address('fd00::77').packed
OTHER = ipaddress.IPv6Address('fd00::99').packed
ALLNODES = ipaddress.IPv6Address('ff02::1').packed
UNSPEC = bytes(16)
LOOP = ipaddress.IPv6Address('::1').packed

def a6(s):
    return ipaddress.IPv6Address(s).packed

def solicited(a):
    g = bytes.fromhex('ff02' + '00' * 9 + '01ff') + a[13:]
    assert len(g) == 16
    return g

def mcast_mac(a):
    return b'\x33\x33' + a[12:]

def csum(data):
    if len(data) % 2:
        data += b'\0'
    s = sum(int.from_bytes(data[i:i + 2], 'big') for i in range(0, len(data), 2))
    while s > 0xffff:
        s = (s & 0xffff) + (s >> 16)
    return 0xffff - s

def icmp(src, dst, msg, fix=True):
    """An ICMPv6 message with its checksum over the pseudo-header."""
    m = bytearray(msg)
    if fix:
        m[2:4] = b'\0\0'
        m[2:4] = csum(src + dst + len(m).to_bytes(4, 'big') + b'\0\0\0\x3a' + bytes(m)).to_bytes(2, 'big')
    return bytes(m)

def ip6(src, dst, nh, payload, hop=64, plen=None, version=6):
    assert len(src) == 16 and len(dst) == 16
    return bytes([version << 4, 0, 0, 0]) + (len(payload) if plen is None else plen).to_bytes(2, 'big') + bytes([nh, hop]) + src + dst + payload

def eth(dst_mac, src_mac, packet, ethertype=0x86dd):
    return dst_mac + src_mac + ethertype.to_bytes(2, 'big') + packet

def to_station(src, dst, nh, payload, hop=64, **kw):
    dmac = mcast_mac(dst) if dst[0] == 0xff else S_MAC
    return eth(dmac, P_MAC, ip6(src, dst, nh, payload, hop, **kw))

def from_station(src, dst, nh, payload, hop=64):
    dmac = mcast_mac(dst) if dst[0] == 0xff else P_MAC
    return eth(dmac, S_MAC, ip6(src, dst, nh, payload, hop))

def echo(ident=0x77, seq=1, data=b'conformance', type_=128):
    return bytes([type_, 0, 0, 0]) + ident.to_bytes(2, 'big') + seq.to_bytes(2, 'big') + data

def echo_reply_to(src, dst, msg):
    """What the station answers an echo request from src (to its dst)."""
    me = dst if dst in (S_LL, S_GL) else (S_LL if src[0] == 0xfe else S_GL)
    r = bytearray(msg)
    r[0] = 129
    return from_station(me, src, 58, icmp(me, src, bytes(r)))

def param_problem(packet, code, ptr):
    """The station's Parameter Problem about the IPv6 packet it got."""
    src, dst = packet[8:24], packet[24:40]
    me = dst if dst in (S_LL, S_GL) else (S_LL if src[0] == 0xfe else S_GL)
    quote = packet[:1280 - 48]
    msg = bytes([4, code, 0, 0]) + ptr.to_bytes(4, 'big') + quote
    return from_station(me, src, 58, icmp(me, src, msg))

def opts_header(nh, options):
    """A Hop-by-Hop or Destination Options header around `options` (padded
    with PadN to a multiple of 8)."""
    body = bytes(options)
    total = 2 + len(body)
    pad = (-total) % 8
    if pad == 1:
        body += b'\0'
    elif pad:
        body += bytes([1, pad - 2]) + bytes(pad - 2)
    return bytes([nh, (2 + len(body)) // 8 - 1]) + body

def lla(type_, mac):
    return bytes([type_, 1]) + mac

def ns(target, options=b''):
    return bytes([135, 0, 0, 0, 0, 0, 0, 0]) + target + options

def na(target, flags, options=b''):
    return bytes([136, 0, 0, 0, flags, 0, 0, 0]) + target + options

def station_na(dst, target, solicited):
    msg = na(target, 0x60 if solicited else 0x20, lla(2, S_MAC))
    return from_station(target, dst, 58, icmp(target, dst, msg), hop=255)

LINES = []

def ex(name, frame, replies):
    LINES.append(f'ex {name} : {frame.hex()} => ' + ' ; '.join(r.hex() for r in replies))

def main():
    # ---- RFC 5952 text --------------------------------------------------------------
    for t, want in [
        ('::', '::'), ('::1', '::1'), ('1::', '1::'), ('fd00::2', 'fd00::2'),
        ('FD00:0:0:0:0:0:0:2', 'fd00::2'), ('2001:db8:0:0:1:0:0:1', '2001:db8::1:0:0:1'),
        ('2001:0db8:0000:0000:0000:ff00:0042:8329', '2001:db8::ff00:42:8329'),
        ('2001:db8:0:1:1:1:1:1', '2001:db8:0:1:1:1:1:1'),      # a lone zero group stays
        ('2001:db8::1:1:1:1:1', '2001:db8:0:1:1:1:1:1'),       # "::" for one group, accepted
        ('1:0:0:2:0:0:0:3', '1:0:0:2::3'), ('1:0:0:2:0:0:3:4', '1::2:0:0:3:4'),   # longest, then first
        ('fe80::5054:ff:fe12:3456', 'fe80::5054:ff:fe12:3456'),
        ('::ffff:1.2.3.4', '::ffff:102:304'), ('::1.2.3.4', '::102:304'),
        ('ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff', 'ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff'),
        ('0:0:0:0:0:0:0:0', '::'), ('a:b:c:d:e:f:0:0', 'a:b:c:d:e:f::'),
    ]:
        assert ipaddress.IPv6Address(t).compressed == want or t.startswith('::ffff:') or t == '::1.2.3.4'
        LINES.append(f'text {t} = {want}')
    for b in ['', ':', ':::', '1:::2', '1::2::3', ':1', '1:', '1:2:3:4:5:6:7', '1:2:3:4:5:6:7:8:9',
              '12345::', 'g::', '1::2:3:4:5:6:7:8', '::1.2.3', '::1.2.3.4.5', '::256.1.1.1',
              '1.2.3.4', 'fd00::2 ', ' ::1', '[::1]', 'fd00::2%eth0']:
        try:
            ipaddress.IPv6Address(b)
            ok = True
        except ValueError:
            ok = False
        assert not ok or b in ('1::2:3:4:5:6:7:8', 'fd00::2%eth0'), b   # Python takes these; inet_pton does not
        LINES.append(f'bad {b}')

    # ---- echo -----------------------------------------------------------------------
    for name, src, dst in [('echo-global', P_GL, S_GL), ('echo-link-local', P_LL, S_LL),
                           ('echo-all-nodes', P_GL, ALLNODES), ('echo-solicited-group', P_GL, solicited(S_GL))]:
        m = icmp(src, dst, echo())
        ex(name, to_station(src, dst, 58, m), [echo_reply_to(src, dst, m)])
    m = icmp(P_GL, S_GL, echo(data=bytes(range(256)) * 4))
    ex('echo-1032-octets', to_station(P_GL, S_GL, 58, m), [echo_reply_to(P_GL, S_GL, m)])
    m = icmp(P_GL, S_GL, echo(data=b''))
    ex('echo-empty', to_station(P_GL, S_GL, 58, m), [echo_reply_to(P_GL, S_GL, m)])
    m = icmp(P_GL, S_GL, echo())
    ex('echo-padding-after-payload', to_station(P_GL, S_GL, 58, m) + bytes(9), [echo_reply_to(P_GL, S_GL, m)])
    ex('echo-short', to_station(P_GL, S_GL, 58, icmp(P_GL, S_GL, bytes([128, 0, 0, 0, 0, 1]))), [])
    bad = bytearray(m); bad[2] ^= 1
    ex('echo-bad-checksum', to_station(P_GL, S_GL, 58, bytes(bad)), [])
    ex('echo-checksum-for-other-dst', to_station(P_GL, S_GL, 58, icmp(P_GL, OTHER, echo())), [])
    ex('echo-to-other-host', to_station(P_GL, OTHER, 58, icmp(P_GL, OTHER, echo())), [])
    ex('echo-from-multicast', to_station(ALLNODES, S_GL, 58, icmp(ALLNODES, S_GL, echo())), [])
    ex('echo-from-loopback', to_station(LOOP, S_GL, 58, icmp(LOOP, S_GL, echo())), [])
    ex('echo-from-unspecified', to_station(UNSPEC, S_GL, 58, icmp(UNSPEC, S_GL, echo())), [])
    ex('echo-to-loopback', to_station(P_GL, LOOP, 58, icmp(P_GL, LOOP, echo())), [])
    ex('echo-reply-ignored', to_station(P_GL, S_GL, 58, icmp(P_GL, S_GL, echo(type_=129))), [])
    ex('unknown-informational', to_station(P_GL, S_GL, 58, icmp(P_GL, S_GL, echo(type_=200))), [])
    ex('payload-past-frame', to_station(P_GL, S_GL, 58, m, plen=len(m) + 1), [])
    ex('version-4-in-ipv6', to_station(P_GL, S_GL, 58, m, version=4), [])
    ex('header-only-39', eth(S_MAC, P_MAC, ip6(P_GL, S_GL, 58, b'')[:39]), [])

    # ---- extension headers --------------------------------------------------------------
    pkt = ip6(P_GL, S_GL, 253, b'unknown')
    ex('unknown-next-header', eth(S_MAC, P_MAC, pkt), [param_problem(pkt, 1, 6)])
    pkt = ip6(P_GL, S_GL, 0, opts_header(253, []) + b'x')
    ex('unknown-after-hop-by-hop', eth(S_MAC, P_MAC, pkt), [param_problem(pkt, 1, 40)])
    pkt = ip6(P_GL, S_GL, 60, opts_header(0, []) + opts_header(58, []) + icmp(P_GL, S_GL, echo()))
    ex('hop-by-hop-not-first', eth(S_MAC, P_MAC, pkt), [param_problem(pkt, 1, 40)])
    for name, opts in [('pad1-padn', [0, 1, 2, 0, 0]), ('unknown-option-skip', [0x1e, 2, 9, 9])]:
        e = icmp(P_GL, S_GL, echo())
        pkt = ip6(P_GL, S_GL, 60, opts_header(58, opts) + e)
        ex(name, eth(S_MAC, P_MAC, pkt), [echo_reply_to(P_GL, S_GL, e)])
    e = icmp(P_GL, S_GL, echo())
    pkt = ip6(P_GL, S_GL, 0, opts_header(58, [0x1e, 0]) + e)
    ex('hop-by-hop-then-echo', eth(S_MAC, P_MAC, pkt), [echo_reply_to(P_GL, S_GL, e)])
    pkt = ip6(P_GL, S_GL, 60, opts_header(58, [0x5e, 0]) + e)
    ex('unknown-option-discard', eth(S_MAC, P_MAC, pkt), [])
    pkt = ip6(P_GL, S_GL, 60, opts_header(58, [1, 0, 0x9e, 0]) + e)
    ex('unknown-option-report', eth(S_MAC, P_MAC, pkt), [param_problem(pkt, 2, 44)])
    pkt = ip6(P_GL, S_GL, 60, opts_header(58, [0xde, 0]) + e)
    ex('unknown-option-report-unicast', eth(S_MAC, P_MAC, pkt), [param_problem(pkt, 2, 42)])
    em = icmp(P_GL, ALLNODES, echo())
    pkt = ip6(P_GL, ALLNODES, 60, opts_header(58, [0xde, 0]) + em)
    ex('unknown-option-quiet-multicast', eth(mcast_mac(ALLNODES), P_MAC, pkt), [])
    pkt = ip6(P_GL, ALLNODES, 60, opts_header(58, [0x9e, 0]) + em)
    ex('unknown-option-report-multicast', eth(mcast_mac(ALLNODES), P_MAC, pkt), [param_problem(pkt, 2, 42)])
    pkt = ip6(P_GL, S_GL, 60, bytes([58, 0, 0x1e, 9]) + bytes(4) + e)
    ex('option-past-header', eth(S_MAC, P_MAC, pkt), [])
    pkt = ip6(P_GL, S_GL, 60, bytes([58, 1]) + bytes(6))
    ex('header-past-packet', eth(S_MAC, P_MAC, pkt), [])
    rt0 = bytes([58, 0, 4, 0]) + bytes(4)                     # segments left 0
    pkt = ip6(P_GL, S_GL, 43, rt0 + e)
    ex('routing-done', eth(S_MAC, P_MAC, pkt), [echo_reply_to(P_GL, S_GL, e)])
    rt1 = bytes([58, 2, 4, 1]) + bytes(4) + OTHER              # segments left 1
    pkt = ip6(P_GL, S_GL, 43, rt1 + e)
    ex('routing-segments-left', eth(S_MAC, P_MAC, pkt), [param_problem(pkt, 0, 42)])
    pkt = ip6(P_GL, S_GL, 44, bytes([58, 0, 0, 1, 0, 0, 0, 7]) + e)
    ex('fragment-dropped', eth(S_MAC, P_MAC, pkt), [])
    ex('no-next-header', eth(S_MAC, P_MAC, ip6(P_GL, S_GL, 59, b'')), [])
    big = ip6(P_GL, S_GL, 253, bytes(1400))
    ex('error-quote-cut-at-1280', eth(S_MAC, P_MAC, big), [param_problem(big, 1, 6)])
    pkt = ip6(P_GL, ALLNODES, 253, b'x')
    ex('no-error-for-multicast', eth(mcast_mac(ALLNODES), P_MAC, pkt), [])
    pkt = ip6(UNSPEC, S_GL, 253, b'x')
    ex('no-error-to-unspecified', eth(S_MAC, P_MAC, pkt), [])

    # ---- Neighbor Discovery ----------------------------------------------------------------
    m = icmp(P_GL, S_GL, ns(S_GL, lla(1, P_MAC)))
    ex('ns-unicast', to_station(P_GL, S_GL, 58, m, hop=255), [station_na(P_GL, S_GL, True)])
    m = icmp(P_GL, solicited(S_GL), ns(S_GL, lla(1, P_MAC)))
    ex('ns-solicited-group', to_station(P_GL, solicited(S_GL), 58, m, hop=255), [station_na(P_GL, S_GL, True)])
    m = icmp(P_LL, solicited(S_LL), ns(S_LL, lla(1, P_MAC)))
    ex('ns-link-local', to_station(P_LL, solicited(S_LL), 58, m, hop=255), [station_na(P_LL, S_LL, True)])
    m = icmp(P_GL, S_GL, ns(S_GL))
    ex('ns-without-source-lla', to_station(P_GL, S_GL, 58, m, hop=255), [station_na(P_GL, S_GL, True)])
    m = icmp(UNSPEC, solicited(S_GL), ns(S_GL))
    ex('ns-dad-defended', to_station(UNSPEC, solicited(S_GL), 58, m, hop=255), [station_na(ALLNODES, S_GL, False)])
    m = icmp(P_GL, S_GL, ns(S_GL, lla(1, P_MAC)))
    ex('ns-hop-254', to_station(P_GL, S_GL, 58, m, hop=254), [])
    mc = bytearray(icmp(P_GL, S_GL, ns(S_GL, lla(1, P_MAC)), fix=False)); mc[1] = 1
    ex('ns-code-1', to_station(P_GL, S_GL, 58, icmp(P_GL, S_GL, bytes(mc)), hop=255), [])
    m = icmp(P_GL, S_GL, ns(ALLNODES, lla(1, P_MAC)))
    ex('ns-multicast-target', to_station(P_GL, S_GL, 58, m, hop=255), [])
    m = icmp(P_GL, solicited(OTHER), ns(OTHER, lla(1, P_MAC)))
    ex('ns-other-target', to_station(P_GL, solicited(OTHER), 58, m, hop=255), [])
    m = icmp(UNSPEC, solicited(S_GL), ns(S_GL, lla(1, P_MAC)))
    ex('ns-dad-with-source-lla', to_station(UNSPEC, solicited(S_GL), 58, m, hop=255), [])
    m = icmp(UNSPEC, S_GL, ns(S_GL))
    ex('ns-dad-to-unicast', to_station(UNSPEC, S_GL, 58, m, hop=255), [])
    m = icmp(P_GL, S_GL, ns(S_GL, bytes([1, 0]) + P_MAC))
    ex('ns-option-length-0', to_station(P_GL, S_GL, 58, m, hop=255), [])
    m = icmp(P_GL, S_GL, ns(S_GL, bytes([1, 2]) + P_MAC))
    ex('ns-option-past-end', to_station(P_GL, S_GL, 58, m, hop=255), [])
    m = icmp(P_GL, S_GL, ns(S_GL)[:20])
    ex('ns-short', to_station(P_GL, S_GL, 58, m, hop=255), [])
    m = icmp(P_GL, S_GL, ns(S_GL, bytes([14, 1, 0, 0, 0, 0, 0, 0]) + lla(1, P_MAC)))
    ex('ns-unknown-option-ignored', to_station(P_GL, S_GL, 58, m, hop=255), [station_na(P_GL, S_GL, True)])
    m = icmp(P_LL, a6('ff02::2'), bytes([133, 0, 0, 0, 0, 0, 0, 0]) + lla(1, P_MAC))
    ex('rs-ignored-by-a-host', to_station(P_LL, a6('ff02::2'), 58, m, hop=255), [])
    m = icmp(P_LL, S_LL, bytes([137, 0, 0, 0, 0, 0, 0, 0]) + OTHER + P_GL)
    ex('redirect-ignored', to_station(P_LL, S_LL, 58, m, hop=255), [])
    m = icmp(P_GL, ALLNODES, na(P_GL, 0x20, lla(2, P_MAC)))
    ex('na-unsolicited', to_station(P_GL, ALLNODES, 58, m, hop=255), [])
    m = icmp(P_GL, ALLNODES, na(P_GL, 0x60, lla(2, P_MAC)))
    ex('na-solicited-to-multicast', to_station(P_GL, ALLNODES, 58, m, hop=255), [])
    m = icmp(P_GL, S_GL, bytes([134, 0, 0, 0, 64, 0, 7, 8]) + bytes(8))
    ex('ra-from-global-ignored', to_station(P_GL, S_GL, 58, m, hop=255), [])

    sys.stdout.reconfigure(encoding='utf-8', newline='\n')
    print('# Setun-11̅ IPv6 conformance vectors (docs/IPV6.md §7), made by')
    print('# tools/gen_ipv6vectors.py from an independent model of the specification.')
    print('#   text IN = OUT              inet_pton6 takes IN, inet_ntop6 gives OUT')
    print('#   bad IN                     inet_pton6 refuses IN')
    print('#   ex NAME : F => G ; H ...   the station, given Ethernet frame F (hex), sends')
    print('#                              exactly G, H, ... (none after "=>": drops F)')
    for l in LINES:
        print(l)
    print(f'# {len(LINES)} vectors', file=sys.stderr)

main()
