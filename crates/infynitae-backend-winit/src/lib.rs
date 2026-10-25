//! Infynitae Native nested development backend (winit window, its input and GL target)
//! (FOUNDATION_ARCHITECTURE C1.1).

#![deny(unsafe_code)]

use std::collections::HashSet;
use std::error::Error;
use std::path::PathBuf;
use std::time::Duration;

use infynitae_core::output::{OutputDescriptor, OutputId, OutputKey, Transform};
use infynitae_core::{Core, InputPort, OutputLifecycleToken, PhysicalEvent};
use infynitae_wayland::session::{Session, SessionOptions};
use infynitae_wayland::{WaylandState, convert};

use smithay::backend::input::{
    AbsolutePositionEvent, ButtonState, Event, InputEvent, KeyState, KeyboardKeyEvent,
    PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::{Renderer, TextureFilter};
use smithay::backend::winit::{self, WinitEvent, WinitGraphicsBackend};
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::Display;
use smithay::reexports::winit::dpi::LogicalSize;
use smithay::reexports::winit::window::Window as WinitWindow;

/// Options for configuring the nested winit compositor.
#[derive(Clone, Debug)]
pub struct WinitCompositorOptions {
    /// Wayland socket name (e.g. "wayland-1"). If None, automatically selects an available socket.
    pub socket_name: Option<String>,
    /// Optional command to execute on compositor startup (e.g. "weston-terminal").
    pub exec: Option<String>,
    /// Window title for the nested compositor window.
    pub title: String,
    /// Initial window width.
    pub width: u32,
    /// Initial window height.
    pub height: u32,
    /// Target output refresh rate in mHz (default: 60_000 mHz = 60 Hz).
    pub refresh_mhz: u32,
    /// Optional maximum run duration (useful for automated testing / benchmarks).
    pub duration: Option<Duration>,
    /// Optional maximum frame count before clean exit.
    pub max_frames: Option<u64>,
    /// Path to xwayland-satellite to supervise (M19); `None`: no X11.
    pub xwayland_satellite: Option<std::path::PathBuf>,
    /// Output scale ×120 for every screen (`None`: auto/120, i.e. 1.0).
    pub scale_120: Option<u32>,
    /// Scaling texture filter ("nearest" for sharp pixels, "linear" for smooth).
    pub scaling_filter: Option<String>,
}

impl Default for WinitCompositorOptions {
    fn default() -> Self {
        Self {
            socket_name: None,
            exec: None,
            title: "Infynitae Native Compositor".to_string(),
            width: 1280,
            height: 800,
            refresh_mhz: 60_000,
            duration: None,
            max_frames: None,
            xwayland_satellite: None,
            scale_120: None,
            scaling_filter: None,
        }
    }
}

/// Calculates the frame duration for a given refresh rate in mHz.
pub fn vblank_period(refresh_mhz: u32) -> Duration {
    Duration::from_nanos(1_000_000_000_000 / u64::from(refresh_mhz.max(1)))
}

/// Internal event-loop data structure passed to calloop.
struct LoopData {
    state: WaylandState,
    session: Session,
    input_port: InputPort,
    output_token: OutputLifecycleToken,
    output_id: OutputId,
    output_key: OutputKey,
    backend: WinitGraphicsBackend<GlesRenderer>,
    last_abs_pos: Option<(f64, f64)>,
    scale_120: u32,
    /// The host asked to close the window.
    closed: bool,
    options: WinitCompositorOptions,
    /// Whether the host is ready for the next frame (NB-1).
    host: HostFrameGate,
    /// Set of known mapped element IDs for detecting newly mapped windows.
    known_mapped: HashSet<infynitae_core::stage::ElementId>,
    config_path: Option<PathBuf>,
    last_config_check: std::time::Instant,
    last_config_mtime: Option<std::time::SystemTime>,
}

/// NB-1 (2026-10-17): a submit swaps with the EGL default swap interval, so it waits for the host
/// compositor's frame callback of the previous one. A host that stops presenting the window
/// (minimised, hidden) stops sending frame callbacks, and a submit then blocks the whole loop —
/// clients, the run limit and SIGTERM — for as long as the window stays hidden. So a frame is
/// submitted only when the host is ready for it: initially, and after winit reports that the
/// host's frame callback for the previous submit arrived (`WinitEvent::Redraw`, which winit
/// delivers on that callback once a redraw is requested). While the host is not ready a due
/// frame waits; the loop sleeps on its event sources. The swap interval is not changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HostFrameGate {
    ready: bool,
}

impl HostFrameGate {
    fn new() -> HostFrameGate {
        HostFrameGate { ready: true }
    }

    /// Whether a frame may be submitted now.
    fn may_submit(&self) -> bool {
        self.ready
    }

    /// A frame was submitted: the next one waits for the host.
    fn submitted(&mut self) {
        self.ready = false;
    }

    /// The host's frame callback arrived (`WinitEvent::Redraw`).
    fn host_ready(&mut self) {
        self.ready = true;
    }
}

/// The nested backend reports an assumed device limit rather than querying the GLES renderer
/// (`GL_MAX_TEXTURE_SIZE`; OB-52). The value is the limit recorded for the reference device
/// (EC-16). If the host GPU has a lower limit, importing a buffer Core allowed may fail.
struct AssumedGlesLimits;

impl infynitae_wayland::limits::RendererLimits for AssumedGlesLimits {
    fn max_buffer_side(&self) -> Result<u32, infynitae_wayland::limits::LimitQueryError> {
        Ok(infynitae_core::scale::DEFAULT_D_MAX)
    }
}

/// Runs the nested compositor with Winit backend.
///
/// Takes ownership of `Core` and the composition root's authority ports (`InputPort`,
/// `OutputLifecycleToken`), wires the nested window, the session, the renderer and the event
/// loop, and blocks until the window is closed or a run limit is reached.
///
/// Frames are paced by the session's frame timer at the configured refresh rate. The host
/// compositor is the display: a submit may wait for it, and when the picture reaches a screen is
/// not known here, so presentation feedback carries the time of the submit and no flags.
pub fn run_nested(
    core: Core,
    input_port: InputPort,
    output_token: OutputLifecycleToken,
    options: WinitCompositorOptions,
) -> Result<(), Box<dyn Error>> {
    run_nested_with(core, input_port, output_token, options, None)
}

/// [`run_nested`] with a loop extension (such as the shell server): it runs after every dispatch,
/// before the clients are served, so its effects reach them with that dispatch's flush.
pub fn run_nested_with(
    core: Core,
    input_port: InputPort,
    mut output_token: OutputLifecycleToken,
    options: WinitCompositorOptions,
    extension: Option<infynitae_wayland::extension::ExtensionFactory>,
) -> Result<(), Box<dyn Error>> {
    let mut event_loop: EventLoop<LoopData> = EventLoop::try_new()?;

    let display: Display<WaylandState> = Display::new()?;
    let mut state = WaylandState::new(&display.handle(), core);
    state.set_host_window_target();

    // Before the window: from here on SIGTERM/SIGINT end the loop instead of the process, so a
    // signal during the rest of start-up still leaves no socket behind.
    let x_display =
        infynitae_wayland::xwayland::reserve_for_session(options.xwayland_satellite.as_deref())?;
    let session = Session::new(
        display,
        &event_loop.handle(),
        &SessionOptions {
            socket_name: options.socket_name.clone(),
            exec: options.exec.clone(),
            refresh_mhz: options.refresh_mhz,
            duration: options.duration,
            max_frames: options.max_frames,
            x_display: x_display.as_ref().map(|d| d.name()),
            x_authority: x_display
                .as_ref()
                .and_then(|d| d.authority_path().map(|p| p.to_path_buf())),
        },
    )?;
    // after Session::new: from here a SIGTERM or SIGINT ends the loop, so what the extension
    // creates (the shell socket) is removed even when the signal comes during start-up
    let mut extension = infynitae_wayland::extension::start(extension, &event_loop.handle());
    let mut xwayland = match (x_display, options.xwayland_satellite.as_deref()) {
        (Some(d), Some(satellite)) => Some(infynitae_wayland::xwayland::start_for_session(
            d,
            satellite,
            session.socket_name(),
            &event_loop.handle(),
        )?),
        _ => None,
    };

    let window_attributes = WinitWindow::default_attributes()
        .with_inner_size(LogicalSize::new(
            options.width as f64,
            options.height as f64,
        ))
        .with_title(&options.title)
        .with_visible(true);
    let (mut backend, winit_event_loop) =
        winit::init_from_attributes::<GlesRenderer>(window_attributes)?;

    // Scaling filter: nearest-neighbor filtering on upscaled surfaces by default
    let use_nearest = !matches!(options.scaling_filter.as_deref(), Some("linear"));
    if use_nearest && let Err(e) = backend.renderer().upscale_filter(TextureFilter::Nearest) {
        eprintln!("[infynitae] Warning: failed to set upscale filter to Nearest: {e:?}");
    }

    let output_key = OutputKey {
        device: "winit".into(),
        connector: "WINIT-0".into(),
        make: "Infynitae".into(),
        model: "Nested".into(),
        serial: None,
    };
    let scale_120 = options.scale_120.unwrap_or(120);
    let descriptor = OutputDescriptor {
        key: output_key.clone(),
        mode: (options.width, options.height),
        transform: Transform::Normal,
        scale_120,
        refresh_mhz: options.refresh_mhz,
    };
    let (output_id, effects) = state
        .core_mut()
        .output_connected(&mut output_token, descriptor)
        .map_err(|e| format!("Failed to connect output: {e:?}"))?;
    state.apply(&effects);

    // Device ceiling D_max (OB-13, F-SCALE-2, OBJ-29), through the renderer-limit interface.
    // This backend reports an assumed limit without querying the GLES renderer (OB-52).
    state.report_renderer_limits(&AssumedGlesLimits)?;

    let cfg_path = std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(format!("{h}/.config/infynitae/config")));
    let initial_mtime = cfg_path
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok());

    let mut data = LoopData {
        state,
        session,
        input_port,
        output_token,
        output_id,
        output_key,
        backend,
        last_abs_pos: None,
        scale_120,
        closed: false,
        options,
        host: HostFrameGate::new(),
        known_mapped: HashSet::new(),
        config_path: cfg_path,
        last_config_check: std::time::Instant::now(),
        last_config_mtime: initial_mtime,
    };

    data.state.reload_config();

    // every host event (input, resize, redraw request) may change what is shown
    event_loop
        .handle()
        .insert_source(winit_event_loop, |event, _, data| {
            handle_winit_event(data, event);
            data.session.request_frame();
        })
        .map_err(|e| format!("Failed to register winit event source: {e}"))?;

    println!("[infynitae] Compositor running. Close the window to exit.");
    while !data.closed && !data.session.finished() {
        // An open scale-settlement deadline (INV-SCALE-3) wakes the loop by itself; none is
        // armed while no epoch is open (INV-IDLE-1).
        data.session
            .wake_at(data.state.core().next_settlement_deadline());
        let restart = xwayland
            .as_mut()
            .and_then(|x| x.poll(std::time::Instant::now()));
        let timeout = match (data.session.timeout(), restart) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        event_loop.dispatch(timeout, &mut data)?;
        // Watches ~/.config/infynitae/config for changes and reloads settings
        if data.last_config_check.elapsed() >= Duration::from_millis(250) {
            data.last_config_check = std::time::Instant::now();
            if let Some(path) = &data.config_path
                && let Ok(meta) = std::fs::metadata(path)
                && let Ok(mtime) = meta.modified()
            {
                if data.last_config_mtime.is_some() && data.last_config_mtime != Some(mtime) {
                    data.last_config_mtime = Some(mtime);
                    println!(
                        "[infynitae] Config change detected (~/.config/infynitae/config). Live reloading..."
                    );
                    data.state.reload_config();
                    data.session.request_frame();
                } else if data.last_config_mtime.is_none() {
                    data.last_config_mtime = Some(mtime);
                }
            }
        }
        if let Some(ext) = extension.as_mut() {
            let (fx, changed) = ext.service(data.state.core_mut());
            data.state.apply(&fx);
            if changed {
                data.session.request_frame();
            }
        }
        data.session.dispatch(&mut data.state);

        let current_mapped = data.state.core().stage().mapped_elements();
        let new_mapped: Vec<_> = current_mapped
            .iter()
            .copied()
            .filter(|id| !data.known_mapped.contains(id))
            .collect();

        data.known_mapped.clear();
        data.known_mapped.extend(current_mapped);

        if data.state.core().focus().is_initial_eligible() {
            if let Some(&topmost) = data.state.core().stage().mapped_elements().last()
                && let Ok(effects) = data
                    .state
                    .core_mut()
                    .grant_initial_focus(&mut data.input_port, topmost)
            {
                data.state.apply(&effects);
                data.session.flush();
            }
        } else if data.state.core().focus_on_open() {
            for &new_id in &new_mapped {
                if let Ok(effects) = data
                    .state
                    .core_mut()
                    .set_focus(&mut data.input_port, Some(new_id))
                {
                    data.state.apply(&effects);
                    data.session.flush();
                }
            }
        }

        if data.state.core().teleport_on_open() {
            for &new_id in &new_mapped {
                let port = &mut data.input_port;
                let _ = data
                    .state
                    .with_effects(|core, fx| core.recenter_element(port, new_id, fx));
                data.session.request_frame();
                data.session.flush();
            }
        }

        // Settle the scale epochs whose deadline has passed, on the session clock.
        let now = data.session.now();
        if data
            .state
            .core()
            .next_settlement_deadline()
            .is_some_and(|deadline| now >= deadline)
        {
            let scale_fx = data.state.core_mut().scale_tick(now);
            data.state.apply(&scale_fx);
            data.session.flush();
        }

        if data.session.frame_due() {
            if data.host.may_submit() {
                render_and_present(&mut data);
            } else {
                // the frame waits for the host (NB-1); its Redraw requests it again
                data.backend.window().request_redraw();
            }
        }

        // M16: withheld callbacks of unobserved elements, on a timer armed only while one is
        // pending (INV-IDLE-1)
        let now = infynitae_wayland::presentation_clock_now();
        let deadline = data.state.frame_fallback(now);
        data.session.set_fallback_deadline(deadline, now);
        let idle_deadline = data.state.idle_notify_check(now);
        data.session.set_idle_deadline(idle_deadline, now);
        let anim_due = data.state.cursor_animation_due(now);
        data.session.set_cursor_animation_due(anim_due);
        data.session.flush();
    }

    println!("[infynitae] Shutting down compositor...");
    // the satellite first: it is a client of the session, and its display goes with it
    drop(xwayland);
    data.session.shutdown_with(&mut data.state);
    if let Ok(effects) = data
        .state
        .core_mut()
        .output_disconnected(&mut data.output_token, data.output_id)
    {
        data.state.apply(&effects);
    }
    data.session.flush();

    println!("[infynitae] Compositor exited cleanly.");
    Ok(())
}

fn handle_winit_event(data: &mut LoopData, event: WinitEvent) {
    match event {
        WinitEvent::Resized { size, scale_factor } => {
            let width = size.w.max(1) as u32;
            let height = size.h.max(1) as u32;
            data.options.width = width;
            data.options.height = height;
            let scale_120 = resolve_scale_120(data.options.scale_120, scale_factor);
            data.scale_120 = scale_120;
            data.last_abs_pos = None; // Reset baseline on resize (Defect B)
            let descriptor = OutputDescriptor {
                key: data.output_key.clone(),
                mode: (width, height),
                transform: Transform::Normal,
                scale_120,
                refresh_mhz: data.options.refresh_mhz,
            };
            if let Ok(effects) = data.state.core_mut().output_reconfigured(
                &mut data.output_token,
                data.output_id,
                descriptor,
            ) {
                data.state.apply(&effects);
            }
        }
        WinitEvent::Focus(focused) => {
            if !focused {
                data.last_abs_pos = None; // Reset baseline on focus loss / cursor leave (Defect B)
            }
        }
        WinitEvent::CloseRequested => {
            data.closed = true;
        }
        WinitEvent::Input(input_event) => {
            handle_input_event(data, input_event);
        }
        // the host's frame callback arrived: it is ready for the next frame (NB-1); the frame
        // itself is requested by the caller, like for every other host event
        WinitEvent::Redraw => data.host.host_ready(),
    }
}

fn handle_input_event(
    data: &mut LoopData,
    input_event: InputEvent<smithay::backend::winit::WinitInput>,
) {
    match input_event {
        InputEvent::Keyboard { event } => {
            let key = event.key_code().raw().saturating_sub(8);
            let pressed = matches!(event.state(), KeyState::Pressed);
            let time_us = Event::time(&event);
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
        InputEvent::PointerMotion { event } => {
            let raw_delta = (
                PointerMotionEvent::<smithay::backend::winit::WinitInput>::delta_x(&event),
                PointerMotionEvent::<smithay::backend::winit::WinitInput>::delta_y(&event),
            );
            let raw_unaccel = (
                PointerMotionEvent::<smithay::backend::winit::WinitInput>::delta_x_unaccel(&event),
                PointerMotionEvent::<smithay::backend::winit::WinitInput>::delta_y_unaccel(&event),
            );
            let delta = normalize_rel_motion(raw_delta, data.scale_120);
            let delta_unaccel = normalize_rel_motion(raw_unaccel, data.scale_120);
            let time_us = Event::<smithay::backend::winit::WinitInput>::time(&event);
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
            let pos = (event.x(), event.y());
            let delta = normalize_abs_motion(&mut data.last_abs_pos, pos, data.scale_120);
            if delta.0 != 0.0 || delta.1 != 0.0 {
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
            let button = event.button_code();
            let pressed = matches!(event.state(), ButtonState::Pressed);
            let modifiers = data.state.seat_modifiers();
            let time_us = Event::time(&event);
            let phys = PhysicalEvent::PointerButton {
                button,
                pressed,
                modifiers,
                time_us,
            };
            let port = &mut data.input_port;
            let _ = data
                .state
                .with_effects(|core, fx| core.physical_input(port, phys, fx));
        }
        InputEvent::PointerAxis { event } => {
            let h = event
                .amount(smithay::backend::input::Axis::Horizontal)
                .unwrap_or(0.0);
            let v = event
                .amount(smithay::backend::input::Axis::Vertical)
                .unwrap_or(0.0);
            let h_120 = event
                .amount_v120(smithay::backend::input::Axis::Horizontal)
                .unwrap_or(0.0) as i32;
            let v_120 = event
                .amount_v120(smithay::backend::input::Axis::Vertical)
                .unwrap_or(0.0) as i32;
            let time_us = Event::time(&event);
            let phys = PhysicalEvent::PointerAxis {
                axis: infynitae_core::input::Axis {
                    source: infynitae_core::input::AxisSource::Wheel,
                    amount: (h, v),
                    v120: (h_120, v_120),
                },
                time_us,
            };
            let port = &mut data.input_port;
            let _ = data
                .state
                .with_effects(|core, fx| core.physical_input(port, phys, fx));
        }
        _ => {}
    }
}

fn render_and_present(data: &mut LoopData) {
    let age = data.backend.buffer_age().unwrap_or(0);
    let render_res = if let Ok((renderer, mut framebuffer)) = data.backend.bind() {
        convert::render_output(
            &mut data.state,
            renderer,
            &mut framebuffer,
            data.output_id,
            age,
        )
    } else {
        // retried at the next cadence point, so clients waiting for a frame are not stalled
        data.session.request_frame();
        return;
    };

    let damage = match render_res {
        Ok(Some(d)) => Some(d),
        Ok(None) => None,
        Err(e) => {
            eprintln!("[infynitae] Render error: {e:?}");
            // retried at the next cadence point, so clients waiting for a frame are not stalled
            data.session.request_frame();
            return;
        }
    };

    // Submit buffer to GPU/host when damaged
    if let Some(damage_rects) = &damage {
        if let Err(e) = data.backend.submit(Some(damage_rects)) {
            eprintln!("[infynitae] Submit error: {e:?}");
            // retried at the next cadence point, so clients waiting for a frame are not stalled
            data.session.request_frame();
            return;
        }
        // the next submit waits for this one's frame callback (NB-1); winit reports it as a
        // Redraw once a redraw is requested
        data.host.submitted();
        data.backend.window().request_redraw();
        data.session.count_frame();
    }

    // Complete frame callbacks and presentation feedback for clocked elements
    let now = infynitae_wayland::presentation_clock_now();
    data.state.frame_tick(data.output_id, now);
    data.session.flush();
}

/// Calculates logical pointer motion delta from an absolute physical position sample,
/// establishing a baseline on entry or re-entry without warping.
pub fn normalize_abs_motion(
    last_abs_pos: &mut Option<(f64, f64)>,
    pos: (f64, f64),
    scale_120: u32,
) -> (f64, f64) {
    let scale = (scale_120 as f64 / 120.0).max(0.001);
    let delta = match *last_abs_pos {
        Some(prev) => ((pos.0 - prev.0) / scale, (pos.1 - prev.1) / scale),
        None => (0.0, 0.0),
    };
    *last_abs_pos = Some(pos);
    delta
}

/// Normalizes relative physical motion delta by scale factor.
pub fn normalize_rel_motion(delta: (f64, f64), scale_120: u32) -> (f64, f64) {
    let scale = (scale_120 as f64 / 120.0).max(0.001);
    (delta.0 / scale, delta.1 / scale)
}

/// Output scale factor (in units of 1/120) to use for a window of the given host scale factor.
///
/// Keeps the user's explicit scale when one was given; only follows the host scale when none was.
pub fn resolve_scale_120(requested: Option<u32>, host_scale_factor: f64) -> u32 {
    requested.unwrap_or_else(|| {
        if host_scale_factor.is_finite() && host_scale_factor > 0.0 {
            (host_scale_factor * 120.0).round().max(1.0) as u32
        } else {
            120
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NB-1: after a submit no further submit happens until the host's frame callback (Redraw);
    /// then exactly one. A host that never answers (a hidden window) leaves frames waiting,
    /// never a blocking submit.
    #[test]
    fn a_frame_is_submitted_only_when_the_host_is_ready() {
        let mut gate = HostFrameGate::new();
        assert!(gate.may_submit(), "the first frame");
        gate.submitted();
        for _ in 0..1000 {
            assert!(
                !gate.may_submit(),
                "no submit while the host has not answered"
            );
        }
        gate.host_ready();
        assert!(gate.may_submit());
        gate.submitted();
        assert!(!gate.may_submit());
    }

    #[test]
    fn test_pointer_relative_normalization_across_scales() {
        // 1.0x scale (120/120)
        assert_eq!(normalize_rel_motion((10.0, 20.0), 120), (10.0, 20.0));
        // 1.25x scale (150/120)
        assert_eq!(normalize_rel_motion((12.5, 25.0), 150), (10.0, 20.0));
        // 1.5x scale (180/120)
        assert_eq!(normalize_rel_motion((15.0, 30.0), 180), (10.0, 20.0));
        // 2.0x scale (240/120)
        assert_eq!(normalize_rel_motion((20.0, 40.0), 240), (10.0, 20.0));
    }

    #[test]
    fn test_pointer_reentrance_baseline_and_warp_prevention() {
        let mut last_abs = None;

        // Initial cursor entry: baseline established without synthetic delta
        let delta = normalize_abs_motion(&mut last_abs, (400.0, 300.0), 120);
        assert_eq!(
            delta,
            (0.0, 0.0),
            "initial entry must establish baseline with zero delta"
        );
        assert_eq!(last_abs, Some((400.0, 300.0)));

        // Normal movement within window
        let delta = normalize_abs_motion(&mut last_abs, (410.0, 320.0), 120);
        assert_eq!(delta, (10.0, 20.0));
        assert_eq!(last_abs, Some((410.0, 320.0)));

        // Host window focus loss or pointer leave: reset baseline
        last_abs = None;

        // Re-entry from completely different position: baseline established, no multi-hundred-pixel warp
        let delta = normalize_abs_motion(&mut last_abs, (100.0, 50.0), 120);
        assert_eq!(
            delta,
            (0.0, 0.0),
            "re-entry must establish new baseline with zero delta"
        );
        assert_eq!(last_abs, Some((100.0, 50.0)));

        // Continued movement from new baseline
        let delta = normalize_abs_motion(&mut last_abs, (105.0, 52.0), 120);
        assert_eq!(delta, (5.0, 2.0));
    }

    #[test]
    fn test_pointer_absolute_normalization_under_hidpi() {
        let mut last_abs = None;
        // 2.0x scale
        normalize_abs_motion(&mut last_abs, (100.0, 100.0), 240);
        let delta = normalize_abs_motion(&mut last_abs, (120.0, 140.0), 240);
        assert_eq!(
            delta,
            (10.0, 20.0),
            "motion delta must be scaled to logical coordinates"
        );
    }

    #[test]
    fn test_vblank_period_calculation_across_refresh_rates() {
        // 60 Hz (60,000 mHz): ~16.666 ms
        let p60 = vblank_period(60_000);
        assert_eq!(p60.as_micros(), 16_666);

        // 120 Hz (120,000 mHz): ~8.333 ms
        let p120 = vblank_period(120_000);
        assert_eq!(p120.as_micros(), 8_333);

        // 144 Hz (144,000 mHz): ~6.944 ms
        let p144 = vblank_period(144_000);
        assert_eq!(p144.as_micros(), 6_944);

        // 165 Hz (165,000 mHz, user host monitor): ~6.060 ms
        let p165 = vblank_period(165_000);
        assert_eq!(p165.as_micros(), 6_060);

        // 240 Hz (240,000 mHz): ~4.166 ms
        let p240 = vblank_period(240_000);
        assert_eq!(p240.as_micros(), 4_166);
    }

    #[test]
    fn test_resolve_scale_120_honours_requested_scale() {
        // User requested scale 2.0 (240/120); host is scale 1.0
        assert_eq!(resolve_scale_120(Some(240), 1.0), 240);
        // User requested scale 2.0 (240/120); host is scale 1.5
        assert_eq!(resolve_scale_120(Some(240), 1.5), 240);
        // User requested scale 1.5 (180/120); host is scale 2.0
        assert_eq!(resolve_scale_120(Some(180), 2.0), 180);
    }

    #[test]
    fn test_resolve_scale_120_follows_host_scale_when_none_requested() {
        // No requested scale; host scale 1.0 -> 120
        assert_eq!(resolve_scale_120(None, 1.0), 120);
        // No requested scale; host scale 1.25 -> 150
        assert_eq!(resolve_scale_120(None, 1.25), 150);
        // No requested scale; host scale 1.5 -> 180
        assert_eq!(resolve_scale_120(None, 1.5), 180);
        // No requested scale; host scale 2.0 -> 240
        assert_eq!(resolve_scale_120(None, 2.0), 240);
        // Non-positive or non-finite host scale falls back safely
        assert_eq!(resolve_scale_120(None, 0.0), 120);
        assert_eq!(resolve_scale_120(None, f64::NAN), 120);
    }
}
