# Infynitae

An infinite-canvas Wayland compositor for Arch Linux, written in Rust. Windows live on one
endless canvas; each screen is a camera you pan and zoom over it. Built for my own desktop and
workflow, with AI agents doing a lot of the typing.

**Ready out of the box, every piece your choice.** A fresh install is a complete desktop:
terminal, launcher, lock screen, wallpaper, screenshots, screen sharing, clipboard history,
session restore. Nothing of it is fixed. Each app is a setting (`TERMINAL`, `LAUNCHER`,
`FILE_MANAGER`, …, or any command), every key can be rebound, the optional helpers (clipboard
history, session restore, the replay buffer, the island) are switches, and the whole
configuration is one commented file. No settings daemons: what the toolkits need is written to
their files once.

Status: my daily desktop, tested on my own hardware only. See [docs/STATUS.md](docs/STATUS.md)
for what is solid, what is untested and what is missing.

## Install

Arch Linux:

```bash
git clone https://github.com/vysaely/infynitae.git ~/infynitae
cd ~/infynitae
./install.sh
```

It asks for your password once at the start and then runs by itself. Then reboot: without a
display manager the machine starts at the Infynitae lock screen (the session starts locked,
nothing runs before you unlock; it is also what RustDesk reaches after a reboot). With a display
manager (GDM, SDDM…), pick **Infynitae** at its login.

For the text login instead, install with `./install.sh --no-autologin`, or later run
`sudo ~/infynitae/live/setup-autologin.sh --off`: logging in on tty1 then starts Infynitae.

Update any time with `infynitae-update` (your config is never overwritten).

## Shortcuts

| Key | Action |
| :--- | :--- |
| `Super + 1..9` | Focus window slot 1..9 |
| `Super + Shift + 1..9` | Focus cluster slot 1..9 |
| `Super + Space` | Center the focused window, zoom back to 100% |
| `Super + Shift + Space` | Center the focused window, keep the zoom |
| `Super + Enter` | Maximize the window, or fit the view to it (`SUPER_ENTER_ACTION`) |
| `Super + Shift + Enter` | Fullscreen (you can still pan out of it) |
| `Super + Left Drag` | Move window |
| `Super + Right Drag` | Resize window from nearest corner |
| `Super + Shift + Left Drag` | Move entire window cluster |
| `Super + Shift + Right Drag` | Resize entire window cluster |
| `Super + Middle Drag` | Pan canvas (`free`, `grab`, or `rts` mode) |
| `Super + Shift + Wheel` | Zoom canvas in / out |
| `Super + Tab` | Cycle window focus (auto-centers camera) |
| `Super + Shift + Tab` | Canvas overview toggle (fits all windows / restores view) |
| `Super + Q` | Terminal (`TERMINAL`) |
| `Super + W` | Launcher (`LAUNCHER`) |
| `Super + E` | Files (`FILE_MANAGER`) |
| `Super + Shift + S` / `Print` | Screenshot full screen |
| `Super + Shift + R` | Region screenshot (`slurp` + `grim`) |
| `Super + Shift + Backspace` | Reload the config and restart the helpers (windows stay) |
| `Super + L` | Lock the screen |
| `Super + V` | Clipboard history (`cliphist`) |
| `Super + A` | Control menu: Wi-Fi, Bluetooth, sound output, power mode, do not disturb |
| `Super + C` | Close focused window |
| `Super + Shift + C` | Force close (ends the app) |
| `Super + Shift + P` | Hide the focused window from screen captures |
| `Super + Shift + I` | Stay awake (no idle screen-off / auto-lock) |
| `Super + Alt + C` | Save a replay clip (`ENABLE_REPLAY_BUFFER`) |
| `Super + Alt + R` | Start / stop recording |
| `Super` (held) | Frees a cursor a game captured (`SUPER_FREES_CURSOR`) |
| `Ctrl + Alt + Backspace` | Restart the compositor (windows close) |
| `Ctrl + Alt + F1..F12` | Switch VT |
| `XF86Audio*` / `XF86MonBrightness*` | Volume & brightness control |

## Changing things

Everything is in one file: **`~/.config/infynitae/config`**. Every option is explained in it with
its default, and each section says when a change applies (most of them as soon as you save).

- Apps: `TERMINAL`, `LAUNCHER`, `FILE_MANAGER`, `BROWSER`: `"auto"` or any command.
- Keys: `BIND_TERMINAL="SUPER+RETURN"` moves a shortcut, `BIND_LAUNCHER="none"` frees one,
  `EXEC_BROWSER="SUPER+B:firefox"` adds your own.
- Look: `CURSOR_THEME`, `ICON_THEME`, `FONT_NAME`, `WALLPAPER`, `OUTPUT_SCALE`.

`Super + Shift + Backspace` reloads it.

## When something goes wrong

`~/.local/state/infynitae/session.log` says what the compositor saw: the screens it found,
config lines it skipped, errors. `infynitae-session stop` ends the session; after three quick
crashes it stops by itself.

## Documentation

| | |
| :--- | :--- |
| [docs/STATUS.md](docs/STATUS.md) | What works, what is untested, what is missing |
| [docs/COMPONENTS.md](docs/COMPONENTS.md) | Every piece of the desktop, what to install instead, and which ones to build ourselves |
| [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) | Working on it: layout, building, the test suite and the checks, the design docs |
| [docs/DECISIONS.md](docs/DECISIONS.md) | The choices behind how it behaves, and why |
| [docs/foundation/](docs/foundation) | The design specification: invariants (`INV-*`), state machines (`M*`), formulas (`F-*`) |

## Acknowledgements

Inspired by [driftwm](https://github.com/malbiruk/driftwm) and [hyprland-infinitie-desktop-v2](https://github.com/sarodscommits/hyprland-infinitie-desktop-v2).

## License

[MIT](LICENSE) © vysaely

