# Exact-rational reference for F-VIEW-1 / F-LATTICE-1 / F-UNPROJECT-1 / F-PROJECT-1, written from the
# canonical text only. Emits Rust table rows. Independent of the Rust implementation.
from fractions import Fraction as F
import math
Q = 1 << 16
DELTA = F(1, 1 << 20)
def rhe(x):  # round half to even, exact
    f = math.floor(x); d = x - f
    if d > F(1, 2): return f + 1
    if d < F(1, 2): return f
    return f if f % 2 == 0 else f + 1
def rha(x):  # round half away from zero (Rust f64::round), exact
    return math.floor(x + F(1, 2)) if x >= 0 else -math.floor(-x + F(1, 2))
def view(cq, scale_120, size, zoom=F(1)):
    k = zoom * F(scale_120, 120)
    out = []
    for c, n in zip(cq, size):
        vq = rhe(F(c) - F(n) * Q / (2 * k))
        r = math.floor(F(vq, 120 * Q)) * 120
        rel = F(vq - r * Q, Q)
        C = rha(rel * k + DELTA)
        ceff = r * Q + rhe(F(C) / k * Q)
        out.append(ceff)
    return out
def unproject(ceff, p, zoom=F(1)):
    return [c + rhe(F(pp) / zoom * Q) for c, pp in zip(ceff, p)]
def q16(px): return int(rhe(F(px) * Q))
rows = []
def case(name, centre_px, scale_120, size, pts):
    cq = [q16(centre_px[0]), q16(centre_px[1])]
    ce = view(cq, scale_120, size)
    for p in pts:
        q = unproject(ce, p)
        rows.append((name, cq, scale_120, size, p, ce, q))
k = lambda s: s / 120.0
# positive / negative / camera offsets / fractional cameras / INC-1 scales / awkward scales
case("origin camera k=1", (0, 0), 120, (1920, 1080), [(0.0, 0.0), (100.25, 50.5), (1919.5, 1079.5)])
case("negative fractional camera k=1.25", (-5000.5, -3000.25), 150, (2560, 1440), [(0.0, 0.0), (0.4, 0.4), (2047.6, 1151.2)])
case("d5 camera k=1.5", (2200.45, 900.15), 180, (3840, 2160), [((i + 0.5) / k(180), (j + 0.5) / k(180)) for i, j in [(0, 0), (1, 2), (3839, 2159), (1919, 1080)]])
case("k=1.75", (123.456, -78.9), 210, (1920, 1080), [((i + 0.5) / k(210), (j + 0.5) / k(210)) for i, j in [(0, 0), (7, 13), (1919, 1079)]])
for s in (132, 160, 200, 270):  # awkward scales 1.1, 1.333.., 1.666.., 2.25
    case(f"awkward k={s}/120", (-17.3, 41.7), s, (1366, 768), [((i + 0.5) / k(s), (j + 0.5) / k(s)) for i, j in [(0, 0), (683, 384), (1365, 767)]])
# exact pixel boundaries and near-boundary points (output-logical p = i/k exactly, and ± ε)
for s in (120, 150, 180):
    pts = []
    for i in (1, 37, 600):
        b = i / k(s)
        pts += [(b, b), (math.nextafter(b, -math.inf), math.nextafter(b, math.inf)), (b - 1e-7, b + 1e-7)]
    case(f"pixel boundaries k={s}/120", (300.0, -200.0), s, (1920, 1080), pts)
# large coordinates approaching ±2^36
L = float((1 << 36) - 10000)
case("near +2^36 k=1.75", (L, L / 3), 210, (3840, 2160), [(0.0, 0.0), (2193.7, 1233.1)])
case("near -2^36 k=1.25", (-L, -L / 3), 150, (2560, 1440), [(0.0, 0.0), (2047.9, 1151.9)])
print(f"// {len(rows)} rows: (case, centre Q16, scale_120, view size, p, c_eff Q16, q Q16)")
for name, cq, s, size, p, ce, q in rows:
    print(f'    ("{name}", ({cq[0]}, {cq[1]}), {s}, ({size[0]}, {size[1]}), ({p[0]!r}, {p[1]!r}), ({ce[0]}, {ce[1]}), ({q[0]}, {q[1]})),')
