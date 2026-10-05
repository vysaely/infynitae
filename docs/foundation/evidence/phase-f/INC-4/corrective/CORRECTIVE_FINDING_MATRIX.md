# Phase F INC-4 Corrective Finding Matrix

This document establishes the authoritative corrective mapping for all findings identified in the independent adversarial review of Phase F Increment 4 (`docs/foundation/audit/phase-f-inc4-review/`).

- **Reviewed commit:** `d5b7b95cbc910c48377c72ae94ccec2b0c05e140`
- **Ratified pre-INC-4 baseline:** `1ee520fa0079d45193f2a7274b31018e9276b0ed`
- **Review verdict:** `INC-4 FAIL — ARCHITECTURAL CORRECTION REQUIRED`
- **Audit preservation commit:** `19231ef` (`docs(audit): record failed Phase F INC-4 independent review`)

---

## Finding Records (F01 – F17)

### INC4-F01 — Keyboard intake bypasses the authority boundary
- **Severity:** Level A
- **Exact observed failure:** Physical keys bypass `Core::physical_input(&mut InputPort, PhysicalEvent)`. Backend invokes public, portless `WaylandState::keyboard_key` after an advisory `Core::check_port`. Wayland adapter decides K1/K3 ownership and writes core-owned OBJ-25 (`key_ownership`) directly via `Core::key_ownership_mut()` and now-public `Ownership::press/release`. Focus mutators `Core::set_focus`, `Core::focus_next`, and `WaylandState::set_focus` are public without capability proofs. `ElementId::new` is `pub const`. Mutation T (backend skipping port check) survived. No `verify-architecture` rule enforced keyboard authority.
- **Reproduction:** Backend can omit `check_port` or supply an unauthenticated port; adapter directly manipulates core key ownership table without core entry points; focus can be mutated from any call site.
- **Root Cause:** Inversion of authority: keyboard intake was implemented inside the Wayland protocol adapter rather than through `Core::physical_input` with an `InputPort` capability token.
- **Canonical invariant/rule:** C1 layer allocation (Core owns input intake, bindings, ownership table; backend submits `PhysicalEvent` with `InputPort`; adapter applies forward/intercept effects only); OBJ-25 ("Forbidden writers: Smithay filter (executes only)"); INV-INPUT-1 (owner core intake).
- **Owner:** Core (`crates/infynitae-core/src/entry.rs`, `crates/infynitae-core/src/input/mod.rs`), Headless Backend (`crates/infynitae-backend-headless/src/lib.rs`), Architecture enforcement (`tools/verify-architecture`).
- **Required correction:**
  1. Add `PhysicalEvent::KeyboardKey { key: u32, pressed: bool }` (or with seat modifier sync) to `PhysicalEvent`.
  2. Require `&mut InputPort` for keyboard intake through `Core::physical_input`.
  3. Move binding resolution, observe mode evaluation, and ownership table writes into Core intake.
  4. Core returns `Vec<Effect>` containing `Effect::KeyboardForward` or `Effect::KeyboardIntercept` (consumed), plus focus/activation effects.
  5. Wayland adapter executes protocol effects only; remove `Core::key_ownership_mut()`; make `Ownership::press/release` crate-private to core.
  6. Make `Core::set_focus`, `Core::focus_next` crate-private (or port-gated), remove public `WaylandState::set_focus`.
  7. Make `ElementId::new` crate-private (constructible only by Stage).
  8. Add `verify-architecture` structural rules and compile probes forbidding adapter writes to `key_ownership` and portless keyboard intake.
- **Required regression test:**
  - Negative runtime test: intake with invalid/foreign `InputPort` fails closed.
  - Compile probe: adapter cannot call `key_ownership_mut` or mutate ownership.
- **Required mutation/control:** Mutation T (backend skipping port check) must be DETECTED.
- **Closure criterion:** Keyboard intake requires `&mut InputPort`; Core is sole authority and writer of OBJ-25; adapter executes effects only; verify-architecture structural checks pass.

---

### INC4-F02 — Negative-control modes ship as public runtime switches
- **Severity:** Level B
- **Exact observed failure:** `WaylandState` exposed public runtime setter methods (`set_k3_enforced`, `set_preemption_strategy`, `set_nested_grab_keep_pointer`) which disable INV-INPUT-6/10 and W-SM-14 in production. Tests manually enabled these flags, leaving production defaults unprotected (mutations V and AB survived).
- **Reproduction:** Call setters at runtime in production to disable preemption or nested grab workarounds.
- **Root Cause:** Test negative controls were implemented as mutable fields in production state structs rather than gated by test-only compilation features.
- **Canonical invariant/rule:** INC-3 convention: negative-control modes gated behind `probe_*` Cargo features with hygiene checks; production default must never silently disable an invariant.
- **Owner:** `infynitae-wayland` (`src/lib.rs`, `src/keyboard.rs`, `src/outputs.rs`, `src/handlers.rs`).
- **Required correction:**
  1. Remove public runtime setters (`set_k3_enforced`, `set_preemption_strategy`, `set_nested_grab_keep_pointer`).
  2. Production defaults are hardcoded to enforce invariants: `k3_enforced = true`, `ordered = true`, `nested_grab_keep_pointer = true`.
  3. Gate negative control variants behind test-only features or internal harness fixtures.
- **Required regression test:** Shipped tests run against canonical production defaults without needing manual setter calls.
- **Required mutation/control:** Mutations V (`nested_grab_keep_pointer = false`) and AB (`k3_enforced = false`) must be DETECTED by the test suite.
- **Closure criterion:** No public runtime setters that disable invariants in production; production defaults protected; mutations V and AB detected.

---

### INC4-F03 — K1/K2 dispatch defects
- **Severity:** Level B
- **Exact observed failure:**
  1. Repeated press of a compositor-owned key is forwarded to the client (ATK-02; `Ownership::press` returned `AlreadyPressed`, but adapter forwarded it).
  2. Release of a key with no recorded press is forwarded to the client (ATK-04; Smithay `input_forward` forwards releases unconditionally, causing protocol error "released 30 not logically down").
- **Reproduction:**
  - ATK-02: Super+B pressed, Super released, B pressed again -> `(48, pressed)` delivered to client.
  - ATK-04: Send release for unpressed key -> client receives release and logs protocol violation.
- **Root Cause:**
  - M12 `Down -> press` transition ignored: repeated press of an already compositor-owned key was forwarded instead of consumed.
  - Release handling treated `None` (unrecorded press) the same as client-owned, forwarding orphan releases.
- **Canonical invariant/rule:** M12, K1 (compositor-owned key remains compositor-owned for entire physical lifetime including repeats), K2 (release follows press owner; release with no press must not be sent).
- **Owner:** `infynitae-core` (`src/input/ownership.rs`, `src/entry.rs`).
- **Required correction:**
  1. In Core intake: if key is already held by Compositor (`AlreadyPressed`), keep Compositor ownership and consume press event.
  2. On release: if recorded owner is `Some(Client)`, emit `KeyboardForward`. If `Some(Compositor)`, emit `KeyboardIntercept` (consume). If `None` (orphan release), CONSUME and drop (do not forward).
  3. Release owner derives strictly from recorded press owner, never recomputed from current modifiers (kills mutation F).
- **Required regression test:**
  - ATK-02 passes: compositor key repeat never leaks to client.
  - ATK-03 passes: release follows recorded owner across modifier change.
  - ATK-04 passes: orphan release is dropped and produces 0 client events.
- **Required mutation/control:** Mutation F (release owner recomputed) must be DETECTED.
- **Closure criterion:** ATK-02, ATK-03, ATK-04 pass; mutation F detected.

---

### INC4-F04 — Harness scenario bindings hard-coded as product policy
- **Severity:** Level B
- **Exact observed failure:** `CANONICAL_BINDINGS` copied the x10 harness test fixtures (`Escape -> Cancel`, `Super+B -> Noop`, `Super+Return -> FocusNext`, `LeftShift -> Noop`). Consequently, plain **Escape was globally swallowed** and never delivered to clients (ATK-01), and Super+B was swallowed for a no-op.
- **Reproduction:** ATK-01: press and release plain `KEY_ESC` with focused client -> swallowed by compositor.
- **Root Cause:** Test harness scenario fixtures from `experiments/b8-harness/src/bin/x10.rs` were mistakenly committed as production compositor bindings in `crates/infynitae-core/src/input/keys.rs`.
- **Canonical invariant/rule:** Canonical binding ownership: test fixtures must not pollute product bindings; plain Escape belongs to clients unless an active compositor interaction captures it.
- **Owner:** `infynitae-core` (`src/input/keys.rs`).
- **Required correction:**
  1. Remove test fixtures (`Super+B`, plain `Escape`, `LeftShift`) from production bindings.
  2. Plain Escape is delivered to clients.
  3. If Escape is used for K6 interaction cancellation, it is only captured when an interaction is actively in progress (`interaction.active()`). When no interaction is active, Escape is forwarded to clients.
- **Required regression test:** ATK-01 passes: plain Escape delivered to client when no interaction is active.
- **Required mutation/control:** Test asserting plain Escape delivered to client.
- **Closure criterion:** ATK-01 passes; plain Escape is never swallowed during normal operation; test fixtures separated from product bindings.

---

### INC4-F05 — OB-4 dismissal injects pointer events into an acquired CameraPan
- **Severity:** Level B
- **Exact observed failure:** Calling `ptr.unset_grab` during popup dismissal replays `motion` to the pending focus (Smithay `PointerHandle::unset_grab` invokes `restore_focus=true` -> `motion`). The client receives `motion 45,40` at the pan press (ATK-09), violating INV-INPUT-3 (zero client events during compositor-owned pan).
- **Reproduction:** ATK-09: popup open; press middle mouse button + Super (CameraPan acquisition); client receives synthetic `wl_pointer.motion 45,40`.
- **Root Cause:** Smithay's `PointerHandle::unset_grab` synthesizes a motion event to restore pointer focus to the surface under the cursor. During CameraPan acquisition, the compositor takes exclusive pointer ownership, so synthetic client motion violates KeepFocus.
- **Canonical invariant/rule:** INV-INPUT-3 (KeepFocus: compositor gesture emits no client events); OB-4 (`popup_done` in reverse creation order before camera change).
- **Owner:** `infynitae-wayland` (`src/outputs.rs`, `src/pointer.rs`).
- **Required correction:**
  1. During `Effect::DismissPopupGrab` on CameraPan acquisition, dismiss the popup grab via `PopupGrab::ungrab(All)` without triggering synthetic motion replay to the client, or clear pointer focus / suppress client motion dispatch while CameraPan is active.
  2. Ensure Smithay pointer grab teardown does not generate client-visible pointer events during CameraPan.
- **Required regression test:** ATK-09 passes: 0 pointer motion/enter/leave/button events delivered to client during CameraPan acquisition with popup dismissal.
- **Required mutation/control:** Mutation O (ignoring DismissPopupGrab) must be DETECTED.
- **Closure criterion:** ATK-09 passes; zero synthetic pointer motion during CameraPan acquisition.

---

### INC4-F06 — D-B6-BOUNDARY rule 1 not on the production path
- **Severity:** Level B
- **Exact observed failure:** Production caller in `handlers.rs:69` passed `causing_output = None` to `Core::popup_constraint_target(parent, causing_output)`. Consequently, Rule 1 never executed on the production path; Rule 2 (fallback to parent clock output) was always used. The core unit test's assertions were inside `if` guards, and mutation P survived.
- **Reproduction:** Inspect `handlers.rs:69`: `core.popup_constraint_target(parent, None)`. Mutation P (dropping rule 1 visibility check) survived.
- **Root Cause:** Adapter did not resolve the causing output of the input event that triggered the grab (or last input event / pointer location output where grab was requested).
- **Canonical invariant/rule:** D-B6-BOUNDARY Rule 1 (anchor output = output of causing input event if parent visible on it, else parent clock output).
- **Owner:** `infynitae-wayland` (`src/handlers.rs`) & `infynitae-core` (`src/protocol.rs`).
- **Required correction:**
  1. In `WaylandState::grab`, determine the causing output: the output corresponding to the pointer's current location when the grab request occurs (or grab serial input event).
  2. Pass `Some(causing_output)` to `Core::popup_constraint_target`.
  3. Remove silent `if` guards in `protocol.rs` unit tests.
  4. Add integration test where candidate outputs differ (causing output != pointer output != clock output) to verify Rule 1 selects causing output.
- **Required regression test:** Test verifying Rule 1 on production path where candidate outputs differ.
- **Required mutation/control:** Mutation P (dropping visibility check in Rule 1) must be DETECTED.
- **Closure criterion:** Production passes causing output; Rule 1 executes; mutation P detected.

---

### INC4-F07 — Focus can target unmapped elements
- **Severity:** Level B
- **Exact observed failure:** `Stage::z` retains unmapped elements. `focus_next` (ATK-05) and `target_lost` successor (ATK-06) selected unmapped toplevels from `Stage::z_order`, which then received `wl_keyboard.enter` and `activated`. Unmapping a focused element keeping focus (mutation W) was untested.
- **Reproduction:**
  - ATK-05: unmap W2, press Super+Enter -> W2 receives focus and `enter W2`.
  - ATK-06: unmap W2, destroy W1 -> successor is unmapped W2.
- **Root Cause:** `FocusModel` operations used `Stage::z_order()` or unfiltered element lists rather than filtering for currently mapped, protocol-eligible elements.
- **Canonical invariant/rule:** OBJ-22 / M11 / INV-INPUT-8 (ActiveElementFocus must only target mapped/live elements).
- **Owner:** `infynitae-core` (`src/input/focus.rs`, `src/protocol.rs`, `src/stage.rs`).
- **Required correction:**
  1. Filter focus candidates in `focus_next` and `target_lost`: only mapped elements (`is_mapped == true`) can receive focus.
  2. On toplevel unmap (`toplevel_unmapped`), if the unmapped element holds focus, trigger `target_lost(id, &live_mapped_elements)`.
- **Required regression test:**
  - ATK-05 passes: `focus_next` skips unmapped elements.
  - ATK-06 passes: successor is never unmapped.
  - Unmap of focused element transfers focus to next live mapped element.
- **Required mutation/control:** Mutation W (unmap of focused element keeps focus) must be DETECTED.
- **Closure criterion:** ATK-05, ATK-06 pass; unmapped elements never receive focus; mutation W detected.

---

### INC4-F08 — Refused popup grabs are not dismissed
- **Severity:** Level B
- **Exact observed failure:** When a client requests `xdg_popup.grab` on an element that does not hold ActiveElementFocus, the adapter returned early without sending `popup_done` (ATK-11). This violates xdg-shell ("If the compositor denies the grab, the popup will be immediately dismissed") and OB-18. PF-WSM7 was vacuous, and mutations N and L survived.
- **Reproduction:** ATK-11: request popup grab on unfocused W2 -> grab refused, but no `popup_done` sent.
- **Root Cause:** In `handlers.rs`, if root != focused element, the handler returned early without calling `popup.send_popup_done()`.
- **Canonical invariant/rule:** xdg-shell specification; OB-18 (protocol-correct refusal: denied grab immediately dismissed with `popup_done`).
- **Owner:** `infynitae-wayland` (`src/handlers.rs`).
- **Required correction:**
  1. On grab refusal (unfocused parent or invalid grab state), immediately call `popup.send_popup_done()`.
  2. Strengthen tests to assert `popup_done` on refused grabs.
- **Required regression test:** ATK-11 passes: refused grab sends `popup_done`.
- **Required mutation/control:** Mutations N (bypassing root/focus check) and L (foreign sequence refusal removed) must be DETECTED.
- **Closure criterion:** ATK-11 passes; refused grab sends `popup_done`; mutations N and L detected.

---

### INC4-F09 — Adapter popup-grab copy diverges from Smithay; preemption hits unrelated grabs
- **Severity:** Level B
- **Exact observed failure:**
  1. Stale popup grab preemption tears down the INC-3 `ZeroOutputGrab` when no outputs exist (ATK-12).
  2. Stale popup grab preemption tears down an implicit client pointer button grab (ATK-14).
  3. Output removal with an active popup grab moves keyboard focus from popup to root (P -> W1) without `popup_done` (ATK-15), violating K8 and xdg-shell ("the top most grabbing popup will always have keyboard focus").
- **Reproduction:**
  - ATK-12: Zero outputs + popup grab + focus change -> ZeroOutputGrab destroyed!
  - ATK-14: Popup destroyed by client, then button held (implicit grab), focus change -> implicit grab destroyed!
  - ATK-15: Popup grab active, disconnect output -> keyboard focus moved P -> W1, no `popup_done`.
- **Root Cause:**
  - `WaylandState` maintained a separate `popup_grab: Option<PopupGrab>` field that was only cleared on explicit preemption, not when popups died, were dismissed, or when Smithay finished the grab.
  - Preemption called `unset_pointer_grab` blindly on the seat, unsetting whatever pointer grab was active (including ZeroOutputGrab or client button grab).
  - Output disconnect triggered Smithay's `PopupPointerGrab::unset`, which unset keyboard grab and refocused root without `popup_done`.
- **Canonical invariant/rule:** Single authority for grab state; INC-3 ZeroOutputGrab invariants; K8 / OB-15 (output removal changes no keyboard state); xdg-shell popup grab invariants.
- **Owner:** `infynitae-wayland` (`src/outputs.rs`, `src/handlers.rs`, `src/pointer.rs`).
- **Required correction:**
  1. Do not maintain a separate diverging grab copy. Check whether the active pointer grab is actually a `PopupPointerGrab` before unsetting it.
  2. Never unset `ZeroOutputGrab` or a client implicit button grab during popup preemption.
  3. On output removal with an active popup grab, ensure the popup grab is properly handled per K8 without moving keyboard focus to root or dropping `popup_done`.
- **Required regression test:**
  - ATK-12 passes: zero-output grab preserved across preemption.
  - ATK-14 passes: implicit client button grab preserved across stale preemption.
  - ATK-15 passes: output removal preserves popup keyboard route.
- **Required mutation/control:** Permanent tests for ATK-12, ATK-14, ATK-15.
- **Closure criterion:** ATK-12, ATK-14, ATK-15 pass; ZeroOutputGrab and client button grabs protected.

---

### INC4-F10 — Focus change with grab synthesizes pointer motion
- **Severity:** Level C (Forward Risk)
- **Exact observed failure:** INV-INPUT-10 requires pointer `unset_grab`, which in Smithay 0.7.0 always replays `motion` to the pending focus (`unset_grab(..., restore_focus=true)` -> `motion`). ATK-10 observed `motion 50,50` on keyboard focus change with an active popup grab. Under a pointer lock (INC-5), this becomes W-SM-16.
- **Reproduction:** ATK-10: keyboard focus change while popup grab active -> `motion 50,50` synthesized.
- **Root Cause:** Smithay's `PointerHandle::unset_grab` unconditionally restores pointer focus and sends motion to the surface under the cursor.
- **Canonical invariant/rule:** INV-INPUT-10, W-SM-16 (INC-5 blocker).
- **Owner:** Architecture / INC-4 & INC-5 interface.
- **Required correction:**
  - Formally adjudicate in Level C adjudication section:
    In INC-4 (unlocked pointer), pointer refocus on grab dismissal is standard Wayland / Smithay behavior.
    However, under CameraPan (compositor gesture), motion is strictly prohibited (INV-INPUT-3, closed in F05).
    For keyboard focus change without camera pan, the synthesized motion is documented as a known Smithay constraint to be handled by W-SM-16 in INC-5 when pointer locks are introduced.
- **Required regression test:** ATK-10 documented and verified in accordance with adjudication.
- **Closure criterion:** Formal adjudication recorded and approved.

---

### INC4-F11 — Successor fallback to top z-order when no focus history exists
- **Severity:** Level C
- **Exact observed failure:** Canonical C4.5 successor policy specifies: "most recently focused live element". When no focus history exists, the implementation fell back to the top of `Stage::z_order`. ATK-07 asserted that a never-focused element should not be focused (should become `Unfocused`).
- **Reproduction:** ATK-07: create W1 and W2, focus W1 (W2 never focused), destroy W1 -> W2 focused from z-order fallback.
- **Root Cause:** Implementation included `.or_else(|| live_elements.last().copied())` when `history` had no live candidate.
- **Canonical invariant/rule:** C4.5 ("most recently focused live element").
- **Owner:** `infynitae-core` (`src/input/focus.rs`).
- **Required correction:**
  - Adjudicate C4.5: when history contains no live elements, focus state transitions to `None` (`Unfocused`).
  - Remove fallback to arbitrary live element; only select from history of previously focused live elements.
- **Required regression test:** ATK-07 passes: successor without history is `None`.
- **Closure criterion:** ATK-07 passes; FocusModel strictly follows C4.5 history-only policy.

---

### INC4-F12 — Shipped verification inadequate, circular, or vacuous
- **Severity:** Level B
- **Exact observed failure:**
  1. KV-2 / F-6 held-key oracle computed expected keys from production `core().key_ownership()`.
  2. F-4 ("single transition") declared in header but never computed.
  3. NEG-2 and OBS-1 did not assert distinguishing failures (OBS-1 had 0 assertions; NEG-2 asserted `popup_dones == ["P"]`).
  4. PF-WSM7 was vacuous (did not check refusal semantics).
  5. No test asserted `activated` toplevel state (mutations Q, R survived).
  6. No T1 click-to-focus test (mutation U survived).
  7. K6 test was run without an active interaction.
  8. Ordered preemption mutations I, J, K all survived.
- **Reproduction:** Mutations F, I, J, K, Q, R, U survived shipped test suite.
- **Root Cause:** Inadequate assertion rigor; lack of independent oracles; tautological test checks.
- **Canonical invariant/rule:** OB-30, TST-07, TST-20, falsification principle.
- **Owner:** Test suites (`t1_keyboard.rs`, `t1_popup_focus.rs`).
- **Required correction:**
  1. Rebuild KV-2 / F-6 with hand-derived expected keys and client wire assertions.
  2. Implement F-4 transition assertion checking for single transition.
  3. Replace NEG-2 with ATK-17 oracle (asserting difference between ordered and pointer-first preemption).
  4. Replace OBS-1 with ATK-16 oracle (asserting routing failure when workaround disabled).
  5. Add explicit tests for `activated` configure events (killing Q and R).
  6. Add T1 click-to-focus test (killing U).
  7. Implement K6 test with real active interaction (CameraPan) and verify cancellation behavior.
  8. Add tests killing I, J, K (order and completeness of preemption).
- **Required regression test:** All new and rebuilt test cases passing.
- **Required mutation/control:** Mutations F, I, J, K, Q, R, U all DETECTED.
- **Closure criterion:** All 14 previously surviving mutations DETECTED; zero circular/vacuous tests.

---

### INC4-F13 — CHECKPOINT incorrectly assigns OB-21 to INC-4
- **Severity:** Level C
- **Exact observed failure:** CHECKPOINT (INC-3 ratification entry) says "OB-21 (T4 intake in INC-4)". But canonically OB-21 belongs to T4 / INC-16 (physical libinput intake on physical hardware), not headless INC-4.
- **Reproduction:** Inspect `docs/foundation/CHECKPOINT.md` line referencing OB-21 in INC-4.
- **Root Cause:** Documentation transcription error during INC-3 ratification.
- **Canonical invariant/rule:** `FOUNDATION_TEST_PLAN.md` OB-21 definition (destination T4, dependencies INC-3, INC-16).
- **Owner:** Foundation documentation (`docs/foundation/CHECKPOINT.md`).
- **Required correction:** Correct CHECKPOINT: clarify that OB-21 is owned by INC-16 / T4 (requires physical libinput and owner hardware authorization); INC-4 is HEADLESS.
- **Closure criterion:** CHECKPOINT updated and verified against `check-foundation-docs`.

---

### INC4-F14 — Evidence and CHECKPOINT misreport test and control counts
- **Severity:** Level B
- **Exact observed failure:** Evidence README and CHECKPOINT stated "106 tests, 68/68 controls" while the actual counts in the reviewed tree were 151 workspace tests and 83 efficacy controls (81 seeded + 2 judge).
- **Reproduction:** Compare reported numbers with actual execution output.
- **Root Cause:** Stale counts copied from draft documentation rather than verified against automated command outputs.
- **Canonical invariant/rule:** Audit integrity and deterministic reporting.
- **Owner:** Evidence documentation (`docs/foundation/evidence/phase-f/INC-4/`, `docs/foundation/CHECKPOINT.md`).
- **Required correction:** Update all evidence files and CHECKPOINT with the exact executed test and control counts.
- **Closure criterion:** All reports state exact counts matching automated tooling outputs.

---

### INC4-F15 — Brief expected HEAD hash discrepancy
- **Severity:** Observation
- **Exact observed failure:** The review brief stated expected HEAD `d5b7b95000572d45cfa2c8f8fb441f71dfbc9b3d`, but the actual commit at `origin/main` was `d5b7b95cbc910c48377c72ae94ccec2b0c05e140`.
- **Reproduction:** `git rev-parse HEAD` returns `d5b7b95cbc910c48377c72ae94ccec2b0c05e140`.
- **Root Cause:** Truncated/garbled SHA-1 hash in the prompt text sharing the same 7-character prefix `d5b7b95`.
- **Canonical invariant/rule:** Commit provenance verification.
- **Owner:** Audit / Handoff.
- **Required correction:** Document that `d5b7b95cbc910c48377c72ae94ccec2b0c05e140` was the true reviewed HEAD.
- **Closure criterion:** Reconciled in documentation.

---

### INC4-F16 — Two competing modifier authorities
- **Severity:** Level B
- **Exact observed failure:** CameraPan read a backend-supplied modifier snapshot from `PhysicalEvent::PointerButton { modifiers }`, whereas keyboard keys used Smithay's XKB state. When Super was physically held on the keyboard, middle-clicking with an empty snapshot failed to acquire CameraPan (ATK-13).
- **Reproduction:** ATK-13: hold Super on keyboard, send middle button press with `modifiers = none()` -> CameraPan not acquired.
- **Root Cause:** Pointer button intake received a static caller-supplied modifier snapshot rather than reading the active modifier state from Smithay's XKB seat state.
- **Canonical invariant/rule:** OBJ-27 (Smithay XKB is the sole modifier authority; Core maintains no duplicate modifier cache).
- **Owner:** Input pipeline (`crates/infynitae-core`, `crates/infynitae-wayland`, `crates/infynitae-backend-headless`).
- **Required correction:**
  1. Synchronize modifier authority: pointer button events must evaluate modifiers using the seat's active XKB modifier state.
  2. In headless backend, ensure pointer button intake queries or receives the seat's dynamic XKB modifier state rather than a decoupled snapshot.
- **Required regression test:** ATK-13 passes: Super held on keyboard + middle click acquires CameraPan regardless of backend button snapshot.
- **Required mutation/control:** Test fails if button intake ignores seat keyboard modifier state.
- **Closure criterion:** Single modifier authority; ATK-13 passes.

---

### INC4-F17 — Popup of unmapped parent not dismissed
- **Severity:** Level C
- **Exact observed failure:** M3 row "parent unmapped/destroyed -> `popup_done` for the tree" was not implemented for unfocused parents (ATK-18).
- **Reproduction:** ATK-18: create popup Q on unfocused W2, unmap W2 -> popup Q not dismissed (`popup_done` not sent).
- **Root Cause:** Popup dismissal on unmap was only triggered if the unmapped element was the active focus target.
- **Canonical invariant/rule:** M3 (`parent unmapped/destroyed -> popup_done for the tree`).
- **Owner:** `infynitae-wayland` (`src/handlers.rs`) & `infynitae-core` (`src/protocol.rs`).
- **Required correction:**
  1. In `toplevel_unmapped` and `toplevel_destroyed`, dismiss all child popups belonging to the unmapped/destroyed surface, even if the parent surface is not focused.
- **Required regression test:** ATK-18 passes: unmapping an unfocused parent dismisses its child popups.
- **Closure criterion:** ATK-18 passes; M3 unmap dismissal holds universally.

---

## 14 Previously Surviving Mutations — Resolution Plan

| Mutation ID | Description | Root Gap in Shipped Suite | Corrective Detection Plan |
|:---:|:---|:---|:---|
| **F** | Release owner recomputed from bindings instead of remembered | Tests released binding keys while modifier was still held | Test releasing key after modifier is released (ATK-03) |
| **I** | Preemption order swapped (pointer first) | NEG-2 asserted `popup_dones` count instead of transient root refocus | Assert client wire log: no transient root enter/leave (ATK-17) |
| **J** | Keyboard grab survives focus transfer | PF-1 did not assert keyboard ungrab | Assert keyboard handle is not grabbed after preemption |
| **K** | Pointer popup grab survives focus transfer | PF-1 did not assert pointer ungrab | Assert pointer handle is not grabbed after preemption |
| **L** | Foreign-sequence refusal removed | No test attempted grab while keyboard grabbed | Add test requesting grab while keyboard is already grabbed |
| **N** | Popup root/focus check bypassed | PF-WSM7 did not assert refusal dismissal | Assert `popup_done` sent on refused grab (ATK-11) |
| **P** | Rule 1 visibility guard dropped | `causing_output` was always `None` in production | Production passes `causing_output`; test with parent invisible on causing output |
| **Q** | Old toplevel keeps `activated` | No shipped test asserted xdg activation state | Test asserting `activated` state cleared on old toplevel |
| **R** | New toplevel never activated | No shipped test asserted xdg activation state | Test asserting `activated` state set on new toplevel |
| **T** | Backend skips `InputPort` check | No foreign-port test for keyboard | Test submitting keyboard key with foreign/invalid port fails |
| **U** | Click-to-focus removed | No T1 test for click-to-focus | Test clicking unfocused surface acquires ActiveElementFocus |
| **V** | W-SM-14 workaround default off | PF-6 forced the switch on | Production default is hardcoded on; test does not toggle switch |
| **W** | Unmap of focused element keeps focus | No test unmapped focused element | Test unmapping focused element triggers target_lost |
| **AB** | K3 enforcement default off | Tests constructed with `k3_enforced = true` | Production default is hardcoded on; test does not toggle switch |

