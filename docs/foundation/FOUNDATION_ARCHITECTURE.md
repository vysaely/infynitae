# Architecture

What the compositor is made of and how the pieces fit, as it is now. The rules each piece must
keep are by ID in the other documents here: invariants (`INV-*`,
[FOUNDATION_INVARIANTS.md](FOUNDATION_INVARIANTS.md)), state machines (`M*`,
[FOUNDATION_STATE_MACHINES.md](FOUNDATION_STATE_MACHINES.md)), objects (`OBJ-*`,
[FOUNDATION_OBJECT_MODEL.md](FOUNDATION_OBJECT_MODEL.md)), decisions (`ADR-*`,
[FOUNDATION_DECISIONS.md](FOUNDATION_DECISIONS.md)). This document holds the namespace map
(§P0), the formulas (`F-*`, §F) and the provisional values (`PROV-*`, §PROV).

How the design was worked out, including the sections code comments cite as `C1`…`C16` and the
amendment layers `C12.R1`…`C12.F17`, is in
[../archive/FOUNDATION_ARCHITECTURE_HISTORY.md](../archive/FOUNDATION_ARCHITECTURE_HISTORY.md).

## The idea

Windows live on one unbounded canvas, the Stage. Each screen is a camera over it: a centre and a
zoom. Users move cameras, not windows; clients are ordinary Wayland clients and can never move a
camera.

## Crates

```
infynitae (binary) ──► backends ──► infynitae-wayland ──► infynitae-core
                        (drm, winit, headless)   │                ▲
                                                 └── infynitae-ipc ┘
```

- **infynitae-core** is the whole spatial and policy model, and nothing else: no Wayland, no I/O,
  no external crate, no `unsafe`. It takes events (physical input, client snapshots, output
  changes, shell commands) and returns `Effect`s (configure this window, enter this surface on
  that output, move the pointer…). Everything it decides is testable without a display.
- **infynitae-wayland** is the Smithay adapter. It turns client requests into snapshots for Core,
  applies Core's effects to clients, renders Core's render list, and runs the session loop, the
  lock, screencopy, the clipboard, XWayland supervision and the pointer drawn on the console.
- **Backends** own a target: real screens and input (DRM/KMS, libinput, libseat), a window inside
  another desktop (winit), or nothing (headless, for the tests). They feed physical events in and
  present frames.
- **infynitae-ipc** is the shell socket: JSON lines, state snapshots and events out, camera, focus
  and window commands in.

`tools/verify-architecture` enforces these edges and the external crates each may name.

## Authority

Who may change what is fixed by types. Core's entry points take a port: `InputPort` (physical
input), `ShellPort` (the user's shell) or `OutputLifecycleToken` (outputs appearing, changing,
leaving). `Core::new` mints each exactly once; they are not `Clone`, not `Send`, and bound to their
Core. An entry point mints an `Authorized` proof, and camera state can only be changed by code that
holds one (INV-CAMERA-1, INV-OUTPUT-2). A client request never reaches a camera. The compile
probes in `enforcement/probes` prove the forbidden paths do not compile.

## Space, rendering and hit-testing

Canvas positions are Q16 fixed-point (`geom`), so a position is exact and the same everywhere.
Each output's camera gives a view (`projection`, F-VIEW-1); windows are projected through a
per-output lattice (F-LATTICE-1) into one canonical rectangle per surface, and the same rectangle
is used to draw and to hit-test, so what you click is what you see to the physical pixel
(F-RENDER-1, F-HIT-1). The Stage is the only store of where windows are (INV-STAGE-1).

## Visibility, frames and scale

After every change that can move what is on a screen, Core recomputes membership: which surfaces
are shown on which outputs (INV-RENDER-3, F-MEMBER-1). It sends `enter`/`leave` from it, drives
frame callbacks only for surfaces that are shown (an off-screen surface gets a slow fallback,
INV-IDLE-1), and derives each window's preferred scale from the outputs and zoom it is seen at
(INV-SCALE-1, F-SCALE-1), with hysteresis and epochs so a zoom gesture does not storm clients.

## Input and focus

Physical input goes to Core first (`entry`). Core owns key and button ownership (a release always
goes where its press went, INV-INPUT-1), the interactions (pan, move, resize, M22), the bindings,
and keyboard focus (the focused element, popup grabs on top, M12). Pointer focus and constraints
(locks and confinement for games) are reconciled after every change (INV-INPUT-5, M10). Super
always belongs to the compositor; holding it frees a captured cursor.

## Windows

Core follows each toplevel through its lifecycle (M1, M2): created, configured, mapped, unmapped,
destroyed. It places new windows (cluster placement by default, F-PLACE-1), grants and ends
fullscreen as a presentation on one output (M9, INV-PRES-1, F-PRESENT-1), handles maximize, fit
and restore of the view (F-FIT-1), and arranges layer surfaces (bars, wallpaper, overlays) and
their exclusive zones.

## Around it

The session (`infynitae-wayland::session`) owns every process it starts and their children
(they end with it). The lock (ext-session-lock-v1) replaces every screen with the lock surface
until the holder unlocks. XWayland runs as a separate client (xwayland-satellite). Everything a
desktop needs beyond the compositor (wallpaper, portal, polkit, recorder, the scripts) is in
`live/`, outside the crates.

## §P0 Namespace authority map

Every identifier family has **exactly one canonical living home**. Other documents cite identifiers and never redefine them.

The last-but-one column names where an ID family was first written down; those places are history
(most of them in `docs/archive/`) and not normative.

A definition counts only inside a `living:` region of the home document, using the namespace's definition marker. A definition marker for the same namespace anywhere else in a living region is a checker failure.

<!-- living:NS -->
| Namespace | ID pattern | Canonical living home | Definition marker | First written down (history, mostly in docs/archive) | Other documents may |
|---|---|---|---|---|---|
| NS | `NS` | FOUNDATION_ARCHITECTURE.md | this table | — | reference |
| INV | `INV-[A-Z]+-[0-9]+` | FOUNDATION_INVARIANTS.md | heading `### INV-…` | ARCHITECTURE C12 (615b0ed), C12.R1 (84ce620), C12.R2 (5827926); legacy I1–I10 in BLOCKERS | reference only |
| F | `F-[A-Z]+-[0-9]+` | FOUNDATION_ARCHITECTURE.md | heading `### F-…` | ARCHITECTURE C3.2/C3.6/CP3A-1/C5; BLOCKERS B1.4/B1.5/B2.2/B2.3/B6.1; invariant rows in C12/C12.R1/C12.R2 | reference only |
| PROV | `PROV-[0-9]+` | FOUNDATION_ARCHITECTURE.md | heading `### PROV-…` | ARCHITECTURE C14.5; BLOCKERS ledger category 3 | reference only |
| OB | `OB-[0-9]+` | FOUNDATION_TEST_PLAN.md | heading `### OB-…` | ARCHITECTURE CP3A-4, C11.2, C12.R1.3, C12.R2.2; audit/1R CORRECTIONS_1R | reference only |
| TIER | `T[0-5]` | FOUNDATION_TEST_PLAN.md | heading `### T…` | ARCHITECTURE C11.1 | reference only |
| TST | `TST-[0-9]+` | FOUNDATION_TEST_PLAN.md | heading `### TST-…` | experiment sources and evidence files | reference only |
| ADR | `ADR-[0-9]+` | FOUNDATION_DECISIONS.md | heading `## ADR-…` | — | reference only |
| EVID | `EV-[PHSW][0-9]+`, `EB-[0-9]+`, `EB-H1`, `EC-[0-9]+`, `ED-[0-9]+`, `ER-[0-9]+`, `ER2-[0-9]+` | FOUNDATION_EVIDENCE.md | catalogue table row `\| ID \|` | EV: this same document §5 (Phase A); EB: BLOCKERS §0/B8.1/B10.1; EC: ARCHITECTURE evidence tables; ED: audit/README; ER: audit/1R; ER2: audit/2R | reference only |
| RISK | `RB-[0-9]+`, `RC-[0-9]+`, `T[ATCPRF]-[0-9]+`, `NB-[0-9]+`, `A-C1`, `A-B1-1`, `C-NB-2` | FOUNDATION_RISKS.md | heading `### …` | BLOCKERS §11/B13.2, ARCHITECTURE C-B/CP2, audit/D1 | reference only |
| PDF | `PD-F[0-3]-[0-9]+` | FOUNDATION_BLOCKERS.md | heading `### PD-F…` (living register) | BLOCKERS finding sections (append-only), audit/README registers | reference only |
| WSM | `W-SM-[0-9]+` | FOUNDATION_BLOCKERS.md | heading `### W-SM-…` (living catalogue) | BLOCKERS B12.4 / ledger cat. 5, ARCHITECTURE C12.R1.2 | reference only |
| SM | `M[0-9]+` | FOUNDATION_STATE_MACHINES.md | heading `## M…` | BLOCKERS B5.3/B9.6/B10.4/B11; audit D2, 1R, 2R | reference only |
| OBJ | `OBJ-[0-9]+` | FOUNDATION_OBJECT_MODEL.md | heading `### OBJ-…` | ARCHITECTURE C2.1/C2.2/C-AUTH; audit D3, 1R, 2R | reference only |
| PL | `PL-[0-9]+` | PROTOTYPE_DECISION_LEDGER.md | heading `### PL-…` | Phase A ledger table (same document) | reference only |
| INC | `INC-[0-9]+` | FOUNDATION_IMPLEMENTATION_PLAN.md | heading `### INC-…` | — | reference only |
<!-- /living -->

Nothing in `docs/archive/` is a canonical home.

## §F Formula registry (canonical `F-*`)

Each normative formula or coordinate algorithm is defined once here. Refinements applied when the registry was built are tagged `[PD-F1-…]`.

"Extracted invariant text" is the part of an invariant's wording that FOUNDATION_INVARIANTS.md writes as a reference to the formula (`[F-…]`).

<!-- living:F -->

### F-VIEW-1 — Per-output view origin
- **Definition:** `v_O = Q16(centre − view_O / (2·k_O))`, with `k_O = zoom_C · scale_O` (camera C bound to output O).
- **Definition:** `phys_O` is the output's physical mode (framebuffer size, `wl_output.mode`) **before** the output transform; `view_O = transform_size_O(phys_O)` is the view extent in physical px (90°/270° classes exchange width and height exactly once). `phys_O` is used only for the damage tracker and the framebuffer (F-RENDER-1); every view, cull, membership, hit and fit computation uses `view_O`. [E-F2-01; supersedes PD-F1-04]
- **Definition:** Representable domain: output mode sides 1 … 2¹⁵ physical px, output scale 0.25 … 8 (30 … 960 in 1/120 units), camera zoom 2⁻²⁰ … 2¹⁶, camera centre within INV-COORD-1. A value outside it is refused where it enters Core — an output descriptor at `output_connected` or `output_reconfigured` (M4), a persisted camera table, whole (M20). A camera value Core computes itself stays inside it by construction: the fit clamps its zoom to 2⁻²⁰ … 2¹⁶ and its centre into INV-COORD-1 (F-FIT-1). [C12.F14; C12.F15]
- **Definition:** Derived range: stored Canvas values obey |v| ≤ 2³⁶ (INV-COORD-1); inside the domain a derived view origin reaches at most |v_O| ≤ 2³⁶ + view_O/(2k_O) ≤ 2³⁶ + 2¹⁵/(2 · 2⁻²⁰ · 0.25) = 2³⁷, so its Q16 value stays within f64's exact integers (2⁵³). [PD-F1-02; C12.F14]
- **Definition:** Quantisation: rounding point R7 (round-ties-even to Q16).
- **Extracted invariant text:** INV-CAMERA-3 ⟦`v_O = centre − view_O/(2k)`⟧
- **Historical sources:** C3.2 [1]; B7.1 D-B7-CENTER; C12 INV-CAMERA-3
- **Amended:** C12.E1R (E-F2-01); C12.F14 (OB-24 D1); C12.F15 (the fit's clamp); previous definitions recorded verbatim in FOUNDATION_ARCHITECTURE §C12.E1R.2, §C12.F14.2 and §C12.F15.2

### F-LATTICE-1 — Reference-relative lattice and physical edges
- **Definition:** `R_O` is output O's *lattice reference* (not an element's anchor): an integral Canvas value, a multiple of 120 per axis, held in O's projection state, not in the camera. It is set to `floor(v_O / 120) · 120` when O's view is (re)defined — O connected; a camera bound or re-bound to O; `k_O` changed; `view_O` changed — and whenever keeping it would make `|v_O − R_O|·k_O` exceed 2³¹ (a conservative bound: C12.F14.4 item 1); otherwise it is kept, also while the camera pans. It is dropped with O and never persisted. `δ = 2⁻²⁰`; `k_O = zoom_C · scale_O`. [C12.F14]
- **Definition:** `C_O = round((v_O − R_O)·k_O + δ)` (integer: the camera on O's physical grid); `c_eff,O = R_O + Q16(C_O / k_O)` (snapped camera, rounding R5).
- **Definition:** `edge(x) = round((x − R_O)·k_O + δ) − C_O` for an integral Canvas edge x (rounding R4); a surface's physical rect is `[edge(x₀), edge(x₁)) × [edge(y₀), edge(y₁))`; origin and size are never rounded separately (F6).
- **Definition:** Evaluation order: compute `(x − R_O)·k_O` in f64 and cull against `[0, view_O)` **before** any i32 conversion; far elements reach |x − R|·k ≈ 2³⁷·k, beyond `i32::MAX`. [PD-F1-01; E-F2-01 symbol]
- **Definition:** Known residual: with the reference kept, widths and gaps do not change while the camera pans at a fixed `k_O`; each (re)definition above re-rounds them once (±1 physical px at k ∉ n/120, ED-1), and so does a restart (the reference is not persisted). Render, membership and hit test read the reference of the frame; f64 edges are not claimed equal to exact arithmetic (C12.F14.4 item 1), and agreement of the presented pixels with Core's rects at k ∉ n/120 is not established (C12.F14.4 item 3, INC-10 exit). [C12.F14]
- **Extracted invariant text:** INV-RENDER-1 ⟦`edge(x) = round((x − R)·k + δ) − C_O` with R output O's lattice reference, δ = 2⁻²⁰, k = zoom·scale⟧
- **Historical sources:** B1.5; C3.2 [1],[3]; C12 INV-RENDER-1
- **Amended:** C12.E1R (E-F2-01); C12.F14 (OB-24 D3/D4/D5); previous definitions recorded verbatim in FOUNDATION_ARCHITECTURE §C12.E1R.2 and §C12.F14.2

### F-RENDER-1 — Render location handed to Smithay
- **Definition:** `L(x) = (x − R_O)·k_O + δ − C_O` (f64) for each surface origin x (and y); the adapter builds `WaylandSurfaceRenderElement::from_surface(.., Point<f64, Physical>(L), ..)` and wraps it in an element whose `geometry()` is the surface's canonical rect — its F-LATTICE-1 rect, the one membership and hit testing use — and whose `draw` passes the destination it is given (that rect, `OutputDamageTracker::render_output`) to the wrapped element unchanged. Smithay's own rect `[round(L), round(fl(dst·k_O) + L))` is not presented; it only decides whether the wrapper remaps damage and opaque regions (C12.F15.3). Implemented by INC-10 (`convert::CanonicalRect`): geometry and draw destination equal the canonical rect exactly; damage of a pass-through surface is Smithay's own, which may cover up to one physical px beyond the rect at its far edges (C12.F17.5). [C12.F15; C12.F17]
- **Definition:** The output's damage tracker is built with `OutputDamageTracker::new(phys_O, Scale(k_O), transform_O)` — the **untransformed** physical mode — and rebuilt (or replaced) whenever the effective k_O or the descriptor changes, a camera zoom change included, before the next frame (C12.F15.4); Smithay derives the element space `transform_size(phys_O) = view_O` itself and GLES sets the viewport to `phys_O`. `OutputDamageTracker::from_output` (output scale only) is not used, because it ignores zoom. [PD-F1-03; E-F2-01; C12.F15]
- **Definition:** A canonical rect is cropped in i64 before Smithay's i32 geometry: each edge is clamped to `[−2³⁰, 2³⁰]` per axis, so the presented pixels lie in the half-open `[−2³⁰, 2³⁰)`, and the width and height are then capped at `i32::MAX = 2³¹ − 1`, so a rect covering the whole box is presented as `[−2³⁰, 2³⁰ − 1)` (outside every view, `view_O ≤ 2¹⁵`); a rect wholly outside the box is presented empty, and a visible surface is never rejected. `src` is cropped so that the visible part keeps exactly the source sampling of the uncropped element — proportional in f64 on the uncropped rect, through the renderers' sampling map for the buffer transform (Smithay's GLES `build_texture_mat` and Pixman `render_texture_from_to`), not Smithay's `CropRenderElement`, which misdraws the four rotating transforms (C12.F17.3). Verified in Pixman pixels for all eight buffer transforms, viewport sources and buffer scale with cuts on whole buffer pixels, and against the GLES texture-matrix algebra; not verified for cuts at fractional buffer pixels or on a GPU (OB-57). [C12.F15; C12.F17]
- **Extracted invariant text:** INV-RENDER-1 ⟦`(x − R)·k + δ − C_O`⟧
- **Historical sources:** B1.5; C3.2 [4],[5]; C3.6; ED-4
- **Amended:** C12.E1R (E-F2-01); C12.F14 (render/hit agreement claim); C12.F15 (canonical rect, option B); C12.F17 (INC-10 status, crop sampling and bounds, damage); previous definitions recorded verbatim in FOUNDATION_ARCHITECTURE §C12.E1R.2, §C12.F14.2, §C12.F15.2 and §C12.F17.2

### F-MEMBER-1 — Output membership
- **Definition:** Surface S is on output O iff its F-LATTICE-1 physical rect intersects `[0, view_O)`; element E is observed by O iff any surface of E is on O. Membership is per surface (popups included) and applied with `Output::enter/leave`.
- **Historical sources:** C3.2 [3]; D-B8-ENTER
- **Amended:** C12.E1R (E-F2-01); previous definition recorded verbatim in FOUNDATION_ARCHITECTURE §C12.E1R.2

### F-UNPROJECT-1 — Output-logical point to Canvas (T5)
- **Definition:** `q = c_eff,O + Q16(p / zoom_C)` for an OutputLogical point p on output O (rounding R1; the Q16 term and c_eff's snap give ≤ 2⁻¹⁷ px + the R5 snap).
- **Historical sources:** B2.2 T5

### F-PROJECT-1 — Canvas to output-logical point (T6)
- **Definition:** `p = (q − c_eff,O) · zoom_C` (exact up to the F-UNPROJECT-1 quantum).
- **Historical sources:** B2.2 T6

### F-HIT-1 — Physical-pixel hit selection
- **Definition:** Pointer (O, p): view-space physical `u = p · scale_O` (the space of F-LATTICE-1 rects; the output transform is applied only by the renderer), pixel `(⌊u_x⌋, ⌊u_y⌋)`. Traverse in render order (INV-RENDER-2). Select the first surface whose F-LATTICE-1 rect covers the pixel **and** whose input region (protocol add/subtract in request order; half-open cell test on the surface-local point `q − origin`, clamped into the surface; never round-to-nearest) contains `q` = F-UNPROJECT-1(p). Delivered coordinates: `q − origin`, clamped into `[0, w) × [0, h)`. [E1R-N1]
- **Definition:** Entries of a ProjectionFrame whose surface no longer exists fail closed (no delivery) until the next frame. [PD-F1-07]
- **Historical sources:** CP3A-2; D-CP3A-HIT
- **Amended:** C12.E1R (E1R-N1); previous definition recorded verbatim in FOUNDATION_ARCHITECTURE §C12.E1R.2

### F-ORIGIN-1 — Single surface-origin function
- **Definition:** `root_origin(E) = anchor(E) − geometry(E).loc`; subsurface: parent origin + `wl_subsurface` position (accumulated); popup: `root_origin + geometry.loc + popup_pos − popup.geometry.loc`. All origins are integral Canvas values. This is the only source of surface Canvas origins for render, membership, hit-test and input.
- **Extracted invariant text:** INV-COORD-6 ⟦anchor − geometry.loc + subsurface/popup offsets⟧
- **Historical sources:** B2.3; B6.1; C3.2 [2]

### F-PAN-1 — Direct-manipulation pan
- **Definition:** `dP_canvas = dp_phys / (s_O · z_C)` for physical displacement `dp_phys = dp_logical · s_O` on the interaction output O; camera `C = C_start − Q16(Σ dp_phys / (s_O · z_C))`, summed in f64 from the gesture origin and quantised once per update (R2). If s_O or z_C change within a gesture, the per-event terms are summed in f64.
- **Extracted invariant text:** INV-COORD-3 ⟦`dP_canvas = dp_phys / (s_O · z_C)`; `C = C_start − Q16(Σ dp_phys / (s_O · z_C))`⟧
- **Historical sources:** C12.R1 INV-COORD-3; ER-5 R1

### F-DRAG-1 — Element drag placement
- **Definition:** `anchor = R3(anchor₀ + Q16(Σ Canvas pointer deltas since the drag start))`, quantised once per update. **Open:** the definition across an Arrangement crossing between independently-camera'd outputs is the OB-34 decision (candidate `anchor = R3(q_now − grab_offset)`, not adopted).
- **Historical sources:** B1.3-7; PD-F2-13

### F-PLACE-1 — Default placement at first windowed presentation
- **Definition:** Per axis, the anchor is `a = c − min(s, u)/2`, rounded once by R3 (F-ROUND-1), where `c` is the camera centre of the placement output (Canvas px), `s` the window-geometry size at the element's first windowed presentation (INV-STAGE-1) and `u = view_O / scale_O` the usable output-logical extent (the whole view until a reserved region exists). For `s ≤ u` the window geometry is centred in the usable region, `a = (c − u/2) + (u − s)/2`; for `s > u` its origin is aligned with the region's origin, `a = c − u/2`.
- **Definition:** The placement output is the first connected output (lowest `OutputId`), whose camera centre is also the provisional anchor written at initial commit. With no output the provisional anchor is kept. Domain: camera zoom 1, where `u` is the Canvas extent of the view; under a persistent zoom ≠ 1 (gated by OB-24) the fitting case is still centred on the camera and the oversized case is not defined by this formula.
- **Definition (cluster placement, `PLACEMENT_POLICY="cluster"`, the default; owner rule 2026-10-31: "every new window opens in free space near the cursor, never overlapping and never moving an existing window"):** the target is the window centred on the pointer (on the placement output; else the camera centre as above), kept wholly inside the usable view when it fits. If the target keeps the 12 px gap from every mapped window it is the anchor; else the nearest candidate spot that does (flush with a window's edge or on the target's lines, gap kept), a spot wholly in the usable view preferred over a nearer one that is not; else the first spot right of every window. No other window moves. A window offered the room at its first map (INV-PROTO-1) is placed again by this rule at the offered size. Child windows (centred over their parent), notifications (bottom right) and an app's successor window (its closed window's place, when free) keep their own rules.
- **Definition (windows that change size by themselves or come back from fullscreen, cluster placement; owner rule 2026-10-31):** a window that grew to a size the compositor or the user did not ask for (not its held size, INV-PROTO-1, and no Super+right drag on it) pushes no window: if it reaches no window its old rect did not already reach, it stays; else it keeps its right, bottom, right-and-bottom edges or its centre, the first of these that is free; else it moves to the nearest free spot as above (targeted at its anchor). A window whose fullscreen ended keeps its place at its first windowed frame if that place is free at the size it draws, else it moves to the nearest free spot the same way; nothing else moves. A size the compositor or the user asked for (Super+Enter's maximize, a Super+right drag) still pushes the windows it newly reaches aside, and a Super+drag places a window where the user lets it go.
- **Historical sources:** C12.F1 (owner amendment 2026-10-13); PL-21
- **Amended:** C12.F5 (P9b); previous definition recorded verbatim in FOUNDATION_ARCHITECTURE §C12.F5.2

### F-ROUND-1 — Rounding points
- **Definition:** R1 unproject OutputLogical → CanvasPos (Q16, ties-even); R2 camera gesture update (Q16 of the f64 gesture sum); R3 placement commit (CanvasPos → integral CanvasPoint, half-even); R4 physical edges (F-LATTICE-1); R5 snapped camera c_eff; R6 host-device deltas: none; R7 view-origin quantisation (F-VIEW-1); R8 popup constraint target: inward rounding (ceiling at the minimum edges, floor at the maximum edges). Forbidden: F1–F7 (B1.4).
- **Definition:** External quantisations (not Infynitae rounding points): protocol `wl_fixed` (1/256) for client pointer and relative-motion values; `preferred_scale` in 1/120 steps; `wl_output.scale` = ⌈scale⌉.
- **Extracted invariant text:** INV-COORD-2 ⟦R7 = view-origin quantisation; R8 = inward rounding of a popup constraint target (ceiling at minimum edges, floor at maximum edges)⟧
- **Historical sources:** B1.4; C3.2 R7; C12.R1 R8

### F-LOCK-1 — Derived Canvas lock point
- **Definition:** `q_lock = surface_origin(S) + lock_point` (F-ORIGIN-1 + the constraint's SurfaceLocal lock point); never stored.
- **Extracted invariant text:** INV-COORD-5 ⟦`surface_origin + lock point`⟧
- **Historical sources:** C12.R1 INV-COORD-5

### F-RELOCATE-1 — Pointer relocation to a surviving output
- **Definition:** Domain: the survivor set S = Outputs ∖ {O_lost} is non-empty. On loss of the interaction output: `a = arr_origin(O_lost) + p` (evaluated before removal); choose the output of S whose Arrangement rect is nearest to a; clamp a into that rect half-open (each coordinate into `[origin, origin + size − 2⁻⁸]`, wl_fixed resolution); `p' = a − arr_origin(O')`. [PD-F1-05]
- **Definition:** S = ∅ is outside the domain: the formula is not evaluated and the seat enters `NoOutput` (INV-OUTPUT-1). [E-F2-05]
- **Historical sources:** C4.6; AUD-4; C12.R1 INV-INPUT-5
- **Amended:** C12.E1R (E-F2-05); previous definition recorded verbatim in FOUNDATION_ARCHITECTURE §C12.E1R.2

### F-RECONF-1 — Pointer on output reconfiguration
- **Definition:** On `output_reconfigured` of the interaction output O: `p' = clamp_halfopen(p, [0, logical_O'))`, each coordinate into `[0, extent − 2⁻⁸]` (wl_fixed resolution), with `logical_O' = view_O' / scale_O'` from the new descriptor; a p already inside is unchanged; no camera value is read or written; the true position is delivered by the batch-end routed motion (INV-INPUT-5); no relative motion is synthesized. [E-F2-03]
- **Historical sources:** C12.E1R INV-OUTPUT-2; spike test output_reconfigured_clamps_pointer_into_new_extent
- **Amended:** C12.E1R (E-F2-03); previous definition recorded verbatim in FOUNDATION_ARCHITECTURE §C12.E1R.2

### F-CONFINE-1 — Confinement clamp
- **Definition:** While a confinement is active on the interaction output O: `p := clamp_halfopen(p + Δ, project_O(region) ∩ [0, logical_O))` with `project_O` = F-PROJECT-1 applied to the region's Canvas rect (region + surface origin); no Arrangement crossing.
- **Historical sources:** C12.R1 INV-INPUT-5; ER-5 R5

### F-SCALE-1 — Scale requirement ρ
- **Definition:** `ρ(E) = max over observing presentations p of output_scale(p) × presentation_zoom(p)`; camera presentations use the camera zoom; a protocol-fullscreen presentation uses zoom 1.
- **Extracted invariant text:** INV-SCALE-1 ⟦max over observing presentations of `output_scale × presentation_zoom`⟧
- **Historical sources:** CP3A-1 S1

### F-SCALE-2 — Ladder and device ceiling
- **Definition:** `H_max(E) = min(H_cfg, H_dev(E))`. Device bound `H_dev(E) = ⌊120 · D_max / s_max(E)⌋ / 120` if `s_max(E) > 0`: the largest multiple of 1/120 that does not exceed `D_max / s_max(E)`, so that `h · side ≤ D_max` holds for every logical side of every buffered surface of E before the client rounds its buffer size; `+∞` if `s_max(E) = 0`. If the quotient is 0 (`s_max(E) > 120 · D_max`) the bound is `1/120`, the wire minimum: that buffer exceeds the device and no emittable hint would fit (residual, OB-13). `s_max(E)` = the largest logical side over all surfaces of E's tree and over every size Core has asked E for while that request stands: the size of the protocol-fullscreen configure while E is granted (C5.2: the output's logical size; at the initial commit, of the grant made in the initial configure), and the saved size offered in the configure that ended E's fullscreen (M9), until E commits a tree whose window-geometry size differs from the one it replaces, is unmapped, or is destroyed (0 iff none has a non-zero extent, e.g. every initial commit without a grant); `D_max` queried from the renderer (re-queried on renderer recreation, PD-F1-08). Domain: every E; the value is finite because `H_cfg` is finite (PROV-1). [E-F2-06; H-1]
- **Definition:** Ladder L(E): every connected output scale ≤ H_max(E) ∪ the integers 1…⌊H_max⌋ ∪ geometric fill so that consecutive rungs differ by a ratio ≤ r, all rungs ≤ H_max(E) (constants: PROV-1, PROV-2).
- **Extracted invariant text:** INV-SCALE-2 ⟦`H_max(E) = min(H_cfg, H_dev(E))`⟧
- **Historical sources:** CP3A-1 S2/S3; C12.R1 INV-SCALE-2
- **Amended:** C12.E1R (E-F2-06); C12.F2 (H-1); C12.F11 (OB-13); previous definitions recorded verbatim in FOUNDATION_ARCHITECTURE §C12.E1R.2, §C12.F2.2 and §C12.F11.2

### F-SCALE-3 — Target, emission and initial hint
- **Definition:** Target `t(ρ)` = smallest rung of L(E) ≥ ρ/(1 + σ), else H_max(E) (σ = PROV-3). Emission: `preferred_scale = round(h·120)`, `preferred_buffer_scale = b(h)` with `b(h) = ⌈h⌉` if `⌈h⌉ ≤ H_dev(E)`, else `max(1, ⌊H_dev(E)⌋)`, only on change. Initial hint: `h := t(ρ_pred)` with ρ_pred = F-SCALE-1 over the presentations whose view intersects the placed window-geometry rect (or the anchor point when its size is 0×0); if none, `t(max connected output scale)`. If the initial configure grants protocol fullscreen on output O (C12.F6), ρ_pred = max(`scale_O`, F-SCALE-1 over the current views of the other outputs whose view intersects the placed window-geometry rect, or the anchor point when its size is 0×0). The grant's own presentation contributes `scale_O` at zoom 1; O's current view does not count. [E1R-N3]
- **Definition:** Residual: if `s_max(E) > D_max` no scale ≥ 1 fits the device (the client's own logical size exceeds it at scale 1); `b = 1` and the residual is carried by OB-13.
- **Historical sources:** CP3A-1 S4/S8; C12.R1 INV-SCALE-5
- **Amended:** C12.E1R (E1R-N3); C12.F12 (pre-map grant); previous definitions recorded verbatim in FOUNDATION_ARCHITECTURE §C12.E1R.2 and §C12.F12.2

### F-FIT-1 — Viewport-fit (one-shot)
- **Definition:** For output O and mapped element W, with g = W's window geometry in Canvas px at the instant of the authorized action, `view_w,O` and `view_h,O` the view extent in physical px (F-VIEW-1) and `scale_O` the output scale: if O is not connected, O presents a protocol-fullscreen element (INV-PRES-1), W is not mapped, or `g.w ≤ 0` or `g.h ≤ 0`, the action is refused — no camera mutation, nothing recorded or evicted, no pointer reconciliation; a restore is refused likewise while O presents. [C12.F15; C12.F17]
- **Definition:** Otherwise `z_raw = min(view_w,O / g.w, view_h,O / g.h) / scale_O` (the view's Canvas extent is `view_O / k_O`, so `scale_O` is divided out exactly once) and `zoom = clamp(z_raw, 2⁻²⁰, 2¹⁶)` (F-VIEW-1's domain); `centre` = the centre of g in Q16 (exact: g is integral), each component clamped into INV-COORD-1 (`|c| ≤ 2³⁶` px). [C12.F15]
- **Definition:** One mutation of O's camera to that centre and zoom. Immediately before it, the camera's exact value is recorded as the newest entry of O's navigation history (OBJ-30), which holds at most 256 entries: when it already holds 256, the oldest entry is evicted first. One authorized restore applies and removes the newest entry, so repeated restores unwind the retained fits newest first; an evicted entry is not recoverable. The history is emptied when O's camera is unbound (M5), so it is never applied to another output or to a later connection (M8). No target is stored and no client change re-fits (INV-CAMERA-5). [C12.F16]
- **Definition:** The clamp belongs to this computation: zoom values from outside Core stay rejected outside the domain where they enter (M4, M20). After the mutation the pointer is reconciled once by M10's view-change rule — at most one true-position event, exactly one when the pointer is on O, no interaction owns it and the reconciliation leaves no lock active; a kept lock receives no absolute motion (INV-INPUT-5). [C12.F15; C12.F17]
- **Historical sources:** C5.1; C12.R1 INV-CAMERA-5
- **Amended:** C12.F15 (units, clamp, refusal, restore history); C12.F16 (history capacity and lifetime); C12.F17 (pointer reconciliation under a constraint; refusal during a presentation); previous definitions recorded verbatim in FOUNDATION_ARCHITECTURE §C12.F15.2, §C12.F16.2 and §C12.F17.2

### F-PRESENT-1 — Protocol-fullscreen presentation view
- **Definition:** A per-output view with zoom 1: `v_P = anchor(W) − max(0, (view_O/scale_O − size(W)) / 2)` (componentwise), then F-VIEW-1's R7 quantisation and F-LATTICE-1 with k = scale_O; a surface larger than the output is origin-aligned and cropped. [PD-F1-09; E-F2-01 symbol]
- **Historical sources:** C5.2; C12.R1 INV-PRES-1
- **Amended:** C12.E1R (E-F2-01); previous definition recorded verbatim in FOUNDATION_ARCHITECTURE §C12.E1R.2

<!-- /living -->

## §PROV Provisional constants and policies (freeze map)

Changing a value **inside** its allowed range is tuning, closed through its owning obligation. Changing a policy whose current choice is written into an invariant is a design change: change the invariant too, and note why in docs/DECISIONS.md. No value freezes from harness or model evidence alone when its destination tier is T3 or above.

<!-- living:PROV -->
### PROV-1 — Client scale ceiling H_cfg
- **Candidate:** 3.0
- **Allowed range:** ≥ max connected output scale; the effective ceiling is always `min(H_cfg, H_dev(E))` (F-SCALE-2)
- **Owning obligation:** OB-12
- **Freeze tier:** T3 (client CPU/memory, X-16) + T4 (buffer memory, frame time)
- **Measurement:** X-16 zoom sweep: renegotiations, client CPU, buffer MB (fdinfo), sharpness at m ∈ {1, 1.04, 1.25, 2, 2.67}
- **Change class:** tuning
- **Status:** PROVISIONAL

### PROV-2 — Ladder ratio r
- **Candidate:** 1.25
- **Allowed range:** 1 < r ≤ 2
- **Owning obligation:** OB-12
- **Freeze tier:** T3/T4
- **Measurement:** as PROV-1
- **Change class:** tuning
- **Status:** PROVISIONAL

### PROV-3 — Sharpness tolerance σ
- **Candidate:** 0.05
- **Allowed range:** 0 ≤ σ < r − 1
- **Owning obligation:** OB-12 (with OB-7 visual)
- **Freeze tier:** T3/T4 visual
- **Measurement:** visibility of magnification m ≤ 1 + σ
- **Change class:** tuning
- **Status:** PROVISIONAL

### PROV-4 — Off-screen fallback period P
- **Candidate:** 1 s
- **Allowed range:** finite and > 0 (INV-FRAME-3)
- **Owning obligation:** OB-8 (after OB-5)
- **Freeze tier:** T5
- **Measurement:** X-7 game/network survival off-screen; off-screen CPU
- **Change class:** tuning (P = ∞ globally would violate INV-FRAME-3)
- **Status:** PROVISIONAL

### PROV-5 — ScaleInteractionEpoch settlement timeout T_settle
- **Candidate:** 300 ms
- **Allowed range:** finite and greater than the repeat interval of discrete spatial input (INV-SCALE-3)
- **Owning obligation:** OB-23
- **Freeze tier:** T3 (real clients) + T4 (real wheel/key timing)
- **Measurement:** renegotiations per discrete interaction, wheel/key/shell-stream inter-step timing distributions
- **Change class:** tuning (the timeout's existence is architectural)
- **Status:** PROVISIONAL

### PROV-6 — Renderer filters
- **Candidate:** Linear (downscale) / Linear (upscale)
- **Allowed range:** any filter supported by the renderer (no geometry effect)
- **Owning obligation:** OB-7
- **Freeze tier:** T3/T4 visual
- **Measurement:** visual comparison at m ∈ {0.67, 0.87, 1.04, 1.25, 2.67}
- **Change class:** tuning
- **Status:** PROVISIONAL

### PROV-7 — Pan cursor behaviour
- **Candidate:** cursor frozen on screen, world slides under it
- **Allowed range:** frozen-cursor or cursor-follows-world
- **Owning obligation:** OB-37
- **Freeze tier:** T3 (UX) / T5 (games)
- **Measurement:** user evaluation; game behaviour under pan
- **Change class:** **invariant amendment** if changed (INV-INPUT-3 KeepFocus and INV-COORD-5 physical pointer state assume the frozen cursor)
- **Status:** PROVISIONAL

### PROV-8 — Popup draw order
- **Candidate:** a popup tree renders immediately above its own element
- **Allowed range:** —
- **Owning obligation:** OB-38
- **Freeze tier:** Level D UX decision
- **Measurement:** occlusion cases with real toolkits (T3)
- **Change class:** **invariant amendment** if changed (INV-RENDER-2, INV-HIT-1 order)
- **Status:** PROVISIONAL

### PROV-9 — Integer preferred scale
- **Candidate:** ⌈h⌉
- **Allowed range:** —
- **Owning obligation:** OB-39
- **Freeze tier:** T3 (integer-only clients)
- **Measurement:** integer-only client sharpness/memory
- **Change class:** **invariant amendment** if changed (INV-SCALE-5 wording)
- **Status:** PROVISIONAL

### PROV-10 — Activation outcomes and shell UX
- **Candidate:** focus grant / attention / ignore; no auto-navigation
- **Allowed range:** auto-navigation only as explicit user policy executed through `ShellPort`
- **Owning obligation:** OB-40
- **Freeze tier:** Level D shell design + T3 (OB-27)
- **Measurement:** activation behaviour of real toolkits
- **Change class:** **invariant amendment** if outcomes change (INV-INPUT-9)
- **Status:** PROVISIONAL

### PROV-11 — Reserved suspension reason StableUnobserved(T)
- **Candidate:** disabled
- **Allowed range:** disabled, or finite T as configuration
- **Owning obligation:** OB-41
- **Freeze tier:** T3/T5 (evidence of benefit required)
- **Measurement:** which clients benefit; CPU/energy off-screen
- **Change class:** **invariant amendment** to enable (INV-FRAME-6 reason set)
- **Status:** PROVISIONAL
<!-- /living -->
