# Epoch time authority — record (INC-6, experimental)

Status: **owner direction 2026-10-14: the session monotonic clock is the intended authority for
discrete scale-epoch deadlines.** Implemented on the INC-6 experimental line. No sealed text was
changed; the wording proposed for M7 below is to be adopted when INC-6 is ratified or when the
first discrete interaction is introduced (INC-10, INC-11 or INC-14), whichever comes first.

## The three clocks and their boundary

| Clock | Type in code | Owner | Used for | Never used for |
|---|---|---|---|---|
| **Session monotonic clock**: milliseconds since the session started | `infynitae_core::scale::SessionTime` | `infynitae_wayland::session::Session` (`Session::now`) | the time of a discrete epoch step; the comparison with a deadline (`Core::scale_tick`); the loop's wake-up (`Session::wake_at`) | client-visible timestamps |
| **Input event time** | `PhysicalEvent.time_us: u64` | the backend's input source | forwarded to clients as the event's time | deadlines; it is whatever the device or host stamps and is not comparable with the session clock |
| **Presentation clock** (`CLOCK_MONOTONIC`, as `wp_presentation` announces) | `std::time::Duration` from `presentation_clock_now` | the adapter | frame and presentation timestamps | deadlines |

`SessionTime` is a distinct type with no `Default`, no zero constant and no conversion from a
bare number other than the explicit `from_millis`, which only the session (and tests) call.
`Core::scale_tick`, `Core::start_scale_interaction` and `Core::next_settlement_deadline` take
and return it, and `Session::wake_at` accepts nothing else. An input timestamp or a presentation
time therefore does not type-check where a deadline time is expected.

## Rules, and where each is enforced

| Rule | Mechanism | Test |
|---|---|---|
| No time-domain mixing | the `SessionTime` type at Core's and the session's boundary | compile-time |
| No zero-time fallback | the batch-end evaluation has no clock at all (`ScaleState::evaluate`); `SessionTime` has no default | `an_evaluation_does_not_move_an_open_epochs_deadline` |
| No deadline reset from unrelated commits | only `ScaleState::discrete_step` writes a discrete deadline; an evaluation is not a step | same, with client commits, another element's lifecycle and a camera drag inside the epoch |
| Monotonic epoch progression | Core keeps the latest `SessionTime` it was given and reads an earlier one as that; a discrete deadline never moves earlier | `epoch_deadlines_do_not_go_backwards_with_the_clock` |
| No stale deadline wake-up | the session holds no timer: its sleep bound is rewritten from Core's current deadline every turn, and `None` clears it | `the_wake_up_follows_the_deadline_and_is_cancelled_with_the_epoch`; session unit test |
| No unnecessary idle wake-up | no deadline, no bound: `Session::timeout` is `None`; a continuous epoch has no deadline | `a_real_camera_drag_opens_an_epoch_without_a_deadline`, `waiting_for_a_deadline_is_one_sleep_not_a_poll` |

## Sealed wording (adopted 2026-10-14 as C12.F4, on `main`)

INV-SCALE-3, clock clause, and three rows of M7:

> **Clock:** the settlement timeout is measured on the session's monotonic clock, from the time
> the last step of the interaction entered Core. A deadline is written only by a step of the
> interaction, never by a client response or any other evaluation, and it never moves earlier.
> Input-event timestamps and presentation timestamps are separate clocks and take no part.

Each sentence against this branch: the clock is `SessionTime`, stamped by `Session::now()`;
only `ScaleState::discrete_step` writes a discrete deadline; an evaluation has no clock; an
earlier time is read as the latest one Core was given. The tests in the table above are the
evidence. The branch remains experimental: the wording is adopted, the implementation is not
ratified.

## What is still not built

No production event opens a discrete epoch. Discrete pan and zoom steps (INC-10), shell streams
(INC-11) and interactive move and resize (INC-14) will each have to stamp their step with
`Session::now()` at the point where the backend hands the event to Core. Nothing here adds such
a source; the only caller of `Core::start_scale_interaction` outside tests is none.
