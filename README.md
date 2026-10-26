# Infynitae

An infinite-canvas Wayland compositor for Arch Linux, written in Rust. Your windows sit on one
endless canvas, and each screen is a camera you pan and zoom over it.

A fresh install is a complete desktop: terminal, launcher, lock screen, wallpaper, screenshots,
screen sharing, clipboard history and session restore. Every piece is a setting you can swap or
turn off, every key can be rebound, and the whole configuration is one commented file.

**Status:** my daily desktop, tested on my own hardware only. [docs/STATUS.md](docs/STATUS.md)
says what is solid, what is untested and what is missing.

## Install

```bash
git clone https://github.com/vysaely/infynitae.git ~/infynitae
~/infynitae/install.sh
```

It asks for your password once, then runs by itself. Reboot when it is done:

- **No display manager:** the machine starts at the Infynitae lock screen.
- **A display manager** (GDM, SDDM…): pick **Infynitae** at its login.

The installer updates the system first, installs the right graphics drivers (NVIDIA included),
and stops before changing anything if there is no internet, disk space or sudo. Its log is
`~/.local/state/infynitae/install.log`.

### Options

| Flag | Effect |
| :--- | :--- |
| `--no-notifications` `--no-keyring` `--no-automount` `--no-clipboard` `--no-fonts` `--no-recorder` `--no-terminal` `--no-bluetooth` `--no-power-profiles` `--no-autorotate` | Leave out that part |
| `--minimal` | Leave out all of them |
| `--replay-buffer` | Keep the last minutes of the screen, saved with `Super + Alt + C` |
| `--no-autologin` | Log in on tty1 instead of booting to the lock screen |

`./install.sh --help` explains each one.

### Update

```bash
infynitae-update
```

Your config is never overwritten, and parts left out at install stay out.

## Shortcuts

| Key | Action |
| :--- | :--- |
| `Super + 1..9` | Focus window slot 1..9 |
| `Super + Shift + 1..9` | Focus cluster slot 1..9 |
| `Super + Space` | Center the focused window at 100% zoom |
| `Super + Shift + Space` | Center the focused window, keep the zoom |
| `Super + Enter` | Maximize the window, or fit the view to it (`SUPER_ENTER_ACTION`) |
| `Super + Shift + Enter` | Fullscreen (you can still pan out of it) |
| `Super + Tab` | Next window |
| `Super + Shift + Tab` | Overview of all windows |
| `Super + Left drag` | Move a window |
| `Super + Right drag` | Resize a window |
| `Super + Shift + Left / Right drag` | Move / resize a whole cluster |
| `Super + Middle drag` | Pan the canvas |
| `Super + Shift + Wheel` | Zoom the canvas |
| Finger on empty canvas | Drag the canvas (two fingers: pinch to zoom) |
| `Super + Q` / `W` / `E` | Terminal / launcher / files |
| `Super + Shift + S` or `Print` | Screenshot |
| `Super + Shift + R` | Region screenshot |
| `Super + V` | Clipboard history |
| `Super + A` | Control menu: Wi-Fi, Bluetooth, volume (all sound or one app), sound output, power, do not disturb |
| `Super + L` | Lock |
| `Super + C` | Close the window (`Shift`: force) |
| `Super + Shift + P` | Hide the window from screenshots and sharing |
| `Super + Shift + I` | Stay awake |
| `Super + Alt + R` | Start / stop recording |
| `Super + Alt + C` | Save a replay clip |
| `Super` (held) | Free a cursor a game captured |
| `Super + Shift + Backspace` | Reload the config (windows stay) |
| `Ctrl + Alt + Backspace` | Restart the compositor (windows close) |

## Configuration

Everything is in **`~/.config/infynitae/config`**. Each option is explained there with its
default, and most changes apply as soon as you save.

```bash
TERMINAL="kitty"                    # apps: "auto" or any command
BIND_TERMINAL="SUPER+RETURN"        # move a shortcut
BIND_LAUNCHER="none"                # free one
EXEC_BROWSER="SUPER+B:firefox"      # add your own
```

## Troubleshooting

`~/.local/state/infynitae/session.log` shows what the compositor saw: the screens it found,
config lines it skipped, errors.

After a crash the session restarts by itself (locked) and a notification says what happened.
The report, with the panic message and the end of the log, is in
`~/.local/state/infynitae/crashes/`; attach it to a bug report. After three crashes in a row
within seconds it stops restarting. `infynitae-session stop` ends the session.

## Documentation

| | |
| :--- | :--- |
| [STATUS](docs/STATUS.md) | What works, what is untested, what is missing |
| [COMPONENTS](docs/COMPONENTS.md) | Every piece of the desktop and what to use instead |
| [DEVELOPMENT](docs/DEVELOPMENT.md) | Layout, building, the test suite and the checks |
| [DECISIONS](docs/DECISIONS.md) | The choices behind how it behaves, and why |
| [foundation/](docs/foundation) | The design specification: invariants, state machines, formulas |

## Acknowledgements

Inspired by [driftwm](https://github.com/malbiruk/driftwm) and
[hyprland-infinitie-desktop-v2](https://github.com/sarodscommits/hyprland-infinitie-desktop-v2).

## License

[MIT](LICENSE) © vysaely
