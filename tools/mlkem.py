#!/usr/bin/env python3
"""ML-KEM-768 (FIPS 203), written from the standard as a reference for
Trinux's user/lib/mlkem.tri. Only hashlib (SHA3, SHAKE) is used.

    from mlkem import keygen, encaps, decaps
"""
import hashlib

N, Q, K, ETA1, ETA2, DU, DV = 256, 3329, 3, 2, 2, 10, 4
ZETA = 17

def bitrev7(i):
    return int(f'{i:07b}'[::-1], 2)

ZETAS = [pow(ZETA, bitrev7(i), Q) for i in range(128)]
GAMMAS = [pow(ZETA, 2 * bitrev7(i) + 1, Q) for i in range(128)]

def H(b): return hashlib.sha3_256(b).digest()
def G(b):
    d = hashlib.sha3_512(b).digest()
    return d[:32], d[32:]
def J(b): return hashlib.shake_256(b).digest(32)
def PRF(eta, s, b): return hashlib.shake_256(s + bytes([b])).digest(64 * eta)

# ---- encoding (§4.2.1) ----

def byte_encode(f, d):
    bits = []
    for a in f:
        for j in range(d):
            bits.append((a >> j) & 1)
    out = bytearray(len(bits) // 8)
    for i, bit in enumerate(bits):
        out[i // 8] |= bit << (i % 8)
    return bytes(out)

def byte_decode(b, d):
    m = 2 ** d if d < 12 else Q
    bits = [(b[i // 8] >> (i % 8)) & 1 for i in range(len(b) * 8)]
    return [sum(bits[i * d + j] << j for j in range(d)) % m for i in range(N)]

def compress(x, d):
    return ((x << d) + Q // 2) // Q % (2 ** d)

def decompress(y, d):
    return (y * Q + (1 << (d - 1))) >> d

# ---- sampling (§4.2.2) ----

def sample_ntt(seed):
    xof = hashlib.shake_128(seed).digest(3 * 512)
    a, j, i = [], 0, 0
    while j < N:
        if i + 3 > len(xof):
            xof = hashlib.shake_128(seed).digest(len(xof) * 2)
        c0, c1, c2 = xof[i], xof[i + 1], xof[i + 2]
        d1 = c0 + 256 * (c1 % 16)
        d2 = c1 // 16 + 16 * c2
        if d1 < Q:
            a.append(d1); j += 1
        if d2 < Q and j < N:
            a.append(d2); j += 1
        i += 3
    return a

def sample_cbd(b, eta):
    bits = [(b[i // 8] >> (i % 8)) & 1 for i in range(len(b) * 8)]
    f = []
    for i in range(N):
        x = sum(bits[2 * i * eta + j] for j in range(eta))
        y = sum(bits[2 * i * eta + eta + j] for j in range(eta))
        f.append((x - y) % Q)
    return f

# ---- NTT (§4.3) ----

def ntt(f):
    f = list(f)
    i, ln = 1, 128
    while ln >= 2:
        for start in range(0, N, 2 * ln):
            z = ZETAS[i]; i += 1
            for j in range(start, start + ln):
                t = z * f[j + ln] % Q
                f[j + ln] = (f[j] - t) % Q
                f[j] = (f[j] + t) % Q
        ln //= 2
    return f

def ntt_inv(f):
    f = list(f)
    i, ln = 127, 2
    while ln <= 128:
        for start in range(0, N, 2 * ln):
            z = ZETAS[i]; i -= 1
            for j in range(start, start + ln):
                t = f[j]
                f[j] = (t + f[j + ln]) % Q
                f[j + ln] = z * (f[j + ln] - t) % Q
        ln *= 2
    return [x * 3303 % Q for x in f]

def multiply_ntts(f, g):
    h = [0] * N
    for i in range(128):
        a0, a1, b0, b1 = f[2 * i], f[2 * i + 1], g[2 * i], g[2 * i + 1]
        h[2 * i] = (a0 * b0 + a1 * b1 * GAMMAS[i]) % Q
        h[2 * i + 1] = (a0 * b1 + a1 * b0) % Q
    return h

def add(f, g): return [(a + b) % Q for a, b in zip(f, g)]
def sub(f, g): return [(a - b) % Q for a, b in zip(f, g)]

# ---- K-PKE (§5) ----

def matrix(rho, transpose=False):
    return [[sample_ntt(rho + bytes([j, i]) if not transpose else rho + bytes([i, j])) for j in range(K)] for i in range(K)]

def pke_keygen(d):
    rho, sigma = G(d + bytes([K]))
    A = matrix(rho)
    n = 0
    s = []
    for i in range(K):
        s.append(sample_cbd(PRF(ETA1, sigma, n), ETA1)); n += 1
    e = []
    for i in range(K):
        e.append(sample_cbd(PRF(ETA1, sigma, n), ETA1)); n += 1
    sh = [ntt(x) for x in s]
    eh = [ntt(x) for x in e]
    th = []
    for i in range(K):
        acc = [0] * N
        for j in range(K):
            acc = add(acc, multiply_ntts(A[i][j], sh[j]))
        th.append(add(acc, eh[i]))
    ek = b''.join(byte_encode(t, 12) for t in th) + rho
    dk = b''.join(byte_encode(x, 12) for x in sh)
    return ek, dk

def pke_encrypt(ek, m, r):
    th = [byte_decode(ek[384 * i:384 * (i + 1)], 12) for i in range(K)]
    rho = ek[384 * K:]
    A = matrix(rho)
    n = 0
    y = []
    for i in range(K):
        y.append(sample_cbd(PRF(ETA1, r, n), ETA1)); n += 1
    e1 = []
    for i in range(K):
        e1.append(sample_cbd(PRF(ETA2, r, n), ETA2)); n += 1
    e2 = sample_cbd(PRF(ETA2, r, n), ETA2)
    yh = [ntt(x) for x in y]
    u = []
    for i in range(K):
        acc = [0] * N
        for j in range(K):
            acc = add(acc, multiply_ntts(A[j][i], yh[j]))
        u.append(add(ntt_inv(acc), e1[i]))
    mu = [decompress(b, 1) for b in byte_decode(m, 1)]
    acc = [0] * N
    for j in range(K):
        acc = add(acc, multiply_ntts(th[j], yh[j]))
    v = add(add(ntt_inv(acc), e2), mu)
    c1 = b''.join(byte_encode([compress(x, DU) for x in ui], DU) for ui in u)
    c2 = byte_encode([compress(x, DV) for x in v], DV)
    return c1 + c2

def pke_decrypt(dk, c):
    u = [[decompress(x, DU) for x in byte_decode(c[320 * i:320 * (i + 1)], DU)] for i in range(K)]
    v = [decompress(x, DV) for x in byte_decode(c[320 * K:], DV)]
    sh = [byte_decode(dk[384 * i:384 * (i + 1)], 12) for i in range(K)]
    acc = [0] * N
    for j in range(K):
        acc = add(acc, multiply_ntts(sh[j], ntt(u[j])))
    w = sub(v, ntt_inv(acc))
    return byte_encode([compress(x, 1) for x in w], 1)

# ---- ML-KEM (§6) ----

def keygen(d, z):
    ek, dkp = pke_keygen(d)
    return ek, dkp + ek + H(ek) + z

def encaps(ek, m):
    Kk, r = G(m + H(ek))
    return Kk, pke_encrypt(ek, m, r)

def decaps(dk, c):
    dkp = dk[:384 * K]
    ek = dk[384 * K:768 * K + 32]
    h = dk[768 * K + 32:768 * K + 64]
    z = dk[768 * K + 64:]
    m = pke_decrypt(dkp, c)
    Kk, r = G(m + h)
    Kbar = J(z + c)
    return Kk if pke_encrypt(ek, m, r) == c else Kbar
