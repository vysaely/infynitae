# INC-3 implementation mapping (planning record, not an amendment)

Written before any INC-3 production change. Baseline `6470e1a` (INC-0/1/2 RATIFIED).

Sources:
- FOUNDATION_IMPLEMENTATION_PLAN INC-3;
- FOUNDATION_STATE_MACHINES M6, M10, M22;
- FOUNDATION_ARCHITECTURE C4.1–C4.6 and F-PAN-1;
- FOUNDATION_BLOCKERS B2.5 / B2.6 / B2.7;
- FOUNDATION_INVARIANTS INV-INPUT-1..4, INV-COORD-3/5, INV-CAMERA-2/4;
- FOUNDATION_TEST_PLAN OB-3, OB-4, OB-21, OB-24, OB-45, OB-50, TST-05;
- the harness sources `experiments/b8-harness/src/{input.rs,bin/x8.rs,bin/xzero.rs,bin/xtransform.rs}`.

**INC-3 (canonical):** `PhysicalEvent` intake through `InputPort`; the button ownership table (OBJ-25); the interaction state machine (OBJ-26, M22, CameraPan only); pointer routing per F-HIT-1; KeepFocus; free-pointer reconciliation; F-PAN-1 at zoom 1; adapter pointer delivery; headless injected input.
- **Execution class:** HEADLESS.

## 1. Coordinate domains on the pointer path (one authoritative chain)

| Step | Domain | Type | Rounding | Owner |
|---|---|---|---|---|
| Device delta Δ (accelerated / unaccelerated) | Arrangement logical px | `(f64, f64)` | R6: none | headless backend (later libinput) |
| a' = arr(O) + p + Δ, clamped to the union of output rects, then located | Arrangement → `(OutputId, OutputLogical p)` | `PointerState::Located` | half-open clamp `[0, extent − 2⁻⁸]` | `core::pointer::locate` |
| Authoritative pointer state | `(OutputId, OutputLogical p)` (D-B2-PTR-STATE, INV-COORD-5a) | `PointerState` | — | `core::pointer` (OBJ-20) |
| T5 unproject `q = c_eff + Q16(p / zoom)` | Canvas Q16 | `CanvasPos` | R1 ties-even | `core::projection::View::unproject` |
| Hit test over the output's ProjectionFrame (F-HIT-1: pixel `⌊p·scale⌋`, render order, input region) | physical px → surface-local Q16 | `Hit { element, surface, local: LocalPos }` | floor; clamp `[0, w)` | `core::render::ProjectionFrame::select` |
| Surface Canvas origin | Canvas integral | `CanvasPoint` | — (integral anchors) | `FrameEntry.origin` (F-ORIGIN-1) |
| Smithay focus origin and location | Smithay "global compositor space" := **Canvas** (D-B2-PTR-SPACE, INV-COORD-5) | `Point<f64, Logical>` | none (Q16 → f64 is exact) | `infynitae-wayland::convert::canvas_to_smithay_global` |
| Client coordinate `location − origin` | SurfaceLocal | wl_fixed | external (1/256) | Smithay |

- **One path.** `Core::route_pointer` is the only function that turns a pointer state into a client target, and it does so only through `Core::hit_test`. The adapter passes `location = origin + hit.local`, so the client receives exactly F-HIT-1's delivered coordinate. Smithay's stored location is derived per dispatch and never read back as policy.
- **No mixing.** Expected *domain* is not authority: the adapter receives only `CanvasPos` / `CanvasPoint` values inside effects, and only `convert` names Smithay geometry types (existing source rule).
- **Camera deltas under a pan.** F-PAN-1 is `dP_canvas = dp_phys / (s_O·z_C)` with `dp_phys = dp_logical·s_O`. The per-event term is summed in f64 from the gesture origin, and `C = C_start − Q16(Σ)` is quantised once per update (R2, INV-COORD-3). The pointer `(O, p)` does not move during a pan.

## 2. State ownership (no duplicate state)

| State | OBJ | Owner / sole writer | Readers | Lifetime | Invalidation |
|---|---|---|---|---|---|
| Pointer physical state `(O, p)` / `NoOutput` | OBJ-20 | `Core.pointer` — written by `physical_input`, `output_connected/reconfigured/disconnected` | adapter via effects; tests via `Core::pointer()` | Core | output lifecycle (F-RECONF-1, F-RELOCATE-1, startup rule) |
| Button ownership table `button → {Client, Compositor}` | OBJ-25 | `core::input::ownership::Ownership` inside `Core.input`, written only by `physical_input` | `physical_input` | from press to release of that button | release of the button (any state, including `NoOutput`) |
| Interaction state `Idle / CameraPan{…} / Orphaned{…}` | OBJ-26 | `core::input::interaction::Interaction` inside `Core.input`, written by `physical_input` (acquire, update, end) and `output_disconnected` (orphaning, C4.6) | `physical_input`, `output_reconfigured` (to suppress events during an interaction) | acquisition → initiator release | initiator release; cancel |
| Camera centre | OBJ-08 | `core::navigation::CameraSet::pan` (private `set_center`, consumes `Authorized`) | everyone read-only | Core | — |
| ProjectionFrame per output | OBJ-15 | `Core.frames` | `hit_test` | until the next frame | dropped on camera change of that output (new in INC-3, the same rule as reconfiguration in INC2-F07), on reconfiguration and on disconnect |
| Modifier state | (OBJ-27, Smithay XKB, INC-4) | **not stored in core** — a per-event snapshot `Modifiers` carried by `PhysicalEvent::PointerButton` (C4.4: "core receives `ModifiersState`… keeps no XKB copy") | binding resolution only | one event | — |
| Smithay pointer focus / pressed buttons / grab | OBJ-27 | Smithay `PointerHandle`, driven only by the adapter's application of core pointer effects | Smithay | seat | effects |

## 3. Requirement → module → verification

| Canonical requirement | Owner | Production module | Verification |
|---|---|---|---|
| `PhysicalEvent` variants: `PointerMotion{delta, delta_unaccel, time_us}`, `PointerButton{button, pressed, modifiers, time_us}`, `PointerAxis{…}`; intake only through `physical_input(&mut InputPort, …)` | C1.5, INV-INPUT-1 | `core::lib`, `core::entry::physical_input` | T0; foreign `InputPort` rejected; DEFERRED list updated |
| Ownership decided at press before client dispatch; releases follow the press owner; motion follows the interaction | INV-INPUT-1, C4.2 P0–P2 | `core::input::ownership` | T0; x8 C7 |
| Acquisition only when the client owns no button | INV-INPUT-2, M6, M22 | `core::input::bindings::resolve` + `entry` | x8 C6; adversarial H |
| CameraPan: motion → camera (F-PAN-1), membership pass, **no client event**; presses during an interaction are compositor-owned | INV-INPUT-3, INV-CAMERA-2, INV-CAMERA-4, M6, M22 | `core::input::interaction`, `core::navigation::CameraSet::pan` | x8 C1/C2/C7; 0 configures during pan (client-observed); anchors unchanged; **bypass negative control** (`probe_pan_motion_bypass`) |
| Sum from the gesture origin, quantised once per update | INV-COORD-3, F-PAN-1, OB-21 (T0 part) | `navigation::pan_target` | T0 1e6-event drift test vs per-event rounding (negative control); exact oracle |
| Zoom ≠ 1 coexisting with pan is gated (OB-24) | M6, OB-24 | `bindings::resolve` refuses acquisition when the output's camera zoom ≠ 1 | T0 + T1 (persisted zoom 1.5: press delivered to the client, camera unchanged) |
| Relative motion only from physical deltas on the client path | INV-INPUT-4 | `entry::physical_input` | x8 (genuine delta only); adversarial |
| KeepFocus during the interaction; on end, one true-position event (re-hit) | M10, C4.5, D-B11-FOCUS | `entry` reconciliation | x8 C1 `motion (11,283)` then `(12,283)`; ≤ 1 event per batch |
| Pointer routing through the ProjectionFrame hit test (no `surface_under`, no `Space`) | INV-HIT-1, F-HIT-1, B2.5 | `Core::route_pointer` → `Core::hit_test` | existing rejected-API rule; T1 routing tests |
| Render/hit agreement on Normal/90/180/270 (+ flipped) on the production path | OB-45 (hit-test part), EE-1 C5 | — | T1 `ee1_c5_*`: each rendered element's pixel set equals the set of pixels whose hit is that element; the sealed hit rule (canvas-exact, untransformed extent) fails as a negative control |
| Arrangement motion across outputs | B2.5 T1 | `core::pointer::locate` | T0 + T1 crossing test |
| Zero-output: last output removed → `set_grab(ZeroOutputGrab, Focus::Clear)` + `motion(None)`; NoOutput consumes motion/button/axis; first connect → `unset_grab`, startup placement, one routed motion | M10, OB-50 (pointer part), EE-3 | `entry::output_disconnected/connected`; adapter `pointer` module | T1 EE-3 cases (none / client-held button); no event in zero state; enter after reconnect; no stuck Smithay button |
| Output removal during a pan → Orphaned (motion consumed); initiator release → reconciliation on the relocated output | C4.6, M6, OB-3 (interaction orphaning) | `entry::output_disconnected` + `input::interaction` | T1 multi-output test; no stuck buttons |
| Reconfiguration of the pointer's output → clamp (F-RECONF-1), at most one true-position event (none during an interaction) | M10 | `entry::output_reconfigured` (no camera authority, 2h rule unchanged) | T1 |
| OB-4 acquisition part | M6, OB-4 | acquisition point in `entry` | **Not closable in INC-3:** `xdg_popup.grab` is not honoured before INC-4 (`XdgShellHandler::grab` is a no-op), so no popup grab can exist when a pan is acquired. The ordering point is documented; `popup_done` dismissal is INC-4. |
| Adapter pointer delivery: seat, `wl_pointer`, relative-pointer manager; effects applied in order | C4.2, C2.1 | `infynitae-wayland::pointer` (new) | source rule: `PointerHandle` calls only in that module; the adapter never constructs `Effect` values |
| Headless injected input | C1.1 | `infynitae-backend-headless::{pointer_motion, pointer_button, pointer_axis}` | T1 lockstep |

## 4. Authority (no new escape surface)
- **Camera mutation.**
  - `CameraSet::pan(&mut self, Authorized, camera, start, sum)` is `pub(crate)`; `set_center` stays private.
  - The proof is minted only in `physical_input`, an existing `MINT_ENTRIES` member.
  - `core::input` cannot mint (`mint` is `pub(in crate::entry)`), and it has no `&mut CameraSet` or `&mut Stage`: its functions return decisions; `entry` applies them.
- **Effects.** New pointer effects are values. A new source rule forbids product crates other than core from constructing `Effect` values, so the adapter and backends cannot fabricate pointer delivery. A second new rule keeps Smithay `PointerHandle` delivery calls (`motion`, `relative_motion`, `button`, `axis`, `frame`, `set_grab`, `unset_grab`) inside `infynitae-wayland/src/pointer.rs`.
- **Runtime negative control.** `probe_pan_motion_bypass` (core feature, wired through the headless crate) delivers motion and relative motion to the client during a pan. It is added to the verifier's runtime list (like 2i): the x8 test must FAIL under it.

## 5. Non-goals (explicitly not implemented)
- **Later increments:**
  - keyboard and XKB, ActiveElementFocus, and popup grabs (INC-4);
  - pointer constraints, locks, confinement and constraint reconciliation (INC-5);
  - epochs and scale (INC-6);
  - toplevel requests (INC-7);
  - frame kick and fallback (INC-8).
- **Not part of any increment yet:**
  - zoom and wheel zoom;
  - Super+1..9, Super+Enter and the other spatial bindings;
  - DRM, a real libinput backend, UI and configuration.
- **Zoom stays possible:**
  - F-PAN-1 is implemented with `z_C` read from the camera.
  - A future zoom about the pointer must keep `Q = unproject(O, p)` fixed (`centre' = Q − (p − view/2)/z'`, camera zoom, **not** output scale). The pointer is stored as `(O, p)`, so such a zoom is a camera-only change and needs no pointer change.
- **Gaming input safety:**
  - KeepFocus;
  - no motion, relative motion, button or axis to any client during a pan (adversarial tests);
  - no synthesised relative motion.
  - Constraints remain INC-5.

## 6. Judgment calls (recorded for review)
1. **Binding and modifier source.**
   - The canonical x8 binding is BTN_MIDDLE (0x112) plus a modifier; INC-3 fixes it to **BTN_MIDDLE + Logo (Super)**.
   - Core keeps no modifier state (C4.4). The backend supplies a per-event `Modifiers` snapshot with each button event. From INC-4 the adapter takes it from Smithay's `KeyboardHandle::modifier_state()`; in INC-3 the headless backend supplies it directly.
   - Rebinding and configuration are out of scope.
2. **Camera change drops the output's ProjectionFrame.** This is the INC2-F07 rule for reconfiguration, so a hit test after a pan uses the list the next render will produce, never a list from a camera that no longer exists.
3. **Zoom ≠ 1:** acquisition is refused (the press follows the client path) on the OB-24 gate.
4. **Camera bound:** a pan target outside ±2³⁶ px saturates at the bound (INV-COORD-1).
5. **Release under NoOutput.** A client-owned button released in the zero-output state is passed to Smithay while the zero-output grab is installed. The grab delivers nothing, but Smithay's pressed-button set stays truthful, so no stuck implicit grab survives a reconnect.
6. **`Authority` enum:** still unused by the physical-input pan (the proof's mint site already identifies the source). Its expectation is re-targeted to INC-11 (shell navigation).
