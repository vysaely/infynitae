# Output Lifecycle & Identity Audit

## 1. Output Lifecycle State Machine (M4 / M5)

The production lifecycle implementation in `crates/infynitae-core/src/entry.rs` and `crates/infynitae-wayland/src/outputs.rs` was audited across all canonical transitions:

### Connection (`output_connected`)
- **Authority:** Enters strictly via `&mut OutputLifecycleToken`. Validates token ownership against `Core::id` (rejects foreign tokens with `EntryError::ForeignPort`).
- **Validation:** Descriptor validated all-or-nothing: requires `mode.0 > 0`, `mode.1 > 0`, `scale_120 > 0`. Invalid descriptor rejected with `EntryError::InvalidDescriptor` before any state is mutated or IDs allocated.
- **Identity:** Allocates a strictly monotonic `OutputId` via `OutputIds::next()`.
- **Arrangement:** Places output right of current outputs: `(right_edge, 0.0)`.
- **Camera Binding:** Mints one `Authorized` proof; calls `CameraSet::bind_on_connect`.
- **Zero-Output Transition:** If pointer state is `NoOutput`, enters startup rule: places pointer at center of new output's logical extent (`clamp_halfopen((w/2, h/2), extent)`).
- **Effects:** Emits `Effect::OutputAdded(id)` followed by `Effect::Enter` from `visibility_pass()`.

### Reconfiguration (`output_reconfigured`)
- **Authority:** Enters via `&mut OutputLifecycleToken`. Mints **zero** `Authorized` proofs (enforced by `verify-architecture`).
- **Preservation:** Preserves `OutputId` and `OutputKey`. Rejects unknown IDs with `EntryError::UnknownOutput`. Rejects modified `OutputKey` with `EntryError::IdentityChanged`.
- **Transactional Rollback:** Invalid descriptors or key mismatches return error before modifying `slot`.
- **Derived State:** Rebuilds `OutputModel`, clears cached `ProjectionFrame`, clamps pointer via `pointer::reconfigured` (`F-RECONF-1`).
- **Wire Events:** Adapter calls `Output::change_current_state()`, broadcasting mode, transform, scale, and location events, followed by `done`.
- **Effects:** Emits `Effect::OutputChanged(id)` followed by leave and enter events from `visibility_pass()`.

### Disconnection (`output_disconnected`)
- **Ordering:** Emits `Effect::Leave` for all surfaces on the output **before** `Effect::OutputRemoved(output)`.
- **Wire Order:** In `WaylandState::apply()`, `Output::leave()` is called while `OutputHandle` is still present, delivering protocol `wl_surface.leave` to clients. Then `output_removed()` unregisters the global via `DisplayHandle::remove_global()`, broadcasting `wl_registry.global_remove`.
- **Camera Unbinding:** Mints one proof; calls `CameraSet::unbind_on_disconnect`. Camera is unbound (`bound = None`) and retained with its exact coordinates and zoom.
- **Pointer Relocation:** If pointer was located on the disconnected output:
  - If survivors exist: calls `pointer::relocate(&lost, p, first, rest)` implementing `F-RELOCATE-1`.
  - If no survivors exist: enters `PointerState::NoOutput`.
- **Frame Drop:** Purges cached `ProjectionFrame` for the output (`self.frames.retain(|(o, _)| *o != output)`).

### Reconnection
- Reconnection with a previously disconnected `OutputKey` allocates a **new** `OutputId` and a **new** Wayland global.
- Re-binds the remembered camera with unchanged center and zoom.
- Emits `OutputAdded` followed by `Enter` for surfaces now observable.

---

## 2. Output Identity Model

The tripartite identity model was audited for determinism and collision safety:

1. **`OutputId` (Runtime Identity):**
   - Type: `struct OutputId(u64)`.
   - Semantics: Monotonically increasing counter (`OutputIds`). Never recycled or reused during the session.
   - Tested: Mutation L (reusing ID) detected by multiple lifecycle and wire tests.

2. **`OutputKey` (Hardware / Persistence Identity):**
   - Type: `struct OutputKey { device: String, connector: String, make: String, model: String, serial: Option<String> }`.
   - Semantics: Compares by structural value (`PartialEq, Eq, Hash`). Persistent identity across reconnections. Not guaranteed unique per physical monitor (two identical monitors without serial share a key).
   - Collision Resolution: Handled deterministically by `INV-CAMERA-3` and runtime 2i.

3. **`Camera` (Spatial Navigation Identity):**
   - Type: `struct Camera { center: CanvasPos, zoom: f64, key: OutputKey, bound: Option<OutputId> }`.
   - Semantics: Spatial viewport into the infinite Canvas. Identified by index in `CameraSet`. Survives output removal.
   - Binding: Exactly 1:1 with active outputs.

---

## 3. Zero-Output State & F-RELOCATE-1 Domain

- `PointerState::NoOutput` is a first-class state.
- Before any output connects, and after all outputs disconnect, `c.pointer() == PointerState::NoOutput`.
- No coordinates or outputs are fabricated.
- `F-RELOCATE-1` domain:
  ```rust
  pub(crate) fn relocate(
      lost: &OutputModel,
      p: (f64, f64),
      first: &OutputModel,
      rest: &[OutputModel],
  ) -> PointerState
  ```
  Taking `first: &OutputModel` guarantees by the Rust type system that `relocate()` cannot be invoked on an empty survivor set.
  When `self.outputs.split_first()` returns `None`, Core directly assigns `PointerState::NoOutput`.
