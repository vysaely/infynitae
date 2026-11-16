# Device: Touchpad gestures (`DEVICE_TOUCHPAD`)

Scope: `crates/infynitae-core`, `crates/infynitae-wayland`, `crates/infynitae-backend-drm`, `crates/infynitae-backend-headless`, `crates/infynitae`, `config/config`, `live/env.sh`, `live/edge-apps/src/cards/settings_input.rs`, `docs/EDGE_SETTINGS.md`, `docs/STATUS.md`, `tools/test-config-defaults`.

## What changed

- `crates/infynitae-core/src/lib.rs:157-177, 519, 630, 899-906, 1468-1476`: added `PhysicalEvent::Gesture{Swipe,Pinch,Hold}*` variants, `Core::touchpad_gestures` (default `true`) with getter/setter, and `frame_kick_effect` handling for `Effect::Gesture*`.
- `crates/infynitae-core/src/protocol.rs:69-89`: added client-delivery `Effect::Gesture{Swipe,Pinch,Hold}*` variants.
- `crates/infynitae-core/src/input/mod.rs:13-44, 434-779`: added `TouchpadGesture` / `GestureKind` ownership state on `InputState` and T0 unit tests for 3-finger pan, 4-finger pinch, 4-finger swipe up overview, 2-finger/hold client forwarding, `touchpad_gestures = false`, and non-finite input rejection.
- `crates/infynitae-core/src/entry.rs:15, 282-352, 2010-2013, 2350-2450, 2702-2984`: wired `PhysicalEvent::Gesture*` into `Core::physical_input`, extracted `begin_camera_pan` and `zoom_by_factor` so 3-finger swipe reuses mouse `CameraPan` and 4-finger pinch reuses `zoom_axis`, added 4-finger swipe up (`SWIPE_OVERVIEW_THRESHOLD = 32.0`) calling `overview_toggle`, and ensured button release / `KeyAction::Cancel` / `pointer_motion` stay consistent with `TouchpadGesture::Pan`.
- `crates/infynitae-wayland/src/lib.rs:72, 306-311, 790, 1243-1244, 1501-1503, 1593, 2881-2911`: advertised `zwp_pointer_gestures_v1` (`PointerGesturesState`), added `TOUCHPAD_GESTURES` config reload (default `true`, empty/removed resets to `true`), and added `reload_config_updates_and_resets_touchpad_gestures`.
- `crates/infynitae-wayland/src/handlers.rs:1224`: added `smithay::delegate_pointer_gestures!(WaylandState)`.
- `crates/infynitae-wayland/src/pointer.rs:356-424`: applied `Effect::Gesture*` to `PointerHandle::gesture_*` using `convert::global_delta`.
- `crates/infynitae-backend-drm/src/lib.rs:44-47, 3692-3698, 4016-4062`: added `gesture_physical` and `handle_input` dispatch for libinput `InputEvent::Gesture{Swipe,Pinch,Hold}*`.
- `crates/infynitae/src/main.rs:350, 358-360, 452-456`: loaded `TOUCHPAD_GESTURES` from environment and config file at startup.
- `crates/infynitae-backend-headless/src/lib.rs:150-267` and `crates/infynitae-backend-headless/tests/t1_touchpad_gestures.rs:1-313`: added gesture injection functions and headless Wayland client tests proving 3-finger swipe pan, 2-finger pinch/hold forwarding, 4-finger pinch zoom around cursor, 4-finger swipe up overview, and `TOUCHPAD_GESTURES=false` forwarding.
- `config/config:74-77`, `live/env.sh:124`, `tools/test-config-defaults:59-61`, `live/edge-apps/src/cards/settings_input.rs:122-130, 281-283, 1011-1052`, `docs/EDGE_SETTINGS.md:249, 254`, `docs/STATUS.md:21, 88-92`: documented and exposed `TOUCHPAD_GESTURES`.

## Found but did not change

Ranked by user impact:

1. **Interrupted gesture state on session lock or touchpad unplug (`crates/infynitae-backend-drm/src/lib.rs:3665-3679, 3860`)** — `is_locked()` drops `Gesture*End` events and `DeviceRemoved` does not cancel an active gesture, so locking or unplugging a touchpad mid-3-finger-pan can leave `Interaction::CameraPan` active until another swipe ends or Escape cancels it. Fix: cancel any active `TouchpadGesture` on session lock and on pointer/touchpad `DeviceRemoved`. Size: **S**.
2. **4-finger pinch epoch damping and `ZoomFeel::Floaty` momentum (`crates/infynitae-core/src/entry.rs:2375-2450`, `crates/infynitae-core/src/scale.rs:130-136`)** — `zoom_by_factor` reuses `zoom_axis` verbatim, so it does not open a continuous `InteractionKind::CameraZoom` scale epoch across pinch updates, and `ZoomFeel::Floaty` adds wheel momentum on each continuous pinch packet. Fix: keep a continuous `CameraZoom` epoch open during `TouchpadGesture::Pinch`, settle on `GesturePinchEnd`, and skip `Floaty` wheel momentum for continuous pinch steps. Size: **S**.
3. **`TOUCHPAD_NATURAL_SCROLL=""` and removed-key reset (`crates/infynitae-wayland/src/lib.rs:1495-1498`)** — empty `TOUCHPAD_NATURAL_SCROLL=""` evaluates to `Some(false)` and removed `TOUCHPAD_TAP` / `TOUCHPAD_NATURAL_SCROLL` lines are not reset on `reload_config` (already tracked in `docs/research/AUDIT_CONFIG.md`). Fix: reset both to `None` before parsing config lines and treat `""` as `None`. Size: **S**.
4. **Touchpad settings card hardware detection (`live/edge-apps/src/cards/settings_input.rs:155-159`)** — `TouchpadData` only checks `/proc/bus/input/devices` for `"Touchpad"` in the device name, hiding the three touchpad settings on touchpads named differently (already noted in `docs/EDGE_SETTINGS.md:254`). Fix: check libinput/udev touchpad capability or `ID_INPUT_TOUCHPAD` instead of substring matching `"Touchpad"`. Size: **S**.

## Needs real hardware to confirm

- Physical libinput `GestureSwipe*`, `GesturePinch*`, and `GestureHold*` delivery on a real laptop touchpad in the DRM console session (`libinput` C event delta scale, finger counts, and `cancelled` transitions when adding/lifting a finger).
- `SWIPE_OVERVIEW_THRESHOLD` (`32.0` logical units in `crates/infynitae-core/src/entry.rs:2837`) feel on real touchpads across different DPI/dimensions.
- Real Wayland client pinch/swipe behavior in Firefox, image viewers, and GTK/Qt apps over `zwp_pointer_gestures_v1`.
