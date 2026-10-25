# Console (TTY) backend — implementation record (2026-10-22)

Status: **IMPLEMENTED, NOT YET RUN**, on branch `feature/tty-backend-2026-10-22` (from
`fix/nested-window-2026-10-21`). EXPERIMENTAL; not pushed. The first run is the owner's, on a
console (T9).

## What was built

- **`crates/infynitae-backend-drm`** (T1–T7):
  - **Device access:** a libseat session gives the primary GPU (opened through the seat) and
    libinput on the seat.
  - **Rendering:** GBM allocator, GLES renderer, and one `DrmCompositor` per connected connector
    at its preferred mode, page-flipped on vblank. Frame callbacks complete at the vblank, or at
    once when the frame changed nothing.
  - **VT switching:** a switch pauses the session (input suspended, DRM paused) and resumes it
    (DRM reactivated, compositor state reset).
  - **Console keys:** Ctrl+Alt+Backspace and Ctrl+Alt+F1…F12 are taken before Core, and their
    releases with them.
- **Binary:** `--tty`, `--nested` and `--scale`. With neither `WAYLAND_DISPLAY` nor `DISPLAY`
  set, the console backend is the default.
- **Adapter:**
  - `convert::output_elements` builds one frame's elements, shared by the damage-tracker path
    (headless, nested) and the DRM compositor.
  - `OutputElement` is surfaces plus solid rects.
  - A console render target (`set_console_target`) draws the cursor: `SeatHandler::cursor_image`
    is kept; the client's cursor surface is drawn at its hotspot, or the built-in arrow. The
    geometry lives in `convert` (INV-COORD-4).
  - Cursor frame callbacks complete with the output's frame.
- **Architecture:** `verify-architecture` and the source rules know the new crate. It may depend
  on core and wayland, never on another backend, and names only `smithay`. It is checked for
  `unsafe` (U-007).

## Evidence

| What | Test | Seeded control ("console: …") |
|---|---|---|
| Console keys need Ctrl and Alt | `infynitae-backend-drm` `the_console_keys_need_ctrl_and_alt` | Ctrl or Alt alone |
| Refresh from the pixel clock | `refresh_comes_from_the_pixel_clock` | rounded up |
| Arrow at the pointer; none off-console | T1 `t1_pixels::the_console_target_draws_the_arrow_at_the_pointer` | no cursor drawn; target asks for none |
| Client cursor at its hotspot; hidden | T1 `t1_pixels::the_console_target_draws_the_clients_cursor_at_its_hotspot` | hotspot ignored; set_cursor not kept |
| Arrow geometry | `convert::cursor_tests::the_arrow_has_its_tip_at_the_hotspot` | — |

Two existing controls were re-anchored after `render_output` was split (same defects).

## Not tested here, and what the first run checks

The DRM/libseat/libinput loop cannot run in the test environment. The first run checks:
- the mode chosen;
- the picture: colours and orientation;
- the cursor;
- keyboard layout and modifiers;
- VT switching back and forth;
- quitting.

## Known limits [F]

- **No hotplug:** screens connected after start are not picked up.
- **No dmabuf import (INC-13):** GPU (EGL/Vulkan) clients may fail or fall back to software;
  shm clients (foot) work.
- **Modifiers after a VT switch:** they may be stuck (Ctrl/Alt released on another VT) until
  pressed again.
- **No hard escape:** if the loop itself hangs, Ctrl+Alt+Backspace cannot be served. The
  launcher's `--duration` is a time limit for a first try.
- **Make and model** are "Unknown" (EDID not read).
