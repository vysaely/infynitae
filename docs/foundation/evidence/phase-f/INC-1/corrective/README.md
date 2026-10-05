# INC-1 corrective verification and ratification

## History

| Step | Commit / record | Result |
|---|---|---|
| INC-1 implementation | `b11be48` (code), `8f44f8b` (evidence) | implemented; OB-2/OB-26 OPEN pending review |
| Independent review | dossier [`audit/phase-f-inc1-review/`](../../../../audit/phase-f-inc1-review/), recorded unchanged in `38c60ca` | **INC-1 CONDITIONAL PASS — CORRECTIONS REQUIRED BEFORE RATIFICATION**; INC1-F06, INC1-F07 (Level B) |
| Corrective patch | `1fa72af` fix(test): close INC1-F06 and INC1-F07 efficacy gaps | tests and controls only; production code unchanged |
| Corrective verification | outputs in this directory at `1fa72af`; GitHub Actions run 37291919770 on `1fa72af` | all PASS (below) |
| Ratification | the commit adding this record | OB-2 and OB-26 `CLOSED`; INC-1 ratified |

The review's verdict and findings stand as written. This record does not replace them.

## INC1-F06 — shared unprojection oracle (Level B): RESOLVED

**Root cause:**
- `t1_slice::vertical_slice_hit_test_equals_presented_surface` and `t1_parity::hit_parity` (TST-10, TST-18) computed the oracle's Canvas point with production `View::unproject`.
- The same function feeds production `ProjectionFrame::select`, so a defect in it moved both sides. ADV-MUT-3, which transposes x and y, was MISSED.

**Correction:**
- **Independent T1 oracle.** `crates/infynitae-backend-headless/tests/common/oracle.rs` derives `c_eff` (F-VIEW-1, F-LATTICE-1) and `q` (F-UNPROJECT-1) from the camera parameters in exact integer arithmetic, written from the formula text.
  - It calls no `View` method and no other production transformation.
  - Both T1 hit oracles use it, and each run also asserts that production `View::c_eff` equals the oracle's.
- **Hand-derived T0 table.** `projection::tests::unproject_matches_the_independent_reference` holds 56 literal rows, generated outside the crate with exact rational arithmetic by [`unproject_reference.py`](unproject_reference.py).
  - It checks `c_eff`, `unproject` and `project`.
  - Coverage:
    - origin, negative and fractional cameras;
    - scales 1, 1.25, 1.5, 1.75 and 1.1, 4/3, 5/3, 2.25;
    - exact pixel boundaries `i/k`, their `nextafter` neighbours, and ±10⁻⁷;
    - centres near +2³⁶ and −2³⁶.
  - Production matched all 56 rows exactly.

**Permanent mutation controls** (`tools/verify-test-efficacy`, all DETECTED):
- ADV-MUT-3, x/y transposition, caught by T1 xhit, the T1 slice and the T0 table;
- unproject sign inversion on x (T1 xhit);
- camera-offset omission on y (T1 xhit).

## INC1-F07 — unshown-surface culling untested (Level B): RESOLVED

**Root cause:** every test tree had all surfaces mapped. `SurfaceTree::shown()` was tested only in isolation, so removing `if !shown { continue; }` from `render::entries_for` (ADV-MUT-2) was MISSED.

**Correction:**
- **T0 tests through `Core::render_list` and `Core::hit_test`:**
  - `unshown_subsurface_is_not_in_the_frame`;
  - `unshown_sibling_is_not_in_the_frame`;
  - `unshown_nested_subsurfaces_are_not_in_the_frame` (an unmapped parent with a mapped child, and a mapped parent with an unmapped child);
  - `subsurface_unmap_and_remap_change_the_frame`.

  Each asserts that the unshown `SurfaceKey` is absent from `ProjectionFrame::entries` and is never hit.
- **T1 test `unmapped_subsurface_leaves_the_frame_and_returns`:**
  - A real client attaches a null buffer to S1. S1 and its nested S1a leave the frame, and the pixel under S1 hits the root.
  - Smithay's own `Window` path presents the same number of surfaces.
  - Remapping S1 restores both surfaces.

  Smithay 0.7.0 skips the children of a surface without a view (`render_elements_from_surface_tree`), which is the semantics `shown()` encodes. No other INC-1 form of "not shown" exists.

**Permanent mutation controls:** ADV-MUT-2, caught at T0 and T1; both DETECTED.

## Adjacent oracle inspection

| Oracle | Shares production code? | Disposition |
|---|---|---|
| T1 hit oracles: region eligibility | used `InputRegion::contains` | **fixed in this patch**: independent `oracle::region_contains` |
| T1 hit oracles: surface origins and input data in `FrameEntry` | inputs come from production F-ORIGIN-1 and extraction | not the property under test; F-ORIGIN-1 is checked against client ground truth (TST-09) and hand-derived T0 values, with mutation controls; reported, unchanged |
| T0 `ob6_…` re-projection property | `unproject` vs `project` | the canonical OB-6 criterion is that round trip; now also anchored by the reference table; unchanged |
| T0 canvas-exact negative control | uses `unproject` | a negative control, not an oracle; unchanged |

No other directly adjacent oracle shares the function it claims to verify.

## Verification at `1fa72af` (outputs in this directory)

- **Cargo:** check, build, test (47 passed), fmt and clippy `-D warnings` all PASS.
- **`tools/verify-architecture`:** PASS. All 27 compile probes give their asserted diagnostics; 46 seeded controls detected, including the F01/F02 regressions.
- **`tools/verify-test-efficacy`:** PASS, 25/25 defects detected (18 previous + 7 new), judge controls correct, canonical tree byte-identical afterwards.
- **Foundation checker:** PASS. Self-test 18/18. Historical spike: PASS.
- **`tools/verify-development-environment`:** ALL CHECKS PASSED.
- **GitHub Actions run 37291919770 on `1fa72af`:** foundation-docs, verify-architecture and workspace-build (including test efficacy) all succeeded.

## Ratification

- **OB-2 `CLOSED`:** pass criterion "equals a reference region evaluator"; closed by the T0 property test plus the T1 xhit port, which now uses an independent region and unprojection oracle.
- **OB-26 `CLOSED`:** xsnap, xhit and X-15 ported to T1 with their negative controls; `Window::surface_under` and `Space` never used, enforced by `smithay-rejected-apis`.
- **Still `OPEN`:**
  - OB-1 (INC-7);
  - OB-6 (T0 part satisfied; T4 part remains);
  - OB-14 (INC-10, OB-45);
  - OB-17 (T3);
  - OB-49 (INC-2: runtime 2i, binding).
- **Mandatory:** INV-CAMERA-3 and INV-OUTPUT-2 remain mandatory. Runtime 2i stays owned by INC-2 / OB-49.
- **Open findings:** Level A 0, Level B 0 (F06 and F07 resolved), Level C 1 (INC1-F01, INC-7).
