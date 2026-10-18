# C12.F17 — amendment package for architecture change control (2026-10-18)

Status: **PREPARED FOR CHANGE-CONTROL REVIEW. Nothing is adopted, nothing ratified, no normative
text changed** — FOUNDATION_ARCHITECTURE, FOUNDATION_INVARIANTS, FOUNDATION_STATE_MACHINES and
the C12.F14/F15/F16 layers are untouched by this package. It gives the exact text of a proposed
layer C12.F17 and of the living-registry replacements it would make, so that adoption is a
mechanical application of §3–§6. INC-10 is NOT ratified; OB-24 stays OPEN. Not pushed.

**Base:** local `main` with the M1 resolution (evidence/phase-f/INC-10/M1_RESOLUTION_RECORD_2026-10-18.md),
on `c4a7263` (INC-10 implemented). Every rule below matches the code and tests on that base; where
it does not, that is stated (§7).

## 1. Authority

- **Independent INC-10 exit review** of `c4a7263` (2026-10-18): READY AFTER SPECIFIED GOVERNANCE
  CLARIFICATIONS 1–6; owner decisions applied by that review: (1) the crop requirement is semantic
  equivalence of visible sampling, an algebraic difference from `CropRenderElement` is not a
  defect when the evidence proves it; (2) under half-open bounds `[−2³⁰, 2³⁰)` the width is 2³¹ and
  the last included coordinate 2³⁰ − 1, a defect only if production violates the contract;
  (3) geometry equals the canonical rect exactly, damage may conservatively expand by ≤ 1 px for
  unremapped elements if complete; (4) Logo+F fits the eligible window under the pointer, no
  target is a no-op, Logo+Shift+F restores, both unavailable during an active pan.
- **Owner brief "INC-10 M1 Resolution and C12.F17 Change Control"** (2026-10-18) §2 and §5:
  locked — never absolute motion; confined — exactly one reconciliation event, confinement kept;
  extra Ctrl/Alt never trigger the bindings (exact matching); a presentation must not let an
  operation silently modify a camera that is not presented — **unless that conflicts with ratified
  text, in which case the conflict is documented and a decision requested** (it does: Q1).

## 2. Summary

| # | Clarification | Ratified text it replaces or extends | Code and tests (base above) | Needs |
|---|---|---|---|---|
| K1 | Crop: source sampling of the visible part preserved; renderers' sampling map, not `CropRenderElement` | C12.F15.5 Policy and Not proven; F-RENDER-1 def. 3 | `convert::crop_src`, `sampled_axes`; Pixman pixels 8 transforms, viewport source, buffer scale; GLES algebra | adoption |
| K2 | Crop box: edges clamped to `[−2³⁰, 2³⁰]`, pixels half-open `[−2³⁰, 2³⁰)`, width capped at 2³¹ − 1 | C12.F15.5 Policy "`[−2³⁰, 2³⁰]`"; F-RENDER-1 def. 3 | `CanonicalRect::with_crop_box`; `the_crop_box_is_half_open_with_the_width_capped_at_i32_max` | adoption |
| K3 | Geometry and draw destination exact; pass-through damage ≤ 1 px beyond the rect at the far edges | C12.F15.4 "exactly"; F-RENDER-1 def. 1 | `a_zoom_change_rebuilds_the_damage_tracker`, `pixels_and_pointer_follow_the_canonical_rect_at_a_seam_zoom` | adoption |
| K4 | Pointer after fit/restore: no event under a kept lock; one under a kept confinement or none | C12.F15.9 Pointer; F-FIT-1 def. 4; INV-INPUT-5 (clause added) | `Core::camera_written`; `t1_fit_constraints` | adoption |
| K5 | Bindings and target: exact modifiers; no target → no-op; inert during a pan | INC-10 record §5 item 5 ("Level D defaults, not owner decisions"); M8 (rows added) | `input::keys`, `entry::fit_viewport`; `t1_fit_bindings`, `navigation_bindings_reject_extra_modifiers` | adoption; **Q1** for presentations |
| K6 | F-RENDER-1 living status: implemented; software evidence only | F-RENDER-1 defs. 1 and 3 | INC-10 and M1 records | adoption |

## 3. Proposed layer text (to be inserted after §C12.F16, before the Phase D package)

The block below is the proposed layer verbatim, except its Status line, which change control
writes at adoption.

---

# §C12.F17 — Canonical registry amendment (Phase F: INC-10 exit clarifications — crop sampling and bounds, damage, pointer reconciliation after a camera action, fit/restore bindings)

**Status: ADOPTED <date> by owner decision** (to be completed at adoption). One invariant changes (INV-INPUT-5: one clause on pointer reconciliation after a camera action); two formulas change (F-RENDER-1, F-FIT-1); M8 gains two binding rows; no invariant is added or removed (57). C12.F14, C12.F15 and C12.F16 are unchanged; the C12.F15 text this layer supersedes is listed in C12.F17.1.

**Inputs:** INV-INPUT-5 (C12.E1R); INV-RENDER-1, F-RENDER-1, F-FIT-1, C12.F15.4, C12.F15.5, C12.F15.9 (C12.F15); INV-CAMERA-5, OBJ-30 (C12.F16); M8, M10, M12, M13; F-LOCK-1, F-CONFINE-1; the INC-10 implementation record and its independent exit review (2026-10-18).

**Provenance:** `evidence/phase-f/C12-F17/C12F17_AMENDMENT_PACKAGE_2026-10-18.md`; `evidence/phase-f/INC-10/IMPLEMENTATION_RECORD_2026-10-18.md`, `M1_RESOLUTION_RECORD_2026-10-18.md`.

**Decisions:** K1 the crop preserves the source sampling of the visible part; K2 the crop box is half-open with an i32 width cap; K3 geometry is exact, pass-through damage may exceed it by ≤ 1 px; K4 after a fit or restore a kept lock gets no absolute motion and a kept confinement gets the one true-position event; K5 the fit/restore bindings match exact modifiers, act on the element under the pointer, are a no-op without one and inert during an interaction; K6 F-RENDER-1 states what INC-10 implemented and verified.

**Where an `INV-*` appears below, this wording is canonical and supersedes C12.E1R. Every other row is unchanged.**

| ID | Statement (canonical) | Owner | Class | Evidence | Tests (tier) | Trigger |
|---|---|---|---|---|---|---|
| **INV-INPUT-5** | A constraint is activated only when its element holds ActiveElementFocus, its surface has pointer focus, the pointer is inside the region, and no compositor pointer interaction is being acquired.<br><br>**Reconciliation** runs once at the end of every core batch in which any of these changed: the interaction output's view or descriptor (including `output_reconfigured`); the constrained surface's geometry or origin; the output set; ActiveElementFocus; the end of a compositor interaction; the end of a client-owned grab sequence.<br><br>Reconciliation is **deferred** while a compositor pointer interaction or a client-owned grab sequence is active. While deferred, core and Smithay keep the same constraint state, no absolute motion reaches a locked surface, and relative motion comes only from physical deltas.<br><br>A lock is kept only if its derived lock point is observable through the interaction output and its element holds ActiveElementFocus.<br><br>A confinement clamps the physical pointer half-open, in the interaction output's OutputLogical space, to the region's projection; Arrangement crossing is disabled; a pointer outside the region after a view change ends it; there is no warp.<br><br>Every end emits `unlocked`/`unconfined` exactly once, and **only from the active state**: no deactivation event is ever sent for an inactive constraint — not on a later `leave()` after an explicit deactivate (any lifetime, any later batch), not on surface destruction, and not for a constraint that was never activated. An explicit `deactivate` is used only when the delivered focus does not change in that batch; otherwise Smithay's `leave()` does it (W-SM-4a). Smithay 0.7.0's `leave()` deactivates unconditionally and no public API avoids it (W-SM-4c), so this invariant depends on the one-line dependency patch P1 (`leave()` deactivates only an active constraint) or an equivalent upstream fix, adopted under the patch policy (INV-ARCH-3, OB-11).<br><br>At most one true-position motion per batch; no replayed or synthetic motion. After a camera action — a fit or a restore (F-FIT-1, M8) — on the pointer's output with no compositor interaction owning the pointer, the batch's one true-position event is sent unless the reconciliation leaves a lock active: a kept or newly activated confinement receives it like an unconstrained pointer; a kept lock receives no absolute motion, and its physical pointer is the derived lock point (F-LOCK-1). Output removal relocates the physical pointer at once (F-RELOCATE-1) or, when no output survives, enters `NoOutput` (INV-OUTPUT-1). Destruction ends constraints through the protocol path. | core + adapter | E+M | B11.4, X-8, ER-2, ER-5 R5, EE-2, EE-3; evidence/phase-f/INC-10 (M1) | T1 (OB-3, OB-31, OB-47); T1 `t1_fit_constraints` (INC-10 review M1) | owner amendment (C12.F17, INC-10 exit review M1) |

## C12.F17.1 Text superseded (must not be cited as normative)

| Superseded | Replaced by |
|---|---|
| INV-INPUT-5 (C12.E1R), which did not say how a kept lock or confinement meets the one true-position event of a camera action | INV-INPUT-5 (C12.F17) |
| C12.F15.4 "a fit to zoom ≠ 1 followed by a client repaint at age 1 must damage and repaint **exactly** the surface's canonical rect" — exact for a remapped surface only | C12.F17.5 |
| C12.F15.5 Policy: "cropped in i64 to `[−2³⁰, 2³⁰]` per axis … with the formula of Smithay's `CropRenderElement` (buffer transform included)" | C12.F17.3–.4; F-RENDER-1 (C12.F17) |
| C12.F15.5 "Not proven: the `src` crop was verified only for buffers without a buffer transform" | C12.F17.3 |
| C12.F15.9 Pointer: "exactly one when the pointer is on O and no interaction owns it (`entry.rs` true-position guard), none otherwise" | C12.F17.6 |
| F-RENDER-1 (C12.F15) "Implemented by INC-10; until then the base presents Smithay's rect …" and "Buffer transforms other than Normal are unverified (C12.F15.5)" | F-RENDER-1 (C12.F17) |
| F-FIT-1 (C12.F15) "exactly one when the pointer is on O and no interaction owns it" | F-FIT-1 (C12.F17) |
| INC-10 implementation record §5 item 5: the fit target and the bindings are "Level D defaults, not owner decisions" | C12.F17.7; M8 (C12.F17) |

## C12.F17.2 Formula amendments (living §F carries the corrected text; the previous text is recorded here verbatim)

### F-RENDER-1 (INC-10 status; crop sampling, bounds and damage)
- **Previous (C12.F15):** `L(x) = (x − R_O)·k_O + δ − C_O` (f64) for each surface origin x (and y); the adapter builds `WaylandSurfaceRenderElement::from_surface(.., Point<f64, Physical>(L), ..)` and wraps it in an element whose `geometry()` is the surface's canonical rect — its F-LATTICE-1 rect, the one membership and hit testing use — and whose `draw` passes the destination it is given (that rect, `OutputDamageTracker::render_output`) to the wrapped element unchanged. Smithay's own rect `[round(L), round(fl(dst·k_O) + L))` is not presented; it only decides whether the wrapper remaps damage and opaque regions (C12.F15.3). Implemented by INC-10; until then the base presents Smithay's rect, which equals the canonical rect at every reachable view (zoom 1, k ∈ n/120). [C12.F15]
- **Previous (C12.F15):** A canonical rect is cropped in i64 to `[−2³⁰, 2³⁰]` per axis before Smithay's i32 geometry, with `src` cropped in the same proportion (Smithay's `CropRenderElement` formula, buffer transform included); a visible surface is never rejected. Buffer transforms other than Normal are unverified (C12.F15.5). [C12.F15]
- **Reason:** INC-10 implemented the wrapper; the named `CropRenderElement` formula misdraws the four rotating buffer transforms in Pixman pixels (`smithays_crop_element_misdraws_rotated_buffers`); a box of 2³¹ px does not fit Smithay's i32 size; buffer transforms are now verified in software (C12.F17.3–.5).

### F-FIT-1 (pointer reconciliation under a constraint)
- **Previous (C12.F16):** The clamp belongs to this computation: zoom values from outside Core stay rejected outside the domain where they enter (M4, M20). After the mutation the pointer is reconciled once by M10's view-change rule — at most one true-position event, exactly one when the pointer is on O and no interaction owns it. [C12.F15]
- **Reason:** "exactly one" cannot hold under a kept lock (zwp_pointer_constraints_v1 and INV-INPUT-5 forbid absolute motion to a locked surface); under a kept confinement INC-10 sent none (exit review M1, fixed).

## C12.F17.3 Crop sampling (K1)

**Requirement.** When a canonical rect is cropped (C12.F17.4), the visible part of the element samples its buffer exactly as the uncropped element would at the same destination pixels, for every buffer transform, viewport source (`wp_viewporter` source rectangle) and buffer scale. The cropped `src` is computed in f64, proportionally on the uncropped rect, through the mapping the renderers apply — Smithay 0.7.0 GLES `build_texture_mat` and Pixman `render_texture_from_to` — not through `Rectangle::to_buffer`, which Smithay's `CropRenderElement` uses and which selects the wrong part of the buffer under 90°, 270°, flipped 90° and flipped 270°.

**Evidence** (production `convert::crop_src` / `sampled_axes`, through `OutputDamageTracker`):
- Pixman pixels, cropped draw equal to the uncropped draw inside the box and nothing outside: all eight transforms (`a_cropped_surface_draws_exactly_its_visible_part_for_every_buffer_transform`); viewport source offset, buffer scale 2 and both, × 8 transforms (`a_cropped_surface_keeps_its_sampling_with_a_viewport_source_and_buffer_scale`).
- The transform oracle against Pixman pixels (`the_transform_oracle_matches_the_pixels`) and against the GLES texture matrix, algebra only (`the_transform_oracle_matches_the_gles_texture_matrix`); extreme magnification checked on the `dst`/`src` the tracker hands to `draw` (`rects_beyond_the_i32_range_are_cropped_with_a_proportional_src`).
- Smithay's own `CropRenderElement` misdrawing the rotating transforms (`smithays_crop_element_misdraws_rotated_buffers`).

**Not verified:** cuts at fractional buffer pixels in pixels (Pixman rounds `src` out to integers; such `src` values are checked arithmetically only); any GLES or GPU execution, including f32 texture-matrix precision at extreme magnification (OB-57).

## C12.F17.4 Crop bounds (K2)

Each edge of the i64 canonical rect is clamped to `[−2³⁰, 2³⁰]`; the presented pixels therefore lie in the half-open `[−2³⁰, 2³⁰)`. The width and height are then capped at `i32::MAX = 2³¹ − 1`, the largest Smithay i32 size: the cap shortens only a rect that covers the whole box, which is presented as `[−2³⁰, 2³⁰ − 1)` — last included pixel 2³⁰ − 2, one short of the mathematical box, outside every view (`view_O ≤ 2¹⁵`). A rect wholly outside the box is presented empty. Every visible edge is the canonical one. A cropped rect is remapped (C12.F15.3 item 3): full damage, no opaque region, no direct scanout. Test: `the_crop_box_is_half_open_with_the_width_capped_at_i32_max`; seeded control "the width cap removed".

## C12.F17.5 Geometry versus damage (K3)

The wrapper's `geometry()` and the destination its `draw` passes on are the canonical rect **exactly**, remapped or not. Damage: for a remapped surface, the whole rect; for a pass-through surface, Smithay's own element damage, which `to_physical_precise_up` rounds outwards, so it may extend up to one physical px beyond the rect at its far edges while always covering the changed part of the rect. The C12.F15.4 regression test therefore asserts: repaint damage covers the canonical rect and exceeds it by at most one pixel at the right and bottom edges, and no pixel is left stale. Opaque regions of a remapped surface stay empty and a remapped surface offers no underlying storage (C12.F15.3, unchanged). The tracker is rebuilt whenever `k_O` differs from the k it was built with (`OutputHandle::tracker_k`), which covers fits, restores, presentations and reconfigurations.

## C12.F17.6 Pointer reconciliation after a camera action (K4)

After a fit or a restore on output O, with the pointer on O and no compositor interaction owning it, the batch's constraint reconciliation runs first (INV-INPUT-5), then:
- **no constraint:** exactly one true-position event (`motion` + `frame`) at the pointer's new surface-local position;
- **confinement kept or activated:** exactly that one event; the confinement stays active in Core and Smithay; the physical pointer does not move (no warp);
- **confinement ended** (the pointer is outside its region under the new view): exactly one `unconfined`, then the one event;
- **lock kept:** no absolute motion; the physical pointer is the derived lock point under the new view (F-LOCK-1); relative motion only from later physical deltas;
- **interaction active:** neither binding resolves (C12.F17.7), so nothing is reconciled.

A lock that the reconciliation ends is outside what INC-10 verified: on that path the adapter reports every constraint as persistent (OB-54). Tests: `fit_and_restore_reconcile_the_pointer_under_each_constraint`, `a_fit_that_ends_the_confinement_ends_it_once_then_reconciles_the_pointer`, `no_fit_reconciliation_while_a_pan_owns_the_pointer`; seeded controls "a kept confinement suppresses the event", "motion reaches a kept lock".

## C12.F17.7 Fit/restore bindings and target (K5)

Rows added to M8, before its `FitViewport` rows:

| From | Trigger | To | Effects | Cites |
|---|---|---|---|---|
| — | key F pressed with Logo held, Ctrl and Alt released, Shift released, no compositor interaction active, the pointer on O | — | the binding is consumed (M12); W = the mapped element hit under the pointer on O's current ProjectionFrame (F-HIT-1): `FitViewport(O, W)`; no element hit: nothing — no mutation, no history change, no pointer event | INV-CAMERA-5 |
| — | key F pressed with Logo and Shift held, Ctrl and Alt released, no compositor interaction active, the pointer on O | — | the binding is consumed (M12): `ViewportRestore(O)` | INV-CAMERA-5 |

Any other modifier combination with F, and any F while a compositor interaction is active, is no binding: the key follows M12's client path. Tests: `navigation_bindings_resolve_fit_and_restore`, `navigation_bindings_reject_extra_modifiers` (T0); `fit_and_restore_through_the_keyboard_bindings`, `extra_ctrl_or_alt_modifiers_trigger_neither_binding`, `logo_f_with_no_element_under_the_pointer_is_a_no_op`, `fit_and_restore_are_inert_during_a_pan` (T1). **During a protocol-fullscreen presentation:** as decided under Q1 (§7); until then §C5's row applies unchanged.

## C12.F17.8 F-RENDER-1 status (K6)

F-RENDER-1's definitions state that INC-10 implemented the wrapper, the crop and the tracker rebuild, and what is verified: Pixman pixels (seams, pointer columns, crop for all transforms, viewport sources, buffer scale; the 12-configuration pixel sweep `pixels_follow_the_canonical_rect_in_every_sweep_configuration`), Smithay's test renderer and draw-argument records, and the GLES texture-matrix algebra. **Unverified:** real GLES/GPU rendering, dmabuf import, direct scanout, hardware planes and display pacing (OB-57, OB-58).

## C12.F17.9 Checker change

`tools/check-foundation-docs`: `F_LAYERS` gains `('C12.F17', 'C12.F17 C12.F17. ADOPTED <date> (owner approval; base <commit>)')`. Fixture 47: INV-INPUT-5's camera-action clause drifts ("a kept lock receives no absolute motion" removed) → `inv-wording-mismatch`.

## C12.F17.10 What this layer does not change

INV-RENDER-1's wording; F-RENDER-1's location formula L and its tracker definition; F-LATTICE-1, F-HIT-1, F-MEMBER-1; F-FIT-1's guard, zoom, clamp, centre and history; INV-CAMERA-5; OBJ-30; M10's rows (pan release keeps "≤ 1", OB-56); M13; §C5 (Q1 open); C12.F14–C12.F16's text.

---

## 4. Living-registry replacements at adoption

- **F-RENDER-1, definition 1** →
  > `L(x) = (x − R_O)·k_O + δ − C_O` (f64) for each surface origin x (and y); the adapter builds `WaylandSurfaceRenderElement::from_surface(.., Point<f64, Physical>(L), ..)` and wraps it in an element whose `geometry()` is the surface's canonical rect — its F-LATTICE-1 rect, the one membership and hit testing use — and whose `draw` passes the destination it is given (that rect, `OutputDamageTracker::render_output`) to the wrapped element unchanged. Smithay's own rect `[round(L), round(fl(dst·k_O) + L))` is not presented; it only decides whether the wrapper remaps damage and opaque regions (C12.F15.3). Implemented by INC-10 (`convert::CanonicalRect`): geometry and draw destination equal the canonical rect exactly; damage of a pass-through surface is Smithay's own, which may cover up to one physical px beyond the rect at its far edges (C12.F17.5). [C12.F15; C12.F17]
- **F-RENDER-1, definition 3** →
  > A canonical rect is cropped in i64 before Smithay's i32 geometry: each edge is clamped to `[−2³⁰, 2³⁰]` per axis, so the presented pixels lie in the half-open `[−2³⁰, 2³⁰)`, and the width and height are then capped at `i32::MAX = 2³¹ − 1`, so a rect covering the whole box is presented as `[−2³⁰, 2³⁰ − 1)` (outside every view, `view_O ≤ 2¹⁵`); a rect wholly outside the box is presented empty, and a visible surface is never rejected. `src` is cropped so that the visible part keeps exactly the source sampling of the uncropped element — proportional in f64 on the uncropped rect, through the renderers' sampling map for the buffer transform (Smithay's GLES `build_texture_mat` and Pixman `render_texture_from_to`), not Smithay's `CropRenderElement`, which misdraws the four rotating transforms (C12.F17.3). Verified in Pixman pixels for all eight buffer transforms, viewport sources and buffer scale with cuts on whole buffer pixels, and against the GLES texture-matrix algebra; not verified for cuts at fractional buffer pixels or on a GPU (OB-57). [C12.F15; C12.F17]
- **F-RENDER-1, Amended:** append "; C12.F17 (INC-10 status, crop sampling and bounds, damage)" and "§C12.F17.2" to the recorded-verbatim list.
- **F-FIT-1, definition 4** →
  > The clamp belongs to this computation: zoom values from outside Core stay rejected outside the domain where they enter (M4, M20). After the mutation the pointer is reconciled once by M10's view-change rule — at most one true-position event, exactly one when the pointer is on O, no interaction owns it and the reconciliation leaves no lock active; a kept lock receives no absolute motion (INV-INPUT-5). [C12.F15; C12.F17]
- **F-FIT-1, Amended:** append "; C12.F17 (pointer reconciliation under a constraint)" and "§C12.F17.2".
- **INV-INPUT-5** (FOUNDATION_INVARIANTS): Wording = the C12.F17 row above; Origin "C12.F17 C12.F17. ADOPTED <date> (owner approval; base <commit>)"; Layers "C12 → C12.R1 → C12.E1R → C12.F17"; Trigger "owner amendment (C12.F17, INC-10 exit review M1)"; the invariant count line gains "1 from C12.F17" and "C12.E1R" loses one (still 57).
- **M8:** the two rows of C12.F17.7.
- **FOUNDATION_IMPLEMENTATION_PLAN INC-10:** exit item 4 "exactly one true-position event after a fit with the pointer on O and no interaction (none during one)" → "… and no lock kept by the reconciliation (C12.F17.6)"; item 6 "rotated and flipped buffer transforms" → "… viewport sources and buffer scale (C12.F17.3)".

## 5. Required tests and controls (all present on the base; each control DETECTED)

T1: `t1_fit_constraints` (3), `t1_fit_bindings` (4), `t1_canonical_rect` (`pixels_follow_the_canonical_rect_in_every_sweep_configuration`, `a_cropped_surface_keeps_its_sampling_with_a_viewport_source_and_buffer_scale`, `the_crop_box_is_half_open_with_the_width_capped_at_i32_max`, and the C12.F15.8 tests); T0: `navigation_bindings_reject_extra_modifiers`. Seeded controls: the eight of the M1 resolution record §7.

## 6. Previous texts quoted (for the reviewer; verbatim from the base)

- C12.F15.4: **Regression test (INC-10):** a fit to zoom ≠ 1 followed by a client repaint at age 1 must damage and repaint exactly the surface's canonical rect at the new k; with the rebuild removed it must fail (seeded control, C12.F15.8).
- C12.F15.5: - **Policy:** the canonical rect is cropped in i64 to `[−2³⁰, 2³⁰]` per axis before the i32 conversion, and `src` is cropped with the same proportion, computed in f64 on the uncropped rect with the formula of Smithay's `CropRenderElement` (buffer transform included). A visible surface is never rejected.
- C12.F15.5: - **Not proven:** the `src` crop was verified only for buffers without a buffer transform. Rotated and flipped buffer transforms are an explicit INC-10 implementation and exit requirement (C12.F15.11); if the crop cannot be made consistent for a transform, that case is reported to change control — this layer does not claim it.
- C12.F15.9: - **Pointer.** "Reconcile once by the same rule as other camera writes" is M10's view-change row: one batch-end reconciliation with at most one true-position event — exactly one when the pointer is on O and no interaction owns it (`entry.rs` true-position guard), none otherwise.
- §C5 (Phase C checkpoint 2, accepted): | fullscreen + viewport-fit on the same output | the presentation view takes precedence for rendering. A fit applied meanwhile changes camera A's state, which is shown when the presentation ends. Neither touches the other's state. |

## 7. Open questions for change control

- **Q1 — fit/restore during a protocol-fullscreen presentation (decision required; framing
  corrected 2026-10-18).** What governs it today, by §P0 (FOUNDATION_ARCHITECTURE line 2569,
  "A definition counts only inside a `living:` region of the home document"; line 2563,
  "Everything above this line is historical"):
  - **Living, normative:** M8's guards (FOUNDATION_STATE_MACHINES lines 116 and 118:
    `FitViewport(O, W)` [O connected ∧ W mapped ∧ g.w > 0 ∧ g.h > 0], `ViewportRestore(O)`
    [O connected ∧ O's history non-empty]), F-FIT-1's refusal list (FOUNDATION_ARCHITECTURE §F,
    line 2720: O not connected, W not mapped, `g.w ≤ 0` or `g.h ≤ 0`) and INV-PRES-1
    (FOUNDATION_INVARIANTS line 625: "Fullscreen never touches cameras or anchors"). None names
    a presentation, so a fit or restore during one is permitted **by omission**: the current
    contract is Option A.
  - **Historical, consistent with it:** §C5's "fullscreen + viewport-fit on the same output" row
    (FOUNDATION_ARCHITECTURE line 626, Phase C checkpoint 2, quoted in §6) and the 1R
    contradiction audit's row 6 (`audit/1R/CONTRADICTION_AUDIT_1R.md`). They record the same
    behaviour but are provenance, not the normative home; an earlier version of this question
    called §C5 the "accepted" rule, which overstated its status.
  The implementation follows the living contract, and a T1 test pins it
  (`a_fit_during_a_presentation_changes_the_hidden_camera_as_ratified`): Logo+F hits the
  presented element on the presentation frame and fits camera A to it, the presented frame does
  not change, history +1; a restore likewise changes camera A unseen. The brief asks that a
  presentation never let an operation silently modify a camera that is not presented. Options:
  **(A)** confirm the living contract (documentation only; the test stays); **(B)** fit and
  restore are refused on O while O presents an element — no camera or history change, no pointer
  event, the key consumed (M12) — by amending the living M8 guards (or the M8 binding rows of
  C12.F17.7) and F-FIT-1's refusal list; §C5's row is then listed in C12.F17.1 as superseded
  provenance. (B) is a two-line guard in `entry::fit_viewport` / `viewport_restore`; its test is
  the present one inverted, and the seeded control "a fit refused during a presentation" (now
  DETECTED as a divergence from the living contract) becomes the regression to keep. Either way:
  does the rule also cover shell-issued fits (INC-11)? Exact decision texts and edits: §8.
  Evidence for a client that resizes to the fullscreen size before the fit: §8.1.
- **Q2 — lock rule beyond camera actions.** INV-INPUT-5 states "no absolute motion reaches a
  locked surface" only while reconciliation is deferred; C12.F17 adds camera actions. The protocol
  forbids `wl_pointer.motion` whenever a lock is active, and `output_reconfigured` currently sends
  one (OB-55). Generalise the clause (then OB-55 is a violation to fix before adoption), or keep
  it scoped as proposed?
- **Q3 — pan release under a kept confinement** sends no true-position event (M10 "≤ 1"); require
  exactly one, as for camera actions (OB-56)?
- **Q4 — F during a pan** is no binding and is forwarded to the focused client (M12), as decided
  by "unavailable during an active pan". Confirm forwarding rather than consuming.
- **Q5 — GPU tolerance.** The pass criterion for fractional crop cuts on a GPU (OB-57) needs a
  tolerance only change control can set.

## 8. Decision texts and ready-to-apply edits (prepared 2026-10-18; nothing here is adopted)

Each entry is the text an owner decision would record, followed by the package edits it implies.
Line numbers refer to this file as revised on branch `prep/inc10-exit-readiness-2026-10-18`.
Every decision is the owner's; this section prepares them and decides none.

### 8.1 Q1 — fit/restore during a protocol-fullscreen presentation

> **Q1 (C12.F17) — owner decision, dated ____.**
> ☐ **A — living contract confirmed.** While output O presents a protocol-fullscreen element
> (INV-PRES-1; `Core::presentation(O)` is some element), `FitViewport(O, W)` and
> `ViewportRestore(O)` keep M8's guards unchanged and change O's camera and navigation history
> (OBJ-30) as at any other time; the presentation keeps rendering precedence, and O shows the
> changed camera when the presentation ends. §C5's historical row stays as provenance.
> ☐ **B — refused during a presentation.** While O presents a protocol-fullscreen element (from
> the presentation's start to its exit decision, INV-PRES-1; a grant still waiting for its
> element's map is not a presentation) the operations in scope are refused: no camera mutation;
> no history entry recorded, evicted or removed; no lattice, frame, membership or epoch change; no
> pointer reconciliation or pointer event.
> **Scope:** ☐ 1. the input bindings only — Logo+F and Logo+Shift+F still resolve and are consumed
> (M12, the F key is not forwarded); shell-issued fit/restore keep the living M8 rule until INC-11
> amends it. ☐ 2. the bindings and the shell-issued `FitViewport` (and a shell restore, if INC-11
> has one) — under B refused with the same state effects; how a refusal is reported to the shell
> is INC-11's ShellPort contract; no keyboard event is involved.

**Edits under A:** C12.F17.7, the sentence "**During a protocol-fullscreen presentation:** as
decided under Q1 (§7); until then §C5's row applies unchanged." becomes "**During a
protocol-fullscreen presentation** (Q1, owner decision <date>): the rows above and M8's guards
apply unchanged; W is the element hit under the pointer on the presentation frame; fit and restore
change O's camera and history, the presentation keeps rendering precedence, and the changed camera
is shown when it ends. Test: `a_fit_during_a_presentation_changes_the_hidden_camera_as_ratified`
(T1)." (with scope 2: "The same holds for shell-issued fit and restore (INC-11)."). C12.F17.10:
"§C5 (Q1 open)" becomes "the presentation rule of M8 and F-FIT-1 (confirmed by Q1, <date>; §C5's
historical row unchanged)". §2 K5 "Needs": "adoption; **Q1** for presentations" becomes
"adoption". §7 Q1: append the decision. No code, test, control or checker change.

**Edits under B:** C12.F17.1 gains "§C5 'fullscreen + viewport-fit on the same output' (historical
provenance) — for the input bindings [scope 1] / entirely [scope 2] | C12.F17.7; M8 (C12.F17)",
and under scope 2 also "F-FIT-1 definition 1 (C12.F15) refusal list | F-FIT-1 (C12.F17)" and
"C12.F16.3 items 3–4 refusal lists | F-FIT-1, M8 (C12.F17)". C12.F17.2 (scope 2) gains an F-FIT-1
block: previous (C12.F15), verbatim "if O is not connected, W is not mapped, or `g.w ≤ 0` or
`g.h ≤ 0`, the action is refused — no camera mutation, nothing recorded."; new "if O is not
connected, O presents a protocol-fullscreen element (INV-PRES-1), W is not mapped, or `g.w ≤ 0`
or `g.h ≤ 0`, the action is refused — no camera mutation, nothing recorded or evicted, no pointer
reconciliation; a restore is refused likewise while O presents". C12.F17.7: scope 1 adds to both
binding rows' trigger "∧ O presents no protocol-fullscreen element" and a third row "F with Logo
(± Shift), Ctrl and Alt released, no interaction, O presenting → the binding is consumed (M12);
nothing else"; scope 2 leaves the rows and points to the amended M8 guards. The living M8 rows
(§4) gain the presentation condition on `FitViewport` and `ViewportRestore` (scope 2) or on the
binding rows (scope 1). C12.F17.9 gains fixture 48: F-FIT-1 definition 1 reverted to its C12.F15
text → `formula-provenance` (scope 2; under scope 1 the T1 test and its control carry the rule).
C12.F17.10: remove "§C5 (Q1 open)", add "INV-PRES-1 and ADR-09 unchanged (the refusal is a
camera-side guard); 1R contradiction audit row 6 stays historical evidence", and under scope 2
replace "F-FIT-1's guard, zoom, clamp" by "F-FIT-1's zoom, clamp". Header Status: "M8 gains two
binding rows, which refuse while O presents …" (scope 1) or "… two formulas change (F-RENDER-1,
F-FIT-1, whose refusal list gains a presentation); M8 gains two binding rows and its fit and
restore guards gain a presentation condition" (scope 2). INV-CAMERA-5 ("refused … when the
history is empty") is not exhaustive; amending it for clarity is a reviewer choice that changes
the invariant count stated in the header. Implementation (separate commits): the guard at the
`KeyAction::FitViewport` / `ViewportRestore` arms (scope 1) or at the top of `fit_viewport` /
`viewport_restore` (scope 2; identical today, `ShellCommand` has no variants); the T1 test
inverted; the control re-seeded as "presentation guard removed"; full gates.

**Evidence (collected 2026-10-18, branch `prep/inc10-exit-readiness-2026-10-18`):**
`t1_fit_bindings::a_fit_during_the_presentation_of_a_resized_client_frames_its_fullscreen_extent`.
A client that obeys the fullscreen configure (1920 × 1080) before Logo+F is fitted at that
geometry: zoom 1, centred on the fullscreen extent (anchor + (960, 540)); after
`unset_fullscreen` it redraws at its saved 800 × 600, nothing re-fits (INV-CAMERA-5), and camera A
shows the window at the view's top-left (canonical rect (0, 0, 800, 600)) instead of framing it;
Logo+Shift+F returns to the camera before the fit. This is the behaviour the living contract
(option A) produces; under B the fit would be refused and the camera unchanged. The test shows the
picture the owner is deciding about; it decides nothing. (The earlier test's client kept its
800 × 600 buffer and was fitted at zoom 1.8.)

### 8.2 Q2 — the lock rule beyond camera actions (OB-55)

> **Q2 — owner decision, dated ____.** ☐ **Widen:** INV-INPUT-5 states that no absolute motion
> reaches a surface whose lock is active, on every path. ☐ **Keep scoped:** C12.F17's clause
> covers camera actions only; the other paths follow the protocol without an invariant clause.

The protocol forbids `wl_pointer.motion` while a lock is active either way (zwp_pointer_constraints_v1
`lock_pointer`). Prepared, not merged: `prep/ob55-q2-gated-2026-10-18` (`output_reconfigured`
reconciles first and sends no true-position event to a kept lock; T1 `t1_reconfigure_lock` fails
on `619187f` with "motion 310,340"; seeded control). **If widened:** the C12.F17 INV-INPUT-5 row's
camera-action sentence becomes "No absolute motion reaches a surface whose lock is active: after a
camera action — a fit or a restore (F-FIT-1, M8) — or an output reconfiguration on the pointer's
output, with no compositor interaction owning the pointer, the batch's one true-position event is
sent unless the reconciliation leaves a lock active; …", fixture 47 is re-anchored on it, and
OB-55's fix is merged before adoption. **If kept scoped:** the fix is still recommended as a
protocol-compliance fix under OB-55, outside C12.F17.

### 8.3 Q3 — pan release (and output relocation) under a kept confinement (OB-56)

> **Q3 — owner decision, dated ____.** ☐ **Exactly one:** the end of an interaction and an output
> relocation send the one true-position event to a kept confinement, as a fit or restore does
> (C12.F17 K4). ☐ **Keep "≤ 1":** M10's rows stay as they are.

`end_interaction` (fix branch) and `output_disconnected` both send the event only when no
constraint remains (`if self.constraint.is_none()`), so a kept confinement gets none. Under
"exactly one" both become `!locked` as in `camera_written`, M10's "interaction ends" row gains
"exactly one unless a lock is kept", and T1 tests are needed (pan release under a kept confinement;
disconnect relocating onto a surface whose confinement is kept). OB-56's second part (a persistent
lock re-activating in the batch whose lock point is unobservable) stays a separate decision.

### 8.4 Q4 — F during a pan

> **Q4 — owner decision, dated ____.** ☐ **Forward** (as implemented: no binding during an
> interaction; F follows M12's client path). ☐ **Consume** (a change to `resolve_key_binding` and
> its tests).

### 8.5 Q5 — GPU tolerance for fractional crop cuts (OB-57)

> **Q5 — owner decision, dated ____.** ☐ **Set now:** the pass criterion for fractional-pixel crop
> cuts on a GPU is ____ (e.g. "≤ 1 physical px of sampling offset at any cut, no pixel outside the
> canonical rect"). ☐ **Defer** to OB-57's T2 work; no GPU claim is made for zoom ≠ 1 until then.

Q5 does not block the software-only INC-10 exit (OB-57 "Blocks: any GPU-rendering claim").
