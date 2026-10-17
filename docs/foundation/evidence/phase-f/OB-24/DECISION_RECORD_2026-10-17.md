# OB-24 — owner decision record and amendment C12.F14 (2026-10-17)

Status: **owner decisions D1–D6 RECORDED; layer C12.F14 written on the governance branch
`governance/ob24-c12f14-2026-10-17` (base `ca2e468`). NOT on `main`: integrating it needs the
owner's authorization. OB-24 stays OPEN (below). INC-10 is NOT started.**

## 1. Authority

Owner brief "INFYNITAE NATIVE — Integrate NB-1 Fix and Finalize OB-24 Decisions" (2026-10-17):
"You are also authorized to record the following OB-24 decisions and prepare the corresponding
amendments":

- D1: zoom domain 2⁻²⁰ through 2¹⁶.
- D2: reject the entire saved camera table if any entry violates the contract.
- D3: use a per-output lattice reference, distinct from a window-position anchor.
- D4: retain the lattice reference during panning; reset it on the approved output/configuration
  transitions. Do not use the disproven fixed-distance re-derivation rule.
- D5: do not persist the lattice reference across sessions.
- D6: document smooth zoom with accepted ±1 px variation as a future design direction only. Do not
  implement animated zoom as part of INC-10 without its own approved scope and visual evidence.

Decision basis: `evidence/phase-f/INC-9/OB24_DECISION_PACKAGE_2026-10-17.md` (copied verbatim
from `experimental/inc-9-zoom-contract` `829b170`; see `INC-9/PROVENANCE.md`).

The layer's wording is the session's, prepared from the package. It differs from the package in
two places (§4), both found while verifying the package. The owner has not reviewed this wording
verbatim; that review is part of authorizing the branch onto `main`.

## 2. Verification of each decision

Verified on `ca2e468` against the frozen rules, the code, serialization and the existing tests.

| # | Ratified text on the base | Code on the base | Result |
|---|---|---|---|
| D1 | CP3A-1 P1 (frozen): "Camera zoom is unbounded; the hint is not." F-VIEW-1: "\|v_O\| < 2³⁷", which holds only with a lower bound on k | `View::new`, `PersistedCameras::from_entries` accept any finite positive zoom; `output::valid` any non-empty mode and positive scale | **contradicts P1** → amended explicitly (C12.F14: P1, F-VIEW-1, M4 rows). The domain also bounds mode sides (1 … 2¹⁵) and scale (0.25 … 8), which F-VIEW-1's exact range needs (`ZOOM_CONTRACT.md`); a real mode stops at 2¹⁴ and a real scale is within it |
| D2 | M20 "validated all-or-nothing"; OB-43 "a corrupt or partially valid file yields an empty/default state"; INV-FAIL-1 | `from_entries` returns `Err` on the first invalid entry; no serializer exists (no `serde`, no file format: Level D, G-OB48); production starts with `PersistedCameras::default()` | **already ratified**; D1 adds out-of-domain zoom as an invalid entry. Corrupt (NaN), partially valid (one entry out of domain, valid first or last), fully valid and empty tables tested on the production entry path |
| D3 | F-LATTICE-1 `R_O = floor(v_O/120)·120`: R recomputed from the view origin; INV-RENDER-1 "R a 120-lattice reference" | `View::new` derives R every time a View is built | **amends F-LATTICE-1 and INV-RENDER-1**; named "lattice reference" (an anchor is an element's Canvas position, M1, INV-STAGE-1) |
| D4 | M4 rebuilds View/ProjectionFrame on connect and reconfigure; M5 re-binding | — | **amends F-LATTICE-1** (set on connect, (re-)binding, `k_O` or `view_O` change, and beyond `\|v_O − R_O\|·k_O` = 2³¹; kept otherwise, also while panning); M4/M5 rows name it. Not the fixed 2³⁷-Canvas-px rule |
| D5 | nothing persists R; M20's lookup holds `(OutputKey, centre, zoom)` only | the persisted tuple has no reference field | **holds by construction**; F-LATTICE-1 says "never persisted"; the ±1 px once after a restart is stated as accepted (F-LATTICE-1 known residual, C12.F14.3) |
| D6 | INV-CAMERA-5: a fit performs one camera mutation; G-OB24: "Discrete or continuous user zoom features stay out" | no zoom animation exists | **record only** (C12.F14.3); no normative text, no implementation, no visual-quality claim |

## 3. What the layer changes

- FOUNDATION_ARCHITECTURE §C12.F14 (new): P1 amended; INV-RENDER-1 row; superseded-text table;
  formula records for F-VIEW-1 and F-LATTICE-1; the corrections in §4.
- Living §F: F-VIEW-1 (representable domain, derived range), F-LATTICE-1 (lattice reference,
  known residual, INV-RENDER-1 extract).
- FOUNDATION_INVARIANTS: INV-RENDER-1 (origin C12.F14), layer list, count line (corrected: it had
  not been updated for C12.F11–F13).
- FOUNDATION_STATE_MACHINES: M4 (reference set on connect and reconfigure, dropped on disconnect;
  two rejection rows for out-of-domain descriptors), M5 (re-binding sets the reference), M6 (the
  OB-24 row: decided; the code refuses a zoomed pan until INC-10), M20 (out-of-domain zoom invalid;
  rejected file → empty/default state).
- FOUNDATION_OBJECT_MODEL OBJ-12, FOUNDATION_DECISIONS ADR-02, FOUNDATION_TEST_PLAN OB-24 history,
  FOUNDATION_IMPLEMENTATION_PLAN G-OB24 status, CHECKPOINT.
- `tools/check-foundation-docs`: `F_LAYERS` gains C12.F14; fixture 43 (INV-RENDER-1 drift).
- Code (`infynitae-core`): the domain (D1) — `geom::{MODE_MAX, SCALE_120_MIN, SCALE_120_MAX,
  ZOOM_MIN, ZOOM_MAX, zoom_in_domain}`, enforced in `View::new`, `output::valid` and
  `PersistedCameras::from_entries` (from `a156e93`, re-based as `58e1b40`); `projection.rs`
  `camera_on_grid` extracted from `View::new` (no behaviour change) so a test can build a view on
  a kept reference. The kept reference itself is **not** implemented: on the base no pan happens
  at zoom ≠ 1, and at k ∈ n/120 a reference switch changes no width or gap. It is INC-10's work.
- The ratified INC-6 test `cull_thresholds_at_extreme_scale_factors` built a View at zoom 10⁶,
  which D1 makes unrepresentable; that case now uses `ZOOM_MAX` (2¹⁶), and two cases are added at
  the domain's extremes of k (2¹⁶ · 8 = 2¹⁹ and 2⁻²⁰ · 0.25 = 2⁻²²). Its purpose — cull thresholds
  equal to `may_show` at extreme k — is kept: its threshold loop still runs k from 10⁻³¹⁰ to
  10³⁰⁰ through the pure threshold function. `view_origin_is_centre_minus_half_view_ties_even`'s tie case moves from
  scale 65536 (outside the scale domain) to zoom 2¹⁶ at scale 1: the same k = 2¹⁶ tie.

## 4. Corrections to the decision package

1. **The reference bound is 2³¹, not 2⁵⁰** (corrected 2026-10-17; the replaced text is in §8).
   The package re-derived the reference when `|v_O − R_O|·k_O` would exceed 2⁵⁰, on
   `anchor_model_sim.py` T3 (0 wrong edges at 2⁵⁰). T3's k values were 73/10·2¹⁶, 9/8·2¹⁵,
   7/5·2¹², 225: their products have a fractional part on a coarse grid, never near ½, so f64
   rounding could not show. `exactness_bound_model.py` (this directory; output `.out`; a random
   sample, not a proof) samples generic k too: wrong edges from 2⁴⁶ at generic k (380 of 3,200 at
   2⁵⁰), and from 2³³ at k = n/120 and at exact ties (k = 1.25·2ⁿ, x < R), where the f64 sum
   loses δ; the sample found none up to 2³². 2³¹ is a conservative engineering choice, not the
   largest bound the argument allows. Rounding-step argument (binary64, round-to-nearest-even, no
   fused multiply-add; `f64::round` half away from zero; integral `x − R` below 2⁵³; exact means
   exact arithmetic on the f64 k): within the bound a visible edge has `|(x − R)·k|` ≤ 2³¹ + 2¹⁵
   (+ k), below 2³², so the f64 product is within 2⁻²² of exact and the sum `+ δ` within
   2⁻²¹ < δ. So δ is never absorbed, exact ties round up on both sides of R as exact arithmetic
   does, and an edge differs from exact arithmetic only where the exact sum is within 2⁻²¹ of a
   half-integer. That does **not** make every edge exact: disagreements exist inside the bound
   (`x − R` = 2,147,483,643, k = `0x1.00000000ffffep+0`: f64 2,147,483,644, exact 2,147,483,643)
   and at the per-frame reference's magnitudes (`x − R` = 37, k = `0x1.03759eb3e4530p+0`). The
   invariant the kept reference needs is different and holds regardless: with R_O and k_O fixed,
   an edge depends only on its coordinate, so widths and gaps survive a pan. It says nothing about
   render/hit agreement (item 3). T0: `kept_reference_rounding_steps_at_the_visible_edge_limit`
   (deterministic, every binary exponent of k in the domain, both sides of R; mutation checks: it
   fails with δ dropped, with δ = 2⁻²³ and with the visible-edge limit raised to 2³³),
   `a_kept_reference_within_the_bound_matches_exact_edges_on_sampled_states` (4,000 sampled
   states; renamed from `a_kept_reference_within_the_bound_rounds_every_visible_edge_exactly`),
   and `beyond_the_bound_a_kept_reference_loses_exact_edges` (loss at 2³⁵).
2. **M4.** The package amended an M4 phrase, "validates the hardware facts", that M4 does not
   contain. The layer adds rejection rows to M4 instead; the base code already rejects an empty
   mode or a non-positive scale (`EntryError::InvalidDescriptor`), undocumented in M4 until now.
   Corrected 2026-10-17: the accepting rows carry the domain guard too, so the accepting and
   rejecting rows partition M4's inputs.
3. **Render/hit agreement** (added 2026-10-17). The layer had said render and hit "agree on every
   frame". Core's render list, membership and hit test do read one View per frame, but the pixels
   presented are Smithay's: `[round(L), round(fl(w·k) + L))` from F-RENDER-1's f64 location L,
   against Core's `[round((x − R)·k + δ) − C, round((x + w − R)·k + δ) − C)`. X-6 measured 0
   differences at k = n/120 with the per-frame reference. `render_hit_probe.rs` (this directory;
   real `from_surface` + `geometry` on DummyRenderer, k swept by single ulps around the right
   edge's rounding point) finds 1 physical px right-edge differences in 15 of 23,659 rects with the
   per-frame reference at k ∉ n/120, and 38 and 16 of 23,659 with a reference kept at
   `|v − R|·k ≈ ±2³¹` (`render_hit_probe.out`). Unreachable on the base (zoom 1); reachable once
   INC-10 adds fit zoom. Render/hit agreement with the kept reference is an INC-10 exit requirement,
   and the current handoff needs an architecture decision before it can meet it.

## 5. Tests and controls

T0 (`infynitae-core`): `a_kept_lattice_reference_keeps_widths_and_gaps_during_a_zoomed_pan`
(k = 1.543125, 240 frames: 0 changes kept; the sealed per-frame reference changes widths — the
ED-1 negative control); `a_kept_reference_within_the_bound_matches_exact_edges_on_sampled_states` (renamed 2026-10-17);
`beyond_the_bound_a_kept_reference_loses_exact_edges`; `a_rejected_persisted_table_applies_none_of_its_entries`
(corrupt, partially valid in both orders, valid, empty; on `Core::new` + `output_connected`);
`kept_reference_rounding_steps_at_the_visible_edge_limit` (added 2026-10-17);
A1's `the_accepted_domain_never_overflows`, `a_view_outside_the_domain_is_refused`,
`the_domain_is_enforced_where_zooms_and_outputs_enter_core`, `core_runs_at_the_extremes_of_the_domain`.

Seeded controls ("C12.F14: …", 7; an eighth added 2026-10-17 — δ = 2⁻²³, below one rounding
step, against the deterministic test): ZOOM_MIN widened; ZOOM_MAX widened; persisted zoom unchecked;
mode unchecked; the camera on the grid ignores the view's reference; δ dropped from the edge; a
rejected table keeps its valid prefix. All DETECTED (run individually on this branch; the full
gate result is in §7).

## 6. Status after this record

- **OB-24: OPEN.** The decision part is recorded (C12.F14). Its pass criterion also asks for
  evidence equivalent to X-6/ED-1 Smithay geometry runs of the decided rule; with the rule
  unimplemented, only model and T0 evidence exists. INC-10's exit must show 0 width/gap changes
  per zoomed pan on the production path. The destination DECISION keeps it OPEN in the checker in
  any case.
- **G-OB24:** amended by C12.F14.5 (Option A, 2026-10-17): met when C12.F14 is on `main` — the
  recorded decision is the prerequisite for beginning INC-10; OB-24 itself stays OPEN until
  INC-10's exit evidence (0 width/gap changes per zoomed pan; render/hit agreement with the kept
  reference on Smithay geometry, §4 item 3) is accepted. Not met today.
- **INC-10:** NOT started. Its remaining owner decisions (fit target, bindings, INC-6
  dependency) are listed in the INC-10 implementation brief and the session report
  (`evidence/phase-f/INC-10/`, `evidence/phase-f/session-2026-10-17/`, branch
  `docs/overnight-2026-10-17`).
- **D6:** a recorded direction; nothing implemented, nothing validated.
- **Fit domain:** not decided by C12.F14 (corrected 2026-10-17). The fit's zoom leaves the domain
  only below 2⁻²⁰; the intended rule (clamp to 2⁻²⁰) and its rationale are recorded in
  FOUNDATION_IMPLEMENTATION_PLAN INC-10, which specifies and tests it.

## 7. Gates

See the commit message of the layer commit and the session report: the full CI set
(`.github/workflows/ci.yml`) plus `verify-development-environment`, run on this branch's head.

## 8. Corrections of 2026-10-17 (owner brief "C12.F14 Governance Corrections", review of `1d5931d`)

Made on the same branch before integration; FOUNDATION_ARCHITECTURE §C12.F14.6 lists every
replaced sentence of the layer. The owner decisions D1–D6 (§1) are unchanged. Replaced here,
verbatim (§4 item 1, from "With the bound 2³¹"):

> With the bound 2³¹ every visible edge stays below 2³² and the f64
> error stays below δ. T0: `a_kept_reference_within_the_bound_rounds_every_visible_edge_exactly`
> (4,000 states over the whole domain, both sides of R, exact i128 reference; mutation check:
> the same test fails with the bound at 2³⁵ and at 2⁵⁰), and
> `beyond_the_bound_a_kept_reference_loses_exact_edges` (loss at 2³⁵).

and §6: "**G-OB24:** decided; resolved when C12.F14 is on `main`."
