# KNOW-ROAD-005: Work Remaining After the INC-7 Exit Preparation

| Field | Value |
| :--- | :--- |
| **id** | KNOW-ROAD-005 |
| **title** | Work Remaining After the INC-7 Exit Preparation |
| **subsystem** | Core/Presentation |
| **source_rev** | integration/inc-7-exit-2026-10-14 @ 93c2605 |
| **date** | 2026-10-14 |
| **evidence_class** | SOURCE-DERIVED |
| **applicability** | INC-7 exit, INC-6 exit, INC-8, INC-11, INC-13, INC-14 |
| **affected_branches** | integration/inc-7-exit-2026-10-14, experimental/inc7-exit-readiness-2026-10-14, perf/inc-6-scale-index |
| **related_invariants** | INV-PRES-1, INV-INPUT-8, INV-STAGE-1, X-15, INV-ARCH-2, OB-1, OB-18, OB-22, OB-32, OB-42 |
| **adoption_status** | DEFERRED |
| **next_action** | Owner decisions on amendment layers C12.F5–F8, then the INC-7 exit review on the integration candidate |

---

## Verified Facts
1. **Governance (INC-7):** layers C12.F5 (placement after fullscreen), C12.F6 (INV-PRES-1 deferred takeover), C12.F7 (INV-INPUT-8 eligibility and retention), C12.F8 (X-15 popup order) are prepared, not adopted. C12.F9 (INV-ARCH-2 test-only allocator) belongs to the INC-6 exit.
2. **Integration:** INC-7 was ported without INC-6 onto `integration/inc-7-exit-2026-10-14` (from the INC-7 line `f077e55`); see KNOW-DEC-005.
3. **Exit obligations:** OB-1, OB-18, OB-32 and OB-22's T1 part have their evidence on the integration candidate; OB-1's order criterion and OB-22's A-3 evidence depend on C12.F8 and C12.F6.
4. **Follow-ups (not INC-7 obligations):** W-SM-18 at the next Smithay pin (remove `ensure_configured` calls, require code 3); W-SM-19 guard as an INC-14 entry condition; INC-8 frame callbacks for presentation-hidden windows (audit A-7); the allocation-free presentation snapshot (needed only with INC-6's OB-42 ratchet); A-10 (three per-element fullscreen side tables); A-5 far-future residual and libinput clock check at INC-13; possible INC-11 policy to clear focus when a presentation hides the focused window.

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
