# The edge in Rust (2026-11-09)

The owner asked for the edge (`live/edge/infynitae-edge.c`, about 6,100 lines of C) to be rewritten in Rust
with the new module design, the old one removed after, keeping what makes it ours: panels of modules
(`PANEL_` lines), every module in any panel, sizes and places of their own, the panel as a small canvas
edited in place; and the same modules as widgets on the desktop.

Design: the canvas "Infynitae widget modules" (three boards: the desktop with a panel, the music module's
three sizes, every other module). One style for every module: a title in 15 px, content as text with small
line icons, thin bars, the accent only on what is on or active, no filled blocks. Corners 0 to 28 px and the
colour scheme (yours from the system, monochrome, neutral, tonal, vibrant, expressive) are settings.

## Rules

- Same command line and signals as the C edge (`panel NAME`, `control`, `apps`, `clipboard`, `toggle`,
  `stop`, `ask`, `--render`, `--print-panels`; SIGRTMIN..SIGRTMIN+3), the same `$XDG_RUNTIME_DIR` files,
  the same `PANEL_` lines and `FAVORITES`: the compositor, `live/launch.sh` and every script keep working
  the day it replaces the C one. Until then both are built and `EDGE_IMPL=rust` picks the new one.
- Its own crate in `live/edge-rs` (own lockfile; outside the compositor workspace and its dependency
  policy, like the C helpers). Drawing with cairo and pango (the same libraries and font rendering as now);
  Wayland through smithay-client-toolkit; D-Bus (tray, media) through zbus.
- Zero cost when closed: no timers, no redraws; a module that shows nothing hides itself.
- Every module drawn offline by `--render` for tests (no display), as `tools/test-edge-render` does now.
- No builds on the owner's PC while a model trains: CI and the test VM.

## Stages

| Stage | What | Done when |
|---|---|---|
| 0 (done) | Skeleton: config (`PANEL_` lines, theme colours, scheme, corners), the module trait with sizes, the card style, `--render` and `--print-panels`; clock and calendar | renders match the canvas; CI builds and tests it |
| 1 (done) | Wayland: layer-shell surfaces per output, double buffers and frame callbacks, pointer, keyboard, touch, signals and the request files, hover edges | a panel opens by key and by hover in the VM |
| 2 (2a done) | Modules on commands: switches (wifi, bluetooth, dnd, nightlight, keyboard, osk, power, replay), sound, session (done), search (done), favorites (done), apps (done), windows (foreign toplevel) (done), memory (done), system (done) | each does what the C one does, VM checks |
| 3 (done) | Modules on D-Bus and files: tray (StatusNotifierItem) (done), media (MPRIS, Melodyae's queue and lyrics) (done), notifications (done), clipboard (done), ask (done) | as the C one, plus the new parts |
| 4 (done) | The canvas: edit mode (move, resize, snap, overlap, undo, reset, add, done), sizes that show more, child cards beside | VM real-input tests as `vmt/pcanvas.py` |
| 5 (done) | Desktop widgets: the same modules on a bottom layer under windows, edited the same way. One `WIDGETS` line (module[:look]@x,y,WxH); a second shell whose one panel is the desktop (the whole screen at 0,0, always open); its input region only the widgets (the whole screen while edited); the clock wakes the edge on the minute, data every 2 s only for widgets that show it; `infynitae-edge widgets` or a right click on a widget edits them; `--render widgets` | placed, edited, kept across restarts |
| 6 (started) | Switch: `EDGE_IMPL=rust` the default, a week of use, then the C edge and its tests removed. Done: `live/build-helpers.sh` builds both (`infynitae-edge-c`, `infynitae-edge-rs`) and `live/edge-pick` points `infynitae-edge` at the one EDGE_IMPL names, so every caller keeps the one name; `infynitae-reload edge` picks again | owner says so |
| 7 | Everything else Infynitae draws in the same look and the same settings: the lock screen, the password prompt (polkit), the screen-share chooser, the control menu, notifications (mako's theme, or the edge's own popups), the volume and brightness popups, the ask prompts, window corners. Each moves to Rust when it is reworked. Started: `infynitae-edge-rs --print-theme` prints the resolved look (LOOK_* lines); `live/settings.sh` gives it to GTK (corners), mako and fuzzel (corners, card colour and opacity, edge, text, accent), today's values without the Rust edge | one look everywhere; one place to change it |
| 8 (started) | The config cleaned up (owner, 2026-11-09): one clear file in sections, every setting once with its comment and values, the edge's and widgets' lines together; old names still read and rewritten once to the new ones, `live/config-check` knowing both | an old config works unchanged and is tidied on update. Done: renamed settings in one list (`live/config-renames`) that live/config.sh, config-check and the update read (tools/test-config-renames); the look settings in one place after SQUARE_CORNERS |

## One look, one set of settings

Shared by the edge, the widgets and stage 7's parts (and synced to GTK, Qt, mako and the terminals by
THEME_SYNC as now):

| Setting | Values | Replaces |
|---|---|---|
| `CORNERS` | 0 (square) to 28 px; default 8 | `SQUARE_CORNERS` (still read: true = 0) |
| `THEME_SCHEME` | yours (SURFACE_COLOR, TEXT_COLOR, BORDER_ACTIVE as they are), monochrome, neutral, tonal, vibrant, expressive (built from BORDER_ACTIVE) | |
| `CARD_OPACITY` | 20 to 100 %; default 90 (70 once CARD_BLUR works) | |
| `CARD_BLUR` | true / false (needs the compositor's blur behind a surface; until then false) | |
| `SWITCH_LOOK` | rows, icons, tiles | |
