# INC-4 implementation mapping (planning record, not an amendment)

Written before any INC-4 production change. Baseline `1ee520fa0079d45193f2a7274b31018e9276b0ed` (INC-0/1/2/3 RATIFIED).

Sources:
- `FOUNDATION_IMPLEMENTATION_PLAN.md` §INC-4;
- `FOUNDATION_STATE_MACHINES.md` M3 (Popup lifecycle), M11 (ActiveElementFocus), M12 (Physical-key ownership), M22 (Compositor interaction);
- `FOUNDATION_INVARIANTS.md` INV-INPUT-6, INV-INPUT-7, INV-INPUT-8, INV-INPUT-10, INV-PROTO-1 (`activated`);
- `FOUNDATION_OBJECT_MODEL.md` OBJ-22 (ActiveElementFocus), OBJ-23 (ProtocolSurfaceRoute), OBJ-25 (Ownership table: buttons, keys), OBJ-27 (XKB state);
- `FOUNDATION_ARCHITECTURE.md` §C4.4 (Keyboard and modifiers, K1–K9), §C4.5 (Focus model), §C4.6 (Popup-grab dismissal on acquisition);
- `FOUNDATION_BLOCKERS.md` W-SM-7 (grab_popup precondition), W-SM-14 (nested grab pointer grab retention), W-SM-15 (unset order), D-B6-BOUNDARY (popup anchor-output rules);
- `FOUNDATION_TEST_PLAN.md` OB-30, OB-15, OB-4, OB-18 (popup part), TST-07 (x10), TST-20 (xpopup_focus);
- Harness reference implementations: `experiments/b8-harness/src/bin/x10.rs`, `experiments/b8-harness/src/keys.rs`, `experiments/b8-harness/src/bin/xpopup_focus.rs`, `experiments/b8-harness/src/server.rs`.

**INC-4 (canonical):** Keyboard ownership and physical-key intake under K-rules (K1–K9, M12); authoritative desktop focus under `ActiveElementFocus` (M11, OBJ-22); popup grab protocol routing under `ProtocolSurfaceRoute` (OBJ-23, M3) with ordered preemption (INV-INPUT-10) and Smithay grab workarounds (W-SM-7, W-SM-14, W-SM-15); xdg_toplevel `activated` state mapping (INV-PROTO-1); popup anchor-output rule 1 (D-B6-BOUNDARY); interaction popup-grab dismissal (OB-4).
- **Execution class:** HEADLESS.

---

## 1. Architectural Foundations & Ownership Model

### 1.1 Keyboard Ownership & XKB Chain (M12, OBJ-25, OBJ-27, C4.4)
- **Authoritative XKB state:** Smithay's `KeyboardHandle` on `seat0` holds the single authoritative XKB state per seat. Core keeps **no** copy (OBJ-27).
- **Intake and Binding Filter:** Core receives the seat's modifier snapshot (`ModifiersState`) and keysym in the filter callback of Smithay's two-phase `input_intercept`.
- **K1 (Decided at Press):** Ownership of each key is decided at physical *press*. `input_intercept` updates XKB and runs the binding filter. If a compositor binding matches and does NOT trigger K3 observe mode, the press is consumed (`Compositor` owned). Otherwise the key is forwarded via `input_forward` (`Client` owned, recorded by Smithay in `forwarded_pressed_keys`).
- **K2 (Release Follows Press Owner):** A physical *release* strictly follows its press owner. If `Compositor` owned, the release is consumed. If `Client` owned, it is forwarded via `input_forward` to whichever surface currently holds keyboard focus.
- **K3 (Observe Mode for Modifier Keys):** Keys whose press changes XKB modifier or lock state are **never** compositor-owned. If `input_intercept` reports `mods_changed`, the event is forwarded even when a binding matches; the binding acts in *observe mode* (the action fires, but the key is forwarded). This prevents stuck modifiers when focus transitions occur (`INV-INPUT-6`).
- **K4 (Focus Change Under Held Keys):** When keyboard focus changes while keys are held, Smithay sends `leave` to the old surface (resetting its logical key state to default) and `enter(forwarded_pressed_keys)` + `modifiers` to the new surface (`INV-INPUT-7`).
- **K5 (Binding While Modifiers Held):** When a binding activates while modifiers are already held, those modifiers were forwarded (K3) and are known to clients. The non-modifier binding key is consumed with its release (K2).
- **K6 (Interaction Cancellation):** Compositor-owned keys stay compositor-owned until physically released; releases are consumed. No synthetic releases are emitted.
- **K7 (Target Loss Under Held Keys):** When the focused surface is unmapped or destroyed, the FocusModel selects a successor. Smithay sends `enter(forwarded_pressed_keys)` + `modifiers` to the successor without sending `leave` to the dead surface.
- **K8 (Output Removal Invariant):** Output removal changes no keyboard state (`OB-15`). Keyboard focus is output-independent (`D-B10-KBD`).
- **K9 (Client-Side Repeat):** Key repeat in Foundation 01 is entirely client-side (`repeat_info`). Compositor-side repeat is unused (`OB-15`).

### 1.2 ActiveElementFocus (M11, OBJ-22, INV-INPUT-8)
- **Sole Policy Authority:** Core `FocusModel` is the sole writer and policy authority for desktop keyboard focus (`ActiveElementFocus`).
- **Explicit Transitions Only:** Changes *only* via explicit action:
  1. Click-to-focus (routed via intake hit testing on client-owned clicks);
  2. Focus-navigation bindings (e.g., `Super+Enter` -> `FocusNext`);
  3. Shell focus command (`ShellPort`, INC-11);
  4. Activation grant (`xdg-activation`, D-B10-ACTIVATION);
  5. Target loss (element unmapped or destroyed -> successor policy from live focus history).
- **Never Changed By:** Camera position, camera motion, element visibility, element membership, or pointer motion (no focus-follows-mouse in Foundation 01).
- **Protocol Mapping (`INV-PROTO-1`):** `xdg_toplevel` `activated` state holds *exactly* when the element holds `ActiveElementFocus`. When focus changes, Core emits configure effects to update `activated` on the element losing focus (unset) and gaining focus (set).

### 1.3 Popup Routing & ProtocolSurfaceRoute (M3, OBJ-23, INV-INPUT-10)
- **Subordinate Authority:** `ProtocolSurfaceRoute` is adapter state managing Smithay popup grabs and their root surface restore. It is strictly subordinate to `ActiveElementFocus`.
- **Precondition & Validation:**
  - `W-SM-7`: Popup grab root must exist, be an active element root, and match the currently focused element (`root == active_element_root`). Otherwise, the grab is refused without panic.
  - Serial validation: keyboard must not be grabbed by an unrelated sequence.
- **W-SM-14 (Nested Grab Pointer Grab Retention):** When a nested popup grab is established (`grab.previous_serial().is_some()`), the adapter preserves the installed `PopupPointerGrab` rather than replacing it, preventing premature keyboard grab teardown.
- **W-SM-15 (Unset Ordering):** Pointer grab must be unset *after* keyboard grab is unset during preemption to avoid transient root focus flickering.

### 1.4 Ordered Preemption Sequence (INV-INPUT-10)
When `ActiveElementFocus` changes (to another element or `None`), or when a compositor interaction acquires (`OB-4`), any active popup grab chain is preempted in one atomic batch in this exact order:
1. `PopupGrab::ungrab(PopupUngrabStrategy::All)` — sends `popup_done` topmost first to all grabbing popups;
2. Keyboard `unset_grab` (`kb.unset_grab(srv)`);
3. Pointer `unset_grab` (`ptr.unset_grab(srv, serial, 0)`);
4. Set keyboard focus to target (`kb.set_focus(srv, target_surface, serial)`).

### 1.5 Anchor Output Selection Rule 1 (D-B6-BOUNDARY, INC3-F08)
When calculating `Core::popup_constraint_target(parent, causing_output)`:
- **Rule 1:** Output on which the causing input event occurred, IF parent is visible on it.
- **Rule 2:** Otherwise, parent's frame-clock output (highest refresh observing output).
- **Rule 3:** Otherwise, `None` (no constraint adjustment).

---

## 2. State Ownership Table (No Duplicate State)

| State | OBJ | Owner / Sole Writer | Readers | Lifetime | Invalidation / Reset |
|---|---|---|---|---|---|
| ActiveElementFocus & Focus History | OBJ-22 | Core `FocusModel` (written on explicit action or target loss) | Core, adapter (via effects), shell | Seat | Successor transition on target destruction |
| ProtocolSurfaceRoute (popup grab chain) | OBJ-23 | Adapter `WaylandState` (`smithay::desktop::PopupGrab`) | Adapter, Smithay seat | Grab duration | Ordered preemption, client unmap, compositor interaction |
| Key Ownership Table (`key -> Owner`) | OBJ-25 | Core `Ownership` (`core::input::ownership`) | Core intake, adapter dispatch | Press to release | Physical release of key |
| XKB State & Modifiers | OBJ-27 | Smithay `KeyboardHandle` (authoritative) | Core (read-only filter snapshot), clients | Seat | Keymap reload |
| Pointer Grab State | OBJ-20 / OBJ-27 | Smithay `PointerHandle` | Adapter | Grab duration | Ordered preemption, release, zero-output grab |
| Popup Constraint Target | — | Core `Core::popup_constraint_target` (pure function) | Adapter popup positioning | Transient | Recomputed on popup configure |

---

## 3. Requirement Mapping

| Requirement | Invariant / Object / State Machine | Authoritative Owner | Production Location | Verification Layer | Negative / Adversarial Control | Obligation |
|---|---|---|---|---|---|---|
| K1: Press ownership decided at press; compositor press consumed | INV-INPUT-1, M12, OBJ-25 | Core intake / adapter | `core::input::keys`, `wayland::keyboard` | T0 / T1 `t1_keyboard` | Non-binding press forwarding | TST-07, OB-15 |
| K2: Release follows press owner; compositor release consumed | INV-INPUT-1, M12, OBJ-25 | Core intake / adapter | `core::input::keys`, `wayland::keyboard` | T0 / T1 `t1_keyboard` | Stuck key check (client state == held keys) | TST-07, OB-15 |
| K3: Modifier keys never compositor-owned (observe mode) | INV-INPUT-6, M12, OBJ-27 | Core intake / adapter | `core::input::keys`, `wayland::keyboard` | T0 / T1 `t1_keyboard` | **NEG control: K3 disabled** (stuck modifier reproducer) | TST-07, OB-15 |
| K4: Focus change under held keys (`leave` + `enter(forwarded)`) | INV-INPUT-7, M12, OBJ-27 | Adapter (Smithay seat) | `wayland::keyboard` | T1 `t1_keyboard` | Duplicate enter / missing leave detection | TST-07, OB-15 |
| K5: Modifiers held when binding activates | INV-INPUT-6, M12 | Core / adapter | `wayland::keyboard` | T1 `t1_keyboard` | Client sees modifier down/up only | TST-07 |
| K6: Compositor-owned keys stay consumed across interaction cancel | INV-INPUT-1, M12, M22 | Core intake | `core::input::keys` | T1 `t1_keyboard` | Synthetic release probe | TST-07 |
| K7: Focused surface unmapped/destroyed with held keys -> successor | INV-INPUT-7, M11, M12 | Core `FocusModel` + adapter | `core::focus`, `wayland::keyboard` | T1 `t1_keyboard`, `t1_popup_focus` | Dead surface `leave` avoidance | TST-07, TST-20 (PF-3) |
| K8: Output removal has no keyboard effect | D-B10-KBD, M12, OBJ-22 | Core `FocusModel` | `core::entry` | T1 `t1_keyboard` | Disconnect output under held keys | OB-15 |
| K9: Compositor-side repeat unused; client repeat_info announced | M12, OBJ-27 | Adapter seat setup | `wayland::lib` (`add_keyboard`) | T1 `t1_keyboard` | Repeat event count probe == 0 | OB-15 |
| ActiveElementFocus sole authority & explicit transitions | INV-INPUT-8, M11, OBJ-22 | Core `FocusModel` | `core::focus` | T0 / T1 `t1_keyboard`, `t1_popup_focus` | Panning / pointer motion does NOT change focus | OB-30 |
| `activated` state on xdg_toplevel matches ActiveElementFocus | INV-PROTO-1, M11 | Core `protocol` + adapter | `core::protocol`, `wayland::outputs` | T1 `t1_popup_focus` | Divergence between focus and `activated` | OB-30 |
| Popup grab precondition & root check (W-SM-7) | W-SM-7, M3, OBJ-23 | Adapter `xdg_shell::grab` | `wayland::handlers` | T1 `t1_popup_focus` | Grab with dead root / non-element root (no panic) | OB-18 |
| Nested popup grab keeps pointer grab (W-SM-14) | W-SM-14, M3, OBJ-23 | Adapter `xdg_shell::grab` | `wayland::handlers` | T1 `t1_popup_focus` (PF-6) | **OBS-1 negative control**: replace pointer grab -> failure | OB-30 |
| Ordered preemption on focus change (INV-INPUT-10) | INV-INPUT-10, M3, M11 | Adapter focus effect apply | `wayland::keyboard` / `outputs` | T1 `t1_popup_focus` (PF-1, PF-2) | **NEG-1 (naive `set_focus`)** and **NEG-2 (pointer-first)** | OB-30 |
| Popup dismissal on compositor interaction acquisition | INV-INPUT-2, M3, M22 | Core entry / adapter | `core::entry`, `wayland::pointer` | T1 `t1_popup_focus` | Pan acquisition dismisses popup before camera changes | OB-4 |
| Anchor output selection rule 1 (D-B6-BOUNDARY) | D-B6-BOUNDARY, M3 | Core `protocol` | `core::protocol::popup_constraint_target` | T0 / T1 | Input on output A while parent visible on A & B -> A chosen | INC3-F08 |
| Headless keyboard injection helper | C1.1 | Headless backend | `infynitae-backend-headless` | T1 harnesses | Foreign port rejected | C1.1 |

---

## 4. Authority Boundaries & Invariant Protections

1. **Zero XKB Copy in Core (`C4.4`, `OBJ-27`):**
   - Core maintains no XKB context, keymap, or modifier bitmasks.
   - Core receives only a read-only `Modifiers` snapshot during filter callbacks or pointer button events.
2. **Focus Authority (`INV-INPUT-8`, `OBJ-22`):**
   - `FocusModel` inside Core is the only entity that decides which element holds keyboard focus.
   - Adapter never initiates a focus change on its own; it receives `Effect::KeyboardFocus(Option<ElementId>)` and `Effect::ToplevelConfigure(ElementId)` from Core and applies them.
3. **Effects Only From Core (`effects-only-from-core` source rule):**
   - Only Core can construct `Effect` enum variants.
   - Adapter and backends only consume and apply effects.
4. **Pointer Delivery Boundaries (`smithay-pointer-only-in-adapter-pointer` source rule):**
   - Smithay `PointerHandle` methods (`motion`, `relative_motion`, `button`, `axis`, `frame`, `set_grab`, `unset_grab`, `get_pointer`) remain strictly encapsulated within `infynitae-wayland/src/pointer.rs`.
   - Pointer grab operations required for popup preemption are dispatched through helper functions in `pointer.rs`.
5. **Port / Token Authority (`INV-OUTPUT-2`, `C1.5`):**
   - `infynitae-wayland` never mentions `InputPort`, `ShellPort`, or `OutputLifecycleToken`.
   - Injected keyboard events in headless backend validate `InputPort` against Core before invoking adapter dispatch.

---

## 5. Scope Boundaries

### IN SCOPE (INC-4 Canonical)
- Core `input::keys` and key ownership tracking (OBJ-25).
- Core `FocusModel` (OBJ-22, M11) with click-to-focus and focus-navigation bindings (e.g., `Super+Enter` -> `FocusNext`).
- Core focus successor policy on target element destruction / unmap.
- Core `Effect::KeyboardFocus`, `Effect::ToplevelConfigure`, and `Effect::DismissPopupGrab`.
- Core `popup_constraint_target` anchor-output rule 1 (D-B6-BOUNDARY).
- Adapter seat keyboard setup: `seat0.add_keyboard(Default::default(), 200, 25)` (`wl_keyboard` v6).
- Adapter two-phase keyboard dispatch: `input_intercept` and `input_forward` implementing K1–K9.
- Adapter `xdg_shell::grab` implementation with `W-SM-7` precondition check, serial check, and `W-SM-14` nested grab preservation.
- Adapter ordered preemption sequence (INV-INPUT-10, W-SM-15): `ungrab(All)` -> `kb.unset_grab` -> `ptr.unset_grab` -> `kb.set_focus`.
- Adapter `xdg_toplevel` `activated` state configuration on focus transitions (INV-PROTO-1).
- Headless backend `keyboard_key` injection helper over `InputPort`.
- Full T0 and T1 verification suite: `t1_keyboard.rs` (TST-07 / x10) and `t1_popup_focus.rs` (TST-20 / xpopup_focus).
- Efficacy and negative mutation controls (K3 disabled, NEG-1 naive, NEG-2 pointer-first, OBS-1 nested replace).

### OUT OF SCOPE (Deferred to Later Increments)
- **INC-5:** Pointer constraints (`zwp_pointer_constraints_v1`), lock/confinement policy (M13, M14), confinement clamp (F-CONFINE-1), constraint reconciliation on output removal (F-RELOCATE-1), W-SM-4 workarounds, OB-11 decision point.
- **INC-6:** Scale policy, fractional scale (`wp_fractional_scale_v1`), preferred buffer scale, `ScaleInteractionEpoch` (M15, OBJ-16, F-SCALE-1/2/3).
- **INC-7:** Full `xdg_toplevel` requests (`set_maximized`, `set_fullscreen`, `set_minimized`, W-SM-13, OB-32).
- **INC-8:** Off-screen frame callback fallback timers and kick policy (M16, OBJ-17).
- **INC-11:** Shell IPC protocol and semantic commands (`ShellPort`, M18).
- **INC-14:** Interactive window move / resize (M22, F-DRAG-1).
- **INC-16:** Physical input stack (libinput, DRM/KMS, T4 physical backend).
- **Future:** Zoom / wheel zoom, custom keybinding configuration, xwayland-satellite (INC-12).

---

## 6. Verification Strategy & Negative Controls

### 6.1 Exit Test Matrix
1. **`t1_keyboard.rs` (TST-07, OB-15):**
   - Scenario 1 (K1): Compositor binding (`Super+B` Noop): press and release consumed, client sees no event.
   - Scenario 2 (K5): Binding while modifier held (`Super` then `B`): client sees modifier down/up, binding key consumed.
   - Scenario 3 (K3): Binding on modifier key (`LeftShift`): forced forward in observe mode, client receives shift down/up, no stuck modifier.
   - Scenario 4 (K4): Focus change while client key held: old gets `leave`, new gets `enter(keys)` + `modifiers`, release reaches new focus.
   - Scenario 5 (K4/K1): Focus change while compositor key held: key absent from `enter`, release consumed.
   - Scenario 6 (K7): Focused window destroyed with client key held: successor gets `enter(keys)` without `leave` to dead window.
   - Scenario 7: Focus-next binding (`Super+Enter`) while held: focus transitions cleanly, enter/leave correct.
   - Scenario 8 (OB-15): Output removal under held keys: no keyboard state altered, no spurious events.
   - Scenario 9 (OB-15): Key repeat probe: compositor sends 0 repeat events; client reads `repeat_info(25, 200)`.
2. **`t1_popup_focus.rs` (TST-20, OB-30, OB-4, OB-18):**
   - `PF-1`: Grab active -> Core focuses W2 -> ORDERED preemption cleanly dismisses popup and focuses W2.
   - `PF-2`: Grab active -> Core focuses None -> ORDERED preemption dismisses popup and clears focus.
   - `PF-3`: Popup parent W1 destroyed by client during grab -> successor W2 focused under ORDERED preemption.
   - `PF-4`: Popup destroyed by client in flight while Core changes focus -> ORDERED preemption dismisses cleanly.
   - `PF-5`: Popup destroyed and dispatched first, then Core focuses W2 -> clean transition.
   - `PF-6`: Nested popup grab (P1 -> P2) keeps pointer grab (W-SM-14) -> key routed to P2, then Core focuses W2.
   - `PF-7`: Client-owned key held during popup grab -> Core focuses W2 -> release reaches W2.
   - `PF-OB4`: Popup grab active -> Compositor CameraPan acquired -> popup dismissed (`popup_done`) before camera changes.
   - `PF-WSM7`: Misbehaving client requests grab on dead surface / unmapped parent -> refused without panic.

### 6.2 Negative Controls (Test Efficacy)
- **`NEG-K3-DISABLED`:** Disable K3 enforcement in intake filter -> modifier key is consumed -> client stuck modifier detected -> test FAILS.
- **`NEG-1-NAIVE`:** Naive `set_focus` without ungrabbing popup -> Smithay/Core focus divergence -> test FAILS.
- **`NEG-2-POINTER-FIRST`:** Pointer unset before keyboard unset -> transient root focus flickering -> test FAILS.
- **`OBS-1-NESTED-REPLACE`:** Nested grab replacing pointer grab -> premature keyboard grab teardown -> test FAILS.

### 6.3 Resource-Lifetime Regression Control
- Run all tests under strict `ulimit -n 1024`.
- Assert descriptor count before and after each test suite run remains within tight baseline threshold (< 50 descriptors), verifying zero descriptor leak.

---

## 7. Judgment Calls & Historical Adjudications

1. **INC3-F08 Adjudication (D-B6-BOUNDARY Rule 1):**
   - The INC-3 review noted that popup anchor output selection from the causing input event is owned by INC-4 alongside popup grab tracking and OB-4 dismissal.
   - `Core::popup_constraint_target` is updated to take an optional causing `OutputId` parameter to satisfy rule 1 when available.
2. **Binding Keycodes:**
   - Linux evdev scancodes: `KEY_ESC = 1`, `KEY_ENTER = 28`, `KEY_B = 48`, `KEY_LEFTSHIFT = 42`, `KEY_LEFTMETA = 125`.
   - Mappings: `Super+Enter` -> `FocusNext`, `Esc` -> `Cancel`, `Super+B` -> `Noop`, `LeftShift` -> `Noop` (observe mode control).
3. **Repeat Info:**
   - Announced rate: 25 characters/sec, 200 ms delay (`add_keyboard(Default::default(), 200, 25)`).
