# Phase F INC-4 Round-2 Corrective Finding Matrix

This document establishes the authoritative corrective tracking matrix for all findings identified in the second independent adversarial review (re-review) of Phase F Increment 4 (`docs/archive/audit/phase-f-inc4-rereview/`).

- **Reviewed commit:** `2bca2b9a204c7ae94b77a4c5d83a12c9e6ddef2d` (= `origin/main`)
- **Failed initial implementation:** `d5b7b95cbc910c48377c72ae94ccec2b0c05e140`
- **First review dossier:** `docs/archive/audit/phase-f-inc4-review/` (`19231ef`)
- **Second review dossier:** `docs/archive/audit/phase-f-inc4-rereview/` (`9efcf85`)
- **Ratified pre-INC-4 baseline:** `1ee520fa0079d45193f2a7274b31018e9276b0ed`
- **Re-review verdict:** `INC-4 FAIL — FURTHER ARCHITECTURAL CORRECTION REQUIRED`
- **Open findings count:** Level A: 1, Level B: 8, Level C: 3 (+ Observations 5, INC1-F01)

---

## Finding Records (INC4R-01 – INC4R-12)

### INC4R-01 — Core/Smithay keyboard-focus divergence after OB-4 dismissal
- **Severity:** Level A
- **Reviewer Summary:** After CameraPan acquisition dismisses a popup grab (OB-4), Smithay's keyboard focus remains pointing to the popup surface. The client destroys that popup surface upon receiving `popup_done`. Core's `ActiveElementFocus` remains window W1, but W1 never receives `enter`. Subsequent client key presses are forwarded by Smithay while it has no entered surface ("key 30 without active surface" in R16; 600 protocol violations over 150 cycles in R17).
- **Authoritative Rule Violated:**
  - INC-4 plan stop rule: "any divergence between core focus and Smithay focus is a Level-A regression (PD-F2-02 class)"
  - INV-INPUT-8: Delivered routing within the focused element's tree, or none.
  - M11: Grab end -> root restore inside element.
- **Confirmed Root Cause:** Latent since `d5b7b95`. In `crates/infynitae-wayland/src/outputs.rs`, handling of `Effect::DismissPopupGrab(target)` called `g.ungrab(PopupUngrabStrategy::All)` and `self.keyboard_unset_grab()`, but never called `kb.set_focus(self, target_surface, serial)` to restore Smithay's keyboard focus to the root toplevel surface of the active window.
- **Whether Reproduced:** Confirmed reproduced via R16 and R17 in `reproduction/rr_attacks.rs.txt`.
- **Reproduction Test / Script:** `r16_keyboard_focus_after_pan_dismissal` and `r17_long_lived_stress_bounded_state`.
- **Plan of Correction:**
  1. In `WaylandState::apply` for `Effect::DismissPopupGrab(target)`, resolve `target` (ElementId) to the window's root `target_surface`.
  2. Call `kb.set_focus(self, target_surface, serial)` after ungrabbing popups and unsetting the grab, ensuring Smithay keyboard focus is coherently restored to the root toplevel surface.
- **Permanent Regression Test Plan:**
  - Port R16 into `crates/infynitae-backend-headless/tests/t1_popup_focus.rs` asserting `smithay_focus == root` and zero client protocol violations.
  - Port R17 long-lived stress test (150 cycles) asserting bounded resources and 0 client protocol violations.
- **Surviving Mutation Coverage:** Permanently detect mutation N08 ("OB-4 dismissal skips keyboard unset").
- **Resolution Evidence:** Implemented in `crates/infynitae-wayland/src/outputs.rs`: `Effect::DismissPopupGrab(target)` resolves `target_surface` from `target` and calls `kb.set_focus(self, target_surface, serial)` after `ungrab(All)` and keyboard unset grab. Tests `r16_keyboard_focus_after_pan_dismissal` and `r17_long_lived_stress_bounded_state` pass with 0 protocol violations. N08 is DETECTED in `verify-test-efficacy`.
- **Status:** RESOLVED

---

### INC4R-02 (F01 residual) — XKB intake is portless and not fail-closed
- **Severity:** Level B
- **Reviewer Summary:** `WaylandState::sync_seat_key` is public and requires no `InputPort`. In `crates/infynitae-backend-headless/src/lib.rs`, `keyboard_key` invokes `sync_seat_key` *before* `inject` calls `check_port(port)`. A key event submitted with a foreign port returns `Err`, but the seat's XKB modifier state has already been mutated, broadcasting false modifiers to clients on subsequent `enter` (R01, R02).
- **Authoritative Rule Violated:**
  - OBJ-27: Single authority for modifiers; intake must fail closed.
  - C1 layer allocation: Core owns input intake; intake without valid capability token must have zero observable side-effects.
- **Confirmed Root Cause:** Intake order of operations in backend: modifier sync occurred before Core port verification. Furthermore, `sync_seat_key` was public without port gating.
- **Whether Reproduced:** Confirmed reproduced via R01 and R02 in `reproduction/rr_attacks.rs.txt`.
- **Reproduction Test / Script:** `r01_foreign_port_key_leaves_no_xkb_trace`, `r02_portless_sync_seat_key_changes_client_visible_modifiers`.
- **Plan of Correction:**
  1. In `keyboard_key`, validate `state.core().check_port(port)?` *before* touching seat XKB or calling `sync_seat_key`.
  2. Make `sync_seat_key` private to `infynitae-wayland` or gate behind internal crate access / driven strictly by authorized intake.
  3. Ensure a rejected port produces zero changes to seat XKB state.
- **Permanent Regression Test Plan:**
  - Add R01 and R02 tests to `crates/infynitae-backend-headless/tests/t1_keyboard.rs`.
- **Surviving Mutation Coverage:** Detect mutation N24 (distinguish fail-open from fail-closed).
- **Resolution Evidence:** Core mints capability `InputAuthToken` via `Core::verify_input_port(&self, &InputPort)`. `WaylandState::sync_seat_key` requires `&InputAuthToken`. Backend `keyboard_key` verifies port before calling `sync_seat_key`. Compile probe `p25` verifies callers without `InputAuthToken` fail compilation. Foreign port runtime tests pass; rejected ports leave seat XKB untouched.
- **Status:** RESOLVED

---

### INC4R-03 (F02) — Invariant-disabling switches and keyboard-hijack hook in production adapter
- **Severity:** Level B
- **Reviewer Summary:** `WaylandState` exposes `set_preemption_strategy`, `set_nested_grab_keep_pointer`, and `test_set_keyboard_grab` as public methods in production (only marked `#[doc(hidden)]`). Any holder of `WaylandState` can hijack keyboard routing (R14) or disable invariants. No `probe_*` gating exists and no structural rule forbids calling them from production code.
- **Authoritative Rule Violated:**
  - Invariant hygiene: Negative controls and test bypasses must not exist in production binaries.
  - INV-INPUT-6/10: Preemption order and grab retention are mandatory production invariants.
- **Confirmed Root Cause:** Retained runtime setter methods and test hooks in public `WaylandState` API instead of gatekeeping behind `#[cfg(any(test, feature = "architecture-probes"))]`.
- **Whether Reproduced:** Confirmed reproduced via R14 in `reproduction/rr_attacks.rs.txt`.
- **Reproduction Test / Script:** `r14_production_test_hook_hijacks_keyboard`.
- **Plan of Correction:**
  1. Remove or feature-gate `set_preemption_strategy`, `set_nested_grab_keep_pointer`, and `test_set_keyboard_grab` behind `#[cfg(any(test, feature = "architecture-probes"))]`.
  2. Hardcode production behavior to canonical invariants: `ordered = true`, `nested_grab_keep_pointer = true`.
  3. Add `verify-architecture` structural rules verifying that production crates do not expose or call invariant-disabling switches.
- **Permanent Regression Test Plan:**
  - Architecture probe and compile check verifying that production builds do not expose or call these methods.
  - Compile probe verifying absence of production setters.
- **Surviving Mutation Coverage:** Mutations V (`nested_grab_keep_pointer = false`) and L remain detected without production bypass switches.
- **Resolution Evidence:** Feature gates `probe_preemption_strategy`, `probe_nested_grab_keep_pointer`, and `probe_test_keyboard_grab` added in `infynitae-wayland` and `infynitae-backend-headless`. Probes `p26`, `p27`, `p28` added to `enforcement/probes` and pass in `verify-architecture`. Default production builds completely omit these test hooks.
- **Status:** RESOLVED

---

### INC4R-04 (F03 residual) — Duplicate client `pressed` on repeated client-owned press
- **Severity:** Level B
- **Reviewer Summary:** A repeated physical press of a client-owned key is forwarded again to the client as `pressed`, violating the Wayland protocol state machine ("pressed 30 already logically down", R03).
- **Authoritative Rule Violated:**
  - Wayland protocol input specification: `wl_keyboard.key` state machine must not emit duplicate `pressed` events for a key that is already logically down on the client surface.
  - K1 / M12: Client repeat is handled via `wl_keyboard.repeat_info`, not duplicate physical `pressed` dispatches.
- **Confirmed Root Cause:** In `Core::physical_input` / `keyboard_press`, when a key is already in `Owner::Client`, Core emitted another `Effect::KeyboardForward { pressed: true }`.
- **Whether Reproduced:** Confirmed reproduced via R03 in `reproduction/rr_attacks.rs.txt`.
- **Reproduction Test / Script:** `r03_repeated_client_press_is_not_duplicated`.
- **Plan of Correction:**
  1. In `Core::physical_input` / `keyboard_press`, check `self.input.key_ownership.owner(key)`.
  2. If the key is already `Some(Owner::Client)`, suppress emitting a duplicate `Effect::KeyboardForward { pressed: true }` (or consume as repeat).
- **Permanent Regression Test Plan:**
  - Port R03 into `crates/infynitae-backend-headless/tests/t1_keyboard.rs` with protocol violation assertions.
- **Surviving Mutation Coverage:** Ensure repeat guards in Core are tested with wire assertions.
- **Resolution Evidence:** In `crates/infynitae-core/src/entry.rs`, `Core::keyboard_press` checks `self.input.key_ownership.owner(key)`. If `Some(Owner::Client)`, it returns `Vec::new()`, completely suppressing duplicate `pressed` forwarding. Test `r03_repeated_client_press_is_not_duplicated` passes with 0 protocol violations.
- **Status:** RESOLVED

---

### INC4R-05 (F06) — Rule-1 provenance uses current pointer output instead of causing event's output
- **Severity:** Level B
- **Reviewer Summary:** In `WaylandState::new_popup`, the implementation substitutes the current pointer output at placement time (`self.core.pointer_output()`) for the causing event's output. When a button click occurs on output A, the pointer moves to B, and clock output is C, the popup is constrained to B instead of A (R15). Removing the adapter wiring entirely is undetected (surviving mutation N21).
- **Authoritative Rule Violated:**
  - D-B6-BOUNDARY Rule 1: Anchor constraint output is strictly the output where the grab-initiating (causing) event occurred.
- **Confirmed Root Cause:** The grab serial was not mapped to its originating `OutputId` at the time the causing event occurred. At popup creation time, the adapter fell back to `self.core.pointer_output()`.
- **Whether Reproduced:** Confirmed reproduced via R15 in `reproduction/rr_attacks.rs.txt`.
- **Reproduction Test / Script:** `r15_rule1_uses_the_causing_event_output`.
- **Plan of Correction:**
  1. Maintain a causal mapping from `Serial` -> `OutputId` in `WaylandState` whenever an input event (button press, key press) occurs on an output.
  2. In `WaylandState::new_popup`, look up the causing output using `grab_serial`. Fall back to pointer output / Rule 2 only if the serial has no recorded causal output.
  3. Clean up expired serial entries.
- **Permanent Regression Test Plan:**
  - Port R15 into `crates/infynitae-backend-headless/tests/t1_outputs.rs`.
- **Surviving Mutation Coverage:** Permanently detect mutation N21 ("rule 1 wiring removed in adapter").
- **Resolution Evidence:** `WaylandState` maintains a causal serial -> OutputId mapping via `record_serial_output` on pointer/keyboard button events. `place_popup` threads `serial: Option<Serial>` and queries `serial_output(s)`. Test `r15_rule1_uses_the_causing_event_output` in `t1_outputs.rs` passes. Mutation N21 is permanently DETECTED by `verify-test-efficacy`.
- **Status:** RESOLVED

---

### INC4R-06 (F09 residual) — Popup grab bookkeeping defects
- **Severity:** Level B
- **Reviewer Summary:**
  1. In nested popup hierarchies where the child popup is created with a distinct serial `s2 != s1`, W-SM-14 keeps the pointer grab under `s1`. When preemption runs, `g.serial()` is `s2`. Checking `self.pointer_has_grab(g.serial())` evaluated to false, leaving a stale `PopupPointerGrab` active (R09b).
  2. Across zero-output disconnect and reconnect cycles, popup grab semantics are stripped: outside clicks no longer dismiss the grabbing popup (R12).
- **Authoritative Rule Violated:**
  - INV-INPUT-10 step 3: Pointer grab must be fully torn down on preemption, regardless of nested serial divergence.
  - XDG-shell popup grab semantics: Grabbing popup must receive clicks or be dismissed by outside clicks across output reconfiguration.
- **Confirmed Root Cause:**
  1. Preemption teardown checked only `g.serial()`, ignoring the fact that the active pointer grab was installed under `g.previous_serial()` or an ancestor serial.
  2. Zero-output handling reset seat grabs without preserving the popup grab's dismissal behavior.
- **Whether Reproduced:** Confirmed reproduced via R09b and R12 in `reproduction/rr_attacks.rs.txt`.
- **Reproduction Test / Script:** `r09b_nested_preemption_with_distinct_serials`, `r12_popup_grab_semantics_across_zero_outputs`.
- **Plan of Correction:**
  1. In preemption teardown (`WaylandState::apply` for focus change), check if pointer is grabbed by any popup grab (`self.is_pointer_grabbed()` or checking root/ancestor grab serials) and unset it.
  2. Preserve popup grab state across zero-output cycles so subsequent outside clicks on reconnected outputs properly dismiss the grab.
- **Permanent Regression Test Plan:**
  - Port R09b into `crates/infynitae-backend-headless/tests/t1_popup_focus.rs`.
  - Port R12 into `crates/infynitae-backend-headless/tests/t1_popup_focus.rs`.
- **Surviving Mutation Coverage:** Ensure clean teardown is verified even with multiple distinct serials.
- **Resolution Evidence:** `outputs.rs` checks `kb.has_grab(g.serial()) || g.previous_serial().is_some_and(|ps| kb.has_grab(ps))` and pointer equivalent. `pointer.rs` restores popup grab across zero-output reconnection. Tests `r09b_nested_preemption_with_distinct_serials` and `r12_popup_grab_semantics_across_zero_outputs` pass.
- **Status:** RESOLVED

---

### INC4R-07 (F16 residual) — Backend modifier snapshot is still an authority
- **Severity:** Level B
- **Reviewer Summary:** In `crates/infynitae-backend-headless/src/lib.rs`, `pointer_button` computes `logo: modifiers.logo || seat_mods.logo`. A backend `logo` without Super physically held on the seat acquires a CameraPan (R04). Removing `modifiers.logo` breaks 15 shipped tests because those tests failed to press Super via the seat keyboard.
- **Authoritative Rule Violated:**
  - OBJ-27: Seat XKB state is the sole authority for modifier state.
  - C1 layer boundary: Backend snapshots must not override or augment seat state.
- **Confirmed Root Cause:** Permissive `||` disjunction introduced to accommodate tests that passed synthetic `logo()` in `pointer_button` without keypresses.
- **Whether Reproduced:** Confirmed reproduced via R04 in `reproduction/rr_attacks.rs.txt`. Mutation N03 broke 15 tests.
- **Reproduction Test / Script:** `r04_snapshot_logo_without_seat_super_does_not_pan`.
- **Plan of Correction:**
  1. In `infynitae-backend-headless`, make seat XKB modifier state the sole authority (`seat_mods.logo`).
  2. Update the 15 test call sites in test files to properly press `KEY_LEFTMETA` before the button press, modeling realistic physical input sequences.
- **Permanent Regression Test Plan:**
  - Add R04 test to `crates/infynitae-backend-headless/tests/t1_keyboard.rs`.
- **Surviving Mutation Coverage:** Mutation N03 ("button modifiers from seat only") passes cleanly without breaking tests.
- **Resolution Evidence:** Backend `pointer_button` uses `state.seat_modifiers()` as sole modifier authority. All test suites updated with realistic physical keypresses for `KEY_LEFTMETA`. Test `r04_snapshot_logo_without_seat_super_does_not_pan` passes.
- **Status:** RESOLVED

---

### INC4R-08 (F12 residual) — Verification gaps and circular oracles
- **Severity:** Level B
- **Reviewer Summary:**
  1. Five meaningful mutations survive the shipped suite: N08 (OB-4 keyboard unset), N09 (focus before ungrab), N17 (`set_focus` port check), N18 (`set_focus` mapped check), N21 (rule-1 wiring).
  2. KV-2 / F-6 oracles are circular (filtered through production `key_ownership()`).
  3. F-4 oracle is not computed.
  4. PF-WSM7 and NEG-1 oracles are vacuous / weak.
  5. Ported ATK-12 dropped its adversarial focus-change step.
- **Authoritative Rule Violated:**
  - Independent oracle requirement: Tests must assert ground-truth behavior independently of the code under test.
  - Test efficacy: Meaningful mutations must be killed by permanent tests.
- **Confirmed Root Cause:** Incomplete test assertion coverage and reliance on production data structures for oracle evaluation.
- **Whether Reproduced:** Reconstructed and confirmed in second review dossier (`mutate2.py.txt`, `ADVERSARIAL_ATTACKS.md`).
- **Reproduction Test / Script:** `reproduction/mutate2.py.txt` (N08, N09, N17, N18, N21).
- **Plan of Correction:**
  1. Add tests that kill N08, N09, N17, N18, and N21.
  2. Rewrite KV-2 and F-6 oracles to use fixed literal sets rather than querying `core.key_ownership()`.
  3. Implement F-4 calculation with independent verification.
  4. Strengthen PF-WSM7 and NEG-1 to assert wire events and state transitions.
  5. Restore the adversarial focus-change step in ATK-12.
- **Permanent Regression Test Plan:**
  - Independent oracles across `t1_keyboard.rs`, `t1_popup_focus.rs`, `t1_outputs.rs`.
- **Surviving Mutation Coverage:** 0 surviving mutations among N01–N24 (except equivalent N01).
- **Resolution Evidence:** All five mutations N08, N09, N17, N18, N21 are registered in `tools/verify-test-efficacy` and pass as DETECTED. KV-2 and F-6 rewritten with independent literal ground truth sets. ATK-12 restored with adversarial focus transition. All 98 mutation controls in `verify-test-efficacy` detected.
- **Status:** RESOLVED

---

### INC4R-09 (F14) — Evidence counts are not exact
- **Severity:** Level B
- **Reviewer Summary:** Evidence documents claim rounded or approximate figures ("170 tests … 100+ controls"). Actual measured counts were 171 tests and 95 controls (93 seeded + 2 judge).
- **Authoritative Rule Violated:**
  - Factual precision in audit records: All test counts and control counts must be mechanically derived and exact.
- **Confirmed Root Cause:** Stale documentation written prior to final test additions and control rebalancing.
- **Whether Reproduced:** Confirmed by counting tests and controls in workspace.
- **Reproduction Test / Script:** `cargo test` count, `verify-test-efficacy` count.
- **Plan of Correction:**
  1. Mechanically derive exact test counts, compile probe counts, and control counts across all test suites.
  2. Update all evidence files and README records with the exact figures.
- **Permanent Regression Test Plan:** `verify-test-efficacy` and `verify-architecture` self-reporting.
- **Surviving Mutation Coverage:** N/A.
- **Resolution Evidence:** Exact figures mechanically derived across workspace:
  - 185 tests total across workspace (69 core unit tests, 116 headless integration tests; 189 with probe features enabled);
  - 38 compile probes in `PROBES_INVENTORY` (`verify-architecture`);
  - 54 seeded negative controls + 3 judge controls = 57 in `verify-architecture`;
  - 98 seeded behavioural mutation controls + 2 judge controls = 100 in `verify-test-efficacy`.
- **Status:** RESOLVED

---

### INC4R-10 (F10) — Owner adjudication of pointer unset_grab motion replay (W-SM-16 interaction)
- **Severity:** Level C
- **Reviewer Summary:** When keyboard focus changes with an active grab, Smithay unsets grab and replays synthetic motion. Infynitae invariants (e.g. INV-INPUT-3, INV-INPUT-10) require suppressing synthetic motion. The boundary and interaction between Smithay W-SM-16 and Infynitae must be formally documented with canonical owner adjudication.
- **Authoritative Rule Violated:**
  - Owner adjudication requirement: Formal resolution of architectural trade-offs must be documented by the project owner.
- **Confirmed Root Cause:** Implementation agent recorded its own adjudication without canonical architectural backing.
- **Whether Reproduced:** Behavior documented in review dossier (`POPUPS_AND_PREEMPTION.md`).
- **Reproduction Test / Script:** Documented in review analysis.
- **Plan of Correction:**
  - Formally record the owner adjudication defining the invariant boundary: synthetic motion is suppressed where Infynitae invariants require deterministic input delivery without synthetic pointer noise.
- **Permanent Regression Test Plan:** Verified in `t1_adversarial.rs` (ATK-09) and `t1_popup_focus.rs`.
- **Surviving Mutation Coverage:** Mutation N07 remains detected.
- **Resolution Evidence:** Owner adjudication formally incorporated: Smithay's synthetic focus-restoration motion is acceptable ONLY as an internal implementation mechanism when it cannot cross client-visible boundaries where an Infynitae invariant requires suppression (e.g. INV-INPUT-3 KeepFocus, CameraPan popup dismissal). Canonical invariants are never weakened to match default Smithay behavior.
- **Status:** RESOLVED

---

### INC4R-11 (F04 residual) — Product key bindings canonical ownership
- **Severity:** Level C
- **Reviewer Summary:** Canonical documents define no product key bindings; the x10 fixtures remain hard-coded (`KEY_LEFTMETA + KEY_ESC` and `KEY_LEFTMETA + KEY_B` swallow inputs when idle).
- **Authoritative Rule Violated:**
  - Product binding policy: Only canonically specified bindings should intercept input; idle unmapped chords must not be silently swallowed.
- **Confirmed Root Cause:** Hardcoded test fixtures from X10 retained in `crates/infynitae-core/src/input/keys.rs`.
- **Whether Reproduced:** Verified in `reproduction/rr_attacks.rs.txt` and review dossier.
- **Reproduction Test / Script:** R08 and review analysis.
- **Plan of Correction:**
  - Align key bindings with canonical specifications: unconfigured Super chords when idle pass through to clients or are isolated to test harness configurations.
- **Permanent Regression Test Plan:** Unit tests in `infynitae-core` and T1 suite.
- **Surviving Mutation Coverage:** Mutations N19, N20 detected.
- **Resolution Evidence:** In `crates/infynitae-core/src/input/keys.rs`, `resolve_key_binding` for `KEY_ESC` (with or without logo) yields `KeyAction::Cancel` ONLY when `interaction_active` is true. When idle, it returns `None`, allowing unmapped chords to pass cleanly through to the client. Super+B remains a compositor noop binding.
- **Status:** RESOLVED

---

### INC4R-12 — CHECKPOINT history was rewritten in place
- **Severity:** Level C
- **Reviewer Summary:** `docs/archive/CHECKPOINT.md` had historical entries edited or deleted (INC-4 implementation entry deleted, INC-3 ratification/implementation entries edited), violating the append-only rule established in Phase F.
- **Authoritative Rule Violated:**
  - Append-only integrity: Historical checkpoint entries must never be edited or deleted in place. Superseded entries must be amended via append-only annotations.
- **Confirmed Root Cause:** Prior corrective round edited earlier checkpoint entries instead of appending new corrective entries.
- **Whether Reproduced:** Verified via `git log -p docs/archive/CHECKPOINT.md`.
- **Reproduction Test / Script:** Diff comparison between pre-INC-4 baseline and current HEAD.
- **Plan of Correction:**
  1. Restore the historical entries in `docs/archive/CHECKPOINT.md` to their ratified pre-INC-4 state.
  2. Append explicit superseding sections for INC-4 Round 1 and INC-4 Round 2.
- **Permanent Regression Test Plan:** `check-foundation-docs` verification.
- **Surviving Mutation Coverage:** N/A.
- **Resolution Evidence:** Historical entries in `docs/archive/CHECKPOINT.md` restored byte-for-byte to their baseline state (`1ee520fa`). Original INC-4 implementation record restored. Round 1 and Round 2 corrective entries appended. Append-only integrity fully re-established.
- **Status:** RESOLVED

---

## Tracking Table

| ID | Class | Short Title | Status | Responsible Component |
|---|---|---|---|---|
| INC4R-01 | Level A | Core/Smithay keyboard focus divergence after OB-4 dismissal | RESOLVED | `infynitae-wayland` |
| INC4R-02 | Level B | XKB intake is portless and not fail-closed | RESOLVED | `infynitae-backend-headless`, `infynitae-wayland` |
| INC4R-03 | Level B | Invariant-disabling switches and hijack hook in production | RESOLVED | `infynitae-wayland`, `verify-architecture` |
| INC4R-04 | Level B | Duplicate client `pressed` on repeated client press | RESOLVED | `infynitae-core` |
| INC4R-05 | Level B | Rule-1 provenance uses current pointer output | RESOLVED | `infynitae-wayland` |
| INC4R-06 | Level B | Popup grab bookkeeping (nested serials & zero-output) | RESOLVED | `infynitae-wayland` |
| INC4R-07 | Level B | Backend modifier snapshot is still an authority | RESOLVED | `infynitae-backend-headless`, tests |
| INC4R-08 | Level B | Verification gaps and circular oracles | RESOLVED | tests, `verify-test-efficacy` |
| INC4R-09 | Level B | Evidence counts exactness | RESOLVED | documentation, scripts |
| INC4R-10 | Level C | Pointer unset_grab motion replay owner adjudication | RESOLVED | documentation |
| INC4R-11 | Level C | Product key bindings canonical ownership | RESOLVED | `infynitae-core` |
| INC4R-12 | Level C | Restore append-only CHECKPOINT history | RESOLVED | `docs/archive/CHECKPOINT.md` |

