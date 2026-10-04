#!/usr/bin/env python3
"""CP3B MODEL (not an experiment): scale/geometry feedback (OB-28) and boundary oscillation.

Uses the D-CP3A-SCALE ladder/target from model.py. Compares:
  NAIVE  : every re-evaluation settles exactly (increase or decrease), whatever caused it
  EPOCH  : rule SC-E — an EPOCH starts at a stimulus that is not a client response (user/shell camera
           action, gesture end, output or membership change). Re-evaluations caused ONLY by the client's
           response to a hint (its geometry change) may move h only in the epoch's first direction.
           A reversal needs a new stimulus. => monotone within an epoch => <= |L| changes per epoch.
  DZI    : rule SC-D — a stream of discrete zoom steps (wheel ticks / key steps) is a "discrete-zoom
           interaction": like a gesture, increases at rung crossings, NO decreases until it ends; it ends
           at the first non-zoom input (button, key, pointer motion) or a settlement event.
"""
import math, importlib.util, sys, os
spec = importlib.util.spec_from_file_location("m", os.path.join(os.path.dirname(__file__), "model.py"))
src = open(spec.origin).read().split("def header(")[0]          # reuse ladder/target only, skip the walkthrough run
m = {}; exec(src, m)
ladder, target = m["ladder"], m["target"]

V = (1920, 1080)            # output A viewport (scale 1.0); output B scale 1.5 observes W at zoom 1
L, HMAX = ladder([1.0, 1.5], (800, 600))

def fit_zoom(g):            # viewport-fit re-fit from the client's logical geometry
    return min(V[0] / g[0], V[1] / g[1])

CLIENTS = {
    "compliant (logical size constant)":            lambda h: (800, 600),
    "constant-physical (logical = 1200x900 / h)":   lambda h: (1200 / h, 900 / h),
    "anti (logical grows with h)":                  lambda h: (500 * h, 375 * h),
    "toggling (size flips with each hint value)":   lambda h: (900, 675) if round(h * 120) % 2 == 0 else (700, 525),
    "adversarial (picks the size that inverts the last move)": None,
}

def run(client_name, policy, iters=60):
    h = 1.5                          # before the user's fit
    g = (800, 600)
    epoch_dir = 0                    # +1 up, -1 down, 0 undecided
    changes = []
    last_move = 0
    for i in range(iters):
        stimulus = (i == 0)          # i == 0: the USER's viewport-fit (a real stimulus); then only client responses
        z = fit_zoom(g)
        rho = max(1.0 * z, 1.5)
        t = target(rho, L, HMAX)
        if t != h:
            d = 1 if t > h else -1
            allowed = True
            if policy == "EPOCH":
                if stimulus:
                    epoch_dir = d
                elif epoch_dir == 0:
                    epoch_dir = d
                elif d != epoch_dir:
                    allowed = False      # reversal caused only by a client response: wait for a stimulus
            if allowed:
                changes.append((i, round(h, 4), round(t, 4)))
                last_move = d
                h = t
        # client responds to the current hint
        if CLIENTS[client_name] is None:
            # adversary: choose geometry that pushes rho against the last move (maximally oscillating)
            g = (1200, 900) if last_move > 0 else (300, 225)
        else:
            g = CLIENTS[client_name](h)
    tail = changes[-6:]
    osc = len(changes) >= 4 and len({c[2] for c in changes[-4:]}) <= 2 and changes[-1][0] > iters - 10
    return h, changes, osc

print("CP3B feedback MODEL. Ladder:", [round(x, 4) for x in L], "H_max", HMAX)
print("Scenario: user viewport-fits W on output A (stimulus), then 60 client-response rounds with no further stimulus.\n")
for name in CLIENTS:
    for pol in ("NAIVE", "EPOCH"):
        h, ch, osc = run(name, pol)
        print(f"{pol:<6} client={name:<58} hint changes={len(ch):>3} final h={h:.4f} still-oscillating-at-end={osc}")
        if pol == "NAIVE" and len(ch) > 6:
            print(f"        first changes: {ch[:6]}")
print()

# ---- wheel wiggle across a rung boundary ----
print("Wheel wiggle: camera A zoom alternates 1.57 <-> 1.59 for 40 ticks (rung boundary 1.5*1.05 = 1.575), B observes at 1.5;")
print("then the user clicks (non-zoom input) which ends a discrete-zoom interaction.")
def wiggle(policy):
    h = 1.5; changes = 0
    for i in range(40):
        z = 1.59 if i % 2 == 0 else 1.57
        t = target(max(z, 1.5), L, HMAX)
        if t > h or (t < h and policy == "NAIVE"):
            h = t; changes += 1
    # interaction end (click): settle exactly at the last zoom
    t = target(max(1.57, 1.5), L, HMAX)
    if t != h:
        h = t; changes += 1
    return changes, h
for pol in ("NAIVE", "DZI"):
    c, h = wiggle(pol)
    print(f"  {pol:<6} hint changes={c:>3} final h={h:.4f}")
