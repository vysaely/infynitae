# INC-10 — exit readiness: OB-14 and OB-53 evidence, C12.F17 decision texts (2026-10-18)

Status: **prepared on branch `prep/inc10-exit-readiness-2026-10-18` (base `619187f`), not on
`main`, not pushed. INC-10 is NOT ratified; OB-14, OB-24 and OB-53 stay OPEN; C12.F17 stays
PROPOSED.** Labels: [F] verified here, [I] interpretation, [O] owner or change-control decision.

## 1. Why this record exists

INC-10's plan lists "OB-14 (zoom ≠ 1 part)", "OB-53 (its part)" and the exit tests "TST-18 d5hit
zoomed configurations; TST-23 R1 and TST-19 as T0 properties". The INC-10 implementation record
and the M1 resolution record cite none of OB-14, OB-53, TST-18, TST-19 or TST-23 [F: no match in
either file]. OB-14 "Blocks: INC-10 exit (zoom ≠ 1 part)"; OB-53 "Blocks: the exits of INC-10
(fit loop, CameraZoom), …". This record maps the evidence to their criteria and adds what was
missing.

## 2. OB-14 (zoom ≠ 1 part)

OB-14 (FOUNDATION_TEST_PLAN): "Hit-test remaining coverage. Test/decision: transforms, zoom ≠ 1,
sub-pixel clamp, multi-output spanning, random trees. Pass criterion: 0 mismatches vs presented;
clamp ≤ ½ px/k. May be closed by: T0 properties + T1 production hit tests". Its History moved the
production transform part to OB-45 (CLOSED at INC-3).

| Criterion | Evidence at zoom ≠ 1 | Status |
|---|---|---|
| 0 mismatches vs presented | `t1_canonical_rect::render_and_hit_agree_through_the_wrapper_at_rounding_points` (139,392 rects, k swept by ulps around rounding points, kept references at ±2²⁰ / ±2³¹, both axes); `pixels_and_pointer_follow_the_canonical_rect_at_a_seam_zoom` (pointer columns); `pixels_follow_the_canonical_rect_in_every_sweep_configuration` (153 frames); **new** `t1_d5hit_zoomed::tst18_d5hit_zoomed_configurations` (the D5 tree with input-region hole, popups and nested subsurfaces; six zooms off the 1/120 lattice incl. ED-1's k = 1080/651; > 1,000,000 points; 0 render, 0 hit mismatches) | [F] met |
| sub-pixel clamp ≤ ½ px/k | **new**, same test: every pixel also sampled just inside its far corner; delivered = q − origin clamped; overrun ≤ ½/k + 3·2⁻¹⁶ Canvas px; the test fails unless the clamp is exercised | [F] met |
| random trees | the sweeps use random cameras (zoom 0.3 … 3.8, fractional centres) over fixed scenes; no random surface trees at zoom ≠ 1 | [F] partial — random trees are covered at zoom 1 only (INC-1 T0) |
| transforms | OB-45 (production, zoom 1, CLOSED); `t1_canonical_rect` crops for all 8 *buffer* transforms; no rotated *output* at zoom ≠ 1 | [F] gap; rotated outputs are OB-46 (T4, release) |
| multi-output spanning | no test of an element seen through two zoomed cameras | [F] gap |

**[O] needed:** whether "random trees", "transforms" and "multi-output spanning" at zoom ≠ 1 are
required for INC-10's exit, or recorded as OB-14's remaining scope (OB-14 stays OPEN either way).
[I] each is a T1 test of moderate size on the existing harness (`t1_d5hit_zoomed` takes a view and
an anchor; a second output and a rotated descriptor are parameters of `Lockstep`).

**TST-18:** its production parity "INC-10 (zoomed configurations)" now exists. **TST-19 / TST-23**
(d5_model R1–R5 "as T0 properties"): not mapped here [U]; their production counterparts appear to
be the T0 lattice tests in `entry.rs` (`the_lattice_reference_follows_f_lattice_1_transitions`,
`a_pan_beyond_the_reference_bound_sets_it_again_on_that_axis`) and the INC-6 epoch tests; a
reviewer should confirm the mapping.

## 3. OB-53 (INC-10 part)

OB-53: "Client resizing in response to scale hints … the viewport-fit feedback loop of EC-22
(INV-CAMERA-5) and discrete CameraZoom epochs (INC-10) … Pass criterion: with an adversarial T1
client: hint changes ≤ (rungs crossed + 1 + one P7 per non-client stimulus) per epoch; the run
ends quiescent; no camera mutation from client geometry (INV-CAMERA-1); re-fit rate ≤ commit rate
(EC-22)".

**Fit loop — new T1** `t1_fit_scale_adversary` [F]: D_max 2000, W with fractional scale and a
viewport, Logo+F (zoom 1.8). The hint rises once at the fit (one-step epoch, 240/120); the
adversary answers every hint (1300 px wide while 1300·h > D_max would hold, else 800 px); P7
lowers the hint exactly once (240 → 184), SC-E keeps it there, the last 20 of 30 rounds change
nothing, every 1300-px buffer fits D_max, the camera is exactly the fitted value and the history
keeps its single entry: 0 re-fits in 30 commits. Changes after the fit: 1 = one P7; the bound
allows rungs crossed (0) + 1 + one P7. Seeded control (SC-E reversal allowed): 30 changes,
240 ↔ 184 — DETECTED.

**CameraZoom — analysis** [F]: `InteractionKind::CameraZoom` occurs in production code only in
its declaration (`scale.rs`); every use is inside `#[cfg(test)]` (`scale.rs` after line 796,
`visibility.rs` after line 253). No input or shell path produces a zoom step: Foundation 01's
zoom producer is the fit (`audit/D2_STATE_MACHINES.md:138`, "Continuous and discrete zoom
bindings are specified in C14.4 but are not Foundation features (B1.3-5)"), and a fit is an
atomic one-step epoch (M7 "atomic camera or Stage action | Closed | evaluate once and settle").
There is therefore no discrete CameraZoom epoch for a T1 test to drive in INC-10.

[O] options for the CameraZoom part of OB-53 (change control; OB-53 stays OPEN):
1. Re-scope it to the increment that introduces zoom bindings or shell zoom (INC-11's semantic
   `Navigate`, or a later zoom-binding increment), by amending OB-53's "Blocks" text.
2. Accept the fit's one-step epoch as INC-10's zoom stimulus, evidenced by
   `t1_fit_scale_adversary`, and record that discrete CameraZoom epochs have no producer.
3. Test discrete CameraZoom epochs through Core's test-only `start_scale_interaction` path (T0 /
   internal harness) — this exercises the epoch machinery but not a production path, so it would
   not meet "T1 production tests".

[I] Option 1 matches the plan's own wording best ("each for its stimuli"): INC-10 introduces no
discrete zoom stimulus.

## 4. C12.F17

The package (`evidence/phase-f/C12-F17/C12F17_AMENDMENT_PACKAGE_2026-10-18.md`) on this branch:
Q1's framing corrected (§7: the living M8 / F-FIT-1 / INV-PRES-1 permit fit and restore during a
presentation by omission; §C5 line 626 is historical per §P0); §8 gives copy-ready decision texts
for Q1–Q5 and the exact edits each option implies; §8.1 records the resizing-client evidence
(`a_fit_during_the_presentation_of_a_resized_client_frames_its_fullscreen_extent`). Still
PREPARED, not adopted.

## 5. Related defect work (branch `fix/defect-verification-2026-10-18`, not merged)

Q-B1 (Escape cancel did not end the pan's epoch or reconcile), OB-54 workaround (a oneshot
constraint Smithay ended stays ended in Core), Q-P1 (no allocation per client click); and, gated
on Q2, OB-55 (`prep/ob55-q2-gated-2026-10-18`). See the session report.

## 6. What remains before change control can accept INC-10's exit evidence

1. [O] Q1–Q5 decided or deferred; C12.F17 adopted (with the edits of the chosen options).
2. [O] OB-14's remaining scope (§2) and OB-53's CameraZoom part (§3) dispositioned.
3. Merge decisions for this branch and the fix branch; full gates on the merged tree.
4. Independent review of this branch, the fix branch and the package.
5. Change-control acceptance; then, separately, INC-10 ratification, OB-24 closure, push.
