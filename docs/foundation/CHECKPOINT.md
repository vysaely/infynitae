# Checkpoint

CURRENT_PHASE: **B — Resolve Architecture Blockers: atomic checkpoint 1 (B1–B6) complete. B7–B13 not started.**
Phase B is NOT complete. Do not advance to Phase C.

Date: 2026-10-04

## Baseline
Phase A evidence baseline commit: `2c8c163784e260af4aaa8c1dc05c6281b0476120` (branch `main`).
Phase B work below is **uncommitted** on top of it.

## COMPLETED_WORK (Phase B, checkpoint 1)
- B1: numeric model decided (Q16 i64, ±2³⁶ bound, integral anchors, rounding points, lattice edge rule). Experiment E1–E9.
- B2: coordinate domains, transformation table T1–T10, anchor = window-geometry origin, pointer chain with Canvas as Smithay global space, I9 restated, type encoding. `Space` question answered (not used), with experiment S1–S7.
- B3: object graph and negative ownership.
- B4: identity (`NonZeroU64` monotonic, never reused; `(InstanceId, ElementId)` across IPC).
- B5: lifecycle states and transition table, difficult cases.
- B6: popups/subsurfaces (no stored position; constraint boundary policy; no pan repositioning; grab and lifetime rules).

## FILES_CHANGED (uncommitted)
- `docs/foundation/FOUNDATION_BLOCKERS.md` (new)
- `docs/foundation/CHECKPOINT.md` (this file)
- `docs/foundation/evidence/phase-b/b1-numeric-output.txt`, `b2-space-output.txt` (new)
- `experiments/b1-numeric/` (no deps), `experiments/b2-space/` (smithay =0.7.0, Cargo.lock copied from Prototype 01; builds offline)

## EVIDENCE_CREATED
EB-1…EB-7 (see FOUNDATION_BLOCKERS §0). Re-run: `cargo run --release --offline` in `experiments/b1-numeric`, `cargo run --offline` in `experiments/b2-space`.

## DECISIONS MADE
D-B1, D-B2-ANCHOR, D-B2-SPACE, D-B2-PTR-SPACE, D-B2-PTR-STATE, D-B2-I9, D-B4, D-B5-REMAP, D-B6-BOUNDARY, D-B6-NO-PAN-REPOSITION. B6.3's grab-dismissal rule is provisional until B10.

## OPEN_BLOCKERS
Q-01 (off-screen callbacks), Q-02 (`suspended` vs I3), Q-06 (preferred scale), Q-07 (Smithay pin, Level B), Q-09 (restated I3). Q-04 is conditional on B8 and EB-H1.
New: EB-H1 (popup enter/leave offset for CSD parents), RB-1…RB-4, X-6.

## NEXT_ACTION
Phase B checkpoint 2 (B7–B9):
1. Build a minimal headless/nested experiment harness: Smithay compositor with ≥2 virtual outputs (different scales) plus a small Wayland test client that logs every received event. It needs no live-session access: a headless backend, or winit nested under the host.
2. Run X-3 (two outputs 1.0/1.5) and X-5 (popup enter/leave), then X-1 (pan trace), then X-2 (off-screen clients: EGL swap-interval-1, Vulkan FIFO, Firefox, mpv).
3. Write B7 (camera/output model), B8 (mandatory worked example), B9 (visibility/off-screen state machine; resolve Q-01, Q-02, Q-09).

Then checkpoint 3: B10–B13 (input/activation, pan, backend boundary, blocker report finalization and Phase B gate).

## LIVE_SYSTEM_TOUCHED
**No.** No compositor run, nested or otherwise. No change to `~/infynitae` or `~/infynitae-native-prototype`. Experiments are pure computation (Smithay `Space` with dummy elements; no Wayland socket opened).
