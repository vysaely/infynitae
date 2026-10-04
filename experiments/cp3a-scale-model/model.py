#!/usr/bin/env python3
"""CP3A unified scale policy, executable MODEL (not an experiment): computes walkthroughs A-E.

Rules (FOUNDATION_ARCHITECTURE CP3A-1):
  S1 rho(E)   = max over observing presentations of output_scale * presentation_zoom
               (protocol-fullscreen presentation view has presentation_zoom = 1)
  S2 ladder L = output scales  U  integers 1..floor(H_cfg)  U  geometric fill so that consecutive
               rungs differ by a ratio <= r;  every rung <= H_max(E)
  S3 H_max(E) = min(H_cfg, device bound D_max / max(logical w, h))   (rounded down to a rung)
  S4 target t(rho) = smallest rung >= rho / (1 + sigma)   (else H_max)
  S5 increase: if t > h  -> h := t immediately (gestures included)
  S6 decrease: if t < h  -> only when no camera gesture is active (gesture end / atomic change)
  S7 unobserved: hold
  S8 emitted fractional = round(h*120)/120 (sent only on change); integer = ceil(h)
CANDIDATE constants (provisional, NOT established): H_cfg=3.0, r=1.25, sigma=0.05. D_max=16384 (measured).
"""
import math

H_CFG, R, SIGMA, D_MAX = 3.0, 1.25, 0.05, 16384

def ladder(output_scales, logical_size):
    hmax_dev = D_MAX / max(logical_size)
    hmax = min(H_CFG, hmax_dev)
    base = sorted({round(s, 6) for s in output_scales if s <= hmax} | {float(i) for i in range(1, int(math.floor(hmax)) + 1)} | {hmax})
    rungs = [base[0]]
    for nxt in base[1:]:
        prev = rungs[-1]
        n = math.ceil(math.log(nxt / prev) / math.log(R) - 1e-9)  # pieces so each ratio <= R
        for i in range(1, n):
            rungs.append(prev * (nxt / prev) ** (i / n))
        rungs.append(nxt)
    return rungs, hmax

def target(rho, L, hmax):
    need = rho / (1 + SIGMA)
    for l in L:
        if l >= need - 1e-12:
            return min(l, hmax)
    return hmax

class Elem:
    def __init__(self, L, hmax, h0):
        self.L, self.hmax, self.h, self.events = L, hmax, h0, []
    def update(self, rho, gesture_active, label=""):
        if rho is None:            # unobserved: hold
            return
        t = target(rho, self.L, self.hmax)
        if t > self.h + 1e-12 or (t < self.h - 1e-12 and not gesture_active):
            old = self.h
            self.h = t
            self.events.append((label, old, t))

def frac(h): return round(h * 120)
def fmt(h): return f"{h:.4f} (wp_fractional_scale {frac(h)}/120, wl_surface.preferred_buffer_scale {math.ceil(h - 1e-9)})"

def header(t): print("\n" + "=" * 100 + "\n" + t + "\n" + "=" * 100)

W = (800, 600)   # W's logical window size for A-E (CSD ignored for brevity)
outs = [1.0, 1.5]
L, hmax = ladder(outs, W)
print("CP3A scale MODEL. Candidate constants: H_cfg=%.2f r=%.2f sigma=%.2f; D_max=%d" % (H_CFG, R, SIGMA, D_MAX))
print("Ladder for output scales %s, window %s: %s ; H_max=%.3f" % (outs, W, [round(x, 4) for x in L], hmax))

def show(name, pres, h_prev=None, gesture=False):
    rho = max(s * z for (s, z) in pres.values()) if pres else None
    t = target(rho, L, hmax)
    h = t if h_prev is None else (t if (t > h_prev or not gesture) else h_prev)
    print(f"  {name}")
    for k, (s, z) in pres.items():
        print(f"     presentation {k}: output scale {s} x zoom {z} = raw requirement {s*z:.3f}; compositor factor = {s*z/h:.3f} ({'downsample' if s*z/h < 1 else 'upsample' if s*z/h > 1.0001 else '1:1'})")
    print(f"     rho = {rho:.3f} -> client hint h = {fmt(h)}")
    return h

header("Case A — mixed DPI baseline: A 1.0 x zoom 1, B 1.5 x zoom 1, W on both")
hA = show("both observing", {"A": (1.0, 1.0), "B": (1.5, 1.0)})
print(f"  Phase B MaxGesture gave 1.5 (fractional 180, integer 2): reproduced = {abs(hA-1.5)<1e-9}")

header("Case B — zoom dominates DPI: A 1.0 x zoom 2.0, B 1.5 x zoom 1")
show("both observing", {"A": (1.0, 2.0), "B": (1.5, 1.0)})

header("Case C — protocol fullscreen on A (presentation zoom 1) while B (1.5) observes; NO fullscreen override")
hC = show("fullscreen on A + B observing", {"A(fullscreen view)": (1.0, 1.0), "B": (1.5, 1.0)})
fs = (1920, 1080)
print(f"  fullscreen size {fs} at h={hC}: buffer {math.ceil(fs[0]*hC)}x{math.ceil(fs[1]*hC)} = {math.ceil(fs[0]*hC)*math.ceil(fs[1]*hC)*4/2**20:.1f} MB per buffer (vs {fs[0]*fs[1]*4/2**20:.1f} MB at 1.0)")
show("B stops observing (camera B moves away; atomic change)", {"A(fullscreen view)": (1.0, 1.0)})

header("Case D — viewport-fit W on A (zoom = min(1920/800, 1080/600) = 1.8), B observes normally")
z_fit = min(1920 / 800, 1080 / 600)
e = Elem(L, hmax, 1.5)
print(f"  before fit: h = {fmt(e.h)}")
e.update(max(1.0 * z_fit, 1.5), gesture_active=False, label="fit (atomic camera change)")
show("fitted", {"A": (1.0, z_fit), "B": (1.5, 1.0)})
e.update(max(1.0 * 1.0, 1.5), gesture_active=False, label="restore (atomic camera change)")
print(f"  hint transitions: {[(l, round(a,4), round(b,4)) for (l,a,b) in e.events]}  -> after restore h = {fmt(e.h)}")

header("Case E — continuous high zoom of camera A (pinch gesture) while B (1.5) observes")
# zoom 1.0 -> 8.0 over 2.0 s at 120 Hz (exponential), reverse to 4.0 over 1.0 s, then A pans away (W leaves A), then gesture ends
e = Elem(L, hmax, 1.5)
steps = []
for i in range(1, 241):
    steps.append(("zoom-in", 1.0 * (8.0 ** (i / 240)), True))
for i in range(1, 121):
    steps.append(("zoom-out", 8.0 * ((4.0 / 8.0) ** (i / 120)), True))
updates = 0
maxmag = 0
for (ph, z, on_a) in steps:
    rho = max(1.0 * z, 1.5)
    e.update(rho, gesture_active=True, label=f"{ph} z={z:.3f}")
    updates += 1
    maxmag = max(maxmag, z / e.h)
print(f"  camera updates during the gesture: {updates}")
for (l, a, b) in e.events:
    print(f"     hint change at {l}: {a:.4f} -> {b:.4f} ({frac(b)}/120)")
print(f"  hint after zoom-in to 8.0 and reversal to 4.0 (still in gesture): {fmt(e.h)}  (no decrease during the gesture)")
print(f"  max compositor magnification on A beyond the client hint: x{maxmag:.3f} (A at zoom 8.0 / hint {e.h:.2f})")
e.update(1.5, gesture_active=True, label="camera A pans away (W leaves A) during gesture")
print(f"  W leaves A while the gesture continues: hint {e.h:.4f} (decrease deferred)")
e.update(1.5, gesture_active=False, label="gesture end (settle)")
print(f"  gesture end: hint -> {fmt(e.h)}")
print(f"  TOTAL client scale renegotiations for {updates + 2} camera/membership updates: {len(e.events)}")
print(f"  without the ladder/ceiling (hint = raw rho every update, protocol 1/120 granularity): "
      f"{sum(1 for i in range(1, len(steps)) if round(max(steps[i][1],1.5)*120) != round(max(steps[i-1][1],1.5)*120))} renegotiations, max hint {round(8.0*120)}/120 = 8.0 "
      f"(800x600 window -> {math.ceil(800*8)}x{math.ceil(600*8)} buffer = {math.ceil(800*8)*math.ceil(600*8)*4/2**20:.0f} MB each)")
