# Infynitae Native — Foundation Implementation Plan (D8)

Status: **Phase D checkpoint 2 (D8) living document; a plan only.** Re-audited in the unsealed Phase E corrective checkpoint E1R (audit/E1R/D8_REAUDIT_E1R.md). Canonical living home of `INC-*` (FOUNDATION_ARCHITECTURE §P0). Nothing here creates a crate, manifest, workspace or source file. Planning an increment does not authorize running it.

**Ordering rule.** Retire the highest architectural, authority and integration uncertainty as early and cheaply as possible, while respecting real dependencies.

The order derives from:
- the risk register (FOUNDATION_RISKS.md), especially the risks that harness evidence has not yet transferred to the production seam (NB-12, A-C1);
- the obligation graph (FOUNDATION_TEST_PLAN.md);
- authority dependencies (FOUNDATION_INVARIANTS.md).

It is **not** "finish core → finish Wayland → finish renderer → finish input".

**Work categories** (exactly one per increment):
- **ENFORCE:** probes, dependency/source checks, lints, the doc checker. Closes enforcement obligations only.
- **HARNESS:** experiment rigs. Never linked into product crates. Produces evidence only.
- **PRODUCT:** production crates. Closes T0/T1/T2 obligations only when the production path is exercised.
- **PHYSICAL:** T3–T5 validation and tuning. Never scheduled for execution in Phase F without separate user authorization.

**Execution classes:**
- **HEADLESS:** no interaction with the desktop session.
- **NESTED:** a client inside the live session; explicit per-run authorization.
- **EXCLUSIVE_HARDWARE:** DRM/KMS/VT ownership; separate explicit authorization; never casually during the user's working session.

Future crate names follow ARCHITECTURE C1.1: `infynitae-core`, `infynitae-wayland`, `infynitae-backend-headless`, `infynitae-backend-winit`, `infynitae` (bin), `infynitae-ipc` (C7). Phase G adds `backend-drm`.

## Dependency graph

```
INC-0 (ENFORCE: OB-19 + OB-36)
  └─► INC-1 (PRODUCT: thin vertical seam)  ◄── first product code; falsifies the harness→production assumptions
        └─► INC-2 (multi-output, output lifecycle token, membership, frames observed)
              ├─► INC-3 (pointer intake, CameraPan @ zoom 1)
              │     ├─► INC-4 (keyboard, ActiveElementFocus, popup routing)
              │     │     └─► INC-5 (pointer constraints, reconciliation, output removal, zero outputs) ◄─ OB-11 decision before any constraint ships (E1R)
              │     └─► INC-6 (scale policy, ScaleInteractionEpoch) ◄── needs INC-2 membership + INC-3 interactions
              ├─► INC-7 (toplevel requests: fullscreen/maximize/activated) ◄── needs INC-4 focus
              └─► INC-8 (off-screen frames: withhold/kick/fallback)
INC-9 (HARNESS: OB-24 evidence) ──► [GATE G-OB24: architecture change control] ──► INC-10 (PRODUCT: zoomed navigation)
                                         (or conservative fallback active)          needs INC-3, INC-6
INC-11 (shell IPC) ◄── INC-6, INC-7
INC-12 (xwayland-satellite) ◄── INC-7 (pre-map fullscreen), INC-4
INC-13 (winit + GLES, NESTED T2) ◄── INC-2, INC-6
[GATE G-OB34: architecture change control] ──► INC-14 (interactive move/resize) ◄── INC-6, INC-5
[GATE G-OB48: persistence identity decision] ──► cross-session camera persistence as a product claim
[GATE G-MIRROR] shared cameras / mirroring: outside Foundation 01
INC-15 (PHYSICAL T3)   INC-16 (PHYSICAL T4, Phase G DRM)   INC-17 (PHYSICAL T5)   — each separately authorized
```

<!-- living:INC -->
### INC-0 — Slice 0: OB-19 entry gate (and OB-36)
- **Category:** ENFORCE
- **Execution class:** HEADLESS
- **Objective:** the first Phase F commit creates the workspace skeleton and the enforcement **before** any protocol or product behaviour.
- **Crates/modules:** workspace manifests; `verify-architecture` (ported from the spike's `verify.sh` incl. probes 2d/2e/2f/2g); CI jobs for `verify-architecture` and `tools/check-foundation-docs` (OB-36, a separate job).
- **Authoritative state introduced:** none (type skeletons for ports, the `OutputLifecycleToken` and `Authorized` only as needed by the probes).
- **Seam touched:** none.
- **Invariants exercised:** INV-ARCH-1, INV-ARCH-2, INV-ARCH-3, INV-CAMERA-1, INV-OUTPUT-2.
- **Risks retired:** TA-6, C-NB-2 (type-level part), RB-6.
- **Obligations:** OB-19 (unconditional gate), OB-36.
- **Exit tests / negative controls:** every probe fails with its asserted diagnostic and no other error; DAG and source checks pass; `--locked` build; checker plus `--self-test`.
- **Entry conditions:** Phase F authorized.
- **Stop / rollback:** any probe passing or failing for the wrong reason stops Phase F; no other increment may begin.
- **Architecture decision required first:** no.
- **Why first:** authority boundaries erode at the first implementation commit if enforcement is postponed (ADR-12). It is the cheapest increment and retires the risk of a protocol-reachable camera path being written at all.

### INC-1 — Slice 1: minimal real vertical seam
- **Category:** PRODUCT
- **Execution class:** HEADLESS
- **Objective:** a deliberately thin end-to-end path through the production boundaries: map one element (root + subsurfaces + one popup) → public Smithay snapshot extraction → core model → render list → DummyRenderer → physical-pixel hit-test.
- **Scope:** one output and one camera at zoom 1, through the `output_connected` path but with no hotplug, no input routing and no scale policy.
- **Crates/modules:** core `stage`, snapshot type and `origin()` (F-ORIGIN-1), `projection` (F-VIEW-1, F-LATTICE-1, F-HIT-1); adapter snapshot extraction, registry, `convert` (F-RENDER-1), minimal effect application; headless backend (one virtual output, DummyRenderer).
- **Authoritative state introduced:** OBJ-01, OBJ-02, OBJ-05, OBJ-06, OBJ-12, OBJ-15.
- **Seam touched:** Smithay compositor/xdg-shell handlers → snapshot; core → render elements; core hit-test.
- **Invariants exercised:** INV-STAGE-1, INV-STAGE-2, INV-STAGE-3, INV-COORD-1, INV-COORD-2, INV-COORD-6, INV-RENDER-1, INV-RENDER-2, INV-HIT-1, INV-ARCH-1.
- **Risks retired or materially tested:** NB-12 (first production evidence), A-C1, TC-1, TC-9, RB-5, W-SM-1/5/11/12 avoidance (TA-5).
- **Obligations:** OB-2, OB-6 (T0 part), OB-26, OB-1 (one popup level).
- **Exit tests (parity, FOUNDATION_TEST_PLAN §4):** TST-09 xsnap/X-15, TST-10 xhit, TST-04 x6, TST-18 d5hit zoom-1 configurations on the production path.
- **Negative controls:** i32 root (x6), canvas-exact hit rule (xhit), `Window::surface_under` (xsnap), plus a source check that `Space` is never constructed.
- **Entry conditions:** INC-0 exit.
- **Stop / rollback:** a failure here falsifies the core/adapter seam. Stop and return to architecture change control before writing any subsystem.
- **Architecture decision required first:** no.
- **Why before any subsystem:** the most dangerous untested assumption is that Phase B–D harness results (all T1-style, non-production) transfer to the production core/adapter boundary (NB-12, A-C1). This slice tests it with the least code. Any alternative (input first, renderer first) builds on an unverified seam.

### INC-2 — Multi-output, output lifecycle, membership, observed frames
- **Category:** PRODUCT
- **Execution class:** HEADLESS
- **Objective:** several outputs through `output_connected`/`output_reconfigured`/`output_disconnected` (startup, runtime hotplug and runtime mode/transform/scale changes); camera creation/binding precedence (1:1, collision rule); per-surface membership in `view_O` space; damage trackers sized `phys_O`; the zero-output state; frame clock; presentation feedback (observed elements).
- **Crates/modules:** core output manager, `navigation` camera lifecycle, visibility pass, frame clock; adapter `Output` globals and enter/leave; headless backend hotplug.
- **Authoritative state introduced:** OBJ-07, OBJ-08, OBJ-09, OBJ-10, OBJ-13, OBJ-17 (observed part).
- **Seam touched:** `wl_output` globals, `wl_surface.enter/leave`, presentation-time.
- **Invariants exercised:** INV-OUTPUT-1, INV-OUTPUT-2, INV-CAMERA-1, INV-CAMERA-3, INV-RENDER-3, INV-FRAME-1, INV-FRAME-2, INV-FRAME-5.
- **Risks retired:** TT-3, TC-9, RB-5, A-C1 (popup membership).
- **Obligations:** OB-3 (removal part, without interactions), OB-19 runtime counterpart (token-only output entry), OB-45 (tracker, membership, damage part), OB-49, OB-50 (core part).
- **Exit tests:** the 2R output-lifecycle tests ported; TST-03 SC1/SC3 and X5; TST-06 x9; E1R: the EE-1 transform matrix (tracker size, membership, damage) on the production adapter; `output_reconfigured` tests (EE-4) and reconnect as a separate case; zero-output core state (no fabricated output or coordinate; first connect leaves it).
- **Negative controls:** `Window::output_enter` popup membership (X5); a probe that the output entry is not callable without the token; E1R: the sealed tracker size (transformed mode) must fail EE-1 C1–C4; reconfiguration probes p10–p13; sealed F-RELOCATE-1 on an empty survivor set must be unreachable.
- **Entry conditions:** INC-1 exit.
- **Stop / rollback:** any camera write outside the token or port entries stops the increment.
- **Architecture decision required first:** no.
- **Why next:** multi-output projection is the product's defining feature and the second highest integration risk. Every later increment (input, scale, frames) depends on membership.

### INC-3 — Pointer intake and CameraPan at zoom 1
- **Category:** PRODUCT
- **Execution class:** HEADLESS
- **Objective:** `PhysicalEvent` intake through `InputPort`; ownership table; interaction state machine (M22, CameraPan only); pointer routing with F-HIT-1; KeepFocus; free-pointer reconciliation.
- **Crates/modules:** core `input::{ownership, interaction, bindings}`, `navigation::pan` (F-PAN-1 with zoom = 1); adapter pointer delivery; headless injected input.
- **Authoritative state introduced:** OBJ-20, OBJ-25, OBJ-26.
- **Seam touched:** `wl_pointer`, relative pointer.
- **Invariants exercised:** INV-INPUT-1, INV-INPUT-2, INV-INPUT-3, INV-INPUT-4, INV-COORD-3, INV-COORD-5, INV-CAMERA-2, INV-CAMERA-4.
- **Risks retired:** TP-1, TT-4 (pan part), NB-12 (input path).
- **Obligations:** OB-4 (acquisition part), OB-21 (T0 part only; T4 later); OB-45 (hit-test part), OB-50 (pointer part).
- **Exit tests:** TST-05 x8 (48 checks) on the production path; zero configures during pan (client-observed); E1R: EE-1 C5 render/hit agreement on Normal/90/180/270 (+ flipped) with the corrected F-HIT-1; zero-output pointer transition (EE-3, no constraint).
- **Negative controls:** the INV-INPUT-3 bypass switch (motion delivered during pan) must fail.
- **Entry conditions:** INC-2 exit.
- **Stop / rollback:** any pan-originated client event.
- **Architecture decision required first:** no (zoom stays 1; see the OB-24 gate).
- **Why next:** input ownership is the authority boundary most exposed to games (DV-3); it is cheap to test headless.

### INC-4 — Keyboard ownership, ActiveElementFocus, popup routing
- **Category:** PRODUCT
- **Execution class:** HEADLESS
- **Objective:** K-rules (M12); ActiveElementFocus (M11); popup grabs with ordered preemption (INV-INPUT-10; W-SM-14/15 workarounds); `activated` mapping.
- **Crates/modules:** core `input::keys`, FocusModel; adapter keyboard intercept/forward, popup grab installation/preemption.
- **Authoritative state introduced:** OBJ-22, OBJ-23 (adapter), OBJ-27 (Smithay).
- **Seam touched:** `wl_keyboard`, `xdg_popup.grab`.
- **Invariants exercised:** INV-INPUT-6, INV-INPUT-7, INV-INPUT-8, INV-INPUT-10, INV-PROTO-1 (`activated`).
- **Risks retired:** TA-3, TP-2, RB-11, RB-2.
- **Obligations:** OB-30, OB-15, OB-4, OB-18 (popup part).
- **Exit tests:** TST-07 x10 (+ K3-disabled NEG); TST-20 xpopup_focus PF-1…PF-7.
- **Negative controls:** NEG-1 naive `set_focus`, NEG-2 pointer-first, OBS-1 nested grab replacing the pointer grab, all expected to fail.
- **Entry conditions:** INC-3 exit.
- **Stop / rollback:** any divergence between core focus and Smithay focus is a Level-A regression (PD-F2-02 class); stop.
- **Architecture decision required first:** no.
- **Why next:** this was a Level-A finding (PD-F2-02) and depends on Smithay grab internals; it must be proven on the production adapter early.

### INC-5 — Pointer constraints, reconciliation, output removal under interaction
- **Category:** PRODUCT
- **Execution class:** HEADLESS
- **Objective:** M13: activation policy, batch-end reconciliation with deferral under compositor interactions and client grabs, confinement clamp (F-CONFINE-1), output removal with relocation (F-RELOCATE-1).
- **Crates/modules:** core PointerModel/constraint policy; adapter constraint activation/deactivation (W-SM-4).
- **Authoritative state introduced:** OBJ-21.
- **Seam touched:** `zwp_pointer_constraints_v1`.
- **Invariants exercised:** INV-INPUT-5, INV-COORD-5, INV-OUTPUT-1.
- **Risks retired:** TA-4, TF-1, TP-3, TC-7, RB-12.
- **Obligations:** OB-31, OB-3, OB-11 (**decision point**: upstream fix or patch per D-B12-PIN; E1R: covers W-SM-4c), OB-20 (T0/T1 parts), OB-47, OB-50 (constraint part), OB-51.
- **Exit tests:** TST-21 xlock_grab DEFER (persistent and oneshot) on the production path; TST-05 x8 C3/C4; TST-23 R5 confinement cases; E1R: EE-2 sequences E1–E11 and EE-3 zero-output constraint cases on the production path (OB-47, OB-50).
- **Negative controls:** SEALED (divergence) must fail; EXPLICIT/persistent (duplicate `unlocked`) and TST-08 x12 (W-SM-4b) fail on the published Smithay and pass only with the OB-11 decision applied (EE-7); EE-2 on the published Smithay shows W-SM-4c; EE-3 N3 (`unset_grab` under a lock) must fail.
- **Entry conditions:** INC-4 exit.
- **Stop / rollback:** core/Smithay constraint divergence.
- **Architecture decision required first:** OB-11 must be decided before shipping **any** production constraint behaviour (E1R: W-SM-4c affects persistent locks and confinements too, and INV-INPUT-5 depends on P1 or an upstream fix). Starting the increment is allowed.
- **Why next:** games depend on it, and it was two Level-A-rigor findings (PD-F2-03/04).

### INC-6 — Scale policy and ScaleInteractionEpoch
- **Category:** PRODUCT
- **Execution class:** HEADLESS
- **Objective:** F-SCALE-1/2/3; `D_max` queried from the (Dummy) renderer; initial hint; ScaleInteractionEpoch for CameraPan, CameraZoom (discrete only at zoom 1), ShellStream, WindowMove/Resize (kinds wired, interactions arrive later); SC-E; P7.
- **Crates/modules:** core `ScaleState`; adapter fractional-scale/preferred-buffer-scale emission.
- **Authoritative state introduced:** OBJ-16.
- **Seam touched:** `wp_fractional_scale_v1`, `wl_surface.preferred_buffer_scale`.
- **Invariants exercised:** INV-SCALE-1 … INV-SCALE-5, INV-IDLE-1 (settlement timer armed only while an epoch is open).
- **Risks retired:** TR-1, TR-2, TR-5, TR-6, RC-2.
- **Obligations:** OB-13 (enforcement, and the limit obtained through the renderer capability interface; incl. E1R zero-extent domain and integer bound), OB-28, OB-23 (T0/T1 part; tuning stays open), OB-42.
- **Exit tests:** TST-16, TST-17, TST-23 R2 and TST-24 as T0 properties; TST-03 SC2 on the production path; adversarial self-resizing client (T1); E1R: EE-5 properties (initial commit with no buffer, 0×N/N×0, first buffer with P7, integer emission within `D_max`).
- **Negative controls:** epoch disabled (40/40 churn); 1R camera-only rule for Stage moves (39).
- **Entry conditions:** INC-2 and INC-3 exits.
- **Stop / rollback:** a hint change per raw spatial event.
- **Architecture decision required first:** no. PROV-1…3 and PROV-5 stay provisional.
- **Why here:** the scale policy is the main source of client-visible churn (EC-17 cost). It needs membership (INC-2) and interactions (INC-3).

### INC-7 — Toplevel request policy: fullscreen, maximize, `activated`
- **Category:** PRODUCT
- **Execution class:** HEADLESS
- **Objective:** M2/M9: answer every request with exactly one configure; fullscreen presentation (F-PRESENT-1); maximize declined; W-SM-13 overrides; popup robustness (W-SM-6/7).
- **Crates/modules:** core `protocol` (FullscreenState, OutputPresentation); adapter xdg handlers.
- **Authoritative state introduced:** OBJ-19.
- **Seam touched:** `xdg_toplevel` requests, configures.
- **Invariants exercised:** INV-PRES-1, INV-PROTO-1, INV-CAMERA-4.
- **Risks retired:** TA-9, TT-7, RB-3.
- **Obligations:** OB-32, OB-22 (T1 part), OB-18, OB-1 (nested popups).
- **Exit tests:** TST-22 xmaximize on the production path; nested popup chain parity.
- **Negative controls:** record-only / deferral (0 answers) must fail.
- **Entry conditions:** INC-4 exit (needs ActiveElementFocus).
- **Stop / rollback:** any unanswered mandatory request.
- **Architecture decision required first:** no.
- **Why here:** mandatory protocol behaviour (Level-A rigor findings PD-F2-10/17); needs focus.

### INC-8 — Off-screen frame policy
- **Category:** PRODUCT
- **Execution class:** HEADLESS
- **Objective:** M16 complete: withhold, kick (incl. input and `popup_done`), fallback timer armed only with `frame_requested`, re-entry liveness; suspension reasons (none producible).
- **Crates/modules:** core frames; adapter callback completion; compositor timer.
- **Authoritative state introduced:** OBJ-17 (unobserved part), OBJ-18.
- **Seam touched:** `wl_surface.frame`, presentation feedback.
- **Invariants exercised:** INV-FRAME-1 … INV-FRAME-6, INV-IDLE-1.
- **Risks retired:** TR-3 (T1 part), TR-4, TT-5, TT-6, RB-9.
- **Obligations:** OB-16 (T1 part).
- **Exit tests:** TST-03 SC3, TST-06 x9; a real-time harness port with a callback-blocked client (TST-11 T1 part, HEADLESS).
- **Negative controls:** pure starve (kick and fallback disabled) blocks the client.
- **Entry conditions:** INC-2 exit.
- **Stop / rollback:** a timer armed with no pending callback (idle violation).
- **Architecture decision required first:** no. PROV-4 stays provisional.
- **Why here:** liveness and idle contract; independent of input, so it may run in parallel with INC-3…INC-7.

### INC-9 — OB-24 evidence: lattice behaviour under zoomed panning
- **Category:** HARNESS
- **Execution class:** HEADLESS
- **Objective:** produce the evidence that architecture change control needs for OB-24:
  - X-6/d5hit-class Smithay geometry runs of the sealed lattice versus the gesture-pinned-reference candidate under zoomed pans (widths, gaps, render/hit agreement);
  - re-pin cost.
- **Crates/modules:** experiments only; never linked into product crates.
- **Authoritative state introduced:** none.
- **Seam touched:** none (evidence).
- **Invariants exercised:** INV-RENDER-1, INV-HIT-1 (as subjects).
- **Risks retired:** none directly; provides the evidence for TC-2 and A-B1-1.
- **Obligations:** OB-24 (evidence part only).
- **Exit tests:** the experiment output recorded as evidence.
- **Negative controls:** the sealed lattice reproduces the ±1 px variation (ED-1).
- **Entry conditions:** none beyond Phase F authorization; may run any time before INC-10.
- **Stop / rollback:** n/a.
- **Architecture decision required first:** no; this increment feeds the decision.
- **Why before INC-10:** OB-24 cannot be resolved as an implementation-local choice.

### GATE G-OB24 (architecture change control, not an increment)
No PRODUCT increment may create a state where **persistent zoom ≠ 1 and camera pan coexist** until OB-24 is resolved through architecture change control: a C12.R3-style amendment with INC-9-class evidence.

**Status 2026-10-17 (as amended by C12.F14.5, Option A):** OB-24 is decided by the owner (D1–D6) and recorded as layer C12.F14 on the governance branch `governance/ob24-c12f14-2026-10-17`. C12.F14 amends this gate's interpretation: the gate requires the recorded architecture decision — C12.F14 on `main` (owner integration authorization) — not OB-24's closure. Once C12.F14 is on `main` the gate is **met** and INC-10 may begin (with its other entry conditions). **OB-24 stays OPEN** until INC-10's implementation and its exit evidence (below) are complete and accepted through architecture change control; it closes only then. **Met since 2026-10-17:** C12.F14 is on local `main` (`270128f`, CHECKPOINT "LAYER C12.F14 INTEGRATED"; not pushed). OB-24 remains OPEN.

**Conservative fallback** if OB-24 is unresolved when INC-10 is reached: navigation from a fitted view first restores the baseline zoom, so persistent zoom ≠ 1 never coexists with pan. The fit itself stays one-shot (INV-CAMERA-5). Discrete or continuous user zoom features stay out.

### INC-10 — Zoomed navigation (viewport-fit, zoomed pan, restore)
- **Category:** PRODUCT
- **Execution class:** HEADLESS
- **Objective:** one-shot `FitViewport` and restore (F-FIT-1 and OBJ-30 as amended by C12.F15); zoomed pan (F-PAN-1) under G-OB24 (met, C12.F14.5); the output's kept lattice reference (F-LATTICE-1 as amended by C12.F14); the canonical-rect render wrapper and the damage-tracker invalidation (INV-RENDER-1, F-RENDER-1 as amended by C12.F15).
- **Crates/modules:** core `navigation::{fit, restore}`, core projection (kept reference); `infynitae-wayland` `convert` (canonical-rect wrapper, crop) and `outputs` (tracker rebuild on a k change); shell and input bindings for fit.
- **Authoritative state introduced:** OBJ-30 (navigation history); `R_O` (OBJ-12, C12.F14).
- **Seam touched:** the adapter's Smithay render handoff (C12.F15), inside `convert`.
- **Invariants exercised:** INV-CAMERA-5, INV-COORD-3, INV-SCALE-1, INV-RENDER-1 at k ∉ n/120, INV-HIT-1, INV-ALLOC-1.
- **Risks retired:** TC-1 (production), TC-3, TA-1 (production confirmation).
- **Obligations:** OB-14 (zoom ≠ 1 part), OB-24 (consumes the decision; its exit evidence is this increment's), OB-53 (its part).
- **Exit tests:** TST-18 d5hit zoomed configurations; TST-23 R1 and TST-19 as T0 properties; the 200-commit "client resize never moves the camera" test on the production path, intact. **OB-24 / C12.F15 exit evidence** (production path; software-renderer results are labelled as such and are not GPU or hardware measurements):
  1. 0 width/gap changes per zoomed pan with the kept reference.
  2. Render/hit agreement on real Smithay geometry through the wrapper: generic k swept by single ulps around rounding points; kept references at ±2²⁰ and ±2³¹ physical px, on both sides of R, on both axes; adjacent surfaces seam-free in Pixman pixel checks where Pixman can represent the magnification; column-by-column pointer targeting; ProbeRenderer `dst`/`src` checks beyond Pixman's range (C12.F15.3).
  3. The deterministic rounding-step test (`kept_reference_rounding_steps_at_the_visible_edge_limit`) and the Q16 test at `|v − R|` = 2³⁷ + 119 px, k = 2⁻²² (C12.F15.7).
  4. F-FIT-1 (C12.F15): fit dimensions below, at and above the lower clamp boundary; a 1 px W at scale 0.25 reaching the upper clamp; scale 2 with a W that exactly fits the view, within ±1 px; zero-size and unmapped W — no mutation, no history entry; centre clamping at +2³⁶ and −2³⁶; every resulting camera accepted by `View::new`; restore returns the exact previous value, also after consecutive fits, and an empty history refuses; exactly one true-position event after a fit with the pointer on O and no interaction (none during one).
  5. Damage invalidation on a zoom change (C12.F15.4 regression test).
  6. i32 cropping beyond the representable range, including rotated and flipped buffer transforms (C12.F15.5); a transform the crop cannot serve is reported to change control.
  7. Map, unmap and destroy during a zoomed pan.
  8. Performance at zoom ≠ 1 compared with zoom 1: p50, p99 and maximum frame time, and INV-ALLOC-1; measured, not estimated.
  9. The seeded controls of C12.F15.8, each DETECTED; TST-04's control redesigned, not deleted.
  10. All repository gates on the exact final implementation commit.
- **Negative controls:** the sealed pan formula (content ≠ hand); a sticky-fit probe (E0277); C12.F15.8.
- **Entry conditions:** INC-3 and INC-6 exits (RATIFIED, CHECKPOINT); C12.F14 on `main` (`270128f`, G-OB24 met); C12.F15 adopted and committed; the integration record in CHECKPOINT ("LAYER C12.F14 INTEGRATED"); and an owner decision on OBJ-30's history depth (C12.F15.9) before restore ships. Or the conservative fallback.
- **Stop / rollback:** any camera change from client geometry; any presented rect that differs from Core's canonical rect.
- **Architecture decision required first:** **yes: OB-24's decision**, recorded by C12.F14 (G-OB24, C12.F14.5), and the render handoff and fit rule, recorded by C12.F15. OB-24 itself stays OPEN until this increment's exit evidence is accepted through architecture change control.
- **Why here:** zoom is reachable only through fit in Foundation 01. It is valuable but not foundational, and it is gated.

### INC-11 — Shell IPC
- **Category:** PRODUCT
- **Execution class:** HEADLESS
- **Objective:** `infynitae-ipc` with `ShellPort`; semantic commands (Navigate, FitViewport, Focus, MoveElement); generation-stamped Snapshot/Subscribe; shell streams as epochs; attention lifecycle.
- **Crates/modules:** `infynitae-ipc`; core shell entry.
- **Authoritative state introduced:** OBJ-31, OBJ-04.
- **Seam touched:** Unix socket (`SO_PEERCRED`).
- **Invariants exercised:** INV-SHELL-1, INV-SHELL-2, INV-CAMERA-1, INV-SCALE-3, INV-STAGE-3.
- **Risks retired:** TA-7, TT-9, TF-5.
- **Obligations:** OB-35, OB-53 (its part).
- **Exit tests:** stale-ID, reconnect and command-atomicity T1 tests; shell stream epoch case (TST-23/TST-24 cases on the production path).
- **Negative controls:** a raw-state command does not exist (probe); shell stream without epoch (churn).
- **Entry conditions:** INC-6, INC-7 exits.
- **Stop / rollback:** any shell path writing state outside the single-writer paths.
- **Architecture decision required first:** no (shell UX PROV-10 stays provisional).

### INC-12 — xwayland-satellite supervisor
- **Category:** PRODUCT
- **Execution class:** HEADLESS (real satellite against the headless backend, private socket)
- **Objective:** M19: compositor-owned X sockets with `-listenfd`; restart on the same display; pre-map fullscreen path; activation.
- **Crates/modules:** bin supervisor.
- **Authoritative state introduced:** OBJ-32.
- **Seam touched:** satellite as a Wayland client.
- **Invariants exercised:** INV-XWAYLAND-1, INV-PRES-1 (pre-map).
- **Risks retired:** TF-4, TP-7 (T1 part).
- **Obligations:** OB-33, OB-9 (T1 part), OB-27 (T1 part).
- **Exit tests:** TST-14 c6-sat on the production path; the restart test of OB-33.
- **Negative controls:** default fullscreen handler (W-SM-13) maps without an anchor.
- **Entry conditions:** INC-4, INC-7 exits.
- **Stop / rollback:** any X11 path reaching Stage or cameras.
- **Architecture decision required first:** no.

### INC-13 — winit backend and GLES renderer (T2)
- **Category:** PRODUCT
- **Execution class:** **NESTED** (explicit per-run authorization)
- **Objective:** real GPU rendering: damage trackers built with Scale(k_O) (F-RENDER-1); renderer filters (PROV-6); `D_max` from the real renderer; host frame timing.
- **Crates/modules:** `infynitae-backend-winit`.
- **Authoritative state introduced:** none new (OBJ-29 on a real GPU).
- **Seam touched:** EGL/GLES, winit.
- **Invariants exercised:** INV-RENDER-1, INV-SCALE-2, INV-FAIL-1 (renderer recreation).
- **Risks retired:** TC-8, TF-2.
- **Obligations:** OB-7 (T2 smoke), OB-52 (real `D_max`; split from OB-13 on 2026-10-14).
- **Exit tests:** T2 smoke (map, render, pan, multi-window, unmap); renderer filters exercised; E1R: a rotated output (90° and 270°, fractional scale) renders under real GLES without clipping or gaps (NESTED; renderer-dependent part of OB-45; no T4 claim).
- **Negative controls:** `from_output` tracker at zoom ≠ 1 renders the wrong size (if INC-10 is active).
- **Entry conditions:** INC-2, INC-6 exits; user authorization for a nested run.
- **Stop / rollback:** any effect on the host session beyond being a client.
- **Architecture decision required first:** no.

### GATE G-OB48 (architecture decision, not an increment)
No PRODUCT increment may claim cross-session camera persistence (loading or saving the persistence file as a product feature) until OB-48 is decided. In-session remembering and the INV-CAMERA-3 collision rule are implemented in INC-2 and are not blocked.

### GATE G-MIRROR (scope boundary, not an increment)
Shared cameras (N outputs → 1 camera, mirroring) are not part of Foundation 01 (INV-CAMERA-3, E-F2-04). Adding them requires architecture change control covering invariants, CameraSet, lifecycle API, persistence, fit semantics (PD-F2-11), state machines and tests together.

### GATE G-OB34 (architecture change control, not an increment)
The cross-output drag definition (F-DRAG-1) is an open decision (OB-34). Interactive window move/resize may not start before it is recorded.

### INC-14 — Interactive window move/resize
- **Category:** PRODUCT
- **Execution class:** HEADLESS
- **Objective:** WindowMove/WindowResize interactions and protocol move/resize transfers; Stage writes through `stage::place` (R3); epochs of kind WindowMove/WindowResize.
- **Crates/modules:** core `input::interaction`, `stage::place`.
- **Authoritative state introduced:** none new (OBJ-26 kinds).
- **Seam touched:** `xdg_toplevel.move/resize`.
- **Invariants exercised:** INV-STAGE-1, INV-INPUT-2, INV-SCALE-3.
- **Risks retired:** TC-6.
- **Obligations:** OB-34 (consumes the decision), OB-53 (its part).
- **Exit tests:** TST-24 WindowMove/Resize cases on the production path (1 settle, not 39); drag stays consistent across an Arrangement crossing per the decision.
- **Negative controls:** epoch disabled for Stage moves (churn).
- **Entry conditions:** **G-OB34 resolved**; INC-5, INC-6 exits.
- **Stop / rollback:** a Stage write outside `stage::place`.
- **Architecture decision required first:** **yes: OB-34**.

### INC-15 — T3 real-client and toolkit campaign
- **Category:** PHYSICAL
- **Execution class:** HEADLESS (real-time private socket) or NESTED (explicit per-run authorization)
- **Objective:** toolkit behaviour.
- **Crates/modules:** none (test runs).
- **Authoritative state introduced:** none.
- **Seam touched:** real toolkits.
- **Invariants exercised:** INV-INPUT-7 (RC-1), INV-SCALE-* (client response), INV-FRAME-3, INV-XWAYLAND-1.
- **Risks retired:** RC-1, RB-1, TP-7, TR-3 (toolkit part).
- **Obligations:** OB-7, OB-9, OB-10, OB-12, OB-16, OB-17, OB-22, OB-23, OB-27, OB-37, OB-39, OB-40.
- **Exit tests:** per-obligation criteria; results per toolkit version.
- **Negative controls:** as each obligation specifies.
- **Entry conditions:** the relevant PRODUCT increments; **separate user authorization**.
- **Stop / rollback:** a client class misbehaves → workaround triage; never an invariant change without change control.
- **Architecture decision required first:** no.

### INC-16 — T4 physical hardware (Phase G DRM backend)
- **Category:** PHYSICAL
- **Execution class:** **EXCLUSIVE_HARDWARE** (separate explicit authorization; never during a working session)
- **Objective:** presentation timing, VRR, libinput devices, hotplug, idle and performance baselines (C14).
- **Crates/modules:** `backend-drm` (Phase G).
- **Authoritative state introduced:** none.
- **Seam touched:** DRM/KMS, libinput, libseat.
- **Invariants exercised:** INV-IDLE-1, INV-FRAME-2, INV-COORD-3 (devices).
- **Risks retired:** RB-8 (partly), TR-3.
- **Obligations:** OB-6 (physical), OB-7, OB-12, OB-21, OB-23, OB-25, OB-44.
- **Exit tests:** C14 procedures; T4 baselines freeze budgets.
- **Negative controls:** idle wakeup attribution.
- **Entry conditions:** Phase G; separate authorization.
- **Stop / rollback:** any risk to the user's session aborts the run.
- **Architecture decision required first:** no.

### INC-17 — T5 games, latency, long duration
- **Category:** PHYSICAL
- **Execution class:** **EXCLUSIVE_HARDWARE**
- **Objective:** games off-screen and under pans; 24 h stability; latency.
- **Crates/modules:** none new.
- **Authoritative state introduced:** none.
- **Seam touched:** real games.
- **Invariants exercised:** INV-FRAME-3, INV-INPUT-3, INV-INPUT-5.
- **Risks retired:** RB-8, TR-3 (games).
- **Obligations:** OB-5, OB-8, OB-20, OB-22 (games), OB-37, OB-41.
- **Exit tests:** per-obligation criteria.
- **Negative controls:** as specified.
- **Entry conditions:** INC-16; separate authorization.
- **Stop / rollback:** as INC-16.
- **Architecture decision required first:** no.
<!-- /living -->

## Provisional values in the plan

No increment freezes a provisional value. PROV-1…PROV-11 (FOUNDATION_ARCHITECTURE §PROV) freeze only through their owning obligations at the tier their records name. Increments use the candidate values and keep them configurable.

## Next-risk rationale (summary)

| Step | Retires | Nearest alternative | Why the chosen step first |
|---|---|---|---|
| INC-0 | authority-path risk at the very first commit | start with core types | enforcement after code exists is the documented failure mode (ADR-12) |
| INC-1 | NB-12 / A-C1: harness evidence vs the production seam | input or renderer first | every later increment rests on this seam; it is the cheapest falsifier |
| INC-2 | multi-output projection, output authority (PD-F2-18 class) | input | membership is a prerequisite of scale, frames and focus semantics |
| INC-3 → INC-5 | input authority (Level-A-class findings PD-F2-02/03/04) | scale first | authority divergence is worse than churn; constraints need focus |
| INC-6 | churn contract | frames | needs membership + interactions; frames are independent and can follow |
| INC-7 | mandatory protocol answers | IPC | needs focus; protocol compliance precedes shell features |
| INC-8 | liveness/idle | — | independent; can parallelise |
| INC-9 → G-OB24 → INC-10 | the open zoom decision | ship zoom with the residual | OB-24 must not become an implementation default |
| INC-11 → INC-14 | shell, X11, real GPU, move/resize | — | lower architectural uncertainty; G-OB34 gates move/resize |
| INC-15 → INC-17 | physical truth | — | separately authorized; never implied by Phase F |
