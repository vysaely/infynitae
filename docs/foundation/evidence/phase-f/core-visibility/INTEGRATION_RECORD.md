# Core visibility remediation — integration record

Status: **integrated on local canonical `main`, not ratified.** This file is evidence, not
authority. It records what was integrated at `85a74cbffd3692fa9965cbe6c68465d3972c6022`, how it
was verified, and what it does not establish.

## 1. Commits

Baseline: `db254c24bfbb6c5598fdb5c7bf4fd79ba12f4524` (ratified INC-5). Fast-forward, no merge
commit, 2026-10-13. Oldest first:

| Commit | Subject |
|---|---|
| `b5b6dc4` | bench(core): event-path cost against the number of elements on the Canvas |
| `2da6b23` | perf(core): look Stage records up by binary search |
| `be449fe` | perf(core): scope the visibility pass to what a batch invalidates |
| `24536d4` | refactor(core): one z-order walk for the render list and its readers |
| `ec11a62` | chore(verify): seed the root-commit pass and the frame tick |
| `85a74cb` | test(core): outputs changing under a held pan, against the full reference |

Changed: `crates/infynitae-core` (sources, one example) and `tools/verify-test-efficacy`.
Unchanged: the adapter, both backends, the binary, every dependency and `Cargo.lock`.

## 2. Scope

The defect (viability review F1, reproduced 2026-10-11): every commit, tree change, destroy and
pan event recomputed the membership of every element on every output, and `Stage` looked a
record up by linear scan once per element, so one event cost O(E²).

The change:

- `Stage` records are found by binary search (they are ascending by id by construction).
- A membership row (element, surface, output) depends on that element's anchor and snapshot and
  that output's view, and on nothing else. A batch that changes one element rewrites that
  element's rows; a pan rewrites the rows of the output bound to the panned camera; output
  connect, reconfigure and disconnect keep the full pass.
- Each element's bounding box is cached with its snapshot (single writer: `Stage::set_tree`) and
  used as a conservative cull before its surfaces are projected.
- `frame_tick` reads the membership table instead of walking the z-order.

No effect is added, dropped or reordered. No authority moves: Stage, CameraSet and the output
set keep their writers; the membership table stays derived (OBJ-13).

**C3.8 / INV-RENDER-4.** No spatial index is introduced: there is no grid, tree or other
structure queried by region, and a pan still tests every mapped element once. The bounding box is
a per-record cache derived from Stage. C3.8's threshold therefore remains unevaluated (it is a T4
measurement). The change nevertheless carries what INV-RENDER-4 would require of an index: it is
derived from Stage only and is differentially tested against the unscoped linear computation.

## 3. Tests

| Test (`infynitae-core`, T0) | Establishes |
|---|---|
| `scoped_visibility_equals_the_full_reference` | 40 seeds × 400 operations (output connect, disconnect, reconfigure; create, map, commit, tree change, unmap, remap, destroy; pans): after every operation the membership table, enter/leave effects and their order, every render list, frame tick and observer set equal an unscoped, unculled reference |
| `output_changes_under_a_pan_equal_the_full_reference` | the same comparison with the pan button as its own operation, 40 seeds × 600: 1,674 output changes under an active pan, 248 removals of the pan's own output, 1,011 motions consumed by an orphaned pan |
| `cull_agrees_with_the_reference_at_the_view_edges` | the cull at each view edge, five scales, four Canvas positions including ±2³⁶ |
| `lookup_agrees_with_a_linear_scan_under_churn` | the binary search under 2,000 creates and destroys |

Seeded controls added to `tools/verify-test-efficacy`: 16 (14 on the scoping, cull, extent,
frame tick and lookup; 2 on outputs changing under a pan, which the first differential test does
not detect and the second does).

Verification at `85a74cb`, before integration and again on `main` after it (separate worktree
and build): `cargo fmt --all --check`; `cargo clippy --workspace --all-targets --locked -- -D
warnings`; `cargo test --workspace --locked` — 257 passed, 0 failed, 0 ignored, 25 suites;
`tools/verify-architecture` PASS; `tools/check-foundation-docs` and `--self-test` PASS;
`tools/verify-test-efficacy` PASS, 126 controls detected and 2 judges as wanted;
`tools/verify-development-environment` PASS.

## 4. Measurements (T0 model, headless; not baselines in the sense of C14.1)

`cargo run --release -p infynitae-core --example core_bench`, one output, about six elements in
view, microseconds per call, median of five runs, 2026-10-13, Ryzen 5 7600X3D, governor
`powersave`, rustc 1.99.0, host in ordinary use.

| Elements | Commit | pan event | commit | render list | remap | frame tick | pointer motion |
|---|---|---|---|---|---|---|---|
| 100 | `db254c2` + bench | 10.9 | 10.2 | 10.2 | 32.8 | 1.14 | 0.06 |
| 100 | `85a74cb` | 1.15 | 0.30 | 2.61 | 1.40 | 0.07 | 0.06 |
| 1,000 | `db254c2` + bench | 320 | 361 | 318 | 1,250 | 10.9 | 0.07 |
| 1,000 | `85a74cb` | 8.00 | 0.32 | 29.7 | 11.7 | 0.07 | 0.07 |
| 3,000 | `db254c2` + bench | 2,624 | 2,624 | 2,641 | 10,255 | 32.7 | 0.06 |
| 3,000 | `85a74cb` | 23.5 | 0.34 | 101 | 41.4 | 0.07 | 0.07 |
| 10,000 | `85a74cb` | 78.0 | 0.36 | 390 | 161 | 0.07 | 0.08 |

Heap allocations per call (a counting allocator in a copy of the benchmark; not a repository
test): pan event 9,005 → 10 at 3,000 elements; render list 9,004 → 10; commit 9,006 → 10;
pointer motion 1 → 1; frame tick 3 → 3.

These are costs inside Core for one call. They are not frame times, presentation times or input
latency, and no such claim is made. No renderer, GPU or hardware was involved.

## 5. Limitations

- Pan, render-list construction, remap and output hotplug still grow linearly with mapped
  elements.
- INV-ALLOC-1 is not met (10 allocations per pan, commit or render-list call). OB-42 stays OPEN.
- Nothing here is a C14 baseline. OB-44 stays OPEN.
- Review was by separate AI contexts and the author; no human review is recorded.

## 6. INC-6

The frozen INC-6 candidate (`f57315c`) does not merge onto this `main` without resolution: three
conflicts (`infynitae-core/src/lib.rs`, `protocol.rs`, `tools/verify-test-efficacy`). A
resolution that keeps INC-6's rule (scale evaluation follows every pass, over all elements)
exists on `experimental/inc-6-on-core-85a74cb`, which is experimental and unratified.

With the scoped passes merged, a debug assertion that predates INC-6 (`toplevel_destroyed`: "a
destroyed element produced visibility effects") is reached by the differential test. The cause
is INC-6's, not this change's: the same six Core calls panic on the unmodified candidate. The
experimental branch carries a correction and the regression test. INC-6's scale evaluation
remains a walk over all elements, so INC-6 keeps only part of the pan saving until that
evaluation is itself scoped.
