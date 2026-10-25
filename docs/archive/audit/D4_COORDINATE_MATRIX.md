# D4 — Coordinate domains and conversion matrix

Baseline: Phase C seal `615b0ed`. Sealed sources:
- B1 (numeric model, R1–R6, F1–F7);
- B2 (domains T1–T10, anchor, pointer chain, types);
- C3.2 (pipeline [1]–[7], R7);
- C3.6 (Smithay boundary);
- CP3A-2 (hit-testing);
- INV-COORD-1…6, INV-RENDER-1, INV-HIT-1.

Evidence: ED-1 (Smithay, d5hit) and ED-2 (model).

## 1. Domains

| # | Domain | Representation (B2.7 type) | Origin / meaning | Owner | Range |
|---|---|---|---|---|---|
| D1 | backend/device | `PhysicalEvent`: Δ accel/unaccel f64, absolute normalised, buttons, keys | device | backend | finite (A-B1-3) |
| D2 | Arrangement | `ArrangementPoint` f64 | pointer-crossing layout of outputs; **non-zero origins** in D5 | output configuration | union of output rects |
| D3 | output physical px | `OutputPhysicalPoint` i32 (render), f64 (pointer) | framebuffer of O after transform | renderer/backend | `[0, mode)` |
| D4 | output-local logical | `OnOutput<OutputLogicalPoint>` f64 | O's viewport, top-left (0,0) | output model | `[0, logical)` (half-open, PD-F1-05) |
| D5 | camera / view | camera: centre `CanvasPos` Q16 + zoom f64; view: `v, R, C, c_eff` | per output, derived | `navigation` / pure | centre ≤ ±2³⁶ (saturating); v up to 2³⁶ + mode/(2k) (PD-F1-02) |
| D6 | Canvas | `CanvasPoint` (integral), `CanvasPos` (Q16 i64) | Stage world, 1 unit = 1 logical px at zoom 1 | Stage | stored ±2³⁶ |
| D7 | element geometry (ElementLocal) | integral | relative to the anchor (= window-geometry origin) | derived | ±2³¹ |
| D8 | xdg window geometry | `Rectangle<i32, Logical>` in RootSurfaceLocal | client-set; loc may be negative | client (P) | i32 |
| D9 | root wl_surface (RootSurfaceLocal) | integral; f64 for pointer | root surface origin | derived | i32 |
| D10 | subsurface-local | integral; f64 | each subsurface's origin | derived | i32 |
| D11 | popup-local | integral; f64 | popup surface origin | derived | i32 |
| D12 | input region | `Vec<(add, Rectangle<i32>)>` surface-local | protocol add/subtract | client (P) | i32 |
| D13 | client pointer | `wl_fixed` (1/256) of SurfaceLocal | delivered by the Smithay seat | Smithay | protocol |
| D14 | relative deltas | `wl_fixed` accel + unaccel | physical Δ only | Smithay | protocol |
| D15 | Smithay renderer f64 | `Point<f64, Physical>` (location), `Scale<f64>` | adapter `convert.rs` only | adapter | f64 |
| D16 | persisted coordinates | anchors i64 integral; camera centre raw Q16 i64; zoom f64 (finite, > 0) | file (Level D) | persistence | validated on load (±2³⁶; reject the whole file otherwise) |

## 2. Conversion matrix

Column abbreviations: **Exact** = exact or lossy; **R** = rounding boundary (R1–R7; "ext" = external, by Smithay or the protocol); **Org** = whether the Arrangement origin participates.

| # | Source → destination | Formula | Layer | Exact | R | Range / overflow | Scale | Zoom | Org | Inverse | INV | Verification |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| X1 | D1 Δ → D2 | `a' = arr(O,p) + Δ`; clamp to the union; `(O',p') = locate(a')` | backend + PointerModel | yes | none (R6) | finite f64 | — | — | **yes** | — (events not re-emitted) | INPUT-4, COORD-4 | T0, T1 X-8 |
| X2 | D1 absolute → D4 | `p = n · logical(O)` | backend | yes | none | — | — | — | no | — | COORD-4 | T1 |
| X3 | D2 ↔ D4 (T2) | `p = a − arr_origin(O)` | output config | yes | none | — | — | — | **yes** | `a = arr_origin + p` | COORD-4 | T0 |
| X4 | D4 → D3 (T3) | `u = transform_O(p · scale_O)`; pixel `⌊u⌋` | output model | yes (f64) | pixel floor (hit only) | — | yes | no | no | `p = transform⁻¹(u)/scale` | HIT-1 | T0; OB-14 (transforms) |
| X5 | D5 camera → view (C3.2 [1]) | `v = Q16(centre − mode_O/(2k))`, `k = zoom·scale`; `R = ⌊v/120⌋·120`; `C = round((v−R)k + δ)`; `c_eff = R + Q16(C/k)` | core pure | lossy | **R7**, R4/R5 | v exact in f64 while \|v\| < 2³⁷ (holds: ≤ 2³⁶ + 2³⁰, PD-F1-02) | yes | yes | no | — | CAMERA-3, COORD-2 | T0 (OB-6); ED-1 |
| X6 | D6 edge → D3 edge (C3.2 [3]/[4]) | `edge(x) = round((x−R)k + δ) − C` | core pure | lossy | **R4** | \|x−R\|·k up to ≈ 2³⁷·k for far elements: **cull in f64 before any i32 (PD-F1-01)** | yes | yes | no | not inverted; see X8 | RENDER-1, HIT-1 | ED-1 (k ∉ n/120: 0 mismatches), X-6 |
| X7 | D6 origin → D15 location | `L = (x−R)k + δ − C` → `from_surface(.., Point<f64,Physical>(L), ..)`; damage tracker built with **`Scale(k)`** (PD-F1-03) | adapter | yes (f64) | ext: Smithay `round(L)`, `round(L + dst·k)` = X6 | bounded by the viewport after culling | yes | yes | no | — | RENDER-1 | X-6, ED-1 |
| X8 | D4 → D6 (T5, input) | `q = c_eff + Q16(p / zoom)` | core pure | lossy ≤ 2⁻¹⁷ (+ c_eff snap) | **R1** (+R5 via c_eff) | — | via k in c_eff | yes | no | X9 | COORD-5, HIT-1 | X-8, ED-2 |
| X9 | D6 → D4 (T6) | `p = (q − c_eff) · zoom` | core pure | yes up to the T5 quantum | none | — | — | yes | no | X8 | — | T0 |
| X10 | anchor → D9 origin | `root = anchor − geometry.loc` | core `origin()` | yes (integral i64) | none | anchor ±2³⁶ + i32 | — | — | no | `anchor = root + geometry.loc` | STAGE-2, COORD-6 | xsnap/xhit, ED-1 |
| X11 | D9 → D10 | `+ Σ subsurface offsets` | core `origin()` | yes | none | i32 sums | — | — | no | `−` | COORD-6 | EC-8/9 |
| X12 | D9 → D11 | `root + geometry.loc + popup_pos − popup.geometry.loc` | core `origin()` | yes | none | — | — | — | no | `−` | COORD-6 | X5, ED-1 (P at (1190,460)) |
| X13 | D6 pointer → D10/D11/D9 (client pointer) | `s = q − surface_origin`, clamped into `[0,w)×[0,h)` | core → adapter → Smithay | yes in f64; **ext** wl_fixed 1/256 | ext | — | — | — | no | `q = origin + s` | HIT-1 | EC-20, ED-1 (clamp 0) |
| X14 | D6 → Smithay pointer global | identity `Point<f64,Logical>(q)` | adapter `convert` | **exact** (\|q\| ≤ 2³⁷) | none | — | — | — | no | identity | COORD-5 | X-8 |
| X15 | D12 containment | protocol add/subtract over the clamped local point, half-open `⌊·⌋` cell test; **never** `to_i32_round` (W-SM-11) | core | yes | none | — | — | — | no | — | HIT-1 | EC-20, ED-1 (hole → S2) |
| X16 | D1 Δ → D14 | `relative_motion(Δacc, Δunacc)` | adapter | **ext** wl_fixed (1/256, EB-20) | ext | — | — | — | no | — | INPUT-4 | X-8; D5 E2 (−1.7 → −1.699) |
| X17 | pan Σ → camera | `camera = start − Q16(Σ)`; **zoom term missing (PD-F2-06)** | `navigation` | lossy ≤ 2⁻¹⁷ per update | **R2** | saturate at ±2³⁶ | — | **should** | no | — | COORD-3 | X-8 (zoom 1); ED-2 epoch 4 |
| X18 | drag → anchor | `anchor = R3(anchor₀ + Q16(Σ canvas Δ))`; **undefined across outputs (PD-F2-13)** | `stage::place` | lossy | **R3** | `Err(OutOfCanvas)` | — | via X8 | via X1 | — | STAGE-1, COORD-3 | T1 |
| X19 | placement → anchor | `CanvasPos → CanvasPoint` round-half-even | `stage::place` | lossy | **R3** | `Err(OutOfCanvas)` | — | — | no | — | STAGE-1 | T0 |
| X20 | popup constraint target | viewport(O) via X8 → parent-geometry-relative → `Rectangle<i32>` | core → adapter | lossy | **none listed → PD-F2-12 (R8)** | — | yes | yes | no | — | COORD-2 | ED-2 F7 |
| X21 | fullscreen presentation view | `v_P = anchor − max(0, (mode/scale − size)/2)`, zoom 1 → X5 | core | lossy | R7 (PD-F1-09) | — | yes | 1 | no | — | PRES-1 | ED-2 epoch 3 |
| X22 | ρ → hint emission | `preferred_scale = round(h·120)`; `preferred_buffer_scale = ⌈h⌉` | core → adapter | lossy (1/120) | ext (protocol) | h ≤ H_max | yes | yes | no | — | SCALE-5 | model |
| X23 | output scale → `wl_output.scale` | `⌈scale⌉` (Smithay) | adapter | lossy | ext | — | yes | — | no | — | — | EB-6 |
| X24 | Canvas ↔ D16 | integral i64 / raw Q16 i64 / f64 bits | persistence | **exact** | none | validated ±2³⁶ | — | stored | no | identity | FAIL-1 | Level D |
| X25 | lock point | sealed: Canvas `q_lock`; proposed: SurfaceLocal `at`, Canvas = `origin(S) + at` (PD-F2-03) | PointerModel | exact | none | — | — | — | no | — | COORD-5, INPUT-5 | ED-2 epoch 3 |
| X26 | relocation after output removal | Arrangement-nearest, half-open clamp `≤ w − 2⁻⁸` (PD-F1-05) | PointerModel | lossy (clamp) | clamp | — | — | — | **yes** | — | OUTPUT-1, INPUT-5 | ED-2 epoch 4 |

## 3. Re-falsification of the numeric model

| Claim | Test in Phase D | Result |
|---|---|---|
| **Q16 Canvas** (INV-COORD-1) | Re-derived for zoom ≠ 1, fit of huge windows and far elements (F5). | **Holds** for stored values. Derived values need a stated bound (PD-F1-02). Cull before i32 (PD-F1-01). |
| **±2³⁶** | Exactness of Canvas ↔ f64 needs \|v\| < 2³⁷ (53 − 16 bits). The worst derived view origin is 2³⁶ + 1.07·10⁹ < 2³⁷. | **Holds** (exact). |
| **Reference-relative lattice** (INV-RENDER-1) | At k ∉ n/120 a reference switch changes widths and gaps by 1 px. Smithay (ED-1): W 1394 → 1393 px, S2 100 → 99 px. Model F1: {1393, 1394} / {265, 266}; constant at k = 180/120; constant with a gesture-pinned reference. | Render/hit agreement **holds** (ED-1: 0 mismatches). Jitter-freedom **fails** for zoomed panning, which is **reachable** (PD-F2-06). |
| **Geometry anchor** (INV-STAGE-2) | Epoch 3: geometry.loc (20,20) → (0,0); anchor stays; root moves; popup origin unchanged (1190,460). | **Holds.** The Canvas lock point does not follow (PD-F2-03). |
| **Exact pixel render/hit agreement** (INV-HIT-1) | ED-1: 7 configurations including k = 1080/651 and k = 1.875, nested subsurface, hole fall-through to a below-subsurface, popup + popup subsurface. | **Holds: 0 mismatches**, delivered clamp 0. Pixel-centre bound note: PD-F0-02. |
| **Gesture-total accumulation** (INV-COORD-3) | Epoch 4 pan: Σ in f64 from the gesture origin, quantised once per update. | **Holds** as an accumulation rule. Its formula lacks zoom (PD-F2-06). If zoom changes inside a gesture, the total must be Σ(Δᵢ/zoomᵢ) in f64. That is path-dependent by nature but still quantised once per update. |
| **Multi-output camera inverse** (T5 per output) | Epochs 1, 2 and 4 with non-zero Arrangement origins: the Arrangement participates only in X1/X3/X26, never in projection (TC-9). | **Holds.** Gaps: `Locked` lacks an output (PD-F2-03); drag across outputs (PD-F2-13); confinement crossing (PD-F2-05); a crossing jump during a client grab is expected (PD-F0-01). |

## 4. Rounding never leaks back into Stage geometry

Stage geometry is written only by `stage::place` (X18, X19, MoveElement), each rounding once at R3. Inputs to `stage::place`:
- **placement:** a policy-chosen `CanvasPos`, or a pointer Canvas position (X8);
- **drag:** pointer Canvas deltas (X8);
- **MoveElement:** a shell-supplied integral point.

No projected or rendered value (X5's v/R/C, X6 edges, X7 L, X21, X22) feeds `stage::place`, cameras or snapshots (F1). Camera writes come from device Σ (X17), Stage/client geometry (fit), or shell values. None of these is a projected value.

**One subtlety (PD-F0-09):**
- X8 defines the pointer's Canvas point on the **snapped** camera `c_eff` (R5). This is deliberate: what you see is what you hit.
- A placement or drag derived from the pointer therefore sees the camera snap: ≤ ½/k logical px before R3, a function of current state, never accumulated.
- This is part of T5's definition, not a write-back of a rendered value. F1 is not violated.

**Result:** the claim holds.
