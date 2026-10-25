# INC-4 Review — Adversarial Attacks, Mutations, Oracle Independence, Counts

All runs: scratch git worktrees of `d5b7b95` under the session scratchpad, `ulimit -n 1024`, production HEAD never modified. Reproduction material: `reproduction/` (attack suite `zz_review.rs.txt`, runner `mutate.py.txt`, cross-check `crosscheck.py.txt`, logs).

## 1. Independent attack suite (T1, production core + adapter + headless backend, client-wire oracles)

| ID | Attack (canonical expectation) | Result on HEAD | Finding |
|---|---|---|---|
| ATK-01 | plain Esc reaches the focused client | **FAIL** — swallowed | F04 |
| ATK-02 | repeat press of compositor-owned key does not leak | **FAIL** — `(48, pressed)` delivered | F03 |
| ATK-03 | release follows press owner across a modifier change | PASS (kills mutation F) | — |
| ATK-04 | ownerless release not forwarded | **FAIL** — "released 30 not logically down" | F03 |
| ATK-05 | focus_next never selects an unmapped element | **FAIL** — W2 unmapped focused, `enter W2` sent | F07 |
| ATK-06 | successor never unmapped | **FAIL** | F07 |
| ATK-07 | no successor fabricated without history | **FAIL** — never-focused W2 focused | F11 (C) |
| ATK-08 | `activated` tracks ActiveElementFocus | PASS | — |
| ATK-09 | OB-4 dismissal emits no pointer events during CameraPan acquisition | **FAIL** — `motion 45,40` | F05 |
| ATK-10 | keyboard focus change synthesizes no pointer motion | **FAIL** — `motion 50,50` (canonical consequence of INV-INPUT-10 + W-SM-16) | F10 (C) |
| ATK-11 | refused grab dismisses the popup | **FAIL** — no `popup_done` (kills mutation N on its first assertion) | F08 |
| ATK-12 | preemption keeps the zero-output grab | **FAIL** — torn down at zero outputs | F09 |
| ATK-13 | pan modifier read from the seat XKB state | **FAIL** — Super held, no pan | F16 |
| ATK-14 | stale popup grab does not kill an implicit button grab | **FAIL** | F09 |
| ATK-15 | output removal keeps the popup keyboard route (K8) | **FAIL** — `leave P, enter W1`, no `popup_done` | F09 |
| ATK-16 | OBS-1 (no W-SM-14 workaround) observably fails | confirmed: route lost (kb active W1, not grabbed) — shipped OBS-1 asserts nothing | F12 |
| ATK-17 | NEG-2 (pointer-first) observably differs from ORDERED | confirmed: transient `enter W2 / leave W2` — shipped NEG-2 asserts nothing distinguishing (kills mutation I) | F12 |
| ATK-18 | popup of an unmapped parent is dismissed (M3) | **FAIL** | F17 (C) |
| RES-1 | 40 cycles of client/keyboard/nested-popup/grab/focus/output-unplug-replug/destroy | PASS — `/proc/self/fd` 4 → 4 | — |

## 2. Independent mutations (each alone; full shipped workspace suite as the detector)

| ID | Mutation | Expected detector | Actual | Verdict |
|---|---|---|---|---|
| A | binding evaluated with pre-update modifier snapshot (one event late) | k3/k4 | suite green | **EQUIVALENT** for the shipped bindings (XKB updated for the modifier's own event; no binding depends on its own key's transition) |
| B | modifier update dropped on client release | KV-3 | k2,k3,k4,k6,k7,k8,k9 | DETECTED |
| C | binding leaks press | k3 | k3,k4,k7,neg_k3 | DETECTED |
| D | binding leaks release | k3 | k3,k4,k7 | DETECTED |
| E | client key swallowed | k1/k5 | 9 tests | DETECTED |
| F | release owner recomputed | K2 test | suite green | **MISSED** (ATK-03 kills it) |
| G | stale focus survives destruction | k6/pf3 | k6, pf3 | DETECTED |
| H | fallback picks wrong element | successor test | core focus test | DETECTED |
| I | preemption order swapped | NEG-2 / F-4 | suite green | **MISSED** (ATK-17 kills it) |
| J | kb grab survives focus transfer | PF-1 | suite green | **MISSED** |
| K | pointer grab survives focus transfer | PF-1 | suite green | **MISSED** |
| L | foreign-sequence refusal removed | — | suite green | **MISSED** |
| M | wrong-client serial accepted | — | — | **NO MECHANISM TO MUTATE** (Observation F18) |
| N | root/focus check bypassed | PF-WSM7 | suite green | **MISSED** (ATK-11 kills it) |
| O | adapter ignores DismissPopupGrab | PF-OB4 | pf_ob4 | DETECTED |
| P | rule 1 visibility guard dropped | core rule-1 test | suite green | **MISSED** |
| Q | old toplevel keeps `activated` | activation test | suite green | **MISSED** (no test exists) |
| R | new toplevel never activated | activation test | suite green | **MISSED** |
| S | keys dropped at zero outputs | k8_ob15 | k8_ob15 | DETECTED |
| T | backend skips InputPort check | foreign-port keyboard test | suite green | **MISSED** (and structurally unenforceable: adapter entry is portless) |
| U | click-to-focus removed | T1 click test | suite green | **MISSED** (ATK-09 kills it) |
| V | W-SM-14 workaround default off | PF-6 | suite green | **MISSED** (PF-6 forces the switch on) |
| W | unmap of focused element keeps focus | K7 unmap | suite green | **MISSED** |
| X | focus_next backwards | core test | core focus test | DETECTED |
| Y | prev `ToplevelConfigure` omitted (core) | core test | core focus test | DETECTED |
| Z | grab does not focus popup | PF-6/7 | pf6, pf7 | DETECTED |
| AA | preemption without any unset | PF-* | pf1…pf7 | DETECTED |
| AB | K3 enforcement default off | k8 | suite green | **MISSED** (tests construct with `k3_enforced = true` explicitly) |
| AC | DismissPopupGrab never emitted (core) | PF-OB4 | pf_ob4 | DETECTED |
| AD | history not purged on loss | — | suite green | **EQUIVALENT** (successor filtered by the live list) |

Totals: 30 mutations attempted (29 applied + M not applicable); **13 DETECTED, 14 MISSED (all meaningful), 2 EQUIVALENT, 0 BROKEN, 1 no-mechanism**. Cross-check: F, I, N, U are each killed by an independent attack test with an on-target message — the survivals are test-suite gaps, not untestable behaviour.

## 3. Test-oracle independence

| Oracle | Assessment |
|---|---|
| t1_keyboard KV-2 / t1_popup_focus F-6 | **circular**: "client-owned held keys" computed from production `core().key_ownership()`; a key mis-classified *and* not forwarded satisfies it (F-6 additionally compares client keys with a filter of themselves) |
| KV-3 | sound: compares client wire with Smithay seat state (the authority) |
| KV-1 / wire checker | sound, independent protocol model |
| F-1 | uses production `window(id)` mapping; acceptable |
| F-4 | **declared, not implemented** |
| F-5 | `>=` count only, no order |
| NEG-1 | asserts only missing `popup_done`, not the divergence ER-1 predicts |
| NEG-2 / OBS-1 | **assert nothing that distinguishes the predicted failure**; OBS-1 has zero assertions |
| PF-WSM7 | asserts only "no keyboard violations"; refused-grab semantics unchecked; the root-mismatch panic path is unreachable by construction |
| rule-1 core test | independent literals and distinct outputs (good), but `if`-guarded |
| focus core test | hand-derived expected effects (good) |
| binding resolver test | literals (good) |

## 4. Test / control count reconciliation (mechanical)

- Test functions (static, unique names): INC-3 117 → INC-4 144; **0 removed, 0 renamed, 27 added** (3 core, 13 `t1_keyboard`, 11 `t1_popup_focus`; the oracle self-test is shared).
- Executed tests (`cargo test --workspace --locked`, `ulimit -n 1024`): INC-3 corrective evidence 122 → INC-4 **151** = 122 + 3 core + 26 T1 (24 new + oracle self-test in the 2 new binaries). Same 151 in CI run 37344638073.
- Efficacy controls: INC-3 78 (76 seeded + 2 judge) → INC-4 **83** (81 seeded + 2 judge); 0 removed, 0 re-anchored, 5 appended. All 81 DETECTED locally and in CI.
- The implementation report / CHECKPOINT / evidence README figures **"106 workspace tests" and "68/68 controls" are wrong**; the committed evidence files themselves show 151 and 81 DETECTED (finding F14). No coverage was lost.
- INC3-F09: `inc3_f09_bounded_resource_cycles` present (`t1_transform.rs:478`) and executed (7/7 t1_transform pass under 1024). INC3-F06: per-invocation `mkdtemp(prefix=test-efficacy-<pid>-)` unchanged; two concurrent invocations observed with distinct `CARGO_TARGET_DIR`s and no residue after interrupt (`reproduction/concurrency.txt`).
