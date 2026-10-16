# Infynitae Native — Foundation Invariants (effective registry)

Status: **Phase D checkpoint 2 (D7) living document, amended by the UNSEALED Phase E corrective checkpoint E1R and by the Phase F amendment layers C12.F1, C12.F2, C12.F3, C12.F4, C12.F5, C12.F6, C12.F7, C12.F8, C12.F10, C12.F11, C12.F12 and C12.F13 (adopted by owner approval).** Canonical living home of the `INV-*` namespace (FOUNDATION_ARCHITECTURE §P0).

This registry is the **effective projection** of sixteen layers. The layers themselves stay where they are and are not edited:
- **C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a**: FOUNDATION_ARCHITECTURE §C12 (sealed Phase C registry, 54 rows);
- **C12.R1 @ 84ce620898445aaadb72f386ec3954e5bbfe2c2f**: FOUNDATION_ARCHITECTURE §C12.R1 (Phase D 1R amendments, 16 rows);
- **C12.R2 @ 5827926782089dc8c6a1c203f844fbc879fce9be**: FOUNDATION_ARCHITECTURE §C12.R2 (Phase D 2R amendments, 6 rows);
- **C12.E1R @ UNSEALED (E1R review gate; base 09075a2)**: FOUNDATION_ARCHITECTURE §C12.E1R (Phase E corrective amendments, 6 rows; not sealed);
- **C12.F1 @ ADOPTED 2026-10-13 (owner approval; base 2436dd4)**: FOUNDATION_ARCHITECTURE §C12.F1 (Phase F amendment: default placement at first map, 1 row);
- **C12.F2 @ ADOPTED 2026-10-13 (owner approval; base 2436dd4)**: FOUNDATION_ARCHITECTURE §C12.F2 (Phase F amendment: device bound on the 1/120 lattice, 1 row);
- **C12.F3 @ ADOPTED 2026-10-14 (owner approval; base d6714f6)**: FOUNDATION_ARCHITECTURE §C12.F3 (Phase F amendment: `configure_bounds(0, 0)`, 1 row);
- **C12.F4 @ ADOPTED 2026-10-14 (owner approval; base d6714f6)**: FOUNDATION_ARCHITECTURE §C12.F4 (Phase F amendment: epoch time authority, 1 row);
- **C12.F5 @ ADOPTED 2026-10-14 (owner approval; base 93c2605)**: FOUNDATION_ARCHITECTURE §C12.F5 (Phase F amendment: default placement at the first windowed presentation, 1 row);
- **C12.F6 @ ADOPTED 2026-10-14 (owner approval; base 93c2605)**: FOUNDATION_ARCHITECTURE §C12.F6 (Phase F amendment: a pre-map grant takes the output when its element maps, 1 row);
- **C12.F7 @ ADOPTED 2026-10-14 (owner approval; base 93c2605)**: FOUNDATION_ARCHITECTURE §C12.F7 (Phase F amendment: focus eligibility and retention under a presentation, 1 row);
- **C12.F8 @ ADOPTED 2026-10-14 (owner approval; base 93c2605)**: FOUNDATION_ARCHITECTURE §C12.F8 (Phase F amendment: popup stacking follows xdg-shell, 1 row);
- **C12.F10 @ ADOPTED 2026-10-14 (owner approval; base 80ead2a)**: FOUNDATION_ARCHITECTURE §C12.F10 (Phase F amendment: unmap of a fullscreen element, RC-1, 1 row; C12.F9 is reserved for the INC-6 exit — withdrawn 2026-10-16; the number stays unused);
- **C12.F11 @ ADOPTED 2026-10-16 (owner approval; base eebb5c3)**: FOUNDATION_ARCHITECTURE §C12.F11 (Phase F amendment: the device bound counts a size Core asks for, OB-13, 1 row);
- **C12.F12 @ ADOPTED 2026-10-16 (owner approval; base eebb5c3)**: FOUNDATION_ARCHITECTURE §C12.F12 (Phase F amendment: the initial hint of a pre-map fullscreen grant, 1 row);
- **C12.F13 @ ADOPTED 2026-10-16 (owner approval; base eebb5c3)**: FOUNDATION_ARCHITECTURE §C12.F13 (Phase F amendment: whose stimulus a presentation change is, 1 row).

Rules:
- Each entry's **wording** is the statement of its latest layer, verbatim, except that complete formula text is replaced by an `[F-…]` reference.
- The replaced text is recorded in that formula's block in FOUNDATION_ARCHITECTURE §F (the only canonical home of `F-*`). `tools/check-foundation-docs` reverses every substitution and verifies the result equals the historical layer row byte for byte.
- **Consequence of violation** is the C12 "Violation means" column. For the three invariants first introduced in 1R/2R it is a D7-authored description, marked as such (D-F0). E1R adds no invariant.
- **ADR** links are derived from FOUNDATION_DECISIONS.md. **Obligations** are cited by ID; their records live in FOUNDATION_TEST_PLAN.md.

Count: **57** effective invariants: 37 from C12, 6 from C12.R1, 2 from C12.R2, 5 from C12.E1R, 0 from C12.F1 (its row is superseded by C12.F5), 1 from C12.F2, 1 from C12.F3, 1 from C12.F4, 1 from C12.F5, 0 from C12.F6 (its row is superseded by C12.F10), 1 from C12.F7, 1 from C12.F8, 1 from C12.F10.

<!-- living:INV -->

### INV-STAGE-1
- **Wording:** Stage is the **only** store and writer of persistent placement (anchors) and z-order. Writers: `stage::place` via (a) placement policy — a provisional anchor at initial commit, and the default placement resolved **once, at the element's first windowed presentation**, when its windowed window-geometry size is authoritative: its first map, unless the element is in protocol fullscreen then, in which case the first commit that applies an acknowledged configure without the fullscreen state and is made while the element is not in protocol fullscreen; an explicit, restored or already resolved anchor is final and is not resolved again; protocol fullscreen never writes an anchor and never resizes the element — (b) interactive drag commit, (c) shell `MoveElement` (same path).
- **Owner:** core `Stage`
- **Class:** D
- **Violation:** second spatial authority (Level A)
- **Origin:** C12.F5 @ ADOPTED 2026-10-14 (owner approval; base 93c2605)
- **Layers:** C12 → C12.F1 → C12.F5
- **Trigger:** owner amendment 2026-10-14 (placement at the first windowed presentation, P9b)
- **Evidence:** B2–B5, C2; evidence/phase-f/placement-amendment; evidence/phase-f/placement-windowed-presentation
- **Tests (historical column):** T0 single-writer API tests; T0 placement properties against an exact model; T1 INV-3 (X-8); T0 `a_window_keeping_the_fullscreen_size_is_placed_at_its_first_windowed_commit`; T1 `placement_after_fullscreen_waits_for_the_acknowledgement`
- **Obligations:** —
- **Formulas:** F-DRAG-1, F-PLACE-1
- **ADR:** ADR-01, ADR-10

### INV-STAGE-2
- **Wording:** An element's anchor is the Canvas position of its xdg window-geometry origin; all surface origins derive from it via one function (INV-COORD-6)
- **Owner:** core
- **Class:** P+S+D
- **Violation:** render/hit/membership disagreement
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** B2.3
- **Tests (historical column):** T1 xsnap/xhit
- **Obligations:** —
- **Formulas:** F-ORIGIN-1
- **ADR:** ADR-01, ADR-03

### INV-STAGE-3
- **Wording:** `ElementId` = `NonZeroU64`, monotonic, never reused within a process; external references are `(InstanceId, ElementId)` and resolve or fail with `StaleElement`
- **Owner:** core `Stage`
- **Class:** D
- **Violation:** ABA aliasing
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** B4
- **Tests (historical column):** T0 property; T1 shell stale-ID
- **Obligations:** —
- **Formulas:** —
- **ADR:** — (no ADR; decision recorded in D-B4)

### INV-STAGE-4
- **Wording:** Lifecycle follows B5's state/transition table; destroy handling is idempotent and order-independent (client death destroys objects in arbitrary order)
- **Owner:** core + adapter
- **Class:** S+D
- **Violation:** stale relationships, panics
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** B5, EB-5
- **Tests (historical column):** T0 state machine; T1 crash tests
- **Obligations:** —
- **Formulas:** —
- **ADR:** — (no ADR; decision recorded in B5 / D-B5)

### INV-STAGE-5
- **Wording:** Unmap retains `ElementId` and anchor; remap re-shows at the anchor on top of z-order
- **Owner:** core
- **Class:** D
- **Violation:** identity loss on remap
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** D-B5-REMAP
- **Tests (historical column):** T1
- **Obligations:** —
- **Formulas:** —
- **ADR:** — (no ADR; decision recorded in D-B5-REMAP)

### INV-STAGE-6
- **Wording:** No projected/rendered/derived value is ever written back to Stage, cameras or snapshots
- **Owner:** all
- **Class:** D
- **Violation:** derived state becomes authority
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** B1 F1, C2
- **Tests (historical column):** T0 (no API), review
- **Obligations:** —
- **Formulas:** —
- **ADR:** ADR-01

### INV-CAMERA-1
- **Wording:** Camera state is mutated only inside `core::navigation`. Every mutation, **including camera creation and output↔camera binding/unbinding**, consumes a fresh `Authorized` proof.<br><br>A proof is mintable only inside `physical_input(&mut InputPort)`, `shell_command(&mut ShellPort)`, or `output_connected`/`output_disconnected(&mut OutputLifecycleToken)` (INV-OUTPUT-2). The proof is not Clone, `!Send`, and consumed by value.<br><br>The output-lifecycle entries only create, bind or unbind cameras. They never take a camera position, centre or zoom from their caller, and never change an existing camera's value.<br><br>Core retains no proof, mode, target or tracking state permitting a later mutation. No protocol event or client-originated change causes a camera mutation. Ports and tokens are unforgeable, `!Send`, bound to their Core, and never stored in protocol state. Animation is not in Foundation 01; any later time-driven step must be an authorized action with its own proof.
- **Owner:** core `navigation`
- **Class:** E+D
- **Violation:** clients can move cameras
- **Origin:** C12.R2 @ 5827926782089dc8c6a1c203f844fbc879fce9be
- **Layers:** C12 → C12.R1 → C12.R2
- **Trigger:** PD-F2-01, PD-F2-18
- **Evidence:** EC-1, EC-15, ER-4, ER2-1
- **Tests (historical column):** T0 probes 2d/2e/2f (OB-19)
- **Obligations:** OB-19
- **Formulas:** —
- **ADR:** ADR-08, ADR-11

### INV-CAMERA-2
- **Wording:** A camera change never changes any element's Canvas geometry
- **Owner:** core
- **Class:** D+E
- **Violation:** cameras become spatial authority
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** I2, X-8 INV-3
- **Tests (historical column):** T0, T1
- **Obligations:** —
- **Formulas:** —
- **ADR:** ADR-04

### INV-CAMERA-3
- **Wording:** Cameras are centre-anchored views (centre Q16, zoom); per-output view origin [F-VIEW-1], where `view_O` is the output's physical mode after its transform (R7).<br><br>Cameras come into existence **only** through `output_connected` (INV-OUTPUT-2). For the connecting `OutputKey`, the camera is the first of:<br>1. the lowest-indexed camera remembered this session for that key that is **currently unbound** (re-bound, value unchanged);<br>2. a new camera seeded from the passive persisted lookup, unless a camera with that key already exists this session (a concurrent key collision), in which case 3 applies;<br>3. a new camera at Core's internal default placement (the placement policy itself is Level D).<br><br>Cameras survive output removal (unbound, value kept) and are not destroyed in-session. **Binding is 1:1 in Foundation 01:** each connected output is bound to exactly one camera, and a camera is bound to at most one output. Shared cameras (mirroring) are not part of Foundation 01 (gate G-MIRROR).
- **Owner:** core `navigation`
- **Class:** D+E
- **Violation:** lost views, wrong projection
- **Origin:** C12.E1R @ UNSEALED (E1R review gate; base 09075a2)
- **Layers:** C12 → C12.R2 → C12.E1R
- **Trigger:** E-F2-01, E-F2-04, E-F3-03
- **Evidence:** D-B7-CENTER, B7.2, ER2-1, EE-1, EE-4
- **Tests (historical column):** T0 (spike tests), T1 hotplug (OB-49)
- **Obligations:** OB-49
- **Formulas:** F-VIEW-1
- **ADR:** ADR-04

### INV-CAMERA-4
- **Wording:** Camera change alone causes no configure (size, state incl. `suspended`, bounds, popup position) and delivers no pointer/relative/button event from the pan gesture. It MAY cause enter/leave, scale changes per INV-SCALE-*, clock changes, presentation outcomes, and after termination one true-position pointer event.
- **Owner:** core
- **Class:** P+E
- **Violation:** pan visible to clients as geometry/input
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** B9.7, SC1, SC4, X-8
- **Tests (historical column):** T1 (client-observed configure count = 0 during pan)
- **Obligations:** —
- **Formulas:** —
- **ADR:** ADR-06, ADR-07

### INV-CAMERA-5
- **Wording:** Viewport-fit is a one-shot authorized navigation action. It snapshots the element geometry at that instant, computes centre and zoom for the invoking output, performs one camera mutation, and terminates. It is not a mode, stores no target, and is never re-fitted by client changes. Restore is a separate authorized action using the camera value recorded at fit time. There is no isolation filter in Foundation 01. A fit never changes an element's size or anchor.
- **Owner:** core `navigation`
- **Class:** E+D
- **Violation:** fit rewrites geometry
- **Origin:** C12.R1 @ 84ce620898445aaadb72f386ec3954e5bbfe2c2f
- **Layers:** C12 → C12.R1
- **Trigger:** PD-F2-01/-11
- **Evidence:** ER-4, ER-5 R3
- **Tests (historical column):** T0 (spike tests), T1
- **Obligations:** —
- **Formulas:** F-FIT-1
- **ADR:** ADR-04, ADR-09

### INV-COORD-1
- **Wording:** Canvas coordinates are Q16 `i64` (2⁻¹⁶ px), \|v\| ≤ 2³⁶; anchors and surface origins are integral
- **Owner:** core
- **Class:** E
- **Violation:** precision collapse, path dependence
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** B1 (E1–E9)
- **Tests (historical column):** T0
- **Obligations:** —
- **Formulas:** F-ROUND-1
- **ADR:** ADR-02

### INV-COORD-2
- **Wording:** Rounding happens only at R1–R8: [F-ROUND-1]. F1–F7 are forbidden.
- **Owner:** core
- **Class:** E+D
- **Violation:** jitter, drift
- **Origin:** C12.R1 @ 84ce620898445aaadb72f386ec3954e5bbfe2c2f
- **Layers:** C12 → C12.R1
- **Trigger:** PD-F2-12
- **Evidence:** B1.4, C3.2, ED-2 F7
- **Tests (historical column):** T0
- **Obligations:** —
- **Formulas:** F-ROUND-1
- **ADR:** ADR-02

### INV-COORD-3
- **Wording:** Direct-manipulation pan: [F-PAN-1], with the sum in f64 from the gesture origin and quantised once per update (R2), never per event. Per-event terms are summed if `s_O` or `z_C` change within the gesture. Every gesture total follows the once-per-update rule.
- **Owner:** core
- **Class:** E+M
- **Violation:** drift (6.1 px / 1e6 events)
- **Origin:** C12.R1 @ 84ce620898445aaadb72f386ec3954e5bbfe2c2f
- **Layers:** C12 → C12.R1
- **Trigger:** PD-F2-06
- **Evidence:** B1 E2b, X-8, ER-5 R1
- **Tests (historical column):** T0; T4 (OB-21)
- **Obligations:** OB-21
- **Formulas:** F-PAN-1, F-ROUND-1
- **ADR:** ADR-02, ADR-04

### INV-COORD-4
- **Wording:** Coordinate domains are distinct Rust types with no cross-domain arithmetic; Smithay geometry types are constructed only in the adapter's `convert` module
- **Owner:** core / adapter
- **Class:** D
- **Violation:** domain mix-ups
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** B2.7, C3.6
- **Tests (historical column):** T0 compile probes
- **Obligations:** —
- **Formulas:** —
- **ADR:** ADR-02

### INV-COORD-5
- **Wording:** Smithay's pointer global space is Canvas. The authoritative seat state is (a) **physical**: `(interaction output, OutputLogical p)`, and (b) **constraint**: `Option<{surface, kind lock/confine, lock point: SurfaceLocal}>`. A constraint never carries an output. The Canvas lock point is derived ([F-LOCK-1]). Smithay's location is derived per dispatch.
- **Owner:** core `PointerModel`
- **Class:** S+E
- **Violation:** wrong client coordinates (Prototype 01 bug)
- **Origin:** C12.R1 @ 84ce620898445aaadb72f386ec3954e5bbfe2c2f
- **Layers:** C12 → C12.R1
- **Trigger:** PD-F2-03
- **Evidence:** B2.5, X-8, ER-2
- **Tests (historical column):** T1
- **Obligations:** —
- **Formulas:** F-LOCK-1, F-UNPROJECT-1
- **ADR:** ADR-07

### INV-COORD-6
- **Wording:** One surface-origin function ([F-ORIGIN-1]) is the only source of surface Canvas origins for render, membership, hit-test and input
- **Owner:** core
- **Class:** E
- **Violation:** render/input disagreement
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** CR-1, EC-9, EC-20
- **Tests (historical column):** T1 xsnap/xhit
- **Obligations:** —
- **Formulas:** F-ORIGIN-1
- **ADR:** ADR-03

### INV-RENDER-1
- **Wording:** Physical edges: [F-LATTICE-1]; surfaces are built with `from_surface` at f64 location [F-RENDER-1]
- **Owner:** core (numbers) / adapter (Smithay call)
- **Class:** E
- **Violation:** gaps, jitter, scale-dependent seams
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** B1.5, X-6
- **Tests (historical column):** T1 X-6 port; T0 properties
- **Obligations:** —
- **Formulas:** F-LATTICE-1, F-RENDER-1
- **ADR:** ADR-02

### INV-RENDER-2
- **Wording:** Render order front-to-back: reverse Stage z-order; within an element each popup tree — the toplevel's `xdg_popup`s newest first (xdg-shell; W-SM-20) — then the root tree, each traversed downward
- **Owner:** core (from snapshot order)
- **Class:** E
- **Violation:** wrong stacking
- **Origin:** C12.F8 @ ADOPTED 2026-10-14 (owner approval; base 93c2605)
- **Layers:** C12 → C12.F8
- **Trigger:** owner amendment 2026-10-14 (xdg-shell popup stacking, P10 #3)
- **Evidence:** X-15 (C12.F8); W-SM-20; evidence/phase-f/popup-stacking
- **Tests (historical column):** T1 `a_newer_popup_is_stacked_above_every_older_one`, `ob1_nested_popup_chain_matches_what_is_presented`
- **Obligations:** —
- **Formulas:** —
- **ADR:** ADR-03

### INV-RENDER-3
- **Wording:** Output membership (`wl_surface.enter/leave`) is computed by Infynitae per surface from physical-pixel projection and applied via `Output::enter/leave`; leave is sent before an output global is destroyed; `Window::output_enter`/`Space` are not used
- **Owner:** core + adapter
- **Class:** P+E
- **Violation:** wrong enter/leave, wrong scale
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** D-B8-ENTER, C3.2[3], X5
- **Tests (historical column):** T1
- **Obligations:** —
- **Formulas:** F-MEMBER-1
- **ADR:** ADR-01

### INV-RENDER-4
- **Wording:** No spatial index unless C3.8's measured threshold is met; if introduced, it is derived from Stage and differentially tested
- **Owner:** core
- **Class:** D
- **Violation:** secondary authority
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** C3.8
- **Tests (historical column):** review + differential T0
- **Obligations:** —
- **Formulas:** —
- **ADR:** ADR-01

### INV-HIT-1
- **Wording:** Hit-testing selects the surface **presented** at the pointer's physical pixel (same edges/order/`c_eff` as rendering) whose input region (protocol add/subtract, half-open on the clamped surface-local point) contains the point; delivered coordinates are `q − origin` clamped into the surface. `Window::surface_under` is not used. **Supersedes D-B2-I9's ½-px tolerance.**
- **Owner:** core
- **Class:** E+P
- **Violation:** clicks land on hidden surfaces
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** EC-20, W-SM-11/12
- **Tests (historical column):** T1 xhit; T0 region evaluator
- **Obligations:** —
- **Formulas:** F-HIT-1, F-LATTICE-1, F-UNPROJECT-1
- **ADR:** ADR-03

### INV-INPUT-1
- **Wording:** Every physical event has exactly one owner, decided at press (before any client dispatch); releases follow the press owner; motion follows the active interaction
- **Owner:** core intake
- **Class:** E
- **Violation:** stuck buttons/keys, leaks
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** B10.3, X-8, X-10
- **Tests (historical column):** T1
- **Obligations:** —
- **Formulas:** —
- **ADR:** ADR-07

### INV-INPUT-2
- **Wording:** A compositor pointer interaction is acquired only when the client owns no pressed button; protocol move/resize/drag are the only client→compositor transfers
- **Owner:** core
- **Class:** P+E
- **Violation:** unmatched client button state
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** B10.3, X-8 C6
- **Tests (historical column):** T1
- **Obligations:** —
- **Formulas:** —
- **ADR:** ADR-07

### INV-INPUT-3
- **Wording:** While a compositor interaction owns the pointer, no absolute motion, relative motion or button event reaches any client; pointer focus is kept (KeepFocus)
- **Owner:** core + adapter
- **Class:** E
- **Violation:** game input leakage, lost locks
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** X-8 (48 checks), D-B11-FOCUS
- **Tests (historical column):** T1 (+ negative control)
- **Obligations:** —
- **Formulas:** —
- **ADR:** ADR-07

### INV-INPUT-4
- **Wording:** Relative motion is emitted only from physical deltas on the client path; nothing is replayed, synthesised or derived from camera changes
- **Owner:** core
- **Class:** E
- **Violation:** phantom motion
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** X-8 INV-4/5
- **Tests (historical column):** T1
- **Obligations:** —
- **Formulas:** —
- **ADR:** ADR-07

### INV-INPUT-5
- **Wording:** A constraint is activated only when its element holds ActiveElementFocus, its surface has pointer focus, the pointer is inside the region, and no compositor pointer interaction is being acquired.<br><br>**Reconciliation** runs once at the end of every core batch in which any of these changed: the interaction output's view or descriptor (including `output_reconfigured`); the constrained surface's geometry or origin; the output set; ActiveElementFocus; the end of a compositor interaction; the end of a client-owned grab sequence.<br><br>Reconciliation is **deferred** while a compositor pointer interaction or a client-owned grab sequence is active. While deferred, core and Smithay keep the same constraint state, no absolute motion reaches a locked surface, and relative motion comes only from physical deltas.<br><br>A lock is kept only if its derived lock point is observable through the interaction output and its element holds ActiveElementFocus.<br><br>A confinement clamps the physical pointer half-open, in the interaction output's OutputLogical space, to the region's projection; Arrangement crossing is disabled; a pointer outside the region after a view change ends it; there is no warp.<br><br>Every end emits `unlocked`/`unconfined` exactly once, and **only from the active state**: no deactivation event is ever sent for an inactive constraint — not on a later `leave()` after an explicit deactivate (any lifetime, any later batch), not on surface destruction, and not for a constraint that was never activated. An explicit `deactivate` is used only when the delivered focus does not change in that batch; otherwise Smithay's `leave()` does it (W-SM-4a). Smithay 0.7.0's `leave()` deactivates unconditionally and no public API avoids it (W-SM-4c), so this invariant depends on the one-line dependency patch P1 (`leave()` deactivates only an active constraint) or an equivalent upstream fix, adopted under the patch policy (INV-ARCH-3, OB-11).<br><br>At most one true-position motion per batch; no replayed or synthetic motion. Output removal relocates the physical pointer at once (F-RELOCATE-1) or, when no output survives, enters `NoOutput` (INV-OUTPUT-1). Destruction ends constraints through the protocol path.
- **Owner:** core + adapter
- **Class:** E+M
- **Violation:** lost/duplicate lock events
- **Origin:** C12.E1R @ UNSEALED (E1R review gate; base 09075a2)
- **Layers:** C12 → C12.R1 → C12.E1R
- **Trigger:** E-F2-02, E-F2-03, E-F2-05
- **Evidence:** B11.4, X-8, ER-2, ER-5 R5, EE-2, EE-3
- **Tests (historical column):** T1 (OB-3, OB-31, OB-47)
- **Obligations:** OB-3, OB-31, OB-47
- **Formulas:** F-RELOCATE-1, F-CONFINE-1, F-LOCK-1
- **ADR:** ADR-07, ADR-14

### INV-INPUT-6
- **Wording:** Keys whose press changes XKB modifier/lock state are never compositor-owned (bindings on them run in observe mode)
- **Owner:** core intake
- **Class:** E
- **Violation:** stuck modifiers
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** K3, X-10 + NEG
- **Tests (historical column):** T1 (+ negative control)
- **Obligations:** —
- **Formulas:** —
- **ADR:** ADR-07

### INV-INPUT-7
- **Wording:** Focus transitions use protocol semantics only (`leave` resets, `enter(keys)` + `modifiers`); no synthetic key events; compositor-owned keys are absent from `enter` and their releases are consumed; client-side destruction of the focused surface → successor `enter` without `leave`
- **Owner:** adapter (Smithay seat) + core
- **Class:** P+S+E
- **Violation:** logically impossible key state
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** K4–K9, X-10, EC-21
- **Tests (historical column):** T1; T3 (OB-10)
- **Obligations:** OB-10
- **Formulas:** —
- **ADR:** ADR-07

### INV-INPUT-8
- **Wording:** **ActiveElementFocus** (which element, or none, owns desktop keyboard focus) has one writer, the core FocusModel. It changes only by explicit action (click-to-focus, focus bindings, shell focus command, activation grant) or target loss, and never to an element **hidden by a presentation** — an output presents another element, the camera bound to that output would show this element, and no output observes it: such an action is declined and leaves the focus where it was, and the successor after a target loss is chosen among the other elements. Camera, visibility, pointer motion, protocol routing and fullscreen requests never change it: requesting or obtaining fullscreen gives a client no focus, and an element that holds the focus keeps it when a presentation or an output change hides it — it then receives keyboard input while hidden, until an explicit action or a target loss moves the focus. Delivered keyboard routing is always within the focused element's tree (INV-INPUT-10), or none.
- **Owner:** core `FocusModel`
- **Class:** D+E
- **Violation:** typing redirected by panning
- **Origin:** C12.F7 @ ADOPTED 2026-10-14 (owner approval; base 93c2605)
- **Layers:** C12 → C12.R1 → C12.F7
- **Trigger:** owner amendment 2026-10-14 (focus eligibility and retention, A-8)
- **Evidence:** D-B10-KBD, ER-1; evidence/phase-f/focus-eligibility
- **Tests (historical column):** T0, T1 (OB-30); T0 `focus_does_not_move_to_an_element_a_presentation_hides`; T1 `focus_does_not_move_to_a_window_the_presentation_hides`, `a_menu_chain_of_a_fullscreen_window`
- **Obligations:** OB-30
- **Formulas:** —
- **ADR:** ADR-07, ADR-14

### INV-INPUT-9
- **Wording:** xdg-activation tokens are validated by provenance (W-SM-8); activation yields focus grant, attention or nothing; never a camera change
- **Owner:** core `protocol`
- **Class:** D+E
- **Violation:** focus stealing, camera hijack
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** D-B10-ACTIVATION, EC-1
- **Tests (historical column):** T0 probes, T1
- **Obligations:** —
- **Formulas:** —
- **ADR:** ADR-07

### INV-SCALE-1
- **Wording:** ρ(E) = [F-SCALE-1]; the protocol-fullscreen presentation contributes `scale_O` (zoom 1); no fullscreen override
- **Owner:** core visibility
- **Class:** D+M
- **Violation:** blurry or overspent buffers
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** S1, CP2-Q1, Case C
- **Tests (historical column):** T0 model properties
- **Obligations:** —
- **Formulas:** F-SCALE-1
- **ADR:** ADR-05, ADR-09

### INV-SCALE-2
- **Wording:** Every emitted hint is a rung of the ladder in force at emission. The ladder contains every connected output scale ≤ H_max(E), the integers, and the geometric fill. [F-SCALE-2], where the device bound `H_dev(E)` is the largest hint expressible on the wire (a multiple of 1/120) for which `s · h ≤ D_max` on every side: `⌊120 · D_max / s_max(E)⌋ / 120` when `s_max(E) > 0` and `+∞` when `s_max(E) = 0`; `s_max(E)` = the largest logical side over all surfaces of E's tree and over every size Core has asked E for while that request stands: the size of the protocol-fullscreen configure while E is granted (C5.2: the output's logical size; at the initial commit, of the grant made in the initial configure), and the saved size offered in the configure that ended E's fullscreen (M9), until E commits a tree whose window-geometry size differs from the one it replaces, is unmapped, or is destroyed (0 iff none has a non-zero extent, e.g. every initial commit without a grant). With no buffer there is nothing to bound, so `H_max(E) = H_cfg`. `D_max` is queried from the renderer and re-queried on renderer recreation. If E's size makes h exceed the bound, h is lowered immediately (P7). A held or epoch-deferred hint may be off-ladder until the next permitted evaluation.
- **Owner:** core `ScaleState`
- **Class:** E+M
- **Violation:** unrenderable/huge buffers
- **Origin:** C12.F11 @ ADOPTED 2026-10-16 (owner approval; base eebb5c3)
- **Layers:** C12 → C12.R1 → C12.E1R → C12.F2 → C12.F11
- **Trigger:** owner amendment 2026-10-16 (C12.F11, OB-13)
- **Evidence:** EC-16/18, ED-2 F3/F4, EE-5; evidence/phase-f/h1-amendment; evidence/phase-f/INC-6
- **Tests (historical column):** T0 (every side at three device limits; resize alternation); T1 (OB-13)
- **Obligations:** OB-13, OB-52
- **Formulas:** F-SCALE-2
- **ADR:** ADR-05

### INV-SCALE-3
- **Wording:** **ScaleInteractionEpoch.** The spatial changes of one logical interaction form one epoch, for every `InteractionKind ∈ {CameraPan, CameraZoom, WindowMove, WindowResize, ShellStream}`: continuous pan, pinch, interactive window move or resize; a run of discrete pan or zoom steps; a bounded shell stream of navigation or `MoveElement` commands.<br><br>**During an epoch:** decreases are suppressed; increases happen only at rung crossings and only in the epoch's direction; client responses cannot reverse it; individual updates are not each a hint change.<br><br>**P7:** a device-bound decrease applies immediately and fixes the direction to down (correctness, not damping).<br><br>**The epoch ends** at interaction termination (gesture end, button release, explicit end-of-stream), at a settlement event (output add/remove/configuration), or, for discrete steps without an explicit end, at the **settlement timeout** with no further step. Pointer motion does not end it.<br><br>At settlement, h is set to the target of the final requirement, evaluated once. The timeout's existence is architectural; its value is provisional (C14.5; candidate 300 ms; OB-23). Atomic camera or Stage actions are one-step epochs. **Clock:** the settlement timeout is measured on the session's monotonic clock, from the time the last step of the interaction entered Core. A deadline is written only by a step of the interaction, never by a client response or any other evaluation, and it never moves earlier. Input-event timestamps and presentation timestamps are separate clocks and take no part.
- **Owner:** core
- **Class:** M
- **Violation:** per-event renegotiation
- **Origin:** C12.F4 @ ADOPTED 2026-10-14 (owner approval; base d6714f6)
- **Layers:** C12 → C12.R1 → C12.R2 → C12.F4
- **Trigger:** owner amendment 2026-10-14 (epoch time authority, H-2)
- **Evidence:** ER-5 R2, ER2-2
- **Tests (historical column):** T0 (+ NEG); T1/T3 (OB-23)
- **Obligations:** OB-23
- **Formulas:** F-SCALE-3
- **ADR:** ADR-05, ADR-11

### INV-SCALE-4
- **Wording:** (a) No hint change per raw spatial event (camera, Stage-move or resize update). (b) Changes happen only at rung crossings, at settlement, or by P7. (c) Per epoch: at most rungs crossed + 1 settlement + one P7 forced decrease per non-client stimulus. (d) No oscillation: outside an epoch, client-caused re-evaluations move h only in the direction of the last non-client stimulus (SC-E); inside an epoch, INV-SCALE-3 applies.
- **Owner:** core
- **Class:** M+E
- **Violation:** scale storms (EC-17: up to 477% client CPU)
- **Origin:** C12.R2 @ 5827926782089dc8c6a1c203f844fbc879fce9be
- **Layers:** C12 → C12.R1 → C12.R2
- **Trigger:** PD-F2-07, PD-F2-19
- **Evidence:** EC-17/19/22, ER-5 R2, ER2-2
- **Tests (historical column):** T0 (+ NEG); T1 (OB-28)
- **Obligations:** OB-28, OB-53
- **Formulas:** —
- **ADR:** ADR-05

### INV-SCALE-5
- **Wording:** At zoom 1 with no zoom interaction, and where the maximum observing output scale ≤ H_max(E), the policy equals Phase B MaxGesture, except that an element held under C14.4 (another client's presentation) keeps a higher hint until a non-client stimulus that touches it. Integer hint = ⌈h⌉, lowered to `max(1, ⌊H_dev(E)⌋)` when ⌈h⌉ > H_dev(E), so that an integer-scale client's buffer also fits the device; emission only on change; unobserved elements hold h; evaluation once per core batch after all its updates. **Initial hint:** `h := t(ρ_pred)` over the presentations whose view intersects the placed window-geometry rect (or the anchor point when the size is 0×0); if none, `t(max connected output scale)`. If the initial configure grants protocol fullscreen on output O (C12.F6), ρ_pred = max(`scale_O`, F-SCALE-1 over the current views of the other outputs whose view intersects the placed window-geometry rect, or the anchor point when its size is 0×0). The grant's own presentation contributes `scale_O` at zoom 1; O's current view does not count.
- **Owner:** core
- **Class:** M
- **Violation:** regressions vs Phase B, double renegotiation
- **Origin:** C12.F13 @ ADOPTED 2026-10-16 (owner approval; base eebb5c3)
- **Layers:** C12 → C12.R1 → C12.E1R → C12.F12 → C12.F13
- **Trigger:** owner amendment 2026-10-16 (C12.F13, presentation holds)
- **Evidence:** P4, S7, S8, AUD-25, ED-2 F8, EE-5; evidence/phase-f/INC-6
- **Tests (historical column):** T0; T1 (OB-13)
- **Obligations:** OB-13
- **Formulas:** F-SCALE-3
- **ADR:** ADR-05

### INV-FRAME-1
- **Wording:** The compositor only completes frame callbacks clients requested; it never invents them
- **Owner:** core + adapter
- **Class:** P
- **Violation:** protocol violation
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** D-B9-FRAMES 1
- **Tests (historical column):** T1
- **Obligations:** —
- **Formulas:** —
- **ADR:** ADR-06

### INV-FRAME-2
- **Wording:** Observed elements: completion once per frame of the clock output = highest-refresh observing output (ties by `OutputId`); presentation `sync_output` = clock output
- **Owner:** core
- **Class:** E+P
- **Violation:** double-rate or starved animation
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** D-B8-CLOCK, SC3
- **Tests (historical column):** T1
- **Obligations:** —
- **Formulas:** —
- **ADR:** ADR-06

### INV-FRAME-3
- **Wording:** Unobserved elements: callbacks withheld, except (a) the **kick**: before a ping, close, non-initial configure, `popup_done` or input event is delivered to the client, withheld callbacks complete once; (b) the **fallback**: at most one completion per period P (finite, configurable) measured from the element's last completion (kick or fallback), driven by a compositor timer armed only while such an element has a pending callback
- **Owner:** core
- **Class:** E
- **Violation:** starvation (vkcube) or wasted wakeups
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** EB-9, D-B9-FRAMES, AUD-24
- **Tests (historical column):** T1/T3 (+ negative control)
- **Obligations:** —
- **Formulas:** —
- **ADR:** ADR-06

### INV-FRAME-4
- **Wording:** Re-entry liveness: pending callbacks complete at the first vblank of the new clock output after observation resumes (≤ one period)
- **Owner:** core
- **Class:** S+E
- **Violation:** stuck clients after re-entry
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** EB-11, SC1/SC3
- **Tests (historical column):** T1
- **Obligations:** —
- **Formulas:** —
- **ADR:** ADR-06

### INV-FRAME-5
- **Wording:** Presentation feedback: superseded commits `discarded`; the latest unobserved commit stays pending and is `presented` when shown, or discarded on supersession/destruction
- **Owner:** Smithay mechanism + core
- **Class:** S+E
- **Violation:** wrong timing feedback
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** X-9
- **Tests (historical column):** T1
- **Obligations:** —
- **Formulas:** —
- **ADR:** ADR-06

### INV-FRAME-6
- **Wording:** `suspended` is set only from semantic reasons {SessionLocked, **OutputsOff**, Parked}, each defined without reference to camera-derived membership; never from camera visibility.
- **Owner:** core
- **Class:** E+D
- **Violation:** configure storms
- **Origin:** C12.R1 @ 84ce620898445aaadb72f386ec3954e5bbfe2c2f
- **Layers:** C12 → C12.R1
- **Trigger:** PD-F2-14
- **Evidence:** D-B9-SUSPEND, SC4
- **Tests (historical column):** T1
- **Obligations:** —
- **Formulas:** —
- **ADR:** ADR-06

### INV-PRES-1
- **Wording:** Protocol fullscreen is an output presentation state.<br><br>**Every `set_fullscreen`/`unset_fullscreen` is answered with exactly one configure.** A request is granted only if W holds ActiveElementFocus, or before W's first map. Otherwise it is declined at once (configure without `fullscreen`, nothing stored).<br><br>The presentation starts when the granted element is mapped — at the grant decision for a mapped element, at its first map for a grant made before it — and ends at the exit decision (unset, unmap, output removal, replacement). Grants are ordered by the time they were made. A grant replaces the older grants on its output when its element is mapped, not before: until then the presented element keeps the output and is told nothing, and a grant whose element is destroyed or unmapped before mapping ends nothing. A grant still waiting for its element's map is replaced by a newer grant whose element maps, never by the map of an older one. The replaced elements get a configure with their saved size, if available. An unmap drops the element's state without answering it: protocol fullscreen sends it no configure and offers it no saved size (a focus-loss configure, INV-PROTO-1, carries neither `fullscreen` nor a size), and its next initial configure is not fullscreen. Focus loss does not end fullscreen. The output choice runs at grant. Fullscreen never touches cameras or anchors; other cameras observe W at its actual size.
- **Owner:** core `protocol` + output model
- **Class:** P+D+E
- **Violation:** camera hijack, hidden size effects
- **Origin:** C12.F10 @ ADOPTED 2026-10-14 (owner approval; base 80ead2a)
- **Layers:** C12 → C12.R1 → C12.F6 → C12.F10
- **Trigger:** owner amendment 2026-10-14 (unmap of a fullscreen element, RC-1)
- **Evidence:** EC-13/14, ER-3; evidence/phase-f/deferred-takeover; evidence/phase-f/fullscreen-unmap-rc1
- **Tests (historical column):** T1 (OB-22, OB-32); T0 `a_pre_map_grant_takes_the_output_when_its_element_maps`, `fullscreen_ends_cleanly_on_unmap_destroy_and_output_changes`; T1 `a_pre_map_grant_takes_the_output_only_when_its_window_maps`, `a_pre_map_grant_that_never_draws_ends_nothing`, `unmap_remap_and_destruction_leave_no_fullscreen_state`
- **Obligations:** OB-22, OB-32
- **Formulas:** F-PRESENT-1
- **ADR:** ADR-09

### INV-OUTPUT-1
- **Wording:** Output removal enters Core **only** through `output_disconnected(&mut OutputLifecycleToken, …)`. Then: membership `leave` before the global is destroyed; camera unbound and kept (value unchanged); fullscreen presentation dropped with the INV-PRES-1 configure; interactions orphaned (ownership kept until release, no camera mutation); if the interaction output was removed and **another output survives**, the physical pointer is relocated at once to the Arrangement-nearest point of a surviving output (half-open, F-RELOCATE-1); constraints reconciled per INV-INPUT-5 (deferred under a client grab); the removal is a ScaleInteractionEpoch settlement event.<br><br>**Zero outputs.** When no output survives, the seat enters the stable `NoOutput` state (OBJ-20); no output and no coordinate is fabricated, and nearest-output selection is never evaluated on an empty set. In the same batch the adapter installs a compositor zero-output pointer grab with `Focus::Clear`, replacing any client implicit grab: the client receives `wl_pointer.leave`, Smithay ends only *active* constraints (one `unlocked`/`unconfined` each), and no absolute motion is delivered. It then submits one `motion(focus = None)` at Smithay's stored location, delivered to no one. While `NoOutput`, physical pointer motion, buttons and axes are consumed by core and delivered nowhere; ActiveElementFocus and keyboard routing are unchanged (not output-bound); cameras stay unbound with their values. The first `output_connected` leaves `NoOutput`: the zero-output grab is removed first, then the pointer is placed by the startup rule (centre of the new output's logical extent) with one routed motion, and constraint reconciliation follows (INV-INPUT-5).
- **Owner:** core output manager
- **Class:** D+E
- **Violation:** dangling protocol objects, stuck input
- **Origin:** C12.E1R @ UNSEALED (E1R review gate; base 09075a2)
- **Layers:** C12 → C12.R1 → C12.R2 → C12.E1R
- **Trigger:** E-F2-05
- **Evidence:** B7.2, C4.6, ER-2, ER2-1, EE-3, EE-4
- **Tests (historical column):** T1 (OB-3, OB-31, OB-50)
- **Obligations:** OB-3, OB-31, OB-50
- **Formulas:** F-RELOCATE-1
- **ADR:** ADR-04

### INV-SHELL-1
- **Wording:** The shell channel accepts only semantic commands executed through the same validated single-writer paths as user input; no raw state, size or memory writes; `ShellPort` lives only in `infynitae-ipc`
- **Owner:** core + ipc
- **Class:** D
- **Violation:** shell becomes arbitrary mutator
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** C7
- **Tests (historical column):** T0 (crate checks), T1
- **Obligations:** —
- **Formulas:** —
- **ADR:** ADR-08, ADR-11

### INV-SHELL-2
- **Wording:** The shell owns no authoritative state; every response/event is a generation-stamped derived copy; stale IDs fail; reconnect = Snapshot + Subscribe
- **Owner:** core + ipc
- **Class:** D
- **Violation:** shell restart loses truth
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** C7.3
- **Tests (historical column):** T1
- **Obligations:** —
- **Formulas:** —
- **ADR:** ADR-11

### INV-XWAYLAND-1
- **Wording:** X11 support is xwayland-satellite as an ordinary Wayland client: X11 windows are ordinary elements; no X11 path to Stage or cameras; satellite crash destroys its elements through the normal client-death path; no internal XWM
- **Owner:** adapter + supervisor
- **Class:** E
- **Violation:** second spatial authority
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** C6, EC-12, EC-13
- **Tests (historical column):** T1/T3
- **Obligations:** —
- **Formulas:** —
- **ADR:** ADR-10

### INV-ARCH-1
- **Wording:** Crate DAG: core has zero dependencies; all edges point to core; wayland ↛ backend; ipc ↛ wayland; backends ↛ each other
- **Owner:** workspace
- **Class:** E+D
- **Violation:** layering collapse
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** C1.3, EC-1
- **Tests (historical column):** T0 `verify-architecture` (OB-19)
- **Obligations:** OB-19
- **Formulas:** —
- **ADR:** ADR-08, ADR-12

### INV-ARCH-2
- **Wording:** `unsafe`: forbidden in core and ipc; elsewhere only listed, justified sites with no authority effect
- **Owner:** workspace
- **Class:** D
- **Violation:** memory/authority bypass
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** C9
- **Tests (historical column):** T0 lint + inventory review
- **Obligations:** —
- **Formulas:** —
- **ADR:** ADR-12

### INV-ARCH-3
- **Wording:** Smithay exact pin, locked graph; workarounds adapter-local; a dependency patch only per the patch policy; a workaround is removed only after its experiment passes without it
- **Owner:** workspace + adapter
- **Class:** D
- **Violation:** silent behaviour drift
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** D-B12-PIN, C9
- **Tests (historical column):** T0 `--locked`; upgrade re-run suite
- **Obligations:** —
- **Formulas:** —
- **ADR:** ADR-12, ADR-13

### INV-FAIL-1
- **Wording:** Recovery rebuilds derived state from authority; an internal invariant violation terminates rather than repairs; required-global failure aborts startup
- **Owner:** core + bin
- **Class:** D
- **Violation:** corrupt authority hidden by best effort
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** C8
- **Tests (historical column):** T1 failure injection
- **Obligations:** —
- **Formulas:** —
- **ADR:** — (no ADR; decision recorded in C8 / D-C8)

### INV-OBS-1
- **Wording:** Observability is never read by decision paths and runs no background subsystem
- **Owner:** all
- **Class:** D
- **Violation:** heisenbugs, idle cost
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** C10
- **Tests (historical column):** review; T0 (no read API)
- **Obligations:** —
- **Formulas:** —
- **ADR:** — (no ADR; decision recorded in C10 / D-C10)

### INV-IDLE-1
- **Wording:** When product-idle (C14.2), Infynitae generates no wakeups of its own: no polling, no periodic timers, no busy loops; every armed timer has a pending obligation behind it
- **Owner:** core + bin + backend
- **Class:** D
- **Violation:** battery/latency regressions
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** C14.2
- **Tests (historical column):** T4 idle measurement
- **Obligations:** —
- **Formulas:** —
- **ADR:** ADR-06, ADR-12

### INV-ALLOC-1
- **Wording:** Core hot paths (input intake, pan, pointer routing, hit-testing, visibility, render-list construction) perform no heap allocation in steady state (reused buffers); Smithay/backend allocations are isolated and measured
- **Owner:** core
- **Class:** D
- **Violation:** frame-time jitter
- **Origin:** C12 @ 615b0ed2f3631b698b0c54783c28f696ef4ff34a
- **Layers:** C12
- **Trigger:** —
- **Evidence:** C14.3
- **Tests (historical column):** T0 counting-allocator test
- **Obligations:** —
- **Formulas:** —
- **ADR:** ADR-12

### INV-INPUT-10
- **Wording:** **ProtocolSurfaceRoute** (Smithay popup grabs and their root restore) is adapter state, subordinate to ActiveElementFocus. It routes only within the focused element's tree and never changes ActiveElementFocus.<br><br>A core focus change to another element or to none, while a grab is active, is applied in one batch in this order:<br>1. `PopupGrab::ungrab(All)`: `popup_done` topmost first, kick first if the client is unobserved;<br>2. keyboard `unset_grab`;<br>3. pointer `unset_grab`;<br>4. `set_focus(target)`.<br><br>A nested grab keeps the installed popup pointer grab (W-SM-14). A grab whose root is not the focused element, or that fails the W-SM-7 precondition, is refused.
- **Owner:** adapter (Smithay grabs), core decides
- **Class:** S+E
- **Violation:** D7-authored (D-F0): keyboard routing to a surface outside the focused element; desktop focus and Smithay focus diverge (ER-1 NEG-1)
- **Origin:** C12.R1 @ 84ce620898445aaadb72f386ec3954e5bbfe2c2f
- **Layers:** C12.R1
- **Trigger:** PD-F2-02
- **Evidence:** ER-1, W-SM-14/15
- **Tests (historical column):** T1 (OB-30)
- **Obligations:** OB-30
- **Formulas:** —
- **ADR:** ADR-14

### INV-PROTO-1
- **Wording:** xdg_toplevel state policy (Foundation 01). `set_maximized`/`unset_maximized` are each answered with exactly one configure that does not grant `maximized`; they never invoke fit, move a camera or change the Stage (adapter overrides Smithay's default, W-SM-13). A pre-configure request is recorded and ignored. `set_minimized` is ignored. `activated` holds exactly when the element holds ActiveElementFocus and changes only on its transitions (a kick trigger if unobserved). No bounds are proposed in Foundation 01. `configure_bounds` is sent only as `(0, 0)` and only with a toplevel's initial configure, where Smithay emits it for a client that binds `xdg_wm_base` version 4 or later (W-SM-17); xdg-shell defines zero dimensions as bounds unknown, equivalent to the event never having been sent. A non-zero bound is never sent.
- **Owner:** core `protocol` + adapter
- **Class:** P+E
- **Violation:** D7-authored (D-F0): mandatory xdg_toplevel requests left unanswered, or maximize used as a camera/Stage path
- **Origin:** C12.F3 @ ADOPTED 2026-10-14 (owner approval; base d6714f6)
- **Layers:** C12.R1 → C12.F3
- **Trigger:** owner amendment 2026-10-14 (configure_bounds)
- **Evidence:** ER-3
- **Tests (historical column):** T1 (OB-32)
- **Obligations:** OB-32
- **Formulas:** —
- **ADR:** ADR-09, ADR-14

### INV-OUTPUT-2
- **Wording:** **Output lifecycle authority.** Output connect, reconfiguration and disconnect, at startup and at runtime alike, enter Core only through `output_connected(&mut OutputLifecycleToken, OutputDescriptor)`, `output_reconfigured(&mut OutputLifecycleToken, OutputId, OutputDescriptor)` and `output_disconnected(&mut OutputLifecycleToken, OutputId)`.<br><br>**The token** is a dedicated authority distinct from `InputPort`: minted only by `Core::new`; private fields (unforgeable); not Clone; `!Send` (never in protocol state); bound to its Core; held exclusively by the backend's output/display event source.<br><br>**`OutputDescriptor`** carries hardware facts only (`OutputKey`, physical mode before the transform, transform, scale, refresh), with no field for camera position, centre, zoom or binding.<br><br>**Reconfiguration** changes the hardware facts of a connected output and is not a reconnect. The `OutputId` is kept and no id is allocated; an unknown id is rejected, and so is a changed `OutputKey` (an identity change is a disconnect plus a connect). It mints no `Authorized` proof, so camera identity and value are unchanged. Derived state (`view_O`, ProjectionFrame, damage tracker, membership) is rebuilt from the new descriptor; the physical pointer on that output is clamped into its new extent (F-RECONF-1); the change is a ScaleInteractionEpoch settlement event and an INV-INPUT-5 reconciliation trigger. Client-visible effects are the `wl_output`/`xdg_output` property events and, only for an element in protocol fullscreen on that output, one configure with the new size.<br><br>**`OutputKey`** is a persistence identity (DRM device, connector, EDID make/model/serial when present). It is distinct from the runtime `OutputId` (never reused) and is not a physical-monitor identity; collisions follow INV-CAMERA-3.<br><br>**Persisted camera state** is a passive read-only lookup table given to `Core::new` and consulted only inside `output_connected`. No public restore API exists.
- **Owner:** core output manager + `navigation`
- **Class:** E+D
- **Violation:** D7-authored (D-F0): protocol-reachable camera creation or re-binding; a coordinate backdoor through output events
- **Origin:** C12.E1R @ UNSEALED (E1R review gate; base 09075a2)
- **Layers:** C12.R2 → C12.E1R
- **Trigger:** E-F2-01, E-F2-03, E-F3-03
- **Evidence:** ER2-1, EE-4
- **Tests (historical column):** T0 probes 2f/2h/2i (OB-19); T1 hotplug and reconfiguration (OB-49)
- **Obligations:** OB-19, OB-49
- **Formulas:** F-RECONF-1
- **ADR:** ADR-04, ADR-08

<!-- /living -->
