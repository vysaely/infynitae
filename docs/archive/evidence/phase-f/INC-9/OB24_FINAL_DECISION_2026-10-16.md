# G-OB24 and the zoom domain — final decision proposal (2026-10-16)

> **Revised 2026-10-17** by `OB24_DECISION_TABLE_2026-10-17.md`: D6's Z2 recommendation is
> **withdrawn** — a measurement (`anim_zoom_sim.py`) shows snapping k to n/120 does not remove
> width jitter during animated zoom (it holds only for pans at a fixed k); D5 now recommends not
> persisting the anchor; D2's basis is stated (M20, OB-43). The claims below about Z2 ("exact
> widths, no jitter at all") are wrong for a changing k.

Status: **PROPOSAL FOR OWNER APPROVAL. Nothing here is adopted; no foundation text is changed
by this file.** It consolidates `ZOOM_CONTRACT.md` (A1, the representable domain) and
`OB24_DECISION_2026-10-16.md` (the anchor) into one decision, resolves the conflict between them
(§0), and gives the exact amendment texts (§3) and the approval record (§5) that a governance
branch would apply. OB-24 stays OPEN until the owner approves and the layers are adopted.

Branch: `experimental/inc-9-zoom-contract` (code for A1: `a156e93`; evidence: INC-9 harness on
`experimental/inc-9-ob24-evidence`).

## 0. Conflict resolved

`ZOOM_CONTRACT.md` §4/§5 (A2, written first) says the lattice reference is fixed "per camera …
persisted with the camera". `OB24_DECISION_2026-10-16.md` recommends that it belongs to the
**output's projection state**. The second is right and supersedes the first: `R_O` is a function
of `v_O`, `view_O` and `k_O = zoom_C · scale_O`, i.e. of the output; with one camera per output
(M5) the two coincide numerically, but camera state must not carry output-specific projection
state (a camera re-bound to another output, or a mode change, would carry a stale anchor).
`ZOOM_CONTRACT.md` §4 and its A2 text are **withdrawn** in favour of §3 here.

## 1. Decisions and recommendations

| # | Topic | Decision proposed |
|---|---|---|
| D1 | **Supported zoom domain** | Camera zoom 2⁻²⁰ … 2¹⁶ (finite); output mode sides 1 … 2¹⁵ px; output scale 0.25 … 8; centre within INV-COORD-1. Proven overflow-free at every corner (debug, release + overflow checks, release); enforced where values enter Core (`a156e93`). |
| D2 | **Invalid persisted camera entries** | An entry with a zoom (or centre) outside the domain is an invalid entry; the table is rejected **whole** and kept for inspection — the existing all-or-nothing rule (M20, INV-FAIL-1, OB-43). Dropping only that entry would amend INV-FAIL-1; not recommended. A zoom *writer* (INC-10) clamps to the domain and never produces such a value. |
| D3 | **Per-output pixel-rounding anchor** | `R_O` is owned by the output's projection state (`ProjectionAnchor { r, k, view }` beside the output model in Core), not by the camera. The camera keeps `(centre, zoom)` only. Render and hit test read the same anchor (ProjectionFrame, INV-HIT-1). |
| D4 | **Anchor lifetime and reconstruction** | Set to `floor(v_O / 120)·120` when the output's view is (re)defined: output connected, camera bound to it, `k_O` changed (zoom, output scale), `view_O` changed (mode, transform); and when `|v_O − R_O|` would exceed 2³⁷ px (a pan across the Canvas: one defined re-anchor). Kept across pans otherwise. Never recomputed per frame of a pan. |
| D5 | **Output reconfiguration and disconnection** | Reconfiguration (mode, scale, transform) re-anchors that output only. Disconnection drops its anchor with the output (no orphan state). Reconnection derives a fresh anchor (or restores a persisted one, D7). Each output re-anchors independently; a window spanning two outputs is rounded per output, as today. |
| D6 | **Animated zoom continuity and jitter** | **Z2**: an animated zoom (k changing every frame) passes only through effective scales `k ∈ n/120` while `k ≥ 1/120`, where every width is exact, so no relative jitter exists; each frame re-anchors (k changed), positions move continuously (the camera centre stays within half a physical px, F-LATTICE-1). The animation ends on a resting zoom that may be any value in the domain (one final re-round). Below `k = 1/120` no lattice exists: frames there re-anchor every frame and the ±1 px relative jitter is **accepted** (Z1) and stated as a residual, pending a T3 visual verdict. Z3 (unsnapped rendering during motion) is not chosen: it needs sub-pixel placement in the renderer (F-RENDER-1 amendment) and hit testing on unsnapped geometry. |
| D7 | **Camera serialization compatibility** | No on-disk format exists yet (Level D, OB-43; `main` starts with an empty table). Rules fixed now: the camera table is versioned and stays `(OutputKey, centre, zoom)`; the projection anchor, if persisted, is a **separate, optional, versioned** table `(OutputKey, R, k, view)`, restored only when `k` and `view` match on connect and `|v_O − R| ≤ 2³⁷`, otherwise re-derived. A file without the anchor table is valid (one-time ±1 px shift of some widths relative to the previous session). A reader ignores an anchor table of an unknown version; an unknown camera-table version rejects the file (D2). Recommendation: persist the anchor (avoids the restart shift); the owner may choose re-derive-only. |

## 2. Evidence and what it does not cover

- Sealed lattice: ±1 px width/gap changes 3–11 times per pan at `k ∉ n/120`; none at `k ∈ n/120`
  (INC-9 harness, Smithay geometry). Pinned reference: none. Render/hit agreement exact.
- Domain: `the_accepted_domain_never_overflows` (900 corner views × 7 edges + 20,000 random) in
  debug, release + overflow checks, release; four seeded controls DETECTED (`a156e93`).
- **Not covered:** multi-output pans with different scales (the algebra is per output; no
  harness run), the T3 visual verdict on Z1 below `k = 1/120`, Z2's step feel for the zoom
  gesture (INC-10), a GPU renderer (INC-13). The owner may require the multi-output harness run
  before adoption; it is a small extension of the INC-9 harness.

## 3. Amendment texts (for a governance branch after approval)

Layer numbering is assigned at adoption (`C12.F14`/`C12.F15` if adopted after INC-6's layers);
`<DATE>`/`<BASE>` filled then.

**A1 — representable domain** (FOUNDATION_ARCHITECTURE P1, living F-VIEW-1, STATE_MACHINES M4):

- P1: "Camera zoom is unbounded; the hint is not." → "Camera zoom is bounded only by the
  representable domain 2⁻²⁰ … 2¹⁶ (F-VIEW-1); the hint is bounded by P1's device limit."
- F-VIEW-1, derived-range definition, replaced by: "Domain: output mode sides 1 … 2¹⁵ px, output
  scales 0.25 … 8, camera zoom 2⁻²⁰ … 2¹⁶, camera centre within INV-COORD-1. Within it |v_O| ≤
  2³⁷ (exact in f64), and every projection intermediate stays below 2⁵⁷ (table in
  evidence/phase-f/INC-9/ZOOM_CONTRACT.md). A value outside the domain is refused where it
  enters Core (persisted table: the table is rejected, M20; output descriptor: the output is not
  connected or not reconfigured, M4); a zoom writer clamps its result to the domain."
- M4: "validates the hardware facts" gains "(mode sides 1 … 2¹⁵, scale 0.25 … 8)".

**A2 — the projection anchor** (living F-LATTICE-1; INV-RENDER-1's extracted text):

- F-LATTICE-1, first definition, replaced by: "`R_O` is output O's projection anchor, an
  integral Canvas lattice reference owned by O's projection state (not by the camera). It is set
  to `floor(v_O / 120) · 120` when O's view is (re)defined — O connected, a camera bound to O,
  `k_O` changed, `view_O` changed — and when `|v_O − R_O|` would exceed 2³⁷ px; it is kept
  otherwise, and dropped with O. `δ = 2⁻²⁰`; `k_O = zoom_C · scale_O`."
- F-LATTICE-1, "Known residual" definition, replaced by: "With the anchor kept across pans,
  widths and gaps do not change while panning at a fixed `k_O`. An animated zoom follows
  F-ZOOM-ANIM-1."
- INV-RENDER-1 extracted text: "… with R O's projection anchor (F-LATTICE-1), δ = 2⁻²⁰, k =
  zoom·scale".

**A3 — animated zoom** (new living formula, owner = core navigation, consumed by INC-10):

- F-ZOOM-ANIM-1: "An animated zoom (a camera zoom changed on successive frames by one
  interaction) presents, while `k_O ≥ 1/120`, only effective scales `k_O ∈ n/120` (n integer)
  on every frame; its last frame may present any zoom in the domain. Below `k_O = 1/120`, frames
  re-anchor at every step and a ±1 physical px relative change of widths is an accepted
  residual. Each step is a non-client stimulus of the zoom's epoch (INV-SCALE-3)."

**A4 — persistence** (M20; OB-43's format, Level D):

- M20 gains: "The camera table is versioned and holds `(OutputKey, centre, zoom)`; an entry
  outside the domain (F-VIEW-1) is invalid and the file is rejected whole. An optional,
  separately versioned projection table `(OutputKey, R, k, view)` is applied at `output_connected`
  only when `k` and `view` equal the connected output's and `|v_O − R| ≤ 2³⁷`; otherwise the
  anchor is derived. An unknown projection-table version is ignored."

**Test-plan changes at adoption:** OB-24 → CLOSED by architecture change control for pans at a
fixed zoom (A2) and for animated zoom at `k ≥ 1/120` (A3), with the evidence of §2; a new
obligation (proposed **OB-54**, T3) carries the visual verdict on the accepted residual below
`k = 1/120` and the multi-output harness run if the owner does not require it before adoption.
INC-10's exit tests gain: anchor kept across a zoomed pan (no width change), re-anchor at each
event of D4, Z2 frames on the lattice, A4 restore/derive rules.

## 4. INC-10 dependency gate

INC-10's entry conditions (FOUNDATION_IMPLEMENTATION_PLAN): **INC-3 exit, INC-6 exit, and
(G-OB24 resolved or the conservative fallback active).** Today: INC-3 RATIFIED; INC-6 NOT
ratified on `main` (its ratification is recorded only on its governance branch once the owner
authorizes it and it is integrated); G-OB24 not resolved (this proposal). **INC-10 must not
start.** It may start after (a) INC-6 is ratified and integrated, and (b) A1–A4 are adopted
(or the owner chooses the conservative fallback instead).

## 5. Approval record (to be completed by the owner; not filled by the agent)

| Item | Owner decision | Date |
|---|---|---|
| D1 / A1 domain 2⁻²⁰ … 2¹⁶, modes ≤ 2¹⁵, scales 0.25 … 8 | | |
| D2 invalid persisted entry rejects the table (keep M20) — or entry-only (amends INV-FAIL-1) | | |
| D3 / A2 anchor owned by the output's projection state | | |
| D4 / D5 lifetime, reconfiguration, disconnection | | |
| D6 / A3 animated zoom: Z2 (+ Z1 residual below 1/120) — or Z1 / Z3 | | |
| D7 / A4 persist the anchor (separate table) — or re-derive only | | |
| Multi-output harness run required before adoption: yes / no | | |
| OB-54 opened for the T3 residual verdict | | |
