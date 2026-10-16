# INC-6 — ratification record, prepared (2026-10-16)

Status: **PREPARED. INC-6 is NOT ratified.** This is the record the owner signs (or refuses).
Nothing here takes effect until then.

## Proposed ratification

- **Ratified commit:** the head of `governance/inc-6-exit-adoption-2026-10-16` after independent
  review (adoption commit of 2026-10-16, base `eebb5c3`).
- **Scope:** the scale policy (INV-SCALE-1…5 as amended by C12.F11–F13; F-SCALE-1…3; ladder,
  epochs, SC-E, P7, presentation holds), the device-limit path through the renderer capability
  interface (OB-13, headless fixture), hot-path allocation contract (OB-42), the INC-6 × INC-7
  scale behaviour of fullscreen (H-9).
- **Obligation dispositions (D7):** OB-13 CLOSED (T1, fixture renderer; the real GPU limit is
  OB-52, OPEN); OB-28 CLOSED (stimuli INC-6 provides; the rest is OB-53, OPEN); OB-42 CLOSED
  (T0, steady state, Core-only test outside Core, checked mechanically). OB-44 OPEN (T4).
- **Exit tests:** `cargo test --workspace` on the governance branch; the T0 tests named in
  C12.F11–F13; T1 `t1_scale`, `t1_device_limit`; `t0_hot_path_allocations`; seeded controls.
- **Accepted residuals:** a held element may keep a higher buffer scale than needed (C12.F13
  cost); prediction is geometric (C12.F12); OB-13's residual (a side > `D_max` at scale 1);
  steady-state-only allocation scope (OB-42).
- **Not covered:** a GPU, a nested or real session (INC-13, T2/T3), interactive zoom (INC-10).

## Before signing

1. Independent review of the governance branch (the adoption record lists every change).
2. Every gate green on its head (`tools/verify-development-environment`).
3. The owner's ratification; then CHECKPOINT gets the ratification entry, the test plan gets the
   D7 status changes, and `main` is fast-forwarded — only on the owner's approval; push only on
   the owner's approval.
