# KNOW-PITFALL-006: Unbounded Off-Screen Frame Callback Heartbeats

| Field | Value |
| :--- | :--- |
| **id** | KNOW-PITFALL-006 |
| **title** | Unbounded Off-Screen Frame Callback Heartbeats |
| **subsystem** | Pitfalls/FrameScheduling |
| **source_rev** | DriftWM / Infynitae INC-7 Review |
| **date** | 2026-10-14 |
| **evidence_class** | DISPUTED |
| **applicability** | Frame callback delivery to non-visible surfaces |
| **affected_branches** | N/A (Architecture Risk) |
| **related_invariants** | INV-FRAME-1, A-7, INC-8 |
| **adoption_status** | REJECTED |
| **next_action** | Implement gated, batch-staggered off-screen callback heartbeat in INC-8 |

---

## Verified Facts
1. Upstream DriftWM implements an unconditional 995ms frame callback heartbeat for all off-screen toplevels ([`src/render/lifecycle.rs:20`](file:///home/vysaely/.gemini/antigravity/scratch/driftwm/src/render/lifecycle.rs#L20)) to prevent EGL swapchain starvation.
2. In an infinite canvas compositor with 3,000 background windows, an ungated 995ms heartbeat causes ~3,000 clients to wake up, re-render, and issue `wl_surface.commit` every second.
3. In Infynitae, each commit triggers damage re-evaluation, stage accounting, and scale passes, inducing high background CPU churn and frame-scheduling tail latency.

## Unverified Hypotheses
- Hypothesized that clients can be categorized by activity (active media vs idle productivity) or gated via systemd cgroup freeze signals.

## Performance & Correctness Implications
- Unbounded heartbeats turn off-screen power savings into continuous background wakeups.

## Limitations & Counterexamples
- Complete starvation (zero callbacks) breaks certain video/game engines upon un-minimizing. The heartbeat is necessary, but must be **strictly gated and batch-staggered**.

## Reproduction Instructions
Simulate 1,000 background clients committing at 1 Hz and measure compositor event-loop dispatch time.

## Links & Citations
- Source Reference: `driftwm/src/render/lifecycle.rs`
- Related Research: [KNOW-RES-005: DXGI Waitable Swapchains](../research/KNOW-RES-005-msft-dxgi-waitable-frame-pacing.md)
- Independent Review: [INDEPENDENT_REVIEW.md](../experiments/KNOW-EXP-001-pull-cost-bench-reproduction.md)
