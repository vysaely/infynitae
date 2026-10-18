# INC-8 — ratification review (2026-10-16)

Status: **REVIEW. INC-8 was RATIFIED for its T1 scope on 2026-10-16 by owner decision (CHECKPOINT, `governance/inc-8-ratification-2026-10-16`); OB-16 stays OPEN.** Original status: review for the owner's ratification decision. Subject:
`experimental/inc-8-offscreen-frames` @ `b110398` (implementation `60c05a1` … `a45b0f0`, review
fix `bdeb219`, review test `b110398`), and its combination with INC-6 on
`integration/inc-6-inc-8-r2-2026-10-16` (`9aea65d`). Everything below is headless (T0/T1).

## Verdict

**Ready to ratify as INC-8's T1 scope, with OB-16 kept OPEN** (its T3 part needs real
applications). No open defect is known. One defect was found by review and fixed (`bdeb219`);
the fix was re-verified independently here.

## 1. The camera-pan popup-grab fix, re-verified

| Check | Result |
|---|---|
| Without a grab, a pan press (Super held, so the key's own legitimate kick is out of the way) completes nothing | `a_pan_press_without_a_popup_grab_kicks_nothing` PASS; seeded "a dismissal with no popup grab kicks the element Core named" DETECTED |
| With a grab, the grab owner's callback completes **before** `popup_done` | `a_popup_done_kicks_before_it_is_sent` PASS; seeded "no kick before popup_done" DETECTED |
| The adapter never builds a Core `Effect` (it passes the grab owner as data) | `verify-architecture` `effects-only-from-core` PASS |
| The owner is the root of the grab's current popup chain, and `None` when the grab has ended | code: `popup_grab_owner` filters `has_ended()`; Core's `frame_kick(effect, owner)` ignores the owner for every other effect |

## 2. Audit across the brief's areas

| Area | Evidence (T1 unless marked) | Verdict |
|---|---|---|
| Popup grabs | the two tests above; F-1 (Smithay's own outside-click `popup_done` bypasses the kick; no path found that hides its owner) | holds; F-1 recorded |
| Fullscreen transitions | `a_window_hidden_by_a_presentation_gets_the_fallback_and_re_entry` (withheld while hidden, fallback once per P, completes at re-entry); `a_pointer_leave_kicks_a_window_hidden_after_the_pointer_entered_it` | holds |
| Scale changes | integration line: `a_hidden_windows_lowered_hint_releases_no_withheld_callback` — a P7 hint to a hidden window releases no callback (a hint is not a kick); seeded control DETECTED | holds on the combined line |
| Multiple outputs | `another_outputs_vblanks_do_not_clock_an_unobserved_window` (new, `b110398`): A observes W, B (144 Hz) does not; A disconnected; B's vblanks complete nothing of W; the fallback completes it once. INV-FRAME-2 clock selection: `clock_is_the_highest_refresh_observer` | holds |
| Window destruction | `a_destroyed_window_disarms_the_fallback_timer`; seeded "a destroyed element keeps its liveness entry" DETECTED | holds |
| Repeated visibility transitions | `repeated_visibility_transitions_neither_starve_nor_duplicate` (12 transitions at dwell phases 7…422 ms; each request completed exactly once, none waited > P + 2 vblanks) | holds |
| Input delivery ordering | the client's own wire log: the withheld callback precedes the key (`a_key_kicks…`), the configure and keyboard `leave` (`a_configure_kicks…`), the pointer `leave` (`a_pointer_leave_kicks…`), `popup_done`; real time (TST-11 T1): first response to a key 86 µs median / 131 µs max over 50 runs, ~700 ms with the kick disabled | holds |
| Zero idle wake-ups | the fallback deadline is armed only for an unobserved element with a pending callback; evaluation and deadline share one predicate (no busy loop); session test `the_fallback_deadline_bounds_the_sleep_only_while_armed` | holds (T4 measurement not done) |
| Allocations | no new steady-state allocation (`frame_fallback` returns an empty `Vec`); OB-42's test covers frame tick | holds |

## 3. The combined INC-6 + INC-8 line

`integration/inc-6-inc-8-r2-2026-10-16` @ `9aea65d`: 469 tests, 359 seeded defects detected, all
gates PASS. The invariants INC-8 owns (INV-FRAME-1…4, INV-IDLE-1) and the INC-6 ones they meet
(INV-SCALE-5 "unobserved elements hold h", P7, C12.F13 holds) are exercised together by the
combined test above. Since then INC-6 r3 fixed a C12.F13 hold-release defect (`d7c6c7c`); it
does not touch INC-8's code, and a re-merge is a mechanical step of the integration plan.

## 4. What ratification would not cover

- **OB-16 T3:** a real application (a game or a video player off-screen, focused) on hardware.
- **T4:** idle wake-up and CPU measurements (OB-44 baselines).
- **F-1…F-4** residuals (README): outside-click `popup_done`; completion stamped at the next
  evaluation; `frame_kick` cost not measured; PROV-4 (P = 1 s) provisional.
- INC-8 is independent of INC-6 by the plan; ratifying it before INC-6 means rebasing INC-6 onto
  it (the rehearsal shows the resolutions).
