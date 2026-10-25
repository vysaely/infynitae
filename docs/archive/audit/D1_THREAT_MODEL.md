# D1 — Threat and falsification model

Baseline: Phase C seal `615b0ed`. Scope: threats that could **falsify** the architecture (an invariant false, an authority boundary crossable, a rule unimplementable), not a feature checklist. Priority **P1** marks a threat that can falsify an `INV-*` or an authority boundary; **P2** marks a correctness or quality threat to one behaviour.

Columns:
- **Expected safe behaviour**: what the sealed architecture claims.
- **Evidence**: existing EC/EB/ED evidence.
- **Tier**: where it is (or must be) verified (C11).
- **Residual**: risk remaining after the disposition.
- **Disposition**: one of:
  - *holds*: analysis or evidence supports the claim;
  - *finding*: links to the register in [README.md](README.md);
  - *deferred*: tier destination exists.

## 1. Authority

| ID | P | Threat | INV | Trigger | Expected safe behaviour | Evidence | Tier | OB | Residual | Disposition |
|---|---|---|---|---|---|---|---|---|---|---|
| TA-1 | P1 | An untrusted client moves a camera | CAMERA-1, CAMERA-4 | any client request | no protocol path reaches `navigation` | EC-1, EC-15 (probes) | T0 | OB-19 | **found path**: a fitted element's commit drives re-fit (C5.1) | **finding PD-F2-01 (A)** |
| TA-2 | P1 | Time-driven camera change without a port | CAMERA-1, SHELL-1 | animated navigation tick (C7.3, AUD-6) | every camera mutation is inside `physical_input`/`shell_command` | none (animation not designed) | T0 | OB-19 | tick source has no port | **finding PD-F2-01 (A)** |
| TA-3 | P1 | A second writer of keyboard focus | INPUT-8 | popup grab active + FocusModel focus change; grab end | FocusModel is the only decider; Smithay applies | ED-3 (source) | T1 | OB-18 | Smithay drops `set_focus`, restores focus itself | **finding PD-F2-02 (A)** |
| TA-4 | P1 | Core mispredicts delivered pointer focus under client grabs → wrong W-SM-4a decision | INPUT-5, OUTPUT-1 | lock + implicit grab + output removal (D5 E4) | exactly one `unlocked`; core and Smithay agree on the lock state | X-8 C4 (no grab case); ED-2 epoch 4 | T1 | OB-3 | lock left active at the client | **finding PD-F2-02 (A)** |
| TA-5 | P1 | Client becomes a Stage writer | STAGE-1, XWAYLAND-1 | X11 move, `set_window_geometry` x/y, popup reposition | anchors change only via `stage::place` | EC-12 (X11 move ignored); B2.3 | T1 | — | none known | holds |
| TA-6 | P1 | Port leaks into protocol state | CAMERA-1, ARCH-1 | future refactor stores a port in `State` | compile error E0277 | EC-15 | T0 | OB-19 | deliberate composition-root edit (C7.4) | holds (accepted residual) |
| TA-7 | P2 | Shell becomes an arbitrary mutator | SHELL-1/2 | malformed or stale commands | semantic, validated, single-writer paths; `StaleElement` | C7 design | T1 | — | shell delta streams bypass gesture semantics (scale churn) | holds for authority; churn → PD-F2-07 |
| TA-8 | P2 | Activation steals focus or navigates | INPUT-9 | `xdg_activation_v1` with forged or old token | provenance check; grant/attention/ignore; no camera | B10.7; W-SM-8 | T1 | OB-27 | token provenance rules untested | deferred (T1) |
| TA-9 | P2 | Fullscreen seizes an output while the user is elsewhere | PRES-1 | unfocused `set_fullscreen` | deferred until focus | C5.2 | T3 | OB-22 | deferred request never expires or cancels | **finding PD-F2-10** |
| TA-10 | P2 | Derived value written back as authority | STAGE-6 | lock point, presentation view, fit zoom | no write-back | C2.1 guards | T0 | — | Canvas lock point is effectively frozen geometry that drifts from its surface | **finding PD-F2-03** |

## 2. Temporal ordering

| ID | P | Threat | INV | Trigger | Expected safe behaviour | Evidence | Tier | OB | Residual | Disposition |
|---|---|---|---|---|---|---|---|---|---|---|
| TT-1 | P1 | Destruction order on client death breaks lifecycle | STAGE-4 | client crash | idempotent, order-independent destroy | EB-5; EC-12 (satellite kill) | T1 | — | none | holds |
| TT-2 | P1 | Two scale evaluations in one batch → double renegotiation | SCALE-5 | gesture end + output removal together | one evaluation per batch | AUD-25 | T0 | — | — | holds |
| TT-3 | P1 | `leave` after the output global is destroyed | RENDER-3, OUTPUT-1 | hotplug | leave first, then destroy global | B7.2 | T1 | OB-3 | — | holds (D5 E4 step 1) |
| TT-4 | P1 | Pointer state stale after a non-pan view change | INPUT-5, CAMERA-4 | shell `Navigate`, fullscreen start, output scale change | true position re-established | B11.4 (pan only) | T1 | OB-3, OB-14 | no trigger defined | **finding PD-F2-04** |
| TT-5 | P2 | Kick ordering: callback completion after the request | FRAME-3 | ping/close/configure/input to unobserved client | complete withheld callbacks **before** the request | EB-9 | T1 | OB-16 | — | holds |
| TT-6 | P2 | Fallback double-fires after a kick | FRAME-3 | kick shortly before fallback | period measured from last completion | AUD-24 | T1 | — | — | holds |
| TT-7 | P2 | Fullscreen presentation start/end vs configure/ack | PRES-1 | fullscreen enter/exit | defined order | C5.2 | T1 | OB-22 | undefined | **finding PD-F2-10** |
| TT-8 | P2 | W-SM-4a: explicit deactivate before focus change → duplicate `unlocked` | INPUT-5 | pan end with lock point off-screen | one `unlocked` | X-8 C4 | T1 | — | grab-aware prediction (TA-4) | holds for ungrabbed; PD-F2-02 |
| TT-9 | P2 | Shell command concurrent with client destruction | SHELL-2 | race | calloop serialises; `StaleElement` | AUD-21 | T1 | — | — | holds |

## 3. Coordinates

| ID | P | Threat | INV | Trigger | Expected safe behaviour | Evidence | Tier | OB | Residual | Disposition |
|---|---|---|---|---|---|---|---|---|---|---|
| TC-1 | P1 | Render ≠ hit at an effective scale not of the form n/120 | HIT-1, RENDER-1 | camera zoom from viewport-fit | 0 mismatches | EC-20 (n/120 only); **ED-1** (k = 1080/651: 0 mismatches) | T1 | OB-14 | rotated transforms untested | holds (extended) |
| TC-2 | P1 | Relative jitter while panning at zoom ≠ 1 | RENDER-1, CAMERA-5 | pan after a fit (fit ends without restoring zoom) | the architecture claims zoom ≠ 1 is static only | **ED-1** (W 1394→1393 px), ED-2 F1 | T1 | OB-24 | the claim is false | **finding PD-F2-06** |
| TC-3 | P1 | Pan speed at zoom ≠ 1 | COORD-3 | pan with zoom ≠ 1 | content follows the hand | ED-2 epoch 4 (56 vs 92 events) | T0 | OB-21 | formula has no zoom term | **finding PD-F2-06** |
| TC-4 | P1 | Q16/f64 range of derived values and i32 conversion | COORD-1, COORD-2 | far elements; fit of huge windows | no overflow, no panic | B1 E1–E9; ED-2 F5 | T0 | OB-6 | ordering not stated | **PD-F1-01, PD-F1-02** |
| TC-5 | P1 | Unlisted rounding point | COORD-2 | popup constraint target with fractional camera | rounding only at R1–R7 | ED-2 F7 | T0 | OB-6 | R8 needed | **finding PD-F2-12** |
| TC-6 | P2 | Element drag across independent cameras | STAGE-1, COORD-3 | `WindowMove` crossing outputs | defined anchor update | B1.3-7 | T1 | — | undefined | **finding PD-F2-13** |
| TC-7 | P2 | Lock point detaches from its surface | COORD-5, INPUT-5 | geometry.loc change, element move | lock stays on the surface point | ED-2 epoch 3 (20 px drift) | T1 | OB-20 | Canvas representation | **finding PD-F2-03** |
| TC-8 | P2 | Zoomed render with the wrong damage-tracker scale | RENDER-1 | `OutputDamageTracker::from_output` | elements sized by k_O | ED-4 | T1/T2 | OB-6 | API detail | **PD-F1-03** |
| TC-9 | P2 | Arrangement origin leaking into projection | COORD-4 | non-zero Arrangement origins | Arrangement only for crossing | ED-2 (D5 uses non-zero origins; projection independent of them) | T0 | — | — | holds |

## 4. Protocol behaviour

| ID | P | Threat | INV | Trigger | Expected safe behaviour | Evidence | Tier | OB | Residual | Disposition |
|---|---|---|---|---|---|---|---|---|---|---|
| TP-1 | P1 | Pan visible to clients as configure or input | CAMERA-4 | any pan, shell stream | 0 configures, 0 pan-originated input | SC1, SC4, X-8; ED-2 epoch 2 (0) | T1 | — | — | holds |
| TP-2 | P1 | Stuck key or modifier across focus changes | INPUT-6, INPUT-7 | binding + focus change + destruction | logical state = physically held client-owned keys | X-10 + NEG, EC-21 | T1/T3 | OB-10 | popup-grab focus divergence | holds for the K-rules; PD-F2-02 |
| TP-3 | P1 | Confinement violated by Arrangement crossing | INPUT-5 | confined pointer crossing outputs | pointer stays in region | none | T1 | OB-14 | enforcement undefined | **finding PD-F2-05** |
| TP-4 | P2 | `suspended` derived from camera visibility via an output-power reason | FRAME-6 | unobserved element; output off | never camera-caused | SC4 | T1 | — | reason definition | **finding PD-F2-14** |
| TP-5 | P2 | Never-activated oneshot lock destroyed on leave | INPUT-5 | W-SM-4b | lock survives | X-12 (defect reproduced) | UP | OB-11 | Smithay defect | deferred (UP) |
| TP-6 | P2 | Popup grab chain panics (`grab_popup` assert) | FAIL-1 | misbehaving client | protocol error, compositor survives | W-SM-7 (source) | T1 | OB-18 | — | deferred (T1) |
| TP-7 | P2 | Satellite X11 menus misplaced | XWAYLAND-1 | override-redirect windows | attached popups | C6-A1 (assumption) | T1/T3 | OB-9 | untested | deferred |

## 5. Resource and liveness

| ID | P | Threat | INV | Trigger | Expected safe behaviour | Evidence | Tier | OB | Residual | Disposition |
|---|---|---|---|---|---|---|---|---|---|---|
| TR-1 | P1 | Scale renegotiation storm via non-gesture camera paths | SCALE-4 | wheel-bound pan, shell delta stream, zoom-at-cursor | ≤ rungs crossed + 1 per interaction | EC-22 (zoom only); **ED-2 F2: 40/40** | T0/T1 | OB-23 | sealed contract evaded | **finding PD-F2-07** |
| TR-2 | P1 | Unrenderable buffer from an output scale above the device bound | SCALE-2 | 6000-px window on a 3.0 output | h ≤ D_max/size | ED-2 F3 | T1 | OB-13 | invariant text contradictory | **finding PD-F2-08** |
| TR-3 | P1 | Starvation of a FIFO-blocked client off-screen | FRAME-3 | unobserved Vulkan client | kick + fallback P | EB-9 | T3/T5 | OB-5, OB-16 | games unmeasured | deferred (T5) |
| TR-4 | P2 | Wakeups while idle | IDLE-1 | fallback timer armed with no pending callback | armed only when needed | C14.2 | T4 | — | core lacks the "callback pending" datum | **PD-F1-06** |
| TR-5 | P2 | Off-ladder hint after hotplug; renegotiation without ρ change | SCALE-2, SCALE-4 | output add/remove | bounded | ED-2 F4, D5 E4 | T0 | — | — | **PD-F2-08**, PD-F0-08 |
| TR-6 | P2 | Extra renegotiation at map | SCALE-5 | first map | one hint before the initial configure | ED-5, ED-2 F8 | T1 | — | rule undefined | **finding PD-F2-09** |
| TR-7 | P2 | Hot-path allocation | ALLOC-1 | pan/route/key | 0 allocations steady state | C14.3 design | T0 | — | — | deferred (T0) |

## 6. Recovery

| ID | P | Threat | INV | Trigger | Expected safe behaviour | Evidence | Tier | OB | Residual | Disposition |
|---|---|---|---|---|---|---|---|---|---|---|
| TF-1 | P1 | Output removal during constraint + grab | OUTPUT-1, INPUT-5 | D5 epoch 4 | leave before destroy; one `unlocked`; pointer relocated | AUD-4; ED-2 | T1 | OB-3 | grab interaction | **PD-F2-02**, PD-F2-03 |
| TF-2 | P1 | Recovery patches derived state instead of rebuilding | FAIL-1 | renderer reset | rebuild from authority | C8 | T1 | — | `D_max` not re-queried | **PD-F1-08** |
| TF-3 | P2 | Orphaned pan keeps mutating an unbound camera | OUTPUT-1, CAMERA-3 | output removed mid-pan | defined | C4.6 | T1 | OB-3 | undefined | **finding PD-F2-11** |
| TF-4 | P2 | Satellite restart breaks `DISPLAY` for running children | XWAYLAND-1 | satellite crash | children reconnect to the same display | EC-12 (death only) | T1 | OB-9 | undefined | **finding PD-F2-16** |
| TF-5 | P2 | Shell restart loses truth | SHELL-2 | shell crash | Snapshot + Subscribe | C7.3 | T1 | — | attention lifecycle undefined | **finding PD-F2-15** |
| TF-6 | P2 | Corrupt persistence partially applied | FAIL-1 | load failure | all or nothing | C8 | Level D | — | format deferred | deferred (Level D) |

## Result

48 threats: 23 P1, 25 P2.

**P1 (can falsify the architecture):**
- **Hold:** 8 (TA-5, TA-6, TT-1, TT-2, TT-3, TC-1, TP-1, and TP-2 for the K-rules).
- **Falsify a sealed claim or expose an unimplementable rule:** 12 (TA-1 … TA-4, TT-4, TC-2, TC-3, TC-5, TP-3, TR-1, TR-2, TF-1). All are D-F2 findings in the register; two are Level A.
- **D-F1 refinements:** 2 (TC-4, TF-2).
- **Deferred to a mapped tier:** 1 (TR-3, T5).

**P2:**
- **Hold:** 5 (TA-7 authority part, TT-5, TT-6, TT-9, TC-9).
- **Findings:** 14 (TA-9, TA-10, TT-7, TT-8, TC-6, TC-7, TC-8, TP-4, TR-4, TR-5, TR-6, TF-3, TF-4, TF-5). Several map to the same register entry.
- **Deferred to mapped tiers:** 6 (TA-8, TP-5, TP-6, TP-7, TR-7, TF-6).

Every threat with a finding links to a register entry; none was absorbed silently.
