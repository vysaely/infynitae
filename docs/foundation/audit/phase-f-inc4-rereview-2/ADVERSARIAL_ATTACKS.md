# Third Review — Attacks, Mutations, Oracles, Counts

## 1. Attack suite (`reproduction/t3_attacks.rs.txt`, results in `t3-run-main.txt`, `t99-t17-results.txt`)
| Test | Target | Result |
|---|---|---|
| t01a/a2/c/d/e/f | INC4R-01 A, C–F | PASS |
| t01b | INC4R-01 B | keyboard PASS; residual pointer grab (R2-05) |
| t01g / T17 | stress | 0 violations; T17 ends with seat Super stuck (R2-04) |
| T02 | fail-closed backend path | PASS |
| T02b / T02c | token binding / reuse | FAIL (R2-02) |
| T04 / T04b | duplicate presses | wire PASS; modifier XKB stuck (R2-04) |
| T05 / T05b | rule 1 | PASS (edge fallbacks noted, R2-10) |
| t06a / t06d | nested depth 3 / reconnect re-install | FAIL (R2-05) |
| t06b / t06c | zero-output grab semantics | PASS |
| T07 | modifier authority & ordering | PASS |
| T10 / T10b | synthetic motion boundary | PASS |
| T11 | bindings | FAIL Super+B (R2-06) |
| T13 | preemption wire, single & nested | PASS |
| t99a–k | residual grab vs input hole disentanglement | R2-01, R2-05 |
| zz_hole (on `1ee520fa`) | latent origin of R2-01 | 0/6 delivered |

## 2. Previous five survivors — see VERDICT §3 (N08, N17, N18, N21 DETECTED; N09 EQUIVALENT on the wire, killed only by production instrumentation).

## 3. New mutation round (`mutations3.txt`, `n09-wire.txt`; detector = shipped suite at default features, as CI runs it)
| Mut | Class | Result |
|---|---|---|
| N09a/b/c | focus before/within teardown | MISSED — **equivalent** (identical wire) |
| N09d | pointer before keyboard | MISSED at default; DETECTED with probe features (neg2) |
| N09e | pointer unset omitted | DETECTED |
| N09f | keyboard unset omitted | MISSED at default; DETECTED with probe features; harmful nested |
| N09g | previous_serial dropped | DETECTED (r09b) |
| M01 | token minted without port check | DETECTED |
| M03 / M04 | skip / wrong OB-4 reconciliation | DETECTED |
| M05 | duplicate press guard removed | DETECTED |
| M06 | snapshot authority restored | DETECTED |
| M07 | current pointer output instead of causal serial | DETECTED |
| **M08** | serial records unbounded | **MISSED** (meaningful) |
| M10 | reconnect without grab restore | DETECTED |
| M12 | unconfigured Super+key swallowed | DETECTED |
| M13 | synthetic pointer replay at OB-4 | DETECTED |
| M14 | K3 disabled | DETECTED |
| M15 | Esc never cancels | DETECTED |
| M16 | zero-output popup refocus removed | DETECTED |
| M17 | refusal without popup_done | DETECTED |
| M18 | grab re-placement removed | DETECTED |
| M19 | button serials not recorded | DETECTED |
Totals (27 applied + N09g re-anchored): 20 detected at default features (+2 more with probe features), 3 equivalent, 1 meaningful missed (M08), 0 broken after re-anchoring. Not expressible as a mutation of current code but demonstrated by attack: token reuse/foreign token (R2-02), keyboard-handle bypass (R2-03), duplicate modifier XKB (R2-04 — no shipped test), stale grabs (R2-05 — no shipped test), Super+B (R2-06 — shipped tests *assert* Super+B is consumed).

## 4. Oracle classification
| Oracle | Class |
|---|---|
| KV-2 (t1_keyboard) | **PARTIALLY CIRCULAR** — hand-written reference model with literal bindings, but reads `core().interaction().active()` for Escape and hard-codes the Super+B fixture |
| F-6 (t1_popup_focus) | INDEPENDENT (compares client keys with the harness's own held set) |
| F-4 | **PARTIAL/WEAK** — "last enter names the target", not "exactly one transition" |
| PF-WSM7 | INDEPENDENT (asserts `popup_done` and no keyboard grab) |
| NEG-1 | PARTIAL (missing `popup_done` + kb grabbed; not the Core/Smithay divergence); probe-feature only |
| ATK-12 port | INDEPENDENT (focus-change step restored; Smithay grab state) |
| N09 control | CIRCULAR (production instrumentation) |
| shipped r17 | PARTIAL (bounds only; no violation assertion) |

## 5. Counts (mechanical)
Tests 185 = core 69 + T1 116 (adversarial 13, input 17, keyboard 32, outputs 10, parity 6, popup_focus 27, slice 4, transform 7); +4 with probe features = 189. verify-architecture: 13 source rules; 38 probes + 1 positive control; [5b] 4; [5c] 5; [6] 54 seeded + 3 judge + 1 tree identity. verify-test-efficacy: 98 seeded + 2 judge = 100. CI 37433415984: 185 tests, 98 detected. All claims match.
