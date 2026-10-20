# INC-12 — integration onto main (2026-10-20)

Status: **EXPERIMENTAL, NOT RATIFIED.** Branch `integration/inc12-on-main-2026-10-20` from `main`
`a058657`; not `main`, not pushed. Authority: the owner's instruction of 2026-10-20 to continue the
development; the merge into `main` and INC-12's exit stay the owner's decisions.

## What was integrated

- `fix/inc-12-satellite-runtime-dir-2026-10-16` (`22ee251`): the xwayland-satellite supervisor
  (M19), xdg-activation (INV-INPUT-9; PROV-10 policy Strict by default), review rounds 1 and 2
  (REVIEW_R1/R2). Merge commit `a7d9bd7`; conflict resolution in its message (main's behaviour
  kept; INC-12 added beside it).
- Review finding **R2-4** (`eaf1adb`): the satellite is spawned as an owned child
  (`ownership::spawn_owned`, `forget_spawned` once reaped), so the session's orphan reaper never
  takes its exit status. T1 `crates/infynitae-wayland/tests/satellite_ownership.rs`; seeded
  control "INC-12 R2-4: the satellite is not an owned child" DETECTED.

## Checked interactions with main [F]

- Activation moves focus like main's other focus changes (`focus_next`, explicit focus): it
  reconciles constraints and sends no true-position event (C12.F17 rules concern pointer events).
- Attention marks are cleared on unmap and on destroy (`FocusModel::target_lost`), the Core half
  of OB-35; its shell-event half is INC-11's.
- Both backends end the satellite before the session (`drop(xwayland)` before
  `session.shutdown()`); session teardown's reaper respects owned children.
- `wp_viewporter`, which satellite 0.8.3 needs, is on main (INC-6).

## Gates on `eaf1adb` (tree `617e7a6`, unchanged before and after) [F]

docs + self-test, knowledge + self-test, architecture, fmt, check, build: PASS · tests
**610 / 0 / 4** (586 of main + 24 of INC-12; the 2 real-satellite tests are ignored without the
binary) · clippy clean · seeds 452 · efficacy **450 / 450 DETECTED** (431 of main + 18 of INC-12 +
R2-4) · envverify ALL CHECKS PASSED · NB-1 exits at 8.56 s.

## INC-12 exit criteria (REVIEW_R2 §"Exit criteria") after this integration

| # | Criterion | State |
|---|---|---|
| 1 | INC-6 ratified and integrated | **met** (main) |
| 2 | OB-40: owner decides the activation policy (Strict proposed) | **open [O]** |
| 3 | Runtime evidence: the real-satellite T1 runs on this line, and the operator session (runbook `READINESS-2026-10-16/OPERATOR_RUNBOOK.md`) | **open**: `xwayland-satellite` is not installed on this machine (`sudo pacman -S xwayland-satellite`); the operator session needs the owner |
| 4 | Orderly shutdown and process ownership | **met** (main; libc self-pipe, owner decision 2026-10-19) and R2-4 applied |
| 5 | OB-9, OB-27, OB-33 T1 parts; their closure is the owner's act at INC-12's exit review | open [O] |
