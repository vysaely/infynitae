# Infynitae Native — Foundation State Machines (effective)

Status: **Phase D checkpoint 2 (D7) living document.** Canonical living home of the state-machine namespace `M*` (FOUNDATION_ARCHITECTURE §P0).

These are the **effective** machines after the 1R and 2R corrections. Every transition cites the invariant (FOUNDATION_INVARIANTS.md) or ADR (FOUNDATION_DECISIONS.md) it implements. Objects are named by `OBJ-*` (FOUNDATION_OBJECT_MODEL.md) and formulas by `F-*` (FOUNDATION_ARCHITECTURE §F).

Historical tables are provenance only and are not edited: BLOCKERS B5.3, B9.6, B10.4 and B11 (Phase B); audit/D2 (e8ec03a); audit/1R/STATE_MACHINES_1R (84ce620); audit/2R/MATRICES_2R (5827926).

One core batch performs, in order:
1. state updates;
2. focus preemption (INV-INPUT-10);
3. constraint reconciliation (INV-INPUT-5);
4. scale evaluation or settlement (INV-SCALE-3, INV-SCALE-5);
5. effects emitted.

<!-- living:SM -->
## M1 — Element lifecycle
Object: OBJ-01/02. Implements INV-STAGE-3, INV-STAGE-4, INV-STAGE-5.

| From | Trigger [guard] | To | Effects | Cites |
|---|---|---|---|---|
| — | `new_toplevel` | AwaitingInitialCommit | mint `ElementId`; no anchor | INV-STAGE-3 |
| AwaitingInitialCommit | commit without buffer | AwaitingFirstBuffer | placement via `stage::place`; initial hint `t(ρ_pred)` (F-SCALE-3); initial configure (pre-map fullscreen granted in it, INV-PRES-1) | INV-STAGE-1, INV-SCALE-5 |
| AwaitingInitialCommit | commit with buffer | client error | `ensure_configured` → protocol error → client death (W-SM-6) | INV-STAGE-4 |
| AwaitingFirstBuffer | buffer commit after ack | Mapped | z-top; membership (F-MEMBER-1); epoch settlement | INV-STAGE-4, INV-RENDER-3 |
| Mapped | commit with buffer | Mapped | snapshot replaced (OBJ-05); membership if geometry changed; P7 check | INV-COORD-6, INV-SCALE-2 |
| Mapped | null-buffer commit | Unmapped | leave all outputs; popups dismissed; interactions cancelled; focus successor; constraints end (M13); fullscreen dropped (M9); hint held | INV-STAGE-5, INV-INPUT-8, INV-PRES-1 |
| Unmapped | initial commit again | AwaitingFirstBuffer | initial configure; anchor kept | INV-STAGE-5 |
| any live | destroy / client death [idempotent, any order] | Destroyed | purge every reference; no event to the dead client | INV-STAGE-4, INV-FAIL-1 |
| any | attention / `transient_for` on unmap or destroy | — | **open implementation obligation OB-35** | INV-STAGE-4 |

## M2 — xdg_toplevel requests
Object: OBJ-19, OBJ-22. Implements INV-PROTO-1, INV-PRES-1, INV-CAMERA-4. Every configure to an unobserved client is preceded by a kick (INV-FRAME-3).

| From | Trigger [guard] | To | Effects | Cites |
|---|---|---|---|---|
| pre-configure | `set_maximized` / `set_fullscreen` | same | recorded; W-SM-13 override sends nothing; fullscreen may be granted in the initial configure | INV-PROTO-1, INV-PRES-1 |
| Mapped | `set_maximized` / `unset_maximized` | Mapped | exactly one configure, without `maximized`; no camera or Stage change | INV-PROTO-1 |
| Mapped | `set_minimized` | Mapped | ignored | INV-PROTO-1 |
| Mapped | ActiveElementFocus gained/lost | Mapped | configure with/without `activated` | INV-PROTO-1, INV-INPUT-8 |
| Mapped | `set_fullscreen` / `unset_fullscreen` | M9 | exactly one configure | INV-PRES-1 |
| Mapped | camera, membership, visibility, scale | Mapped | **no configure** | INV-CAMERA-4 |

## M3 — Popup lifecycle
Object: OBJ-05, OBJ-23. Implements INV-RENDER-2, INV-HIT-1, INV-INPUT-10, INV-COORD-2 (R8).

| From | Trigger [guard] | To | Effects | Cites |
|---|---|---|---|---|
| Created | initial commit | Configured | position = `get_unconstrained_geometry(target)`; target = anchor-output viewport, inward-rounded (R8, F-ROUND-1) | INV-COORD-2 |
| Configured | buffer commit | Mapped | per-surface membership | INV-RENDER-3 |
| Mapped | `grab(serial)` [valid; root = focused element; W-SM-7 precondition] | Grabbing | Smithay popup grabs (nested: keep the pointer grab, W-SM-14) | INV-INPUT-10 |
| Grabbing | core changes ActiveElementFocus | Dismissed | ordered preemption: `ungrab(All)` → keyboard `unset_grab` → pointer `unset_grab` → `set_focus` | INV-INPUT-10 |
| Grabbing | compositor pointer-interaction acquisition | Dismissed | `popup_done` topmost first before the camera change | INV-INPUT-2 |
| any | camera change / anchor output removed | same | no reposition, no configure | INV-CAMERA-4 |
| any | parent unmapped/destroyed | Dismissed | `popup_done` for the tree | INV-STAGE-4 |

## M4 — Output lifecycle
Object: OBJ-10, OBJ-07, OBJ-08. Implements INV-OUTPUT-1, INV-OUTPUT-2, INV-CAMERA-3.

| From | Trigger [guard] | To | Effects | Cites |
|---|---|---|---|---|
| Absent | `output_connected(&mut OutputLifecycleToken, descriptor)` (startup and hotplug alike) | Enabled | global created; camera bound by precedence remembered → persisted → default (M5); Views, membership, ladder; epoch settlement | INV-OUTPUT-2, INV-CAMERA-3, INV-SCALE-3 |
| Enabled | mode/scale/transform change (descriptor update through the token entry) | Enabled | View/ProjectionFrame/damage tracker rebuilt (F-RENDER-1); membership; reconciliation trigger (M13); no camera value change | INV-OUTPUT-2, INV-INPUT-5 |
| Enabled | `output_disconnected(&mut OutputLifecycleToken, id)` | Removed | leave before the global is destroyed; camera unbound and kept; fullscreen dropped with its configure; interactions orphaned without camera mutation; pointer relocated (F-RELOCATE-1); constraints reconciled (deferred under a client grab); settlement | INV-OUTPUT-1 |
| any | protocol event or caller-supplied coordinates | — | impossible: no such entry or field | INV-OUTPUT-2 |

## M5 — Camera lifecycle
Object: OBJ-07, OBJ-09, OBJ-30. Implements INV-CAMERA-1, INV-CAMERA-3, INV-CAMERA-5.

| From | Trigger | To | Effects | Cites |
|---|---|---|---|---|
| — | `output_connected`, no remembered camera for the key | Bound | new camera seeded from the persisted lookup or the internal default; one proof | INV-CAMERA-3, INV-OUTPUT-2 |
| Unbound | `output_connected`, same `OutputKey` | Bound | re-bound, value unchanged | INV-CAMERA-3 |
| Bound | last bound output disconnected | Unbound | value kept | INV-OUTPUT-1 |
| Bound | navigation via `InputPort`/`ShellPort` (pan, jump, fit, restore) | Bound | one mutation per minted proof | INV-CAMERA-1 |
| any | protocol event, client commit, timer | unchanged | — | INV-CAMERA-1 |

## M6 — Camera pan
Object: OBJ-26, OBJ-07. Implements INV-INPUT-1/2/3/4, INV-COORD-3, INV-CAMERA-4.

| From | Trigger [guard] | To | Effects | Cites |
|---|---|---|---|---|
| Idle | binding press [client owns no button] | Panning | press consumed; popup grabs dismissed (M3); epoch opens (CameraPan) | INV-INPUT-2, INV-SCALE-3 |
| Idle | binding press [client owns a button] | Idle | delivered to the client | INV-INPUT-2 |
| Panning | motion `dp_phys` | Panning | camera per F-PAN-1; membership; increases at rungs only; no client event | INV-COORD-3, INV-INPUT-3 |
| Panning | initiator release | Idle | epoch settles; batch-end reconciliation | INV-INPUT-5, INV-SCALE-3 |
| Panning | the pan's output removed | Orphaned | motion consumed and discarded | INV-OUTPUT-1 |
| Orphaned | initiator release | Idle | reconciliation on the relocated output | INV-INPUT-5 |
| any | **persistent zoom ≠ 1** | — | **gated by decision OB-24** (fallback: fit-exit restores baseline zoom) | INV-COORD-3 |

## M7 — ScaleInteractionEpoch
Object: OBJ-16. Implements INV-SCALE-3, INV-SCALE-4, INV-IDLE-1. Kinds: CameraPan, CameraZoom, WindowMove, WindowResize, ShellStream.

| From | Trigger | To | Effects | Cites |
|---|---|---|---|---|
| Closed | continuous interaction begins | Open{continuous} | — | INV-SCALE-3 |
| Closed | discrete spatial step or shell stream command | Open{discrete, deadline = now + T_settle (PROV-5)} | evaluate (increase only) | INV-SCALE-3 |
| Closed | atomic camera or Stage action | Closed | evaluate once and settle | INV-SCALE-3 |
| Open | further step | Open | increases at rung crossings in the epoch direction; decreases suppressed | INV-SCALE-4 |
| Open | client response | Open | no reversal of direction | INV-SCALE-4 |
| Open | `h > H_max(E)` (device bound) | Open{down} | immediate decrease (P7); never used for ordinary decreases | INV-SCALE-2, INV-SCALE-3 |
| Open | pointer motion / unrelated key or button | Open | nothing | INV-SCALE-3 |
| Open | gesture end / release / end-of-stream / deadline / output settlement event | Closed | settle once to `t(ρ)` (F-SCALE-3) | INV-SCALE-3, INV-SCALE-5 |

## M8 — Viewport-fit (one-shot)
Object: OBJ-07, OBJ-30. Implements INV-CAMERA-5, INV-CAMERA-1.

| From | Trigger | To | Effects | Cites |
|---|---|---|---|---|
| — | `FitViewport(O, W)` via input or shell | — | snapshot W now; F-FIT-1; one mutation; record the previous value (OBJ-30); atomic epoch | INV-CAMERA-5 |
| — | `ViewportRestore(O)` via input or shell | — | one mutation to the recorded value | INV-CAMERA-5 |
| — | W resizes / commits / unmaps | — | nothing (no mode exists) | INV-CAMERA-1 |

## M9 — Protocol fullscreen
Object: OBJ-19. Implements INV-PRES-1.

| From | Trigger [guard] | To | Effects | Cites |
|---|---|---|---|---|
| Windowed | `set_fullscreen(o)` [ActiveElementFocus = W, or pre-map] | Fullscreen{O} | output choice; configure `[fullscreen]`; presentation starts (F-PRESENT-1); other elements leave O; reconciliation trigger; settlement | INV-PRES-1 |
| Windowed | `set_fullscreen` [not focused] | Windowed | configure without `fullscreen`; nothing stored | INV-PRES-1 |
| Fullscreen{O} | `unset_fullscreen` / unmap | Windowed | configure with the saved size; presentation ends | INV-PRES-1 |
| Fullscreen{O} by W | grant for O to another element | Windowed (W) | W gets a configure with its saved size | INV-PRES-1 |
| Fullscreen | W loses ActiveElementFocus | Fullscreen | unchanged | INV-PRES-1 |
| Fullscreen{O} | O disconnected | Windowed | configure with the saved size | INV-OUTPUT-1 |

## M10 — Pointer focus
Object: OBJ-20, OBJ-24, OBJ-15. Implements INV-HIT-1, INV-INPUT-3, INV-INPUT-5.

| From | Trigger | To | Effects | Cites |
|---|---|---|---|---|
| any | client-routed motion | per F-HIT-1 | enter/leave/motion | INV-HIT-1 |
| Focused | compositor interaction acquired | Kept | no events (KeepFocus) | INV-INPUT-3 |
| Kept | interaction ends | per re-hit | ≤ 1 true-position event per batch | INV-INPUT-5, INV-CAMERA-4 |
| Focused | client button press | ClientGrab | Smithay decides the delivered focus; core uses the delivered focus for protocol-order decisions | INV-INPUT-5 |
| any | view change, geometry change, output change, ActiveElementFocus change | per batch-end reconciliation | at most one true-position event | INV-INPUT-5 |

## M11 — ActiveElementFocus and ProtocolSurfaceRoute
Object: OBJ-22, OBJ-23. Implements INV-INPUT-8, INV-INPUT-10, INV-PROTO-1. ADR-14.

| From | Trigger | To | Effects | Cites |
|---|---|---|---|---|
| any | click-to-focus, focus binding, shell focus, activation grant | Element(E) | M3 preemption if a grab is active; `leave`/`enter(keys)`; `activated` configures | INV-INPUT-8, INV-INPUT-10, INV-PROTO-1 |
| Element(E), route ToRoot | valid popup grab in E | Element(E), route Grab | ActiveElementFocus unchanged | INV-INPUT-10 |
| Element(E), route Grab | grab ends (client) | Element(E), route ToRoot | Smithay root restore inside E | INV-INPUT-10 |
| Element(E) | target loss | successor | `enter(keys)` without `leave` after client-side destruction | INV-INPUT-7, INV-INPUT-8 |
| any | camera, visibility, pointer motion | unchanged | — | INV-INPUT-8 |

## M12 — Physical-key ownership
Object: OBJ-25, OBJ-27. Implements INV-INPUT-1, INV-INPUT-6, INV-INPUT-7.

| From | Trigger [guard] | To | Effects | Cites |
|---|---|---|---|---|
| Up | press [binding ∧ ¬mods_changed] | Down(Compositor) | consumed | INV-INPUT-1 |
| Up | press [otherwise, incl. bindings on modifier keys] | Down(Client) | forwarded (observe mode for modifiers) | INV-INPUT-6 |
| Down(Compositor) | release | Up | consumed | INV-INPUT-1 |
| Down(Client) | release | Up | forwarded to the current focus | INV-INPUT-7 |
| Down(*) | focus change | same | `enter(forwarded keys)` + modifiers | INV-INPUT-7 |
| Down(*) | device removed | Up | treated as a physical release | INV-INPUT-1 |

## M13 — Pointer constraint (lock and confinement)
Object: OBJ-20, OBJ-21, OBJ-24. Implements INV-COORD-5, INV-INPUT-5, INV-OUTPUT-1.

| From | Trigger [guard] | To | Effects | Cites |
|---|---|---|---|---|
| Requested | batch end [ActiveElementFocus = element(S) ∧ pointer focus S ∧ inside region ∧ no acquisition] | Active | `locked`/`confined` once | INV-INPUT-5 |
| Active | any trigger while a compositor interaction or client-owned grab is active | Active (deferred) | core = Smithay; no absolute motion to a locked surface; relative only from physical deltas | INV-INPUT-5 |
| Active (lock) | batch end [lock point (F-LOCK-1) observable through the interaction output ∧ ActiveElementFocus = element(S)] | Active | nothing | INV-INPUT-5 |
| Active (lock) | batch end [otherwise] | Inactive / destroyed (oneshot) | exactly one `unlocked`; explicit deactivate only if the delivered focus does not change (W-SM-4) | INV-INPUT-5 |
| Active (confine) | motion | Active | clamp per F-CONFINE-1; no Arrangement crossing | INV-INPUT-5 |
| Active (confine) | batch end [pointer outside the region] | Inactive | one `unconfined`; no warp | INV-INPUT-5 |
| Active | interaction output disconnected | Active (deferred) or reconciled | pointer relocated at once (F-RELOCATE-1) | INV-OUTPUT-1 |
| Active | S destroyed/unmapped | — | ended through the protocol path | INV-INPUT-5 |

## M14 — Confinement clamp
Part of M13 (the clamp rows). It keeps its identifier for continuity with audit/D2. Implements INV-INPUT-5 via F-CONFINE-1.

## M15 — Scale hint
Object: OBJ-16. Implements INV-SCALE-1, INV-SCALE-2, INV-SCALE-5.

| From | Trigger | To | Effects | Cites |
|---|---|---|---|---|
| — | placement done (before the initial configure) | h = `t(ρ_pred)` | emitted before the initial configure | INV-SCALE-5 |
| any | batch-end evaluation | per M7 | target on the ladder in force (F-SCALE-2, F-SCALE-3); emission only on change; unobserved elements hold | INV-SCALE-2, INV-SCALE-5 |
| any | ladder change (hotplug) | held value may be off-ladder | every emitted value is a rung | INV-SCALE-2 |

## M16 — Off-screen frame callbacks
Object: OBJ-17, OBJ-05. Implements INV-FRAME-1 … INV-FRAME-5, INV-IDLE-1.

| From | Trigger | To | Effects | Cites |
|---|---|---|---|---|
| Observed{c} | vblank of the clock output c | Observed | complete once; `presented` feedback (sync c) | INV-FRAME-2, INV-FRAME-5 |
| Observed | last observing output lost | Unobserved | withhold; fallback timer armed only if `frame_requested` (OBJ-05) | INV-FRAME-3, INV-IDLE-1 |
| Unobserved | kick: ping, close, non-initial configure, `popup_done`, input | Unobserved | complete once **before** the request | INV-FRAME-3 |
| Unobserved | fallback timer [≥ P (PROV-4) since the last completion] | Unobserved | complete once | INV-FRAME-3 |
| Unobserved | re-entry | Observed | complete at the first vblank of the new clock | INV-FRAME-4 |

## M17 — Suspension
Object: OBJ-18. Implements INV-FRAME-6. Reasons {SessionLocked, OutputsOff, Parked}, none producible in Foundation 01. Each flip sends one configure, a kick trigger. No camera or visibility input.

## M18 — Shell connection
Object: OBJ-31. Implements INV-SHELL-1, INV-SHELL-2.

| From | Trigger [guard] | To | Effects | Cites |
|---|---|---|---|---|
| Listening | connect [`SO_PEERCRED` uid matches] | Connected | — | INV-SHELL-1 |
| Connected | Snapshot / Subscribe(g) | Subscribed | generation-stamped copies | INV-SHELL-2 |
| any | semantic command | same | validated single-writer path (camera commands mint one proof each; shell streams form an epoch) | INV-SHELL-1, INV-CAMERA-1, INV-SCALE-3 |
| any | disconnect | Closed | nothing rolled back; nothing continues (no animation in Foundation 01) | INV-SHELL-2, INV-CAMERA-1 |

## M19 — xwayland-satellite
Object: OBJ-32. Implements INV-XWAYLAND-1. ADR-10.

| From | Trigger | To | Effects | Cites |
|---|---|---|---|---|
| Disabled | startup or first X11 connection (compositor-owned socket, `-listenfd`) | Running | `DISPLAY` exported | INV-XWAYLAND-1 |
| Running | satellite exits | Crashed | X11 clients die; their elements are destroyed through client death | INV-XWAYLAND-1, INV-STAGE-4 |
| Crashed | backoff elapsed | Running | **same X display** (OB-33) | INV-XWAYLAND-1 |
| Crashed | repeated crashes | Degraded | X11 disabled; logged | INV-FAIL-1 |

## M20 — Persistence restore
Object: OBJ-33, OBJ-09. Implements INV-OUTPUT-2, INV-FAIL-1. The file is validated all-or-nothing at startup and given to `Core::new` as a passive lookup. Cameras are applied only inside `output_connected` (M4/M5). Anchors are restored only through placement at initial commit (M1). There is no restore API.

## M21 — Backend and renderer recovery
Object: OBJ-29. Implements INV-FAIL-1, INV-SCALE-2.

| From | Trigger | To | Effects | Cites |
|---|---|---|---|---|
| Running | GPU reset / context lost | RendererLost | no frames (observed callbacks wait, bounded by recovery) | INV-FAIL-1 |
| RendererLost | recreate succeeds | Running | re-import buffers; full damage; damage trackers rebuilt with Scale(k_O) (F-RENDER-1); `D_max` re-queried → possible P7 | INV-FAIL-1, INV-SCALE-2 |
| RendererLost | recreate fails | Terminated | orderly shutdown | INV-FAIL-1 |
| Running | backend lost | Terminated | Foundation 01 rule | INV-FAIL-1 |

## M22 — Compositor interaction
Object: OBJ-26. Implements INV-INPUT-1, INV-INPUT-2, INV-INPUT-3, INV-SCALE-3.

| From | Trigger [guard] | To | Effects | Cites |
|---|---|---|---|---|
| Normal | binding press [client owns no button] | CameraPan / WindowMove / WindowResize | press consumed; epoch opens with the matching kind | INV-INPUT-2, INV-SCALE-3 |
| interaction | any press / motion | same | consumed | INV-INPUT-1, INV-INPUT-3 |
| interaction | initiator release | Normal | reconciliation; epoch settles | INV-INPUT-5, INV-SCALE-3 |
| interaction | cancel | Normal | no revert; owned buttons stay owned | INV-INPUT-1 |
| WindowMove | pointer crosses an Arrangement edge | — | **open decision OB-34** (F-DRAG-1); WindowMove/WindowResize product work is gated (INC-14) | INV-STAGE-1 |
<!-- /living -->
