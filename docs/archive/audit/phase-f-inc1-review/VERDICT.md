# INC-1 Independent Review Verdict

## 1. Formal Verdict

```
INC-1 CONDITIONAL PASS — CORRECTIONS REQUIRED BEFORE RATIFICATION
```

## 2. Justification

The INC-1 implementation successfully establishes the minimal real vertical seam on the production path:
1. One real Wayland client maps an element containing a root surface, subsurface above, subsurface below, nested subsurface, popup, and popup subsurface with custom input regions.
2. The snapshot is extracted via public Smithay 0.7.0 APIs, processed through the single origin function `F-ORIGIN-1`, placed on the Stage via `stage::place`, rendered through `infynitae-core`'s lattice projection and `infynitae-wayland::convert` into `WaylandSurfaceRenderElement`s with `DummyRenderer`, and hit-tested in physical pixels.
3. Zero Level A findings were discovered. Authority invariants (`INV-CAMERA-1`, `INV-OUTPUT-2`), stage invariants (`INV-STAGE-1` through `INV-STAGE-6`), coordinate invariants (`INV-COORD-1`, `INV-COORD-2`, `INV-COORD-4`, `INV-COORD-6`), render invariants (`INV-RENDER-1`, `INV-RENDER-2`), hit-test invariants (`INV-HIT-1`), and architecture invariants (`INV-ARCH-1`, `INV-ARCH-2`) strictly hold.
4. Parity with pinned Smithay 0.7.0 is demonstrated mathematically and empirically with zero pixel drift across scales 1.0, 1.1, 1.25, 1.333, 1.5, 1.667, 1.75, 2.0, 2.25.
5. The `ALLOWED_DEV_EXTERNAL` whitelist for `wayland-client` and `wayland-protocols` in `infynitae-backend-headless` dev-dependencies is strictly isolated to the test harness, required by `FOUNDATION_TEST_PLAN` T1, and protected by four permanent negative controls.

However, independent adversarial analysis and mutation testing revealed two Level B sensitivity and test oracle gaps (`INC1-F06` and `INC1-F07`) that must be addressed before ratification and Phase F progress to INC-2.

## 3. Conditions Required Before Ratification

To achieve unconditional ratification, the following corrective patches must be applied:

### Condition 1: Address Finding INC1-F06 (Test Oracle Independence)
- **Problem:** In `crates/infynitae-backend-headless/tests/t1_parity.rs` and `t1_slice.rs`, the test oracle computes `q = view.unproject(p)` using production `View::unproject` to check `eligible(e, q)`. If `unproject` had coordinate transposition defects, `tst10_xhit` and `vertical_slice_hit_test_equals_presented_surface` would not detect it.
- **Required Action:** Author an independent oracle calculation for unprojection in the T1 test harness, or add an independent test in `infynitae-backend-headless` verifying that unprojected coordinates match an independently computed geometric ground truth across the entire rendered surface tree.

### Condition 2: Address Finding INC1-F07 (Unmapped Surface Culling Test)
- **Problem:** `render::entries_for` contains `if !shown { continue; }`, and `SurfaceTree::shown()` is unit-tested in `snapshot::tests`. However, no test in the test suite exercises `render_list` or `entries_for` with an unmapped/unshown subsurface present in the snapshot. Removing `if !shown { continue; }` passes all 39 existing tests.
- **Required Action:** Add a test in `infynitae-core` (or `t1_slice`) where an element contains an unmapped subsurface, asserting that the unmapped subsurface is strictly omitted from `ProjectionFrame::entries` and cannot be hit-tested. Add a permanent mutation control in `tools/verify-test-efficacy`.

## 4. Obligations Status at Verdict

- `OB-2` (Input region evaluation): **MECHANICALLY SATISFIED — RATIFICATION CONTINGENT ON CONDITIONS**.
- `OB-26` (Smithay parity regression guard): **MECHANICALLY SATISFIED — RATIFICATION CONTINGENT ON CONDITIONS**.
- `OB-1` (Nested popup chains): **OPEN** (Owned by INC-7).
- `OB-6` (Coordinate projection and lattice edges): **T0 PART SATISFIED; OPEN** (T4 physical display part remains).
- `OB-14` (Rotated transforms, zoom ≠ 1): **OPEN** (Owned by INC-10 / OB-45).
- `OB-17` (Real CSD client window geometry): **OPEN** (Owned by T3).
- `OB-49` (Runtime 2i / OutputKey collision binding): **OPEN** (Owned by INC-2).

## 5. Next Authorized Action

The next authorized action is the implementation of the corrective patch resolving `INC1-F06` and `INC1-F07`, followed by independent verification.

**INC-2 MUST NOT BEGIN.**
