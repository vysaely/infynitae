# Phase E corrective checkpoint E1R — audit record

Status: **UNSEALED — stopped at the E1R review gate.** Uncommitted. Phase E is not declared passed. Phase F is not begun. No production crate or workspace exists.

## Baseline (verified before any change)

- Phase C seal `615b0ed2f3631b698b0c54783c28f696ef4ff34a`; Phase D falsification baseline `e8ec03a2ab36f315ae025723ec666b9ad278ac83`; 1R seal `84ce620898445aaadb72f386ec3954e5bbfe2c2f`; 2R seal `5827926782089dc8c6a1c203f844fbc879fce9be`; Phase D final seal `c0a1cee310e56b75d32cf30152a09975ff776700`; HEAD `09075a2d07dbc0cba862baac62136f9f3ff9e7b5` — all ancestors of HEAD.
- Phase E audit `audit/phase-e/` (untracked): sha256 of its 7 files recorded before E1R and re-verified at the gate; unmodified.

## Documents

| File | Content |
|---|---|
| [DISPOSITIONS_E1R.md](DISPOSITIONS_E1R.md) | one disposition per Phase E finding; E1R-N1…N3; Level-A accounting |
| [REPRODUCTION_E1R.md](REPRODUCTION_E1R.md) | Smithay/protocol trace, probes, matrices, wire sequences, decisions |
| [INVARIANT_DIFF_E1R.md](INVARIANT_DIFF_E1R.md) | per-invariant provenance (previous layer and wording, new wording, affected F/M/OBJ/ADR/OB/INC) |
| [D8_REAUDIT_E1R.md](D8_REAUDIT_E1R.md) | implementation-plan re-audit |
| [REGRESSION_E1R.md](REGRESSION_E1R.md) | historical regression re-runs (EE-8) |

Normative changes live in the package: FOUNDATION_ARCHITECTURE §C12.E1R (layer) and §F (formulas), FOUNDATION_INVARIANTS (projection), STATE_MACHINES, OBJECT_MODEL, DECISIONS, TEST_PLAN (OB-45…51), IMPLEMENTATION_PLAN, BLOCKERS (W-SM-4c, W-SM-16), EVIDENCE (EE-1…8), RISKS (RB-10).

## Experiments (HEADLESS / OFFLINE)

| Path | New/changed | Evidence |
|---|---|---|
| `experiments/b8-harness/src/bin/xtransform.rs` | new | EE-1 |
| `experiments/b8-harness/src/bin/xlock_batch.rs` | new | EE-2 |
| `experiments/b8-harness/src/bin/xzero.rs` | new | EE-3 |
| `experiments/b8-harness/src/input.rs` | `deliver_position` made `pub` | — |
| `experiments/e1r-smithay-p1/` | new isolated crate: shared sources built against Smithay 0.7.0 + P1 (vendored copy materialised from the local registry by `build.sh`; `vendor/`, `target/`, `Cargo.lock` ignored) | EE-2, EE-3, EE-7 |
| `experiments/c1-authority-spike/` | `output_reconfigured`, `Transform`, `PointerState`, collision rule; probes p10–p13, `probe_sealed_key_steal`; verify 2h/2i; 6 new tests, 3 updated | EE-4 |
| `experiments/cp3a-scale-model/e1r_hmax.py`, `e1r_fullscreen.py` | new | EE-5, EE-6 |
| `tools/check-foundation-docs` + fixtures 13–18 | E1R layer, `EE-*`, ADR prefix check, formula structure/provenance checks, boundary fixture | checker output |

## Live-system status

No package installed, updated or removed; Rust toolchain unchanged; no DRM/KMS access; no VT switch; Hyprland configuration and the live compositor untouched. All clients ran on private harness sockets.
