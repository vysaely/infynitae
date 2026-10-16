# INC-6 — adoption record (2026-10-16)

Status: **RECORD of an owner adoption. INC-6 is NOT ratified.** Branch
`governance/inc-6-exit-adoption-2026-10-16`, base `eebb5c3` (candidate r4).

## Decision provenance

- **Owner decision:** brief "INFYNITAE — Ratification, Clipboard, Orderly Shutdown and First Nested
  Runtime", Phase 1, 2026-10-16: "The owner's intended direction is to approve the package
  described in the supplied report, subject to confirming that the live revision contains those
  exact texts and no material contradictions. Do not silently substitute different text."
- **Package adopted:** `ADOPTION_PACKAGE_2026-10-16.md`, revision 2, git blob
  `31eeddb8aa27d4835c1bf23e5d941ef18f6de27f` (at `d7c6c7c`). On this branch its text is
  unchanged; only a header note pointing to this record was added above it. Proposal texts it
  cites: C12F11 `cd4a85e9`, C12F12 `ad90ee5a`, C12F13 `208c41c1` (blobs at `d7c6c7c`).
- **Decisions:** D1 C12.F11, D2 C12.F12, D3 C12.F13, D4 C12.F9 withdrawn, D5 OB-28 → OB-28 +
  OB-53, D6 OB-42 wording — **adopted**. D7 (close OB-13, OB-28, OB-42 at the exit; keep OB-44,
  OB-52, OB-53 open) — **approved, executed at INC-6's ratification**, not here.

## Adoption check (the condition of the decision)

| Text | Checked against | Result |
|---|---|---|
| P-1 C12.F11 | `s_max_of` (granted size, restore offer), the offer's lifetime in `protocol.rs` (spent on a window-geometry size change, unmap, destroy; re-grant saves the offer), the initial-commit path | matches |
| P-2 C12.F12 | initial prediction: `max(scale_O, predict_rho_observed(others))`, O's view excluded | matches |
| P-3 C12.F13 | `Sweep::system` (output changes only), camera marks, subjects, holds, settlement guard | matches, **except** "a change of the scale configuration … P7 applies at once": `set_scale_config` deferred P7 to the next evaluation |
| P-4 … P-6 | registry, CHECKPOINT, inventory, test plan | applicable as written |
| Ratified invariants | INV-PRES-1, INV-STAGE-1, INV-CAMERA-1, INV-INPUT-8, INV-ARCH-2 | not weakened: the layers change scale formulas and the scale churn rule only |

**Resolution of the one contradiction, without changing the text:** `eebb5c3` makes
`set_scale_config` apply P7 at once through the device-limit path and return the hints; test
`a_configuration_change_applies_p7_at_once_and_releases_no_hold`. `set_scale_config` has no
production caller. The contradiction was in code, not in the approved semantics, so the gate was
passed rather than stopped.

## How the texts were placed

- §C12.F11, §C12.F12, §C12.F13 inserted after §C12.F10 with `Status: ADOPTED 2026-10-16`, base
  `eebb5c3`; their canonical text is P-1/P-2/P-3 verbatim. The registry requires each layer to
  carry an invariant row: C12.F11's INV-SCALE-2 row replaces its `s_max` clause with **the
  approved F-SCALE-2 sentence verbatim**; C12.F12's INV-SCALE-5 row appends the approved clause
  verbatim; C12.F13's row adds the approved exception verbatim to the F12 row. No other wording.
- The sealed C14.4 is not edited; C12.F13 carries the new paragraph and supersedes C14.4 as far as
  stated.
- Living §F: F-SCALE-2 and F-SCALE-3 definitions amended; `.2` records keep the previous text
  verbatim.
- FOUNDATION_INVARIANTS: INV-SCALE-2 (origin C12.F11), INV-SCALE-5 (origin C12.F13), INV-SCALE-4
  obligations OB-28, OB-53; header lists the new layers and C12.F9's withdrawal.
- FOUNDATION_TEST_PLAN: OB-28 item and history (split); OB-53 new (OPEN, T1); OB-42 pass
  criterion and closure condition (D6) and history; OB-13 history. Plan INC-10/11/14 cite OB-53;
  RISKS RC-2 cites OB-53.
- `tools/check-foundation-docs`: `F_LAYERS` + C12.F11–F13; fixtures 40–42; fixtures 23, 24
  re-anchored on the amended F-SCALE-2 (their seeded defects unchanged).
- Code comments and seeded-control names: "REHEARSAL / NOT ADOPTED" labels removed (no code
  change; anchors updated identically).

## Gates

See the commit of this record (fmt, clippy, tests, verify-architecture, docs + self-test,
knowledge, efficacy).
