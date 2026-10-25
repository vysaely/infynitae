# D3 — Ownership and authority matrix

Baseline: Phase C seal `615b0ed`. The matrix re-derives authority from the sealed rules (C2.1, C2.2, C-AUTH, B3.2, B7.3, B12.1) **and from the behaviour of the code those rules delegate to** (Smithay 0.7.0). Delegated behaviour is where the sealed C-AUTH table fails, through two independent writers it does not list (PD-F2-01, PD-F2-02).

Classes: **A** authoritative · **P** protocol (Smithay-owned) · **D** derived · **C** cache · **PO** presentation-only.

| # | State | Class | Owner | Legal writers | Readers | Reconstruction source | Lifetime | Invalidation | Forbidden writers | INV |
|---|---|---|---|---|---|---|---|---|---|---|
| 1 | Element identity (`ElementId`, `InstanceId`) | A | core `Stage` | Stage minting at `new_toplevel` | all; IPC as `(InstanceId, ElementId)` | none needed (process-local) | element / process | never reused | adapter, shell, persistence, clients | STAGE-3 |
| 2 | Stage anchors | A | core `Stage` | `stage::place` via placement (a), drag commit (b), shell `MoveElement` (c) | origin(), render, hit, membership, shell | persistence at startup (via (a)) | element (kept on unmap) | — | cameras, renderer, adapter, satellite, scale, fit, fullscreen | STAGE-1/2/6 |
| 3 | Z-order | A | core `Stage` | map (top), remap (top), raise by explicit focus action | render, hit | none (process) | element | — | Smithay (no `Space`), clients (`set_parent` is a hint) | STAGE-1 |
| 4 | Lifecycle state | A | core `Stage` | adapter `ProtocolEvent`s → Stage | all | — | element | M1 | — | STAGE-4/5 |
| 5 | Metadata (title, app_id, `transient_for`, `RestoreKey`) | A (copy of client P for title/app_id) | core `Stage` | commit hook | shell, placement | re-read at commit | element | unmap re-read | — | STAGE-4 (**PD-F2-15**) |
| 6 | Attention set | A | core `FocusModel` | activation requests (not granted) | shell | none | until acknowledged / **unmap? undefined** | **PD-F2-15** | camera | INPUT-9 |
| 7 | Surface-tree snapshot | D (of P) | core type, adapter writer | adapter commit hook, wholesale | core | re-extract on next commit | per commit | replaced; dropped on unmap | core, Stage | COORD-6 (**PD-F1-06** adds `frame_requested`) |
| 8 | `SurfaceKey ↔ WlSurface` registry | A (association) | adapter | role creation / destroy | adapter | — | surface | destroy | core (cannot dereference) | COORD-4 |
| 9 | Window geometry, surface sizes, offsets | P | client (Smithay state) | client | snapshot | client re-commit | protocol | per commit | Stage, core, shell (no size writes) | STAGE-2 |
| 10 | Camera centre | A | `core::navigation` | navigation via `InputPort`/`ShellPort` **only** | Views, shell | persistence by `OutputKey` | session (survives outputs) | — | protocol handlers, adapter, fullscreen, scale; **today also: re-fit and animation without a port (PD-F2-01)** | CAMERA-1/3 |
| 11 | Camera zoom | A | `core::navigation` | as #10 (gestures, fit/restore) | Views, scale | as #10 | session | — | scale (reads), clients; **re-fit (PD-F2-01)** | CAMERA-1/5 |
| 12 | Camera ↔ output binding | A | output manager (stored in the camera set) | output add/remove | Views | `OutputKey` memory | output | — | camera set (stores only), Stage | CAMERA-3 |
| 13 | `ViewportFit{O}` | A | `navigation` | fit/restore via ports | View derivation | restore saved camera | until restore/navigate/unmap/removal | M8 | protocol handlers | CAMERA-5 (**PD-F2-11**) |
| 14 | Navigation animation | **undefined owner** | — (C7.3 calls it a "compositor interaction") | **undefined** | View | — | until done/superseded | — | — | **PD-F2-01**, PD-F2-07 |
| 15 | `OutputModel` (mode, scale, transform, refresh) | A | core output manager | backend descriptors + configuration | all | backend re-enumeration | output | M4 | Smithay `Output` (written from it), clients | OUTPUT-1 |
| 16 | Arrangement | A | output configuration | configuration | pointer crossing only | configuration | session | output add/remove | Stage, cameras | COORD-4 |
| 17 | `View(O)` (v, R, C, c_eff) | D | core (pure) | recomputed on camera/output change | render, hit, unproject | recompute | per change | camera, OutputModel, **transform (PD-F1-04)** | everything (I8) | CAMERA-3, COORD-2 |
| 18 | `MembershipTable` | D | core visibility pass | visibility pass only | scale, clock, adapter | recompute | per pass | Stage, camera, output, snapshot | Smithay entered-sets | RENDER-3 |
| 19 | Smithay `Output` entered-sets | P | Smithay | adapter applying Enter/Leave | Smithay dedupe | from #18 | protocol | — | policy reads | RENDER-3 |
| 20 | `ProjectionFrame(O)` | C | core render-list builder | per produced frame | hit-testing | rebuilt on demand | one frame | next frame; **destroyed entries (PD-F1-07)** | Stage, cameras | HIT-1 |
| 21 | Scale requirement ρ | D | core visibility | visibility pass | `ScaleState` | recompute | per pass | membership, zoom, presentation | — | SCALE-1 |
| 22 | `ScaleState` (h, epoch dir, gesture/SC-D flags, ladder) | A (policy state) | core `ScaleState` | batch-end evaluation; P7 on commit | adapter (emits) | recompute from ρ + rules (may emit one settle) | element | ladder on hotplug (**PD-F2-08**) | adapter, clients, fullscreen | SCALE-2…5 |
| 23 | Frame clock per element | D | core | visibility pass | frames | recompute | per pass | membership, refresh | — | FRAME-2 |
| 24 | `FrameLiveness` (last completion, timer armed) | A (policy state) | core frames | completion/kick/fallback logic | timer | reset to "now" (worst case: one early fallback) | element | — | Smithay throttle | FRAME-3/4 (**PD-F1-06**) |
| 25 | `SuspensionReasons` | A | core | semantic transitions only | adapter | — | element | — | camera/visibility | FRAME-6 (**PD-F2-14**) |
| 26 | `FullscreenState` + `OutputPresentation` | A / PO (view) | `core::protocol` + output model | validated client request; unmap; output removal | render, hit, scale | drop; client may re-request | until exit | M9 | navigation, shell | PRES-1 (**PD-F2-10**) |
| 27 | `PointerModel` (`Free(O,p)` / `Locked(S, q)`) | A | core intake | intake; reconciliation; relocation | hit, adapter | — | seat | **PD-F2-03** (needs output; surface-relative point), **PD-F2-04** (triggers) | Smithay (applied copy), cameras | COORD-5, INPUT-5 |
| 28 | Pointer focus (delivered) | D/A | core per routed event; **Smithay during client-owned grabs (C4.2 P3)** | core hit-test → Effect; Smithay grab objects | adapter, W-SM-4a decision | recompute from #20 + #27 | per event | — | cameras | HIT-1, INPUT-3 (**PD-F2-02**) |
| 29 | Client grabs (implicit click, popup, DnD) | P | Smithay | Smithay (per client-owned sequence) | Smithay | protocol | sequence | release, dismissal | core (records owner = client) | INPUT-1 (**PD-F2-02**) |
| 30 | Keyboard `FocusModel` (target, history) | A | core `FocusModel` | explicit actions, target loss, activation grant | adapter, shell | successor policy | seat | — | cameras, visibility, pointer motion, clients; **Smithay `PopupKeyboardGrab` writes it today (PD-F2-02)** | INPUT-8 |
| 31 | Smithay keyboard focus | P (applied) | Smithay | adapter from #30; **popup grab** | protocol | from #30 | — | — | policy reads | INPUT-7 |
| 32 | Ownership table (buttons/keys) | A | core intake | press decisions | intake | physical release on device loss | press…release | — | Smithay filter (executes only) | INPUT-1/2/6 |
| 33 | Interaction state (B10.4) | A | core intake | intake | navigation, scale (gesture flag) | — | interaction | output removal → orphaned | — | INPUT-3 (**PD-F2-01** animation not in it) |
| 34 | XKB state | P (authoritative for XKB) | Smithay `KeyboardHandle` | Smithay on every key | core via filter | keymap reload | seat | — | core (no copy) | INPUT-6 |
| 35 | Constraint objects + activation policy | P + A (policy) | Smithay objects; core policy | Smithay protocol; adapter executes core decisions | core | protocol | protocol | — | camera code | INPUT-5 (**PD-F2-05**) |
| 36 | Client buffers; renderer textures; damage trackers; `D_max` | P / D | client / adapter / renderer | client; adapter per frame | renderer, core ceiling (`D_max`) | client re-commit; re-import; **re-query `D_max` (PD-F1-08)** | per frame / renderer | zoom → tracker scale (**PD-F1-03**) | compositor never alters buffers | RENDER-1, SCALE-2 |
| 37 | Shell connection, subscription generation | PO | `infynitae-ipc` | IPC | shell | Snapshot + Subscribe | connection | disconnect | shell writing core state other than by semantic commands | SHELL-1/2 |
| 38 | Satellite process, X display | P (a client) + supervisor state | bin supervisor | supervisor | — | restart (elements not preserved) | process | crash | any X11 path to Stage/cameras | XWAYLAND-1 (**PD-F2-16**) |
| 39 | Persistence file | C (of A) | core persistence service | save on change; load at startup | — | last good copy | across sessions | atomic replace | partial application | FAIL-1 |
| 40 | Observability | PO | `tracing` | emit only | developers | — | — | — | any decision reading it | OBS-1 |

## Authority searches

### S1 Multiple authoritative writers

| State | Writers found | Verdict |
|---|---|---|
| Keyboard focus (#30/#31) | FocusModel **and** Smithay `PopupKeyboardGrab` (sets the popup, restores the root, ignores foreign `set_focus`; ED-3) | **two writers, silent divergence → PD-F2-02 (Level A)** |
| Pointer focus (#28) | core, and Smithay during client grabs. C4.2 P3 states this, but C2.2/C-AUTH list Smithay as "applied copy / NOT owner" | **inconsistent sealed text; the W-SM-4a decision depends on it → PD-F2-02** |
| Camera centre/zoom (#10/#11) | navigation via ports **and** fit re-fit on client commit (C5.1) **and** animation ticks (C7.3) | **port-free writers → PD-F2-01 (Level A)** |
| Stage anchors (#2) | three callers of one function `stage::place` | single writer (deliberate, C-AUTH) |
| `ScaleState` (#22) | batch-end evaluation and P7 on commit, both inside core's scale module | single module; holds |
| `FullscreenState` (#26) | client request policy; unmap; output removal | one module (`core::protocol`) with lifecycle hooks; holds |

### S2 Authority cycles (Level A if found)

| Cycle | Verdict |
|---|---|
| client commit (size) → re-fit → camera zoom/centre → ρ → h → client size | **a client drives camera authority → PD-F2-01 (Level A).** The scale part is bounded by SC-E; the authority part is the defect. |
| h → client logical size → H_max → h | bounded (P7 + SC-E); no authority crossing |
| fullscreen request (client) → `OutputPresentation` → membership → ρ → h | presentation is protocol-policy state, never camera; bounded; holds |
| FocusModel → Smithay grab → (Smithay writes focus) → not fed back | not a cycle; a **divergence** (PD-F2-02) |
| shell command → core → event → shell | read-only copies; holds (INV-SHELL-2) |

### S3 Derived state that cannot be reconstructed

| State | Problem | Verdict |
|---|---|---|
| "callback pending" per surface | Smithay protocol state, not in the snapshot; core cannot arm the fallback timer only when needed, nor rebuild `FrameLiveness` after recovery | **PD-F1-06** |
| Canvas lock point `q_lock` | stored in Canvas but meaningful only relative to its surface; after an element move it cannot be recomputed | **PD-F2-03** (store it surface-relative; then it is reconstructable) |
| `ScaleState` epoch direction | policy state; after recovery recomputed from ρ (worst case one settle) | holds (C-AUTH) |
| `ProjectionFrame` | rebuilt on demand | holds |

### S4 Caches without invalidation

| Cache | Missing invalidation | Verdict |
|---|---|---|
| `ProjectionFrame(O)` | entries of destroyed surfaces until the next frame | **PD-F1-07** (fail closed) |
| damage tracker scale | `from_output` ignores zoom | **PD-F1-03** |
| ladder L(E) | hotplug; held or deferred h off-ladder | **PD-F2-08** |
| `D_max` | renderer recreation | **PD-F1-08** |
| `View(O)` | output transform (w/h swap) | **PD-F1-04** |

### S5 Protocol objects outliving authority

| Object | Situation | Verdict |
|---|---|---|
| locked-pointer constraint | output removal under an implicit grab: core believes the lock ended, Smithay keeps it active (D5 E4) | **PD-F2-02** |
| `PopupGrab` | outlives a FocusModel decision that moved focus elsewhere | **PD-F2-02** |
| deferred fullscreen request | no cancellation or bound | **PD-F2-10** |
| `wl_output` objects after removal | inert per protocol; core releases its state first (leave before destroy) | holds |
| xdg-activation tokens | validated by provenance; stale tokens ignored | holds (T1 OB-27) |

### S6 Shell or backend state becoming durable truth

| Candidate | Verdict |
|---|---|
| shell-started animation continuing after shell disconnect (C7.3) | its state has **no defined owner or authority path → PD-F2-01** |
| shell presentation-only state (overview, launcher) | not stored by the compositor; holds |
| backend output descriptors | inputs to `OutputModel` (A), not truth themselves; holds |
| `D_max` from the renderer | derived; re-queried (PD-F1-08); holds |
| persistence | cache of A; all or nothing; holds |

**Result:**
- Two Level A authority defects: PD-F2-01 (a client and timers write camera state without a port) and PD-F2-02 (Smithay grabs write or override focus behind the FocusModel).
- One state-representation defect: PD-F2-03.
- Four cache-invalidation refinements (D-F1).
- No shell or backend durable truth beyond the undefined animation owner.
