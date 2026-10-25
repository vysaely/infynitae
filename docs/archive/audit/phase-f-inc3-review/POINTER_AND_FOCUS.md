# Phase F Increment 3 (INC-3) — Pointer Routing, Focus, and Zero-Output Grab Audit

---

## 1. Coordinate Pipeline: Physical -> Output -> Canvas

The pointer subsystem manages the translation between physical device events and Wayland client surface coordinates:

1. **Physical Delta Intake:**
   - Raw relative motion arrives via `PhysicalEvent::PointerMotion { delta, delta_unaccel, time_us }`.
2. **Output Logical Space Placement:**
   - In `crates/infynitae-core/src/pointer.rs`, `locate()` places the pointer inside the arrangement.
   - Points are half-open clamped to the union of output bounding boxes: `[x_O, x_O + w_O - eps)`.
   - On boundary ties, the first output in declaration order wins.
3. **Hit-Testing on `ProjectionFrame` (`INV-HIT-1`, `F-HIT-1`):**
   - In `crates/infynitae-core/src/lib.rs`, `hit_test(output, pt)` retrieves the current `ProjectionFrame` for the output.
   - Physical selection uses floor rounding: `px = (pt.x * s).floor() as i64`.
   - Traverses entries front-to-back (`rev()`), respecting subsurface ordering and input regions (`contains(local_x, local_y)`).
   - Only mapped surfaces (`entry.shown == true`) participate.
4. **Delivered Canvas Coordinates (`INV-COORD-5`):**
   - On hit: `origin + local` Canvas coordinate is delivered to Smithay.
   - On miss: Unprojected background canvas position `view.unproject(pt)` is delivered, and focus is cleared.

---

## 2. Zero-Output Seat Transition (`M10`, `OBJ-20`)

When all outputs are removed, the pointer enters `PointerState::NoOutput`:

1. **Grab Installation (`PointerZeroOutputBegin`):**
   - Installed via `pointer.set_grab(ZeroOutputGrab, serial, Focus::Clear)`.
   - `Focus::Clear` causes Smithay to emit exactly one `wl_pointer.leave` to the currently focused surface.
   - `ZeroOutputGrab` discards all motion, button, and axis events.
2. **State Cleanup:**
   - Adapter sets `state.pending_focus = None`, ensuring that reconnecting an output does not restore a stale pending focus before fresh routing occurs.
3. **Output Reconnection (`PointerZeroOutputEnd`):**
   - When the first output connects, `PointerZeroOutputEnd` unsets the grab.
   - Pointer is positioned at the default center of the new output (`w/2, h/2`) without historical motion or fabricated deltas.

---

## 3. Output Removal & Interaction Orphaning (`C4.6`, `OB-3`)

When an output is removed while a pan interaction is active:
1. `orphan_if_on(output)` transitions `Interaction::CameraPan` to `Interaction::Orphaned`.
2. All subsequent motion until release is silently consumed and discarded; survivor outputs and cameras remain unmodified.
3. If the pointer was on the removed output, `pointer::relocate` selects the nearest survivor output using Euclidean center distance.
4. Batch-end button release transitions `Orphaned` to `Idle` without synthesizing spurious motion.
