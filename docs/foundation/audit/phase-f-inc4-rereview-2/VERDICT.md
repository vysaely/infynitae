# Phase F INC-4 — Third Independent Adversarial Review (Round-2 Corrections): Verdict

- **Reviewed HEAD:** `6819c8a5e28baca46ae73e4f58ce73ba3e5c27ca` (= origin/main; clean tree)
- **Chain:** `1ee520fa` (INC-3 ratified) → `f296a73` → `d5b7b95` (failed) → `19231ef` → `aa216d3` → `2bca2b9` (round 1) → `9efcf85` → `99ec2f0` → `6819c8a` (round 2)
- **CI:** run 37433415984, headSha `6819c8a5…`, success (3/3 jobs)
- **Date:** 2026-10-06. Production untouched; all experiments ran in disposable worktrees with `ulimit -n 1024` and `CARGO_BUILD_JOBS=4`. No live-system action.

## 1. Definitive verdict

```
INC-4 FAIL — FURTHER CORRECTION REQUIRED
```

Round 2 fixed what it targeted most:
- **INC4R-01:** Core/Smithay keyboard focus is now coherent in every attacked scenario (A–G), with zero protocol violations over 150–160-cycle stress.
- **Rule-1 causal provenance:** correct for pointer-caused popups, including a later click on another output.
- **Modifier single authority:** holds.
- **Append-only CHECKPOINT:** restored.
- **Counts:** exact.

It still does not deserve ratification. **No Level A** remains, but **seven Level B** findings are open:
- the duplicate-press path leaves a **stuck modifier**;
- a destroyed popup leaves an **input hole** that swallows clicks (latent since the ratified INC-3 baseline, made routine by INC-4's popup paths);
- **stale pointer grabs** persist after CameraPan dismissal and deep nesting;
- the auth token is not bound to its Core;
- the public `keyboard_handle()` re-opens a portless keyboard capability;
- the x10 fixture binding Super+B still swallows a key;
- a meaningful mutation survives.

## 2. Prior findings (second review) — status

| ID | Status | Basis |
|---|---|---|
| INC4R-01 | **RESOLVED** | t01a/a2/c/d/e/f coherent (Core target = Smithay focus = client `enter`; key legally delivered); t01b keyboard coherent; 160-cycle t01g and 150-cycle T17 have 0 violations. N08, M03 and M04 are detected (shipped `r16`, `r10`). |
| INC4R-02 | **PARTIALLY RESOLVED** | The backend path is fail-closed: T02 covers foreign Shift/Super/A press and release, valid press → foreign release, and foreign press → valid release, with no trace in XKB, Core or the wire; M01 is detected. **But** `InputAuthToken` is not bound to its Core, not consumed and `Send`: a foreign Core's token mutates this seat (T02b), and a retained token is a permanent portless capability (T02c). |
| INC4R-03 | **PARTIALLY RESOLVED** | The three setters and the hook are `#[cfg(feature = "probe_*")]`. No default feature, dev-dependency or other crate enables them (resolver 2; backend probe features forward only explicitly). **But** a capability equivalent remains: public `keyboard_handle()` gives `input_forward`/`set_focus`/`set_grab` to any holder, and no keyboard confinement rule exists (R2-03). New production instrumentation was added (R2-08). |
| INC4R-04 | **PARTIALLY RESOLVED** | No duplicate wire `pressed` (M05 detected). **But** `sync_seat_key` still feeds the duplicate press into XKB: after Shift/Super pressed twice and released once, the seat and the client keep the modifier depressed (T04; T17 ends with seat `0x40`). |
| INC4R-05 | **RESOLVED** | T05: click on A with the pointer on B and the clock on C gives A; A==B gives A; pointer on the clock output gives A; seven crossings give A; the serial of an earlier click on A still means A after a later click on B. Causing output removed gives rule 2 (C). N21, M07, M18 and M19 are detected. Residual Level C: key-serial and evicted-serial popups fall back to the current pointer output (R2-10). |
| INC4R-06 | **PARTIALLY RESOLVED** | R09b (2-level), R12 (T06b, 3 zero-output cycles) and t06c pass. **Fail:** 3-level nested preemption leaves the pointer grab (t06a, t99k); CameraPan dismissal leaves a stale `PopupPointerGrab` installed indefinitely (t01b, t99k); reconnect re-installs a grab for a client-destroyed popup (t06d, t99c). |
| INC4R-07 | **RESOLVED** | T07: snapshot `logo` with seat none gives no pan; seat `logo` with snapshot none pans; MMB→Super, Super down/up→MMB, Super→focus transfer→MMB, and Super→zero outputs→reconnect→MMB all behave canonically, with no one-event lag. M06 is detected. |
| INC4R-08 | **PARTIALLY RESOLVED** | The five old survivors are covered (§3). New survivors: M08 (serial-record bound). N09d/N09f are caught only by the probe-feature NEG-2 comparison, which never runs unmutated in any gate. The shipped `r17` does not assert zero violations. |
| INC4R-09 | **RESOLVED** | 185 tests (69 + 116), 189 with probe features, 38 compile probes, 54 + 3 = 57 architecture controls, 98 + 2 = 100 efficacy controls; all reproduced exactly. |
| INC4R-10 | **RESOLVED in behaviour** (Level C doc residual R2-09) | No client pointer or relative events during CameraPan dismissal, single or nested (T10b, atk09; M13 detected); reconnect with a popup gives a wire identical to the control (T10). Focus-change motion is allowed by the owner decision. The decision text lives only in the evidence matrix. |
| INC4R-11 | **NOT RESOLVED** (misreported RESOLVED) | Idle **Super+B is swallowed** (T11: 0/2 delivered). The matrix itself states "Super+B remains a compositor noop binding". The x10 fixture `LeftShift → Noop` and the dead `Super+Esc` entry also remain in `CANONICAL_BINDINGS`. |
| INC4R-12 | **RESOLVED** | Every section of CHECKPOINT at `1ee520fa` and `d5b7b95` is byte-present in HEAD; the round-1 body is byte-identical (only its heading gained "— ROUND 1"); round 2 is appended. |

## 3. Five previous survivors

| Mut | Result | Detector | Oracle |
|---|---|---|---|
| N08 | DETECTED | `r16_keyboard_focus_after_pan_dismissal` | client wire + Smithay focus |
| N09 (focus set before teardown) | **EQUIVALENT on the client wire**. Independent variants N09a/b/c produce a wire byte-identical to unmutated code, because `PopupKeyboardGrab` absorbs early `set_focus`. The shipped control is killed only by a production `assert!` that instruments the reorder (R2-08). | `pf1` via panic | implementation-local |
| N17 | DETECTED | `set_focus_rejects_foreign_port` | `Err` |
| N18 | DETECTED | `r07_set_focus_on_unmapped_is_refused` | `Err` |
| N21 | DETECTED | `r15_rule1_uses_the_causing_event_output` | client configure literals |

## 4. New findings

### Level A — 0

### Level B — 7
- **R2-01 — Destroyed popup leaves an input hole.** After a client destroys a popup, its surface stays in Core's tree until the parent commits. Clicks inside its former area hit a dead surface and vanish: `leave W1`, then nothing; 0/20 clicks delivered across a Super+Enter focus change (t99e/g/i/j). It reproduces on the ratified INC-3 baseline `1ee520fa` (0/6), so it is latent in INC-1/INC-3 snapshot lifecycle. INC-4 makes it routine: every dismissed or closed menu leaves a hole, and there is no `popup_destroyed` handling. Blocking under the brief's rule for defects on the INC-4 production path; owner to assign the corrective increment.
- **R2-02 — `InputAuthToken` is an unbound, reusable, `Send` capability (T02b, T02c).** This falls short of the project's own proof pattern (bound to its Core, `!Send`, consumed).
- **R2-03 — Portless keyboard capability through public `WaylandState::keyboard_handle()`.** `input_forward` (key injection to clients bypassing Core ownership), `set_focus` (behind ActiveElementFocus) and `set_grab` compile from production backend code and pass every source rule. The equivalent pointer calls are caught by `smithay-pointer-only-in-adapter-pointer`; there is no keyboard counterpart.
- **R2-04 — Duplicate modifier press leaves a stuck modifier** in the seat XKB state and on the client (INV-INPUT-6 violation consequence; T04, T17).
- **R2-05 — Stale pointer grabs.** A `PopupPointerGrab` remains after CameraPan dismissal, single or nested, until replaced. INV-INPUT-10 step 3 is skipped at nesting depth ≥3 (`previous_serial` covers one level), so the next click triggers a synthetic `motion`. A reconnect re-installs a grab for a client-destroyed popup. Client-visible effect demonstrated: the extra motion (t99k). The owner-events confinement against *another* client could not be demonstrated in the single-client harness and is not claimed.
- **R2-06 — INC4R-11 not implemented.** Idle Super+B is swallowed by a retained test fixture.
- **R2-07 — Verification gaps.**
  - M08 (unbounded serial records) survives.
  - N09d/N09f, including the nested keyboard-unset omission (the keyboard grab stays and W1 never gets `enter`), are detected only by the feature-gated NEG-2 comparison test.
  - NEG-1, NEG-2 and OBS-1 never run unmutated in any automated gate.
  - The shipped `r17` asserts no protocol-violation bound.
  - KV-2's model reads `core().interaction()` for Escape (partially circular).
  - F-4 checks "last enter", not "exactly one transition".

### Level C — 3
- **R2-08:** production ordering instrumentation (`popup_preemption_pending/_violated` flags, a side effect in `SeatHandler::seat_state`, and a production `assert!`) exists only to kill an observationally equivalent mutation. The assert is unreachable in canonical code, since `PopupGrab::ungrab` takes no state, but it is test machinery and a panic path in the compositor.
- **R2-09:** the INC4R-10 owner decision is recorded only in `evidence/.../corrective-r2/CORRECTIVE_FINDING_MATRIX.md`, not in INV-INPUT-10 / W-SM-16 canonical text.
- **R2-10:** the rule-1 fallback for keyboard-serial grabs and evicted serials (more than 64 later button events) is the current pointer output. "Else the seat's last input" is ambiguous for keyboard-caused popups.

### Observations
- **O-1:** the round-1 CHECKPOINT heading was relabelled.
- **O-2:** the reviewer scratchpad from the second review was lost in the unexplained shutdown. Byte-identity of the second dossier was verified by spot-check and line counts against the session record, not by `cmp`; the first dossier's two logs are now committed (`.gitignore` exception).
- **O-3:** the second review missed R2-01 and R2-05 (its R10 checked the pan end and keyboard only). Recorded for completeness.
- **O-4:** pre-existing INC1-F01 (INC-7) remains open.

**Open: Level A 0, Level B 7, Level C 3.**

## 5. Verification (all PASS)
- **Cargo:** `check` and `build`; `test` 185 (core 69; T1 116: adversarial 13, input 17, keyboard 32, outputs 10, parity 6, popup_focus 27, slice 4, transform 7); probe-feature `t1_popup_focus` 31 (= 189 total); `fmt`; `clippy -D warnings`.
- **verify-architecture:** 13 rules; 38 probes + 1 positive control; [5b] 4; [5c] 5; [6] 54 seeded + 3 judge + tree identity.
- **verify-test-efficacy:** 98/98 + 2 judge.
- **Other gates:** foundation checker, self-test, spike and environment verifier.
- **CI:** run 37433415984 on the exact HEAD: 185 tests, 98 detected.

## 6. Next action
1. Fix R2-04: XKB must not see a duplicate press. Gate `sync_seat_key` on Core's ownership, or make the XKB update an effect issued after Core decides.
2. Fix R2-05:
   - unset every retained popup pointer grab on any dismissal, matching the grab by its install serial rather than by the last two serials;
   - clear the CameraPan-dismissed grab at pan end without leaking motion;
   - never re-install a grab for an ended or destroyed chain (call `cleanup()` before re-install).
3. Fix R2-01: on popup destruction, re-extract the parent snapshot or drop the popup from Core's tree (owner: assign INC-1/3 corrective vs INC-4).
4. Fix R2-02 and R2-03:
   - bind the token to its Core and make it consumed and `!Send`;
   - replace public `keyboard_handle()` with read-only observers;
   - add a keyboard confinement source rule.
5. Fix R2-06: remove the fixture bindings from product policy.
6. Fix R2-07: add permanent default-suite tests for N09f-nested and M08, run NEG/OBS unmutated in a gate, and make r17 assert zero violations.
7. Adjudicate R2-08 (remove the production instrumentation), R2-09 and R2-10.

INC-5 is not authorized.
