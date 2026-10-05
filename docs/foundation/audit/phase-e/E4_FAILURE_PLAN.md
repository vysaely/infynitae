# Checkpoint E4 — Failure, Performance, Checker & Implementation Plan Audit

**Reviewer:** Independent Principal Systems Architect  
**Baseline:** Phase D Final Seal (`c0a1cee310e56b75d32cf30152a09975ff776700`)

---

## 1. Failure & Recovery Architecture Audit

The failure recovery models across `FOUNDATION_ARCHITECTURE.md §C8`, `FOUNDATION_STATE_MACHINES.md` (M19, M20, M21), and `INV-FAIL-1` were analyzed against adversarial collapse scenarios:

1. **Internal Invariant Violations:** `INV-FAIL-1` specifies termination (panic/abort) over silent best-effort repair of corrupt authoritative state. This prevents insidious authority drift and secondary state corruption.
2. **Shell IPC Crash & Reconnect (M18, INV-SHELL-2):** Shell IPC carries only generation-stamped derived snapshots. Shell restarts do not lose authoritative state; reconnection follows a deterministic `Snapshot + Subscribe` sequence.
3. **Renderer & Context Loss (M21):** On GPU reset or context loss, `M21` halts frame emission, re-creates the renderer, re-imports client buffers, issues full damage, and re-queries `D_max`. If renderer recreation fails, an orderly termination occurs.
4. **Persistence Failure (M20, INV-FAIL-1):** Persistence file corruption is handled all-or-nothing at startup. Corrupt persistence records fall back cleanly to Core's internal defaults without partial application.
5. **Client Death & Protocol Object Destruction:** Client death handling is idempotent and order-independent (`INV-STAGE-4`). Dead surfaces fail closed in `ProjectionFrame` (`PD-F1-07`), and Smithay popup dismissals catch `Err(DeadResource)` without panicking.

---

## 2. Performance Architecture Audit

The complexity budgets and resource contracts in `FOUNDATION_ARCHITECTURE.md §C14` were evaluated:

1. **Idle Contract (`INV-IDLE-1`):** Infynitae generates zero wakeups when idle. Off-screen fallback timers are armed strictly when a client has a pending frame callback (`PD-F1-06`).
2. **Hot-Loop Allocations (`INV-ALLOC-1`):** Core input intake, pan, pointer routing, hit-testing, and render list collection reuse pre-allocated caller-owned buffers (`OBJ-28`). Zero heap allocations occur in steady-state hot paths.
3. **Spatial Index Threshold (`INV-RENDER-4`, `C3.8`):** Linear traversal over window trees is maintained for $N < 100$ elements. A spatial index is explicitly deferred until measured threshold justification, preventing secondary authority desynchronization.
4. **Scale Churn Suppression (`INV-SCALE-3/4`):** Damping via `ScaleInteractionEpoch` limits hint emissions during continuous interactions to rung crossings, one settlement event, and P7 forced decreases.

---

## 3. Documentation Checker Audit (`tools/check-foundation-docs`)

The documentation checker script was analyzed line-by-line and subjected to adversarial mutation probes:

### 3.1 Blind Spot 1: Formula Definition Bypass (Finding E-F3-01)
`tools/check-foundation-docs` validates formulas in §F by checking the line:
`- **Extracted invariant text:** (INV-[A-Z]+-\d+) ⟦(.*)⟧`
and reversing string substitution into `FOUNDATION_INVARIANTS.md`.
**Vulnerability:** The checker **never parses or verifies the actual formula definition** in `- **Definition:** ...`.
An adversarial probe replaced `v_O = Q16(...)` in `F-VIEW-1` with `v_O = 999999999 (CONTRADICTION)`. The checker reported `PASS`, failing to detect the corrupted formula.

### 3.2 Blind Spot 2: ADR Citation Format (Finding E-F1-01)
The checker checks reference tokens matching `REF['INV'] = r'\bINV-[A-Z]+-\d+\b'`.
In `FOUNDATION_DECISIONS.md`, all ADRs cite invariants using bare prefixes (e.g. `STAGE-1, COORD-1`).
**Vulnerability:** The checker completely ignored these citations. An adversarial probe inserting `- **Invariants:** NONEXISTENT-999` into `FOUNDATION_DECISIONS.md` passed without error.

### 3.3 Blind Spot 3: Non-Living Region Exemption
The checker operates exclusively on content delimited by `<!-- living:TAG --> ... <!-- /living -->`. Any contradictory assertions, duplicate definitions, or broken rules placed outside living markers in package files are completely invisible to the checker.

---

## 4. Correlated Evidence Audit

Previous architectural checkpoints (Phase B, C, D) relied heavily on positive experiment logs (`x6`, `xhit`, `d5hit`, `x8`, `x10`, `xsnap`). An adversarial audit of these experiments revealed significant correlated proof failure:

1. **Complete Absence of Output Transform Testing (Rotated Displays):**
   Grep analysis across all test code in `experiments/` revealed that every single test used `Transform::Normal`. Not a single automated test or harness ever initialized an output with `Transform::_90`, `_180`, or `_270`. Consequently, the fatal dimension double-swap in `PD-F1-04` / `F-RENDER-1` went completely undetected by all existing test suites.
2. **Harness Shared Formula Bias:**
   In `xhit.rs`, the test harness defined its own `eligible()` function that mirrored the exact clamping logic proposed for `F-HIT-1`. The test tested the formula against a reimplementation of itself rather than against an independent physical-pixel oracle.

---

## 5. Implementation Plan (D8) Audit

`FOUNDATION_IMPLEMENTATION_PLAN.md` was audited against risk retirement ordering:

1. **Slice 0 Enforcement Gate (`INC-0`):** Properly placed as the entry gate (OB-19). Requires `verify-architecture`, DAG validation, and compile probes before any product code.
2. **Slice 1 Vertical Seam (`INC-1`):** Correctly couples Smithay public snapshot extraction, core projection, DummyRenderer, and physical hit-testing as the minimal end-to-end seam.
3. **Gate Dependencies:**
   - `INC-10` (Zoomed navigation) is properly gated by `GATE G-OB24` (OB-24 decision required first).
   - `INC-14` (Window move/resize) is properly gated by `GATE G-OB34` (cross-output drag policy decision required first).
4. **Execution Classes:** Correctly isolates `HEADLESS`, `NESTED`, and `EXCLUSIVE_HARDWARE` (DRM backend in INC-16 requires explicit authorization).
5. **Omission in INC-2:** `INC-2` introduces multi-output lifecycle, but its exit tests omit verification of rotated outputs (`Transform::_90`), leaving the transform double-swap hazard unmitigated.
