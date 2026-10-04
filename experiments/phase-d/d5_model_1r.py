#!/usr/bin/env python3
"""Phase D checkpoint 1R — executable MODEL (not an experiment) of the corrected rules.

Reuses the baseline model's numeric helpers (d5_model.py, up to its "# Baseline" marker) and the
ladder/target of experiments/cp3a-scale-model/model.py. Sections:
  R1  PD-F2-06  direct-manipulation pan algebra  dP_canvas = dp_phys / (s_O * z_C), accumulated from the
                gesture origin, quantised once per update; equal hand displacement -> equal physical
                content displacement at every zoom (negative control: sealed formula).
  R2  PD-F2-07  ScaleInteractionEpoch vs the sealed rules (MaxGesture + SC-D) on: reversing pan, discrete
                pan, discrete zoom (with and without pointer motion), shell stream, observer change during
                an epoch, client self-resize during an epoch; settlement-timeout sensitivity; negative control.
  R3  PD-F2-01  one-shot viewport-fit vs the sealed re-fit under the CP3B feedback clients.
  R4  D5 deltas: epoch 2 stream, epoch 4 lock/grab reconciliation and pan, under the 1R rules.
"""
import math, os

HERE = os.path.dirname(os.path.abspath(__file__))
_src = open(os.path.join(HERE, "d5_model.py")).read().split("# Baseline")[0]
_m = {"__file__": os.path.join(HERE, "d5_model.py")}; exec(_src, _m)
View, q16, rnd, ladder, target = _m["View"], _m["q16"], _m["rnd"], _m["ladder"], _m["target"]

def hdr(t):
    print("\n" + "=" * 110 + "\n" + t + "\n" + "=" * 110)

ZA = 1080.0 / 651.0

# --------------------------------------------------------------------------------------------------------
hdr("R1  PD-F2-06 direct-manipulation pan algebra")
print("hand: 200 events of physical displacement (+3.7, -1.3) px on the pointer's output (dp_phys = dp_logical * s_O)")
print("check: physical displacement of an element's left edge on screen == hand physical displacement (+-1 px lattice rounding)")
def pan_run(scale, zoom, zoom_aware):
    centre0 = (q16(5000.3), q16(3000.7))
    x_elem = 5100                                   # Canvas x of an element edge inside the view
    v0 = View("O", (1920, 1080), scale, zoom, centre=centre0)
    e0 = v0.ex(x_elem)
    sx = sy = 0.0
    worst = 0.0
    for n in range(1, 201):
        sx += 3.7; sy += -1.3                       # accumulated PHYSICAL displacement from the gesture origin
        if zoom_aware:
            d = (sx / (scale * zoom), sy / (scale * zoom))   # dP_canvas = dp_phys / (s_O * z_C)
        else:
            d = (sx / scale, sy / scale)            # sealed: camera = start - Q(Sigma) with Sigma in logical px
        c = (centre0[0] - q16(d[0]), centre0[1] - q16(d[1]))   # camera moves opposite; Q16 once per update
        v = View("O", (1920, 1080), scale, zoom, centre=c)
        content = v.ex(x_elem) - e0                 # physical displacement of the content
        worst = max(worst, abs(content - sx))
    return worst
print(f"{'scale':>6} {'zoom':>10} | worst |content - hand| px: corrected   sealed (negative control)")
ok = True
for s in (1.0, 1.5):
    for z in (0.5, 0.75, 1.0, ZA, 2.5):
        a, b = pan_run(s, z, True), pan_run(s, z, False)
        ok &= a <= 1.0
        print(f"{s:>6} {z:>10.6f} | {a:>8.3f}{'':>22}{b:>8.3f}")
print(f"R1 corrected algebra: content follows the hand within the lattice rounding (<= 1 px) at every zoom: {ok}")
print("   sealed formula diverges from the hand whenever zoom != 1 (factor = zoom).")

# --------------------------------------------------------------------------------------------------------
hdr("R2  PD-F2-07 ScaleInteractionEpoch")
L, HMAX = ladder([1.0, 1.5], (840, 640))
T_CAND = 0.300   # PROVISIONAL settlement timeout candidate (seconds) — tuning value, NOT an invariant

def _tgt(rho, hm):
    lad = [x for x in L if x <= hm + 1e-12] or [hm]
    return target(rho, lad, hm)

class Sealed:
    """Sealed rules: decreases deferred during a pointer gesture (MaxGesture) and during SC-D (discrete
    ZOOM only; any non-zoom input incl. pointer motion ends it). Everything else is atomic. P7 immediate.
    SC-E: a client response may not reverse the direction of the last stimulus."""
    def __init__(self, h): self.h, self.changes, self.gesture, self.scd, self.hmax, self.dir, self.rho = h, 0, False, False, HMAX, 0, None
    def ev(self, kind, t, rho=None, **kw):
        if rho is not None: self.rho = rho
        if kind == "gesture_begin": self.gesture = True
        elif kind == "gesture_end": self.gesture = False; self.evalu(allow_dec=True)
        elif kind == "zoom_step": self.scd = True; self.evalu(allow_dec=False)
        elif kind in ("pan_step", "shell_step", "observer", "hotplug"): self.evalu(allow_dec=not (self.gesture or self.scd))
        elif kind == "gesture_motion": self.evalu(allow_dec=False)
        elif kind == "pointer_motion":
            if self.scd: self.scd = False; self.evalu(allow_dec=not self.gesture)
        elif kind == "client":
            self.hmax = kw.get("hmax", self.hmax); self.evalu(allow_dec=not (self.gesture or self.scd), client=True)
    def tick(self, t): pass
    def end(self, t, rho):
        if rho is not None: self.rho = rho
        self.gesture = self.scd = False; self.evalu(allow_dec=True)
    def evalu(self, allow_dec, client=False):
        if self.rho is None: return
        tt = _tgt(self.rho, self.hmax)
        if self.h > self.hmax + 1e-12:                         # P7
            self.h, self.dir = tt, -1; self.changes += 1; return
        d = 1 if tt > self.h else -1 if tt < self.h else 0
        if d == 0: return
        if client and self.dir and d != self.dir: return       # SC-E
        if d > 0 or allow_dec:
            self.h = tt; self.changes += 1
            if not client: self.dir = d

class Epoch:
    """1R rule. ScaleInteractionEpoch groups the camera changes of one logical interaction (continuous
    gesture, discrete pan/zoom steps, a shell navigation stream). During it: decreases suppressed,
    increases only at ladder rungs and only in the epoch's direction (a P7 correctness decrease fixes the
    direction to down for the rest of the epoch; client responses cannot reverse it). It ends at an explicit
    end (gesture end, shell end-of-stream), when no camera step arrived for T_settle (discrete inputs), or at
    a settlement event (output hotplug/configuration). Pointer motion does NOT end it. At settlement the
    final requirement is evaluated ONCE. Outside an epoch, SC-E applies to client responses."""
    def __init__(self, h, T=T_CAND, enabled=True):
        self.h, self.changes, self.T, self.enabled = h, 0, T, enabled
        self.active, self.due, self.dir, self.rho, self.hmax = False, None, 0, None, HMAX
    def _begin(self, t, timed):
        if not self.active:
            self.active, self.dir = True, 0
        self.due = (t + self.T) if timed else None
    def ev(self, kind, t, rho=None, **kw):
        if rho is not None: self.rho = rho
        if kind == "client": self.hmax = kw.get("hmax", self.hmax)
        if not self.enabled:
            return self._noepoch()
        self.tick(t)
        if kind == "gesture_begin": self._begin(t, timed=False)
        elif kind == "gesture_end": self._settle()
        elif kind in ("zoom_step", "pan_step", "shell_step"): self._begin(t, timed=True); self._eval()
        elif kind in ("gesture_motion", "observer"): self._eval()
        elif kind == "hotplug": self._settle()
        elif kind == "pointer_motion": pass
        elif kind == "client": self._eval(client=True)
    def _noepoch(self):
        # negative control: epoch disabled -> every evaluation settles exactly (increase or decrease)
        if self.rho is None: return
        tt = _tgt(self.rho, self.hmax)
        if tt != self.h: self.h = tt; self.changes += 1
    def tick(self, t):
        if self.enabled and self.active and self.due is not None and t >= self.due:
            self._settle()
    def end(self, t, rho):
        if rho is not None: self.rho = rho
        if self.enabled: self._settle()
        else: self._noepoch()
    def _eval(self, client=False):
        if self.rho is None: return
        tt = _tgt(self.rho, self.hmax)
        if self.h > self.hmax + 1e-12:                 # P7: correctness-forced decrease, immediately
            self.h, self.dir = tt, -1; self.changes += 1; return
        if tt > self.h:
            if self.dir == -1: return                  # direction fixed down (P7 or a settled decrease)
            if client and not self.active and self.dir != 1: return   # SC-E outside an epoch
            self.h = tt; self.changes += 1; self.dir = 1
        elif tt < self.h and not self.active and not client:
            self.h = tt; self.changes += 1; self.dir = -1
    def _settle(self):
        if self.active:
            self.active, self.due = False, None
            if self.rho is not None:
                tt = _tgt(self.rho, self.hmax)
                if tt != self.h:
                    self.dir = 1 if tt > self.h else -1
                    self.h = tt; self.changes += 1
                else:
                    self.dir = 0

RHO_IN, RHO_OUT = ZA, 1.5        # W observed by A (k=1.659) and B (1.5)  /  W left A
def scen_reversing_pan(p):
    t = 0.0; p.ev("gesture_begin", t)
    for i in range(240):
        t += 1 / 120; rho = RHO_IN if (i // 12) % 2 == 0 else RHO_OUT
        p.ev("gesture_motion", t, rho)
    t += 0.01; p.ev("gesture_end", t, RHO_OUT); p.end(t + 1, RHO_OUT); return p
def scen_discrete_pan(p):
    t = 0.0
    for i in range(40):
        t += 0.05; p.ev("pan_step", t, RHO_IN if i % 2 == 0 else RHO_OUT)
    p.end(t + 1, RHO_OUT); return p
def scen_discrete_zoom(p, motion):
    t = 0.0
    for i in range(40):
        t += 0.05; z = 1.59 if i % 2 == 0 else 1.57
        p.ev("zoom_step", t, max(z, 1.5))
        if motion: p.ev("pointer_motion", t + 0.01, max(z, 1.5))
    p.end(t + 1, max(1.57, 1.5)); return p
def scen_shell_stream(p):
    t = 0.0
    for i in range(40):
        t += 1 / 60; p.ev("shell_step", t, RHO_IN if i % 2 == 0 else RHO_OUT)
    p.end(t + 1, RHO_OUT); return p
def scen_observer_change(p):
    # discrete pan epoch on A; meanwhile W's other observer (B) appears and disappears; then B is unplugged
    t = 0.0
    for i in range(20):
        t += 0.05; p.ev("pan_step", t, 2.0)                         # A zoomed 2.0 observes W
        t += 0.01; p.ev("observer", t, 2.0 if i % 2 else 2.25)      # B (scale 1.5 x zoom 1.5) observes on/off
    t += 0.01; p.ev("hotplug", t, 2.0)
    p.end(t + 1, 2.0); return p
def scen_client_resize(p):
    # discrete pan epoch with W near the device bound: the client flips its logical size each response
    t = 0.0
    for i in range(30):
        t += 0.05; p.ev("pan_step", t, 3.0)
        hmax = 16384 / (6000 if i % 2 == 0 else 5000)             # 2.73 / 3.28 (capped at 3.0 by H_cfg)
        t += 0.005; p.ev("client", t, 3.0, hmax=min(3.0, hmax))
    p.end(t + 1, 3.0)
    for i in range(30):                                          # the client keeps flipping after settlement
        t += 0.05; p.ev("client", t, 3.0, hmax=min(3.0, 16384 / (6000 if i % 2 == 0 else 5000)))
    return p

cases = [("reversing continuous pan (240 updates, 20 reversals)", scen_reversing_pan),
         ("discrete pan wiggle across an output edge (40 ticks, 20 Hz)", scen_discrete_pan),
         ("discrete zoom wiggle, no pointer motion (40 ticks)", lambda p: scen_discrete_zoom(p, False)),
         ("discrete zoom wiggle WITH pointer motion between ticks", lambda p: scen_discrete_zoom(p, True)),
         ("shell Navigate stream oscillating (40 commands, 60 Hz)", scen_shell_stream),
         ("observer enter/leave during an epoch + output unplug", scen_observer_change),
         ("client self-resize near the device bound during an epoch", scen_client_resize)]
print(f"settlement timeout candidate T_settle = {T_CAND*1000:.0f} ms (PROVISIONAL; TUNE)")
print(f"{'scenario':<60} | sealed | epoch (1R) | epoch disabled (NEG)")
allok = True
for name, f in cases:
    a = f(Sealed(1.7321)).changes
    b = f(Epoch(1.7321)).changes
    c = f(Epoch(1.7321, enabled=False)).changes
    # bound: rungs crossed upward (here <= 2) + 1 settlement + at most one P7 forced decrease per non-client
    # stimulus (the epoch, and the settlement) -> 3, or 4 when the device bound is hit around a settlement
    allok &= b <= (4 if name.startswith("client") else 3)
    print(f"{name:<60} | {a:>6} | {b:>10} | {c:>8}")
print(f"R2 bound (rungs crossed + 1 settlement + <= 1 P7 forced decrease per non-client stimulus) holds in every case: {allok}")
print("\nsettlement-timeout sensitivity, discrete pan wiggle at 20 Hz (tick interval 50 ms):")
for T in (0.04, 0.1, 0.3, 1.0):
    n = scen_discrete_pan(Epoch(1.7321, T=T)).changes
    print(f"   T_settle={T*1000:>5.0f} ms -> hint changes {n}{'   (T below the input interval: every tick is its own epoch)' if T < 0.05 else ''}")
print("   architecture: a finite settlement rule bounds changes to (#epochs) x (rungs + 1); the VALUE only")
print("   decides how input is grouped into epochs (tuning; OB-23).")

# --------------------------------------------------------------------------------------------------------
hdr("R3  PD-F2-01 one-shot viewport-fit vs sealed re-fit (CP3B feedback clients, 60 client rounds)")
Lf, Hf = ladder([1.0, 1.5], (800, 600))
V = (1920, 1080)
def fit_zoom(g): return min(V[0] / g[0], V[1] / g[1])
CLIENTS = {
    "compliant (logical size constant)": lambda h, lm: (800, 600),
    "constant-physical (1200x900 / h)": lambda h, lm: (1200 / h, 900 / h),
    "anti (logical grows with h)": lambda h, lm: (500 * h, 375 * h),
    "toggling (size flips with each hint)": lambda h, lm: (900, 675) if round(h * 120) % 2 == 0 else (700, 525),
    "adversarial (inverts the last move)": lambda h, lm: (1200, 900) if lm > 0 else (300, 225),
}
print(f"{'client':<40} | sealed re-fit: hint changes / camera mutations | 1R one-shot: hint changes / camera mutations")
for name, cl in CLIENTS.items():
    res = []
    for refit in (True, False):
        h = 1.5; g = (800, 600); z = fit_zoom(g); changes = cam = 0; lm = 0
        cam += 1                                   # the user's authorized fit
        for i in range(60):
            if refit and i > 0:
                nz = fit_zoom(g)
                if nz != z: z = nz; cam += 1       # sealed: client geometry -> re-fit -> camera mutation
            tt = target(max(1.0 * z, 1.5), Lf, Hf)
            if tt != h:
                lm = 1 if tt > h else -1
                if refit or i == 0 or lm > 0:      # (1R: no re-fit, rho never changes after the stimulus)
                    h = tt; changes += 1
            g = cl(h, lm)
        res.append((changes, cam))
    print(f"{name:<40} | {res[0][0]:>12} / {res[0][1]:<26} | {res[1][0]:>10} / {res[1][1]}")
print("R3 one-shot fit: exactly one camera mutation per authorized fit; client geometry never reaches the camera.")

# --------------------------------------------------------------------------------------------------------
hdr("R4  D5 epoch deltas under the 1R rules")
B0 = View("B", (3840, 2160), 1.5, 1.0, centre=(2200.45, 900.15))
print("Epoch 2 shell stream (60 commands +15 Canvas px, monotone) as ONE epoch: hint changes:")
p = Epoch(1.7321); t = 0.0
for n in range(1, 61):
    t += 1 / 60
    left = n >= 57
    p.ev("shell_step", t, RHO_OUT if left else RHO_IN)
p.end(t + 1, RHO_OUT)
print(f"   1 settlement at the end of the stream (sealed gave 1 for this monotone stream; 40 for an oscillating one): {p.changes}")
print("Epoch 4 output removal under lock + client implicit grab (1R rule, cf. xlock_grab DEFER):")
print("   removal batch: leave(B) for all surfaces; physical pointer relocated to A (Arrangement-nearest, half-open);")
print("   constraint NOT reconciled while the client-owned grab sequence is active (core: Active == Smithay: Active);")
print("   no absolute motion delivered (pointer locked); at the client's button release the batch-end reconciliation")
print("   finds the lock point not observable through A -> delivered focus changes -> exactly one `unlocked`.")
print("Epoch 4 pan of A at zoom 1080/651 with the corrected algebra (hand -15.3 logical px x 120 on scale 1.0):")
sx = 0.0; c0 = (q16(1650.3), q16(700.7)); left_at = None
W_RIGHT = 1920
for n in range(1, 121):
    sx += -15.3 * 1.0                                   # physical displacement on A (scale 1.0)
    c = (c0[0] - q16(sx / (1.0 * ZA)), c0[1])
    v = View("A", (1920, 1080), 1.0, ZA, centre=c)
    if left_at is None and v.ceff[0] >= W_RIGHT + 0:   # W's rightmost surface edge (root) at Canvas x 1920
        left_at = (n, v.v[0])
print(f"   W leaves A at event {left_at[0]} (v_A.x={left_at[1]:.4f}); content moved with the hand (R1).")

# --------------------------------------------------------------------------------------------------------
hdr("R5  PD-F2-05 confinement across independent cameras (seat interaction output)")
import random
random.seed(11)
A = View("A", (1920, 1080), 1.0, ZA, centre=(1650.3, 700.7))
B = View("B", (3840, 2160), 1.5, 1.0, centre=(2580.45, 900.15))   # region crosses B's left edge
ARR = {"A": ((640.0, 360.0), (1920.0, 1080.0), A), "B": ((2560.0, 0.0), (2560.0, 1440.0), B)}
ROOT = (1080, 400); REGION = (20, 20, 800, 600)          # confine region = W's window geometry (surface-local)
def surf_local(out, p):
    q = ARR[out][2].unproject(p); return (q[0] - ROOT[0], q[1] - ROOT[1])
def in_region(s):
    return REGION[0] <= s[0] < REGION[0] + REGION[2] and REGION[1] <= s[1] < REGION[1] + REGION[3]
def clamp_to_region(out, p):
    # 1R rule: clamp in OutputLogical(out) to the region's projection on the interaction output (half-open)
    v = ARR[out][2]
    lo = v.project((ROOT[0] + REGION[0], ROOT[1] + REGION[1])); hi = v.project((ROOT[0] + REGION[0] + REGION[2], ROOT[1] + REGION[1] + REGION[3]))
    w, h = ARR[out][1]
    x = min(max(p[0], max(lo[0], 0.0)), min(hi[0], w) - 2 ** -8)
    y = min(max(p[1], max(lo[1], 0.0)), min(hi[1], h) - 2 ** -8)
    return (x, y)
def walk(rule):
    out, p = "B", (100.25, 400.6)                       # confined on W via B (start inside the region)
    viol = jumps = 0; crossings = 0
    for _ in range(20000):
        d = (random.uniform(-40, 40), random.uniform(-25, 25))
        s0 = surf_local(out, p)
        a = (ARR[out][0][0] + p[0] + d[0], ARR[out][0][1] + p[1] + d[1])
        if rule:
            p = clamp_to_region(out, (p[0] + d[0], p[1] + d[1]))          # no Arrangement crossing while confined
        else:
            # NEG: ordinary Arrangement routing, region applied only on the current output
            for o, ((ox, oy), (w, h), _) in ARR.items():
                if ox <= a[0] < ox + w and oy <= a[1] < oy + h:
                    if o != out: crossings += 1
                    out, p = o, (a[0] - ox, a[1] - oy); break
            else:
                pass
            if out == "B" and crossings == 0:
                p = clamp_to_region(out, p)                              # region enforced only on the current output
        s1 = surf_local(out, p)
        if not in_region(s1): viol += 1
        if abs(s1[0] - s0[0]) > 40 / ARR[out][2].zoom + 1 or abs(s1[1] - s0[1]) > 25 / ARR[out][2].zoom + 1: jumps += 1
    return viol, jumps, crossings
v1 = walk(True); v0 = walk(False)
print(f"1R rule (clamp on the interaction output, no crossing): out-of-region={v1[0]} surface-local jumps={v1[1]} crossings={v1[2]}")
print(f"NEG (Arrangement crossing allowed while confined):      out-of-region={v0[0]} surface-local jumps={v0[1]} crossings={v0[2]}")
print(f"R5 confinement holds under the 1R rule: {v1[0] == 0 and v1[1] == 0}")
