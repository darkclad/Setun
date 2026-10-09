#!/usr/bin/env python3
"""TNET conformance vectors (docs/TNET.md §7): TCRC-18 checks and TL frames,
from an independent model of the specification (plain polynomial long
division over GF(3), one trit at a time).

    python tools/gen_tnetvectors.py > crates/net/tests/tnet_vectors.txt
"""
import random
import sys

T9 = 3 ** 9
# g(x) = x^18 + x^13 + x - 1, coefficients from x^18 down to x^0
G = [1] + [0] * 4 + [1] + [0] * 11 + [1, -1]
assert len(G) == 19

def wrapn(v, n):
    m = 3 ** n
    h = (m - 1) // 2
    return (v + h) % m - h

def trits(v, n):
    out = []
    for _ in range(n):
        r = v % 3
        d = -1 if r == 2 else r
        out.append(d)
        v = (v - d) // 3
    assert v == 0
    return out[::-1]

def untrits(t):
    v = 0
    for d in t:
        v = v * 3 + d
    return v

def m3(v):
    """A GF(3) element as -1, 0, +1."""
    return (v + 1) % 3 - 1

INIT = [9841, 9841]                   # 18 trits of +1: the initial value

def tcrc18(trytes):
    """The remainder of (18 trits of +1, then the message's trits, most
    significant first) · x^18 divided by g, as 18 balanced trits, most
    significant first, then as a number. The +1 start means leading zero
    trytes change the check."""
    bits = []
    for t in INIT + list(trytes):
        bits += trits(t, 9)
    poly = bits + [0] * 18
    for i in range(len(poly) - 18):
        c = poly[i]
        if c:
            for j in range(19):
                poly[i + j] = m3(poly[i + j] - c * G[j])
    rem = poly[-18:]
    return untrits(rem)

def check_trytes(trytes):
    c = tcrc18(trytes)
    t = trits(c, 18)
    return [untrits(t[:9]), untrits(t[9:])]

def word3(v):
    t = trits(v, 27)
    return [untrits(t[0:9]), untrits(t[9:18]), untrits(t[18:27])]

def frame(dst, src, typ, payload):
    body = word3(dst) + word3(src) + [typ] + list(payload)
    return body + check_trytes(body)

rnd = random.Random(18)
LINES = []

def main():
    # TCRC-18 of short messages, edges and random
    msgs = [[], [0], [0, 0], [0, 0, 0], [1], [-1], [9841], [-9841], [1, 0], [0, 1], [0] * 27, [9841] * 9, list(range(-13, 14))]
    for k in range(20):
        msgs.append([rnd.randint(-9841, 9841) for _ in range(rnd.randint(1, 60))])
    for m in msgs:
        LINES.append('crc ' + ' '.join(map(str, m)) + (' ' if m else '') + '= ' + str(tcrc18(m)))
    # whole frames
    W = (3 ** 27 - 1) // 2
    for dst, src, typ, pl in [(1, 2, 1, []), (0, 5, 2, [1, 2, 3]), (W, -W, 1, [9841, -9841, 0]),
                               (3 ** 20, 7, 3, [rnd.randint(-9841, 9841) for _ in range(100)])]:
        f = frame(dst, src, typ, pl)
        LINES.append(f'frame {dst} {src} {typ} : ' + ' '.join(map(str, pl)) + (' ' if pl else '') + '= ' + ' '.join(map(str, f)))
    # corrupted frames, all of which must fail the check: one trit off by
    # one level or two, at every 7th trit of a frame
    f = frame(1, 2, 2, [10, 20, 30])
    # a zero tryte put in front of a broadcast frame is caught too
    z = frame(0, 4, 2, [1])
    LINES.append('bad 0 ' + ' '.join(map(str, z)))
    LINES.append('bad ' + ' '.join(map(str, z[1:])))
    for pos in range(0, len(f) * 9, 7):
        for e in (1, -1):
            g = list(f)
            k, b = divmod(pos, 9)
            t = trits(g[k], 9)
            t[b] = m3(t[b] + e)
            g[k] = untrits(t)
            LINES.append('bad ' + ' '.join(map(str, g)))
    sys.stdout.reconfigure(encoding='utf-8', newline='\n')
    print('# Setun-11̅ TNET conformance vectors (docs/TNET.md §7), made by')
    print('# tools/gen_tnetvectors.py from an independent model of the specification.')
    print('#   crc T... = C              TCRC-18 of the trytes T is the 18-trit number C')
    print('#   frame D S Y : P... = F... a TL frame to D from S of type Y with payload P')
    print('#   bad F...                  a frame whose check fails')
    for l in LINES:
        print(l)
    print(f'# {len(LINES)} vectors', file=sys.stderr)

main()
