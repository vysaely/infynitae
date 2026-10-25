# Phase E E1R Adversarial Re-Review — Registry & Traceability Audit

**Reviewer:** Independent Principal Systems Architect & Senior Rust/Wayland Compositor Engineer  
**Date:** 2026-10-05  

---

## 1. 57-Invariant Registry Mechanical Audit

Mechanical parsing of living invariants in `docs/foundation/FOUNDATION_INVARIANTS.md` yields:
- **Total Effective Invariants:** Exactly **57**
- **Layer Distribution:**
  - Effective from `C12`: **39**
  - Effective from `C12.R1`: **9**
  - Effective from `C12.R2`: **3**
  - Effective from `C12.E1R`: **6**
  - Total: $39 + 9 + 3 + 6 = 57$.

### E1R Amended Invariants Traceability Matrix

| Invariant | Triggering Finding | Formula Ref | State Machine | Object Model | ADR Ref | Obligation |
|---|---|---|---|---|---|---|
| `INV-CAMERA-3` | E-F2-01, E-F2-04, E-F3-03 | `F-VIEW-1` | `M4`, `M5` | `OBJ-07`, `OBJ-12` | `ADR-04` | `OB-49` |
| `INV-INPUT-5` | E-F2-02, E-F2-03, E-F2-05 | `F-RELOCATE-1`, `F-CONFINE-1`, `F-LOCK-1` | `M10`, `M13` | `OBJ-20`, `OBJ-21` | `ADR-07`, `ADR-14` | `OB-3`, `OB-31`, `OB-47` |
| `INV-SCALE-2` | E-F2-06 | `F-SCALE-2` | `M15` | `OBJ-29` | `ADR-05` | `OB-13` |
| `INV-SCALE-5` | E1R-N3 | `F-SCALE-3` | `M15` | `OBJ-16` | `ADR-05` | `OB-13` |
| `INV-OUTPUT-1` | E-F2-05 | `F-RELOCATE-1` | `M4`, `M10`, `M13` | `OBJ-20` | `ADR-04` | `OB-3`, `OB-31`, `OB-50` |
| `INV-OUTPUT-2` | E-F2-01, E-F2-03, E-F3-03 | `F-RECONF-1` | `M4`, `M5`, `M20` | `OBJ-10`, `OBJ-09`, `OBJ-33` | `ADR-04`, `ADR-08` | `OB-19`, `OB-49` |

No duplicate effective invariant exists. No superseded version remains effective.

---

## 2. Formula Registry Mechanical Audit

Mechanical parsing of living formulas in `docs/foundation/FOUNDATION_ARCHITECTURE.md` yields:
- **Total Living Formulas:** Exactly **20**
- **Amended or Introduced in E1R:** Exactly **10**
  1. `F-VIEW-1`: Explicit `phys_O` / `view_O` split.
  2. `F-LATTICE-1`: View-space explicit reference.
  3. `F-RENDER-1`: Untransformed physical mode passed to `OutputDamageTracker::new`.
  4. `F-MEMBER-1`: View-space boundary intersection.
  5. `F-HIT-1`: Pointer hit in view space ($u = p \cdot s_O$).
  6. `F-RELOCATE-1`: Guarded domain $S \ne \emptyset$; $S = \emptyset \implies \text{NoOutput}$.
  7. `F-RECONF-1`: Clamping pointer half-open into new logical extent.
  8. `F-SCALE-2`: $H_{\text{dev}} = +\infty$ at zero extent; finite ceiling guaranteed.
  9. `F-SCALE-3`: Integer buffer scale ceiling $b(h) \le H_{\text{dev}}$.
  10. `F-PRESENT-1`: Viewport dimensions in view space.

Every formula definition specifies domain, coordinate units, rounding rules, and provenance.

---

## 3. State Machine & Object Model Audit

- **State Machines Audited:** M4, M5, M10, M13, M15, M20.
  - Zero-output state correctly transitions into `PointerState::NoOutput` with zero-output grab `Focus::Clear`.
  - Reconfiguration maintains camera binding and identity.
  - Constraint deactivation guarded by `is_active()` under P1 (`W-SM-4c`).
  - Concurrent `OutputKey` collisions degrade deterministically to fresh default cameras.
- **Objects Audited:** OBJ-07, OBJ-09, OBJ-10, OBJ-12, OBJ-20, OBJ-21, OBJ-29, OBJ-33.
  - Cardinality 1:1 strictly enforced in `OBJ-07`.
  - `OBJ-20` named `Located | NoOutput`.
  - `OBJ-10` carries untransformed physical mode.

---

## 4. Obligations Audit

- **Total Living Obligations:** Exactly **51** (`OB-01` … `OB-51`).
- All newly introduced obligations (`OB-45` … `OB-51`) are set to `OPEN`.
- `OB-46` (transformed output validation on hardware) is assigned to `T4` (Physical), ensuring headless tests do not close physical validation.
- `OB-19` remains an unconditional gate before Phase F (`PHASE-F-ENTRY-GATE`).
- `OB-11` correctly records criteria 1 & 2 met and criterion 3 (upstream PR/patch decision) open.
- `OB-48` and `[GATE G-OB48]` gate cross-session camera persistence.

---

## 5. Implementation Plan Audit

- **Total Increments:** Exactly **18** (`INC-0` … `INC-17`).
- `INC-0` remains strictly enforcement-only.
- `INC-1` remains the minimal real vertical slice.
- `INC-5` explicitly requires deciding `OB-11` before shipping production constraint behaviour.
- `[GATE G-MIRROR]` strictly excludes shared-camera mirroring from Foundation 01.
- `[GATE G-OB48]` gates cross-session camera persistence.
- Physical testing tiers (`INC-15`, `INC-16`, `INC-17`) are explicitly designated `PHYSICAL` and require separate run authorization.
