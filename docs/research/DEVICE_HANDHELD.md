# Handhelds and Steam Deck (DEVICE_HANDHELD)

## Changes

- **Default panel orientation from DRM** (`crates/infynitae-backend-drm/src/lib.rs:127-147, 740-767, 976-987`): at connector setup (`connect_screen`, including hotplug via `rescan_screens`), when no `OUTPUT_<NAME>_TRANSFORM` override is configured, `backend-drm` reads the connector's `"panel orientation"` enum property and maps `Normal` -> `Normal`, `Upside Down` -> `R180`, `Left Side Up` -> `R90`, `Right Side Up` -> `R270` (`Unknown` or absent -> `Normal`).
  - Verified by unit test `panel_orientation_maps_kernel_values_and_lets_custom_override` (`crates/infynitae-backend-drm/src/lib.rs:4532-4582`, failed when `resolve_output_transform` ignored `panel_orientation`, passed after) and headless touch test `a_touch_on_a_rotated_panel_maps_through_the_output_transform` (`crates/infynitae-backend-headless/tests/t1_touch.rs:501-549`, 800x1280 panel with `Transform::R270` at 1.5x scale).
  - Reading the real kernel `"panel orientation"` property from DRM ioctls (`read_connector_panel_orientation`) was not run against DRM hardware.
- **Touch mapping check** (`crates/infynitae-backend-drm/src/lib.rs:4071-4104`): `touch_position` already maps physical panel coordinates `(event.x_transformed(screen.size.0), event.y_transformed(screen.size.1))` through `convert::panel_point_to_view(..., screen.size, model.transform)` and divides by `model.scale`. No production code change was needed.
- **Power button short-press suspend on handhelds** (`live/install-lib.sh:81-91, 344-350`): extracted `has_battery` (`${INFYNITAE_POWER_SUPPLY:-/sys/class/power_supply}/BAT*`, matching `live/battery-limit:12`) and `wants_power_key_suspend` (`has_tablet_mode_switch || has_accelerometer || { has_touchscreen && has_battery; }`) so touchscreen+battery handhelds without `SW_TABLET_MODE` or an IIO accelerometer also install `live/50-infynitae-power-key.conf` (`HandlePowerKey=suspend`, `HandlePowerKeyLongPress=poweroff`), while touchscreen monitors on desktops and clamshell laptops stay unchanged.
  - Verified by `tools/test-install-lib:207-234` (failed before adding `has_battery` and `wants_power_key_suspend`, all 25 checks passed after).

## Findings not changed

### 1. Default output scale (`OUTPUT_SCALE`) is fixed at 1.0, not density-based

- **Current behavior**: `config/config:415` ships `OUTPUT_SCALE="1.0"`. `crates/infynitae/src/main.rs:447-453, 493-499` parses `OUTPUT_SCALE` into `ConsoleOptions.scale_120`, and `crates/infynitae-backend-drm/src/lib.rs:970-975, 1283` falls back to `options.scale_120.unwrap_or(120)` when no `OUTPUT_<NAME>_SCALE` is set. `conn.size()` (`crates/infynitae-backend-drm/src/lib.rs:1145`) is read only for `HeadInfo.physical_size`, not to select a default scale.
- **Rule note**: No density-based default scale rule exists today, and `live/env.sh:130`, `live/settings.sh:77`, and `live/edge-apps/src/cards/settings_display.rs:2128-2136` all expect a numeric `OUTPUT_SCALE` default in `config/config`.
- **Proposal**: Add `OUTPUT_SCALE="auto"` (or keep per-output `OUTPUT_<NAME>_SCALE` / numeric `OUTPUT_SCALE` as overrides) and compute default `scale_120` in `backend-drm` from `(w, h)` and `conn.size()` (`mm_w, mm_h > 0`) using diagonal PPI (for example: `< 140` PPI -> `1.0`, `140..185` -> `1.25`, `185..245` -> `1.5`, `>= 245` -> `2.0`), which gives `1.5` for a 7-inch 1280x800 (~215 PPI) panel and `2.0` for 7-inch 1080p (~315 PPI) or 8.8-inch 1600p (~343 PPI) handhelds.

### 2. Audit: Steam Input uinput hotplug, Big Picture (X11), and Deck trackpads

- **Uinput hotplug (`DeviceAdded` / `DeviceRemoved`)**:
  - `LibinputInputBackend` (`crates/infynitae-backend-drm/src/lib.rs:1595-1602`, `vendor/smithay-0.7.0/src/backend/libinput/mod.rs:724-738`) handles udev `add`/`remove` for seat input devices, so a uinput mouse or keyboard appearing mid-session enters `handle_input` through `InputEvent::DeviceAdded` (`crates/infynitae-backend-drm/src/lib.rs:3876-3906`).
  - `DeviceAdded` runs `configure_input_device` (`mouse_accel_profile`, `mouse_accel_speed`, `touchpad_tap`, `touchpad_natural_scroll`), syncs keyboard LEDs, calls `enable_touch()` if touch-capable, and appends to `data.input_devices` so live config reloads (`crates/infynitae-backend-drm/src/lib.rs:1826-1828`) also apply to it.
  - `WaylandState::new` (`crates/infynitae-wayland/src/lib.rs:656, 716`) adds `wl_pointer` and `wl_keyboard` to `seat0` at startup and never removes them, so a uinput pointer appearing mid-session works without a capability roundtrip. When all pointers disconnect (e.g. Steam closes its uinput mouse), the cursor stays visible where it stopped (`wl_pointer` is not removed and pointer device count is not tracked).
  - `DeviceRemoved` (`crates/infynitae-backend-drm/src/lib.rs:3907-3921`) calls `release_held_by(data, device.sysname())` (`3953-3997`) to release keys/buttons still held by that device unless another device holds the same code, removes it from `keyboards` and `input_devices`, and clears `last_abs_pointer`.
  - Any `InputEvent` (including `DeviceAdded` and `DeviceRemoved`) calls `user_activity(data)` (`crates/infynitae-backend-drm/src/lib.rs:3680`), so a background uinput add/remove while idle resets the inactivity timer and wakes DPMS sleep (`3654-3677`).
  - Known input-lifecycle gaps already documented in `docs/research/AUDIT_INPUT_LIFECYCLE.md` (not duplicated as changes here): `data.buttons_down` lacks per-device refcounting when two pointers hold the same button (`#7`), and `disable_touch` / `DeviceRemoved` on one of two touchscreens does not emit `TouchCancel` (`#6`, `#13`).
- **Deck trackpads and `hid-steam`**:
  - Public kernel `drivers/hid/hid-steam.c` fact: the Steam Deck controller starts in hardware "lizard mode" (emulating mouse/keyboard), which Steam disables via `hidraw` while running and replaces with `uinput` virtual pointer/keyboard devices; kernel driver runtime behavior on Deck hardware was **not checked**.
  - `handle_input` ignores `InputEvent::Gesture*` via `_ => {}` (`crates/infynitae-backend-drm/src/lib.rs:3922`), matching the d1 boundary (gestures not implemented here).
- **Steam Big Picture (fullscreen X11 under `xwayland-satellite`)**:
  - `xwayland-satellite` runs as a Wayland client (`crates/infynitae-wayland/src/xwayland.rs`), so fullscreen X11 windows enter Core's fullscreen and game-mode paths (`docs/STATUS.md:122-127`).
  - `osk_check` (`crates/infynitae-wayland/src/osk.rs:77-83, 91`) opens `squeekboard` only when `self.seat.text_input().with_active_text_input(...)` (Wayland `zwp_text_input_v3`) is active. X11 apps under `xwayland-satellite` do not activate `zwp_text_input_v3`, so `squeekboard` does not auto-open in X11 text fields (Big Picture provides its own in-app OSK; manual `infynitae msg osk on` still opens `squeekboard`).
  - `Core` filters `BrightnessUp`/`BrightnessDown` and volume/media keys out when `shortcuts_inhibited` is true (`crates/infynitae-core/src/entry.rs:668-680`, `crates/infynitae-core/src/input/keys.rs:624-635`), so a fullscreen app holding `zwp_keyboard_shortcuts_inhibit_manager_v1` blocks hardware brightness keys.

### 3. Power status

- **Already present**:
  - Hardware brightness keys: `KEY_BRIGHTNESSUP` / `KEY_BRIGHTNESSDOWN` -> `KeyAction::BrightnessUp` / `BrightnessDown` (`crates/infynitae-core/src/input/keys.rs:629-630`) -> `Effect::BrightnessUp` / `BrightnessDown` (`crates/infynitae-core/src/entry.rs:866-867`) -> `live/launch.sh brightness-up` / `brightness-down` (`live/launch.sh:430-505` via `brightnessctl`, `light`, or `/sys/class/backlight/*`).
  - Brightness slider in `edge-rs`: `live/edge-rs/src/modules/volume.rs:18, 135-295` (when `brightnessctl -m` succeeds).
  - Battery charge limit and control-menu readout: `BATTERY_LIMIT`, `live/battery-limit`, `live/99-infynitae-battery.rules`, `live/control-menu:359-387` (`capacity` and `status` from `/sys/class/power_supply/BAT*`).

### Ranked unfixed gaps

| Rank | Gap | Fix (one sentence) | Size |
|---|---|---|---|
| 1 | No density-based default `OUTPUT_SCALE` (`config/config:415`, `crates/infynitae-backend-drm/src/lib.rs:970-975, 1145`) | Support `OUTPUT_SCALE="auto"` and pick a per-output default scale from mode size and DRM `mm_w`/`mm_h` when no explicit `OUTPUT_<NAME>_SCALE` or numeric `OUTPUT_SCALE` is set | M |
| 2 | Lid-less idle only blanks DPMS (`crates/infynitae-backend-drm/src/lib.rs:1838-1895`), leaving CPU/GPU awake on handhelds | Add an optional idle-suspend trigger (or logind `IdleAction=suspend` drop-in for battery devices without a lid switch) after DPMS blanking when not inhibited | M |
| 3 | OSK (`squeekboard`) does not auto-open for X11 apps such as Steam Big Picture (`crates/infynitae-wayland/src/osk.rs:77-91`) | Bridge X11 text-input focus or offer an optional pointer/touch-on-X11 OSK fallback while keeping Steam's own Big Picture OSK usable | M |
| 4 | `PointerMotionAbsolute` scales to raw `screen.size` without `model.transform` (`crates/infynitae-backend-drm/src/lib.rs:3799-3825`) | Transform absolute pointer coordinates through `convert::panel_point_to_view` (or scale to `model.view_size()`) on rotated outputs | S |
| 5 | `DeviceAdded` / `DeviceRemoved` reset idle timer and wake DPMS (`crates/infynitae-backend-drm/src/lib.rs:3680`) | Skip `user_activity(data)` on `DeviceAdded` and `DeviceRemoved` so virtual/uinput churn does not wake a blanked screen | S |
| 6 | `BrightnessUp`/`Down` and `Volume*` are blocked when `shortcuts_inhibited` is true (`crates/infynitae-core/src/entry.rs:668-680`) | Exempt hardware `Brightness*`, `Volume*`, and `Media*` actions from `shortcuts_inhibited` alongside `ControlMenu` and `SpawnLockScreen` | S |
| 7 | Active Rust edge (`live/edge-rs/src/modules/clock.rs`) does not display battery percentage/charging status (present only in frozen `live/edge/infynitae-edge.c:1397-1411, 2584-2601`) | Read `/sys/class/power_supply/BAT*` in `edge-rs` and display battery status in `clock` or a dedicated module when present | M |
| 8 | Cursor stays visible when the last pointer (e.g. Steam uinput mouse) disappears (`crates/infynitae-wayland/src/lib.rs:656`, `crates/infynitae-backend-drm/src/lib.rs:3890-3921`) | Track connected pointer count in `backend-drm` or hide the pointer cursor until pointer motion when no physical/uinput pointer is attached | S |

## Needs real hardware to confirm

- DRM connector `"panel orientation"` property read (`read_connector_panel_orientation`) and upright rendering + touch alignment on a physical Steam Deck (`800x1280`, `Right Side Up`), ROG Ally, or Legion Go.
- Steam client launching/exiting Big Picture mid-session: `hid-steam` lizard-mode handoff, uinput mouse/keyboard `DeviceAdded`/`DeviceRemoved` events, and `xwayland-satellite` fullscreen behavior with Steam Big Picture.
- `50-infynitae-power-key.conf` short-press suspend / wake-to-lock (`infynitae-sleep-lock`) and long-press poweroff on a Steam Deck or other handheld with `BAT*` and `ID_INPUT_TOUCHSCREEN=1`.
- Brightness keys (`KEY_BRIGHTNESSUP` / `KEY_BRIGHTNESSDOWN`) and `/sys/class/backlight/*` on handheld backlight drivers.
