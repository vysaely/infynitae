# Third Review — Keyboard (K1–K9, M12), Modifiers, Bindings

## K1–K9 / M12 (client-wire and seat-state oracles)
| Rule | Verdict | Evidence |
|---|---|---|
| K1 | PASS (wire) | duplicate client presses (×2, ×3, after modifier, after focus transfer, across popup and zero outputs) emit no second `pressed` (T04, T04b); compositor repeat consumed |
| K2 | PASS | shipped release-owner tests; orphan release dropped |
| K3 | PASS (observe mode) | M14 detected |
| K4 | PASS | shipped k4/k5/k8 |
| K5 | PASS | shipped k3 |
| K6 | PASS | M15 detected by shipped `r08_k6_cancel_is_externally_observable` (camera + wire oracle) |
| K7 | PASS | t01c/t01d |
| K8 | PASS | t06b (grab semantics survive 3 zero-output cycles), t01f, atk15 |
| K9 | PASS | k9_ob15 |
| **M12** | **FAIL** | `Down(Client) → press` is suppressed on the wire but still applied to XKB by `sync_seat_key`; a duplicated Shift/Super press leaves the seat **and the client** with the modifier depressed after release (T04: Shift dep 1 / client 0x1; Super 0x40 / 0x40; T17 ends with seat 0x40). This is the stuck-modifier consequence INV-INPUT-6 exists to prevent (R2-04). |

## Modifier single authority (INC4R-07) — **PASS**
`pointer_button` now ignores the backend snapshot (`_modifiers`) and sends `seat_modifiers()`. T07: snapshot-logo/seat-none → no pan; seat-logo/snapshot-none → pan; MMB→Super no; Super↓↑→MMB no; Super→focus transfer→MMB pan; Super→zero outputs→reconnect→MMB pan. No one-event lag (key path reads the post-update XKB state; the button reads the same seat state at its boundary). M06 (snapshot restored) detected by 6+ shipped tests. INC-3 tests were adapted by pressing Super through the keyboard path (`common/input.rs` Rig, `t1_transform` n_pan) — assertions unchanged.

## Binding policy (INC4R-11) — **NOT RESOLVED**
T11 (delivered of 2): plain Esc 2; idle Super+Esc 2; **idle Super+B 0**; Super+K 2; Shift 2; configured Super+Enter 0 (correct, FocusNext). `CANONICAL_BINDINGS` still holds x10 fixtures: `Super+B → Noop` (swallows), `LeftShift → Noop` (observe, harmless), `Super+Esc → Cancel` (dead: Esc handled earlier). Canonical documents define no product binding owner; the only product action exercised is FocusNext. The round-2 matrix marks this RESOLVED while stating Super+B remains swallowed.
