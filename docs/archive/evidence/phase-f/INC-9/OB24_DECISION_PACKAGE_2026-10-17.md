# G-OB24 — decision package for owner review (2026-10-17)

Status: **PREPARED FOR OWNER REVIEW. Nothing is adopted, nothing ratified, no normative text
changed.** It evaluates the owner's proposed decisions D1–D6 (overnight brief, Priority B)
against the ratified texts on local `main` `ca2e468` (INC-6 included) and gives the smallest
amendment each needs. It supersedes, for review purposes, `OB24_DECISION_TABLE_2026-10-17.md`
and `OB24_FINAL_DECISION_2026-10-16.md` §3 where they differ (noted per item). OB-24 stays OPEN.

Authority traced (on `ca2e468`): FOUNDATION_ARCHITECTURE P1 (frozen property), F-VIEW-1,
F-LATTICE-1, F-RENDER-1, F-MEMBER-1, F-FIT-1, F-PAN-1; INV-CAMERA-1/3/5, INV-COORD-1,
INV-RENDER-1, INV-HIT-1, INV-FAIL-1, INV-OUTPUT-2; STATE_MACHINES M4, M5, M20;
FOUNDATION_TEST_PLAN OB-24, OB-43, OB-48; FOUNDATION_IMPLEMENTATION_PLAN G-OB24, INC-10.

## Summary

| # | Proposed decision | Already authorized by ratified text? | Needs | Evidence |
|---|---|---|---|---|
| D1 | Zoom domain 2⁻²⁰ … 2¹⁶ | **No** — contradicts P1 "Camera zoom is unbounded" | owner approval of **A1** | `ZOOM_CONTRACT.md` proof; corner tests, debug/release (`a156e93`) |
| D2 | Reject the whole persisted table if corrupt or partially valid | **Yes** — M20 "validated all-or-nothing"; OB-43 "a corrupt or partially valid file yields an empty/default state" | nothing beyond D1 (which makes an out-of-domain zoom an invalid entry) | `a156e93` (`PersistedCameras::from_entries` rejects the table) |
| D3 | Reference ownership per output | **No** — F-LATTICE-1 defines R as a function of the current `v_O` | owner approval of **A2** (with D4) | INC-9 harness; `anchor_model_sim.py` T1 |
| D4 | Reset on connect, rebinding, scale/mode/transform change; keep while panning; drop on disconnect | **No** (same) — but the reset points coincide with M4's existing "View, ProjectionFrame … rebuilt" rows | owner approval of **A2**; **correction** of the exactness bound (below) | `anchor_model_sim.py` T1–T3 |
| D5 | Do not persist the reference | **Yes, by absence** — nothing persists it today; M20's persisted lookup holds cameras only | nothing (A2 states it) | — |
| D6 | Continuous zoom without snapping, accepting ±1 px | **Not needed by Foundation 01**: no animated zoom exists or is planned there (INV-CAMERA-5: a fit "performs one camera mutation"; G-OB24 fallback: "Discrete or continuous user zoom features stay out"; INC-10 = one-shot fit, restore, zoomed pan) | a record only; acceptance needs T3/T4 visual evidence per OB-24's pass criterion before any animated zoom ships | `anim_zoom_sim.py` |

## D1 — representable zoom domain

**Trace.** P1 (frozen): "Camera zoom is unbounded; the hint is not." F-VIEW-1 (living): "a derived
view origin may reach |v_O| ≤ 2³⁶ + view_O/(2k_O) … it stays exact in f64 because |v_O| < 2³⁷" —
**false without a lower bound on k** (view_O/(2k_O) grows without limit as k → 0; on `main` a
zoom below ~1.4·10⁻¹¹ overflows the Q16 origin, above ~10⁷ the edge arithmetic: `ZOOM_DOMAIN.md`).
INV-CAMERA-1: the output-lifecycle entries never take a zoom from their caller — so out-of-domain
zooms can enter only through the persisted lookup and future zoom writers.

**Contradiction / edge cases.** The frozen P1 must be amended (cannot be "interpreted").
Fit (F-FIT-1) of a window wider than the view at 2⁻²⁰ is clamped to 2⁻²⁰ and shown in part.
Output modes > 2¹⁵ or scales outside 0.25…8 are refused at connect — an INV-OUTPUT-2 consequence
(the output stays unused), not a camera change.

**Smallest amendment A1** (unchanged from `OB24_FINAL_DECISION_2026-10-16.md` §3):
P1 → "Camera zoom is bounded only by the representable domain 2⁻²⁰ … 2¹⁶ (F-VIEW-1); the hint is
bounded by P1's device limit." F-VIEW-1 derived-range definition → the domain sentence (mode sides
1 … 2¹⁵, scales 0.25 … 8, zoom 2⁻²⁰ … 2¹⁶, centre within INV-COORD-1; refused where it enters
Core; writers clamp). M4: "validates the hardware facts" gains "(mode sides 1 … 2¹⁵, scale
0.25 … 8)".

**Tests.** Exist on `experimental/inc-9-zoom-contract` (`a156e93`): the accepted domain never
overflows (900 corner views × 7 edges + 20,000 random; debug, release + overflow checks, release),
refusal at every entry point, Core at the four extremes; 4 seeded controls. They are based on
`23752f1`. **Re-based on `main` `ca2e468`** (branch `experiment/a1-domain-on-main-2026-10-17`,
`58e1b40`): one conflict-free adaptation (`physical_input` → `physical_input_vec`, INC-6's
caller-buffer API) and **one interaction with a ratified INC-6 test**: `cull_thresholds_at_extreme_scale_factors`
builds a View at zoom 10⁶, which A1 makes unrepresentable (`InvalidParams`). With that case moved
to `ZOOM_MAX` (2¹⁶) — the threshold loop at k up to 10³⁰⁰ still covers the arithmetic — Core
177/0, workspace 448/0, domain tests in release with overflow checks 6/0, the four A1 seeded
controls DETECTED. Adopting A1 therefore also amends that INC-6 test; the owner should approve
it as part of A1, not as a separate change.

## D2 — invalid persisted camera state

**Trace.** M20: "The file is validated all-or-nothing at startup and given to `Core::new` as a
passive lookup." OB-43 pass criterion: "a corrupt or partially valid file yields an empty/default
state and is kept for inspection." INV-FAIL-1 violation: "corrupt authority hidden by best effort".

**Finding.** D2 is the ratified rule already. No amendment. Edge case: shipping persisted cameras
is itself gated (G-OB48, OB-48); no on-disk format exists (Level D). The table version (unknown
version → whole file rejected) belongs to the Level D format decision, not to G-OB24.

## D3 + D4 — the lattice reference, per output, kept across pans

**Trace.** F-LATTICE-1: "`R_O = floor(v_O / 120) · 120`" — R follows the view every frame; its
"Known residual" names OB-24. INV-RENDER-1 extracts "R a 120-lattice reference". INV-HIT-1 needs
render/hit agreement ("same edges/order/`c_eff` as rendering"): satisfied as long as hit testing
reads the reference the frame was rendered with (ProjectionFrame). M4 already rebuilds "View
(`view_O`), ProjectionFrame and damage tracker" on reconfigure, and creates the View on connect.

**Edge cases found.**
1. **Exactness bound (correction of the earlier draft).** The earlier draft re-anchored only when
   `|v_O − R_O|` would exceed 2³⁷ px. With a kept reference the visible offsets `(x − R)·k` reach
   `|v − R|·k`; at the top of the domain (k up to 2¹⁹) that exceeds f64's exact integer range
   long before 2³⁷ px. `anchor_model_sim.py` T3: at k ≈ 478,413 (zoom 2¹⁶, scale 7.3) 64 of 300
   edges are wrong at `|v−R|·k ≈ 2⁵²` (|v − R| ≈ 9.4·10⁹ px, inside 2³⁷), none at ≤ 2⁵⁰. **The
   bound must be `|v_O − R_O|·k_O ≤ 2⁵⁰`.** At k ≤ 1 that is never reached inside the Canvas.
2. **Naming.** "Anchor" already means an element's Canvas anchor (M1, M20, INV-STAGE-1). Calling
   R an "anchor" would conflate the two; A2 names it the output's *lattice reference*.
3. **Rebinding.** A camera re-bound to an output (M5 row 2) changes `v_O` arbitrarily: re-derive.
4. **Two outputs.** Each output keeps its own reference; a window spanning two outputs is rounded
   per output (as today). T1: outputs at different scales panning together — 0 width changes
   with kept references, 50–59 changed frames per output (4 pans) with the sealed rule.
5. **Re-anchor events change widths once** (k or view changed: a re-round by definition), never
   at other frames (T2).

**Smallest amendment A2** (replaces the earlier A2):
F-LATTICE-1, first definition → "`R_O` is output O's *lattice reference*, an integral Canvas value
held in O's projection state (not the camera's). It is set to `floor(v_O / 120) · 120` when O's
view is (re)defined — O connected; a camera bound or re-bound to O; `k_O` changed; `view_O`
changed — and whenever keeping it would make `|v_O − R_O|·k_O` exceed 2⁵⁰; otherwise it is kept.
It is dropped with O and never persisted. δ = 2⁻²⁰; `k_O = zoom_C · scale_O`." F-LATTICE-1
"Known residual" → "With the reference kept, widths and gaps do not change while panning at a
fixed `k_O`; they are re-rounded once at each re-definition above." INV-RENDER-1's extracted text
→ "… with R O's lattice reference (F-LATTICE-1) …". M4 rows "connected" and "reconfigured" gain
"lattice reference re-derived"; M5 "re-bound" row gains the same.

**Tests (when A2 is implemented, INC-10):** a zoomed pan of N frames at k ∉ n/120 changes no
width/gap; each re-definition event re-derives R exactly once; render and hit agree on every frame
(INV-HIT-1); the exactness bound triggers a re-derivation at `|v−R|·k > 2⁵⁰` (seeded control:
bound at 2⁵³ → wrong edges at the domain top). Model evidence now: `anchor_model_sim.py`.

## D5 — no persistence of the reference

Nothing to amend: no ratified text persists it; A2 says "never persisted". Consequence: after a
restart a reference is re-derived at connect — some widths may differ by ±1 px from the previous
session, once.

## D6 — animated zoom

**Finding.** Not required by Foundation 01 (see summary). The owner's proposal — continuous zoom
without snapping, accepting ±1 px rasterization differences during motion — is consistent with
the measurement (`anim_zoom_sim.py`: per-frame re-rounding moves ~13 % of widths/gaps against
the zoom direction per frame, ≤ ~1.9 px edge deviation from ideal motion; snapping to n/120 does
not remove it). OB-24's pass criterion requires the residual to be "explicitly accepted with
visual evidence (T3/T4)". **Proposed record (not an amendment):** "An animated zoom, when one is
proposed, re-derives the lattice reference every frame (A2: k changes) and accepts ±1 px
relative rasterization differences during the motion, subject to a T3/T4 visual verdict before
it ships." No Foundation 01 increment needs it.

## What the owner is asked to approve

1. A1 (D1) — amends frozen P1, F-VIEW-1, M4.
2. A2 (D3 + D4 + D5) — amends F-LATTICE-1, INV-RENDER-1's extract, M4, M5; includes the corrected
   exactness bound.
3. The D6 record (no normative change) and whether animated zoom needs any Foundation 01 entry.
4. OB-24 disposition at adoption: CLOSED for pans at a fixed zoom by A2 with the evidence above
   (INC-9 harness, `anchor_model_sim.py`); animated zoom carried by a new OPEN obligation (T3)
   only if the owner wants it tracked now.

Adoption mechanics (after approval): a governance layer after C12.F13 (number assigned then) on a
branch from `main`, checker `F_LAYERS` and drift fixtures, the A1 code (`a156e93`) re-based and
re-tested on `main`. INC-10 may start only after that layer is adopted (G-OB24 resolved) — or
under the documented conservative fallback.
