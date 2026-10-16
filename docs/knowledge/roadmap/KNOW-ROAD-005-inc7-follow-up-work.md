# KNOW-ROAD-005: Work Remaining After the INC-7 Exit Preparation

| Field | Value |
| :--- | :--- |
| **id** | KNOW-ROAD-005 |
| **title** | Work Remaining After the INC-7 Exit Preparation |
| **subsystem** | Core/Presentation |
| **source_rev** | integration/inc-7-exit-2026-10-14 @ 93c2605; reconciled to main f323c2d (INC-7 RATIFIED 39ad614) |
| **date** | 2026-10-14 |
| **evidence_class** | SOURCE-DERIVED |
| **applicability** | INC-7 exit, INC-6 exit, INC-8, INC-11, INC-13, INC-14 |
| **affected_branches** | main (f323c2d); INC-6 lines (perf/inc-6-scale-index, experimental/inc7-exit-readiness-2026-10-14) |
| **related_invariants** | INV-PRES-1, INV-INPUT-8, INV-STAGE-1, X-15, INV-ARCH-2, OB-1, OB-18, OB-22, OB-32, OB-42 |
| **adoption_status** | DEFERRED |
| **next_action** | Carry R-07 into the INC-14 entry conditions, R-10 into INC-8, R-04/R-05 into INC-13, R-06 into the next Smithay pin |

---

## Verified Facts
1. **Governance (INC-7):** layers C12.F5 (placement after fullscreen), C12.F6 (INV-PRES-1 deferred takeover), C12.F7 (INV-INPUT-8 eligibility and retention), C12.F8 (X-15 popup order) ~~are prepared, not adopted~~ — **adopted and ratified 2026-10-14**, together with C12.F10 (unmap of a fullscreen element, [KNOW-DEC-006](../decisions/KNOW-DEC-006-c12f10-unmap-of-a-fullscreen-element.md)). C12.F9 (INV-ARCH-2 test-only allocator) belongs to the INC-6 exit.
2. **Integration:** INC-7 was ported without INC-6 onto `integration/inc-7-exit-2026-10-14` (from the INC-7 line `f077e55`); see KNOW-DEC-005.
3. **Exit obligations:** OB-1, OB-18, OB-32 and OB-22's T1 part have their evidence on the integration candidate; OB-1's order criterion and OB-22's A-3 evidence depend on C12.F8 and C12.F6.
4. **Follow-ups (not INC-7 obligations):** W-SM-18 at the next Smithay pin (remove `ensure_configured` calls, require code 3); W-SM-19 guard as an INC-14 entry condition; INC-8 frame callbacks for presentation-hidden windows (audit A-7); the allocation-free presentation snapshot (needed only with INC-6's OB-42 ratchet); A-10 (three per-element fullscreen side tables); A-5 far-future residual and libinput clock check at INC-13; possible INC-11 policy to clear focus when a presentation hides the focused window.

5. **Reconciliation 2026-10-14 (main `f323c2d`):** **INC-7 RATIFIED** (`39ad614`; CHECKPOINT `f323c2d`). OB-1, OB-18, OB-32 CLOSED; OB-22 T1 part met, stays OPEN (T3/T5, release blocker); OB-13, OB-42, OB-44, OB-52 OPEN. INC-6 not ratified; INC-8 and INC-14 not begun.
6. **Accepted residuals R-01…R-15** (tracked follow-up work, not resolved defects; register `~/Infynitae-INC7-EXIT-PREPARATION-2026-10-14/INC7_RESIDUAL_AND_FOLLOW_UP_REGISTER.md`), by owner of the follow-up:
   | Residual | Owner |
   | :--- | :--- |
   | R-01 retained focus on a hidden window (owner policy); R-13 single-output stickiness | Policy; a possible INC-11 change needs INV-INPUT-8 amended |
   | R-04 far-future device timestamp; R-05 libinput clock domain | INC-13 |
   | R-06 W-SM-18 error code 1 instead of 3 | Next Smithay pin (upstream `641d2ad`) |
   | R-07 W-SM-19 stale popup pointer grab | **INC-14 entry condition** |
   | R-08 W-SM-20 depends on creation order (keep Smithay `Window` paths out, OB-26) | Standing rule |
   | R-09 benchmark codegen-unit sensitivity | Optional measurement ([KNOW-EXP-005](../experiments/KNOW-EXP-005-benchmark-codegen-unit-sensitivity.md)) |
   | R-10 hidden windows get no frame callbacks (A-7) | **INC-8** |
   | R-11 three fullscreen side tables purged together (A-10) | Later refactor |
   | R-12 fullscreen-size windowed client never spends the size on offer | None (informational) |
   | R-14 real-client evidence only `fs_client`, KCalc | OB-22 T3/T5 (release) |
   | R-15 `observers()` allocation in A-8 check | Converges when INC-6 integrates (OB-42) |
   R-02 and R-03 (sealed text vs behaviour; re-gating) are discharged: the layers are adopted and the candidate was re-gated (`INC7_FORMAL_EXIT_DECISION.md`).

## Unverified Hypotheses
- That the integration candidate merges into `main` without conflict after ratification is not checked (no merge was attempted).

## Performance & Correctness Implications
- None of the follow-ups blocks correctness of INC-7 as implemented; R-01 (retained focus on a hidden window) is a recorded policy residual.

## Limitations & Counterexamples
- INC-6 remains unexited; the INC-6×INC-7 scale glue exists only on the combined/experimental lines.

## Reproduction Instructions
See the exit preparation package: `~/Infynitae-INC7-EXIT-PREPARATION-2026-10-14/` (approval package, dependency graph, gate results, obligations evidence, register, checklist).

## Links & Citations
- [KNOW-DEC-005: INC-7 Integration Provenance](../decisions/KNOW-DEC-005-inc7-integration-provenance.md)
- [KNOW-DEC-002: Focus Eligibility and Retention](../decisions/KNOW-DEC-002-a8-fullscreen-focus-eligibility.md)
- [KNOW-PITFALL-007: Smithay 0.7.0 Limitations](../pitfalls/KNOW-PITFALL-007-smithay-0-7-xdg-shell-limitations.md)

*2026-10-16:* C12.F9 was withdrawn by the owner (the allocator test moved to the headless backend; INV-ARCH-2 unchanged); C12.F11–C12.F13 were adopted for INC-6 on `governance/inc-6-exit-adoption-2026-10-16` (not ratified).
