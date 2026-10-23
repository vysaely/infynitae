# Infynitae

Infinite canvas Wayland compositor built in Rust for Arch Linux. Vibecoded for my own system and workflow.

## Keybindings

| Key | Action |
| :--- | :--- |
| `Super + 1..9` | Focus window slot 1..9 |
| `Super + Shift + 1..9` | Focus cluster slot 1..9 |
| `Super + Space` | Recenter window |
| `Super + Enter` | Maximize / fit window to viewport (0% blur) |
| `Super + Shift + Enter` | Borderless fullscreen (pan out anytime) |
| `Super + Middle Drag` | Pan canvas (`free`, `grab`, or `rts` mode) |
| `Super + Shift + Middle Click` | Reset zoom to 100% native scale |
| `Super + Shift + Wheel` | Zoom canvas in / out |
| `Super + Tab` / `Alt + Tab` | Cycle window focus (auto-centers camera) |
| `Super + Shift + Tab` | Window switcher / overview (`fuzzel` / `rofi`) |
| `Super + Q` | Terminal (auto: kitty, alacritty, foot) |
| `Super + W` | Launcher (`fuzzel`) |
| `Super + E` | Files (`yazi`, `thunar`, `dolphin`) |
| `Super + Shift + S` | Screenshot full screen |
| `Super + Shift + R` | Region screenshot (`slurp` + `grim`) |
| `Super + Shift + Backspace` | Reload config & desktop environment |
| `Super + L` | Lock screen (`hyprlock`, `swaylock`, `gtklock`, `waylock`) |
| `Super + Delete` | Close window |
| `Super + Shift + Delete` | Kill process |
| `Ctrl + Alt + Backspace` | Exit |
| `Ctrl + Alt + F1..F12` | Switch VT |
| `XF86Audio*` / `XF86MonBrightness*` | Volume & brightness control |

## Configuration

Single config file at `~/.config/infynitae/config`:

- Scaling filter (`nearest` for razor-sharp 0% blur, `linear` for smooth) and `OUTPUT_SCALE`
- Cluster tiling and smart adjacent window placement (`PLACEMENT_POLICY="cluster"`)
- Auto-focus (`FOCUS_ON_OPEN="1"`) and camera teleportation (`TELEPORT_ON_OPEN="1"`) for new windows
- Mouse speed, acceleration profile (`flat` or `adaptive`), and pan mode (`free`, `grab`, `rts`)
- Window focus model (`sloppy`, `strict`, or `click`)
- Screen sharing & remote desktop cursor suppression (`SCREENCOPY_CURSOR="false"`)
- Wallpaper and live animated background (`WALLPAPER`)
- Keyboard layout, repeat delay, and repeat rate
- Cursor theme, size, and fonts
- Default apps (`TERMINAL`, `LAUNCHER`, `FILE_MANAGER`, `SCREEN_LOCKER`)

Apply changes:
```bash
infynitae-settings apply
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

## Acknowledgements

Inspired by [driftwm](https://github.com/malbiruk/driftwm) and [hyprland-infinitie-desktop-v2](https://github.com/sarodscommits/hyprland-infinitie-desktop-v2).

## License

[MIT](LICENSE) © vysaely

