# Phase F Increment 3 (INC-3) — Independent Reconstruction & Diff Audit

- **Reviewed Range:** `6470e1a5e22065baa940d4f5b5dd0bd2b1e281a7..0c4e961f9d85c62dec9915f2ab0c9141afeeb897`
- **Pre-INC-3 Ratified Baseline:** `6470e1a`
- **Reviewed HEAD:** `0c4e961`

---

## 1. Independent Scope Reconstruction

INC-3 is the third production increment of Phase F. It establishes:
1. **Production Input Authority:**
   - Physical event intake through `InputPort` (`crates/infynitae-core/src/entry.rs`).
   - Private `Authorized::mint()` in `physical_input` passed to `CameraSet::pan`.
   - Compile-time negative probes `p18`–`p21` and source-rule enforcement preventing external authority forgery.
2. **Button Ownership (`OBJ-25`):**
   - Evaluation strictly at button press (`press(button, owner)`).
   - Release strictly follows press owner.
   - Compositor ownership of Logo+MMB prevents any leakage to client.
   - Client ownership of buttons during pan prevents loss or stuck buttons.
3. **CameraPan State Machine (`M22`, `OBJ-26`, `F-PAN-1`):**
   - Direct manipulation formula: `C = C_start - Q16(Σ dp_phys / (s_O * z_C))`.
   - Exact physical-to-logical transformation with scale cancellation: `(d * scale) / (scale * zoom)`.
   - Stage anchors remain strictly immutable during pan (`INV-CAMERA-2`).
4. **Pointer Routing & Reconciliation (`M10`, `KeepFocus`):**
   - KeepFocus suppresses all client motion and relative motion events during pan.
   - Batch-end release produces exactly one true-position reconciliation via `route_pointer()`.
   - Relative motion is never synthesized at reconciliation (`INV-INPUT-4`).
5. **Zero-Output Seat Transition (`M10`, `OBJ-20`):**
   - Headless/Wayland `ZeroOutputGrab` installed with `Focus::Clear` (emits one `wl_pointer.leave`).
   - Discards all motion/buttons/axes while in `PointerState::NoOutput`.
   - Reconnecting output unsets grab and places pointer at default center without stale history.
6. **Hit-Testing against `ProjectionFrame` (`INV-HIT-1`, `F-HIT-1`):**
   - Pointer routes against presented `ProjectionFrame` using floor rounding in physical space.
   - Dropping `ProjectionFrame` on pan ensures immediate reconciliation with newly panned camera.

---

## 2. Complete Diff Audit (`6470e1a..0c4e961`)

The diff spans 20 files, with 2,427 insertions and 110 deletions.

### A. Production Crates
- `crates/infynitae-core/src/entry.rs`:
  - Implements `physical_input(&mut self, port: &mut InputPort, ev: PhysicalEvent) -> Result<Vec<Effect>, EntryError>`.
  - Implements `button_press`, `button_release`, `routed_motion`, `true_position`, `route_pointer`.
  - Handles `Interaction::CameraPan`, `Interaction::Orphaned`, `Interaction::Idle`.
  - Drops `ProjectionFrame` on pan: `self.frames.retain(|(o, _)| *o != output)`.
- `crates/infynitae-core/src/input/`:
  - `ownership.rs`: Implements `ButtonOwnership` map and `Owner` enum (`Compositor`, `Client`).
  - `interaction.rs`: Implements `Interaction` enum and `pan_motion` helper.
  - `bindings.rs`: Pure predicate `is_pan_binding(button, mods)`.
- `crates/infynitae-core/src/navigation.rs`:
  - Added `pan(proof, camera, start, sum)` validating `start` and calculating `pan_target` with saturating clamp.
- `crates/infynitae-core/src/pointer.rs`:
  - Implements `locate` (clamping to union of output rects) and `relocate` (nearest survivor via Euclidean distance).
- `crates/infynitae-wayland/src/pointer.rs`:
  - Implements `ZeroOutputGrab` (`Focus::Clear`), `apply_pointer` mapping Core `Effect` to Smithay pointer methods.
- `crates/infynitae-backend-headless/src/lib.rs`:
  - Implements `pointer_motion`, `pointer_button`, `pointer_axis`, `inject`, and `vblank`.

### B. Enforcement Machinery
- `enforcement/probes/src/lib.rs`:
  - Added probes `p18` (`physical_input` requires `InputPort`), `p19` (`pan` requires `Authorized`), `p20` (`pan` proof not mintable outside Core), `p21` (InputPort not cloneable).
- `enforcement/source-rules/src/main.rs`:
  - Added `effects-only-from-core`: ensures only Core creates `protocol::Effect`.
  - Added `smithay-pointer-only-in-adapter-pointer`: ensures PointerHandle calls stay strictly inside `crates/infynitae-wayland/src/pointer.rs`.
- `tools/verify-test-efficacy`:
  - Added 30 new seeded behavioral controls (47 to 76) covering all INC-3 invariants and formulas.

### C. Test Suites
- `crates/infynitae-backend-headless/tests/t1_input.rs`:
  - 17 integration tests verifying pan, keepfocus, button ownership, zero-output transitions, and reconciliation.
- `crates/infynitae-backend-headless/tests/t1_adversarial.rs`:
  - 13 adversarial tests covering physical boundaries, fractional scaling, overlapping surfaces, negative canvas coords, unmapped surfaces, input region holes, and stage immutability.
- `crates/infynitae-backend-headless/tests/t1_transform.rs`:
  - EE-1 C5 render and hit agreement on the production path.
