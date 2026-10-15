# INC-6 performance finding P-1 — repair record (2026-10-14)

**Status: EXPERIMENTAL, on `perf/inc-6-scale-index` (from `40ddd7d`). Not integrated, not
reviewed. INC-6 is not closed. OB-42 stays OPEN.** This file is evidence, not authority; no
sealed text changed.

## 1. Defect

At the end of every Core batch — every pan event, every client commit, every output change —
`Core::evaluate_scale_policy` visited every mapped element and, for each, looked up its scale
state with a linear search of `Core::scales` (quadratic in the number of windows), collected its
observers into a new sorted vector, and built a new ladder (about five heap allocations per
element). It undid the Core visibility remediation's property that a batch costs what it changes.
Finding: `~/Infynitae-INC6-PERFORMANCE-FINDING-2026-10-14.md`.

## 2. Repair

| Change | Where | Why it is safe |
|---|---|---|
| `ScaleStates`: the per-element scale states kept sorted by element, found by binary search | `scale.rs`, used by `lib.rs`, `protocol.rs` | Still the only store of OBJ-16. Identities are monotonic and never reused (INV-STAGE-3), so a destroyed element's entry cannot be found by a later one; destroy removes it, initial commit replaces it. |
| `Sweep`: which mapped elements a batch-end evaluation visits | `scale.rs`, `lib.rs` | Derived; never decides a hint. See §3. |
| ρ from the element's membership rows without a vector | `visibility.rs` (`observing_outputs`), `scale::compute_rho` takes any iterator | ρ is a maximum: order and repeats do not change it. |
| `Ladders`: the last ladder kept with the output scales, `r` and `H_max` it was built from | `scale.rs` | Self-checking: every lookup compares all three and rebuilds on any difference; nothing has to invalidate it. |
| The list of elements a pass visits is a reused buffer | `Sweep` | Cleared, not freed (C14.3's own direction). |

## 3. Why a scoped evaluation is exact

`ScaleState::evaluate` with unchanged inputs (observers, their scales and zooms, the element's
tree, the outputs, the configuration) changes nothing, except:

1. its epoch is closed and the batch belongs to a continuous interaction: the epoch opens;
2. its epoch is closed, SC-E held its hint away from its target, and the batch is not
   client-caused: the hint moves.

So a batch visits the elements it marks — the committed element; the elements a moved camera's
outputs observed before or observe after (no other element's ρ can change); an element a step
touched outside an evaluation (`start_scale_interaction`) — and runs a full pass when:

- `all`: an input common to every element changed (output set or descriptor — the full
  visibility pass; `set_scale_config`; `set_renderer_device_max`);
- the batch belongs to a continuous interaction and `closed` says some mapped element may have a
  closed epoch (set by every settlement path, by an evaluation that leaves an epoch closed;
  cleared only by a full pass, which computes it exactly);
- the batch is neither client-caused nor part of a continuous interaction (an SC-E hold could
  move; a held element has a closed epoch, so inside a drag `closed` covers it). No such batch
  exists on this line; the rule keeps a future one correct.

Settlement paths leave `h = t(ρ)` and only close epochs; P7 at a `D_max` change sets `all`.

## 4. Tests and controls

| Test | Tier | What it shows |
|---|---|---|
| `scoped_scale_evaluation_equals_the_full_reference` | T0 | 40 random sequences × 400 operations (outputs connected, removed, reconfigured; elements created, mapped, committed, tree-changed, unmapped, remapped, destroyed; a drag pressed, moved and released as separate operations; discrete and continuous steps; ticks on an advancing clock; configuration and `D_max` changes; a camera batch outside a drag) against a Core that evaluates every element at every batch: identical scale state for every element and identical effects after every operation. Coverage asserted: ≈10,000 scoped and ≈2,200 full passes, ≈14,000 hint emissions, ≈1,570 operations with an SC-E hold, ≈800 drag epoch openings, ≈390 P7 decreases. |
| `the_ladder_memo_equals_a_fresh_ladder` | T0 | 3,000 lookups changing one input at a time, including an output list growing behind the same leading scales. |
| `steady_state_batches_do_not_evaluate_every_element` | T0 | 200 windows: after a drag's first step, its steps and all commits are scoped; an output change is full. |
| `t0_hot_path_allocations` (OB-42 ratchet) | T0 | §6. |

Seeded defects run while developing (all detected): a step not marked; a commit not marking its
element; a camera scope from new rows only, from old rows only, or empty; tick, release or
settle-all not raising `closed`; configuration, `D_max` or output change not global; `closed`
ignored or never recorded or forgotten by a scoped pass; a non-client batch outside a drag
scoped; a scoped pass visiting unmapped elements; the ladder memo ignoring `H_max`, `r`, the
output scales or the output count. ρ read from one row only is caught at T1 by `t1_scale`
(5 tests). Nine of them, one for `steady_state_batches_do_not_evaluate_every_element` (every batch a full
pass) and two for the OB-42 ratchet (§6), are the 12 new controls in
`tools/verify-test-efficacy`.

## 5. Measurements

Release build, `core_bench`'s scenario (N windows of a root and two subsurfaces on a 700 px
grid; one 2560×1440 output looking at the first ones), a scratch harness outside the repository
that records each operation's duration and its heap allocations (counting global allocator), 20
warm-up calls, then 2,000 (N < 1,000) or 300 timed calls, pinned to one CPU, same machine and
session. Model only: no Smithay, no renderer, no hardware; not a C14 baseline (OB-44). Fixed =
this branch at the commit of this record.

| operation | N | main median / p99 µs | INC-6 40ddd7d median / p99 µs | fixed median / p99 µs | allocations main / INC-6 / fixed |
|---|---|---|---|---|---|
| pan | 1 | 0.30 / 0.35 | 0.52 / 0.57 | 0.32 / 0.35 | 5 / 13 / 5 |
| pan | 10 | 0.89 / 0.97 | 3.44 / 3.62 | 1.00 / 1.07 | 15 / 72 / 15 |
| pan | 100 | 1.77 / 1.84 | 28.87 / 32.98 | 1.86 / 1.94 | 15 / 525 / 15 |
| pan | 500 | 5.64 / 6.55 | 202.91 / 207.68 | 5.43 / 6.25 | 15 / 2527 / 15 |
| pan | 1000 | 10.49 / 11.41 | 537.12 / 554.73 | 9.84 / 12.93 | 15 / 5028 / 15 |
| pan | 3000 | 29.76 / 34.00 | 3224.78 / 3238.14 | 27.41 / 30.84 | 15 / 15030 / 15 |
| pan-cross | 1 | 0.30 / 0.34 | 0.52 / 0.56 | 0.31 / 0.35 | 5 / 13 / 5 |
| pan-cross | 10 | 1.01 / 1.28 | 3.57 / 3.78 | 1.13 / 1.24 | 18.0 (max 20) / 75.5 (max 78) / 18.0 (max 20) |
| pan-cross | 100 | 1.88 / 2.01 | 29.01 / 33.23 | 2.02 / 2.17 | 18.0 (max 20) / 528.5 (max 531) / 18.0 (max 20) |
| pan-cross | 500 | 5.76 / 6.62 | 203.04 / 207.67 | 5.63 / 5.80 | 18.0 (max 20) / 2530.5 (max 2533) / 18.0 (max 20) |
| pan-cross | 1000 | 10.63 / 11.57 | 548.72 / 575.43 | 10.04 / 10.96 | 18.0 (max 20) / 5031.5 (max 5034) / 18.0 (max 20) |
| pan-cross | 3000 | 29.93 / 34.26 | 3226.69 / 3245.54 | 27.75 / 31.44 | 18.0 (max 20) / 15033.5 (max 15036) / 18.0 (max 20) |
| pan-first | 1 | 0.32 / 0.36 | 0.57 / 0.63 | 0.34 / 0.37 | 5 / 13 / 5 |
| pan-first | 10 | 0.92 / 1.00 | 3.48 / 3.70 | 1.27 / 1.34 | 15 / 72 / 15 |
| pan-first | 100 | 1.80 / 1.88 | 28.79 / 32.95 | 6.73 / 7.62 | 15 / 525 / 15 |
| pan-first | 500 | 5.66 / 6.58 | 204.16 / 209.12 | 35.69 / 40.10 | 15 / 2527 / 15 |
| pan-first | 1000 | 10.47 / 11.32 | 545.89 / 553.70 | 78.85 / 84.61 | 15 / 5028 / 15 |
| pan-first | 3000 | 29.79 / 34.56 | 3223.25 / 3242.68 | 271.70 / 277.57 | 15 / 15030 / 15 |
| pan-release | 1 | 0.33 / 0.39 | 0.54 / 0.59 | 0.33 / 0.37 | 7 / 13 / 7 |
| pan-release | 10 | 0.94 / 1.03 | 3.14 / 3.31 | 0.97 / 1.06 | 17 / 68 / 17 |
| pan-release | 100 | 3.57 / 3.68 | 25.95 / 30.14 | 5.58 / 6.48 | 17 / 518 / 17 |
| pan-release | 500 | 17.58 / 21.52 | 131.53 / 137.09 | 29.96 / 34.17 | 17 / 2518 / 17 |
| pan-release | 1000 | 36.96 / 41.39 | 263.92 / 274.72 | 64.86 / 71.21 | 17 / 5018 / 17 |
| pan-release | 3000 | 128.33 / 133.88 | 811.55 / 820.14 | 228.82 / 229.94 | 17 / 15018 / 17 |
| commit | 1 | 0.37 / 0.42 | 0.65 / 0.70 | 0.37 / 0.39 | 10 / 18 / 10 |
| commit | 10 | 0.39 / 0.61 | 3.00 / 3.13 | 0.41 / 0.44 | 10 / 67 / 10 |
| commit | 100 | 0.40 / 0.46 | 27.62 / 31.87 | 0.44 / 0.48 | 10 / 520 / 10 |
| commit | 500 | 0.41 / 0.47 | 198.72 / 203.26 | 0.46 / 0.50 | 10 / 2522 / 10 |
| commit | 1000 | 0.42 / 0.48 | 530.14 / 548.89 | 0.48 / 0.52 | 10 / 5023 / 10 |
| commit | 3000 | 0.44 / 0.50 | 3218.15 / 3240.35 | 0.51 / 0.54 | 10 / 15025 / 10 |
| commit-far | 1 | 0.38 / 0.44 | 0.66 / 0.71 | 0.37 / 0.40 | 10 / 18 / 10 |
| commit-far | 10 | 0.22 / 0.24 | 2.81 / 2.92 | 0.24 / 0.26 | 5 / 62 / 5 |
| commit-far | 100 | 0.24 / 0.25 | 27.44 / 31.62 | 0.28 / 0.31 | 5 / 515 / 5 |
| commit-far | 500 | 0.26 / 0.28 | 198.66 / 203.31 | 0.30 / 0.33 | 5 / 2517 / 5 |
| commit-far | 1000 | 0.27 / 0.37 | 529.44 / 545.07 | 0.32 / 0.35 | 5 / 5018 / 5 |
| commit-far | 3000 | 0.26 / 0.27 | 3216.30 / 3233.36 | 0.34 / 0.36 | 5 / 15020 / 5 |
| remap | 1 | 0.73 / 0.81 | 1.04 / 1.12 | 0.69 / 0.76 | 22 / 32 / 22 |
| remap | 10 | 0.94 / 1.05 | 8.16 / 9.38 | 0.93 / 1.01 | 25 / 184 / 25 |
| remap | 100 | 1.91 / 2.02 | 83.76 / 89.22 | 1.81 / 1.90 | 28 / 1546 / 28 |
| remap | 500 | 7.08 / 8.10 | 561.10 / 597.19 | 6.49 / 7.40 | 30 / 7554 / 30 |
| remap | 1000 | 14.91 / 18.81 | 1609.57 / 1649.02 | 13.73 / 17.35 | 31 / 15058 / 31 |
| remap | 3000 | 47.66 / 52.33 | 9629.82 / 9675.08 | 47.36 / 51.01 | 33 / 45066 / 33 |
| churn | 1 | 0.70 / 0.80 | 2.11 / 2.24 | 0.80 / 0.89 | 20 / 58 / 22 |
| churn | 10 | 0.90 / 0.98 | 9.26 / 10.98 | 1.01 / 1.07 | 22 / 207 / 24 |
| churn | 100 | 1.79 / 1.92 | 86.01 / 92.10 | 1.86 / 1.94 | 25 / 1569 / 27 |
| churn | 500 | 7.16 / 8.19 | 576.00 / 603.64 | 6.69 / 7.63 | 27 / 7577 / 29 |
| churn | 1000 | 15.24 / 19.00 | 1620.79 / 1661.70 | 14.12 / 17.55 | 28 / 15081 / 30 |
| churn | 3000 | 48.17 / 52.65 | 9656.07 / 9717.00 | 48.53 / 52.74 | 30 / 45089 / 32 |
| motion | 1 | 0.09 / 0.10 | 0.08 / 0.09 | 0.08 / 0.10 | 1 / 1 / 1 |
| motion | 10 | 0.09 / 0.10 | 0.09 / 0.10 | 0.09 / 0.10 | 1 / 1 / 1 |
| motion | 100 | 0.11 / 0.11 | 0.10 / 0.10 | 0.10 / 0.11 | 1 / 1 / 1 |
| motion | 500 | 0.11 / 0.12 | 0.10 / 0.11 | 0.10 / 0.12 | 1 / 1 / 1 |
| motion | 1000 | 0.11 / 0.13 | 0.10 / 0.11 | 0.10 / 0.11 | 1 / 1 / 1 |
| motion | 3000 | 0.11 / 0.11 | 0.11 / 0.12 | 0.11 / 0.12 | 1 / 1 / 1 |
| render | 1 | 0.27 / 0.37 | 0.23 / 0.27 | 0.22 / 0.26 | 5 / 5 / 5 |
| render | 10 | 0.86 / 0.95 | 0.72 / 0.80 | 0.73 / 0.80 | 15 / 15 / 15 |
| render | 100 | 3.49 / 3.59 | 3.38 / 3.48 | 3.33 / 3.40 | 15 / 15 / 15 |
| render | 500 | 17.53 / 21.88 | 17.22 / 20.74 | 17.31 / 20.71 | 15 / 15 / 15 |
| render | 1000 | 37.04 / 41.71 | 36.17 / 40.27 | 36.56 / 40.99 | 15 / 15 / 15 |
| render | 3000 | 123.35 / 133.64 | 124.14 / 129.15 | 124.12 / 129.32 | 15 / 15 / 15 |
| tick | 1 | 0.07 / 0.07 | 0.06 / 0.08 | 0.06 / 0.08 | 2 / 2 / 2 |
| tick | 10 | 0.15 / 0.16 | 0.14 / 0.16 | 0.13 / 0.14 | 4 / 4 / 4 |
| tick | 100 | 0.15 / 0.16 | 0.14 / 0.16 | 0.14 / 0.16 | 4 / 4 / 4 |
| tick | 500 | 0.15 / 0.17 | 0.14 / 0.16 | 0.14 / 0.15 | 4 / 4 / 4 |
| tick | 1000 | 0.15 / 0.17 | 0.14 / 0.16 | 0.13 / 0.15 | 4 / 4 / 4 |
| tick | 3000 | 0.14 / 0.17 | 0.14 / 0.16 | 0.14 / 0.15 | 4 / 4 / 4 |
| hotplug | 1 | 1.04 / 1.15 | 1.96 / 2.10 | 1.48 / 2.46 | 29 / 57 / 39 |
| hotplug | 10 | 2.62 / 2.73 | 11.81 / 14.96 | 3.83 / 4.02 | 54 / 270 / 64 |
| hotplug | 100 | 5.40 / 5.52 | 102.19 / 107.34 | 19.56 / 22.76 | 54 / 2076 / 64 |
| hotplug | 500 | 17.81 / 21.47 | 612.14 / 642.51 | 102.94 / 107.53 | 54 / 10080 / 64 |
| hotplug | 1000 | 33.18 / 37.50 | 1548.59 / 1581.53 | 224.01 / 229.50 | 54 / 20082 / 64 |
| hotplug | 3000 | 86.17 / 90.32 | 7812.70 / 7841.81 | 768.58 / 779.10 | 54 / 60086 / 64 |
| idle-deadline | 1 | — | 0.02 / 0.03 | 0.02 / 0.03 | — / 0 / 0 |
| idle-deadline | 10 | — | 0.02 / 0.03 | 0.02 / 0.03 | — / 0 / 0 |
| idle-deadline | 100 | — | 0.04 / 0.05 | 0.04 / 0.05 | — / 0 / 0 |
| idle-deadline | 500 | — | 0.16 / 0.16 | 0.16 / 0.16 | — / 0 / 0 |
| idle-deadline | 1000 | — | 0.52 / 0.52 | 0.52 / 0.52 | — / 0 / 0 |
| idle-deadline | 3000 | — | 1.50 / 1.50 | 1.50 / 1.51 | — / 0 / 0 |

`pan-first` is the first motion of a drag: it opens a CameraPan epoch on every element, which is
the M7 behaviour as implemented, so it visits every element once per drag. `pan-release`
settles every open epoch. `hotplug` changes the set of output scales and so every element's
ladder. These are the operations that legitimately visit every window; they are linear (about
60–90 ns per element). `churn` (create, map, destroy) allocates 2 more than `main`: the initial
hint's prediction (views) and the state's insertion.

Complexity, per event, n mapped elements, v elements observed by the moved views, k rows:

| Path | `40ddd7d` | fixed |
|---|---|---|
| pan step, steady | O(n²) + 5n allocations | O(v log n) scale work + the membership pass (O(n), as on `main`) |
| commit | O(n²) | O(log n) |
| first drag step, release, output change | O(n²) | O(n log n) |
| scale-state lookup | O(n) | O(log n) |

## 6. OB-42

The authorized test-class counting allocator is `crates/infynitae-core/tests/t0_hot_path_allocations.rs`
(inventory U-002…U-004). It is a **ratchet, not the contract**: OB-42's criterion is zero
allocations after warm-up, which needs Core entries that write into caller-owned buffers
(16 entries return `Vec<Effect>`); no branch has that. The test asserts, at 10, 100 and 1,000
windows: no hot path above its recorded count; the same count whatever the number of windows;
the same count each time an event repeats. Counts (debug build, per event): pan step 15,
crossing pan step 20, routed motion 1, key press and release 2, commit 9, off-view commit 4,
render list 15, frame tick 4 — equal to `main` at `ccb82c5` measured with the same file. The same
file fails on `40ddd7d` (pan step 72 at 10 windows, 5,028 at 1,000).

Gate change for it (`enforcement/source-rules`): Core's integration tests are the one place of a
`forbid` crate where a listed `test` site is accepted; Core's library, examples, benches and
build script and all of `infynitae-ipc` still fail for any site, listed or not. Six new seeded
controls in `tools/verify-architecture` (one positive, five negative: unlisted site in a Core
test, production-class row for it, listed site in a Core example, listed site in a Core library
module named `tests`, listed site in an ipc test). **Reading for review:** INV-ARCH-2 says
"`unsafe`: forbidden in core and ipc"; this reads "core" as Core's library (the scope of its
`#![forbid(unsafe_code)]`), not the separate integration-test crates, as the owner's decision of
2026-10-14 to approve the counting allocator in a Core integration test implies. If the owner
reads INV-ARCH-2 to cover Core's integration tests, the test moves to a backend crate's tests
and the gate change is reverted; nothing else depends on it.

OB-42: **OPEN**. Not met; not moved.
