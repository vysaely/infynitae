# Phase F INC-4 — Independent Adversarial Corrective Re-Review: Verdict

- **Reviewed HEAD:** `2bca2b9a204c7ae94b77a4c5d83a12c9e6ddef2d` (= origin/main)
- **Failed implementation:** `d5b7b95cbc910c48377c72ae94ccec2b0c05e140`
- **Failed review dossier:** `audit/phase-f-inc4-review/` (commit `19231ef`)
- **Corrective matrix:** `aa216d3`
- **Pre-INC-4 baseline:** `1ee520fa0079d45193f2a7274b31018e9276b0ed`
- **Date:** 2026-10-06
- Production implementation not modified; all experiments ran in disposable worktrees. Dossier left untracked.

## 1. Definitive verdict

```
INC-4 FAIL — FURTHER ARCHITECTURAL CORRECTION REQUIRED
```

The corrective commit made real progress:
- Core now owns key ownership. OBJ-25 is structurally closed: no external mutator, the decision logic lives in `Core::physical_input`, and the adapter only executes `KeyboardForward`/`KeyboardIntercept`.
- The unmapped-focus, MRU-successor, refusal, Escape and OB-4 pointer-replay defects are fixed.
- All 14 previously surviving mutations are now detected.

It does not deserve ratification:
- One Level A focus divergence exists on the production path. It was latent since `d5b7b95` and was missed by the first review.
- Eight Level B findings remain. Three original findings are unresolved: F02 (runtime negative-control switches), F06 (rule-1 provenance) and F14 (counts).
- Five meaningful new mutations survive the shipped suite.

## 2. Status of prior findings

| ID | Prior level | Re-review status | Basis |
|---|---|---|---|
| F01 | A | **PARTIALLY RESOLVED.** The Level A core is resolved; a Level B residual remains (R-02). | Ownership is now in Core and the port-gated `physical_input` is the only key path into Core; `Ownership::press/release` are `pub(crate)`; `ElementId::new` is `pub(crate)`; `Core::set_focus` requires a port and a mapped target. **But** the XKB half of intake (`WaylandState::sync_seat_key`) is public and portless and runs *before* the port check. A rejected foreign-port Shift leaves the seat depressed and is announced to clients (R01, R02). |
| F02 | B | **NOT RESOLVED** | `set_preemption_strategy(Naive\|PointerFirst)` and `set_nested_grab_keep_pointer` are still public runtime setters (only `#[doc(hidden)]`). There is a new public `test_set_keyboard_grab` that hijacks the keyboard (R14). No `probe_*` gating and no structural rule. |
| F03 | B | **PARTIALLY RESOLVED** | Compositor repeat (ATK-02) and orphan release (ATK-04) are fixed. A repeated **client-owned** press is forwarded as a duplicate `pressed` (R03, a protocol must-not). |
| F04 | B | **PARTIALLY RESOLVED** (Level B part resolved; residual reclassified Level C, R-11) | Plain Escape reaches clients when idle and cancels an active pan (R08). The x10 harness fixtures are still product bindings: Super+B (no-op) and Super+Esc (even when idle) are swallowed. |
| F05 | B | **RESOLVED** (pointer side) | No pointer or relative events during the pan, and the pan end is identical to a no-popup control (R10); N07 is detected. Adjacent focus defect: R-01. |
| F06 | B | **NOT RESOLVED** | Production substitutes the *current pointer output at popup placement* for the causing event's output. With click on A, pointer on B and clock output C, the popup is constrained to B (R15: x = −200; canonical A gives 360). Removing the adapter wiring entirely is undetected (N21). The rule-1 selection control was retired. |
| F07 | B | **RESOLVED** | R06/R07 pass; `mapped_elements()` is used for every live set. Verification gap: N18 is undetected by the shipped suite (counted under R-08). |
| F08 | B | **RESOLVED** | Every adapter refusal sends `popup_done`; Smithay's own `Err` cases dismiss or post protocol errors (`manager.rs:111-127`). |
| F09 | B | **PARTIALLY RESOLVED** | The zero-output grab survives preemption and reconnect (R11), and the implicit grab is safe (ATK-14). Remaining: a nested grab with distinct serials leaves a stale `PopupPointerGrab` after preemption (R09b). A zero-output cycle silently strips the grab from a still-grabbing popup, so an outside click no longer dismisses it (R12). |
| F10 | C | **NOT ADJUDICATED** | The implementation agent recorded its own "formal adjudication". No owner decision and no canonical text. Behaviour is unchanged: synthetic motion on every keyboard focus change with a grab. |
| F11 | C | **RESOLVED** | No history means Unfocused, literally as C4.5 states; no requirement was weakened (R06). |
| F12 | B | **PARTIALLY RESOLVED** | The 14 replays are detected. Still open: N08, N09, N17, N18 and N21 survive; KV-2/F-6 are still circular; F-4 is still not computed; PF-WSM7 and NEG-1 are unchanged; the ported ATK-12 dropped its focus-change step. |
| F13 | C | **RESOLVED** (record issue R-12) | OB-21 is attributed to INC-16 / T4. |
| F14 | B | **NOT RESOLVED** | The evidence says "170 tests … 100+ controls"; the actual figures are **171** tests and **95** controls (93 seeded + 2 judge). |
| F15 | Obs. | **RESOLVED** | — |
| F16 | B | **PARTIALLY RESOLVED** | The button modifiers are now `snapshot OR seat`. A backend `logo` with no Super held still acquires a pan (R04). The canonical "seat only" change (N03) breaks 15 shipped tests. |
| F17 | C | **RESOLVED** | Implements the existing canonical M3 row (ATK-18). |

## 3. New findings catalog

### Level A — 1
- **INC4R-01 — Core/Smithay keyboard-focus divergence after OB-4 dismissal.**
  - **What happens:** after a CameraPan dismisses a popup grab, Smithay's keyboard focus stays on the popup surface. The client destroys that surface in response to `popup_done`. Core's ActiveElementFocus remains W1, but W1 never receives `enter`. Subsequent keys go to the client while it has no entered surface ("key 30 without active surface"; R16; 600 violations over 150 R17 cycles).
  - **Canonical basis:** INC-4 plan stop rule ("any divergence between core focus and Smithay focus is a Level-A regression (PD-F2-02 class)"); INV-INPUT-8 (delivered routing within the focused element's tree, or none); M11 (grab end → root restore inside E).
  - **Origin:** latent at `d5b7b95` (reproduced there) and missed by the first review, whose ATK-09 checked only pointer events. The corrective commit neither introduced nor fixed it. No shipped test detects it (mutation N08 survives).

### Level B — 8
- **INC4R-02 (F01 residual) — XKB intake is portless and not fail-closed.** `WaylandState::sync_seat_key` is public and needs no port. `keyboard_key` calls it before `inject` checks the port (R01, R02; N24 shows no shipped test covers it).
- **INC4R-03 (F02) — Invariant-disabling switches and a keyboard-hijack hook ship in the production adapter (R14).** No structural rule exists; the source rules still pass with all four calls made from production backend code.
- **INC4R-04 (F03 residual) — Duplicate client `pressed` on a repeated client-owned press (R03).**
- **INC4R-05 (F06) — Rule-1 provenance is wrong** (current pointer output, not the causing event's output). The adapter wiring is untested (N21), and the selection control was retired.
- **INC4R-06 (F09 residual) — Popup grab bookkeeping.** After nested-grab preemption with distinct serials, a stale `PopupPointerGrab` remains (R09b). A zero-output cycle strips the grab from a still-grabbing popup (R12).
- **INC4R-07 (F16 residual) — The backend modifier snapshot is still an authority (R04, N03).**
- **INC4R-08 (F12 residual) — Verification gaps.** Meaningful survivors: N08 (OB-4 keyboard unset), N09 (focus before `ungrab`), N17 (`set_focus` port check), N18 (`set_focus` mapped check), N21 (rule-1 wiring). The circular KV-2/F-6 oracle, uncomputed F-4, vacuous PF-WSM7 and weak NEG-1 remain.
- **INC4R-09 (F14) — Evidence counts are not exact.**

### Level C — 3
- **INC4R-10 (F10):** owner adjudication of INV-INPUT-10 pointer `unset_grab` motion replay (W-SM-16 interaction) is still required before INC-5.
- **INC4R-11 (F04 residual):** the canonical documents define no product key bindings; the x10 fixtures remain hard-coded (Super+B no-op and idle Super+Esc swallowed).
- **INC4R-12:** CHECKPOINT history was rewritten in place. The INC-4 implementation entry was deleted, and the INC-3 ratification and implementation entries were edited, breaking the append-only practice used for INC-1…INC-3.

### Observations — 5
- **INC4R-13:** two reproduction files of the failed-review dossier (`*.log`) were excluded from commit `19231ef` by `.gitignore` (they exist only on disk). The seven Markdown files and five other artifacts are unchanged since `19231ef`.
- **INC4R-14:** K6 cancel ends the pan with no reconciliation (M22 does not require one; relevant to INC-5 INV-INPUT-5).
- **INC4R-15:** N01 (one-event-late modifiers) is equivalent under the current bindings, so the risk is latent.
- **INC4R-16:** p22 proves only that a deleted method name no longer resolves; p24 proves only arity; the `key_ownership_mut` forbidden-name rule is vacuous.
- **INC4R-17:** pre-existing Level C INC1-F01 (INC-7) is still open.

**Open counts: Level A 1, Level B 8, Level C 3 (+ INC1-F01).**

## 4. Verification (scratch worktree of HEAD, `ulimit -n 1024`)

| Check | Result |
|---|---|
| `cargo check` / `build` (`--workspace --locked`) | PASS / PASS |
| `cargo test --workspace --locked` | PASS, **171** tests: core 69; t1_adversarial 13, t1_input 17, t1_keyboard 23, t1_outputs 9, t1_parity 6, t1_popup_focus 23, t1_slice 4, t1_transform 7 |
| `fmt --check` / `clippy -D warnings` | PASS / PASS |
| `verify-architecture` | PASS. 13 structural rules; 34 compile probes + 1 positive control (p22–p24 new); [5b] 4; [5c] 5; [6] 58 ok lines (unchanged; no new seeded structural control); 1 DEFERRED (shell_command) |
| `verify-test-efficacy` | PASS, **93/93 seeded + 2 judge = 95**. Versus 83: +15 added, −3 (K1 and K3 re-anchored to Core, D-B6 `match None` retired) |
| `check-foundation-docs` / `--self-test` | PASS / PASS |
| `c1-authority-spike/verify.sh` | PASS |
| `verify-development-environment` | PASS |
| CI run 37375174576 (push, exact HEAD) | foundation-docs, verify-architecture and workspace-build all success; 171 tests; 93 DETECTED |

## 5. Required next action
1. **Fix INC4R-01.** OB-4 dismissal must leave Smithay keyboard focus on the focused element's root, the equivalent of INV-INPUT-10 step 4, without reintroducing pointer replay. Add a client-wire regression test (the R16 oracle) and make N08 a permanent control.
2. **Fix INC4R-02.** Port-check before any XKB mutation, and make `sync_seat_key` non-public or effect-driven. Add a fail-closed test for a rejected key event.
3. **Fix INC4R-03.** Gate the NEG modes and the foreign-grab hook behind `probe_*` features covered by verify-architecture hygiene.
4. **Fix INC4R-05.** Record the causing-event output at `grab(serial)` or at the input event, distinguish it from the current pointer output, add an R15-style T1 test, and restore a rule-1 selection control.
5. **Fix INC4R-04, -06, -07, -08, -09,** each with independent-oracle tests and permanent controls for N08, N09, N17, N18 and N21.
6. **Obtain owner adjudication of INC4R-10, -11 and -12.** Restore the deleted CHECKPOINT history as an append-only record.
7. This dossier stays unchanged; corrective work is recorded separately. **INC-5 is not authorized.**
