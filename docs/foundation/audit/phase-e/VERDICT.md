# Phase E — Final Contradiction Pass & Formal Verdict

**Reviewer:** Independent Principal Systems Architect  
**Baseline:** Phase D Final Seal (`c0a1cee310e56b75d32cf30152a09975ff776700`)

---

## 1. Final Contradiction Pass (15 Stress Scenarios)

The architecture was evaluated against the 15 mandatory stress scenarios:

1. **Output hotplug + persisted camera + pointer lock:**
   Hotplug enters via `output_connected(&mut OutputLifecycleToken)`. Persisted lookup seeds camera without public restore API. Pointer lock is evaluated at batch-end reconciliation. If lock point is visible on connecting output, lock is maintained. **Status: PASS.**
2. **Popup grab + focus change + surface destruction:**
   Client destroys popup while focus changes. Smithay's `ungrab(All)` dismisses topmost first, catching `Err(DeadResource)` safely. `set_focus` executes cleanly. **Status: PASS.**
3. **Fractional scale + camera zoom + nested subsurface + input region:**
   Subsurface origins accumulate in Canvas space (`F-ORIGIN-1`). Render rects use `F-LATTICE-1`. Hit test uses identical projection. Boundary clamping ensures valid local coordinates. **Status: PASS.**
4. **Fullscreen game + mixed DPI + scale epoch:**
   Game in fullscreen on 1080p output observed by 4K output with zoom 2.0. Scale policy has no fullscreen override (`INV-SCALE-1`), resulting in 8K buffer allocation request. **Status: HAZARD (Finding E-F2-07, Level C).**
5. **Held button + output removal + confinement:**
   Implicit click grab defers constraint reconciliation (`INV-INPUT-5`). Pointer relocated to surviving output. On button release, confinement ends with single `unconfined`. **Status: PASS.**
6. **Viewport fit + client self-resize + pan:**
   Viewport fit is one-shot (`INV-CAMERA-5`); client resize commits without moving camera (`M8`). Panning from fitted view operates at zoom $\ne 1$. Jitter residual bounded by `OB-24`. **Status: PASS (gated by OB-24).**
7. **Shell restart during navigation:**
   Shell holds no authoritative state (`INV-SHELL-2`). Reconnection uses `Snapshot + Subscribe`. Active epochs complete or time out cleanly. **Status: PASS.**
8. **Satellite restart + existing X11 windows:**
   Satellite crash terminates X11 elements through client death (`INV-XWAYLAND-1`). Restart on same display with `-listenfd` gated under `OB-33`. **Status: PASS.**
9. **Output scale change during an interaction epoch:**
   Output scale change triggers settlement event (`INV-SCALE-3`), closing epoch and settling $h$. Subsequent motion continues without hint spam. **Status: PASS.**
10. **Last-output removal while a constrained client is focused:**
    Surviving output set is empty ($\emptyset$). `F-RELOCATE-1` attempts nearest-output selection and panics. `OBJ-20` cannot represent pointer state. **Status: FAIL (Finding E-F2-05, Level B).**
11. **Camera movement + relative pointer + client-held grab:**
    Camera pan consumes motion deltas (`INV-INPUT-3`). No synthetic or camera-derived relative motion is delivered (`INV-INPUT-4`). **Status: PASS.**
12. **Reconnect same connector with different monitor characteristics:**
    `OutputKey` includes make/model/serial. Different monitor gets new camera or persisted lookup. Zero-serial monitors hazard tracked under `E-F3-03`. **Status: PASS.**
13. **Malformed persistence + startup hotplug:**
    Persistence validated all-or-nothing at startup (`INV-FAIL-1`). Corrupt file falls back cleanly to internal defaults. **Status: PASS.**
14. **Client destruction while presentation/frame state is pending:**
    Wayland resource cleanup handles dead callbacks. Latest pending commits discarded on destruction (`INV-FRAME-5`). **Status: PASS.**
15. **Maximum coordinate magnitude + fractional zoom + multi-output projection:**
    Canvas bounded at $|v| \le 2^{36}$. Multiplied by zoom/scale in `f64`, intermediate values $|v \cdot k| \approx 4 \times 10^{11} < 2^{53}$ remain exact integers in floating point. Off-screen geometry culled before integer conversion (`PD-F1-01`). **Status: PASS.**

---

## 2. Gate Evaluation

Per Section 26 of the Phase E Review Instructions:
- **PASS Requirements:**
  - [x] No unresolved contradiction that invalidates a canonical invariant (Except E-F2-04 Level B)
  - [x] Implementation plan does not cross an unresolved architecture gate
  - [x] Review coverage complete
  - [ ] **NO UNRESOLVED E-F2 LEVEL A FINDINGS** — **FAILED** (2 unresolved Level A findings: `E-F2-01`, `E-F2-02`)
  - [ ] **NO UNGUARDED AUTHORITY PATH / IMPOSSIBLE WAYLAND TRANSITION** — **FAILED** (`E-F2-02` causes impossible duplicate `unlocked` wire events)

---

## 3. Formal Verdict

```
================================================================================
PHASE E FAIL — ARCHITECTURE CORRECTIONS REQUIRED
================================================================================
```

### Mandatory Corrections Required Prior to Re-Review

1. **Correction for Finding E-F2-01 (Level A — Rotated Output Dimension Double-Swap):**
   - Disentangle physical hardware mode from transformed logical view dimensions.
   - Amend `F-VIEW-1` and `F-RENDER-1` so that `OutputDamageTracker::new` receives the unrotated hardware buffer dimensions ($1920 \times 1080$), allowing Smithay's internal `transform_size` and GLES `glViewport` to operate correctly.
   - Add rotated output configurations (`Transform::_90`, `_180`, `_270`) to the test harness and `INC-2` exit tests.
2. **Correction for Finding E-F2-02 (Level A — Duplicate `unlocked` Protocol Violation):**
   - Correct `INV-INPUT-5` and `M13` regarding persistent constraint deactivation.
   - When a persistent constraint is explicitly deactivated while retaining pointer focus, it must be removed from Smithay's `data.constraints` table or prevented from emitting a duplicate `unlocked` notification when `pointer.leave()` is subsequently executed.
3. **Recommended Secondary Corrections (Level B):**
   - Add `output_reconfigured(&mut OutputLifecycleToken, OutputId, OutputDescriptor)` to `INV-OUTPUT-2` and `Core` (Finding `E-F2-03`).
   - Reconcile `INV-CAMERA-3` "N outputs may share one camera" with the 1:1 camera-to-output authority model (Finding `E-F2-04`).
   - Guard `F-RELOCATE-1` and `OBJ-20` against empty surviving output sets upon total display disconnect (Finding `E-F2-05`).
   - Add zero-size guard to `F-SCALE-2` to prevent division-by-zero panics on unbuffered initial commits (Finding `E-F2-06`).
