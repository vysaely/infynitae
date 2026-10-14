# KNOW-RES-009: DriftWM Camera, Viewport, Coordinates and Hit-Testing — What Is Known, Compared With Infynitae

| Field | Value |
| :--- | :--- |
| **id** | KNOW-RES-009 |
| **title** | DriftWM Camera, Viewport, Coordinates and Hit-Testing — What Is Known, Compared With Infynitae |
| **subsystem** | Research/DriftWM, Core/Navigation, Core/Input |
| **source_rev** | DriftWM 352333a8fa1b22171492d4b71a54102045c9a19d (`README.md`, `dev/docs/caveats.md`); driftwm_agy_audit.md §2.2, §6.1; Infynitae main f323c2d |
| **date** | 2026-10-14 |
| **evidence_class** | SOURCE-DERIVED |
| **applicability** | INC-6 (scale), INC-8 (frame callbacks), INC-14 (interactive move/resize, grabs), any future camera animation |
| **affected_branches** | main (Infynitae facts); N/A for DriftWM |
| **related_invariants** | INV-CAMERA-1..5, INV-COORD-1..6, INV-PRES-1, INV-INPUT-8 |
| **adoption_status** | RESEARCH |
| **next_action** | If a camera-animation or zoomed-navigation design starts, do the bounded DriftWM source read listed under Unverified Hypotheses first |

---

## Verified Facts
1. **Coverage gap (the main finding).** No recovered study examines DriftWM's camera, viewport, coordinate types, constant-time camera movement or projection in depth. The AGY audit gives them one table row (§2.2) and the rest of the recovered material mentions them in passing. Searched: `~/.gemini/antigravity/scratch/` (six reports), both AGY trajectories, all Claude session logs on this machine, every branch of the repository. Not searched: the old Arch partition (not mounted). Nothing below goes beyond what those sources and DriftWM's own documentation state.
2. **DriftWM, as recorded.**
   - Model: "windows live at their native size on an infinite 2D canvas, and your display is a camera viewing it" (README); pan, zoom, scroll momentum, zoom-to-fit overview, configurable camera anchors.
   - Camera and zoom are per output, stored in `OutputState` behind a per-output mutex; `set_camera_on` refuses while the output is fullscreen, and `enter_fullscreen` writes `output_state` directly to get past that guard (audit §2.2).
   - Fullscreen relocates the window to the camera position and forces zoom 1.0 (rejected for Infynitae: KNOW-PITFALL-001).
   - `wl_surface.enter`/`leave` membership comes from one function, `refresh_window_outputs`, not from Smithay's `Space`; membership sent from elsewhere caused a game on another output to leave fullscreen when a camera panned over its parked window (caveats.md line 9).
   - Window animations are drawn over a state change that is already complete, so input hit-tests the destination; a camera animation is a real viewport movement, so input tracks it live (caveats.md line 89).
   - Pointer focus is a *pull*: `refresh_pointer_focus` re-runs the full hit-test cascade once per event-loop iteration and once per rendered frame and sends only a changed delivery. DriftWM's stated reason: its hit test reads state that changes on many commit paths with no single chokepoint, so a dirty flag "is the naive pull with more code" (caveats.md line 95).
   - Hit-testing walks `Stage` entries linearly (audit §8; VERIFIED_OPPORTUNITIES §2.2).
3. **Infynitae, on main `f323c2d`.**
   - Canvas coordinates are Q16 `i64` (2⁻¹⁶ px), |v| ≤ 2³⁶, anchors and surface origins integral (INV-COORD-1); rounding only at R1–R8 (INV-COORD-2); coordinate domains are separate Rust types and Smithay geometry is built only in the adapter's `convert` (INV-COORD-4).
   - Cameras are centre-anchored views (Q16 centre, zoom), bound 1:1 to outputs, created only at `output_connected`, kept (unbound, value unchanged) when the output goes (INV-CAMERA-3). Every camera mutation consumes an `Authorized` proof minted only from an input port, shell port or output-lifecycle token; no client or protocol event moves a camera (INV-CAMERA-1).
   - **Camera movement does not touch elements.** "A camera change never changes any element's Canvas geometry" (INV-CAMERA-2); a pan sends no configure and no pointer event, and ends with one true-position event (INV-CAMERA-4). Direct-manipulation pan sums in f64 from the gesture origin and quantises once per update (INV-COORD-3).
   - **What is not constant-time.** Moving the camera changes one value, but the visibility and membership pass that follows is linear in mapped elements: "pan, render list, remap and hotplug remain linear in mapped elements" (CHECKPOINT, Core visibility remediation `85a74cb`; measured T0: pan 23.5 µs and render list 101 µs at 3,000 elements). The remediation added a bounding-box cull and binary-search `Stage` lookup; it did not add a spatial index.
   - **Projection and fullscreen.** A presentation is a projection override; the element's anchor never moves (INV-PRES-1, KNOW-ARCH-001).
   - **Hit-testing.** The pointer is hit-tested against the `ProjectionFrame` last shown on its output (F-HIT-1): a front-to-back scan over the frame's culled entries, so cost is linear in the surfaces in that frame, not O(1) (`Core::hit_test`, `render::ProjectionFrame::select`). One surface-origin function feeds render, membership, hit-test and input (INV-COORD-6).
   - **When focus is re-derived.** Not once per loop iteration: Core emits one true-position event (M10) at its own chokepoints — after a visibility/membership change on the pointer's output when no interaction is active, at the end of a pan, at presentation changes — stamped with the input clock (KNOW-ARCH-002). Every client commit already passes through Core, which is the chokepoint DriftWM says it lacks.
4. **Comparison.**
   | Idea | Status in Infynitae |
   | :--- | :--- |
   | Display as a camera over an infinite canvas | Implemented (foundation model) |
   | Per-output camera | Implemented, with 1:1 binding, proof-gated mutation and survival across unplug (stronger than DriftWM's mutex plus runtime guard) |
   | Camera move leaves window geometry alone | Implemented and sealed (INV-CAMERA-2) |
   | Fullscreen by relocating the window | Rejected (KNOW-PITFALL-001) |
   | Membership from one place | Implemented (Core membership; INV-COORD-6 single origin function) |
   | Window animation = hit-test destination; camera animation = live | Still useful when animation arrives; INV-CAMERA-1 already says any time-driven camera step needs its own proof |
   | Pull focus every iteration | Not adopted; Infynitae reconciles at Core chokepoints. Whether pull would cost more *in Infynitae* was never measured (see X-08) |
   | Linear hit-test without spatial index | Same shape in both (culled scan in Infynitae); a spatial index is an open idea, not a plan |
   | Zoom-to-fit overview, camera anchors | Viewport-fit exists as a one-shot authorized action (INV-CAMERA-5); anchors not specified |

## Unverified Hypotheses
- Bounded follow-up if needed (not done here, per the owner's instruction to avoid new large research): read DriftWM's camera math, zoom pick-mode switch and viewport animation in the local clone at `352333a` (`src/state/viewport_animation.rs`, the canvas-math unit tests, `src/input/mod.rs` hit-test cascade). DriftWM is GPL-3.0-or-later: record concepts only, copy no code.
- That a spatial index would pay off below several thousand windows is unmeasured; at 3,000 elements the culled pan is already 23.5 µs (T0 model).

## Performance & Correctness Implications
- "Constant-time camera movement" is true of the camera value, not of the frame: whoever claims O(1) panning must include the visibility pass.
- Any future camera animation (INC-8 or later) must be authorized per step (INV-CAMERA-1) and must keep input on the live viewport, which matches DriftWM's lesson.

## Limitations & Counterexamples
- The DriftWM facts are second-hand (AGY's audit and DriftWM's own documentation), not re-read in source for this record.
- The coverage gap may be closed by material on the old Arch partition.

## Reproduction Instructions
```bash
git show main:docs/foundation/FOUNDATION_INVARIANTS.md | grep -A1 -E '^### INV-(CAMERA|COORD)-'
git show main:crates/infynitae-core/src/render.rs | grep -n -A3 'pub fn select'
sed -n '9p;89p;93,95p' ~/.gemini/antigravity/scratch/driftwm/dev/docs/caveats.md
```

## Links & Citations
- Related: [KNOW-RES-001](KNOW-RES-001-driftwm-state-ownership.md), [KNOW-RES-002](KNOW-RES-002-driftwm-fullscreen-lifecycle.md), [KNOW-ARCH-001](../architecture/KNOW-ARCH-001-camera-projection-independence.md), [KNOW-ARCH-004](../architecture/KNOW-ARCH-004-spatial-membership-culling.md), [KNOW-PITFALL-001](../pitfalls/KNOW-PITFALL-001-physical-canvas-fullscreen-relocation.md), [KNOW-PITFALL-002](../pitfalls/KNOW-PITFALL-002-global-focus-pull-loops.md)
- Corrections: [CONTRADICTIONS_AND_SUPERSEDED.md](../CONTRADICTIONS_AND_SUPERSEDED.md) X-08
