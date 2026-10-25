# Independent Reconstruction of INC-2

## 1. Canonical Charter & Scope

According to `docs/foundation/FOUNDATION_IMPLEMENTATION_PLAN.md` (lines 85–101):

- **Category:** PRODUCT
- **Execution Class:** HEADLESS
- **Objective:**
  - Multiple connected outputs through `output_connected`, `output_reconfigured`, and `output_disconnected` (startup, runtime hotplug, and runtime mode/transform/scale/refresh changes);
  - Camera creation and binding precedence: remembered-and-unbound → persisted lookup (no collision) → internal default;
  - Strict 1:1 binding invariant (`INV-CAMERA-3`);
  - Per-surface membership in `view_O` space (`F-MEMBER-1`, `INV-RENDER-3`);
  - Damage trackers sized `phys_O` (`F-RENDER-1`, `EE-1`);
  - Zero-output state (`PointerState::NoOutput`, no coordinate fabrication);
  - Frame clock selection (`INV-FRAME-2`);
  - Presentation feedback for observed elements (`INV-FRAME-5`).
- **Entry Conditions:** Ratified INC-1 baseline (`a25a890`).
- **Exit Criteria:**
  - Ported 2R output-lifecycle tests (TST-15);
  - Ported TST-03 SC1/SC3 and X5;
  - Ported TST-06 x9;
  - Ported EE-1 transform matrix across C1–C4 on the production adapter;
  - Reconfiguration tests (EE-4) and separate reconnect tests;
  - Zero-output core state tests.
- **Negative Controls:**
  - `Window::output_enter` popup membership control (X5);
  - Output lifecycle token required for all output entries (probes p6, p7, p10–p13);
  - Sealed damage tracker size (`view_O`) failing EE-1 C1–C4;
  - Runtime 2i negative control (`probe_sealed_key_steal`) failing with "1:1 violated";
  - Sealed F-RELOCATE-1 unrepresentable on an empty survivor set.

---

## 2. Invariants, Formulas, Objects, and State Machines

### Invariants Touched
- `INV-OUTPUT-1`: Output removal ordering (leave before global destroyed; camera kept unbound; pointer relocated; `NoOutput` on last removal).
- `INV-OUTPUT-2`: Output lifecycle authority via `&mut OutputLifecycleToken`; `OutputDescriptor` carries hardware facts only; no camera fields.
- `INV-CAMERA-1`: Camera mutations require fresh `Authorized` proof minted in entry module.
- `INV-CAMERA-3`: 1:1 active output <-> bound camera; M5 binding precedence.
- `INV-RENDER-3`: Output membership computed per surface from physical projection; `Window::output_enter` / `Space` forbidden.
- `INV-FRAME-1`: Frame callbacks completed only when requested by client.
- `INV-FRAME-2`: Highest-refresh observing output selected as frame clock; tie-break by lowest `OutputId`.
- `INV-FRAME-5`: Presentation feedback lifecycle (presented with sync = clock; superseded discarded; pending presented on re-entry).

### Formulas Touched
- `F-VIEW-1`: View origin and effective center in Canvas space.
- `F-RENDER-1`: Damage tracker sized `phys_O`, scale `Scale(k_O) = zoom * scale`.
- `F-MEMBER-1`: Surface membership derived from physical projection pixels.
- `F-RELOCATE-1`: Relocation to Arrangement-nearest survivor; half-open clamp `[0, extent - 2⁻⁸]`. Domain: non-empty survivor set.
- `F-RECONF-1`: Pointer clamped into reconfigured logical extent.

### Objects Touched
- `OBJ-07`: `CameraSet` (center, zoom, OutputKey, bound OutputId).
- `OBJ-08`: `OutputLifecycleToken`, `InputPort`, `ShellPort`, `Authorized`.
- `OBJ-09`: `PersistedCameras` (read-only passive lookup).
- `OBJ-10`: `OutputModel` (id, key, phys, transform, scale, refresh, arrangement).
- `OBJ-13`: `MembershipTable` (element, surface, output).
- `OBJ-17`: `FrameLiveness` (observed frame completion part).

### State Machines Touched
- `M4`: Output lifecycle (connect, reconfigure, disconnect, zero-output).
- `M5`: Camera lifecycle (bind on connect, unbind on disconnect, survive outputs).
- `M16`: Row 1 (vblank of clock output completes observed frames).

---

## 3. Explicit Exclusions & INC-3 Boundary

The following are strictly out of scope for INC-2 and deferred to future increments:
- Pointer event intake and routing (`PhysicalEvent`, F-HIT-1 routed motion) → INC-3.
- Compositor pointer grab for zero-output state (`Focus::Clear`) → INC-3.
- CameraPan interaction at zoom 1 → INC-3.
- Keyboard ownership and popup routing → INC-4.
- Pointer constraint reconciliation → INC-5.
- Scale ladder and `ScaleInteractionEpoch` settlement → INC-6.
- Protocol fullscreen configure (`INV-PRES-1`, `xdg_toplevel::set_fullscreen`) → INC-7.
- Off-screen unobserved callback kick and fallback timer → INC-8.
- Camera zoom navigation (`zoom != 1`) → INC-10.

---

## 4. Diff Comparison (`a25a890..8f27a31`)

The diff spans 29 files (3,652 insertions, 230 deletions).
- **Core crate:** Extended `entry.rs`, `navigation.rs`, `output.rs`, `pointer.rs`, `visibility.rs`. No extraneous dependencies added (Core remains std-only).
- **Wayland crate:** Added `outputs.rs` managing `wl_output`, `zxdg_output_v1`, `wp_presentation`. Damage tracker construction in `convert.rs` sizes to `phys_O`.
- **Backend headless:** Added multi-output hotplug and vblank helpers. Integration tests `t1_outputs.rs` and `t1_transform.rs` added.
- **Enforcement & Tools:** `verify-architecture` added stage `[5b]` runtime 2i and reconfiguration seeds. `verify-test-efficacy` added 21 new negative controls.

**Comparison Verdict:** The implementation is **EXACT**. It implements all requirements of INC-2 without scope bleed into INC-3.
