# KNOW-PITFALL-005: Quadratic Scale-State Evaluation (Pre-P1 Defect)

| Field | Value |
| :--- | :--- |
| **id** | KNOW-PITFALL-005 |
| **title** | Quadratic Scale-State Evaluation (Pre-P1 Defect) |
| **subsystem** | Pitfalls/ScalePolicy |
| **source_rev** | 40ddd7d / f1ef262 |
| **date** | 2026-10-14 |
| **evidence_class** | EXPERIMENTALLY VERIFIED |
| **applicability** | Scale policy evaluation performance |
| **affected_branches** | candidate/inc-6-on-main-ede1541 |
| **related_invariants** | INV-SCALE-1..5, OB-42 |
| **adoption_status** | SUPERSEDED |
| **superseded_by** | KNOW-ARCH-004 |
| **next_action** | Maintain P-1 repair in scoped scale evaluation |

---

## Verified Facts
1. On commit `40ddd7d` before rule P-1 was repaired, scale policy evaluation evaluated every element on the Canvas on every commit, allocating 5,028 objects per pan step and 5,022 per commit at 1,000 windows.
2. In commit `f1ef262`, Infynitae repaired P-1 (`scoped_scale_evaluation_equals_the_full_reference`), restricting scale updates to elements observed by an output and verifying that no stale scale state persists for destroyed elements.
3. This reduced allocations from 5,028 to 15 allocations per pan step at 1,000 windows, completely eliminating quadratic behavior.

## Unverified Hypotheses
- None; verified via `t0_hot_path_allocations.rs`.

## Performance & Correctness Implications
- Eliminates memory allocation spikes that cause CPU stutter during interactive camera panning.

## Limitations & Counterexamples
- Reconsideration would only be justified if scale state was global and non-spatial (violating `INV-SCALE-1`).

## Reproduction Instructions
Execute the P-1 scale repair integration test:
```bash
cargo test -p infynitae-core --test t0_hot_path_allocations steady_state_batches_do_not_evaluate_every_element   # as recorded; correction 2026-10-15 below
# 2026-10-15: steady_state_batches_do_not_evaluate_every_element is a Core library test, and the ratchet moved:
cargo test -p infynitae-core --lib steady_state_batches_do_not_evaluate_every_element
cargo test -p infynitae-backend-headless --test t0_hot_path_allocations
```

## Links & Citations
- Source Commit: [`f1ef262`](file:///home/vysaely/infynitae-native)
- Related Architecture: [KNOW-ARCH-004: Spatial Membership Culling](../architecture/KNOW-ARCH-004-spatial-membership-culling.md)
- Related Experiment: [KNOW-EXP-002: OB-42 Global Counting-Allocator Ratchet](../experiments/KNOW-EXP-002-ob42-counting-allocator-ratchet.md)
