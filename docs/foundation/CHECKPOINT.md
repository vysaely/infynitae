# Checkpoint

CURRENT_PHASE: **C — Phase C checkpoint 1 (C1–C4) COMPLETE. STOPPED at the checkpoint 1 review gate. C5 not started.**

Date: 2026-10-04

## Baselines
- Phase A evidence: `2c8c163784e260af4aaa8c1dc05c6281b0476120`
- Phase B checkpoint 1: `9f3f5d4`; checkpoint 2: `90992b839af1bdbe07d128009d8b7460e21fcc88`
- **Phase B seal:** `4a0807ed721ae257e1f4b0e5287d1dfd8c1038c1` (hash recorded in `5647d5f`)
- Phase C checkpoint 1 work below is **uncommitted** on top of `5647d5f`.

## COMPLETED_WORK (Phase C checkpoint 1)
- `docs/foundation/FOUNDATION_ARCHITECTURE.md`:
  - C1: 5 crates, per-crate contract, DAG, enforcement, camera-authority boundary.
  - C2: complexity budget, authority matrix, negative ownership, rejected abstractions.
  - C3: render pipeline with exact lattice integration, order, f64 entry points, I9 cache, index threshold.
  - C4: input/focus architecture; keyboard/modifier rules K1–K9.
  - Assumptions, new findings, verification, consistency check.
- `experiments/c1-authority-spike/`: zero-dependency domain stand-in plus adapter stand-in. `verify.sh` covers behaviour tests, compile_fail doctests, 4 negative probes with asserted error codes, a positive control and a dependency check. Result: **PASS**.

## DECISIONS MADE
- Crate set and DAG (C1.1–C1.3).
- Camera authority via navigation-module privacy + bound, unforgeable ports held outside `State` (C1.5).
- Authority matrix (C2).
- Render pipeline steps [1]–[7] with R7 (REFINEMENT) and explicit filters (provisional).
- D-C4-KBD K1–K9 (REFINEMENT: modifier/lock keys are never compositor-owned).

## FILES_CHANGED (uncommitted)
- `docs/foundation/FOUNDATION_ARCHITECTURE.md` (new)
- `docs/foundation/CHECKPOINT.md`
- `docs/foundation/evidence/phase-c/c1-authority-spike-verify.txt` (new)
- `experiments/c1-authority-spike/` (new)

## OPEN / CARRIED FORWARD
- Phase B ledger categories 2–5 unchanged (CR-1…CR-7, provisional list, DV-1…DV-4, W-SM-1…10).
- New: C-NB-2 (port-in-State residual risk), C-NB-3 / X-15 (render-order equivalence).
- Obligations specified for Phase F: X-10 (keyboard, 8 cases + protocol checker), X-11a/b, X-13, X-15, CR-6 properties, `verify-architecture` port of the spike.

## NEXT_ACTION
Await the user's checkpoint 1 review. If approved: Phase C checkpoint 2 (C5–C10).

## LIVE_SYSTEM_TOUCHED
**No.** Only offline builds and tests of the spike and harness. No clients launched; no changes outside `~/infynitae-native`.
