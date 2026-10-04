# Infynitae Native — Foundation Architecture (Phase C)

Status: **Phase C checkpoint 1 (C1–C4) complete; stopped at the checkpoint 1 review gate.** C5–C16 not started.

Inputs (frozen):
- Phase B seal `4a0807ed721ae257e1f4b0e5287d1dfd8c1038c1`, especially the carry-forward ledger at the end of [FOUNDATION_BLOCKERS.md](FOUNDATION_BLOCKERS.md).
- Decision IDs (`D-B…`), ledger categories (`CR-n` conditional obligations, `DV-n` deferred validation, `W-SM-n` Smithay quirks) and evidence IDs (`EV-`, `EB-`) refer to that document and to [FOUNDATION_EVIDENCE.md](FOUNDATION_EVIDENCE.md).

Rule of this phase: architecture **encodes** Phase B; it does not redesign it. Where C1–C4 refine a Phase B decision, the refinement is labelled **REFINEMENT** with its evidence. No Phase B decision was contradicted (see §C-Z).

New evidence in this checkpoint:

| ID | What | Where |
|---|---|---|
| EC-1 | Authority spike: two-crate workspace (domain stand-in with **zero dependencies**, protocol-adapter stand-in). 3 behaviour tests; 4 negative build probes, each **asserted to fail with its expected error code** (E0624, E0451, E0061, E0603); positive control; dependency check. **PASS** | `experiments/c1-authority-spike/` (`verify.sh`); [`evidence/phase-c/c1-authority-spike-verify.txt`](evidence/phase-c/c1-authority-spike-verify.txt) |
| EC-2 | Stable rustdoc does **not** check `compile_fail` error codes (a doctest marked `compile_fail,E0999` passed). `compile_fail` doctests alone cannot prove *why* a path is forbidden, so the probes in EC-1 replace them as the enforcement mechanism. | spike (transient edit, reverted) |
| EC-3 | SMITHAY: one XKB state per seat, updated for **every** key including intercepted ones; `pressed_keys` vs `forwarded_pressed_keys`; `set_focus` sends `enter(forwarded_pressed_keys)` + `modifiers(seat mods)`; a forwarded key carries `modifiers` only if that key changed them; two-phase `input_intercept` / `input_forward` API | `input/keyboard/mod.rs:209-297, 940-1050, 1315-1365` |
| EC-4 | PROTOCOL (wayland 1.26): `wl_keyboard.leave` "resets all values to their defaults"; `enter` sets the keys logically down; `key`: "must not send … released … [if] the key was not logically down"; `modifiers` may be sent at any time to reset | `wayland.xml` |
| EC-5 | SMITHAY: `OutputDamageTracker::render_output` takes elements **front-to-back**; `render_elements_from_surface_tree` traverses `with_surface_tree_downward` (nearest first); `Renderer::{downscale_filter, upscale_filter}` exist | `backend/renderer/damage/mod.rs:313,461`; `element/surface.rs:107`; `renderer/mod.rs:395-397` |

---

## C1 — Layering and the Cargo dependency DAG

### C1.1 Crates (Foundation 01)

Five crates. Each one exists because it enforces an authority boundary B12 requires, or isolates a dependency.

| Crate | Kind | Why it exists |
|---|---|---|
| `infynitae-core` | lib | The spatial and policy authority (Stage, cameras, identity, geometry, visibility/frame/scale/suspension policy, input ownership, focus). Must exist and be testable without Smithay or any backend (B12, AI2). |
| `infynitae-wayland` | lib | The Smithay adapter: every Wayland object, protocol handler and Smithay quirk lives here and nowhere else (B12.4). |
| `infynitae-backend-winit` | lib | Development backend: nested window, its input and its GL target (B12.1 "backend-specific components"). |
| `infynitae-backend-headless` | lib | Test backend (virtual outputs, virtual vblank, injected physical input), so tests exercise the **production** core + adapter, not a second architecture (B12.1, NB-12). |
| `infynitae` | bin | Composition root: constructs everything, distributes authority ports, runs the event loop. |

**Not created now** (no Foundation requirement yet):
- `backend-drm` (Phase G);
- shell IPC (decided in C7);
- xwayland-satellite supervisor (decided in C6);
- a persistence crate (Level D format).

C5–C10 may add a crate only with a C2-style justification.

**Rejected separations:**
- *A separate geometry/numerics crate.* Numerics are pure values with no authority difference from the rest of the domain. A split would add a crate without adding enforcement. The domain stays zero-dependency either way.
- *Splitting core into stage/camera/input crates.* The authority boundaries inside the domain (e.g. "only navigation mutates cameras") are enforced by **module privacy**, which EC-1 shows is compiler-enforced. Crate splits would not add enforcement, but would force the cross-cutting `Effect` vocabulary into a further shared crate.
- *A `Backend` trait.* Backends are concrete crates wired by the composition root. Winit and headless share the adapter's APIs, so a trait would serve only hypothetical portability (C2 rule).

### C1.2 Per-crate contract

| | `infynitae-core` | `infynitae-wayland` | `infynitae-backend-winit` / `-headless` | `infynitae` (bin) |
|---|---|---|---|---|
| **Responsibilities** | Stage and lifecycle; identity; Canvas numerics/domains; surface-tree geometry snapshots and the **single surface-origin function** (CR-1); cameras (navigation); logical outputs + arrangement; visibility/membership; MaxGesture; frame clock; frame-liveness policy; suspension reasons; projection + lattice; render list; hit-testing; input intake, bindings, ownership table, interaction state machine; pointer model; keyboard-focus model; protocol-event policy; authority ports; `Effect` vocabulary | Smithay `State`; globals at the required versions (`new_v6`); protocol handlers → `ProtocolEvent`; surface-tree **snapshot extraction**; `SurfaceKey` registry; **applying** `Effect`s (seat focus, motion, relative motion, buttons, keyboard forward/intercept, `Output::enter/leave`, preferred scales, frame callbacks, presentation feedback, configures, constraint activation); render-element construction (`from_surface`, f64); all W-SM workarounds | device events → `PhysicalEvent` (submitted with `InputPort`); output discovery → `OutputDescriptor`; vblank/flip → `FrameTick`; renderer instance, render target, submit | creates `Core` + ports; creates the adapter state, the backend and the event loop; hands `InputPort` to the backend's input source and `ShellPort` to the shell IPC source (C7); nothing else |
| **State owned** | everything spatial and policy (C2) | protocol objects (via Smithay), `SurfaceKey ↔ WlSurface` registry, damage trackers, Smithay seat/output internals | device handles, GL/EGL/winit objects, timers | nothing beyond wiring |
| **Explicitly NOT owned** | protocol objects, buffers, textures, devices, timing hardware | Stage, cameras, identity, focus policy, interaction state, membership truth, scale policy | anything spatial or policy | anything |
| **Allowed deps** | **none** (std only) | core, smithay (`wayland_frontend`, `desktop`) | core, wayland, smithay (backend features) | all crates |
| **Forbidden deps** | smithay, wayland-*, calloop, drm, input, winit, libc, any backend crate | any backend crate | the other backend crate | — |
| **Public API surface** | read accessors; `Core::new() → (Core, InputPort, ShellPort)`; `Core::{physical_input(&mut InputPort,…), shell_command(&mut ShellPort,…), protocol_event(…), frame_tick(…), render_list(…), …}` returning `Vec<Effect>`; value types | `WaylandState::{new, apply(effects), build_render_elements(…)}`; handler impls (Smithay traits) | `run(event_loop, InputPort, …)`; render/submit for the composition root | — |
| **Smithay types cross?** | **never** | inside the crate and towards backends only | yes (they are Smithay backends) | yes |
| **Wayland protocol handles cross?** | **never**: the domain sees opaque `SurfaceKey`s / `ElementId`s | inside only | no | no |
| **Backend types cross?** | never | never | inside only | wiring only |
| **Testing** | T0 unit/property tests, no runtime; negative build probes (C1.5) | T1 lockstep protocol tests (Phase B harness pattern) against the real adapter | T1/T2 via the headless backend; T2 nested (winit) | T2 smoke |

### C1.3 Dependency DAG

```
                    infynitae (bin)
                 /        |        \
                v         v         v
 backend-winit   backend-headless   (shell IPC, C7)
        \             /                 |
         v           v                  |
       infynitae-wayland  ──────────────┤
               |                        |
               v                        v
          infynitae-core  <─────────────┘
               |
            (std only)

 smithay:  wayland ─► smithay[wayland_frontend, desktop]
           backend-winit ─► smithay[backend_winit (+egl, renderer_gl)]
           backend-headless ─► smithay[renderer_test]   (test renderer; C11 may refine)
```

Every edge points towards `core`, and there are no cycles. Two edges are forbidden even though Cargo would accept them:
- `core → anything`: the domain would then be able to name Smithay or the event loop;
- `wayland → backend-*`: the adapter would acquire device or timing authority.

### C1.4 Mechanical enforcement of the DAG

| Rule | Mechanism | Strength |
|---|---|---|
| core has no dependencies | `cargo tree -p infynitae-core -e normal --depth 1` is empty (EC-1 step 3, same check) | compiler/Cargo |
| core cannot name Smithay | follows from the line above: the names do not resolve | compiler |
| wayland ↛ backend, backend ↛ backend | `cargo tree -i` checks in `verify-architecture` (same script pattern as EC-1) | Cargo |
| locked dependency graph | `cargo build --locked` in CI; Cargo.lock committed (D-B12-PIN) | Cargo |
| no `unsafe` in core; `unsafe` in wayland only at listed sites | `#![forbid(unsafe_code)]` in core; `#![deny(unsafe_code)]` + per-site `allow` with a safety note in wayland (the calloop `Generic` dispatch idiom) | compiler; C9 completes the policy |

### C1.5 Camera mutation authority (CR-5 / NB-10)

**Required property:** *code handling an untrusted Wayland client request must not possess direct authority to mutate Camera position.*

Constraint from Smithay: every protocol handler is a trait method on the global state and receives `&mut State`. Any design that puts mutable camera access *anywhere reachable from `State`* fails, however disciplined the handlers are.

Design (demonstrated by EC-1):

1. **The `navigation` module is the only mutator.** The camera type's mutating methods are private to `core::navigation` (Rust privacy). Other core modules, including `core::protocol`, which implements policy for client requests, cannot call them. Probe: `error[E0624]: method set_center is private`.
2. **Authority ports.** `Core::new()` returns `(Core, InputPort, ShellPort)`.
   - Ports have private fields (cannot be forged: E0451), are not `Clone`, and carry the minting Core's instance id (a foreign port is rejected at run time: test `ports_are_bound_to_their_core`).
   - The only public entry points that reach `navigation` are `Core::physical_input(&mut InputPort, …)` and `Core::shell_command(&mut ShellPort, …)`. Without a port they do not type-check (E0061).
3. **Ports never live in the protocol state.** The composition root moves `InputPort` into the backend's input-source closure and `ShellPort` into the shell-IPC source closure. `State`, which every Smithay handler receives, contains `Core` but no port. Protocol handlers therefore can read cameras (positive control compiles) but cannot move them.
4. **Protocol policy has no camera vocabulary.** `ProtocolEvent::ActivationRequest` is handled in `core::protocol`. Its outcomes are focus, attention or ignore (D-B10-ACTIVATION). The navigation internals (`Authority`, `pan`, `jump`) are not visible outside core (E0603).

What is compile-time, what is not:

| Path | Enforcement |
|---|---|
| protocol handler → mutable camera | **compile-time** (no mutable accessor; private mutators) |
| protocol handler → forge a port | **compile-time** (private field) |
| protocol handler → call trusted entry points without a port | **compile-time** (signature) |
| core-internal protocol policy → camera mutation | **compile-time** (module privacy) |
| protocol handler → mint a *second* Core to obtain ports | ports are bound to their Core: **run-time** rejection (tested) |
| a future change that stores a port inside `State` | **not** compiler-preventable in Rust (core's port types are public so the composition root can hold them). Mitigation: (i) the adapter crate's public API never takes a port; (ii) `verify-architecture` fails if the adapter crate's sources name `InputPort` or `ShellPort` (textual check, mechanical but not type-level). Recorded as C-NB-2. |
| `unsafe` circumvention | excluded by C1.4 (`forbid(unsafe_code)` in core; audited sites only in the adapter) |

**How an xdg-activation request is processed:**
1. Smithay calls `XdgActivationHandler::request_activation(&mut State, token, data, surface)`.
2. The adapter validates the token per W-SM-8 (provenance override) and maps the surface to `ElementId` via its registry.
3. It calls `state.core.protocol_event(ActivationRequest { element })`.
4. Core returns `Effect::FocusGrant(..)` or `Effect::Attention(..)` or nothing.
5. The shell may later, **with `ShellPort`**, issue `NavigateTo`. That runs in the shell-IPC source closure, not in a Wayland handler.

---

## C2 — Complexity budget and authority matrix

### C2.1 Every significant structure

A = authoritative; D = derived; P = protocol state (Smithay-owned). "Guard" says why a derived value cannot become a second authority.

| Structure | Crate | Exists because | A/D/P | Single writer | Invalidation / lifetime | Guard |
|---|---|---|---|---|---|---|
| `Stage` (element records, anchors, z-order, lifecycle state) | core | I1; B2–B5 | **A** | core (`stage` module; placement only via interaction/placement policy) | element lifetime (B5) | sole store of anchors; no other structure has a write path |
| `ElementId` minting | core | D-B4 | **A** | `Stage` | never reused | no public constructor; external ids become `ElementRef` → `resolve` |
| `SurfaceTree` snapshot per element (surfaces with kind, parent, RootSurfaceLocal offset/size, input region, window-geometry offset, popup configured position) | core (type) / wayland (writer) | the domain must compute origins, membership, hit-tests and render lists without Smithay (CR-1, B12). Re-querying Smithay is impossible in a zero-dependency crate. | **D** (from protocol state) | adapter, on each committed surface-tree state change, synchronously in the commit handler | replaced wholesale per commit; dropped on unmap/destroy | holds no anchor; core never writes it; it is client state by definition |
| `SurfaceKey ↔ WlSurface` registry | wayland | domain must name surfaces without handles | **A** for the association | adapter (role creation / destroy) | surface lifetime | keys are opaque; core cannot dereference them |
| Surface-origin function `origin(element, surface)` | core | CR-1 / NB-2: **one** function for render, membership, hit-test, input | **D** (pure) | — | — | the only code path that adds offsets; adapter and renderer receive results, never recompute |
| `CameraSet` (centre Q16, zoom, binding) | core::navigation | B7 | **A** | `navigation` only (C1.5) | survives outputs (D-B7) | privacy + ports |
| `OutputModel` (logical outputs: id, key, mode choice, scale, transform, refresh, arrangement position) | core | B7, B2.1 Arrangement | **A** for logical configuration | core output manager (from backend descriptors + configuration) | output lifetime (B7.2) | Smithay `Output` state is written **from** it one-way |
| Smithay `Output` mode/scale/transform | wayland | protocol face of an output | **P/D** | adapter, from `OutputModel` | — | never read back as policy input |
| Per-output `View` (v_O, k, R, C_O, viewport) | core | B1.5 per-output projection constants | **D** | recomputed when camera/output changes | per change | pure function of `CameraSet` + `OutputModel` |
| `MembershipTable` ((element, surface) → set of outputs) | core | enter/leave are **diffs** (EV-W03), so the previous state is needed. Also the input to MaxGesture and the clock. | **D** | visibility pass only | rewritten each pass | the only producer of enter/leave `Effect`s; consistency with Smithay's entered-sets is asserted in debug/T1 |
| Smithay `Output` entered-surface sets | wayland (Smithay) | duplicate-suppression of `wl_surface.enter` | **P** | adapter applying `Effect::Enter/Leave` only | protocol | never consulted for policy |
| `ScaleState` per element (current preferred, deferred decrease) | core | MaxGesture is history-dependent (D-B8-SCALE) | **A** (policy state) | visibility pass + gesture end | element lifetime | Smithay per-surface "last sent" values are send-dedupe only |
| Frame clock per element | core | D-B8-CLOCK | **D** | visibility pass | per pass | pure function of membership + refresh |
| `FrameLiveness` per element (last completion time, pending-kick flags) | core | D-B9-FRAMES needs the time since the last fallback completion | **A** (policy state) | frame/kick logic | element lifetime | Smithay's throttle state is not used for policy; the adapter completes callbacks only on `Effect::CompleteFrames` |
| `SuspensionReasons` per element | core | D-B9-SUSPEND | **A** | semantic-state transitions only | element lifetime | no camera/visibility code path writes it |
| `ProjectionFrame` per output (last rendered list: element order, effective camera, surface rects) | core | **D-B2-I9**: hit-testing must use the list the user saw | **D (cache)** | the render-list builder when a frame is produced | replaced each frame for that output | read-only to hit-testing; never written back to Stage; if absent (no frame yet), hit-testing builds one on demand |
| `PointerModel` (`Free(OutputId, p)` / `Locked(SurfaceKey, q_lock)`) | core | D-B2-PTR-STATE + REFINEMENT B10.2 | **A** | input intake | seat lifetime | Smithay pointer location/focus are written per dispatch from it |
| Smithay pointer location + focus | wayland | protocol delivery | **P (applied)** | adapter applying `Effect::Pointer*` | — | not read for policy |
| Ownership table (buttons, keys → owner) | core | D-B10-INTAKE; INV-2 | **A** | intake | per press…release | the Smithay seat never sees compositor-owned presses (X-8 C7) |
| `Interaction` state machine | core | B10.4 | **A** | intake | per interaction | — |
| Keyboard `FocusModel` (target: element / popup / none; attention set) | core | D-B10-KBD | **A** | focus model only (explicit actions, target loss, activation grant) | — | Smithay keyboard focus is written from it; camera/visibility code has no write path |
| Smithay keyboard focus | wayland | protocol delivery | **P (applied)** | adapter | — | not read for policy |
| XKB state (keymap, modifiers, locks, pressed keys) | Smithay `KeyboardHandle` | one state machine per seat; duplicating it would create two XKB truths (C4.4) | **P (authoritative for XKB)** | Smithay, on every key | seat lifetime | core reads modifiers from the filter callback; keeps **no** XKB copy |
| Constraint objects | wayland (Smithay) | protocol objects | **P** | Smithay; activation and deactivation only via adapter `Effect`s | protocol | activation policy lives in core |
| `Effect` list | core → wayland | the one channel from decisions to protocol actions | transient | core | per call | not stored |
| Render elements, damage trackers | wayland / backend | Smithay rendering | **D (transient / renderer state)** | adapter per frame | per frame | built from the core render list only |

### C2.2 Negative ownership

| State | OWNER | Explicitly NOT owners |
|---|---|---|
| Stage placement (anchors) | core `Stage` | cameras, outputs, `MembershipTable`, `ProjectionFrame`, render elements, Smithay `Window`/`Space`, adapter, backend, shell, XWayland, clients |
| Element identity | core `Stage` | `SurfaceKey` registry, `WlSurface`/`ObjectId`, Smithay `Window`, title/app_id/PID, shell, persistence |
| Camera state | core `navigation` | Output, `OutputModel`, renderer, adapter, protocol policy (`core::protocol`), shell (requests only, via `ShellPort`), clients |
| Output visibility / membership | core `MembershipTable` (derived) | Smithay `Output` entered-sets (dedupe only), `Window::output_enter` (unused, W-SM-2), renderer, clients |
| Pointer focus | core `PointerModel` + intake | Smithay seat (applied copy), hit-test cache, clients |
| Keyboard focus | core `FocusModel` | Smithay keyboard (applied copy), cameras, visibility, pointer focus |
| Protocol membership / protocol state (roles, configures, constraints, frame callback queues) | Smithay via adapter | core (it holds snapshots and decisions only), Stage |
| Render geometry (physical edges, f64 locations) | **nobody**: computed per frame from Stage + View | renderer, Smithay elements, damage tracker, `ProjectionFrame` (a read-only record of what was shown) |
| Cached projection results | `ProjectionFrame` (derived, read-only) | Stage, cameras, hit-test writers |

### C2.3 Rejected or deferred abstractions

| Abstraction | Decision | Reason |
|---|---|---|
| Smithay `Space` | rejected | W-SM-1 / D-B2-SPACE |
| R-tree, grid, scene graph, spatial database | **not introduced** | C3.8 threshold: none is met by any Foundation requirement |
| `Backend` trait | not introduced | no Foundation requirement (C1.1) |
| Renderer abstraction of our own | not introduced | Smithay's `Renderer` trait already parameterises the adapter |
| XKB wrapper in core | rejected | would duplicate XKB authority (C4.4) |
| Separate numerics crate | rejected | C1.1 |

---

## C3 — Render pipeline and lattice integration

Rendering consumes authoritative state and never writes it. Every arithmetic step below is a Phase B result (B1.4, B1.5, B2.2 T4–T6, B8.3–B8.5, B9); this section fixes **where** each step lives.

### C3.1 Pipeline

```
 core (pure)                                                          wayland (adapter)            backend
 ───────────                                                          ─────────────────            ───────
 Stage (anchors, z-order)  +  SurfaceTree snapshots  +  CameraSet  +  OutputModel
        │
 [1] View(O)            v_O, k_O, R_O, C_O, c_eff,O          (per output, on change)
        │
 [2] surface Canvas rects via origin()                       (single origin function, CR-1)
        │
 [3] visibility: projected physical rect ∩ [0,mode_O)  ──►  MembershipTable diff ──► Effect::Enter/Leave ──► Output::enter/leave
        │                                                     MaxGesture / clock / frame / suspension ──► Effects ──► scale, callbacks, configures
 [4] render list(O): front-to-back items
        (SurfaceKey, L_f64 = (x−R)·k + δ − C, alpha)  ─────► [5] SurfaceKey→WlSurface;
        │                                                     WaylandSurfaceRenderElement::
        └─► ProjectionFrame(O) (read-only, for I9)              from_surface(renderer, s, states,
                                                                Point<f64,Physical>(L), α, kind)
                                                                     │
                                                              [6] OutputDamageTracker(O)  ──────►  renderer (filters) ─► submit
                                                                                                         │
 [7] FrameTick(O, time, seq) ◄────────────────────────────────────────────────────────────────── vblank/flip
        │
     Effect::CompleteFrames(elements clocked by O) ─► send_frames … ; Effect::Present(O) ─► take_presentation_feedback(...)
```

### C3.2 Step definitions (exact)

| Step | Definition | Numeric domain | Rounding |
|---|---|---|---|
| [1] view origin | `v_O = camera.center − (mode_O / (2·k_O))` with `k_O = zoom · scale_O` | Q16 | **R7 (REFINEMENT of B1.4):** the per-output view origin is quantized to Q16 (round-ties-even). B1.4 listed R1–R6; deriving `v_O` from a centre (D-B7-CENTER) needs this one added quantization, of the same kind and bound as R1 (≤ 2⁻¹⁷ px). |
| [1] lattice | `R_O = floor(v_O / 120) · 120` (integer Canvas), `C_O = round((v_O − R_O)·k_O + δ)`, `δ = 2⁻²⁰`, `c_eff,O = R_O + Q16(C_O / k_O)` | `v_O − R_O` exact in Q16, then f64 (exact, small); `C_O` integer | R4/R5 (B1.4) |
| [2] origins | `root_origin = anchor − geometry.loc`; subsurface: `+ Σ offsets`; popup: `root_origin + geometry.loc + popup_pos − popup.geometry.loc` (B2.3, B6.1). Every surface origin is **integral** Canvas. | integral i64 | none |
| [3] visibility | surface physical rect `[edge(x₀), edge(x₁)) × [edge(y₀), edge(y₁))` intersects `[0, mode_O)`. Element visible on O ⇔ any surface visible. **Membership is defined in physical pixels of O, the same function as rendering**, so "entered" means "some pixel of it is scanned out on O" (EV-W03 wording). | i64 → f64 → i32 | R4 |
| [4] render location | `L(x) = (x − R_O)·k_O + δ − C_O` per surface origin x (and y), as f64 | f64, magnitude bounded by the viewport for anything not culled | **this is the only Canvas → Smithay f64 conversion for rendering** (adapter receives `L` ready-made) |
| [5] element | `from_surface` with `Point<f64, Physical>(L)`. Smithay rounds edges as `round(L)` and `round(L + dst·k)`, which equals `edge(·)` exactly (X-6: 0 / 134,478 mismatches, root and subsurfaces). | Smithay | Smithay edge rounding (= R4) |
| [6] damage / submit | per output `OutputDamageTracker`, elements **front-to-back** (EC-5) | — | — |
| [7] frames / presentation | the clock output's tick completes callbacks for elements clocked by O; presentation feedback taken for those elements on O; unobserved elements per D-B9-FRAMES (fallback timer is a compositor timer, NB-11) | — | — |

**Forbidden here (from B1.4):**
- F1: writing any of [1]–[7] back into Stage, cameras or snapshots;
- F4: f32 anywhere, or i32 before the camera is subtracted;
- F5: absolute f64 Canvas edges;
- F6: rounding origin and size separately;
- using `render_elements_from_surface_tree` with an i32 root (W-SM-5).

### C3.3 Order

Elements: reverse Stage z-order (front first). Within an element, the order reproduces Smithay's own `Window` path:
1. each popup tree in `PopupManager::popups_for_surface` order, using the same offset formula (`desktop/space/wayland/window.rs:106-121`);
2. then the root tree, traversed by `with_surface_tree_downward` (nearest first, as `render_elements_from_surface_tree` does).

The adapter supplies the traversal order in the snapshot. Core only concatenates. **Obligation X-15 (new):** an equivalence test shows that, for a tree with popups and above/below subsurfaces, the render list's surface order equals the order Smithay's `Window::render_elements` would produce.

### C3.4 Logical vs physical, fractional scale, multi-output

- Canvas is logical (B1). OutputLogical exists only for pointer intake (T5). Rendering goes Canvas → OutputPhysical directly via [1]–[4]. No intermediate OutputLogical rounding.
- Fractional scale enters only as `k_O` in [1]–[4]. `wl_output.scale` (integer, rounded up by Smithay) is the protocol face, not used for rendering.
- **Multi-output:** each output has its own View, render list, damage tracker and ProjectionFrame. The same surface appears as separate render elements on each output. Smithay element ids per surface are fine because damage tracking is per output.
- **Subsurfaces:** treated exactly like the root (integral origin, same `L` formula). Smithay's internal subsurface placement is not used for position (the adapter passes each surface's own `L`).
- **Popups:** same. Popups are never repositioned by camera changes (D-B6-NO-PAN-REPOSITION).

### C3.5 MaxGesture consequences and the downsampling boundary

- The client renders at `max(scale)` over observing outputs (D-B8-SCALE). On a lower-scale output, `from_surface` maps the larger buffer into a destination of size `dst·k_O`, so the GPU **minifies**. The downsampling boundary is therefore the renderer's texture sampling, controlled by `Renderer::downscale_filter` (EC-5).
- **Provisional:** set `downscale_filter(Linear)` and `upscale_filter(Linear)` explicitly at renderer creation rather than inheriting a default. Visual quality is **DV-2** (deferred visual validation). Changing the filter does not change any geometry.
- Buffer memory cost while multi-observed: ×(s_max/s)² (2.25× for 1.5/1.0, RB-7).

### C3.6 Where f64 enters Smithay (complete list)

| Conversion | Function (adapter) | Source |
|---|---|---|
| render location | `projected_to_smithay_physical(L) -> Point<f64, Physical>` | core render list |
| pointer location | `canvas_to_smithay_global(q: CanvasPos) -> Point<f64, Logical>` (exact by D-B1-3) | core PointerModel / intake |
| surface focus origin | `canvas_point_to_smithay_global(CanvasPoint) -> Point<f64, Logical>` | origin() |
| membership overlap (if ever passed to Smithay) | not used: `Output::enter/leave` take no rectangles (W-SM-2) | — |
| popup constraint target | `canvas_rect_to_parent_geometry_local(..) -> Rectangle<i32, Logical>` (integral) | B6.2 |

These are the **only** places Smithay geometry types are constructed from Infynitae values. They live in one adapter module (`convert.rs`) and are the test surface for CR-6.

### C3.7 I9 and the projection cache

Hit-testing on O uses `ProjectionFrame(O)`: the element order, effective camera `c_eff,O` and surface rects of the last frame shown on O (D-B2-I9).
- Between frames, Stage or camera changes are reflected in hit-testing from the next frame on.
- What the user sees is what they hit.
- The bound from D-B2-I9 (≤ ½ physical px at fractional `k`; exact at integral `k`) is unchanged.
- **CR-6** (fractional-camera `c_eff` path): T0 property tests over random `(center, k ∈ n/120, zoom = 1)`:
  - `edge(x)` from [1]–[4] equals `round(L)`;
  - unprojecting a pixel centre through `c_eff,O` and re-projecting lands in the same pixel;
  - both hold at magnitudes up to 2³⁶.

### C3.8 No spatial index (and when one would be justified)

Per frame the work is O(E_mapped × S_avg × N_outputs) integer/f64 operations for [2]–[4], plus O(visible) element construction. A spatial index (grid/R-tree) would be justified only by **measurement**, if all three hold:
1. the visibility + render-list build exceeds **10% of the shortest frame budget** of any connected output (165 Hz → > 0.6 ms) on the target machine;
2. this happens at an element count the product actually reaches (to be measured in T4; no target count is assumed);
3. a profile attributes the cost to culling, not to element construction or GPU submit.

If introduced, the index is **derived** from Stage (rebuilt or updated from Stage mutations only, never written elsewhere) and must pass a differential test against the linear scan.

---

## C4 — Focus, input routing and ownership

### C4.1 The eight concepts mapped to code (B10.2, now as boundaries)

| # | Concept | Lives in | Interface |
|---|---|---|---|
| 1 | physical input | backend crate | produces `core::input::PhysicalEvent`; submits with `InputPort` |
| 2 | gesture recognition | `core::input::bindings` (data-driven) | pure: `(event, modifiers, bindings) → Option<Action>` |
| 3 | interaction state + ownership table | `core::input::{interaction, ownership}` | authoritative state (C2.1) |
| 4 | Stage hit-test | `core::projection::hit_test(ProjectionFrame, OutputId, p)` | pure, returns `(ElementId, SurfaceKey, surface origin)` |
| 5 | seat focus | Smithay seat, written by adapter from `Effect::PointerFocus` / `Effect::KeyboardFocus` | applied copy |
| 6 | absolute delivery | adapter: `PointerHandle::motion(focus=(WlSurface, origin), location=Canvas)` | from `Effect::PointerMotion` |
| 7 | relative delivery | adapter: `PointerHandle::relative_motion(Δ physical)` | from `Effect::RelativeMotion` |
| 8 | constraints / lock | core `PointerModel` + activation policy; adapter executes `Effect::{ActivateConstraint, DeactivateConstraint}` with the W-SM-4a ordering rule | — |

Seven of the eight have exactly one owner. The exception is concept 8: a constraint's protocol object is owned by Smithay, while its activation policy is owned by core.

### C4.2 Pointer path

```
backend: PhysicalEvent (Δ accel/unaccel | absolute | button)
  └─► core.physical_input(&mut InputPort, ev)
        P0 normalize (finite)
        P1 ownership continuation (release → owner; motion → active interaction)
        P2 binding resolution before any client dispatch; acquisition precondition: no client-owned button
        P3/P4 client routing: hit_test(ProjectionFrame) → PointerModel update → Effects
  ◄── Vec<Effect>: PointerFocus/PointerMotion/RelativeMotion/Button/Frame | CameraChanged | Enter/Leave | …
adapter: apply(effects) to the Smithay seat, outputs, surfaces
```

The adapter **never** calls the Smithay pointer except to apply effects. A Smithay grab (implicit click grab, popup grab, DnD) is a client-owned sequence. Core records it in the ownership table (owner = client); Smithay's grab object then chooses the focus surface for that sequence (P3).

### C4.3 Pointer constraints and relative motion (unchanged from B10/B11; placement only)

- **Activation policy** (core):
  - activate when the constrained surface has pointer focus and the pointer is inside the constraint region;
  - never during a CameraPan acquisition;
  - deactivate at reconciliation when the lock point is unobservable (B11.4), on focus loss (performed by Smithay), on unmap/destroy, and on session lock.
- **W-SM-4a** (adapter): explicit `deactivate` only when no pointer-focus change is in the same effect batch. Core marks the batch; the adapter enforces the order.
- **W-SM-4b** stays catalogued (X-12 reproducer). There is no intervention in Foundation 01's architecture. The adapter is the only place a future patch would touch.
- **Relative motion** is emitted only from physical deltas and only on the client routing path (INV-1/4/5, X-8).

### C4.4 Keyboard and modifier ownership (CR-2)

**Who owns XKB.** Smithay's `KeyboardHandle` holds the seat's single XKB state (keymap, depressed/latched/locked modifiers, group, LEDs). It is updated for every key event, including intercepted ones (EC-3). Core keeps **no** XKB copy. Core receives `ModifiersState` and the keysym in Smithay's filter callback.

**Protocol facts the rules follow** (EC-4):
- a client's logical keyboard state is set by `enter(keys)` + `modifiers`, changed by `key` and `modifiers`, and **reset to defaults by `leave`**;
- a `released` event for a key that is not logically down (or `pressed` for one already down) **must not** be sent.

**Rules (DECISION D-C4-KBD; REFINEMENT of D-B10-INTAKE for keys):**

| # | Rule | Grounding |
|---|---|---|
| K1 | Ownership is decided at **press** using Smithay's two-phase API. `input_intercept` updates XKB and runs the binding filter, which returns core's decision; then either the press is consumed (compositor-owned) or `input_forward` sends it (client-owned, recorded by Smithay in `forwarded_pressed_keys`). | EC-3 |
| K2 | A **release** follows its press's owner. Compositor-owned → consumed (never forwarded, which also satisfies the protocol's must-not). Client-owned → forwarded to whichever surface has keyboard focus **now**. That is protocol-correct: the key is logically down there, via an earlier `key` or via `enter(keys)` (K4). | EC-4, EC-3 |
| K3 | **Keys that change XKB modifier or lock state are never compositor-owned.** If `input_intercept` reports `mods_changed`, the event is forwarded even when a binding matches; the binding acts in **observe mode** (it fires, the key is still delivered). Reason: `enter` sends `modifiers` computed from the *whole* seat XKB state. A compositor-owned modifier would be announced to a newly focused client whose release could never legally be delivered: a **stuck modifier**. Consequences: a binding's *non-modifier* key is compositor-owned; its modifiers are client-visible (clients see e.g. Super down/up around a compositor binding, which is ordinary behaviour); a "tap a modifier" binding is evaluated on release without owning the key. | EC-3, EC-4 |
| K4 | **Focus change while keys are held.** Smithay sends `leave` to the old surface (its logical state resets to defaults, so no synthetic releases are needed or sent) and `enter(forwarded_pressed_keys)` + `modifiers` to the new one. Because of K1–K3, `forwarded_pressed_keys` is exactly the set of client-owned keys and the seat modifiers are exactly those produced by client-owned keys plus locks, so the new client's logical state is true. | EC-3, EC-4 |
| K5 | **Binding activates while modifiers are already held:** those modifiers were forwarded (K3), so clients already know them. The binding's non-modifier key is consumed with its release (K2). | K2, K3 |
| K6 | **Interaction cancellation** (pan/move/resize cancelled, session lock): compositor-owned keys and buttons stay compositor-owned until physically released; their releases are consumed. No synthetic events. | B10.4 |
| K7 | **Window destruction / unmap of the focused surface:** the focus model chooses a new target (C4.5). Smithay sends `enter(forwarded keys)` + `modifiers` to it (no `leave` to a dead surface). Key ownership is unchanged. | EC-3 |
| K8 | **Output removal** has no keyboard effect. Keyboard focus is output-independent (D-B10-KBD). | — |
| K9 | Key **repeat** is client-side for Foundation 01 (`repeat_info`). `leave` stops it per client semantics. Compositor-side repeat (v10 `repeated`) is not used. | spec |

**Required tests (X-10, T1, probe with a recording `wl_keyboard`, plus a protocol checker that flags every must-not violation):**
1. compositor binding: press and release both consumed; the client sees neither;
2. binding while a modifier is held: client sees modifier down/up and nothing else;
3. attempt to bind a modifier key: forced forward (observe mode); no client ever ends holding a modifier after release;
4. focus change while a client-owned key is held: `leave` to old; `enter` with the key + correct `modifiers` to new; the release reaches the new focus;
5. focus change while a compositor-owned key is held: that key is absent from `enter`; its release is consumed;
6. focused surface destroyed with keys held: the next focus gets a correct `enter`;
7. a binding whose action changes focus (e.g. focus-next) while its keys are held: as in 4/5;
8. input delivery to a focused, unobserved, callback-blocked client triggers the kick (CR-7).

The checker asserts no `key` event violates the protocol's must-not rules and that every client's logical state equals the physically held client-owned keys at every focus change.

### C4.5 Focus model (keyboard and pointer)

| Concept | State | Changes only by | Never changed by |
|---|---|---|---|
| keyboard focus | `FocusModel.target` | click-to-focus (an explicit user action routed by intake); focus-navigation bindings; shell focus command (`ShellPort`); activation **grant** (D-B10-ACTIVATION); target loss → successor policy | camera position or motion, visibility, membership, pointer motion (no focus-follows-mouse in Foundation 01) |
| pointer focus | derived per routed event from `ProjectionFrame` + `PointerModel` | pointer motion (client path), reconciliation, Smithay grabs for client-owned sequences | camera motion **during** CameraPan (KeepFocus, D-B11-FOCUS) |
| attention | `FocusModel.attention` set | activation requests (when focus is not granted) | camera |
| successor on target loss | most recently focused live element (focus history in `FocusModel`) | — | visibility (an unobserved successor is allowed, D-B10-KBD) |

The focus model's outputs are `Effect::KeyboardFocus(Option<target>)` and `Effect::Attention(..)`. Core contains no Smithay call.

### C4.6 Interactions and output removal (CR-3), popup grabs (CR-4)

- **Output removal during an interaction:** the interaction keeps ownership of its buttons and keys. Motion is still consumed. At termination, reconciliation places the pointer on a remaining output: the Arrangement-nearest point (core, pure function of `OutputModel`). If no output remains, the pointer stays `Free` with no output, and delivery is suspended until an output appears. Test X-11a (T1, headless backend can remove outputs).
- **Popup-grab dismissal on acquisition:** core emits `Effect::DismissPopupGrab(element)` before `CameraChanged`. The adapter dismisses in protocol order (`popup_done`, topmost first), checking W-SM-7's precondition. Test X-11b.

---

## C-A. Assumptions introduced in C1–C4

| ID | Assumption | Class |
|---|---|---|
| A-C1 | The adapter can extract a complete `SurfaceTree` snapshot (offsets, sizes, input regions, popup positions, traversal order) through public Smithay API at commit time | ASSUMPTION, low risk: the inputs used in Phase B were all public (`view()`, `SubsurfaceCachedState`, `PopupManager`, geometry) |
| A-C2 | Two-phase `input_intercept`/`input_forward` is sufficient to implement K1–K3 inside one dispatch | SMITHAY API exists (EC-3); behaviour to be verified by X-10 |
| A-C3 | A single-threaded calloop design lets the composition root keep ports outside `State` (closures own them) | DECISION; standard calloop pattern |

## C-B. New blockers / findings (C1–C4)

| ID | Finding | Level | Disposition |
|---|---|---|---|
| C-NB-1 | `compile_fail` doctests do not check error codes on stable (EC-2) | B (verification method) | **RESOLVED**: negative probes with asserted error codes (EC-1) |
| C-NB-2 | A port could be stored in `State` by a future change; Rust cannot forbid it at the type level while the composition root must hold ports | B | **CONDITIONALLY RESOLVED**: adapter API never takes ports + `verify-architecture` textual check + review rule; recorded residual risk |
| C-NB-3 | Render order must equal Smithay's `Window` order (popups, then the root tree; downward traversal) | B | **CONDITIONALLY RESOLVED**: X-15 equivalence test |
| C-NB-4 | Stuck-modifier path: Smithay's `enter` announces seat-wide modifiers, including those from intercepted keys (EC-3) | **A** (input authority, CR-2) | **RESOLVED by design** (K3); X-10 validates |
| C-NB-5 | B1.4's rounding table needs R7 (view origin from camera centre) | B | **RESOLVED** (REFINEMENT recorded in C3.2) |

## C-V. Verification for checkpoint 1

| Check | Status |
|---|---|
| Camera authority is compile-time unreachable from protocol code (4 probes, error codes asserted) | **DONE** in the spike (EC-1); to be ported as `verify-architecture` against the real crates in Phase F |
| Domain has zero dependencies (Stage exists without Smithay/backend) | **DONE** in the spike; same check in Phase F |
| Camera operations cannot mutate Stage (I2) | **DONE** in the spike (`trusted_input_moves_camera_but_not_stage`); T0 property test in Phase F |
| Activation cannot move cameras | **DONE** in the spike (`activation_never_moves_cameras`) |
| DAG edges (`wayland ↛ backend`, `backend ↛ backend`) | **SPECIFIED** (`cargo tree -i` checks); runs once the real crates exist |
| Coordinate conversion tests (C3.6 functions; CR-6 properties) | **SPECIFIED**: T0 |
| Render-order equivalence | **SPECIFIED**: X-15 |
| Keyboard/modifier ownership | **SPECIFIED**: X-10 (8 cases + protocol checker) |
| Output removal / popup-grab dismissal | **SPECIFIED**: X-11a/b |
| Single origin function agreement | **SPECIFIED**: X-13 |

## C-Z. Consistency with Phase B (contradiction check)

| Phase B decision | C1–C4 encoding | Status |
|---|---|---|
| I1 Stage single authority | `Stage` is the only anchor store (C2.1, C2.2) | consistent |
| Cameras are views | `navigation` is the only mutator; no Stage write path (spike test) | consistent |
| Visibility derived | `MembershipTable` derived; Smithay sets are dedupe only | consistent |
| Protocol state ≠ Stage geometry | snapshots are D; anchors are A | consistent |
| `Space` not used | C2.3 | consistent |
| Window-geometry anchor | origin() (C3.2 [2]) | consistent |
| Q16 Canvas, lattice rule | C3.2 (R7 added, a refinement of the same kind as R1) | consistent, **refined** |
| One owner per input event | C4.2 P0–P4 | consistent |
| Pan consumes owned motion | C4.2/C4.3 | consistent |
| Camera ↛ keyboard focus | C4.5 | consistent |
| Clients ↛ cameras | C1.5 | consistent, now mechanically demonstrated |
| Visibility ↛ `suspended` | `SuspensionReasons` has no visibility writer (C2.1) | consistent |
| Frame liveness (withhold/kick/fallback) | C3.2 [7]; fallback is a compositor timer | consistent |
| MaxGesture | `ScaleState` (C2.1); membership defined in physical pixels (C3.2 [3]) | consistent. Membership now uses the render projection, which is **stricter than** but not in conflict with the overlap rule used in the B8 harness (that used integer logical overlap at zoom 1 with integral cameras; for k ≥ 1 a positive integral logical overlap always maps to ≥ 1 physical pixel and zero overlap to zero, so the two coincide there. For k < 1, i.e. future zoom-out, physical membership is the defining rule). |
| D-B10-INTAKE (keys) | K1–K3 | **refined**: modifier/lock keys cannot be compositor-owned (C-NB-4) |

**No Phase B decision is contradicted.** Two refinements (R7, K3) are recorded with evidence.
