# Infynitae

Infinite canvas Wayland compositor built in Rust for Arch Linux. Vibecoded for my own system and workflow.

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
| `Super + Delete` | Close focused window |
| `Super + Shift + Delete` | Force kill window process |
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

## Documentation

For a complete breakdown of every component, toolkit compatibility rule, dependency list, and instructions for customizing or building your own Wayland compositor and desktop environment, see the [Architecture & Standalone Desktop Blueprint](docs/COMPOSITOR_AND_DESKTOP_GUIDE.md).

## Acknowledgements

Inspired by [driftwm](https://github.com/malbiruk/driftwm) and [hyprland-infinitie-desktop-v2](https://github.com/sarodscommits/hyprland-infinitie-desktop-v2).

## License

[MIT](LICENSE) © vysaely

