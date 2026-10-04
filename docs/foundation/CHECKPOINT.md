# Checkpoint

CURRENT_PHASE: **C — Phase C checkpoint 3A (scale/zoom, hit-testing, obligation registry) COMPLETE. STOPPED at the 3A review gate. Checkpoint 3B (C11–C16) not started.**

Date: 2026-10-04

## Baselines
- Phase A evidence: `2c8c163784e260af4aaa8c1dc05c6281b0476120`
- Phase B seal: `4a0807ed721ae257e1f4b0e5287d1dfd8c1038c1`
- Phase C checkpoint 1: `d596416cf57877116e9cfbc8cf6c6fe25c001c67`
- **Phase C checkpoint 2 (C5–C10):** `fc2dd68f843b1eb05828f4116f94e8bab4f478da` (hash recorded in `2b3184a`)
- Checkpoint 3A work below is **uncommitted** on top of `2b3184a`.

## COMPLETED_WORK (3A)
- CP2-Q1 resolved (no fullscreen override) and CP2-R1 refined into the unified scale model D-CP3A-SCALE:
  - rules S1–S9;
  - frozen properties P1–P7;
  - provisional constants H_cfg / r / σ;
  - runtime D_max.
- Evidence:
  - device limits (EC-16);
  - real-client scale response and storm cost for GTK4, kitty and Firefox (EC-17);
  - buffer-cost table (EC-18);
  - executable model with walkthroughs A–E (EC-19).
- Hit-testing D-CP3A-HIT (physical-pixel selection = presented surface). xhit: 0 mismatches over 16 fractional configurations; 12 semantic cases (EC-20).
- RC-1 validated with GTK 4.22.5 (EC-21) and reclassified.
- Complete obligation registry OB-1…OB-29 with destinations.
- Authority recheck: no new cycle.

## DECISIONS
D-CP3A-SCALE (S1–S9, P1–P7) · D-CP3A-HIT (refinement of D-B2-I9) · RC-1 disposition · registry destinations.

## PROVISIONAL (not established)
- H_cfg = 3.0, r = 1.25, σ = 0.05 (X-16)
- off-screen fallback P = 1 s (OB-8)
- optional wheel decrease-settle delay (OB-23, default off)

## FILES_CHANGED (uncommitted)
- `docs/foundation/FOUNDATION_ARCHITECTURE.md` (CP3A sections; EC-20 pixel count corrected)
- `docs/foundation/CHECKPOINT.md`
- `docs/foundation/evidence/phase-c/`: `cp3a-scaleprobe-output.txt`, `cp3a-gtk4win.py`, `cp3a-buffer-cost.txt`, `cp3a-scale-model-output.txt`, `cp3a-xhit-output.txt`, `cp3a-rc1-gtk4.py`, `cp3a-rc1-gtk4-output.txt`
- `experiments/cp3a-scale-model/model.py`
- `experiments/b8-harness/`:
  - new: `bin/scaleprobe.rs`, `bin/rc1.rs`, `bin/xhit.rs`
  - changed: `client.rs` (nested subsurfaces, region add/subtract), `server.rs` (commit buffer sizes)

## LIVE_SYSTEM_TOUCHED
**No changes to the live system.** GTK4 (Python), kitty and Firefox ran against the private harness socket (Firefox with a temporary HOME/profile, `--no-remote`); all terminated. No Hyprland, configuration, package or toolchain changes.

## NEXT_ACTION
Await the user's 3A review. If approved: checkpoint 3B (C11–C16, final verification architecture, contradiction audit, Phase C gate).
