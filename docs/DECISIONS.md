# Decisions

The choices behind how Infynitae behaves, newest understanding first within each topic. The
architecture decisions (`ADR-*`: why Core is a pure state machine, why fixed-point canvas
coordinates, why cameras move only on trusted input) are in
[foundation/FOUNDATION_DECISIONS.md](foundation/FOUNDATION_DECISIONS.md). Code comments cite
these by date, e.g. "(owner, 2026-10-24)".

## Why it exists

- **Its own compositor, after driftwm's idea** (2026-10-04). driftwm showed the idea: one endless
  canvas you drift over instead of workspaces. Infynitae is a separate compositor so that the
  spatial model (per-screen cameras, clusters, fit, fullscreen panning) and every default are its
  own. An earlier version was a Hyprland plugin; a plugin could not give per-screen cameras,
  constant-time panning or exact hit-testing (see the foundation evidence).
- **Ready out of the box, every piece your choice.** A fresh install is a whole desktop; each
  app is a setting, each key can be rebound, the optional helpers are switches, and the
  configuration is one commented file. No settings daemons.

## Keys

- **Super+Q / W / E** start the terminal, launcher and file manager through one dispatcher
  (`live/launch.sh`), so each is a config setting.
- **Super+C closes, Super+Shift+C force-closes** (2026-10-25; Super+Delete before). Force close
  ends the window's client process, pinned by a pidfd so a reused pid is never killed.
- **Super+Enter is a fit toggle** (2026-10-21): it fits the window under the pointer, and pressed
  at the fit it restores the view. Super+F was removed, it duplicated Super+Enter.
  `SUPER_ENTER_ACTION="maximize"` makes it fill the view at 100% instead.
- **Super+Shift+Enter is native fullscreen** (2026-10-21), and with `FULLSCREEN_PAN` you can still
  pan out of it and use Super+1…9 (2026-10-25).
- **Super+Tab cycles focus** and centres the camera on the window (2026-10-21,
  `FOCUS_CYCLE_NAVIGATE`); Super+Shift+Tab is the overview.
- **Super+Space centres the focused window at 100%, Super+Shift+Space keeps the zoom**
  (2026-10-05, restated 2026-10-24). Super+Shift+Middle click no longer resets the zoom
  (2026-10-24).
- **Super+left / Super+right drag move and resize a window, "like Hyprland"** (2026-10-22), from
  the nearest corner; Super+Shift drags the whole cluster. Super+middle during a move pans the
  camera and carries the window (2026-10-22).
- **An app can take the shortcuts while it has focus** (2026-10-25, `keyboard-shortcuts-inhibit`:
  a VM, a remote desktop, a game): its keys go to it, Super-chords included, until it lets go
  or loses focus. Super+L (lock), Ctrl+Alt+Backspace (quit) and Ctrl+Alt+F1… (terminals) always
  stay yours.
- **Holding Super frees a cursor a game captured** (2026-10-25, "feel free in my desktop";
  `SUPER_FREES_CURSOR`).

## Windows and focus

- **Focus follows the pointer, and empty canvas unfocuses** (2026-10-25, `FOCUS_FOLLOWS_MOUSE=
  "strict"`, the owner's choice: "strict is really nice"): leaving a game for the canvas lets go
  of its keyboard and cursor at once. `sloppy` keeps the focus over empty canvas, `click` only
  changes it on a click.
- **Only windows you opened take focus and move the camera** (2026-10-24, `FOCUS_ON_OPEN` /
  `TELEPORT_ON_OPEN="user"`): windows from your shortcuts, your launcher or a command in the
  focused terminal; a popup an app opens by itself appears quietly.
- **A task bar's click focuses a window like your own click** (2026-10-25,
  `wlr-foreign-toplevel-management`): only from the bar you are using (it has your pointer or
  keyboard); a program in the background can only ask for attention, like any app.
- **A dialog opens over its parent** whatever the placement policy (2026-10-24).
- **An app's next window takes the place of the one that just closed** (2026-10-25: Steam's
  login window becoming the main window), if that place is still in view.
- **Cluster placement by default**: a new window goes to the next free spot beside the others.

## Screen and session

- **Everyday basics work out of the box, each one a switch** (2026-10-25): a notification daemon
  (`NOTIFICATIONS`, mako by default), a keyring (gnome-keyring, started by D-Bus on first use),
  automount (`AUTOMOUNT`, udiskie), the user folders, Noto fonts with emoji, and the control menu
  (`CONTROL_MENU`, Super+A) whose parts appear only when their tool is installed. NetworkManager is
  not installed for you: a machine on iwd or systemd-networkd keeps its networking.

- **The compositor draws the pointer on the console** (T8, 2026-10-22): nothing else does there.
  It uses your cursor theme (`CURSOR_THEME`, `CURSOR_SIZE`, sharp at the screen's scale), and a
  built-in arrow only if the theme cannot be found (2026-10-25; `xcursor` reads the theme, a crate
  that was already in the build).
- **The session owns its processes** (2026-10-16): everything the compositor starts, and
  everything those start, ends with the session, and nothing outside it is touched. Shutdown is
  orderly: signals reach the event loop through a self-pipe (2026-10-19).
- **The lock screen fails closed** (2026-10-25): with `START_LOCKED`, the session ends if the lock
  cannot be engaged, and only the client holding the lock can unlock or draw the lock surface.
- **One window can be shared** (2026-10-25): drawn by itself at zoom 1, the size of its window
  geometry (its shadow left out), wherever it is on the canvas and whether or not it is in view.
  The first screen's scale. Never a private window, never anything while locked.
- **A capture is never silent** (2026-10-25): while any client captures (the screen, a window, a
  screenshot), every screen shows a red dot, drawn by the compositor and never into a capture, so
  a remote desktop or a share cannot run unseen even when the app shows nothing itself (RustDesk's
  window could not open in this session and its connections ran without any sign). Only the
  replay buffer's helper is left out, known by its executable's name: a same-user program could
  take that name, but one that can do that can also turn the dot off in the config; the dot
  guards against apps behaving as designed, not against malware.
- **Input methods get nothing while locked** (2026-10-25): the keys go to the lock screen past
  an input method's keyboard grab, a virtual keyboard types nothing, and no text input is enabled,
  so no text (the password field included) reaches an input method or comes from one.
- **Layer surfaces** (bars, wallpaper, the launcher) follow decisions L1–L8 (2026-10-21) in
  [foundation/FOUNDATION_ARCHITECTURE.md](foundation/FOUNDATION_ARCHITECTURE.md).

## Shell IPC

- **JSON lines over a unix socket** (2026-10-20, D1–D9): one JSON object per line, a minimal
  strict parser in-tree, the socket's admission check with rustix, a fixed command set (INC-11).
  Window titles and app ids are reported to the shell.

## How the project is run

- **Solo-project style** (2026-10-25). The phase gates, ratifications and sealed documents of the
  first weeks are in [archive/](archive). Now: the design spec is edited like any other doc and
  kept consistent by `tools/check-foundation-docs`; changes land on `main` once the tests and the
  checks pass; this file records the choices.
- **AI agents do a lot of the work, and nothing lands unreviewed.** Every agent change is read in
  full and must pass the whole test suite and the checks before it is merged.
- **The capture dot steps aside over a fullscreen window** (2026-10-26): anything drawn over a
  game keeps it from going to the screen directly (tearing, game mode), so while a screen shows
  a fullscreen window the dot is hidden there and a notification announces the capture once.
- **Three Smithay patches, by decision** (D-B12-PIN): Smithay 0.7.0 is vendored with three
  authorized changes: P1, pointer constraints; P2 (2026-10-26), async page flips for tearing in
  games, which Smithay 0.7 cannot do; P3 (2026-10-26), an EGL context that is lost instead of
  aborted when the GPU resets or runs out of memory. Anything else is worked around in the
  adapter.
- **Apps must not die with the compositor** (2026-10-26): out of GPU memory (a model training,
  several games), Mesa aborted the compositor and every window closed. The console renderer's
  context is now lost instead (P3, U-009), and the backend makes a new renderer: the screens
  freeze until GPU memory frees up, the windows stay. Qt 6 apps also reconnect after a restart
  (`QT_WAYLAND_RECONNECT`).
