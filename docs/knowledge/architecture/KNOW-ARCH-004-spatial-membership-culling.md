# KNOW-ARCH-004: Spatial Membership Culling and Scoped Scale Passes

| Field | Value |
| :--- | :--- |
| **id** | KNOW-ARCH-004 |
| **title** | Spatial Membership Culling and Scoped Scale Passes |
| **subsystem** | Core/Visibility |
| **source_rev** | 85a74cb / f1ef262 |
| **date** | 2026-10-14 |
| **evidence_class** | EXPERIMENTALLY VERIFIED |
| **applicability** | All Phase F / INC-6+ branches |
| **affected_branches** | main, remediation/b-core-visibility, perf/inc-6-scale-index |
| **related_invariants** | F-MEMBER-1, INV-RENDER-3, INV-SCALE-1..5 |
| **adoption_status** | IMPLEMENTED |
| **next_action** | Maintain allocation bounds in t0_hot_path_allocations |

---

## Verified Facts
1. Naive scene management iterates over all mapped elements ($\mathcal{O}(N)$) on every commit, camera motion, and scale pass.
2. Infynitae Core implements `Membership`: exact 2D spatial tracking determining which elements intersect an output's viewport.
3. During camera panning and window movement, scale policy evaluation ($P_7$) and render element building are strictly scoped to visible members (`entries_culled`), maintaining constant allocation counts independent of the total number of off-view windows.

## Unverified Hypotheses
- Hypothesized that spatial membership culling at Canvas bounds of $2^{36}$ coordinates maintains sub-pixel exactness under fractional scale lattices ($1/120$); verified by test `cull_agrees_with_the_reference_at_the_view_edges`.

## Performance & Correctness Implications
- Prevents quadratic scaling degradation when scaling from $N=10$ to $N=3,000$ mapped windows.
- Keeps pan step execution time under $31\,\mu\text{s}$ at 3,000 windows.

## Limitations & Counterexamples
- Off-screen windows outside the spatial membership set do not receive active render updates, necessitating controlled liveness heartbeats for background swapchains.

## Reproduction Instructions
Execute the spatial culling edge reference tests:
```bash
cargo test -p infynitae-core --lib cull_agrees_with_the_reference_at_the_view_edges
cargo test -p infynitae-core --test t0_hot_path_allocations
```

## Links & Citations
- Source Commit: [`85a74cb`](file:///home/vysaely/infynitae-native), [`f1ef262`](file:///home/vysaely/infynitae-native)
- Related Pitfall: [KNOW-PITFALL-005: Quadratic Scale-State Evaluation](../pitfalls/KNOW-PITFALL-005-quadratic-scale-state-evaluation.md)
