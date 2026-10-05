# Phase E Final Review — Invariants & Findings Register

**Review Date:** 2026-10-05  
**Auditor:** Independent Principal Systems Architect & Senior Rust/Wayland Compositor Engineer  

---

## 1. Classification Taxonomy

Findings discovered during Phase E architecture review are classified according to the following taxonomy:

- **`E2-F0`:** Editorial / bookkeeping only (formatting, typos, non-canonical documentation inconsistencies).
- **`E2-F1`:** Non-semantic refinement (clarifications, naming, tooling adjustments without behavioral divergence).
- **`E2-F2`:** Semantic architecture defect:
  - **Level A:** Blocks Phase F (unresolved contradiction, protocol divergence, impossible geometry).
  - **Level B:** Material but isolatable defect (can be safely gated with an explicit obligation before feature entry).
  - **Level C:** Bounded semantic defect (can be safely deferred behind an existing gate).
- **`E2-F3`:** Validation / evidence gap (missing negative controls, lack of executable verification).

---

## 2. Status of Historical Findings

### 2.1 Original Phase E Audit Findings (`docs/foundation/audit/phase-e/FINDINGS.md`)

| Finding ID | Class | Severity | Description | Final Disposition |
|---|---|---|---|---|
| **E-F2-01** | E-F2 | **Level A** | `phys_O` vs `view_O` transform conflation in `OutputDamageTracker::new` | **CONFIRMED & CORRECTED** in E1R (`F-RENDER-1`, `F-VIEW-1`, `F-LATTICE-1`, `F-MEMBER-1`). Verified by `xtransform` (EE-1). |
| **E-F2-02** | E-F2 | **Level A** | Smithay 0.7.0 `leave()` sends duplicate `unlocked`/`unconfined` events | **CONFIRMED & CORRECTED** in E1R via Smithay P1 patch. Gated at `INC-5` by `OB-11`. Verified by `xlock_batch` (EE-2). |
| **E-F2-03** | E-F2 | Level B | Unspecified output reconfiguration lifecycle & missing pointer clamp | **CONFIRMED & CORRECTED** in E1R via `F-RECONF-1` and `output_reconfigured`. Verified by spike probes (EE-4). |
| **E-F2-04** | E-F2 | Level B | Contradiction between 1:1 camera binding and "N outputs may share one camera" | **CONFIRMED & CORRECTED** in E1R: 1:1 binding enforced normatively; mirroring deferred behind `G-MIRROR`. |
| **E-F2-05** | E-F2 | Level B | Undefined nearest-output relocation on empty survivor set $\emptyset$ | **CONFIRMED & CORRECTED** in E1R: `PointerState::NoOutput` handles $\emptyset$; `ZeroOutputGrab` installed. Verified by `xzero` (EE-3). |
| **E-F2-06** | E-F2 | Level B | `ZeroDivisionError` in `F-SCALE-2` when surface extent is $s_{\text{max}} = 0$ | **CONFIRMED & CORRECTED** in E1R: $H_{\text{dev}} = +\infty, H_{\text{max}} = H_{\text{cfg}}$ on $s_{\text{max}} = 0$. Verified by `e1r_hmax.py` (EE-5). |
| **E-F2-07** | E-F2 | Level C | Fullscreen 8K buffer allocation claim on 1080p display | **DISPROVED WITH EVIDENCE** in E1R: clamped by $H_{\text{cfg}} = 3.0$ to $5760 \times 3240 \le D_{\text{max}}$. Confirmed disproved in this review. |
| **E-F1-01** | E-F1 | — | Bare invariant tokens in ADRs without `INV-` prefix | **CONFIRMED & CORRECTED** in E1R; checker check `adr-unprefixed-inv` added. |
| **E-F3-01** | E-F3 | — | Definition lines in formulas not validated by checker | **CONFIRMED & CORRECTED** in E1R; checker formula structural syntax checks added. |
| **E-F3-02** | E-F3 | — | Headless transform test coverage limited to `Transform::Normal` | **CONFIRMED & CORRECTED** in E1R; `xtransform` matrix (EE-1) added across all 8 transforms. |
| **E-F3-03** | E-F3 | — | Camera stealing on concurrent `OutputKey` collision | **CONFIRMED & CORRECTED** in E1R; deterministic collision allocation added in `bind_on_connect`. |

### 2.2 E1R Corrective Checkpoint Findings (`docs/foundation/audit/E1R/DISPOSITIONS_E1R.md`)

| Finding ID | Class | Severity | Description | Final Disposition |
|---|---|---|---|---|
| **E1R-N1** | E-F2 | **Level A** | `F-HIT-1` applied output transform $u = \text{transform}_O(p \cdot \text{scale}_O)$ to view-space pointer | **CONFIRMED & CORRECTED** in E1R: $u = p \cdot \text{scale}_O$ in view space. Verified across 2,000 points in 10 cases with 0 mismatches (`xtransform`). |
| **E1R-N2** | E-F1 | — | Cursor frame harness optimization in `xtransform` | **CONFIRMED & ACCEPTED**: harmless O(1) performance optimization. |
| **E1R-N3** | E-F2 | Level B | Preferred integer buffer scale $\lceil h \rceil$ could exceed $H_{\text{dev}}$ | **CONFIRMED & CORRECTED** in E1R: $b(h) = \max(1, \lfloor H_{\text{dev}} \rfloor)$ when $\lceil h \rceil > H_{\text{dev}}$. Verified by `e1r_hmax.py` (EE-5). |

---

## 3. New Independent Findings Discovered in Final Review

### Finding E2-F0-01: Discrepancy in Audit Prose Regarding Checker Distinct Check Count

- **Category:** `E2-F0` (Editorial / Bookkeeping only).
- **Severity:** Non-semantic audit documentation defect. Does NOT block Phase F.
- **Attacked Claim:**
  `docs/foundation/audit/phase-e-e1r-review/06_CHECKER_ATTACK.md` lines 12–13 and `E1R_REVIEW_REPORT.md` line 71 stated:
  > *"43 distinct checks executed. All 43 checks returned PASS (0 failures)."*
- **Evidence:**
  Direct execution of `tools/check-foundation-docs` prints exactly **45 distinct check names**, all passing:
  `adr-unprefixed-inv`, `d9-verdict`, `deferred-closed`, `e1r-adds-no-inv`, `formula-duplicate`, `formula-fields`, `formula-provenance`, `formula-ref`, `formula-syntax`, `id-duplicate`, `inc-category`, `inc-exec-class`, `inc-fields`, `inc-ob24-gate`, `inc-physical-authorization`, `inc-slice0-ob19`, `inc-slice1-seam`, `inv-count`, `inv-duplicate`, `inv-fields`, `inv-provenance`, `inv-set`, `inv-wording-mismatch`, `ns-complete`, `ns-duplicate-ownership`, `ns-homes-in-package`, `ns-map-present`, `ob-destination-vocab`, `ob-fields`, `ob-missing-destination`, `ob-missing-pass`, `ob-status-vocab`, `ob19-conditional`, `ob36-scoped`, `package-exists`, `pdf-status`, `pdf2-missing`, `pdf2-open-listed`, `prov-fields`, `refs-resolve`, `region-in-home`, `region-present`, `risk-backed`, `risk-disposition`, `sm-cites-inv`.
- **Diagnosis:**
  In Phase D final seal (`09075a2`), the checker output exactly 40 check names. E1R added 5 new check names (`adr-unprefixed-inv`, `e1r-adds-no-inv`, `formula-fields`, `formula-provenance`, `formula-syntax`), bringing the total to 45. The previous reviewer's prose report contained a manual counting/drafting error ("43") while the actual tool execution in the evidence dossier produced 45.
- **Consequence:**
  None for canonical architecture, mathematical formulas, state machines, or implementation increments. The verification tool is fully functional (18/18 self-test fixtures pass).
- **Required Correction:**
  Documented and resolved in `CHECKER_REVIEW.md` and this register. No canonical architecture changes required.

---

## 4. Finding Counts & Blocking Status

| Category | Finding ID | Blocking Phase F? |
|---|---|---|
| **E2-F0** | `E2-F0-01` (Checker prose count discrepancy) | **NO** |
| **E2-F1** | None | — |
| **E2-F2 (Level A)** | None | — |
| **E2-F2 (Level B)** | None | — |
| **E2-F2 (Level C)** | None | — |
| **E2-F3** | None | — |

**Total Blocking Findings:** **0**.
