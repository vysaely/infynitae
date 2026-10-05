#!/usr/bin/env python3
import sys; sys.dont_write_bytecode = True  # keep the repository free of __pycache__
"""E1R / E-F2-07: does the canonical scale policy request an excessive/impossible buffer for a
protocol-fullscreen element observed from another output? (Phase E claim: rho = 4.0 -> preferred_scale
4.0 -> 7680x4320 buffer.) Uses the historical model's own ladder()/target() (F-SCALE-2/-3).
Separates (1) semantic correctness, (2) resource cost, (3) device-limit safety.
Output: docs/foundation/evidence/E1R/e1r-fullscreen-output.txt
"""
import contextlib, io, importlib.util, math, os
HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("model", os.path.join(HERE, "model.py"))
model = importlib.util.module_from_spec(spec)
with contextlib.redirect_stdout(io.StringIO()):
    spec.loader.exec_module(model)

FS = (1920, 1080)            # protocol-fullscreen logical size on A (scale 1.0)
OUTS = [1.0, 2.0]            # A 1.0, B 2.0 (4K)
def mb(w, h): return w * h * 4 / 2**20

print("E1R E-F2-07 — fullscreen on A (1.0) observed through B's camera (scale 2.0). H_cfg=%.1f D_max=%d" % (model.H_CFG, model.D_MAX))
L, hmax = model.ladder(OUTS, FS)
print("ladder", [round(x, 4) for x in L], "H_max(E)=%.4f  (= min(H_cfg, D_max/1920 = %.3f))" % (hmax, model.D_MAX / 1920))
fail = 0
for zoom_b in [None, 1.0, 1.5, 2.0, 4.0, 8.0]:
    pres = [1.0 * 1.0] + ([2.0 * zoom_b] if zoom_b else [])     # F-SCALE-1: fullscreen presentation uses zoom 1
    rho = max(pres)
    h = model.target(rho, L, hmax)
    bw, bh = round(FS[0] * h), round(FS[1] * h)
    ib = math.ceil(h - 1e-9)
    fits = max(FS) * h <= model.D_MAX and max(FS) * ib <= model.D_MAX
    fail += not fits
    # semantic: physical px per logical px on B at this zoom vs buffer density (>=1 means no upsampling on B)
    dens_b = (h / (2.0 * zoom_b)) if zoom_b else None
    print(f"  B zoom {str(zoom_b):>4}: rho={rho:>4.1f} -> h={h:.4f} (preferred_scale {round(h*120)}/120, buffer_scale {ib}) "
          f"buffer {bw}x{bh} = {mb(bw, bh):6.1f} MiB (x{(h)**2:4.1f} of scale-1)  device-safe={fits}"
          + (f"  B sampling density {dens_b:.2f}" if dens_b else ""))
print("\nPhase E arithmetic (rho=4.0 sent as preferred_scale 4.0, 7680x4320): h would need to exceed H_cfg=3.0;"
      " canonical h = t(4.0) =", model.target(4.0, L, hmax))
claimed_8k = model.target(4.0, L, hmax) >= 4.0
print("  claimed 8K buffer follows from the canonical formulas:", claimed_8k)
fail += claimed_8k
print("\nReviewer's clamp (rho := scale_A = 1.0 while fullscreen): h=%.4f buffer 1920x1080; B at zoom 2 samples %.2f buffer px per physical px"
      " => 4x upsampling on B. That changes the frozen MaxGesture policy (INV-SCALE-1: rho = max over observing presentations, no fullscreen override; CP2-Q1 left open, cost accepted as RB-7), so it is a policy decision, not a defect fix." % (model.target(1.0, L, hmax), 1.0 / 4.0))
print("RESULT:", "PASS (claim disproved: bounded by H_cfg; cost = accepted RB-7 trade-off; device-safe)" if fail == 0 else "FAIL")
raise SystemExit(1 if fail else 0)
