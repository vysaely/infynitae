# Fifth Review — Input Authority, Capability Proofs, and Focus Coherence

## 1. Capability Proof Sealing: `InputAuthToken`

### Architectural Sealing
`InputAuthToken` in `crates/infynitae-core/src/lib.rs` models capability authority for synchronizing Smithay's seat XKB modifier state with an authenticated physical key event:
```rust
#[derive(Debug)]
pub struct InputAuthToken {
    core_id: u64,
    key: u32,
    pressed: bool,
    _not_send: NotSend,
}
```
Properties proved:
1. **Unforgeable:** All fields are private; constructible only via `Core::verify_input_port(&self, port: &InputPort, key: u32, pressed: bool)`.
2. **Instance-bound:** Bound to issuing `Core` via `core_id: u64`. A foreign token presented to another `WaylandState` fails closed (`token.core_id() != self.core.id()`), mutating nothing and returning current seat modifiers.
3. **Event-bound:** Carries the exact physical event `(key, pressed)` authorized. `sync_seat_key(token)` extracts `(token.key(), token.pressed())` directly from the token, preventing key substitution attacks.
4. **Single-use:** Consumed by value in `WaylandState::sync_seat_key(self, token)`. Re-use is a compile-time error (`error[E0382]: use of moved value`, pinned by probe `p31`).
5. **Thread-confined (`!Send`):** Marked with `PhantomData<*const ()>`, preventing transit across thread boundaries (pinned by probe `p30`).

### Token Abandonment
If a backend mints a token via `verify_input_port` and discards it without calling `sync_seat_key`, Core state remains completely uncorrupted. `verify_input_port` performs a read check on `InputPort.core_id` and mints the proof struct without writing to Core's internal tables. Key ownership and focus tables are modified only when the event is submitted via `intake()`. Verified in `attack_token_abandonment_does_not_corrupt_core`.

## 2. Duplicate Key and Modifier Transition Filtering

### Centralized Architecture
In earlier rounds, duplicate physical transition filtering was implemented ad-hoc in the headless backend. Round 4 centralized the transition gate in `WaylandState::sync_seat_key`:
```rust
let (key, pressed) = (token.key(), token.pressed());
let is_down = self.core.is_key_down(key);
if pressed == is_down {
    return (self.seat_modifiers(), false);
}
```
This guarantees a unified, fail-closed policy across all present and future backends:
- **Repeat physical press:** If `pressed == true` and `is_down == true`, returns immediately with zero XKB mutation.
- **Orphan release:** If `pressed == false` and `is_down == false`, returns immediately with zero XKB mutation.
- **Client wire protection:** Physical duplicates do not generate client wire events.
- **Modifier protection:** Pressing Shift $\rightarrow$ duplicate Shift $\rightarrow$ release Shift results in depressed modifiers returning strictly to 0. Verified across all keys (A, Shift, Ctrl, Alt, Super) and contexts (ordinary, focus transfer, popup, zero outputs) in `t04_duplicate_presses` and `attack_duplicate_physical_transitions_absorb_cleanly`.

## 3. Keyboard Confinement

`keyboard_handle()` is confined to `pub(crate)` within `infynitae-wayland` (pinned by compile probe `p29`).
External crates inspect keyboard state solely through read-only observer methods on `WaylandState`:
- `keyboard_is_grabbed(&self) -> bool`
- `keyboard_current_focus(&self) -> Option<WlSurface>`
- `keyboard_modifier_state(&self) -> ModifiersState`
- `seat_modifiers(&self) -> Modifiers`

Source rules in `enforcement/source-rules/src/main.rs` enforce:
- `smithay-keyboard-only-in-adapter`: Forbids `get_keyboard`, `input_forward`, `input_intercept` outside adapter and core.
- `smithay-pointer-only-in-adapter-pointer`: Forbids pointer handle mutation outside adapter pointer module.
- `effects-only-from-core`: Ensures backends never fabricate effects.

## 4. Focus Coherence (Core vs Smithay)

The fundamental invariant `INV-INPUT-8` dictates that Core's `ActiveElementFocus` and Smithay's seat keyboard focus must never diverge.
Under all conditions:
- **Click-to-focus:** Pointer click focuses mapped window in Core; Smithay keyboard focus and `wl_keyboard.enter` follow immediately.
- **Popup opening:** Popup acquires grab; Smithay keyboard focus shifts to popup; Core `ActiveElementFocus` tracks root element.
- **Popup closing:** `reconcile_popup_chain` restores Smithay focus to Core root before any key delivery.
- **Focus transition during popup:** Preemption dismisses popup; Smithay focus shifts directly to newly focused window with zero intermediate spurious enters (W-SM-15).
- **Unmap / Destroy:** Successor window receives focus; unmapped surface loses focus immediately.
