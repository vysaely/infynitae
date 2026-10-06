# Third Review — Popups, Grabs, Ordered Preemption, Rule-1 Provenance, Synthetic Motion

## 1. Popup/grab state map
| State | Owner | Notes |
|---|---|---|
| ActiveElementFocus | Core FocusModel | coherent with Smithay keyboard focus (INC4R-01 resolved) |
| popup tree in Core snapshot | Core (from commits) | **stale after popup destruction until the parent commits** → R2-01 input hole |
| PopupManager popups/grab chain | Smithay | `cleanup()` called only before preemption/dismissal |
| `WaylandState::popup_grab` | adapter cache | filtered by `has_ended()`; **re-installed at reconnect without `cleanup()`** (t06d/t99c) |
| keyboard grab / pointer grab | Smithay handles | pointer grab installed with the *first* serial and kept (W-SM-14); preemption matches `g.serial()` or `previous_serial()` only → depth ≥ 3 missed (t06a); OB-4 path never unsets the pointer grab (owner decision on motion) → **stale `PopupPointerGrab` persists** (t01b, t99k) |
| `serial_outputs` | adapter (VecDeque, cap 64) | button serials → pointer output at the button; bounded (M08 removing the cap survives) |

## 2. Ordered preemption / N09
Canonical order holds in source. Observability (n09-wire.txt): moving `set_focus` before ungrab, between ungrab and keyboard unset, or between keyboard and pointer unset (N09a/b/c) gives a **byte-identical client wire** — Smithay's `PopupKeyboardGrab` ignores focus changes while installed. N09 is therefore observationally equivalent; the shipped control detects it only through production instrumentation (R2-08). Pointer-before-keyboard (N09d) and omitted keyboard unset (N09f) are caught only by the probe-feature NEG-2 comparison; N09f in the nested case is genuinely harmful (keyboard grab stays, W1 never entered). Omitted pointer unset (N09e) and dropped `previous_serial` clause (N09g) detected by default tests.

## 3. W-SM-7 / W-SM-14 / W-SM-15
- W-SM-7: refusal with `popup_done` (shipped PF-WSM7 now asserts it; M17 detected). PASS.
- W-SM-14: production default keep=true, setter gated behind `probe_nested_grab_keep_pointer`. PASS (residual grabs at depth ≥ 3 are R2-05).
- W-SM-15: permanent coverage only via the probe-feature NEG-2 comparison, run inside efficacy mutated trees; never executed unmutated in CI.

## 4. INC4R-06 attacks
| Attack | Result |
|---|---|
| R09b 2-level distinct serials | PASS |
| t06a 3-level distinct serials + focus transfer | `popup_done` P3,P2,P1 correct; **pointer grab remains** |
| R12 / t06b 3 zero-output cycles, click outside | dismissed; PASS |
| t06c nested + reconnect + focus transfer | PASS |
| t06d implicit grab + destroyed popup + zero outputs | zero-output grab kept; **after reconnect a grab is re-installed for the destroyed popup chain** |
| t01b nested + CameraPan | keyboard coherent; **pointer grab remains** |
| t99k popups away from the pointer | stale grab passes same-client events; persists after clicks (pan cases); 3-level: extra synthetic `motion` on next click |

## 5. R2-01 — destroyed popup input hole (root cause isolated)
t99j: popup (with or without grab) destroyed by the client → clicks inside its former area: `leave W1`, then nothing (0 buttons), indefinitely (t99e 0/20, across Super+Enter); clicks outside delivered; a parent commit cures it. No `popup_destroyed` handling exists in the adapter. Reproduced on the ratified INC-3 baseline `1ee520fa` (0/6) and on `d5b7b95`/`2bca2b9` (0/10). Earlier "lockout after pan" observations (t99a/d) are this hole, not the grab.

## 6. D-B6-BOUNDARY rule 1 — **RESOLVED**
Expected x: A 360, B −200, C 40 (all cameras centred on the parent; slide popup 600 wide at x = 390). T05: click A/pointer B/clock C → 360; A==B → 360; pointer on clock C → 360; 7 crossings → 360; earlier click on A, later click on B, A's serial used → 360. Causing output removed → 40 (rule 2). The causal relation is genuine: the recorded serial is the very `ButtonEvent` serial delivered to the client; the grab handler re-places the popup with that serial before the first configure. Key-serial and evicted-serial grabs fall back to the current pointer output (−200) — R2-10 (Level C). Storage bounded to 64 records (unguarded: M08).

## 7. Synthetic motion boundary (INC4R-10) — matches the owner decision
CameraPan dismissal single/nested: no client pointer/relative events (atk09, T10b). Zero-output cycle with a popup grab: client pointer wire identical to the no-popup control (T10). Keyboard focus change with a grab: one `motion` at the current location (no suppression invariant applies outside CameraPan in INC-4; INC-5 locks to re-adjudicate with W-SM-16). Not recorded in canonical text (R2-09).
