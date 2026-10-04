# Checkpoint

CURRENT_PHASE: **C — Phase C checkpoint 2 (C5–C10) COMPLETE. STOPPED at the checkpoint 2 review gate. C11 not started.**

Date: 2026-10-04

## Baselines
- Phase A evidence: `2c8c163784e260af4aaa8c1dc05c6281b0476120`
- Phase B seal: `4a0807ed721ae257e1f4b0e5287d1dfd8c1038c1`
- Phase C checkpoint 1 (C1–C4): `d596416cf57877116e9cfbc8cf6c6fe25c001c67` (hash recorded in `03448ad`)
- **Phase C checkpoint 2 (C5–C10):** `fc2dd68f843b1eb05828f4116f94e8bab4f478da`

## COMPLETED_WORK (checkpoint 2, in the mandated order)
1. X-10 keyboard/modifier ownership: 9 scenarios PASS; negative control FAILS as predicted (stuck modifier without K3). K1–K9 not falsified.
2. Surface-tree snapshot boundary: public API only; dispatch-safe inside `commit`; equals ground truth. X-15 render order and positions PASS. Hit-test agreement with Smithay fully explained by two new Smithay quirks (W-SM-11, W-SM-12).
3. C5 fullscreen vs viewport-fit, with both worked multi-output cases.
4. C6 xwayland-satellite (experiment: ordinary toplevel; X11 move request ineffective; crash path clean; W-SM-13 found).
5. C7 shell boundary, plus C-NB-2 hardening (ports `!Send`; protocol state asserted `Send` → E0277; real Smithay state is `Send`).
6. C8 failure matrix. 7. C9 unsafe/patch policy. 8. C10 observability. 9. Contradiction pass.

## DECISIONS MADE
- D-CP2-HIT (own snapshot hit-test; `Window::surface_under` not used)
- D-C5-FIT (viewport-fit = camera operation in `navigation`)
- D-C5-FS (fullscreen = output presentation; never a camera)
- D-C6 (satellite, narrow)
- D-C7 (`infynitae-ipc` crate, semantic commands, generations)
- D-C8 (failure classes; rebuild derived state; invariant violation terminates)
- C9 policy
- D-C10

## AWAITING USER DECISION (not adopted)
- **CP2-Q1:** preferred scale of a fullscreen element (keep MaxGesture as frozen vs pin to the fullscreen output).
- **CP2-R1:** generalise MaxGesture to the effective scale `k = zoom·scale` (identical at zoom 1; matters for viewport-fit).

## FILES_CHANGED (uncommitted)
- `docs/foundation/FOUNDATION_ARCHITECTURE.md` (CP2 sections)
- `docs/foundation/CHECKPOINT.md`
- `docs/foundation/evidence/phase-c/`: `x10-keyboard-output.txt`, `xsnap-x15-output.txt`, `c6-xwayland-satellite-output.txt`, `c6-sat.sh`, `c1-authority-spike-verify.txt` (updated)
- `experiments/b8-harness/`:
  - new: `src/keys.rs`, `src/snapshot.rs`, `src/bin/{x10,xsnap,sendcheck}.rs`
  - changed: `client.rs` (wl_keyboard + checker, region/subsurface helpers, destroy helper), `server.rs` (commit-time snapshot, W-SM-13 overrides, lifecycle logging), `realclient.rs` (element log)
- `experiments/c1-authority-spike/` (`!Send` ports, Send assertion, port-in-state probe, verify.sh matching)

## LIVE_SYSTEM_TOUCHED
**No changes to the live system.**
- The C6 run started xwayland-satellite, Xwayland and an X11 kitty against the private harness socket on X display `:97`; all terminated and no files were left.
- `/tmp/.X11-unix/X25` is stale from Prototype 01's session, not this work.
- No Hyprland, configuration or package changes; Clippy was **not** installed (the toolchain was left unchanged).

## NEXT_ACTION
Await the user's checkpoint 2 review (including CP2-Q1 and CP2-R1). If approved: Phase C checkpoint 3 (C11–C16).
