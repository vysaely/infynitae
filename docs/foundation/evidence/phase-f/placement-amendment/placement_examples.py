#!/usr/bin/env python3
"""Independent derivation of default-placement coordinates (exact rationals, no Core code).

Per axis: u = usable logical extent = view_px / output_scale; c = camera centre (Canvas px);
s = window-geometry size (logical px). Usable-region origin on the Canvas at zoom 1: c - u/2.
  fits     (s <= u): anchor = (c - u/2) + (u - s)/2 = c - s/2
  oversize (s >  u): anchor = c - u/2
One rounding to an integral Canvas point: round half to even (R3).
Output-logical position of the window's top-left: anchor - (c - u/2).
"""
from fractions import Fraction as F

def r3(v):                       # round half to even on an exact rational
    fl = v.numerator // v.denominator
    d = v - fl
    if d > F(1, 2) or (d == F(1, 2) and fl % 2):
        return fl + 1
    return fl

def axis(c, view_px, scale_120, s):
    u = F(view_px * 120, scale_120)
    a = F(c) - (F(s, 2) if s <= u else u / 2)
    return r3(a), u

CASES = [  # mode, scale_120, camera centre, window
    ((1280, 800), 120, (0, 0), (646, 513)),
    ((1280, 800), 120, (0, 0), (400, 300)),
    ((1280, 800), 120, (0, 0), (1280, 800)),
    ((1280, 800), 120, (0, 0), (2000, 300)),
    ((1280, 800), 120, (0, 0), (300, 2000)),
    ((1280, 800), 120, (0, 0), (5000, 4000)),
    ((1920, 1080), 180, (0, 0), (646, 513)),
    ((1920, 1080), 180, (0, 0), (800, 750)),
    ((3840, 2160), 240, (0, 0), (1900, 1000)),
    ((3840, 2160), 240, (0, 0), (2000, 1000)),
    ((1280, 800), 120, (9000, 700), (646, 513)),
    ((1280, 800), 120, (-5000, -3000), (646, 513)),
    ((1280, 800), 120, (-5000, -3000), (2000, 300)),
    ((1366, 768), 150, (0, 0), (801, 601)),     # fractional usable extent, half-pixel ties
    ((1366, 768), 150, (0, 0), (2000, 601)),    # oversized on a fractional extent
    ((1280, 800), 120, (0, 0), (645, 511)),     # ties on both axes, opposite parities
    ((1280, 800), 120, (1, 1), (645, 511)),
    ((1280, 800), 120, (0, 0), (1, 1)),
]
print("mode       scale  camera          window      anchor            output-logical top-left   fits")
for mode, sc, cam, win in CASES:
    (ax, ux), (ay, uy) = axis(cam[0], mode[0], sc, win[0]), axis(cam[1], mode[1], sc, win[1])
    ox, oy = F(ax) - (cam[0] - ux / 2), F(ay) - (cam[1] - uy / 2)
    fits = ("x" if win[0] <= ux else "-") + ("y" if win[1] <= uy else "-")
    print(f"{mode[0]}x{mode[1]:<5} {sc/120:<5} {str(cam):<15} {win[0]}x{win[1]:<6} ({ax}, {ay}){'':<6} ({float(ox):g}, {float(oy):g}){'':<10} {fits}")
