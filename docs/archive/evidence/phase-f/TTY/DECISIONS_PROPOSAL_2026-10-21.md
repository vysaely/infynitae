# TTY (DRM/KMS) backend: decisions proposal (2026-10-21)

**Goal (the owner, 2026-10-21):** "finish stuff so we could just run it in tty2". Infynitae runs as
the only compositor on a text console. It owns the screen and all input, with no KDE underneath.

**Machine checked:**
- libseat 0.9.3, libinput 1.32, udev 262, gbm/mesa 26.2.4, libdrm 2.4.134 and EGL 1.5.
- One GPU, `/dev/dri/card1` (RX 9070 XT).
- The user is in the `video` and `input` groups; seat0 is managed by logind.

## Decisions (recommendation first)

| # | Question | Recommendation | Alternatives |
|---|---|---|---|
| T1 | Where | New crate `infynitae-backend-drm`. The binary uses it on `--tty`, or automatically when started with neither `WAYLAND_DISPLAY` nor `DISPLAY` set (that is, on a console). | flag only |
| T2 | Session and device access | libseat (via logind here, seatd elsewhere): it opens the GPU and input devices without root, and VT switching pauses and resumes the session. | logind directly over D-Bus |
| T3 | Rendering | GLES on GBM/EGL through Smithay's `DrmCompositor` (the path Smithay's own anvil uses): fast, vsynced, page-flipped. dmabuf import for clients follows in INC-13. | Pixman on dumb buffers: simpler but slow, as in the window |
| T4 | Input | libinput through the seat. Keyboard layout from `XKB_DEFAULT_LAYOUT` etc., `us` by default. | |
| T5 | Screens | Every connected connector, at its preferred mode, on the one GPU; hotplug through udev. | first connector only |
| T6 | Safety | **Ctrl+Alt+Backspace always quits.** Ctrl+Alt+F1…F12 switches VT, so KDE stays one key away on its VT. A crash restores the console. | |
| T7 | Dependencies | The backend crate only gains `drm`, `gbm`, `input`, `libseat`, `udev` (Smithay's own); `verify-architecture`'s allowlist is extended for that crate alone. Core stays at zero dependencies. | |
| T8 | Cursor | On a console nobody draws the pointer, so Infynitae does: the client's cursor surface when it sets one, otherwise a built-in arrow. | xcursor themes (later) |
| T9 | Testing and status | Pure parts are unit-tested: mode choice, escape keys, cursor. My standing rules keep DRM, KMS and VTs off-limits to me, so **the first real run is the owner's**, on tty2, with a log at `~/infynitae/live/logs/tty.log`. Status: EXPERIMENTAL, like the runtime. | |

## Running it (after implementation)

1. Press Ctrl+Alt+F2 and log in.
2. Run `~/infynitae/live/run-tty.sh`.
3. To quit, press Ctrl+Alt+Backspace. Ctrl+Alt+F1 goes back to KDE at any time.

## Size

The first runnable version is the work of this session and the next: session, GPU, one or more
screens, input, cursor. Expect rough edges on the first try (mode, cursor, keyboard layout). Each
run's log tells me what to fix.
