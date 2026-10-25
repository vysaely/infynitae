# Phase D checkpoint 1R — invariant diff

Baseline registry: FOUNDATION_ARCHITECTURE §C12, sealed in `615b0ed` (54 entries). Phase D falsification baseline: `e8ec03a`. The amended wording becomes normative through **§C12.R1** in FOUNDATION_ARCHITECTURE.md. The sealed C12 table is left **unaltered** so that it stays historically identifiable; C12.R1 supersedes the listed rows only.

**Result:** 14 invariants amended, 2 new (`INV-INPUT-10`, `INV-PROTO-1`), 40 unchanged. The registry now has **56** entries.

Evidence IDs (Phase D 1R; see [CORRECTIONS_1R.md](CORRECTIONS_1R.md) §Evidence):
- ER-1 `xpopup_focus`;
- ER-2 `xlock_grab`;
- ER-3 `xmaximize` (maximize and fullscreen);
- ER-4 authority spike with the 1R probes;
- ER-5 `d5_model_1r` (R1–R5);
- ER-6 satellite `--test-listenfd-support`;
- ER-7 regression re-run.

Each entry below has the same layout:
- **Previous (sealed 615b0ed):** the C12 wording, verbatim (abridged only where marked "…").
- **Trigger:** the `PD-F2-*` finding(s).
- **Replacement:** the new wording.
- **Why necessary**, **Evidence**, **ADR**, **Tests/obligations.**

---

### INV-CAMERA-1 (amended)
- **Previous:** "Camera state is mutated only inside `core::navigation`, reachable only with `InputPort` (physical input) or `ShellPort` (shell); ports are unforgeable, `!Send`, bound to their Core and never stored in protocol state."
- **Trigger:** PD-F2-01.
- **Replacement:**
  - Camera state is mutated only inside `core::navigation`, and every mutation consumes a fresh `Authorized` proof.
  - A proof can be minted only inside the two port-carrying entry points (`physical_input(&mut InputPort)`, `shell_command(&mut ShellPort)`). It is not Clone, `!Send`, and consumed by value.
  - Core retains no proof, mode, target or tracking state that would permit a later camera mutation.
  - No protocol event and no client-originated state change (commit, resize, geometry, buffer, scale response, request) causes a camera mutation.
  - Ports are unforgeable, `!Send`, bound to their Core and never stored in protocol state.
  - Time-driven camera evolution (animation) is not part of Foundation 01. If it is introduced later, each step must be an authorized action with its own proof.
- **Why necessary:**
  - The sealed rules C5.1 (re-fit on resize) and C7.3/AUD-6 (animated navigation) required camera mutation outside the port-gated entry points.
  - The sealed mechanism checked only "who may call navigation", not "whether authority may be kept and re-used later".
- **Evidence:** ER-4.
  - Probe 2d: protocol re-fit fails with E0624 (`mint` is private).
  - Probe 2e: a sticky fit that keeps a proof fails with E0277.
  - Test `client_resize_after_fit_never_moves_camera`: 200 adversarial commits, 0 camera changes.
  - ER-5 R3: under the sealed re-fit an adversarial client caused 60 camera mutations; under one-shot fit, 1.
- **ADR:** ADR-08, ADR-04.
- **Tests/obligations:** OB-19 probe list extended (2d, 2e, "no port-free mutator").

### INV-CAMERA-5 (amended)
- **Previous:** "Viewport-fit is a camera operation (centre + zoom on one output; optional isolation) saved/restored in `navigation`; it never changes an element's size or anchor."
- **Trigger:** PD-F2-01, PD-F2-11.
- **Replacement:**
  - Viewport-fit is an **instantaneous, one-shot** navigation action invoked with authority (`FitViewport(output, ElementId)` via input or shell).
  - It snapshots the element's geometry at that instant, computes the centre and the zoom for the invoking output's viewport, performs one camera mutation, and terminates. It is not a mode and stores no target.
  - Later client commits, resizes, geometry, buffer or scale changes never re-fit.
  - A "restore" is a separate authorized navigation action that applies a camera value recorded at fit time.
  - When the camera is shared by several outputs, all of them show the fitted camera (sharing is by definition).
  - The per-output isolation filter is removed from Foundation 01 (deferred, Level D).
  - Fit never changes an element's size or anchor.
- **Why necessary:**
  - A sticky fit was the client-to-camera authority path (PD-F2-01).
  - Per-output fit state on shared cameras was undefined (PD-F2-11).
- **Evidence:** ER-4, ER-5 R3.
- **ADR:** ADR-04, ADR-09.
- **Tests/obligations:** OB-28 rescoped (no re-fit loop remains).

### INV-COORD-2 (amended)
- **Previous:** "Rounding happens only at R1–R7 (R7 = view-origin quantisation); F1–F7 are forbidden."
- **Trigger:** PD-F2-12.
- **Replacement:** Rounding happens only at R1–R8 (R7 = view-origin quantisation; **R8 = inward rounding of a popup constraint target to integral parent-geometry coordinates**: ceiling at the minimum edges, floor at the maximum edges). F1–F7 are forbidden.
- **Why necessary:** the D-B6-BOUNDARY target is fractional at fractional camera positions (D5: x[−179.333328, 2380.666672)), while the positioner needs `Rectangle<i32>`.
- **Evidence:** ED-2 F7.
- **ADR:** ADR-02.
- **Tests/obligations:** OB-6 (T0 property: a constrained popup never exceeds the visible viewport).

### INV-COORD-3 (amended)
- **Previous:** "Gesture deltas accumulate in f64 from the gesture origin and are quantised once per update, never per event."
- **Trigger:** PD-F2-06.
- **Replacement:**
  - **Direct-manipulation pan.** For physical pointer displacement `dp_phys` on the interaction output O (scale `s_O`), with camera zoom `z_C`, the Canvas displacement is `dP_canvas = dp_phys / (s_O · z_C)`.
  - The camera centre moves opposite: `C = C_start − Q16(Σ dp_phys / (s_O · z_C))`.
  - The sum is accumulated in f64 from the gesture origin and quantised once per update (R2), never per event.
  - If `s_O` or `z_C` changes within a gesture, the per-event terms `dp_phys,i / (s_O,i · z_C,i)` are summed in f64.
  - The same once-per-update rule applies to every gesture total.
- **Why necessary:**
  - The sealed `camera = start − Q(Σ)` has no zoom term.
  - Zoomed panning is reachable in Foundation 01: fit exit keeps the zoom, and zoomed navigation is now adopted. Under the sealed formula, content moves z× the hand.
- **Evidence:** ER-5 R1.
  - Corrected: |content − hand| ≤ 0.9 px at zoom {0.5, 0.75, 1, 1080/651, 2.5} × scale {1.0, 1.5}.
  - Sealed: up to 1110 px.
  - D5 epoch 4: W leaves A after 92 events (corrected), against 56 (sealed).
- **ADR:** ADR-02, ADR-04.
- **Tests/obligations:** OB-6, OB-21 (T4 libinput), OB-24 (lattice jitter at zoom ≠ 1, rescoped).

### INV-COORD-5 (amended)
- **Previous:** "Smithay's pointer 'global space' is Canvas. The authoritative pointer is `Free(OutputId, OutputLogical)` or `Locked(SurfaceKey, Canvas lock point)`; Smithay's location is derived per dispatch."
- **Trigger:** PD-F2-03.
- **Replacement:**
  - Smithay's pointer "global space" is Canvas.
  - The authoritative seat pointer state has two separate parts:
    - **(a) Physical pointer state:** `(interaction output: OutputId, p: OutputLogical)`. It is always defined while an output exists, and it is the output through which the user interacts.
    - **(b) Constraint state:** `Option<{surface: SurfaceKey, kind: Lock | Confine, lock point: SurfaceLocal (lock only)}>`.
  - A constraint's identity and lock state never include an output.
  - The Canvas lock point is **derived**: `surface_origin(surface) + lock point`.
  - Smithay's location is derived per dispatch.
- **Why necessary:**
  - `Locked` had no output, so "the pointer's output" was undefined.
  - The Canvas lock point drifted from its surface by 20 px in D5 epoch 3, and detaches entirely under element moves.
- **Evidence:** ED-2 epoch 3; ER-2.
- **ADR:** ADR-07.
- **Tests/obligations:** OB-3, OB-20.

### INV-INPUT-5 (amended)
- **Previous:** "Pointer constraints: activation is compositor policy (focused + inside region; never during pan acquisition); lock reconciliation at pan end keeps the Canvas lock point if it is observable on the pointer's output, else deactivates; on output removal the lock is deactivated and the pointer moves to the Arrangement-nearest output; W-SM-4a ordering (explicit deactivate only without a following focus change)."
- **Trigger:** PD-F2-02 (pointer part), PD-F2-03, PD-F2-04, PD-F2-05.
- **Replacement:**
  - **Activation.** A constraint is activated by compositor policy only when its element holds ActiveElementFocus, its surface holds pointer focus, the pointer is inside the region, and no compositor pointer interaction is being acquired.
  - **Reconciliation triggers.** One reconciliation step runs at the end of every core event batch in which any of these changed:
    - the interaction output's view (camera, protocol-fullscreen presentation, output mode/scale/transform);
    - the constrained surface's geometry or origin;
    - the output set;
    - ActiveElementFocus;
    - the end of a compositor interaction;
    - the end of a client-owned grab sequence.
  - **Reconciliation is deferred** while a compositor pointer interaction is active, or while a client-owned grab sequence (implicit click grab, popup grab, DnD) is active. Meanwhile:
    - the constraint state is unchanged in core **and** Smithay;
    - no absolute motion is delivered to a locked surface;
    - relative motion continues only from physical deltas.
  - **Rules at reconciliation:**
    - Keep a lock whose derived Canvas lock point is observable through the interaction output and whose element still holds ActiveElementFocus; otherwise end it.
    - For a confinement, clamp the physical pointer half-open in `OutputLogical(interaction output)` to the region's projection on that output. Arrangement crossing is disabled while confined. A pointer outside the region after a view change ends the confinement. There is no warp in either case.
    - Ending a constraint emits its `unlocked`/`unconfined` **exactly once**. The explicit `deactivate` is used **only** when the **delivered** pointer focus does not change in the same batch; otherwise Smithay's `leave()` performs it (W-SM-4a).
    - At most one true-position motion (or leave/enter) per batch. No replayed or synthetic absolute or relative motion.
  - **Output removal:** the physical pointer is relocated to the Arrangement-nearest point of a surviving output immediately (half-open clamp). The constraint is reconciled by the rules above, deferred if a client grab is active.
  - **Destruction** of the constrained surface ends the constraint through the protocol path.
- **Why necessary:**
  - Reconciliation was defined only at pan end (PD-F2-04).
  - The sealed W-SM-4a decision used core's hit-test, so under a client grab core and Smithay diverged. The client received an absolute motion while locked (ER-2 SEALED).
  - An explicit deactivate under a client grab duplicates `unlocked` for persistent locks (ER-2 EXPLICIT).
  - Confinement across independent cameras was undefined (PD-F2-05; ER-5 R5 negative control: 7,382 out-of-region positions and 87 surface-local jumps).
- **Evidence:**
  - ER-2: DEFER passes for persistent and oneshot locks: core equals Smithay at every step, exactly one `unlocked`, no motion while locked. The negative controls SEALED and EXPLICIT/persistent fail as predicted.
  - ER-5 R5: the rule gives 0 violations.
- **ADR:** ADR-07, ADR-14.
- **Tests/obligations:** OB-3, OB-31 (new: ER-2 port + real output removal), OB-14.

### INV-INPUT-8 (amended)
- **Previous:** "Keyboard focus changes only by explicit action (click-to-focus, focus bindings, shell focus command, activation grant) or target loss; camera, visibility and pointer motion never change it."
- **Trigger:** PD-F2-02.
- **Replacement:**
  - **ActiveElementFocus**, which Stage element (or none) owns desktop keyboard focus, is core state with a single writer, the FocusModel.
  - It changes only by explicit action (click-to-focus, focus bindings, shell focus command, activation grant) or target loss.
  - Camera, visibility, pointer motion and protocol routing never change it.
  - The effective keyboard routing delivered by the adapter is always a surface of the ActiveElementFocus element's tree (INV-INPUT-10), or none.
- **Why necessary:**
  - Smithay's popup keyboard grab silently ignored a FocusModel focus change. ER-1 NEG-1: core W2, Smithay and the client still on P.
  - It also wrote focus itself on grab end.
  - Both are now subordinate routing inside the focused element.
- **Evidence:** ER-1.
- **ADR:** ADR-14, ADR-07.
- **Tests/obligations:** OB-30 (new).

### INV-INPUT-10 (new)
- **Trigger:** PD-F2-02.
- **Wording:**
  - **ProtocolSurfaceRoute** (Smithay popup grabs: `PopupKeyboardGrab`/`PopupPointerGrab`, and Smithay's root restore on grab end) is adapter protocol state, **subordinate** to ActiveElementFocus. It routes events only within the surface tree of the element that holds ActiveElementFocus, and it never changes ActiveElementFocus.
  - When core changes ActiveElementFocus to another element or to none while a grab is active, the adapter performs, in one batch and in this order:
    1. `PopupGrab::ungrab(All)`: `popup_done` topmost-first, with a kick first if the client is unobserved;
    2. `KeyboardHandle::unset_grab`;
    3. `PointerHandle::unset_grab`;
    4. `KeyboardHandle::set_focus(target)`.
  - A nested grab keeps the installed popup pointer grab and replaces only the keyboard grab (W-SM-14).
  - A grab request whose root is not the focused element, or whose W-SM-7 precondition fails, is refused.
- **Why necessary:** a single element-level authority with an explicit, ordered preemption. The order matters:
  - **Pointer-first** teardown makes Smithay refocus the root transiently: leave P, enter W1, leave W1, enter W2 (ER-1 NEG-2).
  - **Replacing the pointer grab** on a nested grab tears the keyboard grab down: keys reach W1 instead of P2 (ER-1 OBS-1).
- **Evidence:** ER-1.
  - PF-1 … PF-7 pass: focus to another element or none; parent destroyed; popup destroyed in flight and destroyed first; nested; key held through the transition.
  - Every scenario shows no panic, no duplicate enter/leave, no stuck key, and core = Smithay = client.
- **ADR:** ADR-14, ADR-13.
- **Tests/obligations:** OB-30 (new), OB-18.

### INV-SCALE-2 (amended)
- **Previous:** "The client hint h belongs to a finite ladder containing every output scale and is ≤ `H_max(E) = min(H_cfg, D_max / max logical side)`, with `D_max` queried from the renderer; if E's size makes h exceed the device bound, h is lowered immediately."
- **Trigger:** PD-F2-08.
- **Replacement:**
  - Every **emitted** hint h is a rung of the ladder in force at emission. That ladder contains every connected output scale that is ≤ H_max(E), plus the integers and the geometric fill.
  - `H_max(E) = min(H_cfg, D_max / s_max(E))`, where `s_max(E)` is the largest logical side over **all surfaces** of E's tree, each being its own buffer. `D_max` is queried from the renderer, and re-queried on renderer recreation.
  - If E's size makes h exceed the device bound, h is lowered immediately (P7).
  - A held (unobserved) or epoch-deferred hint may be off the current ladder until the next evaluation permitted to change it.
- **Why necessary:**
  - "Every output scale" contradicts the device bound (6000-px window on a 3.0 output).
  - Window geometry under-estimates the root buffer (5490-px root over a 5450 geometry).
  - Hotplug changes the ladder.
- **Evidence:** ED-2 F3, F4.
- **ADR:** ADR-05.
- **Tests/obligations:** OB-13.

### INV-SCALE-3 (amended)
- **Previous:** "Increases happen only when the target rung exceeds h (rung crossings); decreases only when no camera gesture **and** no discrete-zoom interaction is active."
- **Trigger:** PD-F2-07.
- **Replacement:**
  - **ScaleInteractionEpoch.** Camera changes belonging to one logical user or shell interaction form one epoch:
    - a continuous pointer pan;
    - a continuous touchpad or pinch navigation;
    - a run of discrete navigation or zoom steps;
    - a bounded shell navigation stream.
  - **During an epoch:**
    - decreases are suppressed;
    - increases occur only at ladder-rung crossings and only in the epoch's direction (a P7 decrease fixes the direction to down);
    - client responses cannot reverse the direction;
    - individual camera updates do not each become hint changes.
  - **The epoch ends** at an explicit end (gesture end, shell end-of-stream), at a settlement event (output add/remove/configuration change), or, for inputs without an explicit end, when no camera step of the interaction arrived for the **settlement timeout**. Pointer motion does not end it.
  - **At settlement** the final requirement is evaluated once and h is set to its target.
  - The **existence** of the settlement timeout is architectural; its **value** is provisional (C14.5; candidate 300 ms, TUNE OB-23). The timer is armed only while an epoch is open (INV-IDLE-1).
  - An atomic camera action (jump, one-shot fit, restore) is a one-step epoch that settles immediately.
- **Why necessary:**
  - The sealed SC-D covered discrete zoom only and ended on any pointer motion.
  - Discrete pans, zoom-at-cursor and shell streams renegotiated on every step (40/40).
- **Evidence:** ER-5 R2, all with the 1R rule versus the epoch-disabled negative control:

  | Case | 1R rule | Epoch disabled (NEG) |
  |---|---|---|
  | discrete pan wiggle | 1 | 40 |
  | zoom-at-cursor wiggle | 1 | 40 |
  | oscillating shell stream | 1 | 40 |
  | reversing continuous pan | 1 | 20 |
  | observer enter/leave + unplug | 3 | 21 |
  | client self-resize near the device bound | 4 | 61 |

  The timeout's existence bounds changes; its value only groups input (T = 100/300/1000 ms gives 1 change; 40 ms gives 39, below the 50-ms input interval).
- **ADR:** ADR-05.
- **Tests/obligations:** OB-23 (now tunes T_settle), OB-28.

### INV-SCALE-4 (amended)
- **Previous:** "(a) no hint change per raw zoom event; (b) changes only at rung crossings or settlement; (c) per gesture/interaction: ≤ (rungs crossed) + 1; (d) no oscillation: within an epoch, re-evaluations caused only by client responses move h monotonically in the epoch's first direction (SC-E); a reversal needs a new non-client stimulus."
- **Trigger:** PD-F2-07.
- **Replacement:**
  - (a) No hint change per raw camera event (pan or zoom).
  - (b) Changes happen only at rung crossings, at settlement, or by P7.
  - (c) Per ScaleInteractionEpoch: at most (rungs crossed) + 1 settlement + at most one P7 forced decrease per non-client stimulus.
  - (d) No oscillation: outside an epoch, re-evaluations caused only by client responses move h only in the direction of the last non-client stimulus (SC-E); inside an epoch, INV-SCALE-3 applies. A reversal needs a new non-client stimulus.
- **Why necessary:** the sealed bound was stated per gesture, and non-gesture camera streams evaded it.
- **Evidence:** ER-5 R2 (bound holds in all seven scenarios).
- **ADR:** ADR-05.
- **Tests/obligations:** OB-28.

### INV-SCALE-5 (amended)
- **Previous:** "At zoom 1 with no zoom gestures the policy equals Phase B MaxGesture; integer hint = ⌈h⌉; emission only on change; unobserved elements hold h; evaluation happens once per core event batch, after all state updates of that batch."
- **Trigger:** PD-F2-08, PD-F2-09.
- **Replacement:**
  - At zoom 1 with no zoom interaction, **and where the maximum observing output scale ≤ H_max(E)**, the policy equals Phase B MaxGesture.
  - Integer hint = ⌈h⌉; emission only on change; unobserved elements hold h; evaluation once per core event batch after all its state updates.
  - **Initial hint** (before the initial configure): `h := t(ρ_pred)`, where ρ_pred applies INV-SCALE-1 to the presentations whose view intersects the placed window-geometry rect (anchor + configured size, or the anchor point when the size is 0×0). If no presentation intersects it, use t(maximum connected output scale). Mapping is a non-client stimulus.
- **Why necessary:**
  - The P4 equivalence is false beyond the device bound.
  - The initial hint was undefined; the harness used an ad-hoc rule that costs an extra renegotiation (ED-2 F8).
- **Evidence:** ED-2 F3, F8.
- **ADR:** ADR-05.
- **Tests/obligations:** T1 (map-time hint equals the first evaluation; 0 renegotiations at map).

### INV-FRAME-6 (amended)
- **Previous:** "`suspended` is set only from semantic reasons {SessionLocked, AllObservingOutputsOff, Parked}; never from camera visibility."
- **Trigger:** PD-F2-14.
- **Replacement:** `suspended` is set only from semantic reasons {SessionLocked, **OutputsOff**, Parked}; never from camera visibility. Every reason is defined **without** reference to camera-derived membership. `OutputsOff` means every connected output is powered off; its full definition is Level C.
- **Why necessary:** "all observing outputs off" is vacuously true for unobserved elements and flips on camera changes.
- **Evidence:** analysis (D2 M17).
- **ADR:** ADR-06.
- **Tests/obligations:** T1 when output power ships (Level C).

### INV-PRES-1 (amended)
- **Previous:** "Protocol fullscreen is an output presentation state (`OutputPresentation(O) = Exclusive(W)`), accepted only for the focused element or before first map; it never touches cameras or anchors; other cameras observe W at its actual (fullscreen) size; output removal/unmap ends it."
- **Trigger:** PD-F2-10.
- **Replacement:**
  - Protocol fullscreen is an output presentation state (`OutputPresentation(O) = Exclusive(W)`).
  - **Every `set_fullscreen` and `unset_fullscreen` is answered with exactly one configure.**
  - A request is **granted** only if W holds ActiveElementFocus, or if it arrives before W's first map (the initial configure carries it). Otherwise it is **declined at once** with a configure without `fullscreen`: no pending request is stored, and the client may request again.
  - The presentation starts when the grant is decided and ends when the exit is decided (unset, unmap, output removal, or replacement); border fill covers the size mismatch until the client commits.
  - A newer grant for an occupied output replaces the older one, whose element receives a configure leaving fullscreen with its saved size.
  - Loss of ActiveElementFocus does not end fullscreen.
  - Output choice runs when the grant is decided.
  - Fullscreen never touches cameras or anchors. Other cameras observe W at its actual size.
- **Why necessary:**
  - xdg-shell: "the compositor **will respond by emitting a configure event**" for both requests.
  - The sealed "record it and decide when W next gains focus" leaves the mandatory answer pending indefinitely (ER-3 negative control: 0 answers).
  - The presentation timing and the second-requester case were undefined.
- **Evidence:** ER-3 fullscreen: unfocused request gets 1 configure without `fullscreen`; focused request gets 1 configure `[fullscreen]` 1920×1080; unset gets 1 configure; anchor and cameras unchanged.
- **ADR:** ADR-09.
- **Tests/obligations:** OB-22, OB-32 (new).

### INV-PROTO-1 (new)
- **Trigger:** PD-F2-17.
- **Wording:**
  - **xdg_toplevel state policy (Foundation 01).**
  - `set_maximized` and `unset_maximized` are each answered with exactly one configure that does **not** grant `maximized`. Maximize never invokes viewport-fit, never moves a camera, and never changes Stage geometry. Smithay's default handler is bypassed in the adapter (W-SM-13).
  - A request before the first configure is recorded and ignored; the initial configure proceeds normally.
  - `set_minimized` is ignored (the protocol permits this).
  - `activated` is set exactly when the element holds ActiveElementFocus. It is emitted only on ActiveElementFocus transitions, never from camera or visibility, and such a configure is a kick trigger for an unobserved client.
  - `configure_bounds` is not sent in Foundation 01.
- **Why necessary:** the protocol requires an answer to maximize, and no policy existed. The W-SM-13 override removed the only path that answered it (ER-3 negative control: 0 answers).
- **Evidence:** ER-3 maximize: `set_maximized` and `unset_maximized` each get 1 configure `[]`; anchor and both cameras unchanged.
- **ADR:** ADR-09, ADR-14.
- **Tests/obligations:** OB-32 (new; includes the `activated` T1 case).

### INV-OUTPUT-1 (amended)
- **Previous:** "Output removal: membership `leave` before the global is destroyed; camera unbound but kept; fullscreen presentation dropped; interactions orphaned (ownership kept until release); pointer moved to a remaining output."
- **Trigger:** PD-F2-03, PD-F2-11.
- **Replacement:**
  - Output removal:
    - membership `leave` before the global is destroyed;
    - camera unbound but kept;
    - fullscreen presentation dropped, with the configure required by INV-PRES-1;
    - interactions orphaned: ownership kept until release, and **an orphaned interaction performs no camera mutation**;
    - the physical pointer is relocated at once to the Arrangement-nearest point of a surviving output (half-open);
    - constraints are reconciled per INV-INPUT-5 (deferred while a client grab is active);
    - the removal is a ScaleInteractionEpoch settlement event.
- **Why necessary:**
  - The orphaned-pan motion was undefined.
  - The constraint/relocation ordering under a client grab diverged (ER-2).
- **Evidence:** ER-2, ED-2 epoch 4.
- **ADR:** ADR-04, ADR-07.
- **Tests/obligations:** OB-3, OB-31.

---

## Unchanged invariants (40)

STAGE-1 … STAGE-6; CAMERA-2, CAMERA-3, CAMERA-4; COORD-1, COORD-4, COORD-6; RENDER-1 … RENDER-4; HIT-1; INPUT-1, -2, -3, -4, -6, -7, -9; SCALE-1; FRAME-1 … FRAME-5; SHELL-1, SHELL-2; XWAYLAND-1; ARCH-1, -2, -3; FAIL-1; OBS-1; IDLE-1; ALLOC-1.

Notes on unchanged entries:
- **INV-RENDER-1.** The formula is unchanged and render/hit agreement holds at k ∉ n/120 (ED-1). The ±1 px shape change at lattice-reference switches during zoomed panning is a stated visual residual, under obligation **OB-24 (rescoped)**.
- **INV-CAMERA-4.** "after termination one true-position pointer event" is consistent with the INV-INPUT-5 batch rule (at most one per batch).
- **INV-XWAYLAND-1.** PD-F2-16 needs no invariant change. ER-6 verified `-listenfd`; the restart design is an implementation obligation (OB-33).
