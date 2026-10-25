# Status

Updated 2026-10-25.

Infynitae is my daily desktop. It is young and has only run on my own machine: expect rough
edges on other hardware.

## Solid

- **The spatial model**: the endless canvas, a camera per screen (pan, zoom, fit, overview),
  clusters, window slots, fullscreen you can pan out of. Core is a pure state machine with no
  Wayland code in it, tested exactly.
- **The test suite**: about 780 tests, most of them driving real Wayland clients against a
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
- **The session scripts and the C helpers** have the least test coverage of anything here.

## Missing

Everyday basics now in place, each a setting you can swap or turn off: notifications (mako),
a keyring (gnome-keyring, started on first use), USB drives that mount by themselves (udiskie),
the user folders (~/Downloads, ~/Pictures…), emoji and wider font coverage (Noto), and the
control menu on Super+A for Wi-Fi, Bluetooth, the sound output and power modes. Links open in
your browser, screen sharing is one click.

Still missing, because the compositor has to speak the protocol first:

- night light and colour temperature (`wlr-gamma-control`, for wlsunset or gammastep);
- display settings without editing the config (`wlr-output-management`, for wdisplays or kanshi);
- input methods for Japanese, Chinese, Korean (`input-method-v2` and `text-input-v3`, for fcitx5);
- per-app volume in the menu (the apps themselves do it; pavucontrol works).

The keyring asks for its password once per session when you log in without one (autologin);
logging in through a display manager with a password unlocks it only if that display manager's
PAM setup includes `pam_gnome_keyring`.

Also missing:

- `cursor-shape-v1` (apps naming a cursor shape for the compositor to draw); animated cursors
  show their first frame only.
- HDR, tablet and pen support, touch gestures, accessibility and screen readers.
- Packaging and installs other than Arch Linux; the install assumes one user and `~/infynitae`.
- Crash reporting beyond the logs.

## Known limits

- `ext-session-lock-v1` lets any client take over a lock whose client died; the session stays
  locked until then, and the new lock client unlocks it. That is the protocol's design.
- With tty1 autologin and `START_LOCKED`, a broken lock screen ends the session instead of
  leaving you unlocked: safe, but it is not a login screen.
