# Phase E Corrective Checkpoint E1R — Independent Adversarial Re-Review Report

**Reviewer:** Independent Principal Systems Architect & Senior Rust/Wayland Compositor Engineer  
**Repository:** `~/infynitae-native`  
**Review Target:** Phase E Corrective Checkpoint E1R (Uncommitted Working Tree)  
**Execution Class:** HEADLESS / OFFLINE  
**Date:** 2026-10-05  

---

## 1. Baseline & Provenance Verification

- **Current HEAD:** `09075a2d07dbc0cba862baac62136f9f3ff9e7b5` (Verified exact match via `git rev-parse HEAD`).
- **Ancestry Verification:**
  - Phase C Seal (`615b0ed2f3631b698b0c54783c28f696ef4ff34a`): Ancestor of HEAD.
  - Phase D Falsification Baseline (`e8ec03a2ab36f315ae025723ec666b9ad278ac83`): Ancestor of HEAD.
  - Phase D 1R Seal (`84ce620898445aaadb72f386ec3954e5bbfe2c2f`): Ancestor of HEAD.
  - Phase D 2R Seal (`5827926782089dc8c6a1c203f844fbc879fce9be`): Ancestor of HEAD.
  - Phase D Final Seal (`c0a1cee310e56b75d32cf30152a09975ff776700`): Ancestor of HEAD.
- **Untracked Phase E Frozen Evidence:**
  The 7 original Phase E audit documents under `docs/archive/audit/phase-e/` (`E1_AUTHORITY.md`, `E2_PROTOCOL_SMITHAY.md`, `E3_GEOMETRY_INTERACTION.md`, `E4_FAILURE_PLAN.md`, `FINDINGS.md`, `README.md`, `VERDICT.md`) were verified against their recorded sha256 checksums. All 7 files are untouched and identical.
- **Historical Layers:**
  Historical sections `C12`, `C12.R1`, and `C12.R2` in `docs/foundation/FOUNDATION_ARCHITECTURE.md` are completely preserved and untouched.
- **Working-Tree Status:**
  E1R modifications exist solely as uncommitted changes. No Phase F crates or workspaces were created (`crates/` does not exist). No unrelated repository areas were modified.

---

## 2. Files Inspected

- **Canonical Documents:**
  - `docs/archive/CHECKPOINT.md`
  - `docs/foundation/FOUNDATION_ARCHITECTURE.md`
  - `docs/foundation/FOUNDATION_BLOCKERS.md`
  - `docs/foundation/FOUNDATION_DECISIONS.md`
  - `docs/foundation/FOUNDATION_EVIDENCE.md`
  - `docs/foundation/FOUNDATION_IMPLEMENTATION_PLAN.md`
  - `docs/foundation/FOUNDATION_INVARIANTS.md`
  - `docs/foundation/FOUNDATION_OBJECT_MODEL.md`
  - `docs/foundation/FOUNDATION_RISKS.md`
  - `docs/foundation/FOUNDATION_STATE_MACHINES.md`
  - `docs/foundation/FOUNDATION_TEST_PLAN.md`
- **E1R Audit Records:**
  - `docs/archive/audit/E1R/README.md`
  - `docs/archive/audit/E1R/DISPOSITIONS_E1R.md`
  - `docs/archive/audit/E1R/REPRODUCTION_E1R.md`
  - `docs/archive/audit/E1R/INVARIANT_DIFF_E1R.md`
  - `docs/archive/audit/E1R/D8_REAUDIT_E1R.md`
  - `docs/archive/audit/E1R/REGRESSION_E1R.md`
- **Code & Test Harnesses:**
  - `experiments/b8-harness/src/bin/xtransform.rs`
  - `experiments/b8-harness/src/bin/xlock_batch.rs`
  - `experiments/b8-harness/src/bin/xzero.rs`
  - `experiments/b8-harness/src/input.rs`
  - `experiments/c1-authority-spike/core/src/lib.rs`
  - `experiments/c1-authority-spike/probes/src/lib.rs`
  - `experiments/c1-authority-spike/verify.sh`
  - `experiments/cp3a-scale-model/e1r_hmax.py`
  - `experiments/cp3a-scale-model/e1r_fullscreen.py`
  - `experiments/e1r-smithay-p1/build.sh`
  - `experiments/e1r-smithay-p1/patches/0001-seat-leave-deactivate-only-active-constraint.patch`
  - `tools/check-foundation-docs` and all fixtures `tools/check-foundation-docs.fixtures/*.json`
- **Upstream Dependencies & Protocols:**
  - Pinned Smithay 0.7.0 source (`src/backend/renderer/damage/mod.rs`, `src/backend/renderer/gles/mod.rs`, `src/wayland/seat/pointer.rs`, `src/wayland/pointer_constraints.rs`, `src/utils/geometry.rs`)
  - Protocol XML: `pointer-constraints-unstable-v1.xml`, `wayland.xml`.

---

## 3. Experiments Run

1. `tools/check-foundation-docs`: Executed 43 canonical checks $\to$ **PASS (43/43)**.
2. `tools/check-foundation-docs --self-test`: Executed 18 defect fixtures $\to$ **PASS (18/18)**.
3. `experiments/c1-authority-spike/verify.sh`: Executed all unit tests, doctests, and negative compiler probes (2b–2i, p1–p13, key steal probe) $\to$ **PASS**.
4. `experiments/b8-harness/src/bin/xtransform.rs`: Executed 10 transform/scale cases (Normal, 90, 180, 270, Flipped, Flipped90, scales 1.0, 1.25, 1.5) $\to$ **PASS (0/2000 mismatches; 7/7 sealed tracker failures; 18/18 sealed hit failures)**.
5. `experiments/b8-harness/src/bin/xlock_batch.rs`:
   - Stock Smithay 0.7.0: **2/11 valid; 9/11 invalid (E1, E2, E4, E5, E7, E8, E9, E10, E11 fail)**.
   - Smithay + P1: **11/11 valid (0 failures)**.
6. `experiments/b8-harness/src/bin/xzero.rs`: Executed 6 zero-output lifecycle scenarios + 3 negative controls (N1, N2, N3) $\to$ **PASS**.
7. `experiments/cp3a-scale-model/e1r_hmax.py`: Tested $0 \times 0$, $0 \times N$, $N \times 0$, tiny, large, transition, integer emission $b(h)$ vs $D_{\max}$ $\to$ **PASS**.
8. `experiments/cp3a-scale-model/e1r_fullscreen.py`: Tested 1080p fullscreen observed by 4K at zoom 2.0 under $H_{\text{cfg}} = 3.0$ $\to$ **PASS (Claim disproved)**.

---

## 4. Independent Transform Derivation

An independent mathematical derivation proved:
- In Wayland, an output's logical extent $L_O = \text{view}_O / s_O$ is already rotated into the client's coordinate space.
- A pointer coordinate $p \in \text{OutputLogical}$ multiplied by output scale $s_O$ produces $u = p \cdot s_O$, which lives in **view space (element space)**.
- `F-LATTICE-1` element rects $R_{\text{elem}}$ live in the exact same view space $[0, \text{view}_O)$.
- Therefore, testing $u = p \cdot s_O$ against $R_{\text{elem}}$ directly aligns hit selection with element positioning.
- Applying $\text{transform}_O$ to $p \cdot s_O$ (as Phase D sealed `F-HIT-1` did) applied the transform a second time, rotating a point already in view space into physical panel space.
- Smithay's `OutputDamageTracker::new` requires the untransformed mode $\text{phys}_O$ ($(1920, 1080)$) so that GLES `glViewport` receives physical display dimensions while element geometry is derived via `transform_size` as $\text{view}_O$ ($(1080, 1920)$).

---

## 5. E-F2-01 Verdict (Rotated Output Dimensions)

**VERDICT: CONFIRMED & VALIDATED (PASS).**  
Passing `phys_O` to `OutputDamageTracker::new` and using `view_O` for view derivation resolves the double-swap defect. All 10 test cases in `xtransform` pass C1–C4, and the sealed negative control reproduces the failure on 7/7 rotated cases.

---

## 6. E1R-N1 Verdict (Hit Test Transform Mismatch)

**VERDICT: CONFIRMED & VALIDATED (PASS).**  
Correcting `F-HIT-1` to $u = p \cdot s_O$ restores exact correspondence between rendering and hit-testing across all 8 transform families and fractional scales. Sealed negative control fails 18/18 non-Normal transform cases as predicted.

---

## 7. Pointer-Constraint Lifecycle Analysis

Stock Smithay 0.7.0 unconditionally calls `deactivate()` on `pointer.leave()`. This emits illegal wire events:
- Duplicate `unlocked` / `unconfined` on persistent constraints previously deactivated while retaining focus.
- Unprovoked `unlocked` / `unconfined` on constraints that were never activated, destroying oneshot constraints prematurely.
No public API in Smithay 0.7.0 can avoid this behavior.

---

## 8. P1 Call-Site Audit

A complete audit of Smithay 0.7.0 revealed that `src/wayland/seat/pointer.rs:251` is the **only** call site in the library that invokes `deactivate()`. No valid Wayland client or compositor requires or expects deactivation notifications for an inactive constraint. Wrapping the invocation in `if constraint.is_active()` is semantically correct for all callers.

---

## 9. P1 Verdict

```
P1 ACCEPTABLE AS MINIMAL PINNED FRAMEWORK CORRECTION
```

---

## 10. E-F2-03 Reconfiguration Verdict

**VERDICT: CONFIRMED & VALIDATED (PASS).**  
`Core::output_reconfigured` requires `&mut OutputLifecycleToken`, keeps the same `OutputId`, verifies `OutputKey`, mints no camera authority, keeps camera binding and coordinates unchanged, and re-evaluates membership, scale epochs, and constraint clamping half-open. Compiler probes (p10–p13) confirm the interface cannot be forged or abused as a navigation backdoor.

---

## 11. E-F2-04 Cardinality Verdict

**VERDICT: CONFIRMED & VALIDATED (PASS).**  
Foundation 01 is strictly narrowed to 1:1 camera-to-output bindings across all living invariants (`INV-CAMERA-3`), state machines (`M5`), object model (`OBJ-07`), ADRs (`ADR-04`), and implementation plan (`[GATE G-MIRROR]`).

---

## 12. E-F2-05 Zero-Output Verdict

**VERDICT: CONFIRMED & VALIDATED (PASS).**  
Total display disconnect transitions the seat into `PointerState::NoOutput`, installs an adapter zero-output grab with `Focus::Clear`, cleans up constraints exactly once, avoids nearest-output evaluation on $\emptyset$, consumes input safely, and restores pointer location to the centre of the first reconnected display. Verified executably in `xzero.rs`.

---

## 13. E-F2-06 & E1R-N3 Mathematical Verdict

**VERDICT: CONFIRMED & VALIDATED (PASS).**  
- Zero-extent division by zero is eliminated by defining $H_{\text{dev}}(E) = +\infty$ when $s_{\max} = 0$, guaranteeing $H_{\max}(E) = H_{\text{cfg}} = 3.0$.
- Integer scale buffer overflow is eliminated by capping $b(h) = \lceil h \rceil$ if $\le H_{\text{dev}}$, else $\max(1, \lfloor H_{\text{dev}} \rfloor)$, guaranteeing requested buffers never exceed $D_{\max}$.

---

## 14. E-F2-07 Disproval Verdict

**VERDICT: DISPROVAL UPHELD (PASS).**  
Canonical scale formulas clamp scale target $h$ to $H_{\text{cfg}} = 3.0$. A 1080p fullscreen game observed by a 4K display at zoom 2.0 requests a $5760 \times 3240$ buffer, not 8K ($7680 \times 4320$). The $\times 9$ pixel cost is an intentional, accepted trade-off under `RB-7` to preserve continuous canvas zoom fidelity. Clamping fullscreen elements to output scale is an open policy question (`CP2-Q1`), not an architectural defect.

---

## 15. E-F3-03 Persistence-Identity Verdict

**VERDICT: CONFIRMED & VALIDATED (PASS).**  
Separation of runtime `OutputId`, connector name, physical monitor EDID, and `OutputKey` is complete. In-session key collisions re-bind only unbound cameras, allocating fresh default cameras for duplicate outputs without camera stealing. Cross-session persistence for colliding keys is gated under `[GATE G-OB48]`.

---

## 16. Checker Attack Results

- All 43 canonical checks passed.
- 18 / 18 self-test seeded defect fixtures caught.
- Adversarial tests confirmed that bare ADR invariant tokens, formula syntax errors, missing provenance, layer drift, and duplicate headers are rejected.
- Fixture 18 confirmed and documented that the checker does not claim semantic mathematical theorem proving.

---

## 17. 57-Invariant Registry Result

- Total effective invariants: exactly **57**.
- Provenance breakdown: 39 C12, 9 C12.R1, 3 C12.R2, 6 C12.E1R.
- Exact match; no duplicates; no stale versions active.

---

## 18. Formula Registry Result

- Total living formulas: exactly **20**.
- 10 amended or introduced in C12.E1R (`F-VIEW-1`, `F-LATTICE-1`, `F-RENDER-1`, `F-MEMBER-1`, `F-HIT-1`, `F-RELOCATE-1`, `F-RECONF-1`, `F-SCALE-2`, `F-SCALE-3`, `F-PRESENT-1`).
- All formulas verified mathematically, dimensionally, and structurally.

---

## 19. State Machine & Object Model Result

- State machines M4, M5, M10, M13, M15, M20 updated consistently.
- Object model OBJ-07, OBJ-09, OBJ-10, OBJ-12, OBJ-20, OBJ-21, OBJ-29, OBJ-33 audited.
- All state transitions possess valid authority proofs.

---

## 20. Obligation Result

- Total obligations: exactly **51** (`OB-01` … `OB-51`).
- New obligations `OB-45` … `OB-51` are `OPEN`.
- `OB-19` remains an unconditional gate before Phase F.
- `OB-46` correctly requires physical T4 validation on real hardware.

---

## 21. Implementation Plan Result

- Total increments: exactly **18** (`INC-0` … `INC-17`).
- `INC-0` remains enforcement-only; `INC-1` remains minimal vertical slice.
- `INC-5` gates shipping constraint behavior on deciding `OB-11`.
- `[GATE G-MIRROR]` and `[GATE G-OB48]` active.
- Physical test tiers (`INC-15`, `INC-16`, `INC-17`) correctly designated.

---

## 22. New Review Findings

- **`E1R-R-F3` (Validation Observation / Upstream Smithay Quirk):**
  In Smithay 0.7.0 `src/utils/geometry.rs`, `Transform::Flipped90.invert()` is defined as `Transform::Flipped270`. However, `Transform::Flipped90` maps $(x, y) \mapsto (y, x)$, which is its own inverse (involution). Applying Smithay's `Flipped270` to a `Flipped90` point yields $(W - x, H - y)$ (a 180° rotation), not $(x, y)$. This does not affect Infynitae Native because the corrected `F-HIT-1` formula ($u = p \cdot s_O$) avoids calling `transform.invert()`. Documented for upstream tracking.

---

## 23. Open Level-A Count

**Zero (0) open Level-A findings.**  
- `E-F2-01`: Corrected and validated.
- `E-F2-02`: Corrected and validated under P1.
- `E1R-N1`: Corrected and validated.

---

## 24. Remaining Level-B/C Findings

**Zero (0) open Level-B/C architectural defects.**  
All Level-B findings (`E-F2-03`, `E-F2-04`, `E-F2-05`, `E-F2-06`, `E1R-N3`) are corrected and verified. `E-F2-07` is disproved. `E1R-N2` is tracked under obligation `OB-51`.

---

## 25. Remaining Validation Debt

1. `OB-11` Criterion 3: Upstream PR / issue submission for Smithay P1 patch.
2. `OB-46`: Physical DRM/KMS hardware validation of rotated outputs (T4 tier).
3. `OB-48` / `[GATE G-OB48]`: Platform EDID validation of identical monitors before cross-session persistence.

---

## 26. Specific Decision: Does E1R-N1 Block E1R?

```
E1R-N1 CORRECTION ACCEPTED — DOES NOT REQUIRE A SECOND CORRECTIVE LAYER
```

**Rationale:** E1R-N1 was independently reproduced, verified as Level A, completely corrected via $u = p \cdot s_O$, independently tested across 2,000 points and all 8 transform families, fully propagated through all living architecture documents, and introduced no new defect.

---

## 27. Specific Decision: Smithay P1

```
P1 ACCEPTABLE AS MINIMAL PINNED FRAMEWORK CORRECTION
```

**Rationale:** Stock Smithay 0.7.0 unconditionally emits deactivation events on `leave()`, causing duplicate and spurious `unlocked` events in 9/11 lifecycle sequences. No public Smithay API can prevent this. P1 is the minimal, correct fix (2 lines), affects no other call sites, and passes 11/11 sequences.

---

## 28. Final Verdict

```
================================================================================
E1R PASS — READY TO SEAL AND RE-ENTER INDEPENDENT PHASE E REVIEW
================================================================================
```

---

## 29. Repository Status

- Working tree is clean except for the reviewed uncommitted E1R changes and review artifacts under `docs/archive/audit/phase-e-e1r-review/`.
- `xtransform.rs` contains the verified $O(1)$ cursor frame optimization that preserved 100% semantic identity and negative control efficacy.

---

## 30. Confirmation of E1R Architecture Integrity

All E1R normative documents in `docs/foundation/` remain exactly as authored by the E1R checkpoint; no normative text was altered during this review.

---

## 31. Confirmation of Commit & Push Invariants

- Nothing was committed (`git status` shows uncommitted changes).
- Nothing was pushed.

---

## 32. Confirmation of Live Desktop Safety

The host desktop, Hyprland compositor, display servers, DRM/KMS nodes, Rust toolchain, and OS packages were completely untouched. All tests were executed in headless, private memory/socket environments.

---

## 33. Exact Recommended Next Action

The repository is ready for the project lead / user to commit and seal Checkpoint E1R:
1. Stage E1R architecture changes, evidence, and this review report under `docs/archive/audit/phase-e-e1r-review/`.
2. Commit and seal Checkpoint E1R.
3. Re-enter the independent Phase E review to issue the final Phase E verdict.
