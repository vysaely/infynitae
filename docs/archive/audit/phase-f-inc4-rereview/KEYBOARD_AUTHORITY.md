# INC-4 Re-Review — Keyboard Authority (F01), K1–K9, M12

## 1. What is now structurally enforced (and why)

| Claim | Status | Why it holds / fails |
|---|---|---|
| 1. every production keyboard intake path into **Core** requires `InputPort` | **holds** | the only Core entry that changes OBJ-25 is `physical_input(&mut InputPort, PhysicalEvent::KeyboardKey)`; `check(port.core_id)` precedes the match; no other method writes `input.key_ownership` |
| 2. no alternate adapter path decides ownership | **holds** | the decision code exists only in `entry.rs`; the adapter receives `KeyboardForward`/`KeyboardIntercept`; `effects-only-from-core` prevents the adapter from constructing them |
| 3. adapter cannot mutate the ownership table | **holds (compiler)** | `Ownership::press/release` are `pub(crate)`; only `&Ownership` escapes (`keys()`, `key_ownership()`) |
| 4. ownership authoritative in Core | **holds** | — |
| 5. foreign/forged/missing authority fails closed | **FAILS** | `keyboard_key` calls the adapter's `sync_seat_key` (Smithay `input_intercept`, i.e. OBJ-27 + LEDs) **before** `inject` checks the port. R01: foreign-port Shift → `Err(ForeignPort)` but seat `depressed 0x0→0x1`, and the next `enter` tells the client Shift is down |
| 6. ElementId unforgeable externally | **holds** | `pub(crate) const fn new` (p23 E0624); no `From`/deserialize |
| 7. focus not mutable without authority | **holds at Core, untested at runtime** | `Core::set_focus` takes `&mut InputPort` and refuses non-mapped targets; `focus_next` `pub(crate)`. Removing the port check (N17) or the mapped check (N18) survives the shipped suite |
| 8. effects do not hand policy back to the adapter | **mostly holds** | `KeyboardIntercept` is a no-op; `KeyboardForward` carries Core's decision. But the adapter still owns runtime switches that change INV-INPUT-10/W-SM-14 policy (INC4R-03) |
| 9. no hidden duplicate ownership state | **holds for OBJ-25**; Smithay `forwarded_pressed_keys` is the protocol-side set and is driven only by `KeyboardForward` | — |

Additional portless, production-public adapter calls (all compile from production backend code with all 13 structural rules still passing — scratch probe): `sync_seat_key` (XKB mutation, R02), `set_preemption_strategy(Naive)`, `set_nested_grab_keep_pointer(false)`, `test_set_keyboard_grab` (R14: all client keys swallowed).

**F01 verdict: PARTIALLY RESOLVED.** The Level-A inversion (adapter deciding and writing OBJ-25) is gone and is now enforced by visibility, not merely by tests. The physical-intake boundary is still not fail-closed and its XKB half is portless (INC4R-02, Level B).

## 2. Probe analysis

| Probe | Proves | Does not prove |
|---|---|---|
| p22 `core.key_ownership_mut()` → E0599 | the deleted method name is absent | anything about other mutation routes (those are closed by `pub(crate)`, which no probe pins: a future `pub fn keys_mut` would pass p22) |
| p23 `ElementId::new` → E0624 | external construction blocked | — (adequate) |
| p24 `core.set_focus(None)` → E0061 | the signature has a second parameter | that the parameter is checked (N17 survives) or that it is an `InputPort` |
| source rule: `key_ownership_mut` forbidden in `infynitae-wayland` | nothing (name no longer exists anywhere) | — |

## 3. K1–K9 and M12 (client-wire oracles; production ownership never used as the expected value)

| Rule | Verdict | Evidence |
|---|---|---|
| K1 | **FAIL (partial)** | compositor press consumed, compositor repeat consumed (ATK-02 port); **client repeat forwarded twice → `pressed 30 already logically down`** (R03) |
| K2 | PASS | release follows remembered owner (ATK-03; mutation F detected); orphan release dropped (ATK-04; N05 detected) |
| K3 | PASS | observe mode in Core, unconditional (AB detected; R05) |
| K4 | PASS | k4/k5/k8 wire checks |
| K5 | PASS | k3 |
| K6 | PASS (behaviour) | R08: after Esc cancels a pan, motion moves no camera, the owned MMB release is consumed, Esc never reaches the client; N20 detected. The shipped K6 test's oracle is Core state (`interaction().active()`), R08 supplies the independent oracle |
| K7 | PASS | k6, R06 (unmap/destroy sequence), ATK-05/06 ports |
| K8 | **FAIL** | keyboard state is preserved across removal, but output removal + reconnect strips the popup grab of a still-grabbing popup (R12) |
| K9 | PASS | k9_ob15 |
| **M12** | **FAIL** | `Down(Client) → press` transition unguarded (R03) |

Escape: idle → delivered (ATK-01 port); active pan → Cancel, consumed (R08). Super+Esc idle → swallowed (INC4R-11).
