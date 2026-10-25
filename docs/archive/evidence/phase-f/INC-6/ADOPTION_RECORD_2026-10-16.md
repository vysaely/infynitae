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

- §C12.F11, §C12.F12, §C12.F13 inserted after §C12.F10. Every paragraph of P-1/P-2/P-3 is placed
  **verbatim** (`<DATE>` = 2026-10-16, `<BASE>` = `eebb5c3`), checked mechanically (whitespace and
  bold markers normalised) on 2026-10-16; see "Placement review" below for the registry framing
  around them.
- The sealed C14.4 is not edited; C12.F13 carries the new paragraph and supersedes C14.4 as far as
  stated.
- Living §F: F-SCALE-2's `s_max(E)` sentence replaced by P-1's sentence; F-SCALE-3's initial hint
  gains P-2's clause; both **Amended** lines name the layer.
- FOUNDATION_INVARIANTS: INV-SCALE-2 (origin C12.F11), INV-SCALE-5 (origin C12.F13), INV-SCALE-4
  obligations OB-28, OB-53; header lists the new layers and C12.F9's withdrawal.
- FOUNDATION_TEST_PLAN: OB-28 item and history (split); OB-53 new (OPEN, T1); OB-42 pass
  criterion and closure condition (D6) and history; OB-13 history. Plan INC-10/11/14 cite OB-53;
  RISKS RC-2 cites OB-53.
- `tools/check-foundation-docs`: `F_LAYERS` + C12.F11–F13; fixtures 40–42; fixtures 23, 24
  re-anchored on the amended F-SCALE-2 (their seeded defects unchanged).
- Code comments and seeded-control names: "REHEARSAL / NOT ADOPTED" labels removed (no code
  change; anchors updated identically).

## Placement review (2026-10-16, before ratification) — correction of this record

The first version of this section said "their canonical text is P-1/P-2/P-3 verbatim … No other
wording". The INC-6 ratification review found that **not accurate** for the commit `f2bf0da`:
the layers also carried wording that is not in the package (an extra clause in C12.F11's status
sentence, "(canonical here; the sealed C14.4 is not edited)" in C12.F13's, an approval
parenthetical in each status line, bulleted and linked Inputs/Provenance, two composed
**Reason** sentences, one extra test name in C12.F13's list, two commas). None of it changed a
normative sentence, but the decision said "Do not silently substitute different text", so the
layers were corrected in the commit that carries this section: every package paragraph is again
verbatim, and the elements below are the only text in the three layers that the package does
not contain. Each is registry framing that the registry or its checker requires:

| Element | Why it is there |
|---|---|
| Layer headings `# §C12.F1x — Canonical registry amendment (Phase F: …)` and `## C12.F1x.n …` headings | registry layer format (every C12 layer); `check-foundation-docs` finds layers by heading |
| Sub-section numbering: C12.F11 and C12.F12 have `.1` superseded, **`.2` formula amendments**, `.3` what this layer does not change; the package numbers "What this layer does not change" `.2` | P-1/P-2 instruct "add C12.F11/C12.F12 to the Amended line"; the checker (`formula-provenance`) then requires a `<layer>.2` record with **Previous** and **Reason** lines. The package's own numbering cannot satisfy its own instruction; the package's `.2` text is placed verbatim as `.3`. C12.F13 amends no formula and keeps the package numbering. |
| `.2` records: **Previous** = the formula text replaced, verbatim; **Reason** = a pointer to the proposal section (not new wording) | `formula-provenance` rule |
| "Where an `INV-*` appears below, this wording is canonical and supersedes …" and the one-row INV table per layer | registry layer format; the INV rows are the package's sentences placed into the rows (C12.F11: INV-SCALE-2's `s_max` clause = P-1's sentence; C12.F12: P-2's clause appended; C12.F13: P-3's exception) |
| Superseded entries as a two-column table (`Superseded` → `Replaced by`); C12.F11 also lists INV-SCALE-2's old `s_max` clause | registry layer format; the extra row records the INV text the row replaces |
| P-3's label "C14.4, after the list of non-client stimuli of SC-E:" placed in bold; P-1's label "F-SCALE-2, the sentence defining `s_max(E)` (living §F carries the corrected text):" not repeated — its sentence is placed in living §F F-SCALE-2 and in the INV-SCALE-2 row | registry format; the sentences themselves are verbatim |

The test that `eebb5c3` added for P-3's configuration clause
(`a_configuration_change_applies_p7_at_once_and_releases_no_hold`) and the one added in this
review (`a_configuration_change_releases_no_hold_later`, with two seeded controls) are recorded
here and in the ratification record, not in the layer's approved test list.
## Gates

See the commit of this record (fmt, clippy, tests, verify-architecture, docs + self-test,
knowledge, efficacy).
