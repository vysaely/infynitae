# Phase E E1R Adversarial Re-Review — Pointer Constraints & Smithay P1 (E-F2-02)

**Reviewer:** Independent Principal Systems Architect & Senior Rust/Wayland Compositor Engineer  
**Date:** 2026-10-05  
**Focus:** Adversarial audit of pointer constraints lifecycle, protocol compliance, Smithay 0.7.0 call sites, and proposed patch P1.

---

## 1. Protocol Requirements (`pointer-constraints-unstable-v1.xml`)

Per `wp_locked_pointer.unlocked` and `wp_confined_pointer.unconfined`:
- `locked` / `confined`: Notification that the constraint is activated.
- `unlocked` / `unconfined`: Notification that the constraint is **no longer active**.
- `oneshot` constraints become **defunct** upon receiving `unlocked` / `unconfined` and must be destroyed.
- `persistent` constraints may reactivate upon subsequent entry.

**Core Invariant Rule:** Emitting `unlocked` / `unconfined` when a constraint is **already inactive** or was **never activated** is an explicit protocol violation. Clients (games, SDL3, Chromium) tracking active constraint state enter invalid internal states upon receiving unpaired deactivation notifications.

---

## 2. Pinned Smithay 0.7.0 Defect Audit

### Source Analysis
In Smithay 0.7.0:
1. `src/wayland/seat/pointer.rs:251`:
   ```rust
   with_pointer_constraint(self, &pointer, |constraint| {
       if let Some(constraint) = constraint {
           constraint.deactivate();
       }
   });
   ```
2. `src/wayland/pointer_constraints.rs:143-158`:
   ```rust
   pub fn deactivate(self) {
       match self.entry.get() {
           PointerConstraint::Confined(confined) => {
               confined.handle.unconfined();
               confined.active.store(false, Ordering::SeqCst);
           }
           PointerConstraint::Locked(locked) => {
               locked.handle.unlocked();
               locked.active.store(false, Ordering::SeqCst);
           }
       }
       if self.lifetime() == WEnum::Value(Lifetime::Oneshot) {
           self.entry.remove_entry();
       }
   }
   ```

### Why Stock Smithay Fails
- `pointer.leave()` unconditionally executes `constraint.deactivate()` on ANY constraint registered in `data.constraints`.
- If an active persistent constraint was explicitly deactivated by the compositor while keeping focus (e.g. camera pan moves lock point offscreen), `deactivate()` was already called and emitted `unlocked`.
- When focus subsequently leaves the surface in a later batch, `pointer.leave()` calls `deactivate()` a second time, emitting a **duplicate `unlocked`** event on the wire.
- If a constraint was created by a client but never activated by the compositor, `pointer.leave()` emits `unlocked` for a constraint that was never locked. For oneshot constraints, this destroys the constraint before it can ever activate.
- In `experiments/b8-harness/src/bin/xlock_batch.rs`, stock Smithay fails **9 out of 11 lifecycle sequences** (E1, E2, E4, E5, E7, E8, E9, E10, E11).

---

## 3. Public API Feasibility Check

Can this defect be avoided using public Smithay APIs without modifying Smithay?
- `data.constraints` in `PointerConstraintData` is private. There is no public API to unlink or remove an inactive persistent constraint.
- `PointerHandle.wl_pointer` is `pub(crate)` in Smithay. An adapter cannot send `wl_pointer.leave` without invoking Smithay's internal `leave()` logic.
- Ending a constraint by calling `leave()` and then returning emits an immediate duplicate `unlocked` on subsequent exit (proven by probe E11).
- **Conclusion:** No public Smithay API can represent this lifecycle correctly. A framework fix is mandatory.

---

## 4. Audit of Proposed Patch P1

Proposed patch P1 (`experiments/e1r-smithay-p1/patches/0001-seat-leave-deactivate-only-active-constraint.patch`):
```diff
--- a/src/wayland/seat/pointer.rs
+++ b/src/wayland/seat/pointer.rs
@@ -250,7 +250,9 @@
 
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

### Call-Site Audit across Smithay 0.7.0
A comprehensive grep of the entire Smithay 0.7.0 codebase revealed only **four** invocation sites of `with_pointer_constraint`:
1. `src/wayland/seat/pointer.rs:251` — The `leave()` hook (patched by P1).
2. `src/wayland/pointer_constraints.rs:476` — `SetRegion` dispatch for confined pointers (does not call `deactivate`).
3. `src/wayland/pointer_constraints.rs:524` — `SetCursorPositionHint` dispatch for locked pointers (does not call `deactivate`).
4. `src/wayland/pointer_constraints.rs:531` — `SetRegion` dispatch for locked pointers (does not call `deactivate`).

No other site in Smithay calls `deactivate()`.

### Semantic Impact Analysis
- Is there any valid Wayland compositor scenario where `leave()` on a surface should emit `unlocked` / `unconfined` for an inactive constraint? **NO.**
- Under protocol rules, deactivation events report transition from active to inactive. Emitting deactivation on an already-inactive constraint is always invalid.
- P1 restricts automatic deactivation on `leave()` strictly to constraints that are actually active (`constraint.is_active()`).
- All 11/11 lifecycle sequences in `xlock_batch` pass under P1.

---

## 5. Outward Action Strategy & Ranking

Per Section 8 of the review instructions:
1. **Rank 1: Option 2 / 1 (Upstream Issue & PR):** P1 is an unarguable, clean 2-line framework bug fix that benefits all Smithay compositors. It should be submitted upstream.
2. **Rank 2: Option 3 (Pinned Vendored Patch via `[patch.crates-io]`):** Recommended for immediate Phase F foundation stability. Because upstream release cadences are outside the project's control, pinning Smithay `=0.7.0` with P1 vendored under `OB-11` ensures 100% reproducible protocol compliance.
3. **Rank 3: Option 4 (Adapter Redesign):** Not recommended. Redesigning the adapter to avoid `pointer.leave()` would require re-implementing `wl_pointer` protocol handling from scratch.

### Maintenance & Pinning Impact of Vendored Patch
- **Maintenance Cost:** Minimal (3-line patch in one file).
- **Pinning Requirements:** Strict exact pin `=0.7.0` already enforced by `INV-ARCH-3`.
- **ABI/API Impact:** None. Zero changes to public types or signatures.
- **Regression Gates:** `experiments/b8-harness/src/bin/xlock_batch.rs` (EE-2) and `xzero.rs` (EE-3) execute as required gates before shipping `INC-5`.

---

## 6. Verdict on Primary Attack B

`P1 ACCEPTABLE AS MINIMAL PINNED FRAMEWORK CORRECTION`
