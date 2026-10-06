# Phase F INC-4 — Round-4 Corrective Evidence

- **Fourth review:** `audit/phase-f-inc4-rereview-3/` — `INC-4 FAIL — FURTHER CORRECTION REQUIRED` (A 1, B 3, C 4); ingested byte-for-byte as `dc4d52e`.
- **Matrix and design:** [`CORRECTIVE_FINDING_MATRIX.md`](CORRECTIVE_FINDING_MATRIX.md) (with §Closure), [`ARCHITECTURE_RECONSTRUCTION.md`](ARCHITECTURE_RECONSTRUCTION.md).
- **Commits:**
  - `b320cbe` docs (matrix and reconstruction);
  - `a63b7c5` A1+B1;
  - `9da07d7` C3;
  - `d75746b` tests and oracles;
  - `cf743e5` verification infrastructure;
  - this record.
- **Status:** **INC-4 is NOT ratified.** Ready for the fifth independent adversarial review. INC-5 not begun.

## Verification (working tree = `cf743e5`, `ulimit -n 1024`, `CARGO_BUILD_JOBS=4`)

| Check | Result | Log |
|---|---|---|
| `cargo check --workspace --locked` | PASS | `matrix-exit-codes.txt` |
| `cargo build --workspace --locked` | PASS | |
| `cargo test --workspace --locked` | PASS, **214** | `test-matrix.txt` |
| `cargo fmt --all -- --check` | PASS | |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS | |
| `./tools/verify-architecture` | PASS | `verify-architecture.txt` |
| `./tools/verify-test-efficacy` | PASS | `verify-test-efficacy.txt` |
| `./tools/check-foundation-docs` | PASS | |
| `./tools/check-foundation-docs --self-test` | PASS | |
| `./experiments/c1-authority-spike/verify.sh` | PASS | |
| `./tools/verify-development-environment` | PASS | `verify-development-environment.txt` |
| probe-feature `t1_popup_focus` | PASS, 36 | `probe-feature-suite.txt` |
| per-commit `cargo test` and `fmt` for `a63b7c5`, `9da07d7`, `d75746b` | PASS | `per-commit-checks.txt` |

## Mechanically derived counts

| Item | Count |
|---|---|
| normal workspace tests | **214** = core unit **70** + integration (T1) **144** (adversarial 13, input 17, keyboard 36, outputs 12, parity 6, popup_focus 32, popup_lifetime 17, slice 4, transform 7) |
| probe-feature tests | `t1_popup_focus` **36** with `probe_preemption_strategy,probe_nested_grab_keep_pointer,probe_test_keyboard_grab` (+4 over its default 32) → **218** in total |
| compiler probes | **41** + **1** positive control (p31 new) |
| source rules | **14** |
| runtime checks | [5b] **4**, [5c] **5**, [5d] **4** (new: NEG-1, NEG-2, OBS-1, foreign-grab refusal, unmutated) |
| architecture seeded controls | **55** + **3** judge (+ tree-identity check) |
| efficacy seeded controls | **110**: **110** DETECTED, **0** MISSED, **0** BROKEN; **2** judge controls |
| foundation registries | **57** invariants, **20** formulas, **51** obligations |

## Verification topology

- **Default product gate (`cargo test --workspace --locked`, CI `workspace-build`)** owns every product regression, including the A1/B1 suite, W-SM-15 exact wire (`ordered_preemption_exact_keyboard_wire`), the F15/F16 provenance semantics, F-4 and KV-2.
- **`verify-architecture` (CI job)** [5d] owns NEG-1, NEG-2, OBS-1 and the foreign-grab refusal: they run unmutated with their probe features and must pass. Each asserts its predicted failure mode, which proves the detector works.
- **`verify-test-efficacy` (CI `workspace-build`)** owns mutation injection (110 controls); probe features are used only where a negative mode is itself the subject.

## Fresh mutation round (`fresh-mutations.txt`, runner `fresh-mutations-runner.py.txt`)

22 mutations against `cf743e5`, with the default shipped suite as the detector:
- **20 DETECTED;**
- **0 meaningful MISSED;**
- **2 EQUIVALENT** (A1-a, A1-d — argued from pinned Smithay in the matrix §Closure);
- **0 BROKEN.**

Classes covered: A1 teardown/focus/depth/cleanup, B1 lifecycle/hit-test/configure, provenance bound/order/alias/fallback, W-SM-15 order and keyboard-unset omission, authority (gate, cross-Core, key substitution), Super+B fixture.

## Stress (`round4_long_lived_combined_stress`, default suite)

- **Load:** 200 cycles in one session. They cover depth-1/2/3 chains closed by the client or dismissed by focus change, CameraPan with destruction during the pan, zero outputs + reconnect, duplicate Super, and > 64 provenance events.
- **Asserted end state:**
  - FDs: 10 → 10;
  - 0 `wl_keyboard` violations;
  - seat and client depressed modifiers 0;
  - key and button ownership empty;
  - no pointer grab, no keyboard grab;
  - provenance store ≤ 64 (64);
  - Smithay keyboard focus equals the root of Core's focus.
- **Destroyed popups:** absent from hit-testing (`b1_nested_zero_output_and_repeated_destroy`: 40/40 clicks over popup churn).
