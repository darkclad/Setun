"""Generate the real27/real81 conformance vectors (docs/REAL.md, §8).

An independent reference: every operation is computed here in exact
rational arithmetic (Python's Fraction and big integers) from the rules of
docs/REAL.md, sharing no code with crates/real. The vectors go to
crates/real/tests/vectors.txt, which crates/real/tests/conformance.rs
(the library) and crates/core/tests/fpu_conformance.rs (the FPU's
instructions) check.

    python tools/gen_realvectors.py > crates/real/tests/vectors.txt

Line format: `op operands... = result flags`, one vector per line.
A real is `E:M`: the exponent and the 72-trit significand as stored in a
real81 (its value is M * 3^(E - 72)). Integers are plain decimals,
decimal text is in double quotes. Flags are `IV DZ OF UF IX` joined by
`+`, or `-`. A comparison gives lt, eq, gt or un (unordered).
"""

import random
import sys
from fractions import Fraction

N = 72                      # real81 significand trits
N27 = 18
E_MAX = 9841
E_MIN = -9841
TOP = 3 ** (N - 1)          # the infinities' significand
WORD_MAX = (3 ** 27 - 1) // 2
IV, DZ, OF, UF, IX = 1, 2, 4, 8, 16

ZERO = (0, 0)
NAN = (E_MAX, 0)
INF = (E_MAX, TOP)
NINF = (E_MAX, -TOP)


def mmax(n):
    return (3 ** n - 1) // 2


def norm(n):
    return (3 ** (n - 1) - 1) // 2


def kind(r):
    e, m = r
    if e == E_MAX:
        return 'inf' if abs(m) == TOP else 'nan'
    if m == 0:
        return 'zero'
    if e == E_MIN and abs(m) <= norm(N):
        return 'sub'
    return 'normal'


def is_nan(r):
    return kind(r) == 'nan'


def sgn(v):
    return (v > 0) - (v < 0)


def p3(k):
    return Fraction(3) ** k


def value(r):
    e, m = r
    return m * p3(e - N)


def round_half_even(x):
    """The nearest integer to the Fraction x, ties to even."""
    q = x.numerator // x.denominator          # floor
    d = x - q
    if d > Fraction(1, 2) or (d == Fraction(1, 2) and q % 2 == 1):
        q += 1
    return q


def trunc(x):
    q = abs(x.numerator) // x.denominator
    return q if x >= 0 else -q


def fits(ax, e):
    """|x| <= 3^e / 2: x has a significand of at most (3^n - 1)/2 (or the
    exact half above, which rounds down to it) at exponent e."""
    return 2 * ax <= p3(e)


LOG3_2 = 0.6309297535714574           # log 3 of 2


def log3_estimate(x):
    """About log3(x) for a positive Fraction (from the bit lengths)."""
    return (x.numerator.bit_length() - x.denominator.bit_length()) * LOG3_2


def exponent_of(ax):
    """The smallest e with |x| <= 3^e / 2 (no range limit)."""
    e = int(log3_estimate(2 * ax)) + 2
    while not fits(ax, e):
        e += 1
    while fits(ax, e - 1):
        e -= 1
    return e


def round_to(x, n=N):
    """x (a Fraction) to the nearest value with n significand trits, ties
    to the even significand; with the flags."""
    if x == 0:
        return ZERO, 0
    ax = abs(x)
    e = exponent_of(ax)
    if e >= E_MAX:
        return (INF if x > 0 else NINF), OF | IX
    e = max(e, E_MIN)
    scaled = x * p3(n - e)
    m = round_half_even(scaled)
    inexact = m != scaled
    if abs(m) > mmax(n):
        m = sgn(m) * mmax(n)      # the exact half above the largest: the largest
    flags = IX if inexact else 0
    if m == 0:
        return ZERO, flags | (UF if inexact else 0)
    if e == E_MIN and abs(m) <= norm(n) and inexact:
        flags |= UF
    return (e, m * 3 ** (N - n)), flags


def sqrt_round(x):
    """sqrt(x), x a positive Fraction, rounded to 72 trits."""
    # e: smallest with sqrt(x) <= 3^e / 2, i.e. x <= 9^e / 4
    e = int(log3_estimate(4 * x) / 2) + 2
    while not 4 * x <= p3(2 * e):
        e += 1
    while 4 * x <= p3(2 * (e - 1)):
        e -= 1
    if e >= E_MAX:
        return INF, OF | IX
    e = max(e, E_MIN)
    # M = round(sqrt(x * 3^(2(N - e)))); y = p / q
    y = x * p3(2 * (N - e))
    p, q = y.numerator, y.denominator
    # s = floor(sqrt(p / q))
    s = isqrt_frac(p, q)
    exact = s * s * q == p
    # round: compare (s + 1/2)^2 with y, i.e. (2s + 1)^2 q with 4p
    c = (2 * s + 1) ** 2 * q - 4 * p
    m = s + 1 if c < 0 or (c == 0 and s % 2 == 1) else s
    flags = 0 if exact else IX
    if m > mmax(N):
        m = mmax(N)
    if e == E_MIN and m <= norm(N) and not exact:
        flags |= UF
    return (e, m), flags


def isqrt(v):
    if v < 2:
        return v
    x = 1 << ((v.bit_length() + 1) // 2)
    while True:
        y = (x + v // x) // 2
        if y >= x:
            return x
        x = y


def isqrt_frac(p, q):
    """floor(sqrt(p / q))."""
    s = isqrt(p // q)
    while (s + 1) * (s + 1) * q <= p:
        s += 1
    while s * s * q > p:
        s -= 1
    return s


def neg(a):
    if kind(a) == 'zero':
        return ZERO
    return (a[0], -a[1])


def renorm(a):
    return round_to(value(a))


def add(a, b):
    if is_nan(a):
        return a, 0
    if is_nan(b):
        return b, 0
    ka, kb = kind(a), kind(b)
    if ka == 'inf' and kb == 'inf':
        return (a, 0) if a[1] == b[1] else (NAN, IV)
    if ka == 'inf':
        return a, 0
    if kb == 'inf':
        return b, 0
    if ka == 'zero' and kb == 'zero':
        return ZERO, 0
    if ka == 'zero':
        return renorm(b)
    if kb == 'zero':
        return renorm(a)
    return round_to(value(a) + value(b))


def sub(a, b):
    if is_nan(b) and not is_nan(a):
        return b, 0
    return add(a, neg(b))


def mul(a, b):
    if is_nan(a):
        return a, 0
    if is_nan(b):
        return b, 0
    ka, kb = kind(a), kind(b)
    s = sgn(a[1]) * sgn(b[1])
    if (ka == 'inf' and kb == 'zero') or (ka == 'zero' and kb == 'inf'):
        return NAN, IV
    if ka == 'inf' or kb == 'inf':
        return (NINF if s < 0 else INF), 0
    if ka == 'zero' or kb == 'zero':
        return ZERO, 0
    return round_to(value(a) * value(b))


def div(a, b):
    if is_nan(a):
        return a, 0
    if is_nan(b):
        return b, 0
    ka, kb = kind(a), kind(b)
    s = sgn(a[1]) * sgn(b[1])
    if (ka == 'inf' and kb == 'inf') or (ka == 'zero' and kb == 'zero'):
        return NAN, IV
    if ka == 'inf':
        return (NINF if s < 0 else INF), 0
    if kb == 'inf' or ka == 'zero':
        return ZERO, 0
    if kb == 'zero':
        return (NINF if a[1] < 0 else INF), DZ
    return round_to(value(a) / value(b))


def rem(a, b):
    if is_nan(a):
        return a, 0
    if is_nan(b):
        return b, 0
    ka, kb = kind(a), kind(b)
    if ka == 'inf' or kb == 'zero':
        return NAN, IV
    if kb == 'inf':
        return renorm(a)
    if ka == 'zero':
        return ZERO, 0
    va, vb = value(a), value(b)
    q = round_half_even(va / vb)
    r, f = round_to(va - q * vb)
    return r, f & ~IX


def sqrt(a):
    k = kind(a)
    if k in ('nan', 'zero'):
        return a, 0
    if k == 'inf':
        return (a, 0) if a[1] > 0 else (NAN, IV)
    if a[1] < 0:
        return NAN, IV
    return sqrt_round(value(a))


def cmp(a, b):
    if is_nan(a) or is_nan(b):
        return 'un'
    def rank(r):
        k = kind(r)
        if k == 'inf':
            return Fraction(2 * sgn(r[1]) * 10 ** 6000)   # beyond every finite value
        return value(r)
    x, y = rank(a), rank(b)
    return 'lt' if x < y else ('gt' if x > y else 'eq')


def fmin(a, b):
    if is_nan(a) and is_nan(b):
        return a
    if is_nan(a):
        return b
    if is_nan(b):
        return a
    return b if cmp(a, b) == 'gt' else a


def fmax(a, b):
    if is_nan(a) and is_nan(b):
        return a
    if is_nan(a):
        return b
    if is_nan(b):
        return a
    return b if cmp(a, b) == 'lt' else a


def round_int(a, toward_zero):
    if kind(a) in ('nan', 'inf', 'zero'):
        return a, 0
    v = value(a)
    q = trunc(v) if toward_zero else round_half_even(v)
    r, _ = round_to(Fraction(q))
    return r, (IX if q != v else 0)


def scale(a, k):
    if kind(a) in ('nan', 'inf', 'zero'):
        return a, 0
    k = max(-3 * E_MAX, min(3 * E_MAX, k))
    return round_to(value(a) * p3(k))


def exponent(a):
    k = kind(a)
    if k == 'zero':
        return 0, 0
    if k in ('inf', 'nan'):
        return E_MAX, IV
    return exponent_of(abs(value(a))), 0


def fclass(a):
    s = sgn(a[1])
    return {'zero': 0, 'sub': s, 'normal': 2 * s, 'inf': 3 * s, 'nan': 4}[kind(a)]


def to27(a):
    k = kind(a)
    if k in ('zero', 'inf'):
        return a, 0
    if k == 'nan':
        top = trunc(Fraction(a[1], 3 ** (N - N27)))
        if abs(top) == 3 ** (N27 - 1):
            top = 0
        return (E_MAX, top * 3 ** (N - N27)), 0
    return round_to(value(a), N27)


def cvtif(i):
    return round_to(Fraction(i))


def cvtfi(a):
    k = kind(a)
    if k == 'nan':
        return 0, IV
    if k == 'inf':
        return WORD_MAX * sgn(a[1]), IV
    if k == 'zero':
        return 0, 0
    v = value(a)
    q = round_half_even(v)
    if abs(q) > WORD_MAX:
        return WORD_MAX * sgn(q), IV
    return q, (IX if q != v else 0)


def from_decimal(s):
    return round_to(Fraction(s))


# ---- the vectors -------------------------------------------------------------------------------

def fmt_real(r):
    return f'{r[0]}:{r[1]}'


def fmt_flags(f):
    names = [n for n, b in (('IV', IV), ('DZ', DZ), ('OF', OF), ('UF', UF), ('IX', IX)) if f & b]
    return '+'.join(names) if names else '-'


def special_values():
    v = [ZERO, INF, NINF, NAN, (E_MAX, 12345), (E_MAX, -1)]
    one = round_to(Fraction(1))[0]
    v += [one, neg(one), round_to(Fraction(3))[0], round_to(Fraction(1, 3))[0], round_to(Fraction(2))[0],
          round_to(Fraction(1, 2))[0], round_to(Fraction(-7, 2))[0], round_to(Fraction(10))[0]]
    v += [(E_MAX - 1, mmax(N)), (E_MAX - 1, -mmax(N)),           # the largest
          (E_MIN, norm(N) + 1), (E_MIN, -(norm(N) + 1)),        # the smallest normal
          (E_MIN, 1), (E_MIN, -1), (E_MIN, norm(N)),            # subnormals
          (E_MIN, 3 ** 40 + 5)]
    return v


def random_normal(rng, e_lo, e_hi):
    e = rng.randint(e_lo, e_hi)
    m = rng.randint(norm(N) + 1, mmax(N))
    return (e, m if rng.random() < 0.5 else -m)


def random_value(rng):
    r = rng.random()
    if r < 0.5:
        return random_normal(rng, -30, 30)
    if r < 0.7:
        return random_normal(rng, -200, 200)
    if r < 0.8:
        return random_normal(rng, E_MAX - 80, E_MAX - 1)
    if r < 0.9:
        return random_normal(rng, E_MIN, E_MIN + 80)
    m = rng.randint(1, 3 ** rng.randint(1, N - 1))
    return (E_MIN, m if rng.random() < 0.5 else -m)


def small_int(rng):
    return round_to(Fraction(rng.randint(-3000, 3000)))[0]


def main():
    rng = random.Random(0x5E7A_11)
    out = sys.stdout
    out.write('# real27/real81 conformance vectors: generated by tools/gen_realvectors.py\n')
    out.write('# from docs/REAL.md; do not edit. `op operands = result flags`, reals E:M.\n')
    sp = special_values()
    pairs = [(a, b) for a in sp for b in sp]
    pairs += [(random_value(rng), random_value(rng)) for _ in range(300)]
    # close and cancelling operands, integers, exact halves (2k+1)/2
    for _ in range(60):
        a = random_normal(rng, -5, 5)
        pairs.append((a, (a[0], a[1] + rng.randint(-50, 50))))
        pairs.append((small_int(rng), small_int(rng)))
        k = 2 * rng.randint(-1000, 1000) + 1
        pairs.append((round_to(Fraction(k))[0], round_to(Fraction(2 if rng.random() < 0.5 else -2))[0]))
    binary = [('add', add), ('sub', sub), ('mul', mul), ('div', div), ('rem', rem)]
    for name, f in binary:
        for a, b in pairs:
            r, fl = f(a, b)
            out.write(f'{name} {fmt_real(a)} {fmt_real(b)} = {fmt_real(r)} {fmt_flags(fl)}\n')
    for name, f in (('min', fmin), ('max', fmax)):
        for a, b in pairs[:len(sp) ** 2 + 60]:
            out.write(f'{name} {fmt_real(a)} {fmt_real(b)} = {fmt_real(f(a, b))} -\n')
    for a, b in pairs:
        out.write(f'cmp {fmt_real(a)} {fmt_real(b)} = {cmp(a, b)} -\n')
    singles = sp + [random_value(rng) for _ in range(300)] + [small_int(rng) for _ in range(40)]
    singles += [round_to(Fraction(rng.randint(-10 ** 9, 10 ** 9), 3 ** rng.randint(0, 30)))[0] for _ in range(60)]
    for a in singles:
        for name, f in (('sqrt', sqrt), ('rnd', lambda x: round_int(x, False)), ('trunc', lambda x: round_int(x, True)), ('to27', to27)):
            r, fl = f(a)
            out.write(f'{name} {fmt_real(a)} = {fmt_real(r)} {fmt_flags(fl)}\n')
        out.write(f'neg {fmt_real(a)} = {fmt_real(neg(a))} -\n')
        out.write(f'abs {fmt_real(a)} = {fmt_real(neg(a) if a[1] < 0 and not is_nan(a) else a)} -\n')
        out.write(f'class {fmt_real(a)} = {fclass(a)} -\n')
        x, fl = exponent(a)
        out.write(f'exp {fmt_real(a)} = {x} {fmt_flags(fl)}\n')
        x, fl = cvtfi(a)
        out.write(f'cvtfi {fmt_real(a)} = {x} {fmt_flags(fl)}\n')
        for k in (0, 1, -1, 5, -40, 100, -100, 30000):
            r, fl = scale(a, k)
            out.write(f'scale {fmt_real(a)} {k} = {fmt_real(r)} {fmt_flags(fl)}\n')
    for i in [0, 1, -1, 2, 3, 9841, -9841, WORD_MAX, -WORD_MAX] + [rng.randint(-WORD_MAX, WORD_MAX) for _ in range(40)]:
        r, fl = cvtif(i)
        out.write(f'cvtif {i} = {fmt_real(r)} {fmt_flags(fl)}\n')
    decimals = ['0', '1', '-1', '0.5', '1.5', '2.5', '-2.5', '0.1', '0.2', '0.3', '3.14159265358979323846264338327950288419716939937510',
                '1e100', '-1e-100', '1e4694', '1e4695', '1e-4696', '1e-4720', '1e-4760', '123456789012345678901234567890', '0.333333333333333333333333333333333333',
                '2.718281828459045235360287471352662497757', '6.02214076e23', '1.602176634e-19']
    decimals += [f'{rng.randint(1, 10 ** rng.randint(1, 40))}e{rng.randint(-60, 60)}' for _ in range(60)]
    for s in decimals:
        r, fl = from_decimal(s)
        out.write(f'dec "{s}" = {fmt_real(r)} {fmt_flags(fl)}\n')


main()
