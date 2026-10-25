# Epoch time authority (registry layer C12.F4) — record

Status: **adopted 2026-10-14 by owner approval; local main, not pushed.** Evidence and
provenance; the authority is FOUNDATION_ARCHITECTURE §C12.F4.

## What is adopted

The session's monotonic clock is the authority for discrete scale-epoch deadlines. INV-SCALE-3
gains one clause and state machine M7 names the clock in three rows.

> **Clock:** the settlement timeout is measured on the session's monotonic clock, from the time
> the last step of the interaction entered Core. A deadline is written only by a step of the
> interaction, never by a client response or any other evaluation, and it never moves earlier.
> Input-event timestamps and presentation timestamps are separate clocks and take no part.

## The three clocks

| Clock | Owner | Used for | Never used for |
|---|---|---|---|
| Session monotonic clock (milliseconds since the session started) | the session loop | the time of a discrete step; the comparison with a deadline; the loop's wake-up | client-visible timestamps |
| Input event time | the backend's input source | forwarded to clients as the event's time | deadlines |
| Presentation clock (`CLOCK_MONOTONIC`, as `wp_presentation` announces) | the adapter | frame and presentation timestamps | deadlines |

## Where it is implemented and tested

The scale policy is INC-6 and is not on main; this layer is sealed text only. On the INC-6
experimental line the clock is a distinct type (`SessionTime`) accepted by Core's epoch entry
points and by the session's wake-up, and by nothing else. Tests there:

| Property | Test |
|---|---|
| unrelated commits, another element's lifecycle and a camera drag do not move a deadline, also after the clock has advanced | `an_evaluation_does_not_move_an_open_epochs_deadline` |
| monotonic progression: a regressed time is read as the latest seen; a deadline never moves earlier | `epoch_deadlines_do_not_go_backwards_with_the_clock` |
| deadline update by a further step; cancellation by an output settlement event | `the_wake_up_follows_the_deadline_and_is_cancelled_with_the_epoch` |
| the deadline alone wakes the idle loop, in one sleep | `a_settlement_deadline_wakes_the_idle_loop_and_settles_the_epoch`, `waiting_for_a_deadline_is_one_sleep_not_a_poll` |
| no deadline, no wake-up; a camera drag has none | `a_real_camera_drag_opens_an_epoch_without_a_deadline` |

## Not introduced

No production event opens a discrete epoch. Discrete pan and zoom steps (INC-10), shell streams
(INC-11) and interactive move and resize (INC-14) will each stamp their steps on this clock.
