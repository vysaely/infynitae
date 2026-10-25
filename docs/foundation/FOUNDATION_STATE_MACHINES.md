# State machines

The lifecycles Core runs (`M*`): toplevels, outputs, pointer, focus, fullscreen, interactions and
more. Every transition cites the invariant ([FOUNDATION_INVARIANTS.md](FOUNDATION_INVARIANTS.md))
or ADR ([FOUNDATION_DECISIONS.md](FOUNDATION_DECISIONS.md)) it implements; objects are `OBJ-*`
([FOUNDATION_OBJECT_MODEL.md](FOUNDATION_OBJECT_MODEL.md)), formulas `F-*`
([FOUNDATION_ARCHITECTURE.md](FOUNDATION_ARCHITECTURE.md) §F).

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
| AwaitingInitialCommit | commit without buffer | AwaitingFirstBuffer | provisional placement via `stage::place` (a restored or explicit anchor is final); initial hint `t(ρ_pred)` (F-SCALE-3); initial configure (pre-map fullscreen granted in it, INV-PRES-1) | INV-STAGE-1, INV-SCALE-5 |
| AwaitingInitialCommit | commit with buffer | client error | `ensure_configured` → protocol error → client death (W-SM-6) | INV-STAGE-4 |
| AwaitingFirstBuffer | buffer commit after ack | Mapped | **first windowed presentation only (INV-STAGE-1), and only for a provisional anchor; a first map in protocol fullscreen postpones it:** default placement via `stage::place` (F-PLACE-1); z-top; membership (F-MEMBER-1); epoch settlement | INV-STAGE-1, INV-STAGE-4, INV-RENDER-3 |
| Mapped | commit with buffer | Mapped | snapshot replaced (OBJ-05); membership if geometry changed; P7 check; default placement via `stage::place` (F-PLACE-1) if the element was never placed and this commit is its first windowed presentation (INV-STAGE-1) | INV-COORD-6, INV-SCALE-2, INV-STAGE-1 |
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
Object: OBJ-10, OBJ-07, OBJ-08, OBJ-20. Implements INV-OUTPUT-1, INV-OUTPUT-2, INV-CAMERA-3.

| From | Trigger [guard] | To | Effects | Cites |
|---|---|---|---|---|
| Absent | `output_connected(&mut OutputLifecycleToken, descriptor)` (startup and hotplug alike) [descriptor inside the representable domain (F-VIEW-1)] | Enabled | global created; camera bound by precedence remembered-and-unbound → persisted (no collision) → default (M5); Views (`view_O`), lattice reference set (F-LATTICE-1), damage tracker (`phys_O`), membership, ladder; epoch settlement; if the seat is `NoOutput`: zero-output grab removed, pointer placed at the centre of the new output (startup rule), one routed motion, reconciliation (M13) | INV-OUTPUT-2, INV-CAMERA-3, INV-SCALE-3, INV-OUTPUT-1 |
| Absent | `output_connected` [descriptor outside the representable domain (F-VIEW-1: mode sides 1 … 2¹⁵, scale 0.25 … 8): an empty mode (a side of 0), a side above 2¹⁵, or a scale outside 30 … 960 in 1/120 units, a zero scale included; transform and refresh are not constrained] | Absent | rejected (`InvalidDescriptor`), nothing mutated, no `OutputId` allocated (C12.F14) | INV-OUTPUT-2 |
| Enabled | `output_reconfigured(&mut OutputLifecycleToken, id, descriptor)` [known id ∧ same `OutputKey` ∧ descriptor inside the representable domain (F-VIEW-1)]: mode, transform, scale or refresh change | Enabled (same `OutputId`) | View (`view_O`), ProjectionFrame and damage tracker (`phys_O`) rebuilt (F-VIEW-1, F-RENDER-1); lattice reference set again if `k_O` or `view_O` changed (F-LATTICE-1); membership; pointer clamp (F-RECONF-1); `wl_output`/`xdg_output` events; one configure only for an element in protocol fullscreen on this output; epoch settlement; reconciliation trigger (M13); no camera value change; no new `OutputId` | INV-OUTPUT-2, INV-INPUT-5, INV-SCALE-3 |
| Enabled | `output_reconfigured` [unknown id ∨ changed `OutputKey`] | unchanged | rejected, nothing mutated (an identity change is a disconnect plus a connect) | INV-OUTPUT-2 |
| Enabled | `output_reconfigured` [known id ∧ same `OutputKey` ∧ descriptor outside the representable domain (F-VIEW-1), as for `output_connected`] | unchanged | rejected (`InvalidDescriptor`), nothing mutated (C12.F14); an unknown id or changed key is rejected by the row above first | INV-OUTPUT-2 |
| Enabled | `output_disconnected(&mut OutputLifecycleToken, id)` [another output survives] | Removed | leave before the global is destroyed; camera unbound and kept; lattice reference dropped (F-LATTICE-1); fullscreen dropped with its configure; interactions orphaned without camera mutation; pointer relocated (F-RELOCATE-1); constraints reconciled (deferred under a client grab); settlement | INV-OUTPUT-1 |
| Enabled | `output_disconnected(&mut OutputLifecycleToken, id)` [last output] | Removed; seat `NoOutput` | as above, but no relocation: zero-output grab installed with `Focus::Clear` (client: `wl_pointer.leave`; active constraints end once), one `motion(None)` delivered to no one; physical pointer input consumed until the next connect | INV-OUTPUT-1, INV-INPUT-5 |
| any | protocol event or caller-supplied coordinates or zoom | — | impossible: no such entry, field or parameter | INV-OUTPUT-2 |

## M5 — Camera lifecycle
Object: OBJ-07, OBJ-09, OBJ-30. Implements INV-CAMERA-1, INV-CAMERA-3, INV-CAMERA-5.

| From | Trigger | To | Effects | Cites |
|---|---|---|---|---|
| — | `output_connected`, no **unbound** remembered camera for the key | Bound | new camera seeded from the persisted lookup (only if no camera with that key exists this session) or the internal default; one proof | INV-CAMERA-3, INV-OUTPUT-2 |
| Unbound | `output_connected`, same `OutputKey` [lowest unbound camera with that key] | Bound | re-bound under the new `OutputId`, value unchanged, navigation history empty (C12.F16); the output's lattice reference is set from the re-bound camera's view (F-LATTICE-1) | INV-CAMERA-3 |
| Bound | `output_connected`, same `OutputKey` (concurrent key collision) | Bound (unchanged) | never re-bound: binding is 1:1; the new output gets its own camera (row 1) | INV-CAMERA-3 |
| Bound | `output_reconfigured` of its output | Bound | nothing (no proof minted); the navigation history is kept (same `OutputId`) | INV-OUTPUT-2, INV-CAMERA-5 |
| Bound | its output disconnected | Unbound | value kept; navigation history emptied with the unbinding's proof (OBJ-30, C12.F16) | INV-OUTPUT-1, INV-CAMERA-5 |
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
| any | **persistent zoom ≠ 1** | — | **decided by C12.F14** (OB-24 D3/D4: the output's lattice reference is kept while panning, F-LATTICE-1); implemented by INC-10 (`b46021f`, 2026-10-18): a pan at any zoom keeps the reference | INV-COORD-3 |

## M7 — ScaleInteractionEpoch
Object: OBJ-16. Implements INV-SCALE-3, INV-SCALE-4, INV-IDLE-1. Kinds: CameraPan, CameraZoom, WindowMove, WindowResize, ShellStream.

| From | Trigger | To | Effects | Cites |
|---|---|---|---|---|
| Closed | continuous interaction begins | Open{continuous} | — | INV-SCALE-3 |
| Closed | discrete spatial step or shell stream command | Open{discrete, deadline = now + T_settle (PROV-5)}, `now` = the session's monotonic clock when the step enters Core (C12.F4) | evaluate (increase only) | INV-SCALE-3 |
| Closed | atomic camera or Stage action | Closed | evaluate once and settle | INV-SCALE-3 |
| Open | further step | Open | increases at rung crossings in the epoch direction; decreases suppressed; a discrete step moves the deadline to its own `now + T_settle`, never earlier | INV-SCALE-3, INV-SCALE-4 |
| Open | client response | Open | no reversal of direction; the deadline is not touched (a response is not a step) | INV-SCALE-3, INV-SCALE-4 |
| Open | `h > H_max(E)` (device bound) | Open{down} | immediate decrease (P7); never used for ordinary decreases | INV-SCALE-2, INV-SCALE-3 |
| Open | pointer motion / unrelated key or button | Open | nothing | INV-SCALE-3 |
| Open | gesture end / release / end-of-stream / deadline / output settlement event | Closed | settle once to `t(ρ)` (F-SCALE-3) | INV-SCALE-3, INV-SCALE-5 |

## M8 — Viewport-fit (one-shot)
Object: OBJ-07, OBJ-30. Implements INV-CAMERA-5, INV-CAMERA-1.

| From | Trigger | To | Effects | Cites |
|---|---|---|---|---|
| — | key F pressed with Logo held, Ctrl and Alt released, Shift released, no compositor interaction active, the pointer on O | — | the binding is consumed (M12); W = the mapped element hit under the pointer on O's current ProjectionFrame (F-HIT-1): `FitViewport(O, W)`; no element hit: nothing — no mutation, no history change, no pointer event | INV-CAMERA-5 |
| — | key F pressed with Logo and Shift held, Ctrl and Alt released, no compositor interaction active, the pointer on O | — | the binding is consumed (M12): `ViewportRestore(O)` | INV-CAMERA-5 |
| — | `FitViewport(O, W)` via input or shell [O connected ∧ O presents no protocol-fullscreen element ∧ W mapped ∧ g.w > 0 ∧ g.h > 0] | — | snapshot W now; F-FIT-1: zoom clamped into F-VIEW-1's domain, centre into INV-COORD-1; the previous value added as the newest history entry, the oldest evicted first if 256 are held (OBJ-30, C12.F16); one mutation; atomic epoch; one pointer reconciliation (M10) | INV-CAMERA-5 |
| — | `FitViewport(O, W)` [otherwise] | — | refused: no mutation, nothing recorded or evicted, no pointer reconciliation (C12.F15, C12.F16, C12.F17) | INV-CAMERA-5 |
| — | `ViewportRestore(O)` via input or shell [O connected ∧ O presents no protocol-fullscreen element ∧ O's history non-empty] | — | one mutation to the newest entry's exact value, which is removed (OBJ-30); one pointer reconciliation (M10) | INV-CAMERA-5 |
| — | `ViewportRestore(O)` [O not connected ∨ O presents a protocol-fullscreen element ∨ O's history empty] | — | refused: no mutation, history unchanged, no pointer reconciliation (C12.F15, C12.F16, C12.F17) | INV-CAMERA-5 |
| — | W resizes / commits / unmaps | — | nothing (no mode exists) | INV-CAMERA-1 |

## M9 — Protocol fullscreen
Object: OBJ-19. Implements INV-PRES-1.

| From | Trigger [guard] | To | Effects | Cites |
|---|---|---|---|---|
| Windowed | `set_fullscreen(o)` [ActiveElementFocus = W, or pre-map] | Fullscreen{O} | output choice; configure `[fullscreen]`; if W is mapped: older grants on O end, presentation starts (F-PRESENT-1), other elements leave O, reconciliation trigger, settlement; if W is not mapped yet: the grant waits for W's map (INV-PRES-1) | INV-PRES-1 |
| Fullscreen{O}, waiting (W not mapped) | W's first map | Fullscreen{O} | grants on O older than W's end (each element gets a configure with its saved size, if available); presentation starts (F-PRESENT-1); other elements leave O; reconciliation trigger; settlement | INV-PRES-1 |
| Fullscreen{O}, waiting | W destroyed before its map | — | the grant is dropped; nothing else changes | INV-PRES-1 |
| Windowed | `set_fullscreen` [not focused] | Windowed | configure without `fullscreen`; nothing stored | INV-PRES-1 |
| Fullscreen{O} | `unset_fullscreen` | Windowed | configure with the saved size, if available; presentation ends, if it had started | INV-PRES-1 |
| Fullscreen{O} | unmap (M1) | Windowed (unmapped) | state dropped (spec): no configure and no saved size; presentation ends, if it had started; a target-loss configure (M11), if W held ActiveElementFocus, carries neither `fullscreen` nor a size; the next initial configure (M1) is not fullscreen | INV-PRES-1, INV-PROTO-1 |
| Fullscreen{O} by W | a newer grant for O whose element is mapped, at the grant or at that element's map | Windowed (W) | W gets a configure with its saved size, if available; a newer grant still waiting leaves W unchanged | INV-PRES-1 |
| Fullscreen | W loses ActiveElementFocus | Fullscreen | unchanged | INV-PRES-1 |
| Fullscreen{O} | O disconnected | Windowed | configure with the saved size, if available | INV-OUTPUT-1 |

## M10 — Pointer focus
Object: OBJ-20, OBJ-24, OBJ-15. Implements INV-HIT-1, INV-INPUT-3, INV-INPUT-5, INV-OUTPUT-1.

| From | Trigger | To | Effects | Cites |
|---|---|---|---|---|
| any | client-routed motion | per F-HIT-1 | enter/leave/motion | INV-HIT-1 |
| Focused | compositor interaction acquired | Kept | no events (KeepFocus) | INV-INPUT-3 |
| Kept | interaction ends | per re-hit | exactly one true-position event, none while the reconciliation keeps a lock (C12.F17) | INV-INPUT-5, INV-CAMERA-4 |
| Focused | client button press | ClientGrab | Smithay decides the delivered focus; core uses the delivered focus for protocol-order decisions | INV-INPUT-5 |
| any | view change, geometry change, output change, ActiveElementFocus change | per batch-end reconciliation | at most one true-position event | INV-INPUT-5 |
| any | last output removed | `NoOutput` (no focus) | zero-output grab with `Focus::Clear`: `wl_pointer.leave`, no motion to the old focus | INV-OUTPUT-1 |
| `NoOutput` | physical motion, button, axis | `NoOutput` | consumed; delivered nowhere; no relative motion | INV-OUTPUT-1 |
| `NoOutput` | first output connected | per F-HIT-1 | grab removed; startup placement; one routed motion (enter) | INV-OUTPUT-1 |
| Focused | interaction output reconfigured | per batch-end reconciliation | pointer clamped (F-RECONF-1); at most one true-position event, none to a kept lock (C12.F17) | INV-OUTPUT-2, INV-INPUT-5 |
| Focused | output removed, pointer relocated to a surviving output (F-RELOCATE-1) | per batch-end reconciliation | exactly one true-position event unless an interaction owns the pointer or the reconciliation keeps a lock (C12.F17) | INV-OUTPUT-1, INV-INPUT-5 |

## M11 — ActiveElementFocus and ProtocolSurfaceRoute
Object: OBJ-22, OBJ-23. Implements INV-INPUT-8, INV-INPUT-10, INV-PROTO-1. ADR-14.

| From | Trigger | To | Effects | Cites |
|---|---|---|---|---|
| any | click-to-focus, focus binding, shell focus, activation grant [E not hidden by a presentation, INV-INPUT-8] | Element(E) | M3 preemption if a grab is active; `leave`/`enter(keys)`; `activated` configures | INV-INPUT-8, INV-INPUT-10, INV-PROTO-1 |
| any | the same actions [E hidden by a presentation] | unchanged | declined; no effect | INV-INPUT-8 |
| Element(E), route ToRoot | valid popup grab in E | Element(E), route Grab | ActiveElementFocus unchanged | INV-INPUT-10 |
| Element(E), route Grab | grab ends (client) | Element(E), route ToRoot | Smithay root restore inside E | INV-INPUT-10 |
| Element(E) | target loss | successor, chosen among the elements not hidden by a presentation | `enter(keys)` without `leave` after client-side destruction | INV-INPUT-7, INV-INPUT-8 |
| any | camera, visibility, pointer motion, fullscreen request or grant | unchanged (a focused element that becomes hidden keeps the focus) | — | INV-INPUT-8 |

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
| Active (lock) | batch end [otherwise] | Inactive / destroyed (oneshot) | exactly one `unlocked`; explicit deactivate only if the delivered focus does not change (W-SM-4a); no later event (P1, W-SM-4c) | INV-INPUT-5 |
| Active (confine) | motion | Active | clamp per F-CONFINE-1; no Arrangement crossing | INV-INPUT-5 |
| Active (confine) | batch end [pointer outside the region] | Inactive | one `unconfined`; no warp | INV-INPUT-5 |
| Active | interaction output disconnected | Active (deferred) or reconciled | pointer relocated at once (F-RELOCATE-1) | INV-OUTPUT-1 |
| Active | S destroyed/unmapped | — | ended through the protocol path | INV-INPUT-5 |
| Requested / Inactive (persistent) | pointer leaves S, S destroyed, later batches | unchanged | **no event**: deactivation only from Active (requires P1 or an upstream fix, OB-11) | INV-INPUT-5 |
| Active | last output removed | Inactive / destroyed (oneshot) | one `unlocked`/`unconfined` through the zero-output grab's focus clear; reactivation only after reconnect and re-entry | INV-OUTPUT-1, INV-INPUT-5 |
| Active | interaction output reconfigured | per batch-end reconciliation | lock kept iff its lock point is still observable; confinement re-clamped or ended | INV-INPUT-5, INV-OUTPUT-2 |

## M14 — Confinement clamp
Part of M13 (the clamp rows). It keeps its identifier for continuity with audit/D2. Implements INV-INPUT-5 via F-CONFINE-1.

## M15 — Scale hint
Object: OBJ-16. Implements INV-SCALE-1, INV-SCALE-2, INV-SCALE-5.

| From | Trigger | To | Effects | Cites |
|---|---|---|---|---|
| — | placement done (before the initial configure) | h = `t(ρ_pred)` | emitted before the initial configure | INV-SCALE-5 |
| any | batch-end evaluation | per M7 | target on the ladder in force (F-SCALE-2, F-SCALE-3); integer emission `b(h)` bounded by the device; emission only on change; unobserved elements hold | INV-SCALE-2, INV-SCALE-5 |
| any | ladder change (hotplug) | held value may be off-ladder | every emitted value is a rung | INV-SCALE-2 |
| — | initial commit with no buffer (`s_max(E) = 0`) | `H_max = H_cfg` | no division: `H_dev = +∞` by the domain of F-SCALE-2 | INV-SCALE-2 |
| any | first buffer attached (`s_max` becomes > 0) | per P7 | immediate decrease if `h > H_max(E)` | INV-SCALE-2 |

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
Object: OBJ-33, OBJ-09. Implements INV-OUTPUT-2, INV-FAIL-1. The file is validated all-or-nothing at startup and given to `Core::new` as a passive lookup; a zoom outside the representable domain (F-VIEW-1) is an invalid entry, and a rejected file yields the empty/default state (C12.F14, D2). Cameras are applied only inside `output_connected` (M4/M5). Anchors are restored only through placement at initial commit (M1). There is no restore API. `OutputKey` is a persistence identity (INV-OUTPUT-2); on a concurrent key collision the persisted entry stays with the first holder and the duplicate gets a default camera (INV-CAMERA-3). Shipping persisted camera state across sessions is gated by G-OB48 (OB-48).

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
