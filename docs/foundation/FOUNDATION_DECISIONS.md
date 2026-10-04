# Infynitae Native — Foundation Decisions (ADRs)

Phase D checkpoint 1 (D6). Baseline: Phase C seal `615b0ed2f3631b698b0c54783c28f696ef4ff34a`.

These records explain **why** each foundational decision was made. **What** the rules are is normative in [FOUNDATION_ARCHITECTURE.md](FOUNDATION_ARCHITECTURE.md) (canonical registry §C12) and [FOUNDATION_BLOCKERS.md](FOUNDATION_BLOCKERS.md). Phase D findings are cited by ID ([audit/README.md](audit/README.md)).

An ADR whose decision is touched by an open D-F2 finding has the status **"Accepted; under Phase D review"**. The decision stands until the user disposes of the finding.

Evidence class: P protocol · S Smithay source · M math/model · H harness experiment (T1-style) · R real client · D design policy · V provisional · X deferred physical validation.

| ADR | Title | Status |
|---|---|---|
| ADR-01 | Stage is the sole spatial authority; Smithay `Space` rejected | Accepted; under review (PD-F2-13) |
| ADR-02 | Fixed-point Q16 Canvas with a reference-relative lattice | Accepted; under review (PD-F2-06, PD-F2-12) |
| ADR-03 | Window-geometry anchor and exact physical-pixel hit-testing | Accepted |
| ADR-04 | Independent centre-anchored cameras | Accepted; under review (PD-F2-01, -06, -11) |
| ADR-05 | Unified effective scale: MaxGesture, bounded ladder hint, settlement | Accepted; under review (PD-F2-07, -08, -09) |
| ADR-06 | Off-screen withholding with kick/fallback; suspension separated from visibility | Accepted; under review (PD-F2-14) |
| ADR-07 | Pre-client input ownership, gesture isolation, modifier observation | Accepted; under review (PD-F2-02, -03, -04, -05, -17) |
| ADR-08 | Compile-time camera-authority separation | Accepted; under review (**PD-F2-01, Level A**) |
| ADR-09 | Protocol fullscreen is output presentation; viewport-fit is a camera operation | Accepted; under review (PD-F2-01, -10, -17) |
| ADR-10 | X11 via xwayland-satellite only | Accepted; under review (PD-F2-16) |
| ADR-11 | Non-authoritative semantic shell IPC | Accepted; under review (PD-F2-01, -15) |
| ADR-12 | Performance/idle philosophy and the OB-19 Phase F entry gate | Accepted |
| ADR-13 | Exact Smithay pin with adapter-local workarounds | Accepted |

---

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
- **Invariants:** STAGE-1, STAGE-2, STAGE-6, RENDER-3, RENDER-4.
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
- **Invariants:** COORD-1, COORD-2, COORD-3, COORD-4, RENDER-1.
- **Obligations:** OB-6, OB-24.
- **Revisit trigger:** a zoom-motion feature (OB-24), or PD-F2-06 disposed with option (b); a renderer that accepts sub-pixel positioning.

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
- **Invariants:** STAGE-2, COORD-6, HIT-1, RENDER-2.
- **Obligations:** OB-1, OB-2, OB-14, OB-17, OB-26.
- **Revisit trigger:** a protocol-level change to window-geometry semantics; T3 evidence that W-SM-9 re-clamping harms real clients.

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
- **Invariants:** CAMERA-2, CAMERA-3, CAMERA-5, OUTPUT-1.
- **Obligations:** OB-3, OB-24.
- **Revisit trigger:** a decision on PD-F2-01/-06/-11; the zoom-motion feature.

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
- **Invariants:** SCALE-1 … SCALE-5.
- **Obligations:** OB-7, OB-12, OB-13, OB-23, OB-28.
- **Revisit trigger:** X-16 tuning (TUNE); a disposition of PD-F2-07/-08/-09.

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
- **Invariants:** FRAME-1 … FRAME-6, IDLE-1.
- **Obligations:** OB-5, OB-8, OB-16, OB-25.
- **Revisit trigger:** X-7 game/network results; evidence of benefit for a `StableUnobserved(T)` reason.

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
- **Invariants:** INPUT-1 … INPUT-9, COORD-5.
- **Obligations:** OB-3, OB-4, OB-10, OB-11, OB-15, OB-16, OB-20, OB-21.
- **Revisit trigger:** a disposition of PD-F2-02…05/-17; T5 game results (DV-3); a Smithay fix for W-SM-4b.

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
- **Invariants:** CAMERA-1, SHELL-1, ARCH-1.
- **Obligations:** OB-19 (probe list to be extended per PD-F2-01).
- **Revisit trigger:** the PD-F2-01 disposition; any new camera behaviour.

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
- **Invariants:** PRES-1, CAMERA-5, SCALE-1.
- **Obligations:** OB-22.
- **Revisit trigger:** OB-22 results; a PD-F2-10 disposition.

## ADR-10: X11 via xwayland-satellite only

- **Status:** Accepted (D-C6; INV-XWAYLAND-1). Under review: PD-F2-16 (restart display identity; `-listenfd` unverified).
- **Context:** An internal XWM brings ICCCM/EWMH, X11 grabs and self-positioning windows, all of which are second spatial authorities.
- **Decision:** xwayland-satellite runs as an ordinary Wayland client under a supervisor. There is no internal XWM, and X11 position requests are ignored.
- **Evidence:** R: EC-12 (toplevel mapping; a move request had no effect; the crash path) and EC-13 (pre-map fullscreen).
- **Alternatives:** Smithay's `xwayland` feature with an internal XWM.
- **Rejected:** the internal XWM is a second authority and a large surface (C16 non-goal).
- **Consequences:** X11 apps lose self-positioning and do not survive a satellite crash. Override-redirect fidelity is OB-9.
- **Invariants:** XWAYLAND-1, STAGE-1.
- **Obligations:** OB-9, OB-27.
- **Revisit trigger:** T3 evidence that satellite fails a required app class.

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
- **Invariants:** SHELL-1, SHELL-2, CAMERA-1.
- **Obligations:** T1 IPC tests.
- **Revisit trigger:** shell design (Level D); the PD-F2-01 disposition.

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
- **Invariants:** IDLE-1, ALLOC-1, ARCH-1, ARCH-2, ARCH-3.
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
- **Invariants:** ARCH-3.
- **Obligations:** OB-11, OB-18, OB-26.
- **Revisit trigger:** a Smithay release fixing W-SM-4b/11/12; the PD-F2-02 disposition.
