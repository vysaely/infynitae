# INC-8 — off-screen frame policy: experimental implementation

Status: **EXPERIMENTAL. Implemented 2026-10-16 under the owner's continuation brief of that
date (autonomous local development on branches; no merge to `main`, no push). Not reviewed,
not ratified; INC-8's exit is not claimed.** This file is evidence, not authority.

Branch `experimental/inc-8-offscreen-frames`, from canonical `main` `23752f1` (INC-7
ratified; INC-6 not on it). INC-8 depends only on INC-2 (FOUNDATION_IMPLEMENTATION_PLAN
dependency graph), so it is independent of the unratified INC-6 line.

## Sealed scope read before any code

| | |
|---|---|
| Objective | M16 complete: withhold, kick (incl. input and `popup_done`), fallback timer armed only with `frame_requested`, re-entry liveness; suspension reasons (none producible) |
| Invariants | INV-FRAME-1 … INV-FRAME-6, INV-IDLE-1 |
| Objects | OBJ-17 (FrameLiveness), OBJ-18 (SuspensionReasons), OBJ-05 `frame_requested` (PD-F1-06) |
| Obligations | OB-16 (T1 part) |
| Exit tests | TST-03 SC3, TST-06 x9; a real-time harness port with a callback-blocked client (TST-11 T1 part, HEADLESS) |
| Negative controls | pure starve (kick and fallback disabled) blocks the client |
| Stop / rollback | a timer armed with no pending callback (idle violation) |
| Provisional | PROV-4: P = 1 s candidate, finite and > 0 |
| Decisions | ADR-06; D-B9-FRAMES (kick list incl. input; fallback measured from the last completion, AUD-24) |

## What exists

**Core (`core::frames`, OBJ-17).** One entry per element that ever had a pending callback:
`requested` and the last completion the period is measured from.

- *Pending.* The adapter's snapshot carries `frame_requested` per surface: Smithay's committed
  `frame_callbacks` list is non-empty (Smithay appends at every commit and drains on completion,
  `vendor/smithay-0.7.0/src/wayland/compositor/handlers.rs` `merge_into`). Each accepted commit
  (root or child) sets the element's flag from its tree; Core clears it when it decides a
  completion. Smithay's throttling state is not read.
- *Observed.* `Core::frame_tick` (now `&mut self`) returns the observed elements clocked by the
  output, as before, and records their completion. A vblank of an element with nothing pending
  completes nothing and does not move the last completion.
- *Kick* (INV-FRAME-3 (a)). `Core::frame_kick(&Effect)` maps an effect to its recipients and
  returns those that are unobserved with a pending callback, recording the completion. Kicking
  effects: `ToplevelConfigure`, `ToplevelConfigureSize` (non-initial configures),
  `DismissPopupGrab` (`popup_done`), `KeyboardFocus` (old and new focus: `leave`/`enter`),
  `KeyboardForward`, `PointerMotion` (old and new pointer focus), `PointerRelative`,
  `PointerButton`, `PointerAxis`, `ConstraintActivate`/`Deactivate`. Not kicking:
  `InitialConfigure`, `wl_surface` enter/leave, output globals, `PointerFrame`. The pointer
  focus is tracked as a surface and resolved when an event reaches it, so a window hidden after
  the pointer entered it is still kicked before its `leave` or a button (a first version
  resolved it at entry and missed this; found in self-review, reproduced by a T1 test). A
  surface is resolved to its element by searching only the elements that are pending and
  unobserved, so an event to an observed or idle surface costs no search.
- *Fallback* (INV-FRAME-3 (b)). `Core::frame_fallback(now)` completes every unobserved element
  whose callback is pending and whose last completion is at least P ago, and returns them.
  `Core::frame_fallback_deadline()` is the earliest `last + P` over unobserved pending elements,
  `None` when there is none.
- *Time.* Core reads no clock; `now` is the presentation clock the backends already use for
  frames. A completion decided without a time (vblank, kick) is stamped at the next
  `frame_fallback` evaluation, which is never earlier than the completion, so the period is
  never shortened (AUD-24); at worst the fallback is one loop iteration late (OBJ-17's
  reconstruction rule). An element with no completion yet is measured from when it became
  pending.
- `set_frame_fallback_period` (rejects zero), `frame_fallback_period`, `frame_pending`.

**Adapter (`infynitae-wayland::frames`).** `apply` asks `Core::frame_kick` before every effect
and completes the named elements' callbacks first (root tree and every popup tree). The
completion is direct (`callback.done`), because an unobserved element has no output for
Smithay's per-output throttled path; presentation feedback is left pending (INV-FRAME-5).
`WaylandState::frame_fallback(now)` runs Core's evaluation, completes the due elements and
returns the next deadline.

**Backends.** The headless and winit loops call `frame_fallback` once per iteration, after the
iteration's effects, and hand the deadline to `Session::set_fallback_deadline`;
`Session::timeout` sleeps until the earliest of the frame timer, the fallback deadline and the
duration limit. With no unobserved pending callback nothing is armed (INV-IDLE-1).

**Not implemented.** M17 suspension (OBJ-18): no reason {SessionLocked, OutputsOff, Parked} is
producible in Foundation 01, so there is nothing to emit; INV-FRAME-6 holds vacuously (nothing
sets `suspended`). Pings: the compositor sends none (no ping path exists), so there is nothing
to kick before. Compositor-initiated close: no such path exists. `popup_done` sent by
Smithay's own popup grab on an outside click is not routed through a Core effect; its element
is the focused, normally observed one (residual F-1 below).

## Tests (production path)

Core (T0, `core::frames::tests`): no timer when nothing is pending or the element is observed;
one fallback per period from the last completion; a kick completes once and restarts the
period; observed elements are never kicked or fallen back; an empty vblank is not a
completion; destruction forgets the element; the period is finite and positive.

T1 (`crates/infynitae-backend-headless/tests/t1_frames.rs`, 13 tests, real client over a socketpair,
virtual time; the lockstep runs the fallback timer as a virtual event and evaluates it at the
end of every pump):

| Test | Checks |
|---|---|
| `an_unobserved_window_gets_one_callback_per_fallback_period` | withheld for P, one completion at exactly P, nothing without a request (INV-FRAME-1), a redrawing client gets one per P |
| `an_observed_client_is_clocked_by_vblank_and_arms_no_timer` | 120 vblanks: one completion per vblank, deadline `None` throughout (INV-IDLE-1) |
| `a_key_kicks_a_focused_unobserved_client_before_the_key_arrives` | OB-16 (CR-7): wire order `frame W`, `key`; completion in the same dispatch; once |
| `a_configure_kicks_before_it_is_sent` | focus loss: the completion precedes the `activated` configure and the keyboard `leave` |
| `a_kick_restarts_the_fallback_period` | AUD-24 |
| `re_entry_completes_at_the_first_vblank` | INV-FRAME-4 after the output reconnects: nothing at connection, then at the first vblank exactly |
| `re_entry_after_a_pan_completes_before_the_fallback` | INV-FRAME-4 after a camera pan |
| `zero_outputs_still_complete_on_the_fallback` | M16 with no output |
| `an_observed_client_is_not_kicked` | no double rate |
| `a_window_that_stopped_drawing_arms_no_timer_when_it_leaves` | vblank completions are recorded |
| `unobserved_feedback_stays_pending_until_shown_or_superseded` | INV-FRAME-5 (TST-06, INC-8 part) |
| `a_pointer_leave_kicks_a_window_hidden_after_the_pointer_entered_it` | another window's presentation hides W under the pointer: `frame W` immediately before `pointer leave W` |

`t1_inc7_adversarial::a_window_hidden_by_a_presentation_gets_the_fallback_and_re_entry`
replaces `until_inc_8_a_window_hidden_by_a_presentation_gets_no_frame_callback` (INC-7 audit
finding A-7, residual R-10): a window hidden by a fullscreen presentation gets one completion
exactly P after its last completion, none from vblanks, and the next at the first vblank after
the presentation ends. `session::tests::the_fallback_deadline_bounds_the_sleep_only_while_armed`
covers the loop timeout.

Seeded controls added to `tools/verify-test-efficacy` (13): kick disabled (input; configure),
observed element kicked, fallback disabled (unobserved; zero outputs: the "pure starve"
negative control), a kick that does not restart the period, timer armed for an observed
element, vblank completion not recorded, an empty vblank counted as a completion,
`frame_requested` not extracted, the adapter not completing withheld callbacks, the session
ignoring the fallback deadline. The existing session-timeout control was re-anchored on the
rewritten `Session::timeout`.

## Exit tests against the sealed list

| Exit test | State |
|---|---|
| TST-03 SC3 (off-screen withhold/kick/fallback, production parity) | covered at T1 by the suite above (lockstep, virtual time) |
| TST-06 x9 (presentation feedback, INC-8 part) | covered at T1 by `unobserved_feedback_stays_pending_until_shown_or_superseded`: pending through the fallback and a kick, `discarded` when superseded, the latest `presented` at the first vblank after re-entry with `sync_output` = the new clock output. No seeded control yet |
| TST-11 T1 part: a real-time harness port with a callback-blocked client | covered by `t1_frames_realtime::a_key_unblocks_a_fifo_client_off_screen_within_one_frame` (below). OB-16's T1 pass criterion is met in this harness; the obligation stays OPEN until review and its T3 part (real toolkits) |

## Real-time harness (TST-11 T1 part, OB-16)

`crates/infynitae-backend-headless/tests/t1_frames_realtime.rs`. The production stack runs on
the test thread in real time (60 Hz vblank on the presentation clock, fallback evaluated each
iteration). The client runs in its own thread with its own connection and presents like a Mesa
WSI FIFO swapchain: it commits a frame with its callback on a private event queue and blocks
on that queue alone, so its input waits unread. The window is focused, then panned 3000 px off
the output; the client blocks; 300 ms later (inside P) a key is injected.

Measured on the owner's machine (Ryzen 5 7600X3D, debug build, in-process, load average 0.6),
50 runs, time from the key's injection to the client handling it: min 78 µs, median 86 µs,
p95 110 µs, max 131 µs. Pass criterion (OB-16): within one frame (16.7 ms). The seeded control
with the kick disabled is DETECTED: the client stays blocked until the fallback (~700 ms after
the key). 30/30 repeated runs passed. This is not a vkcube/Mesa run (T3) and not a latency
baseline (C14, OB-44).

## Integration with INC-6 (rehearsal, never merge)

`rehearsal/inc-8-on-inc-6-candidate-2026-10-16` (worktree `~/.cache/infynitae-integration/inc8r`)
merges this branch onto the INC-6 candidate `b784b55`. That base contains amendments C12.F11 to
C12.F13, which are not adopted, so the branch must never be merged. Conflicts in 7 files,
resolved semantically:

- INC-6's allocation-free `frame_tick_into(out)` is kept; it takes `&mut self` and records
  completions over the appended slice, adding no allocation (INC-6's OB-42 ratchet test passes).
- INC-6 funnels every commit through `toplevel_committed_acked_into`; the OBJ-17 update is
  applied there, only for an accepted commit.
- INC-6's `Effect::PreferredScale` does not kick. It is not in INV-FRAME-3's kick list, and an
  unobserved element's hint is held, not sent (ARCHITECTURE scenario 11). `frame_kick`'s
  exhaustive match forced this decision at compile time.
- The session sleeps until the earliest of: the frame, the fallback (presentation clock), the
  duration limit, and INC-6's settlement wake (session clock). **Integration note for the
  owner:** two clocks now bound one loop. They are consistent today, because each deadline is
  converted to an `Instant` against its own clock. A single Core clock would be simpler; C12.F4
  names the session clock for epochs and nothing names one for frames.

Gates at the rehearsal merge `57c466f`: 457 tests, clippy, fmt, verify-architecture, docs and
self-test, knowledge (content-only), verify-test-efficacy 351 detected: all PASS.

## Residuals

- **F-1.** `popup_done` sent by Smithay's `PopupGrab` on a click outside the popup chain does not
  pass through `Core::frame_kick`. Its recipient is the grab owner, which holds focus. No path
  found that hides a grab owner while its grab stays open: a camera pan dismisses the grab first
  (INV-INPUT-2, `DismissPopupGrab`, which kicks), and a presentation needs focus elsewhere,
  whose focus change ends the grab. Not covered by a test.
- **F-2.** A kick's completion is stamped at the next evaluation rather than at the kick (Core
  reads no clock). Bounded by one loop iteration; never earlier than allowed.
- **F-3.** `frame_kick` runs for every applied effect. For an observed or idle recipient it is a
  binary search; a surface-keyed effect searches only pending unobserved elements. Not
  measured (C14 baselines, OB-44, are open).
- **F-4.** PROV-4 stays provisional (P = 1 s); OB-5 (games and network clients off-screen, T5)
  is untouched.

## Gate results

At `60c05a1` (the implementation commit; worktree `~/.cache/infynitae-integration/inc8`, own
target dir), 2026-10-16: `cargo fmt --check` PASS; `cargo clippy --workspace --all-targets
--locked -D warnings` PASS; `cargo test --workspace --locked` 366 passed, 0 failed (main
`23752f1`: 347); `tools/verify-architecture` PASS; `tools/check-foundation-docs` and
`--self-test` PASS; `tools/verify-knowledge --content-only` PASS; `tools/verify-test-efficacy`
PASS (246 detected, 0 missed, 0 broken). The commit that adds this record re-runs them; see
CHECKPOINT.

Not run: any nested (winit) or real-client session; no GPU, no hardware, no real-time harness.
