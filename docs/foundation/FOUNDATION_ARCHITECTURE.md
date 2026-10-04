# Infynitae Native — Foundation Architecture (Phase C)

Status: checkpoint 1 (C1–C4) accepted, commit `d596416cf57877116e9cfbc8cf6c6fe25c001c67`. Checkpoint 2 (C5–C10) accepted. **Checkpoint 3A complete; stopped at the 3A review gate.** C11–C16 (checkpoint 3B) not started.

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

---
---

# Phase C checkpoint 3A: scale/zoom, hit-testing, obligation registry (accepted, commit `795800937a1e50e56587cd4080a73ce27d8a8b01`)

Checkpoint 2 accepted: `fc2dd68f843b1eb05828f4116f94e8bab4f478da`.

## CP3A evidence

| ID | What | Where |
|---|---|---|
| EC-16 | Device limits (vulkaninfo, RX 9070 XT / RADV): `maxImageDimension2D = 32768`, **`maxFramebufferWidth = 16384`**. A client cannot render a buffer wider than 16384 px, so for a window of logical width W the client scale is physically bounded by 16384/W. `wp_fractional_scale_v1` itself states **no** bound. | `vulkaninfo`; `fractional-scale-v1.xml` |
| EC-17 | **scaleprobe**: GTK 4.22.5, kitty 0.49.2 (swrast), Firefox 157. **All three honour a 1/120 change with a full re-render and a new buffer** (628→633, 1920→1936, 1280→1291 px). Client CPU grows with scale² for software rendering (kitty 20.7% @1.0 → 218% @3.0). **A per-frame scale storm (60 changes/s)** costs one reallocation per change: GTK4 30% CPU, kitty 477%, Firefox 105% (vs a few % steady). | `b8-harness/src/bin/scaleprobe.rs`; [`evidence/phase-c/cp3a-scaleprobe-output.txt`](evidence/phase-c/cp3a-scaleprobe-output.txt) |
| EC-18 | Buffer-cost table (computed) for candidate ceilings, double/triple buffering, plus a compositor copy for shm | [`evidence/phase-c/cp3a-buffer-cost.txt`](evidence/phase-c/cp3a-buffer-cost.txt) |
| EC-19 | Executable **MODEL** of the unified scale policy (not an experiment): walkthroughs A–E with the candidate constants | `experiments/cp3a-scale-model/model.py`; [`evidence/phase-c/cp3a-scale-model-output.txt`](evidence/phase-c/cp3a-scale-model-output.txt) |
| EC-20 | **xhit**: hit-testing at k ∈ {1.0, 1.25, 1.5, 1.75} × 4 fractional camera positions, every pixel over a tree with a nested subsurface, a below-parent subsurface reached through an **input-region hole**, and a popup + popup subsurface. **Physical-pixel selection vs the surface Smithay actually presents: 0 mismatches** (16 configurations, 159,506–482,002 pixels each). The Phase B canvas-exact rule would mismatch on 399–617 pixels per configuration at fractional k. 12 hand-derived semantic cases PASS. One hand oracle was itself wrong (point inside S1); it was corrected and the record is kept in the source. | `bin/xhit.rs`; [`evidence/phase-c/cp3a-xhit-output.txt`](evidence/phase-c/cp3a-xhit-output.txt) |
| EC-21 | **RC-1 with a real toolkit (GTK 4.22.5)**: the client destroys its keyboard-focused window while Shift+A are held. Observed: no `leave` (undeliverable, EC-7); `enter(W1, keys=[A,Shift])` + `modifiers(shift)`; GTK activates W1 **without** emulating presses; delivers the A and Shift releases with correct state; a following `b` arrives with **no modifier**; 0 GTK warnings/criticals | `bin/rc1.rs`, `cp3a-rc1-gtk4.py`; [`evidence/phase-c/cp3a-rc1-gtk4-output.txt`](evidence/phase-c/cp3a-rc1-gtk4-output.txt) |

---

## CP3A-1. Unified scale model (resolves CP2-Q1 and CP2-R1)

### Concepts kept separate

| # | Concept | Definition | Owner |
|---|---|---|---|
| 1 | **Raw presentation requirement** ρ_p | `output_scale(O) × presentation_zoom(p)` for each presentation p observing element E. Camera views: `presentation_zoom = camera zoom`. Protocol-fullscreen presentation view (C5.2): `presentation_zoom = 1`. ρ(E) = max over observing p. | derived (core visibility) |
| 2 | **Client scale hint** h(E) | the value sent: `wp_fractional_scale_v1.preferred_scale = round(h·120)` and `wl_surface.preferred_buffer_scale = ⌈h⌉`; a member of a finite ladder; bounded | core `ScaleState` (policy state) → protocol |
| 3 | **Compositor factor** m_p = ρ_p / h | > 1: the compositor magnifies (upscale filter); < 1: it minifies (downscale filter) | derived per frame (renderer) |
| 4 | **Gesture-time state** | during an active camera gesture h may only **increase**, and only at ladder-rung crossings | core |
| 5 | **Settled state** | at gesture end or on an atomic camera change, `h := t(ρ)` exactly | core |

**CP2-Q1 resolved: protocol fullscreen does not override MaxGesture.** The fullscreen presentation contributes ρ = scale_O like any presentation; B's 1.5 requirement still counts (Case C).

**CP2-R1 resolved: camera zoom enters the requirement** (ρ includes zoom), but the requirement is **not** sent directly. It passes through the ladder and the ceiling.

### Rules (DECISION D-CP3A-SCALE; S5/S6 = MaxGesture generalised)

| Rule | Statement |
|---|---|
| S1 | ρ(E) = max over observing presentations of `output_scale × presentation_zoom` |
| S2 | **Ladder** L (finite, sorted): every connected output's scale ∪ the integers 1..⌊H_max⌋ ∪ geometric fill such that consecutive rungs differ by a ratio ≤ r; all rungs ≤ H_max(E) |
| S3 | **Ceiling** `H_max(E) = min(H_cfg, D_max / max(logical w, logical h of E))`. `D_max` is **queried from the renderer at runtime** (16384 on the target device), not a constant. |
| S4 | **Target** `t(ρ) = smallest rung ≥ ρ / (1 + σ)`, else H_max (σ = sharpness tolerance) |
| S5 | **Increase**: if t > h then h := t immediately, during gestures too (bounded: one change per rung crossed) |
| S6 | **Decrease**: if t < h, only when no camera gesture is active (gesture end, atomic camera change, membership change outside a gesture) |
| S7 | **Unobserved**: hold h |
| S8 | **Emission**: only on change (Smithay dedupes, EB-12); fractional = round(h·120)/120; integer = ⌈h⌉ (D-B8-SCALE ⌈⌉ rule kept) |
| S9 | **Beyond the ceiling** the compositor magnifies: m_p = ρ_p / h > 1 using `upscale_filter` (C3.5); visual quality is DV-2 |

### Frozen properties (architecture; independent of tuning)

1. **P1 Bounded hint.** h ≤ H_max(E) for any camera zoom. Camera zoom is unbounded; the hint is not. Grounded in the device limit (EC-16) and buffer cost (EC-18).
2. **P2 Finite hint set.** h ∈ L, so |L| is the maximum number of distinct hints. Grounded in EC-17: clients reallocate on every 1/120 change, so protocol granularity must not be policy granularity.
3. **P3 No storms.** Within one gesture, renegotiations ≤ the number of rungs crossed upward, plus one settlement. There are never per-event renegotiations. Grounded in EC-17 storm costs.
4. **P4 Reduction to Phase B.** At zoom 1 with no zoom gestures, S1–S8 give exactly D-B8-SCALE MaxGesture, because every output scale is a rung (Case A).
5. **P5 Determinism.** h is a pure function of (presentation set, zooms, gesture boundaries, ladder configuration). No timers.
6. **P6 No decrease during a gesture** (MaxGesture).
7. **P7 Correctness beats stability.** If E's logical size changes so that h > H_max(E) (device bound), h is lowered immediately, even mid-gesture.

### Gesture settlement: why no timer

- **Continuous zoom** (pinch, drag-zoom) has a physical begin/end, so settlement happens at gesture end (S6).
- **Discrete zoom** (wheel ticks, key steps) has no end. Each tick is an atomic change, so increases and decreases settle exactly. Renegotiations are bounded by rung crossings and the user's discrete input rate.
- The only residual case is a user wiggling the wheel exactly across a rung boundary (≤ 1 change per tick). It is **not** a storm per P3, but it is measurable.

**OB-23** (registry) defines the experiment. If that case proves costly, an *optional* decrease-settle delay may be added, with a configured value: provisional, default off.

### Candidate constants (PROVISIONAL, not established)

| Constant | Candidate | Basis | Tuning experiment |
|---|---|---|---|
| H_cfg | 3.0 | a 1920×1080 window costs 71 MB/buffer at 3.0 vs 127 MB at 4.0 (×2–×3 swapchain); software-rendering client CPU ×9 at 3.0 (EC-17/18); typical output scales ≤ 3. **Not** a quality judgement. | X-16 |
| r (rung ratio) | 1.25 | ≥ 25% steps make every renegotiation substantive (EC-17: 1/120 steps reallocate) while magnification between rungs stays ≤ r | X-16 |
| σ (sharpness tolerance) | 0.05 | avoids jumping a rung for a ≤ 5% requirement excess (Case D: m = 1.039) | X-16 + DV-2 |
| D_max | runtime query (16384 measured) | EC-16 | runtime, not tuned |

**X-16 (tuning experiment):** a zoom sweep 1→8→1 with real clients (GTK4, Firefox, kitty, one game), measuring renegotiations, client CPU, memory and frame time. Plus a visual sharpness comparison at m ∈ {1.0, 1.04, 1.25, 2.0, 2.67}. Destination: T3/T4.

### Mandatory walkthroughs (EC-19 MODEL output; candidate constants)

Window W logical 800×600. Outputs: A scale 1.0, B scale 1.5. Ladder = [1.0, 1.2247, 1.5, 1.7321, 2.0, 2.4495, 3.0], H_max = 3.0.

**Case A: mixed DPI baseline** (A zoom 1, B zoom 1)

| | ρ_p | factor |
|---|---|---|
| A | 1.0 | 0.667 (minify) |
| B | 1.5 | 1.000 |

ρ = 1.5 → **h = 1.5** (`180/120`, integer 2). **Identical to Phase B** (SC1).

**Case B: zoom dominates DPI** (A zoom 2.0, B zoom 1)

| | ρ_p | factor |
|---|---|---|
| A | 2.0 | 1.000 |
| B | 1.5 | 0.750 (minify) |

ρ = 2.0 → **h = 2.0** (`240/120`, integer 2). Client-visible: one `preferred_scale 240` (+ `preferred_buffer_scale 2` unchanged from 2). The client re-renders at 1600×1200.

**Case C: protocol fullscreen on A** (presentation zoom 1) **while B observes**

| | ρ_p | factor |
|---|---|---|
| A (fullscreen view) | 1.0 | 0.667 |
| B | 1.5 | 1.000 |

ρ = 1.5 → **h = 1.5: no fullscreen override.** A 1920×1080 fullscreen surface then renders 2880×1620 (17.8 MB/buffer vs 7.9 MB at 1.0). That cost is accepted by decision CP2-Q1. When camera B stops observing W (an atomic change) → ρ = 1.0 → h = 1.0 (`120/120`).

**Case D: viewport-fit on A** (zoom = min(1920/800, 1080/600) = 1.8), B normal

| Step | Value |
|---|---|
| Stage | W anchor unchanged (C5.1) |
| Camera A | centre on W, zoom 1.8 |
| Camera B | unchanged |
| ρ_A / ρ_B | 1.8 / 1.5 |
| hint | 1.5 → **1.7321** (`208/120`, integer 2) at the fit (atomic) |
| factors | A ×1.039 (slight magnify, within σ); B ×0.866 (minify) |
| restore | ρ = 1.5 → h = 1.5 (atomic, exact settle) |

Two renegotiations total.

**Case E: continuous zoom of A from 1.0 to 8.0 (pinch, 240 updates at 120 Hz), reversal to 4.0 (120 updates), A pans away while still in the gesture, gesture ends**

| Event | Hint |
|---|---|
| z = 1.583 | 1.5 → 1.7321 |
| z = 1.834 | → 2.0 |
| z = 2.107 | → 2.4495 |
| z = 2.594 | → **3.0** (ceiling) |
| z = 2.594 … 8.0 | 3.0. The compositor magnifies up to **×2.667** at z = 8 (S9). |
| reversal 8.0 → 4.0 | **no change** (no decrease in a gesture) |
| W leaves A during the gesture | 3.0 held (decrease deferred) |
| gesture end | → 1.5 (B's requirement) |

**Total: 5 renegotiations for 362 updates.** Without ladder and ceiling (hint = raw ρ at 1/120 granularity): **313** renegotiations and a final hint of 8.0 (an 800×600 window → 6400×4800 buffer, 117 MB each).

### Buffer cost (EC-18, excerpt)

| Logical window | Device bound | MB per buffer @1.0 / 1.5 / 2.0 / 3.0 / 4.0 / 8.0 | ×3 swapchain @3.0 |
|---|---|---|---|
| 800×600 | 20.48 | 1.8 / 4.1 / 7.3 / 16.5 / 29.3 / 117 | 49 MB |
| 1920×1080 | 8.53 | 7.9 / 17.8 / 31.6 / 71.2 / 127 / 506 | 214 MB |
| 2560×1440 | 6.40 | 14.1 / 31.6 / 56.3 / 127 / 225 / **exceeds** | 380 MB |
| 3840×2160 | 4.27 | 31.6 / 71.2 / 127 / 285 / 506 / **exceeds** | 854 MB |

Camera zoom cannot map without bound to client scale: at modest zoom the buffer cost rises quadratically into hundreds of MB per window, and beyond the device bound the buffer cannot even be rendered. The architecture freezes **P1–P7**. H_cfg is a provisional configuration value (OB-12).

---

## CP3A-2. Hit-testing (D-CP3A-HIT; REFINEMENT of D-B2-I9)

**Rule:** the client surface selected by hit-testing is the surface **presented** at the pointer's physical pixel under Infynitae's render model, subject to Wayland input-region semantics. Protocol semantics outrank Smithay helper behaviour: `Window::surface_under` is not used (W-SM-11, W-SM-12).

| Aspect | Specification |
|---|---|
| Order | exactly the render order (C3.3): elements in reverse Stage z-order; within an element: each popup tree (in `popups_for_surface` order), then the root tree. Each tree is traversed **downward** (subsurfaces placed above their parent, then the parent, then those placed below), recursively for nested subsurfaces. |
| Pixel | pointer `(O, p)` → physical `u = p · scale_O` (output transform applied) → pixel `(⌊u_x⌋, ⌊u_y⌋)` |
| Coverage | a surface covers the pixel iff `edge(x₀) ≤ i < edge(x₁)` and likewise for y, using **the same** `edge()` / `R` / `C` / `c_eff` as rendering (C3.2). Fractional camera positions and fractional scales are handled by those functions only. |
| Input region | none ⇒ the whole surface. Otherwise the protocol's add/subtract sequence in request order; containment is **half-open** on the surface-local logical point `q − origin` (q = Canvas point at the pointer) clamped into the surface. Never round-to-nearest (W-SM-11). |
| Selection | the first surface in order that covers the pixel **and** whose input region contains the point. A region hole falls through to whatever is below, including a below-parent subsurface (EC-20). |
| Delivered coordinates | `q − origin` (Canvas, exact), clamped into `[0, w) × [0, h)`. At pixel centres the clamp is provably 0: `round(a) ≤ i ⇒ a ≤ i + ½` (EC-20: max clamp 0.000); off-centre ≤ ½ px / k. |
| Popups | hit before their parent's tree; client-owned popup grabs (Smithay) still decide focus for their sequences (C4.2 P3) |
| Exactness vs Phase B | D-B2-I9 tolerated ≤ ½ physical px of disagreement between hit edges and rendered edges. Measured: 399–617 pixels per configuration at fractional k. D-CP3A-HIT removes it: **0 mismatches** (EC-20). |

**Verified now (EC-8, EC-9, EC-20):**
- parent/subsurface overlap;
- below-parent and above-parent subsurfaces;
- nested subsurface;
- popup/root overlap and popup subsurface;
- input-region subtraction (single hole) with fall-through;
- fractional boundaries at 4 scales × 4 cameras;
- shared render/hit-test origins;
- order equivalence with Smithay rendering.

**For C11 (T0/T1):**
- multi-level popups;
- multi-rect regions with interleaved add/subtract;
- rotated or flipped output transforms;
- zoom ≠ 1 (k not of the form n/120);
- sub-pixel pointer positions and the clamp bound;
- surfaces spanning two outputs;
- property-based random trees.

Smithay's 1,448 disagreements are **not** reproduced and are not a target.

---

## CP3A-3. RC-1: keyboard focus when the client destroys its focused surface

| Aspect | Finding | Class |
|---|---|---|
| Protocol text | wayland 1.26 defines the `wl_keyboard` logical state and that `leave` resets it, but says **nothing** about client-side destruction of the active surface. The ordering is implicit. | PROTOCOL (silence) |
| Mechanism | a `leave` naming a destroyed object cannot be sent (wayland-backend refuses; EC-7) | SMITHAY/wayland-backend source |
| Compositor internal transition | `toplevel_destroyed` → focus model picks the successor (most recently focused live element) → `KeyboardHandle::set_focus(successor)` | DECISION (C4.5) |
| Client-visible sequence | (client) `wl_surface.destroy` → (server) `wl_keyboard.enter(successor, keys = client-owned held keys)` + `modifiers(seat XKB)`; no `leave` | EXPERIMENT (X-10 K-6, EC-21) |
| Next-focus state | keys and modifiers exactly the held client-owned set; subsequent releases delivered to the successor | EXPERIMENT |
| Stale state | **none observed** in GTK 4.22.5: releases processed with the right state, the next key unmodified, no warnings (EC-21) | EXPERIMENT (one toolkit) |
| Remaining validation | Qt 6, SDL2/3 (games), Chromium/Electron, Firefox (multi-window), Wine/Proton | OB-10 → T3 |

**Disposition:** RC-1 is reclassified from "unverified risk" to **"protocol-implicit; validated for GTK4; other toolkits in T3"**. It is not "standard harmless behaviour" by assertion.

---

## CP3A-4. Obligation registry (complete; every open item has a destination)

Destinations:
- **GATE-C**: must close before the Phase C gate (3B)
- **T0**: pure unit/property tests
- **T1**: headless/lockstep integration (production adapter)
- **T2**: nested
- **T3**: real-client compatibility
- **T4**: physical DRM/KMS
- **T5**: games/manual
- **UP**: external/upstream
- **TUNE**: provisional product/performance tuning

| ID | Item | Current evidence | Owner / layer | Future test or decision | Pass/fail criterion | Destination | Blocks implementation? |
|---|---|---|---|---|---|---|---|
| OB-1 | Multi-level popups (CR-1 residual) | one popup level verified (EC-8/9/20) | core origin(), adapter extraction | nested popup chain: order/positions vs Smithay render; hit-test vs presented | 0 mismatches | T1 | no |
| OB-2 | Input-region subtraction, general | single-hole fall-through verified (EC-20) | core hit-test | multi-rect, interleaved add/subtract; random property test | equals a reference region evaluator | T0 + T1 | no |
| OB-3 | Output removal during an active interaction (CR-3, X-11a) | design C4.6 | core interaction + output manager | headless backend removes an output mid-pan/mid-move | INV-1/INV-2 hold; pointer placed on a remaining output; no stuck buttons | T1 | no |
| OB-4 | Popup-grab dismissal on interaction acquisition (CR-4, X-11b) | design C4.6, B6.3 | core + adapter (W-SM-7 precondition) | pan acquired while a popup grab is active | `popup_done` in reverse creation order before the camera change; no panic | T1 | no |
| OB-5 | X-7 games / network clients off-screen (DV-1) | vkcube blocked; kick bounds it (EB-9) | core frame policy | Wine/Proton game + a network client off-screen under withhold/kick/P | no disconnect/freeze beyond P; CPU within budget | T5 | no |
| OB-6 | Fractional-camera snapping/rounding (CR-6) | fractional cameras × 4 scales hit-test/render consistent (EC-20); X-6 edges | core projection | T0 properties: edge = round(L); re-projection lands in the same pixel; magnitudes to 2³⁶; random cameras | all hold | T0 | no |
| OB-7 | Downsampling / magnification quality (DV-2) | none (no renderer in harness) | backend renderer filters | visual comparison at m ∈ {0.67, 0.87, 1.04, 1.25, 2.67} | subjective acceptance; text legibility | T3/T4 | no |
| OB-8 | Off-screen fallback period P (provisional 1 s) | EB-9 | configuration | from OB-5 results | as OB-5 | TUNE (after T5) | no |
| OB-9 | xwayland-satellite override-redirect menus/tooltips (C6-A1) | toplevel path verified (EC-12); menus untested | satellite (external) + adapter popup path | X11 app with menus/tooltips (GTK3 `GDK_BACKEND=x11` under satellite in the harness; later Steam) | menus appear attached at the right place, receive input, dismiss correctly | T1 (GTK3/X11 headless) → T3 (Steam) | no |
| OB-10 | RC-1 other toolkits | GTK4 verified (EC-21) | — | Qt6, SDL, Chromium/Electron, Firefox multi-window, Wine | no stale key/modifier; no client error | T3 | no |
| OB-11 | Smithay oneshot-lock defect W-SM-4b | reproduced (X-12) | adapter / upstream | file upstream issue; decide patch vs accept when pointer-lock code is implemented | X-12 reproducer passes (no `unlocked` for a never-active lock) | UP + Phase F decision | no |
| OB-12 | Scale constants H_cfg, r, σ (provisional) | EC-16–19 | configuration | X-16 zoom sweep with real clients + visual | renegotiations bounded per P3; CPU/memory acceptable; sharpness acceptable | TUNE (T3/T4) | no |
| OB-13 | Runtime `D_max` query and enforcement | EC-16 | backend renderer → core ceiling | query from the renderer at startup; assert h ≤ H_max for large windows | no buffer exceeds the device limit | T1 | no |
| OB-14 | Hit-test remaining coverage | see CP3A-2 | core | transforms, zoom ≠ 1, sub-pixel clamp, multi-output spanning, random trees | 0 mismatches vs presented; clamp ≤ ½ px/k | T0 + T1 | no |
| OB-15 | Keyboard residual: compositor-side repeat (K9, unused) and output removal (K8 no-op) | X-10 | core | confirm no repeat events from the compositor; output removal changes no keyboard state | as stated | T1 | no |
| OB-16 | Input delivery as a kick trigger (CR-7) | design (B10.5) | core frame policy | vkcube-class client off-screen + focused; keys delivered | first client response within one frame of the input | T1 (real-time harness) / T3 | no |
| OB-17 | `Window::geometry` re-clamp (W-SM-9) | source | adapter | observe real CSD clients shrinking their tree | anchor stable; no jump | T3 | no |
| OB-18 | `grab_popup` panic precondition (W-SM-7) and `ensure_configured` (W-SM-6) | source | adapter | misbehaving-client tests | protocol error to the client; compositor survives | T1 | no |
| OB-19 | Port the authority checks to the real crates (`verify-architecture`: dependency DAG, probes, `!Send` assertion, source check) | spike PASS (EC-1, EC-15) | workspace tooling | the same script against the real crates | all probes fail for the expected reason; DAG checks pass | Phase F (first commit of the real workspace) | **yes**: required before any protocol feature work |
| OB-20 | Games under KeepFocus pans and preserved locks (DV-3) | X-8 (protocol level) | core input | real games | no camera jumps, no lost locks, no phantom motion | T5 | no |
| OB-21 | libinput device semantics under pan accumulation (DV-4) | none | backend | high-rate mice, touchpads (begin/end for gestures) | camera = start − Q(Σ); no drift | T4 | no |
| OB-22 | Fullscreen acceptance policy (focused-only / pre-map) UX | C5.2 design; satellite pre-map (EC-13) | core protocol policy | games, video players, X11 via satellite | no output seized while the user is elsewhere; games enter fullscreen | T3/T5 | no |
| OB-23 | Discrete-zoom (wheel) decrease oscillation at a rung boundary | MODEL analysis (bounded, ≤ 1 per tick) | core scale policy | wheel-wiggle experiment with real clients | renegotiation rate acceptable or an optional decrease-settle delay configured | TUNE (with X-16) | no |
| OB-24 | Zoom anti-jitter during *animated/moving* zoom (A-B1-1) | static fit unaffected (C5.1) | core projection | decide a rule before zoom animation or zoomed panning ships | no relative jitter between elements while moving at zoom ≠ 1 | decision before the zoom-motion feature (Level C) | no (static fit OK) |
| OB-25 | Real presentation timing (DRM flips, VRR) beyond X-9 | X-9 (virtual) | backend + core clock | DRM backend presentation feedback | `presented` timestamps from flips; clock output = sync_output | T4 | no |
| OB-26 | Regression guard: never use `Window::surface_under`; keep xsnap/xhit/X-15 as tests | EC-9, EC-20 | core + adapter | port xsnap/xhit to T1 | all pass | T1 | no |
| OB-27 | Satellite: activation and primary selection behaviour | warnings observed (EC-12) | adapter globals | offer `xdg_activation_v1` (+ primary selection when added); X11 apps request focus | B10.7 policy applies; no camera move | T3 | no |
| OB-28 | Client resizing in response to scale hints (feedback loop with H_max and with viewport-fit re-fit), RC-2 | design reasoning (CP3A-5) | core scale + fit | misbehaving test client that changes logical size per hint | no unbounded oscillation (hint changes per P3 bounds; re-fit rate ≤ commit rate) | T1 | no |
| OB-29 | Phase C 3B deliverables (C11–C16, final contradiction audit) | — | — | checkpoint 3B | per master program | **GATE-C** | — |

**GATE-C items other than the 3B deliverables: none.** No open item requires an architecture decision before C11–C16. OB-19 is the only item that blocks implementation, and it belongs to the first Phase F commit.

---

## CP3A-5. Authority recheck (zoom in the scale path)

```
Camera (zoom, centre) ──► View(O) ──► ρ_p (derived) ──► ScaleState h (core policy state) ──► Effect::PreferredScale
                                                                                                 │ (adapter)
                                                                                                 ▼
                                                                      wp_fractional_scale / preferred_buffer_scale (protocol state)
                                                                                                 │ (client)
                                                                                                 ▼
                                                                      client buffer (client state; logical size unchanged by contract)
                                                                                                 │ commit
                                                                                                 ▼
                                                     SurfaceTree snapshot (derived) ──► render list / factor m_p ──► renderer
```

| Check | Result |
|---|---|
| Camera owns view/zoom | yes: `navigation` only (C1.5); scale policy reads zoom and never writes it |
| Stage owns persistent geometry | yes: no arrow in the chain writes anchors |
| Scale policy derives requirements | yes: ρ is derived; h is policy state with a single writer (core) |
| Client buffer scale is protocol state | yes: written by the client; the compositor only suggests it |
| Renderer consumes the buffer | yes: m_p is derived per frame |
| Buffer size never becomes Stage authority | yes: the snapshot carries **logical** size (`view().dst`), not buffer size; logical size is client geometry, never an anchor |
| Fullscreen remains presentation state | yes: contributes ρ = scale_O and nothing else (Case C) |
| Viewport-fit remains camera state | yes: zoom comes from `ViewportFit` in `navigation` (Case D) |

**Possible cycles, examined:**

| Cycle | Why it is bounded |
|---|---|
| h → client → logical size → H_max → h | a compliant client keeps its logical size when the scale changes (protocol: the scale affects the buffer, not window geometry). A non-compliant one is bounded by P1–P3 and S6 (no decrease mid-gesture). |
| h → client size → viewport-fit re-fit → zoom → ρ → h | re-fit is triggered by logical geometry changes only. For a compliant client a hint change never triggers it; for a non-compliant one the loop is bounded by rung crossings and limited to the client's own commit rate. |

Both cycles are the OB-28 test target.

**No new authority cycle exists; no scale or render result writes Stage geometry.**

## CP3A-6. Status of earlier text superseded here

| Earlier text | Now |
|---|---|
| C5.1 "REFINEMENT of D-B8-SCALE (needs approval)" | **adopted** as D-CP3A-SCALE S1 (ρ = scale × zoom), with ladder and ceiling |
| C5.2 Case B "Open policy question CP2-Q1" | **resolved**: no fullscreen override (Case C) |
| C3.7 / D-B2-I9 "≤ ½ px agreement bound" | **refined** to exact physical-pixel agreement (CP3A-2) |
| C2.1 `ScaleState` | now holds h, the ladder (derived from configuration + outputs + D_max) and the gesture flag |
