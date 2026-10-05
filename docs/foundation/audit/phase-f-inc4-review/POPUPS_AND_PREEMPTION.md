# INC-4 Review — Popup Grabs, Ordered Preemption, OB-4, D-B6-BOUNDARY Rule 1, Zero Outputs

## 1. Pinned dependency facts (smithay 0.7.0, wayland-protocols 0.32.13)

| Behaviour | Source | Consequence |
|---|---|---|
| `grab_popup` asserts `root == find_popup_root_surface(popup)`; rejects a committed popup with `invalid_grab`; **no serial provenance check** | `desktop/wayland/popup/manager.rs:58-97` | W-SM-7 holds by construction (adapter derives `root` with the same function); stale/foreign serials are not detectable through Smithay |
| `PopupPointerGrab::unset` → `PopupGrab::unset_keyboard_grab` → `keyboard.unset_grab` + `set_focus(root)` | `popup/grab.rs:371-381, 697-700` | **any** replacement of the popup pointer grab (W-SM-14, W-SM-15, INC-3 zero-output grab) tears the keyboard route down |
| `PointerHandle::unset_grab` → `unset_grab(.., restore_focus=true)` → `motion(pending_focus, location)` | `input/pointer/mod.rs:178-184, 763-783, 785-816` | every ordered preemption and every OB-4 dismissal **replays a `wl_pointer.motion`** (or enter/leave) |
| `KeyboardHandle::input_forward` forwards releases unconditionally | `input/keyboard/mod.rs:1013-1043`, inner `input` 1280-1299 | ownerless releases reach clients (protocol must-not) unless the caller filters |
| `KeyboardHandle::set_focus` routes through the active grab; `enter(forwarded_pressed_keys)` + `modifiers` | `keyboard/mod.rs:1052-1060, 1321-1365` | K4/K7 rely on it — confirmed |
| xdg-shell `xdg_popup.grab`: "If the compositor denies the grab, the popup will be immediately dismissed"; "the top most grabbing popup will always have keyboard focus" | `xdg-shell.xml:1290-1328` | refusal must send `popup_done`; output removal must not move keyboard focus off the grabbing popup |

## 2. Ordered preemption (INV-INPUT-10) — verdict **behaviour PASS on the happy path; verification FAIL**

Source order in `outputs.rs` (`Effect::KeyboardFocus`) matches `ungrab(All) → kb.unset_grab → ptr.unset_grab → set_focus`. Observable proof (ATK-17): with ORDERED the client sees `popup_done P, leave P, enter W1`; with pointer-first it sees `popup_done P, leave P, enter W2 (root, transient), leave W2, enter W1` — W-SM-15 reproduced on the production adapter. **But the shipped NEG-2 test asserts only `popup_dones == ["P"]`, which both orders satisfy**, and F-4 ("single transition") is declared in the file header but never computed. Mutations I (swap), J (omit kb unset), K (omit ptr unset) all **survive** the shipped suite; I is killed by ATK-17.

Reentrancy: the preemption keys on the adapter's own `popup_grab: Option<PopupGrab>`. That copy is never cleared when the grab ends by other means (client destroys popup, Smithay ends it on click-outside, zero-output grab replaces it), so later focus changes run `kb.unset_grab`/`ptr.unset_grab` against **unrelated** grabs:
- ATK-12: zero outputs + popup grab + focus change → the INC-3 zero-output pointer grab is torn down while no output exists;
- ATK-14: popup destroyed by its client, then a client implicit button grab (button held) is removed by the focus change.

## 3. Popup grab model

| Case | Result |
|---|---|
| valid grab on focused element | PASS (PF-1…PF-7) |
| grab on an **unfocused** element | refused silently: **no `popup_done`** (ATK-11; spec violation); mutation N (check removed) survives the shipped suite |
| grab while keyboard grabbed by another sequence | anvil logic, refuses with `ungrab(All)` (dismisses) — mutation L survives (untested) |
| invalid / stale / wrong-client / wrong-event serial | **no mechanism** (Smithay validates nothing; the adapter only compares with an existing grab). Mitigation: the root must already hold ActiveElementFocus. Observation INC4-F18 |
| nested popup (W-SM-14) | PASS (PF-6 detects workaround removal *only because the test forces the switch on*; default flipped to `false` survives — mutation V) |
| popup with wrong parent / parent dismissed | Smithay/protocol errors; not tested |
| parent destroyed | PF-3 destroys the popup *first* (client order) — parent-destroyed-with-live-popup not exercised |
| popup destroyed / in flight | PF-4/PF-5 PASS, no panic |
| click outside popup tree | Smithay PopupPointerGrab path; plus core click-to-focus preempts — not tested |
| compositor binding while grab active | Super+Enter → FocusNext → preemption: exercised by RES-1, no panic |
| CameraPan while grab active (OB-4) | `popup_done` delivered (PF-OB4, ATK-09) **but a synthetic `wl_pointer.motion 45,40` reaches the client at the pan press** (ATK-09) → INV-INPUT-3 breach |
| keyboard focus transfer while popup active | PASS (PF-1, PF-7) |
| output removal / zero outputs | **keyboard route moves P → W1, no `popup_done`, adapter keeps a stale grab** (ATK-15) |
| parent unmapped (M3 last row) | popups of an unmapped, unfocused parent are **not** dismissed (ATK-18) |

W-SM-7 verdict: precondition PASS (cannot panic); refusal semantics FAIL (no dismissal). W-SM-14: PASS behaviour, default unprotected. W-SM-15: PASS behaviour, unverified by the shipped suite.

## 4. OB-4 / CameraPan dismissal — **FAIL**

Core emits `DismissPopupGrab(focus target)` only on acquisition (before any camera change, since the camera moves only on motion) — correct ordering. The adapter's `ptr.unset_grab` replays motion to the client during an acquired CameraPan (ATK-09), the INC-3 stop condition "any pan-originated client event".

## 5. D-B6-BOUNDARY rule 1 — **FAIL (not on the production path)**

Canonical rule 1: anchor = output of the causing input event (grab serial, else seat's last input) if the parent is visible on it. Core `popup_constraint_target(parent, causing_output)` implements the selection, and its unit test does distinguish outputs (1920×1080 vs 800×600, clock output = 120 Hz o2) — the seeded control is genuinely detected. **However the only production caller (`handlers.rs:69`) passes `None`**, so production always uses rule 2. Pointer output, focus output, acquisition output are never consulted; mutation P (visibility guard dropped) survives; the unit test's assertions sit inside `if` guards that silently skip if the membership precondition ever changes. INC3-F08's transfer is therefore not discharged.

## 6. Zero-output / hotplug — **FAIL**

Correct: focus kept across removal and reconnect (K8), keys delivered at zero outputs, no fabricated coordinates (popup target `None` without an observing output), retained cameras unaffected (t1_outputs green). Defects: ATK-12 (zero-output grab removed), ATK-15 (keyboard route changed by output removal).
