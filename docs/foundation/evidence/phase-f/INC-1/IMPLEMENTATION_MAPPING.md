# INC-1 implementation mapping (planning record, not an amendment)

Source: FOUNDATION_IMPLEMENTATION_PLAN INC-1. The slice is: map one element (root + subsurfaces + one popup) → public Smithay snapshot extraction → core model → render list → DummyRenderer → physical-pixel hit-test.
- **Scope:** one output and one camera at zoom 1, through `output_connected`; no hotplug, no input routing, no scale policy.
- **Execution class:** HEADLESS.

| Canonical requirement | Owner (INV / F / OBJ / M) | Production module | Verification |
|---|---|---|---|
| Element identity minted at `new_toplevel`, monotonic `NonZeroU64`, never reused; `(InstanceId, ElementId)` | INV-STAGE-3, OBJ-02, M1 | `infynitae-core::stage` | T0 tests (monotonic, never reused after destroy, stale refs fail) |
| Stage is the only store/writer of anchors and z-order; placement via `stage::place` at initial commit (R3, ±2³⁶) | INV-STAGE-1, INV-STAGE-2, INV-COORD-1, OBJ-01, M1 | `core::stage` (`place` crate-private) | T0 tests; source rule: no other module writes anchors |
| Lifecycle per M1 (AwaitingInitialCommit → AwaitingFirstBuffer → Mapped ↔ Unmapped; destroy idempotent, any order) | INV-STAGE-4, INV-STAGE-5, M1 | `core::stage`, `core::entry` protocol events | T0 valid and illegal transition tests |
| Surface-tree snapshot: values only, opaque `SurfaceKey`, written wholesale per commit by the adapter, dropped on unmap/destroy | OBJ-05, INV-COORD-4, CP2-2 | type `core::snapshot`; writer `infynitae-wayland::snapshot` | T0 malformed-tree rejection; T1 xsnap ground truth |
| Single origin function | F-ORIGIN-1, INV-COORD-6 | `core::snapshot::origins` | T0 origin cases; T1 X-15 positions equal Smithay's `Window` path |
| `SurfaceKey ↔ WlSurface` registry | OBJ-06 | `infynitae-wayland::registry` | T1 (render elements resolve through it) |
| One output connected through the token; one camera, default placement, zoom 1, bound 1:1. A second connect is refused (multi-output, hotplug and precedence are INC-2) | INV-OUTPUT-2, INV-CAMERA-1, INV-CAMERA-3 (single-output case), M4/M5 (startup row only) | `core::entry::output_connected`, `core::navigation`, `core::output` | T0 tests; compile probes p6–p13 unchanged |
| View(O): `v_O` (R7), `view_O` = transformed `phys_O`, `k = zoom·scale` | F-VIEW-1, OBJ-12 | `core::projection` | T0 (tie rounding, transform swap) |
| Lattice: `R_O`, `C_O`, `c_eff` (R5), `edge(x)` (R4), cull before i32 | F-LATTICE-1, INV-RENDER-1, INV-COORD-2 | `core::projection` | T0 properties (OB-6: edge = round(L), re-projection, 2³⁶ magnitudes, random cameras) |
| Render list front-to-back (reverse z-order; popups, then root tree, in snapshot order); `L(x)` f64 | INV-RENDER-2, F-RENDER-1, C3.3 | `core::render` builds the list; `infynitae-wayland::convert` builds `from_surface` elements | T1 x6 (edges vs Smithay, i32-root negative control), T1 X-15 order |
| ProjectionFrame(O): written per produced frame; dead entries fail closed | OBJ-15, F-HIT-1 (PD-F1-07) | `core::render` | T0 fail-closed test |
| Physical-pixel hit-test: `u = p·scale`, floor; render order; input region add/subtract in order, half-open, clamped; delivered `q − origin` clamped | F-HIT-1, F-UNPROJECT-1, INV-HIT-1 | `core::hit`, `core::region` | T0 semantic cases + OB-2 property test vs a reference region evaluator; T1 xhit/d5hit (zoom-1) vs presented; canvas-exact negative control |
| Popup position = `get_unconstrained_geometry(target)`, target = anchor-output viewport, inward-rounded | M3, F-ROUND-1 R8, C3.6 | `core::popup` (target), `infynitae-wayland::convert` | T0 (R8 rounding), T1 (popup configured inside the target) |
| `Window::surface_under` never used; `Space` never constructed | OB-26, W-SM-1/11/12, INV-HIT-1 | source rule in `enforcement/source-rules` | rule + seeded controls |
| Headless backend: one virtual output descriptor, DummyRenderer | C1.2, C1.3 (`renderer_test`) | `infynitae-backend-headless` | T1 lockstep tests with a probe client over a socketpair (T1 tier definition) |

**Judgment calls, each recorded for the review:**
- **Placement policy (Level D):** the initial anchor is the bound camera's centre, rounded R3. With no output, it is the Canvas origin.
- **Test-only dependency:** the T1 probe client needs `wayland-client` and `wayland-protocols` (client). They are dev-dependencies of `infynitae-backend-headless` only.
  - C1.2 assigns T1 to the headless backend, and FOUNDATION_TEST_PLAN T1 requires "recording probe clients over socketpairs".
  - The verifier's external-crate whitelist is stricter than C1.2. It gains a dev-only entry for this crate alone, with seeded controls showing the same crates are still rejected elsewhere and as normal dependencies.
- **Fractional cameras in T1:** camera *state* other than the default cannot be reached before INC-3 (navigation) or INC-2 (persisted lookup). So the xhit/d5hit zoom-1 configurations feed explicit view parameters into the production `projection`, `render`, `hit` and `convert` functions. These are pure functions and grant no authority. The end-to-end test uses the real bound camera.
- **Toplevel requests:** maximize, fullscreen and minimize keep Smithay's default handlers. INV-PROTO-1 and W-SM-13 are INC-7 (M2) and are not claimed here.
