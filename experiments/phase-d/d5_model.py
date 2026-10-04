#!/usr/bin/env python3
"""Phase D checkpoint 1 — executable MODEL (not an experiment) for the D5 four-epoch audit and the
D4/D2 falsification checks. Pure arithmetic of the sealed rules (FOUNDATION_ARCHITECTURE C3.2,
CP3A-1, C14.4, INV-*). Smithay agreement of the edge function for these exact cameras is the
EXPERIMENT d5hit (evidence/phase-d/d5hit-output.txt); this script reproduces its edge table first
and aborts if it differs.

Rounding conventions reproduce Rust/Smithay: f64::round = half away from zero; Q16 = round-ties-even.
Ladder/target are reused verbatim from experiments/cp3a-scale-model/model.py (provisional constants
H_cfg 3.0, r 1.25, sigma 0.05, D_max 16384).
"""
import math, os, sys
from fractions import Fraction

HERE = os.path.dirname(os.path.abspath(__file__))
_src = open(os.path.join(HERE, "..", "cp3a-scale-model", "model.py")).read().split("def header(")[0]
_m = {}; exec(_src, _m)
ladder, target, D_MAX = _m["ladder"], _m["target"], _m["D_MAX"]

DELTA = 2.0 ** -20
LIM = 2 ** 36

def rnd(x):                       # Rust f64::round (half away from zero)
    return math.floor(x + 0.5) if x >= 0 else -math.floor(-x + 0.5)
def q16(x):                       # Q16, round-ties-even (exact on the binary value)
    return float(round(Fraction(x) * 65536)) / 65536

class View:
    """Per-output view (C3.2 [1]): v = Q16(centre - mode/(2k)); R, C, c_eff."""
    def __init__(self, name, mode, scale, zoom, centre=None, v=None):
        self.name, self.mode, self.scale, self.zoom = name, mode, scale, zoom
        self.k = zoom * scale
        if v is None:
            c = (q16(centre[0]), q16(centre[1]))
            v = (q16(c[0] - mode[0] / (2 * self.k)), q16(c[1] - mode[1] / (2 * self.k)))
        self.v = v
        self.R = (math.floor(v[0] / 120) * 120, math.floor(v[1] / 120) * 120)
        self.C = (rnd((v[0] - self.R[0]) * self.k + DELTA), rnd((v[1] - self.R[1]) * self.k + DELTA))
        self.ceff = (self.R[0] + q16(self.C[0] / self.k), self.R[1] + q16(self.C[1] / self.k))
    def ex(self, x): return rnd((x - self.R[0]) * self.k + DELTA) - self.C[0]
    def ey(self, y): return rnd((y - self.R[1]) * self.k + DELTA) - self.C[1]
    def rect(self, s):
        x, y, w, h = s["x"], s["y"], s["w"], s["h"]
        return (self.ex(x), self.ey(y), self.ex(x + w), self.ey(y + h))
    def on(self, s):
        x0, y0, x1, y1 = self.rect(s)
        return x1 > 0 and x0 < self.mode[0] and y1 > 0 and y0 < self.mode[1]
    def unproject(self, p):        # T5: OutputLogical -> Canvas
        return (self.ceff[0] + q16(p[0] / self.zoom), self.ceff[1] + q16(p[1] / self.zoom))
    def project(self, q):          # T6 (exact up to the T5 quantum): Canvas -> OutputLogical
        return ((q[0] - self.ceff[0]) * self.zoom, (q[1] - self.ceff[1]) * self.zoom)
    def desc(self):
        return f"k={self.k:.10f} v=({self.v[0]:.6f},{self.v[1]:.6f}) R={self.R} C={self.C} c_eff=({self.ceff[0]:.6f},{self.ceff[1]:.6f})"

def tree(anchor, gloc=(20, 20), size=(840, 640), popup=True):
    """W's surface tree, front-to-back (C3.3), Canvas origins via the single origin function (INV-COORD-6).
    Layout = d5hit (popup position (100,50) relative to the parent geometry, as the harness configured it)."""
    rx, ry = anchor[0] - gloc[0], anchor[1] - gloc[1]
    t = []
    if popup:
        px, py = anchor[0] + 100 - 10, anchor[1] + 50 - 10          # root + geometry.loc + pos - popup.geometry.loc
        t += [dict(n="PS", x=px + 5, y=py + 5, w=30, h=30, inp=None),
              dict(n="P", x=px, y=py, w=140, h=100, inp=None)]
    t += [dict(n="S1a", x=rx + 37 + 10, y=ry + 11 + 10, w=30, h=20, inp=None),
          dict(n="S1", x=rx + 37, y=ry + 11, w=101, h=53, inp=None),
          dict(n="W", x=rx, y=ry, w=size[0], h=size[1],
               inp=[(True, (gloc[0], gloc[1], size[0] - 2 * gloc[0], size[1] - 2 * gloc[1])), (False, (20, 55, 20, 30))] if gloc != (0, 0)
               else [(True, (0, 0, size[0], size[1])), (False, (20, 55, 20, 30))]),
          dict(n="S2", x=rx - 15, y=ry + 50, w=60, h=40, inp=None)]
    return t

def eligible(s, local):
    if s["inp"] is None:
        return True
    lx = math.floor(min(max(local[0], 0.0), s["w"] - 1e-9)); ly = math.floor(min(max(local[1], 0.0), s["h"] - 1e-9))
    r = False
    for add, (x, y, w, h) in s["inp"]:
        if x <= lx < x + w and y <= ly < y + h:
            r = add
    return r

def hit(view, surfaces, p):
    """INV-HIT-1: pointer (O, p) -> pixel -> first covering surface whose input region contains q."""
    u = (p[0] * view.scale, p[1] * view.scale)
    i, j = math.floor(u[0]), math.floor(u[1])
    q = view.unproject(p)
    for s in surfaces:
        x0, y0, x1, y1 = view.rect(s)
        local = (q[0] - s["x"], q[1] - s["y"])
        if x0 <= i < x1 and y0 <= j < y1 and eligible(s, local):
            cl = (min(max(local[0], 0.0), s["w"]), min(max(local[1], 0.0), s["h"]))
            return s["n"], (i, j), q, local, cl
    return None, (i, j), q, None, None

def canvas_exact(surfaces, q):     # superseded D-B2-I9 rule (for illustration only)
    for s in surfaces:
        local = (q[0] - s["x"], q[1] - s["y"])
        if 0 <= local[0] < s["w"] and 0 <= local[1] < s["h"] and eligible(s, local):
            return s["n"]
    return None

def hdr(t):
    print("\n" + "=" * 110 + "\n" + t + "\n" + "=" * 110)

def fmt_h(h):
    return f"{h:.4f} (preferred_scale {round(h * 120)}/120, preferred_buffer_scale {math.ceil(h - 1e-9)})"

# ------------------------------------------------------------------------------------------------------
# Baseline
# ------------------------------------------------------------------------------------------------------
ZA = 1080.0 / 651.0          # viewport-fit of a 1157x651 element on A, kept after the user navigated (C5.1)
ARR = {"A": (640.0, 360.0), "B": (2560.0, 0.0)}      # Arrangement origins (pointer crossing only)
LOG = {"A": (1920.0, 1080.0), "B": (2560.0, 1440.0)}  # logical sizes
REFRESH = {"A": 165.0, "B": 144.0}
W_ANCHOR = (1100, 420)
U = dict(n="U", x=1700, y=800, w=400, h=300, inp=None)          # a second element (no CSD)

A0 = View("A", (1920, 1080), 1.0, ZA, centre=(1650.3, 700.7))
B0 = View("B", (3840, 2160), 1.5, 1.0, centre=(2200.45, 900.15))
T = tree(W_ANCHOR)

hdr("Cross-check against d5hit (Smithay presented geometry); must be identical")
EXPECT = {"A": {"PS": (205, 149, 255, 199), "P": (197, 141, 429, 307), "S1a": (92, 76, 142, 109), "S1": (75, 60, 243, 148), "W": (14, 41, 1408, 1103), "S2": (-11, 124, 89, 191)},
          "B": {"PS": (412, 428, 457, 473), "P": (404, 420, 614, 570), "S1a": (310, 362, 355, 392), "S1": (295, 347, 446, 426), "W": (239, 330, 1499, 1290), "S2": (217, 405, 307, 465)}}
for v in (A0, B0):
    for s in T:
        r = v.rect(s)
        assert r == EXPECT[v.name][s["n"]], (v.name, s["n"], r)
print("edge tables for A and B equal d5hit's Smithay geometry: OK")

# ------------------------------------------------------------------------------------------------------
hdr("EPOCH 1 — geometry and mixed projection")
print(f"Output A: mode 1920x1080 scale 1.0 165 Hz, Arrangement origin {ARR['A']}; camera A centre Q16(1650.3,700.7) zoom 1080/651 = {ZA:.10f}")
print(f"Output B: mode 3840x2160 scale 1.5 144 Hz, Arrangement origin {ARR['B']}; camera B centre Q16(2200.45,900.15) zoom 1")
for v in (A0, B0):
    print(f"  View {v.name}: {v.desc()}  Canvas viewport x[{v.ceff[0]:.4f},{v.ceff[0] + v.mode[0] / v.k:.4f}) y[{v.ceff[1]:.4f},{v.ceff[1] + v.mode[1] / v.k:.4f})")
print("Stage: W.anchor (1100,420) [only stored datum]; U.anchor (1700,800)")
print("Surface table (Canvas origin, size) and physical rects [x0,x1)x[y0,y1) per output, membership:")
for s in T + [U]:
    ra, rb = A0.rect(s), B0.rect(s)
    print(f"  {s['n']:<4} origin ({s['x']},{s['y']}) {s['w']}x{s['h']:<4}  A x[{ra[0]},{ra[2]}) y[{ra[1]},{ra[3]}) on={A0.on(s)!s:<5}  B x[{rb[0]},{rb[2]}) y[{rb[1]},{rb[3]}) on={B0.on(s)}")
mem_A = [s["n"] for s in T if A0.on(s)]; mem_B = [s["n"] for s in T if B0.on(s)]
print(f"W observed on A by {mem_A}; on B by {mem_B}  -> Observed{{A,B}}, clock = A (165 Hz > 144 Hz), sync_output A")
rho = {"A": A0.k, "B": B0.k}
L, HMAX = ladder([1.0, 1.5], (840, 640))
t1 = target(max(rho.values()), L, HMAX)
print(f"raw requirements rho_A={rho['A']:.4f} rho_B={rho['B']:.4f} -> rho={max(rho.values()):.4f}")
print(f"ladder (outputs 1.0,1.5; largest surface 840x640): {[round(x, 4) for x in L]} H_max={HMAX}")
print(f"target t = smallest rung >= rho/(1+sigma) = {max(rho.values()) / 1.05:.4f} -> h = {fmt_h(t1)}")
print(f"compositor factors m = rho_p / h: A {rho['A'] / t1:.4f} (minify), B {rho['B'] / t1:.4f} (minify)")
print(f"buffer: root 840x640 at h -> {math.ceil(840 * t1)}x{math.ceil(640 * t1)} px")

print("\nHit-tests (INV-HIT-1), pointer positions in OutputLogical:")
cases = [("B", (300.25, 400.6), "W root content"),
         ("B", None, "input-region hole over S2 (Canvas (1105.4,470.3))"),
         ("A", None, "popup P at sub-pixel position (Canvas (1250.3,500.9))"),
         ("A", (8.7, 300.2), "near W's left edge on A")]
for out, p, what in cases:
    v = A0 if out == "A" else B0
    if p is None:
        tgt = (1105.4, 470.3) if "hole" in what else (1250.3, 500.9)
        p = v.project(tgt)
    n, px, q, local, cl = hit(v, T, p)
    arr = (ARR[out][0] + p[0], ARR[out][1] + p[1])
    print(f"  [{out}] {what}: p=({p[0]:.4f},{p[1]:.4f}) Arrangement=({arr[0]:.4f},{arr[1]:.4f}) pixel={px} Canvas q=({q[0]:.6f},{q[1]:.6f}) -> {n}"
          + (f" surface-local delivered ({cl[0]:.6f},{cl[1]:.6f}) clamp={max(abs(cl[0] - local[0]), abs(cl[1] - local[1])):.2e}" if n else ""))
# where would the superseded canvas-exact rule disagree on A? scan W's left/right edge columns at pixel centres
dis = 0; ex = None
for v in (A0,):
    for j in range(0, 1080):
        for i in list(range(0, 30)) + list(range(1395, 1420)):
            q = v.unproject(((i + 0.5) / v.scale, (j + 0.5) / v.scale))
            a = hit(v, T, ((i + 0.5) / v.scale, (j + 0.5) / v.scale))[0]
            b = canvas_exact(T, q)
            if a != b:
                dis += 1
                ex = ex or (i, j, a, b)
print(f"  superseded canvas-exact rule (D-B2-I9) vs INV-HIT-1 along W's vertical edges on A: {dis} disagreeing pixels (first {ex})")

# ------------------------------------------------------------------------------------------------------
hdr("EPOCH 2 — input and constraint overlap (lock via B; camera A moved by a shell Navigate stream)")
pB = (300.25, 400.6)
qlock = B0.unproject(pB)
root = [s for s in T if s["n"] == "W"][0]
slock = (qlock[0] - root["x"], qlock[1] - root["y"])
print(f"pointer Free(B, {pB}) over W root -> keyboard focus W (click-to-focus earlier); lock request (persistent, region = whole surface)")
print(f"activation policy: W focused AND pointer inside region -> activate -> client sees `locked`")
print(f"authoritative pointer (sealed INV-COORD-5): Locked(W, q_lock=Canvas ({qlock[0]:.6f},{qlock[1]:.6f}))  [no OutputId]")
print(f"   same point surface-local (root): ({slock[0]:.6f},{slock[1]:.6f})")
for acc, un in [((3.5, -1.25), (3.0, -1.0)), ((2.25, 0.5), (2.0, 0.5)), ((-1.7, 0.75), (-1.5, 0.6))]:
    fx = lambda v: round(v * 256) / 256
    print(f"   physical delta accel {acc} unaccel {un} -> relative_motion(dx={fx(acc[0])}, dy={fx(acc[1])}, dx_unaccel={fx(un[0])}, dy_unaccel={fx(un[1])}) [wl_fixed]; no wl_pointer.motion (locked)")
print("shell stream: Navigate{output A, Delta(+15,0)} x 60 at 60 Hz (each command atomic under C7.2/C7.3)")
L, HMAX = ladder([1.0, 1.5], (840, 640))
h = t1; changes = []; prev_on = {s["n"]: A0.on(s) for s in T}; Rsw = 0; widths = set(); prevR = A0.R
cen = (q16(1650.3), q16(700.7)); v = A0
left_at = None
for n in range(1, 61):
    cen = (cen[0] + 15, cen[1])
    v = View("A", (1920, 1080), 1.0, ZA, centre=cen)
    if v.R != prevR:
        Rsw += 1; prevR = v.R
    on = {s["n"]: v.on(s) for s in T}
    for s in T:
        if on[s["n"]] != prev_on[s["n"]]:
            print(f"   cmd {n:>2}: v_A.x={v.v[0]:.4f} R_A={v.R[0]} surface {s['n']:<3} {'enter A' if on[s['n']] else 'leave A'}")
    prev_on = on
    if on["W"] or any(on.values()):
        widths.add(v.ex(root["x"] + root["w"]) - v.ex(root["x"]))
    obs = any(on.values())
    rr = max(([v.k] if obs else []) + [B0.k])
    t = target(rr, L, HMAX)
    if t != h:                                   # atomic: increase or decrease applies at once (INV-SCALE-3)
        changes.append((n, round(h, 4), round(t, 4))); h = t
    if not obs and left_at is None:
        left_at = n
print(f"   W fully leaves A at command {left_at}; lattice-reference switches of view A during the stream: {Rsw}; distinct W root widths on A: {sorted(widths)}")
print(f"   hint changes under the sealed rules (stream = atomic changes): {changes}; configures: 0; pointer/relative events from the stream: 0")
A_after_e2 = v
print(f"   end of epoch 2: camera A centre ({cen[0]:.4f},{cen[1]:.4f}); W Observed{{B}}; clock B 144 Hz; h = {fmt_h(h)}")
# oscillation variant (shell smooth-scroll with overshoot or a wheel-bound pan) across the leaving point
def osc_run(gesture_semantics):
    hh = t1; ch = 0; base = (q16(1650.3) + 15 * (left_at - 1), q16(700.7))
    for i in range(40):
        c = (base[0] + (15 if i % 2 == 0 else 0), base[1])
        vv = View("A", (1920, 1080), 1.0, ZA, centre=c)
        obs = any(vv.on(s) for s in T)
        tt = target(max(([vv.k] if obs else []) + [B0.k]), L, HMAX)
        if tt > hh or (tt < hh and not gesture_semantics):
            hh = tt; ch += 1
    if gesture_semantics:                         # one settlement at the end
        if target(B0.k, L, HMAX) != hh:
            ch += 1
    return ch
print(f"   oscillating stream (40 steps across W's edge on A): hint changes sealed={osc_run(False)}; if the stream had gesture semantics={osc_run(True)}")

# ------------------------------------------------------------------------------------------------------
hdr("EPOCH 3 — protocol fullscreen on A while W is observed through B")
vA = A_after_e2
on_before = [s["n"] for s in T + [U] if vA.on(s)]
print(f"before: camera A view {vA.desc()}; on A: {on_before}; W on B only; h = {fmt_h(h)}")
print("W (focused) requests set_fullscreen(wl_output A) -> accepted (INV-PRES-1); saved {800x600, [activated]}; configure [fullscreen,activated] 1920x1080")
P1 = View("A-presentation", (1920, 1080), 1.0, 1.0, v=(q16(W_ANCHOR[0] - (1920 - 800) / 2), q16(W_ANCHOR[1] - (1080 - 600) / 2)))
print(f"presentation view (before the client resizes; centred, border fill): v=({P1.v[0]},{P1.v[1]}) {P1.desc()}")
for s in T:
    r = P1.rect(s); print(f"   {s['n']:<4} A x[{r[0]},{r[2]}) y[{r[1]},{r[3]}) on={P1.on(s)}")
print(f"   U on A: before={U['n'] in on_before}, after=False (only W's tree is presented)")
T3 = tree(W_ANCHOR, gloc=(0, 0), size=(1920, 1080))
P2 = View("A-presentation", (1920, 1080), 1.0, 1.0, v=(float(W_ANCHOR[0]), float(W_ANCHOR[1])))
print(f"client acks + commits 1920x1080, CSD shadow dropped (geometry.loc (0,0)); anchor stays (1100,420); root origin (1080,400) -> (1100,420)")
print(f"presentation view after commit: {P2.desc()}")
for s in T3:
    r = P2.rect(s); rb = B0.rect(s)
    print(f"   {s['n']:<4} origin ({s['x']},{s['y']}) {s['w']}x{s['h']}  A x[{r[0]},{r[2]}) y[{r[1]},{r[3]}) on={P2.on(s)!s:<5}  B x[{rb[0]},{rb[2]}) y[{rb[1]},{rb[3]}) on={B0.on(s)}")
L3, H3 = ladder([1.0, 1.5], (1920, 1080))
rho3 = max(1.0 * 1.0, B0.k)
t3 = target(rho3, L3, H3)
print(f"rho_A(presentation, zoom 1)=1.0 rho_B=1.5 -> rho=1.5; H_max={H3}; t={t3} -> h {fmt_h(h)} -> {fmt_h(t3)} ({'no change' if t3 == h else 'change'})")
print(f"   buffer {math.ceil(1920 * t3)}x{math.ceil(1080 * t3)} = {1920 * t3 * 1080 * t3 * 4 / 2**20:.1f} MB; factors A {1.0 / t3:.3f} B {1.5 / t3:.3f}; no fullscreen override (CP2-Q1)")
print(f"clock: observing {{A 165, B 144}} -> A; presentation feedback sync_output A; camera A untouched: centre still ({cen[0]:.4f},{cen[1]:.4f})")
root3 = [s for s in T3 if s["n"] == "W"][0]
print(f"lock: sealed Canvas q_lock ({qlock[0]:.6f},{qlock[1]:.6f}) is now root-local ({qlock[0] - root3['x']:.6f},{qlock[1] - root3['y']:.6f}) "
      f"instead of ({slock[0]:.6f},{slock[1]:.6f}): the lock point moved {slock[0] - (qlock[0] - root3['x']):.0f},{slock[1] - (qlock[1] - root3['y']):.0f} px relative to the surface without any event")
print(f"   surface-relative representation keeps ({slock[0]:.6f},{slock[1]:.6f}) -> Canvas ({root3['x'] + slock[0]:.6f},{root3['y'] + slock[1]:.6f}); observable on B: {0 <= B0.project((root3['x'] + slock[0], 0))[0] < 2560}")
print("exit: client unset_fullscreen -> configure [activated] 800x600 (saved size) -> commit with CSD; presentation of A returns to camera A's view (unchanged)")
print(f"   W on A after exit: {any(vA.on(s) for s in T)} -> leave A for all W surfaces; U on A again: {vA.on(U)}; rho=1.5 -> h unchanged {fmt_h(t3)}; clock -> B 144 Hz")
h = t3

# ------------------------------------------------------------------------------------------------------
hdr("EPOCH 4 — teardown/hotplug: remove B during lock + held button + held key + open popup; then pan A until W is off-screen")
A4 = View("A", (1920, 1080), 1.0, ZA, centre=(1650.3, 700.7))
print(f"shell Navigate{{A, To(centre (1650.3,700.7))}} (atomic) -> W enters A; rho = max({A4.k:.4f},1.5) -> h {fmt_h(h)} -> {fmt_h(target(A4.k, L, HMAX))}")
h = target(A4.k, L, HMAX)
print("state at removal: Locked(W) via B; BTN_LEFT held (client-owned, Smithay implicit grab on W); key KEY_W held (client-owned); P open; keyboard focus W")
print("remove B:")
print(f"   1. visibility pass without B: wl_surface.leave(B) for {[s['n'] for s in T if B0.on(s)]} of W and for U, BEFORE the wl_output B global is destroyed")
L4, H4 = ladder([1.0], (840, 640))
t4 = target(A4.k, L4, H4)
print(f"   2. ladder without B: {[round(x, 4) for x in L4]}; current h={h:.4f} in new ladder: {any(abs(h - x) < 1e-9 for x in L4)}")
print(f"      rho = {A4.k:.4f} unchanged, target on the new ladder {t4:.4f} -> h {fmt_h(h)} -> {fmt_h(t4)} (decrease; no gesture active; output removal = SC-E stimulus)")
h = t4
pB_lock = B0.project(qlock)
arr = (ARR["B"][0] + pB_lock[0], ARR["B"][1] + pB_lock[1])
near = (min(max(arr[0], ARR["A"][0]), ARR["A"][0] + LOG["A"][0] - 2 ** -8), min(max(arr[1], ARR["A"][1]), ARR["A"][1] + LOG["A"][1] - 2 ** -8))
pA = (near[0] - ARR["A"][0], near[1] - ARR["A"][1])
nA, pxA, qA, _, _ = hit(A4, T + [U], pA)
print(f"   3. pointer: lock point on B p=({pB_lock[0]:.6f},{pB_lock[1]:.6f}) -> Arrangement ({arr[0]:.6f},{arr[1]:.6f}); Arrangement-nearest on A ({near[0]:.6f},{near[1]:.6f})"
      f" -> Free(A, ({pA[0]:.6f},{pA[1]:.6f})) Canvas ({qA[0]:.4f},{qA[1]:.4f}); core hit-test -> {nA}")
print(f"      Smithay's implicit click grab keeps pointer focus on W while BTN_LEFT is held -> delivered focus does NOT change; core's hit says {nA}")
print(f"      motion delivered through the grab: surface-local ({qA[0] - root['x']:.4f},{qA[1] - root['y']:.4f}) (outside W: legal under an implicit grab)")
print("   4. keys: KEY_W stays client-owned and logically down at W (K8: output removal has no keyboard effect)")
print("   5. camera B: unbound, kept, persisted under OutputKey(B); View(B), ProjectionFrame(B), damage tracker(B) dropped")
print(f"   6. popup P: anchor output was B; not repositioned (D-B6-NO-PAN-REPOSITION); still on A: {A4.on(T[1])}")
print(f"   7. clock: W observed on A only -> A (165 Hz); no handover needed (A already the clock)")
print("then: user releases BTN_LEFT (delivered to W, grab ends) and KEY_W (delivered); presses the pan binding on A (no client-owned button: acquisition OK)")
hand = (-15.3, 0.2); N = 120
def pan(div_zoom):
    start = (q16(1650.3), q16(700.7)); sx = sy = 0.0; leave_ev = None; Rs = 0; prevR = None; widths = set(); dists = set()
    for n in range(1, N + 1):
        sx += hand[0]; sy += hand[1]                                    # Σ in f64 from the gesture origin (INV-COORD-3)
        d = (sx / ZA, sy / ZA) if div_zoom else (sx, sy)
        c = (start[0] - q16(d[0]), start[1] - q16(d[1]))               # camera = start - Q(Σ)
        vv = View("A", (1920, 1080), 1.0, ZA, centre=c)
        if prevR is not None and vv.R != prevR:
            Rs += 1
        prevR = vv.R
        obs = any(vv.on(s) for s in T)
        if obs:
            widths.add(vv.ex(root["x"] + root["w"]) - vv.ex(root["x"]))
            dists.add(vv.ex(U["x"]) - vv.ex(root["x"] + root["w"]))
        if not obs and leave_ev is None:
            leave_ev = (n, vv.v[0])
    return leave_ev, Rs, sorted(widths), sorted(dists), c, vv
for dz in (False, True):
    le, Rs, wd, ds, c, vv = pan(dz)
    lab = "sealed formula camera = start - Q(Sigma)" if not dz else "zoom-aware camera = start - Q(Sigma/zoom)"
    print(f"   {lab}: W fully unobserved on A at event {le[0] if le else None} (v_A.x={le[1] if le else float('nan'):.4f}); lattice switches {Rs}; W root widths {wd}; W->U gap widths {ds}")
    if dz:
        A_end = vv
print(f"   during the pan: per-surface wl_surface.leave(A) as each surface leaves; no configure; no pointer/relative/button to clients; h held (no decrease in a gesture)")
print(f"   W Unobserved: callbacks withheld; FrameLiveness.last_completion = last A vblank; fallback due last_completion + P (1 s), timer armed because a callback is pending")
print(f"   kick: KEY presses delivered to W (still keyboard focus) complete its withheld callbacks once, before delivery")
pe = A_end.unproject(pA)
print(f"   pan end: Free(A, p) unchanged -> re-hit at Canvas ({pe[0]:.4f},{pe[1]:.4f}) -> {hit(A_end, T + [U], pA)[0]}; deferred decrease: W unobserved -> hold h = {fmt_h(h)}")

# ------------------------------------------------------------------------------------------------------
hdr("FALSIFICATION CHECKS (D4 / D2)")
# F1 lattice reference switch at zoom != 1 (A-B1-1) vs zoom 1, and a pinned-reference alternative
def sweep(k_zoom, scale, pinned):
    widths, gaps = set(), set()
    v0 = 1000.0; R0 = math.floor(v0 / 120) * 120
    for i in range(0, 3300):
        v = q16(v0 + i * 0.37)
        vw = View("X", (1920, 1080), scale, k_zoom, v=(v, 0.0))
        if pinned:
            vw.R = (R0, 0); vw.C = (rnd((v - R0) * vw.k + DELTA), 0)
        widths.add(vw.ex(1900 + 840) - vw.ex(1900)); gaps.add(vw.ex(2900) - vw.ex(1900 + 840))
    return sorted(widths), sorted(gaps)
for (z, s, pin, lab) in [(1.0, 1.5, False, "zoom 1, scale 1.5 (k = 180/120)"), (ZA, 1.0, False, "zoom 1080/651 (sealed R = floor(v/120)*120)"),
                         (ZA, 1.0, True, "zoom 1080/651, reference pinned for the gesture (alternative)")]:
    wd, gp = sweep(z, s, pin)
    print(f"F1 pan sweep 1221 Canvas px in 0.37-px steps, {lab}: element widths {wd}, neighbour gap {gp}")
# F2 SC-D defeated by interleaved pointer motion (zoom-at-cursor), and discrete pan not covered
Lf, Hf = ladder([1.0, 1.5], (800, 600))
def wiggle(motion_between, policy):
    hh = 1.5; ch = 0; active = False
    for i in range(40):
        z = 1.59 if i % 2 == 0 else 1.57
        active = True                                            # a zoom tick starts/continues SC-D
        t = target(max(z, 1.5), Lf, Hf)
        if t > hh or (t < hh and (policy == "NAIVE" or not active)):
            hh = t; ch += 1
        if motion_between and policy == "SC-D":
            active = False                                       # sealed rule: any pointer motion ends SC-D
            t = target(max(z, 1.5), Lf, Hf)
            if t < hh:
                hh = t; ch += 1
    return ch
print(f"F2 wheel-zoom wiggle 40 ticks across a rung boundary: NAIVE {wiggle(False, 'NAIVE')}; SC-D without motion {wiggle(False, 'SC-D')}; "
      f"SC-D with one pointer-motion event between ticks (zoom-at-cursor) {wiggle(True, 'SC-D')}")
print(f"   discrete PAN steps across an output edge (epoch-2 oscillation): sealed {osc_run(False)} changes / 40 steps (SC-D covers zoom only)")
# F3 device bound vs Phase B equivalence and 'ladder contains every output scale'
Ld, Hd = ladder([1.0, 3.0], (6000, 1000))
print(f"F3 outputs (1.0, 3.0), window 6000x1000 at zoom 1, observed on the 3.0 output: Phase B MaxGesture -> 3.0 -> buffer width {6000 * 3} > D_max {D_MAX};"
      f" unified: H_max={Hd:.4f}, ladder {[round(x, 4) for x in Ld]} (3.0 excluded), h={target(3.0, Ld, Hd):.4f}")
gw, sh = 5450, 20
print(f"   window geometry {gw} wide + {sh}px shadows -> root {gw + 2 * sh}: H_max from geometry = {min(3.0, D_MAX / gw):.4f} -> root buffer {math.ceil((gw + 2 * sh) * min(3.0, D_MAX / gw))} px"
      f" > {D_MAX}; from the largest surface = {D_MAX / (gw + 2 * sh):.4f}")
# F4 ladder change on hotplug
print(f"F4 ladder with B: {[round(x, 4) for x in L]}; without B: {[round(x, 4) for x in L4]}; a held/deferred h=1.5 or 1.7321 is not a rung of the new ladder")
# F5 range
w = 2 ** 31 - 1
z = min(1920 / w, 1080 / 1)
print(f"F5 viewport-fit of a {w}x1 window: zoom={z:.3e}; mode/(2k)={1920 / (2 * z):.4e} -> |v_O| up to 2^36 + {1920 / (2 * z):.3e} (> CANVAS_LIMIT); "
      f"far element edge at |x-R|=2^37, k=1.5: {2 ** 37 * 1.5:.3e} > i32 max {2 ** 31 - 1}")
# F6 Q16 pixel-centre clamp bound
import random
random.seed(7); worst = 0.0
for _ in range(200000):
    k = random.uniform(0.5, 3.0); v = q16(random.uniform(-1e6, 1e6)); x0 = random.randint(-1000000, 1000000)
    vw = View("X", (1920, 1080), 1.0, k, v=(v, 0.0))
    i = vw.ex(x0)
    q = vw.ceff[0] + q16((i + 0.5) / k)
    if q - x0 < 0:
        worst = max(worst, x0 - q)
print(f"F6 pixel-centre delivered-coordinate clamp, 200k random (k, camera, edge): worst {worst:.3e} logical px (CP3A-2 states 'provably 0'; the Q16 terms allow ~2^-16)")
# F7 popup constraint target needs a rounding point
tgt0 = B0.unproject((0, 0)); tgt1 = B0.unproject((2560, 1440))
rel = (tgt0[0] - W_ANCHOR[0], tgt0[1] - W_ANCHOR[1], tgt1[0] - W_ANCHOR[0], tgt1[1] - W_ANCHOR[1])
print(f"F7 popup constraint target = B's viewport in parent-geometry coordinates: x[{rel[0]:.6f},{rel[2]:.6f}) y[{rel[1]:.6f},{rel[3]:.6f}) -> Rectangle<i32,Logical> needs a rounding point not in R1-R7")
# F8 initial hint: harness rule (max scale of outputs containing the anchor point) vs unified policy at map
print(f"F8 initial hint: harness rule gives 1.5 (anchor point on A and B, zoom ignored); unified policy at map gives {target(max(A0.k, B0.k), L, HMAX):.4f} -> one extra renegotiation + reallocation at map")
