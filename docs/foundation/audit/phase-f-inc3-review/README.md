# Phase F Increment 3 (INC-3) — Independent Adversarial Systems Architecture Review Dossier

- **Review Target:** `~/infynitae-native`
- **Reviewed HEAD:** `0c4e961f9d85c62dec9915f2ab0c9141afeeb897`
- **Pre-INC-3 Baseline:** `6470e1a5e22065baa940d4f5b5dd0bd2b1e281a7` (Ratified INC-2)
- **Role:** Independent Principal Systems Architect, Senior Rust Systems Engineer, Wayland/Smithay Compositor Engineer, Input-Stack Engineer, and Adversarial Verification Reviewer.
- **Review Category:** PRODUCT, HEADLESS.
- **Review State:** Complete, untracked review artifacts. Production implementation untouched.

---

## Dossier Structure

1. [`VERDICT.md`](file:///home/vysaely/infynitae-native/docs/foundation/audit/phase-f-inc3-review/VERDICT.md) — Definitive review verdict, executive summary, findings catalog, and exact recommended corrective/ratification actions.
2. [`RECONSTRUCTION.md`](file:///home/vysaely/infynitae-native/docs/foundation/audit/phase-f-inc3-review/RECONSTRUCTION.md) — Independent canonical reconstruction of INC-3 scope, invariants, formulas, objects, state machines, and complete diff audit (`6470e1a..0c4e961`).
3. [`INPUT_AUTHORITY.md`](file:///home/vysaely/infynitae-native/docs/foundation/audit/phase-f-inc3-review/INPUT_AUTHORITY.md) — Audit of input authority encapsulation, `InputPort` uncloneable token, `Authorized` proof passing, and architecture source-rule verifications.
4. [`POINTER_AND_FOCUS.md`](file:///home/vysaely/infynitae-native/docs/foundation/audit/phase-f-inc3-review/POINTER_AND_FOCUS.md) — Audit of pointer coordinate pipeline (Physical -> Output -> Canvas), `ZeroOutputGrab` lifecycle, output crossing, and hit-testing against `ProjectionFrame`.
5. [`CAMERA_PAN.md`](file:///home/vysaely/infynitae-native/docs/foundation/audit/phase-f-inc3-review/CAMERA_PAN.md) — Audit of `F-PAN-1` direct manipulation math, `M22` interaction state machine, button ownership table (`OBJ-25`), `KeepFocus` semantics, and Stage immutability.
6. [`ADVERSARIAL_ATTACKS.md`](file:///home/vysaely/infynitae-native/docs/foundation/audit/phase-f-inc3-review/ADVERSARIAL_ATTACKS.md) — Execution and results of 20 independent adversarial scratch mutations (A through T), test oracle independence audit, and discovery of test-suite defect `INC3-F09`.
7. [`OBLIGATION_REVIEW.md`](file:///home/vysaely/infynitae-native/docs/foundation/audit/phase-f-inc3-review/OBLIGATION_REVIEW.md) — Adjudication of architectural obligations `OB-45`, `OB-50`, `OB-3`, `OB-21`, `OB-4`, and tracking of all remaining open obligations.

---

## Summary of Findings

- **Level A (Architectural / Safety Violation):** 0
- **Level B (Implementation / Test-Suite Defect):** 1
  - `INC3-F09`: File descriptor leak in `t1_transform` (`ee1_c5` / `n_pan_moves_render_and_hit_together`) exhausts file descriptors under standard Linux `ulimit -n 1024`, causing `cargo test --workspace --locked` to panic with `TooManyOpenFiles`.
- **Level C (Specification / Documentation Scoping Contradiction):** 1
  - `INC3-F08`: Popup anchor-output rule 1 listed in INC-3 scope belongs to INC-4 window management / popup grabs (`OB-4`).
- **Observations / Scaffolding Ratifications:** 5
  - `INC3-F01`: Modifier snapshot parameter in headless backend is legitimate scaffolding for INC-3 headless testing; dynamic XKB state integrates in INC-4.
  - `INC3-F02`: Dropping `ProjectionFrame` on camera pan is correct and directly extends `INC2-F07`.
  - `INC3-F03`: Headless `vblank()` producing frames with `DummyRenderer` honors `INV-HIT-1`.
  - `INC3-F06`: `tools/verify-test-efficacy` uses a single shared `target/test-efficacy` directory, susceptible to concurrent build races.
  - `INC3-F07`: `F-PAN-1` direct manipulation preserves exact 1:1 screen-to-canvas dragging without slip across fractional scales.

---

## Definitive Verdict

```
INC-3 CONDITIONAL PASS — CORRECTIONS REQUIRED BEFORE RATIFICATION
```
