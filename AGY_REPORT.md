# AGY Report: Animated Xcursor Cursors

## 1. What was changed

- `crates/infynitae-wayland/src/cursor.rs`: Added `CursorFrame` and `LoadedCursor` to retain all frames of the chosen nominal size in file order, select frame by elapsed time modulo total delay (clamping 0ms delay to 50ms), maintain stable element IDs per frame for damage tracking, and exposed `WaylandState::cursor_animation_due`.
- `crates/infynitae-wayland/src/lib.rs`: Added `cursor_epoch: Duration` to `WaylandState` to track when cursor shapes are set and reset animation pacing.
- `crates/infynitae-wayland/src/session.rs`: Added cursor animation redraw scheduling (`set_cursor_animation_due`, factoring into `timeout()`, and triggering frame requests when due in `dispatch()` and `frame_due()`).
- `crates/infynitae-backend-drm/src/lib.rs`: Hooked `WaylandState::cursor_animation_due` into `session.set_cursor_animation_due` after event dispatch.
- `crates/infynitae-backend-headless/src/lib.rs`: Hooked `state.cursor_animation_due` into `session.set_cursor_animation_due` after event dispatch.
- `crates/infynitae-backend-winit/src/lib.rs`: Hooked `state.cursor_animation_due` into `session.set_cursor_animation_due` after event dispatch.
- `crates/infynitae-backend-headless/tests/t1_cursor_theme.rs`: Added `animated_theme_cursor_changes_pixels_between_two_times` headless integration test verifying pixel changes, frame timing, and damage tracking across animated cursor frames.
- `docs/STATUS.md`: Removed the line stating animated cursors show their first frame only.

## 2. How it was verified

The following commands were executed and passed cleanly:

1. `cargo test --package infynitae-wayland`:
   Result: `test result: ok. 50 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.15s`
   Included unit tests:
   - `cursor::tests::animated_cursor_frames_kept_in_order_and_wraparound`
   - `cursor::tests::animated_cursor_zero_delay_handled`
   - `cursor::tests::single_image_is_not_animated_due_is_none`
   - `cursor::tests::stable_element_id_across_unmoved_frames_and_change_across_frames`
   - `cursor::tests::wayland_state_cursor_animation_due_status_handling`
   - `session::tests::the_cursor_animation_due_bounds_the_sleep_and_requests_a_frame`

2. `cargo test --test t1_cursor_theme`:
   Result: `test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s`
   Included headless test:
   - `animated_theme_cursor_changes_pixels_between_two_times`

3. `cargo test --workspace --locked`:
   Result: `test result: ok.` across all workspace test targets (e.g. infynitae unit tests: 2 passed; infynitae-wayland unit tests: 50 passed; t1_cursor_theme: 6 passed; all t1_* integration tests passed; 0 failures).

4. `cargo fmt --all --check`:
   Result: Clean exit (code 0), no formatting diffs.

5. `cargo clippy --workspace --all-targets --locked -- -D warnings`:
   Result: Clean exit (code 0), 0 warnings.

6. `tools/verify-architecture`:
   Result: `== verify-architecture: PASS` (all 6 phases including compile probes and seeded negative controls passed).

7. `tools/check-foundation-docs`:
   Result: `RESULT: PASS` (all foundation documentation and formula checks passed).

## 3. What was NOT verified

- Real hardware / DRM output on a physical display with an active GPU; testing was conducted via unit tests and headless virtual output rendering tests (`Lockstep`/`Screen`).
- Real interactive desktop session with actual user input and system themes (e.g., Breeze or Adwaita loaded directly from host system directories `/usr/share/icons`), as running or touching host desktop sessions is prohibited.

## 4. Problems, doubts, and notes

- In `convert.rs:981`, `MemoryRenderBufferRenderElement::from_buffer` reuses the buffer's internal ID for element identity. Because `LoadedCursor` caches decoded frames in memory, each frame's buffer has a stable ID throughout its lifetime. Moving between different frames assigns distinct element IDs, producing damage, whereas remaining within the same frame preserves the identical element ID, producing `Ok(None)` damage on an unmoved cursor.
- Clamping of 0ms delays: Xcursor files in the wild occasionally contain 0ms delays for frames (which would cause a division by zero or spin if not clamped). Delays of 0ms are clamped to 50ms (20fps).
- Single-image cursors: If a cursor only contains a single frame of the target nominal size, `is_animated()` returns `false` and `cursor_animation_due` returns `None`, ensuring no unnecessary redraws or timer wakeups occur for static cursors.
