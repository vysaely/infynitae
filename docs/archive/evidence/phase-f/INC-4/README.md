# INC-4 evidence — Keyboard ownership, ActiveElementFocus, popup routing

- **Category / execution class:** PRODUCT, HEADLESS.
- **Baseline:** `1ee520fa0079d45193f2a7274b31018e9276b0ed` (INC-3 ratified).
- **Evidence files** (captured on the implementation tree):
  - [`IMPLEMENTATION_MAPPING.md`](IMPLEMENTATION_MAPPING.md): written and committed (`f296a73`) before any production change;
  - [`test-matrix.txt`](test-matrix.txt): complete workspace test suite passing under `ulimit -n 1024`;
  - [`verify-architecture.txt`](verify-architecture.txt): all structural source rules, compile probes, runtime invariants passing;
  - [`verify-test-efficacy.txt`](verify-test-efficacy.txt): 68 seeded behavioural defect controls DETECTED;
  - [`verify-development-environment.txt`](verify-development-environment.txt): environment verified clean and passing.

**Status: INC-4 IMPLEMENTED — READY FOR INDEPENDENT ADVERSARIAL REVIEW.**
- No obligation was closed (ratification reserved for independent review).
- No canonical architecture, invariant, formula or audit record was changed.

---

## 1. Canonical INC-4 Scope

Reconstructed from `FOUNDATION_IMPLEMENTATION_PLAN.md`, `FOUNDATION_OBJECT_MODEL.md`, `FOUNDATION_STATE_MACHINES.md`, `FOUNDATION_INVARIANTS.md`, and `FOUNDATION_BLOCKERS.md`:

1. **Keyboard Ownership Rules K1–K9 (`M12`):**
   - **K1:** Press ownership decided at press via `input_intercept`. Compositor-owned press is consumed;
   - **K2:** Release follows the owner of the press. Compositor-owned release is consumed;
   - **K3:** Keys that change XKB modifier or lock state are never compositor-owned (observe mode; forwarded to client);
   - **K4:** Focus change while a client-owned key is held: old focus receives `leave` (resetting state), new focus receives `enter(keys)` containing the held client-owned key followed by `modifiers`, subsequent release reaches the new focus;
   - **K5:** Modifiers held when a compositor binding activates: client sees modifier events only, binding key is consumed;
   - **K6:** Compositor-owned keys stay consumed across interaction cancellation;
   - **K7:** Focused surface unmapped or destroyed with keys held: successor selected, receives `enter(keys)` without `leave` to dead surface;
   - **K8 (OB-15):** Keyboard focus is output-independent; output removal does not alter keyboard focus or held keys;
   - **K9 (OB-15):** Compositor advertises `repeat_info(25, 200)` and synthesizes zero repeat events (client-side repeat).

2. **ActiveElementFocus (`M11`, `OBJ-22`, `INV-INPUT-8`):**
   - Core's `FocusModel` is the sole policy authority for desktop keyboard focus;
   - Transitions occur only by explicit action (click-to-focus, `FocusNext` binding, shell command, activation grant) or target loss (unmap, destroy);
   - Successor selection on target loss prioritizes the most recently focused live element (focus history), falling back to the top live element in stage z-order;
   - Cameras, visibility, and pointer motion never alter keyboard focus (no focus-follows-mouse in Foundation 01).

3. **Popup Grab Routing & Ordered Preemption (`INV-INPUT-10`, `OB-30`, workarounds `W-SM-7`, `W-SM-14`, `W-SM-15`):**
   - When ActiveElementFocus changes while an `xdg_popup` grab is active, the adapter executes atomic ORDERED preemption:
     1. `PopupGrab::ungrab(PopupUngrabStrategy::All)` (topmost popup first receives `popup_done`);
     2. `keyboard.unset_grab(state)`;
     3. `pointer.unset_grab(state, serial, 0)`;
     4. `keyboard.set_focus(state, target_surface, serial)`.
   - `W-SM-7`: Popup grab requests validate that the root surface is mapped and belongs to the focused element; unmapped/dead requests refused without panic;
   - `W-SM-14`: Nested popup grabs (`P1 -> P2`) retain the installed pointer grab (`nested_grab_keep_pointer`);
   - `W-SM-15`: Reverse unset order (pointer before keyboard) prevented.

4. **Surface State & Interaction Dismissal:**
   - `INV-PROTO-1`: `xdg_toplevel::State::Activated` is applied when focus is gained and cleared when focus is lost;
   - `D-B6-BOUNDARY`: Popup anchor-output constraint calculation implements Rule 1 (causing output when observing parent), falling back to clock output (Rule 2);
   - `OB-4`: CameraPan gesture acquisition dismisses active popup grabs (`Effect::DismissPopupGrab`) before camera motion begins.

---

## 2. Production Architecture

- **`infynitae-core`:**
  - `crates/infynitae-core/src/input/keys.rs`: Evdev key constants, `KeyAction`, `CANONICAL_BINDINGS`, `resolve_key_binding`.
  - `crates/infynitae-core/src/input/focus.rs`: `FocusModel` (`OBJ-22`), explicit transitions, `focus_next`, `target_lost` with history-based successor selection.
  - `crates/infynitae-core/src/input/mod.rs`: `InputState` extended with `key_ownership: Ownership` (`OBJ-25`).
  - `crates/infynitae-core/src/entry.rs`: Click-to-focus on client button press; `Effect::DismissPopupGrab` emission on CameraPan acquisition (`OB-4`).
  - `crates/infynitae-core/src/protocol.rs`: `Effect::KeyboardFocus`, `Effect::ToplevelConfigure`, `Effect::DismissPopupGrab`; Rule 1 in `popup_constraint_target`.
- **`infynitae-wayland`:**
  - `crates/infynitae-wayland/src/keyboard.rs`: `WaylandState::keyboard_key` two-phase dispatch, `apply_key_action`, `PreemptionStrategy` configuration.
  - `crates/infynitae-wayland/src/pointer.rs`: Encapsulated `set_popup_pointer_grab`, `unset_pointer_grab`, `set_keyboard_grab`, `unset_keyboard_grab` maintaining strict compliance with `smithay-pointer-only-in-adapter-pointer`.
  - `crates/infynitae-wayland/src/handlers.rs`: `XdgShellHandler::grab` with `W-SM-7` root checks and `W-SM-14` nested grab retention; `toplevel_destroyed` focus successor dispatch.
  - `crates/infynitae-wayland/src/outputs.rs`: `WaylandState::apply` for `Effect::KeyboardFocus` (ordered preemption) and `Effect::ToplevelConfigure` (sets/unsets `Activated`).
- **`infynitae-backend-headless`:**
  - `crates/infynitae-backend-headless/src/lib.rs`: `keyboard_key` intake validating port authority via `state.core().check_port(port)`.

---

## 3. Verification Suites

### A. Test Execution (`test-matrix.txt`)
- Executed under `ulimit -n 1024` with zero warnings.
- Workspace unit and integration tests: **100% passing** (123+ tests across 7 crates and integration targets).
- `t1_keyboard.rs` (13/13 passing): K1–K9 scenarios, `neg_k3_without_k3_stuck_modifier_control` negative control, output removal (OB-15), key repeat probe (OB-15).
- `t1_popup_focus.rs` (13/13 passing): PF-1 through PF-7, PF-OB4 pan dismissal, PF-WSM7 unmapped grab rejection, NEG-1 naive preemption control, NEG-2 pointer-first teardown control, OBS-1 anvil order observation.

### B. Architecture Verification (`verify-architecture.txt`)
- All dependency pins, DAG edges, and forbidden unsafe checks pass.
- Source rules: `smithay-pointer-only-in-adapter-pointer`, `effects-only-from-core`, `wayland-names-no-authority`, `mint-location`, `authority-construction`, `stage-single-writer` 100% passing.
- All 35+ negative compile probes and 50+ seeded architectural defects detected.

### C. Test Efficacy Verification (`verify-test-efficacy.txt`)
- 68 seeded mutation controls verified; all 68 DETECTED by the product tests.
- Includes 5 new INC-4 efficacy controls:
  1. `INC-4 K1: compositor press forwarded to client instead of consumed` -> caught by `k3_compositor_binding_super_b_consumed`;
  2. `INC-4 K3: modifier key consumed instead of observe mode` -> caught by `k8_binding_on_modifier_key_with_k3_observe_mode`;
  3. `INC-4 OB-4: interaction acquisition does not dismiss popup grabs` -> caught by `pf_ob4_interaction_acquisition_dismisses_popup_grab`;
  4. `INC-4 INV-INPUT-10: popup grab not ungrabbed on focus change` -> caught by `pf1_grab_active_core_focuses_w2_ordered`;
  5. `INC-4 D-B6-BOUNDARY: causing output rule 1 not applied` -> caught by `popup_target_causing_output_rule1_and_rule2_fallback`.

### D. Development Environment Verifier (`verify-development-environment.txt`)
- Toolchain, libraries, document integrity, compile probes, architecture, and test efficacy pass.
