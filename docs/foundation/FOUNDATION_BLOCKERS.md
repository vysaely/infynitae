# Infynitae Native — Foundation Blockers (Phase B)

> **Phase C note (normative pointer).** The decisions recorded here (D-B…, I1–I10, R/F rules, K-rules, MaxGesture, D-B2-I9) are consolidated, and where refined **superseded**, by the canonical invariant registry in [FOUNDATION_ARCHITECTURE.md §C12](FOUNDATION_ARCHITECTURE.md). Cite `INV-*` identifiers as normative. This document remains the Phase B evidence and decision record. In particular, D-B2-I9's ½-px tolerance is superseded by INV-HIT-1, and D-B8-SCALE's wording by INV-SCALE-1…5.

Status: **Phase B complete (B1–B13). Phase B gate SATISFIED with explicit conditional resolutions (see the gate evaluation at the end). Stopped at the review gate.**
Checkpoints:
- 1 (B1–B6): `9f3f5d4`
- 2 (B7–B9): `90992b839af1bdbe07d128009d8b7460e21fcc88`
- 3 (B10–B13) and **Phase B seal**: `4a0807ed721ae257e1f4b0e5287d1dfd8c1038c1`

Evidence baseline: Phase A artifacts at commit
**`2c8c163784e260af4aaa8c1dc05c6281b0476120`** (`~/infynitae-native`, branch `main`).
Every `EV-*` reference resolves against that commit's `FOUNDATION_EVIDENCE.md`.
New Phase B evidence uses `EB-*` IDs and lives in [`evidence/phase-b/`](evidence/phase-b/).

**Conclusion classes** (per the Phase B authorization):
`PROTOCOL` (protocol-guaranteed), `SMITHAY` (Smithay-0.7.0-source-verified), `EXPERIMENT`
(experimentally verified), `DECISION` (derived architectural decision), `ASSUMPTION` (remaining assumption).

---

## 0. Phase B evidence produced so far

(EB-8 … EB-12 are listed in B8.1; EB-13 … EB-20 in B10.1.)

| ID | What | Where |
|---|---|---|
| EB-1 | Numeric experiment (E1–E9): precision, accumulation, path dependence, jitter, adjacency, large-coordinate edges, round trip, equality | `experiments/b1-numeric/`, output [`evidence/phase-b/b1-numeric-output.txt`](evidence/phase-b/b1-numeric-output.txt) |
| EB-2 | `Space` experiment (S1–S7) with a dummy `SpaceElement` against real Smithay 0.7.0 | `experiments/b2-space/`, output [`evidence/phase-b/b2-space-output.txt`](evidence/phase-b/b2-space-output.txt) |
| EB-3 | Smithay source: render-element rounding (`backend/renderer/element/surface.rs:93-139, 186-232, 263, 294`); `from_surface` takes `Point<f64, Physical>` and is public; `render_elements_from_surface_tree` takes `Point<i32, Physical>` | source |
| EB-4 | Smithay source: `Space` internals (`desktop/space/mod.rs:38-56, 185-200, 258-365, 494-512`) | source |
| EB-5 | Smithay/wayland-backend source: role destruction and client-kill paths (`shell/xdg/handlers/surface/toplevel.rs:156-188`, `popup.rs:69-95`; wayland-backend 0.3.17 `rs/server_impl/client.rs:749-760`, `handle.rs:45-58`); `InnerObjectId {id, serial, client_id, …}` | source |
| EB-6 | Smithay source: xdg unmap detection (`shell/xdg/mod.rs:1608-1640`), opt-in `ensure_configured` (`:1652-1673`), `Window::geometry` clamping (`desktop/wayland/window.rs:154-171`), popup helpers (`desktop/wayland/popup/manager.rs:27-262`), `PositionerState::get_unconstrained_geometry` (`shell/xdg/mod.rs:685-691`), `Scale::Fractional` rounds `wl_output.scale` up (`output.rs:174-182`) | source |
| EB-7 | Protocol text: xdg_surface/xdg_toplevel/xdg_popup interface descriptions, `xdg_toplevel.destroy`, `xdg_popup.{destroy,popup_done,reposition}`, `xdg_toplevel.set_parent`, `xdg_surface.set_window_geometry` (full) | `/usr/share/wayland-protocols/stable/xdg-shell/xdg-shell.xml` (wayland-protocols 1.49) |

---

## B1 — Canvas numeric model

### B1.1 Requirements (derived; not inherited from f64 or i32)

| ID | Requirement | Source |
|---|---|---|
| N1 | Effectively unbounded range, with an explicit, documented limit | master §1, §B1 |
| N2 | Precision independent of position: behavior at the edge equals behavior at the origin | test-ability; EV-H04 history; AI2 |
| N3 | Element placement exact and stable: save/load/move-and-back produce bit-identical positions; equality meaningful | persistence, snapping (Level D) |
| N4 | Sub-pixel camera for trackpad pan and future zoom | master §B1, §B11 |
| N5 | Deterministic and path-independent: the same input deltas give the same camera regardless of event batching | testability; backend independence (B12) |
| N6 | Lossless conversion into Smithay's `f64` logical space; explicit rounding into `i32` physical | EV-S08, EB-3 |
| N7 | Overflow impossible or explicitly handled | master §B1 |
| N8 | No relative jitter between elements while panning; snapped neighbours tile without gaps at fractional scales | product UX; EV-P10 |

### B1.2 Measured properties (EB-1)

| Property | f64 | f32 | i32 (Proto 01) | Q16 = i64 fixed, 2⁻¹⁶ px |
|---|---|---|---|---|
| Spacing at 1e9 px | 1.2e-7 | **64 px** | 1 | 1.5e-5 |
| Spacing at 2³⁶ px | 1.5e-5 | 8192 px | (overflow) | 1.5e-5 |
| Range | ±1.8e308 (precision degrades) | — | ±2.1e9 | ±2⁴⁷ ≈ 1.4e14 |
| Precision uniform over range | **no** (E1) | no | yes | **yes** |
| `a+b+c` associative | **no**: same 1e6 deltas, two batchings differ by **0.24 px** at 2³⁶ (E3) | no | yes | **yes** (E3: bitwise equal) |
| Commutative `a+b == b+a` | yes (IEEE-754 add is commutative) | yes | yes | yes |
| Drift, constant 0.1 px delta × 1e6, per-event accumulation | 1.3e-6 at 0; 2.4e-2 at 1e9; **6.1 px at 2³⁶** (E2b) | — | **−100,000 px** (every delta rounds to 0) | **6.1 px** (quantization bias of 0.1) |
| Same, Q16 with per-gesture f64 sum (Q16-G) | — | — | — | **0** (E2b) |
| Equality | `0.0 == -0.0` with different bits; `NaN != NaN` (E8) | same | total | total (`Eq`, `Ord`, `Hash`) |
| Serialization | exact via shortest round-trip repr (E8), but must exclude NaN/∞/−0 | — | exact | exact integer |

Interpretation:
- **f32 is disqualified** (64 px spacing at 1e9).
- **i32 is disqualified.** It has no sub-pixel camera (N4), it loses every sub-0.5-px delta (EV-P10 is quantified by E2b), and it overflows within N1.
- **f64 meets N1 and N6** but fails N2, N3 (equality semantics), N5 (non-associativity is measurable: 0.24 px) and N8 at large magnitude (E9: 4,288 tiling errors at 2³⁶ when edges are computed from absolute f64 coordinates).
- **Q16 alone** fails N5 in one case: per-event quantization of non-representable deltas has a systematic bias (6.1 px / 1e6 events). That is fixed by a quantization *rule* (Q16-G), not by a different representation.

### B1.3 Decision D-B1 (DECISION, evidence EB-1)

1. **Unit.** 1 Canvas unit = 1 logical pixel at zoom 1.0. Canvas units are client surface-logical units. B2 relies on this.
2. **Representation.** `i64` fixed point with 16 fractional bits (Q47.16). Step 2⁻¹⁶ ≈ 1.53e-5 px.
3. **Declared Canvas bound.** Every Canvas coordinate satisfies |v| ≤ **CANVAS_LIMIT = 2³⁶ px** (68,719,476,736 ≈ 6.9e10 logical px, ≈ 3.6e7 screen widths at 1920 px).
   Chosen because at 2³⁶ the f64 spacing equals the Q16 step (E1), so **every Canvas value converts to f64 exactly**. That makes the Smithay boundary (`Point<f64, Logical>`) lossless (N6).
   Headroom: coordinate + any protocol size (≤ 2³¹) < 2³⁷ px ⇒ raw < 2⁵³ ⇒ still exact in f64, and far below i64 overflow.
4. **Two Canvas types:**
   - `CanvasPoint` / `CanvasRect`: **integral** logical pixels (raw low 16 bits zero, enforced by construction). Used for element anchors and all surface origins derived from them. Integral because every protocol position Infynitae composes with (`wl_subsurface.set_position`, `xdg_popup.configure x/y`, window geometry, positioner rects) is an integer in logical units. Integral anchors make every surface origin integral, so surface-local pointer coordinates are an exact subtraction (B2).
   - `CanvasPos`: **Q16 precise.** Used for camera position and for the pointer's unprojected Canvas position.
5. **Zoom** (not implemented in Foundation 01) is an `f64` factor on the camera, finite and > 0. It is not part of Canvas arithmetic. The algebra in B2 admits it. Zoom-specific jitter rules are Level C (see B1.6).
6. **Overflow.** Domain operations are checked. A placement outside ±CANVAS_LIMIT returns `Err(OutOfCanvas)` with state unchanged. The camera saturates at ±CANVAS_LIMIT (pan stops at the edge). No wrapping arithmetic. Within the bound, i64 overflow is arithmetically impossible.
7. **Accumulation rules** (N5, E2/E2b):
   - *Camera gesture:* `camera = camera_at_gesture_start − Q16(Σ deltas since gesture start, summed in f64)`. Quantization happens once per update, from the gesture origin. **Never** quantize per event.
   - *Element drag:* `anchor = round_to_integral(anchor_at_drag_start + Q16(Σ canvas pointer deltas since drag start))`. Rounding happens once per update, from the drag origin.
   - Across gestures, the camera carries ≤ ½ Q16 step of error per gesture (E2: ≤ 9.1e-4 px after 50,000 gestures). That is not a meaningful invariant and is accepted.

### B1.4 Rounding points

**Permitted** (all others forbidden):

| # | Where | Rule |
|---|---|---|
| R1 | Unproject OutputLogical → `CanvasPos` | Q16 round-ties-even (E7: round trip ≤ zoom·2⁻¹⁷ px at every magnitude) |
| R2 | Camera gesture update | Q16 of the f64 gesture sum, once per update (B1.3-7) |
| R3 | Element placement commit (initial placement, drag update, programmatic move) | `CanvasPos` → integral `CanvasPoint`, round-half-even, once per update from the gesture origin |
| R4 | Projection to OutputPhysical for rendering | Physical **edges** rounded by the lattice rule (B1.5), never origin + size independently |
| R5 | Hit-test/render effective camera | Camera snapped to the output's physical grid (B2.6) |
| R6 | Host-device deltas → f64 | none (kept as delivered) |

**Forbidden:**

| # | Forbidden | Why |
|---|---|---|
| F1 | Writing any projected/rendered/rounded value back into Stage or camera state | I8 |
| F2 | Rounding element positions on camera change | I2 |
| F3 | Per-event quantization of pan or drag deltas | E2b (6.1 px bias; −100,000 px for integer rounding) |
| F4 | Converting absolute Canvas coordinates to f32, or to i32 before the camera is subtracted | E1, overflow |
| F5 | Computing physical edges from absolute f64 Canvas coordinates | E9 (4,288 tiling errors at 2³⁶) |
| F6 | Rounding origin and size separately to produce a rect | E5 (2,888 adjacency defects/scale with an i32 root origin) |
| F7 | Using `f64` equality on Canvas coordinates anywhere | E8 |

### B1.5 Projection rounding to physical pixels (decision, evidence E4/E5/E9)

For output O with effective factor k = zoom · scale (scale = n/120 under `wp_fractional_scale_v1`):

```
R      = lattice reference: floor(camera / 120) * 120     (integral; R*scale is an integer for every n/120 scale)
δ      = 2^-20                                            (constant tie-breaker; ties at exact .5 are otherwise
                                                           decided by f64 noise, E5: 542 defects without it)
C_O    = round((camera - R) * k + δ)                      (integer: camera on O's physical grid)
edge(x)= round((x - R) * k + δ) - C_O                     (x: integral Canvas edge; result: OutputPhysical px)
```

Properties, all verified at zoom 1 by E4/E5/E9 for all 241 scales n/120, n = 120…360:
- **No relative jitter** during sub-pixel pan: the distance between any two elements is independent of the camera (E4 scheme B: 0/1000 steps changed; per-element rounding A: up to 70/1000).
- **Snapped neighbours tile exactly**: the right edge of A equals the left edge of B (E5/E9: 0 defects).
- **Magnitude independence**: a scene at 2³⁶ renders identically to the same scene at the origin (E9: 0 differences).
- **Reference-switch invariance**: when the camera crosses a lattice boundary, R changes by 120 and (x − R)·k changes by n (an integer), so rendered geometry is unchanged. This holds at zoom 1. At zoom ≠ 1 it does not hold in general (B1.6).

Implementation consequence (SMITHAY, EB-3): Smithay rounds a surface tree's edges as `round(location + offset·k)` from an **f64** location, but `render_elements_from_surface_tree` only accepts an **i32** root origin. With an i32 root, E5 measures up to 2,888 adjacency defects per scale. Infynitae must therefore build surface render elements via the public `WaylandSurfaceRenderElement::from_surface(.., location: Point<f64, Physical>, ..)`, passing `location = (x_root − R)·k + δ − C_O`. Smithay's own edge rounding then reproduces `edge(x)` for the root and, algebraically, for every subsurface (offsets are integral logical). Subsurface identity is an algebraic argument, not yet measured. It is an obligation for T1 (C11).

### B1.6 Residual numeric assumptions

| ID | Assumption | Status |
|---|---|---|
| A-B1-1 | Zoom ≠ 1 needs its own anti-jitter rule, since 120·zoom·scale is not integral. Zoom is not Foundation 01 scope. | ASSUMPTION (Level C) |
| A-B1-2 | 2⁻¹⁶ px is fine enough for all camera/pointer purposes. It is 1/13,107 of a physical px at scale 5. | ASSUMPTION, low risk |
| A-B1-3 | libinput/winit deliver deltas as finite f64. Non-finite deltas are rejected at the backend boundary. | DECISION (enforce in adapter) |

---

## B2 — Coordinate algebra

### B2.1 Domains

| Domain | Origin / meaning | Representation | Owner |
|---|---|---|---|
| **HostEvent** | Backend-native (winit window-local f64 logical; libinput accelerated + unaccelerated deltas; absolute devices normalized) | backend types | backend adapter (B12) |
| **Arrangement** | Logical arrangement of physical outputs **for pointer crossing only** (which output lies beyond which edge) | `f64` logical | output configuration |
| **OutputPhysical(O)** | Framebuffer pixels of O after transform | `i32` (render), `f64` (pointer) | renderer/backend |
| **OutputLogical(O)** | Logical px of O's viewport, top-left = (0,0) | `f64` | output model |
| **Canvas** | Stage world (B1) | `CanvasPoint` (integral), `CanvasPos` (Q16) | **Stage** |
| **ElementLocal(E)** | Relative to element anchor = window-geometry origin (B2.3) | integral `i32`/`i64` | derived |
| **RootSurfaceLocal(E)** | Relative to E's root `wl_surface` origin | integral (positions), `f64` (pointer) | derived; Smithay's "window-relative" space |
| **SurfaceLocal(S)** | Relative to any surface S in E's tree (subsurface or popup or root) | integral (positions), `f64` (pointer) | derived |
| **ClientPointer** | `wl_pointer` surface_x/surface_y = SurfaceLocal(target) | `f64` | Smithay seat |
| **Buffer** | Buffer pixels (buffer scale/transform/viewport) | — | **Smithay only**. Infynitae never computes here |

**Is there a compositor-global domain?** Yes, but only two narrowly scoped ones:
- **Smithay's pointer "global compositor space" := Canvas** (EV-S08). DECISION, B2.5.
- **Arrangement** exists only to decide pointer crossing between outputs. It is **not** a spatial domain for windows and owns no element state. Without it, "the pointer leaves output A's right edge" has no answer, because cameras are not placed relative to each other.

### B2.2 Forward and inverse transformations

| # | From → To | Operation | Inverse | Repr. | Lossless? | Rounding | Owner |
|---|---|---|---|---|---|---|---|
| T1 | HostEvent → (O, OutputLogical) | backend adapter: winit maps the window to its single output; libinput deltas move the pointer in Arrangement, then O, p = locate(Arrangement point) | — (events are not re-emitted) | f64 | yes | none (R6) | backend adapter + pointer model |
| T2 | OutputLogical(O) ↔ Arrangement | `a = arr_origin(O) + p` | `p = a − arr_origin(O)` | f64 | yes | none | output configuration |
| T3 | OutputLogical(O) → OutputPhysical(O) | `u = transform_O(p · scale_O)` | `p = transform_O⁻¹(u) / scale_O` | f64 | yes (f64) | render: edges only (R4) | output model |
| T4 | Canvas → OutputPhysical(O) (render) | `edge(x)` of B1.5 | not inverted directly; see T5 | i32 | **lossy** (pixel grid) | R4 | renderer (pure fn of Stage + Camera + Output) |
| T5 | OutputLogical(O) → Canvas (input) | Inverting `edge(x)`: physical `u = p·scale_O` ↦ `x = R + (u + C_O)/k_O`. Hence `q = c_eff,O + Q16(p / zoom_O)` with `c_eff,O = R + C_O/k_O` (computed in Q16) | T6 | `CanvasPos` | ≤ 2⁻¹⁷ px (+ the Q16 rounding of `c_eff,O`) | R1 | camera/output model (pure) |
| T6 | Canvas → OutputLogical(O) (exact projection) | `p = (q − c_eff,O) · zoom` | T5 | f64 | yes up to T5 quantum | none | pure |
| T7 | Canvas → ElementLocal(E) | `l = q − anchor(E)` | `q = anchor(E) + l` | integral / f64 | yes | none | pure |
| T8 | ElementLocal(E) → RootSurfaceLocal(E) | `r = l + geometry_loc(E)` (window geometry origin in root-surface coords; may be negative) | `l = r − geometry_loc(E)` | integral | yes | none | Smithay state read (`Window::geometry()`) |
| T9 | RootSurfaceLocal(E) → SurfaceLocal(S) | `s = r − origin_in_root(S)`, where `origin_in_root(S)` = Σ subsurface offsets, or popup chain (B6) | `r = s + origin_in_root(S)` | integral / f64 | yes | none | Smithay (`Window::surface_under` result) |
| T10 | SurfaceLocal(S) → ClientPointer | identity | identity | f64 | yes | none | Smithay seat |

**Invalid operations that must fail to type-check** (DECISION, encoded per B2.7):
`Canvas ± OutputLogical`, `OutputLogical(A) ± OutputLogical(B)` for different outputs (runtime-tagged, see B2.7),
`CanvasPoint` from `f64` without an explicit rounding function, Canvas → OutputPhysical without an output, ElementLocal of E1 used with E2's anchor, writing any projected type into Stage, and passing a non-Canvas point to the Smithay pointer.

### B2.3 Anchor: what an element's Canvas position refers to (resolves Q-03)

**DECISION D-B2-ANCHOR:** an element's authoritative Canvas position is the Canvas location of the **top-left corner of its effective xdg window geometry**.

Justification:
- **PROTOCOL** (`xdg_surface.set_window_geometry`): "When maintaining a position, the compositor should treat the (x, y) coordinate of the window geometry as the top left corner of the window. A client changing the (x, y) window geometry coordinate should in general not alter the position of the window." Also: "Client-side decorations often have invisible portions like drop-shadows which should be ignored for the purposes of aligning, placing and constraining windows."
- **PROTOCOL** (`xdg_popup.configure`): popup x/y are "relative to the upper left corner of the window geometry of the parent surface". Popup placement composes naturally with this anchor.
- **SMITHAY + EXPERIMENT**: `Space` uses the same convention (`render_location = location − geometry.loc`, EB-2 S1), and `get_popup_toplevel_coords` returns offsets relative to the toplevel's geometry (EB-6). Every Smithay helper assumes this anchor.

Rejected alternatives:
- *Root surface origin* (Prototype 01's render anchor). A client changing its shadow width would move the visible window. That contradicts the protocol sentence above.
- *Buffer origin.* Buffer coordinates are scaled/transformed/viewported, so this is not a logical position.
- *bbox origin.* Popups and subsurfaces change the bbox continually, so the window would jump.

Derived quantities, all **computed, never stored**:

```
anchor(E)             : CanvasPoint             (stored, Stage-owned; the only stored spatial datum)
geometry(E)           : Rect in RootSurfaceLocal (Smithay Window::geometry(); loc may be negative)
root_origin(E)        = anchor(E) - geometry(E).loc            (Canvas, integral)
bbox_canvas(E)        = root_origin(E) + Window::bbox_with_popups()   (may extend left/up of anchor)
surface_origin(S)     = root_origin(E) + origin_in_root(S)     (Canvas, integral)
```

Negative offsets and surface trees larger than the window geometry are handled because nothing assumes `geometry.loc ≥ 0` or `bbox ⊆ geometry`. Rendering, hit-testing, popup placement, enter/leave overlap, input and movement all derive from `anchor` through these formulas. That is the fix for EV-P09.

Edge cases:
- **Unset window geometry**: spec says "the full bounds of the surface, including any subsurfaces"; Smithay returns the bbox (EB-6). The anchor is then the bbox origin. Correct per spec.
- **Geometry changes**: the anchor stays and the root surface moves (per spec). This is client-driven, not pan-driven.
- **Smithay deviation** (SMITHAY, EB-6): `Window::geometry()` re-clamps the set geometry to the *current* bbox on every call, while the spec says the effective geometry "will not be recalculated unless a new call to set_window_geometry is done". Observable only when a client shrinks its surface tree below its declared geometry. Recorded as risk RB-1, not blocking.

### B2.4 The Smithay `Space` question

Answers to the six questions, grounded in EB-2 (experiment) and EB-4 (source):

1. **What `Space` owns** (SMITHAY + EXPERIMENT):
   - an element list in z-order (`elements: Vec<InnerElement>`, back to front);
   - **one** `Point<i32, Logical>` location per element, anchored at the window-geometry origin (S1, S6);
   - a per-element cache of output overlaps (`outputs: HashMap<Output, Rectangle>`);
   - a list of mapped outputs, each with an `i32` location stored in the `Output`'s user data keyed by Space id;
   - **activation side effects**: `map_element`/`raise_element(.., activate=true)` call `set_activate(true)` on the raised element and `set_activate(false)` on every other element (S7). For `Window` that changes xdg `activated` state.
2. **Facilities depending on that state**: `refresh()` (enter/leave), `element_under`, `output_under`, `outputs_for_element`, `element_location`/`bbox`/`geometry`, `render_elements_for_region`/`_for_output`, `space_render_elements`, `render_output`. `LayerMap` (layer-shell) is stored per `Output` (`layer_map_for_output`, `desktop/wayland/layer.rs:48`) and **does not depend on `Space`**.
3. **Independent cameras inside `Space`?** Partially. An output's Space location acts as an **integer** camera, and outputs may overlap (S2). One element can be "entered" on both (S2), and moving one output works like an independent camera move (S4). But:
   - cameras are `i32` only: no sub-pixel camera (N4), no zoom;
   - element locations are `i32`: Canvas beyond ±2³¹ cannot be represented (N1 fails);
   - a point in Space lies on **both** outputs (S3: `output_under` returns `[A, B]`). Space has no notion of which camera the pointer looks through, so pointer state cannot live there.
4. **Would `Space` create a second authority?** Yes. If Stage is authoritative, Space holds a *mirror* of every element location, all camera positions and the z-order. It must be resynchronized on every Stage mutation and **every pan tick** (`map_output` per motion event, which per its docs also "reset[s] its damage memory"). It is a lossy mirror (i32, ±2³¹), and it couples z-order to activation (S7). That is exactly the secondary authority C2 forbids.
5. **A narrower derived/ephemeral role?** Rebuilding a Space per frame would only be justified if it supplied something Infynitae cannot. It supplies enter/leave (with a convention defect, below), hit-testing (with an ambiguous pointer space) and render collection (with i32 roots, F6). It cannot represent the Canvas range. **No narrower role is justified.**
6. **Replacements if `Space` is not used:**

| Space facility | Replacement | Basis |
|---|---|---|
| element locations, z-order | Stage (`anchor`, z-order list) | D-B2-ANCHOR, B3 |
| output locations ("cameras") | Camera model (B7) | B1, B7 |
| `refresh()` enter/leave | Infynitae visibility pass computing per-(element, output) overlap in **RootSurfaceLocal** coords, then calling the public `SpaceElement::output_enter` / `output_leave` on `Window` (or `Output::enter`/`leave`) | EV-S04 |
| `element_under` | Stage hit test over the per-output projected list + `Window::surface_under` | B2.5 |
| render collection | per-output projected list → `WaylandSurfaceRenderElement::from_surface` (f64 location) → `OutputDamageTracker::render_output` (already used without Space in Prototype 01) | B1.5, EB-3 |
| activation | explicit focus model (C4) | — |
| layer surfaces | `LayerMap` per output (Space-independent) | EB-4 |

**DECISION D-B2-SPACE: Infynitae does not use `smithay::desktop::Space`.**

Two Smithay defects this avoids:
- **EV-S12 confirmed experimentally (EB-2 S5).** For F (root origin (100,100), bbox origin (70,70)) with output A's left edge at x=120, Space passed `overlap.loc.x = 50` (relative to the **bbox** origin). The correct root-surface-relative value is 20. `Window::refresh` → `output_update` interprets overlap relative to the **root surface** origin. Any element whose surface tree extends left/up of its root surface gets enter/leave computed against a shifted rectangle. Infynitae must pass overlap in RootSurfaceLocal coordinates.
- **EB-H1, confirmed experimentally at checkpoint 2 (B8.5)**. Originally a static-only hypothesis: In `Window::refresh` (`space/wayland/window.rs:68-80`), popup overlap is shifted by the popup's `location`, which is relative to the toplevel's **geometry** (`popups_for_surface`). But `Window::surface_under` places a popup's surface origin at `geometry.loc + location − popup.geometry.loc` relative to the root surface. Popup enter/leave may therefore be offset by `geometry.loc − popup.geometry.loc` (non-zero for any CSD toplevel). To be tested with a real client (X-5, §8). Affects B8.

### B2.5 Pointer chain (resolves Q-05 and the Prototype 01 offset bug)

Smithay's contract (SMITHAY, EV-S08/EV-P08): `PointerHandle::motion(focus: Option<(surface, origin)>, MotionEvent{location})` delivers `location − origin` to the client, where both are in one "global compositor space".

**DECISION D-B2-PTR-SPACE:** that global space is **Canvas** (`CanvasPos` → `Point<f64, Logical>`, exact by B1.3-3).
Rationale: with integral anchors, `surface_local = q − surface_origin(S)` is exactly the client-correct coordinate at any zoom. Canvas units are surface-logical units, so no division by zoom appears in client coordinates. Implicit button grabs that freeze the focus origin (Smithay's default click grab) also stay correct when the camera moves underneath, because both terms remain in Canvas.

**DECISION D-B2-PTR-STATE:** the **authoritative** pointer position is `(OutputId, OutputLogical p)`. It is owned by the pointer/interaction model, **not** by Smithay and **not** by Canvas.
Smithay's location (Canvas) is **derived** per dispatch by T5. Why: one Canvas point can lie on several outputs (EB-2 S3), so Canvas → output is not a function. Also, a pan changes the Canvas point under a stationary pointer; the pointer itself does not move.

The chain (the master-prompt chain, refined):

```
Host event
  → [T1] backend adapter: (O, p) in OutputLogical(O)  [absolute], or Δ in Arrangement [relative device]
       (relative: a' = arr(O,p) + Δ; clamp to union of output rects; (O', p') = locate(a'))
  → interaction state machine (B10) decides: compositor-consumed (CameraPan, WindowMove, …) or client-routed
  → [T5] q = c_eff,O + Q16(p / zoom_O)                         Canvas (CanvasPos)
  → hit test over O's projected element list, top-down (the SAME list and order the renderer used for O):
       candidate E if q ∈ bbox_canvas(E)
       r = q − root_origin(E)                                   RootSurfaceLocal(E)   [T7+T8]
       (S, origin_in_root(S)) = Window::surface_under(r, ALL)   (Smithay: input regions, popups first)
  → surface_origin(S) = root_origin(E) + origin_in_root(S)     Canvas, integral
  → PointerHandle::motion(focus = Some((S, surface_origin(S))), location = q)
       client receives q − surface_origin(S) = SurfaceLocal(S)  [T9, T10]
```

Prototype 01 passed `origin_in_root(S)` as the focus origin together with an OutputLogical location (`state.rs:182-187`). The two terms were in different spaces, so the client saw a screen position as its local coordinate. The chain above fixes that structurally: both arguments are Canvas by construction (B2.7).

### B2.6 I9 restated precisely (render ↔ hit-test agreement)

**DECISION D-B2-I9.** For every output O:
1. Hit-testing on O traverses **the same projected element list, in the same z-order**, that rendering used for O's most recent frame. Membership and order agree exactly.
2. Both use the **same effective camera** `c_eff,O = R + C_O / k_O` (B1.5): the camera snapped to O's physical grid.
3. Positional agreement:
   - **exact** when k_O · (x − R) is integral for all edges, e.g. scale 1.0 or 2.0 at zoom 1. Then rendered edges are exactly the T6 projections of Canvas edges.
   - at fractional k_O, each rendered edge deviates from the exact projected Canvas edge by **≤ ½ physical px** (single rounding, B1.5), and hit-testing uses the exact edges. The bound is stated, testable (C11), and inherent to integer-pixel rendering. No representation removes it.

### B2.7 Type-system encoding (DECISION; mechanically enforced in Phase F)

- Distinct newtypes, no `From`/`Into` between domains, arithmetic only within a domain:
  `CanvasPoint`/`CanvasRect` (integral), `CanvasPos` (Q16), `CanvasVector`, `OutputLogicalPoint`, `OutputPhysicalPoint`, `ArrangementPoint`, `ElementLocalPoint`, `RootSurfaceLocalPoint`, `SurfaceLocalPoint`.
- `CanvasPoint: Add<CanvasVector>`, but no `Add<CanvasPoint>`, and no `Add` mixing domains.
- Output-bound values carry their `OutputId` at runtime (`OnOutput<T> { output, value }`). Output identity is dynamic (hotplug), so a phantom type cannot express it. Conversions take `&OutputView` and assert identity.
- Rounding is explicit: `CanvasPos::round_to_point(RoundingPoint::PlacementCommit)`. No `as` casts outside the numeric module.
- Smithay types (`Point<f64, Logical>`, `Point<i32, Physical>`) appear **only** in the Smithay adapter crate, through two functions: `canvas_to_smithay_global(CanvasPos)` and `projected_to_smithay_physical(..)`. The pure domain cannot name them (crate boundary, C1).
- Compile-fail tests (e.g. `trybuild`) for the invalid operations of B2.2 are a T0 obligation (C11).

---

## B3 — Object graph and ownership

### B3.1 Relationships (PROTOCOL + SMITHAY + DECISION)

```
Wayland Client 1 ──* wl_surface
wl_surface     0..1 role (permanent once assigned; PROTOCOL)
xdg_surface    0..1 per wl_surface; must get a role object before use (PROTOCOL)
xdg_toplevel   role object; may be destroyed while wl_surface lives; a new xdg_surface+toplevel may follow
xdg_popup      role object; parent = xdg_surface of a toplevel or popup (PROTOCOL)
wl_subsurface  links child wl_surface → parent wl_surface (surface tree)

Stage Element  1 ── 1 xdg_toplevel role object            (DECISION; Foundation 01 has no other element kind)
               1 ── 1 root wl_surface, for the role object's lifetime
               1 ──* surfaces in its tree (subsurfaces, popups, their subsurfaces) — NOT elements
Smithay Window 1 ── 1 Element, owned by the Smithay adapter, keyed by ElementId
xwayland-satellite windows = ordinary xdg_toplevels of the satellite client (no special case)
```

**A Stage Element is not a `wl_surface`.** It is a compositor object whose lifetime is bounded by one `xdg_toplevel` role object. **A `wl_surface` is not necessarily an element.** Subsurfaces, popups, cursor surfaces, DnD icons, layer-shell surfaces and role-less surfaces are not elements.

Surface → element lookup (O(1)): at `new_toplevel` the adapter stores the `ElementId` in the root surface's `data_map`, and keeps a map `xdg_toplevel ObjectId → ElementId`. Protocol `ObjectId`s carry a serial (`InnerObjectId {id, serial, client_id, …}`, EB-5), so they are safe keys. Subsurface/popup → element goes via the root surface (`get_parent` chain / `find_popup_root_surface`). Both associations are cleared in `toplevel_destroyed`.

### B3.2 Negative ownership

| State | OWNS | DOES NOT OWN |
|---|---|---|
| **Canvas placement** (`anchor`) | Stage | Smithay `Window`, `wl_surface`, Output, Camera, Renderer, `Space` (unused), Quickshell/shell, XWayland/satellite, client (window geometry *offset* is client state; the *position* is not) |
| **Z-order** | Stage | Smithay (no `Space`), renderer (reads), focus model (requests raise via Stage), client (`set_parent` is a hint only) |
| **Element identity** | Stage (mints `ElementId`) | Smithay `Window` (no identity role), `wl_surface`/`ObjectId` (protocol-scoped), title/app_id/PID, shell, persistence layer (has a separate restore key, Level D) |
| **Surface ↔ Element relationship** | Smithay adapter (association maps) | Stage (knows ids only, never surfaces), renderer, shell |
| **Surface tree structure** (subsurfaces, popups) | Client + Smithay protocol state | Stage, Infynitae adapter (reads only) |
| **Window geometry offset, size** | Client (protocol state via Smithay) | Stage (reads at derivation time; never caches) |
| **Popup position** | Derived: positioner + parent anchor (B6) | Stage (no popup position stored), renderer |
| **Pointer position** | pointer/interaction model `(OutputId, OutputLogical)` | Smithay seat (derived Canvas copy, per dispatch), Canvas, Camera |
| **Presentation coordinates** | nobody: computed per frame | Stage, Window, Output |

Mirrors: the adapter's association maps are *indices*, not spatial truth. Justification (C2): the alternative is an O(n) scan per input event and per commit (Prototype 01, EV-P16). They are invalidated in exactly one place (`toplevel_destroyed`) and contain no spatial state.

---

## B4 — Element identity

| Option | ABA safety | Cost | Notes |
|---|---|---|---|
| Monotonic counter, never reused | **absolute within a process** | O(1) hash lookup | No dependency; counter restarts per process |
| Generational slot map (index + version) | until a slot's version wraps (2³¹ reuses of one slot for u32 versions) | O(1) vec index | Extra dependency; wraparound reasoning required |
| `Arc` pointer / `Window` identity | none after free (address reuse) | — | Violates I7 |
| Protocol `ObjectId` | protocol-scoped; dies with the role object | — | Wrong lifetime owner (client) |
| UUID | absolute | larger, random | Unnecessary randomness inside a process |

**DECISION D-B4:**
- `ElementId(NonZeroU64)`, minted only by Stage from a monotonic counter starting at 1, **never reused** within a compositor process. Exhaustion (2⁶⁴−1 mints) returns `Err(IdSpaceExhausted)`. That is unreachable in practice: 1e9 mints/s for 584 years.
- No public constructor from integers in the domain API. External integers (IPC) become an **unresolved** `ElementRef` that must be resolved through `Stage::resolve(ElementRef) -> Result<ElementId, StaleElement>`.
- **Cross-process ABA:** the counter restarts with each compositor process, so an external consumer (shell) holding id 5 from a previous run would alias the new run's id 5. IPC identity is therefore `(InstanceId, ElementId)`, where `InstanceId` is a random `u64` drawn at startup. A mismatched instance resolves to `StaleElement`.
- Iteration determinism comes from the z-order list, not from `HashMap` order.
- `app_id`/`title` are metadata. Persistence matching (Level D) uses a separate `RestoreKey` and never an `ElementId`.

Obligations (T0, C11): ids never repeat across arbitrary insert/remove sequences (property test); `get(removed)` is `None` forever; `ElementRef` from a previous `InstanceId` never resolves.

---

## B5 — Element lifecycle

### B5.1 Facts the model rests on

- **PROTOCOL** (xdg_surface): mapping needs (1) a role, (2) committed role state, (3) a committed buffer. The initial commit must carry no buffer. A newly unmapped surface "must perform the initial commit again before attaching a buffer". Attaching or manipulating a buffer before the first configure is a client error.
- **PROTOCOL** (xdg_toplevel): unmapping discards "all attributes (e.g. title, state, stacking, …)" and cancels all active operations (move, resize). `xdg_toplevel.destroy` "destroys the role surface and unmaps the surface". Children of an unmapped parent are re-parented to its parent and are **not** restored on remap (`set_parent`).
- **SMITHAY**: null-buffer unmap is detected automatically (`initial_configure_sent = false`, EB-6), but only when renderer utils (`on_commit_buffer_handler`) are used. The buffer-before-configure error is **opt-in** (`ToplevelSurface::ensure_configured()`, EB-6). `toplevel_destroyed` runs from `Dispatch::destroyed`, then Smithay resets the surface's toplevel data (EB-5).
- **SMITHAY/wayland-backend**: on client death every object's `destroyed` runs in object-map order, **not** parent/child order (EB-5). `toplevel_destroyed` may therefore run after the root `wl_surface` was destroyed.

### B5.2 States

Visibility is **not** a lifecycle state. It is orthogonal (B9) and only defined for `Mapped` elements. The master prompt's `Created`/`RoleAssigned` collapse into the first state, because a `wl_surface` without an `xdg_toplevel` role object is not an element.

| State | Meaning | In Stage render/hit/visibility? | Has anchor? |
|---|---|---|---|
| `AwaitingInitialCommit` | role object exists; no initial commit yet | no | **no** (placement waits for app_id/parent/size hints committed in the initial commit) |
| `AwaitingFirstBuffer` | initial configure sent; no buffer yet | no | yes |
| `Mapped` | buffer committed after configure | **yes** | yes |
| `Unmapped` | was mapped; null buffer attached; role object alive | no | **yes, retained** (DECISION) |
| `Destroyed` | role object gone (destroy or client death) | — (removed from Stage) | removed |

**DECISION D-B5-REMAP:** an unmapped element keeps its `ElementId` and Canvas anchor, and is re-shown there on remap. The protocol discards *client-visible attributes* on unmap (title, states, stacking). Canvas placement is compositor state, not a protocol attribute, so retaining it is protocol-compatible. It also matches the product's persistent-placement intent (§3.1 of the evidence). On remap the element is placed on top of the z-order: "stacking" is discarded by the spec, so the compositor must decide, and top is the least surprising choice. Title/app_id/parent are re-read.

### B5.3 Transition table

| Current | Event | Next | State mutation | Protocol action | Side effects |
|---|---|---|---|---|---|
| — | `new_toplevel` | `AwaitingInitialCommit` | mint `ElementId`; insert record (no anchor); store surface/ObjectId associations | none yet | trace `element.created` |
| `AwaitingInitialCommit` | commit, no buffer | `AwaitingFirstBuffer` | choose anchor (placement policy; must not depend on camera *motion*, I3) | initial `xdg_toplevel.configure` (size may be 0×0 or policy size; states) + preferred scale (B8) | — |
| `AwaitingInitialCommit` | commit **with** buffer | (unchanged) | none | `ensure_configured()` → protocol error `not_constructed`; client is disconnected | → `Destroyed` via client death path |
| `AwaitingFirstBuffer` | commit, no buffer, configure acked or not | `AwaitingFirstBuffer` | none | resend configure only if pending changes | — |
| `AwaitingFirstBuffer` | commit with buffer after ack | `Mapped` | insert at z-order top | none required | visibility pass (B9) → enter (B8); focus policy (C4) |
| `Mapped` | commit with buffer | `Mapped` | none (anchor unchanged even if geometry.loc changed) | — | damage; visibility pass if size/geometry changed |
| `Mapped` | commit with null buffer | `Unmapped` | remove from z-order; keep anchor | Smithay resets initial-configure flag; `output_leave` for all outputs it had entered | cancel interactions targeting it (B10); drop focus (C4); dismiss its popups |
| `Unmapped` | commit, no buffer (new initial commit) | `AwaitingFirstBuffer` | keep anchor | initial configure | — |
| `Unmapped` | commit with buffer **without** new initial configure | (unchanged) | none | `ensure_configured()` → protocol error | client death path |
| any live | `xdg_toplevel.destroy` / `toplevel_destroyed` | `Destroyed` | remove record, anchor, z-order entry; clear associations; purge every reference (focus, grabs, interaction, parent links) | (if mapped) leave events are moot for a destroyed surface, so none required | cancel interactions; focus policy; dismiss its popups |
| any live | client death | `Destroyed` | same as above, via `toplevel_destroyed` (EB-5) | none (client gone) | handlers must not touch the possibly already-dead `wl_surface` |
| `Destroyed` | any lookup by `ElementId` | — | — | — | `Err(StaleElement)` (B4) |

**Invalid transitions** (must be unrepresentable or rejected):
- `Mapped` without having passed `AwaitingFirstBuffer`;
- any state → anchor change caused by camera movement (I2);
- `Destroyed` → anything (no resurrection; a new role object gets a new id);
- `AwaitingInitialCommit` → `Mapped` directly.

### B5.4 Difficult cases

| Case | Behavior |
|---|---|
| Surface destroyed while focused | `Destroyed` transition clears keyboard/pointer focus references **before** anything else reads them. The new focus target comes from the focus policy (C4). Nothing is sent to the dead client. |
| Client crash | All objects' `destroyed` run in arbitrary order (EB-5). The `toplevel_destroyed` handler is **idempotent** and keyed by `ObjectId`, not by a live `wl_surface`. Popups' `popup_destroyed` may run before or after it. Popup handling must tolerate a missing element. |
| Unmap / remap | D-B5-REMAP. The `ElementId` is stable across unmap/remap of the same role object. |
| Role object destroyed, `wl_surface` reused for a new toplevel | New `ElementId`. The old id is stale forever. The root surface's `data_map` association is overwritten at the new `new_toplevel`. |
| Popup parent disappears | B6.5. |
| Stale `ElementId` lookup | `Err(StaleElement)`; never panics; never aliases (B4). |
| `set_parent` to a now-unmapped/destroyed parent | Stage stores `transient_for: Option<ElementId>` as metadata. On parent unmap/destroy, children are re-parented per spec. Parent relations never affect Canvas authority (Level D placement policy may read them). |

---

## B6 — Popups and subsurfaces

### B6.1 Ownership

A popup is **never** a Stage element and has **no stored Canvas position** (DECISION, consistent with EV-W07). Its Canvas location is derived:

```
popup_geometry_origin_in_parent_geometry = configured (x, y)                  [PROTOCOL: xdg_popup.configure]
surface_origin(P) = root_origin(E) + geometry(E).loc + get_popup_toplevel_coords(P) + (x,y) − geometry(P).loc
                  (= root_origin(E) + origin_in_root(P), matching Window::surface_under's formula:
                     geometry.loc + location − popup.geometry.loc)            [SMITHAY]
```

Subsurfaces likewise have no stored position: `surface_origin(S) = parent surface origin + wl_subsurface position`. Positions are integers and double-buffered. Smithay tracks the accumulated offset in the renderer surface state.

### B6.2 Positioning and constraint boundary (Q-04)

- **SMITHAY**: `PositionerState::get_geometry()` ignores constraint adjustment. `get_unconstrained_geometry(target)` applies it against `target`, given **relative to the parent's window geometry** (EB-6).
- **PROTOCOL**: the boundary is "set by the compositor" (EV-W07). On an unbounded canvas it is pure policy.
- **DECISION D-B6-BOUNDARY:** target = the OutputLogical viewport of the **anchor output**, mapped into Canvas by T5 and then into parent-geometry-relative coordinates. The anchor output is:
  1. the output on which the input event that caused the popup (the grab serial, else the seat's last input) occurred, if the parent is visible on it;
  2. else the parent's frame-clock output (D-B8-CLOCK: highest-refresh observing output);
  3. else no constraint adjustment (`get_geometry()`).

  Rationale: the user is looking at the output where they clicked. Multi-output ambiguity is resolved by the input, which the protocol already ties popups to ("must be used in response to some sort of user action").
- **DECISION D-B6-NO-PAN-REPOSITION:** popups are positioned at creation and on `reposition` / reactive parent configure **only**. Camera translation never repositions or reconfigures a popup. A popup is part of its element's rigid surface tree in Canvas, and a pan carries it like the parent. This extends I3 to popups.

**Q-04 status: CONDITIONALLY RESOLVED.** The policy is complete, but rule 2 depends on B8's primary-output policy, and correctness for CSD parents depends on EB-H1 (X-5).

### B6.3 Popup grabs

- **PROTOCOL**: nested grabbing popups; dismissal in reverse creation order; compositor may deny (immediate dismissal); dismissal "can be done by the user clicking outside the surface" (EV-W08).
- **SMITHAY**: `PopupManager::grab_popup` returns a `PopupGrab` (pointer + keyboard grab types exist, `popup/grab.rs:216, 390, 498, 534`). It contains `assert_eq!(root.wl_surface(), find_popup_root_surface(&popup))`, a **panic path**. Infynitae must verify the precondition before calling (failure model, C8). Risk RB-2.
- **DECISION (provisional, finalized in B10):** a compositor-owned interaction (CameraPan, WindowMove, WindowResize) that starts while a popup grab is active **first dismisses the whole grab chain**, in protocol order. This is the "click outside" semantics the spec already describes, and it avoids two simultaneous grab owners.

### B6.4 Rendering and hit-testing

- Popups and subsurfaces render with their element, through the element's surface tree, using the same lattice edge function (B1.5). Popups draw immediately above their own element's tree. A popup of a low element can be occluded by a higher element. That is acceptable for Foundation 01 because popups follow user action on their element, which raises it (focus policy, C4). Revisit in Level D.
- Hit-testing goes through `Window::surface_under(r, WindowSurfaceType::ALL)`, which tests popups first and then the toplevel tree (SMITHAY, `desktop/wayland/window.rs:336-360`). Candidate pruning uses `bbox_canvas(E)` built from `bbox_with_popups`, so popups extending outside the window are hittable.

### B6.5 Lifetime

| Event | Behavior |
|---|---|
| `popup_destroyed` | `PopupManager` forgets it (`cleanup`). If it held the topmost grab, the grab returns to the parent grabbing popup or ends (PROTOCOL). |
| Parent popup dismissed by compositor | Dismiss all nested grabbing popups too, in reverse order (`popup_done` each) (PROTOCOL). |
| Parent element unmapped or destroyed | Dismiss its whole popup tree (`popup_done`). Then no popup can reference an element that no longer renders. |
| Client death | Arbitrary destruction order (EB-5). `PopupManager::cleanup` tolerates dead surfaces. No Stage state references popups, so there is nothing to purge in Stage. |
| Popup visibility / enter-leave | **Not** via `Window::output_enter`'s popup propagation, which X5 shows is offset (EB-H1 confirmed, B8.6). Infynitae computes per-surface membership for every surface of the element, popups included, with the B2.3 origin formulas, and calls `Output::enter`/`leave` itself (D-B8-ENTER). |

---

## B7 — Camera and output model

Evidence: EV-H02 (no prior multi-output product experience), EB-2 (S2/S4: an output-as-camera can be independent, but `Space`'s version is integer-only), B1, B2. Hotplug behaviour is designed here, not implemented.

### B7.1 Objects

| Object | What it is | Identity | Owner |
|---|---|---|---|
| **Output** | A backend display sink: mode, physical size, transform, refresh, fractional scale. The Smithay `Output` / `wl_output` global is its protocol face. | `OutputId` (runtime, monotonic, never reused, like B4); stable `OutputKey` (connector + make/model/serial) for configuration matching | output manager (backend reports modes; configuration chooses mode/scale/transform) |
| **Viewport(O)** | O's logical size = mode / scale, transform applied | derived | **nobody** (computed) |
| **Camera** | A view into the Stage: `center: CanvasPos` (Q16), `zoom: f64` (= 1.0 in Foundation 01) | `CameraId` (monotonic, never reused) | **Camera set**, compositor domain state beside Stage, not inside it |
| **Binding** | `OutputId → CameraId` | — | output manager (policy), stored in the camera set |
| **Arrangement** | Pointer-crossing layout of outputs (B2.1) | — | output configuration |

**DECISION D-B7-CENTER: a camera is anchored at its centre, not its top-left.** Each output derives its view origin:

```
v_O = camera.center − (viewport_O / 2) / zoom        (Q16, per output)
```

`v_O` is the "camera" in the B1.5 and B2.2 formulas (`R`, `C_O`, `c_eff,O` are all per output, computed from `v_O`). Centre anchoring was chosen because:
- zoom (Level C) scales about the visual centre;
- a camera shared by outputs of different sizes shows the same centre on both;
- a mode or scale change keeps the content centred instead of pinned to the top-left.

This changes no B1/B2 result. Those formulas only ever consumed the per-output origin.

### B7.2 Answers to the B7 questions

| Question | Answer | Class |
|---|---|---|
| Who owns Camera? | The camera set, compositor domain state alongside Stage. Not Output, renderer, Smithay, shell or Stage. Shell and IPC may *request* camera changes (C7). | DECISION |
| Is Camera permanently tied to an Output? | No. Cameras are bound to outputs; a binding can change. | DECISION |
| Can Camera survive output removal? | Yes. On removal the binding is dropped and the camera persists unbound. When the same `OutputKey` reappears, the binding (and thus the view) is restored. | DECISION |
| Is Camera persistent? | Yes, across sessions, keyed by `OutputKey` (persistence format is Level D). `CameraId` is runtime-only. | DECISION (format deferred) |
| Can two outputs share Camera state? | Yes: N outputs → 1 camera is a mirrored view. Each output still derives its own `v_O`, scale, edges and frame clock. B8's preferred-scale policy already covers a surface on outputs of different scale. | DECISION |
| Hotplug (conceptual) | **Add:** create the `OutputId`; bind it to the remembered camera for its `OutputKey`, else to a new camera whose placement is Level D policy; run the visibility pass (B9). **Remove:** run the visibility pass with the output gone (every surface it had entered gets `wl_surface.leave`) *before* the `wl_output` global is destroyed; then unbind and keep the camera. Stage is never touched (I4). **Mode/scale/transform change:** recompute the viewport, rerun the visibility pass, re-evaluate preferred scale (B8.3). No configures are derived from it (I3′). | DECISION |

### B7.3 Negative ownership

| State | OWNS | DOES NOT OWN |
|---|---|---|
| Camera centre/zoom | camera set | Output, renderer, Smithay, `Space`, Stage, shell (requests only), XWayland |
| Output properties (mode, scale, transform) | output manager / configuration | camera set, Stage, renderer (reads), clients |
| Output↔camera binding | output manager | camera set (stores but does not decide), Stage |
| Per-output view origin `v_O`, `R`, `C_O` | **nobody**: derived per frame | everything (I8) |
| Arrangement | output configuration | camera set, Stage |

---

## B8 — Multi-output Wayland semantics (mandatory worked example)

### B8.1 Harness and evidence (new)

| ID | Evidence | Where |
|---|---|---|
| EB-8 | Minimal lockstep harness. A Smithay 0.7.0 server (`wl_compositor` v6, xdg-shell, shm, fractional-scale, viewporter, two `wl_output` globals) plus a recording `wayland-client` probe over a socketpair, on a virtual vblank clock. Real: protocol objects, Smithay enter/leave and frame/scale mechanisms, every client-observed event. Modelled (the policies under test): integer cameras, zoom 1, the visibility rule, the clock. | `experiments/b8-harness/` (`server.rs`, `client.rs`, `lockstep.rs`, `bin/scenarios.rs`); output [`evidence/phase-b/b8-scenarios-output.txt`](evidence/phase-b/b8-scenarios-output.txt) |
| EB-9 | Real clients (vkcube/radv, kitty/swrast, Firefox 157) against the harness in real time on a private socket, no host compositor. They were shown on screen, moved off-screen under Starve / Throttle / kick, pinged while off-screen, then shown again. | `experiments/b8-harness/src/bin/realclient.rs`; output [`evidence/phase-b/x2-realclients-output.txt`](evidence/phase-b/x2-realclients-output.txt) |
| EB-10 | SMITHAY: `CompositorState::new` advertises `wl_compositor` **v5**. v6 (`preferred_buffer_scale`) needs `new_v6` (`wayland/compositor/mod.rs:680-706`). Prototype 01 used v5. | source + harness (v6 bind fails against `new`) |
| EB-11 | SMITHAY: pending frame callbacks accumulate across commits (`frame_callbacks.extend`, `wayland/compositor/handlers.rs:146`). A withheld callback is never dropped by a later commit. | source; confirmed by SC3 |
| EB-12 | SMITHAY: `send_surface_state` sends an integer preferred scale; `FractionalScaleState::set_preferred_scale` sends `round(scale·120)`; both send only on change (EV-S07). | source; confirmed by SC1 |

### B8.2 The worked example (EXPERIMENT, SC1; all numbers measured)

Setup: W has 800×600 window geometry plus a 20 px CSD shadow, so its root surface is 840×640 with `geometry.loc = (20,20)`. Anchor (B2.3) = Canvas (1000, 300), so the root origin is (980, 280) and the Canvas bbox is x 980…1820, y 280…920.
- **A**: 1920×1080, scale 1.0, 165 Hz; view origin `v_A = (0,0)`.
- **B**: 3840×2160, scale 1.5 → logical 2560×1440, 144 Hz; `v_B = (600,0)`.

Policies: scale = Max, frames = Starve (SC1 isolates the starvation case; the adopted frame policy is B9), clock = highest refresh, suspension = never.

**Stage and projection (B1.5, B2.2):**
- Stage holds exactly one datum for W: anchor (1000,300).
- On A (k=1, R=0, C_A=0): W's left edge projects to physical x=1000, its root to 980.
- On B (k=1.5, R=600, C_B=0): `edge(1000) = round(400·1.5+δ) = 600`, `edge(980) = 570`, `edge(1820) = 1830`.
- The same Canvas point (1100.5, 400.25) is OutputLogical (1100.5, 400.25) on A and (500.5, 400.25) on B. Through T5→T9 both give client coordinates (120.5, 120.25). The client-visible position is the same whichever camera the pointer looks through (EV-W13, D-B2-PTR-SPACE).

**Trace** (client-observed; the full trace is in EB-8):

| t (ms) | Event | Client receives | Wayland requirement | Smithay mechanism | Infynitae policy |
|---|---|---|---|---|---|
| 0 | W maps; visible on A and B | `preferred_scale 180` (=1.5), `preferred_buffer_scale 2`, one `xdg_toplevel.configure` (the **initial** one), `enter A`, `enter B` | enter on scan-out overlap (EV-W03). Initial configure required (xdg_surface). Preferred scale before configure ("initial wl_surface state … followed by an xdg_surface.configure") | `Output::enter` per surface; `send_surface_state`; `with_fractional_scale` | Scale = max(1.0, 1.5); integer = ⌈1.5⌉ = 2 (B8.3) |
| 0–50 | both visible | `wl_callback.done` at **165/s** (measured 165 in 1 s, SC3-d), not 165+144 | "must not send excessive updates, while still allowing the highest possible update rate" (wl_surface.frame) | `send_frames_surface_tree` with primary = clock output | Frame clock = highest-refresh observing output (A). Presentation `sync_output` = A |
| 50–327 | **Camera A pans** 40 px per A-vblank | no configure; `done` continues at ≈165/s | — | — | I3′ (B9.7) |
| 327.3 | W leaves A's viewport | `leave A` only | leave on losing overlap | `Output::leave` | Clock → B; preferred scale stays 1.5 (max over {B}) |
| 327–611 | B only | `done` at **140–144/s** | — | — | Clock = B |
| 611.1 | **Camera B pans**; W leaves B | `leave B` | — | — | W unobserved: callbacks withheld (in SC1; production adds kick + fallback, B9); scale held (no event); no configure; no suspension (B9.5) |
| 611–1186 | unobserved | nothing (0 `done`) | spec: "should avoid signaling frame callbacks if the surface is not visible" | throttle `None` | (B9) |
| 1186.1 | **Camera A jumps back** | `enter A`, `preferred_scale 120`, `preferred_buffer_scale 1` | — | — | Max over {A} = 1.0. A jump is an atomic camera change, so the decrease applies now (B8.3) |
| 1187.9 | first frame after re-entry | `wl_callback.done` **1,768 µs** after the camera change (A period 6,061 µs); client reallocates to 840×640 | — | EB-11: the withheld callback was still pending | Liveness: completed at the first A vblank (B9.3) |

Across the whole run W received exactly **one** `xdg_toplevel.configure` (the initial one), despite two pans and one jump. Damage is per output: a camera change damages only that output. Focus: camera motion never changes keyboard focus. Pointer focus is re-evaluated at the stationary pointer position after a camera change ends (detail in B10/B11).

### B8.3 Preferred scale for a surface observed at several scales (resolves Q-06)

**What the protocol says (PROTOCOL):** `wl_surface.preferred_buffer_scale` and `wp_fractional_scale_v1.preferred_scale` are **one value per surface**, "sent whenever the compositor's preference changes". `set_buffer_scale` says clients should "pick the same buffer scale as the scale of the output that the surface is displayed on". It is singular, and no text covers a surface displayed on several outputs at different scales. **The protocol does not mandate `max`.** Any choice is compositor policy. No claim about other compositors' consensus is made.

**Measured comparison (EXPERIMENT, SC2).** Three camera gestures:
1. B sweeps across W while A shows W fully.
2. A shows 220 of W's 840 px while B oscillates between showing 300 and 120 px (the visible-area ordering flips every swing).
3. B's viewport edge oscillates ±15 px across W's edge (B membership flaps).

"Upsampled on B" = time W was visible on B while its buffer scale was < 1.5 (blurry). "Downsampled on A" = time visible on A with buffer scale > 1.0 (sharp, extra memory).

| Policy | scale events: gesture 1 / 2 / 3 | buffer reallocations | MB allocated | upsampled on B (ms) | downsampled on A (ms) |
|---|---|---|---|---|---|
| Max (instant) | 2 / 1 / **19** | 23 | 79.0 | **0** | 1687 |
| **MaxGesture** | 2 / 1 / **1** | **2** | 7.0 | **0** | 2174 |
| Min | 0 / 0 / 0 | 1 | 2.2 | 1687 | 0 |
| Primary (smithay-like 2×-area / refresh rule) | 0 / 0 / 0 | 1 | 2.2 | 1687 | 0 |
| LargestArea | 0 / **20** / 0 | 21 | 72.0 | 1340 | 347 |

Interpretation:
- **Min and Primary** are stable but leave the high-DPI output blurry for the entire observation. Primary is biased to the higher refresh (A at 165 Hz) unless B shows ≥ 2× the area.
- **LargestArea** changes with overlap area during camera motion. It flapped 20 times in a 20-swing oscillation with no membership change, and was still blurry 1340 ms. Rejected: it is the "continuous with overlap area" behaviour the directive warns against, and the measurement confirms the hazard.
- **Max** is never blurry, and changes only when membership changes. Those changes are discrete events that already produce `enter`/`leave`. But an oscillation across an output edge renegotiates on every crossing (19 events, 23 reallocations).
- **MaxGesture** keeps Max's zero-blur property and removes the edge-oscillation churn (1 event instead of 19). Its cost is holding the higher scale until the gesture ends (2174 ms vs 1687 ms of downsampled display on A).

**DECISION D-B8-SCALE (INFYNITAE POLICY, not a protocol requirement):**
1. `fractional_preferred(E) = max(scale(O))` over outputs O currently observing any surface of E.
2. **Increases** apply immediately. That is the moment a higher-scale output starts observing E, so no observing output ever upsamples E's content beyond a transient frame.
3. **Decreases** are evaluated only when no camera gesture is active: at the end of a CameraPan or WindowMove interaction, or atomically for discrete camera changes (jumps, bookmarks). No time constant is involved; "gesture" is defined by the interaction model (B10).
4. While E is unobserved the last value is **held**: no events.
5. `integer_preferred(E) = ⌈fractional_preferred(E)⌉`. Integer-only clients render at ≥ the largest scale and are downsampled elsewhere. This is consistent with Smithay advertising `wl_output.scale` rounded up (EB-6). It is policy.
6. **Consequence on A:** with W on A (1.0) and B (1.5), the client renders a 1260×960 buffer (SC1). A shows it minified ×1/1.5. B shows it 1:1. Cost: 2.25× buffer bytes while multi-observed. Quality: minification instead of magnification on A. The minification filter is a renderer choice (C3). Visual quality was **not** measured here; the harness has no renderer.
7. **Determinism:** the value is a pure function of (observing set, gesture boundaries). It never depends on overlap area, time, or event batching.

### B8.4 Frame clock and presentation (EXPERIMENT, SC3)

- When W is visible on A (165 Hz) and B (144 Hz), callbacks complete once per frame of **one** clock output: 165 per second measured, never the sum.
- Clock rule **highest refresh** gives 165/s regardless of area (SC3-d2). The Smithay-like **Primary** rule picks B (144/s) when B shows ≥ 2× the area (SC3-d2).

**DECISION D-B8-CLOCK:** the frame clock of E is the observing output with the highest refresh rate, ties broken by `OutputId`. It changes only on membership changes. Basis: the frame-request text asks for "the highest possible update rate for clients that wait for the reply"; this is a policy reading of that sentence. Presentation feedback for E synchronizes to the clock output (`sync_output`, EV-W09).

Handover (SC3-e): when A stopped observing W while a callback was pending, it completed at B's next vblank (+4,167 µs; B period 6,944 µs). The maximum gap across the handover was 7.197 ms, bounded by old period + new period.

### B8.5 Membership: computed by Infynitae, not by `Window::output_enter`

**EB-H1 is CONFIRMED (EXPERIMENT, X5).** Setup: a CSD toplevel (shadow 20) with a CSD popup (shadow 10) whose true surface spans Canvas x 1090…1310. Smithay's popup membership (via `Window::output_enter` → `refresh`) is offset 10 px left (= `geometry.loc − popup.geometry.loc`):

| Viewport edge | Truth | Observed |
|---|---|---|
| right edge at x=1085 | popup invisible | **`wl_surface.enter` sent** (wrong) |
| left edge at x=1305 | popup visible (5 px) | **no enter** (wrong) |
| right edge at x=1095 | popup visible | enter (correct) |

Together with EV-S12/EB-2 S5 (bbox-relative overlap for the root tree), both Smithay paths for surface membership mis-place parts of the tree whenever window geometry ≠ surface origin, which is the common CSD case.

**DECISION D-B8-ENTER:** Infynitae computes membership per surface:
- traverse the element's surface tree and popup tree;
- compute each surface's Canvas rect with the B2.3 formulas (root origin = anchor − geometry.loc; popup origin as in B6.1);
- intersect with each output's Canvas viewport;
- call the public `Output::enter`/`leave` (EV-S04) directly.

`Window::output_enter`/`output_leave` are not used for membership. The obligation to read subsurface offsets from Smithay's public renderer surface state is checked in T1.

### B8.6 Distinguishing requirement / mechanism / policy (summary)

| Aspect | WAYLAND REQUIREMENT | SMITHAY MECHANISM | INFYNITAE POLICY |
|---|---|---|---|
| Stage identity | none (compositor-internal) | none | one element, one anchor (B2–B4) |
| Surface tree | subsurface/popup positions are parent-relative | `PopupManager`, renderer surface state | derive Canvas origins (B2.3, B6.1) |
| Visibility | `enter` when "some part … within the scanout region of an output" | `Output::enter/leave` | per-surface Canvas ∩ viewport (D-B8-ENTER) |
| Preferred scale | one value per surface; send on change | `send_surface_state`, `set_preferred_scale` | D-B8-SCALE (MaxGesture, ⌈⌉ for integer) |
| Viewporter | client maps buffer → destination size | `ViewporterState` | none (clients use it with fractional scale) |
| Render scale | — | `from_surface` with f64 location | lattice edges per output (B1.5) |
| Frame callbacks | "should avoid … if not visible"; "highest possible update rate" | `send_frames_surface_tree` (primary output, throttle) | D-B8-CLOCK; B9 off-screen policy |
| Presentation feedback | one `sync_output`; `discarded` if never displayed | `take_presentation_feedback` | sync to clock output; B9.4 |
| Damage | — | `OutputDamageTracker` per output | per-output damage; camera change damages only its output |
| Focus | pointer focus is one surface per seat | seat | camera motion does not move keyboard focus (C4) |

---

## B9 — Visibility, off-screen behaviour and I3′

### B9.1 Visibility state (per element; only defined while `Mapped`, B5)

The master prompt's PartiallyVisible / VisibleOnOne / VisibleOnMany distinctions have **no protocol consequence of their own**. Membership is per output (B8.5), the clock depends on the set (B8.4), and scale depends on the set (B8.3). Partial vs full visibility changes nothing observable. The state model therefore keeps only what changes behaviour:

| State | Definition |
|---|---|
| `Unobserved` | no surface of E overlaps any output viewport |
| `Observed{set S ≠ ∅, clock c}` | S = outputs observing some surface of E; c = D-B8-CLOCK(S) |

### B9.2 What the evidence says about off-screen callbacks (resolves Q-01)

| Finding | Class | Evidence |
|---|---|---|
| The spec recommends withholding callbacks for invisible surfaces | PROTOCOL | EV-W02 |
| Withheld callbacks are never lost; Smithay keeps them pending across commits | SMITHAY | EB-11 |
| On re-entry, a pending callback completes at the first vblank of the new clock output: 303 µs (SC3) and 1,768 µs (SC1) in lockstep; 3–6 µs after the camera change in real time; ≤ one period of the clock output by construction | EXPERIMENT | SC1, SC3, EB-9 |
| A one-shot callback requested **while** off-screen also stays pending and completes on re-entry (Starve) | EXPERIMENT | SC3 "one-shot@off" |
| Starvation minimises cost: off-screen CPU 0.0% (vkcube), 0.4% (kitty, still committing ~4/s on its own), 1.2% (Firefox parent; 0 commits) | EXPERIMENT | EB-9 |
| **Starvation can block a client's main loop for the entire off-screen period.** vkcube (Mesa Vulkan WSI, FIFO) answered `xdg_wm_base.ping`s sent while off-screen only after re-entry (2.0–4.0 s later). It cannot process `close`/configure in that state either (same blocked loop). Firefox and kitty answered immediately. | EXPERIMENT | EB-9 |
| A periodic fallback bounds that: Throttle 1000 ms gave pong in 6–18 ms at 0.0–0.2% CPU (vkcube); kitty/swrast cost 10.4% CPU (vs 39% at 250 ms) | EXPERIMENT | EB-9 |
| An **event-driven kick** (complete the client's withheld callbacks once, when the compositor sends it a request needing its main loop) bounds protocol responsiveness with no periodic cost: vkcube pong below harness resolution, 0.0% CPU, 0.6 commits/s off-screen | EXPERIMENT | EB-9 `starve+kick` |
| Whether present-locked **game** loops (Wine/Proton) or network clients suffer from long off-screen starvation (frozen simulation, server timeouts) | **ASSUMPTION: not measured** | no game was run; X-7 |

### B9.3 Decision D-B9-FRAMES

1. **Never invent requests.** The compositor only completes callbacks clients requested (`wl_surface.frame`). It never creates them.
2. **Observed:** complete pending callbacks once per frame of the clock output c (D-B8-CLOCK).
3. **Unobserved:** withhold (spec-recommended), with two bounded exceptions:
   - **(a) Protocol kick.** When the compositor sends a client a request that needs the client's main loop (`xdg_wm_base.ping`, `xdg_toplevel.close`, a non-initial configure, `xdg_popup.popup_done`, and, added at checkpoint 3 (B10.5), **input events delivered to it**), it first completes that client's withheld callbacks once. This bounds protocol responsiveness at zero idle cost (measured for ping; input trigger unmeasured, X-10).
   - **(b) Finite fallback cadence.** Each unobserved element receives at most one completion per fallback period P. **Architectural requirement: P is finite**, so no element is starved for an unbounded time. P is a runtime configuration parameter. Foundation 01's *provisional* default is P = 1 s:
     - the measured cost at 1 s is negligible for GPU clients (vkcube 0.0–0.2%) and for Firefox (= Starve);
     - it lets present-locked loops progress at 1 fps;
     - **the value is not justified by a game measurement (X-7)** and is expected to be revisited in T5.
     - Disabling the fallback (P = ∞) is permitted only per element, by explicit later policy (e.g. a parked state). It is never the global default.
4. **Re-entry liveness invariant:** when E goes `Unobserved → Observed{…, c}`, every pending callback of E completes at c's first vblank after the transition. Bound: ≤ one refresh period of c, plus dispatch.
5. **Multi-output:** completion only on the clock output (no double rate). On clock handover, pending callbacks complete at the new clock's next vblank. The gap is bounded by old + new period (SC3-e).
6. **Complete off-screen pause** (Starve with no kick and no fallback) is **rejected**. It leaves Mesa-WSI-FIFO clients protocol-unresponsive for the whole off-screen period (vkcube, 2–4 s in a 5 s test; unbounded in general).

### B9.4 Presentation feedback, damage, buffers, resources

| Concern | Rule | Class |
|---|---|---|
| Presentation feedback while unobserved | A commit's feedback resolves as `discarded` when superseded by a newer commit (it can never be shown). The latest commit's feedback stays pending, and resolves `presented` (sync to the clock output) when re-observed and shown, or `discarded` if superseded or the element is destroyed. Bounded by client commit activity. | DECISION; T1 obligation (the harness had no presentation-time) |
| Damage | A camera change damages only its output. Unobserved commits produce no damage anywhere. Re-entry damages the element's projected rect on the entering output. | DECISION |
| Buffer retention | The latest committed buffer of every mapped element is retained while unobserved. It is needed for instant re-entry and future off-screen thumbnails (product §3.1). Renderer textures may be evicted under a budget (C3); the client buffer reference is kept. | DECISION |
| Off-screen resource bound | Per element: one retained buffer, at most one callback completion per P, plus one per protocol kick. No compositor work per unobserved element per frame other than O(1) membership bookkeeping. | DECISION |

### B9.5 Suspension (resolves Q-02)

**Measured (EXPERIMENT, SC4):**
- `suspended` is delivered as `xdg_toplevel.configure … [suspended]` plus an `xdg_surface.configure` that must be acked. Every toggle is a configure (EV-W05 confirmed at the client).
- **Immediate (bound to camera visibility):** exactly 1 configure per visibility transition. 48 windows, 8×6 grid, 24 crossing the viewport, one horizontal sweep:

  | Pan speed | Configures/s |
  |---|---|
  | 2,000 px/s | 8.8 |
  | 8,000 px/s | 35.3 |
  | 30,000 px/s | 132.9 |

- **Debounce:**
  - On a single sweep it only *delays*. Total configures including a 3 s settle were **48 for every debounce value**, the same as Immediate.
  - On scrubbing (10 back-and-forth passes) it reduces configures from 1.00 per visibility transition (Immediate) to 0.70 (250 ms) and 0.10 (1 s). The benefit depends entirely on T relative to the user's scrub period, which is behaviour, not a protocol or system property.
  - Delayed re-suspension is still a configure caused by camera translation.
- **Never:** 0 configures in all runs.
- The power benefit that suspension targets is largely obtained by B9.3 for frame-paced clients (vkcube 0.0%, Firefox 1.2%, kitty 0.4% off-screen CPU, with no configure). The residual benefit (clients that animate without frame callbacks, or that honour `suspended` specifically) was **not measured**.

**DECISION D-B9-SUSPEND:** `suspended` is never derived from camera visibility, immediately or debounced.
- **State machine:** `suspended(E) = OR(reasons(E))`, with `reasons ⊆ {SessionLocked, AllObservingOutputsOff, Parked}`.
  - SessionLocked and output-off are Level C; Parked is an explicit user/shell action, Level D.
  - `CameraUnobserved` is **not** a member.
- Each reason change that flips the OR sends one configure; those are semantic transitions, not translation.
- **Reserved extension:** a future `StableUnobserved(T)` reason may be added only with evidence (which clients benefit, what T). T would be configuration, not a constant. It is **not enabled** in Foundation 01.

### B9.6 Transition table

Events: camera change (pan tick or jump), element move/resize/geometry change, output add/remove/mode/scale change, map/unmap (B5), vblank of output O, fallback tick, protocol kick, gesture end.

| State | Event | Next | Membership (`enter`/`leave`) | Frame callbacks | Presentation | Damage | Scale (D-B8-SCALE) | Configure |
|---|---|---|---|---|---|---|---|---|
| (unmapped) | map | `Observed` or `Unobserved` | enter for each observing output | per new state | — | element rect on observing outputs | set **before** the initial configure (B5) | initial only |
| `Unobserved` | camera/move brings E into O's viewport | `Observed{O}` | `enter O` for each surface now overlapping | pending ones complete at O's next vblank (liveness, B9.3-4) | latest commit presented on O | E's rect on O | increase immediately if scale(O) > held value; a decrease waits for gesture end | **none** |
| `Observed{S}` | camera/move adds O′ | `Observed{S∪O′}` | `enter O′` | clock may move to O′ if higher refresh (B8.4) | sync_output follows clock | E's rect on O′ | increase immediately if scale(O′) > current | none |
| `Observed{S}` | camera/move removes O′ (S′ = S∖O′ ≠ ∅) | `Observed{S′}` | `leave O′` | if O′ was the clock: handover, pending complete at the new clock's next vblank | sync_output follows clock | O′ only | decrease deferred to gesture end (or applied now if no gesture) | none |
| `Observed{S}` | last observing output stops observing | `Unobserved` | `leave` for each | withheld; kick + fallback P apply | superseded commits `discarded`; latest pending | none afterwards | held | none |
| `Observed{S}` | vblank of clock c | same | — | complete pending callbacks once | presented (sync c) | — | — | — |
| `Observed{S}` | vblank of O ∈ S, O ≠ c | same | — | none | — | per-output damage only | — | — |
| `Unobserved` | fallback tick (≥ P since last completion) | same | — | complete pending callbacks once | superseded commits `discarded` | none | — | — |
| any mapped | protocol request to E's client needing its main loop | same | — | if `Unobserved`: complete pending callbacks once, before the request | — | — | — | (the request itself, e.g. close/ping; never camera-caused) |
| any mapped | gesture end | same | — | — | — | — | apply any deferred decrease | none |
| any | output O added | recompute | enter for surfaces overlapping O's viewport | clock may change | — | O full | may increase | none |
| any | output O removed | recompute | `leave O` for all surfaces **before** the global is destroyed (B7.2) | handover or → withheld | — | — | decrease follows the gesture rule (a removal is atomic, so applied now) | none |
| any | output scale/mode change | recompute | as geometry dictates | as clock dictates | — | O full | re-evaluate (atomic) | none |
| any mapped | unmap / destroy | (B5) | `leave` all | pending callbacks are moot (client may re-request after remap) | pending feedback `discarded` | old rect | held for remap / dropped | per B5 |

Missing-transition check: every (state × event) pair above has a defined outcome. "Partial visibility" is not a state (B9.1), so it has no transitions. Zoom (Level C) is a camera change and falls under the camera rows.

### B9.7 I3 restated (resolves Q-09)

**I3′ — camera-translation invariant.** A change of camera state alone (centre, and future zoom) MUST NOT:
- (a) change any element's Canvas geometry (I2);
- (b) cause any `xdg_toplevel`/`xdg_popup`/`xdg_surface` configure: no size, state (including `suspended`, B9.5), bounds (EV-W06; bounds are never derived from camera or membership) or popup position (D-B6-NO-PAN-REPOSITION) is derived from camera state;
- (c) deliver to clients any pointer motion, relative motion or button event originating from the camera-pan gesture itself (B11).

A camera change MAY cause, as consequences of **output-membership** changes and never per pan tick:
- `wl_surface.enter`/`leave` (protocol-required, EV-W03);
- preferred-scale changes per D-B8-SCALE (an increase when a higher-scale output starts observing; a decrease only at gesture end);
- frame-callback cadence and clock changes (D-B8-CLOCK, D-B9-FRAMES);
- presentation `sync_output`/`discarded` outcomes.

After a pan gesture ends, the pointer's new surface-local position may be delivered as ordinary pointer motion (B10/B11). It is the true pointer position, not a pan delta.

**Measured:**
- SC1: 2 pans + 1 jump gave 0 non-initial configures, 3 enter/leave events after mapping (leave A, leave B, enter A) and 1 preferred-scale change (at the jump).
- SC4 with suspension Never: 0 configures over 48 windows at up to 30,000 px/s.

Permanent regression test: client-observed `configure` count during pan = 0 (C12).

---

## B10 — Input, grabs, focus and activation

### B10.1 New evidence (checkpoint 3)

| ID | Evidence | Where |
|---|---|---|
| EB-13 | **X-8**: the pan-ownership model under test (`experiments/b8-harness/src/input.rs`) driven by injected physical events. Real Smithay seat, `wp_relative_pointer`, `wp_pointer_constraints`. The probe client records every pointer, relative, lock and confine event. 9 cases, **48 programmatic checks, all PASS**. | `src/bin/x8.rs`; [`evidence/phase-b/x8-pan-input-output.txt`](evidence/phase-b/x8-pan-input-output.txt) |
| EB-14 | **X-6**: Smithay's real `WaylandSurfaceRenderElement` geometry (via `renderer_test`'s `DummyRenderer`) for a root surface plus an odd-offset subsurface. **134,478** rects over 241 scales × 3 magnitudes (up to 2³⁶). The f64-root B1.5 rule had **0** edge mismatches; the i32-root alternative had 72,242. | `src/bin/x6.rs`; [`evidence/phase-b/x6-edges-output.txt`](evidence/phase-b/x6-edges-output.txt) |
| EB-15 | **X-9**: presentation feedback while unobserved. A superseded commit's feedback is `discarded` at supersede time and never presented; the latest unobserved commit stays pending and is `presented` 1.2 ms after re-entry. | `src/bin/x9.rs`; [`evidence/phase-b/x9-presentation-output.txt`](evidence/phase-b/x9-presentation-output.txt) |
| EB-16 | SMITHAY: `pointer_constraints` `activate()` is never automatic (compositor policy); `deactivate()` destroys oneshot constraints (`wayland/pointer_constraints.rs:120-160`). The seat's `WlSurface` pointer-target `leave()` calls `deactivate()` **unconditionally** (`wayland/seat/pointer.rs:246-256`). X-8 C4 observed a duplicated `locked_pointer.unlocked`. | source + X-8 |
| EB-17 | SMITHAY: `PresentationFeedbackCachedState::merge_into` discards superseded feedback; `Drop` discards on destruction (`wayland/presentation/mod.rs:315-345`) | source + X-9 |
| EB-18 | SMITHAY: `RendererSurfaceState::surface_view` is private; the public accessor is `view()` (`backend/renderer/utils/wayland.rs:344`). `SubsurfaceCachedState.location` is public. | source + X-6 (compile error, then `view()`) |
| EB-19 | SMITHAY: `xdg_wm_base` advertised at v6 (`shell/xdg/mod.rs:1252`). `XdgActivationHandler::token_created` default accepts **every** token (`xdg_activation/mod.rs:248-258`). `XdgActivationTokenData` carries `client_id`, `serial: Option<(Serial, WlSeat)>`, `app_id`. | source |
| EB-20 | PROTOCOL + EXPERIMENT: `zwp_relative_pointer_v1` deltas are `wl_fixed` (1/256 px). An unaccelerated 0.9 arrives as 0.898 (X-8). The protocol is coarser than the Q16 Canvas, which is consistent with B1 (the Canvas is never the limiting precision). | X-8 |

### B10.2 The eight concepts and who owns them

| # | Concept | Owner | Representation | Must NOT |
|---|---|---|---|---|
| 1 | Physical/backend input | backend adapter (libinput, winit, headless) | backend-neutral `PhysicalEvent` (button/key with physical code, relative Δ accel+unaccel, absolute normalized, axis, device id, time). Non-finite values rejected (A-B1-3). | decide focus, routing or bindings; call the Smithay seat |
| 2 | Compositor gesture recognition | binding resolver (configuration-driven) | `(modifiers, button/key) → action`; bindings are **data** | hold state across events (that is #3); see Stage geometry |
| 3 | Compositor interaction state | interaction model (domain) | one `Interaction` value plus a **button/key ownership table**: every pressed physical button/key → `Owner::Client(seat delivery) / Owner::Compositor(interaction)` | be duplicated in the Smithay seat (Smithay's grab state covers only client-owned sequences) |
| 4 | Stage hit-testing | pure domain function over Stage + the per-output projection list (B2.5–B2.6) | `(OutputId, OutputLogical) → (ElementId, surface, surface origin in Canvas)` | store results (I8) |
| 5 | Wayland seat/pointer focus | Smithay seat **applies** Infynitae's focus decisions | `PointerHandle` focus + Canvas origin; `KeyboardHandle` focus | be read back as policy truth. Keyboard focus authority is Infynitae's focus model (B10.5); Smithay's keyboard focus is its applied image |
| 6 | Client absolute pointer delivery | Smithay `PointerHandle::motion` | Canvas location − surface Canvas origin (B2.5) | carry camera deltas (I3′c) |
| 7 | Client relative-pointer delivery | Smithay `PointerHandle::relative_motion` | the **physical** delta only, accel + unaccel | be derived from absolute position differences or camera changes (INV-5) |
| 8 | Constraint/lock state | protocol objects in Smithay; **activation policy** in Infynitae; authoritative pointer mode `Free(OutputId, p)` / `Locked(surface, Canvas lock point)` | `with_pointer_constraint`, the Infynitae pointer mode | be activated implicitly (EB-16: it never is); be changed by camera motion except at reconciliation (B11.4) |

**Refinement of D-B2-PTR-STATE.** This is a refinement, not a contradiction. While a lock is active, the authoritative pointer state is `Locked(S, q_lock)`, with `q_lock` a Canvas point. The output-local position is derived from it. Pans never move elements (I2), so `q_lock` remains the lock point through any camera change (X-8 C3).

### B10.3 Intake precedence (DECISION D-B10-INTAKE; verified for pointer buttons by X-8)

Every physical event is routed to **exactly one** owner, in this order:

| Step | Rule |
|---|---|
| P0 | Backend normalization (#1). |
| P1 | **Continuation follows ownership.** A release goes to whoever owned the press, always, including after the interaction that took it has ended (X-8 C7). Motion goes to the active compositor interaction if there is one. |
| P2 | **Binding resolution precedes client dispatch** for new presses. If the press matches a compositor binding and the acquisition precondition holds, the interaction is acquired and the press consumed; the client never sees it. |
| P3 | Client-owned sequences already in Smithay grabs (implicit click grab, popup grab, DnD) proceed through the seat. |
| P4 | Normal routing: hit test (#4) → seat focus (#5) → delivery (#6, #7). |

**Acquisition precondition for pointer gestures:** the client owns no pressed button. If it does, the binding press is delivered to the client like any press (X-8 C6). Rationale:
- The protocol has no way to retract a delivered press except protocol-defined focus transfer. `wl_pointer.leave` does not release buttons.
- Taking over a sequence the client already owns is exactly the stuck-button failure. Delivering instead keeps every client press paired with a release (INV-2).

The **only** legitimate client→compositor transfer of an in-progress press is protocol-initiated: `xdg_toplevel.move`/`resize` and `wl_data_device.start_drag` with a valid serial. There the protocol itself specifies the client loses device focus ("If triggered, the surface will lose the focus of the device used for the move").

**Keys:** the same ownership table applies to keyboard bindings. A key press consumed by a binding has its release consumed; a forwarded press has its release forwarded. Smithay's keyboard filter is per event, so it cannot guarantee this by itself. This is **not** covered by X-8 (no keyboard path in the harness); T1 obligation **X-10**.

**Bindings are configuration.** X-8 used middle button + modifier as test data only; the mechanism references a `PanBinding` value (`input.rs`) and never a hard-coded code.

### B10.4 Interaction state machine

| State | Event | Next | Effect |
|---|---|---|---|
| Normal | binding press, precondition holds | CameraPan / WindowMove / WindowResize | press consumed; owner = compositor; `gesture_active` = true (B8.3) |
| Normal | binding press, client owns a button | Normal | delivered to client (no acquisition) |
| Normal | protocol move/resize request with valid serial of a client-owned press | WindowMove / WindowResize | **transfer**: client loses pointer focus (protocol-defined); that button's release becomes compositor-owned |
| Normal | popup grab requested (valid serial) | Normal + Smithay `PopupGrab` | grab owned by the client sequence (#3 records it) |
| CameraPan / WindowMove / WindowResize | any press | same | consumed; owned until its release (X-8 C7) |
| CameraPan / WindowMove / WindowResize | motion | same | consumed by the interaction (B11 for pan) |
| CameraPan / WindowMove / WindowResize | initiator release | Normal | reconciliation (B11.4); `gesture_active` = false → deferred scale decreases |
| CameraPan / WindowMove / WindowResize | cancel (binding, session lock, compositor shutdown) | Normal | **no revert**; the camera or element stays where it is; owned buttons remain owned until released |
| WindowMove / WindowResize | target element unmapped/destroyed | Normal-after-release | interaction cancels; owned buttons consumed until released |
| CameraPan | focused / locked target destroyed | CameraPan | unaffected; reconciliation uses the free-pointer path (X-8 C9) |
| any interaction | its output removed | same (orphaned) | motion still consumed until the initiator is released; at termination the pointer is placed on a remaining output (nearest in Arrangement). **Design only:** the harness cannot remove outputs (T1 obligation X-11) |
| Normal with popup grab | compositor binding press, client owns no button | CameraPan (etc.) | popup grab chain dismissed first, in protocol order (B6.3; X-11 covers it in T1) |

### B10.5 Keyboard focus and visibility (DECISION D-B10-KBD, INFYNITAE POLICY)

| Option | Accidental typing into an invisible app | Intentional typing while looking elsewhere | Window still visible on another output | Focus changes without explicit action | Returning to the element |
|---|---|---|---|---|---|
| Retain | possible | **works** | correct (output-independent) | **never** | nothing to restore |
| Clear when unobserved | prevented | **broken** | must define "unobserved on which output"; partial answers are arbitrary | **yes** (camera motion changes focus) | needs a restore rule, which is camera-driven focus again |
| Retain + shell indicator | possible but visible | works | correct | never | — |

Decision:
- Keyboard focus changes **only** through:
  - explicit focus actions (click on an element, focus-navigation bindings, shell/IPC focus requests carrying user authority, activation granted by B10.7 policy);
  - loss of the focus target (unmap, destroy, session lock), after which the next target comes from the focus model (C4).
- Camera position, camera motion, Stage visibility and output membership never change keyboard focus.
- Retaining focus on an unobserved element is the defined behaviour. A shell indicator for "keyboard focus is off-screen" is permitted (Level D UX).
- Security: visibility never grants focus, and retaining focus grants nothing the user did not grant. Under this policy the camera cannot be used to steer keystrokes.

**Interaction with D-B9-FRAMES (contradiction found and resolved here):**
- A focused but unobserved client whose main loop is blocked by withheld callbacks (the vkcube class, EB-9) would queue the user's keystrokes until the next fallback tick.
- **Resolution:** delivering input events to an unobserved element's client is a *protocol kick* trigger (D-B9-FRAMES 3a now lists: ping, close, non-initial configure, popup_done, **input delivery**).
- The kick is rate-limited by the input rate. The user typing into an off-screen window is the user asking it to run.
- Not measured with a keyboard client: **X-10**.

### B10.6 Pointer focus, implicit/explicit grabs, multi-output, destruction

- **Pointer focus** is recomputed from the authoritative pointer state on every client-routed event. Exceptions: Smithay grabs (P3) and compositor interactions (KeepFocus, B11).
- **Implicit (click) grab:** the Smithay default grab freezes the focus surface and its Canvas origin. A camera change during such a grab (e.g. via keyboard or IPC; never via the pan binding, by the precondition) changes the pointer's Canvas point. The client receives the true position: elements do not move with the camera, so the frozen Canvas origin stays correct (B2.5).
- **Multi-output:** the pointer is on exactly one output, so hit-testing uses that output's projection only. A window seen on two outputs gives the same client coordinates through either camera (B8.2).
- **Destruction/unmap during a grab:**
  - Smithay grabs on a dead surface deliver nothing.
  - Compositor interactions hold `ElementId`s, and lookups fail closed (B4).
  - Lock target destroyed → free reconciliation (X-8 C9).
- **Output removal during an interaction:** B10.4, design only (X-11).

### B10.7 xdg-activation authority (DECISION D-B10-ACTIVATION)

```
client: get_activation_token(serial, surface, app_id)  ──►  token minted (Smithay)
compositor: token_created(token, data) ──► VALIDATE provenance; Smithay default accepts all (EB-19), so Infynitae overrides:
            serial must match a recent input event delivered to the requesting client's focused surface
client: activate(token, surface) ──► request_activation(token, data, surface)
compositor policy (focus model) ──► one of:
       grant keyboard focus to the target            (a focus action; allowed while the target is unobserved, D-B10-KBD)
       mark attention/urgency on the element          (state the shell renders)
       ignore                                         (invalid / stale / unwanted)
shell (user authority) ──► may issue a CameraCommand to navigate, e.g. when the user acts on an attention indicator
```

**Authority invariant (Foundation 01):** *Wayland client protocol requests cannot directly reposition an Infynitae Camera.* It is encoded mechanically:
- Camera mutation requires a `CameraCommand` with an authority tag constructible only by input interactions, the privileged local control socket (user/shell), and explicit user configuration.
- The Wayland protocol-handler code is in a different module/crate and cannot construct it (Rust visibility; C1 decides the crate boundary).
- This also closes `xdg_toplevel.set_fullscreen(output)`, `move`/`resize` requests, popup positioning, and activation as camera paths. None of them can name a camera.

An optional user setting such as "navigate to activated windows" would be user policy executed by the compositor. It is Level D and off by default. Shell UX (notifications, indicators) is deferred, not designed here.

---

## B11 — Camera-pan input semantics (X-8)

### B11.1 Lifecycle (DECISION D-B11-PAN; ★ = verified by X-8)

| Phase | Rule |
|---|---|
| Acquisition ★ | Binding press with no client-owned button (B10.3). The press is consumed. Records: initiating button, output, camera at start, Σ=0, and the lock point if a lock is active on the focused surface. |
| Ownership ★ | The initiator plus every button pressed during the pan, each until its own release (C7). Motion while active. The **modifier is not part of ownership**: releasing it first changes nothing (C8). |
| Accumulation ★ | Σ += physical Δ (accelerated) in f64. Camera = start − Q(Σ), quantized from the gesture origin (B1). X-8: 30 × (7.3, −2.1) → camera exactly (−219, 63). |
| Camera update ★ | Per motion: camera set mutated; visibility pass (enter/leave per B9); preferred-scale increases only (MaxGesture). Stage untouched (INV-3). |
| Input consumption ★ | No `wl_pointer.motion`, no `relative_motion`, no `wl_pointer.button` to any client while the pan owns the pointer (INV-1, all 9 cases). The pointer's output-local position is **frozen**: the world slides under a stationary cursor, so pans are unbounded and never clip at output edges. Frozen-cursor vs cursor-follows-world is UX policy (revisitable); ownership semantics are the same either way. |
| Existing grabs | Client-owned button ⇒ no acquisition (C6 ★). Popup grab without a held button ⇒ chain dismissed, then acquired (B6.3; X-11). |
| Pointer focus | **KeepFocus** (D-B11-FOCUS, B11.2) ★ |
| Constraints ★ | Active locks persist through the pan with no `unlocked`, persistent or oneshot (C3). Confinement persists if the pointer stays over the surface (C5 small). |
| Relative pointer ★ | Consumed. Nothing is replayed after termination; the first genuine motion yields exactly its own delta (INV-4, INV-5). |
| Cancellation | Terminate without reverting the camera; owned buttons stay owned. |
| Target destruction ★ | No effect on the pan (it has no element target). A destroyed lock target falls back to free reconciliation (C9). |
| Output disappearance | Orphaned pan (B10.4). Design only (X-11). |
| Termination ★ | Initiator release (consumed). `gesture_active` = false → deferred scale decreases apply (B8.3). |

### B11.2 Pan focus mode (DECISION D-B11-FOCUS = KeepFocus; EXPERIMENT X-8 C3)

| | KeepFocus | LeaveFocus (clear focus at acquisition) |
|---|---|---|
| Free pointer | no events during the pan; one true-position motion at the end | `wl_pointer.leave` at acquisition; `enter` at the true position at the end |
| Persistent lock | **no events** | `leave` + `unlocked` at acquisition (Smithay auto-deactivates on leave, EB-16); `enter` + `locked` at the end |
| Oneshot lock | **no events** | `unlocked` → **constraint destroyed**; the game must re-request (the Hyprland-era re-lock churn, EV-H07) |

KeepFocus produces strictly fewer client-visible effects and preserves oneshot locks. Clearing focus during a pan has no protocol requirement behind it: the spec's focus-loss sentence is about client-requested moves, not compositor gestures.

### B11.3 The principal invariant: verified

**"When an Infynitae camera-pan interaction owns a physical motion delta, that same delta produces neither client absolute pointer motion nor client relative-pointer motion."** **EXPERIMENTALLY VERIFIED** in all 9 X-8 cases:
- free pointer, both focus modes;
- locked persistent and oneshot, both modes;
- lock point pushed off the output;
- confined, small and large pans;
- stuck-button cases;
- modifier-first release;
- target destroyed mid-pan.

INV-1 held in each: 0 motion, 0 relative, 0 button during ownership.

**"The initiating event cannot leave the client with a logically stuck button"**: verified (INV-2 in all cases; C6, C7, C8 target it specifically).

### B11.4 Reconciliation at termination (verified unless marked)

| Pointer mode at acquisition | At termination | Client sees |
|---|---|---|
| Free | Re-hit-test at the unchanged output-local position | One `wl_pointer.motion` at the **true** surface-local position, or leave/enter if the surface under the pointer changed. X-8: exactly (11, 283), then (12, 283) after the next +1 motion. Not a replayed delta. |
| Locked, lock point still on the pointer's output | Keep the lock. The output-local position is re-derived from the Canvas lock point. | **nothing** (C3). The next genuine Δ arrives as `relative_motion` only. |
| Locked, lock point no longer observable | End the lock (protocol allows compositor deactivation); free reconciliation. **Ordering rule (W-SM-4a):** deactivate explicitly only if pointer focus will not change; otherwise Smithay's `leave()` deactivates. | `leave`, then exactly one `unlocked` (C4, after the rule ★). Before the rule the client received `unlocked` **twice**, because Smithay's `leave()` deactivates unconditionally (EB-16). |
| Confined, pointer leaves the surface through the pan | Free reconciliation | `leave` + `unconfined` (C5 large). No warping. |

No warping of the client-visible pointer to viewport edges, no synthetic relative motion, no replay of consumed deltas (INV-5 ★).

**Uncertainty recorded, not invented:**
- Whether real games behave well when their surface moves under a held lock (camera moved, lock point kept) is not established. No game was run (X-7, T5).
- The protocol is silent on locked-surface motion caused by the compositor; the chosen rule is protocol-permitted (no events), not protocol-mandated.

---

## B12 — Backend boundary and Smithay contract (resolves Q-07)

### B12.1 Layers and ownership

| Datum / function | OWNER | Explicitly NOT the authority |
|---|---|---|
| Backend events (devices, hotplug, vblank) | backend adapter (winit / DRM-KMS + libinput / headless) | domain, Smithay seat |
| Outputs: logical record (OutputId, mode choice, scale, transform, arrangement, camera binding) | core output manager | backend (reports capabilities and connectors only), Smithay `Output` (protocol face, written one-way from the core record), renderer |
| Outputs: hardware (connectors, CRTCs, modes available) | backend | core (reads descriptors) |
| Rendering / presentation (GPU resources, buffer import, submit) | backend renderer layer | domain (supplies only the per-output projection list), Smithay desktop helpers (only `from_surface` and the damage tracker are used) |
| Frame clocks (when an output can present) | backend (vblank/flip events) | domain decides which output **clocks which element** (D-B8-CLOCK), never hardware timing |
| Input devices | backend | domain (sees `PhysicalEvent`s only) |
| Wayland protocol dispatch, protocol objects, surface trees | Smithay adapter | Stage, backend |
| Surface ↔ ElementId association | Smithay adapter | Stage (knows ids only) |
| Stage (anchors, z-order, identity) | domain | Smithay (`Space` unused), backend, renderer, shell, XWayland |
| Camera set | domain | Output, backend, Smithay, shell (requests via `CameraCommand`), clients (B10.7) |
| Focus / input policy, interaction state, ownership table | domain | Smithay seat (applies only), backend |
| Persistence (Stage/camera state; format Level D) | domain service | backend, Smithay, shell |

**Backend-specific components (complete list):** device/event source; output discovery and mode setting; vblank/flip timing; render target, renderer, buffer import (shm/dmabuf); session/seat management (libseat) in production; the winit window in development. Nothing else.

**Headless/nested testing uses the same model.** The headless backend implements the same backend seam (virtual outputs, virtual vblank, injected `PhysicalEvent`s) under the **production** core and Smithay adapter. The checkpoint 2–3 harness (`server.rs` + `input.rs`) is an experiment rig standing in for that seam. It is **not** the production adapter, and tests written against it are re-expressed against the real seam in Phase F (C11).

**Duplication audit:**
- Smithay `Output` mode/scale is a protocol-facing copy, written only from the core record.
- Smithay seat focus is an applied copy.
- Smithay `Window::geometry()` is client state, read and never cached (B2.3).
- No backend structure stores element or camera positions.

### B12.2 Q-07: dependency and pinning strategy (DECISION D-B12-PIN)

| Item | Decision |
|---|---|
| Source | crates.io release (checksum-verified, reproducible offline), not git. Git `master` was not evaluated (EV-S01). Re-evaluating it is an upgrade decision, not a Foundation input. |
| Cargo dependency | `smithay = { version = "=0.7.0", default-features = false, features = [...] }`: exact `=` pin |
| Cargo.lock | committed (binary workspace); changed only by the upgrade procedure; CI builds with `--locked` |
| Toolchain | pinned via `rust-toolchain.toml` to a specific stable in Phase F (local today: rustc 1.99.0). Smithay 0.7.0's MSRV is 1.80.1. |
| Features: core | `wayland_frontend`, `desktop` (for `Window`, `PopupManager`, desktop utils; **not** `Space`) |
| Features: development backend | `backend_winit` (implies `backend_egl`, `renderer_gl`) |
| Features: production backend (Phase G) | `backend_drm`, `backend_gbm`, `backend_libinput`, `backend_udev`, `backend_session_libseat`, `renderer_gl` (multi-GPU later) |
| Features: tests | `renderer_test` (DummyRenderer, X-6) |
| Not enabled | `xwayland` (xwayland-satellite is an ordinary client, C6), `backend_x11`, `backend_vulkan`, `renderer_pixman` (until needed) |
| Protocol versions required | `wl_compositor` **v6** (`CompositorState::new_v6`, EB-10), `xdg_wm_base` v6 (Smithay default, EB-19), `wp_fractional_scale_v1`, `wp_viewporter`, `wp_presentation`, `zwp_relative_pointer_v1`, `zwp_pointer_constraints_v1`, `xdg_activation_v1`, `zwp_linux_dmabuf_v1` v4+ (feedback), `wl_seat`, `wl_data_device` |
| Transitive boundary | `wayland-server 0.31.14`, `wayland-backend 0.3.17` (rs backend: destructor order EB-5), `wayland-protocols 0.32.13`, `calloop 0.14.5`, all fixed by the lock |
| Patch policy | A `[patch.crates-io]` fork of Smithay is allowed **only** for a correctness defect that is (1) experimentally reproduced, (2) not fixable through public API, and (3) has an upstream issue/PR. Each patch is one minimal commit, listed in this table, with its experiment. The pin remains the crates.io version plus listed patches. Candidate: W-SM-4b. |

### B12.3 What Infynitae relies on, classified

| Behaviour relied upon | Class | Evidence |
|---|---|---|
| Toplevel position is not communicated to clients; `configure` has size + states only | WAYLAND PROTOCOL | EV-W01 |
| Popup position relative to the parent's window geometry | WAYLAND PROTOCOL | EV-W07 |
| `wl_surface.enter`/`leave` = scan-out overlap; preferred scale is one value per surface | WAYLAND PROTOCOL | EV-W03, EV-W04 |
| Frame callbacks "should" be withheld for invisible surfaces | WAYLAND PROTOCOL (recommendation) | EV-W02 |
| Superseded content → `discarded` | WAYLAND PROTOCOL | EV-W09 |
| `wl_pointer.leave` does not release buttons; move/resize transfer focus | WAYLAND PROTOCOL | spec text (B10.3) |
| `Window` standalone (no position); `surface_under` (root-relative); `send_frame`; `take_presentation_feedback`; `on_commit`; `bbox_with_popups`; `geometry` | SMITHAY PUBLIC API | EV-S03, EB-6, EB-15 |
| `Output::enter`/`leave`; `send_surface_state`; `with_fractional_scale`; `with_pointer_constraint`; `PointerHandle::{motion, relative_motion, button, frame}`; `PopupManager`; `PositionerState::get_unconstrained_geometry`; `WaylandSurfaceRenderElement::from_surface` (f64); `RendererSurfaceState::view()`; `CompositorState::new_v6` | SMITHAY PUBLIC API | EV-S04, EV-S07, EB-3, EB-18, EB-10 |
| Withheld frame callbacks accumulate and are never dropped | EXPERIMENTALLY VERIFIED SMITHAY BEHAVIOUR (+ source) | EB-11, SC1, SC3 |
| Pending callbacks complete at the first clock vblank after re-entry | EXPERIMENTALLY VERIFIED | SC1/SC3, EB-9 |
| Superseded presentation feedback discarded automatically; destroyed → discarded | EXPERIMENTALLY VERIFIED (+ source) | EB-15, EB-17 |
| Render-element edges equal B1.5 lattice edges with an f64 location, root and subsurfaces | EXPERIMENTALLY VERIFIED | EB-14 |
| Client death runs every object's `destroyed`, in arbitrary order | SMITHAY / wayland-backend implementation (source) | EB-5 |
| `Window::output_enter` mis-places popups and `bbox ≠ root` trees | SMITHAY QUIRK (experimentally confirmed) | EB-2 S5, X5 |
| `CompositorState::new` = v5 | SMITHAY QUIRK / default | EB-10 |
| Seat `leave()` deactivates constraints unconditionally | SMITHAY QUIRK (source + observed) | EB-16 |
| `ensure_configured` opt-in; `grab_popup` `assert_eq!`; `Window::geometry` re-clamps; `token_created` accepts all; `surface_view` private | SMITHAY QUIRK / implementation detail | EB-6, EB-18, EB-19 |
| MaxGesture, clock choice, frame policy (kick + fallback P), suspension reasons, KeepFocus, intake precedence, keyboard-focus policy, activation policy, anchor, numeric model | INFYNITAE POLICY | B1–B11 |

### B12.4 Quirks, workarounds and deliberate bypasses (all isolated in the Smithay adapter)

| ID | Smithay behaviour | Infynitae response | Verified? |
|---|---|---|---|
| W-SM-1 | `Space` owns placement, z-order and activation (B2.4) | **not used** | design (EB-2) |
| W-SM-2 | `Window::output_enter` / `Space::refresh` mis-compute membership for CSD trees and popups | own per-surface membership via `Output::enter`/`leave` (D-B8-ENTER) | quirk confirmed (X5); replacement tested in T1 |
| W-SM-3 | `CompositorState::new` → v5 | always `new_v6`; T1 asserts the bound version | harness uses v6 (SC1) |
| W-SM-4a | seat `leave()` → unconditional `deactivate()`, duplicating `unlocked` after an explicit deactivate | ordering rule: explicit `deactivate()` only when no pointer-focus change follows | **X-8 C4 ★** (48/48) |
| W-SM-4b | the same unconditional `deactivate()` **destroys a oneshot constraint that was never activated** when the pointer leaves its surface (only possible when Infynitae declines activation while the surface is focused, e.g. pointer outside a lock region) | **No public-API workaround.** A wrapper `PointerFocus` cannot send `wl_pointer.leave` itself (`PointerHandle.wl_pointer` is `pub(crate)`, `input/pointer/mod.rs:36`). Options: upstream fix (`if constraint.is_active()`); a vendored one-line `[patch]` under D-B12-PIN's patch policy; or accept. Decision deferred to Phase F; risk RB-10 | **reproduced (X-12)**; intervention deferred to the implementation phase |
| W-SM-5 | `render_elements_from_surface_tree` takes an i32 root | build elements with `from_surface` (f64) over its own traversal | X-6 |
| W-SM-6 | `ensure_configured` opt-in | call on every role commit | T1 (RB-3) |
| W-SM-7 | `grab_popup` panics on root mismatch | check the precondition first | T1 (RB-2) |
| W-SM-8 | `token_created` default accepts all | override with provenance validation | T1 |
| W-SM-9 | `Window::geometry` re-clamps to the current bbox | accept; compute effective geometry ourselves if T3 shows harm | T3 (RB-1) |

### B12.5 Upgrade procedure

1. Read the changelog and diff for: `desktop/space/wayland/*`, `desktop/wayland/{window,utils,popup/*}.rs`, `wayland/compositor/{mod,handlers}.rs`, `wayland/seat/pointer.rs`, `wayland/pointer_constraints.rs`, `wayland/presentation/mod.rs`, `wayland/shell/xdg/mod.rs`, `wayland/xdg_activation/mod.rs`, `backend/renderer/element/surface.rs`, `backend/renderer/utils/wayland.rs`, `output.rs`.
2. For each W-SM-n, decide whether it still applies. If a quirk was fixed upstream, the workaround may be removed **only after** its experiment passes without it.
3. Bump the `=` pin; `cargo update -p smithay` only; build with `--locked` afterwards.
4. **Re-run before acceptance:** b8-harness `scenarios` (SC1–SC4, X5), `x6`, `x8`, `x9`, and `realclient` with vkcube, kitty and Firefox (Starve/Throttle/kick). Plus the Phase F T0–T2 suites once they exist. `b2-space` only if `Space` is reconsidered. `b1-numeric` is Smithay-independent.
5. Any changed result is recorded as new evidence. A contradiction with a B-decision reopens that decision (F8).

---

## 8. Experiments: status (all of Phase B)

| ID | Experiment | Status | Evidence |
|---|---|---|---|
| E1–E9 | Numeric model | **RUN** | EB-1 |
| S1–S7 | `Space` semantics | **RUN** | EB-2 |
| X-1 | Client trace during pan (configure / enter-leave / scale) | **RUN** | SC1, SC4 |
| X-2 | Real clients off-screen (vkcube, kitty, Firefox; Starve, Throttle, kick) | **RUN** | EB-9 |
| X-3 | Two outputs 1.0/1.5, spanning window | **RUN** | SC1, SC2 |
| X-4 | Smithay overlap convention | **RUN** | EB-2 S5 |
| X-5 | Popup enter/leave offset | **RUN** (defect confirmed) | X5 |
| X-6 | Render-element edges vs lattice rule (root + subsurface, 241 scales, up to 2³⁶) | **RUN, PASS** (0 / 134,478 mismatches) | EB-14 |
| X-7 | Off-screen game / network client | **DEFERRED → T5** (physical/manual; decides P's default only) | — |
| X-8 | Camera-pan input ownership (9 cases, 48 checks) | **RUN, PASS** | EB-13 |
| X-9 | Presentation feedback while unobserved | **RUN, PASS** | EB-15 |
| X-10 (new) | Keyboard-binding key ownership (no stuck keys); input-delivery kick for a blocked unobserved client | **PENDING → T1** (harness has no keyboard path; mechanism is symmetric to the verified button rule) | — |
| X-11 (new) | Output removal during an interaction; popup-grab dismissal on pan acquisition | **PENDING → T1** (harness cannot remove outputs; no grab test) | — |
| X-12 (new) | W-SM-4b: oneshot lock never activated, then pointer leaves | **RUN: defect REPRODUCED at the client** (`unlocked` without `locked`; the lock can never activate afterwards). No intervention made (sealing rule). | `bin/x12.rs`; [`evidence/phase-b/x12-oneshot-lock-defect-output.txt`](evidence/phase-b/x12-oneshot-lock-defect-output.txt) |
| X-13 (new) | Single surface-origin function: render, membership, hit test (incl. `Window::surface_under` popups) and input agree on every surface's Canvas origin (property test) | **PENDING → T0/T1** | — |
| X-14 (new) | Activation/camera authority is unrepresentable for protocol code (compile-fail test) | **PENDING → T0** (needs the crate layout from C1) | — |

Re-run commands: `experiments/b8-harness`: `cargo run --offline --release --bin {scenarios,x6,x8,x9}`; `target/release/realclient …` (CHECKPOINT.md). `experiments/b1-numeric`, `experiments/b2-space`: `cargo run --offline --release`.

---

## B13 — Adversarial blocker audit

### B13.1 Original blockers

| Q | Original question | Evidence gathered | Disposition | Decision | Remaining assumptions | Deferred validation | Confidence / class |
|---|---|---|---|---|---|---|---|
| Q-01 | Off-screen frame-callback policy | EV-W02 (spec "should avoid"); EB-11 (callbacks never dropped); SC3 (re-entry ≤ 1 period, one-shot@off liveness); EB-9 (vkcube blocked 2–4 s under Starve; kick and Throttle bound it; Firefox/kitty fine); X-9 (feedback) | **RESOLVED** | D-B9-FRAMES: withhold; protocol kick (ping, close, configure, popup_done, input); finite fallback P (**provisional 1 s, configuration**); re-entry liveness; single clock | games / network clients under withholding; P value | X-7 (T5), X-10 | HIGH for mechanism (EXPERIMENT + SMITHAY + PROTOCOL); P value = ASSUMPTION |
| Q-02 | `suspended` vs I3 | EV-W05; SC4(a) configure per toggle; SC4(b/c) amplification: Immediate 1.0/transition, debounce only delays (sweep 48 = 48) or depends on T (scrub 0.70 / 0.10) | **RESOLVED** | D-B9-SUSPEND: never from camera visibility; reasons = {SessionLocked, AllObservingOutputsOff, Parked}; `StableUnobserved(T)` reserved, disabled | benefit of suspension for non-frame-paced clients is unmeasured | — (only if the reserved reason is proposed) | HIGH (EXPERIMENT + PROTOCOL) |
| Q-03 | Stage anchor | spec `set_window_geometry`; Space convention (S1); popup helpers (EB-6); X-6, X5 truth computations | **RESOLVED** | D-B2-ANCHOR = window-geometry origin; all origins derived | — | X-13 (single origin function) | HIGH (PROTOCOL + SMITHAY + EXPERIMENT) |
| Q-04 | Popup constraint boundary | EV-W07; `get_unconstrained_geometry` (EB-6) | **RESOLVED** (policy) | D-B6-BOUNDARY: viewport of the anchor output (input output → clock output → none); D-B6-NO-PAN-REPOSITION | UX quality unknown | T3 | MEDIUM-HIGH (DECISION; mechanism SMITHAY) |
| Q-05 | Smithay pointer global space | Smithay focus contract (EV-S08/P08); S3 ambiguity; X-8 exact client coordinates (11,283) | **RESOLVED** | D-B2-PTR-SPACE = Canvas; D-B2-PTR-STATE `(OutputId, OutputLogical)`, refined by `Locked(S, q_lock)` (B10.2) | — | X-13 | HIGH (SMITHAY + EXPERIMENT) |
| Q-06 | Preferred scale on mixed-scale outputs | protocol text (no rule); SC1; SC2 (5 policies) | **RESOLVED as INFYNITAE POLICY** | D-B8-SCALE MaxGesture; ⌈⌉ for the integer scale | downsampling quality on lower-scale outputs not measured (no renderer) | T3 visual check | HIGH for stability/sharpness metrics (EXPERIMENT); quality = ASSUMPTION |
| Q-07 | Smithay pin strategy | EV-S01; EB-10, 16–19; quirk inventory | **RESOLVED** | D-B12-PIN (exact pin, locked, features, protocol versions, patch policy, upgrade procedure, re-run suite) | git master not evaluated | at each upgrade | HIGH (DECISION) |
| Q-08 | Canvas numeric model | E1–E9; X-6 | **RESOLVED** | D-B1 (Q16 i64, ±2³⁶, integral anchors, rounding points, lattice edges) | zoom anti-jitter (A-B1-1, Level C) | before zoom | HIGH (EXPERIMENT) |
| Q-09 | Restated I3 | SC1, SC4 (0 configures), X-8 (pan-owned input: 0 events; reconciliation = true position only) | **RESOLVED** | I3′ (B9.7) | — | permanent regression tests (C12) | HIGH (EXPERIMENT + PROTOCOL) |

### B13.2 Blockers discovered during B1–B12 (not in the original program)

| ID | Blocker | Level | Found | Disposition | Validation |
|---|---|---|---|---|---|
| NB-1 | Smithay membership helpers mis-place CSD/popup surfaces (EV-S12, EB-H1) | A (multi-output semantics) | B2.4, X5 | **CONDITIONALLY RESOLVED**: D-B8-ENTER own per-surface membership. The defect is confirmed; the replacement is designed, not yet implemented. | X-13, T1 enter/leave truth tests |
| NB-2 | Single surface-origin function across render / membership / hit-test / input | A (coordinate algebra) | contradiction pass (§9-4) | **CONDITIONALLY RESOLVED**: one function computes every surface's Canvas origin. `Window::surface_under` may be used for hit-testing only if X-13 proves identical results; otherwise own traversal with Smithay input regions. | X-13 |
| NB-3 | Pure starvation blocks Mesa-WSI-FIFO clients (vkcube) | A (off-screen lifecycle) | X-2 | **RESOLVED** (D-B9-FRAMES kick + finite P) | X-7 for P |
| NB-4 | Unmatched client button state on gesture acquisition | A (input/grab authority) | directive + B10.3 | **RESOLVED, verified** (X-8 INV-2, C6/C7/C8) | X-10 (keys) |
| NB-5 | Smithay `CompositorState::new` advertises v5 | B | SC harness | **RESOLVED** (W-SM-3) | T1 version assertion |
| NB-6 | Smithay `leave()` deactivates constraints unconditionally | B (input correctness, games) | X-8 C4 | 4a **RESOLVED, verified** (ordering rule); 4b **OPEN as risk RB-10**: no public-API fix; patch policy defined | X-12 |
| NB-7 | Smithay activation tokens accepted by default | B (security) | B10.7 | **RESOLVED** (W-SM-8 override with provenance) | T1 |
| NB-8 | Keyboard input to an unobserved, blocked client waits for the fallback tick | B | contradiction pass (§9-6) | **RESOLVED** (input delivery is a kick trigger) | X-10 |
| NB-9 | Interaction during output removal | B | B10.4 | **CONDITIONALLY RESOLVED** (orphaned interaction; design only) | X-11 |
| NB-10 | Camera authority must be mechanically unrepresentable for protocol code | A (activation authority invariant) | directive §5 | **CONDITIONALLY RESOLVED**: `CameraCommand` authority tag plus crate/module visibility. The design is fixed; enforcement depends on C1's crate layout. | X-14 |
| NB-11 | Fallback-tick clock source when no output is active (all outputs off) | B | contradiction pass (§9-6) | **RESOLVED**: the production fallback uses a compositor timer, not output vblanks (the harness used output 0's vblanks only as a stand-in) | T1 |
| NB-12 | Harness ≠ production adapter; tests must not form a second architecture | B (process) | B12.1 | **RESOLVED by rule**: experiments are re-expressed against the production headless seam in Phase F | C11 |
| NB-13 | Zoom ≠ 1 anti-jitter rule | C | B1 | **DEFERRED** (A-B1-1) | before zoom |
| NB-14 | `relative_motion` is `wl_fixed` (1/256 px) | informational | X-8 | **NOTED**: protocol coarser than Canvas; no conflict | — |

---

## 9. Final contradiction pass

| # | Check | Result | Basis |
|---|---|---|---|
| 1 | Coordinate domains agree across rendering, hit-testing and input | **Holds**, with the stated ≤ ½ physical px bound at fractional scale (D-B2-I9). Render edges equal the lattice rule exactly (X-6); client coordinates are exact through the B2.5 chain (X-8: (11,283)). The fractional-camera `c_eff` path was **not** exercised (harness cameras are integer), so it is a T0 property-test obligation, not a demonstrated result. | X-6, X-8, B2.6 |
| 2 | Stage authority singular | **Holds.** Anchors exist only in Stage; `Space` unused; Smithay Output and seat copies are one-way; backend holds no positions; INV-3 held in all X-8 cases. | B2.4, B12.1, X-8 |
| 3 | Cameras never spatial authorities | **Holds.** No camera operation writes Stage; popups are never repositioned by pans; the lock point is a Canvas point carried by the element, not by the camera. | I3′, D-B6, B10.2 |
| 4 | Popup semantics agree with corrected enter/leave | **Contradiction found and resolved as NB-2.** Membership uses Infynitae's origin formula (D-B8-ENTER), while B2.5 hit-testing used `Window::surface_under` (Smithay's formula). The two formulas agree on paper (`geometry.loc + location − popup.geometry.loc`), but that was not tested. Resolution: one origin function, with X-13 as the gate for reusing `surface_under`. | B6.1, B8.5 |
| 5 | MaxGesture agrees with output membership | **Holds.** The scale input is the union of per-surface memberships from D-B8-ENTER (popups included); changes occur only at membership events or gesture end. | B8.3, SC2 |
| 6 | Frame policy vs suspension policy | **Holds**, plus two gaps closed: (a) a suspension configure is a non-initial configure and therefore a kick trigger, so a blocked client can process it; (b) **NB-8** (keyboard input to a blocked unobserved client) and **NB-11** (fallback clock with no outputs) were found here and resolved. | B9.3, B9.5, B10.5 |
| 7 | Gesture acquisition cannot produce unmatched client button state | **Holds for pointer buttons (verified).** Keys are not verified (X-10). Protocol-initiated transfers (move/resize/drag) follow protocol-defined focus loss. | X-8, B10.3 |
| 8 | Pan ownership cannot leak absolute or relative motion | **Holds (verified)**, INV-1 in 9 cases including locked, confined and destroyed targets. | X-8 |
| 9 | Pointer-lock termination cannot create phantom motion | **Holds (verified)**: no replay, no compensation, first genuine Δ only (INV-4/5). The real-game response is unknown (X-7). | X-8 C3, C4 |
| 10 | Keyboard focus policy explicit | **Holds**: D-B10-KBD, recorded as Infynitae policy. | B10.5 |
| 11 | xdg-activation cannot move Cameras | **Holds by design**, with no execution path from protocol handlers to `CameraCommand`. Mechanical enforcement is pending (X-14, NB-10). | B10.7 |
| 12 | Backend abstractions do not duplicate Stage/Camera state | **Holds** (B12.1 duplication audit). | B12.1 |
| 13 | Smithay workarounds isolated | **Holds**: every W-SM lives in the Smithay adapter. W-SM-4a needs one domain→adapter call (`end_lock(surface, focus_will_change)`); the decision stays in the domain, the Smithay call in the adapter. | B12.4 |
| 14 | I3 agrees with actual protocol behaviour | **Holds**: pans produced only enter/leave, membership-driven scale changes and clock changes; pan-owned input produced zero events; a single true-position motion after termination. | SC1, SC4, X-8 |
| 15 | No provisional policy silently became an invariant | **Holds.** Explicitly provisional or revisitable: fallback P = 1 s; frozen-cursor pan UX; popup draw order (B6.4); the MaxGesture cost trade-off (RB-7); ⌈⌉ integer scale; activation outcomes and shell UX; the reserved `StableUnobserved(T)`. Invariants (I1–I10, I3′) contain no numeric policy values. | this document |
| 16 | Phase A product requirement: "camera jumps keep the cursor at the same screen position" | **Holds structurally.** The authoritative pointer is `(OutputId, OutputLogical)` and camera changes do not touch it. Exception by design: while a pointer lock is active, the screen position is derived from the lock point (cursor normally hidden by the client). | B2.5, B10.2 |
| 17 | Output power-off vs membership | **Open question at Level C, not a contradiction.** Whether a powered-off output still "observes" (wl_surface.enter is about scan-out) is undecided. D-B9-SUSPEND's `AllObservingOutputsOff` reason works with either answer. | B9.5 |

---

## 10. Q-01 … Q-09 dispositions (summary; detail in B13.1)

| Q | Status |
|---|---|
| Q-01 | RESOLVED (P provisional) |
| Q-02 | RESOLVED |
| Q-03 | RESOLVED |
| Q-04 | RESOLVED (policy) |
| Q-05 | RESOLVED |
| Q-06 | RESOLVED (Infynitae policy) |
| Q-07 | RESOLVED |
| Q-08 | RESOLVED |
| Q-09 | RESOLVED |

---

## 11. Risks and open hypotheses

| ID | Risk | Severity | Mitigation / test |
|---|---|---|---|
| RB-1 | `Window::geometry()` re-clamps dynamically | low | T3 |
| RB-2 | `grab_popup` panics on root mismatch | medium | precondition check (W-SM-7) |
| RB-3 | `ensure_configured` opt-in | medium | W-SM-6, T1 |
| RB-4 | Arbitrary destruction order on client death | medium | idempotent handlers, T1 |
| RB-5 | Smithay membership helpers wrong for CSD/popups | resolved by design (NB-1) | X-13, T1 |
| RB-6 | Default compositor v5 | low | W-SM-3 |
| RB-7 | MaxGesture memory cost (2.25× while multi-observed) | low | measured (SC2) |
| RB-8 | Fallback P wrong for games or software clients | medium | X-7; P is configuration |
| RB-9 | The kick completes callbacks for all unobserved surfaces of a client | low | bounded per request/input |
| RB-10 (new) | W-SM-4b: never-activated oneshot lock destroyed on pointer leave | medium | X-12; upstream issue or patch policy |
| RB-11 (new) | Keyboard key ownership not yet verified | medium | X-10 |
| RB-12 (new) | Interaction during output removal not yet verified | low-medium | X-11 |
| A-B1-1 | Zoom anti-jitter | low (Level C) | before zoom |

---

---

## Phase B seal: carry-forward ledger

Five categories, kept separate on purpose. Phase C and later phases consume this ledger. Nothing in categories 2–5 may be cited as "resolved".

### 1. Resolved architecture (frozen unless new evidence contradicts it)

| Decision | Section |
|---|---|
| D-B1 numeric model: Q16 i64, ±2³⁶, integral anchors, rounding points R1–R6, forbidden F1–F7 | B1 |
| B1.5 reference-relative lattice edge rule (R = 120-lattice, δ = 2⁻²⁰), f64 location into Smithay | B1.5, X-6 |
| D-B2 coordinate domains, T1–T10, D-B2-ANCHOR (window-geometry origin), D-B2-PTR-SPACE (Canvas), D-B2-PTR-STATE (+ Locked refinement), D-B2-I9 | B2, B10.2 |
| D-B2-SPACE: Smithay `Space` not used | B2.4 |
| B3 object graph; D-B4 identity; B5 lifecycle; D-B5-REMAP | B3–B5 |
| D-B6 popups: no stored position; D-B6-BOUNDARY; D-B6-NO-PAN-REPOSITION | B6 |
| D-B7-CENTER; cameras survive outputs; N:1 binding | B7 |
| D-B8-SCALE (MaxGesture, Infynitae policy); D-B8-CLOCK; D-B8-ENTER (own per-surface membership) | B8 |
| D-B9-FRAMES mechanism (withhold + kick incl. input + finite fallback), D-B9-SUSPEND, I3′ | B9 |
| D-B10-INTAKE, D-B10-KBD, D-B10-ACTIVATION (as a design), D-B11-PAN, D-B11-FOCUS (KeepFocus), W-SM-4a ordering rule | B10–B11 |
| D-B12 layer ownership; D-B12-PIN (exact pin, lock, features, protocol versions, patch policy, upgrade procedure) | B12 |

### 2. Conditionally resolved obligations (design fixed; validation owed)

| ID | Obligation | Validation | Owner phase |
|---|---|---|---|
| CR-1 | Authoritative per-surface enter/leave (D-B8-ENTER) **and** one shared surface-origin function for render, membership, hit-test and input (NB-1, NB-2) | X-13 | C3/C4 design → T0/T1 |
| CR-2 | Keyboard binding and **modifier** ownership: no stuck logical keys or modifiers across bindings, focus changes, cancellation and destruction | X-10 | C4 design → T1 |
| CR-3 | Output removal during an active interaction (orphaned interaction) | X-11a | C4/C5 design → T1 |
| CR-4 | Popup-grab dismissal when a compositor interaction is acquired | X-11b | C4 design → T1 |
| CR-5 | Mechanical prevention of Camera mutation from protocol-handling code (NB-10) | X-14 | **C1** design → T0 |
| CR-6 | Fractional-camera snapping/rounding (`c_eff,O` path): never exercised; harness cameras were integral | T0 property tests | C3 → T0 |
| CR-7 | Input delivery as a kick trigger for blocked, unobserved, focused clients (NB-8) | X-10 | T1 |

### 3. Provisional policy / constants (configurable or revisitable; never invariants)

| Item | Current value | Revisit when |
|---|---|---|
| Off-screen fallback period P | 1 s (configuration) | X-7 results |
| Pan cursor behaviour | cursor frozen on screen, world slides | UX review |
| Popup draw order | immediately above own element | Level D |
| Integer preferred scale | ⌈fractional⌉ | evidence of integer-client problems |
| Activation outcomes and shell UX | focus / attention / ignore; no auto-navigation | shell design |
| `StableUnobserved(T)` suspension reason | reserved, disabled | evidence of benefit |
| MaxGesture memory trade-off (2.25× while multi-observed) | accepted | memory pressure |

### 4. Deferred physical / visual validation

| ID | Validation | Tier |
|---|---|---|
| DV-1 | X-7: Wine/Proton game and a network client off-screen under withholding, kick and P | T5 |
| DV-2 | Downsampling quality on the lower-scale output under MaxGesture (renderer filter) | T3/T4 visual |
| DV-3 | Real games under KeepFocus pans and preserved locks | T5 |
| DV-4 | Real libinput device semantics (acceleration, high-rate mice) under pan accumulation | T4 |

### 5. Catalogued Smithay quirks / workarounds (adapter-local; Smithay pinned per D-B12-PIN)

| ID | Quirk | Disposition | Evidence |
|---|---|---|---|
| W-SM-1 | `Space` owns placement/z-order/activation | not used | EB-2 |
| W-SM-2 | `Window::output_enter` / `Space::refresh` mis-compute CSD/popup membership (EV-S12, EB-H1) | own membership (CR-1) | EB-2 S5, X5 |
| W-SM-3 | `CompositorState::new` = `wl_compositor` v5 | use `new_v6` | EB-10 |
| W-SM-4a | `leave()` deactivates unconditionally → duplicate `unlocked` | ordering rule | X-8 C4 |
| W-SM-4b | same → never-activated oneshot lock destroyed | **defect record + reproducer preserved; no fork/vendor/patch now.** Intervention decided in the implementation phase under the patch policy | X-12 |
| W-SM-5 | `render_elements_from_surface_tree` takes an i32 root | build with `from_surface` (f64) | X-6 |
| W-SM-6 | `ensure_configured` opt-in | call on every role commit | EB-6 |
| W-SM-7 | `grab_popup` `assert_eq!` | check precondition | EB-6 |
| W-SM-8 | `token_created` accepts all | provenance override | EB-19 |
| W-SM-9 | `Window::geometry` re-clamps dynamically | accept; revisit on T3 evidence | EB-6 |
| W-SM-10 | `RendererSurfaceState::surface_view` private | use `view()` | EB-18 |

---

## Phase B gate evaluation

**Level A blockers from the master program (§7):**

| Level A item | Status | Where |
|---|---|---|
| Canvas numeric representation | RESOLVED | B1 |
| Coordinate algebra | RESOLVED; NB-2 CONDITIONALLY RESOLVED (X-13) | B2, §9-4 |
| Stage authority | RESOLVED | B2.4, B3, B12 |
| Element identity | RESOLVED | B4 |
| Stage element ↔ Wayland surface relationship | RESOLVED | B3 |
| Lifecycle ownership | RESOLVED | B5 |
| Multi-output surface semantics | RESOLVED (worked example measured); NB-1 CONDITIONALLY RESOLVED (implementation of D-B8-ENTER, X-13) | B8 |
| Off-screen lifecycle / callback semantics | RESOLVED (P provisional, X-7) | B9 |
| Input / grab authority | RESOLVED for buttons, pan, locks and confinement (X-8); keys and output removal CONDITIONALLY RESOLVED (X-10, X-11) | B10, B11 |
| Backend ownership boundary | RESOLVED | B12 |
| Activation → camera authority (NB-10) | CONDITIONALLY RESOLVED (design fixed; X-14 enforcement) | B10.7 |

**UNRESOLVED Level A blockers: none.**

Every CONDITIONALLY RESOLVED item has:
- a fixed design;
- a named validation experiment (X-10 … X-14) that runs in T0/T1 against the production seam;
- a failure mode that would be an implementation defect, or at worst a vendored one-line Smithay patch under D-B12-PIN's policy (W-SM-4b), rather than a change to ownership, coordinates or protocol semantics.

**B1–B13 status:** all complete.

**Phase B gate: SATISFIED**, with the conditional resolutions and deferred validations above carried forward explicitly. **Stopped at the Phase B review gate. Phase C has not begun and requires explicit approval.**

---
---

## Phase D Architectural Findings

> **Change-control record (Phase D checkpoint 1).** This section lists every finding classified **D-F2** (architecture correction) during Phase D checkpoint 1. Input baseline: the Phase C seal `615b0ed2f3631b698b0c54783c28f696ef4ff34a`. **Nothing here amends the sealed architecture.** The canonical registry (FOUNDATION_ARCHITECTURE §C12) and every sealed rule quoted below stay normative until the user disposes of the finding. Each "minimal correction" is a **proposal**. D-F1/D-F0 findings and the full analysis are in [`audit/`](audit/README.md).
>
> Levels follow the master program: **A** = authority or architecture-breaking (stops advancement), **B** = correctness of a Foundation behaviour, **C** = specification gap with a local fix.

| ID | Level | Title | Status |
|---|---|---|---|
| PD-F2-01 | **A** | Camera evolution without an authority port (viewport-fit re-fit, animated navigation) | OPEN: decision required |
| PD-F2-02 | **A** | Smithay grabs decide focus behind the FocusModel / PointerModel (popup keyboard grab drops `set_focus`; implicit grab defeats the W-SM-4a decision) | OPEN: decision required; T1 reproducer recommended |
| PD-F2-03 | B | `Locked` pointer state lacks its output and stores the lock point in Canvas | OPEN |
| PD-F2-04 | B | Pointer reconciliation is defined only at pan termination | OPEN |
| PD-F2-05 | B | Pointer confinement enforcement undefined across independent cameras | OPEN |
| PD-F2-06 | B | Zoomed panning is reachable in Foundation 01; pan formula has no zoom term; OB-24's deferral premise is false | OPEN |
| PD-F2-07 | B | Discrete camera interactions outside SC-D; SC-D ends on pointer motion | OPEN |
| PD-F2-08 | C | Device bound vs ladder / Phase B equivalence; H_max size basis; off-ladder held hint | OPEN |
| PD-F2-09 | C | Initial client scale hint undefined | OPEN |
| PD-F2-10 | C | Protocol-fullscreen state machine incomplete | OPEN |
| PD-F2-11 | C | Shared cameras vs output-keyed fit/isolation; orphaned-pan motion | OPEN |
| PD-F2-12 | C | Popup constraint target needs a rounding point outside R1–R7 | OPEN |
| PD-F2-13 | C | Window drag across outputs with independent cameras | OPEN |
| PD-F2-14 | C | `AllObservingOutputsOff` is defined over camera-derived membership | OPEN |
| PD-F2-15 | C | Attention / `transient_for` on unmap and destroy undefined | OPEN |
| PD-F2-16 | C | xwayland-satellite restart: X display identity and socket ownership | OPEN: small experiment needed |
| PD-F2-17 | B | xdg_toplevel state policy undefined (`activated`; `set_maximized` must be answered with a configure) | OPEN |

**Advancement:** PD-F2-01 and PD-F2-02 are Level A. Under the Phase D change-control rule, Phase D checkpoint 2 cannot begin until the user has disposed of both. No D-F3 (unresolved blocker) was found.

### PD-F2-01 (Level A): camera evolution without an authority port

- **Sealed rules (preserved):**
  - C1.5 step 2: "The only public entry points that reach `navigation` are `Core::physical_input(&mut InputPort, …)` and `Core::shell_command(&mut ShellPort, …)`".
  - INV-CAMERA-1: camera state is mutated only inside `core::navigation`, reachable only with `InputPort` or `ShellPort`.
  - C5.1: "W changes size while fitted: the zoom is re-fitted (camera follows; Stage untouched)".
  - CP3A-5, cycle 2: "h → client size → viewport-fit re-fit → zoom → ρ → h"; OB-28 "re-fit rate ≤ commit rate".
  - C7.3: "A long-running effect (animated navigation) is a compositor interaction"; AUD-6 "it continues to its end".
  - C14.2: "animation timers | an animation is running".
- **Counterexample:**
  - A fitted element's client commits a new logical size. The commit is handled in `CompositorHandler::commit`, which reaches core through `protocol_event` and holds no port. The sealed re-fit rule nevertheless requires a camera centre/zoom change at that moment. The same holds for an animated navigation: each animation tick changes the camera from a timer or frame source, not from inside a `shell_command` call.
  - Either core gains a port-free path into `navigation`, which breaks INV-CAMERA-1 and the C1.5 claim that EC-1 demonstrated, or the rules are unimplementable.
  - In the re-fit case an untrusted client steers a camera's position and zoom by resizing itself. That is the exact capability C1.5's "required property" forbids.
  - C-AUTH lists "clients" as forbidden writers of camera zoom, so the sealed text contradicts itself. AUD did not detect this.
- **Affected:** INV-CAMERA-1, INV-CAMERA-5, INV-SHELL-1, INV-SCALE-4(d) (its reasoning assumed re-fit), C1.5, C5.1, C7.3, C14.2, CP3A-5, C-AUTH (camera rows), AUD-6, OB-19 (the probes cover only the two entry points), OB-28; ADR-04, ADR-08, ADR-09, ADR-11.
- **Minimal correction (proposal; options):**
  - **(a) Recommended: derived camera modes.**
    - A port-authorised action stores a mode on the camera or output view: `Fitted{W, saved}` or `Animating{from, to, t0, duration, saved_target}`. The camera's stored centre/zoom changes only inside `physical_input`/`shell_command`.
    - The effective view used by C3.2 [1] is a pure function of (mode, W's current snapshot geometry, frame time). A client commit therefore changes a derived View, never camera state. The CP3A-5 cycle stays bounded by SC-E as before.
    - When the animation completes, it writes its target on the next port-carrying call, or the target is the stored value from the start (the animation only interpolates the view).
    - Probe addition for OB-19: no public core function without a port mutates `CameraSet`.
  - **(b)** Make re-fit a one-shot: drop "re-fit on resize" and keep the fitted zoom fixed. Drive animation ticks from a backend source that owns `InputPort`, e.g. `frame_tick(&mut InputPort, …)`. This changes C5.1 behaviour.
  - **(c)** Rejected: a third public entry point without a port. It would re-open the protocol-to-camera path.
- **More evidence needed:** no, this is a decision. After the decision, OB-19's probe list gains the "no port-free mutator" probe.

### PD-F2-02 (Level A): Smithay grabs decide focus behind the FocusModel / PointerModel

- **Sealed rules (preserved):**
  - INV-INPUT-8: keyboard focus changes only through explicit actions or target loss.
  - C2.2: keyboard focus owner is core `FocusModel`; Smithay's keyboard focus is an "applied copy".
  - C4.2 P3: client-owned Smithay grabs choose pointer focus for their sequences.
  - INV-INPUT-5 / W-SM-4a: explicit `deactivate` only when no pointer-focus change follows in the same batch.
- **Counterexample (Smithay 0.7.0 source):**
  - `PopupKeyboardGrab::set_focus` ignores every focus change while the popup grab is active, unless it targets the current popup (`desktop/wayland/popup/grab.rs:456-476`). `KeyboardHandle::set_focus` routes through the active grab (`input/keyboard/mod.rs:1052-1057`).
  - When the grab ends, Smithay itself calls `keyboard.set_focus(root)` (`grab.rs:371-378`) or sets the grab's current focus on the next key (`grab.rs:441-447`).
  - So an `Effect::KeyboardFocus` from the FocusModel during a popup grab is silently dropped. Causes include a shell `Focus` command, an activation grant, a focus binding, or the successor after target loss. Keystrokes then go to the popup's client, which the FocusModel did not choose, and Smithay writes keyboard focus on grab end without core. That is two writers to an authoritative datum.
  - Pointer analogue (D5 epoch 4):
    - A game holds a pointer lock and a client-owned button, so Smithay's implicit click grab is active, and the lock's output is removed.
    - Core's hit-test at the relocated position finds no surface, so core predicts a focus change and, per W-SM-4a, does **not** deactivate explicitly.
    - But the click grab keeps the delivered focus on W, so Smithay's `leave()` never runs and the lock stays active at the client while core believes it ended.
- **Affected:** INV-INPUT-5, INV-INPUT-7, INV-INPUT-8, INV-OUTPUT-1, C2.1/C2.2, C4.2 P3, C4.5, C-AUTH (pointer/keyboard focus rows), B6.3, W-SM-4a; ADR-07.
- **Minimal correction (proposal):**
  1. Client grabs become explicit sub-states of the core models:
     - the adapter reports `ProtocolEvent::{PopupGrabStarted, PopupGrabEnded, ImplicitGrab{surface}…}`;
     - the FocusModel records `target = popup` during a popup grab and `target = root element` after Smithay's restore. This keeps a single writer of truth, with Smithay's behaviour mirrored and never contradicted.
  2. A FocusModel-initiated keyboard-focus change while a popup grab is active first dismisses the grab chain (`popup_done` in protocol order; kick if unobserved), as C4.6 already does for pointer acquisition. It then applies the focus.
  3. The W-SM-4a decision uses the **delivered** focus change (which, under a client-owned grab, is "no change"), not core's hit-test.
- **More evidence needed:** the source evidence suffices for the decision. A T1 reproducer is recommended (set focus during a popup grab; lock + implicit grab + output removal) and should be added to OB-18 / OB-3.

### PD-F2-03 (Level B): `Locked` pointer state lacks its output and stores the lock point in Canvas

- **Sealed rules (preserved):**
  - INV-COORD-5: authoritative pointer = `Free(OutputId, OutputLogical)` or `Locked(SurfaceKey, Canvas lock point)`.
  - B10.2: "Pans never move elements (I2), so `q_lock` remains the lock point through any camera change".
  - INV-INPUT-5 reconciles against "the pointer's output".
- **Counterexample:**
  - **(i) Missing output.** `Locked` carries no output. INV-INPUT-5's "observable on the pointer's output" and the choice of camera for a pan binding pressed while locked are therefore undefined whenever the locked surface is visible on two outputs (D5 epoch 2: W on A and B).
  - **(ii) Detachment from the surface.** The Canvas lock point detaches from the surface whenever the root origin moves without a camera change. D5 epoch 3: fullscreen drops the CSD shadow (geometry.loc (20,20) → (0,0)), and the lock point moves from root-local (140.92, 180.60) to (120.92, 160.60) with no event (`evidence/phase-d/d5-model-output.txt`). Shell `MoveElement` or `WindowMove` of a locked element detach it entirely.
- **Affected:** INV-COORD-5, INV-INPUT-5, B10.2, B11.4, C2.1 `PointerModel`; ADR-07.
- **Minimal correction (proposal):** `Locked{output: OutputId, surface: SurfaceKey, at: SurfaceLocal}`. The Canvas lock point is derived as `surface_origin(surface) + at`. Camera-change behaviour (X-8 C3) is unchanged, because surfaces do not move with cameras.
- **More evidence needed:** no.

### PD-F2-04 (Level B): pointer reconciliation is defined only at pan termination

- **Sealed rules (preserved):**
  - B11.4 and INV-INPUT-5 define reconciliation "at pan end" and on output removal (AUD-4).
  - INV-CAMERA-4 permits "after termination one true-position pointer event".
- **Counterexample.** The following change what lies under a stationary pointer, or where a lock or confinement point is, without any reconciliation rule:
  - atomic camera changes by the shell or keyboard (`Navigate`, jumps; D5 epochs 2 and 4);
  - viewport-fit and restore on the pointer's output;
  - start or end of a protocol-fullscreen presentation on the pointer's output (D5 epoch 3);
  - output mode/scale/transform changes. A 1920×1080 output going from scale 1.0 to 1.5 shrinks its logical rect to 1280×720, so `Free(O, p)` can lie outside it;
  - moves of a locked or confined element;
  - the end of an orphaned interaction.

  A lock point pushed off-screen by a shell `Navigate` stays active indefinitely.
- **Affected:** INV-INPUT-5, INV-CAMERA-4 (wording of the permitted event), INV-OUTPUT-1, B11.4, C4.6; ADR-07.
- **Minimal correction (proposal):**
  - A single reconciliation step runs at the end of every core event batch (INV-SCALE-5's batch) in which any of the following changed: the pointer's output view (camera, presentation, output configuration), the pointer's focus-target geometry, or the pointer's output set.
  - It applies B11.4's rules and clamps `p` half-open into the output.
  - It emits at most one true-position motion (or leave/enter) per batch.
  - During an active compositor interaction it is deferred to termination, as today.
- **More evidence needed:** no. T1 tests are to be added to OB-3/OB-14.

### PD-F2-05 (Level B): pointer confinement enforcement undefined across independent cameras

- **Sealed rules (preserved):**
  - C4.3: activation policy.
  - B11.4: "Confined, pointer leaves the surface through the pan → free reconciliation".
- **Counterexample:**
  - The confinement region is surface-local, while pointer motion is in Arrangement/OutputLogical space. With independent cameras, crossing to another output gives a discontinuous surface-local position (W is shown elsewhere, or not at all, on the other output).
  - Nothing states that confined motion is clamped in the pointer output's view, or that Arrangement crossing is blocked while confined.
  - Nothing defines the state "pointer still over the surface but outside the region" after a camera or geometry change.
- **Affected:** INV-INPUT-5, C4.3, B11.4; ADR-07.
- **Minimal correction (proposal):**
  - While confined, motion is clamped in `OutputLogical(output)` to the projection of the region on the pointer's output (half-open), and Arrangement crossing is disabled.
  - After a view change (PD-F2-04), a pointer outside the region causes deactivation (`unconfined`; no warp, consistent with B11.4).
- **More evidence needed:** no. A T1 test is needed (OB-14 extension).

### PD-F2-06 (Level B): zoomed panning is reachable; the pan formula has no zoom term; OB-24's premise is false

- **Sealed rules (preserved):**
  - C5.1: "a static fitted view has no pan jitter, so A-B1-1 … is still not required"; fit ends when "the user navigates O's camera (user intent wins; no restore)".
  - OB-24: "no (static fit OK)", decision deferred to the zoom-motion feature.
  - B11.1 / INV-COORD-3: `camera = start − Q(Σ)`.
- **Counterexample:**
  - Ending a fit by navigation keeps the fitted zoom, so panning at zoom ≠ 1 occurs in Foundation 01. D5 uses k_A = 1080/651.
  - **Real Smithay geometry** (`evidence/phase-d/d5hit-output.txt`): during a pan, W's width on A changes 1394 → 1393 px and S2's 100 → 99 px at lattice-reference switches.
  - **Model F1** (`d5-model-output.txt`): widths {1393, 1394} and neighbour gaps {265, 266} at zoom 1080/651. They are constant at zoom 1 (k = 180/120), and also constant with a gesture-pinned reference.
  - The formula `camera = start − Q(Σ)` has no zoom term, so content moves 1.659× faster than the hand at k_A. D5 epoch 4: W leaves A after 56 motion events, versus 92 with Σ/zoom.
- **Affected:** INV-COORD-3, INV-RENDER-1 (reference-switch property), INV-CAMERA-5, C5.1, B1.6 A-B1-1, B11.1, OB-24; ADR-02, ADR-04.
- **Minimal correction (options):**
  - **(a) Recommended for Foundation 01:** navigation from a fitted view first restores the pre-fit zoom (fit stays a static-only feature; OB-24's deferral becomes true).
  - **(b)** Keep the zoom, define `camera = start − Q(Σ/zoom)` (zoom constant within a gesture), and pull OB-24 forward. Model F1 suggests a gesture-pinned lattice reference; that would need an X-6-style Smithay experiment and a decision.
- **More evidence needed:** none for (a); an experiment for (b).

### PD-F2-07 (Level B): discrete camera interactions outside SC-D; SC-D ends on pointer motion

- **Sealed rules (preserved):**
  - C14.4 SC-D: "Begins at the first discrete zoom step … Ends at the first non-zoom input from any device (button, key, pointer motion)".
  - INV-SCALE-3: decreases only when no camera gesture and no discrete-zoom interaction is active.
  - INV-SCALE-4: churn contract; P3 "no storms".
- **Counterexample (model):**
  - Discrete **pan** steps are atomic camera changes that settle each time: wheel-bound pan, keyboard pan steps, or a shell `Navigate` delta stream.
  - An oscillation across an output edge therefore renegotiates on every step. Model and D5 epoch 2: **40 changes in 40 steps**, versus 1 with gesture semantics. This is the SC2 edge-oscillation churn that MaxGesture removed, re-entering through non-gesture paths.
  - For zoom, SC-D ends at any pointer motion. Zoom-at-cursor (motion between wheel ticks) therefore reverts to naive behaviour: **40/40**, versus 1 without motion (F2).
- **Affected:** INV-SCALE-3, INV-SCALE-4 (b)/(c), C14.4, C7.3, OB-23; ADR-05.
- **Minimal correction (proposal):**
  - Generalise SC-D to a **discrete camera interaction**: any discrete camera step (pan or zoom; input binding or shell delta command).
  - It ends at a non-camera button or key press, or at a settlement event. Pointer motion does not end it. Decreases are deferred, at memory cost only.
  - Shell delta streams either carry begin/end brackets or fall under the same rule.
- **More evidence needed:** no for the rule; OB-23 tunes it.

### PD-F2-08 (Level C): device bound vs ladder and Phase B equivalence; H_max size basis; off-ladder held hint

- **Sealed rules (preserved):**
  - INV-SCALE-2: "a finite ladder containing every output scale and is ≤ H_max(E) = min(H_cfg, D_max / max logical side)".
  - INV-SCALE-5 / P4: "At zoom 1 … the policy equals Phase B MaxGesture".
- **Counterexample (model F3/F4):**
  - **(i) Contradiction with "every output scale".** Outputs at 1.0 and 3.0, a 6000×1000 window at zoom 1: Phase B gives 3.0, a buffer 18000 px wide, above D_max 16384. The unified policy must give H_max = 2.7307 (`model.py` already excludes such scales). INV-SCALE-2's "every output scale" and P4's equivalence are both false in this regime.
  - **(ii) "Logical size of E" is ambiguous.** A 5450-px window geometry plus 20-px shadows is a 5490-px root: H_max from the geometry is 3.0, which gives a 16470-px root buffer, above the device limit.
  - **(iii) Hotplug changes the ladder.** A held (S7) or gesture-deferred h can then be a non-rung. Epoch 4: 1.7321 is not in the ladder without B.
- **Affected:** INV-SCALE-2, INV-SCALE-5, CP3A-1 S2/S3/P4; ADR-05.
- **Minimal correction (proposal):**
  - The ladder contains the output scales ≤ H_max(E).
  - H_max uses the largest logical side over all surfaces of E's tree (each is its own buffer).
  - P4 holds where the maximum observing output scale ≤ H_max(E).
  - "Every *emitted* hint is a rung of the ladder in force at emission; a held or deferred hint may be off-ladder until the next evaluation permitted to change it".
- **More evidence needed:** no.

### PD-F2-09 (Level C): initial client scale hint undefined

- **Sealed rules (preserved):**
  - B5.3: preferred scale is sent before the initial configure.
  - INV-SCALE-1: ρ is defined over **observing** presentations.
- **Counterexample:** before its first buffer an element observes nothing, so h is undefined. The harness used an ad-hoc rule: the maximum scale of outputs whose viewport contains the anchor point, ignoring zoom (`b8-harness/src/server.rs:535-548`). D5: that rule gives 1.5, then 1.7321 at map, which costs one extra renegotiation and reallocation (F8).
- **Affected:** INV-SCALE-1, INV-SCALE-5, B5.3, B9.6 (map row); ADR-05.
- **Minimal correction (proposal):** initial `h := t(ρ_pred)`. ρ_pred applies S1 to the presentations whose view intersects the placed window-geometry rect (anchor + configured size; the anchor point if the size is 0×0). If no presentation intersects it, use the maximum connected output scale (to be confirmed).
- **More evidence needed:** no.

### PD-F2-10 (Level C): protocol-fullscreen state machine incomplete

- **Sealed rules (preserved):** C5.2 (acceptance, output choice, presentation view, removal, unfullscreen); INV-PRES-1.
- **Undefined reachable transitions:**
  1. Whether the presentation starts at acceptance or at the client's ack/commit, and ends at the exit decision or at the windowed commit.
  2. Lifetime of a deferred request ("record it and decide when W next gains focus"): it has no cancellation by `unset_fullscreen`, unmap or destroy, and no bound.
  3. A second element requests fullscreen on an output already `Exclusive(W)`.
  4. W loses keyboard focus while fullscreen.
  5. The requested output disappears between request and acceptance.
- **Affected:** INV-PRES-1, C5.2, OB-22; ADR-09.
- **Minimal correction (proposal):**
  - The presentation starts at acceptance (border fill covers the size mismatch) and ends at the exit decision.
  - A deferred request is cancelled by `unset_fullscreen`, unmap or destroy, and is evaluated only at the next focus gain.
  - A newer accepted request on an occupied output replaces the old one; the old element gets a configure leaving fullscreen with its saved size.
  - Focus loss does not end fullscreen.
  - The output choice is re-run at acceptance.
- **More evidence needed:** no (OB-22 remains for UX).

### PD-F2-11 (Level C): shared cameras vs output-keyed fit and isolation; orphaned-pan motion

- **Sealed rules (preserved):**
  - INV-CAMERA-3: "N outputs may share one camera".
  - C5.1: `ViewportFit{O}` and the isolation flag are per output; fit zoom is computed from O's viewport; "the user navigates O's camera" ends the fit.
  - C4.6 / B10.4: an orphaned interaction keeps consuming motion.
- **Undefined:**
  - A fit on O changes a camera shared with O2, so O2 zooms too.
  - Whether navigating the shared camera through O2 ends O's fit.
  - Whether an orphaned pan's consumed motion still mutates the now-unbound camera.
- **Affected:** INV-CAMERA-3, INV-CAMERA-5, INV-OUTPUT-1, C4.6, C5.1; ADR-04.
- **Minimal correction (proposal):**
  - Fit, restore and isolation are per-output **view modes** (same mechanism as PD-F2-01 (a)) that override the shared camera on that output only.
  - Navigating a camera ends the fits of all outputs bound to it.
  - An orphaned pan discards its motion (no camera mutation after its output is gone).
- **More evidence needed:** no.

### PD-F2-12 (Level C): popup constraint target needs a rounding point outside R1–R7

- **Sealed rules (preserved):**
  - INV-COORD-2: "Rounding happens only at R1–R7".
  - D-B6-BOUNDARY maps the anchor output's viewport through T5 into parent-geometry coordinates.
  - C3.6: the target is a `Rectangle<i32, Logical>` ("integral").
- **Counterexample:** with a fractional camera at zoom 1, the target is fractional: x[−179.333328, 2380.666672) relative to W's geometry (F7). An unlisted rounding is therefore required.
- **Affected:** INV-COORD-2, C3.6, D-B6-BOUNDARY; ADR-02.
- **Minimal correction (proposal):** **R8**, inward rounding of the constraint target (ceil the minimum edges, floor the maximum edges), so a constrained popup never extends past the visible viewport.
- **More evidence needed:** no.

### PD-F2-13 (Level C): window drag across outputs with independent cameras

- **Sealed rules (preserved):**
  - B1.3-7 / R3: `anchor = round(anchor_at_drag_start + Q16(Σ canvas pointer deltas))`.
  - INV-STAGE-1(b): interactive drag commit is a Stage writer (the move UX itself is deferred, C15).
- **Undefined:** when the pointer crosses outputs, the "Canvas pointer delta" is the jump between two independent cameras' unprojections. It is undefined whether the element jumps to follow the cursor or continues by device deltas, ending up off-cursor.
- **Affected:** INV-STAGE-1, INV-COORD-3, R3; ADR-01, ADR-04.
- **Minimal correction (proposal):** `anchor = round_R3(q_now − grab_offset)`, where q_now is the pointer's current Canvas point (T5 on the authoritative pointer). It is a function of current state, so it is batching-independent (N5) and follows the cursor on whichever output shows it, including during concurrent camera changes.
- **More evidence needed:** no.

### PD-F2-14 (Level C): `AllObservingOutputsOff` is defined over camera-derived membership

- **Sealed rules (preserved):**
  - INV-FRAME-6: `suspended` only from {SessionLocked, AllObservingOutputsOff, Parked}; never from camera visibility.
  - B9 §9-17: whether an off output "observes" is open.
- **Counterexample:**
  - Read literally, "all observing outputs off" is vacuously true for an unobserved element. A camera pan that makes E unobserved would then suspend it, which is exactly what INV-FRAME-6 forbids.
  - Read as "non-empty and all off", a camera change that empties the observing set flips the reason, again a camera-caused configure.
- **Affected:** INV-FRAME-6, D-B9-SUSPEND; ADR-06.
- **Minimal correction (proposal):** define the reason independently of camera membership, e.g. `AllOutputsOff` (every connected output powered off), or freeze the observing set at power-off. The reason stays Level C (not in Foundation 01).
- **More evidence needed:** no.

### PD-F2-15 (Level C): attention and `transient_for` on unmap and destroy undefined

- **Sealed rules (preserved):** B5.3 unmap/destroy side effects; C4.5 attention set; B5.4 `transient_for`.
- **Undefined:**
  - whether an unmapped element keeps its attention mark (it is shell-visible);
  - whether destroy removes it, which is implied but not stated;
  - whether `transient_for` survives the parent's unmap (B5.4 re-parents per spec for *protocol* children, but `transient_for` is Stage metadata).
- **Affected:** INV-STAGE-4, INV-SHELL-2 (event stream); ADR-11.
- **Minimal correction (proposal):** unmap clears attention, with a shell event; destroy purges every reference (already the rule for focus and grabs); `transient_for` follows the protocol re-parenting.
- **More evidence needed:** no.

### PD-F2-16 (Level C): xwayland-satellite restart, X display identity and socket ownership

- **Sealed rules (preserved):** C6: "a chosen free X display"; "restart satellite with backoff"; "lazy or at startup".
- **Undefined:**
  - Whether a restart keeps the X display number. Children spawned with the old `DISPLAY` would otherwise point at nothing or at a foreign server.
  - For lazy start, whether the compositor owns the X socket. That needs satellite's `-listenfd` support, which was **not verified** for 0.8.3.
- **Affected:** INV-XWAYLAND-1, C6, C8 (satellite row); ADR-10.
- **Minimal correction (proposal):** the compositor reserves the display number and socket paths for the session and restarts satellite on the same display.
- **More evidence needed:** **yes**, a small T1 experiment: satellite 0.8.3 `-listenfd`, and a restart on the same display.

### PD-F2-17 (Level B): xdg_toplevel state policy undefined (`activated`, maximize)

- **Sealed rules (preserved):**
  - B2.4 replaces `Space`'s activation side effect with "explicit focus model (C4)".
  - C4.5: the focus model's outputs are `Effect::KeyboardFocus` and `Effect::Attention`.
  - W-SM-13: override `maximize_request`/`fullscreen_request`, because Smithay's defaults send a configure.
  - C15: "maximize/minimize policy: architecturally supported, not physically validated".
- **Counterexample (protocol text, xdg-shell 1.49):**
  - `set_maximized`: "the compositor **will respond by emitting a configure event**. Whether this configure actually sets the window maximized is subject to compositor policies".
  - No Infynitae maximize policy exists, and the W-SM-13 override replaces the only code that sent a configure. A post-map `set_maximized` therefore gets no configure, against the protocol's stated expectation. It is a common path: GTK client-side decorations send it on a title-bar double-click.
  - The `activated` state ("paint as if active") is never mapped from the FocusModel. Whether focus changes produce configures, which are kick triggers for unobserved elements, is therefore undefined.
- **Affected:** INV-INPUT-8, INV-FRAME-3 (kick list includes non-initial configures), INV-CAMERA-4 (must stay true), C4.5, C15, W-SM-13; ADR-07, ADR-09.
- **Minimal correction (proposal):**
  - `activated` holds exactly when the FocusModel target is the element or one of its popups. It is emitted only on FocusModel transitions, never from camera or visibility.
  - Foundation 01 **declines** maximize with a protocol-compliant configure that repeats the current states and size. A pre-map request is recorded and ignored, and the initial configure proceeds as usual.
  - `set_minimized` is ignored (the protocol permits this).
  - `configure_bounds` is not sent (never camera-derived, EV-W06).
- **More evidence needed:** no. T1: a client calls `set_maximized` after map and receives exactly one configure.

---

## Phase D Corrective Checkpoint 1R — dispositions of PD-F2-01 … PD-F2-17

- **Falsification baseline (immutable):** `e8ec03a2ab36f315ae025723ec666b9ad278ac83`. The findings above are preserved unchanged.
- **Status:** 1R corrections applied **uncommitted, for review**.
- **Full chains** (sealed rule → counterexample → finding → replacement rule → INV → ADR → evidence): [audit/1R/CORRECTIONS_1R.md](audit/1R/CORRECTIONS_1R.md).
- **Canonical wording:** FOUNDATION_ARCHITECTURE §C12.R1.
- **Diff with provenance:** [audit/1R/INVARIANT_DIFF_1R.md](audit/1R/INVARIANT_DIFF_1R.md).

| Finding | Level: baseline → 1R re-evaluation | Disposition | Replacement / destination | Evidence | Status |
|---|---|---|---|---|---|
| PD-F2-01 | A → A | A | INV-CAMERA-1/5: one-shot fit; per-action `Authorized` proof; no animation in Foundation 01 | ER-4 (probes 2d E0624, 2e E0277; tests), ER-5 R3 | **RESOLVED** |
| PD-F2-02 | A → A | A | INV-INPUT-8 (ActiveElementFocus), **INV-INPUT-10 (new)** ProtocolSurfaceRoute with ordered preemption; W-SM-14/15 | ER-1 (7 PASS; NEG-1, NEG-2, OBS-1 fail as predicted) | **RESOLVED** |
| PD-F2-03 | B → **A** (Core/protocol divergence) | A | INV-COORD-5 split state; surface-local lock point; INV-OUTPUT-1 | ER-2 | **RESOLVED** |
| PD-F2-04 | B → **A** (divergence) | A | INV-INPUT-5 batch-end reconciliation, triggers, deferral | ER-2 | **RESOLVED** (T1: OB-3/OB-31) |
| PD-F2-05 | B → **A** (mandatory protocol behaviour) | A | INV-INPUT-5 confinement clamp on the interaction output; no crossing | ER-5 R5 | **RESOLVED** (T1: OB-31) |
| PD-F2-06 | B → B | A (algebra) + D (jitter) | INV-COORD-3 direct-manipulation algebra; jitter residual → OB-24 (rescoped) | ER-5 R1; ED-1 | algebra **RESOLVED**; jitter **OBLIGATION** |
| PD-F2-07 | B → B | A | INV-SCALE-3/4 ScaleInteractionEpoch; T_settle provisional (OB-23) | ER-5 R2 | **RESOLVED** |
| PD-F2-08 | C → C | A | INV-SCALE-2/5 text | ED-2 F3/F4 | **RESOLVED** |
| PD-F2-09 | C → C | A | INV-SCALE-5 initial hint | ED-2 F8 | **RESOLVED** |
| PD-F2-10 | C → **A** (unanswered `set_fullscreen`) | A | INV-PRES-1: answer every request; decline at once; timing; replacement | ER-3 | **RESOLVED** |
| PD-F2-11 | C → C | B (+ orphaned-pan rule) | resolved by PD-F2-01; INV-OUTPUT-1 | ER-4 | **RESOLVED** |
| PD-F2-12 | C → C | A | INV-COORD-2 adds R8 | ED-2 F7 | **RESOLVED** |
| PD-F2-13 | C → C | C | OB-34: cross-output drag formula decision before interactive move | — | **OBLIGATION** |
| PD-F2-14 | C → C | A (text) | INV-FRAME-6 `OutputsOff` | analysis | **RESOLVED** |
| PD-F2-15 | C → C | C | OB-35: attention / `transient_for` lifecycle T1 | — | **OBLIGATION** |
| PD-F2-16 | C → C | E (verified) + C | OB-33: restart on the same display via `-listenfd` | ER-6 | **OBLIGATION** |
| PD-F2-17 | B → **A** (unanswered maximize) | A | **INV-PROTO-1 (new)** | ER-3 | **RESOLVED** |

**Open Level-A findings after 1R: none.** **New D-F3 blockers: none.**

Every authorization decision survived empirical testing:
- PD-F2-01 one-shot fit: ER-4;
- PD-F2-02 focus layering with protocol-valid preemption: ER-1, without new hidden state;
- PD-F2-06 algebra: ER-5 R1;
- PD-F2-03 constraint/output separation: ER-2;
- PD-F2-07 epochs: ER-5 R2;
- PD-F2-17 maximize: ER-3.

Two empirical refinements were needed to make the decisions hold. Both are recorded, not forced:
- **Nested grabs** must keep the pointer grab (W-SM-14).
- **Constraint reconciliation** must be **deferred** during client-owned grab sequences. An explicit deactivate duplicates `unlocked` for persistent locks (ER-2 EXPLICIT; W-SM-4a).

---

## Phase D Checkpoint 2 (D7–D9): findings recorded under change control

- **Baseline:** HEAD `2ee8ab6a94f64abce2115ad65812ade37f52b824` (1R seal `84ce620`).
- **Found:** during the D7.2 namespace-authority analysis, **before any document was reorganized**.
- **Status:** D7–D9 **stopped** at PD-F2-18 (Level A), per the checkpoint rule "If a new Level-A issue appears: STOP". **Nothing is repaired here.**

| ID | Class | Level | Title | Status |
|---|---|---|---|---|
| PD-F2-18 | D-F2 | **A** | Camera creation, initial placement, output↔camera binding and persistence restore have no authorization path under the 1R proof rule | **OPEN: decision required; blocks the Phase D gate** |
| PD-F2-19 | D-F2 | C | Effective ScaleInteractionEpoch covers camera changes only; Stage-move interactions (WindowMove/WindowResize, shell `MoveElement` streams) lost their sealed gesture damping | OPEN |
| PD-F1-10 | D-F1 | — | Phase B provisional policies with no owning obligation (pan cursor behaviour, popup draw order, ⌈h⌉ integer scale, activation outcomes/shell UX, reserved `StableUnobserved(T)`) | OPEN: to be applied as obligation bookkeeping during D7 |

### PD-F2-18 (Level A): camera-state writes outside the proof-gated entry points

- **Effective rules (preserved, not edited):**
  - **INV-CAMERA-1 (C12.R1 @ `84ce620`):** "Camera state is mutated only inside `core::navigation`. Each mutation consumes a fresh `Authorized` proof, mintable only inside the port-carrying entry points (`physical_input(&mut InputPort)`, `shell_command(&mut ShellPort)`) … Core retains no proof, mode, target or tracking state permitting a later mutation."
  - **INV-CAMERA-3 (C12 @ `615b0ed`):** "cameras survive output removal; N outputs may share one camera".
- **Sealed rules that write camera state with no stated authorization:**
  - **B7.2 hotplug:** "**Add:** create the `OutputId`; bind it to the remembered camera for its `OutputKey`, else to **a new camera whose placement is Level D policy**". A remove unbinds; when the same `OutputKey` reappears, "the binding (and thus the view) is restored".
  - **B7.2 / C-AUTH:** cameras are persisted and restored, keyed by `OutputKey` (C-AUTH "Recovery source: persistence keyed by `OutputKey`"; AUD-22).
  - **B7.3 / C2.1:** the output↔camera **binding** is owned by the "output manager (policy)", "stored in the camera set".
  - **C1.2:** the backend submits "device events → `PhysicalEvent` (**submitted with `InputPort`**)", but "output discovery → `OutputDescriptor`" has **no port**. The public Core API list (C1.2: `physical_input(&mut InputPort,…)`, `shell_command(&mut ShellPort,…)`, `protocol_event(…)`, `frame_tick(…)`, `render_list(…)`) contains no output entry at all.
  - **1R M5'** (audit/1R/STATE_MACHINES_1R.md) dropped the baseline M5 row "output added, no remembered camera → new `CameraId`; placement Level D", and gave no authorization for creation or binding.
- **Counterexample:**
  - Creating a camera with an initial centre/zoom, binding or re-binding an output to a camera, and restoring persisted centres all **write camera state**. None of them can consume an `Authorized` proof, because none originates in `physical_input` or `shell_command`.
  - The effective invariant therefore either forbids behaviour the architecture requires (hotplug, persistence) or is silently violated by it.
  - **The mechanical consequence is the Level-A part.** The natural implementation of "output discovery → `OutputDescriptor`" is a public, port-free `Core` entry (e.g. `output_event(desc)`). Protocol handlers hold `&mut Core` (C1.5: "Smithay hands handlers the whole state"), so such an entry gives protocol-handling code a compile-time-reachable path to create cameras and to **re-bind which camera an output shows**: in effect, moving a user's view.
  - The OB-19 probes (incl. 1R probes 2d/2e) cover only `navigation` mutators and the two port-gated entries, so this path would pass `verify-architecture`.
  - This is the same class as PD-F2-01, an authority path outside the port boundary, and it meets the Level-A rigor criterion "competing authoritative writers" (output manager vs. navigation).
- **Affected:**
  - INV-CAMERA-1, INV-CAMERA-3, INV-OUTPUT-1, INV-ARCH-1;
  - C1.2, C1.5, B7.2, B7.3, C2.1/C-AUTH (camera and binding rows), M5/M5';
  - OB-19 (probe coverage);
  - ADR-04, ADR-08.
- **Minimal correction (proposal; options; NOT adopted):**
  - **(a) Recommended.** Output descriptors (add, remove, mode/scale/transform change) enter Core through a **port-carrying entry owned by the backend**, e.g. `Core::output_event(&mut InputPort, OutputDescriptor)`. The backend already owns both `InputPort` and output discovery. That entry mints the proof for camera creation, initial placement (Level D policy) and binding.
    - Persisted camera state is applied during `Core::new` (construction, before ports leave the composition root) or through the same port-carrying entry.
    - OB-19 gains a probe: an output entry without a port fails to compile.
  - **(b)** Define camera creation, binding and restore as "construction, not mutation", and make them reachable only from crate-private code called by the port-carrying entries. Same mechanical effect, more wording.
  - **(c) Rejected:** a port-free public output entry. It re-opens a protocol-reachable camera path.
- **More evidence needed:** after the decision, extend the authority spike with one negative probe (output entry without port → fails with its expected error) and one test (hotplug creates and binds a camera only through the port-carrying entry). This is the same scale as 1R's probes 2d/2e.

### PD-F2-19 (Level C): Stage-move interactions are outside ScaleInteractionEpoch

- **Effective rule (preserved):** INV-SCALE-3 (C12.R1) groups the "**camera changes** belonging to one logical interaction" into an epoch.
- **Sealed rules:**
  - B10.4: a binding press → `CameraPan / WindowMove / WindowResize` sets "`gesture_active` = true (B8.3)".
  - D-B8-SCALE.3: decreases are evaluated "at the end of a CameraPan **or WindowMove** interaction".
  - C14.4 SC-E (not superseded) counts "a membership change caused by camera **or Stage movement**" as a stimulus.
- **Counterexample:** a window dragged back and forth across an output edge (WindowMove), or a shell `MoveElement` stream, changes membership at every crossing. No epoch covers it, so each crossing renegotiates: the SC2 edge-oscillation churn returns for Stage movement. The effective invariant and the B10.4 state machine disagree.
- **Level C:** interactive move/resize UX is deferred (C15), and there is no authority or protocol consequence.
- **Minimal correction (proposal):** a ScaleInteractionEpoch also groups the Stage-placement changes of one interaction (WindowMove, WindowResize, a bounded shell `MoveElement` stream), with the same settlement rules.
- **Evidence:** analysis. A model case equivalent to ER-5 R2 is cheap to add once the rule is decided.

### PD-F1-10 (D-F1): provisional policies without an owning obligation

- The Phase B ledger category 3 lists provisional policies that have no `OB-*`: pan cursor behaviour, popup draw order, ⌈h⌉ integer scale, activation outcomes/shell UX, and the reserved `StableUnobserved(T)`.
- The D8.6 requirement ("owning obligation") cannot be met for them as things stand.
- **Proposed D7 application:** one new decision obligation per policy (or one grouped UX-policy obligation) with destination "decision before the feature ships". This is bookkeeping only; no behaviour changes.

### Phase D Corrective Checkpoint 2R — dispositions (uncommitted, for review)

- **Scope:** PD-F2-18 and PD-F2-19 only.
- **Canonical wording:** FOUNDATION_ARCHITECTURE §C12.R2.
- **Provenance:** [audit/2R/INVARIANT_DIFF_2R.md](audit/2R/INVARIANT_DIFF_2R.md).
- The findings above are preserved unchanged.

| Finding | Level | Replacement | Evidence | Status |
|---|---|---|---|---|
| PD-F2-18 | A | Dedicated `OutputLifecycleToken` (**INV-OUTPUT-2, new**). `output_connected`/`output_disconnected` are the only paths that create, bind or unbind cameras (INV-CAMERA-1/3, INV-OUTPUT-1 amended). Hardware-only `OutputDescriptor`. Passive persisted lookup given to `Core::new`, with no public restore API. Startup and runtime hotplug use the same path. | ER2-1: 6 new probes fail for their expected reasons (E0061, private fields, E0560, E0599, E0624, E0277); 3 new behaviour tests pass; all earlier probes still fail as expected | **RESOLVED** |
| PD-F2-19 | C | ScaleInteractionEpoch generalised to `InteractionKind ∈ {CameraPan, CameraZoom, WindowMove, WindowResize, ShellStream}` (INV-SCALE-3/4 amended); P7 kept as an immediate correctness rule | ER2-2: window drag NEG 39 → 2R 1; resize NEG 39 → 1 (P7); shell `MoveElement` bursts NEG 29 → 5 (≤ 2 per epoch); camera regressions unchanged | **RESOLVED** |
| PD-F1-10 | D-F1 | not in 2R scope; to be applied as bookkeeping in D7 | — | OPEN |

**Open Level-A findings after 2R: none.**

