# Phase F Increment 3 (INC-3) — CameraPan, Direct Manipulation Math, and Button Ownership Audit

---

## 1. Direct Manipulation Math (`F-PAN-1`)

The pan interaction implements exact physical direct manipulation:

$$C = C_{\text{start}} - Q16\left(\sum \frac{\Delta p_{\text{phys}}}{s_O \cdot z_C}\right)$$

In `crates/infynitae-core/src/input/interaction.rs`:
```rust
let term = |d: f64| (d * scale) / (scale * zoom);
```
- Physical delta is divided by scale and zoom, exactly cancelling scale.
- Summation accumulates in `f64` throughout the gesture.
- Single Q16 quantisation with ties-to-even rounding is performed only at camera update time (`pan_target` in `crates/infynitae-core/src/navigation.rs`).
- Camera coordinates saturate at `CANVAS_LIMIT_PX * Q16_ONE` ($\pm 2^{36}$ px).
- Direct manipulation is verified against trace literals: screen dragging matches canvas movement exactly, with zero perceptual slip.

---

## 2. Stage Immutability During Pan (`INV-CAMERA-2`)

- Camera pan moves ONLY the camera center.
- `Stage` element anchors remain strictly immutable throughout the gesture.
- Verified by adversarial test `m_pan_never_writes_the_stage`.

---

## 3. Button Ownership State Machine (`OBJ-25`, `M22`)

Button ownership prevents race conditions and event leakage between compositor interactions and client applications:

1. **Ownership Decided at Press:**
   - Evaluated in `Core::button_press`.
   - If Logo+MMB is pressed while no client owns any button, pan is acquired:
     ```rust
     self.input.ownership.press(button, Owner::Compositor);
     ```
   - If client already owns a button, pan acquisition is rejected, preserving client ownership (`x8_c6`).
   - If pan is active, subsequent presses (e.g. LMB during MMB pan) are assigned `Owner::Compositor` and consumed (`x8_c7`).
2. **Release Follows Press Owner:**
   - `Core::button_release` looks up the owner recorded at press time.
   - Compositor-owned release triggers batch-end reconciliation.
   - Client-owned release forwards `wl_pointer.button(released)` to the client.
   - Even if the pan was acquired, released, or orphaned, no compositor buttons ever reach the client.

---

## 4. `KeepFocus` Semantics & Batch-End Reconciliation (`M10`)

- During active pan, `KeepFocus` suppresses all motion and relative motion events to the client.
- The client sees zero motion events while the camera pans underneath.
- Upon release of the initiator button, `Core` performs batch-end reconciliation:
  - Calls `route_pointer()`.
  - Emits exactly ONE true-position `Effect::PointerMotion` with updated Canvas coordinates and `Effect::PointerFrame`.
  - **No Relative Motion Synthesized:** `INV-INPUT-4` is strictly respected; reconciliation never synthesizes spurious relative motion deltas.
