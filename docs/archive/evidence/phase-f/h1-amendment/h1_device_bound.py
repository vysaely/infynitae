#!/usr/bin/env python3
"""H-1, formulation B: independent check of the quantised device bound (exact integers; no Core code).

  W(s, D) = floor(120*D / s)      wire units (1/120) of the bound, s = s_max > 0
  H_dev   = W / 120               (+inf when s = 0; 1/120 when W = 0: stated residual)

Sealed today: H_dev = D/s, emitted as round(120*D/s).
"""
import random, struct
from fractions import Fraction as F

def W(s, D): return (120 * D) // s
def sealed(s, D):                      # round half away from zero of 120*D/s
    return (240 * D + s) // (2 * s)
def buf(s, w, mode):                   # client buffer side for logical side s at w/120
    n = s * w
    return {"floor": n // 120, "round": (2 * n + 120) // 240, "ceil": -(-n // 120)}[mode]

def exhaustive(D):
    over_sealed = over_b = not_max_exact = fits_one_more_round = dev_bound = lower = 0
    for s in range(1, 4 * D + 1):
        w, ws = W(s, D), sealed(s, D)
        if w < 360: dev_bound += 1            # below H_cfg = 3.0: the device bound is the ceiling
        if buf(s, ws, "round") > D: over_sealed += 1
        for mode in ("floor", "round", "ceil"):
            if w >= 1 and buf(s, w, mode) > D: over_b += 1
        if not (s * w <= 120 * D < s * (w + 1)): not_max_exact += 1   # largest w with s*w/120 <= D
        if buf(s, w + 1, "round") <= D: fits_one_more_round += 1
        if w < ws: lower += 1
        assert w // 120 == D // s                                      # b(h) unchanged
    return over_sealed, over_b, not_max_exact, fits_one_more_round, lower, dev_bound

print("D_max   sealed>D   B>D (3 rounding modes)   B not the exact maximum   W+1 still fits if the client rounds   B one step below sealed   sides")
for D in (4096, 8192, 16384):
    o, ob, nm, f1, lo, db = exhaustive(D)
    print(f"{D:<7} {o:<10} {ob:<24} {nm:<25} {f1:<37} {lo:<26} {4*D}")

# boundaries
D = 4096
for s, want in ((1, 120 * D), (D, 120), (D + 1, 119), (120 * D, 1), (120 * D + 1, 0), (2**31 - 1, 0)):
    assert W(s, D) == want, (s, W(s, D), want)
print("boundaries: s=1, s=D (W=120), s=D+1 (119), s=120*D (1), s=120*D+1 (0: residual) ok")

# f64: h = W/120.0 as a double; the wire value round(h*120) must give W back; h*s <= D must hold in doubles too
def f64(x): return struct.unpack("d", struct.pack("d", x))[0]
bad = 0
for w in range(1, 120 * 16384 + 1):
    if round(f64(w / 120.0) * 120.0) != w: bad += 1
rng = random.Random(1)
for _ in range(400_000):
    w = rng.randrange(1, 120 * (2**32 - 1))
    if w < 2**53 and int((f64(w / 120.0) * 120.0) + 0.5) != w: bad += 1
print(f"f64 round trip W -> W/120.0 -> round(.*120): {bad} mismatches (exhaustive to {120*16384}, 400,000 samples to 120*(2^32-1))")
assert 120 * (2**32 - 1) < 2**63
print("overflow: 120*D_max < 2^39 for every u32 D_max; computed in u64")

# rounding direction: nearest instead of floor re-creates the overshoot
print("rounding to nearest instead of down (D=4096): sides over the limit =",
      sum(1 for s in range(1, 4 * 4096 + 1) if buf(s, sealed(s, 4096), "ceil") > 4096), "(ceil clients),",
      sum(1 for s in range(1, 4 * 4096 + 1) if buf(s, sealed(s, 4096), "round") > 4096), "(rounding clients)")
