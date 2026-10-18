# INC-10 — exit-review M1 resolution and evidence record (2026-10-18)

Status: **implementation follow-up on local `main`, not pushed. INC-10 is NOT ratified; OB-24 stays
OPEN.** The governance clarifications are a separate decision package, not adopted:
`evidence/phase-f/C12-F17/C12F17_AMENDMENT_PACKAGE_2026-10-18.md`.

## 1. Authority and base

Owner brief "INC-10 M1 Resolution and C12.F17 Change Control" (2026-10-18), following the
independent read-only exit review of `c4a7263` (recommendation: READY AFTER SPECIFIED GOVERNANCE
CLARIFICATIONS; finding M1, medium). Verified before any change: branch `main` at
`c4a72630bb2b39b835490a305538da61549eaee3`, tree `7a4c04a00cf59bd5197e98ffdfec76d00adf9a51`,
working tree clean, `origin/main` `23752f17651887426e89e15dd80b40f73111aa8e` (local `main` 84
commits ahead), 83 worktrees untouched — as the review reported.

## 2. M1: pointer reconciliation after a fit or restore under a confinement

**Root cause.** `Core::camera_written` (`infynitae-core/src/entry.rs`), the common tail of
`fit_viewport` and `viewport_restore`, ran the constraint reconciliation and then sent the
true-position event only `if self.constraint.is_none()`. Every kept constraint suppressed it. For a
lock that is required (zwp_pointer_constraints_v1: no `wl_pointer.motion` while locked;
INV-INPUT-5). For a confinement it is not: the camera action moves the confined surface under a
pointer that stays where it is on the output, so the client's last surface-local position became
stale until the next physical motion, while F-FIT-1 (living) requires "exactly one when the pointer
is on O and no interaction owns it". The guard was copied from the pan-release path
(`button_release`), where M10 permits "≤ 1" (OB-56).

**Fix (one condition).** The event is suppressed only when the reconciliation leaves a **lock**
active. A kept confinement, an activated confinement and no constraint each get exactly one
true-position event (`true_position_into`: one `PointerMotion` + `PointerFrame`); a confinement
whose region no longer holds the pointer is ended by the reconciliation first (one `unconfined`,
no warp, INV-INPUT-5) and then gets the event. Nothing else changed: the event still requires the
pointer on O and no compositor interaction, the reconciliation order is unchanged, and the
adapter, Smithay and the pointer-constraint state machine are untouched.

**Tests** (`infynitae-backend-headless/tests/t1_fit_constraints.rs`, production path with a real
client, the virtual keyboard, hand-derived oracle from the x8 setup):

| Test | Case | Asserted |
|---|---|---|
| `fit_and_restore_reconcile_the_pointer_under_each_constraint` | no constraint; kept confinement; kept lock — fit (zoom 1.8) then restore (zoom 1) | none/confined: exactly 1 motion at `((p − 960)/1.8 + 420, (p − 540)/1.8 + 320)` within 1/256 px, then (630, 520) after the restore; physical pointer unmoved; confinement active in Core and Smithay, 0 `unconfined`. Lock: 0 absolute motions, 0 `unlocked`, lock active in Core and Smithay, physical pointer = F-LOCK-1 point `(960 + 210·z, 540 + 200·z)` within 2⁻¹⁶ px. Then the next physical motion is routed (1 motion; 0 under the lock) |
| `a_fit_that_ends_the_confinement_ends_it_once_then_reconciles_the_pointer` | region (600, 500, 60, 40) no longer holding the pointer after the fit | exactly 1 `unconfined`, before exactly 1 motion at the new position; Core and Smithay unconfined; no warp |
| `no_fit_reconciliation_while_a_pan_owns_the_pointer` | confinement, one recorded fit, a pan in progress; Logo+F and Logo+Shift+F | zoom and history unchanged; no motion or relative event; confinement kept |

Regression evidence: with `entry.rs` reverted to `c4a7263` the first test fails at the confined
case's one-event assertion; the other two pass on both (they cover paths the fix does not change).
Seeded controls, each DETECTED: "a kept confinement suppresses the true-position event" (the guard
seeded back to `is_some()`) and "absolute motion reaches a kept lock" (the lock exception removed).

**Not covered here (pre-existing, recorded as obligations):** a lock that a view change ends —
on that path the adapter reports every constraint to Core as persistent (OB-54); the
reconfiguration path's motion under a kept lock (OB-55); pan release under a kept confinement
(OB-56).

## 3. Evidence added to the maintained suite

| Test (`t1_canonical_rect.rs` unless named) | Covers | Renderer |
|---|---|---|
| `pixels_follow_the_canonical_rect_in_every_sweep_configuration` | the review's independent sweep (L4): every configuration of the render/hit sweep — x and y axes, scales 1 and 1.5, per-frame reference and kept references at ≈ 2²⁰ and ≈ 2³¹ on both sides of R, negative and 2³⁶-scale origins — on every frame where the wrapper remaps a surface; the painter's oracle on Core's entries (not on presented geometry), all 1920 × 1080 pixels. 153 remapped frames, 0 wrong pixels; ≥ 3 frames required per configuration | Pixman, nearest |
| `a_cropped_surface_keeps_its_sampling_with_a_viewport_source_and_buffer_scale` | the review's E1: viewport source offset, buffer scale 2, and both, × 8 buffer transforms (24 cases); cropped draw = uncropped draw inside the box, nothing outside; cuts on whole buffer pixels | Pixman |
| `the_crop_box_is_half_open_with_the_width_capped_at_i32_max` | the review's E3: edges clamped to `[−2³⁰, 2³⁰]` (pixels in `[−2³⁰, 2³⁰)`); a rect over the whole box presented as `[−2³⁰, 2³⁰ − 1)`; other rects keep their edges; rects outside the box empty | geometry only |
| `t1_fit_bindings.rs` (4 tests) | exact modifiers; no target; inert during a pan; fit during a presentation as ratified by §C5 (§5 below) | — |

Seeded controls, each DETECTED: the wrapper's geometry seeded back to the wrapped element's
(pixel sweep), `src` not cropped (E1), the width cap removed (E3), and the three binding controls
of §5. Renderer limits stay explicit: Pixman only; fractional cuts, GLES and every GPU path are
unverified (OB-57).

## 4. Non-blocking review findings

- **L1 — fixed.** `convert::render_output` borrowed `state.core.output(o)` and cloned the
  `OutputModel` (its `OutputKey` owns strings) on every frame, although the model is read only to
  rebuild the tracker. It now borrows it (`state.core` and `state.outputs` are disjoint fields).
  Measured with a counting allocator around one static `render_output` call (x8 scene,
  `DummyRenderer`, 20 calls after 3 warm-up): **9 → 5 heap allocations per frame**, every call
  equal. The remaining 5 (the render-list entry copy, the element vectors, Smithay's tracker) are
  outside INV-ALLOC-1's Core scope and recorded in OB-58; no test bounds adapter allocations.
- **L2 — fixed.** `tools/verify-test-efficacy` `rebuilt_clean` treated a `--features` build (the
  `t1_popup_focus` control with `probe_test_keyboard_grab`) as having rebuilt the unfeatured
  dependency units from clean source, clearing their stale mark too early; cargo builds featured
  units as separate artifacts. A featured build now clears no stale mark (more rebuilds, never a
  stale reuse). Verified by the full efficacy run on the final tree (commit message).
- **L4 — done** (§3).
- **L6 — open (OB-58).** Frame-time tails without CPU isolation or vblank pacing; fit allocation
  evidence for repeated identical fits only.

## 5. Bindings and target eligibility

- **Exact modifiers (implemented).** `input::keys::resolve_key_binding`: Logo+F and Logo+Shift+F
  resolve only with Ctrl and Alt released. Before, Logo+Ctrl+F and Logo+Alt+F also fitted.
  Logo+Ctrl+F, Logo+Alt+F, their Shift forms and Logo+Ctrl+Alt+F are now no binding: F reaches the
  focused client (M12), camera and history unchanged (T0 `navigation_bindings_reject_extra_modifiers`
  over all 8 combinations; T1 `extra_ctrl_or_alt_modifiers_trigger_neither_binding`, with a
  history entry present).
- **No target (unchanged, now tested at T1).** Logo+F with no element under the pointer: no camera
  or history change and nothing on the wire, at 0, 1 and 256 entries; F is consumed (a resolved
  binding). `logo_f_with_no_element_under_the_pointer_is_a_no_op`.
- **During a pan (unchanged).** Neither binding resolves; F follows M12's client path.
- **During a protocol-fullscreen presentation (unchanged, conflict recorded).** FOUNDATION_ARCHITECTURE
  §C5 ("fullscreen + viewport-fit on the same output", Phase C checkpoint 2, accepted) states:
  "the presentation view takes precedence for rendering. A fit applied meanwhile changes camera
  A's state, which is shown when the presentation ends." The brief asks that a presentation not let
  an operation silently modify a camera that is not presented. That conflicts with ratified text,
  so the behaviour is **not changed**: `a_fit_during_a_presentation_changes_the_hidden_camera_as_ratified`
  pins the ratified behaviour (camera A and its history change; the presented frame does not; the
  fitted view appears when the presentation ends), and a seeded control (a fit refused during a
  presentation) is DETECTED by it. The decision is requested in the C12.F17 package (Q1).

## 6. Pre-existing defects found while testing M1 (scratch runs; not fixed here)

One temporary test file in the workspace, run once and deleted (never committed); output verbatim:

```
FA pan release, confined: wire [] core Some(Confine) smithay true
FA pan release, unconstrained: wire ["motion 590,495", "frame"]
FB oneshot lock: core lifetime Some(Persistent)
FB oneshot lock, point off the output after a fit: wire ["unlocked"] core None smithay false
FB next motion: wire ["motion 191.109375,145", "relative 3,0 unaccel 2.69921875,0", "frame"]
FC reconfigure, locked: wire ["motion 822,628", "frame"] core Some(Lock)
```

Also observed while writing the tests (persistent lock with its point pushed off the output by a
fit): `["unlocked", "locked"]` in one batch. Recorded as OB-54 (FB), OB-55 (FC), OB-56 (FA and the
re-activation). FC is a protocol violation on the reconfiguration path (no `wl_pointer.motion`
while locked); it predates INC-10.

## 7. Seeded controls added (each DETECTED)

380 → 388 controls (plus the 2 judge controls). M1: confinement suppresses the event; motion reaches a kept lock. L4: wrapped
geometry (pixel sweep). E1: `src` not cropped. E3: width cap removed. Bindings: Ctrl/Alt not
matched exactly; a fit with no target fits another element; a fit refused during a presentation
(divergence from §C5). None removed or weakened.

## 8. Not established

GLES or any GPU path, dmabuf, direct scanout, hardware planes, display pacing (OB-57, OB-58);
fractional-pixel crop cuts in pixels; the lock-ends-by-view-change path (OB-54).
