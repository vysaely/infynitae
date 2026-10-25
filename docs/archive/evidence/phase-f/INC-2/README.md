# INC-2 evidence — multi-output, output lifecycle, membership, observed frames

- **Category / execution class:** PRODUCT, HEADLESS.
- **Implementation commit:** `0a03aab`. The evidence below was captured at that commit:
  - [`test-matrix.txt`](test-matrix.txt);
  - [`verify-architecture.txt`](verify-architecture.txt);
  - [`verify-test-efficacy.txt`](verify-test-efficacy.txt);
  - [`verify-development-environment.txt`](verify-development-environment.txt);
  - [`runtime-2i-sealed-rule.txt`](runtime-2i-sealed-rule.txt).

**Status: INC-2 IMPLEMENTED — PENDING INDEPENDENT REVIEW.**
- No obligation was closed.
- No canonical architecture, invariant, formula or audit record was changed.

## 1. Canonical INC-2 (FOUNDATION_IMPLEMENTATION_PLAN)

- **Objective:**
  - several outputs through `output_connected`/`output_reconfigured`/`output_disconnected` (startup, runtime hotplug, and mode/transform/scale changes at runtime);
  - camera creation/binding precedence (1:1, collision rule);
  - per-surface membership in `view_O` space;
  - damage trackers sized `phys_O`;
  - the zero-output state;
  - the frame clock;
  - presentation feedback for observed elements.
- **State:** OBJ-07, 08, 09, 10, 13, 17 (observed part).
- **Seams:** `wl_output` globals, `wl_surface.enter/leave`, presentation-time.
- **Invariants:** INV-OUTPUT-1, INV-OUTPUT-2, INV-CAMERA-1, INV-CAMERA-3, INV-RENDER-3, INV-FRAME-1, INV-FRAME-2, INV-FRAME-5.
- **Obligations:** OB-3 (removal part, without interactions), the OB-19 runtime counterpart, OB-45 (tracker, membership, damage part), OB-49, OB-50 (core part).
- **Formulas:** F-VIEW-1 and F-RENDER-1 (`view_O` / `phys_O`), F-MEMBER-1, F-RELOCATE-1, F-RECONF-1.
- **State machines:** M4 (all rows, Core and adapter part), M5 (all lifecycle rows), M16 row 1.

**M4 effects that belong to later increments, and are not implemented:**
- the zero-output pointer grab, routed motion and seat (INC-3);
- constraint reconciliation (INC-5);
- ScaleInteractionEpoch settlement and the scale ladder (INC-6);
- the protocol-fullscreen configure (INC-7);
- kick and fallback for unobserved elements (INC-8);
- interactions orphaned on removal (INC-3/INC-14).

## 2. Requirement → implementation → test → control → result

| Requirement | Implementation | Test (tier) | Negative control | Result |
|---|---|---|---|---|
| Output entry only with the token (INV-OUTPUT-2) | `Core::output_*` take `&mut OutputLifecycleToken` | probes p6, p7, p10–p13; `foreign_tokens_grant_nothing` (T0) | compile probes; efficacy | PASS |
| Binding precedence remembered-unbound → persisted (no collision) → default; 1:1 (INV-CAMERA-3, M5) | `navigation::CameraSet::bind_on_connect` | `connect_creates_and_binds_one_camera_per_output`, `disconnect_keeps_the_camera_and_reconnect_rebinds_it`, `remembered_camera_wins_over_the_persisted_entry` (T0) | efficacy: N:1, stale binding, seeded collision | PASS |
| **Runtime 2i** (OB-49, transferred from OB-19) | same | `concurrent_output_key_collision_keeps_one_to_one` (T0), `same_key_outputs_keep_one_to_one` (T1, real client, distinct wl_output names) | `probe_sealed_key_steal` (the sealed 2R rule) fails both with "1:1 violated" — `verify-architecture` stage [5b]; efficacy: camera stealing (T0 and T1) | PASS |
| Reconfiguration: same id and camera, no proof, rejections (EE-4) | `Core::output_reconfigured` | `reconfiguration_keeps_identity_and_camera`, `reconfiguration_rejections_mutate_nothing` (T0); `reconfiguration_on_the_wire` (T1: wl_output mode/scale/done, xdg size, enter/leave, key change rejected) | 2h source rule + seeds (mint, navigation, camera re-bind); efficacy: key change accepted | PASS |
| F-RECONF-1 | `pointer::reconfigured` | `reconfiguration_clamps_the_pointer` (T0) | efficacy: clamp skipped | PASS |
| Disconnect: camera unbound and kept, leave before global destroyed, stale ids rejected (INV-OUTPUT-1) | `Core::output_disconnected`; adapter applies leaves, then removes the global | `disconnect_emits_leaves_then_removal` (T0); `lifecycle_on_the_wire` (T1 wire order) | efficacy: no leave (T0, T1), removal before leaves (T1), frame kept | PASS |
| F-RELOCATE-1, domain non-empty | `pointer::relocate(lost, p, first, rest)`: an empty set is unrepresentable | `relocation_uses_survivors_only`, `relocation_picks_the_nearest_survivor` (T0) | "sealed F-RELOCATE-1 on an empty survivor set" is unreachable by type; efficacy: farthest survivor | PASS |
| Zero-output core state | `PointerState::NoOutput` | `zero_output_state_is_stable_and_the_first_connect_leaves_it` (T0) | efficacy: fabricated location | PASS |
| Reconnect separate from reconfiguration | new `OutputId`, same camera | `disconnect_keeps…`, `reconfiguration_rejections…` (T0); `lifecycle_on_the_wire` (T1) | efficacy: OutputId reuse | PASS |
| Membership per surface in `view_O` space (F-MEMBER-1, INV-RENDER-3) | `visibility::MembershipTable` from `render::entries_for`; `Output::enter/leave` from effects | T1 lifecycle/reconfiguration; `membership_uses_the_transformed_view` (T1, 90°) | X5 `Window::output_enter` control enters an invisible popup; efficacy: adapter drops enters | PASS |
| X5 popup membership (TST-03) | same | `x5_popup_membership_and_output_enter_control` (T1, 3 cases) | `Window::output_enter`, driven as `Space` drives it | PASS |
| Damage tracker `phys_O` (F-RENDER-1, EE-1, OB-45 tracker part) | `convert::damage_tracker` | `ee1_tracker_matrix_on_the_production_adapter` (C1–C4, 10 cases); `production_render_output_damages_the_view` (real client, production render path) | sealed tracker (`view_O`) fails 7/7 rotated cases; efficacy: tracker sized `view_O` | PASS |
| Frame clock: highest-refresh observer, ties lowest OutputId (INV-FRAME-2) | `visibility::clock_output`, `Core::frame_tick` | `clock_selection_and_frame_tick`, `unmapped_elements_are_not_observed` (T0); `clock_is_the_highest_refresh_observer` (T1: 165 dones/s), `clock_does_not_depend_on_visible_area` (T1, sliver) | efficacy: lowest refresh, tie by highest id, every observer completes | PASS |
| Callbacks only when requested; withheld while unobserved; re-entry and handover (INV-FRAME-1, SC1/SC3) | `complete_frames` → `Window::send_frame` | `starve_reentry_and_handover` (T1: 0 completions off-screen; first completion at the new clock's first vblank, exact time) | efficacy: no completion | PASS |
| Presentation: presented with `sync_output` = clock; superseded discarded; latest pending until shown (INV-FRAME-5, x9) | `take_presentation_feedback` + `presented` | `x9_presentation_feedback` (T1) | efficacy: no completion | PASS |
| Unique `wl_output` names | adapter `unique_name` | `same_key_outputs_keep_one_to_one` (T1) | efficacy: duplicate names | PASS |
| Popup anchor output = parent's clock output, else none (D-B6-BOUNDARY rules 2 and 3) | `Core::popup_constraint_target` | `popup_target_is_the_viewport_inward_rounded` (T0, hand-derived literals) | INC-1 R8 controls | PASS |

The oracles are independent of the production helpers:
- T0 expectations are hand-derived literals, from the spike's EE-4/ER2-1 values and the INC-1 exact-rational reference.
- T1 expectations are read from the client's ordered wire log, compared with hand-derived values.
- The INC-1 popup-target test, which had compared against production `View::unproject`, was rewritten with reference literals (the same pattern as INC1-F06).

Membership deliberately uses the render path's own rects (`render::entries_for`): F-MEMBER-1 defines membership by the same physical projection as rendering. It is checked against the client's wire events and against hand-derived geometry (X5, rotated view).

## 3. Test efficacy and enforcement

- **`tools/verify-test-efficacy`:** 45/45 seeded defects detected, judge controls correct, canonical tree byte-identical.
  - **New INC-2 controls (21):** camera stealing (T0 and T1), seeded collision, N:1, stale binding, OutputId reuse, key change accepted, clamp skipped, frame kept after disconnect, fabricated zero-output location, farthest survivor, no leave (T0 and T1), removal before leaves, lowest-refresh clock, wrong tie-break, every observer completes, no completion, enters dropped, duplicate names, sealed tracker size.
  - **Retired:** INC-1's "a second output connected" control, because INC-2 authorizes multiple outputs.
  - **Gap found during this work:** the farthest-survivor control was first MISSED, because the relocation test had a single survivor. The test was strengthened to two survivors at different distances.
- **`tools/verify-architecture`:** PASS.
  - New stage [5b] runs runtime 2i on T0 and T1. Each test passes with the 1:1 rule and fails with "1:1 violated" under `probe_sealed_key_steal`.
  - The 2h seeds were re-anchored to the new `output_reconfigured` body, and a re-bind seed was added.
  - 47 seeded controls detected and 27 compile probes as asserted. The F01/F02 and INC-1 controls are unchanged.
- **Dependencies:** none new. Core is still std-only; `smithay =0.7.0`; no `[patch]`; P1 not applied (OB-11 open); no `unsafe`.

## 4. Obligations

| Obligation | INC-2 part | Status recommendation |
|---|---|---|
| OB-49 | Executed with production evidence: EE-4 reconfiguration tests ported (T0 and T1); same `OutputId`; camera identity and value unchanged; tracker/view/membership rebuilt; F-RECONF-1; `wl_output`/`xdg_output` events; rejection of unknown ids and key changes; reconnect separate; p10–p13; **runtime 2i** with the sealed-rule control on the real output manager (T0 and T1) | **mechanically satisfied for every criterion that can execute in INC-2 — OPEN pending review.** Two named items cannot execute yet: "fullscreen configure" (protocol fullscreen is INC-7, INV-PRES-1) and "epoch settlement" (ScaleInteractionEpoch is INC-6). The review must decide whether they transfer to INC-7/INC-6 obligations or block closure (finding INC2-F01). |
| OB-19 runtime counterpart | 2i executes at runtime (above) | evidence for the review; OB-19 stays CLOSED |
| OB-45 | tracker, membership and damage part (EE-1 C1–C4 and the sealed control, rotated membership, production damage) | part satisfied; OPEN (hit-test part INC-3) |
| OB-50 | core part: zero-output state, no fabrication, first connect leaves it | part satisfied; OPEN (pointer part INC-3, constraint part INC-5) |
| OB-3 | removal through `output_disconnected`; pointer placed on a remaining output | removal part without interactions satisfied; OPEN (interactions INC-3/INC-5) |
| OB-1, OB-6, OB-14, OB-17, OB-48 | untouched | OPEN |

## 5. Harness parity (FOUNDATION_TEST_PLAN §4)

| Harness test | INC-2 production port | Not ported here (owner) |
|---|---|---|
| 2R output-lifecycle tests (TST-15) | all 14 behaviours, T0 (+ T1 wire) | — |
| TST-03 SC1/SC3 | clock, simultaneous visibility, sliver, starve, re-entry, handover; visibility changes driven by output lifecycle and persisted camera placement | pan-driven visibility (INC-3 navigation); Throttle fallback (INC-8); scale columns (INC-6) |
| TST-03 X5 | 3 cases + `Window::output_enter` control | — |
| TST-06 x9 | presented / discarded / pending-then-presented | unobserved kick (INC-8) |
| EE-1 | tracker C1–C4 (10 cases) + sealed control, damage, rotated membership | C5 hit agreement (INC-3, OB-45) |

## 6. Findings

| ID | Level | Statement | Disposition |
|---|---|---|---|
| INC2-F01 | C (specification) | OB-49's item names "fullscreen configure" and "epoch settlement", which depend on INC-7 and INC-6 behaviour. OB-49 also "Blocks: INC-2 exit". Same shape as INC0-F04 (2i). | review decision |
| INC2-F02 | observation | D-B6-BOUNDARY replaces INC-1's single-output popup target: the anchor output is the parent's clock output. Rule 1 (input output) waits for INC-3; an unobserved parent gets no constraint (rule 3). | none |
| INC2-F03 | observation | `wayland-server` reuses a removed global's numeric name; tests identify a reconnected output by a new announcement after the removal, not by a fresh number. | none |
| INC2-F04 | observation | Output globals are removed immediately after their leaves (no disable-then-delay); clients see `global_remove` in order. Revisit with real clients (T3). | T3 |
| INC2-F05 | observation | The Arrangement (Level D) places each new output right of those present and never moves on later changes, so a reconfigured output may overlap its neighbour's Arrangement rect. | configuration (Level D) |
| INC2-F06 | observation | The damage tracker's scale is `k_O = zoom · scale`; INC-2 rebuilds it on output changes. Camera zoom changes (INC-10) must also rebuild it (F-RENDER-1). | INC-10 |

**Open findings:** Level A 0, Level B 0, Level C 2 (INC1-F01 owned by INC-7; INC2-F01 needs a review decision).
