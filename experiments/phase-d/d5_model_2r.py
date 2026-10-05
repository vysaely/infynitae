#!/usr/bin/env python3
"""Phase D checkpoint 2R — executable MODEL (not an experiment) for PD-F2-19.

Generalises the ScaleInteractionEpoch of 1R (d5_model_1r.py R2) from camera-only motion to every
continuous spatial interaction:  InteractionKind in {CameraPan, CameraZoom, WindowMove, WindowResize,
ShellStream}.  Compares:
  NEG   the effective 1R rule: only camera interactions open an epoch; Stage-moving interactions are
        evaluated atomically (every membership crossing may decrease immediately)
  2R    the generalised rule: every spatial interaction opens an epoch; decreases deferred to termination
        (release / explicit end / settlement timeout for discrete steps / settlement event); increases only
        at ladder rungs in the epoch direction; P7 (device bound) still applies immediately
Ladder/target reused verbatim from experiments/cp3a-scale-model/model.py (provisional constants).
T_settle = 300 ms is the PROVISIONAL candidate (C14.5 / OB-23), not a frozen value.
"""
import os

HERE = os.path.dirname(os.path.abspath(__file__))
_src = open(os.path.join(HERE, "..", "cp3a-scale-model", "model.py")).read().split("def header(")[0]
_m = {}; exec(_src, _m)
ladder, target = _m["ladder"], _m["target"]

L, HMAX = ladder([1.0, 1.5], (840, 640))
T_SETTLE = 0.300
CAMERA_KINDS = {"CameraPan", "CameraZoom", "ShellStream"}
ALL_KINDS = CAMERA_KINDS | {"WindowMove", "WindowResize", "ShellMove"}

def tgt(rho, hm):
    lad = [x for x in L if x <= hm + 1e-12] or [hm]
    return target(rho, lad, hm)

class Policy:
    def __init__(self, h, grouped_kinds):
        self.h, self.kinds, self.changes = h, grouped_kinds, 0
        self.open, self.continuous, self.due, self.dir, self.rho, self.hmax = False, False, None, 0, None, HMAX
    def _settle(self):
        if not self.open: return
        self.open, self.due = False, None
        t = tgt(self.rho, self.hmax)
        if t != self.h:
            self.dir = 1 if t > self.h else -1; self.h = t; self.changes += 1
    def tick(self, now):
        if self.open and self.due is not None and now >= self.due: self._settle()
    def step(self, now, kind, rho, continuous, hmax=None):
        """one update of an interaction of `kind` that changed the requirement to rho"""
        self.tick(now)
        self.rho = rho
        if hmax is not None: self.hmax = hmax
        if kind in self.kinds:
            if not self.open: self.open, self.dir = True, 0
            self.continuous = continuous
            self.due = None if continuous else now + T_SETTLE
        t = tgt(rho, self.hmax)
        if self.h > self.hmax + 1e-12:                         # P7: correctness-forced, immediate
            self.h, self.dir = t, -1; self.changes += 1; return
        if self.open:                                          # grouped: increases only, at rungs, in direction
            if t > self.h and self.dir != -1:
                self.h = t; self.changes += 1; self.dir = 1
        elif t != self.h:                                      # not grouped: an atomic stimulus settles exactly
            self.dir = 1 if t > self.h else -1; self.h = t; self.changes += 1
    def release(self, now):
        """explicit end of a continuous interaction (button release / gesture end)"""
        self.tick(now)
        if self.open and self.continuous: self._settle()
    def end(self, now):
        self.tick(now + 10)
        self._settle()

RHO_ON_B, RHO_A_ONLY = 1.5, 1.0     # W partly on B (scale 1.5) / only on A (scale 1.0)

def drag(p, kind, n=40, dt=1 / 120, continuous=True):
    t = 0.0
    for i in range(n):
        t += dt
        p.step(t, kind, RHO_ON_B if i % 2 == 0 else RHO_A_ONLY, continuous)
    p.release(t + 0.001)
    p.end(t + 1)
    return p

def resize_near_bound(p):
    # interactive resize: W grows onto B and back (rho toggles); twice it crosses the device bound
    t = 0.0
    for i in range(40):
        t += 1 / 120
        hm = 16384 / (12000 if i in (13, 27) else 4000)  # device bound H_max (1.365) dips below h twice
        p.step(t, "WindowResize", RHO_ON_B if i % 2 == 0 else RHO_A_ONLY, True, hmax=min(3.0, hm))
    p.release(t + 0.001); p.end(t + 1)
    return p

def shell_move_with_pauses(p):
    # shell MoveElement stream: 3 bursts of 10 oscillating steps (60 Hz), 1 s pauses (> T_settle)
    t = 0.0
    for burst in range(3):
        for i in range(10):
            t += 1 / 60
            p.step(t, "ShellMove", RHO_ON_B if i % 2 == 0 else RHO_A_ONLY, False)
        t += 1.0
    p.end(t)
    return p

print("PD-F2-19 model — ScaleInteractionEpoch generalised to spatial interactions")
print(f"ladder {[round(x, 4) for x in L]}; T_settle candidate {T_SETTLE*1000:.0f} ms (PROVISIONAL)\n")
cases = [
    ("WindowMove: window dragged back and forth across B's edge (40 updates)", lambda p: drag(p, "WindowMove")),
    ("WindowResize: resize oscillating across B's edge, device bound crossed twice", resize_near_bound),
    ("ShellMove: shell MoveElement stream, 3 bursts x 10 oscillating steps, 1 s pauses", shell_move_with_pauses),
    ("CameraPan regression: pan oscillating across B's edge (40 updates)", lambda p: drag(p, "CameraPan")),
    ("ShellStream (camera) regression: 40 oscillating Navigate commands", lambda p: drag(p, "ShellStream", continuous=False, dt=1 / 60)),
]
print(f"{'case':<82} | NEG (1R: camera-only epochs) | 2R (all spatial interactions)")
ok = True
for name, f in cases:
    neg = f(Policy(1.5, CAMERA_KINDS)).changes
    new = f(Policy(1.5, ALL_KINDS)).changes
    print(f"{name:<82} | {neg:>28} | {new:>29}")
    if name.startswith("WindowMove"):
        ok &= neg >= 20 and new == 1
    elif name.startswith("WindowResize"):
        ok &= new <= 1 + 2 + 2           # settle + one P7 per bound crossing + re-increase after it
    elif name.startswith("ShellMove"):
        ok &= new <= 6 and neg >= 20     # <= (1 rung + 1 settlement) per epoch; 3 epochs (pauses > T_settle)
    else:
        ok &= new <= 1
print(f"\nwindow drag across a scale boundary: NEG churns, 2R settles in exactly 1 change: {ok}")
print("P7 (device bound) still lowers h immediately inside an epoch; it is a correctness rule, not damping.")
