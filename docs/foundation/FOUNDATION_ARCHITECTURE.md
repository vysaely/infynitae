# Infynitae Native — Foundation Architecture (Phase C)

Status: checkpoint 1 (C1–C4) accepted, commit `d596416cf57877116e9cfbc8cf6c6fe25c001c67`. **Phase C COMPLETE and SEALED** (seal commit `615b0ed2f3631b698b0c54783c28f696ef4ff34a`; final gate approved by the user). Approval means the architecture is sufficiently specified to advance, **not** that the compositor is physically validated. The canonical invariant registry is in C12.

Inputs (frozen):
- Phase B seal `4a0807ed721ae257e1f4b0e5287d1dfd8c1038c1`, especially the carry-forward ledger at the end of [FOUNDATION_BLOCKERS.md](FOUNDATION_BLOCKERS.md).
- Decision IDs (`D-B…`), ledger categories (`CR-n` conditional obligations, `DV-n` deferred validation, `W-SM-n` Smithay quirks) and evidence IDs (`EV-`, `EB-`) refer to that document and to [FOUNDATION_EVIDENCE.md](FOUNDATION_EVIDENCE.md).

> **Effective package (Phase D D7, added as a pointer; historical text below is unchanged):** namespace authority map §P0, formula registry §F and provisional registry §PROV are at the end of this document. Effective invariants: [FOUNDATION_INVARIANTS.md](FOUNDATION_INVARIANTS.md).

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

---
---

# Phase C checkpoint 3B (final): C11–C16, canonical registry, final audit, Phase C gate

Checkpoint 3A accepted: `795800937a1e50e56587cd4080a73ce27d8a8b01`.

## CP3B evidence

| ID | What | Where |
|---|---|---|
| EC-22 | **Feedback MODEL** (not an experiment): client-geometry/scale loops under viewport-fit, five client behaviours; boundary wiggle. Benign clients converge under any policy (1–4 hint changes). **An adversarial client that inverts every move oscillates forever under naive settle-every-time (60 changes in 60 rounds); under epoch monotonicity (SC-E): 1 change.** Wheel wiggle across a rung boundary: naive 40 changes / 40 ticks; discrete-zoom interaction (SC-D): 2. | `experiments/cp3a-scale-model/feedback.py`; [`evidence/phase-c/cp3b-feedback-model-output.txt`](evidence/phase-c/cp3b-feedback-model-output.txt) |
| EC-23 | Target hardware for budgets: AMD Ryzen 5 7600X3D, 30 GiB RAM, AMD Radeon RX 9070 XT (RADV GFX1201), Mesa 26.2.3, Linux 7.2.8 | `/proc/cpuinfo`, `free`, vulkaninfo, EB-9 |

---

## C11 — Verification hierarchy

### C11.1 Tiers

| Tier | Environment | Simulated | Real | Invariants covered | Required tests (minimum) | Cost | Trigger | Retained evidence | A failure means |
|---|---|---|---|---|---|---|---|---|---|
| **T0** | `cargo test` on `infynitae-core` (no Smithay, no runtime) + `verify-architecture` (build probes, dependency checks) | everything outside core | core logic, compiler, Cargo graph | COORD, CAMERA (authority), STAGE (identity/lifecycle state machine), SCALE (policy), FRAME (policy decisions), INPUT (ownership state machine), ARCH | numeric properties (B1 E1–E9 as properties); lattice/edge properties to 2³⁶; projection↔unprojection; ID never reused; lifecycle transition table; MaxGesture/ladder/epoch/DZI properties (EC-19, EC-22 as property tests); ownership table; keyboard K-rules at state level; compile-fail probes with asserted diagnostics; `!Send` port probe; dependency DAG | seconds | every commit (CI) | test log, probe diagnostics | a domain rule or an authority boundary is broken; **blocks merge** |
| **T1** | lockstep headless: production core + production Smithay adapter + headless backend (virtual outputs, virtual vblank, injected `PhysicalEvent`s), recording probe clients over socketpairs (Phase B/C harness pattern, re-expressed against the real seam, NB-12) | time, outputs, devices, rendering (DummyRenderer) | Smithay protocol handling, real `wl_*`/`xdg_*`/`wp_*` traffic, client-observed events | RENDER (order, edges), HIT, membership, INPUT (pan, keys, constraints), FRAME (callbacks, presentation), PROTOCOL (configures, I3′), SHELL (IPC against core), FAIL (client death, output removal) | ports of SC1–SC4, X5, X-6, X-8, X-9, X-10, X-12, xsnap, xhit; OB-1/2/3/4/13/14/15/18/26/28; adversarial misbehaving clients | seconds–a minute | every commit (CI) | client protocol traces, merged logs | protocol behaviour or ordering is wrong; **blocks merge** |
| **T2** | nested: Infynitae on the winit backend inside the developer's session; scripted probe clients | the output (a window), input via host | GL/EGL renderer, real frame timing of the host, real buffers | RENDER on a real GPU, damage, FRAME timing, basic input | smoke: map/render/interact, pan, multi-window, unmap; renderer filters exercised | minutes | on demand / pre-release | screenshots, traces | renderer/backend integration is wrong; does not prove DRM behaviour |
| **T3** | real toolkits/applications against the headless (T1-style, real time) or nested backend: GTK3/4, Qt6, Firefox, Chromium/Electron, kitty, SDL, xwayland-satellite + X11 apps | outputs (headless) or host (nested) | toolkit behaviour | toolkit compatibility of INPUT (RC-1, keys), SCALE (client response), FRAME (off-screen behaviour), XWAYLAND, fullscreen policy, CSD geometry | OB-9, OB-10, OB-16, OB-17, OB-22 (non-game), OB-27, X-16 (client side), DV-2 visual | minutes–hours | manual / scheduled pre-release | client logs, `WAYLAND_DEBUG` traces, CPU/memory | a client class misbehaves; may yield workarounds, never a change of invariants without review |
| **T4** | physical: DRM/KMS + libinput + real monitors (target: 7600X3D, RX 9070 XT, 165 Hz and mixed-DPI outputs) | nothing | everything | presentation timing, VRR/flip feedback, real input devices, multi-monitor hotplug, measured performance and idle | OB-6 on real outputs, OB-7, OB-21, OB-25, C14 measurements, DV-4, hotplug | hours | manual pre-release | measurements, traces, photos/video where visual | physical assumptions are wrong; feeds TUNE |
| **T5** | games, latency, long-duration, adversarial stress on T4 hardware | nothing | everything | DV-1/3, X-7, input-to-photon latency, 24 h stability, memory growth, fuzzed clients | OB-5, OB-8, OB-20, OB-22 (games), long-session C14 | hours–days | manual milestone | measurements, crash dumps | the product is not daily-drivable; may change provisional values |

**Tier honesty rule.** A tier may only claim what it physically represents:
- T0/T1 cannot claim GPU, timing, visual or device behaviour;
- T1 cannot claim toolkit behaviour;
- T2 cannot claim DRM/KMS behaviour;
- T3 results are per-toolkit-version, never universal.

Every evidence record states its tier.

### C11.2 OB-1 … OB-29 destinations (all mapped)

| Tier / destination | Obligations |
|---|---|
| T0 | OB-2 (reference region evaluator), OB-6, OB-14 (transforms, zoom ≠ 1, clamp), OB-12 property part (bounds/monotonicity), OB-23 property part, OB-28 property part, OB-19 (as the Phase F entry gate, below) |
| T1 | OB-1, OB-2, OB-3, OB-4, OB-9 (GTK3/X11 menus via satellite headless), OB-13, OB-14, OB-15, OB-16 (real-time harness), OB-18, OB-26, OB-28 (adversarial client) |
| T2 | renderer-filter smoke for OB-7 |
| T3 | OB-7 (visual), OB-9 (Steam), OB-10, OB-16 (toolkits), OB-17, OB-22 (apps/video), OB-27, OB-12/X-16 client measurements |
| T4 | OB-6 (physical outputs), OB-7, OB-21, OB-25 |
| T5 | OB-5, OB-20, OB-22 (games) |
| UP (upstream) | OB-11 (Smithay issue; patch decision in Phase F) |
| TUNE | OB-8, OB-12, OB-23 |
| Decision before the zoom-motion feature | OB-24 |
| **Phase F entry gate (unconditional)** | **OB-19** |
| GATE-C | OB-29 (this checkpoint) |

### C11.3 Negative controls that stay in verification

A negative control proves a test can see the failure its invariant prevents. Each is kept because the failure is real and cheap to reproduce:

| Invariant | Negative control | Expected outcome | Tier | Status |
|---|---|---|---|---|
| INV-INPUT-6 (modifier keys never compositor-owned) | K3 disabled (`k3_enforced = false`) | client left with a stuck modifier | T1 | **exists** (X-10 NEG) |
| INV-CAMERA-1 / INV-ARCH-2 | forge a port; call input without a port; mutate through the read accessor; reach navigation internals; store a port in protocol state | compile failure with the asserted diagnostic (E0624, private-field, E0061, E0603, E0277) | T0 | **exists** (spike `verify.sh`; ported by OB-19) |
| INV-ARCH-1 | add a forbidden dependency edge (core → smithay; wayland → backend) in a probe manifest | `verify-architecture` fails | T0 | to add with OB-19 |
| INV-SCALE-4 | ladder and epoch rule disabled (naive settle) | churn: 313 renegotiations in Case E; 60/60 adversarial oscillation; 40/40 wiggle | T0 (model as property test) / T1 (real clients) | **exists** (EC-19, EC-22) |
| INV-RENDER-1 / INV-HIT-1 | i32 root location; canvas-exact hit rule | 72,242 edge mismatches (X-6); 399–617 hit mismatches (xhit) | T1 | **exists** |
| INV-INPUT-3 | deliver pan motion to the client (bypass the interaction) | client receives motion/relative during the pan; INV-1 check fails | T1 | to add (one-line switch in the X-8 port) |
| INV-FRAME-3 (no pure starvation) | kick and fallback disabled with a Mesa-WSI-FIFO client off-screen | ping unanswered until re-entry (vkcube: 2–4 s) | T3 (needs a real Vulkan client) | **exists** (EB-9 Starve run) |
| INV-RENDER-4 (own membership) | use `Window::output_enter` for popups | wrong popup enter/leave (X5) | T1 | **exists** |
| INV-INPUT-9 (W-SM-4a) | explicit deactivate before a focus change | duplicate `unlocked` | T1 | **exists** (X-8 C4 history) |

Not kept as negative controls: things that would only inflate counts (e.g. "remove the Stage": nothing compiles).

---

## C12 — Canonical invariant registry

> **Phase D 1R amendment:** the rows INV-CAMERA-1, INV-CAMERA-5, INV-COORD-2, INV-COORD-3, INV-COORD-5, INV-INPUT-5, INV-INPUT-8, INV-SCALE-2…5, INV-FRAME-6, INV-PRES-1 and INV-OUTPUT-1 are **superseded by §C12.R1** (end of this document), which also adds INV-INPUT-10 and INV-PROTO-1. The table below is kept unaltered as the sealed Phase C registry (`615b0ed`). **Phase D 2R:** §C12.R2 further supersedes INV-CAMERA-1, INV-CAMERA-3, INV-OUTPUT-1, INV-SCALE-3 and INV-SCALE-4, and adds INV-OUTPUT-2 (57 entries).

**This table is authoritative.** Legacy identifiers map to it (C12.2). Where a rule was refined, the earlier wording is superseded and must not be cited as normative.

Evidence class: P = protocol, S = Smithay source, E = experiment, M = executable model, D = decision.

| ID | Statement | Authority / owner | Class | Evidence | Enforcing tests (tier) | Violation means |
|---|---|---|---|---|---|---|
| **INV-STAGE-1** | Stage is the **only** store and writer of persistent placement (anchors) and z-order. Writers: `stage::place` via (a) placement policy at initial commit, (b) interactive drag commit, (c) shell `MoveElement` (same path). | core `Stage` | D | B2–B5, C2 | T0 single-writer API tests; T1 INV-3 (X-8) | second spatial authority (Level A) |
| **INV-STAGE-2** | An element's anchor is the Canvas position of its xdg window-geometry origin; all surface origins derive from it via one function (INV-COORD-6) | core | P+S+D | B2.3 | T1 xsnap/xhit | render/hit/membership disagreement |
| **INV-STAGE-3** | `ElementId` = `NonZeroU64`, monotonic, never reused within a process; external references are `(InstanceId, ElementId)` and resolve or fail with `StaleElement` | core `Stage` | D | B4 | T0 property; T1 shell stale-ID | ABA aliasing |
| **INV-STAGE-4** | Lifecycle follows B5's state/transition table; destroy handling is idempotent and order-independent (client death destroys objects in arbitrary order) | core + adapter | S+D | B5, EB-5 | T0 state machine; T1 crash tests | stale relationships, panics |
| **INV-STAGE-5** | Unmap retains `ElementId` and anchor; remap re-shows at the anchor on top of z-order | core | D | D-B5-REMAP | T1 | identity loss on remap |
| **INV-STAGE-6** | No projected/rendered/derived value is ever written back to Stage, cameras or snapshots | all | D | B1 F1, C2 | T0 (no API), review | derived state becomes authority |
| **INV-CAMERA-1** | Camera state is mutated only inside `core::navigation`, reachable only with `InputPort` (physical input) or `ShellPort` (shell); ports are unforgeable, `!Send`, bound to their Core and never stored in protocol state | core `navigation` | E+D | C1.5, EC-1, EC-15 | T0 probes (OB-19) | clients can move cameras |
| **INV-CAMERA-2** | A camera change never changes any element's Canvas geometry | core | D+E | I2, X-8 INV-3 | T0, T1 | cameras become spatial authority |
| **INV-CAMERA-3** | Cameras are centre-anchored views (centre Q16, zoom); per-output view origin `v_O = centre − mode/(2k)` (R7); cameras survive output removal; N outputs may share one camera | core `navigation` | D | D-B7-CENTER, B7.2 | T0, T1 hotplug | lost views, wrong projection |
| **INV-CAMERA-4 (I3′)** | Camera change alone causes no configure (size, state incl. `suspended`, bounds, popup position) and delivers no pointer/relative/button event from the pan gesture. It MAY cause enter/leave, scale changes per INV-SCALE-*, clock changes, presentation outcomes, and after termination one true-position pointer event. | core | P+E | B9.7, SC1, SC4, X-8 | T1 (client-observed configure count = 0 during pan) | pan visible to clients as geometry/input |
| **INV-CAMERA-5** | Viewport-fit is a camera operation (centre + zoom on one output; optional isolation) saved/restored in `navigation`; it never changes an element's size or anchor | core `navigation` | D | C5.1 | T0, T1 | fit rewrites geometry |
| **INV-COORD-1** | Canvas coordinates are Q16 `i64` (2⁻¹⁶ px), \|v\| ≤ 2³⁶; anchors and surface origins are integral | core | E | B1 (E1–E9) | T0 | precision collapse, path dependence |
| **INV-COORD-2** | Rounding happens only at R1–R7 (R7 = view-origin quantisation); F1–F7 are forbidden | core | E+D | B1.4, C3.2 | T0 | jitter, drift |
| **INV-COORD-3** | Gesture deltas accumulate in f64 from the gesture origin and are quantised once per update, never per event | core | E | B1 E2b, X-8 | T0 | drift (6.1 px / 1e6 events) |
| **INV-COORD-4** | Coordinate domains are distinct Rust types with no cross-domain arithmetic; Smithay geometry types are constructed only in the adapter's `convert` module | core / adapter | D | B2.7, C3.6 | T0 compile probes | domain mix-ups |
| **INV-COORD-5** | Smithay's pointer "global space" is Canvas. The authoritative pointer is `Free(OutputId, OutputLogical)` or `Locked(SurfaceKey, Canvas lock point)`; Smithay's location is derived per dispatch. | core `PointerModel` | S+E | B2.5, B10.2, X-8 | T1 | wrong client coordinates (Prototype 01 bug) |
| **INV-COORD-6** | One surface-origin function (anchor − geometry.loc + subsurface/popup offsets) is the only source of surface Canvas origins for render, membership, hit-test and input | core | E | CR-1, EC-9, EC-20 | T1 xsnap/xhit | render/input disagreement |
| **INV-RENDER-1** | Physical edges: `edge(x) = round((x − R)·k + δ) − C_O` with R a 120-lattice reference, δ = 2⁻²⁰, k = zoom·scale; surfaces are built with `from_surface` at f64 location `(x − R)·k + δ − C_O` | core (numbers) / adapter (Smithay call) | E | B1.5, X-6 | T1 X-6 port; T0 properties | gaps, jitter, scale-dependent seams |
| **INV-RENDER-2** | Render order front-to-back: reverse Stage z-order; within an element each popup tree in `popups_for_surface` order, then the root tree, each traversed downward | core (from snapshot order) | E | X-15 | T1 | wrong stacking |
| **INV-RENDER-3** | Output membership (`wl_surface.enter/leave`) is computed by Infynitae per surface from physical-pixel projection and applied via `Output::enter/leave`; leave is sent before an output global is destroyed; `Window::output_enter`/`Space` are not used | core + adapter | P+E | D-B8-ENTER, C3.2[3], X5 | T1 | wrong enter/leave, wrong scale |
| **INV-RENDER-4** | No spatial index unless C3.8's measured threshold is met; if introduced, it is derived from Stage and differentially tested | core | D | C3.8 | review + differential T0 | secondary authority |
| **INV-HIT-1** | Hit-testing selects the surface **presented** at the pointer's physical pixel (same edges/order/`c_eff` as rendering) whose input region (protocol add/subtract, half-open on the clamped surface-local point) contains the point; delivered coordinates are `q − origin` clamped into the surface. `Window::surface_under` is not used. **Supersedes D-B2-I9's ½-px tolerance.** | core | E+P | EC-20, W-SM-11/12 | T1 xhit; T0 region evaluator | clicks land on hidden surfaces |
| **INV-INPUT-1** | Every physical event has exactly one owner, decided at press (before any client dispatch); releases follow the press owner; motion follows the active interaction | core intake | E | B10.3, X-8, X-10 | T1 | stuck buttons/keys, leaks |
| **INV-INPUT-2** | A compositor pointer interaction is acquired only when the client owns no pressed button; protocol move/resize/drag are the only client→compositor transfers | core | P+E | B10.3, X-8 C6 | T1 | unmatched client button state |
| **INV-INPUT-3** | While a compositor interaction owns the pointer, no absolute motion, relative motion or button event reaches any client; pointer focus is kept (KeepFocus) | core + adapter | E | X-8 (48 checks), D-B11-FOCUS | T1 (+ negative control) | game input leakage, lost locks |
| **INV-INPUT-4** | Relative motion is emitted only from physical deltas on the client path; nothing is replayed, synthesised or derived from camera changes | core | E | X-8 INV-4/5 | T1 | phantom motion |
| **INV-INPUT-5** | Pointer constraints: activation is compositor policy (focused + inside region; never during pan acquisition); lock reconciliation at pan end keeps the Canvas lock point if it is observable on the pointer's output, else deactivates; on output removal the lock is deactivated and the pointer moves to the Arrangement-nearest output; W-SM-4a ordering (explicit deactivate only without a following focus change) | core + adapter | E+D | B11.4, X-8 C3/C4, AUD-4 | T1 | lost/duplicate lock events |
| **INV-INPUT-6** | Keys whose press changes XKB modifier/lock state are never compositor-owned (bindings on them run in observe mode) | core intake | E | K3, X-10 + NEG | T1 (+ negative control) | stuck modifiers |
| **INV-INPUT-7** | Focus transitions use protocol semantics only (`leave` resets, `enter(keys)` + `modifiers`); no synthetic key events; compositor-owned keys are absent from `enter` and their releases are consumed; client-side destruction of the focused surface → successor `enter` without `leave` | adapter (Smithay seat) + core | P+S+E | K4–K9, X-10, EC-21 | T1; T3 (OB-10) | logically impossible key state |
| **INV-INPUT-8** | Keyboard focus changes only by explicit action (click-to-focus, focus bindings, shell focus command, activation grant) or target loss; camera, visibility and pointer motion never change it | core `FocusModel` | D | D-B10-KBD | T0, T1 | typing redirected by panning |
| **INV-INPUT-9** | xdg-activation tokens are validated by provenance (W-SM-8); activation yields focus grant, attention or nothing; never a camera change | core `protocol` | D+E | D-B10-ACTIVATION, EC-1 | T0 probes, T1 | focus stealing, camera hijack |
| **INV-SCALE-1** | ρ(E) = max over observing presentations of `output_scale × presentation_zoom`; the protocol-fullscreen presentation contributes `scale_O` (zoom 1); no fullscreen override | core visibility | D+M | S1, CP2-Q1, Case C | T0 model properties | blurry or overspent buffers |
| **INV-SCALE-2** | The client hint h belongs to a finite ladder containing every output scale and is ≤ `H_max(E) = min(H_cfg, D_max / max logical side)`, with `D_max` queried from the renderer; if E's size makes h exceed the device bound, h is lowered immediately | core `ScaleState` | E+M | S2, S3, P1, P2, P7, EC-16, EC-18 | T0; T1 (OB-13) | unrenderable/huge buffers |
| **INV-SCALE-3** | Increases happen only when the target rung exceeds h (rung crossings); decreases only when no camera gesture **and** no discrete-zoom interaction is active | core | M | S5, S6, SC-D | T0 properties | per-event renegotiation |
| **INV-SCALE-4 (churn contract)** | (a) no hint change per raw zoom event; (b) changes only at rung crossings or settlement; (c) per gesture/interaction: ≤ (rungs crossed) + 1; (d) no oscillation: within an epoch, re-evaluations caused only by client responses move h monotonically in the epoch's first direction (SC-E); a reversal needs a new non-client stimulus | core | M+E | P3, EC-17, EC-19, EC-22 | T0 properties (+ negative control); T1 OB-28 adversarial client | scale storms (EC-17: up to 477% client CPU) |
| **INV-SCALE-5** | At zoom 1 with no zoom gestures the policy equals Phase B MaxGesture; integer hint = ⌈h⌉; emission only on change; unobserved elements hold h; evaluation happens once per core event batch, after all state updates of that batch | core | M | P4, S7, S8, AUD-25 | T0 | regressions vs Phase B, double renegotiation |
| **INV-FRAME-1** | The compositor only completes frame callbacks clients requested; it never invents them | core + adapter | P | D-B9-FRAMES 1 | T1 | protocol violation |
| **INV-FRAME-2** | Observed elements: completion once per frame of the clock output = highest-refresh observing output (ties by `OutputId`); presentation `sync_output` = clock output | core | E+P | D-B8-CLOCK, SC3 | T1 | double-rate or starved animation |
| **INV-FRAME-3** | Unobserved elements: callbacks withheld, except (a) the **kick**: before a ping, close, non-initial configure, `popup_done` or input event is delivered to the client, withheld callbacks complete once; (b) the **fallback**: at most one completion per period P (finite, configurable) measured from the element's last completion (kick or fallback), driven by a compositor timer armed only while such an element has a pending callback | core | E | EB-9, D-B9-FRAMES, AUD-24 | T1/T3 (+ negative control) | starvation (vkcube) or wasted wakeups |
| **INV-FRAME-4** | Re-entry liveness: pending callbacks complete at the first vblank of the new clock output after observation resumes (≤ one period) | core | S+E | EB-11, SC1/SC3 | T1 | stuck clients after re-entry |
| **INV-FRAME-5** | Presentation feedback: superseded commits `discarded`; the latest unobserved commit stays pending and is `presented` when shown, or discarded on supersession/destruction | Smithay mechanism + core | S+E | X-9 | T1 | wrong timing feedback |
| **INV-FRAME-6** | `suspended` is set only from semantic reasons {SessionLocked, AllObservingOutputsOff, Parked}; never from camera visibility | core | E+D | D-B9-SUSPEND, SC4 | T1 | configure storms |
| **INV-PRES-1** | Protocol fullscreen is an output presentation state (`OutputPresentation(O) = Exclusive(W)`), accepted only for the focused element or before first map; it never touches cameras or anchors; other cameras observe W at its actual (fullscreen) size; output removal/unmap ends it | core `protocol` + output model | P+D | C5.2, EC-13, EC-14 | T1 | camera hijack, hidden size effects |
| **INV-OUTPUT-1** | Output removal: membership `leave` before the global is destroyed; camera unbound but kept; fullscreen presentation dropped; interactions orphaned (ownership kept until release); pointer moved to a remaining output | core output manager | D | B7.2, C4.6, C8 | T1 (OB-3) | dangling protocol objects, stuck input |
| **INV-SHELL-1** | The shell channel accepts only semantic commands executed through the same validated single-writer paths as user input; no raw state, size or memory writes; `ShellPort` lives only in `infynitae-ipc` | core + ipc | D | C7 | T0 (crate checks), T1 | shell becomes arbitrary mutator |
| **INV-SHELL-2** | The shell owns no authoritative state; every response/event is a generation-stamped derived copy; stale IDs fail; reconnect = Snapshot + Subscribe | core + ipc | D | C7.3 | T1 | shell restart loses truth |
| **INV-XWAYLAND-1** | X11 support is xwayland-satellite as an ordinary Wayland client: X11 windows are ordinary elements; no X11 path to Stage or cameras; satellite crash destroys its elements through the normal client-death path; no internal XWM | adapter + supervisor | E | C6, EC-12, EC-13 | T1/T3 | second spatial authority |
| **INV-ARCH-1** | Crate DAG: core has zero dependencies; all edges point to core; wayland ↛ backend; ipc ↛ wayland; backends ↛ each other | workspace | E+D | C1.3, EC-1 | T0 `verify-architecture` (OB-19) | layering collapse |
| **INV-ARCH-2** | `unsafe`: forbidden in core and ipc; elsewhere only listed, justified sites with no authority effect | workspace | D | C9 | T0 lint + inventory review | memory/authority bypass |
| **INV-ARCH-3** | Smithay exact pin, locked graph; workarounds adapter-local; a dependency patch only per the patch policy; a workaround is removed only after its experiment passes without it | workspace + adapter | D | D-B12-PIN, C9 | T0 `--locked`; upgrade re-run suite | silent behaviour drift |
| **INV-FAIL-1** | Recovery rebuilds derived state from authority; an internal invariant violation terminates rather than repairs; required-global failure aborts startup | core + bin | D | C8 | T1 failure injection | corrupt authority hidden by best effort |
| **INV-OBS-1** | Observability is never read by decision paths and runs no background subsystem | all | D | C10 | review; T0 (no read API) | heisenbugs, idle cost |
| **INV-IDLE-1** | When product-idle (C14.2), Infynitae generates no wakeups of its own: no polling, no periodic timers, no busy loops; every armed timer has a pending obligation behind it | core + bin + backend | D | C14.2 | T4 idle measurement | battery/latency regressions |
| **INV-ALLOC-1** | Core hot paths (input intake, pan, pointer routing, hit-testing, visibility, render-list construction) perform no heap allocation in steady state (reused buffers); Smithay/backend allocations are isolated and measured | core | D | C14.3 | T0 counting-allocator test | frame-time jitter |

### C12.2 Legacy → canonical map (superseded wording must not be cited)

| Legacy | Canonical | Note |
|---|---|---|
| I1 single spatial authority | INV-STAGE-1 | |
| I2 camera ≠ window movement | INV-CAMERA-2 | |
| I3 pan does not configure | INV-CAMERA-4 | **superseded by I3′** |
| I4 outputs view Stage | INV-STAGE-1, INV-CAMERA-3 | |
| I5 multiple outputs may view one element | INV-RENDER-3, INV-SCALE-1, INV-FRAME-2 | |
| I6 explicit coordinate domains | INV-COORD-4 | |
| I7 stable runtime identity | INV-STAGE-3 | |
| I8 presentation is derived | INV-STAGE-6 | |
| I9 render and hit-test share one model | INV-HIT-1, INV-COORD-6 | **D-B2-I9's ½-px tolerance superseded** by exact pixel agreement |
| I10 clients are untrusted | INV-STAGE-4, INV-FAIL-1, INV-CAMERA-1, INV-SCALE-4(d) | |
| R1–R6 (B1.4), R7 (C3.2) | INV-COORD-2 | |
| F1–F7 | INV-COORD-2, INV-STAGE-6 | |
| B1.5 lattice rule | INV-RENDER-1 | |
| K1, K2 | INV-INPUT-1 | |
| K3 | INV-INPUT-6 | |
| K4–K7, K9 | INV-INPUT-7 | |
| K8 | INV-INPUT-8, INV-OUTPUT-1 | |
| X-8 INV-1 … INV-5 | INV-INPUT-3, INV-INPUT-2, INV-CAMERA-2, INV-INPUT-4 | |
| D-B8-SCALE (MaxGesture) | INV-SCALE-1, -3, -5 | **superseded wording**: "max(scale(O))" → `scale × zoom` with ladder/ceiling |
| D-CP3A-SCALE S1–S9, P1–P7 | INV-SCALE-1 … -5 | |
| D-B8-CLOCK | INV-FRAME-2 | |
| D-B8-ENTER | INV-RENDER-3 | |
| D-B9-FRAMES | INV-FRAME-1, -3, -4 | kick list extended (input); fallback reset rule added (AUD-24) |
| D-B9-SUSPEND | INV-FRAME-6 | |
| D-B10-INTAKE | INV-INPUT-1, -2 (+ -6 for keys) | |
| D-B10-KBD | INV-INPUT-8 | |
| D-B10-ACTIVATION | INV-INPUT-9, INV-CAMERA-1 | |
| D-B11-PAN / D-B11-FOCUS | INV-INPUT-3, INV-COORD-3 | |
| B11.4 reconciliation | INV-INPUT-5 | output-removal case added (AUD-4) |
| D-C5-FIT | INV-CAMERA-5 | |
| D-C5-FS | INV-PRES-1 | |
| D-C6 | INV-XWAYLAND-1 | |
| D-C7 | INV-SHELL-1, -2 | |
| D-C8 | INV-FAIL-1 | |
| C9 | INV-ARCH-2, -3 | |
| D-C10 | INV-OBS-1 | |
| D-CP2-HIT / D-CP3A-HIT | INV-HIT-1 | |

---

## C13 — Implementation-readiness audit (zero-memory reader)

Every implementation-critical question below has a normative answer in this document or in FOUNDATION_BLOCKERS.md. Evidence logs are cited, never the only source.

| Question | Normative answer (canonical) | Where |
|---|---|---|
| Owner of every important state; who may mutate it | C2.1, C2.2, **C-AUTH** | this doc |
| Dependency direction | INV-ARCH-1; C1.2–C1.4 | C1 |
| Coordinate domains, numeric types | INV-COORD-1…4; B2.1–B2.2 (T1–T10), B2.7 | Blockers B1/B2 |
| Stage/Camera algebra, projection, lattice rounding | INV-CAMERA-3, INV-COORD-2, INV-RENDER-1; C3.2 [1]–[5] | C3 |
| Surface origin | INV-STAGE-2, INV-COORD-6; B2.3, B6.1, CP2-2 | |
| Lifecycle | INV-STAGE-4/5; B5.2–B5.4 | Blockers B5 |
| Popups/subsurfaces | B6; INV-RENDER-2; CP2-2 | |
| Rendering | C3; INV-RENDER-1…4 | |
| Hit-testing | INV-HIT-1; CP3A-2 | |
| Output membership | INV-RENDER-3; C3.2 [3] | |
| Input ownership, keyboard/pointer focus | INV-INPUT-1…8; C4 | |
| Pointer constraints, relative motion | INV-INPUT-4/5; C4.3; B11.4 | |
| Scale/zoom | INV-SCALE-1…5; CP3A-1; C14.4 (SC-E, SC-D) | |
| Fullscreen / viewport-fit | INV-PRES-1 / INV-CAMERA-5; C5 | |
| Frame pacing, suspension | INV-FRAME-1…6; B9 | |
| Activation | INV-INPUT-9; B10.7; C1.5 | |
| Shell authority | INV-SHELL-1/2; C7 | |
| XWayland | INV-XWAYLAND-1; C6 | |
| Failure recovery | INV-FAIL-1; C8 | |
| Unsafe / dependency patching | INV-ARCH-2/3; C9; D-B12-PIN | |
| Observability | INV-OBS-1; C10 | |
| Verification tiers | C11 | |
| Provisional constants | C14.5 (single list) | |
| Deferred physical validation | C11.2, C-PHYS | |

**Rules promoted into normative text during 3B.** They previously existed only as analysis, evidence or implication:

| Rule | Promoted to |
|---|---|
| epoch monotonicity (SC-E) | C14.4 / INV-SCALE-4(d) |
| discrete-zoom interaction (SC-D) | C14.4 / INV-SCALE-3 |
| the P7 lock | C14.4 |
| fallback measured from the last completion | INV-FRAME-3 |
| scale evaluation once per event batch | INV-SCALE-5 |
| lock reconciliation on output removal | INV-INPUT-5 |
| complete kick trigger list | INV-FRAME-3 |
| required Smithay overrides W-SM-13 | C15 |
| idle and allocation contracts | C14.2/C14.3 |

**Gaps found: none remaining.** Each 3B scenario that exposed an under-specified rule is listed in AUD with its resolution.

---

## C14 — Performance and resource architecture

### C14.1 Methodology (no invented budgets)

Physical facts:
- 165 Hz → 6.061 ms per frame; 144 Hz → 6.944 ms per frame (the B8 outputs);
- the target hardware is EC-23.

There is no production code yet, so production baselines do not exist. Harness numbers are **not** baselines for the compositor. Each budget is frozen only after its T4 baseline exists, using the procedure below.

| Hot path | Metric | Measurement boundary | Current evidence | Desired headroom (principle) | Candidate budget | Evidence required before freezing |
|---|---|---|---|---|---|---|
| input intake | latency p50/p99/p99.9 per event | libinput fd readable → effects applied to the seat | none (no production code) | intake + routing must not delay the next frame's composition | set from T4 baseline | 10⁶-event replay, 1000 Hz mouse, T4 |
| hit-testing | time per query vs element/surface count | `hit_test(ProjectionFrame, OutputId, p)` | xhit correctness only | negligible vs intake | from T0 bench with realistic trees (100/1000 elements) | T0 micro + T4 in context |
| camera transformation | time per camera change | `navigation::pan` → new `View`s | spike: O(1) arithmetic (8 ns micro, **not** compositor performance) | — | none needed unless T4 shows otherwise | T4 profile |
| projection + visibility/culling | time per frame per output vs E | C3.2 [1]–[3] | none | C3.8 threshold (10% of shortest frame) triggers index work | from T4 | T4 with 10/100/1000 elements |
| render-element generation | time + allocations per frame | C3.2 [4]–[5] incl. `from_surface` | none | — | from T4 | T4 profile |
| renderer/GPU submission | CPU time + GPU time per frame | damage tracker → submit | none | frame must complete before the next vblank with margin for client commits | from T4 | GPU timestamps, T4 |
| shell IPC | command latency; events/s | socket read → response written | none | must not block frame composition | from T4 | T4 with a scripted shell |
| input-to-presentation latency | ms, p50/p99 | physical event timestamp → presentation feedback timestamp of the frame showing its effect (software proxy); photodiode for absolute | none | as low as the frame clock allows (≤ ~1 frame + scanout is the physical floor) | none frozen | T4 software proxy; T5 photodiode/LDAT |
| frame kick | latency from trigger to callback | protocol request / input queued → `wl_callback.done` sent | harness: ping answered in the same dispatch (EB-9) | ≤ 1 dispatch | **established qualitatively** (same dispatch) | T1 assertion |
| scale-policy processing | time per batch; renegotiations per gesture | INV-SCALE-5 evaluation point | MODEL (EC-19/22) | negligible | — | T0 bench |
| memory per element/surface | heap bytes core + adapter | counting allocator in T1 with N elements | none | linear, small constant | from T1 | T1 with 1000 elements |
| client-buffer consequences | MB per element vs hint | EC-18 formula | EC-18 (computed), EC-17 (measured) | governed by H_cfg (provisional) | — | X-16 |
| long-session memory stability | RSS slope over 24 h with churn | process RSS + GPU memory (amdgpu fdinfo) | none | zero growth after warm-up | slope ≈ 0 (statistical) | T5 24 h run |

### C14.2 Idle contract (INV-IDLE-1)

**Product-idle** means all of the following hold:
- no input events;
- no client commits;
- no observed element with a pending frame callback;
- no unobserved element with a pending callback whose fallback is due;
- no active interaction or animation;
- no IPC traffic;
- no hotplug.

**Wakeups Infynitae may generate, each only while its obligation exists:**

| Wakeup source | Armed only while |
|---|---|
| frame scheduling (vblank/flip events requested from DRM, or a frame timer) | damage exists or an observed element waits for a callback. **Never** a continuous redraw loop (Prototype 01 violated this, EV-P14). |
| fallback timer (P) | at least one unobserved element has a pending callback |
| animation timers | an animation is running |
| satellite restart backoff | a restart is pending |
| per-device/protocol timeouts Smithay arms internally | those are external dependency behaviour, attributed and measured, not assumed zero |

**External wakeups (outside Infynitae's control):**
- client requests;
- kernel input, udev and DRM events Infynitae did not request;
- Mesa/driver internal threads;
- systemd/D-Bus if used by dependencies.

**Proof procedure (T4).**
1. Idle session (compositor + N idle clients) for 60 s.
2. Count compositor wakeups (`/proc/<pid>/status` voluntary context switches; `perf trace -s` / `strace -c` on epoll and timerfd).
3. Attribute each wakeup via `inf::*` tracing at DEBUG to its source.
4. **Pass:** zero wakeups attributable to Infynitae-generated sources while product-idle. External wakeups are reported but not counted against the invariant.

A literal "0 wakeups" or "0.0% CPU" is not claimed: kernel/driver bookkeeping may wake the process.

### C14.3 Hot-loop allocation contract (INV-ALLOC-1)

| Path | Structural guarantee | Enforcement |
|---|---|---|
| pointer intake/routing, camera pan, keyboard intake | core APIs write into caller-owned reusable buffers (`&mut EffectBuf`, cleared not freed); no `Vec` returned per event (REFINEMENT: C1's `Vec<Effect>` return type becomes a caller-provided buffer) | **T0**: a test-only counting global allocator around a scripted steady-state pan/route/key sequence after warm-up must report **0** allocations |
| hit-testing | iterates `ProjectionFrame` in place | same T0 test |
| visibility, membership diff, render-list construction (core part) | per-output buffers reused frame to frame; membership diffs written into a reused buffer | same T0 test |
| Smithay render-element construction, damage tracking, protocol sends | **may allocate** (Smithay API) | measured: allocations per frame reported in T1/T4; regressions flagged, not forbidden |

No object pools or custom production allocators are introduced; the counting allocator exists only in tests.

### C14.4 Scale-churn contract and feedback damping (INV-SCALE-3/4; resolves OB-28 at the architecture level)

**SC-D: discrete-zoom interaction.**
- Begins at the first discrete zoom step (wheel/axis bound to zoom, or a zoom key binding).
- While active: increases happen only at rung crossings; no decreases.
- Ends at the first non-zoom input from any device (button, key, pointer motion), or at a settlement event (output change, fit/restore, jump).
- An optional decrease-settle delay is **provisional and off by default**.
- Model: wiggle 40 → 2 changes (EC-22).

**SC-E: epoch monotonicity.**
- An **epoch** begins at a non-client stimulus: a user/shell camera action, gesture or SC-D end, output add/remove/config change, a membership change caused by camera or Stage movement, or fullscreen enter/exit.
- Within an epoch, re-evaluations caused **only** by a client's response (a commit changing its logical geometry) may move h only in the epoch's first direction. A reversal waits for the next stimulus.
- **P7 lock:** a correctness-forced decrease (device bound, INV-SCALE-2) is always applied and sets the epoch direction to *down*, so a client cannot push h back up without a new stimulus.

**Convergence argument.** Within an epoch h is monotone over a finite ladder, so there are ≤ |L| changes. Epochs start only from non-client stimuli, so a client alone cannot cause unbounded changes. Model: adversarial client 60 → 1 change (EC-22).

**Churn contract (frozen):** INV-SCALE-4 (a)–(d).

**Adversarial test destination:** OB-28 → T1 test client that, on each `preferred_scale`, picks the logical size inverting the last move, under viewport-fit and two observers. Pass: ≤ |L| + 1 changes per epoch; the run ends quiescent.

### C14.5 Provisional tuning (single authoritative list)

| Value | Current | Allowed range keeping every invariant true | Tuned by | Measurements |
|---|---|---|---|---|
| H_cfg (client scale ceiling) | 3.0 | ≥ max connected output scale; the effective ceiling is always min(H_cfg, D_max/size) | X-16 (T3/T4) | sharpness at m ∈ {1, 1.04, 1.25, 2, 2.67}; client CPU; buffer MB (EC-18 formula + fdinfo); renegotiations; frame-time variance during zoom |
| r (ladder ratio) | 1.25 | 1 < r ≤ 2 | X-16 | same |
| σ (sharpness tolerance) | 0.05 | 0 ≤ σ < r − 1 | X-16 + DV-2 | magnification visibility |
| P (off-screen fallback) | 1 s | finite, > 0 | X-7 (T5) | game/network survival, off-screen CPU |
| SC-D decrease-settle delay | off | off or finite | OB-23 | wiggle renegotiations with real clients |
| renderer filters | Linear/Linear | any | DV-2 | visual |

The architecture is correct for **any** value in the allowed ranges. Each property P1–P7 and INV-SCALE-4 holds independently of the numbers (EC-19/22 parameterised; T0 properties run over the ranges).

---

## OB-19 — Unconditional Phase F entry gate

**The first Phase F commit that creates the production Cargo workspace must, in that same commit and before any compositor or protocol implementation, install:**
1. `verify-architecture` (CI-blocking), ported from `experiments/c1-authority-spike/verify.sh`:
   - camera-authority probes with **asserted diagnostics** (mutate via read accessor, forge port, input without port, navigation internals);
   - the `!Send` port + `Send` protocol-state assertion probe (E0277);
   - `cargo tree` checks: `infynitae-core` has no dependencies; no `wayland → backend`, `ipc ↔ wayland`, `backend ↔ backend` edges;
   - a source check that `infynitae-wayland` never names `InputPort`, `ShellPort`, `physical_input`, `shell_command`;
2. `#![forbid(unsafe_code)]` in core and ipc; `#![deny(unsafe_code)]` elsewhere;
3. `cargo build --locked`; `smithay = "=0.7.0"`; `rust-toolchain.toml` pinned.

The Phase C gate passes with OB-19 open **only** because its design is complete, the spike proves feasibility (EC-1, EC-15), and its closure point (that commit) is mechanically identifiable. Phase F may not bypass or defer it.

---

## C15 — Compatibility contract (Foundation 01)

| Area | Verified (tier, evidence) | Architecturally supported, not physically validated | Deferred | Unsupported |
|---|---|---|---|---|
| Smithay | `=0.7.0` APIs and quirks W-SM-1…13 (T1 harness) | upgrade path (B12.5) | git master | other compositor libraries |
| `wl_compositor` | v6 via `new_v6` (T1) | — | — | v5-only operation (would lose `preferred_buffer_scale`) |
| xdg-shell | `xdg_wm_base` v6: toplevel/popup lifecycle, configure, suspended state, fullscreen requests (T1) | maximize/minimize policy | interactive move/resize UX | — |
| fractional-scale / viewporter | client response measured GTK4, kitty, Firefox (T3, EC-17) | other toolkits | — | — |
| presentation-time | virtual timing (T1, X-9) | DRM flip timestamps, VRR (OB-25) | — | — |
| pointer constraints / relative pointer | protocol behaviour (T1, X-8, X-12) | real games (DV-3) | — | W-SM-4b: never-activated oneshot locks destroyed on leave (OB-11) |
| xdg-activation | design + authority probes (T0) | toolkit behaviour (OB-27) | shell UX | auto-navigation by clients (forbidden) |
| keyboard | XKB via Smithay seat; ownership rules (T1, X-10); GTK4 RC-1 (T3) | Qt/SDL/Chromium/Wine (OB-10) | compositor-side repeat | multi-seat |
| XWayland | xwayland-satellite 0.8.3: toplevels, crash path, pre-map fullscreen (T1-style real time, EC-12/13) | override-redirect menus/tooltips (OB-9), activation, Steam | X11 primary selection | internal XWM, ICCCM/EWMH emulation, X11 grabs, X11 self-positioning |
| backend | winit nested (T2, Prototype 01) | DRM/KMS + libinput + libseat (T4) | multi-GPU | non-Linux |
| renderer/GPU | GLES via Smithay; tested with AMD RADV/radeonsi (Prototype 01); `D_max` from the renderer | other GPUs/drivers | Vulkan renderer | software-only rendering as a supported product mode |
| input devices | — | mouse/keyboard via libinput; touchpad gestures (begin/end) | touch, tablet | — |
| multi-output / mixed scale | 2 outputs at 1.0/1.5 (T1) | real hotplug, rotated/flipped transforms (OB-14) | — | — |
| high refresh | 165/144 Hz clocks (T1 virtual) | physical 165 Hz pacing (T4) | VRR/tearing control | — |
| X11 limitations | X11 move requests ignored (EC-12) | — | — | X11 apps placing their own windows |
| Smithay quirks | W-SM-1…13 catalogued, workarounds adapter-local | — | upstream fixes (OB-11) | — |
| dependency patching | policy (D-B12-PIN, C9) | — | — | ad-hoc forks |

**A harness or single-toolkit result is never a claim of universal hardware or toolkit support.**

---

## C16 — Non-goals for Foundation 01 (aggressive)

| Item | Classification |
|---|---|
| internal XWM / ICCCM-EWMH emulation | **non-goal** (C6) |
| universal compositor framework / reusable library | **non-goal** |
| plugin ecosystem, arbitrary third-party extensions | **non-goal** |
| generic scene graph | **non-goal** (C2.3) |
| generic spatial database/index before measured need | **non-goal** (C3.8 threshold) |
| remote desktop, network transparency | **non-goal** |
| multi-seat | **non-goal** (no Foundation requirement) |
| production telemetry platform, metrics daemon | **non-goal** (C10) |
| generalized dependency-injection / event-bus framework | **non-goal** (C1: `Effect` vocabulary only) |
| cross-platform backend abstraction for its own sake | **non-goal** (C1.1: no `Backend` trait) |
| unlimited zoom-driven client supersampling | **non-goal** (INV-SCALE-2) |
| per-toolkit compatibility hacks | **non-goal** unless a T3 failure is triaged and approved as a documented workaround |
| HDR / colour management | **deferred** (Level C; no master-program requirement for Foundation 01) |
| bookmarks, slots, snapping/clusters, overview UI, animations, decorations, blur, persistence format | **deferred** (Level D, master §C16) |

A future version may revisit any non-goal by an explicit decision; Foundation implementation may not absorb one quietly.

---

## C-PHYS — Remaining physical and dependency obligations

| Item | Tier |
|---|---|
| X-7 games/network clients off-screen (OB-5, OB-8) | T5 |
| real pointer-lock games under KeepFocus pans (OB-20) | T5 |
| visual magnification/downsampling quality (OB-7, DV-2) | T3/T4 |
| fractional camera behaviour on physical outputs (OB-6 physical part) | T4 |
| GPU/device variation (`D_max`, drivers) | T4 (per device) |
| xwayland-satellite override-redirect menus/tooltips (OB-9) | T1 (GTK3/X11) → T3 (Steam) |
| Smithay oneshot-lock defect (OB-11) | UP + Phase F decision |
| broader RC-1 toolkits (OB-10) | T3 |
| latency and high-refresh stress (C14) | T4/T5 |
| long-duration stability (C14) | T5 |

---

## C-AUTH — Final authority reconstruction (from the written artifacts)

Classes: A authoritative · P protocol · D derived · C cached · PO presentation-only.

| State | Owner | Legal writers | Observers | Class | Recovery source | Forbidden writers |
|---|---|---|---|---|---|---|
| Element identity | core `Stage` (INV-STAGE-3) | Stage minting on `new_toplevel` | all | A | none needed (process-local) | adapter, shell, persistence, clients |
| Stage geometry (anchors, z-order) | core `Stage` (INV-STAGE-1) | `stage::place` (placement policy, drag commit, shell `MoveElement`) | render, hit, membership, shell | A | persistence (Level D) on restart | cameras, renderer, adapter, satellite, scale policy, fit/fullscreen |
| Camera position | core `navigation` (INV-CAMERA-1) | navigation via `InputPort`/`ShellPort` | projection, shell | A | persistence keyed by `OutputKey` | protocol handlers, adapter, fullscreen, scale policy |
| Camera zoom | core `navigation` | navigation (gestures, fit/restore) | projection, scale policy | A | as above | scale policy (reads only), clients |
| Output state (logical config) | core output manager | output manager from backend descriptors + configuration | all | A | backend re-enumeration | Smithay `Output` (written from it), clients |
| Output membership | core `MembershipTable` | visibility pass | scale, clock, adapter | D | recompute from Stage + cameras + snapshots | Smithay entered-sets (dedupe only) |
| Surface tree | client via protocol; snapshot in core | adapter commit hook (snapshot) | core | P / D | re-extract on next commit | core, Stage |
| Scale requirement ρ | core visibility | visibility pass | `ScaleState` | D | recompute | — |
| Client scale hint h | core `ScaleState` (INV-SCALE-*) | scale policy only | adapter (emits) | A (policy state) | recompute from ρ + rules (may emit one settle) | adapter, clients, fullscreen |
| Client buffer | client | client | renderer | P | client re-commit | compositor (never alters) |
| Fullscreen | core `FullscreenState` + `OutputPresentation` | protocol policy on validated client requests; output removal/unmap | render, scale (as a presentation) | A (state) / PO (presentation view) | drop on failure; client may re-request | cameras/navigation, shell (no fullscreen write path) |
| Viewport-fit | core `navigation` `ViewportFit` | navigation via ports | projection | A | restore saved camera | protocol handlers |
| Pointer focus | core `PointerModel` + intake | intake | adapter (applies) | A (pointer state) / D (focus per event) | recompute from `ProjectionFrame` | Smithay (applied copy), cameras |
| Keyboard focus | core `FocusModel` | explicit actions, target loss, activation grant | adapter | A | successor policy | cameras, visibility, pointer motion, clients |
| Keyboard ownership | core ownership table | intake | adapter | A | physical device state on device loss | Smithay filter (executes core decisions only) |
| XKB state | Smithay `KeyboardHandle` | Smithay on every key | core (via filter) | P (authoritative for XKB) | keymap reload | core (no copy) |
| Pointer constraints | Smithay objects; core activation policy | Smithay (protocol); adapter executes core decisions | core | P + A (policy) | protocol | camera code |
| Shell | ipc connection | — | — | PO (shell-owned presentation) | reconnect + Snapshot | shell writing core state other than via semantic commands |
| XWayland | satellite (a client) | satellite via Wayland protocol | — | P | restart (elements not preserved) | any X11 path to Stage/cameras |
| Observability | `tracing` | everything (emit only) | developers | PO | — | any decision reading it |
| Persistence | core service (Level D format) | save on change; load at startup | — | C (of A) | atomic file, last good copy | partial application of a corrupt file |

**Two independent writers to any authoritative state: none found.** The two places with multiple *paths* are both deliberate:
- `stage::place` (three callers, one function, one validation);
- keyboard focus (several explicit causes, one writer: `FocusModel`).

---

## AUD — Final adversarial contradiction audit (25 combined scenarios)

| # | Scenario | Analysis (rules applied) | Result |
|---|---|---|---|
| 1 | fullscreen + second camera + mixed DPI + zoom | W fullscreen on A (ρ = 1.0) while camera B at 1.5 with zoom 2 observes W (ρ = 3.0) → h = 3.0 (ceiling). A 1920×1080 fullscreen game renders 5760×3240 (71 MB/buffer). INV-PRES-1 + INV-SCALE-1 as decided (CP2-Q1). | **consistent**. Accepted, stated cost; tunable via H_cfg (C14.5). Not a contradiction. |
| 2 | viewport-fit + ceiling + second observer | 200×100 window fitted on A: zoom 9.6 → ρ 9.6 → h 3.0; m_A = 3.2 (compositor magnifies); B unaffected | **consistent** (INV-SCALE-2, S9) |
| 3 | popup + nested subsurface + fractional camera + input region | covered by xhit (one popup level, nested subsurface, hole, 4 scales × 4 cameras): 0 mismatches | **consistent**; deeper popup levels are OB-1 (T1) |
| 4 | pan + locked pointer + output removal | the pan on A is orphaned (ownership kept); at termination the pointer's output is gone, so the lock point cannot be kept "on the pointer's output". **Under-specified in B11.4** → resolved: deactivate the lock (W-SM-4a ordering) and place the pointer Arrangement-nearest. | **gap closed** → INV-INPUT-5 |
| 5 | held modifier + focused-surface destruction + shell focus request | single-threaded event loop: successor `enter(Shift)`, then shell focus → `leave` + `enter(W3, Shift)` (X-10 K-5/K-6 patterns) | **consistent** |
| 6 | shell restart during navigation | animated navigation is a compositor interaction started with valid authority; it continues to its end; the shell reconnects with a Snapshot | **consistent** (INV-SHELL-2); animation = gesture for scale (decreases deferred) |
| 7 | satellite crash while an X11 window is focused | client death → element destroyed → focus successor; keys per INV-INPUT-7 | **consistent** |
| 8 | satellite crash while an X11 window is fullscreen | destroy → `FullscreenState` dropped → output presents camera view again; cameras untouched | **consistent** (INV-PRES-1) |
| 9 | output removal while fullscreen | presentation dropped; unfullscreen configure with saved size; camera unbound but kept | **consistent** (INV-OUTPUT-1) |
| 10 | output removal while pointer-constrained | pointer moves to a remaining output → focus change → Smithay `leave` deactivates (active constraints correctly); never-active oneshot constraints hit W-SM-4b | **consistent**; known defect OB-11 |
| 11 | off-screen client + kick + scale change + re-entry | unobserved: h held (no `preferred_scale` sent to it), kick on protocol/input; re-entry: scale settles (membership change), callbacks complete at the next clock vblank | **consistent** (INV-SCALE-5, INV-FRAME-3/4) |
| 12 | client self-resize in response to scale | naive policy oscillates for an adversary (EC-22) → SC-E | **gap closed** → INV-SCALE-4(d); test OB-28 |
| 13 | oscillation across an effective-scale boundary | camera gesture: no decreases; discrete wheel: SC-D (EC-22 40 → 2); client-driven: SC-E | **gap closed** → INV-SCALE-3/4 |
| 14 | stale shell ElementId after destroy/recreate | `(InstanceId, ElementId)`, never reused → `StaleElement` | **consistent** |
| 15 | renderer/backend failure with Stage intact | renderer: S + D rebuild; backend: terminate (Foundation 01); Stage never touched | **consistent** (INV-FAIL-1) |
| 16 | popup dismissal during camera movement | camera movement never dismisses or repositions popups; only acquisition of a compositor **pointer** interaction dismisses a popup grab (C4.6) | **consistent** (made explicit here) |
| 17 | camera zoom beyond the client-scale ceiling | compositor magnification m > 1 (S9); hint bounded | **consistent** |
| 18 | simultaneous observers with different zooms and refresh rates | ρ = max(scale × zoom); clock = highest refresh (independent of zoom) | **consistent** |
| 19 | focused off-screen client receiving keyboard input | input delivery is a kick trigger | **consistent** (INV-FRAME-3); OB-16 |
| 20 | lock/confine target becoming non-visible during pan | nothing changes during the pan (I3′); at the end, B11.4 reconciliation | **consistent** |
| 21 | shell command arriving during client destruction | calloop sources never run concurrently; a command observes either before (valid) or after (`StaleElement`) | **consistent** (A-C3) |
| 22 | persistence restoration with a missing output | cameras keyed by `OutputKey` stay unbound; anchors restored; no partial application of corrupt data | **consistent** (C8, Level D) |
| 23 | a Smithay workaround becomes unnecessary after an upgrade | removal only after its experiment passes without it; the semantic regression tests (xhit/xsnap/X5/X-8) keep running | **consistent** (INV-ARCH-3) |
| 24 | frame fallback + ping/kick interaction | double completion if the fallback ignores a just-performed kick → resolved: the fallback period is measured from the element's **last completion** (kick or fallback) | **gap closed** → INV-FRAME-3 |
| 25 | scale decrease at gesture end while the highest-requirement observer disappears simultaneously | two settle evaluations could emit two renegotiations → resolved: evaluate once per core event batch after all updates | **gap closed** → INV-SCALE-5 |

**Contradictions between established invariants: none.** Five under-specifications were found and closed by promoted rules (AUD-4, -12/-13, -24, -25), each consistent with the existing invariants and none weakening one. Scenario 1 is a stated cost of an accepted decision, not an inconsistency.

---

## Phase C gate evaluation

| Criterion | Status |
|---|---|
| C1–C16 complete | **yes**: C1–C4 (CP1), C5–C10 (CP2), scale/hit/registry (CP3A), C11–C16 (this section) |
| implementation-critical normative architecture in repository artifacts | **yes** (C13) |
| canonical invariant registry complete | **yes** (C12: 54 invariants, legacy map, supersessions marked) |
| OB-1 … OB-29 destinations | **yes** (C11.2) |
| provisional tuning separated | **yes** (C14.5, single list) |
| test tiers for every deferred validation | **yes** (C11.1, C-PHYS) |
| compatibility and non-goals explicit | **yes** (C15, C16) |
| no unresolved Level A blocker | **yes** |
| OB-19 bound to the first Phase F production-workspace commit | **yes** (OB-19 section) |
| final contradiction audit without unresolved inconsistency | **yes** (AUD) |

**PHASE C GATE: SATISFIED.** Stopped at the Phase C final review gate. Phase D has not begun.

---
---

# §C12.R1 — Canonical registry amendments (Phase D corrective checkpoint 1R)

Inputs:
- the sealed registry C12 (`615b0ed`);
- the Phase D falsification baseline `e8ec03a` (findings PD-F2-01…17, preserved in FOUNDATION_BLOCKERS.md).

Provenance for every row (previous wording, trigger, reason, evidence, ADR, tests): [audit/1R/INVARIANT_DIFF_1R.md](audit/1R/INVARIANT_DIFF_1R.md). Corrections and dispositions: [audit/1R/CORRECTIONS_1R.md](audit/1R/CORRECTIONS_1R.md).

**Where an `INV-*` appears below, this wording is canonical and supersedes C12. Every other C12 row is unchanged.** Registry size after 1R: 56.

| ID | Statement (canonical) | Owner | Class | Evidence | Tests (tier) | Trigger |
|---|---|---|---|---|---|---|
| **INV-CAMERA-1** | Camera state is mutated only inside `core::navigation`. Each mutation consumes a fresh `Authorized` proof, mintable only inside the port-carrying entry points (`physical_input(&mut InputPort)`, `shell_command(&mut ShellPort)`); the proof is not Clone, `!Send`, and consumed by value. Core retains no proof, mode, target or tracking state permitting a later mutation. No protocol event or client-originated change causes a camera mutation. Ports are unforgeable, `!Send`, bound to their Core, never stored in protocol state. Animation is not in Foundation 01; any later time-driven step must be an authorized action with its own proof. | core `navigation` | E+D | EC-1, EC-15, ER-4, ER-5 R3 | T0 probes incl. 2d/2e (OB-19) | PD-F2-01 |
| **INV-CAMERA-5** | Viewport-fit is a one-shot authorized navigation action. It snapshots the element geometry at that instant, computes centre and zoom for the invoking output, performs one camera mutation, and terminates. It is not a mode, stores no target, and is never re-fitted by client changes. Restore is a separate authorized action using the camera value recorded at fit time. There is no isolation filter in Foundation 01. A fit never changes an element's size or anchor. | core `navigation` | E+D | ER-4, ER-5 R3 | T0 (spike tests), T1 | PD-F2-01/-11 |
| **INV-COORD-2** | Rounding happens only at R1–R8: R7 = view-origin quantisation; R8 = inward rounding of a popup constraint target (ceiling at minimum edges, floor at maximum edges). F1–F7 are forbidden. | core | E+D | B1.4, C3.2, ED-2 F7 | T0 | PD-F2-12 |
| **INV-COORD-3** | Direct-manipulation pan: `dP_canvas = dp_phys / (s_O · z_C)`; `C = C_start − Q16(Σ dp_phys / (s_O · z_C))`, with the sum in f64 from the gesture origin and quantised once per update (R2), never per event. Per-event terms are summed if `s_O` or `z_C` change within the gesture. Every gesture total follows the once-per-update rule. | core | E+M | B1 E2b, X-8, ER-5 R1 | T0; T4 (OB-21) | PD-F2-06 |
| **INV-COORD-5** | Smithay's pointer global space is Canvas. The authoritative seat state is (a) **physical**: `(interaction output, OutputLogical p)`, and (b) **constraint**: `Option<{surface, kind lock/confine, lock point: SurfaceLocal}>`. A constraint never carries an output. The Canvas lock point is derived (`surface_origin + lock point`). Smithay's location is derived per dispatch. | core `PointerModel` | S+E | B2.5, X-8, ER-2 | T1 | PD-F2-03 |
| **INV-INPUT-5** | A constraint is activated only when its element holds ActiveElementFocus, its surface has pointer focus, the pointer is inside the region, and no compositor pointer interaction is being acquired.<br><br>**Reconciliation** runs once at the end of every core batch in which any of these changed: the interaction output's view; the constrained surface's geometry or origin; the output set; ActiveElementFocus; the end of a compositor interaction; the end of a client-owned grab sequence.<br><br>Reconciliation is **deferred** while a compositor pointer interaction or a client-owned grab sequence is active. While deferred, core and Smithay keep the same constraint state, no absolute motion reaches a locked surface, and relative motion comes only from physical deltas.<br><br>A lock is kept only if its derived lock point is observable through the interaction output and its element holds ActiveElementFocus.<br><br>A confinement clamps the physical pointer half-open, in the interaction output's OutputLogical space, to the region's projection; Arrangement crossing is disabled; a pointer outside the region after a view change ends it; there is no warp.<br><br>Every end emits `unlocked`/`unconfined` exactly once. An explicit `deactivate` is used only when the delivered focus does not change in that batch; otherwise Smithay's `leave()` does it (W-SM-4a).<br><br>At most one true-position motion per batch; no replayed or synthetic motion. Output removal relocates the physical pointer at once (Arrangement-nearest, half-open). Destruction ends constraints through the protocol path. | core + adapter | E+M | B11.4, X-8, ER-2, ER-5 R5 | T1 (OB-3, OB-31) | PD-F2-02/-03/-04/-05 |
| **INV-INPUT-8** | **ActiveElementFocus** (which element, or none, owns desktop keyboard focus) has one writer, the core FocusModel. It changes only by explicit action (click-to-focus, focus bindings, shell focus command, activation grant) or target loss. Camera, visibility, pointer motion and protocol routing never change it. Delivered keyboard routing is always within the focused element's tree (INV-INPUT-10), or none. | core `FocusModel` | D+E | D-B10-KBD, ER-1 | T0, T1 (OB-30) | PD-F2-02 |
| **INV-INPUT-10** (new) | **ProtocolSurfaceRoute** (Smithay popup grabs and their root restore) is adapter state, subordinate to ActiveElementFocus. It routes only within the focused element's tree and never changes ActiveElementFocus.<br><br>A core focus change to another element or to none, while a grab is active, is applied in one batch in this order:<br>1. `PopupGrab::ungrab(All)`: `popup_done` topmost first, kick first if the client is unobserved;<br>2. keyboard `unset_grab`;<br>3. pointer `unset_grab`;<br>4. `set_focus(target)`.<br><br>A nested grab keeps the installed popup pointer grab (W-SM-14). A grab whose root is not the focused element, or that fails the W-SM-7 precondition, is refused. | adapter (Smithay grabs), core decides | S+E | ER-1, W-SM-14/15 | T1 (OB-30) | PD-F2-02 |
| **INV-SCALE-2** | Every emitted hint is a rung of the ladder in force at emission. The ladder contains every connected output scale ≤ H_max(E), the integers, and the geometric fill. `H_max(E) = min(H_cfg, D_max / s_max(E))`, where `s_max` is the largest logical side over all surfaces of E's tree. `D_max` is queried from the renderer and re-queried on renderer recreation. If E's size makes h exceed the bound, h is lowered immediately (P7). A held or epoch-deferred hint may be off-ladder until the next permitted evaluation. | core `ScaleState` | E+M | EC-16/18, ED-2 F3/F4 | T0; T1 (OB-13) | PD-F2-08 |
| **INV-SCALE-3** | **ScaleInteractionEpoch.** The camera changes of one logical interaction form one epoch: continuous pan or pinch; a run of discrete pan or zoom steps; a bounded shell navigation stream.<br><br>**During an epoch:** decreases are suppressed; increases happen only at rung crossings and only in the epoch's direction (P7 fixes it down); client responses cannot reverse it; individual camera updates are not each a hint change.<br><br>**The epoch ends** at an explicit end, at a settlement event (output add/remove/configuration), or after a **settlement timeout** with no further step (for inputs without an explicit end). Pointer motion does not end it.<br><br>At settlement, h is set to the target of the final requirement, evaluated once. The timeout's existence is architectural; its value is provisional (C14.5). Atomic camera actions are one-step epochs. | core | M | ER-5 R2 | T0 (+ NEG); T1/T3 (OB-23) | PD-F2-07 |
| **INV-SCALE-4** | (a) No hint change per raw camera event. (b) Changes happen only at rung crossings, at settlement, or by P7. (c) Per epoch: at most rungs crossed + 1 settlement + one P7 forced decrease per non-client stimulus. (d) No oscillation: outside an epoch, client-caused re-evaluations move h only in the direction of the last non-client stimulus (SC-E); inside an epoch, INV-SCALE-3 applies. | core | M+E | EC-17/19/22, ER-5 R2 | T0 (+ NEG); T1 (OB-28) | PD-F2-07 |
| **INV-SCALE-5** | At zoom 1 with no zoom interaction, and where the maximum observing output scale ≤ H_max(E), the policy equals Phase B MaxGesture. Integer hint = ⌈h⌉; emission only on change; unobserved elements hold h; evaluation once per core batch after all its updates. **Initial hint:** `h := t(ρ_pred)` over the presentations whose view intersects the placed window-geometry rect (or the anchor point when the size is 0×0); if none, `t(max connected output scale)`. | core | M | P4, S7, S8, AUD-25, ED-2 F8 | T0; T1 | PD-F2-08/-09 |
| **INV-FRAME-6** | `suspended` is set only from semantic reasons {SessionLocked, **OutputsOff**, Parked}, each defined without reference to camera-derived membership; never from camera visibility. | core | E+D | D-B9-SUSPEND, SC4 | T1 | PD-F2-14 |
| **INV-PRES-1** | Protocol fullscreen is an output presentation state.<br><br>**Every `set_fullscreen`/`unset_fullscreen` is answered with exactly one configure.** A request is granted only if W holds ActiveElementFocus, or before W's first map. Otherwise it is declined at once (configure without `fullscreen`, nothing stored).<br><br>The presentation starts at the grant decision and ends at the exit decision (unset, unmap, output removal, replacement). A newer grant replaces an older one on the same output; the older element gets a configure with its saved size. Focus loss does not end fullscreen. The output choice runs at grant. Fullscreen never touches cameras or anchors; other cameras observe W at its actual size. | core `protocol` + output model | P+D+E | EC-13/14, ER-3 | T1 (OB-22, OB-32) | PD-F2-10 |
| **INV-PROTO-1** (new) | xdg_toplevel state policy (Foundation 01). `set_maximized`/`unset_maximized` are each answered with exactly one configure that does not grant `maximized`; they never invoke fit, move a camera or change the Stage (adapter overrides Smithay's default, W-SM-13). A pre-configure request is recorded and ignored. `set_minimized` is ignored. `activated` holds exactly when the element holds ActiveElementFocus and changes only on its transitions (a kick trigger if unobserved). No `configure_bounds` in Foundation 01. | core `protocol` + adapter | P+E | ER-3 | T1 (OB-32) | PD-F2-17 |
| **INV-OUTPUT-1** | Output removal: membership `leave` before the global is destroyed; camera unbound and kept; fullscreen presentation dropped with the INV-PRES-1 configure; interactions orphaned (ownership kept until release, **no camera mutation**); physical pointer relocated at once to the Arrangement-nearest point of a surviving output (half-open); constraints reconciled per INV-INPUT-5 (deferred under a client grab); the removal is a ScaleInteractionEpoch settlement event. | core output manager | D+E | B7.2, C4.6, ER-2 | T1 (OB-3, OB-31) | PD-F2-03/-11 |

## C12.R1.1 Sealed text superseded (must not be cited as normative)

| Sealed text | Superseded by |
|---|---|
| C5.1: "W changes size while fitted: the zoom is re-fitted"; isolation flag; restore on unmap; the `ViewportFit{O}` mode | INV-CAMERA-5 (one-shot) |
| C5.2: "Otherwise record it and decide when W next gains focus" | INV-PRES-1 (decline at once with a configure) |
| C7.3: "A long-running effect (animated navigation) is a compositor interaction"; AUD-6 "it continues to its end" | INV-CAMERA-1 (no animation in Foundation 01; shell navigation is atomic) |
| CP3A-5 cycle "h → client size → viewport-fit re-fit → zoom → ρ → h" | removed (no re-fit) |
| C14.4 SC-D (incl. "ends at … pointer motion", decrease-settle delay) | INV-SCALE-3 ScaleInteractionEpoch |
| B11.1 / C4 `camera = start − Q(Σ)` | INV-COORD-3 |
| B11.4 / C4.3: reconciliation only at pan end; "focused" undefined; W-SM-4a decided on core's hit-test | INV-INPUT-5 |
| INV-COORD-5 `Locked(SurfaceKey, Canvas lock point)` | INV-COORD-5 (split state) |
| C2.1/C2.2/C-AUTH rows: `ViewportFit`, `PointerModel`, keyboard focus "Smithay = applied copy" | audit/1R/MATRICES_1R.md (D3 deltas) |
| C4.5 "keyboard focus" | ActiveElementFocus (INV-INPUT-8) and ProtocolSurfaceRoute (INV-INPUT-10) |
| AUD-12/13 wording referring to SC-D | INV-SCALE-3/4 |
| C14.5 row "SC-D decrease-settle delay: off" | row "T_settle (ScaleInteractionEpoch settlement timeout): candidate 300 ms; allowed: finite and greater than the discrete-input repeat interval; tuned by OB-23" |

## C12.R1.2 Smithay quirks added

- **W-SM-14:** replacing a popup pointer grab for a nested grab tears the keyboard grab down. Workaround: keep the installed pointer grab.
- **W-SM-15:** pointer-first popup-grab teardown refocuses the root transiently. Workaround: the INV-INPUT-10 order.

Evidence: ER-1. Both are adapter-local (INV-ARCH-3).

## C12.R1.3 Obligations

- OB-19 (probes 2d/2e) extended.
- OB-23, OB-24, OB-28 rescoped.
- OB-30 … OB-35 added.

Details: audit/1R/CORRECTIONS_1R.md § Obligation changes. OB-19 remains the unconditional Phase F entry gate.

---
---

# §C12.R2 — Canonical registry amendments (Phase D corrective checkpoint 2R)

**Inputs:**
- C12 (`615b0ed`) and C12.R1 (`84ce620`);
- findings PD-F2-18 and PD-F2-19 (FOUNDATION_BLOCKERS.md § Phase D Checkpoint 2).

**Provenance:** [audit/2R/INVARIANT_DIFF_2R.md](audit/2R/INVARIANT_DIFF_2R.md).

**Where an `INV-*` appears below, this wording is canonical and supersedes C12 and C12.R1. Every other row is unchanged.** Registry size after 2R: 57.

| ID | Statement (canonical) | Owner | Class | Evidence | Tests (tier) | Trigger |
|---|---|---|---|---|---|---|
| **INV-CAMERA-1** | Camera state is mutated only inside `core::navigation`. Every mutation, **including camera creation and output↔camera binding/unbinding**, consumes a fresh `Authorized` proof.<br><br>A proof is mintable only inside `physical_input(&mut InputPort)`, `shell_command(&mut ShellPort)`, or `output_connected`/`output_disconnected(&mut OutputLifecycleToken)` (INV-OUTPUT-2). The proof is not Clone, `!Send`, and consumed by value.<br><br>The output-lifecycle entries only create, bind or unbind cameras. They never take a camera position, centre or zoom from their caller, and never change an existing camera's value.<br><br>Core retains no proof, mode, target or tracking state permitting a later mutation. No protocol event or client-originated change causes a camera mutation. Ports and tokens are unforgeable, `!Send`, bound to their Core, and never stored in protocol state. Animation is not in Foundation 01; any later time-driven step must be an authorized action with its own proof. | core `navigation` | E+D | EC-1, EC-15, ER-4, ER2-1 | T0 probes 2d/2e/2f (OB-19) | PD-F2-01, PD-F2-18 |
| **INV-CAMERA-3** | Cameras are centre-anchored views (centre Q16, zoom); per-output view origin `v_O = centre − mode/(2k)` (R7).<br><br>Cameras come into existence **only** through `output_connected` (INV-OUTPUT-2). For the connecting `OutputKey`, the camera is the first of:<br>1. the camera remembered this session (re-bound, value unchanged);<br>2. a new camera seeded from the passive persisted lookup;<br>3. a new camera at Core's internal default placement (the placement policy itself is Level D).<br><br>Cameras survive output removal (unbound, value kept) and are not destroyed in-session. N outputs may share one camera. | core `navigation` | D+E | D-B7-CENTER, B7.2, ER2-1 | T0 (spike tests), T1 hotplug | PD-F2-18 |
| **INV-OUTPUT-1** | Output removal enters Core **only** through `output_disconnected(&mut OutputLifecycleToken, …)`. Then: membership `leave` before the global is destroyed; camera unbound and kept (value unchanged); fullscreen presentation dropped with the INV-PRES-1 configure; interactions orphaned (ownership kept until release, no camera mutation); physical pointer relocated at once to the Arrangement-nearest point of a surviving output (half-open); constraints reconciled per INV-INPUT-5 (deferred under a client grab); the removal is a ScaleInteractionEpoch settlement event. | core output manager | D+E | B7.2, C4.6, ER-2, ER2-1 | T1 (OB-3, OB-31) | PD-F2-03/-11, PD-F2-18 |
| **INV-OUTPUT-2** (new) | **Output lifecycle authority.** Output connect and disconnect, at startup and at runtime hotplug alike, enter Core only through `output_connected(&mut OutputLifecycleToken, OutputDescriptor)` and `output_disconnected(&mut OutputLifecycleToken, OutputId)`.<br><br>**The token** is a dedicated authority distinct from `InputPort`: minted only by `Core::new`; private fields (unforgeable); not Clone; `!Send` (never in protocol state); bound to its Core; held exclusively by the backend's output/display event source.<br><br>**`OutputDescriptor`** carries hardware facts only (`OutputKey`, mode, scale, refresh), with no field for camera position, centre, zoom or binding.<br><br>**Persisted camera state** is a passive read-only lookup table given to `Core::new` and consulted only inside `output_connected`. No public restore API exists. | core output manager + `navigation` | E+D | ER2-1 | T0 probes 2f (OB-19); T1 hotplug | PD-F2-18 |
| **INV-SCALE-3** | **ScaleInteractionEpoch.** The spatial changes of one logical interaction form one epoch, for every `InteractionKind ∈ {CameraPan, CameraZoom, WindowMove, WindowResize, ShellStream}`: continuous pan, pinch, interactive window move or resize; a run of discrete pan or zoom steps; a bounded shell stream of navigation or `MoveElement` commands.<br><br>**During an epoch:** decreases are suppressed; increases happen only at rung crossings and only in the epoch's direction; client responses cannot reverse it; individual updates are not each a hint change.<br><br>**P7:** a device-bound decrease applies immediately and fixes the direction to down (correctness, not damping).<br><br>**The epoch ends** at interaction termination (gesture end, button release, explicit end-of-stream), at a settlement event (output add/remove/configuration), or, for discrete steps without an explicit end, at the **settlement timeout** with no further step. Pointer motion does not end it.<br><br>At settlement, h is set to the target of the final requirement, evaluated once. The timeout's existence is architectural; its value is provisional (C14.5; candidate 300 ms; OB-23). Atomic camera or Stage actions are one-step epochs. | core | M | ER-5 R2, ER2-2 | T0 (+ NEG); T1/T3 (OB-23) | PD-F2-07, PD-F2-19 |
| **INV-SCALE-4** | (a) No hint change per raw spatial event (camera, Stage-move or resize update). (b) Changes happen only at rung crossings, at settlement, or by P7. (c) Per epoch: at most rungs crossed + 1 settlement + one P7 forced decrease per non-client stimulus. (d) No oscillation: outside an epoch, client-caused re-evaluations move h only in the direction of the last non-client stimulus (SC-E); inside an epoch, INV-SCALE-3 applies. | core | M+E | EC-17/19/22, ER-5 R2, ER2-2 | T0 (+ NEG); T1 (OB-28) | PD-F2-07, PD-F2-19 |

## C12.R2.1 Sealed or amended text superseded (must not be cited as normative)

| Text | Superseded by |
|---|---|
| C1.2 "output discovery → `OutputDescriptor`" (no authority) and the C1.2 public API list (no output entry) | INV-OUTPUT-2 (`output_connected`/`output_disconnected` with `OutputLifecycleToken`) |
| C1.5 "The only public entry points that reach `navigation` are `Core::physical_input(&mut InputPort, …)` and `Core::shell_command(&mut ShellPort, …)`" | INV-CAMERA-1 (three authority entries; the output-lifecycle entries create, bind or unbind only) |
| B7.2 hotplug "bind it to the remembered camera …, else to a new camera whose placement is Level D policy"; "When the same `OutputKey` reappears, the binding (and thus the view) is restored" | INV-CAMERA-3 (precedence: remembered, then persisted, then internal default; only via the token path) |
| B7.3 / C2.1 "Binding … output manager (policy), stored in the camera set" and C-AUTH camera rows "Recovery source: persistence keyed by `OutputKey`" | INV-CAMERA-1/3, INV-OUTPUT-2; audit/2R/MATRICES_2R.md |
| C12.R1 INV-SCALE-3/4 "camera changes" | INV-SCALE-3/4 (all spatial interactions) |
| B10.4 / D-B8-SCALE.3 `gesture_active` for WindowMove/WindowResize | INV-SCALE-3 (expressed as `InteractionKind` epochs) |

## C12.R2.2 Obligations

- **OB-19** (still the unconditional Phase F entry gate) gains:
  - the probes in `verify.sh` step 2f (p6–p9, `probe_protocol_creates_camera`, `probe_store_output_token_in_state`);
  - a source rule: `infynitae-wayland` must not name `OutputLifecycleToken`, `output_connected` or `output_disconnected`.
- **OB-23** now also covers the settlement timeout for discrete Stage-move steps (shell `MoveElement` streams).

No obligation is closed by 2R.

## C12.R2.3 Clarifications (D-F0, non-semantic; added at the 2R seal)

These restate properties that INV-CAMERA-1, INV-OUTPUT-2 and INV-SCALE-3 above already imply. Nothing here changes behaviour. Each is backed by the evidence named.

**`OutputLifecycleToken` lifecycle:**
1. `Core::new` creates **exactly one** token per Core and returns it once, to its caller: the composition root, which moves it into the backend's output/display source. No method of an existing Core returns, clones, re-creates or replaces a token; the lifecycle entries borrow it (`&mut`) and return none.
   - Evidence: `verify.sh` step 2g source check (one construction site, inside `Core::new`; no other function returns the type), with a seeded negative control.
2. Code holding `&mut Core` (protocol handlers) cannot obtain the token: there is no accessor, the fields are private (probe p7), and it cannot be stored in protocol state (E0277).
   - A second `Core::new` call yields a token bound to a **different** Core, which every entry rejects (`ForeignPort`; test `ports_are_bound_to_their_core`).
3. `OutputDescriptor` conveys facts, not authority. Authority comes only from the borrowed token (probe p8: no camera field).
4. The token authorizes only `output_connected`/`output_disconnected`, i.e. create, bind and unbind. It never authorizes navigation (pan, jump, fit, restore), which requires `InputPort`/`ShellPort`.
   - The spike exposes no navigation entry taking the token; any new one is a reviewed architecture change.

**Device-bound exception inside a ScaleInteractionEpoch (INV-SCALE-3, P7):**
- A device/GPU feasibility bound may force an immediate decrease during an active epoch only when the current hint exceeds the element's device bound, i.e. `h > H_max(E)` (INV-SCALE-2).
- It never applies to an ordinary decrease of the requirement ρ, and it cannot be used to bypass epoch damping.
- After it fires, the epoch direction is down, so client responses cannot raise h again within the epoch (ER-5 R2, ER2-2).
- The 300 ms settlement timeout remains a provisional candidate under OB-23; it is not frozen.

---
---

# Phase D package (D7): living regions of this document

Everything above this line is historical:
- Phase C at its checkpoints and seal;
- the C12.R1 layer (1R) and the C12.R2 layer (2R).

It is preserved verbatim. The sections below are **living** (marked `living:` for `tools/check-foundation-docs`) and are canonical only for the namespaces §P0 assigns to this document: the namespace map itself (`NS`), `F-*` and `PROV-*`.

## §P0 Namespace authority map

Every identifier family has **exactly one canonical living home**. Other documents cite identifiers and never redefine them.

Historical locations stay where they are as provenance. They are never moved, and they are not normative once a living home exists.

A definition counts only inside a `living:` region of the home document, using the namespace's definition marker. A definition marker for the same namespace anywhere else in a living region is a checker failure.

<!-- living:NS -->
| Namespace | ID pattern | Canonical living home | Definition marker | Historical definitions (provenance only) | Other documents may |
|---|---|---|---|---|---|
| NS | `NS` | FOUNDATION_ARCHITECTURE.md | this table | — | reference |
| INV | `INV-[A-Z]+-[0-9]+` | FOUNDATION_INVARIANTS.md | heading `### INV-…` | ARCHITECTURE C12 (615b0ed), C12.R1 (84ce620), C12.R2 (5827926); legacy I1–I10 in BLOCKERS | reference only |
| F | `F-[A-Z]+-[0-9]+` | FOUNDATION_ARCHITECTURE.md | heading `### F-…` | ARCHITECTURE C3.2/C3.6/CP3A-1/C5; BLOCKERS B1.4/B1.5/B2.2/B2.3/B6.1; invariant rows in C12/C12.R1/C12.R2 | reference only |
| PROV | `PROV-[0-9]+` | FOUNDATION_ARCHITECTURE.md | heading `### PROV-…` | ARCHITECTURE C14.5; BLOCKERS ledger category 3 | reference only |
| OB | `OB-[0-9]+` | FOUNDATION_TEST_PLAN.md | heading `### OB-…` | ARCHITECTURE CP3A-4, C11.2, C12.R1.3, C12.R2.2; audit/1R CORRECTIONS_1R | reference only |
| TIER | `T[0-5]` | FOUNDATION_TEST_PLAN.md | heading `### T…` | ARCHITECTURE C11.1 | reference only |
| TST | `TST-[0-9]+` | FOUNDATION_TEST_PLAN.md | heading `### TST-…` | experiment sources and evidence files | reference only |
| ADR | `ADR-[0-9]+` | FOUNDATION_DECISIONS.md | heading `## ADR-…` | — | reference only |
| EVID | `EV-[PHSW][0-9]+`, `EB-[0-9]+`, `EB-H1`, `EC-[0-9]+`, `ED-[0-9]+`, `ER-[0-9]+`, `ER2-[0-9]+` | FOUNDATION_EVIDENCE.md | catalogue table row `\| ID \|` | EV: this same document §5 (Phase A); EB: BLOCKERS §0/B8.1/B10.1; EC: ARCHITECTURE evidence tables; ED: audit/README; ER: audit/1R; ER2: audit/2R | reference only |
| RISK | `RB-[0-9]+`, `RC-[0-9]+`, `T[ATCPRF]-[0-9]+`, `NB-[0-9]+`, `A-C1`, `A-B1-1`, `C-NB-2` | FOUNDATION_RISKS.md | heading `### …` | BLOCKERS §11/B13.2, ARCHITECTURE C-B/CP2, audit/D1 | reference only |
| PDF | `PD-F[0-3]-[0-9]+` | FOUNDATION_BLOCKERS.md | heading `### PD-F…` (living register) | BLOCKERS finding sections (append-only), audit/README registers | reference only |
| WSM | `W-SM-[0-9]+` | FOUNDATION_BLOCKERS.md | heading `### W-SM-…` (living catalogue) | BLOCKERS B12.4 / ledger cat. 5, ARCHITECTURE C12.R1.2 | reference only |
| SM | `M[0-9]+` | FOUNDATION_STATE_MACHINES.md | heading `## M…` | BLOCKERS B5.3/B9.6/B10.4/B11; audit D2, 1R, 2R | reference only |
| OBJ | `OBJ-[0-9]+` | FOUNDATION_OBJECT_MODEL.md | heading `### OBJ-…` | ARCHITECTURE C2.1/C2.2/C-AUTH; audit D3, 1R, 2R | reference only |
| PL | `PL-[0-9]+` | PROTOTYPE_DECISION_LEDGER.md | heading `### PL-…` | Phase A ledger table (same document) | reference only |
| INC | `INC-[0-9]+` | FOUNDATION_IMPLEMENTATION_PLAN.md | heading `### INC-…` | — | reference only |
| D9 | `D9` (gate checklist, verdict, Phase E handoff) | FOUNDATION_BLOCKERS.md | heading `## D9` | — | reference only |
<!-- /living -->

`audit/` and `CHECKPOINT.md` are historical/tracking records. They are never a canonical home.

## §F Formula registry (canonical `F-*`)

Each normative formula or coordinate algorithm is defined once here. Its text is taken from the historical sources it names, with D7-applied PD-F1 refinements tagged `[PD-F1-…]`.

"Extracted invariant text" records, verbatim, the fragment of an invariant's historical wording that FOUNDATION_INVARIANTS.md replaces with this reference. The checker reverses the substitution.

<!-- living:F -->

### F-VIEW-1 — Per-output view origin
- **Definition:** `v_O = Q16(centre − mode_O / (2·k_O))`, with `k_O = zoom_C · scale_O` (camera C bound to output O).
- **Definition:** `mode_O` is the output mode **after** the output transform (rotated outputs swap width and height). [PD-F1-04]
- **Definition:** Derived range: stored Canvas values obey |v| ≤ 2³⁶ (INV-COORD-1), but a derived view origin may reach |v_O| ≤ 2³⁶ + mode_O/(2k_O) (≈ 2³⁶ + 2³⁰ for a fit of a 2³¹-wide window); it stays exact in f64 because |v_O| < 2³⁷. [PD-F1-02]
- **Definition:** Quantisation: rounding point R7 (round-ties-even to Q16).
- **Extracted invariant text:** INV-CAMERA-3 ⟦`v_O = centre − mode/(2k)`⟧
- **Historical sources:** C3.2 [1]; B7.1 D-B7-CENTER; C12 INV-CAMERA-3

### F-LATTICE-1 — Reference-relative lattice and physical edges
- **Definition:** `R_O = floor(v_O / 120) · 120` (integral Canvas lattice reference); `δ = 2⁻²⁰`; `k_O = zoom_C · scale_O`.
- **Definition:** `C_O = round((v_O − R_O)·k_O + δ)` (integer: the camera on O's physical grid); `c_eff,O = R_O + Q16(C_O / k_O)` (snapped camera, rounding R5).
- **Definition:** `edge(x) = round((x − R_O)·k_O + δ) − C_O` for an integral Canvas edge x (rounding R4); a surface's physical rect is `[edge(x₀), edge(x₁)) × [edge(y₀), edge(y₁))`; origin and size are never rounded separately (F6).
- **Definition:** Evaluation order: compute `(x − R_O)·k_O` in f64 and cull against `[0, mode_O)` **before** any i32 conversion; far elements reach |x − R|·k ≈ 2³⁷·k, beyond `i32::MAX`. [PD-F1-01]
- **Definition:** Known residual: at k ∉ n/120 a lattice-reference switch can change a width or gap by ±1 physical px during camera motion (ED-1, ED-2 F1); render/hit agreement is unaffected. Open decision OB-24.
- **Extracted invariant text:** INV-RENDER-1 ⟦`edge(x) = round((x − R)·k + δ) − C_O` with R a 120-lattice reference, δ = 2⁻²⁰, k = zoom·scale⟧
- **Historical sources:** B1.5; C3.2 [1],[3]; C12 INV-RENDER-1

### F-RENDER-1 — Render location handed to Smithay
- **Definition:** `L(x) = (x − R_O)·k_O + δ − C_O` (f64) for each surface origin x (and y); the adapter builds `WaylandSurfaceRenderElement::from_surface(.., Point<f64, Physical>(L), ..)`; Smithay rounds `round(L)` and `round(L + dst·k_O)`, which equals F-LATTICE-1 (X-6, ED-1).
- **Definition:** The output's damage tracker is built with `OutputDamageTracker::new(mode_O, Scale(k_O), transform_O)` and rebuilt when k_O changes; `OutputDamageTracker::from_output` (output scale only) is not used, because it ignores zoom. [PD-F1-03]
- **Extracted invariant text:** INV-RENDER-1 ⟦`(x − R)·k + δ − C_O`⟧
- **Historical sources:** B1.5; C3.2 [4],[5]; C3.6; ED-4

### F-MEMBER-1 — Output membership
- **Definition:** Surface S is on output O iff its F-LATTICE-1 physical rect intersects `[0, mode_O)`; element E is observed by O iff any surface of E is on O. Membership is per surface (popups included) and applied with `Output::enter/leave`.
- **Historical sources:** C3.2 [3]; D-B8-ENTER

### F-UNPROJECT-1 — Output-logical point to Canvas (T5)
- **Definition:** `q = c_eff,O + Q16(p / zoom_C)` for an OutputLogical point p on output O (rounding R1; the Q16 term and c_eff's snap give ≤ 2⁻¹⁷ px + the R5 snap).
- **Historical sources:** B2.2 T5

### F-PROJECT-1 — Canvas to output-logical point (T6)
- **Definition:** `p = (q − c_eff,O) · zoom_C` (exact up to the F-UNPROJECT-1 quantum).
- **Historical sources:** B2.2 T6

### F-HIT-1 — Physical-pixel hit selection
- **Definition:** Pointer (O, p): physical `u = transform_O(p · scale_O)`, pixel `(⌊u_x⌋, ⌊u_y⌋)`. Traverse in render order (INV-RENDER-2). Select the first surface whose F-LATTICE-1 rect covers the pixel **and** whose input region (protocol add/subtract in request order; half-open cell test on the surface-local point `q − origin`, clamped into the surface; never round-to-nearest) contains `q` = F-UNPROJECT-1(p). Delivered coordinates: `q − origin`, clamped into `[0, w) × [0, h)`.
- **Definition:** Entries of a ProjectionFrame whose surface no longer exists fail closed (no delivery) until the next frame. [PD-F1-07]
- **Historical sources:** CP3A-2; D-CP3A-HIT

### F-ORIGIN-1 — Single surface-origin function
- **Definition:** `root_origin(E) = anchor(E) − geometry(E).loc`; subsurface: parent origin + `wl_subsurface` position (accumulated); popup: `root_origin + geometry.loc + popup_pos − popup.geometry.loc`. All origins are integral Canvas values. This is the only source of surface Canvas origins for render, membership, hit-test and input.
- **Extracted invariant text:** INV-COORD-6 ⟦anchor − geometry.loc + subsurface/popup offsets⟧
- **Historical sources:** B2.3; B6.1; C3.2 [2]

### F-PAN-1 — Direct-manipulation pan
- **Definition:** `dP_canvas = dp_phys / (s_O · z_C)` for physical displacement `dp_phys = dp_logical · s_O` on the interaction output O; camera `C = C_start − Q16(Σ dp_phys / (s_O · z_C))`, summed in f64 from the gesture origin and quantised once per update (R2). If s_O or z_C change within a gesture, the per-event terms are summed in f64.
- **Extracted invariant text:** INV-COORD-3 ⟦`dP_canvas = dp_phys / (s_O · z_C)`; `C = C_start − Q16(Σ dp_phys / (s_O · z_C))`⟧
- **Historical sources:** C12.R1 INV-COORD-3; ER-5 R1

### F-DRAG-1 — Element drag placement
- **Definition:** `anchor = R3(anchor₀ + Q16(Σ Canvas pointer deltas since the drag start))`, quantised once per update. **Open:** the definition across an Arrangement crossing between independently-camera'd outputs is the OB-34 decision (candidate `anchor = R3(q_now − grab_offset)`, not adopted).
- **Historical sources:** B1.3-7; PD-F2-13

### F-ROUND-1 — Rounding points
- **Definition:** R1 unproject OutputLogical → CanvasPos (Q16, ties-even); R2 camera gesture update (Q16 of the f64 gesture sum); R3 placement commit (CanvasPos → integral CanvasPoint, half-even); R4 physical edges (F-LATTICE-1); R5 snapped camera c_eff; R6 host-device deltas: none; R7 view-origin quantisation (F-VIEW-1); R8 popup constraint target: inward rounding (ceiling at the minimum edges, floor at the maximum edges). Forbidden: F1–F7 (B1.4).
- **Definition:** External quantisations (not Infynitae rounding points): protocol `wl_fixed` (1/256) for client pointer and relative-motion values; `preferred_scale` in 1/120 steps; `wl_output.scale` = ⌈scale⌉.
- **Extracted invariant text:** INV-COORD-2 ⟦R7 = view-origin quantisation; R8 = inward rounding of a popup constraint target (ceiling at minimum edges, floor at maximum edges)⟧
- **Historical sources:** B1.4; C3.2 R7; C12.R1 R8

### F-LOCK-1 — Derived Canvas lock point
- **Definition:** `q_lock = surface_origin(S) + lock_point` (F-ORIGIN-1 + the constraint's SurfaceLocal lock point); never stored.
- **Extracted invariant text:** INV-COORD-5 ⟦`surface_origin + lock point`⟧
- **Historical sources:** C12.R1 INV-COORD-5

### F-RELOCATE-1 — Pointer relocation to a surviving output
- **Definition:** On loss of the interaction output: `a = arr_origin(O_lost) + p`; choose the surviving output whose Arrangement rect is nearest to a; clamp a into that rect half-open (each coordinate into `[origin, origin + size − 2⁻⁸]`, wl_fixed resolution); `p' = a − arr_origin(O')`. [PD-F1-05]
- **Historical sources:** C4.6; AUD-4; C12.R1 INV-INPUT-5

### F-CONFINE-1 — Confinement clamp
- **Definition:** While a confinement is active on the interaction output O: `p := clamp_halfopen(p + Δ, project_O(region) ∩ [0, logical_O))` with `project_O` = F-PROJECT-1 applied to the region's Canvas rect (region + surface origin); no Arrangement crossing.
- **Historical sources:** C12.R1 INV-INPUT-5; ER-5 R5

### F-SCALE-1 — Scale requirement ρ
- **Definition:** `ρ(E) = max over observing presentations p of output_scale(p) × presentation_zoom(p)`; camera presentations use the camera zoom; a protocol-fullscreen presentation uses zoom 1.
- **Extracted invariant text:** INV-SCALE-1 ⟦max over observing presentations of `output_scale × presentation_zoom`⟧
- **Historical sources:** CP3A-1 S1

### F-SCALE-2 — Ladder and device ceiling
- **Definition:** `H_max(E) = min(H_cfg, D_max / s_max(E))`, `s_max(E)` = largest logical side over all surfaces of E's tree, `D_max` queried from the renderer (re-queried on renderer recreation, PD-F1-08).
- **Definition:** Ladder L(E): every connected output scale ≤ H_max(E) ∪ the integers 1…⌊H_max⌋ ∪ geometric fill so that consecutive rungs differ by a ratio ≤ r, all rungs ≤ H_max(E) (constants: PROV-1, PROV-2).
- **Extracted invariant text:** INV-SCALE-2 ⟦`H_max(E) = min(H_cfg, D_max / s_max(E))`⟧
- **Historical sources:** CP3A-1 S2/S3; C12.R1 INV-SCALE-2

### F-SCALE-3 — Target, emission and initial hint
- **Definition:** Target `t(ρ)` = smallest rung of L(E) ≥ ρ/(1 + σ), else H_max(E) (σ = PROV-3). Emission: `preferred_scale = round(h·120)`, `preferred_buffer_scale = ⌈h⌉`, only on change. Initial hint: `h := t(ρ_pred)` with ρ_pred = F-SCALE-1 over the presentations whose view intersects the placed window-geometry rect (or the anchor point when its size is 0×0); if none, `t(max connected output scale)`.
- **Historical sources:** CP3A-1 S4/S8; C12.R1 INV-SCALE-5

### F-FIT-1 — Viewport-fit (one-shot)
- **Definition:** For output O and element W (geometry snapshot at the instant of the authorized action): `centre = centre(W.geometry)`; `zoom = min(view_w,O / W.w, view_h,O / W.h)`; one camera mutation; the previous camera value is recorded for an optional authorized restore.
- **Historical sources:** C5.1; C12.R1 INV-CAMERA-5

### F-PRESENT-1 — Protocol-fullscreen presentation view
- **Definition:** A per-output view with zoom 1: `v_P = anchor(W) − max(0, (mode_O/scale_O − size(W)) / 2)` (componentwise), then F-VIEW-1's R7 quantisation and F-LATTICE-1 with k = scale_O; a surface larger than the output is origin-aligned and cropped. [PD-F1-09]
- **Historical sources:** C5.2; C12.R1 INV-PRES-1

<!-- /living -->

## §PROV Provisional constants and policies (freeze map)

Changing a value **inside** its allowed range is tuning, closed through its owning obligation. Changing a policy whose current choice is written into an invariant requires an architecture amendment. No value freezes from harness or model evidence alone when its destination tier is T3 or above.

<!-- living:PROV -->
### PROV-1 — Client scale ceiling H_cfg
- **Candidate:** 3.0
- **Allowed range:** ≥ max connected output scale; the effective ceiling is always min(H_cfg, D_max/s_max) (F-SCALE-2)
- **Owning obligation:** OB-12
- **Freeze tier:** T3 (client CPU/memory, X-16) + T4 (buffer memory, frame time)
- **Measurement:** X-16 zoom sweep: renegotiations, client CPU, buffer MB (fdinfo), sharpness at m ∈ {1, 1.04, 1.25, 2, 2.67}
- **Change class:** tuning
- **Status:** PROVISIONAL

### PROV-2 — Ladder ratio r
- **Candidate:** 1.25
- **Allowed range:** 1 < r ≤ 2
- **Owning obligation:** OB-12
- **Freeze tier:** T3/T4
- **Measurement:** as PROV-1
- **Change class:** tuning
- **Status:** PROVISIONAL

### PROV-3 — Sharpness tolerance σ
- **Candidate:** 0.05
- **Allowed range:** 0 ≤ σ < r − 1
- **Owning obligation:** OB-12 (with OB-7 visual)
- **Freeze tier:** T3/T4 visual
- **Measurement:** visibility of magnification m ≤ 1 + σ
- **Change class:** tuning
- **Status:** PROVISIONAL

### PROV-4 — Off-screen fallback period P
- **Candidate:** 1 s
- **Allowed range:** finite and > 0 (INV-FRAME-3)
- **Owning obligation:** OB-8 (after OB-5)
- **Freeze tier:** T5
- **Measurement:** X-7 game/network survival off-screen; off-screen CPU
- **Change class:** tuning (P = ∞ globally would violate INV-FRAME-3)
- **Status:** PROVISIONAL

### PROV-5 — ScaleInteractionEpoch settlement timeout T_settle
- **Candidate:** 300 ms
- **Allowed range:** finite and greater than the repeat interval of discrete spatial input (INV-SCALE-3)
- **Owning obligation:** OB-23
- **Freeze tier:** T3 (real clients) + T4 (real wheel/key timing)
- **Measurement:** renegotiations per discrete interaction, wheel/key/shell-stream inter-step timing distributions
- **Change class:** tuning (the timeout's existence is architectural)
- **Status:** PROVISIONAL

### PROV-6 — Renderer filters
- **Candidate:** Linear (downscale) / Linear (upscale)
- **Allowed range:** any filter supported by the renderer (no geometry effect)
- **Owning obligation:** OB-7
- **Freeze tier:** T3/T4 visual
- **Measurement:** visual comparison at m ∈ {0.67, 0.87, 1.04, 1.25, 2.67}
- **Change class:** tuning
- **Status:** PROVISIONAL

### PROV-7 — Pan cursor behaviour
- **Candidate:** cursor frozen on screen, world slides under it
- **Allowed range:** frozen-cursor or cursor-follows-world
- **Owning obligation:** OB-37
- **Freeze tier:** T3 (UX) / T5 (games)
- **Measurement:** user evaluation; game behaviour under pan
- **Change class:** **invariant amendment** if changed (INV-INPUT-3 KeepFocus and INV-COORD-5 physical pointer state assume the frozen cursor)
- **Status:** PROVISIONAL

### PROV-8 — Popup draw order
- **Candidate:** a popup tree renders immediately above its own element
- **Allowed range:** —
- **Owning obligation:** OB-38
- **Freeze tier:** Level D UX decision
- **Measurement:** occlusion cases with real toolkits (T3)
- **Change class:** **invariant amendment** if changed (INV-RENDER-2, INV-HIT-1 order)
- **Status:** PROVISIONAL

### PROV-9 — Integer preferred scale
- **Candidate:** ⌈h⌉
- **Allowed range:** —
- **Owning obligation:** OB-39
- **Freeze tier:** T3 (integer-only clients)
- **Measurement:** integer-only client sharpness/memory
- **Change class:** **invariant amendment** if changed (INV-SCALE-5 wording)
- **Status:** PROVISIONAL

### PROV-10 — Activation outcomes and shell UX
- **Candidate:** focus grant / attention / ignore; no auto-navigation
- **Allowed range:** auto-navigation only as explicit user policy executed through `ShellPort`
- **Owning obligation:** OB-40
- **Freeze tier:** Level D shell design + T3 (OB-27)
- **Measurement:** activation behaviour of real toolkits
- **Change class:** **invariant amendment** if outcomes change (INV-INPUT-9)
- **Status:** PROVISIONAL

### PROV-11 — Reserved suspension reason StableUnobserved(T)
- **Candidate:** disabled
- **Allowed range:** disabled, or finite T as configuration
- **Owning obligation:** OB-41
- **Freeze tier:** T3/T5 (evidence of benefit required)
- **Measurement:** which clients benefit; CPU/energy off-screen
- **Change class:** **invariant amendment** to enable (INV-FRAME-6 reason set)
- **Status:** PROVISIONAL
<!-- /living -->

## §D7.R PD-F1 refinement layer (applied in D7; provenance-tagged)

Each D-F1 is applied **only** inside the living `F-*`, `OBJ-*` or `OB-*` text named below. Historical text is unchanged.

| PD-F1 | Original (historical) text | Applied where | Replacement / clarification | Why semantics are unchanged |
|---|---|---|---|---|
| PD-F1-01 | C3.2 [3] "i64 → f64 → i32" (no order) | F-LATTICE-1 | compute in f64 and cull before any i32 conversion | membership is already defined as intersection with `[0, mode)`; only the evaluation order that avoids overflow is fixed |
| PD-F1-02 | INV-COORD-1 "\|v\| ≤ 2³⁶" for Canvas | F-VIEW-1 | stored values ≤ 2³⁶; derived view origins ≤ 2³⁶ + mode/(2k), exact below 2³⁷ | states the existing derived bound; no value changes |
| PD-F1-03 | C3.1 [6] "per-output `OutputDamageTracker`" (scale unspecified) | F-RENDER-1 | `OutputDamageTracker::new(mode, Scale(k_O), transform)`, rebuilt when k_O changes | the edge formula already requires scale k_O; this names the API that honours it (ED-4) |
| PD-F1-04 | INV-CAMERA-3 / C3.2 [1] `mode` | F-VIEW-1 | mode after the output transform | rotated outputs present the transformed mode; no behaviour change for unrotated outputs |
| PD-F1-05 | AUD-4 / C4.6 "Arrangement-nearest" | F-RELOCATE-1 | half-open clamp to `size − 2⁻⁸` | already adopted by 1R (INV-INPUT-5 "half-open"); recorded here as the formula |
| PD-F1-06 | CP2-2 snapshot field list (no frame-callback datum) | OBJ-05 (FOUNDATION_OBJECT_MODEL) | snapshot carries `frame_requested`; core clears it when it emits `CompleteFrames` | INV-FRAME-3 already requires arming the fallback only while a callback is pending; this names the datum that makes it implementable |
| PD-F1-07 | C3.7 ProjectionFrame (stale entries unspecified) | F-HIT-1; OBJ-15 | dead surfaces fail closed until the next frame | B4/B10.6 "lookups fail closed" already applies |
| PD-F1-08 | C8 renderer-recreation row | F-SCALE-2 | `D_max` re-queried on renderer recreation | already normative in C12.R1 INV-SCALE-2; recorded in the formula |
| PD-F1-09 | C5.2 "centred with border fill if smaller" | F-PRESENT-1 | presentation view expressed through F-VIEW-1/R7 | same geometry; names the existing rounding point instead of an unlisted one |
| PD-F1-10 | BLOCKERS ledger category 3 policies without owners | PROV-7…PROV-11; OB-37…OB-41 | each policy has an owning obligation and a freeze/decision destination | bookkeeping only; no policy value changes |
| PD-F1-11 | C14.3 counting-allocator test, C8 persistence load rule, C14.1/C14.2 T4 procedures (no owning obligation) | OB-42, OB-43, OB-44 | specified tests now have owning obligations | bookkeeping only; tests and rules unchanged |

