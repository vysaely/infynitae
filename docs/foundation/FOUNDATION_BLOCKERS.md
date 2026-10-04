# Infynitae Native — Foundation Blockers (Phase B)

Status: **IN PROGRESS. Checkpoint 2 of Phase B: B1–B9 complete. B10–B13 not started.**
Phase B is **not** complete. Q-07 (Smithay pin strategy, B12) is still open (§9).
Checkpoint 1 (B1–B6) is commit `9f3f5d4`.

Evidence baseline: Phase A artifacts at commit
**`2c8c163784e260af4aaa8c1dc05c6281b0476120`** (`~/infynitae-native`, branch `main`).
Every `EV-*` reference resolves against that commit's `FOUNDATION_EVIDENCE.md`.
New Phase B evidence uses `EB-*` IDs and lives in [`evidence/phase-b/`](evidence/phase-b/).

**Conclusion classes** (per the Phase B authorization):
`PROTOCOL` (protocol-guaranteed), `SMITHAY` (Smithay-0.7.0-source-verified), `EXPERIMENT`
(experimentally verified), `DECISION` (derived architectural decision), `ASSUMPTION` (remaining assumption).

---

## 0. Phase B evidence produced so far

(EB-8 … EB-12 are listed in B8.1.)

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
   - **(a) Protocol kick.** When the compositor sends a client a request that needs the client's main loop (`xdg_wm_base.ping`, `xdg_toplevel.close`, a non-initial configure, `xdg_popup.popup_done`), it first completes that client's withheld callbacks once. This bounds protocol responsiveness at zero idle cost (measured).
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

## 8. Experiments: status

| ID | Experiment | Status |
|---|---|---|
| E1–E9 | Numeric (EB-1) | **RUN**: B1 |
| S1–S7 | `Space` semantics (EB-2) | **RUN**: B2.4 |
| X-1 | Client-side trace during scripted pan (configure, enter/leave, scale) | **RUN** (SC1, SC4): 0 pan-induced configures. *Pointer* leakage during pan is not covered; the harness has no input path. That part moves to B11 (X-8). |
| X-2 | Real clients off-screen: Starve / Throttle / kick | **RUN** (EB-9): vkcube, kitty, Firefox. mpv and mesa-demos are not installed (not installed, to avoid system changes); games deferred (X-7). |
| X-3 | Two outputs 1.0/1.5, one spanning window | **RUN** (SC1, SC2) |
| X-4 | Smithay overlap convention | **RUN** (EB-2 S5) |
| X-5 | Popup enter/leave offset (EB-H1) | **RUN**: confirmed (B8.5) |
| X-6 | Subsurface edge rounding under f64 root location | **PENDING**: T1 |
| X-7 (new) | Off-screen Wine/Proton game and a network client under D-B9-FRAMES (Starve vs P=1 s vs kick); detect frozen simulation or disconnects | **DEFERRED** to T5. Needs a real game and must not touch the live session. It decides P's default, not the architecture (P is already finite and configurable) |
| X-8 (new) | Pointer/relative-motion leakage during CameraPan | **PENDING**: B11 |
| X-9 (new) | Presentation-feedback resolution for unobserved commits (B9.4) | **PENDING**: T1 |

Harness limitations (stated so they are not mistaken for coverage):
- integer cameras, zoom 1;
- no renderer (visual quality of downsampling not measured);
- no presentation-time;
- no input backend;
- real-client tests used a single output.

---

## 9. Q-01 … Q-09 dispositions

| Q | Question | Status | Where |
|---|---|---|---|
| Q-01 | Off-screen frame-callback policy | **RESOLVED**: D-B9-FRAMES (withhold + protocol kick + finite fallback P; P's default provisional pending X-7) | B9.2–B9.3 |
| Q-02 | `suspended` vs I3 | **RESOLVED**: never from camera visibility; semantic reasons only (D-B9-SUSPEND) | B9.5 |
| Q-03 | Stage anchor | **RESOLVED**: window-geometry origin | B2.3 |
| Q-04 | Popup constraint boundary | **RESOLVED**: anchor-output viewport. Fallback rule 2 now uses the parent's frame-clock output (D-B8-CLOCK). EB-H1 affects only membership, which D-B8-ENTER handles | B6.2, B8.4 |
| Q-05 | Smithay pointer global space | **RESOLVED**: Canvas; authoritative pointer = `(OutputId, OutputLogical)` | B2.5 |
| Q-06 | Single preferred scale on mixed-scale outputs | **RESOLVED** as INFYNITAE POLICY: D-B8-SCALE (MaxGesture; ⌈⌉ for integer scale) | B8.3 |
| Q-07 | Smithay pin strategy | **OPEN** (B12, Level B) | — |
| Q-08 | Canvas numeric model | **RESOLVED** | B1 |
| Q-09 | Restated I3 | **RESOLVED**: I3′ | B9.7 |

---

## 10. Risks and open hypotheses

| ID | Risk / hypothesis | Severity | Mitigation / test |
|---|---|---|---|
| RB-1 | Smithay `Window::geometry()` re-clamps dynamically | low | T3 |
| RB-2 | `PopupManager::grab_popup` panics on root mismatch | medium | Check precondition (C8) |
| RB-3 | Buffer-before-configure enforcement is opt-in | medium | Call `ensure_configured`. T1 |
| RB-4 | Arbitrary destruction order on client death | medium | Idempotent handlers. T1 |
| RB-5 (new) | Smithay membership helpers wrong for CSD trees (EV-S12, EB-H1 confirmed) | **resolved by design** (D-B8-ENTER); medium if regressed | T1 regression: popup/CSD enter/leave against computed truth |
| RB-6 (new) | Default `CompositorState::new` is v5; using it silently disables `preferred_buffer_scale` | low | Use `new_v6`; T1 asserts the bound version |
| RB-7 (new) | MaxGesture holds the higher scale during gestures: 2.25× buffer memory while multi-observed | low | Measured cost (SC2). Revisit if memory pressure appears |
| RB-8 (new) | Fallback P = 1 s may be too slow (game timeouts) or too costly (software clients: kitty/swrast 10% CPU) | medium | X-7; P is configurable |
| RB-9 (new) | The protocol kick completes callbacks for every unobserved surface of a client, including ones the request was not about | low | Bounded: once per compositor request |
| A-B1-1 | Zoom anti-jitter rule unresolved | low (Level C) | Before zoom work |

---

## Phase B gate status

| B-item | Status |
|---|---|
| B1 Numeric model | **Complete** |
| B2 Coordinate algebra | **Complete** |
| B3 Object graph | **Complete** |
| B4 Identity | **Complete** |
| B5 Lifecycle | **Complete** |
| B6 Popups/subsurfaces | **Complete** (Q-04 resolved at checkpoint 2) |
| B7 Camera/output model | **Complete** (checkpoint 2) |
| B8 Multi-output semantics | **Complete** (checkpoint 2); worked example measured |
| B9 Visibility/off-screen | **Complete** (checkpoint 2). P default provisional (X-7) |
| B10 Input & activation | **Not started** |
| B11 Camera pan | **Not started** (X-8) |
| B12 Backend boundary | **Not started** (Q-07) |
| B13 Blocker report finalisation | **Not started** |

Phase B as a whole: **NOT COMPLETE.** Do not advance to Phase C.
