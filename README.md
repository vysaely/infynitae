# Infynitae

An uncompromising infinite canvas Wayland compositor built in Rust for Arch Linux.

Zero bloat. Zero inertia. Sub-millisecond latency. No background daemons eating RAM. Built for my own system.

---

## Keybindings

| Key | Action |
| :--- | :--- |
| `Super + 1..9` | Focus, raise and center window slot 1..9 |
| `Super + Shift + 1..9` | Focus and frame cluster slot 1..9 |
| `Super + Space` | Recenter active window |
| `Super + Enter` | Fit active window to screen |
| `Super + Shift + Enter` | Toggle fullscreen |
| `Super + Shift + Tab` | Toggle 2D canvas radar |
| `Super + Shift + R` | Hot-reload UI and scripts |
| `Super + W` | Dynamic island launcher |
| `Super + Q` | Terminal (`foot`) |
| `Super + E` | File manager (`yazi`) |
| `Super + Shift + S` | Screenshot (`grim` + `wl-copy`) |
| `Super + Delete` | Close focused window |
| `Super + Shift + Delete` | Kill focused process |
| `Ctrl + Alt + Backspace` | Exit compositor |
| `Ctrl + Alt + F1..F12` | Switch VT |

---

## Configuration

Single static file at `~/.config/infynitae/config`:

- Mouse acceleration (`flat` for raw 1:1 input)
- Keyboard repeat delay & rate
- Keyboard layout & options
- Cursor theme & size (synced across Wayland, GTK, Qt, and Steam with 0 daemons)
- Default apps (`foot`, `yazi`, `firefox`)

Sync settings anytime:
```bash
infynitae-settings set MOUSE_ACCEL_PROFILE "flat"
infynitae-settings set CURSOR_THEME "Adwaita"
```

---

## Install (Arch Linux)

```bash
git clone https://github.com/vysaely/infynitae.git
cd infynitae
./install.sh
```

---

## Dev & Sandbox

```bash
infynitae-dev test    # Run nested sandbox inside a window
infynitae-dev logs    # Stream logs live
infynitae-dev build   # Rebuild release binary
```

---

## License

[MIT](LICENSE) © vysaely
