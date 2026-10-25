# INC-9 — the representable zoom domain: contract, proof, amendment texts (2026-10-16)

Status: **EXPERIMENTAL implementation and PROPOSED amendment texts. Nothing is adopted.** Branch
`experimental/inc-9-zoom-contract` from `main` `23752f1`. It replaces candidate O3
(`experimental/inc-9-zoom-domain`, `8f73315`, kept as evidence). O3 kept any zoom in the camera
and froze the projection at a bound: an invalid value lived on silently. Here an invalid value
is never accepted.

The defect itself (exact arithmetic, measured thresholds) is in `ZOOM_DOMAIN.md` on the O3 branch:
on `main` a persisted zoom below ~1.4·10⁻¹¹ (Q16 view origin, `r * Q16_ONE`) or above ~10⁷
(`edge`, `− C`) overflows — a debug panic, a release wrap.

## 1. The contract

| Quantity | Domain | Why this bound |
|---|---|---|
| Camera centre | \|c\| ≤ 2³⁶ px per axis, Q16 | INV-COORD-1 (unchanged) |
| Output mode side | 1 … 2¹⁵ px (32,768) | above every real mode (DRM limits stop at 16,384); bounds `view_O` |
| Output scale | 0.25 … 8 (`scale_120` 30 … 960) | covers every scale in use; bounds `k_O` from both sides |
| Camera zoom | 2⁻²⁰ … 2¹⁶, finite | below: the largest view at the smallest scale keeps its origin within F-VIEW-1's exact range (`2¹⁵ / (2 · 2⁻²⁰ · 0.25) = 2³⁶`); above: one Q16 Canvas quantum (2⁻¹⁶ px) is one logical px, the finest the pointer can address (F-UNPROJECT-1) |
| Element edges | \|x\| ≤ 2³⁶ + 2³¹ px | anchor (INV-COORD-1) plus a protocol size |

**Enforcement, at every point where a value enters Core:**

| Entry point | Rule | Code |
|---|---|---|
| Persisted cameras (`PersistedCameras::from_entries`) | a zoom outside the domain rejects the table, as any invalid entry already does (M20: all-or-nothing) | `navigation.rs` |
| Camera creation (M5) | zoom is a validated persisted value or 1; `debug_assert` | `navigation.rs` |
| Output connect / reconfigure (M4) | mode or scale outside the domain → `InvalidDescriptor`, the output is not connected / not reconfigured | `output::valid` |
| `View::new` | refuses anything outside the domain (`InvalidParams`); unreachable through Core's entries, it stops a caller that bypassed them | `projection.rs` |
| Future zoom writers (INC-10 zoom steps and gestures, F-FIT-1, shell `NavigateTo`) | **must** clamp to the domain explicitly: the camera then holds the bound itself, never a value outside it | amendment A1 below; no writer exists today |

## 2. Proof that accepted states cannot overflow or wrap

With the domain above, `k = zoom · scale ∈ [2⁻²², 2¹⁹]` and `view ≤ 2¹⁵`:

| Intermediate | Bound | Limit it must stay under |
|---|---|---|
| `view / (2k)` | ≤ 2¹⁵ / 2⁻²¹ = 2³⁶ | — |
| `v_O = centre − view/(2k)` | \|v_O\| ≤ 2³⁷ px | F-VIEW-1's exact range; Q16 value ≤ 2⁵³ (exact in f64) |
| `R · 2¹⁶` | ≤ (2³⁷ + 120) · 2¹⁶ < 2⁵⁴ | i64 2⁶³ |
| `C = round((v − R)·k + δ)` | 0 ≤ C ≤ 120 · 2¹⁹ + 1 < 2²⁶ | i64 |
| `c_eff = R + Q16(C/k)` | < 2⁵⁴ | i64 |
| `edge(x) = round((x − R)·k + δ) − C`, \|x − R\| < 2³⁸ | \|·\| < 2⁵⁷ + 2²⁶ | the `f64 → i64` cast saturates at 2⁶³; `− C` cannot overflow |
| `unproject = c_eff + Q16(p / zoom)`, p ≤ 2¹⁵ / 0.25 = 2¹⁷ | \|Q16(p/zoom)\| ≤ 2¹⁷ · 2²⁰ · 2¹⁶ = 2⁵³; sum < 2⁵⁵ | i64 |
| `project`, `pixel`, `render_location` | finite f64 (inputs bounded as above) | — |

Every intermediate is monotone in each parameter (centre, view, zoom, scale, edge), so its
extremes are at the corners of the domain. The tests evaluate exactly those corners:

- `projection::tests::the_accepted_domain_never_overflows`: zoom × scale × view size × centre
  (each axis) at their extremes and neighbours (3 × 3 × 4 × 25 = 900 views, each with 7 edge
  positions per axis), plus 20,000 random states inside the domain. Checked: no overflow, |v| ≤ 2³⁷,
  monotone edges, camera at the view centre within half a physical px plus two Q16 quanta,
  unproject ∘ project = identity within one quantum.
- **Debug** build (overflow checks on): PASS. **Release** build with `-C overflow-checks=on`:
  PASS. **Release** build as shipped (wrapping): PASS — a wrap would break the asserted
  invariants.
- `a_view_outside_the_domain_is_refused`, `the_domain_is_enforced_where_zooms_and_outputs_enter_core`,
  `core_runs_at_the_extremes_of_the_domain` (Core at all four (zoom, output) extremes with the
  camera at the Canvas corner: map, render list, hit test, pointer routing).
- Four seeded controls, all DETECTED: ZOOM_MIN widened to 2⁻³⁰ (origin leaves the exact range),
  ZOOM_MAX widened to 2⁴⁰ (edges overflow), persisted zoom not checked, output mode not checked.

## 3. Consequences

- **Zoom range a user can reach:** 2⁻²⁰ … 2¹⁶. At 2⁻²⁰ a 1920 px wide view at scale 1 shows
  2³¹ Canvas px across; at 2¹⁶ one Canvas px is 65,536 logical px. Neither limit is visible in use.
- **F-FIT-1** (one-shot fit, `zoom = min(view_w / W.w, view_h / W.h)`): a window too large to fit
  at 2⁻²⁰ (wider than ~2³¹ px on a 1920 px view, narrower views proportionally) is fitted at 2⁻²⁰
  and shown in part. Today no fit is executed (INV-CAMERA-5 has no code before INC-10).
- **Persisted cameras:** a stored zoom outside the domain now rejects the table at startup (the
  existing all-or-nothing rule). A table written by Infynitae is always inside it.
- **Outputs:** a mode side over 32,768 px or a scale outside 0.25 … 8 is refused at connect
  (the output stays unused). No real device is affected; a backend reporting such values has a
  bug of its own.

## 4. OB-24: the fixed lattice reference, evaluated

> **Superseded 2026-10-16** by `OB24_FINAL_DECISION_2026-10-16.md` §0: the anchor belongs to the
> output's projection state, not to the camera; this section and A2 below are withdrawn.

Evidence: `README.md` (INC-9 harness). Recommendation there: fix `R` per camera until its
effective scale changes, persisted with the camera. Evaluated against what the brief asks:

- **The reference is per view, not per camera.** `R_O = floor(v_O / 120)·120` depends on `v_O`,
  which depends on the output (`view_O`, `scale_O`). Each output has its own camera today (M5:
  one camera bound to one output), so "per camera" is right as long as that holds; the rule must
  say "re-fixed whenever `k_O` or `view_O` changes" (zoom, output scale, mode or transform).
- **Persistence:** `R` joins `(key, centre, zoom)` in the persisted table (a schema change; G-OB48
  gates shipping persisted cameras anyway). Without it a restart re-derives `R`: a one-time ±1 px
  shift of some widths. A persisted `R` must satisfy `|v_O − R| < 2³⁷` (both are in the domain:
  checked at load) or be re-derived.
- **Zoom transitions:** the rule removes ±1 px width/gap changes **while panning at a fixed zoom**
  only. During an *animated* zoom (INC-10) `k` changes every frame, so `R` is re-fixed every frame
  and every relative edge is re-rounded: the ±1 px relative jitter OB-24 names ("animated/moving
  zoom") is **not** addressed by any choice of `R`. It needs its own rule, for example rendering
  at unsnapped (fractional) positions while `k` animates and snapping when it settles, or
  restricting animated zoom to `k ∈ n/120` steps, where widths are exact. This is a decision for
  G-OB24 that the INC-9 evidence does not yet cover.
- **Multi-output:** two outputs never share a view; each keeps its own `R`. A window spanning two
  outputs is rounded independently on each (as today); pinning does not change that.
- **Hit testing:** unaffected (it reads the `R` that was rendered, ProjectionFrame).

## 5. Amendment texts proposed (NOT ADOPTED)

**A1 — representable domain (F-VIEW-1, P1, output model M4):**

- P1 (FOUNDATION_ARCHITECTURE, "Frozen properties", D-CP3A-SCALE), "Camera zoom is unbounded; the hint is not." → "Camera zoom is bounded only by the
  representable domain 2⁻²⁰ … 2¹⁶ (F-VIEW-1); the hint is bounded by P1's device limit."
- F-VIEW-1, derived-range sentence → "Domain: output mode sides 1 … 2¹⁵ px, output scales
  0.25 … 8, camera zoom 2⁻²⁰ … 2¹⁶, camera centre within INV-COORD-1. Within it |v_O| ≤ 2³⁷
  (exact in f64), and every projection intermediate stays below 2⁵⁷ (table in
  evidence/phase-f/INC-9/ZOOM_CONTRACT.md). A value outside the domain is refused where it enters
  Core (persisted table, output descriptor); a zoom writer clamps its result to the domain."
- M4: "validates the hardware facts" gains "(mode sides 1 … 2¹⁵, scale 0.25 … 8)".

**A2 — G-OB24 (only if the owner chooses the pinned reference):** F-LATTICE-1, `R_O` → "fixed when
the view's effective scale `k_O` or extent `view_O` is set (camera creation, zoom change, output
scale, mode or transform change) as `floor(v_O / 120)·120` at that moment; kept otherwise;
persisted with the camera." Plus a separate rule for animated zoom (§4), to be decided.

## 6. Not covered

A GPU renderer (INC-13) at the extremes; INC-10's zoom writers (none exist); the visual effect of
the bounds (T3).
