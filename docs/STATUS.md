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

Everyday basics a desktop like Windows has built in, with no default here yet (each would be a
lightweight default you can swap or turn off, like `TERMINAL`):

- a notification daemon (until then `notify-send` shows nothing unless you run one);
- a keyring, so the browser, Discord and editors stay logged in;
- pickers for Wi-Fi, Bluetooth and the sound output, per-app volume;
- USB drives that mount by themselves; battery and power modes; night light;
- display settings outside the config file; input methods (Japanese, Chinese…).

Already there: links open in your browser, one-click screen sharing, screenshots, clipboard
history, the lock screen, volume and brightness keys.

Also missing:

- The cursor theme for the compositor's own pointer (in progress), and `cursor-shape-v1`.
- HDR, tablet and pen support, touch gestures, accessibility and screen readers.
- Packaging and installs other than Arch Linux; the install assumes one user and `~/infynitae`.
- Crash reporting beyond the logs.

## Known limits

- `ext-session-lock-v1` lets any client take over a lock whose client died; the session stays
  locked until then, and the new lock client unlocks it. That is the protocol's design.
- With tty1 autologin and `START_LOCKED`, a broken lock screen ends the session instead of
  leaving you unlocked: safe, but it is not a login screen.
