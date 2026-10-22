# Infynitae

Infinite canvas Wayland compositor built in Rust for Arch Linux. Built for my own system and workflow.

## Keybindings

| Key | Action |
| :--- | :--- |
| `Super + 1..9` | Focus window slot 1..9 |
| `Super + Shift + 1..9` | Focus cluster slot 1..9 |
| `Super + Space` | Recenter window |
| `Super + Enter` | Fit window to screen |
| `Super + Shift + Enter` | Fullscreen |
| `Super + Shift + Tab` | Canvas radar HUD |
| `Super + Shift + R` | Reload shell UI |
| `Super + W` | App launcher |
| `Super + Q` | Terminal (`foot`) |
| `Super + E` | File manager (`yazi`) |
| `Super + Shift + S` | Screenshot |
| `Super + Delete` | Close window |
| `Super + Shift + Delete` | Kill process |
| `Ctrl + Alt + Backspace` | Exit |
| `Ctrl + Alt + F1..F12` | Switch VT |

## Configuration

Edit `~/.config/infynitae/config`:

- Mouse speed and acceleration (`flat` or `adaptive`)
- Keyboard layout and repeat rate
- Cursor theme and size
- Default apps (`foot`, `yazi`, `firefox`)

Apply changes:
```bash
infynitae-settings set CURSOR_THEME "Adwaita"
```

## Install

```bash
git clone https://github.com/vysaely/infynitae.git
cd infynitae
./install.sh
```

## License

[MIT](LICENSE) © vysaely
