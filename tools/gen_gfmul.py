"""Generate c25519.tri's gf_mul with the 16x16 product unrolled into
column sums (constant offsets: each product is two loads, a multiply and
an add), then patch it into the file, replacing the one there.
Usage: python tools/gen_gfmul.py user/lib/c25519.tri"""
import re, sys

path = sys.argv[1]
src = open(path, encoding="utf-8").read()

lines = []
lines.append("// The product's 31 columns, written out (constant offsets, no loop")
lines.append("// counters: two loads, a multiply and an add per limb product;")
lines.append("// made by tools/gen_gfmul.py).")
lines.append("fn gf_mul(o: *gf, a: *gf, b: *gf) {")
lines.append("    var x: *int = &a.v[0];")
lines.append("    var y: *int = &b.v[0];")
lines.append("    var t: *int = &_t[0];")
for k in range(31):
    terms = [f"x[{i}] * y[{k - i}]" for i in range(16) if 0 <= k - i < 16]
    # break long sums over lines, four products each
    chunks = [" + ".join(terms[i:i + 4]) for i in range(0, len(terms), 4)]
    body = ("\n        + ").join(chunks)
    lines.append(f"    t[{k}] = {body};")
lines.append("    t[31] = 0;")
lines.append("    var i: int = 0;")
lines.append("    while (i < 31) {")
lines.append("        var c: int = divr(t[i], 65536);")
lines.append("        t[i] -= c * 65536;")
lines.append("        t[i + 1] += c;")
lines.append("        i += 1;")
lines.append("    }")
lines.append("    i = 0;")
lines.append("    while (i < 16) {")
lines.append("        o.v[i] = t[i] + 38 * t[i + 16];")
lines.append("        i += 1;")
lines.append("    }")
lines.append("    gf_carry(o);")
lines.append("    gf_carry(o);")
lines.append("}")
new = "\n".join(lines) + "\n"

start = src.index("// The product's 31 columns") if "// The product's 31 columns" in src else src.index("fn gf_mul(o: *gf, a: *gf, b: *gf) {")
end = src.index("fn gf_sq(", start)
src = src[:start] + new + "\n" + src[end:]
open(path, "w", encoding="utf-8", newline="\n").write(src)
print("ok")
