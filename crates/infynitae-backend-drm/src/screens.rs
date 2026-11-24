//! Screens: output configuration and display changes, the virtual screen, connector properties, hotplug and connecting a screen.

use super::*;

/// Carries out a display configuration (`wlr-output-management`): the new modes on the
/// hardware first, then Core's descriptors and positions. Everything or nothing: a mode the
/// hardware refuses, or a configuration Core refuses, sets every changed screen back.
pub(super) fn apply_output_configuration(data: &mut LoopData, changes: &[HeadChange]) -> bool {
    type Before = (
        usize,
        smithay::reexports::drm::control::Mode,
        smithay::utils::Transform,
    );
    let mut changed: Vec<Before> = Vec::new();
    let revert = |data: &mut LoopData, changed: &[Before]| {
        for &(i, old, old_transform) in changed {
            let s = &mut data.screens[i];
            let _ = s.compositor.use_mode(old);
            s.size = (i32::from(old.size().0), i32::from(old.size().1));
            s.transform = old_transform;
            s.compositor
                .set_output_mode_source(OutputModeSource::Static {
                    size: s.size.into(),
                    scale: smithay::utils::Scale::from(s.k),
                    transform: s.transform,
                });
        }
    };
    for c in changes {
        if c.mode.is_none() && c.transform.is_none() {
            continue;
        }
        let Some(i) = data.screens.iter().position(|s| s.id == c.output) else {
            revert(data, &changed);
            return false;
        };
        let s = &mut data.screens[i];
        let old = s.compositor.pending_mode();
        changed.push((i, old, s.transform));
        if let Some(m) = c.mode {
            let Some(&(_, mode)) = s.modes.iter().find(|(h, _)| *h == m) else {
                revert(data, &changed);
                return false;
            };
            if s.compositor.use_mode(mode).is_err() {
                revert(data, &changed);
                return false;
            }
            s.size = (i32::from(mode.size().0), i32::from(mode.size().1));
        }
        if let Some(t) = c.transform {
            s.transform = convert::output_transform(t);
        }
        s.compositor
            .set_output_mode_source(OutputModeSource::Static {
                size: s.size.into(),
                scale: smithay::utils::Scale::from(s.k),
                transform: s.transform,
            });
    }
    let Some(plan) = data.state.output_change_plan(changes) else {
        revert(data, &changed);
        return false;
    };
    for p in plan {
        if let Some(desc) = p.descriptor {
            match data
                .state
                .core_mut()
                .output_reconfigured(&mut data.output_token, p.output, desc)
            {
                Ok(fx) => data.state.apply(&fx),
                Err(_) => {
                    revert(data, &changed);
                    return false;
                }
            }
        }
        if let Some(pos) = p.position {
            match data
                .state
                .core_mut()
                .output_arranged(&mut data.output_token, p.output, pos)
            {
                Ok(fx) => data.state.apply(&fx),
                Err(_) => {
                    revert(data, &changed);
                    return false;
                }
            }
        }
    }
    // VRR last: nothing to undo. Allowed again, it is the config's mode, or while fullscreen
    for c in changes {
        if let Some(on) = c.adaptive_sync
            && let Some(s) = data.screens.iter_mut().find(|s| s.id == c.output)
        {
            s.vrr_mode = if on {
                data.vrr_mode.filter(|&v| v)
            } else {
                Some(false)
            };
        }
    }
    data.session.request_frame();
    true
}

/// The config's screen settings changed while running (`infynitae_wayland::display_settings`):
/// the screens connected now take their new modes, turns, scales and order as a display settings
/// tool's configuration; screens connected later, VRR and the virtual screen follow the new values.
pub(super) fn apply_display_change(data: &mut LoopData, change: &DisplayChange) {
    let (before, now) = (&change.before, &change.now);
    if now.scale_120 != before.scale_120 || now.screen_scales != before.screen_scales {
        data.scale_120 = now.scale_120.unwrap_or(120);
        data.factory.scale_120 = data.scale_120;
        data.factory.custom_scales = now
            .screen_scales
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
    }
    data.factory.custom_modes = now.screen_modes.clone().into_iter().collect();
    data.factory.custom_rates = now.screen_rates.clone().into_iter().collect();
    data.factory.custom_transforms = now.screen_transforms.clone().into_iter().collect();
    let heads = data.state.display_head_changes(change);
    if !heads.is_empty() && !apply_output_configuration(data, &heads) {
        log_line!(
            "[infynitae] The config's screen modes, turns, scales or order were refused; the screens stay as they were"
        );
    }
    if now.vrr != before.vrr {
        data.vrr_mode = now.vrr;
        data.factory.vrr = now.vrr;
        for s in &mut data.screens {
            s.vrr_mode = now.vrr;
            if s.vrr_supported {
                data.state
                    .set_output_adaptive_sync(s.id, now.vrr != Some(false));
            }
        }
        data.session.request_frame();
    }
    if now.virtual_screen != before.virtual_screen {
        data.virtual_screen_enabled = now.virtual_screen;
        update_virtual_screen(data);
    }
}

/// The virtual screen's mode: the common 1080p, at 60 Hz.
pub const VIRTUAL_SCREEN_SIZE: (i32, i32) = (1920, 1080);

/// Whether the virtual screen comes (`Some(true)`) or goes (`Some(false)`): it is there exactly
/// while it is enabled and no real screen is connected.
pub fn virtual_screen_change(enabled: bool, real_screens: usize, present: bool) -> Option<bool> {
    let wanted = enabled && real_screens == 0;
    (wanted != present).then_some(wanted)
}

/// The virtual screen comes or goes with the real ones: it comes when the last is gone, and goes
/// once a real one is connected (after it, so windows and the pointer have somewhere to be).
pub(super) fn update_virtual_screen(data: &mut LoopData) {
    match virtual_screen_change(
        data.virtual_screen_enabled,
        data.screens.len(),
        data.virtual_screen.is_some(),
    ) {
        Some(true) => {
            let descriptor = OutputDescriptor {
                key: OutputKey {
                    device: "virtual".into(),
                    connector: "VIRTUAL-1".into(),
                    make: "Infynitae".into(),
                    model: "Virtual screen".into(),
                    serial: None,
                },
                mode: (VIRTUAL_SCREEN_SIZE.0 as u32, VIRTUAL_SCREEN_SIZE.1 as u32),
                transform: Transform::Normal,
                scale_120: data.scale_120,
                refresh_mhz: 60_000,
            };
            match data
                .state
                .core_mut()
                .output_connected(&mut data.output_token, descriptor)
            {
                Ok((id, fx)) => {
                    data.state.apply(&fx);
                    data.virtual_screen = Some(VirtualScreen {
                        id,
                        screencopy_texture: None,
                    });
                    log_line!(
                        "[infynitae] No screen connected: a virtual {}x{} screen stands in (VIRTUAL_SCREEN)",
                        VIRTUAL_SCREEN_SIZE.0,
                        VIRTUAL_SCREEN_SIZE.1
                    );
                    data.session.request_frame();
                }
                Err(e) => log_line!("[infynitae] The virtual screen could not be set up: {e:?}"),
            }
        }
        Some(false) => {
            if let Some(v) = data.virtual_screen.take() {
                match data
                    .state
                    .core_mut()
                    .output_disconnected(&mut data.output_token, v.id)
                {
                    Ok(fx) => data.state.apply(&fx),
                    Err(e) => {
                        log_line!("[infynitae] The virtual screen could not be removed: {e:?}")
                    }
                }
                log_line!("[infynitae] A screen is connected: the virtual screen is gone");
            }
        }
        None => {}
    }
}

/// Reads a single named property from a DRM connector and parses its converted value.
fn find_connector_property<T>(
    drm: &impl Device,
    handle: connector::Handle,
    prop_name: &[u8],
    parse: impl FnOnce(property::Value<'_>) -> Option<T>,
) -> Option<T> {
    let props = drm.get_properties(handle).ok()?;
    let (prop_ids, prop_vals) = props.as_props_and_values();
    for (&prop_id, &val) in prop_ids.iter().zip(prop_vals) {
        let Ok(info) = drm.get_property(prop_id) else {
            continue;
        };
        if info.name().to_bytes() == prop_name {
            return parse(info.value_type().convert_value(val));
        }
    }
    None
}

/// Reads the DRM connector `"panel orientation"` enum property name, if present.
fn read_connector_panel_orientation(
    drm: &impl Device,
    handle: connector::Handle,
) -> Option<String> {
    find_connector_property(drm, handle, b"panel orientation", |val| match val {
        property::Value::Enum(Some(ev)) => ev.name().to_str().ok().map(str::to_owned),
        _ => None,
    })
}

/// Reads and parses the EDID of a connector, if present and valid.
fn read_connector_edid(drm: &impl Device, handle: connector::Handle) -> Option<edid::Edid> {
    find_connector_property(drm, handle, b"EDID", |val| {
        let property::Value::Blob(blob_id) = val else {
            return None;
        };
        if blob_id == 0 {
            return None;
        }
        let blob_data = drm.get_property_blob(blob_id).ok()?;
        edid::parse(&blob_data)
    })
}

/// How long a display change must be quiet before the connectors are compared with the screens:
/// a monitor waking or a cable being seated reports several changes, and a monitor that drops
/// its connection for a moment in deep sleep must not lose its screen.
pub(super) const HOTPLUG_SETTLE: Duration = Duration::from_millis(1500);

/// The screens to remove: those whose connector is not among `connected`. A screen turned off
/// (`power_off`, or all of them `asleep`: idle sleep) can read as disconnected and is kept; it is
/// checked again once woken. Pure.
fn screens_gone<I: Copy, C: PartialEq>(
    screens: impl Iterator<Item = (I, C, bool)>,
    connected: &[C],
    asleep: bool,
) -> Vec<I> {
    if asleep {
        return Vec::new();
    }
    screens
        .filter(|(_, c, power_off)| !power_off && !connected.contains(c))
        .map(|(id, _, _)| id)
        .collect()
}

/// A laptop's own panel (eDP, LVDS, DSI), which its lid covers.
fn is_internal_panel(i: connector::Interface) -> bool {
    matches!(
        i,
        connector::Interface::EmbeddedDisplayPort
            | connector::Interface::LVDS
            | connector::Interface::DSI
    )
}

/// Which connected connectors (`internal`: each is a laptop panel) are shown: under a closed lid
/// the panels are left out, but only while another screen is connected, so the session never
/// ends up with none (with no other screen, logind suspends the machine). Pure.
fn lid_shows(lid_closed: bool, internal: &[bool]) -> Vec<bool> {
    let other = internal.iter().any(|i| !i);
    internal
        .iter()
        .map(|i| !(lid_closed && other && *i))
        .collect()
}

/// Hotplug: a screen whose connector is no longer connected is removed (Core disconnects its
/// output: its camera is kept for its return, its windows stay on the canvas), and a newly
/// connected connector becomes a screen.
pub(super) fn rescan_screens(data: &mut LoopData) {
    let Ok(resources) = data.drm.resource_handles() else {
        return;
    };
    let mut connected: Vec<(connector::Handle, connector::Info)> = resources
        .connectors()
        .iter()
        .filter_map(|&h| data.drm.get_connector(h, true).ok().map(|c| (h, c)))
        .filter(|(_, c)| c.state() == connector::State::Connected)
        .collect();
    let internal: Vec<bool> = connected
        .iter()
        .map(|(_, c)| is_internal_panel(c.interface()))
        .collect();
    let shown = lid_shows(data.lid_closed, &internal);
    let mut i = 0;
    connected.retain(|_| {
        i += 1;
        shown[i - 1]
    });
    let handles: Vec<connector::Handle> = connected.iter().map(|(h, _)| *h).collect();
    let gone = screens_gone(
        data.screens
            .iter()
            .map(|s| (s.id, s.connector, s.power_off)),
        &handles,
        data.dpms_active,
    );
    for id in gone {
        data.screens.retain(|s| s.id != id);
        if data.touch_slots.iter().any(|(_, _, o)| *o == id) {
            data.touch_slots.clear();
        }
        data.gamma_screens.lock().unwrap().remove(&id);
        match data
            .state
            .core_mut()
            .output_disconnected(&mut data.output_token, id)
        {
            Ok(fx) => data.state.apply(&fx),
            Err(e) => log_line!("[infynitae] Screen {id:?} could not be disconnected: {e:?}"),
        }
        log_line!("[infynitae] Screen {id:?} disconnected");
    }
    for (handle, conn) in connected {
        if data.screens.iter().any(|s| s.connector == handle) {
            continue;
        }
        match connect_screen(
            &data.factory,
            &mut data.drm,
            &mut data.state,
            &mut data.output_token,
            &data.gamma_screens,
            &data.screens,
            handle,
            &conn,
        ) {
            Ok(Some(screen)) => {
                log_line!("[infynitae] Screen {:?} connected", screen.id);
                data.screens.push(screen);
            }
            Ok(None) => {}
            Err(e) => log_line!("[infynitae] A connected screen could not be set up: {e}"),
        }
    }
    update_virtual_screen(data);
}

/// What setting up a screen needs, kept for screens connected later (hotplug).
pub(super) struct ScreenFactory {
    pub(super) gpu: PathBuf,
    pub(super) gbm: GbmDevice<DrmDeviceFd>,
    pub(super) allocator: GbmAllocator<DrmDeviceFd>,
    pub(super) exporter: GbmFramebufferExporter<DrmDeviceFd>,
    pub(super) render_formats: Vec<smithay::backend::allocator::Format>,
    /// `OUTPUT_<NAME>_MODE`, `_RATE` and `_SCALE`, by lowercased connector name.
    pub(super) custom_modes: HashMap<String, (u16, u16)>,
    pub(super) custom_rates: HashMap<String, u32>,
    pub(super) custom_scales: HashMap<String, u32>,
    pub(super) custom_transforms: HashMap<String, Transform>,
    pub(super) scale_120: u32,
    pub(super) vrr: Option<bool>,
}

/// Sets up the connected connector `handle` as a screen: its mode (the config's, else the
/// preferred one), a CRTC no screen of `screens` uses, its Core output, compositor, modes for
/// display settings tools and gamma ramp. `None` when it has no usable mode or no free CRTC.
#[allow(clippy::too_many_arguments)]
pub(super) fn connect_screen(
    f: &ScreenFactory,
    drm: &mut DrmDevice,
    state: &mut WaylandState,
    token: &mut OutputLifecycleToken,
    gamma_screens: &Mutex<HashMap<OutputId, DrmScreenGamma>>,
    screens: &[Screen],
    handle: connector::Handle,
    conn: &connector::Info,
) -> Result<Option<Screen>, Box<dyn Error>> {
    let resources = drm.resource_handles()?;
    let used: Vec<crtc::Handle> = screens.iter().map(|s| s.crtc).collect();
    let name = format!("{}-{}", conn.interface().as_str(), conn.interface_id());
    let debug_name = format!("{:?}-{}", conn.interface(), conn.interface_id());
    let n_lower = name.to_lowercase();
    let dbg_lower = debug_name.to_lowercase();
    let target_size = f
        .custom_modes
        .get(&n_lower)
        .or_else(|| f.custom_modes.get(&dbg_lower))
        .copied();
    let target_rate = f
        .custom_rates
        .get(&n_lower)
        .or_else(|| f.custom_rates.get(&dbg_lower))
        .copied();
    let conn_scale = f
        .custom_scales
        .get(&n_lower)
        .or_else(|| f.custom_scales.get(&dbg_lower))
        .copied()
        .unwrap_or(f.scale_120);
    let custom_transform = f
        .custom_transforms
        .get(&n_lower)
        .or_else(|| f.custom_transforms.get(&dbg_lower))
        .copied();
    let panel_orientation = if custom_transform.is_none() {
        read_connector_panel_orientation(&*drm, handle)
    } else {
        None
    };
    let transform = resolve_output_transform(custom_transform, panel_orientation.as_deref());
    let draw_transform = convert::output_transform(transform);

    let infos: Vec<ModeInfo> = conn
        .modes()
        .iter()
        .map(|m| ModeInfo {
            size: m.size(),
            refresh_mhz: refresh_mhz(m.clock(), m.hsync().2, m.vsync().2, m.vrefresh()),
            preferred: m.mode_type().contains(ModeTypeFlags::PREFERRED),
        })
        .collect();
    let mode_idx = choose_mode_custom(&infos, target_size, target_rate).or_else(|| {
        if target_size.is_some() || target_rate.is_some() {
            log_line!(
                "[infynitae] Screen {name}: requested custom mode {:?} @ {:?} not found, falling back to preferred mode",
                target_size, target_rate
            );
            choose_mode(&infos)
        } else {
            None
        }
    });
    let Some(mode) = mode_idx.map(|i| conn.modes()[i]) else {
        return Ok(None);
    };
    let crtc = conn
        .encoders()
        .iter()
        .filter_map(|e| drm.get_encoder(*e).ok())
        .flat_map(|e| resources.filter_crtcs(e.possible_crtcs()))
        .find(|c| !used.contains(c));
    let Some(crtc) = crtc else {
        log_line!("[infynitae] No free CRTC for {:?}", conn.interface());
        return Ok(None);
    };
    let (w, h) = mode.size();
    let refresh = refresh_mhz(
        mode.clock(),
        mode.hsync().2,
        mode.vsync().2,
        mode.vrefresh(),
    );
    log_line!(
        "[infynitae] Screen {name}: {w}x{h} @ {}.{:03} Hz (scale: {:.2}x)",
        refresh / 1000,
        refresh % 1000,
        conn_scale as f64 / 120.0
    );
    let surface = drm.create_surface(crtc, mode, &[handle])?;
    let (make, model, serial) = match read_connector_edid(&*drm, handle) {
        Some(e) => (e.make, e.model, e.serial),
        None => ("Unknown".into(), "Unknown".into(), None),
    };
    let descriptor = OutputDescriptor {
        key: OutputKey {
            device: f.gpu.display().to_string(),
            connector: name.clone(),
            make,
            model,
            serial,
        },
        mode: (u32::from(w), u32::from(h)),
        transform,
        scale_120: conn_scale,
        refresh_mhz: refresh,
    };
    let (id, fx) = state
        .core_mut()
        .output_connected(token, descriptor)
        .map_err(|e| format!("output {name}: {e:?}"))?;
    state.apply(&fx);
    let k = state.core().view(id).map_or(1.0, |v| v.k);
    let size = (i32::from(w), i32::from(h));
    let mut compositor = Compositor::new(
        OutputModeSource::Static {
            size: size.into(),
            scale: smithay::utils::Scale::from(k),
            transform: draw_transform,
        },
        surface,
        None,
        f.allocator.clone(),
        f.exporter.clone(),
        [Fourcc::Xrgb8888, Fourcc::Argb8888],
        f.render_formats.iter().copied(),
        drm.cursor_size(),
        Some(f.gbm.clone()),
    )
    .map_err(|e| format!("screen {name}: {e:?}"))?;

    state.add_dmabuf_scanout_formats(compositor.surface().plane_info().formats.iter().copied());

    let vrr_supported = compositor
        .vrr_supported(handle)
        .is_ok_and(|s| !matches!(s, smithay::backend::drm::VrrSupport::NotSupported));
    if !vrr_supported {
        log_line!(
            "[infynitae] Screen {name}: no VRR (FreeSync off on the monitor, or not supported)"
        );
    }
    let mut vrr_on = false;
    match (vrr_supported, f.vrr) {
        (false, _) => {}
        (true, Some(true)) => {
            vrr_on = compositor.use_vrr(true).is_ok();
            log_line!("[infynitae] Screen {name}: VRR always on");
        }
        (true, Some(false)) => log_line!("[infynitae] Screen {name}: VRR off"),
        (true, None) => log_line!("[infynitae] Screen {name}: VRR while a window is fullscreen"),
    }
    let physical_size = conn.size().unwrap_or((0, 0));
    let screen = Screen {
        id,
        connector: handle,
        connector_name: name.clone(),
        physical_size,
        crtc,
        size,
        compositor,
        k,
        transform: draw_transform,
        pending: false,
        queued_locked: false,
        shows_locked: false,
        screencopy_texture: None,
        flip_failures: 0,
        no_scanout_until: None,
        tearing_refused: 0,
        tearing_retry_at: None,
        tearing_logged: false,
        last_tearing_game: None,
        tearing_refusals_logged: Vec::new(),
        vrr_supported,
        vrr_on,
        vrr_mode: f.vrr,
        power_off: false,
        scanned_out: false,
        empty_since_refresh: false,
        schedule: infynitae_wayland::frame_schedule::FrameSchedule::new(
            infynitae_wayland::frame_schedule::LatencyMode::Balanced,
            std::time::Duration::from_nanos(1_000_000_000_000 / u64::from(refresh.max(1))),
        ),
        deferred: None,
        relief: Default::default(),
        relief_on: false,
        latency: Default::default(),
        scanout_logged: None,
        modes: conn
            .modes()
            .iter()
            .map(|m| {
                let refresh = refresh_mhz(m.clock(), m.hsync().2, m.vsync().2, m.vrefresh());
                let head = HeadMode {
                    size: (i32::from(m.size().0), i32::from(m.size().1)),
                    refresh_mhz: refresh as i32,
                    preferred: m.mode_type().contains(ModeTypeFlags::PREFERRED),
                };
                (head, *m)
            })
            .collect(),
    };
    // VRR as tools see it: allowed (always on, or while a window is fullscreen) or off
    let (mm_w, mm_h) = physical_size;
    state.set_output_head_info(
        id,
        HeadInfo {
            modes: screen.modes.iter().map(|(h, _)| *h).collect(),
            physical_size: Some((mm_w as i32, mm_h as i32)),
            adaptive_sync: vrr_supported.then_some(f.vrr != Some(false)),
        },
    );

    // Initialize DRM gamma LUT for this screen
    if let Ok(crtc_info) = drm.get_crtc(crtc) {
        let gamma_len = crtc_info.gamma_length();
        if gamma_len > 0 {
            let sz = gamma_len as usize;
            let mut r = vec![0u16; sz];
            let mut g = vec![0u16; sz];
            let mut b = vec![0u16; sz];
            if drm.get_gamma(crtc, &mut r, &mut g, &mut b).is_err() {
                for i in 0..sz {
                    let val = (i as u32 * 65535 / (sz - 1).max(1) as u32) as u16;
                    r[i] = val;
                    g[i] = val;
                    b[i] = val;
                }
            }
            gamma_screens.lock().unwrap().insert(
                id,
                DrmScreenGamma {
                    crtc,
                    gamma_size: gamma_len,
                    original_gamma: (r, g, b),
                    current_gamma: None,
                },
            );
        }
    }
    Ok(Some(screen))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_lid_leaves_the_panel_out_only_beside_another_screen() {
        assert_eq!(
            lid_shows(true, &[true, false]),
            [false, true],
            "panel out, monitor kept"
        );
        assert_eq!(
            lid_shows(true, &[true]),
            [true],
            "alone: kept (logind suspends)"
        );
        assert_eq!(lid_shows(false, &[true, false]), [true, true], "open: both");
        assert_eq!(
            lid_shows(true, &[false, false]),
            [true, true],
            "no panel: nothing to hide"
        );
        assert!(is_internal_panel(connector::Interface::EmbeddedDisplayPort));
        assert!(!is_internal_panel(connector::Interface::HDMIA));
    }

    #[test]
    fn a_screen_is_gone_only_when_awake_and_disconnected() {
        let screens = [(1, 'a', false), (2, 'b', false), (3, 'c', true)];
        assert_eq!(
            screens_gone(screens.into_iter(), &['a'], false),
            [2],
            "b is unplugged; c is off, so it may only read as disconnected"
        );
        assert!(
            screens_gone(screens.into_iter(), &[], true).is_empty(),
            "asleep: nothing is removed"
        );
        assert!(screens_gone(screens.into_iter(), &['a', 'b', 'c'], false).is_empty());
    }

    #[test]
    fn the_virtual_screen_stands_in_only_while_no_screen_is_connected() {
        // no screen (a monitor off at boot, or unplugged): it comes
        assert_eq!(virtual_screen_change(true, 0, false), Some(true));
        assert_eq!(virtual_screen_change(true, 0, true), None);
        // a screen is connected: it goes, and stays away
        assert_eq!(virtual_screen_change(true, 1, true), Some(false));
        assert_eq!(virtual_screen_change(true, 2, false), None);
        // VIRTUAL_SCREEN=off: never
        assert_eq!(virtual_screen_change(false, 0, false), None);
        assert_eq!(virtual_screen_change(false, 0, true), Some(false));
    }

    #[test]
    fn the_virtual_screen_is_a_screen_core_takes() {
        let d = OutputDescriptor {
            key: OutputKey {
                device: "virtual".into(),
                connector: "VIRTUAL-1".into(),
                make: "Infynitae".into(),
                model: "Virtual screen".into(),
                serial: None,
            },
            mode: (VIRTUAL_SCREEN_SIZE.0 as u32, VIRTUAL_SCREEN_SIZE.1 as u32),
            transform: Transform::Normal,
            scale_120: 120,
            refresh_mhz: 60_000,
        };
        assert!(infynitae_core::output::valid(&d));
    }
}
