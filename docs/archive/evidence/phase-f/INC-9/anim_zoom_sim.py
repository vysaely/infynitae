#!/usr/bin/env python3
"""Animated zoom under the adopted projection formulas (F-VIEW-1, F-LATTICE-1), exactly:
Q16 view origin (round-ties-even), 120-lattice anchor R = floor(v/120)*120 re-derived every frame
(k changes every frame), C = round((v - R)*k + d), edge(x) = round((x - R)*k + d) - C, d = 2^-20.

Compares Z1 (k follows the animation exactly) with Z2 (k snapped to the nearest n/120 on every
frame while k >= 1/120). Measures, per animation:
  - reversals: a width or gap moving against the zoom direction between consecutive frames
    (zooming in, nothing on screen may get narrower; zooming out, nothing may get wider);
  - flicker: a width returning to a value it had two frames before (w, w+1, w);
  - the largest per-frame relative zoom step and the largest per-frame jump of an edge's position
    beyond the ideal motion (discontinuity).
Deterministic (seeded). Output: one line per configuration.
"""
import math
import random

Q16 = 1 << 16
D = 2.0 ** -20


def q16(x):  # round-ties-even to the Q16 grid (R7)
    return round(x * Q16) / Q16


def frame(c, view, k):
    v = q16(c - view / (2 * k))
    r = math.floor(v / 120) * 120
    cc = round((v - r) * k + D)
    return r, cc


def edge(x, r, k, cc):
    return round((x - r) * k + D) - cc


def run(k0, k1, frames, mode, view=1920, seed=1):
    rnd = random.Random(seed)
    c = rnd.uniform(-5000, 5000)
    # windows and gaps spread across what is visible over the whole animation
    span = view / min(k0, k1)
    edges = sorted(set(int(c - span / 2 + rnd.uniform(0, span)) for _ in range(400)))
    prev = prev2 = None
    reversals = flicker = 0
    max_step = 0.0
    max_jump = 0.0
    zoom_in = k1 > k0
    ks = []
    for i in range(frames + 1):
        k = k0 * (k1 / k0) ** (i / frames)  # geometric: constant perceived speed
        if mode == "Z2" and k >= 1 / 120:
            k = max(1, round(k * 120)) / 120
        ks.append(k)
    for i, k in enumerate(ks):
        r, cc = frame(c, view, k)
        pos = [edge(x, r, k, cc) for x in edges]
        widths = [b - a for a, b in zip(pos, pos[1:])]
        if prev is not None:
            pk, ppos, pw = prev
            if k != pk:
                max_step = max(max_step, abs(k / pk - 1))
            for a, b in zip(pw, widths):
                if (zoom_in and b < a) or (not zoom_in and b > a):
                    reversals += 1
            # an edge's jump beyond the ideal motion between the two frames
            v_now = q16(c - view / (2 * k)); v_prev = q16(c - view / (2 * pk))
            for x, p0, p1 in zip(edges, ppos, pos):
                ideal = (x - v_now) * k - (x - v_prev) * pk
                max_jump = max(max_jump, abs((p1 - p0) - ideal))
            if prev2 is not None:
                for a, b, cnow in zip(prev2[2], pw, widths):
                    if cnow == a and b != a:
                        flicker += 1
        prev2, prev = prev, (k, pos, widths)
    distinct = len(set(ks))
    return reversals, flicker, max_step, max_jump, distinct, len(widths) * frames


def main():
    print("config | mode | width/gap reversals | flickers | pairs | max zoom step/frame | max edge jump beyond ideal (px) | distinct k")
    for (k0, k1, frames) in [(1.0, 2.0, 60), (2.0, 1.0, 60), (1.0, 1.1, 30), (0.25, 0.5, 60), (0.05, 0.1, 60), (0.01, 0.02, 60), (4.0, 8.0, 60)]:
        for mode in ("Z1", "Z2"):
            tot = [0, 0, 0.0, 0.0, 0, 0]
            for seed in range(1, 6):
                rv, fl, st, jp, ds, pairs = run(k0, k1, frames, mode, seed=seed)
                tot[0] += rv; tot[1] += fl; tot[2] = max(tot[2], st); tot[3] = max(tot[3], jp); tot[4] = max(tot[4], ds); tot[5] += pairs
            print(f"k {k0}->{k1} in {frames} frames x5 seeds | {mode} | {tot[0]} | {tot[1]} | {tot[5]} | {tot[2]*100:.2f}% | {tot[3]:.2f} | {tot[4]}")


if __name__ == "__main__":
    main()
