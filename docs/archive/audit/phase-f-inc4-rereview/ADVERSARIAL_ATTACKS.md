# INC-4 Re-Review — Attacks, Mutations, Oracles, Counts

All runs: disposable worktrees of `2bca2b9` (and one of `d5b7b95` for R16), `ulimit -n 1024`. Artifacts: `reproduction/`.

## 1. Independent attack suite (R-series, client-wire / external oracles)

| ID | Attack | HEAD result | Finding |
|---|---|---|---|
| R01 | foreign-port Shift must leave no XKB trace | **FAIL** — `Err` returned, seat `depressed 0x0→0x1`, client told Shift is down | INC4R-02 |
| R02 | portless `sync_seat_key` changes client-visible modifiers | **FAIL** | INC4R-02 |
| R03 | repeated client-owned press not duplicated | **FAIL** — `pressed 30 already logically down` | INC4R-04 |
| R04 | snapshot `logo` without seat Super must not pan | **FAIL** — pan acquired | INC4R-07 |
| R05 | temporal modifier boundaries (Super→MMB, MMB→Super, Super released before MMB) | PASS | — |
| R06 | MRU sequence A→B→C, destroy/unmap/destroy/destroy, create D | PASS | — |
| R07 | `set_focus` on unmapped refused | PASS | — |
| R08 | K6 cancel externally observable | PASS | — |
| R09 | nested preemption, shared serial | PASS | — |
| R09b | nested preemption, distinct serials | **FAIL** — stale `PopupPointerGrab` | INC4R-06 |
| R10 | pan with popup: no events during pan; end == no-popup control; keyboard returns to element | **FAIL** (keyboard part) — pointer parts PASS | INC4R-01 |
| R11 | failed-review ATK-12 (with focus change) + nested + reconnect | PASS | — |
| R12 | popup grab across zero outputs; click outside dismisses | **FAIL** | INC4R-06 |
| R13 | root destroyed with live grab | PASS | — |
| R14 | production `test_set_keyboard_grab` hijack | **FAIL** — client keys swallowed | INC4R-03 |
| R15 | rule 1: causing A ≠ pointer B ≠ clock C | **FAIL** — constrained to B | INC4R-05 |
| R16 | Smithay keyboard focus vs Core after OB-4 dismissal | **FAIL** — focus on dead popup; keys without active surface; also reproduced on `d5b7b95` | INC4R-01 |
| R17 | 150-cycle long-lived stress | bounded (fds 10→10, history 1, 0 owned keys/buttons, 1 element, no residual grabs) **but 600 client protocol violations** | INC4R-01 |

## 2. Replay of the 14 previously missed mutations

| Mut | Re-targeting | Detecting test | Oracle independent? | Result |
|---|---|---|---|---|
| F | release owner recomputed (now in Core) | release_follows_press_owner…, release_without_press…, repeat_press… | yes (client wire) | DETECTED |
| I | pointer before keyboard | neg2_pointer_first_teardown_control | yes (wire diff ORDERED vs pointer-first) | DETECTED |
| J | keyboard unset omitted | neg2_pointer_first_teardown_control | yes | DETECTED |
| K | pointer unset omitted | pf1_grab_active_core_focuses_w2_ordered | partly (Smithay `is_grabbed`) | DETECTED |
| L | foreign-sequence refusal removed | pf_grab_refused_when_keyboard_grabbed… | yes (`popup_done`), but uses the production `test_set_keyboard_grab` hook | DETECTED |
| N | root/focus check bypassed | atk11_refused_grab_dismisses_the_popup | yes | DETECTED |
| P | rule-1 visibility guard dropped | core rule-1 test | yes (literals, distinct outputs) | DETECTED |
| Q | Activated never unset | activated_tracks_active_element_focus | yes (wire) | DETECTED |
| R | Activated never set | same | yes | DETECTED |
| T | port check skipped for keys in Core (re-targeted: the old adapter path no longer exists) | keyboard_key_rejects_foreign_port | yes (`Err`) — but does not check XKB (R01) | DETECTED |
| U | click-to-focus removed | click_to_focus_focuses_mapped_window | no (Core focus state) | DETECTED |
| V | W-SM-14 default off | pf6_nested_popup_grab_keeps_pointer_grab | yes | DETECTED |
| W | unmap keeps focus | unmapped_focused_element_loses_focus | no (Core focus state) | DETECTED |
| AB | K3 disabled (now in Core) | k2/k3/k4/k6/… + focus_next… | yes (KV-3 vs seat) | DETECTED |

**14/14 detected.**

## 3. New mutations (distinct failure classes)

| Mut | Class | Result | Detector |
|---|---|---|---|
| N01 | one-event-late modifiers (pre-update snapshot) | MISSED — **EQUIVALENT** for the shipped bindings | — |
| N02 | button modifiers from snapshot only | DETECTED | atk13, plain_escape_cancels… |
| N03 | button modifiers from seat only (canonical design) | "DETECTED" by 15 tests — the suite *requires* the second authority (evidence for INC4R-07) | — |
| N04 | repeated compositor press guard removed | DETECTED | repeat_press… |
| N05 | orphan release forwarded | DETECTED | release_without_press… |
| N06 | refusal without `popup_done` | DETECTED | atk11 |
| N07 | OB-4 reintroduces pointer `unset_grab` | DETECTED | atk09 |
| N08 | OB-4 skips keyboard unset | **MISSED** | — |
| N09 | focus set before `ungrab` | **MISSED** | — |
| N10 | `ungrab(All)` omitted | DETECTED | pf1/pf2/pf6/pf7 |
| N11 | stale popup state preserved | DETECTED | atk14 |
| N12 | unconditional unsets (pre-correction) | DETECTED | atk12 |
| N13 | zero-output popup refocus removed | DETECTED | atk15 |
| N14 | unmapped parent popups kept | DETECTED | atk18 |
| N15 | z-order fallback reintroduced | DETECTED | successor_without_history… |
| N16 | focus_next over unmapped | DETECTED | focus_next_skips_unmapped… |
| N17 | `set_focus` port check removed | **MISSED** | — |
| N18 | `set_focus` mapped check removed | **MISSED** (R07 kills it) | — |
| N19 | plain Esc always compositor-owned | DETECTED | keys unit test, plain_escape_reaches… |
| N20 | Cancel does not end interaction | DETECTED | plain_escape_cancels… |
| N21 | rule-1 adapter wiring removed | **MISSED** | — |
| N22 | forward drops modifiers update | DETECTED | k2/k3/k4… |
| N23 | intercepted key forwarded anyway | DETECTED | k3/k4/k7… |
| N24 | port check moved before XKB sync (a candidate *fix*) | suite green — no test distinguishes fail-open from fail-closed | INC4R-02 |

Totals (24 new): 17 detected (N03 counted separately as evidence), 1 equivalent (N01), **5 meaningful missed (N08, N09, N17, N18, N21)**, 1 probe (N24), 0 broken.

## 4. Oracle independence verdicts

| Oracle / test | Verdict |
|---|---|
| KV-2 (t1_keyboard) / F-6 (t1_popup_focus) | **still circular** (expected set filtered through production `key_ownership()`) |
| F-4 | **still not computed** (header only) |
| NEG-2 | **now non-vacuous** (wire comparison) |
| OBS-1 | **now non-vacuous** (asserts lost routing) |
| PF-WSM7 | **still vacuous** (refusal covered instead by atk11) |
| NEG-1 | unchanged (asserts missing `popup_done` only) |
| K6 test | Core-state oracle; independent behaviour confirmed by R08 |
| click-to-focus / unmap-focus tests | Core-state oracles |
| ported ATK tests | client-wire oracles (good); ATK-12 port **dropped the focus-change step** that made it adversarial |

## 5. Counts (measured)
- Tests: **171** = core 69 + T1 102 (adversarial 13, input 17, keyboard 23, outputs 9, parity 6, popup_focus 23, slice 4, transform 7); CI identical.
- verify-architecture: 13 source rules; 34 compile probes + 1 positive control; [5b] 4; [5c] 5; [6] 58 ok lines (unchanged since INC-3).
- verify-test-efficacy: 93 seeded + 2 judge = **95** (83 − 3 + 15). Evidence claims "170 tests, 100+ controls" (INC4R-09).
