# Placement amendment (registry layer C12.F1) — record

Status: **adopted 2026-10-13 by owner approval; local main, not pushed.** This file is evidence
and provenance. The authority is FOUNDATION_ARCHITECTURE §C12.F1.

## 1. What was approved, and what the layer says

Approved semantics, verbatim: "Default placement is resolved once at first map, when
authoritative initial window geometry becomes available, rather than at the initial surface
commit." With it: Core's default becomes centred placement with the established
oversized-window fallback.

Effects of the layer:

| Document | Change |
|---|---|
| FOUNDATION_ARCHITECTURE §C12.F1 | new registry layer, one row (INV-STAGE-1) |
| FOUNDATION_ARCHITECTURE §F | new formula F-PLACE-1 |
| FOUNDATION_INVARIANTS INV-STAGE-1 | wording, origin, layers, trigger, formulas |
| FOUNDATION_STATE_MACHINES M1 | two rows: provisional placement at initial commit; default placement at first map |
| PROTOTYPE_DECISION_LEDGER PL-21 | the Level D decision recorded |
| `tools/check-foundation-docs` | ordered amendment layers C12.F1, C12.F2; 4 seeded fixtures |

The sealed C12 table is not edited. No invariant is added or removed (57).

## 2. Verification against the approved semantics

| Aspect | Finding |
|---|---|
| Coordinate spaces | anchor: integral Canvas point; camera centre: Canvas Q16; usable extent: output-logical, `view_O / scale_O`, after the output transform; one R3 rounding (ties to even) |
| Rounding | one rounding of the anchor. Checked against an exact integer model on 2,880 cases, including half-pixel ties of both parities and fractional logical extents |
| Remapping | the anchor is kept (INV-STAGE-5); an element is placed exactly once (`Stage` records that it has been mapped) |
| Ownership | `stage::place` only, called from Core's protocol entry at both moments; no other writer |
| Camera | a pan, an output reconfiguration and a second output leave the anchor unchanged (tested) |

Three things in the adopted text go beyond the one approved sentence. Each was checked against
the owner's stated policy and found covered; they are listed so that nothing is adopted
unnoticed:

1. **A provisional anchor is still written at initial commit.** This is the sealed behaviour,
   kept because the initial scale hint (INV-SCALE-5, M15) is predicted from it.
2. **"An explicit or restored anchor is final at initial commit."** The owner's policy list
   ("explicit and restored placement take precedence"). No such path exists in Core today, so
   the clause changes no behaviour and cannot be tested yet.
3. **The oversized fallback** (alignment to the region's origin on an axis that does not fit),
   named in the approval as "the established oversized-window fallback".

Limits stated in the layer: transient windows, reserved regions and placement under a persistent
camera zoom ≠ 1 are not defined. At zoom ≠ 1 the implementation still centres a fitting window
on the camera; its oversized comparison uses the zoom-1 extent.

## 3. Coordinate examples

`placement_examples.py` (exact rationals, no Core code) derives 18 cases; they equal the
expectations of the T0 placement tests. The Core test
`placement_agrees_with_the_exact_model_on_fractional_extents_and_ties` compares 2,880 further
cases with an integer model written in the test.

## 4. Re-derivation of the 41 affected expectations

With Core's default flipped, 41 of 272 tests failed. Each was re-derived from F-PLACE-1 before
the new value was compared with Core's output; every prediction matched on the first run. The
model in every case: a window of geometry `w × h` on the first output with camera centre `c`
has anchor `c − (w/2, h/2)` (R3), where the legacy anchor was `c`.

Two kinds of change were made, chosen per test:

- **Value** — the scenario is unchanged and the expected coordinates move by the anchor shift.
- **Scenario** — the test probes a position defined relative to the window (an edge, a hole, a
  subsurface) or needs the pointer inside it after a clamp; the approach path or a camera was
  moved by the anchor shift so that the test still probes the same thing.

| Suite | Test | Kind | Derivation |
|---|---|---|---|
| t1_adversarial | a_physical_pixel_boundary | scenario | W root spans Canvas x [−420, 420), y [−320, 320): p.x ∈ [540, 1380), top row p.y = 220; path moved to those edges; row y is W-local 320 |
| | b_fractional_scale_edge_and_clamp | scenario | 301×201 on 1280×720: anchor (−150.5, −100.5) → (−150, −100); right edge 151 projects to pixel 1187; path 150.75, +0.5, +0.25; row is W-local 100 |
| | c_negative_canvas_coordinates | value | anchor (−5400, −3300); pointer W-local (420, 320) → enter (421, 321) |
| | d_large_canvas_coordinates_near_the_bound | value | enter (421, 321); right edge 2³⁶ − 1580 still left of 2³⁶ |
| | e_output_transition_uses_the_new_outputs_camera | value | q (41, 0) → W-local (461, 320) |
| | f_output_removal_relocates_to_the_nearest_survivor | value | q (40, 0) → W-local (460, 320), both cases |
| | h_first_output_returns_without_history | value | q (300, 0) → W-local (720, 320) |
| | i_overlapping_surfaces_route_to_the_presented_one | scenario | W1 [−200,200)×[−150,150), W2 [−100,100)×[−50,50); second probe moved to q (150, 10) |
| | j_input_region_holes_fall_through | scenario | hole is Canvas [−100,−50)×[−50,0); path (−90,−40), +50, −11 |
| | k_unmapped_surface_receives_nothing | scenario | S at Canvas [50,150)×[−150,−100); step to S moved to (44, −155); other steps unchanged |
| | l_no_stale_frame_after_reconfiguration | scenario | the fully clamped corner is now outside W; pointer first moved to p.x 860: W-local (800, 590 − 2⁻⁸), y still clamped |
| t1_input | routed_motion_enters_then_moves_with_relative_deltas | value | x8 values + (400, 300): (620, 520), (630, 520) |
| | x8_c1_keepfocus_pan_then_true_position | value | (411, 583), then (412, 583) |
| | release_before_the_next_frame_uses_the_panned_camera | value | (411, 583) |
| | x8_c6_no_acquisition_while_the_client_owns_a_button | value | (634, 520) |
| | x8_c7_presses_during_the_pan_stay_compositor_owned | value | (627, 520), (628, 520) |
| | pan_membership_changes_without_configures | value | W [−420, 420) still outside view [1040, 2960); final (630, 520) |
| | zero_output_transition_with_a_held_button | value | startup pointer W-local (420, 320) |
| | zero_output_transition_free_pointer | value | (420, 320) |
| | removing_the_pan_output_orphans_the_pan | value | q (100, 100) → W-local (520, 420) |
| | reconfiguration_clamps_and_sends_one_true_position | scenario | a view larger than the window leaves every clamped point outside it; mode 800×600 instead: clamp on both axes, W-local (820 − 2⁻⁸, 620 − 2⁻⁸) |
| t1_outputs | x5_popup_membership_and_output_enter_control | scenario | A's camera moved to (1400, 600) so W's anchor is X5's (1000, 300); every X5 value kept |
| | r15_rule1_uses_the_causing_event_output | value | W1 [−200, 200); popup Canvas [190, 790): A 390 (fits), B 0, C 240 |
| | serial_provenance_fifo_eviction_and_fallback | scenario + value | first motion (50, −50) so the pointer is on W1 on output C too (q.y 130); values 0 / 240 / 390 |
| t1_parity | popup_is_configured_inside_the_output_target | value | viewport is parent-relative [−760, 1160): x = 1160 − 1200 = −40 |
| t1_pixels | a_solid_buffer_lands_on_exactly_its_pixels | value | 100×80 at (320 − 50, 240 − 40): [270, 370) × [200, 280) |
| | subsurfaces_and_windows_stack_in_render_order | value | W1 placed as its 200×150 root: geometry origin (220, 165); root (235, 165) after the below-subsurface widens the geometry; W2 at (280, 210) |
| | scaled_and_clipped_surfaces_match_the_render_list | scenario | the window's middle is the view centre: pan by 320/k |
| | damage_follows_commits_and_nothing_else | value | W at (270, 200), S at (280, 210) |
| t1_slice | unmapped_subsurface_leaves_the_frame_and_returns | value | probe pixel shifted by the anchor (−200, −150) |
| t1_transform | membership_uses_the_transformed_view | scenario | W at [−200, 200): camera x 800 (hidden) and 700 (visible) |
| | n_pan_moves_render_and_hit_together | scenario | the 40×30 root is centred, so the tile grid is offset by (20, 15) to stay around the camera |
| core input | client_motion_is_routed_with_its_relative_delta | value | target origin (−200, −150) |
| | a_pan_emits_no_client_effect_and_ends_with_one_true_position | value | q (50, −30) is now inside W: the true position has W as its target |
| core pointer | m14_confinement_clamping_and_arrangement_isolation | scenario | the confinement region's corner moved to the pointer, surface-local (200, 150) |
| core protocol | m1_map_unmap_remap_destroy | value | provisional (0, 0) at initial commit; (−50, −25) at first map; kept on unmap and remap |
| | popup_target_is_the_viewport_inward_rounded | value | parent-relative: (−910, −515) at scale 1, (−498, −283) at 1.75 |
| | default_policy_places_the_origin_at_the_camera_centre | replaced | now `the_legacy_policy_places_the_origin_at_the_camera_centre`: the default is centred; the legacy rule is selected explicitly |
| core lib | unshown_sibling_is_not_in_the_frame | value | probe points from anchor (860, 490) |
| | subsurface_unmap_and_remap_change_the_frame | value | same |
| | dead_frame_entries_fail_closed_until_the_next_frame | value | subsurface covers (870..890, 500..520) |

One test that did not fail, `unshown_subsurface_is_not_in_the_frame`, had been probing a point
that was only coincidentally still on the root; the shared helper now points at the intended
subsurface position.

## 4a. Tests that still passed but no longer tested what they said

A passing test is not evidence that its scenario survived. The seeded-defect tool found the
first case; the rest came from reading every pointer motion and camera position in the suites
that had not failed.

| Where | What had silently changed | Correction |
|---|---|---|
| `t1_popup_lifetime` `b1_mapped_parent_popup_destroyed_is_not_a_parent_commit` | the popup was no longer under the pointer, so the test stopped detecting an un-rebuilt hit-test tree. **Found by the seeded-defect tool** (one control MISSED on the first gate run) | see next row |
| `click_focus` and the "(50, 50)" approach in `t1_popup_lifetime`, `t1_popup_focus`, `t1_keyboard` (12 sites) | documented as "the window's local (50, 50)", inside the 120×80 popup at (10, 10); the pointer was at local (250, 200), outside it | the approach is `(−150, −100)` from the view centre, which is local (50, 50) of a centred 400×300 window |
| `t1_outputs` `clock_does_not_depend_on_visible_area` | "A shows a 20 px sliver" had become 420 px | A's camera moved to x = 60: view right edge 1020, W starts at 1000 |
| comments in `t1_outputs`, Core `entry` and `visibility` tests | named the legacy window position | re-derived; the scenarios themselves still hold |

Checked and left unchanged: the pointer-constraint tests (the pointer is still on W; no region
is position-dependent), the hit-parity and snapshot tests (they read the anchor and the frame),
and the long-lived stress tests, whose assertions are about bounded state. For the stress tests
the relation between pointer and popup differs from before; that was not restored.

## 5. Legacy-anchor coverage kept

`PlacementPolicy::OriginAtCameraCentre` remains an explicit policy. Tests:
`the_legacy_policy_places_the_origin_at_the_camera_centre` (any size; not revised at first map;
a window placed under it keeps its anchor when the policy changes) and
`the_legacy_policy_follows_the_camera` (negative coordinates, R3 ties).

## 6. Not claimed

No real-application or GPU evidence is part of this amendment. Restored and explicit placement,
transient windows and reserved regions are not implemented.
