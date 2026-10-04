# D5 — Four-epoch combined multi-output audit

Baseline: Phase C seal `615b0ed`. Every number below comes from ED-2 ([`d5-model-output.txt`](../evidence/phase-d/d5-model-output.txt)), whose edge tables are asserted equal to Smithay 0.7.0's real render-element geometry in ED-1 ([`d5hit-output.txt`](../evidence/phase-d/d5hit-output.txt)). Rules applied are the **sealed** rules. Where a sealed rule is undefined or yields a wrong result, the step names the finding, and, where it differs, the outcome under the proposed correction.

Assertion verdicts:
- **PASS**: the sealed architecture gives the safe outcome.
- **FAIL → PD-…**: the sealed text gives an unsafe or undefined outcome; a finding is recorded.
- **N/A**: not applicable in this epoch.

## Baseline (shared by all epochs)

| Item | Value |
|---|---|
| Output A | mode 1920×1080, scale 1.0, 165 Hz, **Arrangement origin (640, 360)**, logical 1920×1080 |
| Output B | mode 3840×2160, scale 1.5, 144 Hz, **Arrangement origin (2560, 0)**, logical 2560×1440 (A and B adjacent at x = 2560) |
| Camera A (bound to A) | centre Q16(1650.3, 700.7). Zoom **1080/651 = 1.6589861751**, left from a viewport-fit of a 1157×651 element that the user then navigated away from (C5.1: fit ends, no restore), so k_A is **not** of the form n/120. |
| Camera B (bound to B) | centre Q16(2200.45, 900.15), zoom 1, so k_B = 1.5 |
| Element W | anchor **(1100, 420)** (its only stored datum). Window geometry 800×600 at RootSurfaceLocal (20, 20) (CSD shadow 20), root surface 840×640. |
| W tree (Canvas origins, front-to-back, INV-RENDER-2) | PS (1195,465) 30×30 (popup subsurface) · P (1190,460) 140×100 (popup; geometry.loc (10,10); configured at (100,50) relative to W's geometry) · S1a (1127,421) 30×20 (nested above S1) · S1 (1117,411) 101×53 (above root) · **W root (1080,400) 840×640**, input region = geometry rect minus hole (20,55)+(20×30) · S2 (1065,450) 60×40 (**below** root; reached through the hole) |
| Element U | anchor (1700, 800), 400×300, no CSD |
| Policies | sealed C14.5 provisional constants: H_cfg 3.0, r 1.25, σ 0.05, P 1 s; D_max 16384 |

Origins are produced by the single origin function: `root = anchor − geometry.loc`; subsurfaces `+ offsets`; popup `root + geometry.loc + pos − popup.geometry.loc`. ED-1 extracts them from Smithay's public state and they match.

---

## Epoch 1: geometry and mixed projection

**Views (C3.2 [1]):**

| | k | v (Q16, R7) | R | C | c_eff | Canvas viewport |
|---|---|---|---|---|---|---|
| A | 1.6589861751 | (1071.633331, 375.199997) | (960, 360) | (185, 25) | (1071.513885, 375.069443) | x[1071.5139, 2228.8472) y[375.0694, 1026.0694) |
| B | 1.5 | (920.449997, 180.149994) | (840, 120) | (121, 90) | (920.666672, 180.000000) | x[920.6667, 3480.6667) y[180.0, 1620.0) |

**Physical rects and membership** (INV-RENDER-1, INV-RENDER-3; identical to Smithay's presented geometry, ED-1):

| Surface | A: x[·) y[·) | on A | B: x[·) y[·) | on B |
|---|---|---|---|---|
| PS | [205,255) [149,199) | yes | [412,457) [428,473) | yes |
| P | [197,429) [141,307) | yes | [404,614) [420,570) | yes |
| S1a | [92,142) [76,109) | yes | [310,355) [362,392) | yes |
| S1 | [75,243) [60,148) | yes | [295,446) [347,426) | yes |
| W root | [14,1408) [41,1103) (bottom 23 px cut) | yes | [239,1499) [330,1290) | yes |
| S2 | [−11,89) [124,191) | yes | [217,307) [405,465) | yes |
| U | [1043,1706) [705,1203) | yes | [1169,1769) [930,1380) | yes |

- W is `Observed{A, B}`. Each surface gets `wl_surface.enter(A)` and `enter(B)` from Infynitae's own per-surface membership, popups included (W-SM-2 avoided).
- **Frame clock** = A (165 Hz > 144 Hz); presentation `sync_output` = A (INV-FRAME-2).

**Scale (INV-SCALE-1/2):**
- ρ_A = 1.6590, ρ_B = 1.5, so ρ = 1.6590.
- Ladder (outputs 1.0, 1.5; largest surface 840×640): [1.0, 1.2247, 1.5, 1.7321, 2.0, 2.4495, 3.0], H_max = 3.0.
- Target = smallest rung ≥ 1.6590/1.05 = 1.5800, so **h = 1.7321**: `preferred_scale 208/120`, `preferred_buffer_scale 2`.
- Compositor factors m = ρ_p/h: A 0.9578 and B 0.8660, both minify (downscale filter).
- Root buffer 1455×1109 px.

**Hit-tests (INV-HIT-1), pointer in OutputLogical:**

| Pointer | Arrangement | Pixel | Canvas q | Hit | Delivered (surface-local) |
|---|---|---|---|---|---|
| Free(B, (300.25, 400.6)) | (2860.25, 400.6) | (450, 600) | (1220.916672, 580.600006) | **W root** (PS, P, S1a, S1 do not cover; region contains) | (140.916672, 180.600006), clamp 0 |
| Free(B, (184.7333, 290.3)) | (2744.7333, 290.3) | (277, 435) | (1105.399994, 470.300003) | **S2 (below)**: root covers the pixel but the point is in its region **hole**, so it falls through | (40.399994, 20.300003) |
| Free(A, (296.6037, 208.7512)) (sub-pixel, zoomed) | (936.6037, 568.7512) | (296, 208) | (1250.300003, 500.899994) | **P** (popup before its parent's tree) | (60.300003, 40.899994) |
| Free(A, (8.7, 300.2)) | (648.7, 660.2) | (8, 300) | (1076.758057, 556.023331) | nothing (left of the root; S2 does not cover y 300) | — |

**Assertions:**

| # | Assertion | Verdict |
|---|---|---|
| E1-1 | Stage holds exactly one datum for W (anchor); every surface origin derives from it | PASS |
| E1-2 | Render edges = Smithay presented edges on both outputs, including k ∉ n/120 | PASS (ED-1, 7 configurations, 0 mismatches) |
| E1-3 | Hit-testing selects the presented surface, honours the region hole, and delivers exact coordinates | PASS (ED-1 all pixels; table above) |
| E1-4 | The Arrangement origin does not participate in projection or hit-testing | PASS (it only appears in the Arrangement column) |
| E1-5 | Membership per surface, own computation, popups included | PASS |
| E1-6 | Hint is bounded, a ladder rung, and emitted once; buffer integer scale ⌈h⌉ | PASS |
| E1-7 | Clock = highest-refresh observing output | PASS |
| E1-8 | Fractional camera handled only through [1]–[4] | PASS |
| E1-9 | The zoom on A is reachable in Foundation 01 only through fit-exit-by-navigation, and the sealed text says zoom ≠ 1 is static only | **FAIL → PD-F2-06** (see E2-7, E4-9) |
| E1-10 | Hint before W's first map (prior to this epoch) | **undefined → PD-F2-09**. The harness rule would give 1.5, then 1.7321 at map (one extra renegotiation). |

---

## Epoch 2: input and constraint overlap

**Set-up:**
- The pointer enters W on B at Free(B, (300.25, 400.6)). The client receives `wl_pointer.enter(W, 140.916672, 180.600006)`.
- W already has keyboard focus (an earlier click-to-focus, an explicit action).
- The client requests a **persistent** `zwp_locked_pointer_v1` on W's root (region = whole surface). The activation policy holds (focused ∧ inside region ∧ no pan acquisition), so the client receives **`locked`**.
- Authoritative pointer (sealed INV-COORD-5): `Locked(W, q_lock = Canvas (1220.916672, 580.600006))`. **No output is recorded** (PD-F2-03); the history says B. Surface-local equivalent: (140.916672, 180.600006).

**Physical input during the epoch:**

| Event | Owner (INV-INPUT-1) | Client receives |
|---|---|---|
| Δ accel (3.5, −1.25), unaccel (3.0, −1.0) | client path (no interaction) | `relative_motion(3.5, −1.25, 3.0, −1.0)`; **no** `wl_pointer.motion` (locked) |
| Δ (2.25, 0.5) / (2.0, 0.5) | client | `relative_motion(2.25, 0.5, 2.0, 0.5)` |
| Δ (−1.7, 0.75) / (−1.5, 0.6) | client | `relative_motion(−1.69921875, 0.75, −1.5, 0.6015625)`: the wl_fixed quantisation is external (X16) |
| KEY_W press / release | Client (K1, no binding) | `key` pressed/released, `modifiers` unchanged |

**Camera A pans continuously:**
- With the pointer locked on B, a pointer pan cannot drive A: a pan binding would act on the pointer's output, which `Locked` does not even record (PD-F2-03).
- The in-scope continuous source is a **shell `Navigate{A, Delta(+15, 0)}` stream**: 60 commands at 60 Hz. Each command is atomic (C7.2/C7.3).

| Command | v_A.x | R_A | Membership change |
|---|---|---|---|
| 4 | 1131.6333 | 1080 | S2 `leave(A)` |
| 6 | 1161.6333 | 1080 | S1a `leave(A)` |
| 10 | 1221.6333 | 1200 | S1 `leave(A)` |
| 11 | 1236.6333 | 1200 | PS `leave(A)` |
| 18 | 1341.6333 | 1320 | P `leave(A)` |
| 57 | 1926.6333 | 1920 | W root `leave(A)`: W is now `Observed{B}` |

- Lattice-reference switches during the stream: 8. W's root width on A alternates **1393/1394 px** (A-B1-1 jitter at zoom ≠ 1, PD-F2-06).
- **Scale:** at command 57, ρ = 1.5. The stream is atomic, so the decrease applies at once: **h 1.7321 → 1.5** (`180/120`).
  - If the stream oscillates across W's edge, the sealed rules renegotiate **40 times in 40 steps**, against 1 with gesture semantics (PD-F2-07).
- **Clock:** A → B (handover ≤ old + new period); `sync_output` B.
- **Configures to W: 0.** Pointer, relative or button events caused by the stream: **0**.
- **Lock:** unaffected (camera A is not the pointer's output): no `unlocked`.
- **Focus:** keyboard focus W (unchanged); pointer focus W (locked).
- End of epoch: camera A centre (2550.3, 700.7); W `Observed{B}`; h = 1.5.

**Assertions:**

| # | Assertion | Verdict |
|---|---|---|
| E2-1 | No leaked deltas: relative motion only from physical Δ; nothing from the camera stream | PASS |
| E2-2 | No stuck buttons or keys: KEY_W client-owned and released to W; no compositor-owned key involved | PASS |
| E2-3 | No stuck modifiers (K3) | PASS (none pressed; K3 untouched) |
| E2-4 | Zero pan-induced configures (INV-CAMERA-4) | PASS (0) |
| E2-5 | Lock continuity while another camera moves | PASS (no `unlocked`) |
| E2-6 | The authoritative pointer state identifies the pointer's output | **FAIL → PD-F2-03** (`Locked` has no output) |
| E2-7 | No relative jitter between elements while a zoomed camera moves | **FAIL → PD-F2-06** (1393/1394) |
| E2-8 | Bounded renegotiation for a continuous camera movement | PASS for this monotone stream (1). **FAIL for an oscillating stream** (40/40) **→ PD-F2-07** |
| E2-9 | Keyboard focus unaffected by camera motion (INV-INPUT-8) | PASS |
| E2-10 | Stationary-pointer reconciliation after a non-pan camera change: the stream does not touch B, so none is needed here; the rule's absence matters in E4 and E3 | N/A here (see PD-F2-04) |

---

## Epoch 3: fullscreen cross-camera collision

**Before:**
- Camera A at centre (2550.3, 700.7): view v = (1971.633331, 375.199997), R = (1920, 360), C = (86, 25).
- Only U is on A. W is on B only; h = 1.5; clock B.

**Request:** W (keyboard-focused) sends `set_fullscreen(wl_output A)`.
- Accepted (INV-PRES-1: focused).
- Output choice = the requested output A.
- Saved {800×600, [activated]} (the `activated` state is itself unspecified: PD-F2-17).
- `FullscreenState{W, A}`; `OutputPresentation(A) = Exclusive(W)`.
- Configure `[fullscreen]` 1920×1080. W is observed (B), so no kick is needed.

**Presentation view on A** (a view with zoom 1, PD-F1-09). When it starts is undefined (PD-F2-10); here it starts at acceptance:

| Phase | v | R | C | W root on A | other W surfaces on A |
|---|---|---|---|---|---|
| before the client resizes (800×600 centred, border fill) | (540, 180) | (480, 120) | (60, 60) | [540,1380) [220,860) | PS [655,685)[285,315); P [650,790)[280,380); S1a [587,617)[241,261); S1 [577,678)[231,284); S2 [525,585)[270,310) |
| after ack + commit 1920×1080, CSD shadow dropped (geometry.loc (0,0)) | (1100, 420) | (1080, 360) | (20, 60) | [0,1920) [0,1080) | PS [95,125)[45,75); P [90,230)[40,140); S1a [47,77)[21,41); S1 [37,138)[11,64); S2 [−15,45)[50,90) |

- **Membership:** every W surface gets `enter(A)`. U gets `leave(A)` (only W's tree is presented).
- **Stage:** W.anchor **(1100, 420) unchanged**. The root origin moves (1080,400) → (1100,420) because geometry.loc changed (client-owned). The popup origin stays (1190, 460), since `root + geometry.loc` = anchor.
- **On B** (camera B unchanged): W is drawn at its real 1920×1080 size: root [269,3149) × [360,1980). INV-PRES-1: other cameras observe the fullscreen-sized W.
- **Camera A untouched:** still centre (2550.3, 700.7) and zoom 1080/651. Fullscreen has no camera path (C1.5).

**Scale (CP2-Q1 consistency):**
- ρ_A(presentation, zoom 1) = 1.0 and ρ_B = 1.5, so ρ = 1.5.
- H_max = min(3.0, 16384/1920) = 3.0.
- **h stays 1.5: no fullscreen override.**
- Buffer 2880×1620 = 17.8 MB. Factors: A 0.667 (minify), B 1.000.
- The fullscreen-enter stimulus and the client's resize commit (an SC-E client response) do not change h.

**Clock:**
- Observing {A 165 Hz (presentation), B 144 Hz}, so the clock is A and presentation feedback `sync_output` is A.
- Callbacks complete at 165/s, never at 165 + 144.

**Focus:** keyboard W (unchanged by the request itself). The pointer is still `Locked` on W via B.

**Lock under the sealed representation:**
- q_lock stays at Canvas (1220.916672, 580.600006). Relative to the moved root it is now **(120.916672, 160.600006)** instead of (140.916672, 180.600006): it **drifted 20, 20 px** with no event.
- Surface-relative representation (proposal) keeps (140.916672, 180.600006), which is Canvas (1240.916672, 600.600006), still observable on B.

**Exit:**
- The client sends `unset_fullscreen`, and gets a configure `[]` 800×600 (the saved size).
- The client commits with CSD; the root is back at (1080, 400).
- A's presentation returns to camera A's view, which is **exactly as before**. W is not in it, so every W surface gets `leave(A)` and U gets `enter(A)`.
- ρ = 1.5, so h is unchanged. Clock: B.

**Assertions:**

| # | Assertion | Verdict |
|---|---|---|
| E3-1 | Fullscreen never touches cameras or anchors | PASS |
| E3-2 | Other cameras observe W at its real size | PASS |
| E3-3 | CP2-Q1: no fullscreen scale override; ρ from the presentation (zoom 1) and B | PASS (h = 1.5) |
| E3-4 | Clock = highest-refresh observing output, counting the presentation | PASS (A) |
| E3-5 | Only W's tree on A; U leaves and re-enters on exit | PASS |
| E3-6 | Presentation start/end ordering relative to configure/ack/commit | **undefined → PD-F2-10** |
| E3-7 | The lock point stays on the same surface point through client geometry changes | **FAIL → PD-F2-03** (20 px drift) |
| E3-8 | Pointer reconciliation when a presentation starts or ends on the pointer's output | N/A here (the pointer is on B); undefined in general → PD-F2-04 |
| E3-9 | The `activated` state saved/restored by fullscreen is defined | **undefined → PD-F2-17** |
| E3-10 | Zero camera-caused configures (the two configures are client-requested) | PASS |

---

## Epoch 4: teardown and hotplug

**Pre-step:**
- The shell sends `Navigate{A, To(centre (1650.3, 700.7))}` (atomic). Camera A is back at its epoch-1 view.
- W surfaces get `enter(A)`. ρ = 1.6590, so **h 1.5 → 1.7321** (an increase, applied at once).

**State at removal:**
- `Locked(W)` via B (persistent lock, active).
- BTN_LEFT held: client-owned, so Smithay's implicit click grab is on W.
- KEY_W held (client-owned).
- Popup P open (no grab).
- Keyboard focus W.
- Clock A.

**Remove output B:**

| Step | Rule | Result |
|---|---|---|
| 1 | INV-OUTPUT-1, B7.2: leave before the global is destroyed | `wl_surface.leave(B)` for PS, P, S1a, S1, W, S2 and U, **then** the `wl_output` B global is destroyed |
| 2 | Ladder (S2) without B | [1.0, 1.1892, 1.4142, 1.6818, 2.0, 2.4495, 3.0]. The current h = 1.7321 is **not a rung** (PD-F2-08). ρ = 1.6590 is unchanged, and the target on the new ladder is **1.6818**. No gesture is active and output removal is an SC-E stimulus, so **h 1.7321 → 1.6818** (`202/120`, buffer scale 2). One renegotiation caused only by the ladder change (PD-F0-08). |
| 3 | AUD-4 / INV-INPUT-5: deactivate and relocate | Lock point on B p = (300.25, 400.600006), so Arrangement (2860.25, 400.6). Arrangement-nearest on A (half-open, PD-F1-05) is (2559.996094, 400.6), giving `Free(A, (1919.996094, 40.600006))` at Canvas (2228.8449, 399.5422). Core hit-test: **nothing**. |
| 3a | W-SM-4a: explicit `deactivate` only when no pointer-focus change follows | Core predicts a focus change (W → none), so it does **not** deactivate explicitly and relies on Smithay's `leave()`. **But the implicit click grab keeps the delivered focus on W**, so `leave()` never runs and the **lock stays active at the client while core records it as ended**. **FAIL → PD-F2-02.** With the correction (decide on the delivered focus), the explicit deactivate runs and the client gets exactly one `unlocked`. |
| 3b | Motion through the grab | The client receives `wl_pointer.motion` at surface-local (1148.8449, −0.4578), outside W. That is legal under an implicit grab; it is the true position. |
| 4 | K8 | KEY_W stays client-owned and logically down at W. No keyboard event. |
| 5 | B7.2 | Camera B: unbound, kept, persisted under `OutputKey(B)`. View(B), ProjectionFrame(B) and damage tracker(B) are dropped. |
| 6 | D-B6-NO-PAN-REPOSITION | P is not repositioned (its anchor output is gone). It is still visible on A. |
| 7 | INV-FRAME-2 | W is observed on A only; the clock stays A. No pending-feedback change. |
| 8 | C5.2 | No fullscreen active, so nothing to drop. |

**Then:**
- The user releases BTN_LEFT. It is delivered to W through the grab, and the grab ends. The next routed event re-hits nothing, so W gets `wl_pointer.leave` (PD-F2-04 would do it at batch end).
- The user releases KEY_W; it is delivered to W.
- The user presses the pan binding on A. No client-owned button is held, so acquisition succeeds: `CameraPan{output A, start centre (1650.3, 700.7)}`, KeepFocus (pointer focus none).
- 120 motion events of Δ = (−15.3, 0.2), so Σ = (−1836, 24) in f64, quantised once per update:

| Formula | W fully unobserved at event | v_A.x then | lattice switches | W width on A |
|---|---|---|---|---|
| **sealed** `camera = start − Q(Σ)` | **56** | 1928.4333 | 16 | 1393/1394 |
| zoom-aware `start − Q(Σ/zoom)` (proposal) | 92 | 1920.1033 | 9 | 1393/1394 |

- **During the pan:**
  - each W surface gets `leave(A)` as it leaves the viewport;
  - **no configure**; no motion, relative or button event to any client;
  - h is held (no decrease during a gesture).
- **W becomes `Unobserved`:**
  - callbacks are withheld;
  - `FrameLiveness.last_completion` = the last A vblank before the transition;
  - the fallback is due at `last_completion + 1 s`, with its timer armed because W (a game) has a pending callback (requires PD-F1-06);
  - **kick:** every key delivered to W (W keeps keyboard focus, INV-INPUT-8) first completes its withheld callbacks once. The rate is bounded by the input rate (PD-F0-05).
- **Pan end** (initiator release):
  - `Free(A, (1919.996094, 40.600006))` is unchanged;
  - re-hit at Canvas (3335.4143, 385.0756) [zoom-aware run] finds nothing;
  - W is unobserved, so the deferred decrease does not apply and **h is held at 1.6818**.

**Final state:**

| Item | Value |
|---|---|
| Stage | W.anchor **(1100, 420)**, U.anchor (1700, 800): **unchanged across all four epochs** |
| Outputs | A only |
| Camera A | bound to A; zoom 1080/651; centre after the pan (zoom-aware: x ≈ 2757.0) |
| Camera B | unbound, persisted (2200.45, 900.15), zoom 1 |
| W | Mapped, `Unobserved`; h = 1.6818 held; keyboard focus W; pointer focus none; lock inactive (persistent object remains, may re-activate per policy); P mapped and unobserved; callbacks withheld with fallback P = 1 s and kick-on-input; no FullscreenState, no ViewportFit |
| Configures to W after the initial one | 2 (fullscreen enter and exit, client-requested); **0 camera-caused** |
| Hint renegotiations in E1–E4 | E2: 1; E3: 0; E4: 2 (navigate-in increase; ladder-change decrease). Bounded. |

**Assertions:**

| # | Assertion | Verdict |
|---|---|---|
| E4-1 | `leave` for every surface before the output global is destroyed | PASS |
| E4-2 | Camera survives output removal, unbound and persisted | PASS |
| E4-3 | Lock reconciliation on output removal: exactly one `unlocked`, and core agrees with the client | **FAIL → PD-F2-02** (implicit grab defeats the W-SM-4a prediction) |
| E4-4 | Pointer relocated to the Arrangement-nearest point on a remaining output | PASS (with the half-open clamp, PD-F1-05) |
| E4-5 | No stuck button or key: BTN_LEFT and KEY_W releases delivered to W | PASS |
| E4-6 | Scale re-evaluated once per batch; result a rung of the new ladder | PASS after evaluation; **the held value is off-ladder until then → PD-F2-08** |
| E4-7 | Frame-callback transition Observed → Unobserved with kick and fallback | PASS (requires PD-F1-06 to arm the timer only when needed) |
| E4-8 | Keyboard focus unchanged by output removal and by the camera | PASS |
| E4-9 | Content follows the hand during a zoomed pan | **FAIL → PD-F2-06** (sealed formula moves content 1.659× the hand: 56 vs 92 events) |
| E4-10 | Popup not repositioned by output removal or camera motion | PASS |
| E4-11 | Derived state rebuilt, not patched: View(B), ProjectionFrame(B), tracker(B) dropped; membership recomputed | PASS |
| E4-12 | Orphaned-interaction semantics if the pan's output were removed mid-pan | **undefined → PD-F2-11** (not triggered here) |

---

## Cross-epoch summary

| Category | PASS | FAIL / undefined → finding |
|---|---|---|
| Stage authority, anchors, origin function, popup origin | all | — |
| Render/hit/membership agreement incl. k ∉ n/120 | all (ED-1) | — |
| Zero camera-caused configures; no pan-originated input | all | — |
| Keys and modifiers | all | — |
| CP2-Q1 fullscreen scale | PASS | — |
| Lock state and reconciliation | — | PD-F2-02 (A), PD-F2-03, PD-F2-04 |
| Zoomed panning (jitter, speed) | — | PD-F2-06 |
| Renegotiation under non-gesture camera streams | — | PD-F2-07 |
| Ladder / hint definition | — | PD-F2-08, PD-F2-09 |
| Fullscreen timing; `activated` | — | PD-F2-10, PD-F2-17 |

All four epochs were traced in full with the sealed rules. No step was summarised.
