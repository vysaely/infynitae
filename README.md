# Infynitae

Infinite canvas Wayland compositor built in Rust for Arch Linux. Vibecoded for my own system and workflow.

## Keybindings

| Key | Action |
| :--- | :--- |
| `Super + 1..9` | Focus window slot 1..9 |
| `Super + Shift + 1..9` | Focus cluster slot 1..9 |
| `Super + Space` | Recenter window |
| `Super + Enter` | Fit window to screen |
| `Super + Shift + Enter` | Fullscreen |
| `Super + Q` | Terminal (auto: kitty, alacritty, foot) |
| `Super + W` | Launcher (`fuzzel`) |
| `Super + E` | Files (`yazi`) |
| `Super + Shift + S` | Screenshot |
| `Super + Shift + R` | Reload config |
| `Super + Delete` | Close window |
| `Super + Shift + Delete` | Kill process |
| `Ctrl + Alt + Backspace` | Exit |
| `Ctrl + Alt + F1..F12` | Switch VT |

## Configuration

Single config file at `~/.config/infynitae/config`:

- Mouse speed and acceleration (`flat` or `adaptive`)
- Keyboard layout and repeat rate
- Cursor theme, size, and font
- Default apps (`TERMINAL`, `LAUNCHER`, `FILE_MANAGER`)

Apply:
```bash
infynitae-settings apply
```

## Install

```bash
git clone https://github.com/vysaely/infynitae.git
cd infynitae
./install.sh
```

## Acknowledgements

Inspired by [driftwm](https://github.com/malbiruk/driftwm) and [hyprland-infinitie-desktop-v2](https://github.com/sarodscommits/hyprland-infinitie-desktop-v2).

## License

[MIT](LICENSE) © vysaely
