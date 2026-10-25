# Using Infynitae

Everything here is about running the desktop. Building and changing it is in
[DEVELOPMENT.md](DEVELOPMENT.md).

## Install

Arch Linux (CachyOS works too):

```bash
git clone https://github.com/vysaely/infynitae.git ~/infynitae
cd ~/infynitae
./install.sh
```

The installer:

- installs the packages it needs (build tools, PipeWire, portals, polkit, fuzzel, fonts,
  grim/slurp, wl-clipboard, cliphist, Xwayland), a terminal if there is none, and the Vulkan
  drivers for your GPU (`infynitae-drivers`; it asks before a full system upgrade);
- builds the compositor and the helpers (lock screen, wallpaper, portal, polkit agent,
  recorder) and installs them to `~/.local/bin`;
- puts the session scripts in `~/infynitae/live`, wherever the clone is;
- writes `~/.config/infynitae/config` from the template. An existing config is never
  overwritten; the new template goes next to it as `config.template`;
- adds an "Infynitae" session for display managers.

Update any time, from anywhere: `infynitae-update`. It pulls, rebuilds, and swaps the binaries
in place (running programs keep the old ones until they restart). If your own edits to the repo
clash with the update, it stops before installing anything and leaves them in `git stash`.

## Starting a session

- **From a display manager** (SDDM, GDM, LightDM): pick "Infynitae".
- **From a text console**: log in and run `infynitae-session`.
- **Straight into it on boot**: `live/setup-autologin.sh` logs you in on tty1 and starts the
  session at the lock screen (`START_LOCKED="true"`). If the lock screen cannot start, the
  session ends instead of leaving you logged in unlocked.
- **In a window, for trying things out**: `live/run.sh` (a nested session inside your current
  desktop).

`infynitae-session` restarts the compositor when it quits normally and gives up after three
quick crashes. `Ctrl+Alt+Backspace` restarts it (your windows close), `infynitae-session stop`
ends the session, and `Super+Shift+Backspace` reloads the config and restarts the helpers
without closing anything.

## Configuration

One file: `~/.config/infynitae/config`, a commented shell file. Every option shows its default,
and every section says when its changes apply:

| Mark | Applies |
| :--- | :--- |
| `[save]` | the moment you save the file |
| `[reload]` | on `Super+Shift+Backspace` or `infynitae-reload` |
| `[use]` | the next time the thing is used |
| `[restart]` | on the next compositor start |

`infynitae-settings set KEY value` changes one option from a script (it quotes the value
safely); `infynitae-settings get KEY` reads one. Keys: `BIND_<ACTION>="SUPER+X"` moves a
built-in action to another chord, `BIND_<ACTION>="none"` frees it, `EXEC_<NAME>="CHORD:command"`
binds any command. A line the compositor cannot use is skipped and named in the session log,
and so is a chord bound twice.

## Your own apps

Every app the desktop starts is a setting. `"auto"` takes the first one installed; anything else
is run as given.

| Setting | Used for | `auto` picks |
| :--- | :--- | :--- |
| `TERMINAL` | `Super+Q` | kitty, alacritty, foot, wezterm, ghostty, konsole … |
| `LAUNCHER` | `Super+W` | fuzzel, rofi, tofi, wofi |
| `FILE_MANAGER` | `Super+E` | yazi, lf (in the terminal), thunar, nemo, dolphin |
| `BROWSER` | links | firefox |
| `SCREENSHOT_CMD` | `Print`, `Super+Shift+S` | the built-in grim/slurp handler |
| `CLIPBOARD_PICKER` | `Super+V` | fuzzel |

Your own startup programs go in `~/.config/infynitae/autostart.sh`. To take over every shortcut
action yourself, write `~/.config/infynitae/launch.sh`; it gets the action name as `$1`.

## The helpers

Small programs that make it a desktop. Each one is optional.

| Helper | What it does | Switch |
| :--- | :--- | :--- |
| `infynitae-lock` | lock screen (`Super+L`, idle auto-lock) | `SCREEN_LOCKER`, `IDLE_AUTO_LOCK` |
| `infynitae-wallpaper` | the wallpaper (videos through `mpvpaper`) | `WALLPAPER` |
| `infynitae-portal` | global shortcuts for apps (Discord, OBS push-to-talk) | — |
| `infynitae-polkit` | password prompts for admin actions | — |
| `infynitae-restore` | reopens your apps where they were | `SESSION_RESTORE` |
| cliphist / wl-clip-persist | clipboard history, clipboard that survives its app | `CLIPBOARD_HISTORY`, `CLIPBOARD_PERSIST` |
| `infynitae-recorder` | recording and a replay buffer you can clip from | `ENABLE_REPLAY_BUFFER` |
| quickshell island | an optional bar/launcher UI | `ENABLE_QUICKSHELL` |

`infynitae-reload` (or `Super+Shift+Backspace`) restarts them in place.

## Themes, fonts and cursors

Set `CURSOR_THEME`, `CURSOR_SIZE`, `ICON_THEME`, `FONT_NAME`, `FONT_SIZE` and `COLOR_SCHEME` in
the config. Apps do not read Infynitae's config: each toolkit reads its own files. So on reload
`infynitae-settings` writes your choices into each of them, once, with nothing left running:

| Who reads it | File |
| :--- | :--- |
| GTK 3 and GTK 4 apps | `~/.config/gtk-3.0/settings.ini`, `~/.config/gtk-4.0/settings.ini` |
| KDE and Qt apps | `~/.config/kdeglobals` (only created if missing; your edits stay) |
| the cursor, everywhere | `~/.icons/default/index.theme`, and `XCURSOR_THEME`/`XCURSOR_SIZE` |
| GNOME-style apps | `gsettings`, if it is installed |

A cursor pack is a cursor theme: put it in `~/.icons` (or install it with pacman), list what is
installed with `infynitae-settings list cursors`, and set `CURSOR_THEME`.

## App compatibility

`live/env.sh` sets what each toolkit needs to run natively on Wayland, with X11 as the fallback:
Qt (`QT_QPA_PLATFORM=wayland;xcb`), GTK (`GDK_BACKEND=wayland,x11`), Electron and Chromium
(`ELECTRON_OZONE_PLATFORM_HINT=auto`), Firefox (`MOZ_ENABLE_WAYLAND=1`), SDL (`wayland,x11`),
Java (`_JAVA_AWT_WM_NONREPARENTING=1`, against blank grey windows), and the NVIDIA driver when
an NVIDIA GPU is found. X11 apps (Steam, Wine games) run through `xwayland-satellite`.

## Screen sharing, screenshots, recording

Screen sharing goes through `xdg-desktop-portal-wlr`. With `SCREENSHARE_ASK="false"` an app's
Share button shares your (first) screen without a picker; `"true"` asks every time.
`SCREENCOPY_CURSOR` decides whether the cursor is in captures, and `Super+Shift+P` hides the
focused window from all captures. `Print` and `Super+Shift+S` take a screenshot,
`Super+Shift+R` a region (into `SCREENSHOT_DIR`). With `ENABLE_REPLAY_BUFFER="true"` the last
minutes are kept in memory: `Super+Alt+C` saves a clip, `Super+Alt+R` starts or stops a
recording.

## When something goes wrong

| Log | What is in it |
| :--- | :--- |
| `~/.local/state/infynitae/session.log` | the compositor: screens found, config lines it skipped, errors |
| `~/.local/state/infynitae/lock.log` | the lock screen at session start |
| `~/.local/state/infynitae/restore.log` | what session restore reopened or skipped |

- **The session will not start:** after three quick crashes `infynitae-session` stops. The
  reason is at the end of `session.log`.
- **A setting has no effect:** check its mark in the config. `[restart]` settings need
  `Ctrl+Alt+Backspace`. Look in `session.log` for a skipped line.
- **Screens in the wrong order:** the names are the `Screen …` lines in `session.log`; use them
  in `OUTPUT_ORDER` (both `DP-1` and the older `DisplayPort-1` style work).
- **An app looks blurry or scaled wrong:** `SCALING_FILTER` and `OUTPUT_SCALE` (and the
  per-screen `OUTPUT_<NAME>_SCALE`).
