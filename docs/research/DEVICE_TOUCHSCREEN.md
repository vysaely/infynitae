# Touchscreen and tablet mode (`DEVICE_TOUCHSCREEN`)

Scope: `crates/infynitae-backend-drm/src/lib.rs`, `crates/infynitae-core/src/{lib.rs,entry.rs,layer.rs,protocol.rs,touch.rs}`, `crates/infynitae-wayland/src/{lib.rs,lock.rs,osk.rs,outputs.rs,pointer.rs}`, `crates/infynitae/src/main.rs`, `config/config`, `live/{env.sh,launch.sh}`, `live/edge-apps/src/cards/settings_rows.rs`, `tools/test-config-defaults`, and `crates/infynitae-backend-headless/tests/{audit_input_lifecycle.rs,t1_input_method.rs,t1_lock_touch.rs,t1_touch.rs}`.

## What changed

### 1. Touch device to output mapping, rotation, scale, and slot pinning
- `crates/infynitae-backend-drm/src/lib.rs:267-271,531-533,1058-1063` — Stored `connector_name` and `physical_size` on `Screen`, and added `touch_slots: Vec<(u32, String, OutputId)>` keyed by seat-wide slot (`TouchEventSlot::seat_slot`) and device `sysname`.
- `crates/infynitae-backend-drm/src/lib.rs:4023-4176` — Added `select_touch_screen` (matching libinput `WL_OUTPUT` connector name first without leaking to another output if disconnected, then physical size in mm within 15 mm breaking ties by internal panel > pointer output > first screen, then internal panel > pointer output > first screen), `touch_point_to_output_logical` (`panel_point_to_view` + divide by `model.scale`), and per-slot output pinning across `TouchDown`/`TouchMotion`/`TouchUp`/`TouchCancel`.
- `crates/infynitae-backend-drm/src/lib.rs:853-855,1459-1463,3912-3934` — Cancelled active touches on VT/session pause (`PauseSession`), touch device removal (`DeviceRemoved` when `any_touch_left == false` or the removed device held an active slot), and output disconnect (`rescan_screens` clears `touch_slots` when any slot was on the disconnected output so it stays synced with Core's global `Effect::TouchCancel`).
- `crates/infynitae-wayland/src/pointer.rs:672` — Made `WaylandState::cancel_touches` `pub` for `backend-drm`.

### 2. Touch lifecycle and cancel delivery
- `crates/infynitae-core/src/entry.rs:627-634,3258-3264,3304-3310,3340-3378` — Added `Core::clear_touches`, settled pending touch-resize state (`touch_resize_settle`) across cancel paths, and cancelled touches on `output_reconfigured`, `output_arranged`, and `output_disconnected`.
- `crates/infynitae-core/src/layer.rs:454-560` — Cancelled touches in `layer_committed` when a touched layer unmaps or drops `Exclusive` keyboard interactivity, or when an `Exclusive` above-canvas layer opens while touches exist elsewhere on that output; cancelled touches in `layer_destroyed` when a touched layer is destroyed.
- `crates/infynitae-core/src/protocol.rs:1623,1744-1746,1789-1806` — Added `cancel_element_touches` on window unmap and element destroy, plus cancelling window/canvas touches on that output when fullscreen presentation starts (`membership` pass at `protocol.rs:609-625`).
- `crates/infynitae-wayland/src/lock.rs:86,134-136,471-473` and `crates/infynitae-wayland/src/pointer.rs:659-699,719-763` — Tracked `lock_touch_last` for stationary fingers on lock surfaces so `cancel_touches` sends `wl_touch.motion` + `wl_touch.cancel`; called `Core::clear_touches` before `cancel_touches` in `start_locked`, `SessionLockHandler::lock`, and `disable_touch`; cleared `SessionLock::fingers` inside `cancel_touches`.
- `crates/infynitae-wayland/src/outputs.rs:569-592` — Cancelled active lock-surface touches in `output_changed` and `output_removed` when an output rotates/reconfigures or unplugs while locked.

### 3. Edge swipe (`TOUCH_EDGE_SWIPE`), right-click on empty canvas, and tablet-mode OSK
- `crates/infynitae-core/src/touch.rs:82,380-505` and `crates/infynitae-core/src/entry.rs:638-705` — Added `EdgeSwipeCandidate` and `arm_edge_swipe` / `step_edge_swipe`. An inward swipe from within `ZONE_PX` (24 px) travelling at least `INWARD_PX` (28 px) inward over a closed above-canvas hover strip cancels the window/canvas touch, moves the pointer to the edge strip, and runs `true_position_into` so the existing pointer-hover panel action opens the panel. Open panels/OSK layers thicker than 8 px (`r.geometry.size.w > 8 && r.geometry.size.h > 8`) are left untouched even within 2 px of the edge.
- `crates/infynitae-core/src/lib.rs:497-498,609,878-888`, `crates/infynitae-wayland/src/lib.rs:1240,1404-1406,1602`, `crates/infynitae/src/main.rs:363-367,452-456`, `config/config:74-77`, `live/env.sh:124`, `live/edge-apps/src/cards/settings_rows.rs:198`, `tools/test-config-defaults:59-61` — Added config key `TOUCH_EDGE_SWIPE` (default `true`) across config, env, reload, and tests.
- Right click on empty canvas: checked `crates/infynitae-core/src/entry.rs:2507-2777`, `crates/infynitae-wayland/src/pointer.rs:321-346`, `live/wallpaper/infynitae-wallpaper.c:508-511`, and `live/edge-rs/src/wayland.rs:1707-1755`. Empty canvas has no compositor or wallpaper right-click action (desktop widgets on `Layer::Bottom` in `edge-rs` already receive `wl_touch` and map long press to `BTN_RIGHT`), so no empty-canvas long-press action was added.
- `crates/infynitae-wayland/src/osk.rs:56-127`, `crates/infynitae-wayland/src/outputs.rs:335-338`, `crates/infynitae-backend-drm/src/lib.rs:3854-3860,3902-3911`, `config/config:459-465`, `live/launch.sh:72-75` — Added `OskState::tablet_mode: Option<bool>` and `set_has_tablet_mode_switch`. When a `Switch::TabletMode` device is detected (or `tablet_mode_changed` is called), `OSK="auto"` shows squeekboard on text-field taps only while tablet mode is `true`, and `tablet_mode_changed(false)` hides any shown OSK and clears `osk-visible`. When no `TabletMode` switch exists (`None`), text-field taps continue to show the OSK.

## Found but not changed (ranked)

1. **Smithay `TouchDownGrab` binds all concurrent slots to the first touched surface** (`vendor/smithay-0.7.0/src/input/touch/grab.rs:244-262`).
   - If finger 0 is held on a layer surface (such as squeekboard) and finger 1 taps a window or another layer surface (`Core` skips `take_window_fingers` when any finger is on a layer surface at `crates/infynitae-core/src/entry.rs:330`), Smithay delivers slot 1 to finger 0's layer surface at `(window_global - layer_origin)` instead of the window.
   - Fix: replace `TouchDownGrab` usage or install a per-slot touch grab in `infynitae-wayland` that tracks each slot's own `DownEvent` focus surface. Size: M.

2. **`live/edge-apps` does not implement `wl_touch` yet** (`live/edge-apps/src/wayland.rs`).
   - While `live/edge-rs/src/wayland.rs:1707-1825` handles `wl_touch` (tap, drag, long-press right click), `live/edge-apps/src/wayland.rs` has no `TouchHandler`. Touching inside an `edge-apps` panel opened by an edge swipe will not trigger buttons there unless the user uses a pointer or `edge-rs`.
   - Fix: implement `TouchHandler` in `live/edge-apps/src/wayland.rs` routing taps and drags to `Shell` (matching `edge-rs`'s `TouchTracker`). Size: M.

3. **Unmapping a subsurface or popup while the parent window stays mapped does not cancel a touch held on that child surface** (`crates/infynitae-core/src/protocol.rs:1558`).
   - `cancel_element_touches` runs on root unmap and element destroy, not when a mapped parent commits a reshaped tree without a previously touched popup/subsurface.
   - Fix: in `Core::surface_committed` when `(Lifecycle::Mapped, true)` replaces `tree`, cancel touches if any `Anchor::Canvas` point's surface belonged to this element and is absent or unmapped in the new `tree`. Size: S.

4. **Portrait panels whose EDID physical dimensions are swapped relative to libinput `device.size()`** (`crates/infynitae-backend-drm/src/lib.rs:4110-4129`).
   - `select_touch_screen` compares `(dw, dh)` against `(sw, sh)` without checking swapped `(sh, sw)` if an unswapped match is absent.
   - Fix: fall back to checking swapped `(sh, sw)` within 15 mm only if no unswapped match exists and `WL_OUTPUT` is unset. Size: S.

5. **Swiping inward from an edge starts an initial `TouchDown` on any window under the start point before cancelling it at 28 px inward** (`crates/infynitae-core/src/entry.rs:222-223,670-705`).
   - Window clients near the edge briefly see `wl_touch.down` followed by `wl_touch.cancel` (standard Wayland gesture-stealing behaviour, but an app ignoring `cancel` could react on `down`).
   - Fix: hold `TouchDown` delivery while `edge_swipe` is pending (replaying it if the gesture cancels), if ever needed for misbehaving clients. Size: M.

## What needs real hardware to confirm

- Libinput `TouchEventSlot::seat_slot`, `device.output_name()` (`WL_OUTPUT`), `device.size()` physical mm accuracy, and `Switch::TabletMode` initial/toggle events from `thinkpad_acpi`/`intel-vbtn` on real convertibles and multi-screen setups (`infynitae-backend-drm` cannot run in the headless suite).
- `x_transformed`/`y_transformed` alignment on rotated/scaled panels (`R90`/`R180`/`R270`, fractional scale) with `live/autorotate` (`monitor-sensor` + `wlr-randr`).
- Real squeekboard show/hide round-trip over D-Bus (`sm.puri.OSK0.SetVisible`) when folding/unfolding a convertible and when swiping panels in from screen edges on a live session.
