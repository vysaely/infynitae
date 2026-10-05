# Camera Binding & INV-CAMERA-3 Audit

## 1. The 1:1 Invariant (`INV-CAMERA-3`)

`INV-CAMERA-3` mandates:
> Each connected output is bound to exactly one camera, and a camera is bound to at most one output.

In `crates/infynitae-core/src/navigation.rs`:
```rust
struct Camera {
    center: CanvasPos,
    zoom: f64,
    key: OutputKey,
    bound: Option<OutputId>,
}
```
A camera is bound to at most one `OutputId` via `bound: Option<OutputId>`.

### Binding Precedence on `output_connected`
When an output with `OutputKey` and `OutputId` connects:
1. **Rule 1 (Remembered-and-Unbound):** Search existing cameras for the lowest-indexed camera matching `key` that is **currently unbound** (`c.bound.is_none()`):
   ```rust
   let free = self.cameras.iter().position(|c| &c.key == key && c.bound.is_none());
   if let Some(i) = free {
       self.cameras[i].bound = Some(output);
       return i;
   }
   ```
   If found, it is re-bound with its existing `center` and `zoom` preserved.
2. **Rule 2 (Persisted Lookup):** Check if any camera with `key` already exists in this session:
   ```rust
   let collision = self.cameras.iter().any(|c| &c.key == key);
   let seeded = if collision { None } else { persisted.lookup(key) };
   ```
   If a camera with that key already exists (a concurrent key collision), the persisted entry is NOT seeded to the duplicate output.
   If no camera with that key exists, consult `persisted.lookup(key)`.
3. **Rule 3 (Internal Default):**
   ```rust
   let (center, zoom) = seeded.unwrap_or((CanvasPos { x: 0, y: 0 }, 1.0));
   self.cameras.push(Camera {
       center,
       zoom,
       key: key.clone(),
       bound: Some(output),
   });
   ```
   The camera is created with internal default placement (Canvas origin, zoom 1.0).

---

## 2. Runtime 2i Reproduction

Runtime 2i (transferred from OB-19 via Decision D in INC-0 ratification) tests concurrent `OutputKey` collisions and verifies that the sealed 2R rule (which stole bound cameras) is caught.

### Reproduction Results
1. **Normal Build (1:1 Rule Active):**
   - T0 core: `concurrent_output_key_collision_keeps_one_to_one` → **PASS**.
   - T1 lockstep: `same_key_outputs_keep_one_to_one` → **PASS**.
   Both outputs receive distinct cameras (`ca != cb`), distinct `OutputId`s, and distinct `wl_output` names (`A` and `A-2`).
2. **Negative Control Feature (`--features probe_sealed_key_steal`):**
   - The sealed rule re-binds `.position(|c| &c.key == key)`, stealing a camera even while it is bound to another output.
   - T0 core test fails with:
     ```
     panicked at ... 1:1 violated: a=None b=Some(0)
     ```
   - T1 lockstep test fails with:
     ```
     panicked at ... 1:1 violated: None Some(0)
     ```
   - Stage `[5b]` of `./tools/verify-architecture` executes both tests and asserts this exact failure message.

---

## 3. Adversarial Variant Attacks

Beyond the sealed 2R control, additional variants were tested in scratch:
- **Variant B (Sharing a Camera):** Seeded mutation where duplicate output shares camera index `i`.
  *Result:* Caught by T1 lockstep and T0 collision tests.
- **Variant K (Reconnect Receives Wrong Camera):** Seeded mutation creating fresh camera instead of re-binding remembered camera.
  *Result:* Caught by `lifecycle_on_the_wire` and `disconnect_keeps_the_camera_and_reconnect_rebinds_it`.
- **Reconfiguration Camera Steal / Rebind:** Mutation attempting to rebind a camera during `output_reconfigured`.
  *Result:* Caught by `./tools/verify-architecture` source rule `reconfigured-no-camera-authority`.
