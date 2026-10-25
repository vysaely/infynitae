# Phase F — INC-0 Independent Adversarial Review Verdict

Target Commit: `0c229adf6c7837d7b659b887ae1c15cc09a8c8b3` (branch `main`)
Pre-INC-0 Baseline: `aa8e1d255510dfd2e6f7b7eb3403609f55f04dc0`
Auditor: Independent Principal Systems Architect & Senior Wayland/Smithay Compositor Reviewer
Date: 2026-10-05

========================================================================
FINAL VERDICT:
INC-0 CONDITIONAL PASS — SPECIFICATION RATIFICATION REQUIRED BEFORE INC-1
========================================================================

## 1. Executive Summary

Implementation commit `632b1d0` and evidence commit `0c229ad` have established the production Cargo workspace, compiler enforcement machinery, and CI automation for Infynitae Native.

The architectural boundary separating compile-time enforcement from product behavior has been respected:
- Zero authoritative product state was prematurely implemented.
- Pure core remains completely free of external dependencies (`std` only).
- All 26 compiler probes and 1 positive control compile and fail for their exact asserted diagnostics.
- `#![forbid(unsafe_code)]` and `#![deny(unsafe_code)]` are placed and mechanically enforced.
- Smithay is pinned to `=0.7.0` from crates.io; patch P1 was NOT productionized.
- CI via GitHub Actions (`.github/workflows/ci.yml`) is functional and verified passing.

Adversarial testing revealed **two Level-B enforcement gaps** (blind spots in the AST source-rules checker and DAG validator) and **one Level-C specification wording omission**, alongside the resolution of the **OB-19 / 2i runtime validation transfer**.

---

## 2. Findings Register

### Finding INC0-F01 (LEVEL B) — AST Source-Rules Checker Ignores Reference Getters
- **Subsystem:** `enforcement/source-rules/src/main.rs:373` (`by_value_names`)
- **Severity:** LEVEL B
- **Defect:** `by_value_names` explicitly matches `syn::Type::Reference(_) | syn::Type::Ptr(_) => {}`. Consequently, any function returning an authority type by reference (e.g. `pub fn token(&mut self) -> &mut OutputLifecycleToken`) is ignored by the `authority-return` rule.
- **Architectural Citation:** FOUNDATION_ARCHITECTURE §C12.R2.3 ("No method of an existing Core returns, clones, re-creates or replaces a token... Code holding `&mut Core` cannot obtain the token: there is no accessor").
- **Impact:** While `Core` does not store an `OutputLifecycleToken` today, the AST verifier contains a blind spot that would allow reference getters to pass undetected if a field were added later.
- **Corrective Action Required:** Update `source-rules` to inspect all return types for authority types, whether passed by value, reference (`&`, `&mut`), or pointer.

### Finding INC0-F02 (LEVEL B) — DAG Validator Skips `[dev-dependencies]`
- **Subsystem:** `tools/verify-architecture:300-301` (`dag_problems`)
- **Severity:** LEVEL B
- **Defect:** `dag_problems` filters dependencies with `if d["kind"] not in (None, "build"): continue`, silently skipping dev-dependencies. If `crates/infynitae-core/Cargo.toml` declares `[dev-dependencies]` on `infynitae-wayland` or `smithay`, `tools/verify-architecture` reports `ok infynitae-core has no dependencies`.
- **Architectural Citation:** INV-ARCH-1 ("core has zero dependencies; all edges point to core").
- **Impact:** Forbidden dependencies could be introduced into test configurations of pure core without verifier failure.
- **Corrective Action Required:** Update `dag_problems` to validate `dev` dependencies for `infynitae-core` and `infynitae-ipc`, asserting that `infynitae-core` has zero dev-dependencies.

### Finding INC0-F03 (LEVEL C) — `FOUNDATION_TEST_PLAN.md` OB-19 Text Omits E1R Probes 2h/2i
- **Subsystem:** `docs/foundation/FOUNDATION_TEST_PLAN.md:275`
- **Severity:** LEVEL C
- **Defect:** The living text of OB-19 in `FOUNDATION_TEST_PLAN.md` lists probes up to 2R (2d–2g) but was not updated to reflect §C12.E1R.4 (which added 2h and 2i).
- **Architectural Citation:** FOUNDATION_ARCHITECTURE §C12.E1R.4.
- **Impact:** Editorial inconsistency between living test plan and architecture registry.
- **Corrective Action Required:** Harmonize `FOUNDATION_TEST_PLAN.md` OB-19 text with §C12.E1R.4.

### Finding INC0-F04 (VALIDATION OBSERVATION) — Runtime 2i Transfer to INC-2 / OB-49
- **Subsystem:** `FOUNDATION_IMPLEMENTATION_PLAN.md` INC-0 / INC-2; `FOUNDATION_TEST_PLAN.md` OB-19 / OB-49.
- **Severity:** VALIDATION OBSERVATION
- **Evaluation:** Step 2i (`OutputKey` collision negative control) requires runtime camera creation and 1:1 binding resolution logic, which belongs strictly to INC-2. Transferring runtime execution of 2i to INC-2 / OB-49 is architecturally legitimate and necessary to preserve the INC-0 "zero authoritative state" boundary.

---

## 3. Decisions on Obligations

### OB-19 Decision: Option D
**OB-19 RATIFY CLOSED WITH EXPLICIT TRANSFER OF RUNTIME 2i VALIDATION**
- **Rationale:** All compile-time authority boundaries, type unforgeability, `#![forbid(unsafe_code)]`, dependency DAG restrictions, Smithay `=0.7.0` pin, toolchain `1.99.0` pin, and compile probes (including 2h reconfiguration probes `p10`–`p13` and source check) are verified.
- **Transferred Item:** The runtime execution of the 1:1 camera binding collision negative control (`probe_sealed_key_steal` / `concurrent_output_key_collision`).
- **Destination:** Transferred to **INC-2**, owned by **OB-49** (and the runtime counterpart of OB-19).
- **Invariants Preserved:** `INV-CAMERA-3` (1:1 binding) and `INV-OUTPUT-2` (Output lifecycle authority).

### OB-36 Decision:
**OB-36 RATIFY CLOSED**
- **Rationale:** `.github/workflows/ci.yml` defines the `foundation-docs` job running `tools/check-foundation-docs` and `tools/check-foundation-docs --self-test` on every commit. GitHub Actions run `37279134431` confirms that the job executes and passes.

---

## 4. Open Finding Counts
- **Open LEVEL A:** 0
- **Open LEVEL B:** 2 (INC0-F01, INC0-F02)
- **Open LEVEL C:** 1 (INC0-F03)
- **Validation Observations:** 1 (INC0-F04)

---

## 5. Recommended Next Actions
1. Authorize a targeted corrective patch addressing Finding INC0-F01 (reference getters in `source-rules`) and Finding INC0-F02 (dev-dependencies in `verify-architecture`).
2. Ratify the transfer of runtime 2i validation to INC-2 / OB-49.
3. Record the ratified status of OB-19 and OB-36 in `CHECKPOINT.md`.
4. Do NOT start INC-1 until the project owner explicitly authorizes advancement.
