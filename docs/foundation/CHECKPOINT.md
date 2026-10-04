# Checkpoint

CURRENT_PHASE: **A — Establish Evidence: COMPLETE, gate met. Awaiting explicit user clearance for Phase B.**

Date: 2026-10-04

## COMPLETED_WORK
- A1: Hyprland Infynitae inspected (core plugin model, architecture doc, pointer/off-screen research, integration-audit options). Product semantics vs Hyprland-only mechanisms separated (FOUNDATION_EVIDENCE §3).
- A2: Prototype 01 read in full (1,303 LOC) and its recorded run logs examined. Ledger written.
- A3: Smithay pinned at crates.io 0.7.0 (newest stable). Relied-on APIs verified in source.
- A4: Relevant protocol text extracted from system XML (wayland 1.26.0, wayland-protocols 1.49).
- A5: Evidence matrix written (56 entries: EV-P01…23, EV-H01…07, EV-S01…12, EV-W01…14).

## FILES_CHANGED
New repository `~/infynitae-native` (git-initialized, **nothing committed**):
- `docs/foundation/FOUNDATION_EVIDENCE.md`
- `docs/foundation/PROTOTYPE_DECISION_LEDGER.md`
- `docs/foundation/CHECKPOINT.md`
- `docs/foundation/evidence/phase-a/*` (7 raw evidence files)

No changes to `~/infynitae` or `~/infynitae-native-prototype`. Running `cargo test` in the prototype only touched its `target/`.

## EVIDENCE_CREATED
`docs/foundation/evidence/phase-a/`: prototype source hashes, prototype test + verify output, prototype grep inventory, prototype runtime-log excerpts, Smithay pin, Smithay 0.7.0 source excerpts, Wayland protocol excerpts.

## DECISIONS MADE
None architectural. One procedural decision: foundation docs live in a new repo `~/infynitae-native` (production foundation), not in the prototype (frozen evidence) or the Hyprland repo.

## OPEN_BLOCKERS (inputs to Phase B, see FOUNDATION_EVIDENCE §7)
Q-01 off-screen callback policy (spec says *should avoid*, master prompt says *must not starve*) ·
Q-02 `suspended` vs I3 · Q-03 Stage anchor (surface vs geometry origin) · Q-04 popup constraint boundary ·
Q-05 Smithay pointer "global space" under multiple cameras · Q-06 single preferred scale on mixed-scale outputs ·
Q-08 Canvas numeric model (prototype provides no f64 evidence) · Q-09 restated I3 (pan is client-observable via enter/leave).
Q-07 (Smithay pin strategy) is Level B.

## NEXT_ACTION
On user clearance: begin **Phase B**, starting with B1 (Canvas numeric model) and B2 (coordinate algebra, including Q-03 and Q-05), since every other blocker depends on the coordinate domains. Experiments X-1…X-4 (FOUNDATION_EVIDENCE §8) should be scheduled within Phase B where a blocker cannot be resolved from spec plus source alone (Q-01 and Q-06 likely need X-2/X-3).

Recommended before Phase B: commit `~/infynitae-native-prototype` so evidence references a commit rather than file hashes.

## LIVE_SYSTEM_TOUCHED
**No.** No compositor started, nested or otherwise. No Hyprland config, installation, or session state touched. Network use: `cargo search` / crates.io API read for the latest Smithay version.
