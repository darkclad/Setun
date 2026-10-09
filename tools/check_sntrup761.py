#!/usr/bin/env python3
"""Self-checks of tools/sntrup761.py: encodings round-trip, the ring
inverses are inverses, and the KEM gives both sides the same key. (The
check against another implementation is the handshake with OpenSSH in
crates/trinux/tests/ssh.rs: there is no sntrup761 in OpenSSL.)"""
import os, random, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from sntrup761 import *

rnd = random.Random(761)

def small():
    return [rnd.choice((-1, 0, 1)) for _ in range(P)]

def short():
    f = [1 if rnd.random() < 0.5 else -1 for _ in range(W)] + [0] * (P - W)
    rnd.shuffle(f)
    return f

for _ in range(3):
    h = [rnd.randint(-Q12, Q12) for _ in range(P)]
    s = rq_encode(h)
    assert len(s) == RQ_BYTES and rq_decode(s) == h
    c = round3(h)
    s = rounded_encode(c)
    assert len(s) == ROUNDED_BYTES and rounded_decode(s) == c
    f = small()
    assert small_decode(small_encode(f)) == f and len(small_encode(f)) == SMALL_BYTES
print('encodings round-trip')

g = small()
while poly_inverse(g, 3) is None:
    g = small()
gi = poly_inverse(g, 3)
assert mult(g, [f3(x) for x in gi], f3) == [1] + [0] * (P - 1)
f = short()
fi = poly_inverse([3 * x for x in f], Q)
assert mult([3 * x for x in f], [fq(x) for x in fi], fq) == [1] + [0] * (P - 1)
print('inverses mod 3 and mod q')

ok = 0
for _ in range(3):
    g = small()
    while poly_inverse(g, 3) is None:
        g = small()
    pk, sk = keygen(g, short(), bytes(rnd.randrange(256) for _ in range(SMALL_BYTES)))
    assert len(pk) == PK_BYTES and len(sk) == SK_BYTES
    k1, ct = encaps(pk, short())
    assert len(ct) == CT_BYTES
    assert decaps(sk, ct) == k1
    bad = bytearray(ct); bad[5] ^= 1
    assert decaps(sk, bytes(bad)) != k1
    ok += 1
print(f'{ok} key exchanges agree; a changed ciphertext is rejected')
