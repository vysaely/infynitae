# INC-10 — implementation record (2026-10-18)

Status: **IMPLEMENTED on local `main`; exit evidence PRODUCED, NOT ACCEPTED.** INC-10's exit evidence
waits for independent review and architecture change control; **OB-24 remains OPEN**. Not pushed
(`origin/main` = `23752f1`). Software renderers only: nothing here measures or establishes GLES, a
GPU, dmabuf import, direct scanout or hardware planes.

## 1. Authority and base

Owner brief "INC-10 Production Implementation and Evidence" (2026-10-18): authorizes INC-10 and
production code on local `main`; no push, no closing of OB-24, no rewriting of C12.F14/F15/F16.
Base: local `main` `e75ea72` (C12.F16 adopted), clean; ancestry `270128f` (C12.F14), `dda4d87`,
`da538d6` (C12.F15), `e75ea72` verified.

## 2. Commits

| Commit | Checkpoint | Content |
|---|---|---|
| `b46021f` | kept reference (F-LATTICE-1, C12.F14) | `R_O` per output in Core (`LatticeSlot`), set at connect / (re-)binding / a change of `k_O` or `view_O` / past the 2³¹ bound (per axis), kept while panning, dropped with the output; `View::with_kept_reference`; the INC-3 zoom-1 pan gate removed (G-OB24 met); `grid_value` and the C12.F15.7 Q16 test |
| `2884287` | A, B, C (C12.F15) | `convert::CanonicalRect` (geometry = Core's rect, `draw` passes `dst`, remap → full damage, no opaque regions, no underlying storage), the ±2³⁰ crop, the tracker rebuilt when the frame's k differs; TST-04 redesigned; `t1_canonical_rect` |
| `cdf3a09` | D (F-FIT-1, C12.F15; OBJ-30, C12.F16) | `navigation::fit_target`, `NavigationHistory` (fixed 256-entry ring per bound camera), fit / restore, history emptied at unbinding; Logo+F / Logo+Shift+F; `t1_navigation` |
| `73c0aca` | E | `t1_zoomed_lifecycle`; INV-ALLOC-1 at zoom ≠ 1 and for fit/restore; fit/restore append to the caller's effect buffer |
| this record's commit | evidence, docs | the no-fit-during-an-interaction test; `perf_inc10`; this directory; CHECKPOINT, plan, test plan, object model, state machines |

## 3. Exit evidence (FOUNDATION_IMPLEMENTATION_PLAN INC-10)

| # | Item | Evidence (production path) | State |
|---|---|---|---|
| 1 | 0 width/gap changes per zoomed pan, kept reference | T1 `t1_input::zoomed_pan_moves_by_f_pan_1_and_keeps_widths` (k = 0.73, 240 pan steps across > 5 lattice cells; the per-frame reference changes a width at step 63 — seeded control); T0 `a_kept_lattice_reference_keeps_widths_and_gaps_during_a_zoomed_pan` (C12.F14); `t1_zoomed_lifecycle` (W's root constant through a pan with lifecycle events) | produced |
| 2 | render/hit agreement through the wrapper | `t1_canonical_rect::render_and_hit_agree_through_the_wrapper_at_rounding_points`: 12 configurations (per-frame at scale 1 and 1.5; kept references at ±2²⁰ and 0.999·2³¹ on both sides of R; x and y; negative and 2³⁶-scale origins), k swept ±60 ulps around rounding points: **139,392 rects, 0 presented ≠ Core, 0 seams at A\|B and A/C, 0 wrong pointer pixels of 627,264**, 301 rects where Smithay's own geometry differs (all remapped, none else); Pixman pixels and column-by-column pointer at a seam zoom (`pixels_and_pointer_follow_the_canonical_rect_at_a_seam_zoom`); `render_hit_sweep.out` | produced |
| 3 | rounding-step and Q16 tests | `kept_reference_rounding_steps_at_the_visible_edge_limit` (C12.F14); `grid_value_precision_at_the_conversion_limit`: \|v − R\| just above 2³⁷ px both sides of R, k = 2⁻²², the conversion rounds, error ≤ 3·2⁻²² (and > 0), exact ties round up | produced |
| 4 | F-FIT-1 | `navigation::tests`: lower clamp below/at/above, a 1 px W at scale 0.25 reaching 2¹⁶, scale 2 exact fit, degenerate W, centre exact and clamped at ±2³⁶, `View::new` acceptance; Core: zero-size and no-element fits refused with no mutation and no entry, exact restore, one true-position event after fit and restore, none during an interaction | produced |
| 5 | damage invalidation on a zoom change | `t1_navigation::a_zoom_change_rebuilds_the_damage_tracker` (seeded control: no rebuild); see §5 item 4 on "exactly" | produced |
| 6 | i32 crop, rotated and flipped buffers | all eight buffer transforms: Pixman pixels of a cropped surface equal the uncropped one inside the box and nothing outside (cuts on whole buffer pixels); extreme rects beyond ±2³¹ checked on the `dst`/`src` the tracker hands to `draw`; the transform mapping checked against Pixman pixels and the GLES texture-matrix algebra | produced, **with a deviation from C12.F15.5's named formula (§5 item 1)** |
| 7 | map, unmap, destroy during a zoomed pan | `t1_zoomed_lifecycle` | produced |
| 8 | performance and INV-ALLOC-1 | §4 | produced (software only) |
| 9 | seeded controls of C12.F15.8, TST-04 redesigned | 10 controls for C12.F15.8 (1)–(5), 1 for (6), TST-04's checks moved to the element inside the wrapper with its negative control kept, plus the counterpart `the_wrapper_removes_the_unwrapped_mismatch`; full `verify-test-efficacy` on the final tree | produced (see gate log) |
| 10 | all gates on the final commit | CHECKPOINT records the run | see CHECKPOINT |
| 11 | the bounded history (C12.F16.6) | `lib.rs` tests 1–11 and 13 controls (§6) | produced |

Also: the 200-commit "client resize never moves the camera" test did **not exist** on the production
path (the plan says "intact"); it is added: `t1_navigation::client_resizes_never_move_a_fitted_camera`.
The NB-1 runtime test (ignored by default) was run: passed (`nb1_runtime.out`).

## 4. Performance and allocations (measured; software renderers only)

`perf_inc10` (ignored test, release build), AMD Ryzen 5 7600X3D, rustc 1.99.0, load average ≈ 2;
1920 × 1080 output, 20 client windows × 3 surfaces (real `wl_shm` buffers), 600 frames after 60
warm-up frames per configuration, wall clock per frame, two runs (`perf_frame_times_run{1,2}.out`,
consistent within noise). "Panning" moves the camera by one event before each frame (full repaint);
"static" changes nothing (no damage).

| Panning, run 2 (ms) | p50 | p99 | max |
|---|---|---|---|
| Dummy renderer (Core + adapter + tracker, no drawing), zoom 1 | 0.017 | 0.023 | 0.032 |
| Dummy, zoom 0.73 / 1.797 / 2.391 / 0.25 / 8 | ≤ 0.050 | ≤ 0.057 | ≤ 0.067 |
| Pixman, zoom 1 | 0.513 | 0.574 | 0.642 |
| Pixman, zoom 0.73 | 1.215 | 1.320 | 1.458 |
| Pixman, zoom 1.797 | 1.115 | 1.383 | 1.550 |
| Pixman, zoom 2.391 | 1.023 | 1.101 | 1.199 |
| Pixman, zoom 0.25 | 0.382 | 0.414 | 0.591 |
| Pixman, zoom 8 | 0.824 | 0.875 | 1.192 |

Measured facts: Core's render list is ≤ 0.002 ms p50 in every configuration; the adapter's elements
(wrapper included) ≤ 0.022 ms p50; with no drawing the whole path stays ≤ 0.067 ms. The Pixman
frame at zooms 0.73 … 8 costs about 2× zoom 1 (at 0.25 less), and the difference is in Pixman's
compositing (the "tracker+draw" column), not in Core or the adapter. Hypothesis, not tested: scaled sampling in the
software rasteriser versus 1:1 copies. Not measured: GLES or GPU frame time, presentation or vblank
pacing (the headless backend has no display), other machines.

INV-ALLOC-1 (`allocations_zoomed.out`, counting allocator, debug build): at zoom 0.73 with 100
windows a pan step, a pan step across windows, a routed motion plus the render list allocate 0
after warm-up; 300 fits (through capacity, each evicting) and 256 restores allocate 0 after the
binding's first fit. Limit: these fits repeat one target, so no scale-hint effect is produced; a fit
that changes hints emits them through the scale pass's existing effect `Vec`, which is not measured
here. Smithay's own per-frame allocations (the tracker's state map, the element vector) are outside
INV-ALLOC-1's Core scope and are not counted.

## 5. Findings for change control (the ratified text is not edited)

1. **C12.F15.5 names Smithay's `CropRenderElement` formula for the `src` crop; that formula selects
   the wrong part of the buffer under the four rotating buffer transforms** (90°, 270°, flipped 90°,
   flipped 270°). Shown in Pixman pixels with Smithay's own `CropRenderElement`
   (`smithays_crop_element_misdraws_rotated_buffers`). Cause: it maps the visible part through
   `Rectangle::to_buffer(.., transform, ..)`, the inverse of the rotation Smithay's renderers apply
   (GLES `build_texture_mat`, Pixman `render_texture_from_to`); and Smithay's `transform_rect_in`
   swaps flipped 90°/270° relative to `transform_point_in`. The production crop instead follows the
   renderers' mapping (`convert::sampled_axes`), proportional as C12.F15.5 requires, correct for all
   eight transforms in pixels. **Decision needed:** an erratum to C12.F15.5 / F-RENDER-1 replacing
   "Smithay's `CropRenderElement` formula" by "the renderers' sampling map", and recording that
   buffer transforms are now verified (Pixman pixels, GLES algebra; no GPU run).
2. **The crop box `[−2³⁰, 2³⁰]` spans 2³¹ px, one more than Smithay's i32 `Size` holds.** A rect over
   the whole box keeps its far edge one pixel inside (at 2³⁰ − 1, outside every view). **Decision
   needed:** an erratum stating the box as half-open or the width cap.
3. **Pixman rounds `src` out to integers (`to_i32_up`)**: a fractional cut cannot be drawn exactly by
   the software renderer; pixel checks use cuts on whole buffer pixels; fractional `src` values are
   checked arithmetically. Production crops only beyond ±2³⁰ px.
4. **C12.F15.4 says the age-1 repaint must damage "exactly" the canonical rect.** Exact for a remapped
   surface; for a pass-through surface Smithay rounds the element's own damage outwards
   (`to_physical_precise_up`), measured one row larger. The test accepts at most one pixel at the far
   edges; the over-damage repaints already-correct pixels. **Decision needed:** whether "exactly"
   means this.
5. **Not owner-decided, Level D defaults applied:** the fit target (the element under the pointer on
   the pointer's output) and the bindings (Logo+F fit with Shift absent, Logo+Shift+F restore), the
   INC-10 brief's §11 recommendation; neither binding resolves during an interaction (a pan computes
   the camera from its own start). Change by owner decision; the code isolates both in
   `input::keys` and `entry::fit_viewport`.
6. **Seeded controls whose premise option B removed** were re-targeted, not deleted: "render
   location floored" and "render location one pixel right" (the presented rect no longer depends on
   the location) → TST-04 (the element inside the wrapper must equal Core's rect at k ∈ n/120);
   "tracker built at scale 1" (geometry no longer reads the tracker scale) → the pass-through repaint
   at k ≠ 1. The INC-3 OB-24 gate test and control are replaced by zoomed-pan tests and controls
   (C12.F14 lifted the gate).
7. **A defect in `tools/verify-test-efficacy` (pre-existing, fixed here).** Every control builds in
   one shared target directory from a fresh copy that keeps the canonical files' modification
   times; cargo decides freshness by modification time and its artifacts do not depend on the
   copy's path, so a crate seeded by one control stayed built from the seeded source for later
   controls. Found because the C12.F15.8 (6) control was MISSED in two full runs and DETECTED alone:
   the render-location seed in Core, two controls earlier, made S remapped. The tool now gives every
   file whose last build may have been seeded a fresh time in each copy (per crate unit, until a
   build certainly recompiled it from clean source). Earlier full-run verdicts were taken with this
   defect present; the gate run recorded in CHECKPOINT is the first without it.
8. Living F-RENDER-1 still reads "Implemented by INC-10; until then the base presents Smithay's rect"
   and "Buffer transforms other than Normal are unverified"; they are formula text and change only
   with item 1's erratum.

## 6. Seeded controls added (each DETECTED; `tools/verify-test-efficacy`)

Kept reference: reference not kept; bound ignored; pan not scaled by zoom; C_O without δ. Wrapper
(C12.F15.8): (1) wrapped geometry ×2; (2) `dst` altered ×2; (3) damage passed through after a remap;
(4) opaque regions passed through after a remap; (5) crop disabled, 90° and flipped-270° `src`
inconsistent, `src` not cropped; (6) tracker not rebuilt on a k change. History (C12.F16.6):
capacity 255, capacity 257, newest evicted, a refused fit records, a refused restore pops another
output's history, history kept across a disconnect, a reconfiguration treated as a reconnect, the
restored zoom re-quantised, no eviction (unbounded growth), the ring reserved again at every fit.
Fit: the scale not divided out, the upper clamp missing; M10 no true-position event; Logo+Shift+F
also fitting; bindings resolving during an interaction; a fit without its membership pass.
