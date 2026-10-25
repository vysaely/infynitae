# Phase D — Falsification and formalisation (checkpoint 1: D1–D6)

Status: Phase D checkpoint 1 committed as the immutable **falsification baseline `e8ec03a2ab36f315ae025723ec666b9ad278ac83`**. **Corrective checkpoint 1R** ([1R/CORRECTIONS_1R.md](1R/CORRECTIONS_1R.md)) is complete and UNCOMMITTED for review.

> **1R note.** The registers below are the baseline record and are kept unchanged. Dispositions: [1R/CORRECTIONS_1R.md](1R/CORRECTIONS_1R.md). Canonical amendments: FOUNDATION_ARCHITECTURE §C12.R1. Invariant diff: [1R/INVARIANT_DIFF_1R.md](1R/INVARIANT_DIFF_1R.md). Corrected machines: [1R/STATE_MACHINES_1R.md](1R/STATE_MACHINES_1R.md). Matrix deltas: [1R/MATRICES_1R.md](1R/MATRICES_1R.md). Contradiction audit: [1R/CONTRADICTION_AUDIT_1R.md](1R/CONTRADICTION_AUDIT_1R.md).

Baseline status line (checkpoint 1): **Phase D checkpoint 1 deliverables complete.** Phase D checkpoint 2 (D7–D9) has not begun. Two Level A D-F2 findings (PD-F2-01, PD-F2-02) block advancement until the user disposes of them.

Purpose: an adversarial test of the **sealed** Phase C architecture. It is not a fourth design phase. The 54 canonical `INV-*` invariants (FOUNDATION_ARCHITECTURE §C12) are hypotheses under test. Nothing in `audit/` amends them. Architecture corrections are **recorded** as D-F2 in [FOUNDATION_BLOCKERS.md § Phase D Architectural Findings](../../foundation/FOUNDATION_BLOCKERS.md#phase-d-architectural-findings) and await disposition.

| Artifact | Content |
|---|---|
| this file | input baseline, change control, finding register (all classes) |
| [D1_THREAT_MODEL.md](D1_THREAT_MODEL.md) | falsification threat model (6 categories, prioritised) |
| [D2_STATE_MACHINES.md](D2_STATE_MACHINES.md) | 22 state machines; reachable-undefined search |
| [D3_AUTHORITY_MATRIX.md](D3_AUTHORITY_MATRIX.md) | ownership/authority matrix (40 state items) and the six authority searches |
| [D4_COORDINATE_MATRIX.md](D4_COORDINATE_MATRIX.md) | 16 coordinate domains, 26 conversions, re-falsification of the numeric model |
| [D5_FOUR_EPOCH_AUDIT.md](D5_FOUR_EPOCH_AUDIT.md) | combined multi-output audit, four epochs, concrete numbers and assertions |
| [../FOUNDATION_DECISIONS.md](../../foundation/FOUNDATION_DECISIONS.md) | D6: ADR-01 … ADR-13 |

New evidence (Phase D):

| ID | What | Class / tier | Where |
|---|---|---|---|
| ED-1 | **d5hit**: the D5 tree (root + nested subsurface + below-subsurface reached through an input-region hole + popup + popup subsurface) under the exact D5 cameras, against Smithay 0.7.0's real `WaylandSurfaceRenderElement` geometry. Includes an effective scale **not** of the form n/120 (k_A = 1080/651, from a viewport-fit zoom), a zoom-1.25 stress view on B (k = 1.875), and four samples of a fractional pan of A. **0 physical-selection mismatches and identical edge tables in all 7 configurations (1.24–1.94 M pixels each).** Also shows real shape jitter under zoomed panning (W 1394 → 1393 px, S2 100 → 99 px at lattice-reference switches). | EXPERIMENT, T1-harness (lockstep, DummyRenderer) | `experiments/b8-harness/src/bin/d5hit.rs`; [`evidence/phase-d/d5hit-output.txt`](../evidence/phase-d/d5hit-output.txt) |
| ED-2 | **d5_model**: executable MODEL of the sealed rules. It computes the four D5 epochs and checks F1–F8 (lattice at zoom ≠ 1, SC-D/discrete-pan churn, device bound, ladder change, ranges, pixel-centre clamp, popup target, initial hint). Before computing anything, it asserts that its edge tables equal ED-1's Smithay geometry. | MODEL (not an experiment) | `experiments/phase-d/d5_model.py`; [`evidence/phase-d/d5-model-output.txt`](../evidence/phase-d/d5-model-output.txt) |
| ED-3 | SMITHAY source: `PopupKeyboardGrab::set_focus` ignores focus changes while the grab is active; Smithay restores focus to the root on grab end (`desktop/wayland/popup/grab.rs:371-378, 441-447, 456-476`); `KeyboardHandle::set_focus` routes through the active grab (`input/keyboard/mod.rs:1052-1057`). | SMITHAY source | — |
| ED-4 | SMITHAY source: `OutputDamageTracker::from_output` takes its scale from the `Output`; `OutputDamageTracker::new(size, scale, transform)` accepts any scale; elements are measured with `element.geometry(output_scale)` (`backend/renderer/damage/mod.rs:243-271, 330-381`). | SMITHAY source | — |
| ED-5 | HARNESS source: the harness's initial preferred scale is the maximum scale of outputs whose viewport contains the anchor point, ignoring zoom (`b8-harness/src/server.rs:535-548`). It is a harness rule, not an architecture rule. | harness source | — |

Re-run: `cd experiments/b8-harness && cargo run --offline --release --bin d5hit`; `python3 experiments/phase-d/d5_model.py`.

---

## 1. Phase D input baseline (§1 integrity check)

Checked 2026-10-04, before any analysis:

| Check | Command / observation | Result |
|---|---|---|
| Seal commit resolves | `git cat-file -t 615b0ed2f3631b698b0c54783c28f696ef4ff34a` → `commit`; subject "docs(foundation): seal Phase C (C11-C16, canonical registry, final audit)" | **PASS** |
| `a3ae907` records it | `a3ae907` "record Phase C seal commit hash" touches CHECKPOINT.md and FOUNDATION_ARCHITECTURE.md; the full hash occurs 2× in CHECKPOINT.md and 1× in FOUNDATION_ARCHITECTURE.md at `a3ae907` | **PASS** |
| Working tree clean | `git status --porcelain` → 0 entries (HEAD `a3ae907`, branch `main`) | **PASS** |
| C12 registry normative, 54 entries | 54 `**INV-…**` rows in §C12; "This table is authoritative." present | **PASS** |
| BLOCKERS points to it | "Phase C note (normative pointer)" at the top of FOUNDATION_BLOCKERS.md | **PASS** |
| OB-19 unconditional Phase F entry gate | §"OB-19 — Unconditional Phase F entry gate" (line 1466) and C11.2 "Phase F entry gate (unconditional)" | **PASS** |
| T3–T5, UP, TUNE obligations open | C11.2 rows T3, T4, T5, UP, TUNE present (5 rows); no OB in them is marked done/closed/passed | **PASS** |

**Phase D input baseline = `615b0ed2f3631b698b0c54783c28f696ef4ff34a` (recorded in `a3ae907`).** Every reference in `audit/` to C-sections, `INV-*`, OB-n, D-B…, EC-n or EB-n resolves against that commit.

## 2. Change control applied

| Class | Meaning | Where recorded |
|---|---|---|
| D-F0 | clarification only | register below |
| D-F1 | non-functional refinement (wording, type/range assertion, API detail, test reference; behaviour 100% unchanged) | register below; **not applied** to sealed files |
| D-F2 | architecture correction (formula, transition, state variable, ownership/writers, protocol policy, observable behaviour, invariant meaning) | **FOUNDATION_BLOCKERS.md § Phase D Architectural Findings** (sealed rule preserved, counterexample, affected items, minimal correction proposed, evidence need) |
| D-F3 | unresolved blocker | none found |

Ambiguity rule applied: D-F1 vs D-F2 → D-F2. Applying it moved three findings up: PD-F2-09 (initial hint), PD-F2-15 (attention) and PD-F2-16 (satellite display). Each only *defines* behaviour the sealed text left open, but each is observable.

No sealed file was edited except for two appends: the new FOUNDATION_BLOCKERS.md section, and the CHECKPOINT.md status, which is a status file. FOUNDATION_ARCHITECTURE.md and the registry are **unchanged**.

## 3. Finding register

### D-F2: architecture corrections (detail in FOUNDATION_BLOCKERS.md)

| ID | Level | Title | Found by |
|---|---|---|---|
| PD-F2-01 | **A** | Camera evolution without an authority port (fit re-fit; animated navigation) | D3 cycle search, D2 camera/fit machines |
| PD-F2-02 | **A** | Smithay grabs decide focus behind FocusModel/PointerModel | D3 multiple-writer search (ED-3); D5 epoch 4 |
| PD-F2-03 | B | `Locked` lacks output; Canvas lock point detaches from the surface | D5 epochs 2–3 |
| PD-F2-04 | B | Pointer reconciliation only at pan termination | D2 pointer-focus machine; D5 epochs 2–4 |
| PD-F2-05 | B | Confinement enforcement across independent cameras | D2 confinement machine |
| PD-F2-06 | B | Zoomed panning reachable; pan formula lacks zoom; OB-24 premise false | D4 re-falsification; ED-1, ED-2 F1 |
| PD-F2-07 | B | Discrete camera interactions outside SC-D; SC-D ends on motion | D2 scale-epoch machine; ED-2 F2; D5 epoch 2 |
| PD-F2-08 | C | Device bound vs ladder/P4; H_max size basis; off-ladder held hint | ED-2 F3/F4; D5 epoch 4 |
| PD-F2-09 | C | Initial scale hint undefined | D2 lifecycle × scale; ED-5 |
| PD-F2-10 | C | Protocol-fullscreen state machine incomplete | D2 fullscreen machine; D5 epoch 3 |
| PD-F2-11 | C | Shared camera vs output-keyed fit/isolation; orphaned pan | D2 camera machine |
| PD-F2-12 | C | Popup constraint target rounding point (R8) | D4 matrix; ED-2 F7 |
| PD-F2-13 | C | Window drag across outputs | D4 multi-output inverse |
| PD-F2-14 | C | `AllObservingOutputsOff` defined over camera membership | D2 suspension machine |
| PD-F2-15 | C | Attention / `transient_for` on unmap/destroy | D2 lifecycle machine |
| PD-F2-16 | C | Satellite restart display identity / socket ownership | D2 satellite machine |
| PD-F2-17 | B | xdg_toplevel state policy undefined (`activated`; maximize needs a responding configure) | D2 xdg_toplevel machine (protocol text) |

### D-F1: non-functional refinements (behaviour unchanged; to be applied in D7/Phase F text only after review)

| ID | Refinement | Basis |
|---|---|---|
| PD-F1-01 | Visibility/culling arithmetic must run in i64/f64 and cull **before** any i32 conversion. Far edges reach \|x−R\|·k ≈ 2³⁷·k, well above `i32::MAX` (F5). C3.2 [3] says "i64 → f64 → i32" without the order. | D4, ED-2 F5 |
| PD-F1-02 | INV-COORD-1's bound applies to **stored** Canvas values. Derived view origins can exceed it: \|v_O\| ≤ 2³⁶ + mode/(2k), up to ≈ 2³⁶ + 2³⁰ for a fit of a 2³¹-wide window (F5). They remain f64-exact and far from i64 overflow. State the derived bound; fit centres saturate per B1.3-6. | D4 |
| PD-F1-03 | The adapter builds each output's damage tracker with `OutputDamageTracker::new(mode, Scale(k_O), transform)` and rebuilds it when k_O changes. `from_output` would use the output scale and render zoomed views at the wrong size (ED-4). C3.1 [6] is silent on the scale argument. | D4, ED-4 |
| PD-F1-04 | `mode_O` in `v_O = centre − mode/(2k)` is the mode **after** the output transform (rotated outputs swap w/h). | D4 |
| PD-F1-05 | Output-local pointer clamps are half-open: clamp to `[0, w − 2⁻⁸]` (wl_fixed resolution). This covers Arrangement-nearest relocation, output shrink (PD-F2-04) and confinement (PD-F2-05). D5 epoch 4 uses 1919.996094. | D4, D5 |
| PD-F1-06 | Arming the fallback timer only while needed (INV-FRAME-3, INV-IDLE-1) requires core to know whether a surface has a pending frame callback. Add `frame_requested` to the commit snapshot; core clears it when it emits `CompleteFrames`. The CP2-2 snapshot list omits it. | D3 (unreconstructable derived state) |
| PD-F1-07 | Hit-test results from a `ProjectionFrame` are validated against the live `SurfaceKey` registry; entries of destroyed surfaces fail closed (no delivery) until the next frame (B4/B10.6 "fail closed"). | D3 (cache invalidation) |
| PD-F1-08 | `D_max` is re-queried when the renderer is recreated (C8 renderer-failure row); a lower value triggers P7. | D2 recovery machine |
| PD-F1-09 | The protocol-fullscreen presentation view is a `View` with zoom 1 and `v = anchor − max(0, (mode/scale − size)/2)`. It goes through C3.2 [1], so R7 quantises it; a larger surface is origin-aligned and cropped. No new rounding point is needed. | D4, D5 epoch 3 |

### D-F0: clarifications (no change)

| ID | Clarification |
|---|---|
| PD-F0-01 | Crossing outputs during a client-owned implicit grab produces a jump in the surface-local position, because the cameras are independent. This is protocol-legal and expected. |
| PD-F0-02 | The CP3A-2 pixel-centre clamp claim ("provably 0") holds up to the Q16 rounding of `c_eff` and `p/zoom`: theoretically ≤ ≈ 2⁻¹⁶ logical px. Measured 0 in ED-1 (7 configurations) and in 200,000 random samples (ED-2 F6). |
| PD-F0-03 | B9.6's "(a removal is atomic, so applied now)" holds when no gesture is active. During a gesture, INV-SCALE-3 (canonical) defers the decrease. The registry prevails per the normative pointer. |
| PD-F0-04 | T5 inverts the lattice without δ, an error of 2⁻²⁰/k logical px. That is far below the Q16 quantum and immaterial. |
| PD-F0-05 | The kick-on-input rate is bounded by the physical input rate (RB-9). Confirmed by D5 epoch 4. |
| PD-F0-06 | Hit-testing from a `ProjectionFrame` up to one frame old (I9) delivers coordinates relative to the displayed geometry. This lasts at most one frame after a client commit that moves its own subsurfaces or geometry. It is a stated trade-off, not a defect. |
| PD-F0-07 | Camera retention per `OutputKey` is unbounded in principle; the retention policy is Level D persistence. |
| PD-F0-08 | Hotplugging an output whose scale is a ladder rung can renegotiate an observed element even when ρ is unchanged (epoch 4: 1.7321 → 1.6818). The cost is bounded by hotplug events and follows from the ladder definition (PD-F2-08). |
| PD-F0-09 | The pointer's Canvas point is defined on the snapped camera `c_eff` (T5/R5). A placement or drag derived from the pointer therefore carries ≤ ½/k logical px of camera snap before R3, a function of current state and never accumulated. This is not a write-back of a rendered value (F1 holds; D4 §4). |

No D-F3 finding. Nothing was silently absorbed. Every gap found is in one of the three tables.
