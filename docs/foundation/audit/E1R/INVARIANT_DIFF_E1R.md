# E1R — Invariant diff (per-invariant provenance)

Status: **UNSEALED** (E1R review gate). Generated from the layer rows in FOUNDATION_ARCHITECTURE.md: "previous" is the latest sealed layer row, "new" is the §C12.E1R row, both verbatim.

Registry size: 57 before, 57 after. Amended: 6. Added: 0. Removed: 0.

## INV-CAMERA-3

- **Previous effective layer:** C12.R2 @ 5827926
- **Triggering finding(s):** E-F2-01, E-F2-04, E-F3-03
- **Evidence:** EE-1, EE-4
- **Affected formulas:** F-VIEW-1 (extracted text changed)
- **Affected state machines:** M4, M5, M20
- **Affected objects:** OBJ-07, OBJ-09, OBJ-33
- **Affected ADRs:** ADR-04
- **Affected obligations:** OB-48, OB-49 (new)
- **Affected increments:** INC-2; gates G-OB48, G-MIRROR

**Previous wording (verbatim):**

> Cameras are centre-anchored views (centre Q16, zoom); per-output view origin `v_O = centre − mode/(2k)` (R7).<br><br>Cameras come into existence **only** through `output_connected` (INV-OUTPUT-2). For the connecting `OutputKey`, the camera is the first of:<br>1. the camera remembered this session (re-bound, value unchanged);<br>2. a new camera seeded from the passive persisted lookup;<br>3. a new camera at Core's internal default placement (the placement policy itself is Level D).<br><br>Cameras survive output removal (unbound, value kept) and are not destroyed in-session. N outputs may share one camera.

**New wording (verbatim, C12.E1R):**

> Cameras are centre-anchored views (centre Q16, zoom); per-output view origin `v_O = centre − view_O/(2k)`, where `view_O` is the output's physical mode after its transform (R7).<br><br>Cameras come into existence **only** through `output_connected` (INV-OUTPUT-2). For the connecting `OutputKey`, the camera is the first of:<br>1. the lowest-indexed camera remembered this session for that key that is **currently unbound** (re-bound, value unchanged);<br>2. a new camera seeded from the passive persisted lookup, unless a camera with that key already exists this session (a concurrent key collision), in which case 3 applies;<br>3. a new camera at Core's internal default placement (the placement policy itself is Level D).<br><br>Cameras survive output removal (unbound, value kept) and are not destroyed in-session. **Binding is 1:1 in Foundation 01:** each connected output is bound to exactly one camera, and a camera is bound to at most one output. Shared cameras (mirroring) are not part of Foundation 01 (gate G-MIRROR).

## INV-INPUT-5

- **Previous effective layer:** C12.R1 @ 84ce620
- **Triggering finding(s):** E-F2-02, E-F2-03, E-F2-05
- **Evidence:** EE-2, EE-3, EE-7
- **Affected formulas:** F-RELOCATE-1 (domain), F-RECONF-1 (new); F-CONFINE-1, F-LOCK-1 unchanged
- **Affected state machines:** M10, M13
- **Affected objects:** OBJ-20, OBJ-21
- **Affected ADRs:** ADR-07, ADR-13
- **Affected obligations:** OB-11 (amended), OB-47, OB-50, OB-51 (new)
- **Affected increments:** INC-5 (decision now before any constraint ships)

**Previous wording (verbatim):**

> A constraint is activated only when its element holds ActiveElementFocus, its surface has pointer focus, the pointer is inside the region, and no compositor pointer interaction is being acquired.<br><br>**Reconciliation** runs once at the end of every core batch in which any of these changed: the interaction output's view; the constrained surface's geometry or origin; the output set; ActiveElementFocus; the end of a compositor interaction; the end of a client-owned grab sequence.<br><br>Reconciliation is **deferred** while a compositor pointer interaction or a client-owned grab sequence is active. While deferred, core and Smithay keep the same constraint state, no absolute motion reaches a locked surface, and relative motion comes only from physical deltas.<br><br>A lock is kept only if its derived lock point is observable through the interaction output and its element holds ActiveElementFocus.<br><br>A confinement clamps the physical pointer half-open, in the interaction output's OutputLogical space, to the region's projection; Arrangement crossing is disabled; a pointer outside the region after a view change ends it; there is no warp.<br><br>Every end emits `unlocked`/`unconfined` exactly once. An explicit `deactivate` is used only when the delivered focus does not change in that batch; otherwise Smithay's `leave()` does it (W-SM-4a).<br><br>At most one true-position motion per batch; no replayed or synthetic motion. Output removal relocates the physical pointer at once (Arrangement-nearest, half-open). Destruction ends constraints through the protocol path.

**New wording (verbatim, C12.E1R):**

> A constraint is activated only when its element holds ActiveElementFocus, its surface has pointer focus, the pointer is inside the region, and no compositor pointer interaction is being acquired.<br><br>**Reconciliation** runs once at the end of every core batch in which any of these changed: the interaction output's view or descriptor (including `output_reconfigured`); the constrained surface's geometry or origin; the output set; ActiveElementFocus; the end of a compositor interaction; the end of a client-owned grab sequence.<br><br>Reconciliation is **deferred** while a compositor pointer interaction or a client-owned grab sequence is active. While deferred, core and Smithay keep the same constraint state, no absolute motion reaches a locked surface, and relative motion comes only from physical deltas.<br><br>A lock is kept only if its derived lock point is observable through the interaction output and its element holds ActiveElementFocus.<br><br>A confinement clamps the physical pointer half-open, in the interaction output's OutputLogical space, to the region's projection; Arrangement crossing is disabled; a pointer outside the region after a view change ends it; there is no warp.<br><br>Every end emits `unlocked`/`unconfined` exactly once, and **only from the active state**: no deactivation event is ever sent for an inactive constraint — not on a later `leave()` after an explicit deactivate (any lifetime, any later batch), not on surface destruction, and not for a constraint that was never activated. An explicit `deactivate` is used only when the delivered focus does not change in that batch; otherwise Smithay's `leave()` does it (W-SM-4a). Smithay 0.7.0's `leave()` deactivates unconditionally and no public API avoids it (W-SM-4c), so this invariant depends on the one-line dependency patch P1 (`leave()` deactivates only an active constraint) or an equivalent upstream fix, adopted under the patch policy (INV-ARCH-3, OB-11).<br><br>At most one true-position motion per batch; no replayed or synthetic motion. Output removal relocates the physical pointer at once (F-RELOCATE-1) or, when no output survives, enters `NoOutput` (INV-OUTPUT-1). Destruction ends constraints through the protocol path.

## INV-OUTPUT-1

- **Previous effective layer:** C12.R2 @ 5827926
- **Triggering finding(s):** E-F2-05
- **Evidence:** EE-3, EE-4
- **Affected formulas:** F-RELOCATE-1 (domain)
- **Affected state machines:** M4, M10, M13
- **Affected objects:** OBJ-20
- **Affected ADRs:** ADR-07, ADR-04
- **Affected obligations:** OB-50 (new)
- **Affected increments:** INC-2 (core), INC-3 (pointer), INC-5 (constraints)

**Previous wording (verbatim):**

> Output removal enters Core **only** through `output_disconnected(&mut OutputLifecycleToken, …)`. Then: membership `leave` before the global is destroyed; camera unbound and kept (value unchanged); fullscreen presentation dropped with the INV-PRES-1 configure; interactions orphaned (ownership kept until release, no camera mutation); physical pointer relocated at once to the Arrangement-nearest point of a surviving output (half-open); constraints reconciled per INV-INPUT-5 (deferred under a client grab); the removal is a ScaleInteractionEpoch settlement event.

**New wording (verbatim, C12.E1R):**

> Output removal enters Core **only** through `output_disconnected(&mut OutputLifecycleToken, …)`. Then: membership `leave` before the global is destroyed; camera unbound and kept (value unchanged); fullscreen presentation dropped with the INV-PRES-1 configure; interactions orphaned (ownership kept until release, no camera mutation); if the interaction output was removed and **another output survives**, the physical pointer is relocated at once to the Arrangement-nearest point of a surviving output (half-open, F-RELOCATE-1); constraints reconciled per INV-INPUT-5 (deferred under a client grab); the removal is a ScaleInteractionEpoch settlement event.<br><br>**Zero outputs.** When no output survives, the seat enters the stable `NoOutput` state (OBJ-20); no output and no coordinate is fabricated, and nearest-output selection is never evaluated on an empty set. In the same batch the adapter installs a compositor zero-output pointer grab with `Focus::Clear`, replacing any client implicit grab: the client receives `wl_pointer.leave`, Smithay ends only *active* constraints (one `unlocked`/`unconfined` each), and no absolute motion is delivered. It then submits one `motion(focus = None)` at Smithay's stored location, delivered to no one. While `NoOutput`, physical pointer motion, buttons and axes are consumed by core and delivered nowhere; ActiveElementFocus and keyboard routing are unchanged (not output-bound); cameras stay unbound with their values. The first `output_connected` leaves `NoOutput`: the zero-output grab is removed first, then the pointer is placed by the startup rule (centre of the new output's logical extent) with one routed motion, and constraint reconciliation follows (INV-INPUT-5).

## INV-OUTPUT-2

- **Previous effective layer:** C12.R2 @ 5827926
- **Triggering finding(s):** E-F2-01, E-F2-03, E-F3-03
- **Evidence:** EE-4
- **Affected formulas:** F-RECONF-1 (new), F-VIEW-1, F-RENDER-1
- **Affected state machines:** M4, M5, M20
- **Affected objects:** OBJ-08, OBJ-10, OBJ-33
- **Affected ADRs:** ADR-04, ADR-08
- **Affected obligations:** OB-19 (probes 2h/2i added), OB-48, OB-49 (new)
- **Affected increments:** INC-2

**Previous wording (verbatim):**

> **Output lifecycle authority.** Output connect and disconnect, at startup and at runtime hotplug alike, enter Core only through `output_connected(&mut OutputLifecycleToken, OutputDescriptor)` and `output_disconnected(&mut OutputLifecycleToken, OutputId)`.<br><br>**The token** is a dedicated authority distinct from `InputPort`: minted only by `Core::new`; private fields (unforgeable); not Clone; `!Send` (never in protocol state); bound to its Core; held exclusively by the backend's output/display event source.<br><br>**`OutputDescriptor`** carries hardware facts only (`OutputKey`, mode, scale, refresh), with no field for camera position, centre, zoom or binding.<br><br>**Persisted camera state** is a passive read-only lookup table given to `Core::new` and consulted only inside `output_connected`. No public restore API exists.

**New wording (verbatim, C12.E1R):**

> **Output lifecycle authority.** Output connect, reconfiguration and disconnect, at startup and at runtime alike, enter Core only through `output_connected(&mut OutputLifecycleToken, OutputDescriptor)`, `output_reconfigured(&mut OutputLifecycleToken, OutputId, OutputDescriptor)` and `output_disconnected(&mut OutputLifecycleToken, OutputId)`.<br><br>**The token** is a dedicated authority distinct from `InputPort`: minted only by `Core::new`; private fields (unforgeable); not Clone; `!Send` (never in protocol state); bound to its Core; held exclusively by the backend's output/display event source.<br><br>**`OutputDescriptor`** carries hardware facts only (`OutputKey`, physical mode before the transform, transform, scale, refresh), with no field for camera position, centre, zoom or binding.<br><br>**Reconfiguration** changes the hardware facts of a connected output and is not a reconnect. The `OutputId` is kept and no id is allocated; an unknown id is rejected, and so is a changed `OutputKey` (an identity change is a disconnect plus a connect). It mints no `Authorized` proof, so camera identity and value are unchanged. Derived state (`view_O`, ProjectionFrame, damage tracker, membership) is rebuilt from the new descriptor; the physical pointer on that output is clamped into its new extent (F-RECONF-1); the change is a ScaleInteractionEpoch settlement event and an INV-INPUT-5 reconciliation trigger. Client-visible effects are the `wl_output`/`xdg_output` property events and, only for an element in protocol fullscreen on that output, one configure with the new size.<br><br>**`OutputKey`** is a persistence identity (DRM device, connector, EDID make/model/serial when present). It is distinct from the runtime `OutputId` (never reused) and is not a physical-monitor identity; collisions follow INV-CAMERA-3.<br><br>**Persisted camera state** is a passive read-only lookup table given to `Core::new` and consulted only inside `output_connected`. No public restore API exists.

## INV-SCALE-2

- **Previous effective layer:** C12.R1 @ 84ce620
- **Triggering finding(s):** E-F2-06
- **Evidence:** EE-5
- **Affected formulas:** F-SCALE-2 (domain)
- **Affected state machines:** M15
- **Affected objects:** OBJ-16, OBJ-29
- **Affected ADRs:** ADR-05
- **Affected obligations:** OB-13 (amended)
- **Affected increments:** INC-6

**Previous wording (verbatim):**

> Every emitted hint is a rung of the ladder in force at emission. The ladder contains every connected output scale ≤ H_max(E), the integers, and the geometric fill. `H_max(E) = min(H_cfg, D_max / s_max(E))`, where `s_max` is the largest logical side over all surfaces of E's tree. `D_max` is queried from the renderer and re-queried on renderer recreation. If E's size makes h exceed the bound, h is lowered immediately (P7). A held or epoch-deferred hint may be off-ladder until the next permitted evaluation.

**New wording (verbatim, C12.E1R):**

> Every emitted hint is a rung of the ladder in force at emission. The ladder contains every connected output scale ≤ H_max(E), the integers, and the geometric fill. `H_max(E) = min(H_cfg, H_dev(E))`, where the device bound `H_dev(E)` is the supremum of the hints whose buffers fit the device: `D_max / s_max(E)` when `s_max(E) > 0` and `+∞` when `s_max(E) = 0`; `s_max` is the largest logical side over all surfaces of E's tree and is 0 exactly when no surface has a non-zero extent (every initial commit). With no buffer there is nothing to bound, so `H_max(E) = H_cfg`. `D_max` is queried from the renderer and re-queried on renderer recreation. If E's size makes h exceed the bound, h is lowered immediately (P7). A held or epoch-deferred hint may be off-ladder until the next permitted evaluation.

## INV-SCALE-5

- **Previous effective layer:** C12.R1 @ 84ce620
- **Triggering finding(s):** E1R-N3
- **Evidence:** EE-5
- **Affected formulas:** F-SCALE-3 (integer emission)
- **Affected state machines:** M15
- **Affected objects:** OBJ-16
- **Affected ADRs:** ADR-05
- **Affected obligations:** OB-13 (amended)
- **Affected increments:** INC-6

**Previous wording (verbatim):**

> At zoom 1 with no zoom interaction, and where the maximum observing output scale ≤ H_max(E), the policy equals Phase B MaxGesture. Integer hint = ⌈h⌉; emission only on change; unobserved elements hold h; evaluation once per core batch after all its updates. **Initial hint:** `h := t(ρ_pred)` over the presentations whose view intersects the placed window-geometry rect (or the anchor point when the size is 0×0); if none, `t(max connected output scale)`.

**New wording (verbatim, C12.E1R):**

> At zoom 1 with no zoom interaction, and where the maximum observing output scale ≤ H_max(E), the policy equals Phase B MaxGesture. Integer hint = ⌈h⌉, lowered to `max(1, ⌊H_dev(E)⌋)` when ⌈h⌉ > H_dev(E), so that an integer-scale client's buffer also fits the device; emission only on change; unobserved elements hold h; evaluation once per core batch after all its updates. **Initial hint:** `h := t(ρ_pred)` over the presentations whose view intersects the placed window-geometry rect (or the anchor point when the size is 0×0); if none, `t(max connected output scale)`.

## Invariants whose wording is unchanged but whose formula changed

| Invariant | Formula | Why the wording stays | Finding |
|---|---|---|---|
| INV-RENDER-1 | F-RENDER-1 (tracker size), F-LATTICE-1 (cull symbol) | the extracted edge/location text is unchanged | E-F2-01 |
| INV-RENDER-3 | F-MEMBER-1 (`view_O`) | the statement is in terms of projection, not of a size symbol | E-F2-01 |
| INV-HIT-1 | F-HIT-1 (`u = p · scale_O`) | the invariant (hit = presented surface) was right; the formula contradicted it | E1R-N1 |
| INV-PRES-1 | F-PRESENT-1 (`view_O/scale_O`) | no size symbol in the wording | E-F2-01 |

Reviewed and unchanged: INV-CAMERA-1 (the reconfiguration entry mints no proof, so the list of minting entries stays exact), INV-INPUT-10 (E1R-N2 is a validation obligation, OB-51), INV-SCALE-1 (E-F2-07 disproved), INV-ARCH-3 (P1 goes through the existing patch policy).
