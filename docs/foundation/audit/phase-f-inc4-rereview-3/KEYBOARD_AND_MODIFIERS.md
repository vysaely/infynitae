# Fourth Review — Keyboard, Duplicates, Bindings

## R2-04 / M12 — RESOLVED
U04: {A, Shift, Ctrl, Alt, Super} × {press/dup/release, press/dup/dup/release, press/focus transfer/dup/release, press/popup/dup/release, press/zero outputs/dup/reconnect/release}: after release, seat depressed 0; a fresh focus transfer `enter`s with keys {} and depressed 0; Core owns no keys; 0 violations (independent oracle: client wire + seat observer). U04b: press, dup, release, orphan release → wire exactly `[(A,pressed),(A,released)]`. F01 (gate removed) detected by shipped r17/t04; F18 (orphan release synced) equivalent.

| Rule | Verdict |
|---|---|
| K1 | PASS |
| K2 | PASS |
| K3 | PASS |
| K4 | PASS |
| K5 | PASS |
| K6 | PASS — Esc during CameraPan (with a popup and a client-owned key held) cancels, camera frozen after cancel, Esc not leaked (U11); F13 detected |
| K7 | PASS |
| K8 | PASS for focus/keys; nested-menu close residual is R3-A1 (not output-related) |
| K9 | PASS |
| M12 | PASS at Core, wire and seat levels |

## R2-06 bindings — RESOLVED
Delivered 2/2: Super+B, Super+Escape, Super+K, Super+A, plain Escape, Shift, Super alone. Super+Enter consumed (0 delivered) and focus moved. Removing `KEY_ESC` from `CANONICAL_BINDINGS` did not regress K6: cancellation is the interaction rule in `resolve_key_binding`, not a product binding. F12 (Super+B re-added) detected.
