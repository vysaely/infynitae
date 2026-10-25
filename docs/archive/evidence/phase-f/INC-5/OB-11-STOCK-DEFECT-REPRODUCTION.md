# Phase F INC-5 — OB-11 Stock Defect Historical Evidence
## Preserved Reproduction of Smithay 0.7.0 Defect W-SM-4c Prior to Patch P1

- **Incremental Phase:** Phase F INC-5
- **Baseline Commit:** `2680b99eb6d5353473ff362f5b4a904372f1fdfb`
- **Dependency Under Test:** Unpatched `smithay = "=0.7.0"` from crates.io
- **Crates.io Source Checksum:** `740cea6927892bc182d5bf70c8f79806c8bc9f68f2fb96e55a30be171b63af98`
- **Date Recorded:** October 7, 2026
- **Status:** PRESERVED HISTORICAL EVIDENCE (Pre-P1 Baseline)

---

## 1. Summary of Defect Observations

In stock published Smithay 0.7.0, two related defects exist in constraint lifecycle management:
1. **W-SM-4b (Oneshot constraint defuncted prematurely):** An unactivated oneshot constraint registered on a surface is defuncted and destroyed when pointer leaves another surface or when pointer focus shifts, delivering an illegitimate `unlocked` event. Reproduced on test harness `experiments/b8-harness/src/bin/x12.rs` (`TST-08`).
2. **W-SM-4c (Duplicate deactivation across event batches):** When an active constraint is explicitly deactivated (or destroyed) while the pointer remains on the surface, the first `unlocked` event is delivered to the client. Subsequently, when the pointer physically leaves the surface, Smithay's internal `PointerHandle::leave()` method unconditionally executes `with_pointer_constraint(..., |c| c.deactivate())` without checking `if constraint.is_active()`. This emits a **duplicate second `unlocked` event** across distinct event batches for a single activation lifecycle.

---

## 2. Stock Reproduction Sequence

The following deterministic sequence was executed against stock `smithay = "=0.7.0"` in `crates/infynitae-backend-headless/tests/t1_pointer_constraints.rs::ob47_e1_persistent_explicit_deactivate_then_exit`:

```rust
// 1. Surface 0 enters focus and registers a persistent lock
focus_element(&mut r, 0);
r.ls.probe.lock_pointer(&qh, 0, false);
r.ls.pump();
r.frame();
assert!(helper_smithay_active(&r, 0)); // Active: locked event delivered

// 2. Explicit deactivation while focus is held
let root = helper_server_surface(&r, 0);
let surface_key = r.ls.state.registry().key(&root).unwrap();
let fx = r.ls.state.core_mut().destroy_pointer_constraint(surface_key);
r.ls.state.apply(&fx);
r.ls.pump();
r.frame();
// Result: 1st "unlocked" event delivered to client

// 3. Pointer moves off surface
let m = r.mark();
r.motion((1000.0, 1000.0));
let exit_events = r.since(m);

// 4. Stock Smithay behavior:
let duplicate_unlocked = exit_events.iter().filter(|s| **s == "unlocked").count();
assert_eq!(duplicate_unlocked, 1);
```

### Observed Event Log on Stock Smithay 0.7.0
```text
running 1 test
exit_events: [
    "leave",
    "unlocked",  <-- SPURIOUS SECOND UNLOCKED EVENT (W-SM-4c defect)
    "frame"
]
test ob47_e1_persistent_explicit_deactivate_then_exit ... ok (asserting duplicate_unlocked == 1)
```

---

## 3. Preserved Artifacts & Reference Repositories
1. **Source File with Defect:** `smithay-0.7.0/src/wayland/seat/pointer.rs`, lines 246–256.
2. **Harness Negative Controls:**
   - `experiments/b8-harness/src/bin/x12.rs` (TST-08 W-SM-4b reproducer)
   - `experiments/b8-harness/src/bin/xlock_batch.rs` (EE-2 E1 W-SM-4c reproducer)
3. **Candidate Evaluated:** `experiments/e1r-smithay-p1/patches/0001-seat-leave-deactivate-only-active-constraint.patch`.
4. **Adjudication:** On October 7, 2026, the project owner adjudicated Option B, authorizing the application of patch P1 to eliminate this defect in production under `D-B12-PIN`.
