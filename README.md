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

## Keybindings

| Key | Action |
| :--- | :--- |
| `Super + 1..9` | Focus window slot 1..9 |
| `Super + Shift + 1..9` | Focus cluster slot 1..9 |
| `Super + Space` | Center the focused window, zoom back to 100% |
| `Super + Shift + Space` | Center the focused window, keep the zoom |
| `Super + Enter` | Maximize / fit window to viewport (0% blur) |
| `Super + Shift + Enter` | Borderless fullscreen (pan out anytime) |
| `Super + Left Drag` | Move window |
| `Super + Right Drag` | Resize window from nearest corner |
| `Super + Shift + Left Drag` | Move entire window cluster |
| `Super + Shift + Right Drag` | Resize entire window cluster |
| `Super + Middle Drag` | Pan canvas (`free`, `grab`, or `rts` mode) |
| `Super + Shift + Wheel` | Zoom canvas in / out |
| `Super + Tab` | Cycle window focus (auto-centers camera) |
| `Super + Shift + Tab` | Canvas overview toggle (fits all windows / restores view) |
| `Super + Q` | Terminal (auto: kitty, alacritty, foot) |
| `Super + W` | Launcher (auto: `fuzzel`, `rofi`, `tofi`, `wofi`) |
| `Super + E` | Files (`yazi`, `thunar`, `dolphin`) |
| `Super + Shift + S` / `Print` | Screenshot full screen |
| `Super + Shift + R` | Region screenshot (`slurp` + `grim`) |
| `Super + Shift + Backspace` | Reload config & desktop environment |
| `Super + L` | Lock screen (native `infynitae-lock`) |
| `Super + V` | Clipboard history (`cliphist`) |
| `Super + C` | Close focused window |
| `Super + Shift + C` | Force kill window process |
| `Super + Shift + P` | Hide the focused window from screen captures |
| `Super + Shift + I` | Stay awake (no idle screen-off / auto-lock) |
| `Super + Alt + C` | Save a replay clip (`ENABLE_REPLAY_BUFFER`) |
| `Super + Alt + R` | Start / stop recording |
| `Super` (held) | Frees a cursor a game captured (`SUPER_FREES_CURSOR`) |
| `Ctrl + Alt + Backspace` | Restart the compositor (`infynitae-session stop` quits) |
| `Ctrl + Alt + F1..F12` | Switch VT |
| `XF86Audio*` / `XF86MonBrightness*` | Volume & brightness control |

Any action can be rebound, and any command bound, in the config (`BIND_<ACTION>`, `EXEC_<NAME>`).

## Configuration

Single config file at `~/.config/infynitae/config` (the install writes the commented template;
each section says when its changes apply, most of them the moment you save):

- Scaling filter (`nearest` for razor-sharp 0% blur, `linear` for smooth) and `OUTPUT_SCALE`
- Cluster tiling and smart adjacent window placement (`PLACEMENT_POLICY="cluster"`)
- Focus and camera for new windows (`FOCUS_ON_OPEN`, `TELEPORT_ON_OPEN`: by default only windows you opened)
- Mouse speed, acceleration profile (`flat` or `adaptive`), and pan mode (`free`, `grab`, `rts`)
- Window focus model (`sloppy`, `strict`, or `click`)
- Screen sharing & remote desktop cursor suppression (`SCREENCOPY_CURSOR="false"`)
- Wallpaper (`WALLPAPER`; a video plays through `mpvpaper` when installed)
- Keyboard layout, repeat delay, and repeat rate
- Cursor theme, size, and fonts
- Default apps (`TERMINAL`, `LAUNCHER`, `FILE_MANAGER`, `BROWSER`, `SCREEN_LOCKER`)
- Session restore, clipboard history, screen sharing prompt, replay buffer and recorder

Restart the helpers (wallpaper, portal, recorder, clipboard…) without closing windows:
```bash
infynitae-reload          # or Super + Shift + Backspace
```

Reset canvas zoom to 100%:
```bash
infynitae-settings restore
```

## Install

```bash
git clone https://github.com/vysaely/infynitae.git
cd infynitae
./install.sh
```

## Update

Run anytime from anywhere to pull latest changes, rebuild, and update without touching your custom configs:
```bash
infynitae-update
```

## When something goes wrong

`~/.local/state/infynitae/session.log` has what the compositor saw: the screens it found, config
lines it skipped, errors. `Ctrl+Alt+Backspace` restarts the compositor, `infynitae-session stop`
ends the session, and after three quick crashes the session stops by itself.

## Documentation

| | |
| :--- | :--- |
| [docs/STATUS.md](docs/STATUS.md) | What works, what is untested, what is missing |
| [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) | Working on it: layout, building, the test suite and the checks, the design docs |
| [docs/DECISIONS.md](docs/DECISIONS.md) | The choices behind how it behaves, and why |
| [docs/foundation/](docs/foundation) | The design specification: invariants (`INV-*`), state machines (`M*`), formulas (`F-*`) |

## Acknowledgements

Inspired by [driftwm](https://github.com/malbiruk/driftwm) and [hyprland-infinitie-desktop-v2](https://github.com/sarodscommits/hyprland-infinitie-desktop-v2).

## License

[MIT](LICENSE) © vysaely

