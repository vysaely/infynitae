# KNOW-PITFALL-002: Global Unconditional Focus Recalculation (The Focus Pull Loop)

| Field | Value |
| :--- | :--- |
| **id** | KNOW-PITFALL-002 |
| **title** | Global Unconditional Focus Recalculation (The Focus Pull Loop) |
| **subsystem** | Pitfalls/Input |
| **source_rev** | DriftWM / Infynitae INC-7 Review |
| **date** | 2026-10-14 |
| **evidence_class** | CONTRADICTED |
| **applicability** | Pointer focus dispatch architecture |
| **affected_branches** | N/A (Rejected Anti-Pattern) |
| **related_invariants** | INV-INPUT-8, OB-42 |
| **adoption_status** | REJECTED |
| **next_action** | Maintain spatial culling and dirty-state focus push |

---

## Verified Facts
1. Upstream DriftWM implements a "focus pull" model (`refresh_pointer_focus`) that executes an unconstrained hit-test cascade across all stage entries on **every event-loop iteration** and **every frame tick**.
2. Independent benchmarking on a 12-thread Linux host revealed that a bare canvas pull costs $9.92\,\mu\text{s}$ (80% slower than claimed) and real motion costs $15.80\,\mu\text{s}$.
3. At high event rates (e.g. 8,000 Hz gaming mice) and desktop scale ($N=3,000$ windows), this introduces quadratic $\mathcal{O}(N \cdot E)$ CPU overhead, causing severe event-loop tail latency and cache thrashing.

## Unverified Hypotheses
- None; empirical testing confirms that dirty-state boundary push achieves identical consistency at a fraction of the CPU overhead.

## Performance & Correctness Implications
- Unconditional pull models saturate CPU cores during fast mouse movement and degrade frame pacing.

## Limitations & Counterexamples
- Reconsideration would only be justified if an environment had zero reliable commit hooks or boundary events (e.g. legacy X11 without input redirection).

## Reproduction Instructions
Execute the independent benchmark reproduction:
```bash
cargo test --release --bin driftwm -- tests::pull_cost::pull_cost_bench --ignored --nocapture
```

## Links & Citations
- Related Experiment: [KNOW-EXP-001: Independent Reproduction of Focus Pull Benchmark](../experiments/KNOW-EXP-001-pull-cost-bench-reproduction.md)
- Related Architecture: [KNOW-ARCH-004: Spatial Membership Culling](../architecture/KNOW-ARCH-004-spatial-membership-culling.md)
