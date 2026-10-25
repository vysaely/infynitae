# Third Review — Authority (InputPort / InputAuthToken / probes) and Focus Coherence (INC4R-01)

## 1. Integrity
- HEAD = origin/main = `6819c8a5e28baca46ae73e4f58ce73ba3e5c27ca`; clean tree; ancestry `1ee520fa → … → 2bca2b9 → 9efcf85 → 99ec2f0 → 6819c8a`.
- First dossier unchanged since `19231ef` except the two `*.log` files now committed by `9efcf85` (`.gitignore` `!docs/archive/audit/**/*.log`). Second dossier committed by `9efcf85`, unchanged since; contents spot-checked against the session record (verdict, counts, R15/R17/N03 lines; line counts) — the original scratch copies were lost in the shutdown, so no `cmp` was possible.
- CHECKPOINT (mechanical, section-level): all 20 sections at `1ee520fa` and all 21 at `d5b7b95` are byte-present in HEAD; the round-1 section body is byte-identical (heading relabelled); round 2 appended. **Append-only restored.**
- No canonical `FOUNDATION_*` document changed in round 2.

## 2. INC4R-01 — Core/Smithay focus coherence: **RESOLVED**
Oracle per step: Core ActiveElementFocus vs Smithay `current_focus()` (alive?) vs client `wl_keyboard` logical focus, then a probe key that must be legally delivered (no wire violation) into the focused element's tree.

| Attack | Result |
|---|---|
| A popup grab → CameraPan → dismissal → popup destroyed → key | coherent |
| A2 same, client keeps the popup alive after `popup_done` | coherent |
| B nested (distinct serials) → CameraPan → both destroyed → key | keyboard coherent; **stale pointer grab** (R2-05) |
| C root unmaps during the pan after dismissal | coherent; successor focused |
| D root destroyed during the pan after dismissal | coherent |
| E dismissal → focus change to another root → key | coherent |
| F dismissal → zero outputs → key; reconnect → key | coherent both |
| G 160 cycles (nested every 2nd, zero-output every 5th) | 0 violations; history 1; ownership empty |
| T17 (second review's R17 loop) | **0 violations** (was 600); bounded |
Mutations N08, M03 (skip reconciliation), M04 (reconcile to None) all detected by shipped client-wire tests.

## 3. INC4R-02 — fail-closed intake: **PARTIALLY RESOLVED**
- `keyboard_key` now calls `verify_input_port` before `sync_seat_key`. T02: foreign Shift/Super/A press, foreign release, valid press → foreign release, foreign press → valid release — XKB `(dep,lat,lock,pressed)`, Core key table and client wire all unchanged after every rejection.
- `InputAuthToken { pub(crate) _private: () }`: not externally constructible (private field), not `Clone`/`Copy`/`Default`. **But:** (a) not bound to a Core — `sync_seat_key` ignores provenance; a token minted by another Core for *its* port mutates this seat (T02b: xkb `(0,0,0,0)→(1,0,0,1)`, client told Shift down); (b) taken by reference, never consumed, no lifetime tie — one verification yields a permanent portless capability (T02c); (c) `Send` (compile-asserted), so it can live in protocol state — unlike `InputPort`/`OutputLifecycleToken`/`Authorized`.
- p28 proves only that `sync_seat_key` has three parameters.

## 4. INC4R-03 / structural enforcement
| Probe | Proves | Gap |
|---|---|---|
| p22 | `key_ownership_mut` absent | name-only (closed in practice by `pub(crate)` press/release) |
| p23 | `ElementId::new` private | adequate |
| p24 | `set_focus` arity | runtime port check covered by `set_focus_rejects_foreign_port` (N17) |
| p25–p27 | the three setter/hook names absent without features | **capability-equivalent bypass via public `keyboard_handle()`**: `input_forward`, `set_focus`, `set_grab` compile from backend production code and pass all 13 source rules (pointer equivalents are caught) — R2-03 |
| p28 | `sync_seat_key` arity | token unbound/reusable (R2-02) |
Feature topology: `probe_*` features exist only on `infynitae-wayland` and are forwarded by `infynitae-backend-headless` features of the same name; no default, no dev-dependency enabling, resolver 2 → not enabled in normal builds (verified with the probe crate: p25–p27 fail E0599).
Production instrumentation added for N09 (R2-08): `popup_preemption_pending/_violated`, a side effect in `SeatHandler::seat_state`, a production `assert!` around `PopupGrab::ungrab` (which takes no state, so the assert can only fire for an inserted mutation).

## 5. Focus model (F07/F11 regression check)
MRU/Unfocused semantics re-confirmed by the shipped R06/R07 ports and t01c/d; N15-class fallback absent; `set_focus` port and mapped checks detected (N17/N18).
