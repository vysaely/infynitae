# Infynitae Native — Foundation Decisions (ADRs)

Phase D checkpoint 1 (D6). Baseline: Phase C seal `615b0ed2f3631b698b0c54783c28f696ef4ff34a`.

These records explain **why** each foundational decision was made. **What** the rules are is normative in [FOUNDATION_ARCHITECTURE.md](FOUNDATION_ARCHITECTURE.md) (canonical registry §C12) and [FOUNDATION_BLOCKERS.md](FOUNDATION_BLOCKERS.md). Phase D findings are cited by ID ([audit/README.md](audit/README.md)).

An ADR whose decision is touched by an open D-F2 finding has the status **"Accepted; under Phase D review"**. The decision stands until the user disposes of the finding.

**Phase D 1R:** the corrections are recorded as "1R amendment" notes inside the affected ADRs. The canonical rules are FOUNDATION_ARCHITECTURE §C12.R1; provenance is in audit/1R/.

Evidence class: P protocol · S Smithay source · M math/model · H harness experiment (T1-style) · R real client · D design policy · V provisional · X deferred physical validation.

| ADR | Title | Status |
|---|---|---|
| ADR-01 | Stage is the sole spatial authority; Smithay `Space` rejected | Accepted; PD-F2-13 → obligation OB-34 (1R) |
| ADR-02 | Fixed-point Q16 Canvas with a reference-relative lattice | Accepted; **amended 1R** (R8; zoom-aware pan; OB-24 rescoped); **amended C12.F14** (zoom domain; the output's lattice reference, kept while panning); **amended C12.F15** (one canonical rect for render and hit) |
| ADR-03 | Window-geometry anchor and exact physical-pixel hit-testing | Accepted |
| ADR-04 | Independent centre-anchored cameras | Accepted; **amended 1R** (one-shot fit; direct-manipulation pan); **amended 2R** (output-lifecycle authority); **amended C12.F15** (fit units and clamp; restore history); **amended C12.F16** (history bound 256, emptied at unbinding); **amended C12.F17** (fit and restore refused during a presentation) |
| ADR-05 | Unified effective scale: MaxGesture, bounded ladder hint, settlement | Accepted; **amended 1R** (ScaleInteractionEpoch; device-bound ladder; initial hint); **amended 2R** (epochs for all spatial interactions); **amended F2** (device bound on the 1/120 lattice) |
| ADR-06 | Off-screen withholding with kick/fallback; suspension separated from visibility | Accepted; **amended 1R** (`OutputsOff`) |
| ADR-07 | Pre-client input ownership, gesture isolation, modifier observation | Accepted; **amended 1R** (split pointer/constraint state; reconciliation; confinement); **amended C12.F17** (no absolute motion to a locked surface; one true-position event at an interaction's end, a relocation, a reconfiguration and a presentation's start or end) |
| ADR-08 | Compile-time camera-authority separation | Accepted; **amended 1R** (per-action `Authorized` proof; PD-F2-01 resolved) |
| ADR-09 | Protocol fullscreen is output presentation; viewport-fit is a camera operation | Accepted; **amended 1R** (answer every request; maximize declined; one-shot fit) |
| ADR-10 | X11 via xwayland-satellite only | Accepted; PD-F2-16 → OB-33 (`-listenfd` verified, ER-6) |
| ADR-11 | Non-authoritative semantic shell IPC | Accepted; **amended 1R** (shell navigation atomic; no animation in F01); PD-F2-15 → OB-35 |
| ADR-12 | Performance/idle philosophy and the OB-19 Phase F entry gate | Accepted |
| ADR-13 | Exact Smithay pin with adapter-local workarounds | Accepted; W-SM-14/15 added (1R) |
| ADR-14 | Element-level focus authority vs protocol surface routing | **Accepted (1R, new)** |

---


> **D7:** this document is the canonical living home of `ADR-*` (FOUNDATION_ARCHITECTURE §P0). Normative rules live in FOUNDATION_INVARIANTS.md (`INV-*`), FOUNDATION_ARCHITECTURE.md (`F-*`, `PROV-*`) and FOUNDATION_TEST_PLAN.md (`OB-*`); ADRs explain why and cite those IDs.

<!-- living:ADR -->
## ADR-01: Stage is the sole spatial authority; Smithay `Space` rejected

- **Status:** Accepted (B2.4, C2; INV-STAGE-1). Under Phase D review for the drag formula across outputs (PD-F2-13). The authority decision itself is not in question.
- **Context:** A spatial desktop needs one truth for where windows are. Smithay's `Space` is the conventional container. Prototype 01 kept positions in several places and produced offset bugs (EV-P09/P16).
- **Decision:** Only core `Stage` stores anchors and z-order, written by one function `stage::place`. Everything else (cameras, membership, render lists, Smithay state, shell, XWayland) reads or derives.
- **Evidence:**
  - EB-2: `Space` stores i32 locations (fails ±2³⁶), couples z-order to activation (S7), has no notion of which camera the pointer looks through (S3), and would need resyncing on every pan tick.
  - EB-2 S5 and X5: `Space`/`Window::output_enter` mis-place CSD and popup membership.
  - X-8 INV-3: Stage was untouched in all 9 pan cases.
- **Alternatives:**
  - `Space` as a derived per-frame mirror;
  - per-output `Space`s as cameras;
  - a scene graph.
- **Rejected:** all three. A mirror is a second, lossy authority that has to be resynchronised on every pan; per-output `Space`s are integer-only; a scene graph is a C16 non-goal.
- **Consequences:** Infynitae owns membership, hit-testing and render collection (ADR-03). Every Smithay helper that assumes `Space` is bypassed. That is W-SM-1/2, and the workarounds are adapter-local (ADR-13).
- **Invariants:** INV-STAGE-1, INV-STAGE-2, INV-STAGE-6, INV-RENDER-3, INV-RENDER-4.
- **Obligations:** OB-26 (regression guard).
- **Revisit trigger:** a Smithay `Space` that supports i64/fixed-point locations and non-activating raises **and** passes xsnap/xhit/X5 unmodified.

## ADR-02: fixed-point Q16 Canvas with a reference-relative lattice

- **Status:** Accepted (B1, C3.2; INV-COORD-1…3, INV-RENDER-1).
- **Under review:**
  - PD-F2-12 adds a rounding point (R8) for popup constraint targets;
  - PD-F2-06: the jitter-freedom of the lattice holds at zoom 1, but zoomed panning is reachable in Foundation 01.
- **Context:** An "effectively unbounded" canvas must keep precision uniform, be path-independent, and render without seams at fractional scales.
- **Decision:**
  - i64 Q16 (2⁻¹⁶ px), with stored values bounded by ±2³⁶ so that every value converts to f64 exactly.
  - Integral anchors.
  - Rounding only at listed points.
  - Gesture totals quantised once per update.
  - Physical edges by `edge(x) = round((x − R)k + δ) − C` with a 120-lattice reference.
- **Evidence:**
  - M/H: E1–E9. f64 is non-associative (0.24 px), drifts 6.1 px per 1e6 events and gives 4,288 tiling errors at 2³⁶; i32 loses sub-pixel deltas; Q16 with gesture totals gives 0 drift.
  - H: X-6, 0 / 134,478 edge mismatches against Smithay.
  - H: ED-1, 0 mismatches at k = 1080/651 and 1.875.
- **Alternatives:** f64 Canvas; i32 Canvas; per-element rounding; absolute f64 edges.
- **Rejected:** f64 (non-associative, equality issues, edge errors at magnitude); i32 (range, no sub-pixel camera); per-element rounding (relative jitter, up to 70/1000 steps in E4).
- **Consequences:**
  - The domain carries explicit types and rounding functions (INV-COORD-4).
  - Smithay receives an f64 location built by one adapter function (W-SM-5 avoided).
  - At zoom ≠ 1 the reference switch is not shape-invariant. ED-1 measured a ±1 px width change.
- **Invariants:** INV-COORD-1, INV-COORD-2, INV-COORD-3, INV-COORD-4, INV-RENDER-1.
- **Obligations:** OB-6, OB-24.
- **Revisit trigger:** a zoom-motion feature (OB-24), or PD-F2-06 disposed with option (b); a renderer that accepts sub-pixel positioning.
- **1R amendment:**
  - R8 (inward rounding of popup constraint targets) is added to the rounding set (PD-F2-12).
  - Gesture totals are zoom-aware (INV-COORD-3, PD-F2-06).
  - The lattice is kept: render/hit agreement holds at k ∉ n/120 (ED-1). The ±1 px shape change at reference switches during zoomed panning is a stated residual, OB-24 rescoped (candidate: gesture-pinned reference).
- **C12.F14 amendment (owner decision 2026-10-17, OB-24):**
  - The reference is the output's lattice reference, kept while panning and set again when the output's view is (re)defined or `|v_O − R_O|·k_O` would exceed 2³¹; never persisted (F-LATTICE-1).
  - Camera zoom is bounded to the representable domain 2⁻²⁰ … 2¹⁶ (F-VIEW-1), which keeps the Q16 view origin exact.
  - Corrected 2026-10-17 (C12.F14.4): the 2³¹ bound is a conservative choice under a rounding-step argument, not a guarantee that f64 edges equal exact arithmetic. The 1R statement above that render/hit agreement holds at k ∉ n/120 (ED-1) rests on sampled runs: Smithay geometry swept at rounding points shows 1 physical px right-edge differences between the presented rect and Core's F-LATTICE-1 rect at k ∉ n/120 (`evidence/phase-f/OB-24/render_hit_probe.out`). Agreement there, with the kept reference, is an INC-10 exit requirement.
- **C12.F15 amendment (owner decision 2026-10-17, option B):**
  - Core's F-LATTICE-1 integer rect is the one canonical rect of a surface: the adapter wraps Smithay's element so that its geometry and draw destination are that rect (INV-RENDER-1, F-RENDER-1). Damage and opaque regions are remapped conservatively; rects beyond Smithay's i32 range are cropped, not rejected.
  - Rejected: renderer-authoritative geometry (Core adopting Smithay's rect: its right edge and the next surface's left edge come from different formulas, so seams return) and snapping k to a coarse lattice (changes the fit, needs a proof per reachable case, blocks D6).

## ADR-03: window-geometry anchor and exact physical-pixel hit-testing

- **Status:** Accepted (D-B2-ANCHOR, D-CP3A-HIT; INV-STAGE-2, INV-COORD-6, INV-HIT-1).
- **Context:** A window's position must not jump when its client changes CSD shadows or subsurfaces. What the user clicks must be what they see, including at fractional scales and fractional cameras.
- **Decision:**
  - The anchor is the Canvas position of the xdg window-geometry origin.
  - All surface origins come from one function.
  - Hit-testing selects the surface **presented** at the pointer's physical pixel, using the render edges and order, plus protocol input-region semantics.
  - `Window::surface_under` is not used.
- **Evidence:**
  - P: `set_window_geometry` ("treat (x, y) … as the top left corner"; popups are relative to the window geometry).
  - S/H: `Space` uses the same convention (EB-2 S1).
  - H: EC-20, 0 mismatches over 16 configurations, while the Phase B canvas-exact rule mismatched on 399–617 pixels.
  - H: ED-1, 0 mismatches at k ∉ n/120.
  - S: W-SM-11 (input regions shifted ½ px) and W-SM-12 (a below-subsurface steals hits) explain every Smithay disagreement.
- **Alternatives:** root-surface origin; bbox origin; `Window::surface_under`; a ½-px tolerance.
- **Rejected:**
  - root or bbox anchors: CSD changes move the window;
  - `surface_under`: contradicts rendered stacking and protocol regions;
  - the tolerance: measurably wrong (EC-20).
- **Consequences:**
  - Core needs a public-API surface-tree snapshot per commit (CP2-2).
  - Hit-testing uses the last rendered frame (I9 trade-off, PD-F0-06).
- **Invariants:** INV-STAGE-2, INV-COORD-6, INV-HIT-1, INV-RENDER-2.
- **Obligations:** OB-1, OB-2, OB-14, OB-17, OB-26.
- **Revisit trigger:** a protocol-level change to window-geometry semantics; T3 evidence that W-SM-9 re-clamping harms real clients.
- **E1R amendment (unsealed, E1R-N1):** the hit pixel is computed in view space, `u = p · scale_O` (F-HIT-1); the output transform belongs to the renderer only. The sealed `transform_O(p · scale_O)` disagreed with the presented surface on every non-Normal output (EE-1). The decision (exact physical-pixel hit-testing against presented geometry) is unchanged.

## ADR-04: independent centre-anchored cameras

- **Status:** Accepted (B7, D-B7-CENTER; INV-CAMERA-2/3/5).
- **Under review:**
  - **PD-F2-01 (A):** fit re-fit and animation mutate cameras without a port;
  - **PD-F2-06:** fit exit keeps the zoom, so zoomed panning is reachable;
  - **PD-F2-11:** shared cameras versus output-keyed fit state.
- **Context:** Infynitae is a spatial desktop in which each output is a view into one Stage. Outputs differ in size and scale and come and go.
- **Decision:**
  - Cameras live beside the Stage in `navigation`.
  - A camera is a centre (Q16) plus a zoom.
  - Each output derives its view origin `v_O = centre − mode/(2k)` (R7).
  - Cameras survive output removal and are keyed by `OutputKey`.
  - N outputs may share one camera.
- **Evidence:**
  - D: centre anchoring keeps content centred across mode/scale changes and mirrored outputs of different size.
  - H: SC1 (two independent cameras, measured trace).
  - H: D5 epochs (Arrangement origins never enter projection).
- **Alternatives:** outputs as integer `Space` positions; top-left-anchored cameras; cameras owned by outputs.
- **Rejected:** `Space` positions (ADR-01); top-left anchoring (zoom and mirroring shift content); output-owned cameras (lost on hotplug).
- **Consequences:**
  - The pointer needs an Arrangement for crossing outputs.
  - Its Canvas position is per-output (T5).
  - A window drag across outputs has no continuous Canvas path (PD-F2-13).
  - Crossing during a client grab jumps (PD-F0-01).
- **Invariants:** INV-CAMERA-2, INV-CAMERA-3, INV-CAMERA-5, INV-OUTPUT-1.
- **Obligations:** OB-3, OB-24.
- **Revisit trigger:** a decision on PD-F2-01/-06/-11; the zoom-motion feature.
- **1R amendment:**
  - Viewport-fit is one-shot (INV-CAMERA-5). A client can invalidate what the user sees but can never move a camera.
  - Pan follows direct manipulation: `dP = dp_phys/(s_O·z_C)` (INV-COORD-3).
  - Zoomed navigation is part of Foundation 01.
  - An orphaned pan mutates nothing (INV-OUTPUT-1).
  - Rejected in 1R: a sticky fit mode, because it is the client→camera path (PD-F2-01).
- **2R amendment:**
  - Cameras are created, bound and unbound only through the dedicated `OutputLifecycleToken` path (INV-OUTPUT-2). It is coordinate-free (hardware-only descriptors).
  - Precedence on connect: remembered session camera, then passive persisted entry, then internal default.
  - Startup equals hotplug. There is no public restore API (PD-F2-18; ER2-1).
  - Rejected: overloading `InputPort` (it would merge device-input and display-lifecycle authority) and a port-free output entry (protocol-reachable camera path).
- **E1R amendment (unsealed; E-F2-01, E-F2-03, E-F2-04, E-F3-03):**
  - Foundation 01 binds cameras **1:1** to outputs. "N outputs may share one camera" is withdrawn; shared cameras (mirroring) are deferred behind gate G-MIRROR (FOUNDATION_IMPLEMENTATION_PLAN).
  - The view uses the transformed `view_O`; Smithay's damage tracker uses the physical `phys_O` (F-VIEW-1, F-RENDER-1).
  - Output reconfiguration is a third lifecycle entry, `output_reconfigured`; it never mints camera authority (INV-OUTPUT-2).
- **C12.F15 amendment (owner decision 2026-10-17):**
  - F-FIT-1's zoom is `min(view_w,O / g.w, view_h,O / g.h) / scale_O`, clamped into F-VIEW-1's domain; a degenerate or unmapped W is refused.
  - Each successful fit records the camera value before it; one restore reverses the most recent fit, repeated restores unwind earlier ones (OBJ-30).
  - `OutputKey` is a persistence identity, not a physical-monitor identity; collisions are resolved deterministically (INV-CAMERA-3). Persisting cameras across sessions is gated by G-OB48.
- **C12.F16 amendment (owner decision 2026-10-18):**
  - The history holds at most 256 entries per output; a successful fit at capacity evicts the oldest, which is lost permanently; a refused fit or restore changes nothing.
  - The history is part of a camera's binding to one `OutputId`: emptied when the camera is unbound, kept across `output_reconfigured`; it is never applied to another output or a later connection (INV-CAMERA-5).
- **C12.F17 amendment (owner decisions 2026-10-19, Q1 B scope 2):**
  - Fit and restore, from input or shell, are refused while their output presents a protocol-fullscreen element: no camera, history or pointer change; the bindings still resolve and are consumed (M8, F-FIT-1).

## ADR-05: unified effective scale (MaxGesture, bounded ladder hint, settlement)

- **Status:** Accepted (D-B8-SCALE → D-CP3A-SCALE, C14.4; INV-SCALE-1…5).
- **Under review:**
  - PD-F2-07: discrete camera interactions; SC-D ends on motion;
  - PD-F2-08: device bound versus "every output scale", P4 equivalence, H_max basis, off-ladder hint;
  - PD-F2-09: initial hint.
- **Context:**
  - The protocol allows one preferred scale per surface but gives no rule for a surface seen at several scales (B8.3).
  - Clients reallocate on every 1/120 change (EC-17), and a scale storm costs up to 477% client CPU.
  - Camera zoom can exceed any sensible buffer size (EC-18) and the device limit (EC-16).
- **Decision:**
  - ρ = max(scale × zoom) over observing presentations.
  - The hint h is a rung of a finite ladder, capped at H_max = min(H_cfg, D_max/size).
  - Increases happen at rung crossings; decreases only when no gesture or discrete-zoom interaction is active.
  - Epoch monotonicity (SC-E) stops client feedback loops.
  - Evaluation once per batch; no fullscreen override.
- **Evidence:**
  - R: SC2, five policies compared; MaxGesture is 0 ms blurry with 1 event where Max needed 19.
  - R: EC-17, real client cost.
  - M: EC-19, 5 renegotiations versus 313 in Case E.
  - M: EC-22, adversary 60 → 1; wiggle 40 → 2.
  - M: ED-2 F2/F3, the limits of those claims.
- **Alternatives:** Min; Primary (Smithay-like); LargestArea; raw ρ at 1/120 granularity; a fullscreen override (CP2-Q1).
- **Rejected:**
  - Min and Primary are blurry on the high-DPI output;
  - LargestArea flaps during motion;
  - raw ρ causes storms and unbounded buffers;
  - the fullscreen override was rejected by user decision: the cost was accepted (AUD-1).
- **Consequences:**
  - Buffer cost while multi-observed rises as (s_max/s)².
  - Minification on lower-scale outputs (DV-2).
  - Compositor magnification beyond the ceiling.
- **Invariants:** INV-SCALE-1, INV-SCALE-2, INV-SCALE-3, INV-SCALE-4, INV-SCALE-5.
- **Obligations:** OB-7, OB-12, OB-13, OB-23, OB-28.
- **Revisit trigger:** X-16 tuning (TUNE); a disposition of PD-F2-07/-08/-09.
- **1R amendment:**
  - Gesture-type-specific damping (MaxGesture flag + SC-D) is replaced by one concept, **ScaleInteractionEpoch** (INV-SCALE-3/4). It ends by explicit end, settlement event or timeout; the timeout's existence is architectural, its value provisional (OB-23).
  - The ladder contains output scales ≤ H_max, with H_max over all surfaces (INV-SCALE-2).
  - Initial hint defined (INV-SCALE-5).
  - Evidence: ER-5 R2 (40 → 1 for discrete pan, zoom-at-cursor and shell streams; negative control 40).
- **2R amendment:**
  - ScaleInteractionEpoch covers every spatial interaction: CameraPan, CameraZoom, WindowMove, WindowResize, ShellStream (INV-SCALE-3).
  - Window drags across a scale boundary settle in 1 change instead of 39 (ER2-2).
  - P7 stays an immediate correctness exception.
  - The 300 ms settlement timeout remains a provisional candidate (OB-23), not a frozen value.
- **E1R amendment (unsealed; E-F2-06, E1R-N3, E-F2-07):** `H_max(E) = min(H_cfg, H_dev(E))` with `H_dev = +∞` at zero extent (F-SCALE-2); the integer hint is bounded by the device, `b(h)` (F-SCALE-3). The fullscreen policy is unchanged: E-F2-07's 8K claim does not follow from the formulas (h ≤ H_cfg = 3.0, EE-6); CP2-Q1 remains an open policy question.
- **F2 amendment (C12.F2, owner approval 2026-10-13; H-1):** the device bound is the largest multiple of 1/120 not above `D_max / s_max(E)`, `H_dev(E) = ⌊120 · D_max / s_max(E)⌋ / 120` (F-SCALE-2), so that the emitted fractional hint never exceeds it. The bound is on the exact product `h · side`; how a client rounds its buffer size is not assumed. The ladder, the target, the emission and the integer hint are unchanged.

## ADR-06: off-screen withholding with kick/fallback; suspension separated from visibility

- **Status:** Accepted (D-B9-FRAMES, D-B9-SUSPEND; INV-FRAME-1…6). Under review: PD-F2-14 (one suspension reason is defined over camera membership).
- **Context:** The spec recommends withholding frame callbacks from invisible surfaces. Pure withholding blocks Mesa-WSI-FIFO clients' main loops for the whole off-screen period (vkcube: pings unanswered for 2–4 s). Deriving `suspended` from visibility sends one configure per crossing, up to 133 configures/s.
- **Decision:**
  - Withhold callbacks while unobserved.
  - Complete them once before any request that needs the client's loop, and before input delivery (the kick).
  - Complete them at most once per finite period P (provisional 1 s), with the timer armed only when needed.
  - Complete them within one period after re-entry.
  - `suspended` comes only from semantic reasons.
- **Evidence:**
  - P: EV-W02.
  - R: EB-9 (Starve/Throttle/kick with vkcube, kitty, Firefox).
  - H: SC3, SC4, X-9.
  - S: EB-11 (callbacks are never dropped).
- **Alternatives:** pure starve; periodic throttle only; visibility-driven `suspended` (immediate or debounced).
- **Rejected:** pure starve (liveness failure); throttle only (periodic cost); visibility-driven `suspended` (configure storms; debounce only delays them, 48 = 48).
- **Consequences:** Present-locked games run at about 1 fps off-screen (X-7 unmeasured, OB-5). Core must know when callbacks are pending (PD-F1-06).
- **Invariants:** INV-FRAME-1, INV-FRAME-2, INV-FRAME-3, INV-FRAME-4, INV-FRAME-5, INV-FRAME-6, INV-IDLE-1.
- **Obligations:** OB-5, OB-8, OB-16, OB-25.
- **Revisit trigger:** X-7 game/network results; evidence of benefit for a `StableUnobserved(T)` reason.
- **1R amendment:** the reason `AllObservingOutputsOff` is replaced by `OutputsOff`, defined without camera membership (INV-FRAME-6, PD-F2-14).
- **D7 note (PD-F0-11, editorial):** affected invariants also include INV-CAMERA-4. A camera change causes no configure, which is the reason `suspended` is never derived from visibility.

## ADR-07: pre-client input ownership, gesture isolation, modifier observation

- **Status:** Accepted (D-B10-INTAKE, D-B11-PAN/FOCUS, D-C4-KBD; INV-INPUT-1…9).
- **Under review:**
  - **PD-F2-02 (A):** Smithay grabs decide focus behind the core models;
  - PD-F2-03: `Locked` representation;
  - PD-F2-04: reconciliation triggers;
  - PD-F2-05: confinement;
  - PD-F2-17: the `activated` state.
- **Context:** A compositor camera gesture must never leak into games or leave stuck buttons, keys or modifiers. The protocol cannot retract a delivered press, and `enter` announces the seat-wide modifier state.
- **Decision:**
  - Every physical event has one owner, decided at press before any client dispatch; releases follow the press.
  - A pan is acquired only when the client owns no button.
  - Pan motion never reaches clients (KeepFocus).
  - Relative motion comes only from physical deltas.
  - Modifier and lock keys are never compositor-owned (observe mode).
  - Keyboard focus changes only by explicit action or target loss.
- **Evidence:**
  - H: X-8, 48/48 checks.
  - H: X-10, 9 scenarios plus a negative control that reproduces the stuck modifier.
  - R: EC-21 (GTK4, RC-1).
  - P: `wl_keyboard`/`wl_pointer` semantics (EC-4).
- **Alternatives:** LeaveFocus during pans; compositor-owned modifiers; focus-follows-camera; synthetic releases.
- **Rejected:**
  - LeaveFocus destroys oneshot locks (B11.2);
  - compositor-owned modifiers leave stuck modifiers (X-10 NEG);
  - focus-follows-camera lets the camera steer keystrokes;
  - synthetic releases violate the protocol's must-not rules.
- **Consequences:**
  - Clients see the modifiers of compositor bindings.
  - Focus may stay on an off-screen window; input to it is a kick.
  - Grabs owned by Smithay must be reconciled with the core models (PD-F2-02).
- **Invariants:** INV-INPUT-1, INV-INPUT-2, INV-INPUT-3, INV-INPUT-4, INV-INPUT-5, INV-INPUT-6, INV-INPUT-7, INV-INPUT-8, INV-INPUT-9, INV-COORD-5.
- **Obligations:** OB-3, OB-4, OB-10, OB-11, OB-15, OB-16, OB-20, OB-21.
- **Revisit trigger:** a disposition of PD-F2-02…05/-17; T5 game results (DV-3); a Smithay fix for W-SM-4b.
- **1R amendment:**
  - Seat state is split into physical `(interaction output, p)` and a constraint `{surface, kind, SurfaceLocal lock point}` (INV-COORD-5).
  - One batch-end reconciliation, deferred during compositor interactions **and client-owned grab sequences** (INV-INPUT-5; ER-2 shows the deferral is the only strategy that keeps core and Smithay equal with exactly one `unlocked`).
  - Confinement is clamped on the interaction output with no crossing (ER-5 R5).
  - Focus layering: ADR-14.
- **D7 note (PD-F0-11, editorial):** affected invariants also include INV-CAMERA-4. Pan-originated input never reaches clients.
- **E1R amendment (unsealed; E-F2-02, E-F2-05):**
  - A constraint deactivation event is sent only from the active state. This needs the dependency patch P1 (Smithay `leave()` deactivates only an active constraint) or an upstream fix, under ADR-13's patch policy (OB-11); no public-API alternative exists (W-SM-4c, EE-2).
  - Zero outputs: the seat enters `NoOutput` through a compositor zero-output grab installed with `Focus::Clear`, never through `unset_grab` (W-SM-16, EE-3).
- **C12.F17 amendment (owner decisions 2026-10-19, Q2 widen, Q3 exactly one):**
  - No absolute motion reaches a surface whose lock is active, on any path (camera actions, output reconfiguration, a presentation's start or end, an interaction's end, an output relocation).
  - Those paths send exactly one true-position event, after the batch's reconciliation, with the pointer on an output and no interaction owning it, unless a lock is kept: a kept confinement receives it (INV-INPUT-5, M10).

## ADR-08: compile-time camera-authority separation

- **Status:** Accepted (C1.5, C7.4; INV-CAMERA-1, INV-ARCH-1). **Under review: PD-F2-01 (Level A).** Two sealed rules (re-fit, animation) need camera mutation outside the two port-gated entry points.
- **Context:** Every Smithay handler receives `&mut State`. Any design that makes camera mutation reachable from `State` lets an untrusted client move cameras.
- **Decision:**
  - Only `core::navigation` mutates cameras.
  - It is reachable only through `physical_input(&mut InputPort)` and `shell_command(&mut ShellPort)`.
  - Ports are unforgeable, not `Clone`, `!Send`, bound to their Core, and never stored in protocol state (E0277).
  - Protocol policy has no camera vocabulary.
- **Evidence:** H (compiler): EC-1 and EC-15. Probes fail with their asserted diagnostics: E0624, private-field, E0061, E0603, E0277.
- **Alternatives:** runtime authority tags; code-review discipline; a separate process.
- **Rejected:** runtime tags can be forged by any code holding `State`; review discipline is not mechanical; a separate process is disproportionate for Foundation 01.
- **Consequences:**
  - The composition root distributes ports.
  - Any time-driven or derived camera behaviour must be expressed without a port-free mutator. PD-F2-01's recommended correction is derived view modes.
- **Invariants:** INV-CAMERA-1, INV-SHELL-1, INV-ARCH-1.
- **Obligations:** OB-19 (probe list to be extended per PD-F2-01).
- **Revisit trigger:** the PD-F2-01 disposition; any new camera behaviour.
- **1R amendment:**
  - Authority is per action: each camera mutation consumes a fresh `Authorized` proof minted only in the port-carrying entry module (E0624 elsewhere), and retaining one fails the `Send` assertion (E0277).
  - This closes PD-F2-01's "authority retained for later" path, which port-gating alone did not cover (ER-4).
- **D7 note (PD-F0-10, editorial):** effective since 2R, camera authority has three proof-minting entries: `InputPort`, `ShellPort`, and the dedicated `OutputLifecycleToken` for output connect/disconnect, which can only create, bind or unbind cameras (INV-CAMERA-1, INV-OUTPUT-2). The decision text above predates 2R; it is not rewritten.
- **E1R note (unsealed, E-F2-03):** `output_reconfigured` borrows the `OutputLifecycleToken` but mints no `Authorized` proof; the three camera-authority entries are unchanged (verify.sh 2h, EE-4).

## ADR-09: protocol fullscreen is output presentation; viewport-fit is a camera operation

- **Status:** Accepted (C5; INV-PRES-1, INV-CAMERA-5).
- **Under review:**
  - PD-F2-01: fit re-fit authority;
  - PD-F2-10: fullscreen state machine;
  - PD-F2-17: maximize answer and `activated`.
- **Context:** A client's fullscreen request and the user's "frame this window" are different intents with different authorities. Conflating them would hand clients camera control.
- **Decision:**
  - Fullscreen is a per-output presentation state that clients request under focus policy. It never touches cameras or anchors, and other cameras see the real size.
  - Fit is a user or shell camera operation, saved and restored in `navigation`.
  - No fullscreen scale override (CP2-Q1).
- **Evidence:**
  - P: EC-14 (spec text).
  - H: EC-13 (satellite sends a pre-map fullscreen; W-SM-13).
  - M: Case C, AUD-1.
  - M/H: D5 epoch 3.
- **Alternatives:** fullscreen as a camera move; hiding W on other cameras; a fullscreen-only scale.
- **Rejected:**
  - a camera move gives clients camera authority;
  - hiding W lies about the protocol state (one surface, one size);
  - a fullscreen-only scale was the user's decision (cost accepted).
- **Consequences:** A fullscreen game also visible on a higher-scale camera renders at that scale (AUD-1: up to 71 MB/buffer).
- **Invariants:** INV-PRES-1, INV-CAMERA-5, INV-SCALE-1.
- **Obligations:** OB-22.
- **Revisit trigger:** OB-22 results; a PD-F2-10 disposition.
- **1R amendment:**
  - Every `set_fullscreen`/`unset_fullscreen`/`set_maximized`/`unset_maximized` is answered with exactly one configure (protocol requirement; ER-3).
  - Unfocused fullscreen requests are declined at once (no pending state).
  - Maximize is declined and never invokes fit (INV-PRES-1, INV-PROTO-1).
  - Fit is one-shot (INV-CAMERA-5).

## ADR-10: X11 via xwayland-satellite only

- **Status:** Accepted (D-C6; INV-XWAYLAND-1). Under review: PD-F2-16 (restart display identity; `-listenfd` unverified).
- **Context:** An internal XWM brings ICCCM/EWMH, X11 grabs and self-positioning windows, all of which are second spatial authorities.
- **Decision:** xwayland-satellite runs as an ordinary Wayland client under a supervisor. There is no internal XWM, and X11 position requests are ignored.
- **Evidence:** R: EC-12 (toplevel mapping; a move request had no effect; the crash path) and EC-13 (pre-map fullscreen).
- **Alternatives:** Smithay's `xwayland` feature with an internal XWM.
- **Rejected:** the internal XWM is a second authority and a large surface (C16 non-goal).
- **Consequences:** X11 apps lose self-positioning and do not survive a satellite crash. Override-redirect fidelity is OB-9.
- **Invariants:** INV-XWAYLAND-1, INV-STAGE-1.
- **Obligations:** OB-9, OB-27.
- **Revisit trigger:** T3 evidence that satellite fails a required app class.
- **1R:** xwayland-satellite 0.8.3 supports `-listenfd` (ER-6). Restart on the same X display is OB-33.

## ADR-11: non-authoritative semantic shell IPC

- **Status:** Accepted (D-C7; INV-SHELL-1/2).
- **Under review:**
  - PD-F2-01: an animated navigation started by the shell has no authority path;
  - PD-F2-15: the attention lifecycle in the event stream.
- **Context:** A Quickshell-class shell needs to navigate, focus and observe. A raw state API would make it a second authority.
- **Decision:**
  - The shell sends semantic commands through the same validated single-writer paths as user input.
  - Its view is a generation-stamped derived copy: Snapshot plus Subscribe.
  - The shell owns no authoritative state.
  - `ShellPort` lives only in `infynitae-ipc`.
- **Evidence:** D: C7; same-uid trust statement.
- **Alternatives:** a raw state API; a shell inside the compositor; Wayland-protocol-based shell control.
- **Rejected:**
  - a raw API creates arbitrary mutation;
  - an in-process shell couples crash domains;
  - a protocol-based shell puts privilege on the client channel (ADR-08).
- **Consequences:** Shell restart is stateless. Shell delta streams need gesture semantics for scale (PD-F2-07).
- **Invariants:** INV-SHELL-1, INV-SHELL-2, INV-CAMERA-1.
- **Obligations:** T1 IPC tests.
- **Revisit trigger:** shell design (Level D); the PD-F2-01 disposition.
- **1R amendment:** shell navigation commands are atomic. Animated navigation is not part of Foundation 01 (INV-CAMERA-1). Shell delta streams form a ScaleInteractionEpoch (INV-SCALE-3).

## ADR-12: performance/idle philosophy and the OB-19 Phase F entry gate

- **Status:** Accepted (C14, OB-19; INV-IDLE-1, INV-ALLOC-1).
- **Context:** Prototype 01 redrew continuously (EV-P14). Budgets invented without a baseline are fiction. Authority boundaries erode at the first implementation commit if their enforcement is postponed.
- **Decision:**
  - No invented budgets; each is frozen only after its T4 baseline.
  - No Infynitae wakeups while product-idle; every timer has an obligation behind it.
  - No heap allocation on core hot paths in steady state.
  - The first production-workspace commit must install `verify-architecture`, the forbid/deny lints and the locked pins before any feature work (OB-19, unconditional).
- **Evidence:**
  - D: C14.1–C14.3.
  - H/compiler: EC-1, EC-15 (gate feasibility).
  - Facts: 165/144 Hz frame times (C14.1).
- **Alternatives:** best-effort performance; enforcement later in Phase F.
- **Rejected:** best-effort hides regressions; late enforcement is exactly when layering collapses.
- **Consequences:** The early commits of Phase F are tooling. Performance claims wait for T4.
- **Invariants:** INV-IDLE-1, INV-ALLOC-1, INV-ARCH-1, INV-ARCH-2, INV-ARCH-3.
- **Obligations:** OB-19 (gate) and the C14 measurements.
- **Revisit trigger:** T4 baselines (to freeze budgets). The gate itself is not revisitable without a user decision.

## ADR-13: exact Smithay pin with adapter-local workarounds

- **Status:** Accepted (D-B12-PIN, C9; INV-ARCH-3). This is a consolidation: the decision recurs across ADR-01/03/07 and is worth recording on its own.
- **Context:**
  - Infynitae relies on specific Smithay 0.7.0 behaviour and on 13 catalogued quirks (W-SM-1…13), several of them confirmed defects (W-SM-2, W-SM-4b, W-SM-11, W-SM-12).
  - Phase D added two source-level behaviours with authority impact: `PopupKeyboardGrab` (ED-3) and the `OutputDamageTracker` scale (ED-4).
- **Decision:**
  - Exact `=0.7.0` pin with a committed lockfile.
  - Every workaround lives in the adapter.
  - A dependency patch only under the patch policy (reproduced, no public-API fix, upstream issue).
  - A workaround is removed only after its experiment passes without it.
- **Evidence:** S/H: EB-2…EB-19, EC-10/11/13, X-12, ED-3, ED-4.
- **Alternatives:** track git master; fork Smithay; write our own protocol layer.
- **Rejected:** git master is unevaluated and drifts; a fork carries maintenance cost; our own protocol layer is disproportionate.
- **Consequences:**
  - Upgrades follow B12.5 and re-run the suite (including d5hit).
  - Grab-related behaviour must be mirrored in core (PD-F2-02).
- **Invariants:** INV-ARCH-3.
- **Obligations:** OB-11, OB-18, OB-26.
- **Revisit trigger:** a Smithay release fixing W-SM-4b/11/12; the PD-F2-02 disposition.
- **1R:** W-SM-14 (a nested popup grab replacing the pointer grab tears down the keyboard grab) and W-SM-15 (pointer-first grab teardown refocuses the root) were added, both with adapter-local workarounds (ER-1).
- **E1R amendment (unsealed, E-F2-02):** P1 is the first patch candidate meeting criterion (1) reproduced (EE-2) and criterion (2) no public-API fix (W-SM-4c). Criterion (3), an upstream issue or PR, is open (OB-11) and is the user's outward action. The pin stays the published crate until that decision; P1 lives only in the isolated experiment `experiments/e1r-smithay-p1`.

## ADR-14: element-level focus authority vs protocol surface routing (new, 1R)

- **Status:** Accepted (Phase D 1R decision; INV-INPUT-8 amended, INV-INPUT-10 new).
- **Context:**
  - Smithay's popup grabs decide keyboard and pointer routing while a popup grab is active.
  - `PopupKeyboardGrab` ignores `set_focus` from outside, and Smithay restores focus to the root itself (ED-3).
  - A single "keyboard focus" written by both core and Smithay diverged in practice (ER-1 NEG-1).
- **Decision:**
  - Two concepts at two levels.
  - **ActiveElementFocus** (core, sole policy authority) answers "which Stage element owns desktop focus".
  - **ProtocolSurfaceRoute** (adapter, Smithay grabs) answers "where, inside that element's surface tree, protocol events route during a valid popup grab". It is subordinate and never changes ActiveElementFocus.
  - A core change of ActiveElementFocus preempts incompatible grabs in a fixed order (INV-INPUT-10).
- **Evidence:**
  - ER-1: PF-1…PF-7 pass, including parent destroyed, popup destroyed in flight, nested grabs and keys held.
  - Negative controls: the naive path diverges; pointer-first teardown flickers; replacing the pointer grab on nesting loses routing.
- **Alternatives:**
  - (a) Rename and keep two writers.
  - (b) Disable Smithay popup grabs and route popups ourselves.
  - (c) Defer the focus change until the client dismisses its popups.
- **Rejected:**
  - (a) keeps the divergence.
  - (b) re-implements protocol grab semantics that Smithay already gets right within an element.
  - (c) lets a client veto a user's focus action.
- **Consequences:**
  - Opening a popup never changes desktop focus.
  - The shell, activation and focus bindings act on elements.
  - Smithay's root restore is accepted as routing inside the same element.
  - Two Smithay quirks become adapter workarounds (W-SM-14/15).
- **Invariants:** INV-INPUT-8, INV-INPUT-10, INV-INPUT-5 (constraint activation needs ActiveElementFocus), INV-PROTO-1 (`activated`).
- **Obligations:** OB-30, OB-18.
- **Revisit trigger:** a Smithay release changing `PopupKeyboardGrab`/`PopupPointerGrab` semantics; T3 toolkits whose popups rely on keeping focus across element switches.
<!-- /living -->
