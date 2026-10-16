# Orderly shutdown on SIGTERM / SIGINT — design and evidence (2026-10-16)

Status: **PROPOSAL for the owner** (branch `feature/orderly-shutdown-2026-10-16`, from `main`
23752f1). It adds one dependency to the adapter (`signal-hook =0.3.18`, default features off;
`signal-hook-registry 1.4.5` comes with it, both depend only on `libc`) and the matching
allowance in `tools/verify-architecture`. Both need owner approval (INV-ARCH-3 dependency rule).
Supersedes the calloop-based proposal of 60012ce (cherry-picked, then replaced — see §2).

## 1. Behaviour

- From the first statement of `Session::new` — before the socket exists — SIGTERM and SIGINT
  only end the event loop; the caller then shuts down exactly as for a run limit.
- A signal before that point has its default action: the process ends before it created any
  socket or lock file, so nothing is left behind either way (measured, §3).
- Teardown order, unchanged from the run-limit path: the loop stops (no further client
  dispatch), the session's children get SIGTERM, a bounded wait (`CHILD_GRACE` = 500 ms), then
  SIGKILL, each reaped; the backend and output are dropped; the socket and its lock file are
  removed when the loop that owns their source is dropped. Only processes this compositor
  started are signalled.
- A second (or third) signal during teardown changes nothing: the handlers only write to the
  self-pipe, and after the session is dropped the signals are ignored (signal-hook does not
  restore the default action on unregister). One orderly shutdown, exit status 0.
- No wait is unbounded: the only wait is the child grace period.

## 2. Why not calloop's signal source

The pinned calloop 0.14.5 has a `signals` feature (signalfd). It blocks the signals in the
thread's mask, and **children inherit a blocked mask** (std does not reset it; the adapter
denies `unsafe`, so no `pre_exec` reset is possible). Measured on this branch with that
source: the session's command had `SigBlk: 0000000000004002` (SIGINT and SIGTERM blocked), so
the SIGTERM sent at shutdown never reached it and every shutdown waited the full grace period
and escalated to SIGKILL (~510 ms instead of ~13 ms). With signalfd a second signal arriving
after the source was dropped was delivered with its default action and killed the compositor
mid-teardown (`signal: 2 (SIGINT)` exit). The original 60012ce avoided the first defect only
for the first child by registering after it was spawned; any later child (e.g. a satellite
restart) would have inherited the mask.

signal-hook's `low_level::pipe::register` is a safe API: an async-signal-safe handler writes a
byte to a socket pair the loop watches as a `Generic` source. Nothing is blocked.

## 3. Tests (`crates/infynitae/tests/shutdown.rs`, real binary, headless, private XDG_RUNTIME_DIR)

| Test | Result (measured, this machine, debug build) |
|---|---|
| `sigterm_shuts_down_cleanly` | exit 0 after 12.8 ms; command gone, socket + lock removed |
| `sigint_shuts_down_cleanly` | exit 0 after 12.8 ms; same |
| `repeated_signals_shut_down_once` (TERM, INT, TERM) | exit 0 after 11.6 ms; same |
| `session_children_do_not_inherit_blocked_signals` | command's `SigBlk` all zero |
| `a_child_ignoring_sigterm_is_killed_after_the_grace_period` | exit 0 after 507 ms; command killed and reaped |
| `a_signal_during_start_up_leaves_nothing_behind` (0–60 ms) | 0–2 ms: killed by the default action before any socket existed; ≥ 3 ms: exit 0 after ~13 ms; nothing left behind in any case |

The clean-shutdown tests assert `< 400 ms`, so a regression to an inherited blocked mask (full
grace period) fails them. The earlier proposal's **1.1 s estimate is not used**: measured
teardown is ~13 ms with a well-behaved command and ~0.51 s with one that ignores SIGTERM.

## 4. Not covered here

- The X11 satellite (INC-12 line, not on `main`): its stop/reap order is exercised in the
  combined runtime line, not on this branch.
- A real display backend (DRM/KMS) does not exist yet; release of a real output is untested.
- The nested (winit) backend creates the session before its window, so a signal during window
  creation ends the loop rather than the process; exercised in nested runtime validation only.

## 5. Session process ownership (2026-10-16, `feature/session-process-ownership-2026-10-16`)

Status: **PROPOSAL for the owner.** No new dependency (rustix through Smithay's re-export, as
`session.rs` already uses it); no `unsafe`.

**Boundary:** the compositor's own process subtree. At start-up (after the signal handlers,
before any child) the compositor becomes a **child subreaper** (`PR_SET_CHILD_SUBREAPER`): a
descendant whose parent exits — a double-forked daemon, a `setsid` process — is re-parented to
the compositor, not to init or the desktop's service manager. A process is a member only while
the kernel lists it as a child (`/proc/<pid>/task/<tid>/children`) of the compositor or of
another member; each is pinned by a pidfd opened while that link held, and signalled through
the pidfd, so a recycled pid is never signalled. No process group, session id, name or uid
selects a target. Outside by construction: processes another service starts for a client
(D-Bus activation, the user's service manager); sessions run with D-Bus disabled.

**Runtime:** SIGCHLD (signal-hook pipe, no mask) wakes the loop; adopted orphans that exited are
reaped (`reap_adopted`), never a child an owner holds (`register_spawned` /
`forget_spawned`). **Teardown** (`end_session_tree`, from `Session::shutdown`): SIGTERM to every
member, re-scanning for members that appear during the grace period; after `CHILD_GRACE`
(500 ms) SIGKILL to the members left; then a bounded wait (≤ 500 ms) and reaping. A process stuck
in uninterruptible sleep is reported as a survivor rather than waited for indefinitely.

| Test (`shutdown.rs`, real binary) | Measured (debug build, this machine) |
|---|---|
| `descendants_and_daemons_end_with_the_session` (grandchild + double-forked `setsid` daemon; an outsider started by the test survives) | exit 0 after 8.8 ms; all members gone; outsider alive |
| `a_descendant_ignoring_sigterm_is_killed_after_the_grace_period` (+ its child, which inherits the ignored SIGTERM, under a living parent) | exit 0 after 511 ms; all gone |
| `repeated_signals_end_the_tree_once` (TERM, then INT + TERM 100 ms later) | exit 0 after 510 ms; all gone |
| `a_signal_during_start_up_leaves_no_session_process` (0–80 ms) | 0–2 ms: default action, nothing started; ≥ 5 ms: exit 0 in 7–9 ms; every member seen is gone |
| `an_adopted_orphan_is_reaped_while_the_session_runs` | the orphan's zombie disappears while the session runs |
| earlier tests (§3) on this branch | SIGTERM / SIGINT 7.1–7.5 ms; three signals 8.9 ms; stubborn child 517 ms |

Seeded controls (all DETECTED): no subreaper; only direct children ended; adopted orphans not
reaped. The satellite (INC-12 line) is ended first by its supervisor (`terminate_children`) and
registered as an owned child there; its Xwayland is a member of the subtree and is swept too
(exercised on the runtime line).
