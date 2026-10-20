# INC-10 — change-control record: exit criteria and blocking obligations mapped to evidence (2026-10-20)

Status: **PREPARED for architecture change control; not accepted. INC-10 is NOT ratified; OB-24
and every other OB stay OPEN.** Branch `prep/inc10-change-control-2026-10-20` (from local `main`
`56b3f12`); not `main`, not pushed (`origin/main` `23752f1`, re-fetched 2026-10-20).
Labels: [F] verified here (path, test, commit, log) · [I] interpretation · [O] owner or
change-control decision · [U] unverified.

## 1. Authority and base

Owner authorization 2026-10-20 (session brief items 1–3): run the full gates on local `main`
`56b3f12`; map every INC-10 exit criterion and every obligation blocking it to its evidence on
`56b3f12`; list anything unmet; a fresh-context adversarial review of this record against the
living foundation text, and fix what it finds. Not authorized: ratifying INC-10, closing OB-24 or
any OB, pushing, touching `main`.

Base [F]: `main` = `56b3f123acaef9a066b8a78810d295df5a66b3b0` (C12.F17 ADOPTED 2026-10-20, owner
fast-forward over `16f13c9`), tree `5f800659f580bfa002524a20ec2ad26c82c11d1f`. Every test name
below was checked to exist (`git grep "fn <name>"`). This branch adds, on top of `56b3f12`:
`b594900` (test hygiene: a kill-and-reap guard in one `t1_tiny_compositor` test; CHECKPOINT note),
`632873c` (tests and controls the review asked for, §7; no production code), and this record. No
production code changes after `56b3f12`.

## 2. Gates

| Tree | Log | Result |
|---|---|---|
| `56b3f12` (worktree `main-gate-56b3f12`, own `CARGO_TARGET_DIR`) | `~/.cache/infynitae-integration/session-2026-10-20b/gates-main-56b3f12/` | ⟨G56⟩ |
| this branch's tip (`b594900` + `632873c` + record) | `…/session-2026-10-20b/gates-cc-tip/` | ⟨GTIP⟩ |

Exit item 10 ("all repository gates on the exact final implementation commit") is met by the first
row for the implementation and by the second for the commit a ratification would name [I: the tip
adds tests, controls and documentation only].

## 3. Entry conditions (FOUNDATION_IMPLEMENTATION_PLAN INC-10)

| Condition | Evidence | State |
|---|---|---|
| INC-3 and INC-6 exits | CHECKPOINT: INC-0 … INC-7 RATIFIED on local `main`; INC-6 ratified `ca2e468` | [F] met |
| C12.F14 on `main` (G-OB24) | `270128f` is an ancestor of `56b3f12`; CHECKPOINT "LAYER C12.F14 … INTEGRATED" | [F] met |
| C12.F15 adopted and committed | `da538d6` ancestor; `F_LAYERS` | [F] met |
| OBJ-30 depth decided (C12.F16, 256) | `e75ea72` ancestor; `F_LAYERS` | [F] met |
| C12.F17 (exit clarifications; not an entry condition, needed by items 4 and 6) | `56b3f12`; CHECKPOINT "LAYER C12.F17 ADOPTED 2026-10-20" | [F] adopted |

## 4. Exit tests and exit evidence items 1–11

Test files are under `crates/infynitae-backend-headless/tests/` unless a Core path is given.
"(new)" marks tests added on this branch (`632873c`) after the review.

| # | Criterion (plan text, abbreviated) | Evidence | State |
|---|---|---|---|
| — | TST-18 d5hit zoomed configurations | `t1_d5hit_zoomed::tst18_d5hit_zoomed_configurations` (D5 tree, input-region hole, popups, nested subsurfaces; six zooms off 1/120 incl. ED-1's 1080/651; > 10⁶ points; 0 render, 0 hit mismatches; clamp exercised); controls "TST-18 zoomed / OB-14: F-UNPROJECT-1 drops the zoom on x", "TST-18 zoomed / OB-14 clamp: delivered coordinates not clamped below the surface's far edge". TST-18's negative control is "—" | [F] met |
| — | TST-23 R1 as a T0 property (pan algebra; negative control: the sealed pan formula) | (new) T0 `entry::tests::tst23_r1_content_follows_the_hand_at_every_zoom_and_scale`: the model's grid (scale {1, 1.5} × zoom {0.5, 0.75, 1, 1080/651, 2.5}, centre (5000.3, 3000.7), 200 events of (+3.7, −1.3) physical px) through the production pan entries and `Core::view`'s edges: content within 1 px of the hand after every event, both axes; control "TST-23 R1 (T0 grid): the sealed pan formula" (checked by hand: 370 px off at zoom 0.5). Also T1 `t1_input::zoomed_pan_moves_by_f_pan_1_and_keeps_widths` (zoom 0.73, scale 1) with "INC-10 F-LATTICE-1: a pan acquired at zoom != 1 does not scale by the zoom". The INC-3 controls on pan sign and output scale run on zoom-1 tests and are not R1 ports | [F] met |
| — | TST-19 (d5_model F1–F8) and its negative controls (sealed pan formula; canvas-exact rule) | **F1** (reference switch): T0 `projection::a_kept_lattice_reference_keeps_widths_and_gaps_during_a_zoomed_pan`, `entry::the_lattice_reference_follows_f_lattice_1_transitions`, `a_pan_beyond_the_reference_bound_sets_it_again_on_that_axis`; controls "INC-10 F-LATTICE-1: the output's lattice reference is not kept", "… the kept-reference bound is ignored", "F-LATTICE-1: lattice reference truncates instead of flooring". **F5** (range): `navigation::tests::fit_zoom_clamps_at_the_upper_bound`, `the_fit_centre_is_exact_and_clamped_into_the_canvas`, and its i32 part by item 6. **F6** (Q16 clamp): `projection::grid_value_precision_at_the_conversion_limit`, the clamp in `t1_d5hit_zoomed`. **Canvas-exact rule** at zoom ≠ 1: (new) in `tst18_d5hit_zoomed_configurations`, the superseded rule must disagree with the presented selection at every zoom (it does); the INC-1 control `render::tests::physical_selection_differs_from_the_canvas_exact_rule_at_fractional_scale` is zoom 1 only. **Sealed pan formula**: the TST-23 R1 row. **F2** (wheel-zoom SC-D churn): a discrete CameraZoom stimulus; none exists in Foundation 01 and its epochs moved with OB-53's CameraZoom part (owner decision 2026-10-19). **F3** (device bound): INC-6 `t1_device_limit` (5 tests), `scale::tests::h1_device_bound_function_is_exact_and_total`, `e1r_n3_negative_control_sealed_ceil_exceeds_device_bound`. **F8** (initial hint): INC-6/INC-7 `t1_scale::initial_scale_hint_emitted_before_first_configure`, `protocol::tests::a_pre_map_grant_initial_hint_fits_the_device_limit`. **F4** (ladder change on hotplug; a held hint off the new ladder): no named production test found [U]. **F7** (popup constraint target, R8): `protocol::tests::popup_target_is_the_viewport_inward_rounded` (INC-1, zoom 1); zoom ≠ 1 untested | [F] F1, F5, F6, both negative controls; [I] F2 moved with OB-53; [F] F3, F8 at INC-6; **[O] F4 unmapped, F7 at zoom 1 only** (U-2) |
| — | 200-commit "client resize never moves the camera", production path | `t1_navigation::client_resizes_never_move_a_fitted_camera` (added at INC-10: it did not exist before, IMPLEMENTATION_RECORD §3); no seeded control (the plan names none) | [F] met |
| 1 | 0 width/gap changes per zoomed pan with the kept reference | T1 `t1_input::zoomed_pan_moves_by_f_pan_1_and_keeps_widths` (k = 0.73, 240 steps across > 5 lattice cells; asserts per-surface widths on Core's render list; control "INC-10 F-LATTICE-1: the output's lattice reference is not kept (set from every view origin)": a width changes at step 63); `t1_zoomed_lifecycle::lifecycle_while_zoomed_and_panning_keeps_render_hit_and_membership_consistent` (sizes); gaps: T0 `projection::a_kept_lattice_reference_keeps_widths_and_gaps_during_a_zoomed_pan` | [F] met [I: gaps are asserted at T0 only; with one reference and integral Canvas edges they follow from the widths by construction, C12.F14.4 item 3] |
| 2 | Render/hit agreement through the wrapper (ulp sweeps, ±2²⁰/±2³¹ references, both axes, Pixman seams, pointer columns, ProbeRenderer beyond Pixman) | `t1_canonical_rect::render_and_hit_agree_through_the_wrapper_at_rounding_points` (12 configurations; figures 139,392 rects, 0 presented ≠ Core, 0 seams, 0 wrong pointer pixels of 627,264 from `render_hit_sweep.out`, committed at `c4a7263`; the sweep code is unchanged since and asserts the zeros in every gate run), `pixels_and_pointer_follow_the_canonical_rect_at_a_seam_zoom`, `pixels_follow_the_canonical_rect_in_every_sweep_configuration`, `the_wrapper_removes_the_unwrapped_mismatch`, `the_tracker_draws_the_wrapped_element_at_the_canonical_rect` and `rects_beyond_the_i32_range_are_cropped_with_a_proportional_src` (draw-argument records beyond Pixman's range); controls C12.F15.8 (1), (2) ×2, review L4 | [F] met (software renderers only); 7 of the 8 side × axis × magnitude combinations: no y-axis configuration with a kept 2²⁰ reference below R (`t1_canonical_rect.rs:380–430`) |
| 3 | Rounding-step test; Q16 at \|v − R\| = 2³⁷ + 119, k = 2⁻²² | `projection::kept_reference_rounding_steps_at_the_visible_edge_limit`, `projection::grid_value_precision_at_the_conversion_limit`; controls "C_O without δ", "C12.F14: δ dropped from the physical edge", "δ below one rounding step" | [F] met |
| 4 | F-FIT-1 (C12.F15, C12.F17): clamps, 1 px W at 0.25, scale-2 exact fit, degenerate/unmapped W, centre ±2³⁶, `View::new`, exact restore, exactly one true-position event unless a lock is kept, none during an interaction, refused while O presents | `navigation::tests::{fit_zoom_clamps_at_the_lower_bound, fit_zoom_clamps_at_the_upper_bound, a_window_that_fits_the_view_at_scale_2_fits_at_zoom_1, a_degenerate_window_has_no_fit, the_fit_centre_is_exact_and_clamped_into_the_canvas}`; Core `lib.rs` `a_fit_records_the_prior_value_and_restore_returns_it_exactly`, `restore_is_exact_for_a_zoom_off_the_lattice`, `refused_fits_change_neither_camera_nor_history`, (new) `refused_fits_of_an_unmapped_or_zero_height_window_and_without_an_output`, `no_fit_or_restore_during_an_interaction`; T1 `t1_navigation::fit_and_restore_through_the_keyboard_bindings`, `t1_fit_constraints::{fit_and_restore_reconcile_the_pointer_under_each_constraint, a_fit_that_ends_the_confinement_ends_it_once_then_reconciles_the_pointer, no_fit_reconciliation_while_a_pan_owns_the_pointer}`, `t1_fit_bindings::{fit_and_restore_are_refused_while_the_output_presents, a_fit_during_the_presentation_of_a_resized_client_is_refused, fit_and_restore_are_inert_during_a_pan}`; controls "F-FIT-1: the zoom is not divided by the output scale", "… not clamped at the upper bound", "INC-10 M10: no true-position event after a fit" (detected by `fit_and_restore_through_the_keyboard_bindings`), "review M1" ×2, "C12.F17 Q1 B: presentation guard removed" (fit, restore), "C12.F17 Q4: F during a pan is consumed" | [F] met |
| 5 | Damage invalidation on a zoom change | `t1_navigation::a_zoom_change_rebuilds_the_damage_tracker`; control C12.F15.8 (6); "exactly" read per C12.F17.5 (K3: ≤ 1 px far-edge over-damage for pass-through surfaces) | [F] met |
| 6 | i32 crop beyond the range; rotated/flipped buffer transforms, viewport sources, buffer scale (C12.F17.3); a transform the crop cannot serve reported | `t1_canonical_rect::{a_cropped_surface_draws_exactly_its_visible_part_for_every_buffer_transform, a_cropped_surface_keeps_its_sampling_with_a_viewport_source_and_buffer_scale, rects_beyond_the_i32_range_are_cropped_with_a_proportional_src, the_transform_oracle_matches_the_pixels, the_transform_oracle_matches_the_gles_texture_matrix, smithays_crop_element_misdraws_rotated_buffers, the_crop_box_is_half_open_with_the_width_capped_at_i32_max}`; controls C12.F15.8 (5) ×4, review E1, E3. Reported transform: Smithay's `CropRenderElement` formula under the four rotating transforms — decided by C12.F17.3 (K1) | [F] met for whole-pixel cuts on Pixman; fractional cuts and GPU: OB-57 (does not block) |
| 7 | Map, unmap, destroy during a zoomed pan | `t1_zoomed_lifecycle::lifecycle_while_zoomed_and_panning_keeps_render_hit_and_membership_consistent`; no seeded control (the plan names none) | [F] met |
| 8 | Performance at zoom ≠ 1 vs zoom 1 (p50, p99, max) and INV-ALLOC-1, measured | INV-ALLOC-1: `t0_hot_path_allocations::zoomed_hot_paths_and_fit_restore_allocate_nothing` (asserts 0; every gate run). Frame times: `perf_inc10` re-run on `56b3f12` (release, two runs, `perf_frame_times_56b3f12_run{1,2}.out` in this directory): ⟨PERF⟩; earlier runs on `c4a7263`: `perf_frame_times_run{1,2}.out` | [F] measured, software renderers only, unpaced, no CPU isolation (OB-58, does not block) |
| 9 | Seeded controls of C12.F15.8 each DETECTED; TST-04's control redesigned | §2 efficacy runs; TST-04 counterpart "INC-10 C12.F15.8 (1): the wrapper presents the wrapped element's geometry (TST-04 counterpart)", the old TST-04 controls kept | [F] met (⟨G56⟩) |
| 10 | All gates on the exact final implementation commit | §2 | [F] ⟨G56⟩ |
| 11 | Bounded history (C12.F16.6 items 1–11) and its controls | 1 `restore_with_an_empty_history_is_refused`; 2 `a_fit_records_the_prior_value_and_restore_returns_it_exactly`; 3 (new) `restores_unwind_fits_of_two_elements_at_different_zooms` (two elements, zooms 16 and 9.6, distinct values) and `restores_unwind_consecutive_fits_newest_first`; 4 `the_history_holds_exactly_256_entries`; 5 `the_257th_fit_evicts_only_the_oldest_entry`; 6 `refused_fits_change_neither_camera_nor_history` (no element, zero width, at 0/1/256) and (new) `refused_fits_of_an_unmapped_or_zero_height_window_and_without_an_output` (unmapped, zero height, no output, each at 0/1/256); 7 `restore_with_an_empty_history_is_refused` and (new) `a_restore_after_the_output_is_disconnected_changes_nothing`; 8 `two_outputs_keep_separate_histories`; 9 `disconnect_empties_the_history_and_reconfiguration_keeps_it`; 10 `a_hundred_thousand_fits_keep_the_history_bounded` (now alternating two elements, asserting the alternation) and `zoomed_hot_paths_and_fit_restore_allocate_nothing` (eviction allocates nothing); 11 `a_fit_records_the_prior_value_and_restore_returns_it_exactly` (`View::new`, one event, anchor and size kept), `navigation::tests::the_ring_keeps_the_newest_256_values`. Controls: capacity 255/257, newest evicted, a refused fit records, a refused restore pops another output's history, history kept across a disconnect, reconfiguration as reconnect, zoom re-quantised, no eviction, ring reserved again | [F] met, with two readings [I]: item 7's "a restore … to a disconnected `OutputId`" has no entry before INC-11 (`ShellCommand` has no variant; the binding restores on the pointer's output), so the test is its reachable form; the control "zoom … clamped on restore" is not seeded — the only zoom clamp is the domain `[2⁻²⁰, 2¹⁶]` and every history value lies in it, so a clamp on restore is the identity and any narrower clamp would be an arbitrary seed |

**Negative controls named by the plan** [F]: the sealed pan formula (TST-23 R1 row); the sticky-fit
probe (`tools/verify-architecture` probe `probe_sticky_fit`, which enables Core's
`probe_sticky_fit_retains_authority`; E0277; `arch` gate); C12.F15.8 and C12.F16.6 (items 9, 11).

**Stop / rollback conditions** [F]: no camera change from client geometry
(`client_resizes_never_move_a_fitted_camera`, `t1_fit_scale_adversary`); no presented rect
differing from Core's canonical rect (item 2).

**Invariants exercised** [F]: INV-CAMERA-5 (items 4, 11), INV-COORD-3 (item 1, TST-23 R1),
INV-SCALE-1 (`t1_fit_scale_adversary`), INV-RENDER-1 at k ∉ n/120 (item 2), INV-HIT-1 (TST-18,
item 2), INV-ALLOC-1 (item 8).

**Risks** [I]: the plan lists TC-1 (production), TC-3 and TA-1 (production confirmation) as
retired. FOUNDATION_RISKS is not edited by ratification commits (INC-6's `ca2e468` left it
unchanged); it was edited by an exit package (`f2bf0da`, INC-6: TC-4's obligations). TC-1 and TA-1
are MITIGATED; TC-3 is OBLIGATION (OB-21, OB-24) — closing OB-24 leaves TC-3 citing a closed
obligation next to OB-21 (OPEN, blocks release). Whether TC-3's row changes is a change-control
choice [O]; the prepared commits do not change it.

## 5. Obligations

| OB | Blocks INC-10's exit? (living "Blocks" on `56b3f12`) | INC-10 part and evidence | Disposition |
|---|---|---|---|
| **OB-24** | **no** — Blocks: INC-10 only "until G-OB24 is met — … by the recorded decision (C12.F14 on `main`), not by this obligation's closure"; G-OB24 is met (`270128f`). Its closure follows acceptance of INC-10's exit evidence ("May be closed by: … acceptance of INC-10's exit evidence for the decided rule (C12.F14.5)") | Decision C12.F14 on `main`. Pass criterion part 1 ("evidence equivalent to X-6/ED-1 Smithay geometry runs"): item 2 (Smithay geometry through the wrapper) and item 1. Part 2 ("the decided rule shows no relative shape change during zoomed pans, or the residual is explicitly accepted with visual evidence (T3/T4)"): see U-1 — **not met as worded** | OPEN; closure needs U-1 and U-8 decided [O] |
| **OB-53** | **yes, its INC-10 part**: "the exits of INC-10 (fit loop), …" | `t1_fit_scale_adversary::an_adversarial_client_under_a_fit_changes_the_hint_at_most_once_more_and_never_moves_the_camera`: one hint rise at the fit, exactly one P7, last 20 of 30 rounds quiescent, 0 re-fits in 30 commits, camera exact; control "OB-53 INC-10 part / SC-E: a client response may reverse the hint under a fit" | INC-10 part met [F]; OB-53 stays OPEN (CameraZoom re-scoped by owner decision 2026-10-19; INC-14, INC-11 parts) |
| OB-14 | no — "none for INC-10 (owner decision 2026-10-19: the zoom ≠ 1 mismatch and clamp parts are met)" | TST-18 row; item 2. The clamp is checked at ½/k + 3·2⁻¹⁶ Canvas px (the Q16 terms of R1 and R5), not exactly the pass criterion's "≤ ½ px/k" | OPEN (random trees, rotated outputs, multi-output spanning at zoom ≠ 1) |
| OB-54 | no ("none assigned") | workaround: `t1_constraint_lifetime::{an_ended_oneshot_lock_is_never_activated_again, an_ended_persistent_lock_activates_again}`, control "Q-B3 OB-54"; W-SM-21 | OPEN (pass criterion: requested lifetime in Core) |
| OB-55 | no ("none assigned") | `t1_reconfigure_lock`: `a_reconfiguration_sends_no_motion_to_a_kept_lock` (now also asserting the physical pointer at the derived lock point), (new) `a_reconfiguration_sends_one_true_position_event_to_a_kept_confinement`, `a_reconfiguration_still_reconciles_an_unconstrained_pointer`; controls "Q-B2 OB-55: output_reconfigured sends absolute motion to a kept lock", (new) "OB-55 / C12.F17.6: output_reconfigured sends no true-position event to a kept confinement" | OPEN; its pass criterion's tests and control exist on this branch; closure needs change control [O] |
| OB-56 | no ("none assigned") | part 1 (Q3): `t1_kept_confinement` (4 tests); three Q3 controls and one Q2/Q3 control | OPEN (part 2: activation of an unobservable lock point, undecided) |
| OB-57 | no ("INC-10's software-only exit is not blocked") | — (T2, INC-13) | OPEN; no GPU claim for zoom ≠ 1 |
| OB-58 | no ("none assigned") | item 8 (software, unpaced) | OPEN (TUNE, INC-13) |
| OB-59 | no ("any drag-and-drop support claim") | `t1_clipboard::a_drag_started_on_a_real_press_is_refused` | OPEN |

[F] By the living text, only OB-53's INC-10 part blocks INC-10's exit, and it is met. INC-10's
ratification and OB-24's closure are therefore separable: INC-10 can be ratified on this evidence
with OB-24 left OPEN until U-1 and U-8 are decided.

## 6. Unmet, uncertain or stale

- **U-1 [O] (OB-24 part 2: the bound reset is not an accepted residual).** Living F-LATTICE-1
  (FOUNDATION_ARCHITECTURE, living §F) sets `R_O` "when O's view is (re)defined — O connected; a
  camera bound or re-bound to O; `k_O` changed; `view_O` changed — **and** whenever keeping it
  would make `|v_O − R_O|·k_O` exceed 2³¹"; its known residual accepts the ±1 px re-rounding only at
  "each (re)definition above" and after a restart. The bound reset is listed apart from the
  (re)definitions, so a reset during a pan (`a_pan_beyond_the_reference_bound_sets_it_again_on_that_axis`)
  is a relative shape change during a zoomed pan that the living text has **not** accepted, and
  OB-24 accepts a residual only "with visual evidence (T3/T4)", which does not exist. [F] Not every
  fit or restore re-defines the view: `camera_view` keeps R when k and `view_O` are unchanged
  (`crates/infynitae-core/src/lib.rs`, the `camera_view` keep branch). [I] Today a pan reaches the
  bound only after 2³¹ physical px of hand travel from R (about 10⁶ screen widths), but a later
  camera jump at constant k — INC-11's `Navigate`, `navigation.rs` "Jump arrives later" — can put
  the view near the bound, after which a short pan re-rounds edges by ±1 px. Options for change
  control: (a) a later layer's erratum adding the bound reset to F-LATTICE-1's accepted residual,
  with the T3/T4 visual-evidence clause explicitly waived or satisfied (owner only); (b) close OB-24
  for Foundation 01's navigation (pan, fit, restore) only and open a forward obligation on INC-11's
  `Navigate` (a jump near the bound); (c) leave OB-24 OPEN. The prepared closure commit assumes (a)
  or (b) has been decided first; it is not to be applied otherwise.
- **U-2 [O] (TST-19 parity).** F4 (ladder change on hotplug) has no named production test; F7 (popup
  constraint target, R8) is tested at zoom 1 only (INC-1). F2 left with OB-53's CameraZoom part.
  Change control decides whether these are INC-10 exit gaps or OB-14/OB-53 scope.
- **U-3 [F] (stale plan status line).** FOUNDATION_IMPLEMENTATION_PLAN INC-10 "Implementation status
  (2026-10-18)" still reads "the clarifications are prepared, not adopted, as the C12.F17 package";
  C12.F17 was adopted on `56b3f12`. The prepared ratification commit appends the adoption and the
  ratification to that line (living region `living:INC`).
- **U-4 [F] (stale CHECKPOINT date).** CHECKPOINT's `Date:` line reads 2026-10-18; the ratification
  commit updates it.
- **U-5 [I] (commit subject).** The adoption commit `56b3f12` is titled "[PREPARED FOR OWNER
  ADOPTION — not applied to main; not pushed]"; it was adopted by the owner's fast-forward. A commit
  message cannot be changed without rewriting `main`; the CHECKPOINT entry is the record. The
  prepared commits below use subjects that stay true before and after they are applied.
- **U-6 [O] (F-LATTICE-1 known residual).** Living F-LATTICE-1 says agreement of presented pixels
  with Core's rects at k ∉ n/120 "is not established (C12.F14.4 item 3, INC-10 exit)". Item 2 now
  establishes it on software renderers. Formula text changes only by a layer; the prepared commits
  do not edit it (an erratum with U-1 (a), or accept it as a forward reference satisfied here).
- **U-7 [F] (CHECKPOINT CURRENT_PHASE).** It lists "OB-54 … OB-58 … are OPEN" without OB-59 (added by
  C12.F17.11) and "INC-10 is implemented, not ratified"; the ratification commit rewrites those
  clauses.
- **U-8 [F] [O] (OB-24 cannot be closed by the checker as it stands).** `tools/check-foundation-docs`
  rule `deferred-closed` fails for every obligation whose Destination is in `DEFERRED_DEST`
  (line 55), which includes `DECISION` (OB-24's), and self-test fixture `04-deferred-closed.json`
  ("deferred decision OB-24 falsely marked closed") asserts exactly that. The test plan's own rule is
  weaker ("cannot be `CLOSED` without the evidence its pass criterion names"). Closing OB-24 needs a
  checker change, itself an owner decision; the prepared closure commit makes the narrowest one (a
  `DECISION` obligation may be CLOSED only when its History records a "Ratified CLOSED at" closure
  by architecture change control; T2–T5, TUNE and UP unchanged; fixture 04 retargeted to OB-56; new
  fixture 49).
- **U-9 [F] (other stale living text, not edited by the prepared commits).** OB-24's History says the
  awaited exit evidence is "FOUNDATION_IMPLEMENTATION_PLAN INC-10 (items 1–10)"; the plan now has
  11 items (C12.F16). OB-53's Item still says "discrete CameraZoom epochs (INC-10)" and its "May be
  closed by" says "closes when all three parts are met", while its Blocks (C12.F17.11) names four
  parts. These are registry fields changed by amendment, not by ratification [O: with the next
  layer].
- **Not covered (by design, not blocking):** GPU/GLES rendering, dmabuf, scanout, pacing (OB-57,
  OB-58); T3/T4 visual evidence; random trees, rotated outputs, multi-output spanning at zoom ≠ 1
  (OB-14); discrete CameraZoom epochs (OB-53, re-scoped); drag-and-drop (OB-59).

## 7. Independent review (2026-10-20)

A fresh-context, read-only adversarial review of the draft (`a56cc3c`) against the living text, the
code and the tests found 13 issues (2 high, 6 medium, 5 low); it confirmed the test names, hashes,
control mappings, the F-FIT-1, item-3, item-6, TST-18 and OB-53 evidence and U-3, U-4, U-7. What
was done:

| # | Sev. | Finding | Resolution |
|---|---|---|---|
| 1 | high | The prepared OB-24 closure changes the docs checker without saying so; branch named wrongly; dry-run commit subjects claimed an authorization that does not exist | U-8; §8 names the real branch; subjects reworded (U-5) |
| 2 | high | U-1: the bound reset is not an accepted residual; "reachable length" is not in the text; a fit does not always re-define; INC-11 jumps make the bound reachable | U-1 rewritten; OB-24 part 2 marked not met; closure gated on (a)/(b) |
| 3 | med. | OB-24 "Blocks" misread (G-OB24 met: it does not block INC-10) | §5 corrected; ratification and closure separable |
| 4 | med. | TST-19's canvas-exact negative control not ported at zoom ≠ 1 | ported in `tst18_d5hit_zoomed_configurations` (`632873c`) |
| 5 | med. | TST-19 F2–F4, F7, F8 attribution unsupported | mapped per finding; F4 and F7 → U-2 [O] |
| 6 | med. | TST-23 R1 "as T0 properties" not met (one T1 configuration) | T0 grid test + control (`632873c`) |
| 7 | med. | Item 11 overclaimed (items 3, 6, 7, 10; isolation test uncited; "clamped" control) | four tests added or extended (`632873c`); item 7 and "clamped" readings stated [I] |
| 8 | med. | OB-55 evidence lacked the confinement case and the lock-point check | added, with a control (`632873c`) |
| 9 | low | Item 1: gaps asserted at T0 only | stated |
| 10 | low | Item 2: one y-axis configuration missing; figures from `c4a7263` | stated |
| 11 | low | Citations (probe name, M10 control's test, Q4 test, Q3 control count, OB-14 clamp bound) | corrected |
| 12 | low | Risks paragraph inaccurate; TC-3 would cite a closed OB | corrected |
| 13 | low | Stale OB-24 History "(items 1–10)", OB-53 Item / May-be-closed; no controls for two tests | U-9; stated in §4 |

⟨RECHECK⟩

## 8. What change control is asked to decide [O]

1. Accept INC-10's exit evidence (§4) on `56b3f12` + this branch, deciding U-2 (F4, F7) and noting
   U-6.
2. Ratify INC-10 — independent of OB-24 (§5). Prepared, not applied: the first commit on
   `prep/inc10-ratification-2026-10-20`.
3. Decide U-1 ((a), (b) or (c)) and U-8 (the checker change). Only after (a) or (b) and U-8: close
   OB-24 — the second commit on that branch, prepared, not applied.
