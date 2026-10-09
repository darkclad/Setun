#!/usr/bin/env python3
"""The ISA conformance vectors (docs/SETUN26_ISA.md §16): an independent
model of the integer core, the MMU, traps and the vector unit, written from
the specification, not from the emulator. Each vector is a state before,
the instruction words, and the state after one instruction.

    python tools/gen_isavectors.py > crates/core/tests/isa_vectors.txt

The file format is described in the header it writes. Floating-point
arithmetic has its own suite (docs/REAL.md); here only "FPU off" traps.
"""
import random
import sys

W = 3 ** 27
WMAX = (W - 1) // 2
T9 = 3 ** 9
TMAX = (T9 - 1) // 2
PAGE = T9
RAM_FRAMES = 27                       # the runner's memory
RAM_TOP = PAGE * RAM_FRAMES - TMAX - 1
ITB = 14580                           # 729 · 20
HANDLER = 30000                       # the runner's table: vector v -> HANDLER + 3v

# ---- balanced arithmetic --------------------------------------------------------------

def wrapn(v, n):
    m = 3 ** n
    h = (m - 1) // 2
    return (v + h) % m - h

def wrap(v):
    return wrapn(v, 27)

def trits(v, n=27):
    """Most significant first."""
    out = []
    for _ in range(n):
        r = v % 3
        d = -1 if r == 2 else r
        out.append(d)
        v = (v - d) // 3
    assert v == 0, 'does not fit'
    return out[::-1]

def untrits(t):
    v = 0
    for d in t:
        v = v * 3 + d
    return v

def field(v, a, b, n=27):
    return untrits(trits(v, n)[a - 1:b])

def with_field(v, a, b, x):
    t = trits(v)
    t[a - 1:b] = trits(wrapn(x, b - a + 1), b - a + 1)
    return untrits(t)

def sign(v):
    return (v > 0) - (v < 0)

def tritwise(a, b, f, n=27):
    return untrits([f(x, y) for x, y in zip(trits(a, n), trits(b, n))])

def merge(dst, mask, src, n=27):
    # mask 0 keeps dst, +1 takes src, -1 gives 0
    return untrits([d if m == 0 else (s if m == 1 else 0) for d, m, s in zip(trits(dst, n), trits(mask, n), trits(src, n))])

# ---- encoding (§3.1) --------------------------------------------------------------------

def enc(a, b, x=0, y=0, z=0):
    return a * 3 ** 18 + b * T9 + x * 729 + y * 27 + z

def enc_s(a, b, x, imm6):
    y, z = divmod_bal(imm6, 27)
    return enc(a, b, x, y, z)

def divmod_bal(v, m):
    """v = q·m + r with r balanced (m = 27: r is 3 trits)."""
    assert m == 27
    r = wrapn(v, 3)
    return (v - r) // 27, r

ALU, ALUQ, MEM, IO, JALR, LIQ, BRQ, MEMR, VALU, VMEM, VMOV = 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11
ALUI, MEMD, LI, LEA, JAL, BR, BR3, IOI, FLD = 6561, 6562, 6563, 6564, 6565, 6566, 6567, 6568, 6569

# ---- the machine model -------------------------------------------------------------------

MODE_POS = {'RING': 27, 'IE': 26, 'PG': 25, 'FE': 24, 'IOPL': 23, 'TF': 22, 'S': 21, 'C': 20, 'VE': 19}
MODE_DEFAULT = {'RING': 1, 'IE': 0, 'PG': 0, 'FE': 1, 'IOPL': 0, 'TF': 0, 'S': 0, 'C': 0, 'VE': 1}

class Fault(Exception):
    def __init__(self, v, addr=None, acc=None, info=0, nxt=False):
        self.v, self.addr, self.acc, self.info, self.nxt = v, addr, acc, info, nxt

READ, FETCH, WRITE = -1, 0, 1

class State:
    def __init__(self):
        self.r = {i: 0 for i in range(-13, 14)}
        self.v = {i: (0, 0, 0) for i in range(-13, 14)}
        self.mode = dict(MODE_DEFAULT)
        self.cr = {i: 0 for i in range(1, 9)}
        self.cr[4] = ITB
        self.mem = {}                 # physical tryte -> value
        self.pc = 0
        self.halted = 0
        self.out = []

    def copy(self):
        s = State()
        s.r, s.v, s.mode, s.cr, s.mem = dict(self.r), dict(self.v), dict(self.mode), dict(self.cr), dict(self.mem)
        s.pc, s.halted, s.out = self.pc, self.halted, list(self.out)
        return s

    def mode_value(self):
        return sum(self.mode[k] * 3 ** (27 - p) for k, p in MODE_POS.items())

    def set_mode_value(self, v):
        for k, p in MODE_POS.items():
            self.mode[k] = field(v, p, p)

    def setr(self, i, v):
        if i != 0:
            self.r[i] = v

    def setv(self, i, w):
        if i != 0:
            self.v[i] = tuple(w)

    # physical memory
    def bus_ok(self, pa):
        return -TMAX <= pa <= RAM_TOP

    def pread(self, pa, va, acc):
        if not self.bus_ok(pa):
            raise Fault(-10, va, acc)
        return self.mem.get(pa, 0)

    def pwrite(self, pa, t, va):
        if not self.bus_ok(pa):
            raise Fault(-10, va, WRITE)
        self.mem[pa] = t

    def pword(self, pa, va, acc):
        return self.pread(pa - 1, va, acc) * 3 ** 18 + self.pread(pa, va, acc) * T9 + self.pread(pa + 1, va, acc)

    def pwword(self, pa, v, va):
        t = trits(v)
        for k in range(3):
            self.pwrite(pa - 1 + k, untrits(t[9 * k:9 * k + 9]), va)

    # the MMU (§5)
    def translate(self, va, acc):
        if self.mode['PG'] != 1:
            return va
        ring = self.mode['RING']
        e1a = self.cr[1] + 3 * field(va, 1, 9)
        e1 = self.pword(e1a, va, acc)
        self.check_pte(e1, acc, ring, va)
        e2a = PAGE * field(e1, 1, 18) + 3 * field(va, 10, 18)
        e2 = self.pword(e2a, va, acc)
        self.check_pte(e2, acc, ring, va)
        ad = field(e2, 22, 22)
        new = 1 if acc == WRITE else (-1 if ad == 0 else ad)
        if new != ad:
            self.pwword(e2a, with_field(e2, 22, 22, new), va)
        return PAGE * field(e2, 1, 18) + field(va, 19, 27)

    @staticmethod
    def check_pte(e, acc, ring, va):
        v, x, r = field(e, 19, 19), field(e, 20, 20), field(e, 21, 21)
        if v == 0:
            raise Fault(-3, va, acc)
        ok = ring >= r and {WRITE: v == 1, FETCH: x != 0, READ: x != -1}[acc]
        if not ok:
            raise Fault(-4, va, acc)

    # virtual accesses
    def load_tryte(self, va):
        return self.pread(self.translate(va, READ), va, READ)

    def check_word(self, va, acc):
        if va % 3 != 0:
            raise Fault(-5, va, acc)
        return self.translate(va, acc)

    def load_word(self, va, acc=READ):
        pa = self.check_word(va, acc)
        return self.pword(pa, va, acc)

def lane_trytes(w):
    out = []
    for word in w:
        out += [field(word, 1, 9), field(word, 10, 18), field(word, 19, 27)]
    return out

def from_trytes(t):
    return tuple(t[3 * k] * 3 ** 18 + t[3 * k + 1] * T9 + t[3 * k + 2] for k in range(3))

LANE_OK = {1, 2, 11, 12, 13, 14, 15, 19, 20, 21}       # ADD SUB MIN MAX TMUL TSAT CONS NEG SGN ABS

def lane_op(f, a, b, n):
    w = lambda v: wrapn(v, n)
    if f == 1: return w(a + b)
    if f == 2: return w(a - b)
    if f == 11: return tritwise(a, b, min, n)
    if f == 12: return tritwise(a, b, max, n)
    if f == 13: return tritwise(a, b, lambda x, y: x * y, n)
    if f == 14: return tritwise(a, b, lambda x, y: sign(x + y), n)
    if f == 15: return tritwise(a, b, lambda x, y: x if x == y else 0, n)
    if f == 19: return -a
    if f == 21: return w(abs(a))
    return sign(a)

def alu(st, f, a, b, rd_old):
    """(result or None for CMP); sets S and C on st."""
    m = st.mode
    def arith(v):
        lo = wrap(v)
        m['C'] = (v - lo) // W
        return lo
    if f == 1: r = arith(a + b)
    elif f == 2: r = arith(a - b)
    elif f == 3: r = arith(a + b + m['C'])
    elif f == 4: r = arith(a - b + m['C'])
    elif f in (5, 6):
        p = a * b
        lo = wrap(p)
        hi = (p - lo) // W
        if f == 5:
            m['C'] = sign(hi)
            r = lo
        else:
            r = hi
    elif f in (7, 8):
        if b == 0:
            raise Fault(-6)
        q = abs(a) // abs(b) * sign(a) * sign(b)
        r = q if f == 7 else a - b * q
    elif f in (9, 10):
        if b == 0:
            raise Fault(-6)
        md = abs(b)
        rem = a % md
        if 2 * rem > md:
            rem -= md
        r = (a - rem) // b if f == 9 else rem
    elif f == 11: r = tritwise(a, b, min)
    elif f == 12: r = tritwise(a, b, max)
    elif f == 13: r = tritwise(a, b, lambda x, y: x * y)
    elif f == 14: r = tritwise(a, b, lambda x, y: sign(x + y))
    elif f == 15: r = tritwise(a, b, lambda x, y: x if x == y else 0)
    elif f == 16:
        if b >= 27 or b <= -27:
            r = 0
        elif b >= 0:
            r = wrap(a * 3 ** b)
        else:
            k = -b
            r = (a - wrapn(a, k)) // 3 ** k
    elif f == 17:
        m['S'] = sign(a - b)
        return None
    elif f == 18: r = trits(a)[b - 1] if 1 <= b <= 27 else 0
    elif f == 19: r = -a
    elif f == 20: r = sign(a)
    elif f == 21: r = abs(a)
    elif f == 22:
        t = trits(a)
        r = next((i for i, d in enumerate(t) if d != 0), 27)
    elif f == 23: r = wrap(a & b)
    elif f == 24: r = wrap(a | b)
    elif f == 25: r = wrap(a ^ b)
    elif f == 26: r = wrap(a & ~b)
    elif f == 27:
        y = max(-81, min(81, b))
        r = wrap(a * 2 ** y) if y >= 0 else a >> -y
    elif f == 28: r = merge(rd_old, a, b)
    elif f in (29, 30, 31):
        if not 0 <= b <= 27:
            raise Fault(-1)
        n = 27 if b == 0 else b
        low = trits(a)[27 - n:]
        if f == 29: r = low.count(1)
        elif f == 30: r = low.count(-1)
        else: r = untrits(low[::-1])
    else:
        raise Fault(-1)
    m['S'] = sign(r)
    return r

def execute(st):
    """One instruction on st (in place), as §3: returns nothing; raises Fault."""
    pc = st.pc
    w1 = st.load_word(pc, FETCH)
    a, b = field(w1, 1, 9), field(w1, 10, 18)
    x, y, z = field(w1, 19, 21), field(w1, 22, 24), field(w1, 25, 27)
    lt = trits(w1)[0]
    if lt == -1:
        if st.mode['FE'] != 1:
            raise Fault(-12)
        raise AssertionError('FPU arithmetic is not modelled here')
    imm, nxt = (0, pc + 3)
    if lt == 1:
        imm, nxt = st.load_word(pc + 3, FETCH), pc + 6
    imm6 = y * 27 + z
    rel = lambda off: wrap(pc + 3 * off)
    R = st.r
    if a == 0:
        priv = b in (2, 5, 6, 7, 8, 9)
        if priv and st.mode['RING'] != 1:
            raise Fault(-2)
        if b in (1, 10, 8, 9):
            pass
        elif b == 2:
            st.halted = 1
        elif b == 3:
            st.pc = nxt
            raise Fault(-7, nxt=True)
        elif b == 4:
            st.pc = nxt
            raise Fault(0, info=x * 729 + y * 27 + z, nxt=True)
        elif b == 5:
            st.set_mode_value(st.cr[6])
            st.pc = st.cr[5]
            if st.mode['RING'] < 1:
                st.r[13], st.cr[7] = st.cr[7], st.r[13]
            return
        elif b == 6:
            st.setr(x, st.cr.get(y, 0) if 1 <= y <= 8 else (st.mode_value() if y == 0 else (CPUID if y == 9 else 0)))
        elif b == 7:
            if not 0 <= x <= 8:
                raise Fault(-1)
            if x == 0:
                st.set_mode_value(R[y])
            else:
                st.cr[x] = R[y]
        else:
            raise Fault(-1)
    elif a in (ALU, ALUQ, ALUI):
        bv = R[z] if a == ALU else (z if a == ALUQ else imm)
        res = alu(st, b, R[y], bv, R[x])
        if res is not None:
            st.setr(x, res)
    elif a in (MEM, MEMD):
        size = 1 if b in (1, 3) else 3
        va = wrap(R[y] + (size * z if a == MEM else imm))
        if b == 1:
            st.setr(x, st.load_tryte(va))
        elif b == 2:
            st.setr(x, st.load_word(va))
        elif b == 3:
            pa = st.translate(va, WRITE)
            st.pwrite(pa, wrapn(R[x], 9), va)
        elif b == 4:
            pa = st.check_word(va, WRITE)
            st.pwword(pa, R[x], va)
        elif b == 5:
            old = st.load_word(va)
            pa = st.check_word(va, WRITE)
            st.pwword(pa, R[x], va)
            st.setr(x, old)
        else:
            raise Fault(-1)
    elif a == MEMR:
        if b not in (1, 2):
            raise Fault(-1)
        va = R[y]
        pa = st.check_word(va, WRITE)
        old = st.load_word(va)
        if b == 1:
            st.pwword(pa, merge(old, R[z], R[x]), va)
        else:
            s = sign(old - R[x])
            if s == 0:
                st.pwword(pa, R[z], va)
            st.mode['S'] = s
            st.setr(x, old)
    elif a in (IO, IOI):
        if st.mode['RING'] < st.mode['IOPL']:
            raise Fault(-2)
        port = R[y] + z if a == IO else imm
        if b == 1:
            st.setr(x, wrap(3 * port + 1))
        elif b == 2:
            st.out.append((port, R[x]))
        else:
            raise Fault(-1)
    elif a == JALR:
        target = wrap(R[y] + 3 * z)
        st.setr(x, wrap(nxt))
        st.pc = target
        return
    elif a == LIQ:
        st.setr(x, imm6)
    elif a == LI:
        st.setr(x, imm)
    elif a == LEA:
        st.setr(x, wrap(pc + imm))
    elif a == JAL:
        st.setr(x, wrap(nxt))
        st.pc = rel(imm)
        return
    elif a in (BRQ, BR):
        off = imm6 if a == BRQ else imm
        if x != 0:
            t = trits(x, 3)
            pos, neg = 1 in t, -1 in t
            if pos and neg:
                raise Fault(-1)
            flag = st.mode['S'] if pos else st.mode['C']
            if t[flag + 1] == (1 if pos else -1):
                st.pc = rel(off)
                return
    elif a == BR3:
        s = st.mode['S']
        if s == -1:
            st.pc = rel(field(imm, 1, 9))
            return
        if s == 1:
            st.pc = rel(field(imm, 10, 18))
            return
    elif a == FLD:
        pos, ln = field(imm, 1, 9), field(imm, 10, 18)
        ra = R[y]
        if b in (1, 2):
            if pos < 1 or ln < 1 or pos + ln - 1 > 27:
                raise Fault(-1)
            res = field(ra, pos, pos + ln - 1) if b == 1 else with_field(R[x], pos, pos + ln - 1, ra)
        elif b in (3, 4):
            if not 1 <= ln <= 41:
                raise Fault(-1)
            mask = (1 << ln) - 1
            if b == 3 and 0 <= pos <= 80:
                res = (ra >> pos) & mask
            elif b == 4 and 0 <= pos < ln:
                v = ra & mask
                res = ((v << pos) | (v >> (ln - pos))) & mask
            else:
                raise Fault(-1)
        else:
            raise Fault(-1)
        st.mode['S'] = sign(res)
        st.setr(x, res)
    elif a in (VALU, VMEM, VMOV):
        if st.mode['VE'] != 1:
            raise Fault(-13)
        V = st.v
        if a == VALU:
            if 1 <= b <= 54:
                f, words = (b - 27, True) if b > 27 else (b, False)
                if f not in LANE_OK:
                    raise Fault(-1)
                if words:
                    st.setv(x, [lane_op(f, V[y][k], V[z][k], 27) for k in range(3)])
                else:
                    ta, tb = lane_trytes(V[y]), lane_trytes(V[z])
                    st.setv(x, from_trytes([lane_op(f, ta[j], tb[j], 9) for j in range(9)]))
            elif b == 55:
                st.setv(x, [merge(V[x][k], V[y][k], V[z][k]) for k in range(3)])
            elif b == 56:
                st.setr(x, sum(lane_trytes(V[y])))
            elif b == 57:
                ta, tw = lane_trytes(V[y]), lane_trytes(V[z])
                st.setr(x, sum(ta[j] * trits(tw[j], 9)[8] for j in range(9)))
            else:
                raise Fault(-1)
        elif a == VMEM:
            if b not in (1, 2):
                raise Fault(-1)
            va = R[y] + 9 * z
            acc = WRITE if b == 2 else READ
            if va % 9 != 0:
                raise Fault(-5, va, acc)
            if b == 1:
                st.setv(x, [st.load_word(wrap(va + 3 * k)) for k in range(3)])
            else:
                pas = [st.check_word(wrap(va + 3 * k), WRITE) for k in range(3)]
                for k in range(3):
                    st.pwword(pas[k], V[x][k], wrap(va + 3 * k))
        else:
            if b in (1, 2):
                if not 0 <= z <= 8:
                    raise Fault(-1)
            elif b in (3, 4):
                if not 0 <= z <= 2:
                    raise Fault(-1)
            if b == 1:
                st.setr(x, lane_trytes(V[y])[z])
            elif b == 2:
                t = lane_trytes(V[x])
                t[z] = wrapn(R[y], 9)
                st.setv(x, from_trytes(t))
            elif b == 3:
                st.setr(x, V[y][z])
            elif b == 4:
                w = list(V[x])
                w[z] = R[y]
                st.setv(x, w)
            elif b == 5:
                st.setv(x, from_trytes([wrapn(R[y], 9)] * 9))
            elif b == 6:
                st.setv(x, [R[y]] * 3)
            else:
                raise Fault(-1)
    else:
        raise Fault(-1)
    st.pc = nxt

CPUID = 7 + 3 ** 26

def step(pre):
    """The state after one instruction (or its trap), and the fault if any.
    A faulting instruction changes nothing but the AD trits its address
    translations set; a single-step trap (-8) comes after a completed one."""
    work = pre.copy()
    try:
        execute(work)
        if pre.mode['TF'] == 1 and not work.halted:
            raise Fault(-8, nxt=True)
        return work, None
    except Fault as f:
        if f.v == -8:
            st = work
        else:
            st = pre.copy()
            st.mem = keep_ad(pre.mem, work.mem)
        deliver(st, f, work.pc if f.nxt else pre.pc)
        return st, f

def keep_ad(before, after):
    """Memory after a faulting instruction: only page-table AD updates."""
    m = dict(before)
    for k, v in after.items():
        if before.get(k, 0) != v and k in AD_TRYTES:
            m[k] = v
    return m

AD_TRYTES = set()                     # set by MMU cases: trytes holding E2 entries

def deliver(st, f, epc):
    ring = st.mode['RING']
    st.cr[6] = st.mode_value()
    st.cr[5] = wrap(epc)
    st.fault = f
    st.fring = ring
    if ring < 1:
        st.r[13], st.cr[7] = st.cr[7], st.r[13]
    st.mode['RING'], st.mode['IE'], st.mode['TF'] = 1, 0, 0
    st.halted = 0
    st.pc = HANDLER + 3 * f.v
    # (the runner's table is identity-mapped: reading it changes nothing)

# ---- writing vectors ------------------------------------------------------------------

def state_tokens(st, base=None, trapped=False):
    out = []
    for i in range(-13, 14):
        if i and st.r[i] != (base.r[i] if base else 0):
            out.append(f'r{i}={st.r[i]}')
    for i in range(-13, 14):
        if i and st.v[i] != (base.v[i] if base else (0, 0, 0)):
            out.append(f'v{i}={st.v[i][0]},{st.v[i][1]},{st.v[i][2]}')
    for k in MODE_POS:
        ref = base.mode[k] if base else MODE_DEFAULT[k]
        if st.mode[k] != ref:
            out.append(f'{k}={st.mode[k]}')
    for i in range(1, 9):
        if i in (2, 3) and trapped:
            continue
        ref = base.cr[i] if base else (ITB if i == 4 else 0)
        if st.cr[i] != ref:
            out.append(f'cr{i}={st.cr[i]}')
    keys = sorted(set(st.mem) | (set(base.mem) if base else set()))
    for k in keys:
        v = st.mem.get(k, 0)
        if base is None or base.mem.get(k, 0) != v:
            out.append(f't{k}={v}')
    return out

NAMES = {}

def emit(name, pre, code):
    """code: list of (physical address, word)."""
    for addr, wd in code:
        t = trits(wd)
        for k in range(3):
            pre.mem[addr - 1 + k] = untrits(t[9 * k:9 * k + 9])
    st, f = step(pre)
    code_mem = {addr - 1 + k for addr, _ in code for k in range(3)}
    pre_tok = [t for t in state_tokens(pre) if not (t.startswith('t') and int(t[1:].split('=')[0]) in code_mem)]
    post = state_tokens(st, pre, f is not None)
    post.append(f'pc={st.pc}')
    if st.halted:
        post.append('halted=1')
    for port, val in st.out:
        post.append(f'out={port}:{val}')
    if f is not None:
        post.append(f'trap={f.v}')
        post.append(f'fring={st.fring}')
        if f.v in (-3, -4, -5, -10):
            post.append(f'facc={f.acc}')
            post.append(f'faddr={wrap(f.addr)}')
        if f.v == 0:
            post.append(f'finfo={f.info}')
    n = NAMES.get(name, 0) + 1
    NAMES[name] = n
    code_tok = ' '.join(f'{a}:{w}' for a, w in code)
    LINES.append(f'{name}.{n} | {" ".join(pre_tok)} | {code_tok} | {" ".join(post)}')

LINES = []

# ---- the cases -----------------------------------------------------------------------

rnd = random.Random(27)

EDGE = [0, 1, -1, 2, -2, 3, -3, 13, -13, 14, 364, -364, 9841, -9841, 9842, 3 ** 13, -(3 ** 13), 3 ** 26, -(3 ** 26),
        WMAX, -WMAX, WMAX - 1, -WMAX + 1, (WMAX + 1) // 2, 2 ** 32, -(2 ** 32), 2 ** 41 - 1, -(2 ** 41)]

def rword():
    k = rnd.random()
    if k < 0.3:
        return rnd.choice(EDGE)
    if k < 0.5:
        return rnd.randint(-400, 400)
    return rnd.randint(-WMAX, WMAX)

def fresh(**mode):
    st = State()
    st.mode.update(mode)
    return st

ALU_NAMES = {1: 'add', 2: 'sub', 3: 'adc', 4: 'sbc', 5: 'mul', 6: 'mulh', 7: 'div', 8: 'mod', 9: 'divr', 10: 'modr',
             11: 'min', 12: 'max', 13: 'tmul', 14: 'tsat', 15: 'cons', 16: 'sh', 17: 'cmp', 18: 'trit', 19: 'neg',
             20: 'sgn', 21: 'abs', 22: 'tlz', 23: 'band', 24: 'bor', 25: 'bxor', 26: 'bandn', 27: 'bsh', 28: 'mrg',
             29: 'tcp', 30: 'tcn', 31: 'trev'}

def small_b(f):
    """A second operand suited to f."""
    if f == 16: return rnd.choice([0, 1, -1, 5, -5, 13, -13, 26, -26, 27, -27, 28, 40, -41, 1000, -WMAX])
    if f == 18: return rnd.choice([0, 1, 2, 13, 26, 27, 28, -1, 100])
    if f == 27: return rnd.choice([0, 1, -1, 7, -7, 32, -32, 41, -41, 63, -64, 80, 81, -81, 82, -90, 500])
    if f in (29, 30, 31): return rnd.choice([0, 1, 5, 9, 26, 27, 28, -1])
    if f in (7, 8, 9, 10) and rnd.random() < 0.15: return 0
    if f in (9, 10) and rnd.random() < 0.3: return rnd.choice([2, -2, 4, -4, 6, 10])
    return rword()

def alu_cases():
    for f in range(1, 32):
        nm = ALU_NAMES[f]
        for k in range(60):
            a = rword()
            b = small_b(f)
            if f in (9, 10) and b in (2, -2, 4, -4, 6, 10) and rnd.random() < 0.5:
                a = b // 2 * rnd.choice([1, 3, -1, -5])       # ties
            st = fresh(S=rnd.choice([-1, 0, 1]), C=rnd.choice([-1, 0, 1]))
            rd, ra, rb = rnd.choice([1, 2, -5, 13]), 3, 4
            if k % 10 == 7:
                rd = 3                                        # rd = ra
            if k % 15 == 11:
                rd = 0                                        # result dropped
            st.r[3], st.r[4] = a, b
            if rd not in (0, 3, 4):
                st.r[rd] = rword()
            emit(f'alu.{nm}', st, [(0, enc(ALU, f, rd, ra, rb))])
        for k in range(16):
            q = rnd.randint(-13, 13)
            st = fresh(S=rnd.choice([-1, 0, 1]), C=rnd.choice([-1, 0, 1]))
            st.r[5] = rword()
            st.r[6] = rword()
            emit(f'aluq.{nm}', st, [(0, enc(ALUQ, f, 6, 5, q))])
        for k in range(16):
            st = fresh(S=rnd.choice([-1, 0, 1]), C=rnd.choice([-1, 0, 1]))
            st.r[-7] = rword()
            st.r[8] = rword()
            emit(f'alui.{nm}', st, [(0, enc(ALUI, f, 8, -7)), (3, small_b(f) if rnd.random() < 0.5 else rword())])
    # r0 reads 0 as an operand
    for f in (1, 2, 17, 19):
        st = fresh()
        st.r[2] = 77
        emit('alu.r0', st, [(0, enc(ALU, f, 1, 0, 2))])
    # reserved functions
    for f in (0, 32, 40, 9841, -1, -9841):
        for a in (ALU, ALUQ):
            st = fresh(S=1, C=-1)
            st.r[1] = 5
            emit('alu.reserved', st, [(0, enc(a, f, 1, 1, 2))])
        emit('alu.reserved', fresh(), [(0, enc(ALUI, f, 1, 1)), (3, 9)])

def fld_cases():
    for k in range(80):
        st = fresh(S=rnd.choice([-1, 0, 1]), C=rnd.choice([-1, 0, 1]))
        st.r[1], st.r[2] = rword(), rword()
        b = rnd.choice([1, 2])
        if rnd.random() < 0.8:
            ln = rnd.randint(1, 27)
            pos = rnd.randint(1, 28 - ln)
        else:
            pos, ln = rnd.choice([(0, 3), (1, 0), (27, 2), (-1, 5), (5, -2), (1, 28), (25, 4)])
        emit('fld.ext' if b == 1 else 'fld.ins', st, [(0, enc(FLD, b, 1, 2)), (3, pos * 3 ** 18 + ln * T9)])
    for k in range(60):
        st = fresh(C=1)
        st.r[2] = rword()
        b = rnd.choice([3, 4])
        if rnd.random() < 0.8:
            ln = rnd.randint(1, 41)
            pos = rnd.randint(0, 80) if b == 3 else rnd.randint(0, ln - 1)
        else:
            pos, ln = rnd.choice([(0, 0), (0, 42), (81, 8), (-1, 8), (8, 8), (40, 41)])
        emit('fld.bext' if b == 3 else 'fld.brot', st, [(0, enc(FLD, b, 1, 2)), (3, pos * 3 ** 18 + ln * T9)])
    for b in (0, 5, -1):
        emit('fld.reserved', fresh(), [(0, enc(FLD, b, 1, 2)), (3, 3 ** 18 + T9)])

DATA = 6000

def put_word(st, addr, v):
    t = trits(v)
    for k in range(3):
        st.mem[addr - 1 + k] = untrits(t[9 * k:9 * k + 9])

def mem_cases():
    for k in range(120):
        st = fresh()
        for d in range(-9, 12, 3):
            put_word(st, DATA + d, rword())
        st.r[2] = DATA + rnd.choice([0, 3, -3, 1, -1, 2])
        st.r[1] = rword()
        b = rnd.choice([1, 2, 3, 4, 5])
        if rnd.random() < 0.6:
            q = rnd.randint(-3, 3)
            emit(f'mem.{["", "ldt", "ldw", "stt", "stw", "swap"][b]}', st, [(0, enc(MEM, b, 1, 2, q))])
        else:
            off = rnd.randint(-9, 9)
            emit(f'memd.{["", "ldt", "ldw", "stt", "stw", "swap"][b]}', st, [(0, enc(MEMD, b, 1, 2)), (3, off)])
    # a store's register is the loaded one when rd = ra
    st = fresh()
    put_word(st, DATA, 123456)
    st.r[2] = DATA
    emit('mem.ldw', st, [(0, enc(MEM, 2, 2, 2, 0))])
    # bus error: frame -10 has no device
    for b in (1, 2, 3, 4):
        st = fresh()
        st.r[2] = -10 * PAGE
        emit('mem.bus', st, [(0, enc(MEM, b, 1, 2, 0))])
    for b in (0, 6, -1):
        st = fresh()
        st.r[2] = DATA
        emit('mem.reserved', st, [(0, enc(MEM, b, 1, 2, 0))])
    # MEMR
    for k in range(60):
        st = fresh(S=rnd.choice([-1, 0, 1]))
        old = rword()
        put_word(st, DATA, old)
        st.r[2] = DATA if rnd.random() < 0.85 else DATA + rnd.choice([1, -1, 2])
        b = rnd.choice([1, 2])
        st.r[1] = old if (b == 2 and rnd.random() < 0.4) else rword()
        st.r[3] = rword()
        emit('memr.stwm' if b == 1 else 'memr.cas3', st, [(0, enc(MEMR, b, 1, 2, 3))])
    for b in (0, 3, -2):
        st = fresh()
        st.r[2] = DATA
        emit('memr.reserved', st, [(0, enc(MEMR, b, 1, 2, 3))])
    # misaligned: both are write accesses, checked before the read
    for b in (1, 2):
        for off in (1, -1):
            st = fresh()
            put_word(st, DATA, 5)
            st.r[1], st.r[2], st.r[3] = 5, DATA + off, 7
            emit('memr.misaligned', st, [(0, enc(MEMR, b, 1, 2, 3))])

def flow_cases():
    for k in range(20):
        st = fresh()
        v = rnd.randint(-364, 364)
        emit('liq', st, [(0, enc_s(LIQ, 0, rnd.choice([1, -13, 0]), v))])
        st = fresh()
        emit('li', st, [(0, enc(LI, 0, 4)), (3, rword())])
        st = fresh()
        emit('lea', st, [(0, enc(LEA, 0, 4)), (3, rword())])
        st = fresh()
        emit('jal', st, [(0, enc(JAL, 0, 12)), (3, rnd.choice([rnd.randint(-1000, 1000), rword()]))])
        st = fresh()
        st.r[5] = rnd.choice([3000, -3, rword()])
        rd = rnd.choice([12, 5, 0])
        emit('jalr', st, [(0, enc(JALR, 0, rd, 5, rnd.randint(-13, 13)))])
    for cond in range(-13, 14):
        for s in (-1, 0, 1):
            for c in (-1, 0, 1):
                if rnd.random() < 0.5:
                    continue
                st = fresh(S=s, C=c)
                if rnd.random() < 0.5:
                    emit('brq', st, [(0, enc_s(BRQ, 0, cond, rnd.randint(-364, 364)))])
                else:
                    emit('br', st, [(0, enc(BR, 0, cond)), (3, rnd.randint(-100000, 100000))])
    for s in (-1, 0, 1):
        for k in range(3):
            st = fresh(S=s)
            n, p = rnd.randint(-9841, 9841), rnd.randint(-9841, 9841)
            emit('br3', st, [(0, enc(BR3, 0)), (3, n * 3 ** 18 + p * T9)])

def sys_cases():
    for ring in (1, 0, -1):
        for b in range(0, 12):
            st = fresh(RING=ring, S=1, C=-1)
            st.r[13], st.cr[7] = 3000, 6000
            st.cr[5], st.cr[6] = 99, (st.mode_value() if b != 5 else
                                       sum(v * 3 ** (27 - MODE_POS[kk]) for kk, v in {'RING': -1, 'IE': 1, 'S': -1}.items()))
            emit(f'sys.{b}', st, [(0, enc(0, b, 2, 3, 4))])
    for b in (-1, 40, 9841):
        emit('sys.reserved', fresh(), [(0, enc(0, b))])
    for n in (0, 1, -1, 9841, -9841, 300):
        y, z = divmod_bal(n, 27)
        x, y = divmod_bal(y, 27)
        emit('sys.call', fresh(), [(0, enc(0, 4, x, y, z))])
    # MFC / MTC every control register
    for crn in range(-13, 14):
        if crn == 10:
            continue
        st = fresh()
        for i in range(1, 9):
            st.cr[i] = 1000 + i if i != 4 else ITB
        emit('sys.mfc', st, [(0, enc(0, 6, 1, crn))])
        st = fresh()
        st.r[2] = 3 * 777 if crn != 0 else State().mode_value() + 729   # MODE: S := +1
        if crn == 4:
            st.r[2] = ITB                                  # keep the table where the runner put it
        emit('sys.mtc', st, [(0, enc(0, 7, crn, 2))])
    # IRET to each ring
    for ring in (1, 0, -1):
        st = fresh()
        st.r[13], st.cr[7] = 3000, 6000
        target = State()
        target.mode.update(RING=ring, S=-1, C=1, IE=1)
        st.cr[6], st.cr[5] = target.mode_value(), 300
        emit('sys.iret', st, [(0, enc(0, 5))])
    # single step
    for b in (1, 4):
        emit('sys.tf', fresh(TF=1), [(0, enc(0, b))])
    st = fresh(TF=1)
    emit('sys.tf', st, [(0, enc(LIQ, 0, 1, 0, 5))])

def io_cases():
    for ring in (1, 0, -1):
        for iopl in (1, 0, -1):
            st = fresh(RING=ring, IOPL=iopl)
            st.r[13], st.cr[7] = 3000, 6000
            st.r[2], st.r[1] = 162, 4242
            b = rnd.choice([1, 2])
            emit('io', st, [(0, enc(IO, b, 1, 2, rnd.randint(-13, 13)))])
            emit('ioi', st.copy(), [(0, enc(IOI, b, 1)), (3, rnd.randint(-9841, 9841))])
    emit('io.reserved', fresh(), [(0, enc(IO, 3, 1, 2, 0))])

def reserved_cases():
    for a in (12, 13, 100, 3280, -1, -3280, 6570, 6600, 9841):
        st = fresh()
        code = [(0, enc(a, 1, 1, 2, 3))]
        if a > 3280:
            code.append((3, 5))
        emit('reserved', st, code)
    emit('reserved.zero', fresh(), [(0, 0)])
    # misaligned fetch: PC itself must be a multiple of 3 (the runner starts at 0); a jump to 1 is legal
    st = fresh()
    st.r[5] = 1
    emit('jalr.odd', st, [(0, enc(JALR, 0, 0, 5, 0))])
    # FPU off
    for a in (-6561, -6559, -4373):
        emit('fpu.off', fresh(FE=0), [(0, enc(a, 1, 1, 2, 3)), (3, 0)])

def vword():
    return (rword(), rword(), rword())

def vector_cases():
    for b in list(range(1, 58)) + [0, 58, -1]:
        for k in range(4 if b in LANE_OK or b - 27 in LANE_OK or b > 54 else 1):
            st = fresh(S=1, C=-1)
            st.v[1], st.v[2], st.v[3] = vword(), vword(), vword()
            if b == 57 and k == 0:
                st.v[3] = (1, -1, 3 ** 18 + 2)
            emit(f'valu.{b}', st, [(0, enc(VALU, b, 1, 2, 3))])
    for vd in (0, 4):
        st = fresh()
        st.v[2], st.v[3] = vword(), vword()
        emit('valu.v0', st, [(0, enc(VALU, 1, vd, 0, 3))])
    for b in (1, 2):
        for base in (5994, 6003, 5997, 5993):
            st = fresh()
            for d in range(-9, 30, 3):
                put_word(st, base + d, rword())
            st.v[1] = vword()
            st.r[2] = base
            emit('vmem.' + ('vld' if b == 1 else 'vst'), st, [(0, enc(VMEM, b, 1, 2, rnd.choice([0, 1, -1])))])
    st = fresh()
    st.r[2] = DATA
    emit('vmem.reserved', st, [(0, enc(VMEM, 3, 1, 2, 0))])
    for b in range(0, 8):
        for z in (0, 2, 8, 9, -1, 3):
            st = fresh()
            st.v[1], st.v[2] = vword(), vword()
            st.r[1], st.r[2] = rword(), rword()
            emit(f'vmov.{b}', st, [(0, enc(VMOV, b, 1, 2, z))])
    for a, b in ((VALU, 1), (VMEM, 1), (VMOV, 1)):
        st = fresh(VE=0)
        st.r[2] = DATA
        emit('vector.off', st, [(0, enc(a, b, 1, 2, 0))])

# MMU: an L1 table at 3^10 · 3, the L2 tables after it; identity maps for
# the code page (VA page 0) and the vector table's page (page 1), and
# test pages with each combination of flags.
PTB = 3 ** 10 * 3                     # 177147
L2 = PTB + 3 ** 10                    # an L2 table for L1 index 0

def pte(frame, v=1, x=1, ring=1, ad=0, g=0):
    e = frame * T9
    for pos, val in ((19, v), (20, x), (21, ring), (22, ad), (23, g)):
        e = with_field(e, pos, pos, val)
    return e

def mmu_state(**mode):
    st = fresh(PG=1, **mode)
    st.cr[1] = PTB
    put_word(st, PTB, pte(L2 // T9, ring=-1))         # L1[0] -> L2, open to users
    put_word(st, L2 + 0, pte(0, ring=-1, ad=-1))      # page 0: code
    put_word(st, L2 + 3, pte(1, ring=-1, ad=-1))      # page 1: the vector table
    for e2a in (L2 + 0, L2 + 3):
        for k in range(3):
            AD_TRYTES.add(e2a - 1 + k)
    return st

def mmu_cases():
    flags = [(1, 1, -1), (-1, 1, -1), (1, -1, -1), (1, 0, -1), (0, 1, -1), (1, 1, 1), (1, 1, 0), (-1, -1, -1)]
    for idx, (v, x, ring) in enumerate(flags):
        page = 2 + idx                                 # VA page 2.. -> frame 14 + idx
        frame = 14 + idx                               # (frames 8-13 hold the tables)
        for acc_op in ('ldw', 'stw', 'ldt', 'stt'):
            for cur in (1, -1):
                for ad in (0, -1, 1):
                    if rnd.random() < 0.45:
                        continue
                    st = mmu_state(RING=cur)
                    st.r[13], st.cr[7] = 3000, 6000
                    e2a = L2 + 3 * page
                    put_word(st, e2a, pte(frame, v, x, ring, ad))
                    for k in range(3):
                        AD_TRYTES.add(e2a - 1 + k)
                    va = page * T9 + 30 * rnd.choice([1, -1])
                    put_word(st, frame * T9 + (va - page * T9), 4242)
                    st.r[2], st.r[1] = va, 777
                    b = {'ldt': 1, 'ldw': 2, 'stt': 3, 'stw': 4}[acc_op]
                    emit(f'mmu.{acc_op}', st, [(0, enc(MEM, b, 1, 2, 0))])
        # instruction fetch from a code page with these flags
        for cur in (1, -1):
            st = mmu_state(RING=cur)
            st.r[13], st.cr[7] = 3000, 6000
            put_word(st, L2 + 0, pte(0, v, x, ring, 0))
            emit('mmu.fetch', st, [(0, enc(0, 1))])
    # E1 problems: not present, and an E1 that forbids users
    for e1 in (0, pte(L2 // T9, ring=1), pte(L2 // T9, v=-1, ring=-1)):
        for op_b in (2, 4):
            st = mmu_state(RING=-1)
            st.r[13], st.cr[7] = 3000, 6000
            put_word(st, PTB + 3, e1)                 # L1[1] covers VA 3^18 ..
            st.r[2] = 3 ** 18
            emit('mmu.e1', st, [(0, enc(MEM, op_b, 1, 2, 0))])
    # an instruction fetch that is denied: code page not executable
    st = mmu_state()
    put_word(st, L2 + 0, pte(0, x=0, ring=-1, ad=-1))
    emit('mmu.nofetch', st, [(0, enc(0, 1))])
    st = mmu_state()
    put_word(st, L2 + 0, pte(0, v=0))
    emit('mmu.nocode', st, [(0, enc(0, 1))])
    # SWAP on a read-only page: the read marks it accessed, the write faults
    st = mmu_state()
    put_word(st, L2 + 6, pte(14, v=-1, ring=-1, ad=0))
    for k in range(3):
        AD_TRYTES.add(L2 + 6 - 1 + k)
    put_word(st, 14 * T9 + 30, 55)
    st.r[2] = 2 * T9 + 30
    emit('mmu.swap', st, [(0, enc(MEM, 5, 1, 2, 0))])
    # STWM and CAS3 on a read-only page: the write check comes first, so the
    # page is not even marked accessed (unlike SWAP above)
    for b in (1, 2):
        st = mmu_state()
        put_word(st, L2 + 6, pte(14, v=-1, ring=-1, ad=0))
        put_word(st, 14 * T9 + 30, 55)
        st.r[1], st.r[2], st.r[3] = 55, 2 * T9 + 30, 9
        emit('mmu.memr', st, [(0, enc(MEMR, b, 1, 2, 3))])

def main():
    alu_cases()
    fld_cases()
    mem_cases()
    flow_cases()
    sys_cases()
    io_cases()
    reserved_cases()
    vector_cases()
    mmu_cases()
    sys.stdout.reconfigure(encoding='utf-8', newline='\n')
    print('# Setun-11̅ ISA conformance vectors (docs/SETUN26_ISA.md §16), made by')
    print('# tools/gen_isavectors.py from an independent model of the specification.')
    print('#')
    print('# name | state before | code | state after one instruction')
    print('#')
    print('# The machine: RAM of 27 frames, zero except as given; PC = 0; MODE')
    print('# RING=1 IE=0 PG=0 FE=1 IOPL=0 TF=0 S=0 C=0 VE=1 except as given; ITB')
    print(f'# (cr4) = {ITB}, and the vector table there holds {HANDLER} + 3v for each')
    print('# vector v (the runner writes it); other control registers 0. IN from')
    print('# port p reads 3p + 1, wrapped.')
    print('#')
    print('# Tokens: rN=, vN=w0,w1,w2 (word lanes), the MODE fields by name, crN=,')
    print('# tA= (the tryte at physical address A); code is address:word pairs.')
    print('# After: only what changed, plus pc=, halted=1, out=port:value, and for')
    print('# a trap: trap= (vector), fring= (FCAUSE ring) and, for memory faults,')
    print('# facc= (FCAUSE access) and faddr= (FADDR); finfo= for SYS. FCAUSE and')
    print('# FADDR are compared only through these.')
    for line in LINES:
        print(line)
    print(f'# {len(LINES)} vectors', file=sys.stderr)

main()
