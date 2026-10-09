#!/usr/bin/env python3
"""The format conformance vectors (docs/FORMATS.md §5): tvarints, TTV values
in their text notation, and TIM pictures, from an independent model of the
specification. /bin/fmtvec checks Trinux's libraries against them.

    python tools/gen_fmtvectors.py > user/share/fmtvectors.txt
"""
import random
import sys
from fractions import Fraction

G = 3 ** 8                 # a tvarint group's modulus
GH = (G - 1) // 2          # 3280
T9 = 3 ** 9

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

# ---- tvarint (§1) ----

def tvar(v):
    """The canonical trytes: groups least significant first, control +1 on
    all but the last."""
    groups = []
    while True:
        g = wrapn(v, 8)
        groups.append(g)
        v = (v - g) // G
        if v == 0:
            break
    return [g + (G if i < len(groups) - 1 else 0) for i, g in enumerate(groups)]

def tryte_word(v):
    """A word as 3 trytes, most significant first."""
    t = trits(v, 27)
    return [untrits(t[0:9]), untrits(t[9:18]), untrits(t[18:27])]

# ---- real81 (REAL.md §3), for the integers and thirds used here ----

def real81(x):
    """9 trytes of an exactly representable x = k · 3^j."""
    x = Fraction(x)
    if x == 0:
        return [0] * 9
    # find E with 1/6 < |x / 3^E| < 1/2
    e = 0
    while abs(x) / Fraction(3) ** e >= Fraction(1, 2):
        e += 1
    while abs(x) / Fraction(3) ** e <= Fraction(1, 6):
        e -= 1
    m = x / Fraction(3) ** e * 3 ** 72
    assert m.denominator == 1, 'not exact in real81'
    t = trits(e, 9) + trits(int(m), 72)
    return [untrits(t[9 * k:9 * k + 9]) for k in range(9)]

def real_text(x):
    """Trine's rtoa(x, 12): 12 significant digits, trailing zeros dropped,
    rounded to nearest (no value here is an exact tie), plain for decimal
    exponents -5 ... 11, else d.ddde+N / d.ddde-N; then '.0' when there is
    neither a point nor an exponent."""
    x = Fraction(x)
    if x == 0:
        s = '0'
    else:
        neg = x < 0
        a = abs(x)
        e10 = 0
        while a >= 10 ** (e10 + 1):
            e10 += 1
        while a < Fraction(10) ** e10:
            e10 -= 1
        scaled = a / Fraction(10) ** (e10 - 11)
        near = (2 * scaled.numerator + scaled.denominator) // (2 * scaled.denominator)
        assert 2 * scaled != 2 * int(scaled) + 1, f'{x}: an exact tie'
        if near == 10 ** 12:
            near //= 10
            e10 += 1
        cut = near
        ds = str(cut).rstrip('0') or '0'
        if -5 <= e10 < 12:
            if e10 >= 0:
                ip = ''.join(ds[k] if k < len(ds) else '0' for k in range(e10 + 1))
                fp = ds[e10 + 1:]
            else:
                ip = '0'
                fp = '0' * (-e10 - 1) + ds
            s = ip + ('.' + fp if fp else '')
        else:
            s = ds[0] + ('.' + ds[1:] if len(ds) > 1 else '') + 'e' + ('-' if e10 < 0 else '+') + str(abs(e10))
        s = ('-' if neg else '') + s
    if '.' not in s and 'e' not in s:
        s += '.0'
    return s

def text_exact(x):
    """Does the 12-digit text read back to x itself? (decimals of up to 12
    digits that are integers, so the parse is exact)"""
    t = real_text(x)
    return Fraction(t) == Fraction(x)

# ---- TTV (§2) ----

NULL, TRUTH, INT, REAL, TEXT, OCTETS, LIST, MAP = range(8)

class R:                    # a real in a value tree
    def __init__(self, x):
        self.x = Fraction(x)

class O:                    # octets
    def __init__(self, b):
        self.b = bytes(b)

class Truth:
    def __init__(self, t):
        self.t = t

def ttv(v):
    if v is None:
        return [NULL]
    if isinstance(v, Truth):
        return [TRUTH, v.t]
    if isinstance(v, bool):
        raise TypeError
    if isinstance(v, int):
        return [INT] + tvar(v)
    if isinstance(v, R):
        return [REAL] + real81(v.x)
    if isinstance(v, str):
        cps = [ord(c) for c in v]
        assert all(0 < c <= 9841 for c in cps)
        return [TEXT] + tvar(len(cps)) + cps
    if isinstance(v, O):
        return [OCTETS] + tvar(len(v.b)) + list(v.b)
    if isinstance(v, list):
        out = [LIST] + tvar(len(v))
        for x in v:
            out += ttv(x)
        return out
    if isinstance(v, dict):
        out = [MAP] + tvar(len(v))
        for k, x in v.items():
            out += ttv(k) + ttv(x)
        return out
    raise TypeError(v)

def text(v):
    """The canonical notation."""
    if v is None:
        return 'null'
    if isinstance(v, Truth):
        return {1: 'true', 0: 'unknown', -1: 'false'}[v.t]
    if isinstance(v, int):
        return str(v)
    if isinstance(v, R):
        return real_text(v.x)
    if isinstance(v, str):
        return '"' + v.replace('\\', '\\\\').replace('"', '\\"').replace('\n', '\\n') + '"'
    if isinstance(v, O):
        return 'x"' + v.b.hex() + '"'
    if isinstance(v, list):
        return '[' + ', '.join(text(x) for x in v) + ']'
    if isinstance(v, dict):
        return '{' + ', '.join(text(k) + ': ' + text(x) for k, x in v.items()) + '}'
    raise TypeError(v)

# ---- TIM (§4) ----

TIM_MAGIC = ord('T') * 3 ** 18 + ord('I') * T9 + ord('M')
TAU_MAGIC = ord('T') * 3 ** 18 + ord('A') * T9 + ord('U')

def tau(rate, ch, frames, samples, reserved=0, magic=TAU_MAGIC, version=1):
    out = []
    for v in (magic, version, rate, ch, frames, reserved):
        out += tryte_word(v)
    for x in samples:
        out += tryte_word(x)
    return out

def predict(s, w, ch, x, y, c):
    at = (y * w + x) * ch + c
    a = s[at - ch] if x > 0 else 0
    b = s[at - w * ch] if y > 0 else 0
    d = s[at - w * ch - ch] if x > 0 and y > 0 else 0
    lo, hi = min(a, b), max(a, b)
    if d >= hi:
        return lo
    if d <= lo:
        return hi
    return a + b - d

def tim(w, h, ch, depth, coding, s):
    out = []
    for v in (TIM_MAGIC, 1, w, h, ch, depth, coding, 0):
        out += tryte_word(v)
    if coding == 0:
        return out + list(s)
    run = 0
    for y in range(h):
        for x in range(w):
            for c in range(ch):
                r = s[(y * w + x) * ch + c] - predict(s, w, ch, x, y, c)
                if r == 0:
                    run += 1
                    if run == GH:
                        out.append(run - G)
                        run = 0
                else:
                    if run:
                        out.append(run - G)
                        run = 0
                    out += tvar(r)
    if run:
        out.append(run - G)
    return out

# ---- the vectors ----

rnd = random.Random(81)
LINES = []

def line(*parts):
    LINES.append(' '.join(str(p) for p in parts))

def main():
    W27 = (3 ** 27 - 1) // 2
    L81 = (3 ** 81 - 1) // 2
    # tvarint: the edges of each length, both signs, words and longs
    vals = {0, 1, -1, 7, -5, GH, -GH, GH + 1, -GH - 1, 10000, -10000, (G * G - 1) // 2, (G * G + 1) // 2,
            (G ** 3 - 1) // 2, (G ** 3 + 1) // 2, W27, -W27, W27 + 1, -W27 - 1, L81, -L81, 3 ** 40, -(3 ** 40)}
    for k in range(40):
        vals.add(rnd.randint(-W27, W27))
        vals.add(rnd.randint(-L81, L81))
    for v in sorted(vals):
        line('tvar', v, '=', *tvar(v))
    # refused as any integer: a zero last group, a -1 control, cut short,
    # past 81 trits, more than 11 trytes
    line('tvar!', G + 5, 0)
    line('tvar!', 0 - G * 0 + G, 0)
    line('tvar!', -5000)
    line('tvar!', 5 - G)
    line('tvar!', G + 1)
    line('tvar!', G + 1, G + 2)
    line('tvar!', *([G] * 10 + [2]))
    line('tvar!', *([G + 1] * 11 + [1]))
    line('tvar!', G + 3, -G - 1)
    # refused as an int, accepted as a long
    for v in (W27 + 1, -W27 - 1, 3 ** 30, L81):
        line('tvari!', *tvar(v))
    # TTV
    values = [
        None, Truth(1), Truth(0), Truth(-1), 0, 1, -1, 3281, -10000, W27, -W27 - 5, L81,
        R(0), R(1), R(-1), R(3), R(-27), R(100), R(6561), R(Fraction(1, 3)), R(Fraction(-1, 9)), R(3 ** 40),
        R(10 ** 20), R(-3 ** 20), R(123456789012), R(-10 ** 13),
        '', 'a', 'Setun', 'Сетунь', 'quote " and \\ and\nnewline', 'x' * 100,
        O([]), O([0, 255, 10]), O(range(0, 256, 17)),
        [], [1, 2, 3], [None, Truth(0), [[]]], {}, {'name': 'Сетунь', 'trits': 27},
        {1: Truth(1), -1: Truth(-1), 'list': [O([1]), R(9)]},
        [{'a': [1, [2, [3, {}]]]}, 'z'],
    ]
    def lossy(v):
        if isinstance(v, R):
            return not text_exact(v.x)
        if isinstance(v, list):
            return any(lossy(x) for x in v)
        if isinstance(v, dict):
            return any(lossy(k) or lossy(x) for k, x in v.items())
        return False
    for v in values:
        line('ttv>' if lossy(v) else 'ttv', text(v), '=', *ttv(v))
    # refused: an unknown tag, a truth of 2, a negative or too long
    # length, a count past the data, octets past 255, a cut real, a cut
    # tvarint, trailing data after one value, and a bad tvarint in a length
    for bad in ([8], [-1], [TRUTH, 2], [TRUTH], [TEXT, 5, 97], [TEXT, -1], [OCTETS, 1, 256], [OCTETS, 1, -1],
                [LIST, 50], [LIST, 2, NULL], [MAP, 1, NULL], [REAL, 0, 0, 0], [INT, G + 1], [INT, G + 1, 0],
                [NULL, NULL], [TEXT, G + 1, 0], [LIST, -G + 3]):
        line('ttv!', *bad)
    # TAU: a header and word samples (§3)
    for rate, ch, frames in ((8000, 1, 0), (48000, 2, 3), (44100, 9, 1), (1, 1, 5)):
        smp = [rnd.choice([0, 1, -1, W27, -W27, rnd.randint(-W27, W27)]) for _ in range(ch * frames)]
        line('tau', rate, ch, frames, ':', *smp, '=', *tau(rate, ch, frames, smp))
    for bad in (tau(8000, 1, 1, [5], magic=TAU_MAGIC - 1), tau(8000, 1, 1, [5], version=2), tau(0, 1, 1, [5]),
                tau(-8000, 1, 1, [5]), tau(8000, 0, 1, []), tau(8000, 10, 1, [0] * 10), tau(8000, 1, -1, []),
                tau(8000, 1, 1, [5], reserved=1), tau(8000, 2, 2, [1, 2, 3]), tau(8000, 1, 1, [])[:17]):
        line('tau!', *bad)
    # TIM: each depth and coding, flat areas, edges, noise, long runs
    def pic(w, h, ch, depth, kind):
        lim = (3 ** depth - 1) // 2
        s = []
        for y in range(h):
            for x in range(w):
                for c in range(ch):
                    if kind == 'flat':
                        v = 0
                    elif kind == 'edge':
                        v = (lim if x >= w // 2 else -lim // 2) + c * (1 if lim > 2 else 0)
                    elif kind == 'ramp':
                        v = wrapn(x * 3 + y - c, depth) if depth > 1 else (x + y) % 3 - 1
                    else:
                        v = rnd.randint(-lim, lim)
                    s.append(max(-lim, min(lim, v)))
        return s
    for (w, h, ch, depth, kind) in [(1, 1, 1, 1, 'noise'), (3, 2, 1, 6, 'noise'), (4, 4, 3, 3, 'edge'), (5, 3, 3, 6, 'ramp'),
                                     (8, 8, 1, 9, 'noise'), (9, 9, 1, 2, 'ramp'), (60, 60, 1, 4, 'flat'), (81, 41, 1, 1, 'flat'),
                                     (7, 5, 3, 9, 'edge'), (2, 9, 3, 5, 'noise')]:
        s = pic(w, h, ch, depth, kind)
        for coding in (0, 1):
            if coding == 0 and w * h * ch > 400:
                continue
            line('tim', w, h, ch, depth, coding, ':', *s, '=', *tim(w, h, ch, depth, coding, s))
    # the known answer of fmttest: 25 20 5 / 30 0 7
    s = [25, 20, 5, 30, 0, 7]
    line('tim', 3, 2, 1, 6, 1, ':', *s, '=', *tim(3, 2, 1, 6, 1, s))
    good = tim(2, 2, 1, 3, 1, [1, 0, 0, 0])
    good_raw = tim(2, 2, 1, 3, 0, [1, 0, 0, -1])
    def hdr(**kw):
        f = dict(magic=TIM_MAGIC, version=1, w=2, h=2, ch=1, depth=3, coding=0, res=0)
        f.update(kw)
        out = []
        for k in ('magic', 'version', 'w', 'h', 'ch', 'depth', 'coding', 'res'):
            out += tryte_word(f[k])
        return out
    for bad in (hdr(magic=TIM_MAGIC + 1) + [0] * 4, hdr(version=2) + [0] * 4, hdr(w=0), hdr(h=-1), hdr(ch=2) + [0] * 8,
                hdr(depth=0) + [0] * 4, hdr(depth=10) + [0] * 4, hdr(coding=2) + [0] * 4, hdr(res=1) + [0] * 4,
                hdr() + [0] * 3, hdr() + [0] * 5, hdr() + [14, 0, 0, 0], good_raw[:-1], good + [0],
                good[:-1], hdr(coding=1) + [-G], hdr(coding=1) + [0, -G + 3], hdr(coding=1) + [-G + 5],
                hdr(w=19684) + [0], hdr()[:20]):
        line('tim!', *bad)
    sys.stdout.reconfigure(encoding='utf-8', newline='\n')
    print('# Setun-11\u0305 format conformance vectors (docs/FORMATS.md §5), made by')
    print('# tools/gen_fmtvectors.py from an independent model of the specification.')
    print('#   tvar V = T...      V encodes as exactly these trytes, which decode to V')
    print('#   tvar! T...         refused, as an int and as a long')
    print('#   tvari! T...        refused as an int (too large), read as a long')
    print('#   ttv N = T...       the value in notation N encodes as T, and T prints as N')
    print('#   ttv> N = T...      T prints as N (a real whose 12-digit text is not exact)')
    print('#   ttv! T...          not one valid TTV value')
    print('#   tim W H C D K : S... = T...   samples S encode as the file T (K: 0 raw,')
    print('#                      1 predicted), and T decodes to S')
    print('#   tim! T...          not a valid TIM file')
    print('#   tau R C F : S... = T...   a TAU file of rate R, C channels, F frames')
    print('#                      and samples S is T, and T reads back as that')
    print('#   tau! T...          not a valid TAU file')
    for l in LINES:
        print(l)
    print(f'# {len(LINES)} vectors', file=sys.stderr)

main()
