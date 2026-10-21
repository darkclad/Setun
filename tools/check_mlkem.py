"""Checks tools/mlkem.py against OpenSSL (3.5 or later): from the seed of
a key OpenSSL makes, keygen here gives the same encapsulation key, and
OpenSSL's encapsulation to it decapsulates here to OpenSSL's secret.

    python tools/check_mlkem.py
"""
import os, re, subprocess, sys, tempfile
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from mlkem import keygen, decaps, encaps

def run(*a):
    return subprocess.run(a, capture_output=True, check=True).stdout

def hexblock(text, label):
    m = re.search(label + r':\s*\n((?:\s+[0-9a-f:]+\n)+)', text)
    return bytes.fromhex(''.join(m.group(1).split()).replace(':', ''))

d = tempfile.mkdtemp()
ok = 0
for trial in range(5):
    key, pub, ct, ss = (os.path.join(d, n) for n in ('k.pem', 'p.pem', 'ct.bin', 'ss.bin'))
    run('openssl', 'genpkey', '-algorithm', 'ML-KEM-768', '-out', key)
    text = run('openssl', 'pkey', '-in', key, '-text', '-noout').decode().replace(chr(13), '')
    seed = hexblock(text, 'seed')
    ek_ossl = hexblock(text, 'ek')
    ek, dk = keygen(seed[:32], seed[32:])
    assert ek == ek_ossl, 'encapsulation key differs'
    run('openssl', 'pkey', '-in', key, '-pubout', '-out', pub)
    run('openssl', 'pkeyutl', '-encap', '-inkey', pub, '-pubin', '-out', ct, '-secret', ss)
    c = open(ct, 'rb').read()
    s = open(ss, 'rb').read()
    assert decaps(dk, c) == s, 'shared secret differs'
    # and a corrupted ciphertext gives the implicit rejection key, not s
    bad = bytearray(c); bad[0] ^= 1
    assert decaps(dk, bytes(bad)) != s
    ok += 1
print(f'{ok} OpenSSL keys: keygen and decaps agree')
