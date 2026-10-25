# Fourth Review — Integrity, Round-3 Diff, Authority, Focus Coherence

## Integrity
- HEAD = origin/main = `60e5429821793ac2745ec6a9fcc171217061f224`; clean tree; ancestry `1ee520fa → … → 6819c8a → 033be36 → 60e5429`; no INC-5 code (no constraint paths in the diff).
- Dossiers: no change since their commits; third dossier reproduction files compared with the reviewer's surviving originals — byte-identical (8/8).
- CHECKPOINT: every section at `1ee520fa` (20), `d5b7b95` (21) and `6819c8a` (23) byte-present in HEAD; round 3 prepended above round 2 (append-only). No `FOUNDATION_*` document changed.
- No separate round-3 matrix commit; the matrix is part of `60e5429` (`evidence/phase-f/INC-4/corrective-r3/`).

## Round-3 production diff (authority before → after; new state/lifetime/failure modes)
| Path | Change | New state / lifetime | New failure mode |
|---|---|---|---|
| core `InputAuthToken` | `{core_id, NotSend}`; `id()`, `is_key_down()` | none | not event-scoped (R3-C3) |
| adapter `sync_seat_key(token, …)` | consumes token; foreign `core_id` → returns current mods, no mutation | — | none (fail-closed, no panic) |
| backend `keyboard_key` | transition gate from `core.is_key_down` before XKB sync | — | gate lives per backend (R3-C3) |
| core `keys.rs` | bindings = {Super+Enter} | — | none |
| adapter `keyboard_handle` | `pub(crate)`; read-only observers added | — | — |
| adapter `popup_destroyed` | cleanup; clear grab if ended or topmost; `element_committed(parent)` or **all elements** | — | lifecycle advanced by a destroy (R3-B1); nested chains not released (R3-A1) |
| adapter `PanDismissedPopupGrab` | replaces the popup pointer grab at pan acquisition; first post-pan event unsets with `restore_focus=false` and forwards | until first pointer event | none found |
| adapter preemption | pointer unset keyed on `popup_pointer_grab_serial` (install serial) | serial lives with the grab | — |
| adapter reconnect | `cleanup()` then restore only a live top grab, else clear bookkeeping | — | — |
| adapter `serial_outputs_len()` | observer | — | — |
| instrumentation | removed | — | — |

## Token (R2-02)
Identity: `NEXT_ID: AtomicU64` from 1 per `Core::new` — no reuse within a process, wrap not reachable; numeric equality is the ratified `InputPort` binding mechanism. Not forgeable (private fields), not `Clone`/`Copy`/`Default`, consumed by value (no reuse), `!Send` (`PhantomData` marker; p30 E0277), cross-Core rejected without mutation or panic (U02). Same-Core misuse (sync without inject) desynchronises seat vs Core (U02) — trusted-backend residual (R3-C3).

## Keyboard confinement (R2-03)
`keyboard_handle` `pub(crate)` (p29 E0624). Source rule bans `get_keyboard`, `input_forward`, `input_intercept` outside adapter/core (method, qualified path, macro tokens) — helper functions, closures, re-exports and UFCS in backend code are caught by name. Not type-system enforced: `WaylandState::seat()` is still public (INC-3) and `KeyboardHandle::from_resource(&WlKeyboard)` is not on the list (requires a server-side resource; exotic). Same standard as the ratified pointer rule — accepted.

## Focus coherence — R3-A1 (Level A)
Compositor-initiated dismissals stay coherent (pan, focus change, reconnect: U05, U09). Client-initiated close of a chain of depth ≥ 2: keyboard grab left installed, Smithay focus on a dead surface, Core target = root, client without an active surface; next key → `enter(keys={K})` + `key K pressed` (D02 depth 2/3; C09). Depth 1 clean. Present at `6819c8a`.
