# Components

Every piece of the desktop around the compositor: what Infynitae uses by default, what you can
install instead, how to switch, and whether it is worth building our own. **Ours** means it is in
this repository already.

## Apps and menus

| Piece | Default | Alternatives you can install | Switch | Build our own? |
| :--- | :--- | :--- | :--- | :--- |
| Terminal | first installed: kitty, alacritty, foot… | wezterm, ghostty, konsole, any | `TERMINAL` | No: huge, and good ones exist |
| Launcher | the edge (Super+W), fuzzel without it | rofi, tofi, wofi, walker, anyrun | `LAUNCHER`, `ENABLE_EDGE` | **Yes, a good fit**: a launcher that knows the canvas (open here, jump to a window). The edge is a start |
| Menus (control menu, clipboard) | `fuzzel --dmenu` | `rofi -dmenu`, tofi, bemenu, `wofi --dmenu` | `MENU_PICKER` | Comes with our launcher |
| File manager | thunar with tumbler's previews (installed unless another is in use); yazi, lf (in the terminal), nemo, dolphin when installed | nautilus, pcmanfm, any | `FILE_MANAGER` | No |
| System monitor | a window if installed (missioncenter, gnome-system-monitor, plasma-systemmonitor), else btop, htop or top in the terminal | any | `SYSTEM_MONITOR` | No |
| Browser | the system default | any | `BROWSER` | No |

## Desktop services

| Piece | Default | Alternatives | Switch | Build our own? |
| :--- | :--- | :--- | :--- | :--- |
| Notifications | the edge (ours), else first installed: mako, swaync, dunst, fnott | mako, swaync (with a panel), dunst, fnott | `NOTIFICATIONS` | Ours (pop-ups and history in `edge-rs` and `edge-apps`; `EDGE_IMPL="c"` falls back) |
| Volume / brightness pop-ups | `notify-send` from `live/launch.sh` | swayosd, avizo, wob | none yet (the edge's sound card shows the level) | **Yes, small**: a native on-screen display |
| Control menu (Wi-Fi, Bluetooth, volume, sound output, power) | `live/control-menu` (ours) on fuzzel | nm-applet, iwgtk, blueman, pwvucontrol, a bar's widgets | `CONTROL_MENU` | Ours already; the edge's control panel (Super+A) is the graphical one |
| Bar / panel | the edge (ours: `live/edge-rs` by default, `live/edge-apps` as `EDGE_IMPL="apps"` replacing it; `live/edge` in C is archived as `EDGE_IMPL="c"`) | waybar, ironbar, eww, quickshell | `ENABLE_EDGE="false"`, then start yours in `autostart.sh`; `LAUNCHER` / `CONTROL_MENU` give it Super+W / Super+A | Ours (`live/edge` and `live/edge-rs` are frozen) |
| Clipboard history | cliphist + wl-clip-persist | clipman, copyq | `CLIPBOARD_HISTORY`, `CLIPBOARD_PICKER` | Low value |
| Screenshots | `infynitae-shot` (ours, `live/shot-rs`), else `grim` + `slurp` | grimblast, flameshot, satty (annotate) | `SCREENSHOT_CMD` (whole screen) | Ours (`live/shot-rs`) |
| Recording, replay buffer | `infynitae-recorder` (ours) + ffmpeg | wf-recorder, gpu-screen-recorder, OBS | `ENABLE_REPLAY_BUFFER` | Ours |
| Wallpaper | `infynitae-wallpaper` (ours) | swaybg, swww, mpvpaper | `WALLPAPER` | Ours |
| Lock screen | `infynitae-lock` (ours) | swaylock, gtklock, hyprlock | `SCREEN_LOCKER` | Ours |
| Admin password prompts | `infynitae-polkit` (ours) | polkit-gnome, lxqt-policykit, hyprpolkitagent | `POLKIT_AGENT` | Ours |
| Keyring (saved passwords) | gnome-keyring | KeePassXC (Secret Service), kwallet | whichever is installed answers D-Bus | No: security code, leave it to the experts |
| USB automount | infynitae-automount (ours: a shell watcher on udisks2) | udiskie | `AUTOMOUNT` | No |
| Formatting a drive | infynitae-format-drive (ours: udisks2; asks like the panels; "Format drive..." in PCManFM-Qt's menu) | GNOME Disks | none | No |
| Questions from scripts | `infynitae-edge ask` (ours: a list or a field drawn like the panels; prints the answer) | fuzzel, zenity | none | No |
| Session restore | `infynitae-restore` (ours) | none comparable | `SESSION_RESTORE` | Ours |
| Settings sync (GTK, Qt, cursor) | `infynitae-settings` (ours) | nwg-look, qt6ct | edit the config | Ours |
| Global shortcuts for apps | `infynitae-portal` (ours) | none for this compositor | always on | Ours |
| Screen sharing (a screen or one window) | xdg-desktop-portal-wlr (+ gtk for file dialogs) | none that fits better | `SCREENSHARE_ASK` (true: pick a screen or a window) | Later, medium: our own portal could show a window picker on the canvas |
| Login | your display manager, or tty autologin | SDDM, GDM, greetd + tuigreet | system setting | **Maybe, medium**: a greeter in the lock screen's look, for greetd |
| X11 apps | xwayland-satellite | (none needed) | `XWAYLAND` | No |

Already supported: display settings (wdisplays, nwg-displays, kanshi, wlr-randr:
`wlr-output-management`; mode, scale, position and rotation, not yet turning a screen off), night light (wlsunset, gammastep: `wlr-gamma-control`),
screen power (wlopm: `wlr-output-power-management`), external idle tools (swayidle, hypridle:
`ext-idle-notify`; idle and auto-lock are built in already), task bars and window switchers
(waybar's taskbar, rofi's window mode: `wlr-foreign-toplevel-management`,
`ext-foreign-toplevel-list`), apps naming their cursor (`cursor-shape-v1`), apps taking the
shortcuts while focused (`keyboard-shortcuts-inhibit`), input methods (fcitx5, ibus:
`input-method-v2`, `text-input-v3`, `virtual-keyboard`; none is installed or started for you).
