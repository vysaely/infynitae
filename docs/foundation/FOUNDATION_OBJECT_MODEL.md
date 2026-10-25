# Object model

Core's objects (`OBJ-*`): what each holds, who may write it, and how long it lives. Where an
invariant defines the authority or the writers, the entry cites it; the invariant is the rule.

Classes: **A** authoritative · **P** protocol (Smithay-owned) · **D** derived · **C** cache · **PO** presentation-only.

<!-- living:OBJ -->
### OBJ-01 — Stage (anchors, z-order, lifecycle state)
- **Class:** A
- **Owner:** core `stage`
- **Writers (authority per cited invariants):** `stage::place` callers and lifecycle events, as INV-STAGE-1/-4 define
- **Readers:** render list, hit-test, membership, shell
- **Lifetime / invalidation:** element lifetime (anchor kept on unmap)
- **Reconstruction:** persistence via placement at initial commit
- **Forbidden writers:** cameras, renderer, adapter, satellite, scale policy, fit, fullscreen, shell (except `MoveElement` through `stage::place`)
- **Invariants:** INV-STAGE-1, INV-STAGE-2, INV-STAGE-4, INV-STAGE-5, INV-STAGE-6

### OBJ-02 — Element identity (`ElementId`, `InstanceId`)
- **Class:** A
- **Owner:** core `stage`
- **Writers (authority per cited invariants):** Stage minting at `new_toplevel` (INV-STAGE-3)
- **Readers:** all; IPC as `(InstanceId, ElementId)`
- **Lifetime / invalidation:** process
- **Reconstruction:** none needed
- **Forbidden writers:** adapter, shell, persistence, clients
- **Invariants:** INV-STAGE-3

### OBJ-03 — Element metadata (title, app_id, `transient_for`, RestoreKey)
- **Class:** A (copy of client data)
- **Owner:** core `stage`
- **Writers (authority per cited invariants):** commit hook
- **Readers:** shell, placement
- **Lifetime / invalidation:** element
- **Reconstruction:** re-read at commit
- **Forbidden writers:** —
- **Invariants:** INV-STAGE-4 (lifecycle open item OB-35)

### OBJ-04 — Attention set
- **Class:** A
- **Owner:** core FocusModel
- **Writers (authority per cited invariants):** activation requests not granted (INV-INPUT-9)
- **Readers:** shell
- **Lifetime / invalidation:** until acknowledged; unmap/destroy rule is OB-35
- **Reconstruction:** none
- **Forbidden writers:** camera, visibility
- **Invariants:** INV-INPUT-9

### OBJ-05 — Surface-tree snapshot
- **Class:** D (of protocol state)
- **Owner:** type in core; written by the adapter commit hook
- **Writers (authority per cited invariants):** adapter commit hook, wholesale per commit
- **Readers:** core
- **Lifetime / invalidation:** per commit; dropped on unmap/destroy
- **Reconstruction:** re-extract on the next commit
- **Forbidden writers:** core, Stage
- **Invariants:** INV-COORD-6, INV-STAGE-2; fields include `frame_requested` per surface (D7 refinement PD-F1-06), cleared by core when it emits `CompleteFrames` (INV-FRAME-3)

### OBJ-06 — `SurfaceKey ↔ WlSurface` registry
- **Class:** A (association)
- **Owner:** adapter
- **Writers (authority per cited invariants):** role creation / destroy
- **Readers:** adapter
- **Lifetime / invalidation:** surface
- **Reconstruction:** —
- **Forbidden writers:** core (keys are opaque)
- **Invariants:** INV-COORD-4

### OBJ-07 — CameraSet (centre, zoom, OutputKey, binding)
- **Class:** A
- **Owner:** core `navigation`
- **Writers (authority per cited invariants):** only `navigation`, one `Authorized` proof per write, minted in one of the three entries (INV-CAMERA-1); creation/binding only via INV-OUTPUT-2
- **Readers:** View derivation, scale, shell
- **Lifetime / invalidation:** session (survives outputs, INV-CAMERA-3)
- **Reconstruction:** remembered → persisted (OBJ-09) → default, on connect
- **Forbidden writers:** protocol handlers, client commits, timers, output descriptor contents
- **Invariants:** INV-CAMERA-1, INV-CAMERA-2, INV-CAMERA-3, INV-CAMERA-5, INV-OUTPUT-2
- **E1R (unsealed):** binding cardinality 1:1 (a camera is bound to at most one output; each connected output to exactly one camera). Re-binding takes the lowest **unbound** camera with the key; a concurrent key collision creates a new default camera not seeded from persistence (INV-CAMERA-3; EE-4). `output_reconfigured` is not a writer.

### OBJ-08 — Authority ports and tokens (`InputPort`, `ShellPort`, `OutputLifecycleToken`, per-action `Authorized`)
- **Class:** A (capabilities)
- **Owner:** minted by `Core::new` (ports, token) / entry module (`Authorized`)
- **Writers (authority per cited invariants):** none after minting
- **Readers:** their holders: backend input source, IPC source, backend output/display source
- **Lifetime / invalidation:** process
- **Reconstruction:** none (never re-issued, C12.R2.3)
- **Forbidden writers:** protocol state (E0277), adapter crate (OB-19 source rules)
- **Invariants:** INV-CAMERA-1, INV-OUTPUT-2, INV-SHELL-1

### OBJ-09 — Persisted camera lookup (`PersistedCameras`)
- **Class:** C (of A), read-only
- **Owner:** core
- **Writers (authority per cited invariants):** none after `Core::new`
- **Readers:** `output_connected` only
- **Lifetime / invalidation:** process
- **Reconstruction:** persistence file
- **Forbidden writers:** any public restore API (none exists)
- **Invariants:** INV-OUTPUT-2, INV-FAIL-1
- **E1R (unsealed):** keyed by `OutputKey`, a persistence identity — not a physical-monitor identity, not the runtime `OutputId`. Cross-session use is gated by G-OB48 (OB-48).

### OBJ-10 — OutputModel (mode, scale, transform, refresh)
- **Class:** A
- **Owner:** core output manager
- **Writers (authority per cited invariants):** output-lifecycle entries (INV-OUTPUT-2)
- **Readers:** all
- **Lifetime / invalidation:** output
- **Reconstruction:** backend re-enumeration through the same entry
- **Forbidden writers:** Smithay `Output` (written from it), clients
- **Invariants:** INV-OUTPUT-1, INV-OUTPUT-2
- **E1R (unsealed):** fields are `OutputKey`, physical mode `phys_O` (before the transform), transform, scale, refresh. `view_O = transform_size(phys_O)` is derived, never stored. Writers: `output_connected` and `output_reconfigured` (same `OutputId`, same `OutputKey`); `output_disconnected` removes (INV-OUTPUT-2).

### OBJ-11 — Arrangement
- **Class:** A
- **Owner:** output configuration
- **Writers (authority per cited invariants):** configuration
- **Readers:** pointer crossing (F-RELOCATE-1, F-CONFINE-1)
- **Lifetime / invalidation:** session
- **Reconstruction:** configuration
- **Forbidden writers:** Stage, cameras
- **Invariants:** INV-COORD-4

### OBJ-12 — View(O) (`v_O`, R, C, `c_eff`)
- **Class:** D for `v_O`, C and `c_eff`; the lattice reference `R_O` is A — per-output state that depends on history (C12.F14)
- **Owner:** core: pure functions for `v_O`, C, `c_eff`; `R_O` held in O's projection state (not in the camera)
- **Writers (authority per cited invariants):** `v_O`, C, `c_eff` recomputed on camera/output change (F-VIEW-1, F-LATTICE-1); `R_O` written only on F-LATTICE-1's transitions — O's connect, a camera (re-)binding, a change of `k_O` or `view_O`, `|v_O − R_O|·k_O` beyond 2³¹ — reached through the output-lifecycle and navigation entries
- **Readers:** render, hit, unproject
- **Lifetime / invalidation:** `v_O`, C, `c_eff`: per change; `R_O`: the output's (dropped on disconnect), kept across pans
- **Reconstruction:** `v_O`, C, `c_eff`: recompute from the camera, the output and `R_O`. `R_O`: none — not persisted, not restorable; at the next connect a new reference is set from that view (an initialisation, not a restoration; widths and gaps may differ by ±1 physical px from the previous session, once, D5)
- **Forbidden writers:** everything else (nothing is written back, INV-STAGE-6); no client, protocol event or persistence writes `R_O`
- **Invariants:** INV-CAMERA-3, INV-COORD-2, INV-RENDER-1
- **E1R (unsealed):** derived from `view_O` (F-VIEW-1); rebuilt on `output_reconfigured`.
- **C12.F14:** R is the output's lattice reference: held per output, not derived from each view origin; set at O's connect, a camera (re-)binding, a change of `k_O` or `view_O`, and when `|v_O − R_O|·k_O` would exceed 2³¹; kept while panning; dropped with O; never persisted. Because the kept reference depends on the view origins that came before, it is history-dependent state, not derived geometry: the fields above say so (corrected 2026-10-17; the earlier "Class D, core (pure), Reconstruction: recompute" is recorded in FOUNDATION_ARCHITECTURE §C12.F14.6). `v_O`, C and `c_eff` stay derived. Implemented by INC-10 (`b46021f`, 2026-10-18): Core's per-output lattice slot, read by every camera view (until then the code set R from every view origin).

### OBJ-13 — MembershipTable
- **Class:** D
- **Owner:** core visibility pass
- **Writers (authority per cited invariants):** visibility pass only (F-MEMBER-1)
- **Readers:** scale, clock, adapter
- **Lifetime / invalidation:** per pass
- **Reconstruction:** recompute
- **Forbidden writers:** Smithay entered-sets
- **Invariants:** INV-RENDER-3

### OBJ-14 — Smithay `Output` entered-sets
- **Class:** P
- **Owner:** Smithay
- **Writers (authority per cited invariants):** adapter applying Enter/Leave only
- **Readers:** Smithay dedupe
- **Lifetime / invalidation:** protocol
- **Reconstruction:** from OBJ-13
- **Forbidden writers:** policy reads
- **Invariants:** INV-RENDER-3

### OBJ-15 — ProjectionFrame(O)
- **Class:** C
- **Owner:** core render-list builder
- **Writers (authority per cited invariants):** per produced frame
- **Readers:** hit-test (F-HIT-1)
- **Lifetime / invalidation:** one frame; entries of dead surfaces fail closed (D7 refinement PD-F1-07)
- **Reconstruction:** rebuilt on demand
- **Forbidden writers:** Stage, cameras
- **Invariants:** INV-HIT-1

### OBJ-16 — ScaleState (h, ladder, ScaleInteractionEpoch{open, kind, direction, deadline})
- **Class:** A (policy state)
- **Owner:** core
- **Writers (authority per cited invariants):** batch-end evaluation, settlement, P7 (INV-SCALE-3/5)
- **Readers:** adapter (emits)
- **Lifetime / invalidation:** element
- **Reconstruction:** recompute from ρ + rules (may emit one settle)
- **Forbidden writers:** adapter, clients, fullscreen
- **Invariants:** INV-SCALE-1 … INV-SCALE-5

### OBJ-17 — FrameLiveness (last completion, timer armed)
- **Class:** A (policy state)
- **Owner:** core
- **Writers (authority per cited invariants):** completion / kick / fallback logic
- **Readers:** fallback timer
- **Lifetime / invalidation:** element
- **Reconstruction:** reset to now (worst case: one early fallback)
- **Forbidden writers:** Smithay throttle
- **Invariants:** INV-FRAME-1, INV-FRAME-3, INV-FRAME-4

### OBJ-18 — SuspensionReasons
- **Class:** A
- **Owner:** core
- **Writers (authority per cited invariants):** semantic transitions only
- **Readers:** adapter
- **Lifetime / invalidation:** element
- **Reconstruction:** —
- **Forbidden writers:** camera, visibility
- **Invariants:** INV-FRAME-6

### OBJ-19 — FullscreenState + OutputPresentation
- **Class:** A / PO (presentation view, F-PRESENT-1)
- **Owner:** core `protocol` + output model
- **Writers (authority per cited invariants):** validated client requests (answered per INV-PRES-1); unmap; output removal; replacement
- **Readers:** render, hit, scale
- **Lifetime / invalidation:** until exit
- **Reconstruction:** dropped; client may re-request
- **Forbidden writers:** `navigation`, shell
- **Invariants:** INV-PRES-1

### OBJ-20 — Seat physical pointer state `Located(interaction output, OutputLogical p) | NoOutput`
- **Class:** A
- **Owner:** core intake
- **Writers (authority per cited invariants):** intake; reconciliation; relocation (F-RELOCATE-1)
- **Readers:** hit-test, adapter
- **Lifetime / invalidation:** seat
- **Reconstruction:** Arrangement-nearest on output loss
- **Forbidden writers:** Smithay (applied copy), cameras
- **Invariants:** INV-COORD-5, INV-INPUT-5
- **E1R (unsealed, E-F2-05):** the state is `PointerState = Located { output, p } | NoOutput`. As data this is equivalent to `Option<(OutputId, OutputLogical)>`; the named state is kept because it is paired with the adapter's zero-output pointer grab (INV-OUTPUT-1). `NoOutput` holds before the first output and after the last one is removed; no output or coordinate is fabricated. Transitions: F-RELOCATE-1 (survivor set non-empty), `NoOutput` (empty), startup placement on the first connect, F-RECONF-1 on reconfiguration. Spike: `PointerState` (EE-4).

### OBJ-21 — Constraint state `{surface, kind, lock point: SurfaceLocal}`
- **Class:** A (policy) + P (protocol object)
- **Owner:** core activation policy; Smithay owns the object
- **Writers (authority per cited invariants):** activation/reconciliation (INV-INPUT-5); Smithay via protocol
- **Readers:** adapter
- **Lifetime / invalidation:** protocol object lifetime
- **Reconstruction:** protocol
- **Forbidden writers:** camera code
- **Invariants:** INV-COORD-5, INV-INPUT-5
- **E1R (unsealed, E-F2-02):** a deactivation event leaves this object only from the active state; Smithay's `leave()` must not deactivate an inactive constraint (P1 / upstream, OB-11; W-SM-4c).

### OBJ-22 — ActiveElementFocus (+ focus history)
- **Class:** A
- **Owner:** core FocusModel
- **Writers (authority per cited invariants):** explicit actions, target loss, activation grant (INV-INPUT-8)
- **Readers:** adapter, shell
- **Lifetime / invalidation:** seat
- **Reconstruction:** successor policy
- **Forbidden writers:** cameras, visibility, pointer motion, protocol routing, clients
- **Invariants:** INV-INPUT-8, INV-PROTO-1

### OBJ-23 — ProtocolSurfaceRoute (popup grab chain)
- **Class:** P (adapter, subordinate)
- **Owner:** adapter (Smithay `PopupGrab`)
- **Writers (authority per cited invariants):** validated `xdg_popup.grab`; client popup destruction; ordered preemption (INV-INPUT-10)
- **Readers:** Smithay seat
- **Lifetime / invalidation:** grab chain
- **Reconstruction:** protocol
- **Forbidden writers:** changing ActiveElementFocus
- **Invariants:** INV-INPUT-10

### OBJ-24 — Client-owned grab sequences (implicit click grab, DnD)
- **Class:** P
- **Owner:** Smithay
- **Writers (authority per cited invariants):** Smithay per client-owned sequence
- **Readers:** core uses the delivered focus for protocol-order decisions
- **Lifetime / invalidation:** press…release
- **Reconstruction:** protocol
- **Forbidden writers:** core (records owner = client)
- **Invariants:** INV-INPUT-1, INV-INPUT-5

### OBJ-25 — Ownership table (buttons, keys)
- **Class:** A
- **Owner:** core intake
- **Writers (authority per cited invariants):** press decisions (INV-INPUT-1/-6)
- **Readers:** intake
- **Lifetime / invalidation:** press…release
- **Reconstruction:** physical release on device loss
- **Forbidden writers:** Smithay filter (executes only)
- **Invariants:** INV-INPUT-1, INV-INPUT-2, INV-INPUT-6

### OBJ-26 — Interaction state (CameraPan, WindowMove, WindowResize; orphaned)
- **Class:** A
- **Owner:** core intake
- **Writers (authority per cited invariants):** intake
- **Readers:** navigation, ScaleInteractionEpoch
- **Lifetime / invalidation:** interaction
- **Reconstruction:** —
- **Forbidden writers:** —
- **Invariants:** INV-INPUT-3, INV-OUTPUT-1, INV-SCALE-3

### OBJ-27 — XKB state
- **Class:** P (authoritative for XKB)
- **Owner:** Smithay `KeyboardHandle`
- **Writers (authority per cited invariants):** Smithay on every key
- **Readers:** core via filter
- **Lifetime / invalidation:** seat
- **Reconstruction:** keymap reload
- **Forbidden writers:** core (no copy)
- **Invariants:** INV-INPUT-6, INV-INPUT-7

### OBJ-28 — `Effect` buffer
- **Class:** transient
- **Owner:** core
- **Writers (authority per cited invariants):** core per call (caller-owned buffer, C14.3)
- **Readers:** adapter
- **Lifetime / invalidation:** per call
- **Reconstruction:** —
- **Forbidden writers:** —
- **Invariants:** INV-ALLOC-1

### OBJ-29 — Render elements, damage trackers, `D_max`
- **Class:** D / P
- **Owner:** adapter / renderer
- **Writers (authority per cited invariants):** adapter per frame (F-RENDER-1); `D_max` queried and re-queried
- **Readers:** renderer; core ceiling (F-SCALE-2)
- **Lifetime / invalidation:** per frame / renderer
- **Reconstruction:** re-import; re-query
- **Forbidden writers:** —
- **Invariants:** INV-RENDER-1, INV-SCALE-2
- **E1R (unsealed, E-F2-01):** each damage tracker is `OutputDamageTracker::new(phys_O, Scale(k_O), transform_O)` (physical, untransformed size); render elements live in `view_O` space.

### OBJ-30 — Navigation history (camera values before fits)
- **Class:** A (data)
- **Owner:** core `navigation`
- **Writers (authority per cited invariants):** an authorized fit adds the camera value immediately before it, evicting the oldest entry first when 256 are held (F-FIT-1); an authorized restore removes the newest entry (M8); `output_disconnected`'s unbinding empties the history (M5)
- **Readers:** authorized restore
- **Lifetime / invalidation:** the binding of its camera to one `OutputId`: emptied when the camera is unbound; kept across `output_reconfigured`; an entry remains until a restore removes it, it is evicted as the oldest of 256, or the binding ends
- **Reconstruction:** none (not persisted)
- **Forbidden writers:** protocol handlers, clients, persistence
- **Invariants:** INV-CAMERA-5
- **C12.F15:** one entry per successful fit (owner decision 2026-10-17: one restore reverses the most recent fit; repeated restores unwind earlier fits). Previously "camera value before the last fit", lifetime "until the next fit". The depth bound C12.F15.9 left open is decided by C12.F16. Implemented by INC-10.
- **C12.F16:** a bounded stack (owner decision 2026-10-18): at most 256 entries per output, the oldest evicted when a successful fit finds it full (lost permanently); each entry is the exact prior camera value (Q16 `CanvasPos` and `f64` zoom, copied, restored verbatim); a refused fit or restore changes nothing; fixed storage of 256 entries per bound camera, none for an unbound one (C12.F16.3). Previously lifetime "with its camera; an entry remains until a restore removes it", no bound. Implemented by INC-10 (`cdf3a09`, 2026-10-18: `navigation::NavigationHistory`).

### OBJ-31 — Shell connection and subscription generation
- **Class:** PO
- **Owner:** `infynitae-ipc`
- **Writers (authority per cited invariants):** IPC
- **Readers:** shell
- **Lifetime / invalidation:** connection
- **Reconstruction:** Snapshot + Subscribe
- **Forbidden writers:** shell writing core state except by semantic commands
- **Invariants:** INV-SHELL-1, INV-SHELL-2

### OBJ-32 — xwayland-satellite process and X display
- **Class:** P (client) + supervisor state
- **Owner:** bin supervisor
- **Writers (authority per cited invariants):** supervisor
- **Readers:** —
- **Lifetime / invalidation:** process / session
- **Reconstruction:** restart on the same display (OB-33)
- **Forbidden writers:** any X11 path to Stage or cameras
- **Invariants:** INV-XWAYLAND-1

### OBJ-33 — Persistence file
- **Class:** C (of A)
- **Owner:** core persistence service
- **Writers (authority per cited invariants):** save on change; load at startup
- **Readers:** `Core::new` (OBJ-09)
- **Lifetime / invalidation:** across sessions
- **Reconstruction:** last good copy; all-or-nothing
- **Forbidden writers:** partial application
- **Invariants:** INV-FAIL-1
- **E1R (unsealed, E-F3-03):** entries are keyed by `OutputKey` (persistence identity). Collision handling: INV-CAMERA-3. Cross-session product claims gated by G-OB48.

### OBJ-34 — Observability (`tracing`)
- **Class:** PO
- **Owner:** `tracing`
- **Writers (authority per cited invariants):** emit only
- **Readers:** developers
- **Lifetime / invalidation:** —
- **Reconstruction:** —
- **Forbidden writers:** any decision reading it
- **Invariants:** INV-OBS-1

<!-- /living -->
