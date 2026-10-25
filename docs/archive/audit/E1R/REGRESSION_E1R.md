# E1R — Regression record (EE-8)

Status: **UNSEALED**. Re-run of the historical experiments materially affected by E1R, on the published Smithay 0.7.0 (the sealed dependency) unless noted. "Identical" means byte-identical to the sealed evidence file apart from its recorded command/timestamp header (two lines). Shared harness change in E1R: `Intake::deliver_position` made `pub` (visibility only).

| Experiment | Why affected | Result | Classification |
|---|---|---|---|
| authority spike (`verify.sh`, tests) | output lifecycle, descriptor, pointer state, binding rule changed | verify PASS (steps 1–3 incl. 2f/2g unchanged, new 2h/2i); 14 tests pass. Three historical tests updated to assert the new pointer effects explicitly (first connect leaves `NoOutput`; last disconnect enters it); descriptor gains `transform` | **updated and passing** |
| xhit | F-HIT-1 changed (identical at Normal) | identical | unchanged and passing |
| d5hit | hit/projection across outputs | identical | unchanged and passing |
| xsnap | membership/snapshot (F-MEMBER-1 symbol) | identical | unchanged and passing |
| x6 | physical edges (F-LATTICE-1 symbol) | identical | unchanged and passing |
| xlock_grab | constraint lifecycle (INV-INPUT-5) | identical (published Smithay); under P1, EXPLICIT/persistent becomes valid, SEALED still fails (EE-7) | unchanged and passing; P1 behaviour change recorded |
| x8 | constraints during pan | identical; under P1 48/48 unchanged (EE-7) | unchanged and passing |
| x12 | W-SM-4b | identical (defect still reproduced on the published crate); under P1 not reproduced (EE-7) | unchanged and passing (negative control preserved) |
| xpopup_focus | INV-INPUT-10 (E1R-N2 relates) | identical | unchanged and passing |
| xmaximize | fullscreen/maximize policy (E-F2-07) | identical | unchanged and passing |
| scenarios (B8) | scale/membership scenarios | identical except one server log line already recorded as a pre-existing difference in phase-d/1r-regression-summary.txt:53 | unchanged and passing |
| cp3a model.py, feedback.py | F-SCALE-2/-3 | identical | unchanged and passing |
| d5_model.py, d5_model_1r.py, d5_model_2r.py | scale epochs / P7 | identical | unchanged and passing |
| checker + self-test | E-F1-01, E-F3-01 | PASS; self-test 18/18 (12 historical + 5 new + 1 boundary) | **updated and passing** |

Not re-run (not affected by any E1R change): x9 (presentation feedback), x10 (keyboard), rc1/realclient/scaleprobe/sendcheck (Phase B/C real-client probes, not touched), xwayland-satellite checks (C6/ER-6).

No historical experiment is failing or superseded.
