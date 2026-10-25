# INC-5 Implementation Mapping & Authority Model (Planning Record)

Written before any INC-5 production change.
Baseline: `2680b99eb6d5353473ff362f5b4a904372f1fdfb` (Phase F INC-0/1/2/3/4 RATIFIED).

Canonical Category: `PRODUCT`
Execution Class: `HEADLESS`
Title: `Pointer constraints, reconciliation, output removal under interaction`

Sources:
- `FOUNDATION_IMPLEMENTATION_PLAN.md` § INC-5;
- `FOUNDATION_STATE_MACHINES.md` M13 (Pointer constraint), M14 (Confinement clamp), M5 (Multi-output lifecycle), M11 (ActiveElementFocus), M22 (Compositor interaction);
- `FOUNDATION_INVARIANTS.md` INV-INPUT-5, INV-COORD-5, INV-OUTPUT-1, INV-INPUT-3, INV-INPUT-8, INV-INPUT-10, INV-ARCH-3;
- `FOUNDATION_OBJECT_MODEL.md` OBJ-21 (ConstraintState), OBJ-20 (PointerState), OBJ-22 (ActiveElementFocus), OBJ-24 (Client-owned grab), OBJ-23 (ProtocolSurfaceRoute);
- `FOUNDATION_ARCHITECTURE.md` § F-LOCK-1, F-CONFINE-1, F-RELOCATE-1, F-RECONF-1;
- `FOUNDATION_BLOCKERS.md` W-SM-4 (4a, 4b, 4c), W-SM-16, D-B12-PIN, PD-F2-03, PD-F2-04, PD-F2-05;
- `FOUNDATION_TEST_PLAN.md` OB-31, OB-3, OB-47, OB-50, OB-51, OB-11, OB-20 (T0/T1 protocol parts);
- Harness reference implementations: `experiments/b8-harness/src/bin/xlock_grab.rs` (TST-21), `experiments/b8-harness/src/bin/xlock_batch.rs` (EE-2), `experiments/b8-harness/src/bin/xzero.rs` (EE-3), `experiments/e1r-smithay-p1` (OB-11 P1 patch candidate).

---

## 1. Authority Model (Non-Negotiable)

| State / Datum | Authoritative Owner | Readers | Mutation Port / Trigger | Forbidden Writers |
|---|---|---|---|---|
| **Physical Pointer Position** | Core `PointerModel` (`OBJ-20`) | Core hit-test, constraint policy, adapter (via effects) | `InputPort` (`PointerMotion`), `OutputLifecycleToken` (`connect`, `reconfig`, `disconnect`) | Smithay (applied copy), cameras, clients |
| **Interaction Output** | Core `PointerState::Located { output, .. }` (`OBJ-20`) | Core camera lookup, hit-test, constraint policy, adapter | `InputPort` / `OutputLifecycleToken` | Smithay, cameras, clients |
| **ActiveElementFocus** | Core `FocusModel` (`OBJ-22`) | Core constraint guard, adapter (`KeyboardFocus`, `ToplevelConfigure`), shell | Explicit action (`set_focus`, `focus_next`, target loss successor) | Cameras, visibility, pointer motion, clients |
| **Delivered Pointer Focus** | Smithay `PointerHandle.current_focus()` | Adapter, Core (read during batch reconciliation) | Applied by Adapter via `ptr.motion` or client grab / `ZeroOutputGrab` | Core directly (Core computes `PointerTarget`) |
| **Constraint Requests & Lifetime** | Smithay `PointerConstraintsState` & Wayland protocol objects | Adapter `PointerConstraintsHandler` | Client Wayland protocol requests (`lock_pointer`, `confine_pointer`, `destroy`) | Core (Core has no protocol resource handles) |
| **Active Constraint State (`OBJ-21`)** | Core `PointerModel` (`Option<ConstraintState>`) | Core intake, routing, batch reconciliation, adapter | Core batch-end reconciliation pass (`reconcile_constraints`) | Smithay, cameras, clients |
| **Client-Owned Implicit Grab (`OBJ-24`)** | Smithay seat pointer grab | Core input state (`client_owns_any()`), adapter | Client-owned button press and release | Core (cannot forge client grabs) |
| **Compositor Pointer Interaction** | Core `Interaction` (`OBJ-26`, `CameraPan`) | Core intake, reconciliation (causes deferral), adapter | Compositor-owned button press (`InputPort`), pan motion, release | Smithay, clients |
| **Protocol Activation / Deactivation Events** | Adapter `infynitae-wayland` | Wayland client | Adapter applying Core effects (`activate()`, `deactivate()`) | Backend, Core directly |
| **Smithay Constraint Objects** | Smithay `PointerConstraint` | Adapter | Adapter via `with_pointer_constraint` (`activate()`, `deactivate()`) | Core (Core forbids unsafe and Smithay dependency) |
| **Relative Pointer Delivery** | Adapter `zwp_relative_pointer_v1` | Wayland client | Physical deltas routed by Core `routed_motion` (`PointerRelative`) | Synthetic motion, camera pan, output relocation |
| **Output Lifecycle** | Core `OutputModel` & `OutputLifecycleToken` (`OBJ-01`, `OBJ-02`) | Core, Adapter, Wayland clients | `Core::output_{connected, reconfigured, disconnected}` gated by token | Backend, Wayland clients, Smithay |

---

## 2. OBJ-21 — Authoritative Constraint Representation

Core `PointerModel` represents the constraint as:
```rust
pub struct ConstraintState {
    pub surface: SurfaceKey,
    pub kind: ConstraintKind,
    pub lock_point: LocalPoint,
}

pub enum ConstraintKind {
    Lock,
    Confine,
}
```
- **Constraint never carries an output** (`INV-COORD-5`).
- **Constraint never stores a Canvas lock point** (`INV-COORD-5`).
- Canvas lock point is strictly derived per [`F-LOCK-1`]:
  $$\mathbf{q}_{\text{lock}} = \text{surface\_origin}(S) + \text{lock\_point}$$
  where `surface_origin` follows the current authoritative placement of surface $S$.

---

## 3. State Machine M13 / M14 Matrix

| Current State | Trigger / Event | Guard | Target State | Core Action & Effects | Cites |
|---|---|---|---|---|---|
| **Requested** | Batch end | `ActiveElementFocus == element(S)` $\land$ pointer focus on $S$ $\land$ pointer inside region $\land$ $\neg\text{interaction.active()}$ | **Active** | Emit `Effect::ConstraintActivate { surface, kind }` | `INV-INPUT-5` |
| **Active** | Compositor interaction begins (`CameraPan`) | — | **Active (Deferred)** | Defer reconciliation; relative motion from physical deltas only; suppress absolute motion to locked surface | `INV-INPUT-5`, `INV-INPUT-3` |
| **Active** | Client-owned grab begins (`OBJ-24`, button press) | — | **Active (Deferred)** | Defer reconciliation; core and Smithay maintain identical state | `INV-INPUT-5`, `TA-4` |
| **Active (Deferred)** | Compositor interaction ends OR client grab ends | Lock point observable through interaction output $\land$ `ActiveElementFocus == element(S)` | **Active** | Reconcile against settled state; resume normal routing | `INV-INPUT-5` |
| **Active (Deferred)** | Compositor interaction ends OR client grab ends | Lock point NOT observable OR focus lost | **Inactive** | Single deactivation via `leave()` or explicit `deactivate()` | `INV-INPUT-5`, `W-SM-4a` |
| **Active (Lock)** | Batch end (view, geometry, origin, or output change) | Lock point observable through interaction output $\land$ `ActiveElementFocus == element(S)` | **Active** | Maintain lock; no motion events to locked surface | `INV-INPUT-5`, `F-LOCK-1` |
| **Active (Lock)** | Batch end | Lock point NOT observable OR `ActiveElementFocus != element(S)` | **Inactive / Defunct** | Emit `Effect::ConstraintDeactivate { surface, kind }` (or rely on `leave()`) | `INV-INPUT-5`, `W-SM-4a` |
| **Active (Confine)** | Pointer motion | Inside region $\cap$ output bounds | **Active** | Physical delta clamped per `F-CONFINE-1`; Arrangement crossing inhibited | `INV-INPUT-5`, `M14`, `F-CONFINE-1` |
| **Active (Confine)** | Batch end | Pointer outside region | **Inactive / Defunct** | Emit `Effect::ConstraintDeactivate { surface, kind }` | `INV-INPUT-5` |
| **Active** | Interaction output disconnected | Surviving outputs exist | **Active (Relocated)** | Relocate physical pointer per `F-RELOCATE-1`; reconcile (deferred if grab active) | `INV-OUTPUT-1`, `F-RELOCATE-1`, `OB-3` |
| **Active** | Last output removed | Survivor set empty | **Inactive (`NoOutput`)** | Seat enters `NoOutput`; install `ZeroOutputGrab(Focus::Clear)`; emit exactly one deactivation; no absolute motion | `INV-OUTPUT-1`, `OB-50` |
| **Inactive (`NoOutput`)** | First output connected | — | **Requested / Reconcile** | Remove `ZeroOutputGrab`; place pointer at center; routed motion; reconcile constraints | `INV-OUTPUT-1`, `OB-50` |

---

## 4. Requirement & Obligation Mapping

| Obligation | Item | Canonical Criterion | Production Path | Negative Control | Blocks |
|---|---|---|---|---|---|
| **OB-31** | `xlock_grab` port to T1 + confinement | Persistent & oneshot locks with DEFER strategy pass; core == Smithay at every step; exactly 1 `unlocked`; confinement clamped | `t1_pointer_constraints.rs` | SEALED (divergence) and EXPLICIT/persistent (duplicate `unlocked`) fail | INC-5 exit |
| **OB-3** | Output removal mid-interaction | Interaction kept until release; pointer relocated via F-RELOCATE-1; constraint reconciled at grab end | `t1_pointer_constraints.rs` | Sealed F-RELOCATE-1 on empty survivor set (panics) | INC-5 exit |
| **OB-47** | Constraint-lifecycle regression | Sequences E1–E11 pass; single deactivation from active state only | `t1_pointer_constraints.rs` | Stock Smithay shows W-SM-4c duplicate deactivation | INC-5 exit |
| **OB-50** | Zero-output constraint lifecycle | One output $\rightarrow$ active constraint $\rightarrow$ last output removed $\rightarrow$ `NoOutput` $\rightarrow$ reconnect; exact wire order; single deactivation | `t1_pointer_constraints.rs` | `N3` (`unset_grab` under lock) fails | INC-5 exit |
| **OB-51** | W-SM-16 popup preemption under lock | Popup preemption while lock active delivers ZERO absolute motion to locked surface | `t1_pointer_constraints.rs` | Synthetic motion on ungrab detected | INC-5 exit |
| **OB-11** | Smithay 0.7.0 defect W-SM-4b/4c | Reproduce on stock Smithay (TST-08 x12); evaluate candidate P1 (`experiments/e1r-smithay-p1`); decision gate before production constraint shipping | Adjudication report / patch under D-B12-PIN | Stock Smithay failure on X-12 | INC-5 decision point / release |
| **OB-20** | Games under KeepFocus pans & locks | T0/T1 protocol parts: TST-05 x8 C3 (pan with lock), C4 (pan with confinement); relative motion delivered; lock kept | `t1_pointer_constraints.rs` | Pan motion leaks to client (probe_pan_motion_bypass) | Release (T5 part stays OPEN) |
