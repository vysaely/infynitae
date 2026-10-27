# Blockers and Smithay quirks

The register of design findings (`PD-F*`) and their status, and the catalogue of Smithay 0.7 quirks (`W-SM-*`) the adapter works around. The findings as first written, and the Phase B blocker analysis, are in [../archive/FOUNDATION_BLOCKERS_HISTORY.md](../archive/FOUNDATION_BLOCKERS_HISTORY.md).

### Finding register (canonical PD-F*)

Status vocabulary:
- `RESOLVED`: the architecture was corrected under change control;
- `OPEN`: dispositioned to an obligation or decision that is not yet satisfied;
- `APPLIED`: a D-F1 applied in the D7 refinement layer;
- `NOTED`: D-F0 clarification.

Original finding text: the sections above (PD-F2) and audit/README (PD-F1-01…09, PD-F0-01…09, historical).

<!-- living:PDF -->
### PD-F2-01
- **Class:** D-F2
- **Level (found → re-evaluated):** A → A
- **Title:** Camera evolution without an authority port
- **Status:** RESOLVED
- **Resolution / destination:** 1R: INV-CAMERA-1, INV-CAMERA-5 (ER-4)

### PD-F2-02
- **Class:** D-F2
- **Level (found → re-evaluated):** A → A
- **Title:** Smithay grabs decide focus behind the core models
- **Status:** RESOLVED
- **Resolution / destination:** 1R: INV-INPUT-8, INV-INPUT-10 (ER-1)

### PD-F2-03
- **Class:** D-F2
- **Level (found → re-evaluated):** B → A
- **Title:** `Locked` lacks output; Canvas lock point detaches
- **Status:** RESOLVED
- **Resolution / destination:** 1R: INV-COORD-5, INV-INPUT-5 (ER-2)

### PD-F2-04
- **Class:** D-F2
- **Level (found → re-evaluated):** B → A
- **Title:** Pointer reconciliation only at pan termination
- **Status:** RESOLVED
- **Resolution / destination:** 1R: INV-INPUT-5 (ER-2)

### PD-F2-05
- **Class:** D-F2
- **Level (found → re-evaluated):** B → A
- **Title:** Confinement enforcement across independent cameras
- **Status:** RESOLVED
- **Resolution / destination:** 1R: INV-INPUT-5 (ER-5)

### PD-F2-06
- **Class:** D-F2
- **Level (found → re-evaluated):** B → B
- **Title:** Zoomed panning reachable; pan formula lacks zoom; OB-24 premise false
- **Status:** OPEN
- **Resolution / destination:** algebra resolved in 1R (INV-COORD-3, F-PAN-1); ±1 px shape residual open as decision OB-24

### PD-F2-07
- **Class:** D-F2
- **Level (found → re-evaluated):** B → B
- **Title:** Discrete camera interactions outside SC-D
- **Status:** RESOLVED
- **Resolution / destination:** 1R: INV-SCALE-3/4 (ER-5)

### PD-F2-08
- **Class:** D-F2
- **Level (found → re-evaluated):** C → C
- **Title:** Device bound vs ladder/P4; H_max basis; off-ladder hint
- **Status:** RESOLVED
- **Resolution / destination:** 1R: INV-SCALE-2/5 (ED-2)

### PD-F2-09
- **Class:** D-F2
- **Level (found → re-evaluated):** C → C
- **Title:** Initial scale hint undefined
- **Status:** RESOLVED
- **Resolution / destination:** 1R: INV-SCALE-5 (ED-2)

### PD-F2-10
- **Class:** D-F2
- **Level (found → re-evaluated):** C → A
- **Title:** Protocol-fullscreen state machine incomplete; unanswered requests
- **Status:** RESOLVED
- **Resolution / destination:** 1R: INV-PRES-1 (ER-3)

### PD-F2-11
- **Class:** D-F2
- **Level (found → re-evaluated):** C → C
- **Title:** Shared camera vs output-keyed fit; orphaned pan
- **Status:** RESOLVED
- **Resolution / destination:** 1R: by PD-F2-01 + INV-OUTPUT-1

### PD-F2-12
- **Class:** D-F2
- **Level (found → re-evaluated):** C → C
- **Title:** Popup constraint target rounding point
- **Status:** RESOLVED
- **Resolution / destination:** 1R: INV-COORD-2 R8

### PD-F2-13
- **Class:** D-F2
- **Level (found → re-evaluated):** C → C
- **Title:** Window drag across outputs
- **Status:** OPEN
- **Resolution / destination:** decision OB-34 (F-DRAG-1 open); gates INC-14

### PD-F2-14
- **Class:** D-F2
- **Level (found → re-evaluated):** C → C
- **Title:** `AllObservingOutputsOff` over camera membership
- **Status:** RESOLVED
- **Resolution / destination:** 1R: INV-FRAME-6

### PD-F2-15
- **Class:** D-F2
- **Level (found → re-evaluated):** C → C
- **Title:** Attention / `transient_for` on unmap/destroy
- **Status:** OPEN
- **Resolution / destination:** implementation obligation OB-35

### PD-F2-16
- **Class:** D-F2
- **Level (found → re-evaluated):** C → C
- **Title:** Satellite restart display identity / socket ownership
- **Status:** OPEN
- **Resolution / destination:** `-listenfd` verified (ER-6); implementation obligation OB-33

### PD-F2-17
- **Class:** D-F2
- **Level (found → re-evaluated):** B → A
- **Title:** xdg_toplevel state policy (maximize answer, `activated`)
- **Status:** RESOLVED
- **Resolution / destination:** 1R: INV-PROTO-1 (ER-3)

### PD-F2-18
- **Class:** D-F2
- **Level (found → re-evaluated):** A → A
- **Title:** Camera creation/binding/restore without authorization
- **Status:** RESOLVED
- **Resolution / destination:** 2R: INV-OUTPUT-2, INV-CAMERA-1/3, INV-OUTPUT-1 (ER2-1)

### PD-F2-19
- **Class:** D-F2
- **Level (found → re-evaluated):** C → C
- **Title:** Stage-move interactions outside ScaleInteractionEpoch
- **Status:** RESOLVED
- **Resolution / destination:** 2R: INV-SCALE-3/4 (ER2-2)

### PD-F1-01
- **Class:** D-F1
- **Status:** APPLIED
- **Applied in:** FOUNDATION_ARCHITECTURE §D7.R → F-LATTICE-1
- **Semantics:** unchanged (justification in §D7.R)

### PD-F1-02
- **Class:** D-F1
- **Status:** APPLIED
- **Applied in:** FOUNDATION_ARCHITECTURE §D7.R → F-VIEW-1
- **Semantics:** unchanged (justification in §D7.R)

### PD-F1-03
- **Class:** D-F1
- **Status:** APPLIED
- **Applied in:** FOUNDATION_ARCHITECTURE §D7.R → F-RENDER-1
- **Semantics:** unchanged (justification in §D7.R)

### PD-F1-04
- **Class:** D-F1
- **Status:** APPLIED
- **Applied in:** FOUNDATION_ARCHITECTURE §D7.R → F-VIEW-1
- **Semantics:** unchanged (justification in §D7.R)

### PD-F1-05
- **Class:** D-F1
- **Status:** APPLIED
- **Applied in:** FOUNDATION_ARCHITECTURE §D7.R → F-RELOCATE-1
- **Semantics:** unchanged (justification in §D7.R)

### PD-F1-06
- **Class:** D-F1
- **Status:** APPLIED
- **Applied in:** FOUNDATION_ARCHITECTURE §D7.R → OBJ-05
- **Semantics:** unchanged (justification in §D7.R)

### PD-F1-07
- **Class:** D-F1
- **Status:** APPLIED
- **Applied in:** FOUNDATION_ARCHITECTURE §D7.R → F-HIT-1, OBJ-15
- **Semantics:** unchanged (justification in §D7.R)

### PD-F1-08
- **Class:** D-F1
- **Status:** APPLIED
- **Applied in:** FOUNDATION_ARCHITECTURE §D7.R → F-SCALE-2
- **Semantics:** unchanged (justification in §D7.R)

### PD-F1-09
- **Class:** D-F1
- **Status:** APPLIED
- **Applied in:** FOUNDATION_ARCHITECTURE §D7.R → F-PRESENT-1
- **Semantics:** unchanged (justification in §D7.R)

### PD-F1-10
- **Class:** D-F1
- **Status:** APPLIED
- **Applied in:** FOUNDATION_ARCHITECTURE §D7.R → PROV-7…PROV-11, OB-37…OB-41
- **Semantics:** unchanged (justification in §D7.R)

### PD-F1-11
- **Class:** D-F1
- **Status:** APPLIED
- **Applied in:** FOUNDATION_TEST_PLAN OB-42 (INV-ALLOC-1 counting-allocator test), OB-43 (persistence all-or-nothing load), OB-44 (C14 performance/idle T4 baselines); FOUNDATION_RISKS TR-7, TF-6, TR-4
- **Semantics:** unchanged. Specified tests (C14.3, C8, C14.1/C14.2) that had no owning obligation now have one; found in D7 by the checker (`risk-backed`).

### PD-F0-01
- **Class:** D-F0
- **Status:** NOTED
- **Note:** crossing outputs during a client implicit grab yields a surface-local jump (expected)

### PD-F0-02
- **Class:** D-F0
- **Status:** NOTED
- **Note:** pixel-centre clamp bound ≤ ~2⁻¹⁶ (measured 0)

### PD-F0-03
- **Class:** D-F0
- **Status:** NOTED
- **Note:** B9.6 removal decrease vs INV-SCALE-3 during gestures: the registry prevails

### PD-F0-04
- **Class:** D-F0
- **Status:** NOTED
- **Note:** T5 omits δ (2⁻²⁰/k): immaterial

### PD-F0-05
- **Class:** D-F0
- **Status:** NOTED
- **Note:** kick rate bounded by physical input rate

### PD-F0-06
- **Class:** D-F0
- **Status:** NOTED
- **Note:** ≤ 1-frame ProjectionFrame staleness (I9 trade-off)

### PD-F0-07
- **Class:** D-F0
- **Status:** NOTED
- **Note:** camera retention per OutputKey is a Level D persistence policy

### PD-F0-08
- **Class:** D-F0
- **Status:** NOTED
- **Note:** hotplug of a ladder-rung output can renegotiate without a ρ change (bounded)

### PD-F0-09
- **Class:** D-F0
- **Status:** NOTED
- **Note:** pointer Canvas point defined on the snapped camera; no write-back of rendered values

### PD-F0-10
- **Class:** D-F0
- **Status:** NOTED
- **Note:** ADR-08 decision text predates the 2R output-lifecycle entry: D7 editorial note added

### PD-F0-11
- **Class:** D-F0
- **Status:** NOTED
- **Note:** INV-CAMERA-4 missing from ADR-06/ADR-07 affected-invariant lists: D7 editorial notes added

<!-- /living -->

### Smithay quirk catalogue (canonical W-SM-*)

<!-- living:WSM -->
### W-SM-1
- **Quirk:** `Space` owns placement/z-order/activation
- **Infynitae response:** not used (ADR-01)
- **Evidence:** EB-2
- **Obligations:** OB-26

### W-SM-2
- **Quirk:** `Window::output_enter` / `Space::refresh` mis-compute CSD/popup membership
- **Infynitae response:** own per-surface membership (F-MEMBER-1)
- **Evidence:** EB-2, EB-H1
- **Obligations:** OB-26

### W-SM-3
- **Quirk:** `CompositorState::new` advertises `wl_compositor` v5
- **Infynitae response:** always `new_v6`
- **Evidence:** EB-10
- **Obligations:** OB-19

### W-SM-4
- **Quirk:** seat `leave()` deactivates constraints unconditionally (4a: duplicate `unlocked` after an explicit deactivate; 4b: a never-activated oneshot lock is destroyed; 4c (E1R, E-F2-02): an already-inactive persistent lock or confinement gets a second `unlocked`/`unconfined` when focus leaves in a later batch or the surface is destroyed, and a never-activated persistent lock gets `unlocked`)
- **Infynitae response:** 4a: explicit deactivate only when the delivered focus does not change; reconciliation deferred under client grabs (INV-INPUT-5). 4b and 4c: no public-API workaround (EE-2 E11); dependency patch P1 (`if constraint.is_active()` in `leave()`) or an upstream fix, decided under OB-11
- **Evidence:** EB-16, ER-2, X-12 (TST-08), EE-2, EE-7
- **Obligations:** OB-11, OB-31, OB-47

### W-SM-5
- **Quirk:** `render_elements_from_surface_tree` takes an i32 root
- **Infynitae response:** `from_surface` with f64 location (F-RENDER-1)
- **Evidence:** EB-14
- **Obligations:** OB-26

### W-SM-6
- **Quirk:** `ensure_configured` is opt-in
- **Infynitae response:** call on every role commit
- **Evidence:** EB-6
- **Obligations:** OB-18

### W-SM-7
- **Quirk:** `grab_popup` asserts the root
- **Infynitae response:** check the precondition; refuse invalid grabs (INV-INPUT-10)
- **Evidence:** EB-6, ER-1
- **Obligations:** OB-18

### W-SM-8
- **Quirk:** `token_created` accepts every token
- **Infynitae response:** provenance override (INV-INPUT-9)
- **Evidence:** EB-19
- **Obligations:** OB-27

### W-SM-9
- **Quirk:** `Window::geometry` re-clamps to the current bbox
- **Infynitae response:** accepted; revisit on T3 evidence
- **Evidence:** EB-6
- **Obligations:** OB-17

### W-SM-10
- **Quirk:** `RendererSurfaceState::surface_view` private
- **Infynitae response:** use `view()`
- **Evidence:** EB-18
- **Obligations:** OB-26

### W-SM-11
- **Quirk:** input regions tested with round-to-nearest (½ px shift)
- **Infynitae response:** own hit-test (F-HIT-1)
- **Evidence:** EC-10
- **Obligations:** OB-26

### W-SM-12
- **Quirk:** below-subsurface wins `surface_under`
- **Infynitae response:** own hit-test in render order
- **Evidence:** EC-11
- **Obligations:** OB-26

### W-SM-13
- **Quirk:** default maximize/fullscreen handlers send configure early
- **Infynitae response:** overrides; answers per INV-PROTO-1 / INV-PRES-1
- **Evidence:** EC-13, ER-3
- **Obligations:** OB-32

### W-SM-14
- **Quirk:** a nested popup grab replacing the `PopupPointerGrab` tears down the keyboard grab
- **Infynitae response:** keep the installed pointer grab for nested grabs
- **Evidence:** ER-1
- **Obligations:** OB-30

### W-SM-15
- **Quirk:** pointer-first popup-grab teardown refocuses the root transiently
- **Infynitae response:** fixed preemption order (INV-INPUT-10)
- **Evidence:** ER-1
- **Obligations:** OB-30

### W-SM-16
- **Quirk:** `PointerHandle::unset_grab` restores focus with a `motion` at the stored location, delivered to the grabbed surface even while it holds an active lock (E1R)
- **Infynitae response:** the zero-output transition uses `set_grab(…, Focus::Clear)` with a compositor grab that delivers nothing (INV-OUTPUT-1); INV-INPUT-10 step 3 is under test (OB-51)
- **Evidence:** EE-3 (N3)
- **Obligations:** OB-50, OB-51

### W-SM-17
- **Quirk:** `ToplevelSurface::send_configure` emits `xdg_toplevel.configure_bounds(0, 0)` with a toplevel's first configure to every client that bound `xdg_wm_base` ≥ 4; there is no API to omit it
- **Infynitae response:** bounds are always set (INV-PROTO-1 as amended 2026-10-27), so the event carries the screen's usable area, never `(0, 0)`; Smithay sends it again only when it changed
- **Evidence:** vendored Smithay 0.7.0 source; client traces (KCalc, fs_client); T1 `configure_bounds_is_the_screens_room_and_follows_it`, `a_portrait_screen_gives_portrait_room`
- **Obligations:** —

### W-SM-18
- **Quirk:** a buffer committed on an xdg surface that has not acknowledged a configure is answered with `xdg_surface.error` **1** (`not_constructed`, "Surface has not been configured yet") by `ToplevelSurface::ensure_configured` / `PopupSurface::ensure_configured`; xdg-shell names **3** (`unconfigured_buffer`). Upstream defect of Smithay 0.7.0 (2025-06-24, the pinned version); fixed upstream by commit `641d2ad` ("wayland/shell/xdg: Track configured state through MultiCache last_acked", PR #1817, merged 2025-09-17), which removes `ensure_configured` and posts `UnconfiguredBuffer` from pre-commit hooks. No released Smithay contains the fix as of 2026-10-14
- **Infynitae response:** none: the client is disconnected either way and nothing else differs. No downstream patch. The adapter cannot post the right code itself (Smithay 0.7.0 exposes no `xdg_surface` handle). Tests accept both codes (`t1_popup_policy::assert_unconfigured_error`). At the next pin update: drop the adapter's two `ensure_configured` calls (upstream enforces the rule), require code 3 in the tests, retire this record
- **Evidence:** vendored `wayland/shell/xdg/mod.rs` (1652, 2045); upstream commit `641d2ad`; T1 `t1_popup_policy`, `t1_inc7_misbehaving`
- **Obligations:** — (pin update, D-B12-PIN)

### W-SM-19
- **Quirk:** when a client holding a popup grab disconnects, the keyboard grab ends at once but the pointer grab object stays installed until the next pointer event, when it finds its popup gone and ungrabs itself
- **Infynitae response:** none today. Observable consequence: between the disconnect and the next pointer event the adapter's "pointer grabbed" reading (`is_pointer_grabbed`) is true; no event reaches the dead client or any other client, and nothing decides on that reading outside a pointer event. **INC-14 entry condition:** before interactive move/resize (or anything else) decides on the grab state without a pointer event, the adapter must end the pointer grab when the grabbing client's last popup is destroyed (`PointerHandle::unset_grab` in the popup-destruction path) and a T1 test must show no grab right after the disconnect
- **Evidence:** T1 `t1_inc7_misbehaving` (no grab after one motion and one click); P10b
- **Obligations:** INC-14 (above)

### W-SM-20
- **Quirk:** `PopupManager::popups_for_surface` (and so `Window::render_elements`) lists, for each root popup in creation order, its children oldest first and then itself: an older sibling, or an older popup's subtree, comes before (in front of) a newer popup. xdg-shell: "A newly created xdg_popup will be stacked on top of all previously created xdg_popup surfaces associated with the same xdg_toplevel"
- **Infynitae response:** (X-15 as amended by FOUNDATION_ARCHITECTURE §C12.F8, INV-RENDER-2) the adapter numbers each xdg popup at creation and orders a toplevel's popup trees newest first in the snapshot, so rendering and hit-testing follow the protocol; within one popup tree Smithay's order is kept. The render order equals Smithay's except where this quirk applies
- **Evidence:** vendored `desktop/wayland/popup/manager.rs` (288, 347); T1 `a_newer_popup_is_stacked_above_every_older_one`, `ob1_nested_popup_chain_matches_what_is_presented`
- **Obligations:** X-15 (C12.F8); OB-1


### W-SM-21
- **Quirk:** Smithay 0.7.0's `PointerConstraint` keeps the constraint's lifetime private (`lifetime` field, `wayland/pointer_constraints.rs` 179; no accessor), so a compositor cannot read whether a lock or confinement is `oneshot` or `persistent`. Smithay itself ends a oneshot constraint on deactivation and destroys it, without telling the compositor through `PointerConstraintsHandler`
- **Infynitae response:** (OB-54; owner decision 2026-10-19: keep the adapter workaround, no Smithay patch now) the adapter reports every constraint to Core as `ConstraintLifetime::Persistent` and, at the end of each effect batch, drops from Core a constraint whose Smithay object has been destroyed (`WaylandState::drop_constraints_smithay_destroyed`), so Core never re-activates a oneshot constraint Smithay ended. A Smithay patch exposing the lifetime would need the patch policy (INV-ARCH-3)
- **Evidence:** vendored `wayland/pointer_constraints.rs` (179); T1 `an_ended_oneshot_lock_is_never_activated_again` (`t1_constraint_lifetime`) with its seeded control
- **Obligations:** OB-54
<!-- /living -->
