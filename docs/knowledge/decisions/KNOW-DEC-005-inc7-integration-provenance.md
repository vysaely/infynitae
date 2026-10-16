# KNOW-DEC-005: INC-7 Integration Provenance (INC-7 Without INC-6)

| Field | Value |
| :--- | :--- |
| **id** | KNOW-DEC-005 |
| **title** | INC-7 Integration Provenance (INC-7 Without INC-6) |
| **subsystem** | Core/Presentation |
| **source_rev** | integration/inc-7-exit-2026-10-14 @ 93c2605 (from experimental/inc-7-toplevel-requests @ f077e55) |
| **date** | 2026-10-14 |
| **evidence_class** | EXPERIMENTALLY VERIFIED |
| **applicability** | UNRATIFIED integration candidate for the INC-7 exit; not merged, not pushed |
| **affected_branches** | main (f323c2d); historical: integration/inc-7-exit-2026-10-14, experimental/inc7-exit-readiness-2026-10-14, experimental/inc-7-toplevel-requests |
| **related_invariants** | INV-PRES-1, INV-INPUT-8, INV-STAGE-1, X-15, OB-1, OB-18, OB-32, OB-22 |
| **adoption_status** | IMPLEMENTED |
| **next_action** | None for INC-7. INC-6 must be rebased onto main f323c2d; R-15 converges then |

---

## Verified Facts
1. **Why a separate line.** The INC-7 exit-readiness candidate (`419d5f8`) is built on the combined candidate and contains INC-6 (`40ddd7d`, the P-1 repair, the INC-6×INC-7 scale glue), which has not exited. The INC-7 line `f077e55` contains no INC-6 and contains `main` `ccb82c5`.
2. **What was ported** onto `f077e55`: merge of the audit branch (`f1551d4`: A-1, A-2, suites, record, A-5 test) → `ccd7f50`; A-5 → `d70c3be`; fixtures `app_probe`, `fs_client` → `0a69719`; A-3 → `b1aa758`; A-8 → `0f45cfd`; P9b → `334b9b3`; X-15 → `23860b3`; control re-anchor → `d350ed2`; A-8 fast path → `2aa8d65`; W-SM-18/19/20 records → `93c2605`. Each ported message carries its source commit.
3. **INC-6 dependencies found and handled.** A-8 called INC-6's `MembershipTable::observing_outputs` → adapted to the existing `observers(element).is_empty()` (same predicate). `app_probe` called `report_renderer_limits` (OB-13, INC-6) → that line removed. The stale-scale-state assertion of `489180e` (tests INC-6's index) and the INC-6×INC-7 glue were not ported. No INC-7 fix needed INC-6 code to work.
4. **Gates on 93c2605** (not inferred from the source line): fmt, clippy, 347 tests (0 failed, 0 ignored), architecture, docs, docs self-test, 231/231 seeded defects, development environment — all PASS. The 60 tests and 63 controls present only on the source line are all INC-6, its session clock, or the GLES application-validation test (listed in the preparation package).
5. **Real clients on 93c2605:** `fs_client` 13/13 and KCalc 11/11 captures byte-identical to the source candidate's; A-5 times 1020, 1020, 1040, 1040, 1060; binary run clean.
6. **Performance against the INC-7 line `f077e55`:** pan, commit, remap, churn, hotplug and allocations equal; render list +6.6 % and drag release +8 % at 3,000 windows — instruction counts per render list +7 % (1,024,592 → 1,096,531), but with `codegen-units = 1` 973,406 vs 973,415: a codegen-unit partitioning effect (see KNOW-EXP-005), not added logic.

7. **Reconciliation 2026-10-14 (main `f323c2d`):** **Outcome.** The integration candidate `93c2605` was carried, unchanged in code, through the governance branches (`80ead2a` C12.F5–F8; `a43a149` C12.F10; `99c24cd` comment fix) to the ratification commit `39ad614` and the CHECKPOINT clarification `f323c2d`. Local `main` fast-forwarded `ccb82c5 → 39ad614 → f323c2d`, and `origin/main` was fast-forwarded `4d2fb3d → f323c2d` by owner approval (audit: `~/Infynitae-INC7-EXIT-PREPARATION-2026-10-14/INC7_RATIFICATION_AND_PUBLICATION_AUDIT.md`). The source-line commits cited elsewhere in this knowledge base (`07acfeb`, `0fe8f06`, `df72202`, `3b0a3d7`, `489180e`, `e40eb70`) are **not** on main; their main equivalents are listed in Fact 2.

## Unverified Hypotheses
- ~~A future merge of this branch into `main` (after ratification) is assumed conflict-free; not attempted.~~ Resolved: it was a fast-forward (Fact 7).

## Performance & Correctness Implications
- INC-7 can exit without waiting for INC-6, provided the owner adopts C12.F5–F8 (C12.F9 belongs to INC-6).

## Limitations & Counterexamples
- The integration candidate carries no INC-6 behaviour: no scale hints, no OB-42 ratchet, no test-only `unsafe`.

## Reproduction Instructions
```bash
git log --oneline experimental/inc-7-toplevel-requests..integration/inc-7-exit-2026-10-14
# gates, from a worktree of 93c2605 with its own target/runtime/tmp directories:
cargo fmt --all --check && cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --locked --offline && tools/verify-architecture && tools/check-foundation-docs
tools/check-foundation-docs --self-test && tools/verify-test-efficacy && tools/verify-development-environment
```

## Links & Citations
- Preparation package: `~/Infynitae-INC7-EXIT-PREPARATION-2026-10-14/` (dependency graph, approval package, gate results, obligations evidence, register, checklist)
- [KNOW-ROAD-005: Remaining Work](../roadmap/KNOW-ROAD-005-inc7-follow-up-work.md)
- [KNOW-EXP-005: Codegen-Unit Sensitivity](../experiments/KNOW-EXP-005-benchmark-codegen-unit-sensitivity.md)

*2026-10-16:* C12.F9 was withdrawn by the owner (the allocator test moved to the headless backend; INV-ARCH-2 unchanged); C12.F11–C12.F13 were adopted for INC-6 on `governance/inc-6-exit-adoption-2026-10-16` (not ratified).
