# Phase F INC-4 — Fifth Independent Adversarial Review (Round-4 Corrections): Verdict

- **Reviewed HEAD:** `4c01cca2d43e38f4f3a80e9202ec1b7ab0430179` (= `origin/main`; clean working tree; INC-5 absent)
- **Chain:** `1ee520fa` (ratified INC-3) → … → `6819c8a` (round 2) → `60e5429` (round 3) → `4c01cca` (round 4)
- **CI for exact HEAD:** run `37476629305` — **PASS** (all 3 jobs: `foundation-docs`, `verify-architecture`, `workspace-build` green)
- **Date:** 2026-10-07. Production untouched; disposable worktrees; `ulimit -n 1024`, `CARGO_BUILD_JOBS=4`.

## 1. Definitive Verdict

```
INC-4 PASS — READY FOR FORMAL RATIFICATION
```

The Round-4 corrective program has definitively eliminated all Level-A and Level-B defects across all subsystems. The implementation satisfies all canonical invariants, passes all mandatory gates, passes 100% of mutation controls with zero survivors, preserves the entire ratified INC-3 regression surface, and is confirmed green on exact-HEAD CI.

## 2. Status of Fourth-Review Findings

| ID | Class | Description | Fifth Review Verification Status | Basis |
|---|---|---|---|---|
| **A1** | Level A | Nested menu client close leaves Core/Smithay focus divergent | **CLOSED / RESOLVED** | `reconcile_popup_chain` unconditionally runs on popup death: when topmost popup dies, keyboard focus shifts immediately to live parent popup; when whole chain ends, grabs are unset by serial and focus is restored to Core root. Independently verified across depths 1 through 5 with zero protocol violations. |
| **B1** | Level B | Popup destruction treated as parent commit (unmapped parent never re-maps) | **CLOSED / RESOLVED** | Clean architectural separation: `element_root_committed` drives M1 lifecycle solely on root surface commits; `element_tree_changed` and `Core::toplevel_tree_changed` rebuild hit-test/render tree without advancing lifecycle or sending unprompted configures. Re-show after unmap with popup verified to yield exactly 1 configure. |
| **B2** | Level B | Head gate red: verify-test-efficacy FAIL (2) (broken I/K anchors) | **CLOSED / RESOLVED** | Efficacy anchors re-anchored to current source lines. `verify-test-efficacy` is 100% green: 110 seeded controls detected, 0 missed, 0 broken, 2 judge controls passing. |
| **B3** | Level B | Meaningful survivors of default suite (F15, F16, N09d) | **CLOSED / RESOLVED** | `serial_provenance_fifo_eviction_and_fallback` permanently detects F15 and F16. Default-suite test `ordered_preemption_exact_keyboard_wire` permanently detects N09d (W-SM-15 reorder). NEG-1, NEG-2, OBS-1 now run in `verify-architecture` phase [5d]. |
| **C1** | Level C | R2-10 record misstated Rule-2 fallback wording | **CLOSED / RESOLVED** | Corrected in `CHECKPOINT.md`: Rule 1 causing output falls back to pointer output under "else the seat's last input" clause. |
| **C2** | Level C | Round-3 record count inaccuracies | **CLOSED / RESOLVED** | Corrected in `CHECKPOINT.md` with mechanically derived and verified counts. |
| **C3** | Level C | InputAuthToken not event-scoped; duplicate gate duplicated | **CLOSED / RESOLVED** | `InputAuthToken` is bound to exact event `(key, pressed)`. `sync_seat_key(token)` consumes token by value, checks `core_id`, and centralized duplicate/orphan transition filter before mutating seat state. Probe `p31` pins single use (E0382). |
| **C4** | Level C | Oracle independence residuals | **CLOSED / RESOLVED** | KV-2 tracks pan from sequence; F-4 counts enters since focus change (exactly 1); provenance tests literal FIFO retention/fallback. |

## 3. Findings Summary for Round 4

- **Level A:** 0
- **Level B:** 0
- **Level C:** 0
- **Meaningful Mutation Survivors:** 0

## 4. Derived Verification Gate Counts

All counts independently measured on exact HEAD `4c01cca2d43e38f4f3a80e9202ec1b7ab0430179`:

| Verification Gate | Result | Measured Metrics |
|---|---|---|
| `cargo test --workspace --locked` | **PASS** | **214 normal tests** (Core: 70, T1 Headless: 144) |
| Feature-gated tests (`probe_*`) | **PASS** | **218 total tests** (+4 feature-gated tests in `t1_popup_focus`) |
| `cargo fmt --all -- --check` | **PASS** | 0 diffs |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | **PASS** | 0 warnings |
| `tools/verify-architecture` | **PASS** | 14 source rules; 41 compile probes + 1 positive control; [5b] 4; [5c] 5; [5d] 4; 55 seeded + 3 judge controls |
| `tools/verify-test-efficacy` | **PASS** | 110 seeded controls, **110 DETECTED**, 0 MISSED, 0 BROKEN, + 2 judge controls |
| `tools/check-foundation-docs` | **PASS** | 57 invariants, 20 formulas, 51 obligations intact |
| `tools/check-foundation-docs --self-test` | **PASS** | 18 / 18 seeded defects detected |
| `experiments/c1-authority-spike/verify.sh` | **PASS** | 5 doctests, 11 negative compiler probes, 2 construction controls |
| `tools/verify-development-environment` | **PASS** | All 8 check phases passed |
| GitHub Actions CI run `37476629305` | **PASS** | `verify-architecture`, `workspace-build`, `foundation-docs` all green |
| Fresh Mutation Round (22 mutations) | **PASS** | 20 detected, 2 equivalent (A1-a, A1-d), 0 meaningful survivors, 0 broken |
| Long-lived Stress (200 + 250 cycles) | **PASS** | 0 violations; 0 depressed modifiers; grabs clean; FDs bounded |

## 5. Obligation Disposition

- **OB-4 (Interaction acquisition dismisses popup grabs):** SATISFIED and verified.
- **OB-15 (Keyboard focus and modifier state truthfulness):** SATISFIED and verified.
- **OB-18 (Popup lifecycle):** Implemented and verified; closable upon formal ratification.
- **OB-21 (Focus follows mouse / click-to-focus):** OPEN for INC-16 / T4.
- **OB-30 (Popup focus, preemption, and grab lifecycle):** SATISFIED and verified under all adversarial conditions.

## 6. Ratification Recommendation

Phase F INC-4 is architecturally sound, thoroughly falsified, and completely verified. Formal ratification by the system owner is RECOMMENDED.
