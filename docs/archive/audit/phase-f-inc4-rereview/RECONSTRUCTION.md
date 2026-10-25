# INC-4 Re-Review — Integrity, Reconstruction, Diff Audit

## 1. Integrity

| Check | Result |
|---|---|
| HEAD = origin/main = `2bca2b9a204c7ae94b77a4c5d83a12c9e6ddef2d` | PASS (fetched; clean tree) |
| Ancestry `1ee520fa → f296a73 → d5b7b95 → 19231ef → aa216d3 → 2bca2b9` | PASS (linear, 5 commits after baseline) |
| Failed review dossier | `19231ef` added 7 Markdown files + 5 reproduction files; **no change since** (`git diff 19231ef..HEAD -- audit/phase-f-inc4-review` empty). Reproduction scripts and logs on disk are byte-identical to the reviewer's originals (`cmp`). Two `*.log` reproduction files were excluded by `.gitignore:21` and are **not in git** (INC4R-13). |
| Ratified INC-0…INC-3 evidence and audit dossiers; Phase A–E evidence | unchanged since `1ee520fa` |
| Canonical `FOUNDATION_*` documents | **unchanged** by the corrective work (no canonical weakening) |
| CHECKPOINT | INC-4 implementation entry **deleted**; INC-3 ratification/implementation lines **edited in place** (annotated "corrected per INC4-F13") — INC4R-12 |

## 2. Reconstructed INC-4 scope (unchanged from the first review, re-derived)
IN: M12/K1–K9 with core intake owning OBJ-25 (C1 table: backend submits `PhysicalEvent` with `InputPort`; Core owns intake/bindings/ownership; adapter applies forward/intercept); OBJ-27 XKB solely Smithay; M11/OBJ-22 (writers: click-to-focus, bindings, shell, activation grant, target loss → MRU live successor); INV-INPUT-10 ordered preemption + W-SM-7/14/15; M3 grab and parent-unmap rows; INV-PROTO-1 `activated`; OB-4 (`popup_done` before camera change, INV-INPUT-3 intact); OB-15; OB-30 (PF-1…7 + NEG-1/NEG-2/OBS-1 failing as predicted); OB-18 popup part; D-B6-BOUNDARY rule 1 (anchor = output of the causing input event — grab serial, else the seat's last input — if the parent is visible there).
OUT: constraints/locks/W-SM-16 consequences (INC-5); kick (INC-8); maximize/fullscreen (INC-7); shell focus (INC-11); OB-21 (T4/INC-16).

No corrective change bleeds into INC-5 behaviour (no constraint code). Forward risks for INC-5: INC4R-10 (unset_grab motion), INC4R-14 (cancel without reconciliation).

## 3. Diff audit `d5b7b95..2bca2b9` (40 files)

| Area | Change | Assessment |
|---|---|---|
| core `entry.rs` | `PhysicalEvent::KeyboardKey{key, pressed, modifiers, mods_changed, time_us}` handled in port-checked `physical_input`; `keyboard_press` (repeat guard, K3, bindings, ownership), `keyboard_release` (owner-driven, orphan dropped), `apply_key_action` (FocusNext, Cancel→Idle) | correct direction; client-owned repeat unguarded (INC4R-04) |
| core `keys.rs` | Esc without logo → Cancel only during an interaction; Super+Esc Cancel always | F04 B part fixed; fixture policy remains (INC4R-11) |
| core `focus.rs` / `stage.rs` / `protocol.rs` | fallback removed; `mapped_elements()`; `ElementId::new` `pub(crate)` | F07/F11 fixed |
| core `lib.rs` | `key_ownership_mut` removed; `set_focus(&mut InputPort, ..)` + mapped check; `focus_next` `pub(crate)`; `pointer_output()` | runtime port check untested (N17) |
| core `ownership.rs` | `press/release` `pub(crate)` | OBJ-25 structurally closed |
| backend `lib.rs` | `keyboard_key`: **`sync_seat_key` before `inject`**; `pointer_button`: snapshot **OR** seat mods; `set_focus(state, port, target)` | INC4R-02, INC4R-07 |
| adapter `keyboard.rs` | intake removed; `set_preemption_strategy`, `set_nested_grab_keep_pointer` kept `pub` + `#[doc(hidden)]` | INC4R-03 |
| adapter `lib.rs` | `k3_enforced` removed; **pub `sync_seat_key`**, `seat_modifiers` | INC4R-02 |
| adapter `outputs.rs` | effects `KeyboardForward` (input_forward) / `KeyboardIntercept` (no-op); preemption guarded by `has_ended()` and `has_grab(g.serial())`; OB-4 path no longer calls pointer `unset_grab` | latest-serial guard misses nested first-serial pointer grab (INC4R-06); OB-4 leaves kb focus on the popup (INC4R-01) |
| adapter `pointer.rs` | zero-output begin re-focuses the popup; **pub `test_set_keyboard_grab`** (installs an arbitrary keyboard grab) | INC4R-03; R12 |
| adapter `handlers.rs` | popup dismissal on parent unmap/destroy; `popup_done` on every refusal; rule 1 fed with `pointer_output()` | F08/F17 fixed; INC4R-05 |
| enforcement | p22/p23/p24; forbidden name `key_ownership_mut` | see KEYBOARD_AUTHORITY §3 |
| efficacy | +15 controls, −3 (K1/K3 re-anchored, D-B6 rule-1 selection retired) | header updated |
| tests | t1_keyboard 13→23, t1_popup_focus 13→23 (mostly ports of the first review's ATK suite); `neg_k3` removed (K3 NEG now only as efficacy control) | see ADVERSARIAL_ATTACKS §4 |
