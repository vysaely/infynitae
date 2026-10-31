# D2 — State machines

Baseline: Phase C seal `615b0ed`. This document formalises 22 machines. Where a sealed table already exists, it stays **normative** and is referenced, not copied. This document adds the columns the sealed tables lack (writer, invalidations, events, invariants, tier) and the result of an explicit search for **reachable but undefined** transitions. Every such transition found is a register entry ([README.md](README.md)).

Conventions:
- **Writer**: the single authoritative writer of the machine's state.
- **Effects**: protocol side effects (via `Effect` → adapter).
- **Inval.**: derived state invalidated.
- **Ev.**: `tracing` target (C10) and shell event (C7.3).
- **Guards** are in `[…]`.
- "core batch" = one `Core` entry-point call; scale evaluation and (proposed, PD-F2-04) pointer reconciliation run once at its end.

Undefined-transition search method: for each machine, take every (state × trigger) pair. The triggers are all external events the system can receive: protocol requests, physical input, shell commands, outputs, timers, failures. Each pair was checked for a defined outcome in the sealed text. "Defined" means the sealed text states the next state and the side effects, or they follow uniquely from rules it states.

---

## M1 Element lifecycle (Stage view)

- **Sealed table:** B5.2/B5.3 (normative). Writer: core `Stage` (from adapter `ProtocolEvent`s). INV: STAGE-3/4/5, CAMERA-4, RENDER-3. Tier: T0 (transition table), T1 (crash/order).
- **States:** `AwaitingInitialCommit` → `AwaitingFirstBuffer` → `Mapped` ⇄ `Unmapped`; any live → `Destroyed`.

| From | Trigger [guard] | To | Effects | Inval. | Ev. |
|---|---|---|---|---|---|
| — | `new_toplevel` | AIC | none | — | `inf::stage` created; IPC `ElementCreated` |
| AIC | commit, no buffer | AFB | placement (`stage::place` (a)); **initial hint** (see below); initial configure | — | placed |
| AFB | commit + buffer [acked] | Mapped | z-top; enter per membership | MembershipTable, ProjectionFrame(next), ScaleState, clock | mapped |
| Mapped | commit + buffer | Mapped | damage | snapshot replaced; membership if geometry changed; P7/SC-E | — |
| Mapped | commit null buffer | Unmapped | leave all; dismiss popups; cancel interactions; focus successor; lock/confine deactivated (C4.3); FullscreenState dropped; ViewportFit restore | membership rows, ProjectionFrame (next), clock; ScaleState **held** | unmapped |
| Unmapped | initial commit again | AFB | initial configure (hint = held h) | — | — |
| any live | destroy / client death [idempotent] | Destroyed | none to the dead client; successor focus enter | all of the above + `SurfaceKey` registry entries | destroyed |

**Undefined, found:**
- **Initial hint:** ρ is defined only over observing presentations, and an element in AIC has none. The harness used an ad-hoc anchor-point rule (ED-5). **PD-F2-09.**
- **Attention mark** on unmap, and `transient_for` after the parent's unmap. **PD-F2-15.**
- A **deferred fullscreen request** pending at unmap/destroy. **PD-F2-10.**
- **ProjectionFrame entries** of a destroyed element until the next frame. **PD-F1-07** (fail closed).

## M2 xdg_toplevel protocol (configure/ack)

- Writer: Smithay protocol state, driven by adapter `Effect::Configure`; configure **causes** are core decisions. INV: CAMERA-4, PRES-1, FRAME-3 (non-initial configure = kick trigger), STAGE-4. Tier: T1.
- **States:** `RoleCreated` → `InitialConfigured(serial)` → `Acked` → `Mapped` → (`Unmapped`, `initial_configure_sent = false`) → …; `Destroyed`.
- **Configure causes allowed in Foundation 01:** initial; fullscreen enter/exit (C5.2); suspension reason change (INV-FRAME-6, reasons not in F01); output removal ending fullscreen.
- **Never a cause:** camera, membership, visibility, scale (preferred scale is a separate event).

| From | Trigger | To | Effects |
|---|---|---|---|
| RoleCreated | commit with buffer | client error | `ensure_configured` → protocol error (W-SM-6) → death path |
| RoleCreated | `set_fullscreen`/`set_maximized` before the first commit | RoleCreated | recorded; W-SM-13 override sends **nothing** |
| InitialConfigured | `ack_configure(s)` + buffer commit | Mapped | M1 |
| Mapped | `set_fullscreen` | Mapped | M9 |
| Mapped | `set_maximized` / `unset_maximized` | Mapped | **undefined**: the protocol requires a responding configure; no policy exists |
| Mapped | keyboard focus gained or lost | Mapped | **undefined**: the `activated` state is never mapped from the FocusModel |
| Mapped | `set_minimized` | Mapped | ignored (protocol permits it) |
| any | client death | Destroyed | M1 |

**Undefined, found:** maximize answer and `activated` mapping. **PD-F2-17 (Level B).**

## M3 Popup lifecycle

- **Sealed:** B6.1–B6.5, C4.6, D-B6-BOUNDARY, D-B6-NO-PAN-REPOSITION. Writer: Smithay `PopupManager` (protocol); core holds snapshot entries and the grab-chain record. INV: RENDER-2/3, HIT-1, CAMERA-4. Tier: T1 (OB-1, OB-4, OB-18).
- **States:** `Created(positioner)` → `Configured(pos)` → `Mapped` → [`Grabbing`] → `Dismissed`/`Destroyed`.

| From | Trigger [guard] | To | Effects | Inval. |
|---|---|---|---|---|
| Created | initial commit | Configured | position = `get_unconstrained_geometry(target)`, target per D-B6-BOUNDARY (anchor output → clock output → none) | — |
| Configured | buffer commit | Mapped | enter per surface membership (own, D-B8-ENTER) | membership, ProjectionFrame |
| Mapped | `grab(serial)` [serial valid; W-SM-7 precondition] | Grabbing | Smithay `PopupGrab` (pointer + keyboard) | **FocusModel bypassed (PD-F2-02)** |
| Mapped/Grabbing | `reposition` / reactive parent configure | same | new position | snapshot |
| any | camera change | same | **none** (no reposition, no configure) | membership only |
| Grabbing | compositor pointer-interaction acquisition | Dismissed | `popup_done` topmost first (C4.6) | — |
| any | parent unmapped/destroyed | Dismissed | `popup_done` for the whole tree | — |
| any | client death | Destroyed | `PopupManager::cleanup` | — |

**Undefined, found:**
- The constraint target is fractional, so a rounding point is needed. **PD-F2-12.**
- A keyboard-focus change by the FocusModel during `Grabbing` is silently dropped by Smithay. **PD-F2-02.**
- Anchor output removed after positioning: the popup is not repositioned (defined by D-B6-NO-PAN-REPOSITION; D5 E4 step 6). Not a finding.

## M4 Output lifecycle

- **Sealed:** B7.2, C8, INV-OUTPUT-1. Writer: core output manager (from backend descriptors and configuration). INV: OUTPUT-1, RENDER-3, CAMERA-3, FRAME-2, SCALE-*. Tier: T1 (OB-3), T4 (hotplug).
- **States:** `Absent` → `Enabled(mode, scale, transform, refresh, arrangement pos)` → `Removed`. DPMS-off is a Level C sub-state.

| From | Trigger | To | Effects | Inval. | Ev. |
|---|---|---|---|---|---|
| Absent | backend descriptor | Enabled | global created; bind camera (remembered by `OutputKey`, else new; placement Level D) | ladder (S2), Views, membership pass, clock, scale settle | `inf::visibility`; IPC `OutputAdded` |
| Enabled | mode/scale/transform change | Enabled | `wl_output` mode/scale; membership pass; scale re-evaluation (atomic unless a gesture is active) | View(O), ProjectionFrame(O), damage tracker(O) (**PD-F1-03**), ladder | — |
| Enabled | removal | Removed | **1.** leave for every surface on O; **2.** destroy global; **3.** unbind camera (kept); **4.** fullscreen on O dropped + unfullscreen configure; **5.** orphan interactions; **6.** lock deactivated + pointer → Arrangement-nearest | Views, ProjectionFrame(O), tracker(O), membership rows, ladder, clock | IPC `OutputRemoved` |

**Undefined, found:**
- After a mode/scale change, `Free(O, p)` can lie outside O's new logical rect. **PD-F2-04** (+ PD-F1-05 clamp).
- After the ladder changes, a held or deferred hint can be off-ladder. **PD-F2-08.**
- Removal while a client grab holds the pointer. **PD-F2-02.**

## M5 Camera lifecycle

- **Sealed:** B7.1–B7.3, INV-CAMERA-1/3. Writer: `core::navigation` (ports). Tier: T0/T1.
- **States:** `Bound(outputs ≠ ∅)` ⇄ `Unbound(OutputKey)`; created on first bind; never destroyed in-session (PD-F0-07).

| From | Trigger | To | Effects | Inval. |
|---|---|---|---|---|
| — | output added, no remembered camera | Bound | new `CameraId`; placement Level D | Views |
| Bound | last bound output removed | Unbound | kept, keyed by `OutputKey`; persisted | Views of that output |
| Unbound | same `OutputKey` reappears | Bound | view restored | Views, membership |
| Bound | navigation (input/shell) | Bound | centre/zoom (Q16 / f64) | Views of **all** bound outputs, membership, scale, ProjectionFrames (next) |
| Bound | client commit of a fitted element | Bound | **re-fit (C5.1) without a port** | — |
| Bound | animation tick | Bound | **mutation without a port** | — |

**Undefined, found:**
- Re-fit and animation reach `navigation` without a port. **PD-F2-01 (Level A).**
- A camera shared by N outputs while a fit or isolation is keyed per output. **PD-F2-11.**
- An orphaned pan's motion against an unbound camera. **PD-F2-11.**

## M6 Camera pan (pointer gesture)

- **Sealed:** B11.1 (verified, X-8), B10.4, C4.6. Writer: interaction state (intake) + `navigation`. INV: INPUT-1/2/3/4, COORD-3, CAMERA-2/4. Tier: T1 (X-8 port), T4 (OB-21), T5 (OB-20).
- **States:** `Idle` → `Panning{initiator, output, camera_start, Σ, lock}` → `Idle`, with `Orphaned` as a sub-state after output removal.

| From | Trigger [guard] | To | Effects |
|---|---|---|---|
| Idle | binding press [no client-owned button] | Panning | press consumed; popup grab chain dismissed first; `gesture_active = true` |
| Idle | binding press [client owns a button] | Idle | delivered to the client (X-8 C6) |
| Panning | motion Δ | Panning | `Σ += Δ`; `camera = start − Q(Σ)` (**no zoom term: PD-F2-06**); membership; scale increases only |
| Panning | any press | Panning | consumed until its release |
| Panning | initiator release | Idle | reconciliation (B11.4); deferred decreases |
| Panning | the pan's output removed | Orphaned | motion consumed. **Undefined:** does the motion still move the unbound camera? (**PD-F2-11**) |
| Orphaned | initiator release | Idle | pointer → Arrangement-nearest; reconciliation |
| Panning | cancel (binding, session lock) | Idle | no revert; owned buttons stay owned |

**Undefined, found:**
- Pan formula at zoom ≠ 1, and lattice jitter during zoomed panning. **PD-F2-06.**
- Orphaned-pan motion. **PD-F2-11.**
- Which output/camera a pan binding pressed while `Locked` acts on: `Locked` has no output. **PD-F2-03.**

## M7 Camera zoom interaction

- **Sealed:** CP3A-1 (S5/S6), C14.4 SC-D, INV-SCALE-3/4. Writer: `navigation` (zoom); `ScaleState` (hint). Tier: T0 (model), T1/T3 (OB-23, X-16).
- **Scope note:** Foundation 01's only zoom producer is viewport-fit (C5.1). Continuous and discrete zoom bindings are specified in C14.4 but are not Foundation features (B1.3-5). Fit-exit-by-navigation nevertheless makes **zoomed panning** reachable (PD-F2-06).
- **States:** `NoZoomInteraction` → `ContinuousZoom` (pinch; begin/end) / `DiscreteZoom` (SC-D) → `NoZoomInteraction`.

| From | Trigger | To | Effects |
|---|---|---|---|
| none | pinch begin | ContinuousZoom | gesture flag; increases at rung crossings |
| ContinuousZoom | pinch end | none | settle |
| none | wheel/key zoom step | DiscreteZoom | increase at rung crossings; no decrease |
| DiscreteZoom | button/key/**pointer motion**; settlement event | none | settle |

**Undefined / falsified, found:**
- SC-D ends on pointer motion, so zoom-at-cursor reverts to naive renegotiation (40/40).
- Discrete **pan** steps and shell delta streams have no interaction at all (40/40).

Both are **PD-F2-07.**

## M8 Viewport-fit

- **Sealed:** C5.1, INV-CAMERA-5. Writer: `navigation` (ports). Tier: T0/T1.
- **States:** `Unfitted` → `Fitted{W, saved centre, saved zoom, isolation}` → `Unfitted`.

| From | Trigger | To | Effects |
|---|---|---|---|
| Unfitted | fit(O, W) via port | Fitted | camera := (centre(W), zoom = min(view/W)); scale settle (atomic); focus may move to W (explicit action) |
| Fitted | restore via port | Unfitted | camera := saved; settle |
| Fitted | user navigates O's camera | Unfitted | **no restore; zoom kept → zoomed panning (PD-F2-06)** |
| Fitted | W resizes (client commit) | Fitted | **re-fit: camera mutation without a port (PD-F2-01)** |
| Fitted | W unmapped/destroyed | Unfitted | restore |
| Fitted | O removed | Unfitted | restore the camera values; camera unbound |
| Fitted | protocol fullscreen on O | Fitted (hidden) | presentation takes precedence for rendering |

**Undefined, found:**
- Fit on a camera shared with another output. **PD-F2-11.**
- Navigation through another output bound to the same camera. **PD-F2-11.**

## M9 Protocol fullscreen

- **Sealed:** C5.2, INV-PRES-1, CP2-Q1 resolution. Writer: `core::protocol` (FullscreenState) + output model (OutputPresentation). Tier: T1, T3/T5 (OB-22).
- **States:** `Windowed` → `Pending(output?)` (request while unfocused) → `Fullscreen{O, saved}` → `Windowed`.

| From | Trigger [guard] | To | Effects |
|---|---|---|---|
| Windowed | `set_fullscreen(o)` [focused, or before first map] | Fullscreen | output choice; configure `[fullscreen]` with O's logical size; `OutputPresentation(O) = Exclusive(W)`; other elements leave O; ρ_O = scale_O |
| Windowed | `set_fullscreen` [not focused] | Pending | recorded |
| Pending | W gains focus | Fullscreen | as above |
| Fullscreen | `unset_fullscreen` | Windowed | configure with the saved size; presentation returns to the camera view |
| Fullscreen | O removed | Windowed | presentation dropped; configure with the saved size |
| Fullscreen | unmap | Windowed | state dropped (spec) |

**Undefined, found (PD-F2-10):**
- Whether the presentation starts and ends at the decision or at the client's commit.
- `Pending` × {`unset_fullscreen`, unmap, destroy}: no cancellation and no bound.
- `Fullscreen{O}` × another element's accepted request for O.
- `Fullscreen` × W loses focus.
- `Pending(o)` × o removed.
- Presentation start/end on the pointer's output requires pointer reconciliation (**PD-F2-04**).

## M10 Pointer focus

- **Sealed:** C4.2, C4.5, B10.6, INV-HIT-1, INV-INPUT-3. Writer: core `PointerModel` + intake. **During client-owned Smithay grabs, Smithay decides the delivered focus (C4.2 P3).** Tier: T1.
- **States:** `None` / `Focused(surface, origin)` / `KeptDuringInteraction(surface)` / `ClientGrab(surface)` (Smithay-decided).

| From | Trigger | To | Effects |
|---|---|---|---|
| any | client-routed motion | per hit-test | enter/leave/motion |
| Focused | compositor interaction acquired | KeptDuringInteraction | none (KeepFocus) |
| KeptDuringInteraction | termination | per re-hit | ≤ 1 true-position event (B11.4) |
| Focused | client button press | ClientGrab | Smithay implicit grab |
| ClientGrab | release | per hit-test | Smithay decides |
| any | atomic camera change / fit / fullscreen presentation / output reconfiguration / element move | **undefined**: no re-hit, no event | — |

**Undefined, found:**
- No reconciliation after non-pan view changes. **PD-F2-04.**
- Core's prediction of the delivered focus under `ClientGrab`, used by the W-SM-4a decision. **PD-F2-02.**

## M11 Keyboard focus

- **Sealed:** C4.5, D-B10-KBD, INV-INPUT-7/8, RC-1. Writer: core `FocusModel`. Tier: T0, T1, T3 (OB-10).
- **States:** `None` / `Element(E)` / `Popup(P of E)`, plus `history` and the `attention` set.

| From | Trigger | To | Effects |
|---|---|---|---|
| any | click-to-focus, focus binding, shell `Focus`, activation grant | Element(E) | leave/enter(keys) + modifiers (Smithay) |
| Element(E) | target loss (unmap/destroy/session lock) | successor (history) | enter without leave if the client destroyed it (RC-1) |
| Element(E) | client popup grab | Popup(P) | **written by Smithay's `PopupKeyboardGrab`, not by the FocusModel** |
| Popup(P) | grab ends | Element(E) | **Smithay calls `set_focus(root)` itself** (grab.rs:378) |
| Popup(P) | FocusModel change (shell/activation/binding/successor) | **diverges**: Smithay ignores `set_focus` while the grab is active (grab.rs:456-476) | — |
| any | camera, visibility, pointer motion | unchanged | — |

**Undefined / falsified, found:**
- Two writers of keyboard focus, and silent divergence. **PD-F2-02 (Level A).**
- The `activated` state is not mapped. **PD-F2-17.**

## M12 Physical-key ownership

- **Sealed:** C4.4 K1–K9 (normative), X-10. Writer: core intake (ownership table); Smithay executes intercept/forward. INV: INPUT-1/6/7. Tier: T1 (+ NEG).
- **States per key:** `Up` → `Down(Client)` | `Down(Compositor)` → `Up`.

| From | Trigger [guard] | To | Effects |
|---|---|---|---|
| Up | press [binding match ∧ ¬mods_changed] | Down(Compositor) | consumed |
| Up | press [else] | Down(Client) | forwarded (incl. observe-mode bindings on modifiers, K3) |
| Down(Compositor) | release | Up | consumed |
| Down(Client) | release | Up | forwarded to the **current** focus |
| Down(*) | focus change | same | enter(forwarded keys) + modifiers |
| Down(*) | device removed | Up | treated as a physical release (C8) |
| Down(Compositor) | interaction cancelled | same | stays owned until release (K6) |

**Undefined:** none found beyond PD-F2-02. With a popup grab active, a forwarded key goes to the popup's client even if the FocusModel moved focus elsewhere.

## M13 Pointer lock

- **Sealed:** B11.2/B11.4, INV-INPUT-5, W-SM-4a/4b, AUD-4. Writer: core activation policy + `PointerModel`; Smithay owns the protocol objects. Tier: T1, T5 (OB-20), UP (OB-11).
- **States:** `Requested(inactive)` → `Active{Locked(...)}` → `Inactive` (persistent) / `Destroyed` (oneshot).

| From | Trigger [guard] | To | Effects |
|---|---|---|---|
| Requested | [focused ∧ pointer in region ∧ no pan acquisition] | Active | `locked`; pointer := `Locked(S, q_lock)` |
| Active | camera pan (any) | Active | none (KeepFocus) |
| Active | pan end [lock point observable on the pointer's output] | Active | none |
| Active | pan end [not observable] | Inactive | W-SM-4a ordering; one `unlocked` |
| Active | output removed | Inactive | deactivate; relocate pointer (AUD-4) |
| Active | unmap/destroy of S | Inactive | — |
| Requested(oneshot) | pointer leaves S without activation | Destroyed | **W-SM-4b defect** (OB-11) |

**Undefined / falsified, found:**
- `Locked` has no output, so "the pointer's output" is undefined. **PD-F2-03.**
- The Canvas lock point drifts from S on geometry.loc changes and element moves. **PD-F2-03.**
- No reconciliation after a shell or keyboard camera change, fullscreen presentation or output reconfiguration. **PD-F2-04.**
- Output removal under a client implicit grab: the W-SM-4a decision is wrong. **PD-F2-02.**

## M14 Pointer confinement

- **Sealed:** C4.3, B11.4 (row "confined"). Writer: as M13. Tier: T1.
- **States:** `Requested` → `Active(region)` → `Inactive` / `Destroyed`.

| From | Trigger | To | Effects |
|---|---|---|---|
| Requested | [focused ∧ inside region] | Active | `confined` |
| Active | pointer motion | Active | **clamp rule undefined** (domain, crossing) |
| Active | pan moves the surface away from the pointer | Inactive | `leave` + `unconfined` |
| Active | after a view change, pointer over S but outside the region | **undefined** | — |

**Undefined, found:** clamp domain, Arrangement crossing while confined, and pointer outside the region but over S. **PD-F2-05.**

## M15 Scale hint (epoch and interaction)

- **Sealed:** CP3A-1 S1–S9, P1–P7; C14.4 SC-D/SC-E/P7 lock; INV-SCALE-1…5. Writer: core `ScaleState`. Tier: T0 (model properties), T1 (OB-13, OB-28), TUNE.
- **State:** `h`, `epoch{direction ∈ {up, down, none}}`, `gesture_active`, `discrete_zoom_active`, ladder L(E).

| From | Trigger | To | Effects |
|---|---|---|---|
| any | evaluation at batch end, t > h | h := t | emit (if observed) |
| any | evaluation, t < h [no gesture ∧ no SC-D ∧ (stimulus ∨ epoch dir down)] | h := t | emit |
| any | client geometry response, reversing the epoch direction | unchanged | wait for a stimulus (SC-E) |
| any | size change so that h > H_max | h := t (P7) | epoch := down |
| any | unobserved | hold | — |
| — | **first map (no observing presentation)** | **undefined** | — |
| any | ladder change (hotplug) while held or deferred | **h off-ladder** | — |

**Undefined / falsified, found:**
- Initial hint. **PD-F2-09.**
- Off-ladder h; contradiction between the device bound and "every output scale"; H_max basis. **PD-F2-08.**
- Discrete camera interactions; SC-D ending on motion. **PD-F2-07.**

## M16 Off-screen frame callbacks

- **Sealed:** B9.3/B9.6 (normative), INV-FRAME-1…5, AUD-24. Writer: core `FrameLiveness`; the adapter completes callbacks on `Effect::CompleteFrames`. Tier: T1, T3/T5.
- **States per element:** `Observed{clock c}` / `Unobserved{last_completion, timer_armed}`.

| From | Trigger | To | Effects |
|---|---|---|---|
| Observed | clock vblank | Observed | complete once; `presented` feedback (sync c) |
| Observed | leaves the last output | Unobserved | withheld; timer armed iff a callback is pending (**PD-F1-06**) |
| Unobserved | kick (ping, close, non-initial configure, `popup_done`, input) | Unobserved | complete once before the request |
| Unobserved | timer [≥ P since last completion] | Unobserved | complete once |
| Unobserved | re-entry | Observed | complete at the first vblank of the new clock (≤ 1 period) |
| Observed | clock output removed | Observed{new c} / Unobserved | handover ≤ old + new period |

**Undefined:** none, beyond PD-F1-06 (the core-side "callback pending" datum is needed to honour the timer rule).

## M17 Suspension

- **Sealed:** D-B9-SUSPEND, INV-FRAME-6. Writer: core `SuspensionReasons`. Tier: T1.
- **State:** `reasons ⊆ {SessionLocked, AllObservingOutputsOff, Parked}`. `suspended = OR`; each flip is one configure (a kick trigger). Foundation 01: no reason is producible, so `suspended` is never sent.

**Falsified, found:** `AllObservingOutputsOff` is defined over camera-derived membership. It is vacuous for unobserved elements and flips on camera changes. **PD-F2-14.**

## M18 Shell connection and restart

- **Sealed:** C7.1–C7.3, INV-SHELL-1/2. Writer: `infynitae-ipc` (connection state; holds `ShellPort`). Tier: T1.
- **States:** `Listening` → `Connected(peer uid ok)` → `Subscribed(gen ≥ g)` → `Closed`.

| From | Trigger [guard] | To | Effects |
|---|---|---|---|
| Listening | connect [`SO_PEERCRED` uid matches] | Connected | — |
| Listening | connect [uid mismatch] | Listening | refused |
| Connected | `Snapshot` | Connected | generation-stamped copy |
| Connected | `Subscribe(g)` | Subscribed | events with gen > g |
| any | command | same | validate → single-writer path → response (request id, gen) |
| any | disconnect | Closed | nothing rolled back; animations continue (**PD-F2-01**: the animation's authority path is undefined) |

**Undefined, found:** the attention lifecycle as seen by the event stream. **PD-F2-15.**

## M19 xwayland-satellite lifecycle

- **Sealed:** C6, C8 (satellite row), INV-XWAYLAND-1. Writer: supervisor in the bin crate. Tier: T1/T3 (OB-9, OB-27).
- **States:** `Disabled` → `Starting(display :N)` → `Running` → `Crashed` → `Backoff(n)` → `Starting` …; `Degraded` (X11 disabled after repeated crashes).

| From | Trigger | To | Effects |
|---|---|---|---|
| Disabled | startup or first X11 connection (lazy) | Starting | spawn with `WAYLAND_DISPLAY`; export `DISPLAY` to children |
| Running | satellite exits | Crashed | X11 clients die; their elements are destroyed via client death |
| Crashed | backoff elapsed | Starting | **same or a different `:N`? undefined** |
| Backoff | repeated crashes | Degraded | logged; X11 disabled |

**Undefined, found:** display identity across restarts, and who owns the X socket for lazy start (`-listenfd` unverified). **PD-F2-16.**

## M20 Persistence restore (Level D; shape only)

- **Sealed:** C8 (persistence row), B7.2 (camera by `OutputKey`), B4 (`RestoreKey`). Writer: core persistence service. Tier: Level D.
- **States:** `NotLoaded` → `Validated(all-or-nothing)` → `Applied` / `Rejected(kept for inspection)`.
- Cameras restore per `OutputKey` (unbound until the output appears); element anchors restore through placement policy (a) at initial commit when the `RestoreKey` matches.

**Undefined:** only Level D details (match collisions, retention: PD-F0-07). No Foundation finding.

## M21 Backend and renderer recovery

- **Sealed:** C8 (renderer/backend rows), INV-FAIL-1. Writer: bin + backend. Tier: T1 (failure injection), T4.
- **States:** `Running` → `RendererLost` → `Recreating` → `Running` | `Terminated`.

| From | Trigger | To | Effects | Inval. |
|---|---|---|---|---|
| Running | GPU reset / context lost | RendererLost | no frames → observed callbacks wait (bounded by recovery time) | — |
| RendererLost | recreate OK | Running | re-import buffers; full damage | damage trackers; textures; **`D_max` re-query (PD-F1-08)** → possible P7 |
| RendererLost | recreate fails | Terminated | orderly shutdown | — |
| Running | backend lost (DRM/winit) | Terminated | Foundation 01 rule | — |

**Undefined:** `D_max` re-query only (D-F1).

## M22 Compositor interaction (B10.4)

- **Sealed:** B10.4 (normative), C4.6, INV-INPUT-1/2/3. Writer: core intake. Tier: T1.
- **States:** `Normal` / `CameraPan` / `WindowMove` / `WindowResize` (+ orphaned).
- Animated navigation is called a "compositor interaction" in C7.3/AUD-6 but is **not** a state here.

**Undefined, found:**
- An animation has no state in this machine. Its pointer ownership (none?), its gesture flag (AUD-6 says it counts as a gesture for scale), and its supersession by "user input" (C7.3: which input?) are undefined. **PD-F2-01** (authority path) and **PD-F2-07** (scale semantics).
- `WindowMove` across outputs. **PD-F2-13.**
- `WindowMove` of a locked element. **PD-F2-03.**

---

## Summary of the undefined-transition search

| Machine | Undefined or falsified transitions | Register |
|---|---|---|
| M1 lifecycle | initial hint; attention/`transient_for`; pending fullscreen at unmap; stale ProjectionFrame | PD-F2-09, PD-F2-15, PD-F2-10, PD-F1-07 |
| M2 xdg_toplevel | maximize answer; `activated` | PD-F2-17 |
| M3 popup | rounding of the constraint target; focus during a grab | PD-F2-12, PD-F2-02 |
| M4 output | pointer outside the shrunk output; off-ladder h; removal under a grab | PD-F2-04, PD-F2-08, PD-F2-02 |
| M5 camera | port-free mutation; shared camera; orphaned pan | PD-F2-01, PD-F2-11 |
| M6 pan | zoom term + jitter; orphaned motion; pan while locked | PD-F2-06, PD-F2-11, PD-F2-03 |
| M7 zoom | SC-D ends on motion; discrete pan | PD-F2-07 |
| M8 fit | re-fit authority; exit keeps zoom; shared camera | PD-F2-01, PD-F2-06, PD-F2-11 |
| M9 fullscreen | timing; pending lifetime; second requester; focus loss; output gone | PD-F2-10 |
| M10 pointer focus | no reconciliation outside pan; grab prediction | PD-F2-04, PD-F2-02 |
| M11 keyboard focus | two writers; `activated` | PD-F2-02, PD-F2-17 |
| M12 key ownership | — (via PD-F2-02) | — |
| M13 lock | output; drift; triggers; grab | PD-F2-03, PD-F2-04, PD-F2-02 |
| M14 confinement | clamp/crossing/outside region | PD-F2-05 |
| M15 scale | initial; off-ladder; discrete interactions | PD-F2-09, PD-F2-08, PD-F2-07 |
| M16 frames | pending-callback datum | PD-F1-06 |
| M17 suspension | reason over camera membership | PD-F2-14 |
| M18 shell | attention events; animation authority | PD-F2-15, PD-F2-01 |
| M19 satellite | display identity | PD-F2-16 |
| M20 persistence | Level D only | — |
| M21 recovery | `D_max` re-query | PD-F1-08 |
| M22 interaction | animation state; cross-output move | PD-F2-01, PD-F2-07, PD-F2-13, PD-F2-03 |
