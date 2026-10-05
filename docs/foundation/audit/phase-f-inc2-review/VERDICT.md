# Phase F Increment 2 (INC-2) — Independent Adversarial Review Verdict

## 1. Verdict

**INC-2 CONDITIONAL PASS — CORRECTIONS REQUIRED BEFORE RATIFICATION**

---

## 2. Review Metrics & Verification Results

| Check / Tool | Invocation | Result |
|---|---|---|
| Git Working Tree | `git status` | Clean, HEAD == origin/main (`8f27a31`) |
| Historical Ancestry | `git merge-base` | Seals Reachable: `615b0ed`, `e8ec03a`, `84ce620`, `5827926`, `c0a1cee`, `09075a2`, `1ae1b73`, `e9d3673`, `cfb54f0`, `aa8e1d2`, `a3710e1`, `a25a890` |
| Cargo Compilation | `cargo check --workspace --locked` | PASS (0 warnings) |
| Workspace Build | `cargo build --workspace --locked` | PASS |
| Workspace Test Suite | `cargo test --workspace --locked` | PASS (75 tests passed: 9 `t1_outputs`, 6 `t1_parity`, 4 `t1_slice`, 4 `t1_transform`, 52 `infynitae_core`) |
| Code Formatting | `cargo fmt --all -- --check` | PASS |
| Lints | `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS (0 warnings) |
| Architecture Verification | `./tools/verify-architecture` | PASS (47 seeded controls, 27 compiler probes, runtime 2i T0/T1) |
| Test Efficacy Verifier | `./tools/verify-test-efficacy` | PASS (45/45 seeded defects detected) |
| Foundation Docs Check | `./tools/check-foundation-docs` | PASS (57 invariants, 20 formulas, 51 obligations intact) |
| Foundation Self-Test | `./tools/check-foundation-docs --self-test` | PASS (18/18 seeded doc defects detected) |
| Historical Spike Probe | `./experiments/c1-authority-spike/verify.sh` | PASS (all compiler probes pass) |
| Dev Environment Check | `./tools/verify-development-environment` | PASS (all 8 check suites pass) |
| Continuous Integration | `.github/workflows/ci.yml` / GitHub Actions | PASS (workflows verified green) |

---

## 3. Findings Catalog

### LEVEL A: Architectural / Safety Violations (0 open)
*None.* The implementation strictly preserves the sealed architecture, capability ownership, unforgeable tokens, private minting, and the 1:1 camera binding invariant.

---

### LEVEL B: Implementation / Test-Sensitivity Defects (1 open)

#### Finding INC2-F07: Test Suite Lacks Sensitivity to `output_reconfigured` ProjectionFrame Invalidation
- **Subsystem:** `crates/infynitae-core/src/entry.rs:120`, `crates/infynitae-core/src/lib.rs:245-260`.
- **Condition:** In `Core::output_reconfigured`, line 120 correctly contains:
  ```rust
  self.frames.retain(|(o, _)| *o != output);
  ```
  However, if this line is completely deleted or commented out, **100% of workspace tests and 100% of `verify-test-efficacy` checks pass**.
- **Architectural Consequence:** `Core::hit_test(output, p)` reuses a cached `ProjectionFrame` if one exists in `self.frames`. If `output_reconfigured` fails to purge the output's frame, a subsequent hit-test without an intervening `render_list()` evaluates against the pre-reconfiguration geometry, transform, and extent.
- **Classification:** Level B (Test-Sensitivity Defect). Under Section 32 and 34 of the review charter, a test-sensitivity defect blocks unconditional ratification.
- **Remediation:**
  1. Add a test `reconfiguration_drops_the_output_frame` in `crates/infynitae-core/src/entry.rs` proving that a point outside the reconfigured extent no longer hits the pre-reconfiguration frame.
  2. Add an efficacy control in `tools/verify-test-efficacy` asserting detection when line 120 is omitted.

---

### LEVEL C: Specification / Documentation Contradictions (2 open)

#### Finding INC2-F01: `OB-49` Specification Contradiction Across Increment Boundaries
- **Subsystem:** `docs/foundation/FOUNDATION_TEST_PLAN.md` (lines 641–651).
- **Condition:** The "Item:" description of OB-49 includes "fullscreen configure" and "epoch settlement" while declaring `Blocks: INC-2 exit`. However, protocol fullscreen configure is canonically owned by INC-7 (`INV-PRES-1`), and scale interaction epoch settlement is canonically owned by INC-6 (`ScaleInteractionEpoch`). Neither is implementable or executable in INC-2.
- **Classification:** Level C (Specification Scoping Defect).
- **Remediation:** Formally ratify Decision E in change control, separating the INC-6 and INC-7 items from OB-49 and transferring them to obligations owned by INC-6 and INC-7.

#### Finding INC1-F01 (Tracked from INC-1): Fullscreen Configure Response Policy
- **Subsystem:** Protocol policy for toplevel state. Owned by INC-7.

---

### Observations (5 open)

- **INC2-F02:** `D-B6-BOUNDARY` popup constraint target uses parent clock output; unobserved parent gets no constraint.
- **INC2-F03:** `wayland-server` numeric global ID reuse handled correctly by client wire assertions.
- **INC2-F04:** Immediate `wl_output` global removal following `wl_surface.leave` is protocol-compliant and valid for headless stage.
- **INC2-F05:** Output arrangement rectangles can overlap after reconfiguration; permitted under Level D policy.
- **INC2-F06:** Damage tracker rebuild will be required on camera zoom changes in INC-10.

---

## 4. Required Corrective & Ratification Actions

1. **Resolution of INC2-F07 (Level B):**
   - Author test `reconfiguration_drops_the_output_frame` in `crates/infynitae-core/src/entry.rs`.
   - Register the mutation `("INC-2: reconfiguration keeps the output's ProjectionFrame", ...)` in `tools/verify-test-efficacy`.
2. **Resolution of INC2-F01 (Level C):**
   - Formally ratify Decision E transferring "fullscreen configure" to INC-7 and "epoch settlement" to INC-6.
   - Synchronize `FOUNDATION_TEST_PLAN.md` under `OB-49`.
3. **Ratification:** Following completion of the corrective patch and ratification review, INC-2 may be ratified PASS, enabling owner authorization of INC-3.
