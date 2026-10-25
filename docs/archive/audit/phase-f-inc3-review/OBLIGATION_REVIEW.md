# Phase F Increment 3 (INC-3) — Architectural Obligation Review

---

## 1. INC-3 Primary Obligations

### A. `OB-45` — Render and Hit-Test Agreement Under Transformations
- **Canonical Requirement:** Selection of a surface at screen coordinate $(x, y)$ must yield the identical surface rendered at $(x, y)$ under all active zoom and pan transforms (`INV-HIT-1`).
- **Implementation Status:** The production implementation in `crates/infynitae-core/src/lib.rs` (`hit_test`) and `crates/infynitae-backend-headless/tests/t1_transform.rs` (`ee1_c5_render_hit_agreement_on_the_production_path`) rigorously verifies 0 mismatches across 10 distinct zoom/pan/subsurface cases.
- **Review Disposition:** **OPEN — BLOCKED BY INC3-F09.** While the mathematical and architectural implementation is correct, the test suite panics under default Linux `ulimit -n 1024` due to `INC3-F09`. Once `INC3-F09` is resolved in a corrective patch, `OB-45` can be formally closed.

### B. `OB-50` — Input Routing Through Transformed Layers
- **Canonical Requirement:** Input event coordinates must be accurately inverted through camera view transforms and element arrangements to reach client local surface spaces.
- **Implementation Status:** Satisfied for pointer input across multiple outputs, transformed cameras, fractional scales, and subsurfaces.
- **Review Disposition:** **OPEN.** Surface constraints (such as lock-to-output or persistent zoom transforms) and constraint reconciliation belong to INC-5.

### C. `OB-3` — Output Disconnect Resource Cleanup
- **Canonical Requirement:** Disconnecting an output must clean up all associated resources, orphan interactions, and safely relocate the pointer.
- **Implementation Status:** `Core::output_removed` correctly removes frames, unbinds cameras, orphans active pan interactions (`C4.6`), and relocates the pointer to the nearest survivor output (`F-RELOCATE-1`) or transitions to `NoOutput`.
- **Review Disposition:** **OPEN.** Dynamic layout constraint reconciliation upon output removal is scheduled for INC-5.

### D. `OB-21` — Input Subsystem Architecture Enforcement
- **Canonical Requirement:** Architecture enforcement must prevent external forgery of input capabilities or direct manipulation of compositor state without tokens.
- **Implementation Status:** `InputPort` capability token, `Authorized` proof for camera panning, and AST rules `effects-only-from-core` and `smithay-pointer-only-in-adapter-pointer` are fully implemented and verified.
- **Review Disposition:** **OPEN.** Physical libinput intake (T4 integration) remains to be implemented in INC-4.

### E. `OB-4` — Popup Dismissal on Unhandled Click
- **Canonical Requirement:** Unhandled clicks outside a popup hierarchy must dismiss active popup grabs.
- **Implementation Status:** Precondition noted in code comments.
- **Review Disposition:** **OPEN.** Popup grabs, popup anchor-output rules, and `ActiveElementFocus` belong to INC-4.

---

## 2. Status of Remaining Obligations

All remaining architectural obligations (`OB-1` through `OB-51`) remain tracked according to their designated canonical increment owners. No premature closure or boundary violation has occurred.
