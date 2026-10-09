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
out.append('// ---- end of vectors ----')
print('\n'.join(out))
