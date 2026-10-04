# Checkpoint

CURRENT_PHASE: **D — checkpoint 1 (D1–D6) deliverables complete, UNCOMMITTED, stopped at the Phase D checkpoint 1 review gate.** Two Level A D-F2 findings (PD-F2-01, PD-F2-02) block Phase D checkpoint 2 until disposed of. Phase C: COMPLETE and SEALED (`615b0ed2f3631b698b0c54783c28f696ef4ff34a`).

Date: 2026-10-04

## Baselines
- Phase A evidence: `2c8c163784e260af4aaa8c1dc05c6281b0476120`
- Phase B seal: `4a0807ed721ae257e1f4b0e5287d1dfd8c1038c1`
- Phase C checkpoint 1: `d596416cf57877116e9cfbc8cf6c6fe25c001c67`
- Phase C checkpoint 2: `fc2dd68f843b1eb05828f4116f94e8bab4f478da`
- **Phase C checkpoint 3A:** `795800937a1e50e56587cd4080a73ce27d8a8b01` (hash recorded in `fd9a80c`)
- **Phase C seal (checkpoint 3B):** `615b0ed2f3631b698b0c54783c28f696ef4ff34a`

## PHASE D CHECKPOINT 1 (uncommitted)
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
