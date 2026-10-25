# Components

Every piece of the desktop around the compositor: what Infynitae uses by default, what you can
install instead, how to switch, and whether it is worth building our own. **Ours** means it is in
this repository already.

## Apps and menus

| Piece | Default | Alternatives you can install | Switch | Build our own? |
| :--- | :--- | :--- | :--- | :--- |
| Terminal | first installed: kitty, alacritty, foot… | wezterm, ghostty, konsole, any | `TERMINAL` | No: huge, and good ones exist |
| Launcher | fuzzel | rofi, tofi, wofi, walker, anyrun | `LAUNCHER` | **Yes, a good fit**: a launcher that knows the canvas (open here, jump to a window). The Quickshell island is a start |
| Menus (control menu, clipboard) | `fuzzel --dmenu` | `rofi -dmenu`, tofi, bemenu, `wofi --dmenu` | `MENU_PICKER` | Comes with our launcher |
| File manager | yazi, lf (in the terminal), thunar, nemo, dolphin | nautilus, pcmanfm, any | `FILE_MANAGER` | No |
| Browser | the system default | any | `BROWSER` | No |

## Desktop services

| Piece | Default | Alternatives | Switch | Build our own? |
| :--- | :--- | :--- | :--- | :--- |
| Notifications | mako | swaync (with a panel), dunst, fnott | `NOTIFICATIONS` | **Yes, small**: notifications placed on the screen you look at, matching the canvas look |
| Volume / brightness pop-ups | `notify-send` from `live/launch.sh` | swayosd, avizo, wob | none yet | **Yes, small**: a native on-screen display |
| Control menu (Wi-Fi, Bluetooth, sound, power) | `live/control-menu` (ours) on fuzzel | nm-applet, iwgtk, blueman, pwvucontrol, a bar's widgets | `CONTROL_MENU` | Ours already; a graphical panel later |
| Bar / panel | none (optional Quickshell island, ours) | waybar, ironbar, eww, quickshell | `ENABLE_QUICKSHELL`, or start yours in `autostart.sh` | **Yes, medium**: the island is the start |
| Clipboard history | cliphist + wl-clip-persist | clipman, copyq | `CLIPBOARD_HISTORY`, `CLIPBOARD_PICKER` | Low value |
| Screenshots | grim + slurp | grimblast, flameshot, satty (annotate) | `SCREENSHOT_CMD` | **Yes, small**: the compositor can already capture; a native region picker is little work |
| Recording, replay buffer | `infynitae-recorder` (ours) + ffmpeg | wf-recorder, gpu-screen-recorder, OBS | `ENABLE_REPLAY_BUFFER` | Ours |
| Wallpaper | `infynitae-wallpaper` (ours) | swaybg, swww, mpvpaper | `WALLPAPER` | Ours |
| Lock screen | `infynitae-lock` (ours) | swaylock, gtklock, hyprlock | `SCREEN_LOCKER` | Ours |
| Admin password prompts | `infynitae-polkit` (ours) | polkit-gnome, lxqt-policykit, hyprpolkitagent | `POLKIT_AGENT` | Ours |
| Keyring (saved passwords) | gnome-keyring | KeePassXC (Secret Service), kwallet | whichever is installed answers D-Bus | No: security code, leave it to the experts |
| USB automount | udiskie (udisks2) | (udiskie is the usual one) | `AUTOMOUNT` | No |
| Session restore | `infynitae-restore` (ours) | none comparable | `SESSION_RESTORE` | Ours |
| Settings sync (GTK, Qt, cursor) | `infynitae-settings` (ours) | nwg-look, qt6ct | edit the config | Ours |
| Global shortcuts for apps | `infynitae-portal` (ours) | none for this compositor | always on | Ours |
| Screen sharing | xdg-desktop-portal-wlr (+ gtk for file dialogs) | none that fits better | fixed | Later, medium: our own portal could show a window picker on the canvas |
| Login | your display manager, or tty autologin | SDDM, GDM, greetd + tuigreet | system setting | **Maybe, medium**: a greeter in the lock screen's look, for greetd |
| X11 apps | xwayland-satellite | (none needed) | `XWAYLAND` | No |

## Needs the compositor first

These tools exist and are good, but need a Wayland protocol Infynitae does not speak yet. Each is
compositor work in this repository, then the tool just works:

| Piece | Tools that would work | Protocol to add |
| :--- | :--- | :--- |
| Display settings app | wdisplays, nwg-displays, kanshi (profiles), wlr-randr | `wlr-output-management` |
| Input methods (Japanese, Chinese, Korean) | fcitx5, ibus | `input-method-v2`, `text-input-v3` |

Already spoken, so these just work: night light (wlsunset, gammastep: `wlr-gamma-control`),
screen power (wlopm: `wlr-output-power-management`), external idle tools (swayidle, hypridle:
`ext-idle-notify`; idle and auto-lock are built in already), task bars and window switchers
(waybar's taskbar, rofi's window mode: `wlr-foreign-toplevel-management`,
`ext-foreign-toplevel-list`), apps naming their cursor (`cursor-shape-v1`), apps taking the
shortcuts while focused (`keyboard-shortcuts-inhibit`).
