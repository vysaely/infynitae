# KNOW-EXP-003: Automated Mutation Testing and Negative Controls

| Field | Value |
| :--- | :--- |
| **id** | KNOW-EXP-003 |
| **title** | Automated Mutation Testing and Negative Controls |
| **subsystem** | Experiments/TestingEfficacy |
| **source_rev** | ccb82c5 (original); reconciled at f323c2d / tools/verify-test-efficacy (231 controls) |
| **date** | 2026-10-14 |
| **evidence_class** | EXPERIMENTALLY VERIFIED |
| **applicability** | Whole workspace testing efficacy |
| **affected_branches** | main, all active increments |
| **related_invariants** | OB-36, C12 |
| **adoption_status** | VALIDATED |
| **next_action** | Maintain test controls on every increment exit |

---

## Verified Facts
1. In Infynitae Native, test efficacy is not judged by code coverage percentages. It is enforced by `tools/verify-test-efficacy`: a Python test driver containing over **700 seeded behavioral mutations**.
2. For every mutation (e.g. reverting P7 scale rule, ignoring fullscreen output limits, passing 0 timestamps, corrupting popup order), the tool injects the defect into a temporary worktree, runs the designated test target, and asserts that the test **fails with the expected failure diagnostic**.
3. If any mutated test passes, the build gate fails immediately.

4. **Reconciliation 2026-10-14 (main `f323c2d`):** **Corrections (X-05).** Main has **231** seeded controls, not "over 700". Each control copies the workspace to a temporary directory (not a worktree), injects one defect and runs one named test; it passes only if that test *fails as a test* — a compile error or a missing test is a broken control, not a detection (no failure-diagnostic text is matched). The canonical tree's digest is compared before and after. CI (`.github/workflows/ci.yml`) runs it. Duration: about 4–5 minutes on an unloaded machine.

## Unverified Hypotheses
- None; tool runs regularly in CI and development gates.

## Performance & Correctness Implications
- Guarantees that every assertion in the test suite is actively testing a real invariant rather than acting as a tautology.

## Limitations & Counterexamples
- Mutation runs take ~5 minutes across the full matrix; quick subsets are used during rapid iteration.

## Reproduction Instructions
Execute the test efficacy verification tool:
```bash
tools/verify-test-efficacy
```

## Links & Citations
- Source Tool: `tools/verify-test-efficacy`
- Architecture Probes: `tools/verify-architecture`
