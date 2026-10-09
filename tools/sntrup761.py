#!/usr/bin/env python3
"""sntrup761 (Streamlined NTRU Prime, the NIST round 3 submission's
parameter set used by OpenSSH), written from the specification as a
reference for Trinux's user/lib/sntrup.tri. Only hashlib is used.

    from sntrup761 import keygen, encaps, decaps
    keygen(g_seed, f_seed, rho) -> (pk, sk)     deterministic from the inputs
    encaps(pk, r) -> (key, ct)                  r: a short polynomial
    decaps(sk, ct) -> key

The ring is Z[x]/(x^p - x - 1) with p = 761, q = 4591, w = 286. Small
polynomials have coefficients -1, 0, 1; short ones exactly w nonzero.
"""
import hashlib

P, Q, W = 761, 4591, 286
Q12 = (Q - 1) // 2
SMALL_BYTES = (P + 3) // 4           # 191
RQ_BYTES = 1158
ROUNDED_BYTES = 1007
HASH_BYTES = 32
PK_BYTES = RQ_BYTES
CT_BYTES = ROUNDED_BYTES + HASH_BYTES  # 1039
SK_BYTES = 2 * SMALL_BYTES + PK_BYTES + SMALL_BYTES + HASH_BYTES  # 1763

def fq(x):
    """x modulo q, as -q12 .. q12."""
    return (x + Q12) % Q - Q12

def f3(x):
    return (x + 1) % 3 - 1

# ---- the ring ----

def mult(f, g, mod):
    """f · g in Z_mod[x]/(x^p - x - 1)."""
    h = [0] * (2 * P - 1)
    for i, a in enumerate(f):
        if a:
            for j, b in enumerate(g):
                if b:
                    h[i + j] += a * b
    for i in range(2 * P - 2, P - 1, -1):
        h[i - P] += h[i]
        h[i - P + 1] += h[i]
    return [mod(x) for x in h[:P]]

def poly_inverse(f, m):
    """The inverse of f in GF(m)[x]/(x^p - x - 1), or None."""
    # extended Euclid on polynomials, lowest coefficient first
    def trim(a):
        while a and a[-1] % m == 0:
            a.pop()
        return a
    def sub_mul(a, b, c, shift):
        # a - c · x^shift · b
        a = a + [0] * max(0, len(b) + shift - len(a))
        for i, x in enumerate(b):
            a[i + shift] = (a[i + shift] - c * x) % m
        return trim(a)
    modulus = [m - 1, m - 1] + [0] * (P - 2) + [1]   # x^p - x - 1
    r0, r1 = trim([x % m for x in modulus]), trim([x % m for x in f])
    s0, s1 = [], [1]
    while r1:
        while len(r0) >= len(r1) and r0:
            c = r0[-1] * pow(r1[-1], m - 2, m) % m
            shift = len(r0) - len(r1)
            r0 = sub_mul(r0, r1, c, shift)
            s0 = sub_mul(s0, s1, c, shift)
        r0, r1 = r1, r0
        s0, s1 = s1, s0
    if len(r0) != 1:
        return None
    inv = pow(r0[0], m - 2, m)
    s = [x * inv % m for x in s0] + [0] * P
    # reduce modulo x^p - x - 1
    for i in range(len(s) - 1, P - 1, -1):
        if s[i]:
            s[i - P] = (s[i - P] + s[i]) % m
            s[i - P + 1] = (s[i - P + 1] + s[i]) % m
            s[i] = 0
    return s[:P]

# ---- encodings ----

def encode(R, M):
    """The NTRU Prime Encode: values R[i] in 0 .. M[i]-1 into octets."""
    out = bytearray()
    if len(M) == 1:
        r, m = R[0], M[0]
        while m > 1:
            out.append(r & 255)
            r >>= 8
            m = (m + 255) >> 8
        return bytes(out)
    R2, M2 = [], []
    i = 0
    while i < len(M) - 1:
        m0 = M[i]
        r = R[i] + R[i + 1] * m0
        m = M[i + 1] * m0
        while m >= 16384:
            out.append(r & 255)
            r >>= 8
            m = (m + 255) >> 8
        R2.append(r)
        M2.append(m)
        i += 2
    if i < len(M):
        R2.append(R[i])
        M2.append(M[i])
    return bytes(out) + encode(R2, M2)

def decode(s, M):
    """The inverse of encode."""
    if len(M) == 1:
        if M[0] == 1:
            return [0]
        r, k, m = 0, 0, M[0]
        while m > 1:
            r |= s[k] << (8 * k)
            k += 1
            m = (m + 255) >> 8
        return [r % M[0]] if M[0] <= 65536 else [r]
    # the pairs' moduli and how many octets each pair emits
    M2, emits = [], []
    i = 0
    while i < len(M) - 1:
        m = M[i] * M[i + 1]
        e = 0
        while m >= 16384:
            m = (m + 255) >> 8
            e += 1
        M2.append(m)
        emits.append(e)
        i += 2
    if i < len(M):
        M2.append(M[i])
    head = sum(emits)
    R2 = decode(s[head:], M2)
    R = []
    pos = 0
    i = 0
    for j, e in enumerate(emits):
        r = R2[j]
        for b in range(e - 1, -1, -1):
            r = (r << 8) | s[pos + b]
        pos += e
        m0 = M[2 * j]
        R.append(r % m0)
        R.append((r // m0) % M[2 * j + 1])
    if len(M) % 2:
        R.append(R2[-1])
    return R

def small_encode(f):
    out = bytearray()
    for i in range(P // 4):
        x = sum((f[4 * i + j] + 1) << (2 * j) for j in range(4))
        out.append(x)
    out.append(f[P - 1] + 1)
    return bytes(out)

def small_decode(s):
    f = []
    for i in range(P // 4):
        x = s[i]
        for j in range(4):
            f.append(((x >> (2 * j)) & 3) - 1)
    f.append((s[P // 4] & 3) - 1)
    return f

def rq_encode(h):
    return encode([x + Q12 for x in h], [Q] * P)

def rq_decode(s):
    return [x - Q12 for x in decode(s, [Q] * P)]

def rounded_encode(c):
    return encode([(x + Q12) * 10923 >> 15 for x in c], [(Q + 2) // 3] * P)

def rounded_decode(s):
    return [x * 3 - Q12 for x in decode(s, [(Q + 2) // 3] * P)]

# ---- the scheme ----

def hash_prefix(b, data):
    return hashlib.sha512(bytes([b]) + data).digest()[:HASH_BYTES]

def round3(a):
    """Each coefficient to the nearest multiple of 3."""
    return [x - f3(x) for x in a]

def weight(f):
    return sum(1 for x in f if x)

def keygen_core(g, f):
    """(h, ginv) for small g (invertible mod 3) and short f."""
    ginv = poly_inverse(g, 3)
    assert ginv is not None, 'g not invertible mod 3'
    ginv = [f3(x) for x in ginv]
    finv3 = poly_inverse([3 * x for x in f], Q)
    finv3 = [fq(x) for x in finv3]
    h = mult(finv3, g, fq)
    return h, ginv

def keygen(g, f, rho):
    """pk, sk from small g, short f and 191 random octets rho."""
    h, ginv = keygen_core(g, f)
    pk = rq_encode(h)
    sk = small_encode(f) + small_encode(ginv) + pk + rho + hash_prefix(4, pk)
    return pk, sk

def hash_confirm(r_enc, cache):
    return hash_prefix(2, hash_prefix(3, r_enc) + cache)

def hash_session(b, r_enc, ct):
    return hash_prefix(b, hash_prefix(3, r_enc) + ct)

def hide(r, pk, cache):
    r_enc = small_encode(r)
    h = rq_decode(pk)
    c = round3(mult(h, r, fq))
    ct = rounded_encode(c) + hash_confirm(r_enc, cache)
    return r_enc, ct

def encaps(pk, r):
    assert weight(r) == W
    r_enc, ct = hide(r, pk, hash_prefix(4, pk))
    return hash_session(1, r_enc, ct), ct

def decaps(sk, ct):
    f = small_decode(sk[:SMALL_BYTES])
    ginv = small_decode(sk[SMALL_BYTES:2 * SMALL_BYTES])
    pk = sk[2 * SMALL_BYTES:2 * SMALL_BYTES + PK_BYTES]
    rho = sk[2 * SMALL_BYTES + PK_BYTES:3 * SMALL_BYTES + PK_BYTES]
    cache = sk[3 * SMALL_BYTES + PK_BYTES:]
    c = rounded_decode(ct[:ROUNDED_BYTES])
    cf3 = [fq(3 * x) for x in mult(c, f, fq)]
    e = [f3(x) for x in cf3]
    ev = mult(e, ginv, f3)
    if weight(ev) == W:
        r = ev
    else:
        r = [1] * W + [0] * (P - W)
    r_enc, cnew = hide(r, pk, cache)
    if cnew == ct:
        return hash_session(1, r_enc, ct)
    return hash_session(0, rho, ct)
