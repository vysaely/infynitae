# Phase D checkpoint 1R — corrected state machines

These machines replace the corresponding D2 machines ([../D2_STATE_MACHINES.md](../D2_STATE_MACHINES.md), baseline `e8ec03a`). Machines not listed (M1, M3, M4, M12, M16, M18–M21) are unchanged, except for the cross-machine rules noted at the end. The normative wording is FOUNDATION_ARCHITECTURE §C12.R1. Every replaced transition names the finding that produced it.

## M2' xdg_toplevel requests (PD-F2-10, PD-F2-17 → INV-PRES-1, INV-PROTO-1)

| From | Trigger [guard] | To | Protocol effect |
|---|---|---|---|
| RoleCreated | `set_fullscreen` / `set_maximized` before the first commit | RoleCreated | recorded. Fullscreen: granted in the initial configure if the pre-map policy accepts (satellite path). Maximize: ignored. W-SM-13 override sends nothing. |
| Mapped | `set_maximized` | Mapped | **exactly one configure, without `maximized`**; no camera, no Stage change |
| Mapped | `unset_maximized` | Mapped | exactly one configure |
| Mapped | `set_minimized` | Mapped | ignored |
| Mapped | ActiveElementFocus gained / lost | Mapped | configure with / without `activated` (kick first if unobserved) |
| Mapped | `set_fullscreen` | M9' | exactly one configure (grant or decline) |
| Mapped | `unset_fullscreen` | M9' | exactly one configure |

Configure causes in Foundation 01:
- initial;
- fullscreen grant, decline or exit;
- maximize answer;
- `activated` change;
- suspension reason change (none in F01).

Never: camera, membership, visibility, scale. **Tests:** ER-3, OB-32.

## M5' Camera lifecycle and authority (PD-F2-01, PD-F2-11 → INV-CAMERA-1/5, INV-OUTPUT-1)

| From | Trigger | To | Effect |
|---|---|---|---|
| Bound | navigation via `physical_input` / `shell_command` (pan, jump, **FitViewport**, restore) | Bound | **one mutation per minted `Authorized` proof**; Views of all bound outputs, membership, scale epoch |
| Bound | any protocol event or client commit | Bound | **none** (no proof can be minted; ER-4 2d) |
| Bound | last bound output removed | Unbound | kept, persisted; an orphaned interaction on it **does not mutate it** |
| Unbound | same `OutputKey` reappears | Bound | view restored |

Removed transitions: "client commit of a fitted element → re-fit"; "animation tick". Animation is not in Foundation 01.

## M6' Camera pan (PD-F2-06, PD-F2-07 → INV-COORD-3, INV-SCALE-3)

| From | Trigger [guard] | To | Effect |
|---|---|---|---|
| Idle | binding press [no client-owned button] | Panning{output O, C_start, Σ_phys = 0, s_O, z_C} | press consumed; popup grabs dismissed (INV-INPUT-10 order); **ScaleInteractionEpoch opens** |
| Panning | motion `dp_phys` | Panning | `Σ_phys += dp_phys`; **`C = C_start − Q16(Σ_phys / (s_O·z_C))`**; membership; increases at rungs only |
| Panning | initiator release | Idle | epoch settles (one evaluation); batch-end reconciliation (M13') |
| Panning | the pan's output removed | Orphaned | motion consumed and **discarded**; epoch settles (removal is a settlement event) |
| Orphaned | initiator release | Idle | reconciliation on the relocated output |
| Panning | cancel | Idle | no revert; epoch settles |

## M7' ScaleInteractionEpoch (replaces M7 zoom interaction and the SC-D part of M15; PD-F2-07)

- **States:** `Closed` · `Open{kind ∈ {Continuous, Discrete, ShellStream}, direction ∈ {none, up, down}, deadline?}`
- **Writer:** core `ScaleState`.
- **INV:** SCALE-3, SCALE-4, IDLE-1.
- **Tier:** T0 (ER-5 R2 as property tests), T1/T3 (OB-23).

| From | Trigger | To | Effect |
|---|---|---|---|
| Closed | continuous gesture begin (pointer pan, touchpad, pinch) | Open{Continuous} | — |
| Closed | discrete camera step (wheel/key pan or zoom) or shell navigation command | Open{Discrete / ShellStream, deadline = now + T_settle} | evaluate (increase only) |
| Closed | atomic camera action (jump, one-shot fit, restore) | Closed | one-step epoch: evaluate once and settle |
| Open | further step of the same interaction | Open (deadline renewed for discrete/stream) | evaluate: increase at rung crossings in the epoch direction; **decrease suppressed** |
| Open | client response (commit changing logical size) | Open | increase only if direction ≠ down; never a reversal |
| Open | E's size pushes h above the device bound (P7) | Open{down} | immediate decrease |
| Open | pointer motion, key or button not bound to the camera | Open | **nothing** (does not end the epoch) |
| Open{Continuous} | gesture end | Closed | **settle**: one evaluation, h := target |
| Open{Discrete/ShellStream} | deadline reached (timer) / shell end-of-stream | Closed | settle |
| Open | output add/remove/configuration change | Closed | settle (settlement event) |
| Closed | client response | Closed | SC-E: only in the direction of the last stimulus |

- The timer is armed only while an epoch is Open (INV-IDLE-1).
- T_settle is provisional (candidate 300 ms). It must exceed the discrete-input repeat interval (ER-5: 40 ms gives 39 changes, ≥ 100 ms gives 1).

## M8' Viewport-fit (PD-F2-01 → INV-CAMERA-5)

| From | Trigger | To | Effect |
|---|---|---|---|
| — | `FitViewport(O, W)` via input or shell | — (no state) | snapshot W's geometry **now** → centre and zoom for O's viewport → one authorized mutation → record the previous camera value for an optional restore → done |
| — | `ViewportRestore(O)` via input or shell | — | one authorized mutation to the recorded value (if any) |
| — | W resizes, W commits, W unmaps | — | **nothing** (no mode exists) |

Removed: the sticky `Fitted` state, re-fit on resize, restore on unmap, and the isolation flag (deferred, Level D).

## M9' Protocol fullscreen (PD-F2-10 → INV-PRES-1)

| From | Trigger [guard] | To | Effect |
|---|---|---|---|
| Windowed | `set_fullscreen(o)` [W holds ActiveElementFocus, or before first map] | Fullscreen{O} | output choice now; configure `[fullscreen]` with O's size; **presentation starts now** (border fill until the client commits); other elements leave O; epoch settlement |
| Windowed | `set_fullscreen` [not focused] | Windowed | **configure without `fullscreen` now**; nothing stored |
| Fullscreen{O} | `unset_fullscreen` | Windowed | configure with the saved size; presentation ends now |
| Fullscreen{O} by W | another element's grant for O | Windowed (W) | W gets a configure leaving fullscreen with its saved size; O presents the new element |
| Fullscreen | W loses ActiveElementFocus | Fullscreen | unchanged |
| Fullscreen{O} | O removed | Windowed | configure with the saved size |
| Fullscreen | unmap | Windowed | state dropped (spec) |
| any | presentation start or end on the interaction output | same | M13' reconciliation trigger |

## M10'/M11' ActiveElementFocus and ProtocolSurfaceRoute (PD-F2-02 → INV-INPUT-8, INV-INPUT-10; ADR-14)

| Machine | States | Writer |
|---|---|---|
| **ActiveElementFocus** (core) | `None` / `Element(E)`, plus history and the attention set | FocusModel only |
| **ProtocolSurfaceRoute** (adapter) | `ToRoot(E)` / `Grab{chain: P1…Pn of E}` | the adapter's Smithay grab objects; subordinate |

| From | Trigger | To | Effect |
|---|---|---|---|
| AEF Element(E), Route ToRoot | client `xdg_popup.grab` [valid serial; root = E; W-SM-7 precondition] | Route Grab{P} | Smithay popup grabs installed (nested: keep the pointer grab, W-SM-14); **AEF unchanged** |
| Route Grab{…Pn} | client destroys or dismisses popups (grab ends) | Route ToRoot(E) | Smithay refocuses E's root; AEF unchanged (same element) |
| AEF Element(E), Route Grab | core focus change to E2 or None (explicit action, activation grant, target loss) | AEF E2/None, Route ToRoot(E2)/— | **ordered preemption in one batch:** `ungrab(All)` (`popup_done` topmost first; kick if unobserved) → keyboard `unset_grab` → pointer `unset_grab` → `set_focus(target)` → `activated` configures |
| AEF Element(E) | target loss (unmap/destroy) | successor | grab chains of E are dismissed or dead; successor `enter(keys)` + modifiers |
| any | camera, visibility, pointer motion | unchanged | — |

Pointer focus (M10) keeps its states. During a client-owned grab sequence the **delivered** focus is Smithay's, and core uses the delivered focus for every protocol-order decision (W-SM-4a).

## M13'/M14' Pointer constraint (lock and confine as one machine; PD-F2-02/03/04/05 → INV-COORD-5, INV-INPUT-5, INV-OUTPUT-1)

**Seat state:**
- `Physical{interaction output, p}`;
- `Constraint: None | Requested{S, kind} | Active{S, kind, lock point: SurfaceLocal}`;
- `ClientSequence: none | grab(surface)`;
- `CompositorInteraction: none | …`.

| From | Trigger [guard] | To | Effect |
|---|---|---|---|
| Requested | batch end [AEF = element(S) ∧ pointer focus S ∧ inside region ∧ no acquisition] | Active | `locked`/`confined` once |
| Active | camera change of a compositor interaction | Active | nothing (deferred) |
| Active | client-owned grab sequence active + any trigger | Active | **deferred**; core = Smithay = Active; no absolute motion to a locked surface; relative motion from physical Δ only |
| Active | batch end with a trigger (view change, geometry change, output set change, AEF change, end of an interaction or client sequence) [lock point observable through the interaction output ∧ AEF = element(S)] | Active | nothing |
| Active(lock) | batch end, trigger, [not observable ∨ AEF ≠ element(S)] | Inactive / destroyed (oneshot) | **exactly one `unlocked`**: explicit `deactivate` only if the **delivered** focus does not change in the batch; otherwise via Smithay `leave()` (W-SM-4a) |
| Active(confine) | motion | Active | clamp half-open to the region's projection in `OutputLogical(interaction output)`; **no Arrangement crossing** |
| Active(confine) | batch end, trigger, [pointer outside the region] | Inactive | one `unconfined`; no warp |
| Active | interaction output removed | Active (deferred) or reconciled | the physical pointer is relocated **at once** (Arrangement-nearest, half-open); the constraint follows the rows above |
| Active | S destroyed / unmapped | — | protocol path ends it; no core event synthesised |

ER-2 shows this machine's DEFER path against the sealed path (divergence) and the explicit path (duplicate `unlocked`).

## M15' Scale hint (PD-F2-07/08/09 → INV-SCALE-2/3/4/5)

| From | Trigger | Effect |
|---|---|---|
| — (before the initial configure) | placement done | **h := t(ρ_pred)** (INV-SCALE-5); emitted before the initial configure |
| any | evaluation | per M7'; the target is computed on the ladder in force: output scales ≤ H_max(E), H_max over all surfaces |
| any | ladder change (hotplug) | a held or deferred h may be off-ladder until the next permitted evaluation; every **emitted** h is a rung |

## M17' Suspension (PD-F2-14 → INV-FRAME-6)

`reasons ⊆ {SessionLocked, OutputsOff, Parked}`. Every reason is independent of camera membership; none is producible in Foundation 01.

## M22' Compositor interaction

- `CameraPan` opens a Continuous ScaleInteractionEpoch.
- Animated navigation is not a Foundation 01 interaction (PD-F2-01).
- `WindowMove` across outputs: the formula decision is OB-34.
- `WindowMove` of a locked element: a constraint reconciliation trigger (geometry change), deferred until the move ends.

## Cross-machine rules added in 1R

1. **One batch, one order:**
   - state updates;
   - focus preemption (INV-INPUT-10);
   - constraint reconciliation (INV-INPUT-5);
   - scale evaluation or settlement (INV-SCALE-3/5);
   - effects emitted.
2. **Kick precedence:** `popup_done`, maximize/fullscreen answers and `activated` configures to an unobserved client are kick triggers (INV-FRAME-3 unchanged).
3. **Timers armed only with an obligation:** the ScaleInteractionEpoch deadline is the only new timer (INV-IDLE-1).
