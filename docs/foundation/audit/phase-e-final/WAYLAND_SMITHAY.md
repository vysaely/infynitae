# Phase E Final Review — Wayland Protocol & Smithay Framework Audit

**Review Date:** 2026-10-05  
**Auditor:** Independent Principal Systems Architect & Senior Rust/Wayland Compositor Engineer  

---

## 1. Pinned Smithay 0.7.0 Pointer Constraint Audit

The pinned dependency under review is `smithay = "=0.7.0"` (specifically `/home/vysaely/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/smithay-0.7.0`).

### 1.1 Source Code Mechanics in Stock Smithay

In stock Smithay 0.7.0, pointer constraints are governed by:
- `src/wayland/pointer_constraints.rs`
- `src/wayland/seat/pointer.rs`

Inspection of `src/wayland/seat/pointer.rs` lines 246–263 reveals the `leave` implementation for focused pointer surfaces:
```rust
fn leave(&self, seat: &Seat<D>, _data: &mut D, serial: Serial, time: u32) {
    if let Some(pointer) = seat.get_pointer() {
        pointer.wp_pointer_gestures.leave::<D>(self, serial, time);
        pointer.wl_pointer.leave(self, serial, time);

        with_pointer_constraint(self, &pointer, |constraint| {
            if let Some(constraint) = constraint {
                constraint.deactivate();
            }
        });
    }
    // ...
}
```

Now inspecting `src/wayland/pointer_constraints.rs` lines 143–158:
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

### 1.2 Protocol Defect Mechanism (Finding E-F2-02)

1. **Unconditional Event Emission:** `deactivate()` unconditionally sends `unlocked` or `unconfined` across the Wayland wire, regardless of whether `self.is_active()` is true.
2. **Unconditional Call on Leave:** When a surface loses pointer focus, `pointer.rs:leave()` invokes `constraint.deactivate()` on *any* registered constraint for that surface.
3. **Invalid Protocol Sequences Produced by Stock Smithay:**
   - **Explicit Deactivation Followed by Leave:** If a compositor deactivates a persistent constraint while the surface is focused (e.g. user toggles overlay or hits Escape), `unlocked` is sent. When the pointer subsequently leaves the surface, `leave()` calls `deactivate()` again, sending a second, duplicate `unlocked` event without an intervening `locked`.
   - **Never-Activated Constraint Followed by Leave:** If a client creates a constraint but compositor policy denies or defers activation (e.g. awaiting user gesture), `leave()` sends `unlocked` for a lock that was never locked.
   - **Oneshot Lock Premature Destruction:** A never-activated oneshot lock is destroyed and removed on `leave()`, emitting an invalid `unlocked`.

### 1.3 Call-Site Exhaustion Search

An exhaustive search across the entire Smithay 0.7.0 codebase for calls to `deactivate()`:
```bash
grep -rn "\.deactivate(" smithay-0.7.0/src/
```
Yielded only two matches:
1. `src/wayland/input_method/input_method_handle.rs:153`: `instance.object.deactivate();` (unrelated input method).
2. `src/wayland/seat/pointer.rs:253`: `constraint.deactivate();`

There is **exactly one** invocation of pointer constraint deactivation in the entire Smithay library.

---

## 2. Independent Evaluation of Smithay Patch P1

Patch P1 replaces lines 251–255 of `src/wayland/seat/pointer.rs`:
```rust
with_pointer_constraint(self, &pointer, |constraint| {
    if let Some(constraint) = constraint {
        if constraint.is_active() {
            constraint.deactivate();
        }
    }
});
```

### 2.1 Adversarial State-Transition Analysis

| State / Transition | Behavior Under Stock Smithay | Behavior Under P1 | Protocol Correctness |
|---|---|---|---|
| **Active Persistent Lock $\to$ Leave** | Sends `unlocked`, sets `active=false` | Sends `unlocked`, sets `active=false` | **VALID** (paired) |
| **Inactive Persistent Lock $\to$ Leave** | Sends duplicate `unlocked` (**INVALID**) | No-op (`is_active` is false) | **VALID** (no duplicate) |
| **Active Oneshot Lock $\to$ Leave** | Sends `unlocked`, removes entry | Sends `unlocked`, removes entry | **VALID** (paired & cleared) |
| **Never-Active Oneshot $\to$ Leave** | Sends `unlocked`, removes entry (**INVALID**) | No-op, entry preserved | **VALID** (no bogus event) |
| **Active Confinement $\to$ Leave** | Sends `unconfined`, sets `active=false` | Sends `unconfined`, sets `active=false` | **VALID** (paired) |
| **Inactive Confinement $\to$ Leave** | Sends duplicate `unconfined` (**INVALID**) | No-op (`is_active` is false) | **VALID** (no duplicate) |
| **Focused Surface Destroyed** | Double `unlocked` if deactivated | Cleans up without duplicate events | **VALID** |
| **Rapid Focus Churn (Enter/Leave)** | Duplicate events if not re-activated | Strict 1:1 enter/activate, leave/deactivate | **VALID** |
| **Zero-Output Transition** | Sends duplicate events under implicit grab | Clears active constraint exactly once | **VALID** |

### 2.2 Execution of Multi-Batch Reproducer (`xlock_batch`)

The binary `xlock_batch` was executed against both stock Smithay 0.7.0 and Smithay + P1:

```
STOCK SMITHAY 0.7.0:
  SUMMARY: 2/11 cases VALID; INVALID: ["E1", "E2", "E4", "E5", "E7", "E8", "E9", "E10", "E11"]

SMITHAY 0.7.0 + P1:
  SUMMARY: 11/11 cases VALID; INVALID: []
```

### 2.3 Dependency Governance and OB-11 Placement

The architecture handles the P1 dependency cleanly:
1. `OB-11` is open with destination `UP`.
2. `OB-11` explicitly blocks `INC-5` (pointer constraints implementation) and release.
3. `INC-0` through `INC-4` do not touch pointer constraints and have no dependency on P1 or upstream PR merge status.
4. Foundation 01 establishes a clear patch-policy boundary (`D-B12-PIN`) that isolates external framework flaws.

---

## 3. Zero-Output Behavior Audit (`xzero`)

The zero-output lifecycle represents one of the most failure-prone transitions in Wayland compositors:
$$\text{Output 1} \longrightarrow \text{Outputs} = \emptyset \longrightarrow \text{Output 2}$$

### 3.1 Mechanism Under E1R

1. **Detection:** When the last output disconnects, the survivor set becomes $\emptyset$. `INV-OUTPUT-1` mandates entering `PointerState::NoOutput`.
2. **Implicit Grab Termination:** An implicit grab caused by a held client button would keep focus pinned to a vanished surface. The adapter installs `ZeroOutputGrab` with `Focus::Clear`.
3. **Focus Clearing & Constraint Deactivation:** `Focus::Clear` triggers `pointer.rs:leave()`, which sends `wl_pointer.leave` to the client. Under P1, any active constraint is deactivated exactly once.
4. **Motion Suppression:** While in `PointerState::NoOutput`, physical motions and buttons are consumed by core and routed nowhere; no relative or absolute motion leaks to clients.
5. **Reconnection & Startup Placement:** When a new output connects, `ZeroOutputGrab` is removed and the pointer is placed at the center of the new output's logical extent, generating one clean `wl_pointer.enter`.

### 3.2 Execution of `xzero`

Execution of `xzero` under both stock and P1 verified:
- Negative Control N1 (sealed `F-RELOCATE-1` with $\emptyset$ survivors): reproduces panic as predicted.
- Negative Control N2 (removal without grab handling): reproduces invalid focus retention on vanished surface.
- Negative Control N3 (`unset_grab` under lock): reproduces illegal absolute motion delivery to a locked surface.
- E1R rule: **PASS** across all constraint types (`None`, `PersistentLock`, `OneshotLock`, `Confine`) and button states.
