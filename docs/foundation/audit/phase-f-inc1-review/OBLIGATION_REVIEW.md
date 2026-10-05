# Architectural Obligations Review and Dispositions

## 1. Primary Obligations Under Review

### 1.1 Obligation OB-2: General Input-Region Subtraction
- **Canonical Definition (`FOUNDATION_ARCHITECTURE.md` line 1053):**
  "Input-region subtraction, general. Multi-rect, interleaved add/subtract; random property test. Equals a reference region evaluator. T0 + T1."
- **Review Evaluation:**
  - T0 unit property test `ob2_region_equals_reference_evaluator` tests 400 random regions with up to 8 operations across 2,304 cells against an independent 2D bitmap reference evaluator. 0 differences.
  - T1 tests (`tst10_xhit_selection_equals_presented` and `vertical_slice_hit_test_equals_presented_surface`) verify input regions on a real Wayland client over a socketpair, including hole punch-through and shadow exclusion.
  - Independent adversarial test `adversarial_complex_input_regions` probed 4-deep nested island/hole structures and negative coordinates.
  - Test efficacy control `input region: first matching request decides instead of the last` confirmed that traversal order defects are caught.
- **Disposition:** `OB-2 MECHANICALLY SATISFIED — READY FOR RATIFICATION UPON CONDITIONAL CLEARANCE`.

### 1.2 Obligation OB-26: Smithay Parity Regression Guard
- **Canonical Definition (`FOUNDATION_ARCHITECTURE.md` line 1077, `FOUNDATION_TEST_PLAN.md` line 361):**
  "Regression guard: never use `Window::surface_under`; keep xsnap/xhit/X-15 as tests on the production path. Ensure `Space` is never constructed."
- **Review Evaluation:**
  - T1 regression test suite in `crates/infynitae-backend-headless/tests/t1_parity.rs` runs all four core tests (`tst04_x6`, `tst09_xsnap`, `tst10_xhit`, `tst18_d5hit_zoom1`) against the production core, adapter, and Smithay `DummyRenderer`.
  - Negative controls:
    - `surface_under` negative control confirms disagreement with presented stacking, explained 100% by W-SM-11 and W-SM-12.
    - canvas-exact negative control confirms disagreement at fractional scales.
    - i32-root negative control confirms that integer-truncated root locations fail edge parity.
  - Statically enforced by `enforcement/source-rules`: rule `smithay-rejected-apis` checks all 6 product crates and forbids `Space`, `surface_under`, `render_elements_from_surface_tree`, `output_enter`, and `output_leave`. 3 seeded controls verify rejection.
- **Disposition:** `OB-26 MECHANICALLY SATISFIED — READY FOR RATIFICATION UPON CONDITIONAL CLEARANCE`.

---

## 2. Tracking of Other Obligations Exercised or Touched

| ID | Title | Scope in INC-1 | Status | Next Owner |
|---|---|---|---|---|
| `OB-1` | Nested popup chains | 1 popup level supported in INC-1 | **OPEN** | INC-7 Exit |
| `OB-6` | Coordinate projection & lattice edge formulas | T0 property test satisfied (4,000 random cameras); physical display part deferred | **OPEN** | T4 (Physical Output) |
| `OB-14` | Transforms & zoom ≠ 1 | Normal transform at zoom 1 verified; rotated transforms and zoomed cameras deferred | **OPEN** | INC-10 / OB-45 |
| `OB-17` | Real CSD clients | Window geometry loc accepted | **OPEN** | T3 (Real Hardware) |
| `OB-49` | Runtime 2i / OutputKey binding | Refusal of second output preserves boundary; binding precedence not implemented | **OPEN** | INC-2 (First Owner) |
| `OB-50` | Reconfiguration authority isolation | `output_reconfigured` mints no proof and calls no navigation items | **OPEN** | INC-2 |
