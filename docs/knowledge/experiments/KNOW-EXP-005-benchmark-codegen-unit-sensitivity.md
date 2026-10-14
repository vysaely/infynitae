# KNOW-EXP-005: Core Benchmark Sensitivity to Codegen-Unit Partitioning

| Field | Value |
| :--- | :--- |
| **id** | KNOW-EXP-005 |
| **title** | Core Benchmark Sensitivity to Codegen-Unit Partitioning |
| **subsystem** | Core/Visibility |
| **source_rev** | 0fe8f06 → df72202 (experimental/inc7-exit-readiness-2026-10-14) |
| **date** | 2026-10-14 |
| **evidence_class** | INDEPENDENTLY REPRODUCED |
| **applicability** | Any release-build comparison of Core event paths between commits |
| **affected_branches** | experimental/inc7-exit-readiness-2026-10-14 |
| **related_invariants** | OB-44 |
| **adoption_status** | RESEARCH |
| **next_action** | Before comparing two commits' timings, compare instructions per operation; optionally measure `codegen-units = 1` for release builds |

---

## Verified Facts
1. At 3,000 windows the steady pan step costs 27.56 µs median at `740b879` / `0fe8f06` and 30.26 µs at `df72202` and later (three interleaved rounds, reproducible: 30.25–30.43 µs), although `df72202` (P9b placement) changes nothing on the pan path. Allocations are equal (15).
2. Hardware counters on a pan-only loop (200,000 steps minus setup): 547,821 → 601,815 instructions per step (+9.9 %), 114,709 → 126,081 cycles. Real extra work, not an alignment or cache effect.
3. Same data and same calls: placement after setup is identical (anchor hash, 9 membership rows); an instrumented copy counts 9,039 `projection::edge` calls per step in both builds; the software `round` both call is byte-identical.
4. `MembershipTable::with_output` (source unchanged) compiles to 372 instructions before and 391 after (+14 `mov`).
5. **With `codegen-units = 1`, both builds execute exactly 568,466 instructions per pan step.** The difference comes from the compiler partitioning the crate into codegen units differently after unrelated functions changed, which changes inlining and register allocation in the membership loop.

## Unverified Hypotheses
- That `codegen-units = 1` gives better or more stable release performance overall is not measured.

## Performance & Correctness Implications
- A ±10 % timing change on a Core path between two commits can be caused by an unrelated edit elsewhere in the crate. Attributing it to the edited code, or to "code layout", without counting instructions is wrong (an earlier checkpoint did the latter; see CONTRADICTIONS_AND_SUPERSEDED).
- Magnitude here: ≈ 0.9 ns per window per pan step; 30 µs is 0.5 % of a 165 Hz frame. Not material.

## Limitations & Counterexamples
- Measured with the scratch `perfbench` harness (outside the repository), model only (no Smithay, no renderer, no hardware); not a C14 baseline (OB-44).

## Reproduction Instructions
Build each commit's `infynitae-core` into the harness, then:
```bash
PANLOOP=0      perf stat -x, -e instructions:u ref-<commit>/target/release/perfbench x 3000
PANLOOP=200000 perf stat -x, -e instructions:u ref-<commit>/target/release/perfbench x 3000
# instructions per step = (second − first) / 200000; repeat with [profile.release] codegen-units = 1
```
Harness and data: `~/.cache/infynitae-integration/perf/bench/`; review report `~/Infynitae-INC7-EXIT-REVIEW-2026-10-14/INC7_FINAL_GOVERNANCE_REVIEW.md` §5.

## Links & Citations
- Related Pitfall: [KNOW-PITFALL-005: Quadratic Scale-State Evaluation](../pitfalls/KNOW-PITFALL-005-quadratic-scale-state-evaluation.md)
- Related Architecture: [KNOW-ARCH-004: Spatial Membership Culling](../architecture/KNOW-ARCH-004-spatial-membership-culling.md)
