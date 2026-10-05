# INC-4 Review — Keyboard Ownership, Modifier Authority, ActiveElementFocus, Activation

Independent attack tests: `ATK-nn` (scratch suite `zz_review.rs`, see ADVERSARIAL_ATTACKS.md). Mutations: letters (same file).

## 1. K1–K9 (C4.4, M12)

| Rule | Canonical requirement | Implementation | Authoritative state | Shipped tests | Independent result | Verdict |
|---|---|---|---|---|---|---|
| K1 | ownership decided at press by **core's** decision returned from the intercept filter; consumed or forwarded | `infynitae-wayland/src/keyboard.rs` `keyboard_key`: the **adapter** branches on `matched`/`mods_changed`/`k3_enforced` and writes OBJ-25 via `core.key_ownership_mut().press(..)`; `press()`'s "already pressed" result is ignored | OBJ-25 `Core.input.key_ownership` (written by adapter) | k1, k3 | ATK-02: Super+B, Super up, B pressed again → **B press forwarded** to client while the table still says Compositor (leak, then stuck/consumed release). Decision site is the adapter, not core intake (INC4-F01). | **FAIL** (INC4-F01, INC4-F03) |
| K2 | release follows the press owner; never-pressed release must not be sent (protocol must-not) | `release(key) == Some(Compositor)` → consume, **else forward** (incl. `None`) | OBJ-25 | k3, k4, k7 | ATK-03 (B client-owned, Super pressed, B released) PASS. ATK-04: release with no press → forwarded → client sees "released 30 not logically down". Mutation F (owner recomputed at release) **MISSED** by the shipped suite (every shipped test releases binding keys while the modifier is still held). | **FAIL** (INC4-F03, INC4-F12) |
| K3 | modifier/lock-changing keys never compositor-owned (observe mode) | `Some(action) if !(mods_changed && k3_enforced)`; observe branch forwards | Smithay `mods_changed` | k8, neg_k3 | correct for the shipped binding set; control K3 detected | **PASS** (mechanism) — but the guard is a runtime-disableable field (INC4-F02) |
| K4 | focus change while held: `leave` old, `enter(forwarded)`+`modifiers` new | Smithay `KeyboardHandle::set_focus` (keyboard/mod.rs:1321-1365) | Smithay `forwarded_pressed_keys` | k4, k5, k8 | PASS on wire | **PASS** |
| K5 | binding while modifiers held: modifiers client-visible, binding key consumed with release | as K1/K2 | — | k3 | PASS | **PASS** |
| K6 | interaction cancel: compositor-owned keys stay owned until release; no synthetics | `KeyAction::Cancel => {}` — no cancel mechanism exists | — | k7 "interaction cancelled" — **no interaction exists in the test** | not exercisable; test vacuous for K6 | **NOT DEMONSTRATED** (INC4-F12) |
| K7 | focused surface destroyed/unmapped → successor; `enter` without `leave` | `Core::toplevel_destroyed`/unmap → `FocusModel::target_lost` | OBJ-22 | k6, pf3 | destroy PASS; **successor may be an unmapped element** (ATK-06) | **FAIL** (INC4-F07) |
| K8 | output removal has no keyboard effect | no keyboard code on disconnect | — | k8_ob15 (no popup) | ATK-15: with a popup grab, disconnecting the last output **moves keyboard focus P → W1** (INC-3 zero-output pointer grab replaces `PopupPointerGrab`, whose `unset` tears the keyboard grab down, smithay `desktop/wayland/popup/grab.rs:371-381, 697-700`) with no `popup_done` | **FAIL** (INC4-F09) |
| K9 | client-side repeat only | `add_keyboard(_, 200, 25)`; no compositor repeat | — | k9_ob15 | PASS (rate 25, delay 200; 0 synthesized repeats over 500 ms) | **PASS** |

**M12 verdict:** transitions Up→Down(Compositor/Client), Down→Up and focus-change rows hold on the shipped scenarios; the `Down → press` (repeat) case is unguarded (ATK-02), an ownerless release is forwarded (ATK-04), and the state machine is executed and written by the adapter rather than core intake. **FAIL.** "Device removed → treated as release" has no device model in INC-4 (headless; deferred with INC-16).

## 2. Modifier authority

- Keys: Smithay `KeyboardHandle` XKB updated inside `input_intercept` *before* the filter runs (keyboard/mod.rs:978-1003), so the binding sees the post-event state: no one-event-late/early error on the key path (mutation A semantically **equivalent** for the shipped bindings; ATK-03 and the shipped k2–k4 confirm client-visible modifiers equal seat state).
- Pointer buttons: `PhysicalEvent::PointerButton { modifiers }` is still a **backend-supplied snapshot** (INC3-F01 was ratified as scaffolding "until INC-4 integrates dynamic XKB"). ATK-13: Super physically held on the seat keyboard + MMB with an empty snapshot → **no CameraPan**; conversely a snapshot with `logo` and no key held acquires the pan. Two modifier authorities coexist (OBJ-27 says Smithay is the only one). **FAIL** (INC4-F02b → filed as INC4-F16).
- Observe-mode does not desynchronise clients (KV-3 holds in all shipped and independent key scenarios).

## 3. Binding semantics

Canonical documents define **no product key bindings**. `Super+Enter→FocusNext, Super+B→Noop, Esc→Cancel, LeftShift→Noop` are the **x10 harness scenario fixtures** (`experiments/b8-harness/src/bin/x10.rs:48-52`), copied verbatim into `CANONICAL_BINDINGS`. Consequences in production: **plain Escape is never delivered to any client** (ATK-01; Cancel is a no-op), Super+B is swallowed for nothing, Super+Return is taken. Super+Q / Super+H/J/K/L mentioned in the brief are **not** present. Classification: test fixtures hard-coded as product policy in core (INC4-F04, Level B; the absence of a canonical binding set is noted for adjudication).

## 4. ActiveElementFocus (OBJ-22, M11, INV-INPUT-8)

| Transition | Result |
|---|---|
| Unfocused → Active (click-to-focus) | works (ATK-09/10 use it); **no shipped T1 test**; mutation U (click-to-focus removed) result in ADVERSARIAL_ATTACKS |
| Active(A) → Active(B) | PASS (k5, pf1, ATK-08) |
| Active → Unfocused | only via the public `set_focus(None)` hook; no canonical production trigger in INC-4 |
| target destroyed | PASS for mapped successors; ATK-06 unmapped successor; ATK-07 fabricated successor |
| target unmaps | target loss runs; live set includes other unmapped elements |
| popup opens / closes | ActiveElementFocus unchanged (ADR-14) — PASS |
| output disappears / zero outputs / reconnect | focus retained (K8) — PASS; keyboard *route* broken with a popup grab (ATK-15) |
| focus history staleness | destroyed and unmapped ids are purged from history (`retain(!= dead)` on every loss) — no stale ids in history; **staleness lives in the live list** (`Stage::z` keeps unmapped elements; `stage.rs:166-176`) |

`focus_next` (ATK-05) focuses an **unmapped** element and Smithay sends it `wl_keyboard.enter` + `activated`. Focus cycling over `z_order` is not canonical either way (no canonical order is specified for focus-navigation); render order and focus history are distinct in the implementation (history is separate), which is correct.

Fallback "top live element" is **not** the canonical policy (C4.5: most recently focused live element). Verdict focus fallback: **FAIL** (INC4-F07 Level B; INC4-F11 Level C).

## 5. INV-PROTO-1 (`activated`)

Behaviour on mapped elements is correct (ATK-08 PASS: exactly one activated element through set/clear/destroy-successor). Defects: no shipped test asserts `activated` at all (mutations Q and R **MISSED**); unmapped elements can be activated (ATK-05); `activated` configures are applied *before* the M3/INV-INPUT-10 preemption in the same batch (effects `[ToplevelConfigure(prev), ToplevelConfigure(next), KeyboardFocus]`), the reverse of M11's effect listing (Observation); every transition uses `send_configure` (unconditional), duplicate configures are acceptable. Popups never receive toplevel activation (only `ToplevelConfigure` on toplevels). Verdict: **behaviour PASS, verification FAIL** (INC4-F12).
