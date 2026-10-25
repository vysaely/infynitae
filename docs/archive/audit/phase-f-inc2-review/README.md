# Phase F Increment 2 (INC-2) — Independent Adversarial Systems Architecture Review Dossier

- **Review Target:** `~/infynitae-native`
- **Reviewed HEAD:** `8f27a3176f0e602659deb9ca83ea8f8aa85a5161`
- **Pre-INC-2 Baseline:** `a25a89073ef8ae5eef4a8ea1702ea8148b52eb4d` (Ratified INC-1)
- **Role:** Independent Principal Systems Architect, Senior Rust Engineer, Wayland/Smithay Compositor Engineer, and Adversarial Verification Reviewer.
- **Review Category:** PRODUCT, HEADLESS.
- **Review State:** Complete, untracked review artifacts. Production implementation untouched.

---

## Dossier Structure

1. [`VERDICT.md`](VERDICT.md) — Definitive review verdict, executive summary, findings catalog, and exact recommended corrective/ratification actions.
2. [`RECONSTRUCTION.md`](RECONSTRUCTION.md) — Independent canonical reconstruction of INC-2 scope, invariants, formulas, objects, state machines, and diff audit (`a25a890..8f27a31`).
3. [`OUTPUT_LIFECYCLE.md`](OUTPUT_LIFECYCLE.md) — Exhaustive audit of output connection, reconfiguration, disconnection, reconnection, zero-output state, identity semantics, and transactional rollback.
4. [`CAMERA_BINDING.md`](CAMERA_BINDING.md) — Audit of `INV-CAMERA-3` (1:1 active binding), M5 precedence, runtime 2i reproduction, and sealed key-steal negative control.
5. [`MEMBERSHIP_AND_FRAMES.md`](MEMBERSHIP_AND_FRAMES.md) — Audit of `F-MEMBER-1`, `wl_surface.enter/leave`, damage tracker sizing (`phys_O` vs `view_O`), EE-1 transform matrix, frame clock selection (`INV-FRAME-2`), and presentation feedback (`INV-FRAME-5`).
6. [`ADVERSARIAL_ATTACKS.md`](ADVERSARIAL_ATTACKS.md) — Execution and results of 15 independent adversarial scratch mutations (A through O), test oracle independence audit, and discovery of test-sensitivity defect `INC2-F07`.
7. [`OBLIGATION_REVIEW.md`](OBLIGATION_REVIEW.md) — Adjudication of architectural obligations `OB-49`, `OB-45`, `OB-50`, `OB-3`, tracking of open obligations, and analysis of the `OB-49` specification contradiction.

---

## Summary of Findings

- **Level A (Architectural / Safety Violation):** 0
- **Level B (Implementation / Test-Sensitivity Defect):** 1
  - `INC2-F07`: Test suite lacks sensitivity to `output_reconfigured` ProjectionFrame invalidation (`entry.rs:120`). Commenting out frame invalidation on reconfiguration fails zero existing workspace tests.
- **Level C (Specification / Documentation Contradiction):** 2
  - `INC1-F01` (from INC-1 review; owned by INC-7): Fullscreen configure response policy.
  - `INC2-F01`: `OB-49` item description contradicts increment boundaries by naming "fullscreen configure" (INC-7) and "epoch settlement" (INC-6) while blocking INC-2 exit.
- **Observations:** 5
  - `INC2-F02`: `D-B6-BOUNDARY` popup anchor output selection uses parent's clock output.
  - `INC2-F03`: Wayland server global numeric name reuse handled by wire test assertions.
  - `INC2-F04`: Immediate output global removal following leaves valid for headless stage.
  - `INC2-F05`: Arrangement rectangles can overlap after reconfiguration; permitted under Level D policy.
  - `INC2-F06`: Damage tracker rebuilding on camera zoom changes in INC-10.

---

## Definitive Verdict

```
INC-2 CONDITIONAL PASS — CORRECTIONS REQUIRED BEFORE RATIFICATION
```
