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
living foundation text. Not authorized: ratifying INC-10, closing OB-24 or any OB, pushing,
touching `main`.

Base [F]: `main` = `56b3f123acaef9a066b8a78810d295df5a66b3b0` (C12.F17 ADOPTED 2026-10-20, owner
fast-forward over `16f13c9`), tree `5f800659f580bfa002524a20ec2ad26c82c11d1f`. Every test name
below was checked to exist on `56b3f12` (`git grep "fn <name>" 56b3f12`). The only commit on this
branch before this record is `b594900` (test hygiene: a kill-and-reap guard in one
`t1_tiny_compositor` test; CHECKPOINT note); it changes no production code.

## 2. Gates

| Tree | Log | Result |
|---|---|---|
| `56b3f12` (worktree `main-gate-56b3f12`, own `CARGO_TARGET_DIR`) | `~/.cache/infynitae-integration/session-2026-10-20b/gates-main-56b3f12/` | ⟨G56⟩ |
| this branch's tip (record + `b594900`) | `…/session-2026-10-20b/gates-cc-tip/` | ⟨GTIP⟩ |

Exit item 10 ("all repository gates on the exact final implementation commit") is met by the
first row for the implementation and by the second row for the commit a ratification would name
[I: the tip adds documentation and one test guard only].

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

| # | Criterion (plan text, abbreviated) | Evidence on `56b3f12` | State |
|---|---|---|---|
| — | TST-18 d5hit zoomed configurations | `t1_d5hit_zoomed::tst18_d5hit_zoomed_configurations` (D5 tree, input-region hole, popups, nested subsurfaces; six zooms off 1/120 incl. ED-1's 1080/651; 0 render, 0 hit mismatches; clamp exercised); control "TST-18 zoomed / OB-14 clamp: delivered coordinates not clamped below the surface's far edge" | [F] met |
| — | TST-23 R1 as a T0 property (pan algebra, content follows the hand) | T1 `t1_input::zoomed_pan_moves_by_f_pan_1_and_keeps_widths`; controls "INC-10 F-LATTICE-1: a pan acquired at zoom != 1 does not scale by the zoom", "INC-3 F-PAN-1: camera pan sign reversed", "… output scale omitted from the pan term" (the sealed pan formula) | [F] met at T1 [I: the plan says "as T0 properties"; the production property is a T1 test on the production path, stronger than T0; no separate T0 port] |
| — | TST-19 (d5_model F1–F8) as T0 properties | F1 (reference switch): T0 `projection::a_kept_lattice_reference_keeps_widths_and_gaps_during_a_zoomed_pan`, `entry::the_lattice_reference_follows_f_lattice_1_transitions`, `a_pan_beyond_the_reference_bound_sets_it_again_on_that_axis`, controls "INC-10 F-LATTICE-1: the kept-reference bound is ignored", "F-LATTICE-1: lattice reference truncates instead of flooring"; F5 (range): `navigation::tests::fit_zoom_clamps_at_the_upper_bound`, `the_fit_centre_is_exact_and_clamped_into_the_canvas`; F6 (Q16 clamp): `projection::grid_value_precision_at_the_conversion_limit`, `t1_d5hit_zoomed` clamp; negative control "canvas-exact rule": `render::tests::physical_selection_differs_from_the_canvas_exact_rule_at_fractional_scale` (Core, INC-1) | [I] mapped for F1, F5, F6; F2–F4 and F8 are scale-policy findings ported by INC-6 (ratified), F7 (popup constraint target, R8) by INC-5 [U: no line-by-line port record of F2–F4, F7, F8 exists; the reviewer is asked to confirm] |
| — | 200-commit "client resize never moves the camera", production path | `t1_navigation::client_resizes_never_move_a_fitted_camera` (added at INC-10: it did not exist before, IMPLEMENTATION_RECORD §3) | [F] met |
| 1 | 0 width/gap changes per zoomed pan with the kept reference | `t1_input::zoomed_pan_moves_by_f_pan_1_and_keeps_widths` (k = 0.73, 240 steps, > 5 lattice cells; per-frame reference changes a width at step 63 — control "INC-10 F-LATTICE-1: the output's lattice reference is not kept (set from every view origin)"); T0 `a_kept_lattice_reference_keeps_widths_and_gaps_during_a_zoomed_pan`; `t1_zoomed_lifecycle::lifecycle_while_zoomed_and_panning_keeps_render_hit_and_membership_consistent` | [F] met |
| 2 | Render/hit agreement through the wrapper (ulp sweeps, ±2²⁰/±2³¹ references, both axes, Pixman seams, pointer columns, ProbeRenderer beyond Pixman) | `t1_canonical_rect::render_and_hit_agree_through_the_wrapper_at_rounding_points` (139,392 rects, 0 presented ≠ Core, 0 seams, 0 wrong pointer pixels), `pixels_and_pointer_follow_the_canonical_rect_at_a_seam_zoom`, `pixels_follow_the_canonical_rect_in_every_sweep_configuration` (12 configurations), `the_wrapper_removes_the_unwrapped_mismatch`, `the_tracker_draws_the_wrapped_element_at_the_canonical_rect` and `rects_beyond_the_i32_range_are_cropped_with_a_proportional_src` (draw-argument records beyond Pixman's range); controls C12.F15.8 (1), (2) ×2, review L4 | [F] met (software renderers only) |
| 3 | Rounding-step test; Q16 at \|v − R\| = 2³⁷ + 119, k = 2⁻²² | `projection::kept_reference_rounding_steps_at_the_visible_edge_limit`, `projection::grid_value_precision_at_the_conversion_limit`; controls "C_O without δ", "δ dropped", "δ below one rounding step" | [F] met |
| 4 | F-FIT-1 (C12.F15, C12.F17): clamps, 1 px W at 0.25, scale-2 exact fit, degenerate/unmapped W, centre ±2³⁶, `View::new`, exact restore, exactly one true-position event unless a lock is kept, none during an interaction, refused while O presents | `navigation::tests::{fit_zoom_clamps_at_the_lower_bound, fit_zoom_clamps_at_the_upper_bound, a_window_that_fits_the_view_at_scale_2_fits_at_zoom_1, a_degenerate_window_has_no_fit, the_fit_centre_is_exact_and_clamped_into_the_canvas}`; Core `lib.rs` `a_fit_records_the_prior_value_and_restore_returns_it_exactly`, `restore_is_exact_for_a_zoom_off_the_lattice`, `refused_fits_change_neither_camera_nor_history`, `no_fit_or_restore_during_an_interaction`; T1 `t1_fit_constraints::{fit_and_restore_reconcile_the_pointer_under_each_constraint, a_fit_that_ends_the_confinement_ends_it_once_then_reconciles_the_pointer, no_fit_reconciliation_while_a_pan_owns_the_pointer}`; `t1_fit_bindings::{fit_and_restore_are_refused_while_the_output_presents, a_fit_during_the_presentation_of_a_resized_client_is_refused}`; controls "F-FIT-1: the zoom is not divided by the output scale", "… not clamped at the upper bound", "M10: no true-position event after a fit", "review M1: absolute motion reaches a kept lock after a fit or restore", "review M1: a kept confinement suppresses the true-position event", "C12.F17 Q1 B: presentation guard removed" (fit, restore) | [F] met |
| 5 | Damage invalidation on a zoom change | `t1_navigation::a_zoom_change_rebuilds_the_damage_tracker`; control C12.F15.8 (6); "exactly" read per C12.F17.5 (K3: ≤ 1 px far-edge over-damage for pass-through surfaces) | [F] met |
| 6 | i32 crop beyond the range; rotated/flipped buffer transforms, viewport sources, buffer scale (C12.F17.3); a transform the crop cannot serve reported | `t1_canonical_rect::{a_cropped_surface_draws_exactly_its_visible_part_for_every_buffer_transform, a_cropped_surface_keeps_its_sampling_with_a_viewport_source_and_buffer_scale, rects_beyond_the_i32_range_are_cropped_with_a_proportional_src, the_transform_oracle_matches_the_pixels, the_transform_oracle_matches_the_gles_texture_matrix, smithays_crop_element_misdraws_rotated_buffers, the_crop_box_is_half_open_with_the_width_capped_at_i32_max}`; controls C12.F15.8 (5) ×4, review E1, E3. Reported transform: Smithay's `CropRenderElement` formula under the four rotating transforms — decided by C12.F17.3 (K1) | [F] met for whole-pixel cuts on Pixman; fractional cuts and GPU: OB-57 (Q5 defer; does not block, §5) |
| 7 | Map, unmap, destroy during a zoomed pan | `t1_zoomed_lifecycle::lifecycle_while_zoomed_and_panning_keeps_render_hit_and_membership_consistent` | [F] met |
| 8 | Performance at zoom ≠ 1 vs zoom 1 (p50, p99, max) and INV-ALLOC-1, measured | INV-ALLOC-1: `t0_hot_path_allocations::zoomed_hot_paths_and_fit_restore_allocate_nothing` (asserts 0; runs in every gate). Frame times: `perf_inc10` re-run on `56b3f12` (release, two runs): `perf_frame_times_56b3f12_run{1,2}.out` in this directory (⟨PERF⟩); earlier runs on `c4a7263`: `perf_frame_times_run{1,2}.out` | [F] measured, software renderers only, unpaced, no CPU isolation (OB-58, does not block) |
| 9 | Seeded controls of C12.F15.8 each DETECTED; TST-04's control redesigned | §2 efficacy run (every control DETECTED, the C12.F15.8, C12.F16 and C12.F17 controls included); TST-04 counterpart "the wrapper presents the wrapped element's geometry (TST-04 counterpart)" | [F] met (⟨G56⟩) |
| 10 | All gates on the exact final implementation commit | §2 | [F] ⟨G56⟩ |
| 11 | Bounded history (C12.F16.6 items 1–11) and its controls | Core `lib.rs`: `restore_with_an_empty_history_is_refused`, `restores_unwind_consecutive_fits_newest_first`, `the_history_holds_exactly_256_entries`, `the_257th_fit_evicts_only_the_oldest_entry`, `refused_fits_change_neither_camera_nor_history`, `disconnect_empties_the_history_and_reconfiguration_keeps_it`, `a_hundred_thousand_fits_keep_the_history_bounded`; `navigation::tests::the_ring_keeps_the_newest_256_values`; eviction allocating nothing: `zoomed_hot_paths_and_fit_restore_allocate_nothing`; ten C12.F16 controls (capacity 255/257, newest evicted, refused fit records, another output's history popped, history kept across a disconnect, reconfiguration as reconnect, re-quantised zoom, no eviction, ring reserved again) | [F] met |

**Negative controls named by the plan** [F]: the sealed pan formula (item TST-23 R1 row); the
sticky-fit probe (`tools/verify-architecture` E0277 probe `probe_sticky_fit_retains_authority`,
run by the `arch` gate); C12.F15.8 and C12.F16.6 (items 9, 11).

**Stop / rollback conditions** [F]: no camera change from client geometry
(`client_resizes_never_move_a_fitted_camera`, `t1_fit_scale_adversary`); no presented rect
differing from Core's canonical rect (item 2: 0 of 139,392).

**Invariants exercised** [F]: INV-CAMERA-5 (items 4, 11), INV-COORD-3 (item 1, TST-23 R1),
INV-SCALE-1 (`t1_fit_scale_adversary`), INV-RENDER-1 at k ∉ n/120 (item 2), INV-HIT-1 (TST-18,
item 2), INV-ALLOC-1 (item 8).

**Risks** [I]: the plan lists TC-1 (production), TC-3 and TA-1 (production confirmation) as
retired. FOUNDATION_RISKS has never been updated at a ratification (its last edit is `270128f`;
INC-6's ratification `ca2e468` left it unchanged); TC-1 is MITIGATED, TA-1 MITIGATED, TC-3
OBLIGATION (OB-21, OB-24). Whether the ratification changes TC-3's disposition is a
change-control choice [O]; the prepared ratification commit does not change it.

## 5. Obligations

| OB | Blocks INC-10's exit? (living "Blocks" on `56b3f12`) | INC-10 part and evidence | Disposition |
|---|---|---|---|
| **OB-24** | yes — INC-10 consumes the decision; "May be closed by: architecture change control only … acceptance of INC-10's exit evidence for the decided rule (C12.F14.5)" | Decision C12.F14 on `main` (`270128f`, G-OB24 met). Pass criterion part 1 ("evidence equivalent to X-6/ED-1 Smithay geometry runs"): item 2 (Smithay geometry through the wrapper, 139,392 rects) and item 1 (0 width/gap changes per zoomed pan). Part 2 ("the decided rule shows no relative shape change during zoomed pans, or the residual is explicitly accepted with visual evidence (T3/T4)"): first disjunct by item 1 for pans within one reference; see §6 U-1 | OPEN; closes with acceptance of this evidence (prepared closure commit) [O] |
| **OB-53** | yes, its INC-10 part only: "the exits of INC-10 (fit loop), …" | `t1_fit_scale_adversary::an_adversarial_client_under_a_fit_changes_the_hint_at_most_once_more_and_never_moves_the_camera`: one hint rise at the fit, exactly one P7, last 20 of 30 rounds quiescent, 0 re-fits in 30 commits, camera exact; control "OB-53 INC-10 part / SC-E: a client response may reverse the hint under a fit" | INC-10 part met [F]; OB-53 stays OPEN (CameraZoom part re-scoped by owner decision 2026-10-19; INC-14, INC-11 parts) |
| **OB-14** | no — "none for INC-10 (owner decision 2026-10-19: the zoom ≠ 1 mismatch and clamp parts are met)" | TST-18 row; item 2 | OPEN (random trees, rotated outputs, multi-output spanning at zoom ≠ 1) |
| OB-54 | no ("none assigned") | workaround: `t1_constraint_lifetime::{an_ended_oneshot_lock_is_never_activated_again, an_ended_persistent_lock_activates_again}`, control "Q-B3 OB-54"; W-SM-21 | OPEN (pass criterion: requested lifetime in Core) |
| OB-55 | no ("none assigned") | `t1_reconfigure_lock::a_reconfiguration_sends_no_motion_to_a_kept_lock` + unconstrained counterpart; control "Q-B2 OB-55" | OPEN; fix evidenced, awaits change-control acceptance [O] |
| OB-56 | no ("none assigned") | part 1 (Q3): `t1_kept_confinement` (4 tests), three Q3 controls | OPEN (part 2: activation of an unobservable lock point, undecided) |
| OB-57 | no ("INC-10's software-only exit is not blocked") | — (T2, INC-13) | OPEN; no GPU claim for zoom ≠ 1 |
| OB-58 | no ("none assigned") | item 8 (software, unpaced) | OPEN (TUNE, INC-13) |
| OB-59 | no ("any drag-and-drop support claim") | `t1_clipboard::a_drag_started_on_a_real_press_is_refused` | OPEN |

[I] Of the obligations, only OB-24 changes status at INC-10's exit; OB-55's fix and OB-56's first
part are evidenced but their "May be closed by" needs change control *and*, for OB-56, a decision
on part 2 — the brief does not authorize closing them and nothing here proposes it.

## 6. Unmet, uncertain or stale

- **U-1 [O] (OB-24 part 2).** F-LATTICE-1 resets `R_O` "whenever keeping it would make
  `|v_O − R_O|·k_O` exceed 2³¹", including during a pan (`a_pan_beyond_the_reference_bound_sets_it_again_on_that_axis`);
  its known residual accepts one ±1 px re-rounding at each (re)definition. A pan reaches the bound
  only after 2³¹ physical px of travel from the reference [I: about 10⁶ screen widths by hand; a
  fit changes `k_O` and is a re-definition, not a pan]. OB-24's pass criterion accepts a residual
  only "with visual evidence (T3/T4)", and no T3/T4 evidence exists. Change control must decide
  whether the bound reset is within the first disjunct (no shape change during zoomed pans of a
  reachable length) or needs that visual evidence; the prepared closure commit records the first
  reading and is not applied.
- **U-2 [U] (TST-19).** F2–F4, F7, F8 have no explicit production-parity record (§4).
- **U-3 [F] (stale living status line).** FOUNDATION_IMPLEMENTATION_PLAN INC-10 "Implementation
  status (2026-10-18)" still reads "the clarifications are prepared, not adopted, as the C12.F17
  package"; C12.F17 was adopted on `56b3f12`. The prepared ratification commit replaces that line
  (living region `living:INC`).
- **U-4 [F] (stale CHECKPOINT date).** CHECKPOINT's `Date:` line reads 2026-10-18; the ratification
  commit updates it.
- **U-7 [F] (CHECKPOINT CURRENT_PHASE).** It lists "OB-54 … OB-58 … are OPEN" without OB-59
  (added by C12.F17.11) and "INC-10 is implemented, not ratified"; the ratification commit
  rewrites the line.
- **U-5 [I] (commit subject).** The adoption commit `56b3f12` is titled "[PREPARED FOR OWNER
  ADOPTION — not applied to main; not pushed]"; it was adopted by the owner's fast-forward. A
  commit message cannot be changed without rewriting `main`; the CHECKPOINT entry is the record.
- **U-6 [F] (F-LATTICE-1 known residual).** Living F-LATTICE-1 says agreement of presented pixels
  with Core's rects at k ∉ n/120 "is not established (C12.F14.4 item 3, INC-10 exit)". Item 2 now
  establishes it on software renderers. The sentence is formula text and changes only by a layer;
  the prepared commits do not edit it [O: an erratum with a later layer, or accept as a
  forward reference satisfied by this record].
- **Not covered (by design, not blocking):** GPU/GLES rendering, dmabuf, scanout, pacing (OB-57,
  OB-58); T3/T4 visual evidence; random trees, rotated outputs, multi-output spanning at zoom ≠ 1
  (OB-14); discrete CameraZoom epochs (OB-53, re-scoped); drag-and-drop (OB-59).

## 7. Review

⟨REVIEW⟩

## 8. What change control is asked to accept [O]

1. INC-10's exit evidence (§4) on `56b3f12` (+ this branch), with U-1 and U-6 decided.
2. Then, separately: INC-10 ratification and OB-24's closure — prepared on
   `prep/inc10-ratification-2026-10-20`, not applied.
