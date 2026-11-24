//! Input: libinput events to Core, held keys, touch and tablets, the input screen, device settings.

use super::*;

/// The virtual devices' input (`zwlr_virtual_pointer_v1`, seat-keymap virtual keyboards: a remote
/// desktop) to Core like a mouse and a keyboard plugged in: it is activity (the screens wake),
/// the keys go through the seat's key path but not the console's own (no VT switch from afar).
pub(super) fn apply_virtual_input(data: &mut LoopData) {
    let events = data.state.take_virtual_input();
    if events.is_empty() {
        return;
    }
    user_activity(data);
    for ev in events {
        let time_us = infynitae_wayland::presentation_clock_now().as_micros() as u64;
        match ev {
            infynitae_wayland::virtual_input::VirtualInput::Key { key, pressed } => {
                core_key(data, key, pressed, time_us);
            }
            ev => {
                if let Some(phys) = data.state.virtual_pointer_event(ev, time_us) {
                    let port = &mut data.input_port;
                    let _ = data
                        .state
                        .with_effects(|core, fx| core.physical_input(port, phys, fx));
                }
            }
        }
    }
    data.session.request_frame();
}

/// Input was seen: the idle timer starts again, and screens asleep wake.
fn user_activity(data: &mut LoopData) {
    data.last_user_activity = std::time::Instant::now();
    let now = infynitae_wayland::presentation_clock_now();
    for s in &mut data.screens {
        s.latency.input(now);
    }
    if data.dpms_active {
        log_line!("[infynitae] User input detected: waking up screens from DPMS sleep");
        data.dpms_active = false;
        // a screen that read as disconnected while asleep and did not come back is removed then
        data.rescan_at = Some(std::time::Instant::now() + Duration::from_secs(3));
        for s in &mut data.screens {
            let _ = s.compositor.reset_state();
            s.pending = false;
            s.shows_locked = false;
        }
        // Re-apply client's gamma ramp after DPMS wake
        {
            let gamma_map = data.gamma_screens.lock().unwrap();
            for screen in gamma_map.values() {
                if let Some((r, g, b)) = &screen.current_gamma {
                    let _ = data.drm.device_fd().set_gamma(screen.crtc, r, g, b);
                }
            }
        }
        data.state
            .set_all_outputs_power_mode(infynitae_wayland::output_power::OutputPowerMode::On);
        data.session.request_frame();
    }
}

fn handle_input(data: &mut LoopData, event: InputEvent<LibinputInputBackend>) {
    user_activity(data);
    track_held(data, &event);
    if let InputEvent::PointerButton { event } = &event {
        let button = event.button_code();
        data.buttons_down.retain(|b| *b != button);
        if matches!(event.state(), ButtonState::Pressed) {
            data.buttons_down.push(button);
        }
    }
    // the lid (also while locked): closed, the laptop's panel is left out while another screen
    // is connected (with none, logind suspends the machine, its default); open, it comes back
    if let InputEvent::SwitchToggle { event } = &event {
        if SwitchToggleEvent::switch(event) == Some(Switch::Lid) {
            let closed = matches!(SwitchToggleEvent::state(event), SwitchState::On);
            if closed != data.lid_closed {
                log_line!(
                    "[infynitae] Lid {}",
                    if closed { "closed" } else { "opened" }
                );
                data.lid_closed = closed;
                data.rescan_at = Some(std::time::Instant::now());
            }
        }
        // tablet mode (a convertible folded): the on-screen keyboard follows
        if SwitchToggleEvent::switch(event) == Some(Switch::TabletMode) {
            let on = matches!(SwitchToggleEvent::state(event), SwitchState::On);
            log_line!("[infynitae] Tablet mode {}", if on { "on" } else { "off" });
            data.state.tablet_mode_changed(on);
        }
        return;
    }
    // locked: pointer and touch go nowhere (keys go to the lock surface, below); a button
    // Core holds and that is released meanwhile gets its release after the unlock
    if data.state.is_locked()
        && !matches!(
            event,
            InputEvent::Keyboard { .. }
                | InputEvent::DeviceAdded { .. }
                | InputEvent::DeviceRemoved { .. }
                | InputEvent::TouchDown { .. }
                | InputEvent::TouchMotion { .. }
                | InputEvent::TouchUp { .. }
                | InputEvent::TouchCancel { .. }
                | InputEvent::TouchFrame { .. }
        )
    {
        return;
    }
    // touches: the lock screen's first (its keyboard), else Core's; locked, the fingers Core
    // holds from before the lock are given up
    if let Some(ev) = touch_physical(data, &event) {
        if data.state.is_locked() {
            core_touch(data, PhysicalEvent::TouchCancel);
        }
        if !data.state.lock_touch(&ev) {
            core_touch(data, ev);
        }
        return;
    }
    if let Some(ev) = gesture_physical(&event) {
        let port = &mut data.input_port;
        let _ = data
            .state
            .with_effects(|core, fx| core.physical_input(port, ev, fx));
        return;
    }
    if let Some(ev) = tablet_event(data, &event) {
        core_tablet(data, ev);
        return;
    }
    match event {
        InputEvent::Keyboard { event } => {
            let key = event.key_code().raw().saturating_sub(8);
            let pressed = matches!(event.state(), KeyState::Pressed);
            // T6: the console's own keys, before Core (and so before any binding or client)
            if pressed {
                let m = data.state.seat_modifiers();
                match console_key(key, m.ctrl, m.alt) {
                    Some(ConsoleKey::Quit) if data.state.is_locked() => {
                        // quitting would restart the session unlocked: not while locked
                        log_line!("[infynitae] Ctrl+Alt+Backspace ignored: the session is locked");
                        data.taken.push(key);
                        return;
                    }
                    Some(ConsoleKey::Quit) => {
                        log_line!("[infynitae] Ctrl+Alt+Backspace: quitting");
                        data.quit = true;
                        data.taken.push(key);
                        return;
                    }
                    Some(ConsoleKey::SwitchVt(vt)) => {
                        if let Err(e) = data.seat.change_vt(vt) {
                            log_line!("[infynitae] VT {vt}: {e}");
                        }
                        data.taken.push(key);
                        return;
                    }
                    None => {}
                }
            } else if let Some(i) = data.taken.iter().position(|&k| k == key) {
                data.taken.swap_remove(i);
                return;
            }
            let time_us = Event::time(&event);
            let mut dev = Event::device(&event);
            // the session lock takes keys before Core (lock.rs)
            if !data.state.lock_key(key, pressed, time_us) {
                core_key(data, key, pressed, time_us);
            }
            update_keyboard_leds(&data.state, &mut data.keyboards, &mut dev);
        }
        InputEvent::PointerMotion { event } => {
            data.last_abs_pointer = None;
            let scale = (f64::from(data.scale_120) / 120.0).max(0.001);
            let delta = (event.delta_x() / scale, event.delta_y() / scale);
            let delta_unaccel = (
                event.delta_x_unaccel() / scale,
                event.delta_y_unaccel() / scale,
            );
            let time_us = Event::time(&event);
            let phys = PhysicalEvent::PointerMotion {
                delta,
                delta_unaccel,
                time_us,
            };
            let port = &mut data.input_port;
            let _ = data
                .state
                .with_effects(|core, fx| core.physical_input(port, phys, fx));
        }
        InputEvent::PointerMotionAbsolute { event } => {
            let current_ptr = data.state.core().pointer();
            let (current_output, current_p) = match current_ptr {
                infynitae_core::pointer::PointerState::Located { output, p } => (Some(output), p),
                infynitae_core::pointer::PointerState::NoOutput => (None, (0.0, 0.0)),
            };
            let screen = current_output
                .and_then(|out_id| data.screens.iter().find(|s| s.id == out_id))
                .or_else(|| data.screens.first());
            let (target_phys_x, target_phys_y) = match (screen, &data.virtual_screen) {
                (Some(s), _) => (event.x_transformed(s.size.0), event.y_transformed(s.size.1)),
                (None, Some(_)) => (
                    event.x_transformed(VIRTUAL_SCREEN_SIZE.0),
                    event.y_transformed(VIRTUAL_SCREEN_SIZE.1),
                ),
                (None, None) => (event.x(), event.y()),
            };
            let scale = screen
                .map(|s| s.id)
                .or(data.virtual_screen.as_ref().map(|v| v.id))
                .and_then(|id| data.state.core().output(id).map(|o| o.scale))
                .unwrap_or_else(|| (f64::from(data.scale_120) / 120.0).max(0.001));
            let delta =
                calculate_abs_pointer_delta_scale((target_phys_x, target_phys_y), current_p, scale);
            data.last_abs_pointer = Some((target_phys_x / scale, target_phys_y / scale));

            if delta.0.abs() > 0.0001 || delta.1.abs() > 0.0001 {
                let time_us = Event::time(&event);
                let phys = PhysicalEvent::PointerMotion {
                    delta,
                    delta_unaccel: delta,
                    time_us,
                };
                let port = &mut data.input_port;
                let _ = data
                    .state
                    .with_effects(|core, fx| core.physical_input(port, phys, fx));
            }
        }
        InputEvent::PointerButton { event } => {
            let pressed = matches!(event.state(), ButtonState::Pressed);
            if !pressed {
                data.last_abs_pointer = None;
            }
            let phys = PhysicalEvent::PointerButton {
                button: event.button_code(),
                pressed,
                modifiers: data.state.seat_modifiers(),
                time_us: Event::time(&event),
            };
            let port = &mut data.input_port;
            let _ = data
                .state
                .with_effects(|core, fx| core.physical_input(port, phys, fx));
        }
        InputEvent::PointerAxis { event } => {
            let source = match event.source() {
                AxisSource::Wheel | AxisSource::WheelTilt => {
                    infynitae_core::input::AxisSource::Wheel
                }
                _ => infynitae_core::input::AxisSource::Continuous,
            };
            let amount = |a| event.amount(a).unwrap_or(0.0);
            let v120 = |a| event.amount_v120(a).unwrap_or(0.0) as i32;
            let phys = PhysicalEvent::PointerAxis {
                axis: infynitae_core::input::Axis {
                    source,
                    amount: (amount(Axis::Horizontal), amount(Axis::Vertical)),
                    v120: (v120(Axis::Horizontal), v120(Axis::Vertical)),
                },
                time_us: Event::time(&event),
            };
            let port = &mut data.input_port;
            let _ = data
                .state
                .with_effects(|core, fx| core.physical_input(port, phys, fx));
        }
        InputEvent::DeviceAdded { mut device } => {
            configure_input_device(&mut device, data.state.input_config());
            if device.has_capability(smithay::reexports::input::DeviceCapability::Touch) {
                log_line!("[infynitae] Touchscreen: {}", device.name());
                data.state.enable_touch();
            }
            if device.has_capability(smithay::reexports::input::DeviceCapability::Switch)
                && device
                    .switch_has_switch(smithay::reexports::input::event::switch::Switch::TabletMode)
                    == Ok(true)
            {
                data.state.set_has_tablet_mode_switch(true);
            }
            if device.has_capability(smithay::reexports::input::DeviceCapability::TabletTool) {
                log_line!("[infynitae] Tablet: {}", device.name());
                let desc = smithay::wayland::tablet_manager::TabletDescriptor::from(&device);
                data.state.add_tablet(&desc);
            }
            if device.has_capability(smithay::reexports::input::DeviceCapability::TabletPad) {
                log_line!("[infynitae] Tablet pad: {}", device.name());
                let buttons = device.tablet_pad_number_of_buttons().max(0) as u32;
                let modes = (0..device.tablet_pad_number_of_mode_groups().max(0) as u32)
                    .filter_map(|i| device.tablet_pad_mode_group(i))
                    .map(|g| g.number_of_modes())
                    .max()
                    .unwrap_or(1)
                    .max(1);
                let path = Some(format!("/dev/input/{}", device.sysname()));
                data.state
                    .add_tablet_pad(infynitae_wayland::TabletPadDescriptor {
                        sysname: device.sysname().to_owned(),
                        path,
                        buttons,
                        modes,
                        tablet: smithay::wayland::tablet_manager::TabletDescriptor::from(&device),
                    });
            }
            let is_kb =
                device.has_capability(smithay::reexports::input::DeviceCapability::Keyboard);
            let is_ptr =
                device.has_capability(smithay::reexports::input::DeviceCapability::Pointer);
            log_line!(
                "[infynitae] Input device detected: {} (keyboard: {is_kb}, pointer: {is_ptr})",
                device.name()
            );
            if is_kb {
                let led_state = data.state.keyboard_led_state();
                let mut leds = smithay::reexports::input::Led::empty();
                if led_state.caps.unwrap_or(false) {
                    leds |= smithay::reexports::input::Led::CAPSLOCK;
                }
                if led_state.num.unwrap_or(false) {
                    leds |= smithay::reexports::input::Led::NUMLOCK;
                }
                if led_state.scroll.unwrap_or(false) {
                    leds |= smithay::reexports::input::Led::SCROLLLOCK;
                }
                device.led_update(leds);
                data.keyboards.push(device.clone());
            }
            data.input_devices.push(device);
        }
        InputEvent::DeviceRemoved { device } => {
            release_held_by(data, device.sysname());
            data.keyboards.retain(|k| k.sysname() != device.sysname());
            data.input_devices
                .retain(|d| d.sysname() != device.sysname());
            data.last_abs_pointer = None;
            if device.has_capability(smithay::reexports::input::DeviceCapability::Switch)
                && !data.input_devices.iter().any(|d| {
                    d.has_capability(smithay::reexports::input::DeviceCapability::Switch)
                        && d.switch_has_switch(
                            smithay::reexports::input::event::switch::Switch::TabletMode,
                        ) == Ok(true)
                })
            {
                data.state.set_has_tablet_mode_switch(false);
            }
            if device.has_capability(smithay::reexports::input::DeviceCapability::Touch) {
                let had_slot = data
                    .touch_slots
                    .iter()
                    .any(|(_, sys, _)| sys == device.sysname());
                let any_touch_left = data
                    .input_devices
                    .iter()
                    .any(|d| d.has_capability(smithay::reexports::input::DeviceCapability::Touch));
                if !any_touch_left || had_slot {
                    data.touch_slots.clear();
                    core_touch(data, PhysicalEvent::TouchCancel);
                    data.state.cancel_touches();
                }
                if !any_touch_left {
                    data.state.disable_touch();
                }
            }
            if device.has_capability(smithay::reexports::input::DeviceCapability::TabletTool) {
                let desc = smithay::wayland::tablet_manager::TabletDescriptor::from(&device);
                data.state.remove_tablet(&desc);
            }
            if device.has_capability(smithay::reexports::input::DeviceCapability::TabletPad) {
                data.state.remove_tablet_pad(device.sysname());
            }
        }
        _ => {}
    }
}

/// Notes which device holds which button or key down ([`LoopData::held_by_device`]).
fn track_held(data: &mut LoopData, event: &InputEvent<LibinputInputBackend>) {
    let (device, code, is_key, pressed) = match event {
        InputEvent::PointerButton { event } => (
            event.device(),
            event.button_code(),
            false,
            matches!(event.state(), ButtonState::Pressed),
        ),
        InputEvent::Keyboard { event } => (
            event.device(),
            event.key_code().raw().saturating_sub(8),
            true,
            matches!(event.state(), KeyState::Pressed),
        ),
        _ => return,
    };
    let name = device.sysname();
    data.held_by_device
        .retain(|(d, c, k)| !(d == name && *c == code && *k == is_key));
    if pressed {
        data.held_by_device.push((name.to_owned(), code, is_key));
    }
}

/// A device went away: what it still held down is released, unless another device holds the
/// same button or key.
fn release_held_by(data: &mut LoopData, sysname: &str) {
    let (gone, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut data.held_by_device)
        .into_iter()
        .partition(|(d, _, _)| d == sysname);
    data.held_by_device = kept;
    let now_us = infynitae_wayland::presentation_clock_now().as_micros() as u64;
    for (_, code, is_key) in gone {
        if data
            .held_by_device
            .iter()
            .any(|(_, c, k)| *c == code && *k == is_key)
        {
            continue;
        }
        log_line!(
            "[infynitae] {sysname} went away holding {} {code}: released",
            if is_key { "key" } else { "button" }
        );
        if is_key {
            // as the key path: a console key's release is the console's; the lock takes keys first
            if let Some(i) = data.taken.iter().position(|&k| k == code) {
                data.taken.swap_remove(i);
            } else if !data.state.lock_key(code, false, now_us) {
                core_key(data, code, false, now_us);
            }
        } else {
            data.buttons_down.retain(|b| *b != code);
            if data.state.is_locked() {
                // released after the unlock, as any button let go while locked
                continue;
            }
            data.last_abs_pointer = None;
            let phys = PhysicalEvent::PointerButton {
                button: code,
                pressed: false,
                modifiers: data.state.seat_modifiers(),
                time_us: now_us,
            };
            let port = &mut data.input_port;
            let _ = data
                .state
                .with_effects(|core, fx| core.physical_input(port, phys, fx));
        }
    }
}

/// A key for Core: the seat's XKB state first (through the input token), then Core's intake.
pub(super) fn core_key(data: &mut LoopData, key: u32, pressed: bool, time_us: u64) {
    if let Ok(token) = data
        .state
        .core()
        .verify_input_port(&data.input_port, key, pressed)
    {
        let (modifiers, mods_changed) = data.state.sync_seat_key(token);
        let phys = PhysicalEvent::KeyboardKey {
            key,
            pressed,
            modifiers,
            mods_changed,
            time_us,
        };
        let port = &mut data.input_port;
        let _ = data
            .state
            .with_effects(|core, fx| core.physical_input(port, phys, fx));
    }
}

/// A libinput touch event as Core's (and the lock's) touch: where it is on which screen
/// (`touch_position`), its seat-wide slot and time. `None` for any other event, or a touch off
/// every screen.
fn touch_physical(
    data: &mut LoopData,
    event: &InputEvent<LibinputInputBackend>,
) -> Option<PhysicalEvent> {
    Some(match event {
        InputEvent::TouchDown { event } => {
            let slot = TouchEventSlot::seat_slot(event);
            let sysname = Event::device(event).sysname().to_owned();
            let (output, p) = touch_position(data, event, None)?;
            data.touch_slots.retain(|(s, _, _)| *s != slot);
            data.touch_slots.push((slot, sysname, output));
            PhysicalEvent::TouchDown {
                slot,
                output,
                p,
                time_us: Event::time(event),
            }
        }
        InputEvent::TouchMotion { event } => {
            let slot = TouchEventSlot::seat_slot(event);
            let pinned = data
                .touch_slots
                .iter()
                .find(|(s, _, _)| *s == slot)
                .map(|(_, _, o)| *o)?;
            let (_, p) = touch_position(data, event, Some(pinned))?;
            PhysicalEvent::TouchMotion {
                slot,
                p,
                time_us: Event::time(event),
            }
        }
        InputEvent::TouchUp { event } => {
            let slot = TouchEventSlot::seat_slot(event);
            data.touch_slots.retain(|(s, _, _)| *s != slot);
            PhysicalEvent::TouchUp {
                slot,
                time_us: Event::time(event),
            }
        }
        InputEvent::TouchCancel { .. } => {
            data.touch_slots.clear();
            PhysicalEvent::TouchCancel
        }
        InputEvent::TouchFrame { .. } => PhysicalEvent::TouchFrame,
        _ => return None,
    })
}

/// Converts a libinput touchpad gesture event into Core's `PhysicalEvent`.
fn gesture_physical(event: &InputEvent<LibinputInputBackend>) -> Option<PhysicalEvent> {
    Some(match event {
        InputEvent::GestureSwipeBegin { event } => PhysicalEvent::GestureSwipeBegin {
            fingers: GestureBeginEvent::fingers(event),
            time_us: Event::time(event),
        },
        InputEvent::GestureSwipeUpdate { event } => PhysicalEvent::GestureSwipeUpdate {
            delta: (
                GestureSwipeUpdateEvent::delta_x(event),
                GestureSwipeUpdateEvent::delta_y(event),
            ),
            time_us: Event::time(event),
        },
        InputEvent::GestureSwipeEnd { event } => PhysicalEvent::GestureSwipeEnd {
            cancelled: GestureEndEvent::cancelled(event),
            time_us: Event::time(event),
        },
        InputEvent::GesturePinchBegin { event } => PhysicalEvent::GesturePinchBegin {
            fingers: GestureBeginEvent::fingers(event),
            time_us: Event::time(event),
        },
        InputEvent::GesturePinchUpdate { event } => PhysicalEvent::GesturePinchUpdate {
            delta: (
                GesturePinchUpdateEvent::delta_x(event),
                GesturePinchUpdateEvent::delta_y(event),
            ),
            scale: GesturePinchUpdateEvent::scale(event),
            rotation: GesturePinchUpdateEvent::rotation(event),
            time_us: Event::time(event),
        },
        InputEvent::GesturePinchEnd { event } => PhysicalEvent::GesturePinchEnd {
            cancelled: GestureEndEvent::cancelled(event),
            time_us: Event::time(event),
        },
        InputEvent::GestureHoldBegin { event } => PhysicalEvent::GestureHoldBegin {
            fingers: GestureBeginEvent::fingers(event),
            time_us: Event::time(event),
        },
        InputEvent::GestureHoldEnd { event } => PhysicalEvent::GestureHoldEnd {
            cancelled: GestureEndEvent::cancelled(event),
            time_us: Event::time(event),
        },
        _ => return None,
    })
}

/// One touch event to Core (its effects to the clients).
pub(super) fn core_touch(data: &mut LoopData, ev: PhysicalEvent) {
    let port = &mut data.input_port;
    let _ = data
        .state
        .with_effects(|core, fx| core.physical_input(port, ev, fx));
}

struct InputScreenCandidate<'a> {
    id: OutputId,
    connector: &'a str,
    physical_size: (u32, u32),
}

fn is_internal_connector(connector: &str) -> bool {
    ["eDP", "LVDS", "DSI"]
        .iter()
        .any(|p| connector.starts_with(p))
}

/// Chooses which connected screen owns a touch or tablet device:
/// 1. Explicit libinput `WL_OUTPUT` udev mapping (`device.output_name()`). If set, it binds
///    exclusively to that connector name (`None` when that output is not connected).
/// 2. Non-zero physical size match (`device.size()` in mm vs DRM `physical_size` within 15 mm),
///    breaking ties by internal panel > pointer output > first screen.
/// 3. Internal panel (`eDP`, `LVDS`, `DSI`), else `pointer_output`, else first connected screen.
fn select_input_screen(
    output_name: Option<&str>,
    device_size_mm: Option<(f64, f64)>,
    screens: &[InputScreenCandidate<'_>],
    pointer_output: Option<OutputId>,
) -> Option<OutputId> {
    if let Some(name) = output_name.filter(|s| !s.is_empty()) {
        return screens.iter().find(|s| s.connector == name).map(|s| s.id);
    }
    if let Some((dw, dh)) = device_size_mm.filter(|&(w, h)| w > 0.0 && h > 0.0) {
        const SIZE_TOLERANCE_MM: f64 = 15.0;
        let best = screens
            .iter()
            .filter(|s| s.physical_size.0 > 0 && s.physical_size.1 > 0)
            .filter_map(|s| {
                let (sw, sh) = (f64::from(s.physical_size.0), f64::from(s.physical_size.1));
                let dx = (dw - sw).abs();
                let dy = (dh - sh).abs();
                (dx <= SIZE_TOLERANCE_MM && dy <= SIZE_TOLERANCE_MM).then_some((
                    ((dx + dy) * 1000.0).round() as u64,
                    u8::from(!is_internal_connector(s.connector)),
                    u8::from(Some(s.id) != pointer_output),
                    s.id,
                ))
            })
            .min_by_key(|&(err, not_internal, not_ptr, _)| (err, not_internal, not_ptr));
        if let Some((_, _, _, id)) = best {
            return Some(id);
        }
    }
    screens
        .iter()
        .find(|s| is_internal_connector(s.connector))
        .or_else(|| screens.iter().find(|s| Some(s.id) == pointer_output))
        .or_else(|| screens.first())
        .map(|s| s.id)
}

fn pointer_output_id(core: &infynitae_core::Core) -> Option<OutputId> {
    match core.pointer() {
        infynitae_core::pointer::PointerState::Located { output, .. } => Some(output),
        infynitae_core::pointer::PointerState::NoOutput => None,
    }
}

fn screen_candidates(screens: &[Screen]) -> Vec<InputScreenCandidate<'_>> {
    screens
        .iter()
        .map(|s| InputScreenCandidate {
            id: s.id,
            connector: &s.connector_name,
            physical_size: s.physical_size,
        })
        .collect()
}

/// Converts a panel-pixel point `(px, py)` on a panel of physical size `phys` into the
/// output's logical coordinates using `transform` and `scale`.
fn touch_point_to_output_logical(
    panel_px: (f64, f64),
    phys: (i32, i32),
    transform: Transform,
    scale: f64,
) -> (f64, f64) {
    let (x, y) = convert::panel_point_to_view(panel_px, phys, transform);
    let scale = scale.max(0.001);
    (x / scale, y / scale)
}

/// Where a touch is: on `pinned_output` (for motions in an active slot), else the screen
/// selected for `event.device()` (`select_input_screen`); in that screen's logical px, turned
/// with its rotation (the touchscreen reports the panel's own orientation).
fn touch_position<E>(
    data: &LoopData,
    event: &E,
    pinned_output: Option<OutputId>,
) -> Option<(OutputId, (f64, f64))>
where
    E: AbsolutePositionEvent<LibinputInputBackend>,
{
    let core = data.state.core();
    let target_id = match pinned_output {
        Some(id) => id,
        None => {
            let candidates = screen_candidates(&data.screens);
            let dev = Event::device(event);
            select_input_screen(
                dev.output_name(),
                dev.size(),
                &candidates,
                pointer_output_id(core),
            )?
        }
    };
    let screen = data.screens.iter().find(|s| s.id == target_id)?;
    let model = core.output(screen.id)?;
    let p = touch_point_to_output_logical(
        (
            event.x_transformed(screen.size.0),
            event.y_transformed(screen.size.1),
        ),
        screen.size,
        model.transform,
        model.scale,
    );
    Some((screen.id, p))
}

pub(super) fn dispatch_libinput_event(
    data: &mut LoopData,
    event: smithay::reexports::input::Event,
) {
    use smithay::backend::libinput::PointerScrollAxis;
    use smithay::reexports::input::Event as LE;
    use smithay::reexports::input::event::{
        DeviceEvent, EventTrait, GestureEvent, KeyboardEvent, PointerEvent, SwitchEvent,
        TabletPadEvent, TabletToolEvent as LTE, TouchEvent as LTouchEvent,
        gesture::{GestureHoldEvent, GesturePinchEvent, GestureSwipeEvent},
        tablet_pad::TabletPadEventTrait,
    };
    let mapped = match event {
        LE::Device(DeviceEvent::Added(e)) => Some(InputEvent::DeviceAdded { device: e.device() }),
        LE::Device(DeviceEvent::Removed(e)) => {
            Some(InputEvent::DeviceRemoved { device: e.device() })
        }
        LE::Touch(LTouchEvent::Down(event)) => Some(InputEvent::TouchDown { event }),
        LE::Touch(LTouchEvent::Motion(event)) => Some(InputEvent::TouchMotion { event }),
        LE::Touch(LTouchEvent::Up(event)) => Some(InputEvent::TouchUp { event }),
        LE::Touch(LTouchEvent::Cancel(event)) => Some(InputEvent::TouchCancel { event }),
        LE::Touch(LTouchEvent::Frame(event)) => Some(InputEvent::TouchFrame { event }),
        LE::Keyboard(KeyboardEvent::Key(event)) => Some(InputEvent::Keyboard { event }),
        LE::Pointer(PointerEvent::Motion(event)) => Some(InputEvent::PointerMotion { event }),
        LE::Pointer(PointerEvent::MotionAbsolute(event)) => {
            Some(InputEvent::PointerMotionAbsolute { event })
        }
        LE::Pointer(PointerEvent::ScrollWheel(event)) => Some(InputEvent::PointerAxis {
            event: PointerScrollAxis::Wheel(event),
        }),
        LE::Pointer(PointerEvent::ScrollFinger(event)) => Some(InputEvent::PointerAxis {
            event: PointerScrollAxis::Finger(event),
        }),
        LE::Pointer(PointerEvent::ScrollContinuous(event)) => Some(InputEvent::PointerAxis {
            event: PointerScrollAxis::Continuous(event),
        }),
        LE::Pointer(PointerEvent::Button(event)) => Some(InputEvent::PointerButton { event }),
        LE::Gesture(GestureEvent::Swipe(GestureSwipeEvent::Begin(event))) => {
            Some(InputEvent::GestureSwipeBegin { event })
        }
        LE::Gesture(GestureEvent::Swipe(GestureSwipeEvent::Update(event))) => {
            Some(InputEvent::GestureSwipeUpdate { event })
        }
        LE::Gesture(GestureEvent::Swipe(GestureSwipeEvent::End(event))) => {
            Some(InputEvent::GestureSwipeEnd { event })
        }
        LE::Gesture(GestureEvent::Pinch(GesturePinchEvent::Begin(event))) => {
            Some(InputEvent::GesturePinchBegin { event })
        }
        LE::Gesture(GestureEvent::Pinch(GesturePinchEvent::Update(event))) => {
            Some(InputEvent::GesturePinchUpdate { event })
        }
        LE::Gesture(GestureEvent::Pinch(GesturePinchEvent::End(event))) => {
            Some(InputEvent::GesturePinchEnd { event })
        }
        LE::Gesture(GestureEvent::Hold(GestureHoldEvent::Begin(event))) => {
            Some(InputEvent::GestureHoldBegin { event })
        }
        LE::Gesture(GestureEvent::Hold(GestureHoldEvent::End(event))) => {
            Some(InputEvent::GestureHoldEnd { event })
        }
        LE::Tablet(LTE::Axis(event)) => Some(InputEvent::TabletToolAxis { event }),
        LE::Tablet(LTE::Proximity(event)) => Some(InputEvent::TabletToolProximity { event }),
        LE::Tablet(LTE::Tip(event)) => Some(InputEvent::TabletToolTip { event }),
        LE::Tablet(LTE::Button(event)) => Some(InputEvent::TabletToolButton { event }),
        LE::Switch(SwitchEvent::Toggle(event)) => Some(InputEvent::SwitchToggle { event }),
        LE::TabletPad(TabletPadEvent::Button(btn)) => {
            user_activity(data);
            let pressed = matches!(
                btn.button_state(),
                smithay::reexports::input::event::tablet_pad::ButtonState::Pressed
            );
            let dev = btn.device();
            data.state.tablet_pad_button(
                dev.sysname(),
                btn.button_number(),
                pressed,
                btn.time_usec(),
            );
            None
        }
        _ => None,
    };
    if let Some(ev) = mapped {
        handle_input(data, ev);
    }
}

fn tablet_axes<E>(event: &E) -> infynitae_wayland::TabletAxes
where
    E: TabletToolEvent<LibinputInputBackend>,
{
    infynitae_wayland::TabletAxes {
        pressure: event.pressure_has_changed().then(|| event.pressure()),
        distance: event.distance_has_changed().then(|| event.distance()),
        tilt: (event.tilt_x_has_changed() || event.tilt_y_has_changed())
            .then(|| (event.tilt_x(), event.tilt_y())),
        rotation: event.rotation_has_changed().then(|| event.rotation()),
        slider: event.slider_has_changed().then(|| event.slider_position()),
        wheel: event
            .wheel_has_changed()
            .then(|| (event.wheel_delta(), event.wheel_delta_discrete())),
    }
}

fn tablet_position<E>(data: &LoopData, event: &E) -> Option<(OutputId, (f64, f64))>
where
    E: AbsolutePositionEvent<LibinputInputBackend>,
{
    let core = data.state.core();
    let dev = Event::device(event);
    let candidates = screen_candidates(&data.screens);
    if let Some(screen_id) = select_input_screen(
        dev.output_name(),
        dev.size(),
        &candidates,
        pointer_output_id(core),
    ) {
        let screen = data.screens.iter().find(|s| s.id == screen_id)?;
        let model = core.output(screen.id)?;
        let p = touch_point_to_output_logical(
            (
                event.x_transformed(screen.size.0),
                event.y_transformed(screen.size.1),
            ),
            screen.size,
            model.transform,
            model.scale,
        );
        return Some((screen.id, p));
    }
    if dev.output_name().is_some_and(|s| !s.is_empty()) {
        return None;
    }
    let virtual_screen = data.virtual_screen.as_ref()?;
    let model = core.output(virtual_screen.id)?;
    let (view_w, view_h) = model.view_size();
    let x = event.x_transformed(view_w as i32);
    let y = event.y_transformed(view_h as i32);
    let scale = model.scale.max(0.001);
    Some((virtual_screen.id, (x / scale, y / scale)))
}

fn tablet_event(
    data: &LoopData,
    event: &InputEvent<LibinputInputBackend>,
) -> Option<infynitae_wayland::TabletEvent> {
    use smithay::wayland::tablet_manager::TabletDescriptor;
    Some(match event {
        InputEvent::TabletToolProximity { event } => match event.state() {
            ProximityState::In => {
                let (output, p) = tablet_position(data, event)?;
                let dev = Event::device(event);
                infynitae_wayland::TabletEvent::ProximityIn {
                    tablet: TabletDescriptor::from(&dev),
                    tool: TabletToolEvent::tool(event),
                    output,
                    p,
                    axes: tablet_axes(event),
                    time_us: Event::time(event),
                }
            }
            ProximityState::Out => infynitae_wayland::TabletEvent::ProximityOut {
                tool: TabletToolEvent::tool(event),
                time_us: Event::time(event),
            },
        },
        InputEvent::TabletToolAxis { event } => {
            let (output, p) = tablet_position(data, event)?;
            let dev = Event::device(event);
            infynitae_wayland::TabletEvent::Axis {
                tablet: TabletDescriptor::from(&dev),
                tool: TabletToolEvent::tool(event),
                output,
                p,
                axes: tablet_axes(event),
                time_us: Event::time(event),
            }
        }
        InputEvent::TabletToolTip { event } => {
            let (output, p) = tablet_position(data, event)?;
            let dev = Event::device(event);
            infynitae_wayland::TabletEvent::Tip {
                tablet: TabletDescriptor::from(&dev),
                tool: TabletToolEvent::tool(event),
                output,
                p,
                down: matches!(
                    TabletToolTipEvent::tip_state(event),
                    TabletToolTipState::Down
                ),
                axes: tablet_axes(event),
                time_us: Event::time(event),
            }
        }
        InputEvent::TabletToolButton { event } => infynitae_wayland::TabletEvent::Button {
            tool: TabletToolEvent::tool(event),
            button: infynitae_wayland::tablet_tool_button::<LibinputInputBackend>(event),
            pressed: matches!(
                TabletToolButtonEvent::button_state(event),
                ButtonState::Pressed
            ),
            time_us: Event::time(event),
        },
        _ => return None,
    })
}

fn core_tablet(data: &mut LoopData, ev: infynitae_wayland::TabletEvent) {
    let phys = data.state.tablet_event(ev);
    let port = &mut data.input_port;
    for p in phys {
        let _ = data
            .state
            .with_effects(|core, fx| core.physical_input(port, p, fx));
    }
}

fn update_keyboard_leds(
    state: &WaylandState,
    keyboards: &mut [smithay::reexports::input::Device],
    current: &mut smithay::reexports::input::Device,
) {
    use smithay::reexports::input::Led;
    let led_state = state.keyboard_led_state();
    let mut leds = Led::empty();
    if led_state.caps.unwrap_or(false) {
        leds |= Led::CAPSLOCK;
    }
    if led_state.num.unwrap_or(false) {
        leds |= Led::NUMLOCK;
    }
    if led_state.scroll.unwrap_or(false) {
        leds |= Led::SCROLLLOCK;
    }
    current.led_update(leds);
    for kb in keyboards.iter_mut() {
        kb.led_update(leds);
    }
}

/// Calculates the logical pointer delta needed to move from `current_p` to an absolute position
/// `(abs_x, abs_y)` transformed to screen pixels and normalized by `scale`.
pub fn calculate_abs_pointer_delta_scale(
    abs_px: (f64, f64),
    current_p: (f64, f64),
    scale: f64,
) -> (f64, f64) {
    let scale = scale.max(0.001);
    let target_logical = (abs_px.0 / scale, abs_px.1 / scale);
    (
        target_logical.0 - current_p.0,
        target_logical.1 - current_p.1,
    )
}

/// Calculates the logical pointer delta needed to move from `current_p` to an absolute position
/// `(abs_x, abs_y)` transformed to screen pixels and normalized by `scale_120`.
#[cfg(test)]
pub fn calculate_abs_pointer_delta(
    abs_px: (f64, f64),
    current_p: (f64, f64),
    scale_120: u32,
) -> (f64, f64) {
    let scale = (f64::from(scale_120) / 120.0).max(0.001);
    calculate_abs_pointer_delta_scale(abs_px, current_p, scale)
}

/// Resolves whether touchpad tap-to-click should be enabled.
/// Unset defaults to true (TOUCHPAD_TAP="true").
pub fn parse_lock_when_away(content: &str) -> bool {
    let mut lock_when_away = None;
    let mut legacy_idle = None;
    for (k, v) in content.lines().filter_map(infynitae_wayland::config_line) {
        match k.trim() {
            "LOCK_WHEN_AWAY" => {
                if let Some(b) = infynitae_wayland::parse_bool(v.trim()) {
                    lock_when_away = Some(b);
                }
            }
            "IDLE_AUTO_LOCK" => {
                if let Some(b) = infynitae_wayland::parse_bool(v.trim()) {
                    legacy_idle = Some(b);
                }
            }
            _ => {}
        }
    }
    lock_when_away.or(legacy_idle).unwrap_or(true)
}

pub fn resolve_touchpad_tap(configured: Option<bool>, env_val: Option<&str>) -> bool {
    configured
        .or_else(|| env_val.map(|t| infynitae_wayland::parse_bool(t).unwrap_or(false)))
        .unwrap_or(true)
}

pub(super) fn configure_input_device(
    device: &mut smithay::reexports::input::Device,
    config: &infynitae_wayland::InputDeviceConfig,
) {
    use smithay::reexports::input::AccelProfile;

    // 1. Mouse Acceleration Profile (flat = raw 1:1, adaptive = curve)
    let env_profile = std::env::var("MOUSE_ACCEL_PROFILE").ok();
    let profile = config
        .mouse_accel_profile
        .as_deref()
        .or(env_profile.as_deref());
    if let Some(profile_str) = profile {
        let desired = match profile_str.to_lowercase().as_str() {
            "flat" => Some(AccelProfile::Flat),
            "adaptive" => Some(AccelProfile::Adaptive),
            _ => None,
        };
        if let Some(p) = desired
            && device.config_accel_profiles().contains(&p)
        {
            let _ = device.config_accel_set_profile(p);
        }
    }

    // 2. Pointer Speed / Sensitivity (-1.0 to 1.0, 0.0 is default)
    let speed = config.mouse_accel_speed.or_else(|| {
        std::env::var("MOUSE_ACCEL_SPEED")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
    });
    if let Some(speed) = speed {
        let clamped = speed.clamp(-1.0, 1.0);
        let _ = device.config_accel_set_speed(clamped);
    }

    // 3. Touchpad Tap-to-click (default: true, config/config:64)
    let env_tap = std::env::var("TOUCHPAD_TAP").ok();
    let tap = resolve_touchpad_tap(config.touchpad_tap, env_tap.as_deref());
    if device.config_tap_finger_count() > 0 {
        let _ = device.config_tap_set_enabled(tap);
    }

    // 4. Touchpad Natural Scrolling
    let nat = config.touchpad_natural_scroll.or_else(|| {
        std::env::var("TOUCHPAD_NATURAL_SCROLL")
            .ok()
            .map(|t| infynitae_wayland::parse_bool(&t).unwrap_or(false))
    });
    if let Some(enable) = nat
        && device.config_scroll_has_natural_scroll()
    {
        let _ = device.config_scroll_set_natural_scroll_enabled(enable);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_touchpad_tap_defaults_to_true() {
        use super::resolve_touchpad_tap;
        assert!(resolve_touchpad_tap(None, None));
        assert!(resolve_touchpad_tap(Some(true), None));
        assert!(resolve_touchpad_tap(None, Some("true")));
        assert!(resolve_touchpad_tap(None, Some("1")));
        assert!(!resolve_touchpad_tap(Some(false), None));
        assert!(!resolve_touchpad_tap(None, Some("false")));
        assert!(!resolve_touchpad_tap(None, Some("0")));
        assert!(resolve_touchpad_tap(None, Some("YES")));
        assert!(resolve_touchpad_tap(None, Some("On")));
        assert!(!resolve_touchpad_tap(None, Some("NO")));
        assert!(!resolve_touchpad_tap(None, Some("invalid")));
        assert!(resolve_touchpad_tap(Some(true), Some("false")));
        assert!(!resolve_touchpad_tap(Some(false), Some("true")));
    }

    #[test]
    fn pointer_motion_absolute_delta_maps_to_target() {
        // 1.0x scale (120/120)
        let delta = calculate_abs_pointer_delta((100.0, 200.0), (960.0, 540.0), 120);
        assert_eq!(delta, (-860.0, -340.0));
        assert_eq!((960.0 + delta.0, 540.0 + delta.1), (100.0, 200.0));

        // 2.0x scale (240/120)
        let delta = calculate_abs_pointer_delta((1000.0, 800.0), (0.0, 0.0), 240);
        assert_eq!(delta, (500.0, 400.0));
    }

    #[test]
    fn pointer_motion_absolute_delta_maps_to_target_with_output_scale() {
        // 1.5x scale
        let delta = calculate_abs_pointer_delta_scale((1500.0, 900.0), (0.0, 0.0), 1.5);
        assert_eq!(delta, (1000.0, 600.0));

        // 2.0x scale
        let delta = calculate_abs_pointer_delta_scale((1920.0, 1080.0), (500.0, 300.0), 2.0);
        assert_eq!(delta, (460.0, 240.0));
    }

    #[test]
    fn parse_lock_when_away_prefers_unified_key_and_falls_back_to_idle_auto_lock() {
        use super::parse_lock_when_away;
        assert!(parse_lock_when_away(""));
        assert!(parse_lock_when_away("IDLE_AUTO_LOCK=\"true\"\n"));
        assert!(!parse_lock_when_away("IDLE_AUTO_LOCK=\"false\"\n"));
        assert!(!parse_lock_when_away(
            "LOCK_WHEN_AWAY=\"false\"\nIDLE_AUTO_LOCK=\"true\"\n"
        ));
        assert!(!parse_lock_when_away(
            "IDLE_AUTO_LOCK=\"true\"\nLOCK_WHEN_AWAY=\"false\"\n"
        ));
        assert!(!parse_lock_when_away(
            "LOCK_WHEN_AWAY=\"\"\nIDLE_AUTO_LOCK=\"false\"\n"
        ));
        assert!(parse_lock_when_away("IDLE_AUTO_LOCK=\"not-a-bool\"\n"));
    }

    #[test]
    #[ignore = "AUDIT_CONFIG: parse_lock_when_away ignores SLEEP_LOCK fallback read by shell helpers"]
    fn audit_config_lock_when_away_honors_sleep_lock_fallback() {
        assert!(!super::parse_lock_when_away("SLEEP_LOCK=\"false\"\n"));
    }

    #[test]
    fn touch_output_selection_prefers_udev_name_then_physical_size_then_internal_then_pointer() {
        let (mut core, _, _, mut token) = Core::new(Default::default());
        let make_output = |connector: &str| OutputDescriptor {
            key: OutputKey {
                device: "test".into(),
                connector: connector.into(),
                make: "Test".into(),
                model: "Test".into(),
                serial: None,
            },
            mode: (1920, 1080),
            transform: infynitae_core::output::Transform::Normal,
            scale_120: 120,
            refresh_mhz: 60_000,
        };
        let (edp, _) = core
            .output_connected(&mut token, make_output("eDP-1"))
            .unwrap();
        let (hdmi, _) = core
            .output_connected(&mut token, make_output("HDMI-A-1"))
            .unwrap();
        let (dp, _) = core
            .output_connected(&mut token, make_output("DP-1"))
            .unwrap();

        let laptop_and_ext = [
            InputScreenCandidate {
                id: edp,
                connector: "eDP-1",
                physical_size: (294, 165),
            },
            InputScreenCandidate {
                id: hdmi,
                connector: "HDMI-A-1",
                physical_size: (527, 296),
            },
        ];

        // 1. Explicit WL_OUTPUT (output_name) selects the second/external output over internal.
        assert_eq!(
            select_input_screen(Some("HDMI-A-1"), None, &laptop_and_ext, Some(edp)),
            Some(hdmi)
        );
        // Unconnected WL_OUTPUT does not leak touches to another output.
        assert_eq!(
            select_input_screen(Some("DP-2"), None, &laptop_and_ext, Some(edp)),
            None
        );

        // 2. Without WL_OUTPUT, physical size match picks the second output over internal eDP-1.
        assert_eq!(
            select_input_screen(None, Some((528.0, 295.0)), &laptop_and_ext, Some(edp)),
            Some(hdmi)
        );

        // 3. No WL_OUTPUT and no size match falls back to internal panel even if pointer is elsewhere.
        assert_eq!(
            select_input_screen(None, None, &laptop_and_ext, Some(hdmi)),
            Some(edp)
        );
        assert_eq!(
            select_input_screen(None, Some((100.0, 100.0)), &laptop_and_ext, Some(hdmi)),
            Some(edp)
        );

        // 4. Desktop with two external monitors: size picks second monitor even if pointer is on first.
        let two_ext = [
            InputScreenCandidate {
                id: dp,
                connector: "DP-1",
                physical_size: (600, 340),
            },
            InputScreenCandidate {
                id: hdmi,
                connector: "HDMI-A-1",
                physical_size: (344, 194),
            },
        ];
        assert_eq!(
            select_input_screen(None, Some((345.0, 193.0)), &two_ext, Some(dp)),
            Some(hdmi)
        );
        // Without size match or internal monitor, falls back to pointer output, then first.
        assert_eq!(
            select_input_screen(None, None, &two_ext, Some(hdmi)),
            Some(hdmi)
        );
        assert_eq!(select_input_screen(None, None, &two_ext, None), Some(dp));
    }

    #[test]
    fn touch_point_in_view_handles_rotation_and_scale() {
        let phys = (1920, 1080);
        // Normal at 1.5x scale: panel (300, 150) -> logical (200, 100).
        let p = touch_point_to_output_logical((300.0, 150.0), phys, Transform::Normal, 1.5);
        assert!((p.0 - 200.0).abs() < 1e-6 && (p.1 - 100.0).abs() < 1e-6);

        // R90 at 1.5x scale: view size is (1080, 1920) -> logical (720, 1280).
        // Panel (0, 1080) is view top-left (0, 0); panel (1920, 0) is view bottom-right.
        let tl = touch_point_to_output_logical((0.0, 1080.0), phys, Transform::R90, 1.5);
        assert!(tl.0.abs() < 1e-6 && tl.1.abs() < 1e-6, "tl={tl:?}");
        let br = touch_point_to_output_logical((1920.0, 0.0), phys, Transform::R90, 1.5);
        assert!(
            (br.0 - 720.0).abs() < 1e-6 && (br.1 - 1280.0).abs() < 1e-6,
            "br={br:?}"
        );

        // R180 at 2.0x scale: panel (1920, 1080) -> view (0, 0).
        let r180 = touch_point_to_output_logical((1920.0, 1080.0), phys, Transform::R180, 2.0);
        assert!(r180.0.abs() < 1e-6 && r180.1.abs() < 1e-6, "r180={r180:?}");

        // R270 at 1.0x scale: panel (1920, 0) -> view (0, 0).
        let r270 = touch_point_to_output_logical((1920.0, 0.0), phys, Transform::R270, 1.0);
        assert!(r270.0.abs() < 1e-6 && r270.1.abs() < 1e-6, "r270={r270:?}");
    }
}
