# KNOW-EXP-002: OB-42 Global Counting-Allocator Ratchet

| Field | Value |
| :--- | :--- |
| **id** | KNOW-EXP-002 |
| **title** | OB-42 Global Counting-Allocator Ratchet |
| **subsystem** | Experiments/MemoryAllocation |
| **source_rev** | f1ef262 / 740b879 |
| **date** | 2026-10-14 |
| **evidence_class** | EXPERIMENTALLY VERIFIED |
| **applicability** | Core steady-state allocation ratchet |
| **affected_branches** | perf/inc-6-scale-index, experimental/combined-perf-audit-2026-10-14 |
| **related_invariants** | INV-ALLOC-1, OB-42, U-002..U-004 |
| **adoption_status** | IMPLEMENTED |
| **next_action** | Drive counts toward zero using caller-owned buffer vectors |

---

## Verified Facts
1. **OB-42 Contract:** Demands zero heap allocation on steady-state hot paths.
2. **The Test-Class Unsafe Allocator (Decision of 2026-10-14):** Implemented in `crates/infynitae-core/tests/t0_hot_path_allocations.rs`, a custom `Counting` global allocator intercepts `alloc`/`dealloc` using a thread-local counter (`COUNT`), forwarding all operations to `System` (UNSAFE_INVENTORY U-002..U-004). *2026-10-15 (INC-6 final-resolution branch, owner direction):* moved unchanged to `crates/infynitae-backend-headless/tests/t0_hot_path_allocations.rs`; Core's integration-test exception removed; C12.F9 not adopted.
3. **Ratchet Invariants Verified:** At $N=10$, $N=100$, and $N=1,000$ windows:
   - `Pan`: $\le 15$ allocations.
   - `PanCrossing`: $\le 20$ allocations.
   - `Route`: $\le 1$ allocation.
   - `Key`: $\le 2$ allocations.
   - `Commit`: $\le 9$ allocations.
   - `CommitOffView`: $\le 4$ allocations.
   - `RenderList`: $\le 15$ allocations.
   - `FrameTick`: $\le 4$ allocations.
4. **Independence of Window Count:** The allocation counts are **strictly identical** whether there are 10, 100, or 1,000 windows on the Canvas, proving the absence of per-window heap allocation.

## Unverified Hypotheses
- Hypothesized that replacing `Vec<Effect>` returns with pre-allocated scratch ring buffers will achieve the final OB-42 contract of 0 allocations.

## Performance & Correctness Implications
- Prevents memory fragmentation, heap lock contention, and garbage collection latency spikes during interactive panning.

## Limitations & Counterexamples
- This was a ratchet, not yet the final contract. *2026-10-16 (INC-6 exit review r2, correction of interpretation):* since `09171b3` (final-resolution branch, carried by `candidate/inc-6-exit-2026-10-16` and its r2) the file is the contract itself: it asserts **0** allocations per event on all nine measured hot paths (Pan, PanCrossing, Route, Key, Commit, CommitOffView, RenderList, FrameTick, PresentedCommit) at 10, 100 and 1,000 windows. The bounds in fact 3 are history (`f1ef262`), not the current criterion. What it does not measure: frees (not counted by design), allocations on other threads (none: Core is single-threaded), and adapter or backend code outside Core's entries. OB-42's closure remains the owner's decision at the INC-6 exit.

## Reproduction Instructions
Execute the counting-allocator ratchet test:
```bash
cargo test -p infynitae-core --test t0_hot_path_allocations   # until 2026-10-15
cargo test -p infynitae-backend-headless --test t0_hot_path_allocations   # from 2026-10-15 (INC-6 final-resolution branch)
```

## Links & Citations
- Source Test: `crates/infynitae-core/tests/t0_hot_path_allocations.rs` (*2026-10-15:* `crates/infynitae-backend-headless/tests/t0_hot_path_allocations.rs` on the INC-6 final-resolution branch)
- Governance Evidence: `docs/archive/evidence/phase-f/INC-6/PERFORMANCE_P1_REPAIR.md`
- Related Pitfall: [KNOW-PITFALL-005: Quadratic Scale-State Evaluation](../pitfalls/KNOW-PITFALL-005-quadratic-scale-state-evaluation.md)
