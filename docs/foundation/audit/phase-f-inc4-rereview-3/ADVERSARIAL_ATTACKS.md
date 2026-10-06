# Fourth Review — Attacks, Mutations, Oracles, Counts

Attack suite `reproduction/u4_attacks.rs.txt` (results `key-results.txt`, `u4-run-main.txt`); scratch harness helper `toplevel_uncommitted` (diff included); attribution runs on `6819c8a` (`zz_base_6819c8a.rs.txt`).

## Fresh mutations (detector = shipped suite, default features; `mutations4.txt`, `mutate5.py.txt`)
| Mut | Target | Result |
|---|---|---|
| F01 | duplicate seat mutation restored | DETECTED |
| F02 | cross-Core token accepted | DETECTED |
| F07 | destruction does not purge hit target | DETECTED |
| F09 | PanDismissed grab never cleared | DETECTED |
| F10 | reconnect liveness check removed | EQUIVALENT |
| F11 | preemption ignores install serial | DETECTED |
| F12 | Super+B swallowed | DETECTED |
| F13 | Escape never cancels | DETECTED |
| F14 | serial store unbounded | DETECTED |
| **F15** | evicts the newest record | **MISSED** (U12/U12c kill it) |
| **F16** | unknown serial aliases latest record | **MISSED** (U12b/U12c kill it) |
| F17 | synthetic motion at pan end | DETECTED |
| F18 | orphan release synced to XKB | EQUIVALENT |
| F19 | no PanDismissed grab installed | DETECTED |
| F20 | popup_destroyed keeps grab bookkeeping | EQUIVALENT |
| N09d | pointer unset before keyboard unset | **MISSED** (only feature-gated NEG-2; efficacy control I BROKEN) |
| N09e | pointer unset omitted | DETECTED |
| N09f | keyboard unset omitted | DETECTED |
Totals: 18 applied — 12 detected, 3 meaningful missed, 3 equivalent, 0 broken mutations. Not expressible as a mutation of current code, demonstrated by attack: R3-A1, R3-B1. Token reuse/Send are compile-time impossible (consumed value; p30).

## Oracles
| Oracle | Class |
|---|---|
| KV-2 | PARTIAL — still reads `core().interaction().active()` for Escape |
| F-4 | PARTIAL — "last enter names target", not exactly one |
| NEG-1 / NEG-2 / OBS-1 | INDEPENDENT assertions, but feature-gated and run by no gate unmutated |
| R17 (shipped) | INDEPENDENT — now asserts 0 violations and 0 seat/client modifiers |
| M08 (shipped) | PARTIAL — boundedness only; eviction semantics untested |

## Counts (mechanical)
Tests 195 (69 + 126); probe-feature t1_popup_focus 36 (total 199); source rules 14; compile probes 40 + 1 positive; architecture 55 seeded + 3 judge (+ tree identity); efficacy 99 seeded (97 DETECTED, **2 BROKEN**) + 2 judge; foundation 57 invariants, 51 obligations. Round-3 CHECKPOINT figures "34 compile probes" and "54 seeded" are stale; "195 tests" correct.
