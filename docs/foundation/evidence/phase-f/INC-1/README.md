# INC-1 evidence — minimal real vertical seam

- **Category / execution class:** PRODUCT, HEADLESS.
- **Implementation mapping:** [`IMPLEMENTATION_MAPPING.md`](IMPLEMENTATION_MAPPING.md).
- **Raw outputs** (run on the implementation tree before commit; the commit adds nothing but these documents):
  - [`test-matrix.txt`](test-matrix.txt): cargo check, build, test, fmt and clippy, the foundation checker and its self-test, and the historical spike;
  - [`verify-architecture.txt`](verify-architecture.txt);
  - [`verify-test-efficacy.txt`](verify-test-efficacy.txt).

**Status: INC-1 IMPLEMENTED — PENDING INDEPENDENT REVIEW.**
- OB-2 and OB-26 are mechanically satisfied but stay `OPEN` until the review ratifies them, as OB-19 and OB-36 did for INC-0.
- No canonical architecture, invariant, formula or audit record was changed.

## 1. What runs end to end

A real Wayland client (probe, over a socketpair, in lockstep) maps one element: root, subsurfaces above and below, a nested subsurface, one popup with its own subsurface, and an input region with a hole. The pipeline then runs:

1. **Snapshot:** `infynitae-wayland` extracts the surface-tree snapshot in the commit hook (Smithay 0.7.0 public API only).
2. **Lifecycle:** `infynitae-core` drives M1 and places the element at its initial commit (`stage::place`).
3. **Render list:** Core builds the render list for the one output, through the camera created by `output_connected`.
4. **Elements:** `infynitae-wayland::convert` turns it into `WaylandSurfaceRenderElement`s with `DummyRenderer`, via `infynitae-backend-headless`.
5. **Hit-test:** Core selects in physical pixels from the recorded ProjectionFrame.

## 2. Classification of evidence

| Property | Class | Evidence |
|---|---|---|
| Hit-test selects the presented surface (INV-HIT-1, F-HIT-1), real camera, output scales 1, 1.25, 1.5, 1.75 | TESTED (T1) | `t1_slice::vertical_slice_hit_test_equals_presented_surface`: every pixel over the tree, 0 mismatches |
| Core edges = Smithay presented edges on the real frame (INV-RENDER-1) | TESTED (T1) | same test, every entry |
| TST-09 xsnap: snapshot = client ground truth; X-15 order and positions = `Window::render_elements` | TESTED (T1) | `t1_parity::tst09_xsnap_x15_and_surface_under_control` |
| Negative control: `Window::surface_under` disagrees, only by W-SM-11/12 | TESTED (T1) | same test |
| TST-04 x6: f64 `from_surface` edges = `edge(x)`, scales n/120 (120..=360) × magnitudes up to 2³⁶ | TESTED (T1, production pure functions with explicit views) | `t1_parity::tst04_x6…`: 63 016 rects, 0 mismatches; i32-root control mismatches |
| TST-10 xhit: physical selection = presented at k ∈ {1, 1.25, 1.5, 1.75} × 4 fractional cameras | TESTED (T1, explicit views) | `t1_parity::tst10_xhit…`: 0 mismatches; canvas-exact control mismatches |
| TST-18 d5hit zoom-1 configuration (3840×2160, scale 1.5, D5 camera) | TESTED (T1, explicit view) | `t1_parity::tst18_d5hit_zoom1…`: 0 mismatches |
| TST-18 zoomed configurations | DEFERRED | INC-10 (zoom ≠ 1) |
| OB-2: region = reference evaluator, random multi-rect interleaved add/subtract | TESTED (T0) | `render::tests::ob2_region_equals_reference_evaluator` (400 random regions × 2 304 cells) |
| OB-6 T0: edge = round(L), re-projection same pixel, magnitudes to 2³⁶, random cameras | TESTED (T0) | `projection::tests::ob6_…` (4 000 random cameras) |
| OB-6 physical-output part | DEFERRED | T4 |
| F-VIEW-1 R7 ties-even, F-LATTICE-1 floor of negative origins, transform swap | TESTED (T0) | projection and output tests |
| F-ORIGIN-1 (root, nested subsurface, popup, popup subsurface; malformed trees rejected) | TESTED (T0 + T1) | snapshot tests; xsnap |
| M1 map / unmap / remap / destroy; illegal transitions rejected without mutation | TESTED (T0) | `protocol::tests::m1_…` |
| M1 destroy through the protocol purges the element | TESTED (T1) | `t1_slice::destroyed_element_leaves_the_render_list` |
| INV-STAGE-3 ids monotonic, never reused, stale refs | TESTED (T0) | stage tests |
| PD-F1-07 dead frame entries fail closed | TESTED (T0) | `tests::dead_frame_entries_fail_closed…` |
| M3 popup target = output viewport, inward-rounded (R8); popup slid inside it | TESTED (T0 + T1) | `popup_target_is_the_viewport_inward_rounded`; `t1_parity::popup_is_configured_inside_the_output_target` |
| M4 startup row: first `output_connected` creates and binds one camera, default placement, zoom 1 | TESTED (T0) | `entry::tests::first_connect…` |
| INV-CAMERA-1: protocol events never move cameras; protocol policy cannot mint | TESTED (T0) + STATICALLY ENFORCED | `protocol_events_never_move_cameras`; compile probes; rule `mint-location` |
| INV-STAGE-1 single writer; INV-STAGE-6 nothing written back | STATICALLY ENFORCED | rule `stage-single-writer` + seed |
| OB-26 `Space`, `surface_under`, `render_elements_from_surface_tree`, `output_enter/leave` never used | STATICALLY ENFORCED | rule `smithay-rejected-apis` + 3 seeds |
| INV-COORD-4 Smithay geometry only in `convert` | STATICALLY ENFORCED | rule `smithay-geometry-only-in-convert` + seed |
| Multi-output, binding precedence, 1:1 collision (2i), hotplug, reconfiguration, membership, frames | DEFERRED | INC-2 (OB-49 owns runtime 2i) |
| Pointer/keyboard intake, seat, routing | DEFERRED | INC-3, INC-4 |
| Scale policy | DEFERRED | INC-6 |
| Toplevel requests (INV-PROTO-1, W-SM-13) | DEFERRED | INC-7 (finding INC1-F01) |
| Nested popup chains (OB-1) | DEFERRED | INC-7 exit |

## 3. Test efficacy

`tools/verify-test-efficacy` (new, also run in CI) injects one defect at a time into a temporary copy. It requires the named test to fail as a test; a compile error or an unmatched filter is a BROKEN control.

**18/18 defects detected.** Two judge controls also behave: a harmless seed is MISSED, and an unknown test name is BROKEN.

The defects:
- **Formulas:**
  - edges floored instead of rounded (T0 and T1);
  - view origin rounding half away from zero;
  - lattice reference truncated instead of floored;
  - popup geometry offset sign.
- **Hit-test:**
  - first matching region request decides;
  - delivered coordinates unclamped;
  - entries traversed back to front (T0 and T1).
- **Lifecycle and scope:**
  - elements back to front;
  - dead frame entries delivered;
  - unmap keeps the snapshot;
  - buffer before configure accepted;
  - second output accepted;
  - popup target floored at its minimum edges.
- **Adapter:**
  - popup geometry origin dropped;
  - render location floored;
  - subsurface stacking ignored.

The canonical tree is byte-identical afterwards.

## 4. Enforcement changes (INC-0 baseline preserved)

- **Source rules** (`enforcement/source-rules`): new rules `stage-single-writer`, `smithay-rejected-apis` and `smithay-geometry-only-in-convert`. `unsafe-policy` now also scans `tests/`, `benches/`, `examples/` and `build.rs`.
- **`tools/verify-architecture`:**
  - **New dev-only whitelist:** `ALLOWED_DEV_EXTERNAL` admits `wayland-client` and `wayland-protocols` as **dev-dependencies of `infynitae-backend-headless` only**, for the T1 probe client.
    - Grounds: FOUNDATION_TEST_PLAN T1 ("recording probe clients over socketpairs") and C1.2 (T1 via the headless backend). C1.2 forbids neither edge.
    - Seeded controls show the same crates are still rejected as a normal dependency of the backend, and as dev-dependencies of the adapter, core or ipc.
  - **Seeds:** 10 new seeded controls; 46 detected in total.
- **Unchanged:** every INC-0 probe, F01/F02 seed, pin and judge control is unchanged and passing.

## 5. Obligations

| Obligation | Criterion | Mechanism | Command | Evidence | Disposition |
|---|---|---|---|---|---|
| OB-2 | equals a reference region evaluator; T0 property + T1 xhit port | `InputRegion::contains` (last matching request decides, half-open) | `cargo test -p infynitae-core ob2`; `cargo test -p infynitae-backend-headless --test t1_parity tst10` | 0 differences; T1 0 mismatches | **mechanically satisfied — OPEN pending independent review** |
| OB-26 | xsnap/xhit/X-15 ported to T1; `Window::surface_under` never used | T1 tests; rule `smithay-rejected-apis` | `cargo test -p infynitae-backend-headless`; `tools/verify-architecture` | all pass; seeds detected | **mechanically satisfied — OPEN pending independent review** |
| OB-6 | T0 part: edge = round(L), re-projection, 2³⁶, random cameras | `projection::View` | `cargo test -p infynitae-core ob6` | 4 000 cameras, all hold | T0 part satisfied; **OPEN** (T4 part) |
| OB-1 | nested popup chains | — | — | one level only | OPEN (INC-7 exit) |
| OB-14 | transforms, zoom ≠ 1, spanning, random trees | — | — | zoom-1, Normal only | OPEN (INC-10, OB-45) |
| OB-17 | real CSD clients (W-SM-9) | `Window::geometry` accepted | — | — | OPEN (T3) |
| OB-49 (runtime 2i) | — | — | — | not started | OPEN (INC-2) |

## 6. Findings

| ID | Level | Statement | Disposition |
|---|---|---|---|
| INC1-F01 | C | The adapter keeps Smithay's default `maximize_request`/`fullscreen_request`, which send a configure, including before the initial configure (W-SM-13). INV-PROTO-1 and INV-PRES-1 behaviour is INC-7 (M2) and is not claimed. | open; INC-7 |
| INC1-F02 | observation | Smithay 0.7.0's xdg-shell delegate requires `SeatHandler`, so `WaylandState` holds a `SeatState` with no seat and no `wl_seat` global; nothing routes input. | none |
| INC1-F03 | observation | Placement policy (Level D) chosen: the anchor at initial commit is the bound camera's centre, rounded R3, or the Canvas origin without an output. | review may revise |
| INC1-F04 | observation | `output_connected` refuses a second output (`Unsupported`) until INC-2. `OutputKey` stays opaque (`OutputKey::opaque()`), because no key comparison exists before INC-2. | INC-2 |
| INC1-F05 | observation | Fractional cameras and 2³⁶ magnitudes are reached on the production pure functions with explicit view parameters, not through camera state, which cannot change before INC-3. | INC-3/INC-10 re-run on real camera state |

Open Level A: 0. Open Level B: 0. Open Level C: 1 (INC1-F01, owned by INC-7).
