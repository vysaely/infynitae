# Status

Updated 2026-10-26.

Infynitae is my daily desktop. It is young and has only run on my own machine, so expect rough
edges on other hardware.

## Solid

- **The spatial model:** the endless canvas, a camera per screen (pan, zoom, fit, overview),
  clusters, window slots, fullscreen you can pan out of. The core is a pure state machine with
  no Wayland code in it, tested exactly.
- **The test suite:** about 830 tests, most of them real Wayland clients against a headless
  compositor. `tools/verify-test-efficacy` seeds bugs on purpose to prove the tests catch them,
  and `tools/verify-architecture` checks that the architecture holds.
- **The desktop:** lock screen, wallpaper, screenshots, clipboard history, session restore,
  recording and a replay buffer, polkit prompts, notifications (mako), a keyring
  (gnome-keyring), USB automount (infynitae-automount on udisks2: encrypted drives ask for their passphrase, a notification opens or ejects a drive, `infynitae-automount eject` makes every drive safe to pull), the control menu (Super+A: Wi-Fi, Bluetooth, volume
  for all sound or one app, sound output, power mode), global shortcuts for
  apps, layer shell for bars, X11 apps through xwayland-satellite, VRR and direct scanout for
  games, idle screen-off and auto-lock.
- **Screen sharing:** a screen or a single window, through the portal or screencopy. A
  private window (`PRIVACY_APPS`, `PRIVACY_TITLES`, Super+Shift+P) is never shared, and nothing
  is while the screen is locked. A small red dot shows on every screen while anything captures
  it, and is never in the capture itself (`CAPTURE_INDICATOR`). Over a fullscreen window it
  steps aside, so a game still goes to the screen directly, and a notification says the screen
  is being captured instead.
- **RustDesk as a host** (seen working on this machine): the picture through the portal, input
  through its own virtual devices. The session restarts RustDesk's service once the compositor
  runs, which needs the polkit rule in `live/remote/` (the installer adds it).
- **Staying awake:** apps that play video or a game keep the screens on, both Wayland apps
  (idle-inhibit) and others over D-Bus (`org.freedesktop.ScreenSaver`). Locking still turns
  the screens off on time.
- **Protocols for the usual tools:** display settings (wlr-randr, wdisplays, kanshi), night
  light (wlsunset, gammastep), screen power (wlopm), idle tools (swayidle, hypridle), task bars
  and window switchers (waybar, rofi), apps taking the shortcuts while focused (VMs, remote
  desktops), cursor shapes, input methods (fcitx5, ibus: start one from `autostart.sh`).

## Not yet tried on real hardware

These pass the test suite but have not run on a real screen or device yet:

- **Other machines.** The console backend (real screens, GPUs, input) cannot run in the test
  suite and has only run on my machine. Multi-GPU has never been tried.
- **Monitor hotplug:** a screen plugged in, unplugged or woken is found about 1.5 s after the
  last change, and the session can start with no screen and wait for one. A sleeping screen is
  never removed while asleep.
- **Rotation, mode and VRR changes** from display tools or `OUTPUT_<NAME>_TRANSFORM`. Captures
  and touch turn with the screen. VRR as tools see it is whether a screen may use it (on, or
  auto while fullscreen); turning it off from a tool lasts until the screen reconnects.
- **Auto-rotate** on convertibles (`AUTO_ROTATE`, needs iio-sensor-proxy): the laptop's own
  screen turns with the device.
- **Touchscreens** as real touch (`wl_touch`): apps get each finger, so scrolling, pinching and
  gestures work inside them. A tap focuses the window under it. A finger on empty canvas drags
  the canvas; two fingers pinch-zoom it around their midpoint.
- **Virtual screen** (`VIRTUAL_SCREEN`, on by default): with no screen connected (a monitor off
  that reports itself unplugged, or none at all), a virtual 1920x1080 screen stands in, so remote
  desktop still has a screen; it goes once a real one is connected. A monitor that is off but
  still connected keeps being drawn as before.
- **Zoomed-out windows averaged** (`SCALING_FILTER`, `fast` turns it off): text and thin lines
  stay legible below 100% instead of shimmering; checked on this GPU off-screen. `MAX_APP_SCALE`
  sets how far apps redraw sharper as you zoom in (default 1: zooming in magnifies without the app redrawing; 2 or 3 redraw sharper).
- **Open with, everywhere:** `infynitae-open-with FILE` picks an app for a file (the default
  marked) and can save it as the default in `mimeapps.list`, which GTK, KDE and `xdg-open` share;
  `xdg-open` says so when no app opens a type. KDE apps (Dolphin) get the menu file their "Open
  With" needs. File managers' "Open Terminal Here" runs `TERMINAL` (`xdg-terminal-exec`).
- **Tablet mode** (convertibles; `OSK`, `--no-osk`): folded, an on-screen keyboard
  (squeekboard) runs and shows for text fields; opened, it is stopped, so the keyboard types
  into apps directly (running, it is an input method, and on a ThinkPad a terminal then took
  text only from it). In a terminal its backspace does nothing (it deletes text the terminal
  does not report). The lock screen has its own key grid
  at the first tap, and while locked a finger reaches only the lock surface, so the password
  never leaves the lock screen. Tested off-screen and in the test suite; not yet on a
  touchscreen.
- **Out of GPU memory, windows stay open:** when the GPU resets or refuses the compositor's work
  (seen with a model training on it), the compositor makes a new renderer instead of being
  aborted by Mesa; the screens freeze until memory frees up. Tested: the robust context on this
  GPU; not yet: a real reset.
- **Hardware cursor** (`HARDWARE_CURSOR`): the cursor on the GPU's cursor plane, so it moves
  without a redraw and a fullscreen game with a cursor is still shown directly. Where the GPU
  refuses a cursor, it is drawn as before.
- **Screen sharing only on change:** a remote desktop or a screen share gets a frame when the
  screen changed since its last one, with just what changed; a still screen costs nothing.
- **Tearing for games** (`TEARING`, Super+Shift+T): a fullscreen window shown directly flips
  each frame at once (an async page flip) when its app asks (`tearing-control-v1`, a Vulkan or
  OpenGL game with vsync off) or after Super+Shift+T on it (X11 games, which cannot ask). A
  driver that refuses gets the usual flip. In the test suite; this GPU (amdgpu) reports support,
  not yet seen in a game.
- **Game mode** (`GAME_MODE`, `--no-gamemode`): a fullscreen window the GPU shows directly is
  a game, registered with Feral's gamemode (full CPU speed) until it leaves fullscreen. A
  fullscreen video shown directly counts too.
- **Apps out of sight yield the CPU** (`BACKGROUND_PRIORITY`): an app none of whose windows has
  been on a screen for 15 seconds runs at nice 10, with the processes it started, and returns at
  once when one is seen. A shared window, the game and X11 apps are never lowered. It needs the
  installer's limit (`/etc/security/limits.d/90-infynitae-nice.conf`) and a new login; before
  that nothing is lowered.
- **Battery limit** (`BATTERY_LIMIT`, control menu): the battery stops charging at a level
  (80%) on laptops whose firmware supports it (ThinkPads and many others); a udev rule lets the
  user set it. Tested against a stand-in battery; read on a ThinkPad S1 Yoga 12.
- **thermald** on Intel laptops (`--no-thermal`): Intel's thermal manager, running.
- **Default apps** (`--no-viewers` leaves the apps out): folders open in Thunar, videos and music
  in Celluloid, pictures in Ristretto, text in Mousepad, set for the file types with no default of
  yours in `~/.config/mimeapps.list` (install and update). Tested against a stand-in home; not yet
  on a fresh install.
- **Password store unlocked at login:** gnome-keyring starts before the start lock screen, and
  the lock screen's password opens it (`pam_gnome_keyring`), so apps no longer ask for a second
  password. Not yet tested on hardware.
- **Laptop lid:** closing it with another screen connected moves everything to that screen
  until the lid opens. With no other screen, logind suspends as usual.
- **Night light, screen power,** the capture dot, single-window sharing.
- **Recent display and lock fixes:** the lock confirmed only once it is on screen, idle
  screen-off staying off, per-screen scale for touch and tablets, region screencopy over dmabuf.
- **Crash reports:** a crash leaves a report in `~/.local/state/infynitae/crashes/` (the last
  10) and the next start says so in a notification. Tested with a fake compositor, not a real
  crash yet.
- **Remote desktop through the portal** (`REMOTE_DESKTOP`, off by default): apps like AnyDesk
  get the screen through xdg-desktop-portal-wlr and, once you allow it (this time, or always),
  the keyboard and mouse as real input: Super shortcuts work, the lock screen gets no keys from
  it. Display tools and VNC servers (wayvnc) can also drive the pointer
  (`zwlr_virtual_pointer_v1`). Tested end to end on a private bus against a headless compositor,
  not yet with a real app.
- **The session scripts and the C helpers** have the least test coverage of anything here.

## Missing

- **Pens and drawing tablets,** HDR, accessibility and screen readers.
- **Output management:** tools cannot yet turn a screen off through it (wlopm can).
- **Hardware:** a second GPU, NVIDIA's explicit sync.
- **Packaging** for anything other than Arch Linux. The install assumes one user and
  `~/infynitae`.

## Known limits

- `ext-session-lock-v1` lets any client take over a lock whose client died. The session stays
  locked until then, and the new lock client unlocks it. That is the protocol's design.
- With tty1 autologin and `START_LOCKED`, a broken lock screen ends the session instead of
  leaving you unlocked. That is safe, but it is not a login screen.
- The keyring asks for its password once per session when you log in without one (autologin).
  A display manager unlocks it only if its PAM setup includes `pam_gnome_keyring`.
- RustDesk without the polkit rule still connects, but its own "someone is connected" window
  cannot open.
