# E1R — Reproduction record

Status: **UNSEALED**. Method per finding: reproduce against the sealed architecture → read pinned Smithay 0.7.0 → read protocol XML → smallest executable probe with a negative control → classify → only then correct. All runs HEADLESS/OFFLINE (`cargo --offline`); no package was installed; the live desktop was not touched. Smithay paths are relative to `~/.cargo/registry/src/index.crates.io-…/smithay-0.7.0/`.

## E-F2-01 — rotated output dimensions (Level A): reproduced exactly

**Source trace** (physical mode → tracker → damage space → renderer size → transform → GLES viewport → projection → framebuffer):

| Step | Smithay 0.7.0 | What it means |
|---|---|---|
| `OutputDamageTracker::new(size, scale, transform)` | `src/backend/renderer/damage/mod.rs:243-249` (`OutputModeSource::Static`) | the size is stored as given |
| `from_output` (not used by Infynitae) | `src/output.rs:517-529` | uses `current_mode.size`, the **untransformed** `wl_output` mode — Smithay's own convention |
| `render_output` | `damage/mod.rs:330` `(output_size, …, output_transform)`; `:335` `output_transform.invert()`; `:340` `output_geo = transform_size(output_size)` | element/damage space is the **transformed** size, derived from the stored one |
| renderer call | `damage/mod.rs:369` `renderer.render(framebuffer, output_size, output_transform)` | the stored size goes to the renderer |
| `Renderer::render` contract | `src/backend/renderer/mod.rs:406-407` "dimensions of the display **before** the `dst_transform`" | stored size must be the physical framebuffer size |
| GLES | `src/backend/renderer/gles/mod.rs:2056` `glViewport(0, 0, output_size)`; `:2067` swap for the projection only | viewport = stored size; projection space = swapped |

With the sealed rule (`mode_O` after the transform fed to the tracker), a 1920×1080 panel at 90° gets `glViewport(0,0,1080,1920)` and an element space of 1920×1080: double swap.

**Executable probe** `experiments/b8-harness/src/bin/xtransform.rs` (EE-1): real `OutputDamageTracker::render_output` + real `SolidColorRenderElement`s into `ProbeRenderer`, a CPU rasteriser with the GLES viewport/scissor rule above. Handedness (which of 90/270 is clockwise on glass) is not claimed; every check is invariant under inverting the mapping. No GPU, no T4 claim.

| Case (panel 1920×1080) | view (after transform) | OutputLogical | E1R tracker = `phys_O` | sealed tracker = `view_O` | hit E1R `p·s` | hit sealed `t(p·s)` / `t⁻¹(p·s)` |
|---|---|---|---|---|---|---|
| Normal 1.0 | 1920×1080 | 1920×1080 | C1–C4 ok | ok (no swap) | 0/200 | 0/200, 0/200 |
| 90° 1.0 | 1080×1920 | 1080×1920 | ok | **4 fail** (viewport 1080×1920; 1 814 400 px uncovered; 907 200 lost) | 0/200 | 200/200, 200/200 |
| 180° 1.0 | 1920×1080 | 1920×1080 | ok | ok | 0/200 | 199/200, 199/200 |
| 270° 1.0 | 1080×1920 | 1080×1920 | ok | **4 fail** | 0/200 | 200/200, 200/200 |
| 90° 1.25 | 1080×1920 | 864×1536 | ok | **4 fail** | 0/200 | 200/200, 200/200 |
| 270° 1.25 | 1080×1920 | 864×1536 | ok | **4 fail** | 0/200 | 200/200, 200/200 |
| 90° 1.5 | 1080×1920 | 720×1280 | ok | **4 fail** | 0/200 | 200/200, 200/200 |
| 270° 1.5 | 1080×1920 | 720×1280 | ok | **4 fail** | 0/200 | 200/200, 200/200 |
| Flipped90 1.25 | 1080×1920 | 864×1536 | ok | **4 fail** | 0/200 | 199/200, 200/200 |
| Flipped 1.0 | 1920×1080 | 1920×1080 | ok | ok | 0/200 | 198/200, 198/200 |

C1 viewport = panel; C2 view covers the panel exactly (no gap, no lost fragment); C3 the four view edges land on the four panel edges at full length (no off-by-one, no extra pixel); C4 full-frame damage = view extent; C5 render/hit agreement (cursor element vs hit tile, corners + 195 seeded points). Result: E1R rules 0 failures in 10 cases; sealed tracker fails 7/7 rotated cases and 0 checks on unrotated ones; sealed hit fails 18/18 non-Normal cases → **E1R-N1**.

Membership, project/unproject and boundary rounding are formulated in `view_O` space in both the sealed and the corrected architecture (the sealed `mode_O` already meant the transformed size there), so they do not discriminate; the corrected F-LATTICE-1/F-MEMBER-1 only make the symbol explicit. C2/C3 are the boundary-rounding checks at the physical edges.

## E-F2-02 — duplicate `unlocked` across batches (Level A): reproduced, wider than reported

**Smithay:** `src/wayland/pointer_constraints.rs:143-157` — `deactivate()` sends `unlocked`/`unconfined` unconditionally and removes only oneshot entries (`:156`). `src/wayland/seat/pointer.rs:246-256` — `leave()` calls `deactivate()` on any registered constraint, active or not. `with_pointer_constraint` exposes no removal; `PointerHandle.wl_pointer` is `pub(crate)` (`src/input/pointer/mod.rs:36`), so a wrapper `PointerTarget` cannot send `wl_pointer.leave` without `WlSurface::leave`.

**Protocol** (`pointer-constraints-unstable-v1.xml:249-264, 315-331`): `locked`/`unlocked` are state notifications ("activated" / "no longer active"); the XML does not define a second `unlocked` without `locked`. The defect is therefore judged against the architecture's own promise: INV-INPUT-5 "exactly once", violation clause "lost/duplicate lock events"; M13 "exactly one `unlocked`". Phase E's claim of fatal asserts in SDL3/Chromium was not tested and is not relied on.

**Probe** `xlock_batch` (EE-2), validator = per-object state machine (activation only from inactive, deactivation only from active, nothing after a oneshot end). Wire order, published Smithay:

| Case | Wire (after `locked`/`confined`) | Published | P1 |
|---|---|---|---|
| E1 persistent, explicit deactivate (A), exit to W2 (B) | `unlocked` · `leave W` · **`unlocked`** · `enter W2` | INVALID | VALID |
| E2 … exit to empty | `unlocked` · `leave W` · **`unlocked`** | INVALID | VALID |
| E3 oneshot, explicit deactivate, exit | `unlocked` · `leave W` · `enter W2` | VALID | VALID |
| E4 persistent, explicit deactivate, surface destroyed | `unlocked` · **`unlocked`** (no `leave`: surface dead) | INVALID | VALID |
| E5 persistent, explicit deactivate, output removed | `unlocked` · `leave W` · **`unlocked`** | INVALID | VALID |
| E6 persistent, deactivate, reactivation, exit | `unlocked` · `locked` · `leave W` · `unlocked` | VALID | VALID |
| E7 persistent, deactivate, held button, exit after release | `unlocked` · button · motion · release · `leave W` · **`unlocked`** | INVALID | VALID |
| E8 persistent confinement | `unconfined` · `leave W` · **`unconfined`** | INVALID | VALID |
| E9 persistent never activated, exit | `leave W` · **`unlocked`** | INVALID | VALID |
| E10 oneshot never activated (W-SM-4b) | `leave W` · **`unlocked`** (lock defunct) | INVALID | VALID |
| E11 public-API alternative: end by `leave`, return, exit | `leave` · `unlocked` · `enter W` · `leave` · **`unlocked`** | INVALID | VALID |

E11 shows that adapter-owned lifecycle tracking cannot avoid the duplicate: any later `leave` of a surface with a registered inactive constraint emits it. Single authority is preserved by P1 (Smithay sends only what core's policy activated). **P1** (`experiments/e1r-smithay-p1/patches/0001-…patch`): `if constraint.is_active()` around the `deactivate()` in `leave()`, applied to a copy of the registry source inside the experiment; `cargo tree` confirms the patched crate is linked and the dependency set is otherwise identical. Regression under P1 (EE-7): x8 48/48 unchanged; x12 W-SM-4b no longer reproduced; xlock_grab EXPLICIT/persistent becomes valid, SEALED still fails (that divergence is independent of P1). Upstream search (2026-10-05) found no Smithay issue or fix; filing one is an outward action left to the user (OB-11 criterion 3).

## E-F2-03 — dynamic output reconfiguration (Level B): reproduced

The sealed spike had only `output_connected`/`output_disconnected`; M4 row 2 named a "descriptor update through the token entry" that did not exist. Corrected: `Core::output_reconfigured(&mut OutputLifecycleToken, OutputId, OutputDescriptor)` (spike, EE-4).

Camera semantics, decided:
1. **Camera identity:** unchanged (same `OutputId`, same binding; test `output_reconfigured_keeps_identity_and_camera`).
2. **Centre `(c_x, c_y)` and zoom:** unchanged — the entry mints no `Authorized` proof (verify.sh 2h source check + seeded negative control).
3. **Logical viewport V_O:** recomputed as `view_O / (zoom · scale)` around the unchanged centre (F-VIEW-1); centre anchoring keeps content centred.
4. **Transformed dimensions:** `view_O = transform_size(phys_O)` from the new descriptor; the tracker is rebuilt with `phys_O` (F-RENDER-1).
5. **Scale epoch:** yes — a reconfiguration is a ScaleInteractionEpoch settlement event (as connect/disconnect), because it changes ρ and observation.
6. **Visible elements:** membership re-evaluated at batch end (F-MEMBER-1); `wl_surface.enter/leave` as needed.
7. **Confinement:** INV-INPUT-5 reconciliation trigger ("the interaction output's view or descriptor"); re-clamped or ended, never warped.
8. **Locks:** same trigger; kept iff the lock point is still observable through the interaction output.
9. **Output-local pointer:** clamped half-open into the new logical extent (F-RECONF-1); no Canvas target, no relative motion synthesized; the true position goes out with the batch-end routed motion.
10. **Client configures:** none required for ordinary toplevels (window size is Stage state, independent of outputs). Required: `wl_output` mode/scale/transform + `done` and `xdg_output` logical size (protocol), and one configure for an element in protocol fullscreen on that output (its size is the output's logical size). Scale changes reach clients as preferred-scale emissions, not configures.

Negative probes: p10 (no token, E0061), p11 (`InputPort` instead of the token, E0308), p12 (zoom in the descriptor, E0560), p13 (camera argument, E0061). Reconnect vs reconfiguration are separate tests (`output_reconfigured_rejects_unknown_foreign_and_identity_change` shows reconnect allocates a new `OutputId` and that the stale id is rejected).

## E-F2-04 — camera cardinality (Level B): reproduced; narrowed

Reconstruction: "N outputs may share one camera" originates in Phase B B7.2 as a capability answer ("Can two outputs share Camera state? Yes"). No Foundation 01 flow requires it: there is no binding command, the 2R lifecycle binds by `OutputKey`, `CameraSet.bound` is `Option<OutputId>`, and PD-F2-11's shared-fit rules exist only to make sharing coherent. Decision: 1:1 for Foundation 01; mirroring behind gate G-MIRROR, requiring change control across invariants, CameraSet, lifecycle API, persistence, fit, machines and tests together. One coherent model now holds in INV-CAMERA-3, M5, OBJ-07, ADR-04 and the spike.

## E-F2-05 — zero outputs (Level B): reproduced; Phase E's fix partial

**Wayland/Smithay semantics:** `wl_pointer.leave` means the seat's pointer no longer focuses the surface. Nothing in wayland.xml requires it when an output disappears, but leaving focus in place lets the client believe it still has usable pointer focus on an output that no longer exists — which the authorization forbids. Smithay routes focus only through `PointerHandle::motion`; while a client implicit grab is active, `motion(None)` is ignored (N2), and `unset_grab` restores focus through a `motion` that reaches a locked surface (`src/input/pointer/mod.rs:763-782`, N3). `set_grab(…, Focus::Clear)` clears focus through Smithay's internal motion path with no client motion (`:735-760`).

**Rule (INV-OUTPUT-1):** `NoOutput`; zero-output grab with `Focus::Clear`; one `motion(None)` at the stored location (delivered to no one); input consumed while `NoOutput`; grab removed first on reconnect, then startup placement.

**Wire (EE-3, published Smithay; identical with P1):**

| Case | removal | zero-output | reconnect |
|---|---|---|---|
| no constraint | `leave W` | — | `enter W` |
| persistent lock | `leave W` · `unlocked` | — | `enter W` · `locked` |
| oneshot lock | `leave W` · `unlocked` | — | `enter W` (lock defunct) |
| confinement | `leave W` · `unconfined` | — | `enter W` · `confined` |
| held button | `leave W` | — (release consumed) | `enter W` |
| held button + persistent lock | `leave W` · `unlocked` | — | `enter W` · `locked` |

Negative controls: N1 sealed F-RELOCATE-1 on ∅ panics; N2 without grab handling the client keeps focus (motion only, no leave); N3 `unset_grab` delivers `wl_pointer.motion` to the locked surface. `Option<(OutputId, OutputLogical)>` is sufficient as data; OBJ-20 names it `PointerState::{Located, NoOutput}` because the adapter grab is part of the state. Keyboard focus is not output-bound and is unchanged; cameras persist unbound.

## E-F2-06 — zero-size singularity (Level B): partially reproduced

`s_max(E) = 0` iff every surface of E has zero extent: reachable at every xdg_toplevel initial commit (no buffer), which is exactly when F-SCALE-3 computes the initial hint. 0×N and N×0 give `s_max = N > 0`: not singular. After a buffer is attached each side is ≥ 1 (wl_shm rejects non-positive sizes; wp_viewport destinations are > 0 or unset). Domain definition: `H_dev(E) = sup{h > 0 : h·side ≤ D_max ∀ buffered sides}`, which is `D_max/s_max` for `s_max > 0` and `+∞` for the empty constraint set; `H_max = min(H_cfg, H_dev)` is finite because H_cfg is (PROV-1). Equivalence to `H_cfg` at zero extent therefore follows from the meaning of the bound, not from a guard. EE-5 cases: 0×0, unbuffered tree, 0×N, N×0, buffered root + unbuffered subsurface, ordinary, large; transition 0×0 → 800×600 / 8000×100 / 16384² with P7. Negative control: sealed formula raises `ZeroDivisionError`.

While testing the transition, **E1R-N3** appeared: after P7 lowers h to 2.048 for an 8000-px side, `preferred_buffer_scale = ⌈h⌉ = 3` asks an integer-scale client for 24 000 px.

## E-F2-07 — fullscreen scale (Level C): disproved

Semantic correctness, resource cost and the device limit, separately (EE-6): the canonical ρ is 4.0 for zoom 2 on a 2.0 output, but `h = t(ρ) ≤ H_max(E) = min(3.0, 16384/1920) = 3.0`, so the buffer is 5760×3240 (71 MiB, ×9) — valid, device-safe, and the RB-7 trade-off accepted under the frozen MaxGesture policy. The 8K claim ignores H_cfg. Clamping ρ to the fullscreen output's scale would give a 1920×1080 buffer sampled at 0.25 buffer px per physical px on the observing output: a policy change (CP2-Q1, still open), not a defect fix. The device-bound exception (P7) is unchanged.

## E-F3-03 — output identity

| Identity | Meaning | Lifetime | Unique? |
|---|---|---|---|
| runtime `OutputId` | one connected output instance | until disconnect; never reused | yes, by allocation |
| connector identity | DRM device + connector name (or the backend's virtual output name) | topology; reused across monitors | yes among simultaneously connected outputs of one device; not across docks/MST reshuffles |
| physical-monitor identity | EDID make/model/serial | the monitor | **no**: serial may be 0 or absent; identical monitors collide |
| persistence identity `OutputKey` | hash of (device, connector, make, model, serial-if-present) | across sessions | **no** guarantee; treated as a best-effort key |

EDID serial is not made mandatory. Deterministic collision rule (INV-CAMERA-3): re-bind only the lowest unbound camera with the key; a concurrent duplicate gets a new default camera not seeded from persistence. The sealed rule re-bound a camera that was already bound, leaving the first output without a camera (spike `probe_sealed_key_steal`, EE-4). A different physical monitor arriving on the same connector with the same key reuses the remembered view: accepted and documented (view state only, no authority). Platform behaviour and whether ambiguous keys are persisted at all: OB-48, gate G-OB48.

## E-F1-01, E-F3-01 — checker

E-F1-01: 47 bare tokens (`STAGE-1`, ranges such as `FRAME-1 … FRAME-6`) on 14 ADR lines were never validated. All are now canonical `INV-*` (ranges expanded; no alias namespace). The new check `adr-unprefixed-inv` found the defect in the live package before the correction (38 distinct tokens) and is seeded by fixture 13.

E-F3-01: the checker now validates formula fields, Definition-line structure and amendment provenance (fixtures 14–16) and documents its limit with a boundary fixture (18: `u = p · scale_O` → `u = p / scale_O` passes). It is not a theorem prover; formula correctness rests on EE-1…EE-6 and the Smithay/protocol citations above.
