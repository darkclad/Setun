#!/usr/bin/env python3
"""Vectors for /bin/pqtest: SHA-3 and SHAKE (from Python's hashlib) and
ML-KEM-768 (from tools/mlkem.py, itself checked against OpenSSL by
tools/check_mlkem.py). Prints Trine source to paste into user/bin/pqtest.tri
between the "vectors" markers.

    python tools/gen_pqvectors.py
"""
import hashlib
import sys
sys.path.insert(0, __file__.rsplit('/', 1)[0] if '/' in __file__ else '.')
from mlkem import keygen, encaps, decaps
import sntrup761 as sn
import random

def msg(n, seed=3):
    return bytes((i * 7 + seed) % 256 for i in range(n))

def h(b):
    return hashlib.sha3_256(b).hexdigest()

out = []
out.append('// ---- vectors (tools/gen_pqvectors.py) ----')
lens = [0, 1, 3, 71, 72, 73, 135, 136, 137, 167, 168, 169, 300]
out.append(f'var sha3_lens: [{len(lens)}]int = {{ {", ".join(map(str, lens))} }};')
for name, f in [('sha3_256', lambda b: hashlib.sha3_256(b).hexdigest()), ('sha3_512', lambda b: hashlib.sha3_512(b).hexdigest()),
                ('shake128', lambda b: hashlib.shake_128(b).hexdigest(200)), ('shake256', lambda b: hashlib.shake_256(b).hexdigest(64))]:
    vals = [f(msg(n)) for n in lens]
    out.append(f'var {name}_want: [{len(lens)}]*char = {{')
    for v in vals:
        out.append(f'    "{v}",')
    out.append('};')
# ML-KEM-768: two key pairs, an encapsulation each, and a corrupted ciphertext
cases = []
for k in range(2):
    d, z, m = msg(32, 11 + k), msg(32, 41 + k), msg(32, 71 + k)
    ek, dk = keygen(d, z)
    key, ct = encaps(ek, m)
    assert decaps(dk, ct) == key
    bad = bytearray(ct); bad[100] ^= 4
    rej = decaps(dk, bytes(bad))
    assert rej != key
    cases.append((11 + k, 41 + k, 71 + k, h(ek), h(dk), h(ct), key.hex(), rej.hex()))
out.append(f'var mlkem_cases: [{len(cases) * 8}]*char = {{')
for c in cases:
    out.append(f'    "{c[0]}", "{c[1]}", "{c[2]}", "{c[3]}",')
    out.append(f'    "{c[4]}", "{c[5]}",')
    out.append(f'    "{c[6]}", "{c[7]}",')
out.append('};')
# sntrup761: deterministic g, f, rho and r from a seeded generator; the
# inputs as their encodings, the outputs as SHA3-256 hashes (and the keys)
rnd = random.Random(761)
def small():
    return [rnd.choice((-1, 0, 1)) for _ in range(sn.P)]
def short():
    f = [1 if rnd.random() < 0.5 else -1 for _ in range(sn.W)] + [0] * (sn.P - sn.W)
    rnd.shuffle(f)
    return f
sncases = []
for k in range(2):
    g = small()
    while sn.poly_inverse(g, 3) is None:
        g = small()
    f, r = short(), short()
    rho = bytes(rnd.randrange(256) for _ in range(sn.SMALL_BYTES))
    pk, sk = sn.keygen(g, f, rho)
    key, ct = sn.encaps(pk, r)
    assert sn.decaps(sk, ct) == key
    bad = bytearray(ct); bad[200] ^= 16
    rej = sn.decaps(sk, bytes(bad))
    if k == 0:
        sn_pk0 = pk
        sn_ginv0 = [sn.f3(x) for x in sn.poly_inverse(g, 3)]
    sncases.append((sn.small_encode(g).hex(), sn.small_encode(f).hex(), rho.hex(), sn.small_encode(r).hex(), h(pk), h(sk), h(ct), key.hex(), rej.hex()))
# the first public key itself, so the encodings and encapsulation can be
# checked apart from key generation; and 1/g mod 3 of the first g
out.append(f'var sntrup_pk0: [1]*char = {{ "{sn_pk0.hex()}" }};')
out.append(f'var sntrup_ginv0: [1]*char = {{ "{sn.small_encode(sn_ginv0).hex()}" }};')
out.append(f'var sntrup_cases: [{len(sncases) * 9}]*char = {{')
for c in sncases:
    for v in c:
        out.append(f'    "{v}",')
out.append('};')
out.append('// ---- end of vectors ----')
print('\n'.join(out))
