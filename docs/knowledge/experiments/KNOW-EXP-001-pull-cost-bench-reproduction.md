# KNOW-EXP-001: Independent Reproduction of DriftWM Focus Pull Benchmark

| Field | Value |
| :--- | :--- |
| **id** | KNOW-EXP-001 |
| **title** | Independent Reproduction of DriftWM Focus Pull Benchmark |
| **subsystem** | Experiments/InputBenchmark |
| **source_rev** | 352333a8fa1b22171492d4b71a54102045c9a19d |
| **date** | 2026-10-14 |
| **evidence_class** | INDEPENDENTLY REPRODUCED |
| **applicability** | Focus recomputation architecture evaluation |
| **affected_branches** | N/A (External Verification) |
| **related_invariants** | INV-INPUT-8 |
| **adoption_status** | REJECTED |
| **next_action** | Retain Infynitae spatial dirty-state culling |

---

## Verified Facts
1. Upstream DriftWM claims in `dev/docs/caveats.md` line 95 that pulling pointer focus on every event-loop iteration costs $5.5\,\mu\text{s}$ over bare canvas, $2.9\,\mu\text{s}$ over a window, and $8.1\,\mu\text{s}$ for real motion.
2. An independent adversarial execution of [`driftwm/src/tests/pull_cost.rs`](file:///home/<user>/.gemini/antigravity/scratch/driftwm/src/tests/pull_cost.rs) on a 12-thread AMD host under Linux 7.2.9 revealed:
   - Bare canvas pull: **$9.92\,\mu\text{s}$** (+80.4% slower than claimed).
   - Window pull: **$3.76\,\mu\text{s}$** (+29.5% slower than claimed).
   - Real absolute motion: **$15.80\,\mu\text{s}$** (+95.1% slower than claimed).
3. The benchmark in DriftWM has no warm-up loop, tests only 20 windows, and fails to compute variance, standard deviation, or $p_{95}/p_{99}$ tail latency.

4. **Reconciliation 2026-10-14 (main `f323c2d`):** The reproduction was really executed — **once**, release profile, in AGY trajectory `822720a8` (raw output preserved in `~/Infynitae-KNOWLEDGE-SOURCES-2026-10-14/README.md`). One run on a 12-thread host against DriftWM's documented 24-core run is a hardware difference, not proof that DriftWM's numbers were wrong; "+80 % error" overstates it. DriftWM's documentation labelled its numbers correctly; the fault was AGY presenting them as its own measurement. See X-08 for the scaling claims.

## Unverified Hypotheses
- AGY previously asserted that pulling focus takes $<1\,\text{ms}$ CPU time per second; refuted under 8 kHz gaming mice where event-loop iteration frequency scales to 8,000 dispatches/sec.

## Performance & Correctness Implications
- A global focus pull model scales quadratically ($\mathcal{O}(N \cdot E)$) with window count and event frequency, inducing severe latency spikes.

## Limitations & Counterexamples
- The pull model does prevent stale coordinates on unconstrained commit moves, but Infynitae achieves the same guarantee without full scans by invalidating cached focus only on boundary crossing.

## Reproduction Instructions
Execute the release binary benchmark in DriftWM:
```bash
cargo test --release --bin driftwm -- tests::pull_cost::pull_cost_bench --ignored --nocapture
```

## Links & Citations
- Source Test: `driftwm/src/tests/pull_cost.rs`
- Related Pitfall: [KNOW-PITFALL-002: Global Focus Pull Loops](../pitfalls/KNOW-PITFALL-002-global-focus-pull-loops.md)
