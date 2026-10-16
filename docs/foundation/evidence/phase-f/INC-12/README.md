# INC-12 — xwayland-satellite supervisor: experimental implementation

Status: **EXPERIMENTAL, implemented and tested headless; NOT reviewed, NOT ratified, NOT
integrated.** Branch `experimental/inc-12-xwayland-satellite` from `main` `23752f1`. Produced
under the owner brief of 2026-10-16 (§6: isolated local source downloads and builds authorized;
no interference with KDE's Xwayland; no X11 path activated in the live session).

## Sealed scope read before any code

| | |
|---|---|
| Plan | INC-12 (PRODUCT, HEADLESS): M19, compositor-owned X sockets with `-listenfd`, restart on the same display; OBJ-32; INV-XWAYLAND-1; OB-33 (T1), OB-9 and OB-27 (T1 parts); exit tests TST-14 c6-sat on the production path and OB-33's restart test |
| Entry conditions in the plan | INC-4, INC-7 exits (both ratified) |
| Stop rule | any X11 path reaching Stage or cameras |

## What exists

- `infynitae_wayland::xwayland` (new):
  - `XDisplay::reserve(root, numbers)` takes the first free display number: it creates the lock
    file `<root>/.X<n>-lock` exclusively (pid in ten columns) and binds the abstract socket
    `@<root>/.X11-unix/X<n>` and the socket file. A number with any existing lock, socket or
    socket file is skipped, never taken over. Drop removes the socket file and a lock that still
    names this process, nothing else. Sessions use `/tmp` and numbers 1…63 (`:0` is left to the
    host's X server).
  - `XwaylandSupervisor` (M19): Disabled → Running at startup; Running → Crashed on any exit;
    Crashed → Running after the backoff on the **same** display (the listening sockets stay
    open in the compositor, so a client that connects meanwhile waits in the backlog);
    → Degraded after `max_exits` exits within `window` (X11 off; `display_name()` is `None`).
    The satellite gets `:<n> -listenfd <abstract> -listenfd <file>`, `WAYLAND_DISPLAY` of the
    session, no `DISPLAY` and no `NOTIFY_SOCKET`. The sockets are passed as duplicates made
    without close-on-exec only for the spawn (no `unsafe`). Backoff 1 s, 5 exits in 60 s:
    provisional values, not measured.
- `Session`: `SessionOptions::x_display`; a client the session starts gets that `DISPLAY`, or
  **none**. Before this, a client started by the compositor inherited the compositor's own
  `DISPLAY` (on this desktop `:0`, KDE's Xwayland), so an X11 client could appear outside the
  session: an INV-XWAYLAND-1 leak, now closed.
- Headless and winit run loops and the binary: `--xwayland-satellite <PATH>` (off by default)
  reserves the display, exports it to the session's clients, starts the supervisor after the
  socket listens, folds its restart deadline into the loop's timeout, and ends the satellite
  before the session.
- Nothing reaches Core: the satellite is an ordinary Wayland client; its windows are ordinary
  elements, placed by Core's placement policy.

**Not implemented:** M19's "first X11 connection" trigger (the satellite starts at startup);
`xdg_activation_v1` and primary selection (OB-27, T3); override-redirect menus (OB-9, needs a
GTK3/X11 run).

## Findings from the isolated experiments (2026-10-16)

Satellite v0.8.3 (`b83eab9`, the version of ER-6 and the Phase C c6 run) was cloned to
`~/.cache/infynitae-integration/third-party/xwayland-satellite` and built there with its own
target directory, `--locked`, without the `systemd` feature. Nothing was installed. Every run
used `unshare -Urnm` with a private tmpfs on `/tmp` (private `/tmp/.X11-unix`, private abstract
socket namespace through the network namespace). The host's `/tmp/.X11-unix` held only `X0`
before and after every run.

1. **INC-12 needs INC-6.** Satellite 0.8.3 panics at start with "Could not bind wp_viewporter:
   NotPresent" against `main`. `wp_viewporter` exists only on the INC-6 line. Against the INC-6
   candidate it starts and serves X clients. The plan lists only INC-4 and INC-7 as entry
   conditions; INC-12's exit test cannot pass before INC-6 is integrated. (Plan correction for
   the owner; the supervisor itself does not depend on INC-6.)
2. **The lock file is compatible with `-listenfd`.** With the supervisor holding
   `/tmp/.X<n>-lock` (its own pid), Xwayland started by satellite with `-listenfd` neither
   complains nor replaces it.
3. **Restart on the same display works with compositor-owned sockets (OB-33).** Satellite
   killed with SIGKILL: Xwayland goes with it, the X client's connection breaks ("X connection
   to :7 broken") and it exits 1; a satellite started again on the same two sockets serves a new
   client with the same `DISPLAY`, which runs and exits 0.
4. **In `-listenfd` mode satellite logs to syslog** (`/dev/log`, the host journal) and falls
   back to stderr only when syslog is unreachable. Tests hide the journal socket inside the
   mount namespace; a real session would log to the journal.

## Tests

| Test | Tier | What it shows | Seeded control |
|---|---|---|---|
| `xwayland::tests::a_display_owns_its_lock_and_sockets_and_removes_only_them` | T0 (adapter crate) | lock with our pid, socket file, cleanup of only our files | — |
| `…::a_taken_number_is_skipped_and_left_alone` | T0 | an existing lock or socket file is skipped and left untouched | "M19: a taken display number is taken over" |
| `…::the_satellite_is_given_the_display_the_sockets_and_the_session` | T0 | `:<n>`, both fds open as sockets by the numbers given, `WAYLAND_DISPLAY`, no `DISPLAY` | "M19: the satellite is not given the display's listening sockets" |
| `…::an_exited_satellite_restarts_on_the_same_display_until_it_exits_too_often` | T0 | Crashed → backoff → Running on the same display → Degraded | "an exited satellite is never started again", "repeated exits never end in Degraded" |
| `…::exits_outside_the_window_do_not_count` | T0 | the window bounds the count | — |
| `…::shutdown_ends_the_satellite_and_the_display_files_go` | T0 | SIGTERM/SIGKILL, reaped, files removed | — |
| `session::tests::a_client_command_sets_or_removes_display` | T0 | the session's `DISPLAY` or none, independent of the test's own environment | "INV-XWAYLAND-1: a session client inherits the compositor's own DISPLAY" |
| `session::tests::a_session_client_gets_the_sessions_x_display_or_none` | T0 | the same through a real spawned client | — |
| `t1_xwayland_satellite::a_satellite_restarts_on_the_same_display_and_its_clients_die_with_it` | **T1, ignored by default** (needs the satellite binary, `unshare`, `mount`, `xmessage`) | real satellite + Xwayland + `xmessage` against the production session, Core, adapter and headless backend, isolated: the X window maps as an element with an anchor; SIGKILL of the satellite destroys it through client death and the X client exits non-zero; the restarted satellite serves the same `DISPLAY` and a new client maps | — |

The T1 test fails on this branch at its first step (the satellite exits until Degraded: no
`wp_viewporter`); it is run on the rehearsal merge with the INC-6 × INC-8 integration line (see
CHECKPOINT / the rehearsal commit). How to run it:

```
INFYNITAE_XWAYLAND_SATELLITE=<path to xwayland-satellite> \
  cargo test -p infynitae-backend-headless --test t1_xwayland_satellite -- --ignored --nocapture
```

## Not run

Any nested (winit) or real session with the satellite; the live desktop was never given an X
display by this work. No GPU. OB-9 and OB-27 untouched. OB-33 stays OPEN until the owner accepts
the T1 run on the integrated line.
