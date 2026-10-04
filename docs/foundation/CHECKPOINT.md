# Checkpoint

CURRENT_PHASE: **B — COMPLETE and SEALED (review gate cleared by the user 2026-10-04).** The seal commit is recorded in the next commit (a commit cannot contain its own hash).
The carry-forward ledger (5 categories) is at the end of FOUNDATION_BLOCKERS.md.

Date: 2026-10-04

## Baselines
- Phase A evidence: `2c8c163784e260af4aaa8c1dc05c6281b0476120`
- Phase B checkpoint 1 (B1–B6): `9f3f5d4`
- Phase B checkpoint 2 (B7–B9): `90992b839af1bdbe07d128009d8b7460e21fcc88`
- Phase B checkpoint 3 (B10–B13) + seal: this commit (hash recorded in the following commit)

## COMPLETED_WORK (checkpoint 3)
- Harness additions, kept minimal:
  - pointer, relative-pointer and constraints bindings in the probe;
  - constraints, relative-pointer and presentation globals in the server;
  - `input.rs`: the ownership model under test;
  - subsurface support;
  - `renderer_test` feature for X-6.
- Experiments: X-8 (9 cases, 48 checks PASS), X-6 (0 / 134,478 edge mismatches), X-9 (PASS). SC1–SC4/X5 re-run after the harness changes: identical output.
- Written to FOUNDATION_BLOCKERS.md: B10 (input/grab/focus/activation), B11 (pan semantics), B12 (backend boundary + Q-07 Smithay contract), B13 (adversarial audit), the final contradiction pass, and the gate evaluation.

## DECISIONS MADE (checkpoint 3)
- D-B10-INTAKE (ownership resolved before dispatch; acquisition only with no client-owned button)
- D-B10-KBD (keyboard focus changes only by explicit action or target loss)
- D-B10-ACTIVATION (no protocol path to cameras; token provenance)
- D-B11-PAN / D-B11-FOCUS (KeepFocus)
- D-B12-PIN (exact pin, locked, features, protocol versions, patch policy, upgrade + re-run suite)
- Kick extended to input delivery
- Locked pointer authoritative state `(surface, Canvas lock point)`
- W-SM-4a ordering rule

## FILES_CHANGED (uncommitted)
- `docs/foundation/FOUNDATION_BLOCKERS.md`
- `docs/foundation/CHECKPOINT.md`
- `docs/foundation/evidence/phase-b/x6-edges-output.txt`, `x8-pan-input-output.txt`, `x9-presentation-output.txt` (new)
- `experiments/b8-harness/` (`input.rs`, `bin/x6.rs`, `bin/x8.rs`, `bin/x9.rs`, client/server/lockstep additions, Cargo.toml features)

## OPEN ITEMS CARRIED FORWARD (none is an unresolved Level A blocker)
- Conditional resolutions with named validations:
  - NB-1/NB-2 → X-13
  - keys → X-10
  - output removal + popup-grab dismissal → X-11
  - W-SM-4b → X-12
  - activation authority enforcement → X-14
- Deferred: X-7 (games/network off-screen; decides fallback P), RB-10 (Smithay oneshot-constraint defect; patch policy).

## SEAL ADDITIONS
- X-12 reproducer for W-SM-4b (defect reproduced at the client; no fork, vendor or patch made).
- Carry-forward ledger.

## NEXT_ACTION
Phase C checkpoint 1 (C1–C4) as authorized.

## LIVE_SYSTEM_TOUCHED
**No.** Checkpoint 3 experiments were in-process lockstep runs (socketpair, virtual time). No real clients were launched in this checkpoint. Nothing in `~/infynitae`, `~/infynitae-native-prototype`, the Hyprland configuration or system packages was changed.
