# Checkpoint

CURRENT_PHASE: **PHASE F — INC-5 IMPLEMENTED — READY FOR INDEPENDENT ADVERSARIAL REVIEW.** Pointer constraints (`zwp_pointer_constraints_v1`), lock/confinement policy (M13, M14), confinement clamp (F-CONFINE-1), constraint reconciliation on output removal (F-RELOCATE-1), and interaction preemption restoration implemented on the production path. OB-11 adjudicated under Option B (P1 adopted under D-B12-PIN). INC-0, INC-1, INC-2, INC-3, and INC-4 are RATIFIED. **INC-5 has NOT been ratified.** **INC-6 has NOT begun.**

Date: 2026-10-07

## PHASE F — INC-5 IMPLEMENTATION (evidence: evidence/phase-f/INC-5/)
- **Baseline commit:** `2680b99eb6d5353473ff362f5b4a904372f1fdfb` (ratified INC-4).
- **Scope & Deliverables:**
  - Protocol seam: `zwp_pointer_constraints_v1` support in `infynitae-wayland` (`LockedPointer` and `ConfinedPointer` global and surface handlers).
  - Core authority: `Core::pointer_constraint_request`, `Core::pointer_constraint_destroy`, `OBJ-21`, `M13` state machine, `F-LOCK-1` region/lifetime evaluation, and `F-CONFINE-1` continuous confinement bounding clamp.
  - Interaction preemption & reconciliation: grab preemption restoration on popup close (`Core::pointer_preemption_restored`), and output removal reconciliation (`F-RELOCATE-1`).
  - Integration test suite: 13 tests in `crates/infynitae-backend-headless/tests/t1_pointer_constraints.rs` covering OB-31 (pointer lock), OB-20 (confinement), OB-50 (confinement clamp), OB-51 (output removal under lock), OB-47 (explicit deactivate then exit under P1), and Checkpoint L 200-cycle stress test.
- **OB-11 Owner Adjudication & Patch P1 Adoption (D-B12-PIN):**
  - Owner decision: Option B — Adopt Patch P1 (minimal 4-line guard `if constraint.is_active() { constraint.deactivate(); }` in Smithay `PointerHandle::leave()`).
  - Stock defect reproduction: documented in `docs/foundation/evidence/phase-f/INC-5/OB-11-STOCK-DEFECT-REPRODUCTION.md`.
  - Canonical patch file: `patches/0001-seat-leave-deactivate-only-active-constraint.patch`.
  - Vendored dependency: pinned `vendor/smithay-0.7.0` under root `[patch.crates-io]`.
  - Strict architectural pinning: `tools/verify-architecture` enforces SHA-256 of `pointer.rs` (`9c120d4101d7b2d3907d018a400dcd52e32e3be6b6cf6b64c80d0b421bbcb512`), verifies byte-identity with crates.io registry for all other files, and enforces 6 seeded negative controls (patch removal, arbitrary patches, unpinned sources, revision changes, local modifications, dependency upgrades).
  - Wire verification: `ob47_e1` asserts `duplicate_unlocked == 0`.
  - Upstream status: tracked as outward submission; no upstream fix claimed landed. Patch remains pinned and removable when upstream lands an equivalent fix.
- **Obligation dispositions:**
  - **OB-11:** SATISFIED / CLOSED for the INC-5 decision gate under Option B (patch P1 under D-B12-PIN).
  - **OB-3, OB-20, OB-31, OB-47, OB-50, OB-51:** Satisfied and verified on the production path.
- **Declaration:**
  - INC-0 through INC-4 are RATIFIED.
  - **INC-5 has NOT been ratified.**
  - **INC-6 has NOT begun.**
  - Headless execution only; live compositor and physical display untouched.

## PHASE F — EXPERIMENTAL RUNNABLE PROTOTYPE (TINY COMPOSITOR) INTEGRATION
- **Integrated Candidate:** `4e7a7f892491df198216c384fae0a0ef74e5472a`
- **Baseline Commit:** `4d2fb3dc01ece2fabd37f1467c9f9831795e7ed4` (Phase F INC-5 candidate).
- **Scope & Deliverables:**
  - Runnable Nested Compositor: `infynitae-backend-winit::run_nested` with Winit event loop, GL presentation, and Calloop dispatch.
  - Headless Compositor Runner: `infynitae-backend-headless::run_headless` with virtual outputs and automated duration/frame bounds.
  - CLI Composition Root: `crates/infynitae/src/main.rs` with socket selection, client execution (`--exec`), sizing, and refresh rate pacing.
  - Core Initial Focus Activation: `Core::grant_initial_focus` and `FocusModel::grant_initial_focus` enforcing `INV-INPUT-8` (activation grant on first map without focus-stealing on subsequent mappings).
  - Child Process Supervision: `terminate_children` implementing bounded 500 ms graceful SIGTERM polling, SIGKILL escalation, and waitpid zombie reaping.
  - Performance & Event Hardening: Calloop generic display event loop integration, configurable refresh pacing (60–240 Hz), and suppression of redundant idle GPU buffer presentation.
  - Extended Test Suite: 5 end-to-end integration tests in `tests/t1_tiny_compositor.rs` and robust in-flight leave tracking under C Wayland backend (`client_system`).
- **Deferred Protocol Capabilities (Documented Future Increment Ownership):**
  - `wp_fractional_scale_v1`: Canonical future increment INC-6 (F-SCALE-1/2/3).
  - `wp_viewporter`: Canonical future increments INC-6 / INC-15.
  - `wp_cursor_shape_v1`: Canonical future increment INC-15.
- **Declaration:**
  - INC-0 through INC-4 are RATIFIED.
  - **INC-5 has NOT been ratified.**
  - **INC-6 has NOT begun.**
  - Live nested compositor execution enabled for testing; host KWin desktop preserved.


## PHASE F — INC-4 FORMAL RATIFICATION AND SEAL
- **Ratified implementation commit:** `4c01cca2d43e38f4f3a80e9202ec1b7ab0430179` (origin/main).
- **Ratified baseline:** INC-3 commit `1ee520fa0079d45193f2a7274b31018e9276b0ed`.
- **Review verdicts:**
  - Fifth independent adversarial review (`audit/phase-f-inc4-rereview-4/`, commit `b22da28`): `PASS — READY FOR FORMAL RATIFICATION` (Level A = 0, Level B = 0).
  - Final focused ratification audit: `PASS — FORMAL RATIFICATION AUTHORIZED` (Level A = 0, Level B = 0).
- **Final defect counts:** Level A = 0, Level B = 0.
- **Level-C residual observations (non-blocking for INC-4 ratification):**
  - **C-1:** Popup first commit already carrying a buffer is currently accepted (toplevel equivalent is rejected correctly). W-SM-6 popup portion (`ensure_configured`) remains unresolved and is deferred to INC-7.
  - **C-2:** Focused-window unmap may produce a deactivation configure; non-blocking observation.
  - **C-3:** Fifth-review obligation recommendations for OB-18 and OB-21 were corrected by the final ratification audit using the canonical obligation register (`FOUNDATION_TEST_PLAN.md`), keeping parent obligations with deferred or unfulfilled components OPEN while recording partial progress in history.
  - **Carried observations:**
    - Trusted `InputPort` holder boundary: the port is the root of trust; Core cannot inspect XKB keymaps (C4.4).
    - Pinned Smithay 0.7.0 middle-popup behavior: Smithay grab teardown unrolls cleanly to the live ancestor or active element root.
    - Two semantically equivalent Round-4 mutations (A1-a: chain end keeps grab; A1-d: reconcile to `current_grab()`) retired and documented in Round-4 matrix.
- **Obligation dispositions (`docs/foundation/FOUNDATION_TEST_PLAN.md`):**
  - **OB-4:** CLOSED. Popup-grab dismissal on interaction acquisition (CR-4, X-11b) verified by T1 production tests (`pf_ob4_interaction_acquisition_dismisses_popup_grab`, `r10_pan_with_popup_no_synthetic_events_and_clean_return`, and `a1_camerapan_overlapping_destruction`).
  - **OB-15:** CLOSED. Compositor-side repeat (K9 unused) and output removal (K8 no-op) verified by T1 production tests (`k8_ob15_output_removal_under_held_keys`, `k9_ob15_key_repeat_probe`, `t04_duplicate_presses`, `release_follows_press_owner_after_modifier_change`, and `sync_seat_key`).
  - **OB-30:** CLOSED. Port `xpopup_focus` (ER-1) to T1 against production adapter (ActiveElementFocus vs ProtocolSurfaceRoute, ordered preemption INV-INPUT-10, nested-grab W-SM-14 workaround) verified by T1 production tests (`pf_1` through `pf_7`, `ordered_preemption_exact_keyboard_wire`) and permanently gated by `./tools/verify-architecture` [5d] against negative controls NEG-1, NEG-2, and OBS-1 under probe flags.
  - **OB-18:** OPEN. The W-SM-7 popup grab panic precondition is fully satisfied at INC-4 via `grab_popup` validation, `reconcile_popup_chain`, and depth 1..4 adversarial tests (`a1_inner_popup_closes_then_key`, `a1_client_closes_whole_chain_then_key`, `a1_middle_popup_disappears_then_key`, `a1_outer_popup_disappears_then_key`, `a1_root_unmaps_then_key`, `a1_destruction_at_depth_4`). Per C-1, the W-SM-6 popup portion (`ensure_configured` / initial commit carrying buffer) remains open pending INC-7. OB-18 therefore remains OPEN in the register, blocking INC-7 exit.
  - **OB-21:** OPEN. libinput device semantics under pan accumulation (DV-4) destination is T4 (physical hardware testing); ownership remains INC-16 / T4.
- **Derived verification evidence (`ulimit -n 1024`, `CARGO_BUILD_JOBS=4`):**
  - Workspace test suite: 214 normal tests pass (core 70, T1 144: adversarial 13, input 17, keyboard 36, outputs 12, parity 6, popup_focus 32, popup_lifetime 17, slice 4, transform 7); probe-feature `t1_popup_focus` 36 tests pass (218 total).
  - Cargo fmt and clippy (`--workspace --all-targets --locked -- -D warnings`): PASS.
  - `./tools/verify-architecture`: PASS (14 source rules, 41 compile probes + 1 positive control, runtime checks [5b] 4, [5c] 5, [5d] 4, 55 seeded + 3 judge controls).
  - `./tools/verify-test-efficacy`: PASS (110 seeded, 110 DETECTED, 0 MISSED, 0 BROKEN, + 2 judge).
  - `./tools/check-foundation-docs` and `--self-test`: PASS (57 invariants, 20 formulas, 51 obligations).
  - `./experiments/c1-authority-spike/verify.sh`: PASS.
  - `./tools/verify-development-environment`: PASS.
  - CI verification on implementation commit `4c01cca2d43e38f4f3a80e9202ec1b7ab0430179`: run `37476629305` (all jobs green: foundation-docs, verify-architecture, workspace-build).
- **Ratification declaration:**
  **Phase F INC-4 is RATIFIED.**
  No production behavior was changed during ratification.
  The live system was untouched.
  **INC-5 has NOT begun.**

## PHASE F — INC-4 CORRECTIVE PROGRAM — ROUND 4 (record: evidence/phase-f/INC-4/corrective-r4/)
- **Review:** `INC-4 FAIL — FURTHER CORRECTION REQUIRED` (fourth review, `audit/phase-f-inc4-rereview-3/`, ingested byte-for-byte as `dc4d52e`; reviewed HEAD `60e5429`). Findings: Level A 1 (A1), Level B 3 (B1–B3), Level C 4 (C1–C4).
- **Corrective commits:** `b320cbe` (matrix, reconstruction), `a63b7c5` (A1, B1), `9da07d7` (C3), `d75746b` (tests, oracles), `cf743e5` (verification); each passes `cargo test --workspace --locked` and `fmt` on its own.
- **A1 (Level A):** client-driven close of a popup chain of depth ≥ 2 left the keyboard grab installed and Smithay focus on a dead popup. Popup death now runs `reconcile_popup_chain`: the keyboard moves to the topmost live grabbing popup at once; when none is live, exactly once, the keyboard and pointer grabs are released by their install serials and the keyboard is routed to Core's ActiveElementFocus root. **Provenance:** introduced by INC-4 itself (`d5b7b95` reproduces it); cannot exist on the ratified INC-3 baseline `1ee520fa` (no keyboard, no popup grabs) — not an inherited foundation defect.
- **B1 (Level B):** the Round-3 R2-01 fix ran the parent's M1 lifecycle on popup death (unmapped parent re-armed, re-show unanswered). New `Core::toplevel_tree_changed` (snapshot only, never M1) serves popup death and child-surface commits; M1 runs only on the root surface's own commit. The same split removes the pre-existing child-commit variant (fourth-review O-1). R2-01 stays fixed (former popup area hits the parent).
- **B2 (Level B):** efficacy controls I and K were BROKEN (stale anchors after a rustfmt reflow); re-derived from current source. `verify-test-efficacy` and `verify-development-environment` PASS.
- **B3 (Level B):** F15/F16 detected by a literal FIFO/fallback provenance test; N09d (W-SM-15) detected by the default-suite exact ORDERED keyboard-wire test; NEG-1, NEG-2, OBS-1 now run unmutated in `verify-architecture` [5d] (CI).
- **C1 correction (appended; the Round-3 text is unchanged):** the Round-3 R2-10 entry called the provenance fallback "Rule 2 (output containing the pointer cursor)". That was wrong. Canonical D-B6-BOUNDARY:
  - **Rule 1** is the output of the input event that caused the popup — the grab serial, *else the seat's last input* — if the parent is visible there.
  - **Rule 2** is the parent's frame-clock output (highest-refresh observing output).
  - **Rule 3** is no constraint adjustment.

  The implementation already matches. A recorded serial resolves to its causing output; a missing, evicted or keyboard serial uses the pointer's current output under Rule 1's "else the seat's last input" clause; Rule 2 applies when the parent is not visible on that output. No code change.
- **C2 correction (appended):** the Round-3 entry claimed
  - "34 compile probes", "54 seeded controls" and `verify-test-efficacy` PASS;
  - that `sync_seat_key` "asserts".

  The fourth review measured 40 + 1 positive compile probes, 55 seeded + 3 judge architecture controls, and efficacy **FAIL (2)** with 99 seeded (97 detected, 2 BROKEN) + 2 judge. `sync_seat_key` fails closed without an assert. Cause: the record was written without mechanically deriving the counts or running the final gate.
- **C3 (disposition):** the token now carries the key event it authorizes (`verify_input_port(port, key, pressed)`, `sync_seat_key(token)`). The duplicate/orphan transition gate moved from the headless backend into `sync_seat_key` (one implementation for every backend). Compile probe p31 pins single use (E0382). Residual (owner-level): a trusted port holder can mint a proof without submitting the event; the port is the root of trust, and Core cannot hold XKB (C4.4).
- **C4 (disposition):** KV-2 reference model tracks CameraPan from the test sequence; F-4 asserts exactly one enter since the focus change; the provenance test checks FIFO order, retention and fallback, not only size; NEG/OBS gated.
- **Equivalent mutations retired/recorded:** "chain end keeps the keyboard grab" and "reconcile to the chain's `current_grab()`" — argued from pinned Smithay and the grab-record invariant in the matrix §Closure.
- **Verification (mechanically derived; `ulimit -n 1024`, `CARGO_BUILD_JOBS=4`):**
  - **Cargo:** check, build and test PASS. **214 tests** = core 70 + T1 144 (adversarial 13, input 17, keyboard 36, outputs 12, parity 6, popup_focus 32, popup_lifetime 17, slice 4, transform 7); probe-feature `t1_popup_focus` 36 (218 in total). fmt and clippy `-D warnings` PASS.
  - **`verify-architecture`:** PASS. 14 source rules; 41 compile probes + 1 positive control; runtime checks [5b] 4, [5c] 5, [5d] 4; 55 seeded + 3 judge controls.
  - **`verify-test-efficacy`:** PASS. 110 seeded, 110 DETECTED, 0 MISSED, 0 BROKEN, + 2 judge.
  - **Other gates:** foundation checker PASS (57 invariants, 20 formulas, 51 obligations); self-test PASS; authority spike PASS; `verify-development-environment` PASS.
  - **Fresh mutation round:** 22 mutations; 20 detected, 0 meaningful missed, 2 equivalent, 0 broken.
  - **Stress:** 200 cycles; asserted clean end state; FDs 10 → 10; 0 violations.
- **INC-3 regression:** all ratified INC-3 suites and efficacy controls pass, including CameraPan/KeepFocus/true-position, zero output, ProjectionFrame invalidation, transform/hit agreement and the bounded-resource test.
- **Obligations:**
  - OB-4: OPEN, behaviour met; awaiting review.
  - OB-15: OPEN, behaviour met; awaiting review.
  - OB-30: OPEN, behaviour and NEG/OBS gate met; awaiting review.
  - OB-18 (popup part): closable only at ratification.
  - OB-21: OPEN (INC-16 / T4).
- **CI (exact HEAD):** run 37475576796 on `80a5328a6ba338ae4d2a8025e35b471c83649628` — foundation-docs, verify-architecture (incl. [5d]) and workspace-build all success; 214 tests; `verify-test-efficacy: PASS` (110 DETECTED, 0 FAIL). This entry's own commit is verified by the CI run on it.
- **LIVE_SYSTEM_TOUCHED:** no.
- **NEXT:** STOP for the fifth independent adversarial review. INC-4 is NOT ratified. INC-5 has NOT begun.

## PHASE F — INC-4 CORRECTIVE PROGRAM — ROUND 3 (record: evidence/phase-f/INC-4/corrective-r3/)
- **Review:** `INC-4 FAIL — FURTHER CORRECTION REQUIRED` (dossier preserved byte-identically in `audit/phase-f-inc4-rereview-2/`, commit `033be36`). Findings: Level A 0, Level B 7 (R2-01–R2-07), Level C 3 (R2-08–R2-10).
- **Corrective Finding Matrix:** `evidence/phase-f/INC-4/corrective-r3/CORRECTIVE_FINDING_MATRIX.md`.
- **Core Architecture & Authority Corrections:**
  - **R2-01 (Level B, pre-existing INC-3 foundation defect resolved):** In `crates/infynitae-wayland/src/handlers.rs`, implemented `XdgShellHandler::popup_destroyed` callback. When client destroys a popup surface, Wayland handler purges the popup from popup state, clears any active popup pointer grab if the destroyed surface held it, and notifies the core via `element_committed` to re-extract the parent toplevel snapshot immediately without requiring an asynchronous parent redraw commit. Note: This defect pre-existed in the ratified INC-3 foundation baseline (`1ee520fa`), where destroyed popup surfaces left stale snapshots and grabbed states; it is now fully resolved and guarded by permanent regressions `t99g_popup_destroyed_by_client_then_clicks` and `t99j_stale_popup_area`.
  - **R2-02 (Level B):** Bound `InputAuthToken` capability proof. `InputAuthToken` is non-Send (`_not_send: NotSend`) and bound to the issuing `Core` instance via `core_id: u64`. `WaylandState::sync_seat_key` consumes the token by value (`token: InputAuthToken`), asserting `token.core_id() == self.core.id()`. Token cannot be forged, transferred across threads, or reused. Probes `p25_call_sync_seat_key_without_auth_token` and `p30_input_auth_token_send` enforce capability invariants at compile time. Runtime tests `t02b_token_is_bound_to_its_core` and `t02c_token_consumed_and_cannot_be_reused` verify one-shot consumption and core binding.
  - **R2-03 (Level B):** Adapter keyboard encapsulation. Restricted `keyboard_handle(&self)` visibility to `pub(crate)`. Replaced direct keyboard handle access in backend tests with read-only state observers (`keyboard_is_grabbed`, `keyboard_current_focus`, `keyboard_modifier_state`, `serial_outputs_len`). Added enforcement probe `p29_keyboard_handle_outside_adapter` (verifying compile error E0624 outside `infynitae-wayland`) and structural source rule `smithay-keyboard-only-in-adapter` in `tools/verify-architecture`.
  - **R2-04 (Level B):** Pre-Smithay duplicate key/modifier filter. In `crates/infynitae-backend-headless/src/lib.rs`, `keyboard_key` checks `is_transition`: physical duplicate presses (already pressed) and orphan releases (already released) bypass `sync_seat_key`, preventing seat XKB modifier corruption. Tested independently for regular keys, Shift, Super, Ctrl, and Alt across repeat press, multi-repeat, focus transfer, zero outputs, and release sequences in `t04_duplicate_presses`.
  - **R2-05 (Level B):** Pan dismissed popup grab & synthetic motion boundary. Implemented `PanDismissedPopupGrab` and `set_pan_dismissed_pointer_grab` in `crates/infynitae-wayland/src/pointer.rs`. At CameraPan acquisition (`Effect::DismissPopupGrab`), replaces `PopupPointerGrab` with `PanDismissedPopupGrab` keeping focus (`Focus::Keep`) and leaking zero events (preserving INV-INPUT-3). Upon pan completion/reconciliation, unsets with `restore_focus: false` and delivers the true-position motion without synthetic motion replay. In `outputs.rs`, `Effect::KeyboardFocus` checks `popup_pointer_grab_serial` to cleanly unset 3-level nested grabs. In `pointer.rs`, `PointerZeroOutputEnd` verifies the current grab target is an alive popup surface before reinstalling. Permanent regression tests `t01b_nested_pan_dismissal`, `t06a_three_level_nested_distinct_serials_focus_transfer`, `t06d_implicit_button_grab_then_zero_outputs_with_popup`.
  - **R2-06 (Level B):** Unconfigured key bindings. Removed test fixtures `KEY_B`, `KEY_ESC`, `KEY_LEFTSHIFT` from `CANONICAL_BINDINGS` in `crates/infynitae-core/src/input/keys.rs`. Only `Super+Enter -> FocusNext` remains canonically bound. Unconfigured keys (Super+B, idle Esc) are never swallowed and pass through to focused client wire. Permanent regression test `t11_unconfigured_keys_are_never_swallowed`.
  - **R2-07 (Level B):** Verification gaps and mutant detection. Added M08 test `serial_outputs_storage_bounded_at_capacity` in `tests/t1_outputs.rs`. In `r17_long_lived_stress_bounded_state`, asserted zero protocol violations (`violations.len() == 0`) and zero depressed modifiers (`depressed == 0`). Updated N08 and N09 mutants in `tools/verify-test-efficacy` targeting default-suite tests `t01b_nested_pan_dismissal` and `t06a_three_level_nested_distinct_serials_focus_transfer`, and added M08 mutant control.
  - **R2-08 (Level C):** Removed production ordering instrumentation (`popup_preemption_pending`, `popup_preemption_ordering_violated`) and production `assert!` from `crates/infynitae-wayland`. Preemption ordering verified via external observers and test assertions.
  - **R2-09 (Level C adjudication / canonicalization):** Formally canonicalized owner adjudication for INC4R-10: Smithay synthetic focus restoration motion is strictly an internal Smithay mechanism and must NEVER cross the client-visible protocol wire in states where an Infynitae invariant (such as INV-INPUT-3 during camera pan) requires suppression. The camera pan grab isolation (`PanDismissedPopupGrab`) cleanly preserves this boundary.
  - **R2-10 (Level C adjudication / canonicalization):** Formally canonicalized D-B6-BOUNDARY Rule-1 fallback: when an interaction serial is unrecorded, evicted from the serial LRU ring buffer (>64 entries), or caused by a non-pointer event (e.g. keyboard serial without a tracked output), `Core::pointer_output()` correctly falls back to Rule 2 (output containing the pointer cursor, or primary output if unmapped), maintaining deterministic output assignment without panic or stall.
- **Enforcement & Verification:**
  - `verify-architecture`: PASS (34 compile probes including p29 and p30, 54 seeded controls, all structural source rules passing).
  - `verify-test-efficacy`: PASS (all mutants detected including M08, N08, N09; judge controls PASS).
  - `check-foundation-docs`: PASS.
  - `verify-development-environment`: PASS.
  - Full workspace regression suite: PASS (195 tests, 100% passing under `ulimit -n 1024`).
- **NEXT:** STOP for fourth independent adversarial review. Do NOT declare ratified. Do NOT begin INC-5.

## PHASE F — INC-4 CORRECTIVE PROGRAM — ROUND 2 (record: evidence/phase-f/INC-4/corrective-r2/)
- **Review:** `INC-4 FAIL — FURTHER ARCHITECTURAL CORRECTION REQUIRED` (dossier preserved byte-identically in `audit/phase-f-inc4-rereview/`, commit `9efcf85`). Findings: Level A 1 (INC4R-01), Level B 8 (INC4R-02–INC4R-09), Level C 3 (INC4R-10–INC4R-12).
- **Corrective Finding Matrix:** `evidence/phase-f/INC-4/corrective-r2/CORRECTIVE_FINDING_MATRIX.md` (commit `99ec2f0`).
- **Core Architecture & Authority Corrections:**
  - **INC4R-01 (Level A blocker):** In `Effect::DismissPopupGrab(target)`, resolved `target_surface` and invoked `kb.set_focus` after `g.ungrab(PopupUngrabStrategy::All)` and `keyboard_unset_grab()`. Eliminates focus divergence and protocol violations where keys reached surfaces without entered keyboard focus. Permanent regression tests `r16_keyboard_focus_after_pan_dismissal` and `r17_long_lived_stress_bounded_state`.
  - **INC4R-02 (Level B):** Capability sealing. Minted non-forgeable `InputAuthToken` via `Core::verify_input_port(&self, &InputPort)`. `sync_seat_key(&InputAuthToken)` requires proof of authorization. Compile probe `p25_call_sync_seat_key_without_auth_token` ensures capability cannot be forged. Runtime test `set_focus_rejects_foreign_port` verifies refusal on foreign ports.
  - **INC4R-03 (Level B):** Adapter surface minimality. Removed unconstrained `WaylandState::test_set_keyboard_grab`, `test_simulate_focus_without_core`, `set_ordered_preemption_strategy`, `set_nested_grab_keep_pointer` from default builds; gated behind explicit compile-time test probe features `probe_test_keyboard_grab`, `probe_preemption_strategy`, and `probe_nested_grab_keep_pointer`. Enforcement compile probes `p26`, `p27`, `p28` ensure production builds lack these test escape hatches.
  - **INC4R-04 (Level B):** Wire repeat semantics. In `entry.rs`, repeat client press is ignored (`owner(key) == Some(Owner::Client)` returns `Vec::new()`), matching Linux input and Wayland protocol expectations and preventing spurious repeat forwarding.
  - **INC4R-05 (Level B):** Anchor output causality. `place_popup` threads causing event serial `Option<Serial>` and looks up `serial_output(s)` (D-B6 Rule 1). Tested in `r15_rule1_uses_the_causing_event_output`. Mutant N21 permanently killed.
  - **INC4R-06 (Level B):** Popup preemption lifecycle across zero outputs and distinct serials. Updated `outputs.rs` to check both `g.serial()` and `g.previous_serial()` on preemption. Updated `pointer.rs` to reinstall popup grab across zero-output reconnect. Tested in `r09b_nested_preemption_with_distinct_serials` and `r12_popup_grab_semantics_across_zero_outputs`.
  - **INC4R-07 (Level B):** Headless intake modifier authority. Backend `pointer_button` uses seat XKB modifier state as sole source of truth (`state.seat_modifiers()`), not synthetic or external flags.
  - **INC4R-08 (Level B):** Verification gaps and surviving mutants resolved. Permanent tests and controls added for N08 (OB-4 keyboard unset), N09 (focus before ungrab ordering check), N17 (`set_focus` port check), N18 (`set_focus` mapped check), and N21 (rule-1 wiring). Circular KV-2 oracle replaced with independent truth oracle.
  - **INC4R-09 (Level B):** Test and control inventory mechanically exact and audited across workspace.
  - **INC4R-10 (Level C adjudication):** Formally adjudicated: Smithay synthetic focus restoration motion is acceptable ONLY as an internal implementation mechanism when it cannot cross client-visible boundary where an Infynitae invariant requires suppression. Canonical invariants (INV-INPUT-3) strictly preserved.
  - **INC4R-11 (Level C adjudication):** Formally adjudicated: Canonical product key binding policy vs test fixtures. Esc binding only active during in-progress interaction; unmapped keys pass through to client in idle state.
  - **INC4R-12 (Level C adjudication):** Formally adjudicated: Append-only integrity of `CHECKPOINT.md` restored. Historical pre-INC-4 entries restored byte-identically; Round 1 and Round 2 corrective entries appended.
- **Enforcement & Verification:**
  - `verify-architecture`: PASS (32 compile probes, 54 seeded controls, all structural source rules passing).
  - `verify-test-efficacy`: PASS (73/73 seeded behavioural defects detected, including N08, N09, N17, N18, N21; judge controls PASS).
  - `check-foundation-docs`: PASS.
  - `verify-development-environment`: PASS.
  - Full workspace regression suite: PASS (100% passing under `ulimit -n 1024`).
- **NEXT:** STOP for third independent adversarial review. Do NOT declare ratified. Do NOT begin INC-5.

## PHASE F — INC-4 CORRECTIVE PROGRAM — ROUND 1 (record: evidence/phase-f/INC-4/corrective/)
- **Review:** `INC-4 FAIL — ARCHITECTURAL CORRECTION REQUIRED` (dossier preserved byte-identically in `audit/phase-f-inc4-review/`, commit `19231ef`). Findings: Level A 1 (F01), Level B 11 (F02–F09, F12, F14, F16), Level C 4 (F10, F11, F13, F17), Observation 1 (F15).
- **Corrective Finding Matrix:** `evidence/phase-f/INC-4/corrective/CORRECTIVE_FINDING_MATRIX.md` (commit `aa216d3`).
- **Core Architecture & Authority Corrections:**
  - **F01 (Level A):** Intake authority restored. Removed `Core::key_ownership_mut()`. Routed keyboard intake through `Core::physical_input(&mut InputPort, PhysicalEvent::KeyboardKey)`. Core evaluates bindings and ownership, emitting `Effect::KeyboardForward` and `Effect::KeyboardIntercept`. `ElementId::new` and ownership press/release made crate-private (`pub(crate)`). Probes `p22_mutate_key_ownership`, `p23_forge_element_id`, `p24_set_focus_without_port` added.
  - **F02 (Level B):** Removed public test switch `k3_enforced` from `WaylandState`. K3 observe mode enforced unconditionally in Core (`entry.rs`).
  - **F03 (Level B):** Fixed K1/K2 dispatch. Re-press of held compositor key consumed (`KeyboardIntercept`). Orphan release dropped (never forwarded).
  - **F04 (Level B):** Plain Escape only cancels when an interaction is actively in progress (`interaction.active()`); forwarded to focused client otherwise.
  - **F05 (Level B):** CameraPan acquisition dismisses popup grabs via `Effect::DismissPopupGrab` without invoking `ptr.unset_grab`, preventing synthetic motion replay.
  - **F06 (Level B):** D-B6-BOUNDARY Rule 1 placed on production path via `Core::pointer_output()`.
  - **F07 (Level B):** Port-gated `Core::set_focus` validates target is mapped (`Lifecycle::Mapped`). `mapped_elements()` added to `Stage`. Unmapping focused element triggers target loss.
  - **F08 (Level B):** Refused grab on unfocused/mismatched surface immediately sends `xdg_popup.popup_done` and returns.
  - **F09 (Level B):** `self.popups.cleanup()` invoked prior to grab preemption so dead grabs do not unset unrelated implicit button grabs. Grab state cleared across zero-output transitions if popup grab was active.
  - **F10 (Level C):** Formally adjudicated: Smithay synthetic motion on ungrab is standard Wayland behavior, isolated under CameraPan, tracked under W-SM-16 for INC-5 locks.
  - **F11 (Level C):** Formally adjudicated: Successor fallback without focus history transitions to `None` (`Unfocused`). Arbitrary z-order fallback eliminated.
  - **F12 (Level B):** Shipped verification rebuilt with independent oracles, non-circular assertions, permanent regression tests ATK-01..ATK-18, and RES-1 resource stability.
  - **F13 (Level C):** Formally adjudicated: Reassigned OB-21 to INC-16 / T4 physical libinput intake.
  - **F14 (Level B):** Corrected test and control counts across all evidence documents.
  - **F15 (Observation):** Reconciled brief SHA-1 hash to actual reviewed commit `d5b7b95cbc910c48377c72ae94ccec2b0c05e140`.
  - **F16 (Level B):** Unified modifier authority via `WaylandState::seat_modifiers()`. Pointer button intake merges seat XKB modifiers.
  - **F17 (Level C):** Formally adjudicated: Unmapping or destroying a parent window dismisses its child popups via `popup_done`.
- **Enforcement & Verification:**
  - `verify-architecture`: PASS (34 compile probes including p22, p23, p24; all structural source rules passing).
  - `verify-test-efficacy`: PASS (all 14 previously surviving mutations F, I, J, K, L, N, P, Q, R, T, U, V, W, AB permanently detected; judge controls PASS).
  - `check-foundation-docs`: PASS.
  - `verify-development-environment`: PASS.
  - Full workspace regression suite: PASS (100% under `ulimit -n 1024`).
- **NEXT:** STOP for independent adversarial re-review. Do NOT declare ratified. Do NOT begin INC-5.

## PHASE F — INC-4 IMPLEMENTATION (evidence: evidence/phase-f/INC-4/)
- **Scope:** PRODUCT, HEADLESS, canonical INC-4 (mapping preserved before implementation: `IMPLEMENTATION_MAPPING.md`).
  - **core:**
    - `input::keys`: canonical bindings (Super+Enter -> FocusNext, Super+B -> Noop, Esc -> Cancel, Shift -> Noop), pure binding resolver;
    - `input::focus`: `FocusModel` (`OBJ-22`, `M11`, `INV-INPUT-8`), explicit focus transitions, `focus_next`, target loss successor selection with live element fallback;
    - `input::mod`: `InputState` extended with `key_ownership: Ownership` (`OBJ-25`);
    - `entry.rs`: click-to-focus on client button press, `Effect::DismissPopupGrab` on CameraPan acquisition (`OB-4`);
    - `protocol.rs`: `Effect::KeyboardFocus`, `Effect::ToplevelConfigure`, `Effect::DismissPopupGrab`; popup constraint target calculation with anchor-output rule 1 (`D-B6-BOUNDARY`, `INC3-F08`).
  - **adapter (`infynitae-wayland`):**
    - `keyboard.rs`: `WaylandState::keyboard_key` two-phase dispatch (`input_intercept` + `input_forward`), K1–K9 ownership semantics, K3 observe mode enforcement, `PreemptionStrategy` configuration;
    - `pointer.rs`: strictly encapsulated grab helpers (`set_popup_pointer_grab`, `unset_pointer_grab`, `set_keyboard_grab`, `unset_keyboard_grab`) preserving `smithay-pointer-only-in-adapter-pointer` compliance;
    - `handlers.rs`: `XdgShellHandler::grab` with `W-SM-7` root checks and `W-SM-14` nested grab retention; `toplevel_destroyed` successor focus transition;
    - `outputs.rs`: atomic ORDERED preemption under `Effect::KeyboardFocus` (ungrab(All) -> kb.unset -> ptr.unset -> set_focus) and `Effect::ToplevelConfigure` (sets/unsets `Activated`).
  - **headless backend:** `keyboard_key` intake validating port authority via `state.core().check_port(port)`.
- **Not implemented (owned later):**
  - Pointer constraints, locks, and confinement (INC-5);
  - Epochs and frame pacing (INC-6);
  - Fullscreen and maximize (INC-7);
  - Real-client integration and kick (INC-8);
  - Zoom navigation and continuous scaling (INC-9/10).
- **Tests:** 100% passing under `ulimit -n 1024`.
  - `t1_keyboard.rs`: 13 tests (K1–K9, NEG-K3 modifier control, OB-15 output removal, OB-15 repeat probe).
  - `t1_popup_focus.rs`: 13 tests (PF-1 through PF-7, PF-OB4 pan dismissal, PF-WSM7 unmapped grab rejection, NEG-1 naive control, NEG-2 pointer-first control, OBS-1 anvil order observation).
  - Full workspace regression suite passes with 0 failures and 0 warnings.
- **Enforcement:**
  - `verify-architecture`: PASS (31 compile probes, 54 seeded controls, all structural source rules intact).
  - `verify-test-efficacy`: PASS (68/68 seeded behavioural defects detected, including 5 new INC-4 mutation controls).
  - `check-foundation-docs`: PASS (57 invariants, 20 formulas, 51 obligations intact).
  - `verify-development-environment`: PASS.
- **Obligations:**
  - None closed by implementation. All obligations remain pending independent ratification review.
- **NEXT:** STOP for independent adversarial review by another agent. Do NOT declare ratified. Do NOT begin INC-5.

## PHASE F — INC-3 RATIFICATION (record: evidence/phase-f/INC-3/corrective/)
- **Review:** `INC-3 CONDITIONAL PASS — CORRECTIONS REQUIRED BEFORE RATIFICATION` (dossier recorded unchanged in `85d128a`). Findings INC3-F09 (Level B) resolved; INC3-F08 (Level C) and INC3-F06 (Observation) adjudicated.
- **Corrective patch `0187344` (tests and controls only; production code unchanged):**
  - **INC3-F09:** Resolved file-descriptor exhaustion under standard limit `ulimit -n 1024`. Single shared dynamically-resizing shm pool in test harness `Probe` reduces shm descriptors from 192 client/server FDs to 1 client/server FD per probe client. `Drop for Lockstep` deterministically terminates server client and purges buffer resources via `dispatch_single_client`. Peak test suite FD usage reduced from >1939 to 23 FDs (14 FDs in `ee1_c5`), returning to baseline 9 FDs.
  - **Regression test:** Permanent test `inc3_f09_bounded_resource_cycles` in `crates/infynitae-backend-headless/tests/t1_transform.rs` asserts bounded FD growth ($\le 5$) over 20 repeated Lockstep + 192-subsurface cycles.
- **Tooling isolation `ffc9725` (INC3-F06):** Isolated `target/test-efficacy-<pid>-<rand>` per execution in `tools/verify-test-efficacy` with automatic cleanup, eliminating concurrency races.
- **Adjudication (INC3-F08):** Confirmed Level C finding that popup anchor-output rule 1 belongs canonically to INC-4 (`OB-4` / `OB-18`).
- **Obligations:**
  - `OB-45`: **CLOSED** (Render and hit-test agreement under coordinate transformations across all 10 cases passes cleanly under `ulimit -n 1024`; 0 mismatches; sealed control fails 9/9 non-Normal cases).
  - Still `OPEN`: `OB-50` (constraints in INC-5), `OB-3` (constraint reconciliation in INC-5), `OB-21` (T4 intake in INC-4), `OB-4` (popup grab handling in INC-4), `OB-1`, `OB-6`, `OB-11`, `OB-14`, `OB-17`, `OB-48`.
- **Verification:** everything PASS under `ulimit -n 1024`.
  - Cargo: 122 tests (66 core T0, 50 distinct T1, 6 oracle self-tests), fmt, clippy `-D warnings`.
  - Verifiers: `verify-architecture` (31 compile probes, 54 seeded controls); `verify-test-efficacy` (78/78 detected, judge controls PASS); development environment verifier PASS.
- **Findings:** open Level A 0, Level B 0, Level C 2 (INC1-F01 in INC-7, INC3-F08 in INC-4).
- **LIVE_SYSTEM_TOUCHED:** no.
- **NEXT:** INC-4 only upon explicit owner authorization. INC-4 has NOT begun.

## PHASE F — INC-3 (implemented on baseline `6470e1a`; evidence: evidence/phase-f/INC-3/)
- **Scope:** PRODUCT, HEADLESS, canonical INC-3 (mapping written before code: `IMPLEMENTATION_MAPPING.md`).
  - **core:**
    - `input::{ownership, interaction, bindings}`;
    - `PhysicalEvent` variants (motion, button with a modifier snapshot, axis);
    - `physical_input`;
    - `navigation::CameraSet::pan` + `pan_target` (F-PAN-1, R2, saturating at ±2³⁶);
    - `pointer::{moved, locate}` (Arrangement union clamp);
    - `Core::route_pointer` through `hit_test`;
    - M10 pointer effects on connect, reconfigure and disconnect;
    - orphaning (C4.6);
    - a camera change drops the output's ProjectionFrame.
  - **adapter:**
    - seat `seat0` with a pointer, `zwp_relative_pointer_manager_v1`;
    - `pointer` module (the only `PointerHandle` caller);
    - the zero-output grab (`Focus::Clear`, then `motion(None)`);
    - Canvas↔Smithay global conversion in `convert`.
  - **headless backend:** injected virtual-mouse input; `vblank` produces the output's frame.
- **Not implemented (owned later):**
  - keyboard and XKB, popup grabs, D-B6-BOUNDARY rule 1 (INC-4);
  - constraints and reconciliation with locks (INC-5);
  - epochs (INC-6), fullscreen (INC-7), kick (INC-8);
  - zoom (gated by OB-24, INC-9/10).
- **Tests:** 121 passing.
  - T0 66 (13 new).
  - T1: 49 distinct tests (30 new: x8 C1/C6/C7/C8/C9, EE-3, orphaning, adversarial A–N, EE-1 C5 across 10 transform cases with the sealed hit rule failing 9/9, pan render/hit agreement), plus the oracle self-test in each of the 6 T1 binaries.
- **Enforcement:**
  - `verify-architecture` PASS: 31 compile probes; 54 seeded controls; new rules `effects-only-from-core` and `smithay-pointer-only-in-adapter-pointer`; [5c] INV-INPUT-3 bypass switch caught on T0 and T1; `physical_input` port binding at runtime (DEFERRED: `shell_command` only).
  - `verify-test-efficacy` PASS (76/76, 30 new INC-3 controls).
  - Checker PASS (self-test PASS); spike PASS; environment verifier PASS.
  - fmt and clippy `-D warnings` clean. No new dependencies; smithay `=0.7.0`; no unsafe; P1 not applied.
- **Obligations (none closed):**
  - OB-45: all parts' pass criteria met (tracker INC-2, hit INC-3); OPEN pending review.
  - OB-50: pointer part satisfied; OPEN (INC-5).
  - OB-3: CameraPan orphaning satisfied; OPEN (INC-5).
  - OB-21: T0 part; OPEN (T4).
  - OB-4: acquisition point only; no popup grab can exist before INC-4; OPEN.
- **Findings:** no new Level A/B/C; observations INC3-F01…F08 (README §8). Open: Level C 1 (INC1-F01, INC-7).
- **LIVE_SYSTEM_TOUCHED:** no.
- **NEXT:** independent adversarial review of INC-3. Do not begin INC-4.

## PHASE F — INC-2 RATIFICATION (record: evidence/phase-f/INC-2/corrective/)
- **Review:** `INC-2 CONDITIONAL PASS — CORRECTIONS REQUIRED BEFORE RATIFICATION` (dossier recorded unchanged in `b229462`). Findings INC2-F07 (Level B) and INC2-F01 (Level C) adjudicated.
- **Corrective patch (tests and controls only; production code unchanged):**
  - **INC2-F07:** `reconfiguration_drops_the_output_frame` in `crates/infynitae-core/src/entry.rs` verifying that mode shrink, 90° transform change, and scale change immediately invalidate and drop the cached `ProjectionFrame`, preventing stale hit-testing or frame reuse before a new frame is rendered.
  - **Controls:** Permanent mutation control in `tools/verify-test-efficacy` (INC2-F07: neutralizing frame invalidation on reconfiguration), detected (46/46).
- **Decision E Ratification:**
  - Formally ratified Decision E: OB-49 ownership correction transferring protocol fullscreen configure to INC-7 (`INV-PRES-1`, `OB-22`/`OB-32`) and ScaleInteractionEpoch settlement to INC-6 (`ScaleInteractionEpoch`, `OB-13`/`OB-47`).
  - All INC-2 reconfiguration criteria and runtime 2i negative controls are satisfied.
  - OB-49 is marked `CLOSED`.
- **Verification:** everything PASS.
  - Cargo: 76 tests (53 core, 19 distinct T1 + 4 self-tests), fmt, clippy `-D warnings`.
  - Verifiers: `verify-architecture` (27 probes, 47 seeded controls); `verify-test-efficacy` (46/46).
  - Repository checks: checker 57/20/51 (self-test 18/18), spike, environment verifier.
- **Obligations:**
  - OB-49 `CLOSED`.
  - Still `OPEN`: OB-1, OB-6 (T4 part), OB-11 (P1 gate), OB-14, OB-17, OB-45 (hit-test portion owned by INC-3), OB-48, OB-50 (adapter/constraint portions owned by INC-3/5), OB-3 (interaction orphaning owned by INC-3).
- **Findings:** open Level A 0, Level B 0 (INC2-F07 resolved), Level C 1 (INC1-F01, INC-7).
- **LIVE_SYSTEM_TOUCHED:** no.
- **NEXT:** INC-3 only upon explicit owner authorization. INC-3 has NOT begun.

## PHASE F — INC-2 (implemented: `0a03aab`; evidence: evidence/phase-f/INC-2/)
- **Scope:** PRODUCT, HEADLESS, canonical INC-2.
  - **core:** output manager (M4/M5, OutputKey, PersistedCameras, Arrangement), PointerState (lifecycle part), visibility pass (OBJ-13) and frame clock.
  - **adapter:** wl_output + xdg-output + presentation globals, enter/leave, damage trackers (`phys_O`), frame completion.
  - **headless backend:** hotplug, reconfiguration, disconnection, virtual vblank.
- **Not started (owned later):** zero-output seat grab and routed motion (INC-3), reconciliation (INC-5), epoch settlement (INC-6), fullscreen configure (INC-7), kick and fallback (INC-8).
- **Tests:** 75 passing (T0 core 52; T1 19 distinct tests plus the oracle self-test in each of the 4 T1 binaries). Runtime 2i passes with the 1:1 rule and fails with "1:1 violated" under the sealed rule, on T0 and T1 (`verify-architecture` [5b]). EE-1 tracker matrix on the production adapter, with the sealed control failing 7/7 rotated cases.
- **Enforcement:** `verify-architecture` PASS (27 probes, 47 seeded controls); `verify-test-efficacy` PASS (45/45, 21 new INC-2 controls); checker 57/20/51, self-test 18/18; no new dependencies; smithay `=0.7.0`; no unsafe; P1 not applied (OB-11 open).
- **Obligations:** OB-49 mechanically satisfied except items that cannot execute yet (fullscreen configure, epoch settlement); OB-45 (tracker/membership/damage part), OB-50 (core part) and OB-3 (removal part) partly satisfied; all `OPEN`.
- **Findings:** Level A 0, Level B 0, Level C 2 (INC1-F01, INC2-F01).
- **LIVE_SYSTEM_TOUCHED:** no.
- **NEXT (at implementation):** independent adversarial INC-2 review. Superseded by the ratification entry above.

## PHASE F — INC-1 RATIFICATION (record: evidence/phase-f/INC-1/corrective/)
- **Review:** `INC-1 CONDITIONAL PASS — CORRECTIONS REQUIRED BEFORE RATIFICATION` (dossier recorded unchanged in `38c60ca`). Findings INC1-F06 and INC1-F07 (Level B); INC1-F01…F05 adjudicated.
- **Corrective patch `1fa72af` (tests and controls only; production code unchanged):**
  - **INC1-F06:** an independent exact-integer unprojection and region oracle for T1, plus a 56-row hand-derived T0 table.
  - **INC1-F07:** T0 and T1 coverage of unshown surfaces.
  - **Controls:** 7 permanent mutation controls (ADV-MUT-3 ×3, sign inversion, camera-offset omission, ADV-MUT-2 ×2), all detected.
- **Verification:** everything PASS.
  - Cargo: 47 tests, fmt, clippy.
  - Verifiers: `verify-architecture`; `verify-test-efficacy` 25/25.
  - Repository checks: checker 57/20/51 (self-test 18/18), spike, environment verifier.
  - CI: run 37291919770 on `1fa72af`.
- **Obligations:**
  - OB-2 and OB-26 `CLOSED`.
  - Still `OPEN`: OB-1, OB-6 (T4 part), OB-14, OB-17, OB-49 (runtime 2i, INC-2).
- **Findings:** open Level A 0, Level B 0, Level C 1 (INC1-F01, INC-7).
- **NEXT (at ratification):** INC-2 upon explicit owner authorization. Superseded by the INC-2 entry above.

## PHASE F — INC-1 (evidence: evidence/phase-f/INC-1/)
- **Scope:** PRODUCT, HEADLESS, exactly the canonical INC-1.
  - **core:** `stage`, `snapshot` + F-ORIGIN-1, `projection` (F-VIEW-1, F-LATTICE-1, F-UNPROJECT-1), `render` (ProjectionFrame, F-HIT-1), M1 lifecycle in `protocol`, single-output `output_connected` creating and binding one camera.
  - **adapter:** compositor/xdg-shell handlers, snapshot extraction, `SurfaceKey` registry, `convert` (F-RENDER-1).
  - **headless backend:** one virtual output, DummyRenderer.
- **Tests:** 32 core T0 and 7 T1 lockstep tests with a real probe client. Every pixel matches the presented surface at four output scales; xsnap/X-15, x6, xhit and the d5hit zoom-1 configuration are ported with their negative controls.
- **Enforcement:** `tools/verify-architecture` PASS.
  - **New rules:** `stage-single-writer`, `smithay-rejected-apis`, `smithay-geometry-only-in-convert`; unsafe scan now covers `tests/`.
  - **Dependencies:** dev-only `wayland-client`/`wayland-protocols` for the headless backend's T1 tests.
  - **Seeds:** 46 seeded controls.
- **Test efficacy:** new `tools/verify-test-efficacy` PASS (18/18 seeded behavioural defects caught; also in CI).
- **Not started:** multi-output, binding precedence, 2i (INC-2 / OB-49); input (INC-3/4); scale (INC-6); toplevel requests (INC-7, finding INC1-F01, Level C).
- **Registries:** unchanged (57 invariants, 20 `F-*`, OB-1…OB-51); checker PASS, self-test 18/18.
- **LIVE_SYSTEM_TOUCHED:** no.
- **NEXT (at implementation):** independent INC-1 review. Superseded by the ratification entry above.

## PHASE F — INC-0 (implemented: `632b1d0`; evidence: evidence/phase-f/INC-0/; review: audit/phase-f-inc0-review/)
- **Scope:** ENFORCE, HEADLESS. The production workspace (`crates/`: core, wayland, backend-headless, backend-winit, ipc, bin) carries the authority **shape** only; no product behaviour. Enforcement-only crates live in `enforcement/`.
- **Pins:** `rust-toolchain.toml` 1.99.0; `Cargo.lock` tracked; `smithay = "=0.7.0"` from crates.io in the adapter; no patch (P1 stays experimental; OB-11 open).
- **Enforcement:** `tools/verify-architecture` PASS: DAG (normal, dev, build, target); syn-based source rules (2g, 2h, mint location, wayland names, unsafe policy, authority escape prevention across value/ref/mut/ptr/aliases); 26 negative probes with asserted diagnostics plus 1 positive control; 30 seeded negative controls plus 3 judge controls. CI: `.github/workflows/ci.yml` (foundation-docs, verify-architecture, workspace-build).
- **Independent Review & Corrective Patch:** Independent adversarial review confirmed compiler probes, unsafe policy, and workspace isolation, identifying findings INC0-F01 (reference getters) and INC0-F02 (dev-dependencies). Corrective patch resolved both findings with seeded regression controls.
- **Decision D Ratification:** OB-19 ratified `CLOSED` with explicit transfer of runtime 2i validation (`probe_sealed_key_steal` / `concurrent_output_key_collision`) to INC-2 / OB-49. `INV-CAMERA-3` and `INV-OUTPUT-2` remain mandatory without waiver. OB-36 ratified `CLOSED` with CI workflow execution verified.
- **Registries:** unchanged (57 invariants, 20 `F-*`, OB-1…OB-51); checker PASS, self-test 18/18.
- **LIVE_SYSTEM_TOUCHED:** no.
- **NEXT:** INC-1 only upon explicit owner authorization.

## PHASE E FINAL REVIEW (recorded: `cfb54f091bd2a19065a45b32662e8f0bbddd62fc`)
- **Verdict:** `PHASE E PASS — PHASE F MAY BEGIN` (audit/phase-e-final/VERDICT.md). Baseline reviewed: E1R seal `1ae1b736515b8c52c3e6ba7596b22c81f7098390`, tracking `e9d36732ac4fa9e934ee1b90bb696e9545c598ab`. Zero unresolved Level-A findings.
- **Architecture:** unchanged by the review and by this entry; registries as in E1R (57 effective invariants; 20 `F-*`; OB-1…OB-51).
- **Downstream gates (from the verdict, not new):** OB-11 before INC-5; OB-24 (G-OB24) before INC-10; OB-34 (G-OB34) before INC-14; OB-45 at INC-3 exit; OB-46 for physical rotation claims; G-MIRROR; G-OB48.
- **NEXT:** Phase F entry increment **INC-0** (closes OB-19, unconditional entry gate), only on explicit owner authorization. Not begun.

## PHASE E CORRECTIVE CHECKPOINT E1R (sealed: `1ae1b736515b8c52c3e6ba7596b22c81f7098390`)
- **E1R_SEAL:** `1ae1b736515b8c52c3e6ba7596b22c81f7098390`. Independent re-review: `E1R PASS — READY TO SEAL AND RE-ENTER INDEPENDENT PHASE E REVIEW` (audit/phase-e-e1r-review/); accepted E1R-N1 correction (no second corrective layer) and P1 as the minimal pinned framework correction.
- **Record:** audit/E1R/ (README, DISPOSITIONS, REPRODUCTION, INVARIANT_DIFF, D8_REAUDIT, REGRESSION).
- **Layer:** FOUNDATION_ARCHITECTURE §C12.E1R amends 6 invariants (INV-CAMERA-3, INV-INPUT-5, INV-OUTPUT-1, INV-OUTPUT-2, INV-SCALE-2, INV-SCALE-5); 57 effective invariants; 20 `F-*` (10 amended incl. new F-RECONF-1); OB-45…51; EE-1…8; W-SM-4c, W-SM-16; gates G-OB48, G-MIRROR.
- **Level A:** E-F2-01 and E-F2-02 corrected with regression evidence and negative controls; E-F2-02's correction depends on dependency patch P1 or an upstream fix (OB-11 criterion 3 open). **New Level A E1R-N1 (F-HIT-1 transform) found and corrected; under the E1R rules it blocks E1R pending review.**
- **Checker:** PASS; self-test 18/18.
- **NEXT:** INDEPENDENT FINAL PHASE E REVIEW OF THE SEALED E1R BASELINE. Phase F remains locked until that verdict passes and is accepted.

## Baselines
- Phase A evidence: `2c8c163784e260af4aaa8c1dc05c6281b0476120`
- Phase B seal: `4a0807ed721ae257e1f4b0e5287d1dfd8c1038c1`
- Phase C checkpoint 1: `d596416cf57877116e9cfbc8cf6c6fe25c001c67`
- Phase C checkpoint 2: `fc2dd68f843b1eb05828f4116f94e8bab4f478da`
- **Phase C checkpoint 3A:** `795800937a1e50e56587cd4080a73ce27d8a8b01` (hash recorded in `fd9a80c`)
- **Phase C seal (checkpoint 3B):** `615b0ed2f3631b698b0c54783c28f696ef4ff34a`

## PHASE D CHECKPOINT 2 — D7–D9 (sealed: `c0a1cee310e56b75d32cf30152a09975ff776700`)
- **Package:** 11 living documents. New: FOUNDATION_INVARIANTS, _OBJECT_MODEL, _STATE_MACHINES, _TEST_PLAN, _RISKS, _IMPLEMENTATION_PLAN. Living regions appended to ARCHITECTURE, EVIDENCE, BLOCKERS, DECISIONS and the LEDGER; historical text untouched.
- **Namespace map** in ARCHITECTURE §P0; 17 namespaces, one home each.
- **Registries:** 57 effective invariants; 19 `F-*`; 11 `PROV-*`; 44 `OB-*` (OB-36 checker CI; OB-37…41 PD-F1-10; OB-42…44 PD-F1-11); 66 risks; 114 evidence entries; 34 objects; 22 machines; 25 tests; 23 ledger entries; 18 increments.
- **Findings in D7:** PD-F1-11 (D-F1, applied); PD-F0-10 and PD-F0-11 (editorial ADR notes). No new D-F2.
- **Checker:** `tools/check-foundation-docs` PASS; `--self-test` 12/12 seeded defects detected.
- **D9:** 19/19 PASS. Open D-F2: PD-F2-06 (B), PD-F2-13 (C), PD-F2-15 (C), PD-F2-16 (C).
- **NEXT:** Phase E (independent adversarial review) only after explicit authorization.

## PHASE D CORRECTIVE CHECKPOINT 2R (sealed: `5827926782089dc8c6a1c203f844fbc879fce9be`)
- **Trigger:** D7–D9 stopped at PD-F2-18 (Level A). PD-F2-19 (Level C) and PD-F1-10 recorded at the same time (FOUNDATION_BLOCKERS.md § Phase D Checkpoint 2).
- **PD-F2-18 resolved:**
  - dedicated `OutputLifecycleToken` (INV-OUTPUT-2, new);
  - INV-CAMERA-1/3 and INV-OUTPUT-1 amended;
  - hardware-only descriptor; passive persisted lookup; no public restore API.
  - Evidence ER2-1: authority spike, 6 new probes and 3 new tests; `verify.sh` PASS.
- **PD-F2-19 resolved:** INV-SCALE-3/4 cover all spatial interactions. Evidence ER2-2 (`d5_model_2r`): window drag 39 → 1, with a negative control.
- **Registry:** 57 entries (§C12.R2, audit/2R/INVARIANT_DIFF_2R.md).
- **Clarifications at the seal (C12.R2.3, D-F0, non-semantic):**
  - `OutputLifecycleToken` lifecycle: exactly one per Core, returned once to the composition root, never re-issued, lifecycle operations only. Backed by the `verify.sh` 2g source check with a seeded negative control.
  - Device-bound (P7) exception: only when `h > H_max(E)`; never a damping bypass.
  - 300 ms remains provisional (OB-23).
- **PD-F1-10:** left open for D7.
- **NEXT:** D7–D9 only after explicit user authorisation.

## PHASE D CORRECTIVE CHECKPOINT 1R (sealed: `84ce620898445aaadb72f386ec3954e5bbfe2c2f`)
- **Baseline:** checkpoint 1 committed as `e8ec03a2ab36f315ae025723ec666b9ad278ac83` ("preserve Phase D falsification baseline"). This hash is recorded here and in audit/README.md, audit/1R/* and FOUNDATION_BLOCKERS.md.
- **Dispositions:** all 17 PD-F2 findings disposed of (FOUNDATION_BLOCKERS.md § Phase D Corrective Checkpoint 1R; audit/1R/CORRECTIONS_1R.md).
  - 13 resolved by architecture correction (A);
  - 1 resolved by another correction (B, PD-F2-11);
  - 3 now obligations: PD-F2-13 → OB-34, PD-F2-15 → OB-35, PD-F2-16 → OB-33;
  - PD-F2-06 jitter residual → OB-24 rescoped.
- **Re-evaluation:** 5 findings raised to Level A (PD-F2-03, -04, -05, -10, -17); all resolved.
- **Registry:** amended under provenance in FOUNDATION_ARCHITECTURE §C12.R1 (sealed C12 table unaltered). 14 invariants amended, 2 new (INV-INPUT-10, INV-PROTO-1); 56 entries.
- **Evidence:**
  - ER-1 xpopup_focus;
  - ER-2 xlock_grab;
  - ER-3 xmaximize (maximize and fullscreen);
  - ER-4 authority spike probes 2d/2e;
  - ER-5 d5_model_1r;
  - ER-6 satellite `-listenfd`;
  - ER-7 regression re-run.
  - Every experiment ran with its negative control, all as expected.
- **New Smithay quirks:** W-SM-14 and W-SM-15. **New ADR:** ADR-14.
- **Provisional constant:** T_settle candidate 300 ms (OB-23).
- **Explicit residual (NOT resolved, NOT validated):** the ±1 physical-pixel shape variation at lattice-reference switches during zoomed pan (ED-1, ED-2 F1). It is not a rendering guarantee. OB-24 keeps it open as a **pre-implementation decision for zoomed navigation** (candidate: gesture-pinned reference; visual acceptance T3/T4).
- **NEXT:** D7–D9 only after explicit user authorisation.

## PHASE D CHECKPOINT 1 (committed as e8ec03a)
- Input baseline verified (audit/README.md §1): seal resolves; `a3ae907` records it; tree was clean; 54-entry C12 registry normative; BLOCKERS pointer present; OB-19 unconditional; T3–T5/UP/TUNE open.
- D1 threat model; D2 22 state machines; D3 authority matrix (40 items, 6 searches); D4 coordinate matrix (16 domains, 26 conversions); D5 four-epoch audit; D6 FOUNDATION_DECISIONS.md (ADR-01…13).
- Findings: 17 D-F2 (A: 2, B: 6, C: 9), recorded in FOUNDATION_BLOCKERS.md § Phase D Architectural Findings; 9 D-F1; 9 D-F0; no D-F3. Registry NOT edited.
- New evidence: ED-1 d5hit (Smithay, 0 mismatches incl. k ∉ n/120), ED-2 d5_model (MODEL), ED-3/4/5 (source).
- NEXT: user disposition of PD-F2-01/02 (Level A) and the other D-F2s; commit of checkpoint 1 only on authorisation; D7–D9 not begun.

## COMPLETED_WORK (3B)
- C11: verification tiers T0–T5 (environment, simulated/real, invariants, tests, cost, trigger, evidence, meaning of failure, tier-honesty rule); OB-1…OB-29 all mapped; negative controls retained.
- C12: canonical invariant registry, 54 `INV-*` with owner, class, evidence, tests, tier and violation consequence; legacy → canonical map; supersessions marked (D-B2-I9 tolerance → INV-HIT-1; D-B8-SCALE wording → INV-SCALE-*).
- C13: implementation-readiness audit; rules promoted into normative text.
- C14:
  - performance methodology (no invented budgets);
  - idle contract (INV-IDLE-1);
  - allocation contract (INV-ALLOC-1);
  - scale-churn contract (INV-SCALE-4);
  - feedback damping SC-E/SC-D with model evidence (EC-22);
  - provisional tuning list.
- OB-19 formalised as the unconditional Phase F entry gate.
- C15 compatibility contract; C16 non-goals; physical obligations (C-PHYS).
- Authority reconstruction (C-AUTH): no dual writers.
- Final adversarial audit (25 scenarios): no contradictions between invariants; five under-specifications closed (AUD-4, -12/-13, -24, -25).
- Phase C gate evaluation: satisfied.

## FILES_CHANGED (committed in the seal)
- `docs/foundation/FOUNDATION_ARCHITECTURE.md` (3B sections)
- `docs/foundation/FOUNDATION_BLOCKERS.md` (normative-pointer note)
- `docs/foundation/CHECKPOINT.md`
- `docs/foundation/evidence/phase-c/cp3b-feedback-model-output.txt`
- `experiments/cp3a-scale-model/feedback.py`

## PROVISIONAL (C14.5)
H_cfg 3.0 · r 1.25 · σ 0.05 · P 1 s · SC-D decrease delay off · renderer filters Linear.

## OPEN, WITH DESTINATIONS
- OB-19 = Phase F entry gate (first production-workspace commit).
- All other OBs in C11.2 (T0–T5 / UP / TUNE / zoom-motion decision).

## LIVE_SYSTEM_TOUCHED
**No.** 3B ran only offline model scripts and documentation work; no clients, compositors or system changes.

## NEXT_ACTION
Phase D (falsification and document reorganisation) only after explicit user authorisation. Approval scope: the architecture is sufficiently specified to advance; it is **not** physically validated or production-ready. T3–T5 and upstream obligations remain authoritative future requirements.
