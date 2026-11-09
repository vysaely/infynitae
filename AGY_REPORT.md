# AGY Report: edge-rs Stage 1

## 1. What Changed and Why

- `live/edge-rs/Cargo.toml`: Added `smithay-client-toolkit = "0.20"`, `signal-hook-registry = "1.4"`, and `libc = "0.2"` for the Wayland layer-shell client, `calloop` event loop integration, and async-signal-safe pipe notifications.
- `live/edge-rs/Cargo.lock`: Updated lockfile for the added dependencies (`--locked` builds succeed).
- `live/edge-rs/src/modules.rs`: Added `cal_month` offset to `World`, `Module::click` / `Module::key` (and `modules::click` / `modules::key`) dispatch, `shifted_month` and `calendar_arrows`, drawn `<` / `>` month arrows in the calendar header, and unit tests for changing the displayed month.
- `live/edge-rs/src/main.rs`: Added CLI parsing into `Action` (`Run`, `Panel(name)`, `Stop`, `PrintPanels`, `Render`), `panel_rect`, `hover_strip`, and `panel_hover_box`, the `Shell` open/close and input state machine (including config `mtime` check on panel open), and unit tests for CLI parsing, hover strips per side/alignment, state machine transitions, and config reload.
- `live/edge-rs/src/wayland.rs`: Implemented the running Wayland client using `smithay-client-toolkit` and `calloop` with one `wlr-layer-shell` `Layer::Top` surface per output, two `wl_shm` buffers per surface reused only after compositor release, frame-callback pacing, hover/panel input regions, pointer/keyboard/touch forwarding, `$XDG_RUNTIME_DIR/infynitae-edge-rs.pid` and `.panel` files, and a signal self-pipe registered in `calloop`.
- `docs/EDGE_RUST.md`: Ticked stages 0 and 1 in the stage table.

### New Dependencies
- `smithay-client-toolkit = "0.20"` (default features `calloop` and `xkbcommon`): Provides `wlr-layer-shell` (`LayerShell`, `LayerSurface`), `wl_shm` `SlotPool`/`Buffer` with release tracking, `CompositorState`/`Region`, `OutputState`, `SeatState` (pointer, keyboard with `xkbcommon`, touch), and re-exports `calloop`, `calloop-wayland-source`, and `wayland-client`.
- `signal-hook-registry = "1.4"`: Registers handlers for `SIGTERM`, `SIGINT`, `SIGUSR1`, `SIGUSR2`, and `SIGRTMIN..=SIGRTMIN+3` that write a single byte into a non-blocking pipe read by `calloop` (`calloop::signals::Signals` does not cover `SIGRTMIN+n` real-time signals).
- `libc = "0.2"`: Provides `pipe2`, `read`, `write`, `kill`, `SIGTERM`, `SIGINT`, `SIGUSR1`, `SIGUSR2`, and `SIGRTMIN()`.

## 2. How It Was Verified

Ran inside `/home/vysaely/.cache/agy-tasks/edge-rs-stage1/wt/live/edge-rs`:

1. `cargo fmt --check`
   - Result: exit code `0` (no formatting diffs).
2. `cargo clippy --locked --all-targets -- -D warnings`
   - Result: `Finished dev profile [unoptimized + debuginfo] target(s) in 0.04s`, exit code `0`, `0` warnings.
3. `cargo test --locked`
   - Result: `test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s`
   - Tests run:
     - `config::tests::a_line_changes_only_what_it_says_and_new_names_make_panels`
     - `config::tests::a_placed_panel_keeps_its_size_and_rectangles`
     - `config::tests::built_in_panels_follow_the_edge_and_the_control_side`
     - `config::tests::looks_commas_and_unknown_words`
     - `config::tests::theme_settings_with_their_older_names`
     - `theme::tests::monochrome_has_no_hue`
     - `theme::tests::yours_takes_the_system_colours_as_they_are`
     - `modules::tests::calendar_month_arrows_change_shown_month`
     - `modules::tests::clock_with_calendar_look_forwards_arrow_clicks`
     - `tests::command_line_parses_into_actions`
     - `tests::hover_strip_rectangles_per_side_and_alignment`
     - `tests::open_close_state_machine_transitions`
     - `tests::config_reloads_when_mtime_changes_on_open`

Ran from the repository root (`/home/vysaely/.cache/agy-tasks/edge-rs-stage1/wt`):

4. `sh tools/test-edge-rs`
   - Result:
     - `13` `--print-panels` comparisons between C and Rust edges: all `13` `ok`.
     - `4` `--render` checks (`apps`, `windows`, `control`, `clipboard` at `1920x1080`): all `4` `ok`.
     - Final line: `all edge-rs tests passed` (exit code `0`).

## 3. What Was NOT Verified

- Did not run `infynitae-edge-rs` (with no arguments or `panel NAME`) against a live Wayland compositor or display, as instructed not to run the new edge on the owner's live desktop session and no screen/VM is available in this worker environment.
- Live Wayland layer-shell surface mapping, shm buffer release under a compositor, keyboard focus grab (`KeyboardInteractivity::Exclusive`), hover strip pointer entry/leave on a real output, and `SIGRTMIN+2` IPC between two running processes must be tested in the manager's VM.

## 4. Problems, Doubts, and Observations

- **Click-outside input region on Wayland**: Item 2 specifies setting the surface input region when open to the panel's rectangle (`(ox, oy, pw, ph)`) and closing on Escape or a click outside the panel. Both are implemented (`OutputSurface::set_input_region` sets the panel's rectangle, extended across the 8 px margin to the screen edge when opened by hover so moving from the 2 px strip into the panel does not cross an un-regioned gap; `Shell::click` closes when `(x, y)` is outside `(ox, oy, pw, ph)`). Note that on a Wayland compositor, pointer button events outside a surface's `wl_region` are routed to the surface beneath rather than the layer surface unless the input region covers the full output while keyboard interactivity is grabbed (which is what the C edge does when `kb_grab` is true).
- **Linux `/proc/<pid>/comm` truncation**: Because `"infynitae-edge-rs"` is 17 bytes and Linux truncates `/proc/<pid>/comm` to 15 bytes (`"infynitae-edge-"`), `wayland::running_pid` checks `comm.trim().starts_with("infynitae-edge-")` so it matches `infynitae-edge-rs` while still distinguishing it from the C binary `infynitae-edge`.
