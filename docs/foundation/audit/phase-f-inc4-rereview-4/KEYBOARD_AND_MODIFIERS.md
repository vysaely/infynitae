# Fifth Review — Keyboard, Modifiers, and Binding Policy

## 1. Product Binding Policy vs Test Fixtures

### Canonical Binding Registry
In `crates/infynitae-core/src/input/keys.rs`:
```rust
pub const CANONICAL_BINDINGS: [KeyBinding; 1] = [
    KeyBinding {
        evdev: KEY_ENTER,
        needs_logo: true,
        action: KeyAction::FocusNext,
    },
];
```
Only `Super+Enter -> FocusNext` is canonically bound.
Historical test fixtures (`Super+B`, `Super+Esc`, `Super+LeftShift`) were completely purged from production bindings.

### Unconfigured Key Passthrough
Any key combination not explicitly defined in `CANONICAL_BINDINGS` passes through to the focused client wire:
- `Super+B` is delivered directly to the client as a normal key event.
- `Super+Esc`, `Super+K`, `Super+A`, `Shift`, `Ctrl`, `Alt` are delivered to the client wire without compositor interception.
- Plain `Escape` cancels active interaction (e.g. CameraPan) under K6 when an interaction is in progress, but otherwise passes through to the client wire. Verified in `t11_unconfigured_keys_are_never_swallowed`.

## 2. Temporal Modifier Boundaries (M12 / K1..K3)

The key handling state machine maintains strict separation between compositor-owned and client-owned keys:
1. **Ownership at Press:** The owner of a key (`Compositor` vs `Client`) is determined at key press and remembered until key release.
2. **Modifier Transition Across Press/Release:** If a modifier is pressed, an unconfigured key is pressed, the modifier is released, and then the key is released, the release follows the owner decided at press time (`release_follows_press_owner_after_modifier_change`).
3. **K3 Observe Mode:** Modifier keys themselves are observed (not consumed) unless explicitly configured as a pure compositor binding (`k8_binding_on_modifier_key_with_k3_observe_mode`).

## 3. End-State Cleanliness

In both 200-cycle and 250-cycle continuous stress sessions:
- `keyboard_modifier_state().serialized.depressed == 0`
- `probe.kb.depressed == 0`
- `core.keys().pressed().is_empty()`
- `core.buttons().pressed().is_empty()`
No modifiers remain stuck, and no ownership leaks occur.
