# Status

Updated 2026-10-25.

Infynitae is my daily desktop. It is young and has only run on my own machine: expect rough
edges on other hardware.

## Solid

- **The spatial model**: the endless canvas, a camera per screen (pan, zoom, fit, overview),
  clusters, window slots, fullscreen you can pan out of. Core is a pure state machine with no
  Wayland code in it, tested exactly.
- **The test suite**: about 830 tests, most of them driving real Wayland clients against a
  headless compositor, plus checks that seed bugs on purpose to prove the tests catch them
  (`tools/verify-test-efficacy`) and that the architecture holds (`tools/verify-architecture`).
- **The desktop around it**: lock screen, wallpaper, screen sharing (portal + screencopy),
  screenshots, clipboard history, session restore, recording and a replay buffer, polkit
  prompts, global shortcuts for apps, layer shell for bars, X11 apps through
  xwayland-satellite, VRR and direct scanout for games, idle screen-off and auto-lock.

## Untested or unproven

- **Other hardware.** The console backend (real screens, GPUs, input devices) cannot run in the
  test suite; it has only run on my machine. Multi-GPU has never been tried.
- **Recent display and lock fixes on a real screen**: the lock being confirmed only once it is
  on screen, idle screen-off staying off, per-screen scale for touch and tablets, region
  screencopy over dmabuf (2026-10-25).
- **Monitor hotplug** (2026-10-25): a screen plugged in, unplugged or woken is found about
  1.5 s after the last change, and the session can start with no screen and wait for one (a
  monitor off at boot). A screen asleep (idle screen-off, or turned off by a tool) is never
  removed while asleep, only 3 s after it wakes if it is really gone. Not yet tried by
  plugging a real monitor.
- **The session scripts and the C helpers** have the least test coverage of anything here.

## Missing

Everyday basics now in place, each a setting you can swap or turn off: notifications (mako),
a keyring (gnome-keyring, started on first use), USB drives that mount by themselves (udiskie),
the user folders (~/Downloads, ~/Pictures…), emoji and wider font coverage (Noto), and the
control menu on Super+A for Wi-Fi, Bluetooth, the sound output and power modes. Links open in
your browser, screen sharing is one click.

The compositor now also speaks the protocols these tools need: night light (wlsunset,
gammastep), screens turned off and on by a tool (wlopm), external idle tools (swayidle,
hypridle), task bars and window switchers (waybar's taskbar, rofi's window mode: both window-list
protocols), apps that take the shortcuts while focused (VMs, remote desktops), apps naming
their cursor shape, and input methods for Japanese, Chinese or Korean (fcitx5, ibus: install one
and start it from `autostart.sh`, e.g. `fcitx5 -d`). An input method gets nothing while the
screen is locked. Night light and screen power have only run in the test suite, not on
a real screen yet.

While any app captures the screen or a window (screen sharing, a remote desktop such as
RustDesk, a screenshot), a small red dot shows in the top right corner of every screen, and for 3 s
after the last frame; it is never in what is captured. The replay buffer does not show it
(`CAPTURE_INDICATOR`). Only tested in the test suite so far.

Screen sharing can share one window instead of a screen: an app asking for a window (Discord's
window tab) gets a menu of the windows, an app asking for a screen still gets the first one at
once; with `SCREENSHARE_ASK="true"` every share asks, in a menu of the screens and the windows. A shared window is drawn by itself, wherever it is on the
canvas; a private window (`PRIVACY_APPS`, `PRIVACY_TITLES`) is never shared, and nothing is
while the screen is locked. Only tested in the test suite so far.

RustDesk works as a host (seen on this machine, 2026-10-25): the picture through the
screen-sharing portal, the keyboard and mouse through the virtual devices its root service
makes. Its service decides X11 or Wayland once per login, while the session is still a text
console, so the session restarts it once the compositor runs; that needs the polkit rule in
`live/remote/` installed once (root). Without it the connections still work, but RustDesk's own
"someone is connected" window cannot open; the capture indicator shows them either way.

Display settings tools (wlr-randr, wdisplays, kanshi) can change a screen's mode, scale,
position and rotation while you work (`wlr-output-management`), and `OUTPUT_<NAME>_TRANSFORM`
turns a screen from the start. A turned screen is captured the way wlroots does it (in the
panel's orientation, which grim and the screen-sharing portal turn back), and a touchscreen's
touches turn with it. They cannot yet turn a screen off or switch VRR; mode changes and rotation
have only run in the test suite, not on a real screen.

Still missing, because the compositor has to speak the protocol first:

- remote desktop apps that need the `RemoteDesktop` portal for their input (AnyDesk, GNOME's
  clients);
- per-app volume in the menu (the apps themselves do it; pavucontrol works).

The keyring asks for its password once per session when you log in without one (autologin);
logging in through a display manager with a password unlocks it only if that display manager's
PAM setup includes `pam_gnome_keyring`.

Also missing:

- HDR, tablet and pen support, touch gestures, accessibility and screen readers.
- Packaging and installs other than Arch Linux; the install assumes one user and `~/infynitae`.
- Crash reporting beyond the logs.

Apps keep the screens on and the session unlocked while they play video or a game: Wayland
apps through the idle-inhibit protocol (while their window is open), others over D-Bus
(`org.freedesktop.ScreenSaver`, answered by infynitae-portal with a logind idle inhibitor that
ends with the app). `systemd-inhibit --what=idle <command>` does the same by hand; Super+Shift+I
stays as the switch. Locking still turns the screens off on time, whatever an app asks.

Closing a laptop's lid with another screen connected leaves the laptop's panel out (windows and
the pointer stay on the other screen) until the lid opens; with no other screen, logind suspends
the machine as it does by default. Not yet tried on a laptop.

Not handled yet on laptops and other hardware: touch as real touch (it acts as a mouse), pens, a
second GPU, and NVIDIA's explicit sync.

## Known limits

- `ext-session-lock-v1` lets any client take over a lock whose client died; the session stays
  locked until then, and the new lock client unlocks it. That is the protocol's design.
- With tty1 autologin and `START_LOCKED`, a broken lock screen ends the session instead of
  leaving you unlocked: safe, but it is not a login screen.
