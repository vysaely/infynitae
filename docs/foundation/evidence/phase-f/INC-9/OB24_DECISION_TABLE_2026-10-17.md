# G-OB24 — decision table for the owner (2026-10-17)

Status: **FOR APPROVAL. Nothing here is adopted.** OB-24 stays OPEN; INC-10 must not start until
an approved decision record exists. This table revises `OB24_FINAL_DECISION_2026-10-16.md` on
three points (D2 basis stated, D7 recommendation changed, D6 recommendation **withdrawn** after a
measurement); that file's amendment texts A1–A4 are the drafting basis and change as noted.

| # | Question | Recommendation | Basis | Owner decision |
|---|---|---|---|---|
| D1 | Supported zoom domain | Camera zoom **2⁻²⁰ … 2¹⁶** (finite); output mode sides 1 … 2¹⁵ px; output scales 0.25 … 8; centre within INV-COORD-1 (amendment A1) | proof table and corner tests in debug, release + overflow checks and release (`ZOOM_CONTRACT.md`, `a156e93`); four seeded controls | |
| D2 | Saved camera state with an invalid zoom | **Reject the whole table** (the current rule). Entry-only rejection is not recommended because documented rules require all-or-nothing: M20 "The file is validated all-or-nothing at startup", OB-43's pass criterion "a corrupt or partially valid file yields an empty/default state", and INV-FAIL-1's violation "corrupt authority hidden by best effort". Entry-only would need amendments of M20 and OB-43 (and the owner's reading of INV-FAIL-1). | M20, OB-43, INV-FAIL-1 | whole table / entry-only (with amendments) |
| D3 | Pixel-rounding anchor ownership | **Per output** (the output's projection state), not the camera; the camera keeps `(centre, zoom)` (amendment A2) | `R_O` depends on `v_O`, `view_O`, `k_O` — output facts | |
| D4 | Anchor lifecycle | Set on: output connected; camera bound/re-bound to it; `k_O` changed (zoom, output scale); `view_O` changed (mode, transform); a pan that would take `\|v_O − R_O\|` beyond 2³⁷ px. Kept across pans otherwise. Dropped on disconnect. Each output independently. | INC-9 harness (pans: 0 width changes with a kept anchor, 3–11 per pan without) | |
| D5 | Anchor persistence | **Do not persist.** Reconstruct from the active output configuration on connect. Consequence: after a restart some widths may differ by ±1 px from the previous session — once, at connect, not during use. A4 shrinks to the camera table's versioning and D2's rule. | the anchor is runtime projection state; persisting it couples the file to output configurations | |
| D6 | Animated zoom | **No rule recommended yet — the earlier Z2 recommendation is withdrawn.** Measured (`anim_zoom_sim.py`, exact F-VIEW-1/F-LATTICE-1 arithmetic, 5 seeds × 400 edges): with integer-snapped geometry re-rounded each frame, widths/gaps move against the zoom direction in ~13 % of width/gap comparisons per frame (k 1→2: Z1 13,820, Z2 13,790 of 107,640), with edge deviations up to ~1.9 px from ideal motion, **whether or not k is snapped to n/120**. Snapping only lowers the count where it removes frames: zoomed out it leaves 7 distinct zoom levels in 60 frames at k 0.05→0.1 (16.7 % steps) and 2 at k 0.01→0.02 (100 % steps) — discontinuity, not stability. The INC-9 "exact at k ∈ n/120" result holds for pans (k fixed), not for a changing k. Options: Z1 accept ±1 px during motion, decided on a T3 visual verdict; Z3 unsnapped (fractional) placement while k changes and snap at rest — needs sub-pixel placement in the renderer (F-RENDER-1 amendment; Smithay places at integer points) and hit testing on unsnapped geometry. | measurement in this table; `anim_zoom_sim.out` | Z1 (+T3) / Z3 / other |
| D7 | Camera serialization compatibility | Versioned camera table `(OutputKey, centre, zoom)`; unknown camera-table version rejects the file (D2); no anchor table (D5). No format exists yet (Level D, OB-43); files without a version do not exist. | — | |

## Documents that require approval

1. This table (decisions D1–D7).
2. `OB24_FINAL_DECISION_2026-10-16.md` §3 amendment texts **A1** (domain: P1, F-VIEW-1, M4) and
   **A2** (anchor: F-LATTICE-1, INV-RENDER-1 extracted text) as drafted; **A3** (animated zoom)
   must be redrafted after D6 is decided; **A4** (persistence) reduces to D2 + D7 if D5 is
   approved.
3. The governance layer that adopts them (numbered at adoption, after C12.F13), with the
   test-plan change: OB-24 closed for pans at a fixed zoom (A2); a new obligation for animated
   zoom (D6) unless the owner decides it with evidence first.

## INC-10 gate

INC-10's entry conditions: INC-3 exit (met), INC-6 exit (ratified and on local `main` since
2026-10-17, not pushed), and G-OB24 resolved or the conservative fallback. G-OB24 is **not**
resolved. INC-10 does not start.
