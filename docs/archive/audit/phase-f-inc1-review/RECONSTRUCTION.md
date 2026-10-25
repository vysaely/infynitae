# Canonical Architecture Reconstruction for INC-1

## 1. Authority and Scope Boundaries

Increment INC-1 is defined by:
- `docs/foundation/FOUNDATION_IMPLEMENTATION_PLAN.md` (lines 67–84)
- `docs/foundation/FOUNDATION_ARCHITECTURE.md` (C1.1, C1.2, C1.3, C1.5, C2.1, C2.2, C3.3, C3.6, C3.7, C7.4)
- `docs/foundation/FOUNDATION_INVARIANTS.md` (INV-STAGE-1..6, INV-COORD-1,2,4,6, INV-RENDER-1,2, INV-HIT-1, INV-ARCH-1, INV-OUTPUT-2, INV-CAMERA-1,3)
- `docs/foundation/FOUNDATION_OBJECT_MODEL.md` (OBJ-01, OBJ-02, OBJ-05, OBJ-06, OBJ-12, OBJ-15)
- `docs/foundation/FOUNDATION_STATE_MACHINES.md` (M1, M3, M4/M5 startup row)
- `docs/foundation/FOUNDATION_TEST_PLAN.md` (T0, T1, TST-04, TST-09, TST-10, TST-18 zoom-1)

## 2. Independent Requirement Matrix

| ID | Requirement | Canonical Authority | Required Behavior | Evidence Class | Implemented? | Verified? | Finding |
|---|---|---|---|---|---|---|---|
| REQ-01 | Stage Element Identity | INV-STAGE-3, OBJ-02, M1 | NonZeroU64, strictly monotonic, never reused across process | TESTED | Yes | Yes (T0) | None |
| REQ-02 | Stage Single Store / Writer | INV-STAGE-1, INV-STAGE-2, INV-COORD-1, OBJ-01 | Stage is sole store/writer of placement & z-order; `place` crate-private | RUNTIME + STATIC | Yes | Yes (T0 + syn rule) | None |
| REQ-03 | Element Lifecycle | INV-STAGE-4, INV-STAGE-5, M1 | AwaitingInitialCommit → AwaitingFirstBuffer → Mapped ↔ Unmapped; idempotent destroy | TESTED | Yes | Yes (T0 + T1) | None |
| REQ-04 | Surface Tree Snapshot | OBJ-05, INV-COORD-4, CP2-2 | Values only; opaque SurfaceKey; written wholesale per commit; dropped on unmap/destroy | COMPILED + TESTED | Yes | Yes (T0 + T1) | None |
| REQ-05 | Single Origin Function | F-ORIGIN-1, INV-COORD-6 | CanvasPoint origins: root = anchor − geom.loc; popup = root + geom.loc + pos − pop.geom.loc; sub = parent + offset | PROVED + TESTED | Yes | Yes (T0 + T1) | None |
| REQ-06 | Surface Registry | OBJ-06, C2.2 | Bi-directional mapping SurfaceKey ↔ WlSurface; adapter-only | TESTED | Yes | Yes (T1) | None |
| REQ-07 | Single Output Startup | INV-OUTPUT-2, INV-CAMERA-1, INV-CAMERA-3, M4/M5 | First connect creates & binds one camera, zoom 1. Second connect refused with Unsupported | TESTED + STATIC | Yes | Yes (T0 + probes) | None |
| REQ-08 | View Projection | F-VIEW-1, OBJ-12 | $v_O = Q16(\text{centre} - \text{view}_O / (2k))$; ties-even R7; transform size swap | PROVED + TESTED | Yes | Yes (T0) | None |
| REQ-09 | Lattice & Edges | F-LATTICE-1, INV-RENDER-1, INV-COORD-2 | $R = \lfloor v/120 \rfloor \cdot 120$; $C = \text{round}((v - R)k + \delta)$; $\text{edge}(x) = \text{round}((x - R)k + \delta) - C$ | PROVED + TESTED | Yes | Yes (T0 + T1) | None |
| REQ-10 | Render List Construction | INV-RENDER-2, F-RENDER-1, C3.3 | Front-to-back: reverse z-order, popups then root downward; f64 location $L(x) = (x - R)k + \delta - C$ | TESTED | Yes | Yes (T1) | INC1-F07 (coverage) |
| REQ-11 | ProjectionFrame | OBJ-15, F-HIT-1, PD-F1-07 | Stored per frame; dead entries fail closed (no delivery / no fall-through) | TESTED | Yes | Yes (T0) | None |
| REQ-12 | Physical Hit-Test | F-HIT-1, F-UNPROJECT-1, INV-HIT-1 | $u = \lfloor p \cdot \text{scale} \rfloor$; front-to-back traversal; half-open input region; delivered $q - \text{origin}$ clamped | PROVED + TESTED | Yes | Yes (T0 + T1) | INC1-F06 (oracle) |
| REQ-13 | Popup Placement Target | M3, F-ROUND-1 R8, C3.6 | Target = output viewport inward-rounded (R8); positioner unconstrained geometry slide | TESTED | Yes | Yes (T0 + T1) | None |
| REQ-14 | Rejected Smithay APIs | OB-26, W-SM-1,5,11,12 | `Space`, `surface_under`, `render_elements_from_surface_tree`, `output_enter/leave` forbidden | STATICALLY ENFORCED | Yes | Yes (syn rule + seeds) | None |
| REQ-15 | Headless Backend Seam | C1.2, C1.3 | One virtual output, DummyRenderer, socketpair probe client | TESTED | Yes | Yes (T1) | None |
| REQ-16 | Safe Code Policy | INV-ARCH-2, C9 | `#![forbid(unsafe_code)]` in core/ipc, `#![deny(unsafe_code)]` elsewhere, zero unsafe blocks | STATICALLY ENFORCED | Yes | Yes (syn rule + probes) | None |
| REQ-17 | Dependency DAG | INV-ARCH-1, C1.2, C7.4 | core has 0 deps; smithay reachable only via adapter/backend; dev whitelist restricted | STATICALLY ENFORCED | Yes | Yes (verify-arch) | None |
| REQ-18 | Negative Controls | FOUNDATION_TEST_PLAN §4 | i32-root control, canvas-exact control, surface_under control (W-SM-11/12) | TESTED | Yes | Yes (T1) | None |

## 3. Scope Boundary Audit

1. **INC-1 Inclusions:**
   - Single real Wayland client over socketpair with toplevel, subsurface above/below, nested subsurface, popup, popup subsurface, custom input region.
   - Core stage, lifecycle, projection, render list, hit-test.
   - Adapter snapshot extraction, registry, convert.
   - Headless backend with DummyRenderer.
2. **INC-2 Defers (Strictly preserved):**
   - Multi-output connect/reconfigure/disconnect (`Unsupported`).
   - OutputKey comparisons / persistence lookups (opaque type).
   - Binding precedence and runtime 2i (OB-49).
   - Surface membership in `view_O` and `wl_surface.enter/leave`.
   - Damage tracking and damage trackers sized `phys_O`.
   - Frame clock, vblank, presentation feedback.
