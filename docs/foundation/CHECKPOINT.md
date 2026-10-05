# Checkpoint

CURRENT_PHASE: **PHASE F — INC-2 IMPLEMENTED, PENDING INDEPENDENT REVIEW.** Multi-output lifecycle, binding precedence (1:1, runtime 2i), membership, damage trackers sized `phys_O`, zero-output core state, frame clock and presentation feedback run on the production path (`0a03aab`). OB-49 is mechanically satisfied for every criterion that can execute in INC-2 and stays `OPEN` (finding INC2-F01). **INC-3 has NOT begun.**

Date: 2026-10-05

## PHASE F — INC-2 (implemented: `0a03aab`; evidence: evidence/phase-f/INC-2/)
- **Scope:** PRODUCT, HEADLESS, canonical INC-2.
  - **core:** output manager (M4/M5, OutputKey, PersistedCameras, Arrangement), PointerState (lifecycle part), visibility pass (OBJ-13) and frame clock.
  - **adapter:** wl_output + xdg-output + presentation globals, enter/leave, damage trackers (`phys_O`), frame completion.
  - **headless backend:** hotplug, reconfiguration, disconnection, virtual vblank.
- **Not started (owned later):** zero-output seat grab and routed motion (INC-3), reconciliation (INC-5), epoch settlement (INC-6), fullscreen configure (INC-7), kick and fallback (INC-8).
- **Tests:** 75 passing (T0 core 52; T1 19 distinct tests plus the oracle self-test in each of the 4 T1 binaries). Runtime 2i passes with the 1:1 rule and fails with "1:1 violated" under the sealed rule, on T0 and T1 (`verify-architecture` [5b]). EE-1 tracker matrix on the production adapter, with the sealed control failing 7/7 rotated cases.
- **Enforcement:** `verify-architecture` PASS (27 probes, 47 seeded controls); `verify-test-efficacy` PASS (45/45, 21 new INC-2 controls); checker 57/20/51, self-test 18/18; no new dependencies; smithay `=0.7.0`; no unsafe; P1 not applied (OB-11 open).
- **Obligations:** OB-49 mechanically satisfied except items that cannot execute yet (fullscreen configure, epoch settlement); OB-45 (tracker/membership/damage part), OB-50 (core part) and OB-3 (removal part) partly satisfied; all `OPEN`.
- **Findings:** Level A 0, Level B 0, Level C 2 (INC1-F01, INC2-F01).
- **LIVE_SYSTEM_TOUCHED:** no.
- **NEXT:** independent adversarial INC-2 review. INC-3 only after that review and explicit owner authorization.

## PHASE F — INC-1 RATIFICATION (record: evidence/phase-f/INC-1/corrective/)
- **Review:** `INC-1 CONDITIONAL PASS — CORRECTIONS REQUIRED BEFORE RATIFICATION` (dossier recorded unchanged in `38c60ca`). Findings INC1-F06 and INC1-F07 (Level B); INC1-F01…F05 adjudicated.
- **Corrective patch `1fa72af` (tests and controls only; production code unchanged):**
  - **INC1-F06:** an independent exact-integer unprojection and region oracle for T1, plus a 56-row hand-derived T0 table.
  - **INC1-F07:** T0 and T1 coverage of unshown surfaces.
  - **Controls:** 7 permanent mutation controls (ADV-MUT-3 ×3, sign inversion, camera-offset omission, ADV-MUT-2 ×2), all detected.
- **Verification:** everything PASS.
  - Cargo: 47 tests, fmt, clippy.
  - Verifiers: `verify-architecture`; `verify-test-efficacy` 25/25.
  - Repository checks: checker 57/20/51 (self-test 18/18), spike, environment verifier.
  - CI: run 37291919770 on `1fa72af`.
- **Obligations:**
  - OB-2 and OB-26 `CLOSED`.
  - Still `OPEN`: OB-1, OB-6 (T4 part), OB-14, OB-17, OB-49 (runtime 2i, INC-2).
- **Findings:** open Level A 0, Level B 0, Level C 1 (INC1-F01, INC-7).
- **NEXT (at ratification):** INC-2 upon explicit owner authorization. Superseded by the INC-2 entry above.

## PHASE F — INC-1 (evidence: evidence/phase-f/INC-1/)
- **Scope:** PRODUCT, HEADLESS, exactly the canonical INC-1.
  - **core:** `stage`, `snapshot` + F-ORIGIN-1, `projection` (F-VIEW-1, F-LATTICE-1, F-UNPROJECT-1), `render` (ProjectionFrame, F-HIT-1), M1 lifecycle in `protocol`, single-output `output_connected` creating and binding one camera.
  - **adapter:** compositor/xdg-shell handlers, snapshot extraction, `SurfaceKey` registry, `convert` (F-RENDER-1).
  - **headless backend:** one virtual output, DummyRenderer.
- **Tests:** 32 core T0 and 7 T1 lockstep tests with a real probe client. Every pixel matches the presented surface at four output scales; xsnap/X-15, x6, xhit and the d5hit zoom-1 configuration are ported with their negative controls.
- **Enforcement:** `tools/verify-architecture` PASS.
  - **New rules:** `stage-single-writer`, `smithay-rejected-apis`, `smithay-geometry-only-in-convert`; unsafe scan now covers `tests/`.
  - **Dependencies:** dev-only `wayland-client`/`wayland-protocols` for the headless backend's T1 tests.
  - **Seeds:** 46 seeded controls.
- **Test efficacy:** new `tools/verify-test-efficacy` PASS (18/18 seeded behavioural defects caught; also in CI).
- **Not started:** multi-output, binding precedence, 2i (INC-2 / OB-49); input (INC-3/4); scale (INC-6); toplevel requests (INC-7, finding INC1-F01, Level C).
- **Registries:** unchanged (57 invariants, 20 `F-*`, OB-1…OB-51); checker PASS, self-test 18/18.
- **LIVE_SYSTEM_TOUCHED:** no.
- **NEXT (at implementation):** independent INC-1 review. Superseded by the ratification entry above.

## PHASE F — INC-0 (implemented: `632b1d0`; evidence: evidence/phase-f/INC-0/; review: audit/phase-f-inc0-review/)
- **Scope:** ENFORCE, HEADLESS. The production workspace (`crates/`: core, wayland, backend-headless, backend-winit, ipc, bin) carries the authority **shape** only; no product behaviour. Enforcement-only crates live in `enforcement/`.
- **Pins:** `rust-toolchain.toml` 1.99.0; `Cargo.lock` tracked; `smithay = "=0.7.0"` from crates.io in the adapter; no patch (P1 stays experimental; OB-11 open).
- **Enforcement:** `tools/verify-architecture` PASS: DAG (normal, dev, build, target); syn-based source rules (2g, 2h, mint location, wayland names, unsafe policy, authority escape prevention across value/ref/mut/ptr/aliases); 26 negative probes with asserted diagnostics plus 1 positive control; 30 seeded negative controls plus 3 judge controls. CI: `.github/workflows/ci.yml` (foundation-docs, verify-architecture, workspace-build).
- **Independent Review & Corrective Patch:** Independent adversarial review confirmed compiler probes, unsafe policy, and workspace isolation, identifying findings INC0-F01 (reference getters) and INC0-F02 (dev-dependencies). Corrective patch resolved both findings with seeded regression controls.
- **Decision D Ratification:** OB-19 ratified `CLOSED` with explicit transfer of runtime 2i validation (`probe_sealed_key_steal` / `concurrent_output_key_collision`) to INC-2 / OB-49. `INV-CAMERA-3` and `INV-OUTPUT-2` remain mandatory without waiver. OB-36 ratified `CLOSED` with CI workflow execution verified.
- **Registries:** unchanged (57 invariants, 20 `F-*`, OB-1…OB-51); checker PASS, self-test 18/18.
- **LIVE_SYSTEM_TOUCHED:** no.
- **NEXT:** INC-1 only upon explicit owner authorization.

## PHASE E FINAL REVIEW (recorded: `cfb54f091bd2a19065a45b32662e8f0bbddd62fc`)
- **Verdict:** `PHASE E PASS — PHASE F MAY BEGIN` (audit/phase-e-final/VERDICT.md). Baseline reviewed: E1R seal `1ae1b736515b8c52c3e6ba7596b22c81f7098390`, tracking `e9d36732ac4fa9e934ee1b90bb696e9545c598ab`. Zero unresolved Level-A findings.
- **Architecture:** unchanged by the review and by this entry; registries as in E1R (57 effective invariants; 20 `F-*`; OB-1…OB-51).
- **Downstream gates (from the verdict, not new):** OB-11 before INC-5; OB-24 (G-OB24) before INC-10; OB-34 (G-OB34) before INC-14; OB-45 at INC-3 exit; OB-46 for physical rotation claims; G-MIRROR; G-OB48.
- **NEXT:** Phase F entry increment **INC-0** (closes OB-19, unconditional entry gate), only on explicit owner authorization. Not begun.

## PHASE E CORRECTIVE CHECKPOINT E1R (sealed: `1ae1b736515b8c52c3e6ba7596b22c81f7098390`)
- **E1R_SEAL:** `1ae1b736515b8c52c3e6ba7596b22c81f7098390`. Independent re-review: `E1R PASS — READY TO SEAL AND RE-ENTER INDEPENDENT PHASE E REVIEW` (audit/phase-e-e1r-review/); accepted E1R-N1 correction (no second corrective layer) and P1 as the minimal pinned framework correction.
- **Record:** audit/E1R/ (README, DISPOSITIONS, REPRODUCTION, INVARIANT_DIFF, D8_REAUDIT, REGRESSION).
- **Layer:** FOUNDATION_ARCHITECTURE §C12.E1R amends 6 invariants (INV-CAMERA-3, INV-INPUT-5, INV-OUTPUT-1, INV-OUTPUT-2, INV-SCALE-2, INV-SCALE-5); 57 effective invariants; 20 `F-*` (10 amended incl. new F-RECONF-1); OB-45…51; EE-1…8; W-SM-4c, W-SM-16; gates G-OB48, G-MIRROR.
- **Level A:** E-F2-01 and E-F2-02 corrected with regression evidence and negative controls; E-F2-02's correction depends on dependency patch P1 or an upstream fix (OB-11 criterion 3 open). **New Level A E1R-N1 (F-HIT-1 transform) found and corrected; under the E1R rules it blocks E1R pending review.**
- **Checker:** PASS; self-test 18/18.
- **NEXT:** INDEPENDENT FINAL PHASE E REVIEW OF THE SEALED E1R BASELINE. Phase F remains locked until that verdict passes and is accepted.

## Baselines
- Phase A evidence: `2c8c163784e260af4aaa8c1dc05c6281b0476120`
- Phase B seal: `4a0807ed721ae257e1f4b0e5287d1dfd8c1038c1`
- Phase C checkpoint 1: `d596416cf57877116e9cfbc8cf6c6fe25c001c67`
- Phase C checkpoint 2: `fc2dd68f843b1eb05828f4116f94e8bab4f478da`
- **Phase C checkpoint 3A:** `795800937a1e50e56587cd4080a73ce27d8a8b01` (hash recorded in `fd9a80c`)
- **Phase C seal (checkpoint 3B):** `615b0ed2f3631b698b0c54783c28f696ef4ff34a`

## PHASE D CHECKPOINT 2 — D7–D9 (sealed: `c0a1cee310e56b75d32cf30152a09975ff776700`)
- **Package:** 11 living documents. New: FOUNDATION_INVARIANTS, _OBJECT_MODEL, _STATE_MACHINES, _TEST_PLAN, _RISKS, _IMPLEMENTATION_PLAN. Living regions appended to ARCHITECTURE, EVIDENCE, BLOCKERS, DECISIONS and the LEDGER; historical text untouched.
- **Namespace map** in ARCHITECTURE §P0; 17 namespaces, one home each.
- **Registries:** 57 effective invariants; 19 `F-*`; 11 `PROV-*`; 44 `OB-*` (OB-36 checker CI; OB-37…41 PD-F1-10; OB-42…44 PD-F1-11); 66 risks; 114 evidence entries; 34 objects; 22 machines; 25 tests; 23 ledger entries; 18 increments.
- **Findings in D7:** PD-F1-11 (D-F1, applied); PD-F0-10 and PD-F0-11 (editorial ADR notes). No new D-F2.
- **Checker:** `tools/check-foundation-docs` PASS; `--self-test` 12/12 seeded defects detected.
- **D9:** 19/19 PASS. Open D-F2: PD-F2-06 (B), PD-F2-13 (C), PD-F2-15 (C), PD-F2-16 (C).
- **NEXT:** Phase E (independent adversarial review) only after explicit authorization.

## PHASE D CORRECTIVE CHECKPOINT 2R (sealed: `5827926782089dc8c6a1c203f844fbc879fce9be`)
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
- **NEXT:** D7–D9 only after explicit user authorisation.

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
