# Phase F INC-5 — OB-11 Decision Gate Dossier
## Evaluation of Smithay 0.7.0 Defect W-SM-4c & Patch Candidate P1

- **Incremental Phase:** Phase F INC-5
- **Baseline Commit:** `2680b99eb6d5353473ff362f5b4a904372f1fdfb` (INC-4 Ratified)
- **Authoritative Gate:** `FOUNDATION_TEST_PLAN.md` § OB-11; `FOUNDATION_DECISIONS.md` § D-B12-PIN; `FOUNDATION_INVARIANTS.md` § INV-ARCH-3, INV-INPUT-5.
- **Date:** October 7, 2026

---

## 1. Executive Summary & Decision Mandate

`OB-11` is a canonical architectural decision point blocking Phase F INC-5 release. Under `FOUNDATION_TEST_PLAN.md` § OB-11:
> *"The decision now gates the **whole** production constraint surface (INC-5), not only oneshot behaviour... Patch-policy criteria: (1) reproduced — met; (2) no public-API fix — met; (3) upstream issue/PR — open (outward action for the user)."*

This dossier provides the exhaustive evaluation required by Checkpoint J before shipping the INC-5 pointer constraints implementation. It characterizes:
1. The exact behavior of stock published Smithay 0.7.0 (`W-SM-4b` and `W-SM-4c`);
2. The exact violated invariants (`INV-INPUT-5`, `M13`);
3. The root cause in Smithay's seat pointer leave implementation;
4. The candidate patch P1 evaluated in `experiments/e1r-smithay-p1`;
5. Semantics, ABI, and upstream compatibility;
6. Formal recommendation under `D-B12-PIN`.

---

## 2. Defect Specification: W-SM-4b and W-SM-4c

### 2.1 Phenomenon
Under Wayland `zwp_pointer_constraints_v1`, when a surface's pointer constraint is deactivated (either explicitly by compositor policy or protocol lifecycle), the client receives an `unlocked` or `unconfined` event.

In published stock Smithay 0.7.0:
1. **W-SM-4b (Oneshot lock defuncted before enter):** When a oneshot lock request is registered on an unentered surface and the pointer leaves another surface or is repositioned, Smithay's internal pointer leave path unconditionally invokes `deactivate()`, firing an `unlocked` event for a lock that was never active. (Reproduced in harness experiment `X-12` / `TST-08`).
2. **W-SM-4c (Cross-batch duplicate deactivation):** When a persistent lock is deactivated explicitly (e.g. via `Effect::ConstraintDeactivate`), the client receives an `unlocked` event. If the pointer subsequent leaves the surface, Smithay's `PointerHandle::leave()` unconditionally invokes `deactivate()` on the registered constraint without checking whether it is currently active. This delivers a **duplicate second `unlocked` event** to the client for a single activation lifecycle. (Reproduced in T1 integration test `ob47_e1_persistent_explicit_deactivate_then_exit`).

### 2.2 Violated Canonical Invariants
- **`INV-INPUT-5` (Constraint activation and lifecycle):**
  > *"Every constraint transition is deterministic: Requested $\to$ Active $\to$ Inactive/Defunct. No spurious unlocked/unconfined event may be emitted to a client without a preceding active state."*
- **`M13` (Pointer Constraint State Machine):**
  > State transitions are strictly governed between `Requested`, `Active`, `Active (Deferred)`, and `Defunct`. Deactivation must occur exactly once per activation epoch.

---

## 3. Minimal Root Cause Analysis

In Smithay 0.7.0 (`src/wayland/seat/pointer.rs`, lines 246–256):

```rust
// Stock Smithay 0.7.0: src/wayland/seat/pointer.rs
if let Some((surface, _)) = old_focus {
    // ...
    with_pointer_constraint(self, &pointer, |constraint| {
        if let Some(constraint) = constraint {
            constraint.deactivate(); // <-- UNCONDITIONAL CALL
        }
    });
}
```

And in `src/wayland/pointer_constraints.rs` lines 143–157:
```rust
impl PointerConstraint {
    pub fn deactivate(&mut self) {
        // ...
        match self {
            PointerConstraint::Locked(locked) => {
                locked.deactivate(); // Sends zwp_locked_pointer_v1::unlocked unconditionally!
            }
            PointerConstraint::Confined(confined) => {
                confined.deactivate(); // Sends zwp_confined_pointer_v1::unconfined unconditionally!
            }
        }
        self.active = false;
    }
}
```

Because `leave()` does not check `if constraint.is_active()`, any surface with an inactive registered persistent constraint receives a redundant `deactivate()` upon pointer exit.

---

## 4. Evaluated Candidate Patch: P1

### 4.1 Patch Text
The minimal P1 patch (`experiments/e1r-smithay-p1/patches/0001-seat-leave-deactivate-only-active-constraint.patch`):

```diff
--- a/src/wayland/seat/pointer.rs
+++ b/src/wayland/seat/pointer.rs
@@ -250,7 +250,9 @@ impl<D: SeatHandler> PointerHandle<D> {
 
             with_pointer_constraint(self, &pointer, |constraint| {
                 if let Some(constraint) = constraint {
-                    constraint.deactivate();
+                    if constraint.is_active() {
+                        constraint.deactivate();
+                    }
                 }
             });
         }
```

### 4.2 Behavior & Verification Under P1
In `experiments/e1r-smithay-p1`:
- All 11 constraint lifecycle sequences (`EE-2` E1–E11) pass cleanly.
- `X-12` reproducer passes (zero spurious `unlocked` events).
- Spurious duplicate `unlocked` event on exit is eliminated (`duplicate_unlocked == 0`).
- No public API or ABI is altered. The patch is 4 lines of pure guard logic.

---

## 5. D-B12-PIN Compliance & Evaluation Criteria

Under `FOUNDATION_DECISIONS.md` § D-B12-PIN, a dependency patch is permissible if and only if three criteria are met:
1. **Criterion 1 (Reproduced):** Confirmed and reproduced in repository harness (`X-12`, `experiments/e1r-smithay-p1`, and production T1 test `crates/infynitae-backend-headless/tests/t1_pointer_constraints.rs::ob47_e1_persistent_explicit_deactivate_then_exit`). **CRITERION MET.**
2. **Criterion 2 (No Public-API Workaround):** `with_pointer_constraint` exposes no unregister/removal method; `PointerHandle.wl_pointer` is `pub(crate)` in Smithay, preventing adapter-level suppression of `leave()` without dropping seat pointer management. No public API exists to remove an inactive persistent constraint from Smithay's internal registry without destroying the Wayland resource. **CRITERION MET.**
3. **Criterion 3 (Upstream Action / Owner Adjudication):** An upstream issue / PR must be tracked, and the repository owner must adjudicate whether to apply P1 to the workspace dependencies or retain published stock Smithay with explicit negative control assertions. **ADJUDICATION REQUIRED.**

---

## 6. Options for Owner Adjudication

### Option A: Retain Published Stock Smithay 0.7.0 (Current Workspace State)
- **Mechanism:** Workspace builds against unmodified `smithay =0.7.0` from `crates.io`.
- **Negative Control:** T1 test `ob47_e1_persistent_explicit_deactivate_then_exit` explicitly asserts the stock defect behavior (`duplicate_unlocked == 1`), acting as an active regression sensor.
- **Pros:** Zero workspace patches, no custom cargo registry / vendor overlays, strict compliance with published dependency pins, 100% clean CI without local path dependencies.
- **Cons:** Clients experiencing explicit deactivation followed by pointer exit receive one duplicate `unlocked` event.

### Option B: Apply Patch P1 via `[patch.crates-io]`
- **Mechanism:** Configure `[patch.crates-io]` in root `Cargo.toml` pointing to `experiments/e1r-smithay-p1/vendor/smithay-0.7.0`.
- **Test Adjustment:** Invert `ob47_e1` assertion to `duplicate_unlocked == 0`.
- **Pros:** Eliminates `W-SM-4c` defect completely; enforces ideal protocol cleanliness on wire.
- **Cons:** Requires modifying root `Cargo.toml` with `[patch]`, which currently triggers architectural check in `./tools/verify-architecture` (Check 1: `root Cargo.toml has no [patch] or [replace] (P1 not productionized; OB-11 open)`).

---

## 7. Architect Recommendation

The architect recommends **Option A** for Phase F INC-5:
1. Stock Smithay 0.7.0 is preserved;
2. `tools/verify-architecture` passes cleanly without `[patch]` overrides;
3. `ob47_e1` guards and documents the exact stock defect behavior as a living negative control;
4. Upstream issue submission is queued as an outward user action per `FOUNDATION_TEST_PLAN.md` § OB-11;
5. If the project owner desires Option B in a future increment, `tools/verify-architecture` check 1 and `Cargo.toml` can be updated simultaneously under formal owner authorization.

This completes the `OB-11` Decision Gate evaluation.
