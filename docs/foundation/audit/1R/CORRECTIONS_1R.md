# Phase D checkpoint 1R — corrections and dispositions

Status: **corrective checkpoint 1R complete; UNCOMMITTED, for review.**
- Falsification baseline (immutable): `e8ec03a2ab36f315ae025723ec666b9ad278ac83`.
- Sealed architecture: `615b0ed`.

The 17 PD-F2 findings recorded in that baseline (FOUNDATION_BLOCKERS.md § Phase D Architectural Findings) are **preserved verbatim**. This document disposes of each one with the chain:

`sealed rule → counterexample → PD-F2 finding → replacement rule → affected INV-* → affected ADR → required evidence`

Companion artifacts:
- [INVARIANT_DIFF_1R.md](INVARIANT_DIFF_1R.md): wording diff with provenance; normative through FOUNDATION_ARCHITECTURE §C12.R1;
- [STATE_MACHINES_1R.md](STATE_MACHINES_1R.md);
- [MATRICES_1R.md](MATRICES_1R.md);
- [CONTRADICTION_AUDIT_1R.md](CONTRADICTION_AUDIT_1R.md).

## Evidence (Phase D 1R)

| ID | What | Class / tier | Result | Where |
|---|---|---|---|---|
| ER-1 | **xpopup_focus**: Smithay `grab_popup` + `PopupKeyboardGrab`/`PopupPointerGrab`; core changes ActiveElementFocus while a popup grab is active. 10 scenarios with checks F-1…F-6 (core = Smithay = client focus, no protocol-order violation, a single transition, `popup_done`, no panic, no stuck key). | EXPERIMENT, T1-harness | NEG-1 (sealed naive `set_focus`) **diverges**: core W2, Smithay and client on P. NEG-2 (pointer grab unset first) gives a transient root focus (2 enters). OBS-1 (nested grab replacing the pointer grab) **loses keyboard routing** to the root (new **W-SM-14**). The ORDERED preemption passes PF-1…PF-7 (focus to other/none, parent destroyed, popup destroyed in flight or first, nested with W-SM-14 workaround, key held). **All as expected.** | `b8-harness/src/bin/xpopup_focus.rs`; [`xpopup-focus-output.txt`](../../evidence/phase-d/xpopup-focus-output.txt) |
| ER-2 | **xlock_grab**: active lock + client implicit grab + interaction-output loss (D5 epoch 4 at protocol level). | EXPERIMENT, T1-harness | SEALED (persistent and oneshot): core and Smithay **diverge**, plus an absolute motion delivered while locked. EXPLICIT/persistent: **duplicate `unlocked`** (W-SM-4a). EXPLICIT/oneshot: pass. **DEFER (1R): pass for both lifetimes**: agreement at every step, exactly one `unlocked`, no phantom motion. **All as expected.** | `bin/xlock_grab.rs`; [`xlock-grab-output.txt`](../../evidence/phase-d/xlock-grab-output.txt) |
| ER-3 | **xmaximize**: maximize and fullscreen answer policy. | EXPERIMENT, T1-harness | Maximize ON: set and unset each answered by 1 configure without `maximized`; anchor and cameras unchanged. Fullscreen ON: unfocused request declined with 1 configure; focused request granted (`[fullscreen]` 1920×1080); unset answered. **Negative controls** (record-only / sealed deferral): **0 answers**. All as expected. | `bin/xmaximize.rs`; [`xmaximize-output.txt`](../../evidence/phase-d/xmaximize-output.txt) |
| ER-4 | **Authority spike, 1R.** `Authorized` proof minted only in the port-carrying entry module; one-shot `FitViewport`. Probe **2d**: protocol re-fit fails with **E0624** (`mint` private). Probe **2e**: a sticky fit keeping a proof fails with **E0277**. 5 behaviour tests, including 200 adversarial resizes after a fit with 0 camera changes, and a stale-element fit that mutates nothing. | EXPERIMENT (compiler, T0) | **PASS** (all old probes still fail for their expected reasons) | `experiments/c1-authority-spike/` (`verify.sh`); [`authority-spike-1r-verify.txt`](../../evidence/phase-d/authority-spike-1r-verify.txt) |
| ER-5 | **d5_model_1r**: R1 pan algebra; R2 ScaleInteractionEpoch with negative control and timeout sensitivity; R3 one-shot fit; R4 D5 deltas; R5 confinement with negative control. | MODEL | R1: \|content − hand\| ≤ 0.9 px at every zoom (sealed: up to 1110 px). R2: 40 → 1 for discrete pan, zoom-at-cursor and shell stream; the bound holds in all 7 cases. R3: adversarial client 60 → 1 camera mutation. R5: 0 violations (negative control: 7,382 out-of-region positions, 87 jumps). | `experiments/phase-d/d5_model_1r.py`; [`d5-model-1r-output.txt`](../../evidence/phase-d/d5-model-1r-output.txt) |
| ER-6 | xwayland-satellite 0.8.3 `--test-listenfd-support` exits 0 (the binary documents "return 0 if supports -listenfd"). Run with no display variables and a private runtime directory. | EXPERIMENT (local binary) | `-listenfd` supported | this table |
| ER-7 | Regression re-run of d5hit, xhit, xsnap, x10, x8, x12, x6, x9 and scenarios after the additive harness changes; d5_model.py re-run | EXPERIMENT | All reproduce their recorded evidence; differences are header lines only. d5_model output is bit-identical. | [`1r-regression-summary.txt`](../../evidence/phase-d/1r-regression-summary.txt) |

**New Smithay quirks (adapter-local; catalogue continues W-SM-1…13):**

| ID | Quirk | Infynitae response | Evidence |
|---|---|---|---|
| W-SM-14 | Installing a nested popup grab with the conventional pattern (keyboard grab, then pointer grab) replaces the installed `PopupPointerGrab`. Its `unset()` tears the keyboard grab down (`grab.rs:697-699` → `unset_keyboard_grab`) and refocuses the root, so keys go to the toplevel instead of the nested popup. | For a nested grab, keep the installed popup pointer grab (it shares the grab chain); replace only the keyboard grab | ER-1 OBS-1 / PF-6 |
| W-SM-15 | Tearing a popup grab down pointer-first runs `PopupPointerGrab::unset` → `set_focus(root)` before the intended target: a transient leave/enter on the root | Fixed preemption order (INV-INPUT-10) | ER-1 NEG-2 / PF-1 |

## Severity re-evaluation and dispositions

Disposition classes:
- **A**: architecture correction required before D7 (done in 1R);
- **B**: resolved by another 1R correction;
- **C**: implementation obligation, architecture sufficient;
- **D**: deferred physical validation;
- **E**: upstream/dependency.

Level-A rigor was applied to any finding that creates competing authoritative writers, Core/protocol divergence, client-controlled camera mutation, or undefined mandatory protocol behaviour.

| Finding | Baseline level | Re-evaluated | Why | Disposition | Status |
|---|---|---|---|---|---|
| PD-F2-01 | A | **A** | client-controlled camera mutation | A | **resolved** |
| PD-F2-02 | A | **A** | competing writers; Core/protocol divergence | A | **resolved** |
| PD-F2-03 | B | **A** | Core/protocol divergence (ER-2) | A | **resolved** |
| PD-F2-04 | B | **A** | Core/protocol divergence (stale active lock with no reconciliation) | A | **resolved** |
| PD-F2-05 | B | **A** | undefined mandatory protocol behaviour (confinement must hold) | A | **resolved** |
| PD-F2-06 | B | B | algebra: wrong formula, no authority issue; jitter: visual | A (algebra) + D (jitter) | algebra **resolved**; jitter → OB-24 (rescoped), visual T3/T4 |
| PD-F2-07 | B | B | churn contract evaded; no authority issue | A | **resolved** |
| PD-F2-08 | C | C | contradictory invariant text | A (text) | **resolved** |
| PD-F2-09 | C | C | undefined, not authority | A (rule) | **resolved** |
| PD-F2-10 | C | **A** | **undefined mandatory protocol behaviour**: `set_fullscreen` must be answered (found in 1R; ER-3 negative control: 0 answers) | A | **resolved** |
| PD-F2-11 | C | C | — | B (by PD-F2-01: fit is no longer a mode) + one rule (orphaned pan) | **resolved** |
| PD-F2-12 | C | C | invariant text (rounding set) | A | **resolved** |
| PD-F2-13 | C | C | interactive move UX is deferred (C15) | C | obligation **OB-34** |
| PD-F2-14 | C | C | invariant text; reason not in Foundation 01 | A (text) | **resolved** |
| PD-F2-15 | C | C | — | C | obligation **OB-35** |
| PD-F2-16 | C | C | dependency capability (`-listenfd`) now verified (ER-6) | E (verified) + C | obligation **OB-33** |
| PD-F2-17 | B | **A** | undefined mandatory protocol behaviour (maximize answer) | A | **resolved** |

**Level-A findings after 1R:** none open. Six findings were raised to Level A by the rigor rule: PD-F2-03, -04, -05, -10, -17 (PD-F2-01 and -02 were already Level A). All are resolved, with experiments.

---

## Correction chains

### PD-F2-01: client → camera authority severed
- **Sealed rule:**
  - C5.1 "W changes size while fitted: the zoom is re-fitted";
  - C7.3/AUD-6 animated navigation "continues to its end";
  - INV-CAMERA-1 port-gated entry.
- **Counterexample:** a fitted element's commit requires a camera mutation from a port-less protocol path. Animation ticks have no port.
- **Replacement:**
  - Viewport-fit is a one-shot authorized action: snapshot, compute, one mutation, terminate.
  - No client event ever re-fits.
  - Each camera mutation consumes a fresh `Authorized` proof minted only inside the entry module. A proof cannot be retained (it is `!Send`; protocol state is asserted `Send`).
  - Animated navigation is **not** in Foundation 01. Shell `Navigate` commands are atomic; C7.3's "long-running effect" and AUD-6's continuation are superseded. A later animation feature must drive each step through a port-carrying entry.
- **INV:** CAMERA-1, CAMERA-5 (amended).
- **ADR:** ADR-08, ADR-04, ADR-09, ADR-11.
- **Evidence:** ER-4 (probes 2d/2e, tests), ER-5 R3 (feedback re-run: one camera mutation per authorized fit for every client class).
- **Mechanical check for Phase F:** OB-19 gains probes 2d and 2e, plus a source rule: `Authorized::mint` appears only in the entry module.

### PD-F2-02: desktop focus vs popup protocol routing
- **Sealed rule:**
  - INV-INPUT-8 / C4.5: FocusModel is the only keyboard-focus writer;
  - C2.2: Smithay focus is an applied copy;
  - C4.2 P3: Smithay grabs decide focus for client sequences.
- **Counterexample:**
  - ER-1 NEG-1: a FocusModel change is ignored by `PopupKeyboardGrab`, so core and Smithay diverge.
  - Smithay restores focus to the root on grab end.
  - ER-2 SEALED: under an implicit click grab, core's W-SM-4a prediction diverges from Smithay.
- **Replacement:**
  - **ActiveElementFocus** (core; sole element-level authority) is separate from **ProtocolSurfaceRoute** (adapter; popup grabs; subordinate, routing only inside the focused element's tree).
  - A core focus change preempts incompatible grabs in one batch: `ungrab(All)` → keyboard `unset_grab` → pointer `unset_grab` → `set_focus`.
  - Nested grabs keep the pointer grab (W-SM-14).
  - Pointer-side decisions (W-SM-4a) use the **delivered** focus. Constraint reconciliation is deferred during client-owned grab sequences.
- **INV:** INPUT-8 (amended), **INPUT-10 (new)**, INPUT-5 (amended).
- **ADR:** **ADR-14 (new)**, ADR-07, ADR-13.
- **Evidence:** ER-1 (7 positive scenarios, 3 negative or observation controls), ER-2.
- **No new blocker:** Smithay's public API performs the preemption safely: no assertion and no panic in ER-1, including a popup destroyed in flight.

### PD-F2-03: constraint coordinate domain and interaction output
- **Sealed rule:** INV-COORD-5 `Locked(SurfaceKey, Canvas lock point)`; INV-INPUT-5 "the pointer's output".
- **Counterexample:** no output in the state. The Canvas lock point drifts 20 px when geometry.loc changes (D5 epoch 3) and detaches on element moves.
- **Replacement:**
  - Seat physical pointer state `(interaction output, OutputLogical p)` is kept separately from the constraint state `{surface, kind, lock point: SurfaceLocal}`.
  - The constraint never carries an output; the Canvas point is derived.
  - On loss of the interaction output: relocate the physical pointer at once (existing Arrangement-nearest rule, half-open), reconcile the constraint per INV-INPUT-5 (deferred under a client grab), and emit each event exactly once.
  - No "primary output" is introduced.
- **INV:** COORD-5, INPUT-5, OUTPUT-1 (amended).
- **ADR:** ADR-07.
- **Evidence:** ER-2: the D5 epoch-4 divergence reproduced (SEALED) and resolved (DEFER).

### PD-F2-04 / PD-F2-05: reconciliation and confinement (one constraint architecture)
- **Sealed rule:** B11.4 reconciliation only at pan end; C4.3 activation only.
- **Counterexample:**
  - Shell/keyboard camera changes, presentation changes, output reconfiguration, element moves and focus changes left constraints unreconciled.
  - Confinement crossed outputs (ER-5 R5 negative control).
- **Replacement (INV-INPUT-5 amended):**
  - batch-end reconciliation on the listed triggers;
  - deferral during compositor interactions and client grabs;
  - lock kept only if observable through the interaction output **and** its element holds ActiveElementFocus;
  - confinement clamped on the interaction output, with no Arrangement crossing; outside the region after a view change → `unconfined` (no warp);
  - destruction ends constraints through the protocol path;
  - exactly-once events; no phantom absolute or relative motion.
- **During camera motion:** compositor interactions defer reconciliation (unchanged KeepFocus behaviour). A non-interaction camera change (shell, atomic) is reconciled at batch end.
- **When the constrained surface stops being visible through the interaction output:** a lock ends, a confinement ends (no warp).
- **INV:** INPUT-5, CAMERA-4 (consistent), OUTPUT-1.
- **ADR:** ADR-07.
- **Evidence:** ER-2, ER-5 R5. The trigger set is T1 work (OB-3, OB-31).

### PD-F2-06: zoomed-pan algebra
- **Sealed rule:** B11.1/INV-COORD-3 `camera = start − Q(Σ)`; C5.1/OB-24 "zoom ≠ 1 is static".
- **Counterexample:** content moved 1.659× the hand (D5 epoch 4). Lattice jitter appears at R switches (ED-1).
- **Replacement:**
  - `dP_canvas = dp_phys / (s_O · z_C)`; `C = C_start − Q16(Σ dp_phys / (s_O · z_C))`, accumulated from the gesture origin.
  - Zoomed navigation is part of Foundation 01, and fit exit keeps the zoom.
- **INV:** COORD-3 (amended).
- **ADR:** ADR-02, ADR-04.
- **Evidence:** ER-5 R1; ED-1 (render/hit agreement at the zoomed k holds).
- **Residual (D):** ±1 px shape change at lattice-reference switches during zoomed panning (ED-1, ED-2 F1).
  - **OB-24 rescoped:** the zoom-motion anti-jitter decision is needed before Phase F implements zoomed navigation, not at a later zoom-motion feature.
  - Candidate: a gesture-pinned lattice reference (ED-2 F1: constant widths). It needs an X-6-style Smithay experiment.
  - Visual acceptance belongs to T3/T4.
  - Not adopted in 1R: it would change INV-RENDER-1's formula, and the authorization did not decide it.

### PD-F2-07: ScaleInteractionEpoch
- **Sealed rule:** INV-SCALE-3 gesture/SC-D; C14.4 SC-D ends on any non-zoom input.
- **Counterexample:** 40/40 renegotiations for discrete pan, zoom-at-cursor and shell streams (ED-2 F2; D5 epoch 2).
- **Replacement:**
  - One concept, ScaleInteractionEpoch (INV-SCALE-3, -4 amended).
  - Settlement by explicit end, by settlement event, or by timeout.
  - The timeout's existence is architectural; its value is provisional (candidate 300 ms; C14.5 + OB-23).
  - SC-D and its "decrease-settle delay" are subsumed. SC-E is kept outside epochs.
- **ADR:** ADR-05.
- **Evidence:** ER-5 R2 (all seven scenario classes bounded; negative control reproduces 40; timeout sensitivity).

### PD-F2-08: device bound vs ladder text
- **Replacement:** INV-SCALE-2 (emitted hints are rungs of the ladder in force; output scales ≤ H_max; H_max over all surfaces; off-ladder held hints allowed); INV-SCALE-5 (conditional P4).
- **ADR:** ADR-05.
- **Evidence:** ED-2 F3/F4. **Disposition A.**

### PD-F2-09: initial hint
- **Replacement:** INV-SCALE-5 initial-hint rule: predicted ρ over views intersecting the placed rect, otherwise the maximum output scale.
- **ADR:** ADR-05.
- **Evidence:** ED-2 F8 (the extra renegotiation the harness rule causes). **Disposition A.**

### PD-F2-10: protocol fullscreen state machine (raised to Level A)
- **Sealed rule:** C5.2 "record it and decide when W next gains focus".
- **Counterexample:** xdg-shell requires a configure answer. The deferral leaves the request unanswered (ER-3 negative control: 0). Presentation timing, the second requester and lifetime were also undefined.
- **Replacement:** INV-PRES-1 amended:
  - every request is answered;
  - grant only for the focused element or before map; decline at once otherwise; no pending state;
  - the presentation starts and ends with the decision;
  - a newer grant replaces an older one;
  - focus loss does not end fullscreen;
  - output choice runs at grant.
- **ADR:** ADR-09.
- **Evidence:** ER-3. **Disposition A.**

### PD-F2-11: shared cameras vs output-keyed fit; orphaned pan
- Fit is no longer a mode (PD-F2-01): **disposition B**.
- One rule added (INV-OUTPUT-1): an orphaned interaction does not mutate the camera.
- Isolation is removed from Foundation 01. Fit on a shared camera applies to all bound outputs by definition.
- **ADR:** ADR-04.
- **Evidence:** ER-4 (fit is one mutation).

### PD-F2-12: rounding point R8
- INV-COORD-2 now lists R8 (inward rounding of the popup constraint target).
- **ADR:** ADR-02.
- **Evidence:** ED-2 F7. **Disposition A.**

### PD-F2-13: window drag across outputs: **C, obligation OB-34**
- **Why no architectural mutation now:**
  - Interactive move UX is deferred (C15).
  - INV-STAGE-1 already fixes the writer (`stage::place`, R3).
  - Only the cross-output delta definition is open, and no Foundation feature drags across outputs before that UX exists.
- **Destination:** a decision recorded before Phase F implements interactive move. The candidate is `anchor = R3(q_now − grab_offset)`.
- **Pass criterion:** the element stays under the cursor across an Arrangement crossing; the result is batching-independent (T0 property) with no drift (T1).

### PD-F2-14: OutputsOff
- INV-FRAME-6 reason renamed and defined independently of camera membership. **Disposition A (text).**
- **ADR:** ADR-06.

### PD-F2-15: attention and `transient_for` on unmap/destroy: **C, obligation OB-35**
- **Why:** both are core metadata with a single writer; no authority or protocol consequence.
- **Rule for Phase F:** unmap clears attention (with a shell event); destroy purges every reference; `transient_for` follows protocol re-parenting.
- **Pass:** T1 shell-event test: no attention mark on unmapped or destroyed elements; no stale `transient_for`.

### PD-F2-16: satellite restart: **E (verified) + C, obligation OB-33**
- ER-6 verifies `-listenfd`.
- **Design for Phase F:** the compositor reserves the X display number and owns its sockets, passing them with `-listenfd`; a restart reuses the same display. This is consistent with C6, so INV-XWAYLAND-1 is unchanged.
- **Pass:** T1. Kill satellite while an X11 client is running; the restart serves the same `DISPLAY`, and a new X11 client launched with the old `DISPLAY` connects.

### PD-F2-17: maximize policy (raised to Level A)
- **Replacement:** new INV-PROTO-1 (decline maximize with a configure; no camera, no Stage; `activated` ⇔ ActiveElementFocus; minimize ignored; no bounds).
- **ADR:** ADR-09, ADR-14.
- **Evidence:** ER-3 (positive and negative control).
- The `activated` mapping is specified; its T1 test is OB-32.

---

## Obligation changes

| ID | Change |
|---|---|
| OB-19 | probe list extended: 2d (protocol re-fit → E0624), 2e (sticky authority → E0277); source rule "`Authorized::mint` only in the entry module" |
| OB-23 | now tunes the ScaleInteractionEpoch settlement timeout (candidate 300 ms); the SC-D decrease delay is subsumed |
| OB-24 | **rescoped**: zoomed panning is in Foundation 01. The anti-jitter decision is due **before Phase F implements zoomed navigation**; candidate: gesture-pinned lattice reference; visual acceptance T3/T4 |
| OB-28 | rescoped: no re-fit loop exists; remaining scope is client self-resize under epochs + P7 (T1 adversarial client) |
| **OB-30 (new)** | port ER-1 (xpopup_focus) to T1 against the production adapter; keep NEG-1/NEG-2/OBS-1 as negative controls |
| **OB-31 (new)** | port ER-2 (xlock_grab) to T1 with real output removal (merges with OB-3/X-11a); add confinement cases (R5) |
| **OB-32 (new)** | ER-3 maximize/fullscreen answers in T1, plus the `activated` mapping test |
| **OB-33 (new)** | satellite restart on the same display via `-listenfd` (T1) |
| **OB-34 (new)** | cross-output drag formula decision before interactive move (PD-F2-13) |
| **OB-35 (new)** | attention / `transient_for` lifecycle T1 test (PD-F2-15) |

C14.5 provisional list gains: **T_settle (ScaleInteractionEpoch settlement timeout): candidate 300 ms; allowed range: finite and greater than the discrete-input repeat interval; tuned by OB-23.** SC-D's "decrease-settle delay: off" is removed (subsumed).
