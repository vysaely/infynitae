# Phase F INC-4 — Fourth Independent Adversarial Review (Round-3 Corrections): Verdict

- **Reviewed HEAD:** `60e5429821793ac2745ec6a9fcc171217061f224` (= origin/main; clean tree; INC-5 absent)
- **Chain:** `1ee520fa` → … → `6819c8a` (round 2) → `033be36` (third dossier) → `60e5429` (round 3)
- **CI for HEAD:** run 37450191322 — **failure** (`workspace-build`: `verify-test-efficacy: FAIL (2)`)
- **Date:** 2026-10-06. Production untouched; disposable worktrees; `ulimit -n 1024`, `CARGO_BUILD_JOBS=4`.

## 1. Definitive verdict

```
INC-4 FAIL — FURTHER CORRECTION REQUIRED
```

Round 3 resolved most of what it targeted:

- **Former-popup-area input hole:** fixed.
- **Duplicate keys:** clean at the Core, client wire *and* seat/XKB levels for A, Shift, Ctrl, Alt and Super in every context.
- **Auth token:** now Core-bound, consumed and `!Send`, and fails closed without an assert.
- **Keyboard handle:** confined.
- **`PanDismissedPopupGrab`:** correct in every pan scenario attacked.
- **Reconnect:** no longer resurrects dead grabs.
- **Bindings:** fixed, with K6 intact.
- **Production instrumentation:** removed.
- **Long-run stress:** clean (0 violations; bounded state).

It is still not ratifiable:

- **Level A:** a client closing a nested menu (depth ≥ 2) leaves Core/Smithay keyboard focus divergent and produces a protocol violation on the next key. This is latent and was missed by every prior review, including mine.
- **Level B:**
  - The R2-01 fix treats popup destruction as a parent commit, which can **prevent a window from ever re-mapping**.
  - The HEAD's own efficacy gate and CI are **red**.
  - Meaningful mutations survive.

## 2. Third-review findings — status

| ID | Status | Basis |
|---|---|---|
| R2-01 input hole | **RESOLVED** (but see R3-B1) | Former-area clicks delivered (U01a), including immediate destroy, zero-output destroy and reconnect, and 20× create/destroy (U01e); F07 is detected by shipped `t99g`/`t99j`. |
| R2-02 token | **RESOLVED** (Level C residual R3-C3) | `core_id` comes from a process-wide `AtomicU64` (the same mechanism as the ratified `InputPort` binding); the token has a private field, is not `Clone`, is consumed by value, and is `!Send` via `PhantomData` (p30 E0277). A cross-Core token is rejected silently with no mutation (U02); there is no assert or panic path. |
| R2-03 keyboard confinement | **RESOLVED** (Observation) | `keyboard_handle()` is `pub(crate)` (p29 E0624); the `smithay-keyboard-only-in-adapter` rule forbids `get_keyboard`/`input_forward`/`input_intercept` outside the adapter and core. Enforcement is visibility plus source scan, the same standard as the ratified INC-3 pointer rule. `KeyboardHandle::from_resource(&WlKeyboard)` is not on the scanner list but needs a server-side `wl_keyboard` resource (exotic). |
| R2-04 duplicates / M12 | **RESOLVED** | U04: 25 key×context cases leave the seat, client and Core clean; a legitimate release is never lost (U04b); F01 is detected by shipped tests. |
| R2-05 stale grabs | **RESOLVED** | `PanDismissedPopupGrab`: popup destroyed during the pan, nested inner destroyed, three-level destroyed, focus change and Esc cancel all show no events during the pan, the grab gone after the first post-pan event, and clicks delivered (U05); 30 cycles clean (U05b); three-level preemption clean (U09); reconnect resurrects nothing (U10). F09, F11, F17 and F19 are detected. |
| R2-06 bindings | **RESOLVED** | `CANONICAL_BINDINGS` = {Super+Enter → FocusNext}. Super+B, Super+Esc, Super+K, Super+A, Esc, Shift and Super are all delivered; Super+Enter is consumed and moves focus; Esc during a pan cancels with no leak (U11). |
| R2-07 verification | **PARTIALLY RESOLVED** | Shipped r17 now asserts zero violations and zero modifiers, and M08/F14 is detected. **But** F15/F16 (eviction semantics), N09d (W-SM-15 reorder) and the broken I/K controls remain (R3-B2, R3-B3); KV-2 still reads Core interaction state for Escape; F-4 is still "last enter"; NEG/OBS still run in no gate. |
| R2-08 instrumentation | **RESOLVED** | The flags, the `seat_state` hook and the production assert are gone; no replacement found. |
| R2-09 decision record | **RESOLVED** | Appended to CHECKPOINT round 3 (a change-control record; no canonical text rewritten); behaviour matches. |
| R2-10 rule-1 fallback | **Behaviour acceptable; record wrong** (R3-C1) | An evicted, unknown or missing serial resolves to the current pointer output (U12/U12b) — defensible as canonical rule 1's "else the seat's last input" clause. The record calls it "Rule 2 (output containing the pointer cursor)", but canonical Rule 2 is the parent's **frame-clock output**. |

## 3. Findings

### Level A — 1
- **R3-A1 — Nested-menu close leaves Core/Smithay keyboard focus divergent.**
  - **What happens:** a client closes a grabbing popup chain of depth ≥ 2 itself, in protocol-mandated reverse order (P2, P1 or P3, P2, P1). The `PopupKeyboardGrab` stays installed, Smithay keyboard focus sits on a dead surface while Core ActiveElementFocus is the root, and the client has no active surface.
  - **Consequence:** the next key arrives as `enter(root, keys={K})` **plus** `key K pressed`, a `wl_keyboard` protocol must-not ("pressed 37 already logically down"; D02, C09).
  - **Scope:** depth 1 is clean. This is the ordinary path for every submenu that closes on a click inside the menu.
  - **Canonical basis:** INC-4 plan stop rule ("any divergence between core focus and Smithay focus is a Level-A regression (PD-F2-02 class)"); INV-INPUT-8; INV-INPUT-7 (no impossible key state).
  - **Origin:** reproduced identically on `6819c8a`; latent across rounds and missed by all three prior reviews, mine included (none closed a nested menu from the client side and then typed).
  - **Root cause:** `popup_destroyed` clears state only when `has_ended()` or the destroyed surface is `current_grab()`. When the last popups of a chain die one by one, neither condition releases the keyboard grab.

### Level B — 3
- **R3-B1 — Popup destruction is treated as a parent commit (introduced by round 3).**
  - **Mechanism:** `popup_destroyed` calls `element_committed(parent)`, which runs `Core::toplevel_committed`, i.e. the **M1 lifecycle state machine**, not just a hit-test snapshot rebuild. For an unmapped parent (null-buffer commit, so its popup is dismissed and the client destroys it), the lifecycle advances Unmapped → AwaitingFirstBuffer and sends an unsolicited configure with no client commit.
  - **Consequence:** when the client later performs the protocol-required re-initial commit, it gets **no configure** (D01: 0 with a popup vs 1 without; 1 on `6819c8a`). A conforming client waiting for that configure can never re-map the window.
  - **Further risk:** the `else` branch (root not resolvable) "commits" **every** element. My U01d attempt did not reach that branch, so it is recorded as an unproven risk.
  - **Pending client state:** none is exposed — extraction reads committed state.
- **R3-B2 — The HEAD's own gate is red.** `verify-test-efficacy: FAIL (2)`: controls **INC-4 I** and **INC-4 K** are BROKEN (their seed anchors no longer exist after the round-3 rewrite of `outputs.rs`). CI run 37450191322 on the exact HEAD failed. The round-3 record claims verify-test-efficacy PASS.
- **R3-B3 — Meaningful survivors of the shipped default suite.**
  - **N09d** (pointer `unset_grab` before keyboard `unset_grab`, the W-SM-15 reorder): its efficacy control (I) is broken and the only detector, NEG-2, is feature-gated and runs in no gate, so W-SM-15 currently has **no working automated detector**.
  - **F15** (evicts the newest record) and **F16** (an unknown serial aliases the latest record): the M08 test checks only `len == 64`; the reviewer's U12/U12b/U12c kill both.

### Level C — 4
- **R3-C1:** the R2-10 record misstates canonical Rule 2. Correct the wording, or adjudicate the "seat's last input" reading explicitly.
- **R3-C2:** record inaccuracies.
  - The R2-02 text says `sync_seat_key` "asserts"; the code fails closed silently, which is the better behaviour.
  - CHECKPOINT round 3 states "34 compile probes"; the actual count is 40 (+1 positive control).
  - It states "54 seeded"; the actual count is 55 (+3 judge).
- **R3-C3:** the authority boundary is not event-scoped. The duplicate-transition gate lives in the *headless backend*, so every future backend must replicate it. A same-Core token can sync XKB for a key Core never sees (U02: seat Shift held, Core none). The port holder is trusted, so this is a design residual for the owner.
- **R3-C4:** oracle residuals: KV-2 reads Core for Escape; F-4 is not "exactly one"; NEG-1/NEG-2/OBS-1 still run in no gate unmutated.

### Observations
- **O-1:** U01b. A popup *commit* on a parent that never made its initial commit runs the parent's lifecycle (initial configure) — pre-existing INC-1 snapshot-on-commit design, protocol-legal but exotic.
- **O-2:** the R2-01 historical record is accurate (latent since `1ee520fa`, found in the third review, corrected in round 3); INC-3 records are unchanged; shipped `t99g`/`t99j` prevent recurrence.
- **O-3:** equivalent mutations F10, F18 and F20 (redundant guards).

**Open: Level A 1, Level B 3, Level C 4.**

## 4. Verification (scratch worktree of HEAD)

| Check | Result |
|---|---|
| `cargo check` / `build` | PASS |
| `cargo test --workspace --locked` | PASS, **195** = core 69 + T1 126 (adversarial 13, input 17, keyboard 36, outputs 11, parity 6, popup_focus 32, slice 4, transform 7) |
| probe-feature `t1_popup_focus` | **36** (+4 → 199) |
| `fmt` / `clippy -D warnings` | PASS / PASS |
| `verify-architecture` | PASS — 14 source rules; **40** compile probes + 1 positive control; [5b] 4; [5c] 5; [6] **55** seeded + 3 judge + tree identity |
| `verify-test-efficacy` | **FAIL (2)** — 99 seeded (97 DETECTED, 2 BROKEN) + 2 judge |
| `check-foundation-docs` / `--self-test` | PASS / PASS (57 invariants; 51 obligations × 7 fields) |
| authority spike | PASS |
| `verify-development-environment` | **FAIL** (only because it runs efficacy) |
| CI 37450191322 | foundation-docs pass, verify-architecture pass, workspace-build **fail** |

## 5. Next action
1. **R3-A1:** when a destroyed popup leaves its chain without a live grabbing popup, perform the INV-INPUT-10 teardown (keyboard `unset_grab`, pointer grab by install serial) and `set_focus(root)` *before* the next input. Add a client-wire regression for depth 2/3 close + key.
2. **R3-B1:** on popup destruction, rebuild only the hit-test/render snapshot (`set_tree`), without running the M1 lifecycle transition. Add a D01-style test (unmap with popup → re-initial commit → exactly one configure) and remove or guard the all-elements `else` branch.
3. **R3-B2/B3:**
   - re-anchor the I and K controls;
   - add a default-suite detector for the W-SM-15 reorder;
   - add eviction-semantics tests (newest survives, evicted never aliases);
   - get CI green on the exact HEAD.
4. Correct the R3-C1/C2 records; adjudicate R3-C3.

INC-5 is not authorized. INC-4 is not ratified.
