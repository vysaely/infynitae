# Infynitae Native — Foundation Architecture (Phase C)

Status: checkpoint 1 (C1–C4) accepted, commit `d596416cf57877116e9cfbc8cf6c6fe25c001c67`. **Checkpoint 2 (C5–C10) complete; stopped at the checkpoint 2 review gate.** C11–C16 not started.

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

---
---

# Phase C checkpoint 2 (C5–C10): accepted, commit `fc2dd68f843b1eb05828f4116f94e8bab4f478da`

Execution order as mandated: X-10 → snapshot boundary → C5 → C6 → C7 → C8 → C9 → C10 → contradiction pass.

## CP2 evidence

| ID | What | Where |
|---|---|---|
| EC-6 | **X-10** keyboard/modifier ownership on Smithay 0.7.0's real seat keyboard, with a client-side `wl_keyboard` protocol checker. 9 scenarios **PASS**; negative control (K3 disabled) **FAILS as predicted** (client left with `depressed=0x1` after the physical release) | `experiments/b8-harness/src/{keys.rs,bin/x10.rs}`; [`evidence/phase-c/x10-keyboard-output.txt`](evidence/phase-c/x10-keyboard-output.txt) |
| EC-7 | wayland-backend 0.3.17 does not send an event whose object argument is a destroyed object (`get_object(..)?` → `InvalidId`, `rs/server_impl/client.rs:189-196`). Therefore no `wl_keyboard.leave` can name a surface the client destroyed. The spec (wayland 1.26) does not define the logical active surface after client-side destruction. | source + X-10 K-6 |
| EC-8 | **Surface-tree snapshot via public API only** (external crate compiles): root, subsurfaces (incl. negative-offset, placed *below*), input regions, window geometry, popup with its own subsurface. Equals client ground truth; **extracted inside `CompositorHandler::commit` without deadlock** | `src/snapshot.rs`, `bin/xsnap.rs`; [`evidence/phase-c/xsnap-x15-output.txt`](evidence/phase-c/xsnap-x15-output.txt) |
| EC-9 | **X-15 PASS**: snapshot front-to-back order **and** positions equal Smithay's `Window::render_elements` exactly | same |
| EC-10 | **W-SM-11 (new)**: Smithay tests input regions with `point.to_i32_round()` (round-to-nearest) but surface bounds with exact containment, shifting every input region by −½ px (`desktop/wayland/utils.rs:25-50`) | source + xsnap (1,448 grid mismatches, all explained) |
| EC-11 | **W-SM-12 (new)**: `under_from_surface_tree` overwrites its result for every match and early-stops only after a node's whole subtree (`desktop/wayland/utils.rs:94-150`). A subsurface placed **below** its parent wins hit-tests over the parent, contradicting the stacking Smithay itself renders (EC-9). 928/928 residual mismatches match this pattern; 0 unexplained. | source + xsnap |
| EC-12 | **C6**: xwayland-satellite 0.8.3 against the harness: X11 window → ordinary `xdg_toplevel` (title from X11, `app_id` = WM_CLASS); an X11 move request (`xdotool windowmove 777 333`) leaves X11 position 0,0 and reaches nothing; satellite `kill -9` → Xwayland and X11 client die, element removed via the ordinary `toplevel_destroyed` path; satellite warns about missing `xdg_activation_v1` and primary selection | `realclient` + [`evidence/phase-c/c6-sat.sh`](evidence/phase-c/c6-sat.sh); [`evidence/phase-c/c6-xwayland-satellite-output.txt`](evidence/phase-c/c6-xwayland-satellite-output.txt) |
| EC-13 | **W-SM-13 (new)**: Smithay's *default* `XdgShellHandler::{maximize_request, fullscreen_request}` call `send_configure()` (`shell/xdg/mod.rs:1143-1153`). xwayland-satellite sends `fullscreen_request` **before the first commit** for a screen-sized X11 window. With the defaults, the initial configure went out before Infynitae's placement ran (element mapped with no anchor). With overrides, placement ran normally. | source + C6 run (with and without override) |
| EC-14 | PROTOCOL: `set_fullscreen` / `unset_fullscreen` / state `fullscreen` text (subject to compositor policy; centre + border fill; "other screen content not part of the same surface tree" must not be visible below) | `xdg-shell.xml` |
| EC-15 | Authority-spike hardening: ports are `!Send`; protocol state is statically asserted `Send`. Storing a port in protocol state fails with **E0277**. All probes asserted on their exact diagnostic. **PASS**. **Real Smithay 0.7.0 protocol state is `Send`** (harness `Server` compiles under `assert_send`). | `experiments/c1-authority-spike/verify.sh`; `b8-harness/src/bin/sendcheck.rs`; [`evidence/phase-c/c1-authority-spike-verify.txt`](evidence/phase-c/c1-authority-spike-verify.txt) |

All harness suites re-run after the CP2 harness changes: X-6, X-8, X-9, X-10, X-12 and xsnap pass; the Phase B scenario output is identical apart from the enriched log line.

---

## CP2-1. X-10 result (CR-2): K1–K9 NOT falsified

| Scenario | Protocol sequence at the client (abridged) | Result |
|---|---|---|
| K-1 ordinary key | `key A pressed`, `key A released` | PASS |
| K-2 modifier + key | `key Shift`, `modifiers dep=1`, `key A`, …, `modifiers dep=0` | PASS |
| K-3 binding Super+B | Super forwarded (`dep=0x40`); B press **and** release consumed; client never sees B | PASS |
| K-4 focus change via binding while Super held | `leave W1`; `enter W2 keys={125}` (Super only; the compositor-owned Enter is absent); `modifiers dep=0x40`; Super release delivered to W2 | PASS |
| K-5 focus change while A held | `enter W2 keys={30}`; A's release delivered to W2 | PASS |
| K-6 focused surface destroyed with Shift+A held | no `leave` (undeliverable, EC-7); `enter W1 keys={30,42}` + `modifiers dep=1`; releases to W1 | PASS* |
| K-7 cancel with compositor-owned keys held, focus change meanwhile | `enter W2 keys={125}`: B, Esc and Enter absent; all their releases consumed; **nothing synthesized** | PASS |
| K-8 binding on a modifier key (K3 observe mode), focus change while held | Shift forwarded; `enter W2 keys={42}` `dep=1`; release delivered; ends `dep=0` | PASS |
| K-9 focus lost (none) while keys held, then regained | `leave`; later `enter keys={42}` `dep=1`; release delivered | PASS |
| **NEG** K-8 with K3 disabled | Shift intercepted; `enter W2` + `modifiers dep=1` (seat-wide XKB, EC-3); the release is intercepted, so **W2 is left with Shift stuck** | FAIL, as predicted |

Every step was checked against: no `wl_keyboard` must/must-not violation; focused client's logical keys = physically held client-owned keys; client modifiers = Smithay seat XKB (`serialized` depressed/latched/locked); nothing held at the end.

\* **Protocol ambiguity (recorded, not hidden).** When the client destroys its focused surface, no `leave` is deliverable (EC-7) and the spec does not say what the logical active surface becomes. The checker treats client-side destruction as nulling the client's logical active surface. That is the only coherent reading, because the client itself performed the destruction. The following `enter` then sets keys and modifiers completely. Infynitae relies on this reading. **Risk RC-1** (low): a toolkit that treats a post-destruction `enter` as a violation would misbehave in any compositor; T3 observes real toolkits.

Not represented: output removal (K8: keyboard focus is output-independent); compositor-side key repeat (K9: not used).

**Smithay seat-wide XKB behaviour confirmed** by the negative control: it is the reason K3 exists, and K3 removes the failure.

---

## CP2-2. Surface-tree snapshot boundary (A-C1 → RESOLVED)

### What core needs, and where it comes from (all public, EC-8)

| Needed for | Datum | Smithay 0.7.0 public source |
|---|---|---|
| all | root surface, element association | `ToplevelSurface::wl_surface()`; adapter registry |
| render order, membership, hit-test | subsurface tree + stacking order | `with_surface_tree_downward` (nearest first) |
| projection, lattice rounding | subsurface offset relative to parent | `RendererSurfaceStateUserData` → `view().offset` (also `SubsurfaceCachedState.location`) |
| projection, membership | surface logical size (after viewport/scale) | `view().dst` |
| render, membership, hit-test | mapped (has buffer) | `RendererSurfaceState::buffer()` |
| hit-test | input region | `SurfaceAttributes::input_region` → `RegionAttributes { rects }` |
| anchor relationship | window geometry (RootSurfaceLocal) | `Window::geometry()` (W-SM-9 caveat) |
| popup trees | popups of the toplevel + offset relative to the toplevel geometry; popup geometry | `PopupManager::popups_for_surface`, `PopupKind::geometry()` |
| popup placement (constraint) | positioner | `PositionerState::get_unconstrained_geometry` (B6.2) |

### Answers

| Question | Answer |
|---|---|
| What remains in `infynitae-wayland` | Every handle (`WlSurface`, `ToplevelSurface`, `PopupKind`, `Window`); the `SurfaceKey ↔ WlSurface` registry; the extraction code (`snapshot.rs` pattern); render-element construction; applying effects |
| What crosses into core | `SurfaceTree` per element: `geometry` (RootSurfaceLocal rect) + front-to-back list of `{key: SurfaceKey, kind: Root/Sub/Popup/PopupSub, origin: RootSurfaceLocal integral point, size, mapped, input: Option<Vec<(add, rect)>>}` and, for popups, configured position + constraint result. Values only. |
| Do handles cross? | **No.** `SurfaceKey` is opaque (the adapter mints it; core cannot dereference it) |
| Crate-private fields needed? | **No.** Compiled in an external crate. The private `surface_view` field (EB-18) is reached through public `view()`. |
| Safe during dispatch? | **Yes.** Extracted inside `CompositorHandler::commit` on every commit in the run, no deadlock (EC-8). Rule: read per-surface data only from the `SurfaceData` passed to traversal callbacks; never call `with_states` for a surface inside its own traversal. |
| Derived or authority? | **Derived** (C2.1): replaced wholesale per commit by a single writer (the adapter's commit hook), dropped on unmap/destroy, never holds an anchor, never written by core. Client state by definition; Stage authority is untouched. |

### Consequences (DECISIONS)

- **D-CP2-HIT:** core hit-tests from the snapshot (front-to-back, half-open containment for both surface bounds and input regions). **`Window::surface_under` is not used** for hit-testing:
  - W-SM-11 shifts input regions by ½ px;
  - W-SM-12 lets below-subsurfaces steal input from their parent, contradicting rendered stacking.

  With both explained, the snapshot hit-test agrees with Smithay at every one of 200,000 grid points. CR-1 / NB-2 (single origin function) is therefore validated for render, hit-test and order at scale 1; fractional edges were already covered by X-6. Status: **RESOLVED for the tested shapes**. Multi-level popups and region subtraction are T1 extensions (CR-1 residual).
- **X-15 / C-NB-3: RESOLVED** (EC-9).
- The render list (C3.3) uses the snapshot's order directly; the adapter does not re-derive order.

---

## C5 — Protocol fullscreen versus viewport-fit

Two different things with different authorities:

| | **Viewport-fit** (Infynitae) | **Protocol fullscreen** (client request) |
|---|---|---|
| What it is | a **camera** operation on one output: frame element W | an **output presentation** state: one output exclusively presents W's surface tree (EC-14: "other screen content … not visible below") |
| Who may start it | user input (`InputPort`) or shell (`ShellPort`) | a client request, accepted by core fullscreen **policy** |
| State owner | `core::navigation` `ViewportFit` per output (saved camera) | `core` `FullscreenState` per element + `OutputPresentation` per output |
| Touches cameras? | **yes** (that is what it is) | **never** (C1.5: protocol code cannot) |
| Touches Stage anchor? | never | never |
| Touches W's size? | never | yes, via configure (protocol-defined: the configure size is the fullscreen area) |

### C5.1 Viewport-fit (D-C5-FIT)

- **Fit** = `navigation::fit(output O, element W)`: camera of O gets `centre = centre(W.geometry)`, `zoom = min(view_w / W.w, view_h / W.h)`. Optional per-output **isolation** flag: only W's tree is rendered on O.
- **Saved state** `ViewportFit{O}: {element, saved centre, saved zoom, isolation}` lives in `navigation`.
- **Restore** = put the saved camera back.
- **Ends when:** restored explicitly; the user navigates O's camera (user intent wins; no restore); W unmapped/destroyed (restore); O removed (restore the camera values, the camera survives unbound, B7).
- **W changes size while fitted:** the zoom is re-fitted (camera follows; Stage untouched).
- **Zoom dependency.** Fit needs camera zoom ≠ 1, so viewport-fit is the first Foundation consumer of zoom:
  - projection already carries `k = zoom·scale` everywhere (B2, C3);
  - a static fitted view has no pan jitter, so A-B1-1 (zoom anti-jitter during motion) is still not required;
  - lattice tiling exactness (E5/E9) is a zoom-1 property; under fit, only W is relevant.
- **REFINEMENT of D-B8-SCALE (needs approval; flagged, not silently adopted).** With zoom ≠ 1 the effective scale on O is `k_O = zoom·scale_O`. MaxGesture stated "max(scale(O))", which equals max(k_O) at zoom 1. Proposed generalisation: **max over observing outputs of k_O** (capped by a configured maximum). Without it, a fitted window is magnified from a lower-resolution buffer (blurry). At zoom 1 nothing changes, and the MaxGesture rule itself is unchanged.

**Case A: worked example (viewport-fit W on output A)**

Setup as B8: W window geometry 800×600 at anchor (1000,300) (root (980,280), 20 px CSD). A: 1920×1080 @1.0, 165 Hz. B: 3840×2160 @1.5 → 2560×1440 logical, 144 Hz.

| Step | Value |
|---|---|
| Stage before | W.anchor = (1000,300) |
| Camera A before | centre (960,540), zoom 1 → v_A = (0,0) |
| Camera B before | centre (1880,720), zoom 1 → v_B = (600,0) |
| Request | user binding (InputPort) or shell `ViewportFit{A, W}` (ShellPort) |
| Camera A after | centre = (1400,600); zoom = min(1920/800, 1080/600) = min(2.4, 1.8) = **1.8**; v_A = (1400 − 1920/3.6, 600 − 1080/3.6) = (866.67, 300) → Q16 (R7) |
| Stage after | **W.anchor = (1000,300): unchanged** |
| Output A projection | k = 1.8; R = 840; C = round((866.67−840)·1.8+δ) = 48; W geometry edges → x 240…1680, y 0…1080: fills the height, 240 px bars left/right (isolation: bars are background, not other elements) |
| Output B projection | **unchanged** (camera B untouched): W at x 570…1830 phys as before |
| Membership | W observed on A and B (unchanged); with isolation, other elements leave A |
| Focus | fit is an explicit user action on W: the focus model may focus W (D-B10-KBD permits explicit actions) |
| Preferred scale | zoom-1 rule: max(1.0, 1.5) = 1.5 (W magnified ×1.2 on A). With the proposed generalisation: max(1.8, 1.5) = 1.8 (client buffer 1440×1080; sharp on A, minified on B) |
| Frame clock | highest-refresh observing output: A (165 Hz); unchanged |
| Restoration | camera A ← (960,540, zoom 1), isolation off; W untouched; scale re-evaluated at once (atomic camera change): 1.5 |

### C5.2 Protocol fullscreen (D-C5-FS)

**Policy and state:**
- **Acceptance:** honour `set_fullscreen` when W has keyboard focus, or when the request arrives before W's first map. The pre-map form is exactly how xwayland-satellite expresses screen-sized X11 windows (EC-13). Otherwise record it and decide when W next gains focus. The spec permits deferral or refusal ("subject to compositor policies"). Rationale: a client must not seize an output's presentation while the user is elsewhere.
- **Output choice:** the requested output if it exists. Else W's frame-clock output if W is observed. Else the output under the pointer. Else the first output.
- **State:** `FullscreenState{element W, output O, saved: {window-geometry size, states}}` and `OutputPresentation(O) = Exclusive(W)`.
- **Configure:** `fullscreen` state, size = O's logical size at zoom 1 (here 1920×1080).
- **Rendering on O:** a **presentation view** replaces camera A's view: W's window-geometry origin maps to O's origin at k = scale_O, centred with border fill if smaller (EC-14). Only W's surface tree (popups included) is drawn on O. **Camera A is not touched.** The presentation view is a per-output render input, not a camera.
- **Hit-testing and input on O** go through the presentation view (it is what `ProjectionFrame(O)` records, I9).
- **Stage:** W.anchor unchanged. W's **size** changes, because the client resizes to 1920×1080 per configure.

**Case B: worked example (fullscreen request targeting output A)**

| Step | Value |
|---|---|
| Client request | `set_fullscreen(wl_output A)` while W focused → accepted |
| Saved | geometry size 800×600; states [] |
| Configure | `[fullscreen]` 1920×1080 |
| Element after ack/commit | anchor (1000,300) (unchanged); window geometry **1920×1080**: client-owned size |
| Output A | presentation view: W at physical (0,0)…(1920,1080); only W's tree drawn; other elements leave A (membership) |
| **Output B (camera B unchanged)** | B renders the Stage as always, so it shows W **at its anchor with its new 1920×1080 size** (x 570… on B's physical grid). **This is the explicit consequence: one wl_surface has one size; every camera observing W sees the fullscreen-sized W.** Infynitae does not pretend otherwise and does not hide W on B. |
| Preferred scale | **MaxGesture unchanged (frozen):** W observed on A (1.0) and B (1.5) → 1.5. A fullscreen game on A that is also visible on B renders at 1.5× (2880×1620 buffer). **Open policy question CP2-Q1** (not adopted): pin a fullscreen element's preferred scale to its fullscreen output while fullscreen. Requires approval and evidence (performance, DV-1). |
| Popups | constraint boundary = output A (the fullscreen output); rendered in the presentation view |
| Focus | unchanged by the request itself (no focus grant from a client request) |
| Output A removed while fullscreen | presentation dropped; core sends a configure leaving fullscreen with the saved size (compositor policy; the spec allows the compositor to change states); camera A survives unbound (B7) |
| Unfullscreen (`unset_fullscreen` or policy) | configure without `fullscreen`, offering the saved size (spec: "may include the previous window geometry dimensions"); presentation of A returns to camera A's view, which is **exactly as it was** |

**Other cases (defined):**

| Case | Behaviour |
|---|---|
| `set_fullscreen(NULL)` | output choice rule above |
| W visible on several outputs | fullscreen on the chosen output only; other cameras observe W as in Case B |
| unmap/remap while fullscreen | the spec discards client state on unmap, so `FullscreenState` and the presentation are dropped; on remap the client must request again |
| request before first map (X11 via satellite) | recorded; the initial configure carries `fullscreen` if accepted; placement (anchor) runs first regardless (EC-13) |
| fullscreen + viewport-fit on the same output | the presentation view takes precedence for rendering. A fit applied meanwhile changes camera A's state, which is shown when the presentation ends. Neither touches the other's state. |
| restoration ownership | `FullscreenState.saved` is owned by core and only used for the unfullscreen configure; the client may choose its own size (protocol) |

**Mechanical guarantee:** fullscreen handling lives in `core::protocol` (client-request policy) and output presentation state. It has no access to `navigation` (C1.5), so a fullscreen request **cannot** move a camera.

---

## C6 — XWayland: xwayland-satellite, deliberately narrow (D-C6)

**Decision: adopt xwayland-satellite as Foundation 01's X11 path.** No internal XWM, no ICCCM/EWMH emulation, no X11 grab architecture.

| Requirement | Finding | Class |
|---|---|---|
| Preserve Stage/Camera authority | satellite is an ordinary Wayland client. X11 toplevels arrive as `xdg_toplevel`s and become elements through the normal lifecycle (B5). An X11 **position request had no effect** at all (EC-12): there is no X11 path to Stage. | EXPERIMENT |
| Representation to Infynitae | element with title (X11 name) and `app_id` (WM_CLASS); override-redirect windows (menus, tooltips) are presented by satellite as Wayland popups/subsurfaces of their parent (satellite's design; **not exercised here**, no menu-using X11 client) | EXPERIMENT (toplevel); ASSUMPTION (popups) → T3 |
| Process/socket lifecycle | the compositor spawns satellite with `WAYLAND_DISPLAY` set to its own socket and a chosen free X display; satellite runs Xwayland itself | DECISION (supervisor in the bin crate; C1 later) |
| Startup | lazy or at startup (configuration); `DISPLAY` exported to children spawned by the compositor/shell | DECISION |
| Crash isolation | satellite crash → Xwayland and all X11 clients die; their elements are destroyed by the normal client-death path; Stage stays consistent (EC-12) | EXPERIMENT |
| Restart | restart satellite with backoff; X11 apps do **not** survive (X11 clients died with the server) | EXPERIMENT (death); DECISION (restart) |
| Identity across failure | **none** (new elements, new ids). Persistence/restore keys (Level D) may later re-match by `app_id`/title. | DECISION |
| Focus/activation | ordinary keyboard focus on the satellite's surfaces; satellite uses `xdg_activation_v1` if offered (it warned when absent), so it is subject to the same B10.7 policy. X11 focus-stealing semantics are not emulated. | EXPERIMENT (warning); DECISION |
| Global X11 coordinate assumptions | X11 sees a screen of the Arrangement size (here 1920×1080, window at 0,0); X11 move/configure requests do not reach Infynitae. Apps that position their own windows in screen coordinates will not be obeyed (by design: Stage is the authority) | EXPERIMENT |
| Fullscreen | satellite sends `fullscreen_request` (pre-map for screen-sized windows) → C5 policy; requires **W-SM-13** overrides | EXPERIMENT |
| Security | satellite is a normal client: no privilege, no camera path (C1.5) | DECISION |

**Deferred:** internal XWM; broad ICCCM/EWMH; X11 grabs; legacy edge cases; override-redirect fidelity (T3 with Steam/menu-heavy apps); X11 primary selection (satellite: unsupported without the protocol).

**XWayland can never become a second Stage authority:** its only channel into Infynitae is the Wayland protocol, and Wayland has no client-to-Stage position path.

---

## C7 — Trusted shell boundary (D-C7)

### C7.1 Identity, trust, channel

- **Channel:** a Unix stream socket `$XDG_RUNTIME_DIR/infynitae/shell.sock`, directory mode 0700, socket 0600.
- **Admission:** `SO_PEERCRED` uid must equal the compositor's uid.
- **Trust statement (honest):**
  - Any process of the same user can connect, exactly as with `hyprctl`.
  - The authority boundary C1.5 enforces is between the **Wayland protocol channel** and Infynitae authority. It is not between same-user processes, which the Linux desktop does not isolate anyway.
  - Sandboxed (Flatpak) apps do not see the runtime directory unless it is exported.
- **Crate:** `infynitae-ipc` (new; justified by C2: it is the only holder of `ShellPort`, and it isolates serialization dependencies away from zero-dependency core). Deps: `core` only (+ a serialization crate). Must not depend on `wayland` or backends. `ShellPort` lives in the IPC source closure, not in protocol state (C1.5, C7.4).

### C7.2 Operations are semantic actions

| Category | Examples (illustrative, **not** a frozen API) | Path in core |
|---|---|---|
| Navigation | `Navigate{output, to: Element | CanvasPos | Delta}`, `ViewportFit{output, element}`, `ViewportRestore{output}` | `navigation` (ShellPort) |
| Focus | `Focus{element}`, `AcknowledgeAttention{element}` | focus model (explicit-action path) |
| Spatial action | `MoveElement{element, to: CanvasPoint}`: **the same validated placement path as an interactive drag commit** (R3 rounding, ±2³⁶ bound, mapped, not fullscreen; rejected otherwise) | `stage::place` (single writer) |
| Queries | `Snapshot{…}` → immutable, generation-stamped copy of Stage metadata, cameras, outputs, focus, attention | read-only |
| Events | element lifecycle, focus, attention, camera changes, output changes, each with a monotonically increasing **generation** | read-only |

**There is no `set_element_canvas_rect`, no raw state write, no mutable access to `Core`.** Every command is a value that core validates and executes through the same single-writer paths user input uses. **No size writes:** element size is client-owned protocol state (C5).

### C7.3 Protocol semantics

| Concern | Rule |
|---|---|
| Command ordering | per connection, executed in order; each response carries the request id and the resulting generation |
| Event ordering | single totally ordered stream per connection (generation numbers); a subscriber that misses events re-syncs by `Snapshot` + events with generation > snapshot's |
| Stale IDs | `(InstanceId, ElementId)` (D-B4); mismatch or destroyed → `StaleElement` error; never aliases |
| Reconnect / shell restart | stateless from the compositor's view: Snapshot + Subscribe. Compositor keeps **no** shell-owned authoritative state. |
| Shell crash | connection dropped; nothing to roll back (commands are atomic); compositor-owned animations started by shell commands continue or end on their own |
| Cancellation | commands are atomic. A long-running effect (animated navigation) is a compositor interaction, superseded by the next navigation or by user input. |
| Presentation-only state (shell-owned) | overview open/closed, launcher, HUD layout, thumbnail selection. The compositor does not store it. |
| Authoritative vs derived | everything the shell receives is a **derived copy**; nothing the shell holds is authority |

### C7.4 Camera authority hardening (C-NB-2 → mechanically guarded)

| Mechanism | Effect | Evidence |
|---|---|---|
| navigation-module privacy, private port fields, port-required entry points | protocol code cannot move cameras or forge authority | EC-1 (E0624, private-field diagnostic, E0061, E0603) |
| ports are `!Send`; adapter protocol state `const`-asserted `Send` | **storing a port in protocol state is a compile error (E0277)** | EC-15; real Smithay protocol state is `Send` |
| ports consumed by value by their sinks (`backend::run(InputPort)`, `ipc::serve(ShellPort)`), minted only by `Core::new` | the composition root is the only place both exist | design |
| `verify-architecture` source check: the `infynitae-wayland` sources must not mention `InputPort`, `ShellPort`, `physical_input`, `shell_command` | conspicuous, mechanical | to be ported in Phase F |
| `infynitae-ipc` must not depend on `infynitae-wayland` (and vice versa) | shell privilege cannot leak into protocol handling | `cargo tree` check |

Residual (accepted): a deliberate change to the composition root could pass a port into protocol code by parameter. That would be a conspicuous, reviewed edit and violate the source check. Accidental leakage through state is now a compile error.

---

## C8 — Failure model (D-C8)

Classes: **R** reject operation · **L** recover locally · **D** reconstruct derived state · **S** restart subsystem · **G** degrade feature · **T** terminate compositor.

| Failure | Class | Authoritative state that survives | Action |
|---|---|---|---|
| Wayland client crash/disconnect | L + D | Stage minus that client's elements (B5 destroy path; idempotent, any order: EB-5); cameras; focus history | elements destroyed; focus successor; membership/scale/clock recomputed |
| Client protocol error | L | same | Smithay posts the error and disconnects → as above |
| Shell crash/disconnect | L | everything (shell owns nothing authoritative) | drop the connection; nothing to roll back; shell reconnects with a Snapshot |
| Invalid shell command | R | unchanged | error response; no partial effects (commands validated before any write) |
| Stale `ElementId` (shell or internal) | R | unchanged | `StaleElement`; never aliases |
| Output disappearance | L + D | Stage, cameras (unbound, B7), focus | B7.2: `leave` before the global goes; fullscreen presentation dropped (C5); interactions orphaned (C4.6); membership/scale/clock recomputed |
| Input device disappearance | L | ownership table entries of that device's held buttons/keys | treat as physical release of every held input of that device (a **real** device state change, not synthesis); orphaned interactions end |
| Renderer failure (GPU reset / context lost) | S (renderer) + D | all core state; client buffers | recreate renderer and damage trackers; re-import buffers; full damage; if recreation fails → T |
| Backend failure (DRM device lost, winit window closed) | T (Foundation 01) | — | orderly shutdown; clients disconnected. DRM session pause/resume is Phase G. |
| xwayland-satellite crash | S + L | Stage minus X11 elements | X11 clients die (EC-12), elements destroyed normally; restart satellite with backoff; repeated crash → G (X11 disabled; logged) |
| Smithay adapter error (handler `Result` errors, unexpected protocol state) | L or R | core untouched | log + reject the operation, or disconnect the offending client (`post_error`). Never patch core state to "fit". |
| Internal invariant violation in core (e.g. membership without element, two anchors) | T | — | **do not continue**: an inconsistent authority must not be "repaired" by guessing. Panic-free design: core functions return `Result`; invariant checks run in debug and on cheap checkpoints in release. A violation is a bug and terminates with a diagnostic dump. |
| Persistence load failure (Level D) | G | empty/default Stage | start with nothing restored; never partially apply a corrupt file; keep the file for inspection |
| Partial startup (some globals fail) | T for **required** globals; G for optional ones | — | required: compositor v6, xdg_wm_base, shm, seat, outputs, presentation, fractional-scale, viewporter. Optional: dmabuf (shm fallback), pointer-constraints/relative-pointer (games degrade), xdg-activation (no activation). |
| Required protocol global initialisation failure | T | — | abort with diagnostics before accepting clients |

**Rule: recovery rebuilds derived state from authority.** After any L/S action, `MembershipTable`, `ProjectionFrame`s, scale/clock assignments and render state are **recomputed from Stage + cameras + outputs + snapshots**, never patched incrementally from a possibly inconsistent mirror. Stage corruption is never hidden by best-effort recovery (it is the T row).

---

## C9 — `unsafe` and dependency-patch policy (compact)

**`unsafe`:**
- `infynitae-core`: `#![forbid(unsafe_code)]`.
- `infynitae-ipc`: `#![forbid(unsafe_code)]`.
- `infynitae-wayland` and backends: `#![deny(unsafe_code)]`, with per-site `#[allow(unsafe_code)]` only where safe Rust cannot express the operation. Each site must carry:
  1. a `// SAFETY:` invariant;
  2. the narrowest possible scope;
  3. a listing in `docs/foundation/UNSAFE_INVENTORY.md` (created when the first site exists);
  4. a verification note (test or argument);
  5. no authority or ownership effect (an `unsafe` block may not touch core state).

  The one known required site is calloop's `Generic::get_mut` for `Display::dispatch_clients` (the standard Smithay idiom).
- `unsafe` can never bypass authority: core is `forbid`, and ports cannot be forged even with `unsafe` from outside without UB-level misuse, which review of the inventory catches.

**Dependency patches:**
- Smithay stays pinned per D-B12-PIN (`=0.7.0`, locked).
- A patch is permitted only under the D-B12-PIN patch policy: reproduced defect (experiment), no public-API fix, an upstream issue/PR, one minimal commit, listed with its reproducer, and removed once upstream fixes it.
- **No patch is made in this checkpoint.** W-SM-4b (X-12) remains a documented, reproduced, isolated integration defect.
- No patch-management framework.

---

## C10 — Observability (D-C10)

`tracing` spans and events, structured fields, no background machinery.

| Field | Meaning |
|---|---|
| `element` | `ElementId` |
| `camera` | `CameraId` |
| `output` | `OutputId` |
| `interaction` | interaction/gesture id |
| `gen` | Stage/event generation |
| `client` | client id |
| `surface` | `SurfaceKey` |

| Target (category) | Events (default level) |
|---|---|
| `inf::stage` | element created/mapped/unmapped/destroyed; **every Stage mutation** (anchor before→after, cause: drag/shell/placement) (INFO) |
| `inf::camera` | camera mutation (cause and authority: input/shell/fit), fit/restore (DEBUG per change; INFO for fit) |
| `inf::focus` | keyboard focus transitions (cause), attention (INFO) |
| `inf::input` | ownership transitions (press owner, consumed release, acquisition, cancellation) (DEBUG); individual motion events are **TRACE only** |
| `inf::visibility` | membership enter/leave, preferred-scale transitions, clock changes (DEBUG) |
| `inf::frames` | kick (reason), fallback completion (DEBUG); per-frame activity TRACE only |
| `inf::protocol` | configure generation (serial, cause), fullscreen requests/decisions, activation decisions (DEBUG/INFO) |
| `inf::shell` | command received, result, generation (INFO) |
| `inf::failure` | subsystem failure/recovery (WARN/ERROR) |
| `inf::workaround` | activation of each W-SM workaround (e.g. a W-SM-13 override hit, a W-SM-4a ordering decision) (DEBUG) |

**Rules:**
- Observability is never read by any decision path; it is not state.
- No telemetry, metrics daemon, polling, timers or network.
- High-frequency categories are TRACE and compiled-in but disabled by default. With no subscriber interested, a disabled `tracing` callsite costs a cached check. That is negligible idle cost and starts no background subsystem.
- Diagnostic dumps (the C8 T-row) are produced on demand or on fatal error only.

---

## CP2 contradiction pass

| # | Check | Result |
|---|---|---|
| 1 | X-10 agrees with C4 | **Yes**: K1–K9 not falsified; the negative control shows K3 is necessary; one protocol ambiguity recorded (RC-1) |
| 2 | Fullscreen and viewport-fit have separate state and authority | **Yes**: `ViewportFit` in `navigation` (ports) vs `FullscreenState` + `OutputPresentation` in protocol policy (C5) |
| 3 | Protocol fullscreen cannot move cameras | **Yes**: the presentation view is a render input, not a camera; protocol policy cannot reach `navigation` (C1.5, EC-1) |
| 4 | Multi-output fullscreen semantics explicit | **Yes**: Case B; the size consequence for other cameras is stated, not hidden |
| 5 | XWayland cannot become Stage authority | **Yes**: X11 move request had no effect (EC-12); satellite is an ordinary client |
| 6 | Shell privilege does not leak into protocol handling | **Yes**: ports outside protocol state (E0277 guard); `ipc ↛ wayland`, `wayland ↛ ipc` |
| 7 | Shell IPC cannot become arbitrary Stage mutation | **Yes**: semantic commands only; `MoveElement` uses the drag-commit path; no size or raw writes (C7.2) |
| 8 | Failure recovery reconstructs derived state | **Yes**: C8 rule; invariant violation terminates rather than repairs |
| 9 | `unsafe`/patch policy cannot bypass authority | **Yes**: core and ipc `forbid(unsafe_code)`; no patch now |
| 10 | Observability observational | **Yes**: no decision reads it (C10) |
| 11 | Snapshot does not create a second spatial model | **Yes**: derived per commit, no anchors, single writer (CP2-2) |
| 12 | Render order agrees with Smithay | **Yes**: X-15 PASS (EC-9) |
| 13 | No new subsystem gains persistent spatial authority | **Yes**: new state is `ViewportFit` (camera snapshot), `FullscreenState` (saved size/states), `OutputPresentation` (render input) and the IPC connection; none holds anchors |
| 14 | Hit-testing agrees with rendered stacking | **Contradiction found in Smithay, not in Infynitae**: `Window::surface_under` disagrees with rendered stacking (W-SM-12) and with half-open region containment (W-SM-11). Resolved by D-CP2-HIT. |
| 15 | Fullscreen vs MaxGesture | **Tension recorded, not resolved by silent change**: MaxGesture (frozen) makes a fullscreen element also visible on a higher-scale output render at that higher scale. CP2-Q1 is an open policy question requiring approval. |
| 16 | Viewport-fit vs MaxGesture | **Refinement proposed, not adopted**: generalise to effective scale `k = zoom·scale` (identical at zoom 1). Requires approval because it touches a frozen decision's wording. |

**No Phase B or C1 invariant is contradicted.** Two items need your decision: CP2-Q1 and the MaxGesture effective-scale generalisation. Neither blocks the architecture: both are policy values over the same mechanism.

## CP2 findings, conditions, risks

| ID | Item | Status |
|---|---|---|
| A-C1 | snapshot via public API | **RESOLVED** (EC-8) |
| C-NB-2 | port in protocol state | **mechanically guarded** (E0277) + source check; residual: deliberate composition-root edit |
| C-NB-3 / X-15 | render order | **RESOLVED** (EC-9) |
| CR-1 / NB-2 | single origin function | **RESOLVED for tested shapes** (D-CP2-HIT); multi-level popups and region subtraction → T1 |
| CR-2 / X-10 | keyboard ownership | **VALIDATED** (EC-6) |
| RC-1 (new) | client-side destruction of the keyboard-active surface: protocol ambiguity | low risk; T3 |
| W-SM-11, W-SM-12, W-SM-13 (new) | Smithay quirks | adapter-local workarounds: own hit-test; override `maximize_request` / `fullscreen_request` |
| CP2-Q1 (new) | preferred scale of a fullscreen element | **open policy question** (needs approval + evidence) |
| CP2-R1 (new) | MaxGesture generalised to `k = zoom·scale` | **proposed refinement** (needs approval) |
| C6-A1 (new) | satellite override-redirect fidelity (menus/tooltips) | ASSUMPTION → T3 |
| CR-3, CR-4 | output removal during interaction; popup-grab dismissal | still T1 (X-11a/b), designs in C4.6 / C5 |
