# INC-6 — formal ratification record (2026-10-16)

Status: **RATIFIED** on `governance/inc-6-exit-adoption-2026-10-16`. **Not merged into `main`,
not pushed**; integration needs the owner's separate authorization.

## Authority

Owner brief "INFYNITAE — Final Ratification Gates and Guided Runtime Readiness", §1: "Verify the
adoption record, adopted amendment texts, current implementation, and all required ratification
gates. Specifically verify the scale-configuration timing correction and its tests. If the
branch is fully consistent with the adopted authority, complete the formal ratification record on
the designated governance branch. Otherwise, document the exact blocker. Do not merge or push."
Adoption: owner approval 2026-10-16 of package revision 2 (`ADOPTION_RECORD_2026-10-16.md`).

## Ratified commit

**`7aa2070`** (`governance/inc-6-exit-adoption-2026-10-16`): adoption `f2bf0da` + ratification
review `7aa2070`, on candidate r4 `eebb5c3` (from the INC-6 line and canonical `main` `23752f1`).
This record is a documentation-only commit on top of it.

## Verification performed before recording

| Check | Result |
|---|---|
| Adopted texts vs package rev. 2 | Every normative sentence of P-1/P-2/P-3 verbatim in the registry layers, the INV rows and living §F (mechanical check, whitespace/bold normalised). The review found **non-package wording** in `f2bf0da` (status clauses, composed Reason lines, an extra test name, "(canonical)", two commas) and the record's claim "no other wording" inaccurate; corrected in `7aa2070` — layers restored to the package text; the only remaining non-package text is registry framing the checker requires, enumerated in `ADOPTION_RECORD_2026-10-16.md` "Placement review". |
| Implementation vs adopted texts | P-1 (`s_max_of`, restore offer lifetime), P-2 (pre-map prediction), P-3 (holds, touching, settlement guard, system stimuli) match; the one contradiction (P-3 configuration clause) is resolved in code by `eebb5c3`. |
| Scale-configuration timing (P-3) | `set_scale_config` applies P7 at once via the device-bound path and returns the hints. Tests: `a_configuration_change_applies_p7_at_once_and_releases_no_hold` (P7 at once; hold kept), `a_configuration_change_releases_no_hold_later` (added in review: a non-lowering change and the full pass it schedules release no hold). Note recorded: when P7 lowers a held element it lowers it to the target of its requirement, after which the hold ends by P-3's own rule ("requirement back at or above its hint"). Seeded controls: P7 deferred; configuration change as a system stimulus — both DETECTED. |
| Gates on `7aa2070` | fmt, clippy `-D warnings`, `cargo test --workspace` 444 passed / 0 failed, `verify-architecture`, `check-foundation-docs` + self-test, `verify-knowledge`, `verify-test-efficacy` 344 detected — all PASS; `tools/verify-development-environment`: "ALL CHECKS PASSED". |
| Exit tests | T1 `t1_scale` (19), `t1_device_limit` (6), `t0_hot_path_allocations` (1), Core lib (173): PASS. The T1 test the package lists for the integration line (`a_hidden_windows_lowered_hint_releases_no_withheld_callback`) belongs to the INC-6 × INC-8 line, not this branch, as the package states. |
| Ratified invariants of other increments | INV-PRES-1, INV-STAGE-1, INV-CAMERA-1, INV-INPUT-8, INV-ARCH-2 not weakened (adoption check, unchanged by the review). |

## Scope

Scale policy (INV-SCALE-1…5 as amended by C12.F11–F13; F-SCALE-1…3; ladder, epochs, SC-E, P7,
presentation holds); device-limit path through the renderer capability interface (OB-13,
headless fixture); hot-path allocation contract (OB-42, steady state); INC-6 × INC-7 scale
behaviour of fullscreen (H-9).

## Obligation dispositions (D7, executed by this record)

OB-13 **CLOSED** (T1, fixture renderer; the real GPU limit is OB-52, OPEN). OB-28 **CLOSED**
(stimuli INC-6 provides; the remainder is OB-53, OPEN). OB-42 **CLOSED** (T0, steady state,
Core-only test outside Core, checked mechanically). OB-44 OPEN (T4), OB-52 OPEN (T2, INC-13),
OB-53 OPEN (T1, later increments).

## Accepted residuals

A held element may keep a higher buffer scale than needed (C12.F13 cost); prediction is
geometric (C12.F12); OB-13's residual (a side > `D_max` at scale 1); steady-state-only allocation
scope (OB-42).

## Not covered

A GPU and a real renderer limit (INC-13, OB-52); nested or real-session visual evidence (T2/T3);
interactive zoom (INC-10, gated by G-OB24); INC-8 interaction beyond the integration line.

## Next (owner's decision)

Integration of `governance/inc-6-exit-adoption-2026-10-16` (with this record) into `main`
(a fast-forward: `main` is still `23752f1`, from which the branch descends); push only with the
owner's approval.
