#!/usr/bin/env python3
import sys; sys.dont_write_bytecode = True  # keep the repository free of __pycache__
"""E1R / E-F2-06: the domain of F-SCALE-2 (device ceiling H_max) at zero spatial extent.

Sealed (C12.R1 INV-SCALE-2 / F-SCALE-2):  H_max(E) = min(H_cfg, D_max / s_max(E)),
  s_max(E) = largest logical side over all surfaces of E's tree.

E1R definition (from what the bound MEANS, not a guard): the device bound is the supremum of the hints
whose buffers fit the device,
  H_dev(E) = sup { h > 0 : h * side <= D_max for every logical side of every BUFFERED surface of E }
           = D_max / s_max(E)   if s_max(E) > 0
           = +inf               if s_max(E) = 0   (no buffer: the constraint set is empty, nothing to bound)
  H_max(E) = min(H_cfg, H_dev(E))  -> equals H_cfg when s_max(E) = 0 (H_cfg is finite, PROV-1).
s_max(E) = 0 iff NO surface of E has a non-zero side, i.e. 0x0 everywhere; 0xN / Nx0 have s_max = N > 0.

Reachability (protocol + Smithay): an xdg_toplevel's initial commit carries no buffer (xdg-shell requires
the initial commit without a buffer; F-SCALE-3 computes the initial hint at exactly that point, M1), so
every surface of the tree has extent 0x0 => s_max = 0 is REACHABLE on every window's first commit.
After a buffer is attached each side is >= 1 logical px (wl_shm rejects non-positive width/height;
wp_viewport destination must be > 0 or unset), so 0xN / Nx0 arise only from unbuffered members of a
tree, which do not change s_max.

Negative control: the sealed formula (as implemented by cp3a-scale-model/model.py `ladder`) raises
ZeroDivisionError on s_max = 0.
Output: docs/foundation/evidence/E1R/e1r-hmax-output.txt
"""
import contextlib, io, math, importlib.util, os

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("model", os.path.join(HERE, "model.py"))
model = importlib.util.module_from_spec(spec)
with contextlib.redirect_stdout(io.StringIO()):
    spec.loader.exec_module(model)  # the historical model; its demo output is discarded
H_CFG, D_MAX = model.H_CFG, model.D_MAX


def s_max(tree):
    """tree: list of (w, h) logical extents of E's surfaces; unbuffered surfaces are (0, 0)."""
    return max((max(w, h) for (w, h) in tree), default=0)


def h_dev(tree):
    s = s_max(tree)
    return D_MAX / s if s > 0 else math.inf


def h_max_e1r(tree):
    return min(H_CFG, h_dev(tree))


def sealed(tree):
    """F-SCALE-2 as sealed, via the historical model's own ladder() (max logical side of the tree)."""
    s = s_max(tree)
    _, hm = model.ladder([1.0, 1.5], (s, s))
    return hm


def feasible(h, tree):
    """the meaning of the bound: every buffered side fits the device"""
    return all(h * side <= D_MAX + 1e-9 for (w, hh) in tree for side in (w, hh) if (w, hh) != (0, 0))


cases = [
    ("first commit: toplevel, no buffer (0x0)", [(0, 0)]),
    ("unbuffered tree: root + subsurface, no buffers", [(0, 0), (0, 0)]),
    ("0xN (degenerate side, other side N)", [(0, 600)]),
    ("Nx0", [(800, 0)]),
    ("buffered root + unbuffered subsurface", [(800, 600), (0, 0)]),
    ("ordinary 800x600", [(800, 600)]),
    ("large 8000x100 (device bound below H_cfg)", [(8000, 100)]),
]
print("E1R E-F2-06 — F-SCALE-2 domain at zero extent. H_cfg=%.2f D_max=%d" % (H_CFG, D_MAX))
fail = 0
for name, tree in cases:
    try:
        s = "%.4f" % sealed(tree)
    except ZeroDivisionError as e:
        s = "ZeroDivisionError (%s)" % e
    e = h_max_e1r(tree)
    ok = math.isfinite(e) and e > 0 and feasible(e, tree) and (s_max(tree) > 0 or e == H_CFG)
    # supremum property: anything above H_dev is infeasible when the device constraint exists
    if s_max(tree) > 0 and h_dev(tree) < H_CFG:
        ok &= not feasible(h_dev(tree) * 1.001, tree)
    fail += not ok
    print(f"  {name:<50} s_max={s_max(tree):>5}  sealed H_max={s:<45} E1R H_max={e:.4f}  {'ok' if ok else 'FAIL'}")

print("\nTransition zero extent -> first buffer (P7 device bound applies at the commit that attaches it):")
for first in [(800, 600), (8000, 100), (16384, 16384)]:
    # initial hint at 0x0 for rho_pred = 4.5 (scale 1.5 output observed at zoom 3): ladder bounded by
    # H_max(0x0) = H_cfg; model.ladder at (1,1) has the same ceiling (16384/1 > H_cfg)
    L0, hm0 = model.ladder([1.0, 1.5], (1, 1))
    assert hm0 == h_max_e1r([(0, 0)])
    h0 = model.target(4.5, L0, hm0)
    hm = h_max_e1r([first])
    forced = h0 > hm + 1e-12
    h1 = hm if forced else h0
    ok = feasible(h1, [first]) and h1 <= hm + 1e-12
    fail += not ok
    print(f"  0x0 (h={h0:.4f}, H_max={h_max_e1r([(0, 0)]):.4f}) -> {first}: H_max={hm:.4f}; P7 forced decrease={forced}; h={h1:.4f}; buffer {first[0]*h1:.0f}x{first[1]*h1:.0f} <= {D_MAX}: {'ok' if ok else 'FAIL'}")

print("\nE1R-N3 — integer emission vs the device bound (integer-scale clients render at preferred_buffer_scale):")
def b_sealed(h, tree):
    return math.ceil(h - 1e-9)                      # F-SCALE-3 / INV-SCALE-5 / PROV-9: ceil(h)
def b_e1r(h, tree):
    c = math.ceil(h - 1e-9)                         # E1R: ceil(h) unless it breaks the device bound
    hd = h_dev(tree)
    return c if c <= hd + 1e-9 else max(1, math.floor(hd + 1e-9))
neg_n3 = 0
for tree in ([(800, 600)], [(8000, 100)], [(6000, 4000)], [(5000, 5000)], [(0, 0)], [(20000, 100)]):
    h = h_max_e1r(tree)                             # worst case: hint at the ceiling
    bs, be = b_sealed(h, tree), b_e1r(h, tree)
    side = s_max(tree)
    fit_s, fit_e = side * bs <= D_MAX, side * be <= D_MAX
    residual = side > D_MAX                         # logical side beyond the device even at scale 1
    ok = fit_e or residual
    fail += not ok
    neg_n3 += (not fit_s) and not residual
    print(f"  s_max={side:>6} h={h:.4f}  sealed ceil={bs} -> {side*bs:>6}px {'fits' if fit_s else 'EXCEEDS D_max'} | E1R b={be} -> {side*be:>6}px {'fits' if fit_e else ('RESIDUAL: side > D_max at scale 1' if residual else 'EXCEEDS')}")
print("  negative control: sealed ceil(h) exceeds D_max in %d case(s)%s" % (neg_n3, " (reproduces E1R-N3)" if neg_n3 else " (UNEXPECTED)"))
fail += neg_n3 == 0

try:
    model.ladder([1.0], (0, 0))
    neg = "sealed formula returned (UNEXPECTED)"
    fail += 1
except ZeroDivisionError:
    neg = "sealed formula raises ZeroDivisionError on s_max = 0 (negative control reproduces E-F2-06)"
print("\nNEGATIVE CONTROL:", neg)
print("RESULT:", "PASS" if fail == 0 else "FAIL (%d)" % fail)
raise SystemExit(1 if fail else 0)
