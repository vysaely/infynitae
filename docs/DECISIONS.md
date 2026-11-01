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

- **Super+W is the apps, Super+L the windows, Super+Shift+L locks** (owner, 2026-11-01; for a few
  hours that day Super+W was the windows and Super+D the apps, and Super+D is free now). One bar did both, so a search for a window also filled the panel with apps. Now
  Super+W only searches and starts apps, and Super+L only lists and narrows open windows.
  `LAUNCHER` and `BIND_LAUNCHER` follow the apps; `BIND_WINDOWS` moves the list. Without the edge
  Super+L does nothing.
- **Super+Shift+Escape opens a system monitor** (owner, 2026-11-01; `SYSTEM_MONITOR`), also over a
  fullscreen game holding the shortcuts, as a stuck game is when it is needed.
- **The launcher puts what you started recently first** (owner, 2026-11-01). An exact name comes
  first, then recently started apps whose name matches, latest first, then other names. Apps count
  as started only from the launcher, as there is no other record of them
  (`~/.local/state/infynitae/recent-apps`).
- **Often used on the left hand, rare on the right** (2026-10-31). Super+Tab, Q, W, E, A, S, C,
  V and Space are the everyday keys; Super+Shift+L, Shift+O, Shift+P, Shift+I, Alt+P and
  Shift+BackSpace the rare ones. So **Super+S is the region screenshot** (Super+Shift+S the whole
  screen), **Super+Shift+O the tearing toggle** and **Super+Alt+P recording**; the old chords
  (Super+Shift+R, Super+Shift+T, Super+Alt+R) stay as aliases, and a binding of the user's
  (`BIND_*`, `EXEC_*`) still wins over both.
- **Super+Tab flips between the last two windows** (2026-10-31), most recently used order, so two
  apps can be switched fast. It never goes further back, even with Super held (2026-11-01, owner:
  the step-through of 2026-10-31 felt like cycling every window). **Super+arrow** goes to the
  nearest window that way on the canvas and brings it into view (2026-11-01). **Attention first:** a
  window that opened without the focus (FOCUS_ON_OPEN "user" or "priority" held it back) or
  whose activation was refused wants attention; Super+Tab goes to the most recent such window
  first, brings it into view as Super+1..9 does, and clears its mark; the next Super+Tab goes
  back. A window loses the mark whenever it is focused. The edge marks such a window's tile.
  **With focus following the mouse** (2026-11-01): hover moves the focus only when the pointer
  crosses into something else, so a nudge after Super+Tab keeps its window; and a window counts
  as used only after 0.3 s (or a click), so one the pointer just passed is not the "last window".
- **Super+Q / W / E** start the terminal, launcher and file manager through one dispatcher
  (`live/launch.sh`), so each is a config setting.
- **Super+Shift+C closes, Super+Ctrl+Shift+C force-closes** (owner, 2026-11-01; Super+C and Super+Shift+C from 2026-10-25, Super+Delete before). Force close
  ends the window's client process, pinned by a pidfd so a reused pid is never killed.
- **Super+Enter is a fit toggle** (2026-10-21): it fits the window under the pointer, and pressed
  at the fit it restores the view. Super+F was removed, it duplicated Super+Enter.
  `SUPER_ENTER_ACTION="maximize"` makes it fill the view at 100% instead.
- **Super+Shift+Enter is native fullscreen** (2026-10-21), and with `FULLSCREEN_PAN` you can still
  pan out of it and use Super+1…9 (2026-10-25).
- **Super+Tab cycles focus** and centres the camera on the window (2026-10-21,
  `FOCUS_CYCLE_NAVIGATE`); Super+Shift+Tab is the overview. Its order is the recently used one
  since 2026-10-31 (above).
- **Super+Space centres the focused window at 100%, Super+Shift+Space keeps the zoom**
  (2026-10-05, restated 2026-10-24). Super+Shift+Middle click no longer resets the zoom
  (2026-10-24).
- **Super+left / Super+right drag move and resize a window, "like Hyprland"** (2026-10-22), from
  the nearest corner; Super+Shift drags the whole cluster. Super+middle during a move pans the
  camera and carries the window (2026-10-22).
- **Clients hear first which formats the screen shows directly** (2026-11-01, Dota was composited
  every frame and stuttered at full FPS): a scanout tranche in the linux-dmabuf feedback, for every
  client, not per surface, as the simple first step. A game asking to tear gets game mode even when
  composited.
- **A client's frame is applied when the GPU has finished it** (2026-11-01, games lost about 1
  frame in 11 at full FPS): until then its window shows the previous frame. Every fullscreen window
  gets game mode, however it is shown. LATENCY_STATS also logs why frames are not shown directly.
- **An app can take the shortcuts while it has focus** (2026-10-25, `keyboard-shortcuts-inhibit`:
  a VM, a remote desktop, a game): its keys go to it, Super-chords included, until it lets go
  or loses focus. Super+Shift+L (lock), and since 2026-11-01 Super+Tab, Super+W, Super+L and Super+A (a fullscreen game
  took them, "what if I need it"), Ctrl+Alt+Backspace (quit) and Ctrl+Alt+F1… (terminals) always stay yours.
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
  automount (`AUTOMOUNT`, infynitae-automount on udisks2; udiskie before 2026-10-31), the user folders, Noto fonts with emoji, and the control menu
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
  replay buffer's helper is left out. Since 2026-10-31 (owner) it is known by the private socket
  the compositor opens for it (`$XDG_RUNTIME_DIR/infynitae/<socket>.recorder`, in a 0700
  directory no sandbox is given), never by an executable's name, which any app could copy.
- **A dead lock screen is started again** (owner, 2026-10-31): when the lock's holder dies
  while locked, the compositor runs the locker itself (`launch.sh lock-screen`), again every 5
  seconds until one holds the lock; until then the screens stay black and locked.
- **Input methods get nothing while locked** (2026-10-25): the keys go to the lock screen past
  an input method's keyboard grab, a virtual keyboard types nothing, and no text input is enabled,
  so no text (the password field included) reaches an input method or comes from one.
- **Layer surfaces** (bars, wallpaper, the launcher) follow decisions L1–L8 (2026-10-21) in
  [foundation/FOUNDATION_ARCHITECTURE.md](foundation/FOUNDATION_ARCHITECTURE.md).

## Security and trust

- **Privileged protocols go only to the apps allowed them** (owner, 2026-10-31;
  [SECURITY.md](SECURITY.md)). Screen capture, clipboard management, virtual input, the input
  method, the window list, display control, layer-shell and the session lock are each a power
  with a list (`ALLOW_*`). An app not allowed one is never shown those globals. Sandboxed apps
  connect through `security-context-v1` and get a power only when their app id is listed; a
  native app is known by its executable's device and inode, never its file name. The default
  lets every native app in (the word `native`) for all powers but the lock, so OBS, screenshot
  tools and `wtype` keep working; a native app runs with the user's rights anyway. Layer-shell
  is a power too: a sandboxed app drawing over every window could imitate a lock or password
  prompt. The lock is Infynitae's own (`infynitae-lock`) and the `SCREEN_LOCKER` command's.

- **Screen sharing asks once per app** (owner, 2026-10-31): the ScreenCast portal always goes
  through infynitae-portal, which asks before an app's first share (`SCREENSHARE_ASK="app"`, the
  new default; it was "false", sharing the first screen at once). "Always" is kept per sandbox
  app id; an app outside a sandbox has no id the portal can trust and is asked every time (the
  remote desktop's shared "(unsandboxed)" entry is gone for the same reason). Only
  xdg-desktop-portal may call the backend.

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
- **No Python at run time** (2026-10-26): the session's helpers that talked to the compositor
  and restored the session were Python; they are the binary's own subcommands now (`infynitae msg`,
  `infynitae restore`), so the desktop needs no Python. Development tools still use it.
- **The capture dot steps aside over a fullscreen window** (2026-10-26): anything drawn over a
  game keeps it from going to the screen directly (tearing, game mode), so while a screen shows
  a fullscreen window the dot is hidden there and a notification announces the capture once.
- **Four Smithay patches, by decision** (D-B12-PIN): Smithay 0.7.0 is vendored with four
  authorized changes: P1, pointer constraints; P2 (2026-10-26), async page flips for tearing in
  games, which Smithay 0.7 cannot do; P3 (2026-10-26), an EGL context that is lost instead of
  aborted when the GPU resets or runs out of memory; P4 (2026-10-31), that context created at a
  high GPU priority (`EGL_IMG_context_priority`) and the granted one read back, so the desktop
  stays smooth while a game loads the GPU (written 2026-10-30 without a decision, approved
  2026-10-31). Anything else is worked around in the adapter.
- **Apps must not die with the compositor** (2026-10-26): out of GPU memory (a model training,
  several games), Mesa aborted the compositor and every window closed. The console renderer's
  context is now lost instead (P3, U-009), and the backend makes a new renderer: the screens
  freeze until GPU memory frees up, the windows stay. Qt 6 apps also reconnect after a restart
  (`QT_WAYLAND_RECONNECT`).
- **A new window takes the focus unless you are busy** (2026-10-29, `FOCUS_ON_OPEN="priority"`,
  the new default): windows rank against the focus. One you opened always takes it; with Do Not
  Disturb on (Super+A, a flag file the compositor reads) no other does; a fullscreen window (a
  game, a video) keeps it against all but its own dialogs; otherwise the new window takes it. A
  held window opens without the focus, asks for attention and leaves the camera where it is.
