# Checkpoint

CURRENT_PHASE: **D corrective checkpoint 2R complete (PD-F2-18, PD-F2-19 resolved), UNCOMMITTED, stopped at the Checkpoint 2R review gate.** D7–D9 have not been performed. 1R seal: `84ce620898445aaadb72f386ec3954e5bbfe2c2f`; HEAD `2ee8ab6`. Phase D checkpoint 1 is committed as the immutable falsification baseline **`e8ec03a2ab36f315ae025723ec666b9ad278ac83`**. No Level-A finding is open after 1R. D7–D9 have not begun. Phase C: COMPLETE and SEALED (`615b0ed2f3631b698b0c54783c28f696ef4ff34a`).

Date: 2026-10-04

## Baselines
- Phase A evidence: `2c8c163784e260af4aaa8c1dc05c6281b0476120`
- Phase B seal: `4a0807ed721ae257e1f4b0e5287d1dfd8c1038c1`
- Phase C checkpoint 1: `d596416cf57877116e9cfbc8cf6c6fe25c001c67`
- Phase C checkpoint 2: `fc2dd68f843b1eb05828f4116f94e8bab4f478da`
- **Phase C checkpoint 3A:** `795800937a1e50e56587cd4080a73ce27d8a8b01` (hash recorded in `fd9a80c`)
- **Phase C seal (checkpoint 3B):** `615b0ed2f3631b698b0c54783c28f696ef4ff34a`

## PHASE D CORRECTIVE CHECKPOINT 2R (uncommitted)
- **Trigger:** D7–D9 stopped at PD-F2-18 (Level A). PD-F2-19 (Level C) and PD-F1-10 recorded at the same time (FOUNDATION_BLOCKERS.md § Phase D Checkpoint 2).
- **PD-F2-18 resolved:**
  - dedicated `OutputLifecycleToken` (INV-OUTPUT-2, new);
  - INV-CAMERA-1/3 and INV-OUTPUT-1 amended;
  - hardware-only descriptor; passive persisted lookup; no public restore API.
  - Evidence ER2-1: authority spike, 6 new probes and 3 new tests; `verify.sh` PASS.
- **PD-F2-19 resolved:** INV-SCALE-3/4 cover all spatial interactions. Evidence ER2-2 (`d5_model_2r`): window drag 39 → 1, with a negative control.
- **Registry:** 57 entries (§C12.R2, audit/2R/INVARIANT_DIFF_2R.md).
- **Clarifications at the seal (C12.R2.3, D-F0, non-semantic):**
  - `OutputLifecycleToken` lifecycle: exactly one per Core, returned once to the composition root, never re-issued, lifecycle operations only. Backed by the `verify.sh` 2g source check with a seeded negative control.
  - Device-bound (P7) exception: only when `h > H_max(E)`; never a damping bypass.
  - 300 ms remains provisional (OB-23).
- **PD-F1-10:** left open for D7.
- **NEXT:** user review of 2R; commit only on authorisation; D7–D9 not begun.

## PHASE D CORRECTIVE CHECKPOINT 1R (sealed: `84ce620898445aaadb72f386ec3954e5bbfe2c2f`)
- **Baseline:** checkpoint 1 committed as `e8ec03a2ab36f315ae025723ec666b9ad278ac83` ("preserve Phase D falsification baseline"). This hash is recorded here and in audit/README.md, audit/1R/* and FOUNDATION_BLOCKERS.md.
- **Dispositions:** all 17 PD-F2 findings disposed of (FOUNDATION_BLOCKERS.md § Phase D Corrective Checkpoint 1R; audit/1R/CORRECTIONS_1R.md).
  - 13 resolved by architecture correction (A);
  - 1 resolved by another correction (B, PD-F2-11);
  - 3 now obligations: PD-F2-13 → OB-34, PD-F2-15 → OB-35, PD-F2-16 → OB-33;
  - PD-F2-06 jitter residual → OB-24 rescoped.
- **Re-evaluation:** 5 findings raised to Level A (PD-F2-03, -04, -05, -10, -17); all resolved.
- **Registry:** amended under provenance in FOUNDATION_ARCHITECTURE §C12.R1 (sealed C12 table unaltered). 14 invariants amended, 2 new (INV-INPUT-10, INV-PROTO-1); 56 entries.
- **Evidence:**
  - ER-1 xpopup_focus;
  - ER-2 xlock_grab;
  - ER-3 xmaximize (maximize and fullscreen);
  - ER-4 authority spike probes 2d/2e;
  - ER-5 d5_model_1r;
  - ER-6 satellite `-listenfd`;
  - ER-7 regression re-run.
  - Every experiment ran with its negative control, all as expected.
- **New Smithay quirks:** W-SM-14 and W-SM-15. **New ADR:** ADR-14.
- **Provisional constant:** T_settle candidate 300 ms (OB-23).
- **Explicit residual (NOT resolved, NOT validated):** the ±1 physical-pixel shape variation at lattice-reference switches during zoomed pan (ED-1, ED-2 F1). It is not a rendering guarantee. OB-24 keeps it open as a **pre-implementation decision for zoomed navigation** (candidate: gesture-pinned reference; visual acceptance T3/T4).
- **NEXT:** D7–D9 only after explicit user authorisation.

## PHASE D CHECKPOINT 1 (committed as e8ec03a)
- Input baseline verified (audit/README.md §1): seal resolves; `a3ae907` records it; tree was clean; 54-entry C12 registry normative; BLOCKERS pointer present; OB-19 unconditional; T3–T5/UP/TUNE open.
- D1 threat model; D2 22 state machines; D3 authority matrix (40 items, 6 searches); D4 coordinate matrix (16 domains, 26 conversions); D5 four-epoch audit; D6 FOUNDATION_DECISIONS.md (ADR-01…13).
- Findings: 17 D-F2 (A: 2, B: 6, C: 9), recorded in FOUNDATION_BLOCKERS.md § Phase D Architectural Findings; 9 D-F1; 9 D-F0; no D-F3. Registry NOT edited.
- New evidence: ED-1 d5hit (Smithay, 0 mismatches incl. k ∉ n/120), ED-2 d5_model (MODEL), ED-3/4/5 (source).
- NEXT: user disposition of PD-F2-01/02 (Level A) and the other D-F2s; commit of checkpoint 1 only on authorisation; D7–D9 not begun.

## COMPLETED_WORK (3B)
- C11: verification tiers T0–T5 (environment, simulated/real, invariants, tests, cost, trigger, evidence, meaning of failure, tier-honesty rule); OB-1…OB-29 all mapped; negative controls retained.
- C12: canonical invariant registry, 54 `INV-*` with owner, class, evidence, tests, tier and violation consequence; legacy → canonical map; supersessions marked (D-B2-I9 tolerance → INV-HIT-1; D-B8-SCALE wording → INV-SCALE-*).
- C13: implementation-readiness audit; rules promoted into normative text.
- C14:
  - performance methodology (no invented budgets);
  - idle contract (INV-IDLE-1);
  - allocation contract (INV-ALLOC-1);
  - scale-churn contract (INV-SCALE-4);
  - feedback damping SC-E/SC-D with model evidence (EC-22);
  - provisional tuning list.
- OB-19 formalised as the unconditional Phase F entry gate.
- C15 compatibility contract; C16 non-goals; physical obligations (C-PHYS).
- Authority reconstruction (C-AUTH): no dual writers.
- Final adversarial audit (25 scenarios): no contradictions between invariants; five under-specifications closed (AUD-4, -12/-13, -24, -25).
- Phase C gate evaluation: satisfied.

## FILES_CHANGED (committed in the seal)
- `docs/foundation/FOUNDATION_ARCHITECTURE.md` (3B sections)
- `docs/foundation/FOUNDATION_BLOCKERS.md` (normative-pointer note)
- `docs/foundation/CHECKPOINT.md`
- `docs/foundation/evidence/phase-c/cp3b-feedback-model-output.txt`
- `experiments/cp3a-scale-model/feedback.py`

## PROVISIONAL (C14.5)
H_cfg 3.0 · r 1.25 · σ 0.05 · P 1 s · SC-D decrease delay off · renderer filters Linear.

## OPEN, WITH DESTINATIONS
- OB-19 = Phase F entry gate (first production-workspace commit).
- All other OBs in C11.2 (T0–T5 / UP / TUNE / zoom-motion decision).

## LIVE_SYSTEM_TOUCHED
**No.** 3B ran only offline model scripts and documentation work; no clients, compositors or system changes.

## NEXT_ACTION
Phase D (falsification and document reorganisation) only after explicit user authorisation. Approval scope: the architecture is sufficiently specified to advance; it is **not** physically validated or production-ready. T3–T5 and upstream obligations remain authoritative future requirements.
