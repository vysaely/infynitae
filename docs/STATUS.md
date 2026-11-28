# Status

Updated 2026-11-28 (edge: docs/EDGE_APPS.md, settings: docs/EDGE_SETTINGS.md, window rules: docs/DECISIONS.md).

Infynitae is my daily desktop. It is young and has only run on my own machine, so expect rough
edges on other hardware.

## Solid

- **The spatial model:** the endless canvas, a camera per screen (pan, zoom, fit, overview),
  clusters, window slots, fullscreen you can pan out of, Super+Tab as Windows' Alt+Tab with
  the camera going to each window in place of thumbnails. The core is a pure state machine with
  no Wayland code in it, tested exactly.
- **The test suite:** 1,398 `#[test]` in `crates/` (plus 489 in `live/edge-apps` and 37 in `live/shot-rs`, counted 2026-11-28), most of them real Wayland clients against a headless
  compositor; the test harness isolates the launcher in a temporary directory so shortcuts and reload actions never reach the live desktop. `tools/verify-test-efficacy` seeds bugs on purpose to prove the tests catch them (sharing build paths so cargo rebuilds only what a seed touched),
  `tools/verify-architecture` checks that the architecture holds, and CI tests config defaults and README shortcuts against the code.
- **The desktop:** lock screen, wallpaper, screenshots, clipboard history (kept in memory by the compositor), session restore,
  recording and a replay buffer (defaulting to hevc), polkit prompts, notifications (the edge by default, else mako, swaync, dunst, or fnott), a keyring
  (gnome-keyring, or infynitae-keyring once switched via `infynitae-keyring`'s `tools/switch.sh`), USB automount (infynitae-automount on udisks2: drives named by Roman numeral like drive letters, mounted at /mnt/V, `DRIVE_NAMES`, docs/DRIVES.md; encrypted drives ask for their passphrase, a notification opens or ejects a drive, `infynitae-automount eject` makes every drive safe to pull; right click a drive's folder in PCManFM-Qt, "Format drive...", to erase it as exFAT, FAT32 or ext4, `infynitae-format-drive` from a terminal), the control menu (Super+A: Wi-Fi, Bluetooth, volume
  for all sound or one app, sound output, power mode), global shortcuts for
  apps, layer shell for bars, X11 apps through xwayland-satellite (can be disabled with `XWAYLAND=false`), VRR and direct scanout for
  games, idle screen-off and auto-lock. Shipped defaults match `config/config`: `FOCUS_FOLLOWS_MOUSE="strict"`, `FOCUS_ON_OPEN="user"`, `PAN_MODE="free"`, `CARD_OPACITY="100"`, `TEARING="true"`, `THEME_FROM_WALLPAPER="accent"`, `SESSION_RESTORE="restart"`, `EDGE_POSITION="bottom"`, `ICON_THEME="auto"`, `TOUCHPAD_TAP="true"`, `TOUCHPAD_GESTURES="true"`, and `SQUARE_CORNERS` unset (defaults to true).
- **Screen sharing:** a screen or a single window, through the portal or screencopy (screencopy hides the cursor by default). The
  portal asks before an app's first share (this time, always, or deny; `SCREENSHARE_ASK`, defaults to `app`). A
  private window (`PRIVACY_APPS`, which accepts spaces or commas, `PRIVACY_TITLES` separated by commas, Super+Shift+P) is never shared, and nothing
  is while the screen is locked. While anything captures, the edge's background apps module says
  "Screen capture" and which app (`CAPTURE_INDICATOR="edge"`, the default since 2026-11-12: nothing is
  drawn over the screen, so a game keeps going to it directly); `"dot"` shows a small red dot instead.
- **App permissions** (2026-10-31, [SECURITY.md](SECURITY.md)): sandboxed apps (Flatpak, through
  security-context-v1) get no capture, clipboard management, virtual input, input method,
  window list, display control, layer-shell or lock unless their app id is listed in
  `ALLOW_*`; desktop apps are known by their real program file and allowed by default, except
  the lock.
- **RustDesk as a host** (seen working on this machine): the picture through the portal, input
  through its own virtual devices. The session restarts RustDesk's service once the compositor
  runs, which needs the polkit rule in `live/remote/` (the installer adds it).
- **Login screen** (`sudo infynitae-login-screen on`, 2026-11-26): greetd starts Infynitae as its
  greeter account with the lock screen in greeter mode: the last user filled in (Tab: another
  user), the password checked by greetd, which opens the session and starts the desktop as that
  user; the keyring opens with the same password, and START_LOCKED does not ask again. A crashed
  login screen starts again; `off` brings the text login back, and tty2 keeps one either way.
  Tried in the test VM (boot, wrong and right password, log out and in, a killed login screen),
  not yet on the owner's PC.
- **Staying awake:** apps that play video or a game keep the screens on, both Wayland apps
  (idle-inhibit) and others over D-Bus (`org.freedesktop.ScreenSaver`). Locking still turns
  the screens off on time.
- **Protocols for the usual tools:** display settings (wlr-randr, wdisplays, kanshi), night
  light (wlsunset, gammastep), screen power (wlopm), idle tools (swayidle, hypridle), task bars
  and window switchers (waybar, rofi), apps taking the shortcuts while focused (VMs, remote
  desktops), cursor shapes, input methods (fcitx5, ibus: start one from `autostart.sh`).

## Not yet tried on real hardware

These pass the test suite but have not run on a real screen or device yet:

- **The edge from apps is the only edge (2026-11-24):** `live/edge-apps`
  ([EDGE_APPS.md](EDGE_APPS.md)), the one the owner runs. The C edge and the Rust edge were
  deleted; what they did that this one does not is listed in EDGE_APPS.md section 11. It draws
  the desktop's widgets (`WIDGETS`); they are changed in the config, not arranged in place. The
  widgets are tested against a mock compositor only, not yet on a screen.
- **The 2026-11-06 overhaul** (tried as a fresh default user in the test VM, not yet on the
  owner's screen):
  - **Lock screen:** the wallpaper blurred and dimmed behind it, the user's picture
    (AccountsService, `~/.face`) or up to two initials, the full name, a larger clock; the
    wallpaper helper prepares the pictures (`~/.cache/infynitae`), the lock screen reads them only
    once the lock holds. The lock screen wipes password buffers and memory-locks them against swap,
    drops core dumps and ptrace, detaches PAM workers, and refuses `--greeter` as root;
    `infynitae-sleep-lock` keeps the sleep inhibitor if an external locker exits within the first second.
  - **Panels of modules (2026-11-07):** each panel is one config line, its key, its place on
    any screen edge and its modules in order (`PANEL_<NAME>`); new panels of your own; a
    favorites module (`FAVORITES` in the config); arrows move to the nearest item
    in every panel; panels unfold from their anchored screen edge (170 ms open, 105 ms close;
    inline lists 175 ms); panels opened by key stay open when the pointer
    leaves; overlapping cards route clicks and drags to the topmost card; ask cards close when
    their asking program ends, and later requests queue.
  - **Panels (Super+W, Super+L, Super+A, Super+V):** one look (sans-serif 12-15 px, 32 px rows,
    hairline outlines, the accent only for what is selected or on); the launcher, window list and
    clipboard are one-column lists with every item reachable (Up/Down, Page Up/Down); apps search
    ranks names starting with the query before matches inside, ordered by recent use within each
    rank; shrinking panels clear their previous bounds without leftover pixels; dragging sliders
    coalesces commands, and media seek commits on release; calendar jumps to today on title click;
    invoking a notification action keeps older unread notifications; the memory card protects the
    compositor and session processes; touch-and-hold emits a right click; the control panel
    spells everything out.
  - **Window menu and Keep on top (2026-11-16, Super+M 2026-11-24):** `xdg_toplevel.show_window_menu` (right click on a
    client title bar) and `Super+M` (`BIND_WINDOW_MENU`) open a menu in the edge
    with Keep on top (check mark when on), Minimize, Maximize or Restore, Fullscreen and Close;
    `KEEP_ON_TOP` is editable on the Windows settings card. Tested in `infynitae-core`,
    `infynitae-ipc`, `infynitae-backend-headless` and `infynitae-edge-apps`; not yet tried in the
    live desktop session on real hardware.
  - **Shorter shortcut dispatch** (24 ms of shell setup a press down to 8 ms), the wallpaper no longer
    keeps its decoded image (about 70 MB with a large wallpaper); edge system and memory modules
    read GPU telemetry, temperatures, and memory directly from `/proc` and `/sys` without subshells.

- **Other machines.** The console backend (real screens, GPUs, input) cannot run in the test
  suite and has only run on my machine. Multi-GPU has never been tried.
- **Monitor hotplug:** a screen plugged in, unplugged or woken is found about 1.5 s after the
  last change, and the session can start with no screen and wait for one. A sleeping screen is
  never removed while asleep.
- **Rotation, mode and VRR changes** from display tools or `OUTPUT_<NAME>_*` (`OUTPUT_<NAME>_TRANSFORM`, `OUTPUT_<NAME>_MODE`, `OUTPUT_<NAME>_SCALE`, `OUTPUT_<NAME>_RATE`; checked by `config-check`). When `OUTPUT_<NAME>_TRANSFORM` is unset, `backend-drm` reads the connector's DRM `"panel orientation"` property (for sideways handheld and tablet panels such as 800x1280). `OUTPUT_ORDER` accepts spaces or commas and overrides `OUTPUT_LAYOUT` with a warning. Captures
  and touch turn with the screen (unit-tested for `panel orientation` mapping and tested in the headless suite for touch on a rotated 800x1280 output; reading the DRM property from a real panel is not yet tested on hardware). VRR as tools see it is whether a screen may use it (on, or
  auto while fullscreen); turning it off from a tool lasts until the screen reconnects.
- **Auto-rotate** on convertibles (`AUTO_ROTATE`, needs iio-sensor-proxy): the laptop's own
  screen turns with the device.
- **Touchscreens** as real touch (`wl_touch`): apps get each finger, so scrolling, pinching and
  gestures work inside them. Each touch device maps to its matching output (by `WL_OUTPUT`,
  physical size in mm, internal panel, pointer output, or first screen), tracks active slots
  per output, and turns with the output's rotation and scale. A tap focuses the window under
  it. A finger on empty canvas drags the canvas; two fingers pinch-zoom it around their
  midpoint; three fingers on a window move or resize it. Swiping in from a screen edge opens
  that side's edge panel (`TOUCH_EDGE_SWIPE`, on by default). Active touches cancel cleanly when
  a window unmaps or closes, an exclusive panel opens or closes, fullscreen starts, the output
  rotates or unplugs, a touchscreen disconnects, the session locks or pauses, or `TouchCancel`
  arrives; `TouchFrame` events are delivered. Tested in `infynitae-backend-drm` unit tests and
  headless Wayland client tests (`t1_touch`, `t1_lock_touch`, `audit_input_lifecycle`); not yet
  tested on real touchscreen hardware.
- **Touchpad gestures** (`zwp_pointer_gestures_v1`, `TOUCHPAD_GESTURES`, true by default):
  three-finger swipe pans the canvas 1:1 with the fingers, four-finger pinch zooms around the
  cursor, and four-finger swipe up toggles the overview; two-finger gestures, holds, and all
  gestures when `TOUCHPAD_GESTURES=false` go to the hovered app. Tested in core unit tests and
  headless Wayland client tests (`t1_touchpad_gestures`); not yet tested on real touchpad hardware.
- **Pens and drawing tablets** (`zwp_tablet_manager_v2`): drawing apps that bind tablet-v2
  receive proximity, tip, motion, pressure, tilt, distance, rotation, slider, wheel, stylus
  buttons and pad buttons in surface-local coordinates through the output's camera, and a tap
  focuses and raises the window under the pen. Apps without tablet-v2 receive pointer motion and
  left clicks instead, with no double input to tablet-v2 apps. Tablets match their screen using
  the same rule as touchscreens (`WL_OUTPUT`, physical size in mm within 15 mm, internal panel,
  pointer output, or first screen, falling back to the virtual screen when no physical screen is
  connected). Tested in unit tests (`infynitae-core`, `infynitae-backend-drm`) and with real
  Wayland probe clients in `t1_tablet` (with panned and zoomed camera, pad buttons and session
  lock); not yet tested on real tablet hardware or in a live DRM session.
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
- **Tablet mode** (convertibles; `OSK`, `--no-osk`): squeekboard runs hidden from the session's
  start. When the device has a `TabletMode` switch, `OSK="auto"` shows squeekboard on text-field
  taps only in tablet mode and hides it when leaving tablet mode; on devices without a
  `TabletMode` switch, text-field taps show it (`OSK="always"` adds mouse clicks; `OSK="off"`
  is manual only). Running, it is an input method, and on a ThinkPad a terminal then took text
  only from it; in a terminal its backspace does nothing (it deletes text the terminal does not
  report). The lock screen has its own key grid at the first tap, and while locked a finger
  reaches only the lock surface, so the password never leaves the lock screen. Tested in the
  headless test suite (`t1_input_method`, `t1_lock_touch`); not yet tested on real convertible
  or touchscreen hardware.
- **Out of GPU memory, windows stay open:** when the GPU resets or refuses the compositor's work
  (seen with a model training on it), the compositor makes a new renderer instead of being
  aborted by Mesa; the screens freeze until memory frees up. Tested: context loss handling on this
  GPU; not yet: a real reset.
- **Hardware cursor** (`HARDWARE_CURSOR`): the cursor on the GPU's cursor plane, so it moves
  without a redraw and a fullscreen game with a cursor is still shown directly. Where the GPU
  refuses a cursor, it is drawn as before.
- **Screen sharing only on change:** a remote desktop or a screen share gets a frame when the
  screen changed since its last one, sending only changed areas; a still screen costs nothing.
- **Tearing for games** (`TEARING`, Super+Shift+O; Super+Shift+T too): a fullscreen window
  shown directly flips each frame at once (an atomic async page flip writing only the primary
  plane's `FB_ID`) when `TEARING="always"`, when its app asks (`tearing-control-v1`; `TEARING="true"`,
  the default; hints apply on the next surface commit and destroying the object restores vsync on the next
  commit), or after Super+Shift+O on it (X11 games, which cannot ask). Visible unlocked cursors
  (menus) and active overlay planes stay on vblank cleanly; pointer-locked tearing fullscreen
  windows suppress the cursor so the cursor plane stays free, transition frames sync once before
  async flips start, and driver or plane refusals log the plane and property once per game. In the
  unit and headless test suite (policy, cursor suppression, and pure async commit builder); `amdgpu`
  supports async flips here, not yet re-tested in a live game after the atomic-commit fix.
- **Game mode** (`GAME_MODE`, `--no-gamemode`): a fullscreen window, or one filling the screen
  that tears (with `TEARING="always"` any, a maximised browser too), shown directly or not, is
  registered with Feral's gamemode (full CPU speed) until it has been off screen for 5 s
  continuously. While a fullscreen window or one that plays like a game (pointer held, asked to
  tear, game content type, an X11 window) is on, notifications wait (`GAME_QUIET`): one drawn
  over a game ends its direct path to the screen and tearing.
- **Real-time audio and desktop** (`--no-realtime`): real-time scheduling rights (`rtkit`, `realtime` group from `realtime-privileges`).
  With them the compositor's main thread runs at real-time priority 2, else at nice -10 where the
  limit allows; with reset-on-fork either way, so the apps it opens start at normal priority
  (before 2026-11-22 they inherited real-time 2).
- **Apps first when memory runs out:** what the session and the shortcuts open gets an
  out-of-memory score of 300; the compositor, the session script until the login lock, and the
  lock screen keep the score they started with (lowering it needs root), so the kernel ends an
  app first.
- **ntsync**: NT synchronisation for Wine and Proton loaded at boot (`/etc/modules-load.d/ntsync.conf`) when the kernel has the module.
- **No background memory compaction**: `vm.compaction_proactiveness = 0` (`/etc/sysctl.d/99-infynitae-memory.conf`) prevents latency spikes under load.
- **A full GPU slows the windows out of use** (`GPU_RELIEF`): while a game fills a screen, a
  screen missing one refresh in twenty for a second completes the frames of windows that are
  neither focused nor filling it at about 60 FPS for 30 s, longer if it had to start again at
  once. With no game filling the screen nothing in view is slowed (2026-11-06: picture-in-picture
  beside a video was held at 30 FPS). A game can only be slowed
  this way if it waits for its frame callbacks.
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
- **Password store unlocked at login:** gnome-keyring (or infynitae-keyring once enabled with `infynitae-keyring`'s `tools/switch.sh`) starts before the start lock screen, and
  the lock screen's password opens it (`pam_gnome_keyring`), so apps no longer ask for a second
  password. Not yet tested on hardware.
- **Laptop lid and handheld power key:** closing the lid with another screen connected moves everything to that screen
  until the lid opens. With no other screen, logind suspends as usual. On convertibles and touchscreen handhelds with a battery, the installer adds `50-infynitae-power-key.conf` so a short power-button press suspends and a long press shuts down (tested in `tools/test-install-lib`, not yet on handheld hardware).
- **Night light, screen power,** the capture dot, single-window sharing.
- **Recent display and lock fixes:** the lock confirmed only once it is on screen, idle
  screen-off staying off, per-screen scale for touch and tablets, region screencopy over dmabuf.
- **Crash reports:** a crash leaves a report in `~/.local/state/infynitae/crashes/` (the last
  10) and the next start says so in a notification. Tested with a fake compositor, not a real
  crash yet.
- **Remote desktop through the portal** (`REMOTE_DESKTOP`, off by default): apps like AnyDesk
  get the screen through Infynitae's portal and, once you allow it (this time, or always),
  the keyboard and mouse as real input: Super shortcuts work, the lock screen gets no keys from
  it. Display tools and VNC servers (wayvnc) can also drive the pointer
  (`zwlr_virtual_pointer_v1`). Tested end to end on a private bus against a headless compositor,
  not yet with a real app.
- **Screen sharing and app screenshots without xdg-desktop-portal-wlr** (2026-11-28):
  `infynitae-portal` streams a screen or a window to PipeWire itself (GPU buffers when the app
  takes them) and takes an app's screenshot with `infynitae-shot`; wlr is no longer started.
  Tested with `tools/test-portal-screencast` (D-Bus checks) and `tools/vm-edge/pscreencast.py` (VM); not yet tried with Discord, OBS or Firefox on real hardware.
- **Compositor clipboard and history** (2026-11-28): the compositor keeps copies after an app closes (`CLIPBOARD_PERSIST`, up to 32 MiB) and an in-memory history (`CLIPBOARD_HISTORY`, at most 300 entries and 64 MiB, cleared at logout; password-manager hints and private windows stay out of it), without `wl-clip-persist` or `cliphist`; `Super+V` searches, pins and copies back through `infynitae msg clipboard`.
- **Runs from its install checkout** (2026-11-28): `install.sh` and `infynitae-update` record the folder in `~/.config/infynitae/checkout`; scripts, the compositor, the edge and the portal run from it instead of assuming `~/infynitae` (`tools/test-checkout`).
- **Edge unfold animation, memory release and idle wakeups** (2026-11-26..28): panels unfold from their anchored screen edge (170 ms open, 105 ms close; 175 ms inline lists), return cleared buffer pages with `madvise(MADV_REMOVE)` when closed, and wake 0 times per second while closed (down from 2/s; `live/edge-apps/tests/idle_wakeups.rs`).
- **Undoable updates and helper privilege dropping** (2026-11-27): `infynitae-update` saves `<name>.previous` copies in `~/.local/bin`, `infynitae-update --rollback` swaps them back without `sudo`, `infynitae-session` falls back to `infynitae.previous` after two crashes within 5 s at start, non-escalating helpers drop `NO_NEW_PRIVS`, and input parsers have `cargo-fuzz` targets in `fuzz/` ([RECOVERY.md](RECOVERY.md), [SECURITY.md](SECURITY.md)).
- **Launcher search across desktop items** (2026-11-27): typing in `Super+W` also lists matching open windows, settings rows, clipboard entries, replay clips, recordings, recent files and files by name via `plocate` (4 rows per kind; `tools/vm-edge/psearch.py`).
- **Screenshot markup** (2026-11-26): after a `Super+S` region or window pick, `infynitae-shot` opens an editor with arrow, box, pen, text, pixelate, crop and undo (`Enter` copies, `Ctrl+S` saves and copies, `Escape` keeps unmarked, `Shift` skips the editor; `tools/vm-edge/pshot.py`).
- **Shortcut help overlay** (2026-11-26): `Super+/` (and `Super+F1`) opens a searchable overlay of bound shortcuts with live `BIND_*` and `EXEC_*` overrides from the config (`tools/vm-edge/pshortcuts.py`).
- **Volume and brightness pop-up** (2026-11-26): the edge draws a 240x56 bottom-centre on-screen display (`infynitae-osd`, 1.2 s auto-hide, excluded from notification history) for volume, mute and brightness keys (`tools/vm-edge/posd.py`).
- **Waiting update counts** (2026-11-26): `infynitae-update-check` (user timer every 6 hours, `UPDATE_CHECK`, default true) counts waiting Arch packages (`checkupdates`) and Infynitae commits, notifies once, and shows counts on Settings -> This PC (`tools/test-update-check`).
- **One system-bus watcher** (2026-11-26): `infynitae-bus-watch` watches both UDisks2 automount and logind sleep-lock signals in one C process instead of separate shell and `gdbus` processes (`tools/test-automount`, `tools/test-sleep-lock`).
- **Settings clean-up** (2026-11-25): expert workarounds and replacement commands live in the `Advanced` section and `settings/advanced` card, `ICON_THEME` defaults to `auto` (Papirus when installed, else Adwaita), and retired keys (`EDGE_IMPL`, `CLIPBOARD_PICKER`, `CONTROL_MENU`, `PANELS_SNAP`, `PANELS_OVERLAP`) are ignored without warnings.
- **Desktop journal** (2026-11-25): security-relevant events from the compositor and helpers log locally to `~/.local/state/infynitae/journal/journal.tsv` (capped at 2 MiB), read with `infynitae msg journal` ([SECURITY.md](SECURITY.md)).
- **Security upkeep** (2026-11-28): CI runs `cargo-audit` against RustSec advisories on every push and nightly across all four shipped `Cargo.lock` files and `xwayland-satellite`; Dependabot opens weekly PRs; `tools/upstream-check` checks newer Smithay and `xwayland-satellite` releases.
- **Replay buffer on disk or another drive** (2026-11-22..24): `REPLAY_STORAGE="disk"` keeps `gpu-screen-recorder`'s replay buffer on disk instead of RAM, and `REPLAY_DISK_DIR` places it on a chosen drive while saving clips into `CLIPS_DIR` (`tools/test-recorder-backend`).
- **Drag-and-drop and frame-pacing protocols** (2026-11-24..26): Wayland drag-and-drop follows Core's pointer routing with drag icons; `wp_fifo_v1`, `wp_commit_timing_v1` and `linux-drm-syncobj-v1` explicit sync are offered (explicit sync not yet tried on NVIDIA hardware); shared-memory screencopy copies into client buffers on a worker thread.
- **Window control and cluster sorting** (2026-11-22..24): `Super+Shift+Delete` force-closes Wayland and X11 apps (via `infynitae-xres-pid`) even while shortcuts are inhibited; unresponsive windows asked to close prompt after 5 s; `Super+Shift+G` sorts a cluster by app (`CLUSTER_SORT` automates tidy/app placement, and spiral/tiles/line bind via `BIND_CLUSTER_LAYOUT`).
- **Windows and Android programs** (2026-11-20): `.exe`/`.msi` open via `infynitae-windows-run` (umu-run and Proton inside bubblewrap) and `.apk` via `infynitae-android-run` (Waydroid), both sandboxed through `infynitae-sandbox-socket` (`security-context-v1`, [SECURITY.md](SECURITY.md); `tools/test-windows-run`, `tools/test-android-run`).
- **The session scripts and the C helpers** have the least test coverage of anything here.

## Root drive and memory guard

On a btrfs root (`--no-disk-guard` skips it), the installer sets up dynamic and periodic data
chunk reclaim (`/etc/tmpfiles.d/btrfs-reclaim.conf`), a weekly light data balance
(`btrfs balance start -dusage=30 -dlimit=10 /`, never `-musage`), monthly `btrfs-scrub@-.timer`,
and a 4 GiB emergency reserve below the partition when at least 20 GiB is unallocated. It adds no
`systemd-oomd` rules: a swap-kill rule on `-.slice` closed the whole desktop session (compositor
and every app) on the owner PC on 2026-11-21, because the session scope was the largest swap user.

The user timer `infynitae-disk-guard.timer` runs `live/infynitae-disk-guard` every 60 s (~4 ms
per tick on this PC). When root unallocated space drops below twice the data chunk size and
metadata free space drops below 256 MiB, it runs `/usr/local/lib/infynitae-disk-fix` via `pkexec`
(passwordless for local active sessions in `/etc/polkit-1/rules.d/50-infynitae-disk-fix.rules`)
to release the reserve, run the light balance, and shrink the reserve back when more than 20 GiB
remains unallocated, posting one notification. When `systemd-oomd` kills a cgroup since the last
tick, it posts "Closed <app> - RAM was full". Tested against sample sysfs trees, fake journals,
and stubbed installer/btrfs runs (`tools/test-disk-guard`, `tools/test-disk-fix`,
`tools/test-install-lib`); not yet run in a real outage. See [RECOVERY.md](RECOVERY.md) for
update rollback (`infynitae-update --rollback`, `snap-pac` compatibility, and Limine recovery).

## Settings that needed a restart (2026-11-21)

Every key that was tagged `[restart]` in `config/config`, or sat in its "Displays & sharpness"
section, and what it needs now. `[save]` is the console backend's check of the file every 250 ms;
`[reload]` is Super+Shift+Backspace (`infynitae-reload`).

| Key | Was | Now | How, or why not |
|---|---|---|---|
| `XKB_DEFAULT_LAYOUT`, `XKB_DEFAULT_OPTIONS` | restart | save | The seat's keymap is built again and sent to every app; apps and actions started later get the new names. An emptied line goes back to the layout the compositor started with, which is the system's only when the config had none at start. |
| `OUTPUT_SCALE`, `OUTPUT_<NAME>_SCALE` | restart | save | Each screen takes its new scale and the screens are placed side by side again, the way a display tool's change is carried out. A position set with `wlr-randr` is kept until a scale, the order or the layout changes. |
| `OUTPUT_ORDER`, `OUTPUT_LAYOUT` | restart | save | The screens are placed again in the new order. Names are matched on the kernel's name (`DP-1`), not the second form the start also accepts. An emptied `OUTPUT_ORDER` keeps the order there is; a changed `OUTPUT_LAYOUT` turns it round. |
| `VRR_ENABLED` | restart | save | Every screen takes the new mode, display tools are told. Console backend only; no test runs it (it needs a real screen). |
| `VIRTUAL_SCREEN` | restart | save | The virtual screen is added or removed at once. Console backend only; no test runs it. |
| `SCALING_FILTER`, `MAX_APP_SCALE`, `ZOOM_SHARPEN` | save | save | Already read on save; their lines said so inside a `[restart]` section. |
| `OSK` | restart (the section) | reload | The compositor already read it on save. A removed line now goes back to `auto`. squeekboard, without which a tap opens nothing, was started only at login; a reload starts it when `OSK` is not off. |
| `START_TERMINAL`, `START_LOCKED` | restart | at login | They only do something when a session starts. |
| `BATTERY_LIMIT` | restart (the section) | reload | `infynitae-reload battery-limit` sets the file's limit when it is not the one in force; `off` lets the battery charge to full. The settings card and the control menu apply it at once; at login `session.sh` sets a percentage and leaves `off` alone. Tested against a stand-in battery folder (`tools/test-reload-match`). |
| `AUTO_ROTATE` | restart (the section) | reload | `infynitae-reload autorotate` stops the `autorotate` helper and starts it again when `AUTO_ROTATE` is not off, so `AUTO_ROTATE_INVERT` and `_OUTPUT` are read again too. Not run on a convertible. |
| `OUTPUT_<NAME>_MODE`, `_RATE`, `_TRANSFORM` | restart | save | A changed `_MODE` or `_RATE` line gives the screen the mode they pick at start (a size it lacks, or a removed line: its preferred mode; `_RATE` alone with no `_MODE` line: that rate at the size it has). A changed `_TRANSFORM` line turns it; a removed one leaves it turned until the next start (the turn it started with may be the panel's own). The screens are placed side by side again. Tested headless (`t1_live_displays`); console backend not run on hardware. |
| `XWAYLAND` | restart | restart | The X display and its cookie are reserved at start and every app gets them as `DISPLAY`. Turning it on needs a display no running app knows; off needs the backends to stop the on-demand start (a listening socket in their event loop) and to wait for the last X11 window. Not safe to change here. |
| `REMOTE_DESKTOP` | restart | reload | It picks the portal backends: `live/env.sh` writes `infynitae-portals.conf` and `infynitae.portal`, now only when their content changes. xdg-desktop-portal reads them only when it starts, so `infynitae-reload portal` restarts it when one is newer than its start. That restart ends every app's portal sessions (screen shares, file dialogs), so it happens only after a change. Before, the files were rewritten on every Super+key and the running portal kept the old routing until it next started. |

## Missing

- HDR, accessibility and screen readers.
- **Output management:** tools cannot yet turn a screen off through it (wlopm can).
- **Hardware:** a second GPU.
- **Packaging** for anything other than Arch Linux. The install assumes one user.

## Known limits

- `ext-session-lock-v1` lets any client take over a lock whose client died. The session stays
  locked until then, and the new lock client unlocks it. That is the protocol's design.
- With tty1 autologin and `START_LOCKED`, a broken lock screen ends the session instead of
  leaving you unlocked. That is safe, but it is not a login screen.
- The keyring asks for its password once per session when you log in without one (autologin).
  A display manager unlocks it only if its PAM setup includes `pam_gnome_keyring`.
- RustDesk without the polkit rule still connects, but its own "someone is connected" window
  cannot open.
