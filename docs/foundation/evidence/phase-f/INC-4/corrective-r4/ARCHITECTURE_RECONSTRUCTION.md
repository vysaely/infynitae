# INC-4 Round 4 — Architecture Reconstruction (popup lifetime, input, scene, lifecycle)

Written before the Round-4 production change was finalized; this file is the implementation
target. Tests do not read it: every test expectation is a literal derived from its own sequence.

- **Pre-correction HEAD:** `60e5429821793ac2745ec6a9fcc171217061f224` (= origin/main at start)
- **Fourth-review ingestion commit:** `dc4d52e1f8b359b3616817174286b7e5791d4d9e` (dossier `audit/phase-f-inc4-rereview-3/`, 19 files, SHA-256 identical before and after commit)

## 1. Four consequences, kept separate

A popup's death (client `xdg_popup.destroy`) has three consequences. None of them is the fourth.

| # | Consequence | Authority (single owner) | Mechanism after Round 4 |
|---|---|---|---|
| 1 | **Lifetime**: the popup is no longer live | the client (protocol); Smithay's grab chain (`PopupGrabInner`) is its record | Smithay calls `popup_destroyed` from the `xdg_popup` resource's `destroyed` callback after `destroy_notify` (`wayland/shell/xdg/handlers/surface/popup.rs:67-81`), so `PopupSurface::alive()` is already false; `PopupManager::cleanup()` moves it out of the active chain |
| 2 | **Input**: the popup grab chain must agree with Core's ActiveElementFocus (INV-INPUT-8/10) | ActiveElementFocus: Core `FocusModel`; ProtocolSurfaceRoute (keyboard and pointer grabs): adapter, through the install serials it recorded | `WaylandState::reconcile_popup_chain` (A1) |
| 3 | **Scene**: the dead surface stops participating in hit-testing and rendering (R2-01) | Core `Stage` snapshot (OBJ-05), fed by adapter extraction of committed state | `element_tree_changed` → `Core::toplevel_tree_changed` (B1) |
| 4 | **Parent commit**: the parent's M1 lifecycle (map, unmap, initial configure) | the parent client's own `wl_surface.commit` of the ROOT surface | `element_root_committed` → `Core::toplevel_committed`, called **only** from `commit` of the root surface |

Round 3 implemented 1+3 through the primitive for 4 (B1). A1 existed because 1 was not connected to 2 for client-driven close.

## 2. A1 root-cause model (pinned Smithay 0.7.0)

- `PopupGrab::has_ended()` is `!root.alive() || !toplevel_grab.active()`; `current_grab()` is the topmost live grabbing popup, else the root (`desktop/wayland/popup/grab.rs:316-336`).
- `PopupKeyboardGrab::input` refocuses **lazily**: it calls `handle.set_focus(current_grab())` inside key delivery (`grab.rs` impl `KeyboardGrab::input`). By then `KeyboardHandle::input_forward` has already inserted the key into `forwarded_pressed_keys` (`input/keyboard/mod.rs:1013-1043`), so the late `enter` lists the key and the `pressed` event follows. That is a `wl_keyboard` must-not.
- **Depth 1 worked by accident.** Round 3's `popup_destroyed` released the pointer grab. Smithay's `PopupPointerGrab::unset` → `PopupGrab::unset_keyboard_grab` released the keyboard grab and refocused the root, but only if `keyboard.has_grab(pointer-grab serial)` (`grab.rs:371-381, 697-700`).
- **Depth ≥2 diverged.** W-SM-14 keeps the FIRST level's pointer grab (serial s1), while each level installs a new keyboard grab (s2, s3). The serial test fails, so the keyboard grab survives on a dead chain and Smithay focus stays on a dead popup while Core holds the root. The next key goes through the lazy path, producing the violation.
- **Why CameraPan teardown did not cover it.** `Effect::DismissPopupGrab` and ordered preemption are *Core-initiated*; client-initiated death had no path to the input consequence other than the accidental one above.
- **Why tests missed it.** No test closed a depth ≥2 chain from the client and then typed. CameraPan, focus change and reconnect tests all dismiss from the compositor side.
- **Provenance.** Reproduced on `d5b7b95` (the original INC-4 implementation; `zz_a1` probe, 1 violation) and on `6819c8a`. It cannot exist on the ratified INC-3 baseline `1ee520fa`, where the seat has no keyboard and `XdgShellHandler::grab` is a no-op. **A1 was introduced by INC-4 itself; it is not an inherited foundation defect.**

### Terminal condition and its action (exactly once)

`reconcile_popup_chain`, called after `cleanup()` on every popup death:

- **A grabbing popup is still live** (`!has_ended()`): move the keyboard to `current_grab()` now, if it is not already there (xdg-shell: "the grab will be returned to the parent of the popup"). The keyboard and pointer grabs stay.
- **None is live** (`has_ended()`, the terminal condition): the adapter forgets the chain (`popup_grab = None`), so the branch cannot run twice. Then:
  1. release the popup **keyboard** grab by its recorded install serial;
  2. release the popup **pointer** grab by its recorded install serial;
  3. route the keyboard to the root of Core's ActiveElementFocus, or to nothing if Core is unfocused (INV-INPUT-10 steps 2–4; step 1 is moot because the popups are dead).

  It never invents a target and never targets a dead or unmapped surface: Core's focus is always a Mapped element (F07).

## 3. `element_committed` caller audit (B1, prompt §11)

| Caller (before Round 4) | Classification | After Round 4 |
|---|---|---|
| `commit` of the element's ROOT surface | genuine client commit | `element_root_committed` (M1) |
| `commit` of a popup / desynchronised subsurface of the element | **suspicious** — a child's commit ran the PARENT's M1 (an uncommitted parent got its initial configure from its popup's commit; fourth review O-1) | `element_tree_changed` (snapshot only) |
| `popup_destroyed` (root resolved) | **wrong** — compositor-internal; B1 | `element_tree_changed` |
| `popup_destroyed` (root unresolved: every element) | **wrong** — could "commit" unrelated elements | `element_tree_changed` for every element (cannot advance any lifecycle) |

`Core::toplevel_tree_changed` replaces the snapshot of a **Mapped** element whose extracted root is mapped, and returns the visibility-pass effects. In every other state it is a no-op. It can never map, unmap, place or configure. Extraction reads only **committed** cached state, so pending client state is never promoted.

## 4. Popup destruction state table (implementation target)

L = live grabbing popups after the transition; C = Core ActiveElementFocus; K = Smithay keyboard focus; KG/PG = popup keyboard/pointer grab; HT = hit-test membership of the dead popup; M1 = parent lifecycle transition.

| Transition | L | C | K | KG | PG | HT | M1 | Client keyboard wire | Pointer wire |
|---|---|---|---|---|---|---|---|---|---|
| single popup destroyed by client | ∅ | root | root | released | released (install serial) | removed | none | `enter root` once (no `leave` to the dead popup, EC-7) | one restore `motion` (W-SM-16, allowed outside a pan by the INC4R-10 decision) |
| inner of nested destroyed (outer lives) | {outer} | root | outer | kept | kept | removed | none | `enter outer` once, before any key | none |
| last of a nested chain destroyed | ∅ | root | root | released | released | removed | none | `enter root` once | one restore `motion` |
| middle destroyed (protocol violation Smithay 0.7.0 does not report) | {outer, topmost} | root | topmost | kept | kept | removed | none | none if already on topmost | none |
| destroyed during CameraPan | ∅ (dismissed at acquisition) | root | root (set at dismissal) | released at dismissal | replaced by `PanDismissedPopupGrab` | removed | none | none during the pan | none during the pan; first post-pan event clears the pan grab without replay |
| destroyed with zero outputs | ∅ | root | root | released | the zero-output grab is NOT the popup grab: untouched | removed | none | `enter root` once | none (zero-output grab active) |
| root unmaps | ∅ (dismissed on unmap) | successor (MRU) | successor | released | released | parent tree dropped | Mapped→Unmapped by the root's own commit only | `enter successor` | — |
| root destroyed | ∅ | successor (MRU) | successor | released | released | element purged | destroy (M1, client request) | `enter successor` | — |

## 5. C3 analysis (authority scope)

- **Who can obtain an `InputAuthToken`:** only a holder of the `InputPort` (the backend's input source), via `Core::verify_input_port`. It is `!Send`, consumed on use, and not `Clone`. The production holder is a single function, `infynitae_backend_headless::keyboard_key`, which mints and consumes it within one event.
- **Event scope (Round 4):** the token now carries the event it authorizes (`key`, `pressed`), and `sync_seat_key(token)` takes the key from the token. A proof for key X cannot update the seat for key Y. The type gets simpler (two fewer parameters), not more complex.
- **Transition gate (Round 4):** moved from the headless backend into `WaylandState::sync_seat_key`, next to the XKB mutation it guards. Every backend now gets one implementation; a future backend cannot forget it.
- **Residual (owner-level, not changed):** a port holder can still mint a valid proof for a real key and never submit the event to Core. That requires the trusted input source itself to be wrong; the port is the root of trust (INV-CAMERA-1/C1.5 model). Making Core drive the XKB update would require Smithay XKB state inside Core, which C4.4 forbids (Core keeps no XKB copy).
