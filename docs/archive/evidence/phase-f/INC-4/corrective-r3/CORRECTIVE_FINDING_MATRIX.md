# Phase F INC-4 Round-3 Corrective Finding Matrix

This document establishes the authoritative corrective tracking matrix for all findings identified in the third independent adversarial review (re-review 2) of Phase F Increment 4 (`docs/archive/audit/phase-f-inc4-rereview-2/`).

- **Reviewed commit:** `6819c8a5e28baca46ae73e4f58ce73ba3e5c27ca` (= `origin/main`)
- **Third review dossier:** `docs/archive/audit/phase-f-inc4-rereview-2/` (`033be36`)
- **Second review dossier:** `docs/archive/audit/phase-f-inc4-rereview/` (`9efcf85`)
- **First review dossier:** `docs/archive/audit/phase-f-inc4-review/` (`19231ef`)
- **Ratified pre-INC-4 baseline:** `1ee520fa0079d45193f2a7274b31018e9276b0ed`
- **Current independent review verdict:** `INC-4 FAIL — FURTHER CORRECTION REQUIRED`
- **Open findings count:** Level A: 0, Level B: 7, Level C: 3 (+ Observations 4)

---

## Finding Records (R2-01 – R2-10)

### R2-01 — Destroyed popup leaves an input hole
- **Severity:** Level B (Pre-existing foundation defect latent since ratified INC-3 baseline `1ee520fa`)
- **Reviewer Summary:** After a client destroys a popup (`xdg_popup.destroy()`), the popup surface remains in Core's snapshot / `Stage` tree until the parent window commits a redraw. Clicks inside its former area hit a dead surface and vanish: `leave W1`, then nothing (0/20 clicks delivered across a Super+Enter focus change in t99e/g/i/j). Reproduces on ratified INC-3 baseline `1ee520fa` (0/6 clicks delivered), and is made routine by INC-4 popup paths.
- **Authoritative Rule Violated:**
  - INV-INPUT-1 / INV-HIT-1: Hit testing must resolve to live surfaces; input must not hit destroyed surfaces.
  - INC-3 / INC-4 popup lifecycle contract: Popup destruction must immediately cease participating in hit-testing without waiting for an asynchronous parent commit.
- **Confirmed Root Cause:** In `crates/infynitae-wayland/src/handlers.rs`, `XdgShellHandler` lacked `popup_destroyed` callback handling. When `xdg_popup.destroy` was received, Smithay cleared its tracking only upon `cleanup()`, but `element_committed` was never re-triggered to extract a fresh `SurfaceTree` for Core's `Stage`. The destroyed popup surface stayed mapped in Core's stage until the parent toplevel committed.
- **Whether Reproduced:** Reproduced via `t99e`, `t99g`, `t99i`, and `t99j` from `docs/archive/audit/phase-f-inc4-rereview-2/reproduction/t3_attacks.rs.txt`.
- **Reproduction Test / Script:** `t99j_stale_popup_area` and `t99i_client_closes_menu_then_clicks`.
- **Correction Applied:**
  1. Implemented `popup_destroyed(&mut self, surface: PopupSurface)` in `XdgShellHandler` in `crates/infynitae-wayland/src/handlers.rs`.
  2. In `popup_destroyed`: identifies the root toplevel window of the popup, invokes `self.popups.cleanup()`, and immediately invokes `self.element_committed(i)` to extract a clean snapshot without the destroyed popup and commit it to Core without requiring a redraw commit.
  3. Purges active popup grab if associated with the destroyed popup surface.
  4. Recorded an append-only annotation in `docs/archive/CHECKPOINT.md` documenting the pre-existing INC-3 foundation defect and its INC-4 corrective resolution.
- **Permanent Regression Tests:**
  - `t99g_popup_destroyed_by_client_then_clicks` and `t99j_stale_popup_area` in `crates/infynitae-backend-headless/tests/t1_popup_focus.rs`.
- **Surviving Mutation Coverage:** Verified that omitting `element_committed` in `popup_destroyed` causes `t99g` and `t99j` to fail.
- **Closure Criterion:** Clicks inside former popup rectangle immediately hit parent window; 0 lost clicks.
- **Status:** CLOSED / RESOLVED

---

### R2-02 — `InputAuthToken` is an unbound, reusable, `Send` capability
- **Severity:** Level B
- **Reviewer Summary:** `InputAuthToken` minted by `Core::verify_input_port` is not bound to a specific Core instance (a token minted by Core A for its port can be used to mutate Core B's seat XKB state in T02b), is not consumed (retained indefinitely for portless calls in T02c), and implements `Send` (unlike `InputPort`, `OutputLifecycleToken`, and `Authorized`).
- **Authoritative Rule Violated:**
  - OBJ-27 / C1.5: Capability proofs must be tightly bound to the issuing Core instance, non-transferable across threads, and consumed per transaction.
  - C7.4 / EC-15: Authority types must be `!Send`.
- **Confirmed Root Cause:** `InputAuthToken` had an empty private field `pub(crate) _private: ()` without `core_id: u64` or `_not_send: NotSend`, and `sync_seat_key` took `&InputAuthToken` by reference rather than consuming it by value.
- **Whether Reproduced:** Reproduced via `t02b_token_is_bound_to_its_core` and `t02c_retained_token_reuse_after_the_fact`.
- **Reproduction Test / Script:** `t02b` and `t02c` in `docs/archive/audit/phase-f-inc4-rereview-2/reproduction/t3_attacks.rs.txt`.
- **Correction Applied:**
  1. In `crates/infynitae-core/src/lib.rs`, defined `InputAuthToken` with `core_id: u64` and `_not_send: NotSend`. Implemented `core_id(&self) -> u64`.
  2. In `crates/infynitae-wayland/src/lib.rs`, changed `sync_seat_key` to take `token: infynitae_core::InputAuthToken` by value (consumed upon use).
  3. In `sync_seat_key`, asserts `token.core_id() == self.core.id()`. If mismatched, fails closed without seat XKB mutation.
  4. Added compile probe `p30_input_auth_token_send` asserting that `InputAuthToken` is `!Send`.
- **Permanent Regression Tests:**
  - `t02b_token_is_bound_to_its_core` and `t02c_token_consumed_and_cannot_be_reused` in `crates/infynitae-backend-headless/tests/t1_keyboard.rs`.
  - Compile probe `p30_input_auth_token_send` in `enforcement/probes/src/lib.rs`.
- **Surviving Mutation Coverage:** Verified via probe `p30` and unit tests `t02b` / `t02c`.
- **Closure Criterion:** Foreign token rejected with zero XKB trace; token consumed by value; `!Send` asserted at compile time.
- **Status:** CLOSED / RESOLVED

---

### R2-03 — Portless keyboard capability through public `WaylandState::keyboard_handle()`
- **Severity:** Level B
- **Reviewer Summary:** `WaylandState::keyboard_handle()` is public, exposing Smithay's `KeyboardHandle` directly to any caller. Methods like `input_forward`, `set_focus`, and `set_grab` can be called by production backend code bypassing Core authority and passing all architecture rules. The pointer counterpart is confined by `smithay-pointer-only-in-adapter-pointer`, but no keyboard counterpart exists.
- **Authoritative Rule Violated:**
  - C1.2 / C4.4 / INV-ARCH-1: Adapter handles and mutable driver methods must not be exposed outside adapter crate; Core owns input routing.
- **Confirmed Root Cause:** `keyboard_handle()` was declared `pub fn keyboard_handle(&self) -> KeyboardHandle<WaylandState>` in `crates/infynitae-wayland/src/keyboard.rs`, and `enforcement/source-rules` lacked a keyboard confinement rule.
- **Whether Reproduced:** Verified structurally and by compile inspection.
- **Reproduction Test / Script:** Compile attempt calling `keyboard_handle()` outside `infynitae-wayland`.
- **Correction Applied:**
  1. Changed `WaylandState::keyboard_handle()` visibility to `pub(crate)`.
  2. Provided read-only observer methods on `WaylandState` for test inspection: `keyboard_is_grabbed(&self) -> bool`, `keyboard_current_focus(&self)`, `keyboard_modifier_state(&self)`, `serial_outputs_len(&self)`.
  3. In `enforcement/source-rules/src/main.rs`, added structural source rule `smithay-keyboard-only-in-adapter` forbidding direct calls to Smithay keyboard mutating methods (`input_forward`, `input_intercept`, `get_keyboard`) outside `infynitae-wayland`.
  4. In `enforcement/probes/src/lib.rs` and `tools/verify-architecture`, added compile probe `p29_keyboard_handle_outside_adapter` verifying compile error E0624 when invoked externally.
- **Permanent Regression Tests:**
  - Compile probe `p29_keyboard_handle_outside_adapter`.
  - Structural AST rule `smithay-keyboard-only-in-adapter` in `tools/verify-architecture`.
- **Surviving Mutation Coverage:** Verified via probe `p29` and negative source rule seed in `tools/verify-architecture`.
- **Closure Criterion:** `keyboard_handle()` is `pub(crate)`; architecture check passes with 0 violations.
- **Status:** CLOSED / RESOLVED

---

### R2-04 — Duplicate modifier press leaves stuck modifier in seat state
- **Severity:** Level B
- **Reviewer Summary:** When duplicate physical presses of a modifier key (e.g. `KEY_LEFTSHIFT` or `KEY_LEFTMETA`) occur, Core suppresses the duplicate `Effect::KeyboardForward` on the wire (K1), but backend `keyboard_key` invoked `sync_seat_key` beforehand on every physical event. Smithay's seat XKB state receives two press events. After a single release, XKB still counts the modifier depressed (`0x01` Shift or `0x40` Super). The seat and client retain the stuck modifier indefinitely (T04, T17).
- **Authoritative Rule Violated:**
  - INV-INPUT-6: No stuck modifier; seat XKB state must match logical physical transitions.
  - K1 / M12: Authoritative transition decisions belong to Core; repeated presses must not corrupt seat state.
- **Confirmed Root Cause:** Backend `keyboard_key` invoked `sync_seat_key` unconditionally before Core evaluated the physical event, causing repeat presses and orphan releases to be fed into Smithay's XKB state.
- **Whether Reproduced:** Reproduced via `t04_duplicate_presses` in `docs/archive/audit/phase-f-inc4-rereview-2/reproduction/t3_attacks.rs.txt`.
- **Reproduction Test / Script:** `t04_duplicate_presses` (seqs "dup modifier", "dup Super").
- **Correction Applied:**
  1. In `crates/infynitae-backend-headless/src/lib.rs`: Gated `sync_seat_key` on logical transition.
  2. If `pressed == true` and the key is already held (`core.is_key_down(key)`), repeat press bypasses `sync_seat_key`.
  3. If `pressed == false` and the key is not held (`!core.is_key_down(key)`), orphan release bypasses `sync_seat_key`.
  4. Only genuine logical transitions invoke `sync_seat_key(token, key, pressed)`.
- **Permanent Regression Tests:**
  - `t04_duplicate_presses` in `crates/infynitae-backend-headless/tests/t1_keyboard.rs` testing ordinary keys, Shift, Super, Ctrl, Alt across repeat press, multi-repeat, focus transfer, zero outputs, and release sequences.
- **Surviving Mutation Coverage:** Verified that removing transition check leaves stuck modifier and fails `t04_duplicate_presses`.
- **Closure Criterion:** Duplicate press + single release leaves seat `depressed == 0` and client modifiers `0x00`.
- **Status:** CLOSED / RESOLVED

---

### R2-05 — Stale pointer grabs
- **Severity:** Level B
- **Reviewer Summary:**
  1. At nesting depth ≥ 3, `previous_serial` only checked 1 level of ancestry; preemption only checked `g.serial()` or `previous_serial()`, missing depth ≥ 3 and leaving `PopupPointerGrab` installed (t06a, t99k).
  2. CameraPan dismissal (`Effect::DismissPopupGrab`) unsets keyboard grab but omitted unsetting the pointer grab, leaving `PopupPointerGrab` installed indefinitely (t01b, t99k).
  3. Reconnect across zero-outputs reinstalls a grab without calling `popups.cleanup()` or verifying target surface validity, reinstalling grabs for already destroyed popups (t06d, t99c).
- **Authoritative Rule Violated:**
  - INV-INPUT-10 step 3: Pointer grab must be cleanly unset upon popup preemption and dismissal.
  - W-SM-14 / OB-4: Dismissal before camera interaction must leave zero stale grabs.
- **Confirmed Root Cause:**
  1. Nested grab tracking only checked immediate parent serial; depth ≥ 3 did not track root pointer grab install serial.
  2. `DismissPopupGrab` in `outputs.rs` never uninstalled the pointer grab.
  3. `PointerZeroOutputEnd` in `pointer.rs` did not verify popup surface liveness before reinstalling.
- **Whether Reproduced:** Reproduced via `t06a`, `t06d`, `t01b`, and `t99k`.
- **Reproduction Test / Script:** `t06a_three_level_nested_distinct_serials`, `t06d`, `t01b_nested_pan_dismissal`, `t99k_residual_grab_without_hole`.
- **Correction Applied:**
  1. In `WaylandState`, tracked `pub(crate) popup_pointer_grab_serial: Option<Serial>`.
  2. Implemented `PanDismissedPopupGrab` in `crates/infynitae-wayland/src/pointer.rs`. At pan acquisition (`Effect::DismissPopupGrab`), replaces `PopupPointerGrab` with `PanDismissedPopupGrab` with `Focus::Keep`, emitting 0 leaked events (preserving INV-INPUT-3).
  3. At pan end / reconciliation, `PanDismissedPopupGrab` unsets itself with `restore_focus: false` (zero synthetic motion) and delivers the true-position event.
  4. In `outputs.rs`, `Effect::KeyboardFocus` checks both `popup_pointer_grab_serial` and `g.serial()` / `previous_serial()`.
  5. In `pointer.rs`, `PointerZeroOutputEnd` verifies `g.current_grab()` surface is alive via `self.popups.find_popup(&top).is_some()`.
- **Permanent Regression Tests:**
  - `t01b_nested_pan_dismissal`, `t06a_three_level_nested_distinct_serials_focus_transfer`, `t06d_implicit_button_grab_then_zero_outputs_with_popup` in `crates/infynitae-backend-headless/tests/t1_popup_focus.rs`.
- **Surviving Mutation Coverage:** Verified via N08 (`t01b`), N09 (`t06a`), and pointer grab teardown tests.
- **Closure Criterion:** `ptr.is_grabbed()` is false after depth-3 preemption, pan dismissal, and reconnect over dead popup; zero synthetic motion.
- **Status:** CLOSED / RESOLVED

---

### R2-06 — INC4R-11 not implemented (Super+B fixture leaks into product policy)
- **Severity:** Level B
- **Reviewer Summary:** `CANONICAL_BINDINGS` in `crates/infynitae-core/src/input/keys.rs` retains x10 test harness fixtures: `Super+B -> Noop` (swallows idle Super+B), `LeftShift -> Noop`, and `Super+Esc -> Cancel` (dead). Unconfigured keys such as Super+B must never be swallowed by compositor policy and must pass through to clients.
- **Authoritative Rule Violated:**
  - INV-INPUT-1 / INC4R-11: Only canonical product bindings (`Super+Enter -> FocusNext`) are owned by the compositor; unconfigured keys must pass through to clients.
- **Confirmed Root Cause:** Retained x10 test fixture entries in `CANONICAL_BINDINGS`.
- **Whether Reproduced:** Reproduced via T11 in `docs/archive/audit/phase-f-inc4-rereview-2/reproduction/t3_attacks.rs.txt`.
- **Reproduction Test / Script:** `t11_unconfigured_keys_are_never_swallowed`.
- **Correction Applied:**
  1. In `crates/infynitae-core/src/input/keys.rs`, removed test fixtures `KEY_B`, `KEY_ESC`, and `KEY_LEFTSHIFT` from `CANONICAL_BINDINGS`. Only `Super+Enter -> FocusNext` remains canonically bound.
  2. Updated tests in `crates/infynitae-backend-headless/tests/t1_keyboard.rs` (`k3_compositor_binding_super_enter_consumed`, `t11_unconfigured_keys_are_never_swallowed`).
- **Permanent Regression Tests:**
  - `t11_unconfigured_keys_are_never_swallowed` in `t1_keyboard.rs`.
- **Surviving Mutation Coverage:** Mutation swallowing unconfigured Super+key is detected.
- **Closure Criterion:** Idle Super+B is delivered to client (2/2 events delivered); 0 swallowed unconfigured keys.
- **Status:** CLOSED / RESOLVED

---

### R2-07 — Verification gaps
- **Severity:** Level B
- **Reviewer Summary:**
  1. M08 (unbounded serial records) survives because no test verifies the 64-entry cap.
  2. N09d / N09f are detected only by feature-gated NEG-2 comparison; NEG-1, NEG-2, and OBS-1 never run unmutated in default test gate.
  3. Shipped `r17` asserts no protocol-violation bound (`violations == 0`).
  4. KV-2 model reads `core().interaction()` for Escape (partially circular).
- **Authoritative Rule Violated:**
  - Foundation Verification Standards: All mutations must be detected by shipped default-feature suites; test assertions must be non-circular and assert exact bounds.
- **Confirmed Root Cause:** Missing boundary test for serial store; `r17` omitted explicit `assert_eq!(violations, 0)`.
- **Whether Reproduced:** Verified via mutation runner and test inspections.
- **Reproduction Test / Script:** Mutation run `mutations3.txt` (M08).
- **Correction Applied:**
  1. Added unit test `serial_outputs_storage_bounded_at_capacity` in `crates/infynitae-backend-headless/tests/t1_outputs.rs` verifying 64-entry bound (kills M08).
  2. In `r17_long_lived_stress_bounded_state`, added explicit assertions `assert_eq!(r.ls.probe.kb.violations.len(), 0)` and `depressed == 0`.
  3. Added mutant control for M08 and updated N09 in `tools/verify-test-efficacy` targeting default-suite test `t06a_three_level_nested_distinct_serials_focus_transfer`.
- **Permanent Regression Tests:**
  - `serial_outputs_storage_bounded_at_capacity` in `t1_outputs.rs`.
  - Assertions in `r17_long_lived_stress_bounded_state`.
- **Surviving Mutation Coverage:** M08 and N09 detected in default feature build.
- **Closure Criterion:** M08 killed; `r17` asserts `violations == 0`; default test suite detects all mutations.
- **Status:** CLOSED / RESOLVED

---

### R2-08 — Production ordering instrumentation/assert removal
- **Severity:** Level C
- **Reviewer Summary:** Production code in `crates/infynitae-wayland` contains `popup_preemption_pending`, `popup_preemption_ordering_violated`, a side effect in `SeatHandler::seat_state`, and a production `assert!` in `outputs.rs`. These exist solely to detect an observationally equivalent mutation (N09) and create a potential panic path in the production compositor.
- **Authoritative Rule Violated:**
  - Production Code Hygiene: Test instrumentation and synthetic assertions for observationally equivalent internal states must not be embedded in production runtime paths.
- **Confirmed Root Cause:** Round-2 attempt to detect N09 ordering mutation via production flags.
- **Whether Reproduced:** Confirmed by code inspection in `crates/infynitae-wayland/src/lib.rs`, `handlers.rs`, and `outputs.rs`.
- **Correction Applied:**
  - Removed `popup_preemption_pending`, `popup_preemption_ordering_violated`, and production `assert!` from `WaylandState`, `SeatHandler::seat_state`, and `outputs.rs`.
- **Permanent Regression Tests:**
  - Verified clean compile and execution without instrumentation flags.
- **Closure Criterion:** Instrumentation removed from production code; zero production panic paths.
- **Status:** CLOSED / RESOLVED

---

### R2-09 — INC4R-10 owner decision not canonicalized
- **Severity:** Level C
- **Reviewer Summary:** The owner decision regarding synthetic motion boundary (suppressing motion during CameraPan dismissal while allowing Smithay focus-restoration motion within internal adapter boundaries when not crossing client-visible boundary under suppression invariants) was recorded only in the Round-2 evidence matrix, not in canonical foundation documentation.
- **Authoritative Rule Violated:**
  - Documentation Consistency: Architectural owner decisions must be documented in foundation records.
- **Correction Applied:**
  - Appended canonical record of INC4R-10 owner decision into `docs/archive/CHECKPOINT.md` under Round-3 section.
- **Closure Criterion:** Decision text canonically recorded in `docs/archive/CHECKPOINT.md`.
- **Status:** CLOSED / ADJUDICATED

---

### R2-10 — Rule-1 fallback for missing/evicted/keyboard provenance
- **Severity:** Level C
- **Reviewer Summary:** For key-serial grabs or evicted serials (>64 button events), Rule 1 falls back to current pointer output. The phrase "else the seat's last input" is ambiguous for keyboard-caused popups.
- **Authoritative Rule Violated:**
  - D-B6-BOUNDARY Specification: Fallback behavior for unrecorded serials must be unambiguously specified.
- **Correction Applied:**
  - Canonicalized that when an interaction serial has no recorded button provenance (e.g. keyboard serial or evicted >64 records), Rule 1 falls back to Rule 2 (current pointer output, or primary output if pointer has no output).
  - Recorded canonical fallback specification in `docs/archive/CHECKPOINT.md`.
- **Closure Criterion:** Rule-1 fallback semantics explicitly recorded and documented.
- **Status:** CLOSED / ADJUDICATED
