"""Generate bignum.tri's unrolled Montgomery column passes for 16 and 24
limbs (P-256, P-384) and patch them in before mont_mul, replacing any
made before. Usage: python tools/gen_mont.py user/lib/bignum.tri"""
import sys

path = sys.argv[1]
src = open(path, encoding="utf-8").read()

def gen(n):
    L = []
    L.append(f"// The columns of mont_mul for {n} limbs, written out (tools/gen_mont.py).")
    L.append(f"fn _mont_cols{n}(a: *int, b: *int, m: *int, minv: int) {{")
    L.append("    var u: *int = &_bn_u[0];")
    L.append("    var t: *int = &_bn_t[0];")
    L.append("    var acc: int = 0;")
    L.append("    var uk: int = 0;")
    for k in range(2 * n - 1):
        terms = [f"a[{i}] * b[{k - i}]" for i in range(n) if 0 <= k - i < n]
        terms += [f"u[{j}] * m[{k - j}]" for j in range(n) if 0 <= k - j < n and j < k and j < n]
        chunks = [" + ".join(terms[i:i + 4]) for i in range(0, len(terms), 4)]
        L.append("    acc = acc + " + "\n        + ".join(chunks) + ";")
        if k < n:
            L.append("    uk = bext(bext(acc, 0, 16) * minv, 0, 16);")
            L.append(f"    u[{k}] = uk;")
            L.append("    acc = bsh(acc + uk * m[0], -16);")
        else:
            L.append(f"    t[{k - n}] = bext(acc, 0, 16);")
            L.append("    acc = bsh(acc, -16);")
    L.append(f"    t[{n - 1}] = bext(acc, 0, 16);")
    L.append(f"    t[{n}] = bsh(acc, -16);")
    L.append("}")
    return "\n".join(L) + "\n\n"

marker = "// o = a·b·R^-1 mod m (o may be a or b), by product scanning"
# drop what an earlier run made
import re
while re.search(r"// The columns of mont_mul for \d+ limbs, written out", src):
    a = re.search(r"// The columns of mont_mul for \d+ limbs, written out", src).start()
    b = src.index("\n}\n", a) + 3
    src = src[:a] + src[b:].lstrip("\n")
i = src.index(marker)
src = src[:i] + gen(16) + gen(24) + src[i:]
open(path, "w", encoding="utf-8", newline="\n").write(src)
print("ok")
