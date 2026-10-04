# Checkpoint

CURRENT_PHASE: **B — Resolve Architecture Blockers: checkpoint 2 (B7–B9) complete. B10–B13 not started.**
Phase B is NOT complete. Do not advance to Phase C.

Date: 2026-10-04

## Baselines
- Phase A evidence: `2c8c163784e260af4aaa8c1dc05c6281b0476120`
- Phase B checkpoint 1 (B1–B6): `9f3f5d4`
- Checkpoint 2 work below is **uncommitted** on top of `9f3f5d4`.

## COMPLETED_WORK (checkpoint 2)
- Minimal experiment harness `experiments/b8-harness/`:
  - Smithay 0.7.0 server (no renderer) plus a recording `wayland-client` probe, lockstep on a virtual vblank clock (`bin/scenarios`);
  - a real-time variant for real clients on a private socket (`bin/realclient`).
- Experiments run: SC1 (B8 worked example), SC2 (preferred-scale policies), SC3 (frame-callback liveness), SC4 (suspension and configure amplification), X5 (popup membership), X-2 (vkcube, kitty, Firefox off-screen: Starve, Throttle, kick).
- B7 camera/output model, B8 multi-output semantics, B9 visibility/off-screen state machine and I3′, all written to FOUNDATION_BLOCKERS.md.

## DECISIONS MADE (checkpoint 2)
D-B7-CENTER, D-B8-SCALE (MaxGesture; ⌈⌉ integer), D-B8-CLOCK (highest refresh), D-B8-ENTER (own per-surface membership), D-B9-FRAMES (withhold + protocol kick + finite fallback P; P = 1 s provisional), D-B9-SUSPEND (never from camera visibility), I3′.
Q-01, Q-02, Q-04, Q-06, Q-09 resolved. Q-07 open.

## FILES_CHANGED (uncommitted)
- `docs/foundation/FOUNDATION_BLOCKERS.md`
- `docs/foundation/CHECKPOINT.md`
- `docs/foundation/evidence/phase-b/b8-scenarios-output.txt`, `x2-realclients-output.txt` (new)
- `experiments/b8-harness/` (new)

## EVIDENCE_CREATED
EB-8 … EB-12 (FOUNDATION_BLOCKERS B8.1). Re-run in `experiments/b8-harness`:
- `cargo run --offline --release --bin scenarios`
- `target/release/realclient <private runtime dir> <starve|throttle:MS>[+kick] <vis_s> <off_s> <ret_s> -- <client>`

## OPEN_BLOCKERS / PENDING
- Q-07 (Smithay pin, B12).
- X-6 (subsurface edge rounding), X-8 (pointer leakage during pan, B11), X-9 (presentation feedback, T1).
- X-7 (off-screen game and network client) deferred to T5. It decides P's default, not the architecture.

## NEXT_ACTION
Phase B checkpoint 3: B10 (input/grab/activation model, including xdg-activation policy), B11 (camera pan input semantics, X-8; the harness needs a minimal seat/pointer injection path), B12 (backend boundary, Q-07 pin strategy), B13 (finalise blocker report and Phase B gate).

## LIVE_SYSTEM_TOUCHED
**No changes to the live system.**
- Real-client tests ran vkcube, kitty and Firefox as child processes against the harness on a private socket in the scratch directory. DISPLAY and HYPRLAND_* were removed from their environment, so they could not reach the running Hyprland session.
- Firefox used a temporary HOME/profile in the scratch directory with `--no-remote`.
- No changes to `~/infynitae`, `~/infynitae-native-prototype`, the Hyprland configuration, or installed packages.
