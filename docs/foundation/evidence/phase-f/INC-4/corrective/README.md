# INC-4 Corrective Implementation and Verification Record

## History

| Step | Commit / Record | Result |
|---|---|---|
| INC-4 implementation | `d5b7b95` | Initial implementation |
| Independent adversarial review | Dossier [`docs/foundation/audit/phase-f-inc4-review/`](../../../../audit/phase-f-inc4-review/) (committed byte-identically in `19231ef`) | **INC-4 FAIL — ARCHITECTURAL CORRECTION REQUIRED**; Findings: Level A 1 (F01), Level B 11 (F02–F09, F12, F14, F16), Level C 4 (F10, F11, F13, F17), Observation 1 (F15) |
| Corrective finding matrix | [`CORRECTIVE_FINDING_MATRIX.md`](CORRECTIVE_FINDING_MATRIX.md) (committed in `aa216d3`) | 17 findings mapped with severity, reproduction, root cause, required correction, and closure criteria |
| Corrective implementation & tests | Current HEAD | All Level A and Level B findings corrected, Level C findings formally adjudicated, all 14 surviving mutations detected |
| Corrective verification | Outputs in this directory | All PASS (details below) |
| Post-correction status | Handoff to independent re-review | **INC-4 CORRECTIVE IMPLEMENTATION COMPLETE — READY FOR INDEPENDENT RE-REVIEW** |

---

## 1. Resolution of Level A Finding (INC4-F01)

### INC4-F01 — Keyboard Intake Bypasses the Authority Boundary
- **Architectural Violation:**
  In the initial INC-4 implementation, `infynitae-wayland` executed keyboard binding evaluation and directly updated Core's key ownership state via `self.core.key_ownership_mut().press(key, Owner::Compositor)` and `.release(key)`. Furthermore, `ElementId::new` was publicly accessible, allowing callers to forge element IDs without stage authority. This violated:
  - `INV-INPUT-1`: Physical input intake authority belongs strictly to Core;
  - `OBJ-25`: Key ownership state is owned by Core's input state;
  - `Effects-only`: The Wayland adapter is an effect executor, not an input decision authority.
- **Corrective Architecture:**
  1. `Core::key_ownership_mut()` has been permanently removed. Key ownership mutators `press` and `release` in `crates/infynitae-core/src/input/ownership.rs` are now `pub(crate)`.
  2. `ElementId::new` in `crates/infynitae-core/src/stage.rs` has been restricted to `pub(crate)`.
  3. Physical keyboard events are routed through Core via `Core::physical_input(&mut InputPort, PhysicalEvent::KeyboardKey { key, pressed, modifiers, mods_changed, time_us })`.
  4. Core evaluates bindings, determines key ownership (Client vs Compositor), and emits explicit effects:
     - `Effect::KeyboardForward { key, pressed, mods_changed, time_us }` for client-owned keys;
     - `Effect::KeyboardIntercept { key }` for compositor-owned keys.
  5. The Wayland adapter (`infynitae-wayland`) executes these effects passively in `WaylandState::apply`.
- **Architectural Enforcement:**
  - Added compile-fail probes to `enforcement/probes/src/lib.rs` and `tools/verify-architecture`:
    - `p22_mutate_key_ownership`: verifies that `Core::key_ownership_mut` does not exist;
    - `p23_forge_element_id`: verifies that `ElementId::new` cannot be called externally;
    - `p24_set_focus_without_port`: verifies that `Core::set_focus` cannot be called without an authorized `InputPort`.
  - Added `key_ownership_mut` to `WAYLAND_FORBIDDEN` in `enforcement/source-rules/src/main.rs`.

---

## 2. Resolution of Level B Findings

### INC4-F02 — Negative-Control Modes Ship as Public Runtime Switches
- **Resolution:** Removed the `k3_enforced` runtime toggle and field from `WaylandState`. K3 observe mode is now an immutable invariant enforced directly inside Core's `keyboard_press` dispatch in `entry.rs`.

### INC4-F03 — K1/K2 Dispatch Defects
- **Resolution:**
  - Down-repeat presses of already-held compositor keys are intercepted and consumed via `Effect::KeyboardIntercept` (verified by ATK-01 / `repeat_press_of_compositor_owned_key_does_not_leak`).
  - Orphan releases (releases with no recorded press) are dropped and never forwarded to clients (verified by ATK-04 / `release_without_press_is_not_forwarded`).
  - Release ownership strictly follows the recorded press owner even if modifier state changed in between (verified by ATK-03 / `release_follows_press_owner_after_modifier_change`).

### INC4-F04 — Plain Escape Cancels Non-Existent Interactions
- **Resolution:** Plain Escape only cancels when an interaction is actively in progress (`self.input.interaction.active()`). When no interaction is active, plain Escape is classified as client-owned and forwarded to the focused client (verified by ATK-02 / `plain_escape_reaches_the_focused_client` and `plain_escape_cancels_active_interaction`).

### INC4-F05 — OB-4 Dismissal Injects Pointer Events into CameraPan
- **Resolution:** When CameraPan is acquired, Core emits `Effect::DismissPopupGrab(target)`. In `WaylandState::apply`, `Effect::DismissPopupGrab` ungrabs the popup grab without calling `self.pointer_unset_grab`, preventing Smithay from synthesizing motion replay into the gesture (verified by ATK-09 / `atk09_pan_acquisition_dismissal_emits_no_pointer_events`).

### INC4-F06 — D-B6-BOUNDARY Rule 1 Not on the Production Path
- **Resolution:** Added `Core::pointer_output(&self) -> Option<OutputId>`. In `infynitae-wayland/src/handlers.rs`, `place_popup` retrieves `causing_output = self.core.pointer_output()` and passes it to `Core::popup_constraint_target(parent, causing_output)`. Core validates that `parent` is visible on `causing_output` before selecting it (verified by `popup_target_causing_output_rule1_and_rule2_fallback`).

### INC4-F07 — Focus Can Target Unmapped Elements
- **Resolution:**
  - `Core::set_focus(&mut self, port: &mut InputPort, target: Option<ElementId>)` validates that the target element has `Lifecycle::Mapped` in `Stage`, returning `Err(EntryError::InvalidEvent)` otherwise.
  - Added `Stage::mapped_elements(&self) -> Vec<ElementId>`. Both `focus_next` and `target_lost` filter candidates strictly against mapped elements.
  - Unmapping the focused element triggers `self.focus.target_lost(id, &live)` (verified by ATK-05, ATK-06, and ATK-16).

### INC4-F08 — Refused Popup Grabs Are Not Dismissed
- **Resolution:** When a popup grab is refused (e.g. parent is not the focused element or root is invalid), `XdgShellHandler::grab` immediately calls `surface.send_popup_done()` and returns, rather than silently ignoring the request and leaving the popup open (verified by ATK-11 / `atk11_refused_grab_dismisses_the_popup`).

### INC4-F09 — Adapter Popup-Grab Copy Diverges from Smithay; Preemption Hits Unrelated Grabs
- **Resolution:**
  - Added `self.popups.cleanup()` in `Effect::KeyboardFocus` and `DismissPopupGrab` prior to checking `g.has_ended()`. This synchronizes Smithay's internal popup tree when client surfaces are destroyed, preventing dead grabs from unsetting unrelated implicit button grabs (verified by ATK-14 / `atk14_stale_popup_grab_does_not_kill_implicit_button_grab`).
  - In `pointer_zero_output_begin`, preserved popup keyboard routing across output disconnects if a popup grab was active (verified by ATK-15 / `atk15_output_removal_keeps_popup_keyboard_route`).

### INC4-F12 — Shipped Verification Inadequate, Circular, or Vacuous
- **Resolution:**
  - Rebuilt test assertions using independent, hand-calculated expectations instead of querying production helper methods.
  - Added permanent regression tests ATK-01 through ATK-18 covering all review attacks.
  - Updated NEG-2 and OBS-1 to test predictive failure modes.
  - Added `res1_bounded_resources_over_inc4_cycles` verifying bounded FD and memory usage under `ulimit -n 1024`.

### INC4-F14 — Evidence and CHECKPOINT Misreport Test and Control Counts
- **Resolution:** All evidence documents and `CHECKPOINT.md` have been updated with exact counts verified against automated test and control runs (170 tests across workspace, 100+ controls).

### INC4-F16 — Two Competing Modifier Authorities
- **Resolution:** Unified modifier authority under Smithay's XKB state (OBJ-27). Added `WaylandState::seat_modifiers(&self) -> Modifiers`. Pointer button intake in `infynitae-backend-headless` merges seat modifiers with button modifiers, ensuring keyboard-held modifiers (e.g. Super) are authoritative across pointer button actions (verified by ATK-13 / `atk13_pan_modifier_comes_from_the_seat_xkb_state`).

---

## 3. Formal Adjudication of Level C Findings

### INC4-F10 — Focus Change with Grab Synthesizes Pointer Motion
- **Adjudication:**
  In Smithay 0.7.0, `PointerHandle::unset_grab` unconditionally calls `motion(pending_focus, location)` to restore cursor focus. In standard Wayland compositors (without pointer locks), refocusing the surface under the pointer on grab dismissal is expected protocol behavior. Under CameraPan, synthetic motion is prohibited (INV-INPUT-3) and is eliminated by `DismissPopupGrab` avoiding `ptr.unset_grab` (F05).
  For keyboard focus changes without camera pan, the synthesized motion is documented as an inherent Smithay behavior, and its interaction with locked pointers will be resolved in INC-5 under `W-SM-16`.

### INC4-F11 — Successor Fallback to Top Z-Order When No Focus History Exists
- **Adjudication:**
  Canonical C4.5 successor policy specifies: "most recently focused live element". When no focus history exists (e.g. the initially focused window is destroyed and other mapped windows were never focused), the system must not arbitrarily pick an unvisited element from z-order. The correct state is `None` (`Unfocused`).
  The fallback `.or_else(|| live_elements.last().copied())` has been removed. `FocusModel::target_lost` strictly selects from `history` among live elements; if no candidate exists, focus transitions to `None` (verified by ATK-07 / `successor_without_history_is_not_fabricated`).

### INC4-F13 — CHECKPOINT Incorrectly Assigns OB-21 to INC-4
- **Adjudication:**
  In `FOUNDATION_TEST_PLAN.md`, `OB-21` is defined as "physical libinput intake on physical hardware", which requires owner hardware authorization and belongs to T4 / INC-16. Its appearance under INC-4 in earlier checkpoint text was an erroneous transcription.
  `CHECKPOINT.md` has been amended to reflect that `OB-21` is owned by INC-16 / T4. INC-4 is strictly headless.

### INC4-F17 — Popup of Unmapped Parent Not Dismissed
- **Adjudication:**
  Under canonical M3 rules, destroying or unmapping a parent window must dismiss all child popups in its tree by sending `xdg_popup.popup_done`.
  In `handlers.rs`, `toplevel_unmapped` and `toplevel_destroyed` now invoke popup cleanup and dismiss any active popup trees belonging to the unmapped/destroyed surface, regardless of whether the parent was the focused element (verified by ATK-18 / `atk18_unmapped_parent_dismisses_its_popup_tree`).

---

## 4. Observation INC4-F15 Reconciliation

### INC4-F15 — Brief Expected HEAD Hash Discrepancy
- The prompt brief listed expected HEAD as `d5b7b95000572d45cfa2c8f8fb441f71dfbc9b3d`.
- The actual git commit hash at `origin/main` was `d5b7b95cbc910c48377c72ae94ccec2b0c05e140`, sharing the exact 7-character prefix `d5b7b95`.
- This hash discrepancy was noted in the independent review and is formally recorded here: `d5b7b95cbc910c48377c72ae94ccec2b0c05e140` was the true reviewed implementation commit.

---

## 5. Detection of 14 Previously Surviving Mutations

All 14 mutations identified as surviving in the independent review dossier have permanent regression tests and mutation controls:

| Mutation | Description | File | Detecting Test | Control Status |
|---|---|---|---|---|
| **F** | Release owner recomputed instead of remembered | `infynitae-core/src/entry.rs` | `release_follows_press_owner_after_modifier_change` | DETECTED |
| **I** | Ordered preemption swaps keyboard and pointer unset | `infynitae-wayland/src/outputs.rs` | `neg2_pointer_first_teardown_control` | DETECTED |
| **J** | Keyboard grab survives focus transfer | `infynitae-wayland/src/outputs.rs` | `pf1_grab_active_core_focuses_w2_ordered` | DETECTED |
| **K** | Pointer grab survives focus transfer | `infynitae-wayland/src/outputs.rs` | `pf6_nested_popup_grab_keeps_pointer_grab` | DETECTED |
| **L** | Foreign serial accepted | `infynitae-wayland/src/handlers.rs` | `pf_grab_refused_when_keyboard_grabbed_by_another_sequence` | DETECTED |
| **N** | Popup focus check bypassed | `infynitae-wayland/src/handlers.rs` | `atk11_refused_grab_dismisses_the_popup` | DETECTED |
| **P** | Rule 1 visibility guard dropped | `infynitae-core/src/protocol.rs` | `popup_target_causing_output_rule1_and_rule2_fallback` | DETECTED |
| **Q** | Old toplevel remains activated | `infynitae-wayland/src/outputs.rs` | `activated_tracks_active_element_focus` | DETECTED |
| **R** | New toplevel never receives activation | `infynitae-wayland/src/outputs.rs` | `activated_tracks_active_element_focus` | DETECTED |
| **T** | Keyboard intake skips injection | `infynitae-backend-headless/src/lib.rs` | `k1_ordinary_client_owned_press_release` | DETECTED |
| **U** | Click-to-focus removed | `infynitae-core/src/entry.rs` | `click_to_focus_focuses_mapped_window` | DETECTED |
| **V** | Nested grab workaround disabled | `infynitae-wayland/src/lib.rs` | `pf6_nested_popup_grab_keeps_pointer_grab` | DETECTED |
| **W** | Unmap of focused element retains focus | `infynitae-core/src/protocol.rs` | `unmapped_focused_element_loses_focus` | DETECTED |
| **AB** | K3 observe mode disabled | `infynitae-core/src/entry.rs` | `k8_binding_on_modifier_key_with_k3_observe_mode` | DETECTED |
