#!/usr/bin/env python3
"""G-OB24 D3/D4 model checks (2026-10-17). Exact F-VIEW-1 / F-LATTICE-1 arithmetic; no product code.

T1  multi-output pans: each output has its own camera (zoom z), scale s, k = z*s, and its own
    reference R. Per pan, count frames where any on-screen width or gap changes, for
    SEALED (R = floor(v/120)*120 re-derived every frame) and KEPT (R fixed while k and view are
    unchanged). Outputs with different scales pan at the same time.
T2  lifecycle: a pan during which the output's scale changes once (D4: re-anchor at the event):
    KEPT may change widths at that one frame (k changed: widths are re-rounded by definition),
    and at no other frame.
T3  precision of a kept reference far from the view: edge(x) computed in f64 versus exactly
    (rational arithmetic) for |v - R| * k around 2^52 .. 2^57. Shows that the re-anchor bound
    must keep |v - R| * k < 2^52 (not a fixed 2^37 px) for f64 edges to stay exact.
"""
import math
import random
from fractions import Fraction as F

Q16 = 1 << 16
D = 2.0 ** -20


def q16(x):
    return round(x * Q16) / Q16


def edges_f64(xs, v, r, k):
    cc = round((v - r) * k + D)
    return [round((x - r) * k + D) - cc for x in xs]


def widths(es):
    return [b - a for a, b in zip(es, es[1:])]


def pan(z, s, view, frames, step, seed, mode, change_at=None, s2=None):
    rnd = random.Random(seed)
    k = z * s
    c = rnd.uniform(-1e6, 1e6)
    span = view / k
    xs = sorted(set(int(c - span + rnd.uniform(0, 3 * span)) for _ in range(300)))
    r_kept = None
    prev = None
    changes = []
    for f in range(frames):
        if change_at is not None and f == change_at:
            k = z * s2
            r_kept = None  # D4: k changed -> re-anchor
        c += step
        v = q16(c - view / (2 * k))
        if mode == "SEALED" or r_kept is None:
            r = math.floor(v / 120) * 120
            if mode == "KEPT":
                r_kept = r
        else:
            r = r_kept
        lo, hi = v, v + view / k
        vis = [x for x in xs if lo - 2 * span <= x <= hi + 2 * span]
        w = widths(edges_f64(vis, v, r, k))
        if prev is not None and len(prev) == len(w) and prev != w:
            changes.append(f)
        prev = w if prev is None or len(prev) == len(w) else w
    return changes


def t1():
    print("T1 multi-output pans (frames with a width/gap change per 240-frame pan, 4 seeds per output)")
    outputs = [(1.0, 1.0), (1.0, 1.25), (1.2345, 1.0), (0.9137, 1.25), (1.2345, 1.5), (0.7071, 2.0), (1.4142, 1.75)]
    for z, s in outputs:
        k = z * s
        res = {}
        for mode in ("SEALED", "KEPT"):
            res[mode] = sum(len(pan(z, s, 1920, 240, 7.3, seed, mode)) for seed in range(4))
        on = "k in n/120" if abs(k * 120 - round(k * 120)) < 1e-9 else "k not in n/120"
        print(f"  zoom {z} scale {s} k={k:.4f} ({on}): SEALED {res['SEALED']}  KEPT {res['KEPT']}")


def t2():
    print("T2 scale change during a pan (KEPT re-anchors at the event frame 120)")
    for z, s, s2 in [(1.2345, 1.0, 1.25), (0.9137, 1.25, 1.5), (1.4142, 1.5, 1.0)]:
        ch = []
        for seed in range(4):
            ch += pan(z, s, 1920, 240, 7.3, seed, "KEPT", change_at=120, s2=s2)
        other = [f for f in ch if f != 120]
        print(f"  zoom {z} scale {s}->{s2}: change frames {sorted(set(ch))}; outside the event: {len(other)}")


def exact_edge(x, v, r, k):
    d = F(1, 1 << 20)
    def rnd(q):  # round half away from zero, as Rust f64::round
        fl = math.floor(q)
        frac = q - fl
        return fl + 1 if frac >= F(1, 2) else fl
    return rnd(F(x - r) * k + d) - rnd((F(v) - F(r)) * k + d)


def t3():
    print("T3 kept reference far from the view: f64 edges vs exact (300 edges per row)")
    rnd = random.Random(7)
    # non-dyadic effective scales at the top of the domain (zoom up to 2^16, scale up to 8)
    for k in (F(65536) * F(73, 10), F(32768) * F(5, 4) * F(9, 10), F(4096) * F(7, 5), F(150) * F(3, 2)):
        kf = float(k)
        for e in (40, 46, 50, 52, 53, 55, 57):
            off = 2 ** e / kf  # |v - R| in Canvas px such that |v - R| * k = 2^e
            if off > 2 ** 37:
                continue
            r = 0
            v = q16(off)
            vv = F(round(v * Q16), Q16)
            xs = [int(v) + rnd.randint(0, 1000) for _ in range(300)]
            bad = 0
            for x in xs:
                a = round((x - r) * kf + D) - round((v - r) * kf + D)  # the product as Core computes it in f64
                b = exact_edge(x, vv, r, k)
                bad += a != b
            print(f"  k={float(k):g}  |v-R|*k ~ 2^{e}  (|v-R| = {off:.3g} px): {bad} wrong edges")


if __name__ == "__main__":
    t1()
    t2()
    t3()
