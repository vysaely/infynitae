//! Infynitae Native headless backend: virtual outputs, virtual input injection, virtual vblank, and
//! the test renderer, so tests exercise the production core and adapter (FOUNDATION_ARCHITECTURE C1.1, NB-12).
//!
//! Virtual outputs are connected, reconfigured and disconnected through [`OutputLifecycleToken`].
//! Virtual input events are injected through [`InputPort`]. A virtual vblank per output drives Core's
//! frame clock and frame production through the adapter's damage trackers.

#![deny(unsafe_code)]

use std::time::Duration;

use infynitae_core::input::Axis;
use infynitae_core::input::bindings::Modifiers;
use infynitae_core::output::{OutputDescriptor, OutputId, OutputKey, Transform};
use infynitae_core::render::FrameEntry;
use infynitae_core::stage::ElementId;
use infynitae_core::{EntryError, FrameError, InputPort, OutputLifecycleToken, PhysicalEvent};
use infynitae_wayland::{WaylandState, convert};
use smithay::backend::renderer::test::DummyRenderer;

/// A virtual output: hardware facts only.
#[derive(Clone, Debug, PartialEq)]
pub struct VirtualOutput {
    /// Persistence identity the backend derives (here: a virtual connector).
    pub key: OutputKey,
    /// Physical mode before the transform.
    pub mode: (u32, u32),
    pub transform: Transform,
    pub scale_120: u32,
    pub refresh_mhz: u32,
}

impl VirtualOutput {
    pub fn descriptor(&self) -> OutputDescriptor {
        OutputDescriptor {
            key: self.key.clone(),
            mode: self.mode,
            transform: self.transform,
            scale_120: self.scale_120,
            refresh_mhz: self.refresh_mhz,
        }
    }

    /// The key of a headless virtual connector (`device` "headless", no EDID serial).
    pub fn key(connector: &str) -> OutputKey {
        OutputKey {
            device: "headless".into(),
            connector: connector.into(),
            make: "Infynitae".into(),
            model: "Virtual".into(),
            serial: None,
        }
    }
}

/// Output discovery or hotplug: the backend's output source holds the token and connects through
/// it; the adapter applies Core's effects (global, enters).
pub fn connect(
    state: &mut WaylandState,
    token: &mut OutputLifecycleToken,
    output: &VirtualOutput,
) -> Result<OutputId, EntryError> {
    let (id, effects) = state
        .core_mut()
        .output_connected(token, output.descriptor())?;
    state.apply(&effects);
    Ok(id)
}

/// A mode, transform, scale or refresh change of a connected virtual output.
pub fn reconfigure(
    state: &mut WaylandState,
    token: &mut OutputLifecycleToken,
    id: OutputId,
    output: &VirtualOutput,
) -> Result<(), EntryError> {
    let effects = state
        .core_mut()
        .output_reconfigured(token, id, output.descriptor())?;
    state.apply(&effects);
    Ok(())
}

/// Unplug: leaves first, then the global is destroyed (INV-OUTPUT-1).
pub fn disconnect(
    state: &mut WaylandState,
    token: &mut OutputLifecycleToken,
    id: OutputId,
) -> Result<(), EntryError> {
    let effects = state.core_mut().output_disconnected(token, id)?;
    state.apply(&effects);
    Ok(())
}

/// The virtual input device: one physical event through the input port; the adapter applies
/// Core's effects in order.
pub fn inject(
    state: &mut WaylandState,
    port: &mut InputPort,
    ev: PhysicalEvent,
) -> Result<(), EntryError> {
    state.with_effects(|core, fx| core.physical_input(port, ev, fx))
}

/// Relative motion of the virtual mouse (accelerated and unaccelerated logical deltas).
pub fn pointer_motion(
    state: &mut WaylandState,
    port: &mut InputPort,
    delta: (f64, f64),
    delta_unaccel: (f64, f64),
    time_us: u64,
) -> Result<(), EntryError> {
    let ev = PhysicalEvent::PointerMotion {
        delta,
        delta_unaccel,
        time_us,
    };
    inject(state, port, ev)
}

/// A button of the virtual mouse, with the seat's modifier snapshot at that event.
pub fn pointer_button(
    state: &mut WaylandState,
    port: &mut InputPort,
    button: u32,
    pressed: bool,
    _modifiers: Modifiers,
    time_us: u64,
) -> Result<(), EntryError> {
    let seat_mods = state.seat_modifiers();
    let ev = PhysicalEvent::PointerButton {
        button,
        pressed,
        modifiers: seat_mods,
        time_us,
    };
    inject(state, port, ev)
}

/// Scrolling on the virtual mouse.
pub fn pointer_axis(
    state: &mut WaylandState,
    port: &mut InputPort,
    axis: Axis,
    time_us: u64,
) -> Result<(), EntryError> {
    inject(state, port, PhysicalEvent::PointerAxis { axis, time_us })
}

/// A physical key event on the virtual keyboard (evdev keycode and press/release).
/// Submitted through `InputPort` (C1.5, INV-INPUT-1, K1–K3).
pub fn keyboard_key(
    state: &mut WaylandState,
    port: &mut InputPort,
    key: u32,
    pressed: bool,
    time_us: u64,
) -> Result<(), EntryError> {
    let token = state.core().verify_input_port(port, key, pressed)?;
    let (modifiers, mods_changed) = state.sync_seat_key(token);
    let ev = PhysicalEvent::KeyboardKey {
        key,
        pressed,
        modifiers,
        mods_changed,
        time_us,
    };
    inject(state, port, ev)
}

/// Set keyboard focus to an element or clear focus.
/// Must be authorized by holding an `InputPort` (K5, OBJ-11).
pub fn set_focus(
    state: &mut WaylandState,
    port: &mut InputPort,
    target: Option<ElementId>,
) -> Result<(), EntryError> {
    let effects = state.core_mut().set_focus(port, target)?;
    state.apply(&effects);
    Ok(())
}

/// A virtual vblank of `output` at `now` (monotonic): the backend produces the output's frame
/// with the test renderer (recording its ProjectionFrame, so hit-testing reads what was shown,
/// INV-HIT-1); then Core's frame clock decides which observed elements complete, and the adapter
/// completes their callbacks and presents their feedback.
pub fn vblank(state: &mut WaylandState, output: OutputId, now: Duration) {
    // an output without a view has no frame to produce; the frame clock still ticks
    let _ = render_frame(state, &mut DummyRenderer, output);
    state.frame_tick(output, now);
}

/// The off-screen fallback timer at `now` (monotonic, M16): due unobserved elements complete;
/// returns when the timer must fire next, `None` when it is not armed (INV-IDLE-1).
pub fn frame_fallback(state: &mut WaylandState, now: Duration) -> Option<Duration> {
    state.frame_fallback(now)
}

/// The virtual vblank period of an output (refresh in mHz).
pub fn vblank_period(refresh_mhz: u32) -> Duration {
    Duration::from_nanos(1_000_000_000_000 / u64::from(refresh_mhz.max(1)))
}

/// What the headless renderer reports as its device limit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceLimit {
    /// The largest buffer side, in device pixels.
    MaxBufferSide(u32),
    /// The query fails, as a real renderer's may (a lost context, a driver error).
    Unavailable(String),
}

impl Default for DeviceLimit {
    /// 16384: the framebuffer limit recorded for the reference device (EC-16). A fixture value,
    /// chosen so that headless runs behave as the reference device would; it says nothing about
    /// the machine the tests run on.
    fn default() -> Self {
        DeviceLimit::MaxBufferSide(HEADLESS_REFERENCE_DEVICE_LIMIT)
    }
}

/// EC-16, `maxFramebufferWidth` of the reference device: the headless renderer's default limit.
pub const HEADLESS_REFERENCE_DEVICE_LIMIT: u32 = 16384;

/// The headless backend's renderer: Smithay's test renderer, which draws nothing, together with
/// the device limit a real renderer would have. There is no device behind it, so the limit is a
/// documented, deterministic fixture value, configurable so that tests can stand in for devices
/// with other limits. It is reported through the same interface a real renderer uses (OB-13);
/// what a real GPU reports is OB-52 and is not shown by anything this type does.
#[derive(Debug)]
pub struct HeadlessRenderer {
    pub dummy: DummyRenderer,
    limit: DeviceLimit,
}

impl HeadlessRenderer {
    pub fn new(limit: DeviceLimit) -> Self {
        Self {
            dummy: DummyRenderer,
            limit,
        }
    }

    /// Replaces what the device reports, as a renderer recreated on another device would. The
    /// compositor learns of it only when the limits are reported again.
    pub fn set_device_limit(&mut self, limit: DeviceLimit) {
        self.limit = limit;
    }
}

impl Default for HeadlessRenderer {
    fn default() -> Self {
        Self::new(DeviceLimit::default())
    }
}

impl infynitae_wayland::limits::RendererLimits for HeadlessRenderer {
    fn max_buffer_side(&self) -> Result<u32, infynitae_wayland::limits::LimitQueryError> {
        match &self.limit {
            DeviceLimit::MaxBufferSide(side) => Ok(*side),
            DeviceLimit::Unavailable(why) => {
                Err(infynitae_wayland::limits::LimitQueryError(why.clone()))
            }
        }
    }
}

#[derive(Debug)]
pub enum RenderError {
    Frame(FrameError),
    Renderer(<DummyRenderer as smithay::backend::renderer::RendererSuper>::Error),
}

/// Core's render list for an output (recorded as its ProjectionFrame) turned into Smithay render
/// elements with the test renderer, front to back, each at its canonical rect (INV-RENDER-1).
pub fn render_frame(
    state: &mut WaylandState,
    renderer: &mut DummyRenderer,
    output: OutputId,
) -> Result<Vec<(FrameEntry, convert::SurfaceElement<DummyRenderer>)>, RenderError> {
    let frame = state
        .core_mut()
        .render_list(output)
        .map_err(RenderError::Frame)?;
    let (k, entries) = (frame.view.k, frame.entries.clone());
    convert::render_elements(renderer, state.registry(), &entries, k).map_err(RenderError::Renderer)
}

/// Options for configuring headless compositor execution.
#[derive(Clone, Debug)]
pub struct HeadlessCompositorOptions {
    /// Wayland socket name (e.g. "wayland-1"). If None, automatically selects an available socket.
    pub socket_name: Option<String>,
    /// Optional command to execute on compositor startup (e.g. "weston-terminal").
    pub exec: Option<String>,
    /// Virtual output width.
    pub width: u32,
    /// Virtual output height.
    pub height: u32,
    /// Virtual output refresh in mHz.
    pub refresh_mhz: u32,
    /// Optional maximum run duration before clean exit.
    pub duration: Option<Duration>,
    /// Optional maximum frame count before clean exit.
    pub max_frames: Option<u64>,
    /// What the headless renderer reports as its device limit.
    pub device_limit: DeviceLimit,
    /// Path to xwayland-satellite to supervise (M19); `None`: no X11.
    pub xwayland_satellite: Option<std::path::PathBuf>,
}

impl Default for HeadlessCompositorOptions {
    fn default() -> Self {
        Self {
            socket_name: None,
            exec: None,
            width: 1920,
            height: 1080,
            refresh_mhz: 60_000,
            duration: None,
            max_frames: None,
            device_limit: DeviceLimit::default(),
            xwayland_satellite: None,
        }
    }
}

/// Runs a headless compositor session: a virtual output, a listening Wayland socket, and a
/// virtual vblank on the session's frame timer. Returns when a run limit is reached.
/// The display settings clients applied (`wlr-output-management`): a headless output has no
/// hardware to set, Core takes the new modes, scales and positions (each screen's new
/// descriptor, then its position).
pub fn apply_output_configurations(state: &mut WaylandState, token: &mut OutputLifecycleToken) {
    for c in state.take_output_configurations() {
        let ok = state.output_change_plan(&c.changes).is_some_and(|plan| {
            plan.into_iter().all(|p| {
                if let Some(desc) = p.descriptor {
                    match state.core_mut().output_reconfigured(token, p.output, desc) {
                        Ok(fx) => state.apply(&fx),
                        Err(_) => return false,
                    }
                }
                if let Some(pos) = p.position {
                    match state.core_mut().output_arranged(token, p.output, pos) {
                        Ok(fx) => state.apply(&fx),
                        Err(_) => return false,
                    }
                }
                true
            })
        });
        state.finish_output_configuration(c, ok);
    }
}

pub fn run_headless(
    core: infynitae_core::Core,
    input_port: infynitae_core::InputPort,
    output_token: infynitae_core::OutputLifecycleToken,
    options: HeadlessCompositorOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    run_headless_with(core, input_port, output_token, options, None)
}

/// [`run_headless`] with a loop extension (such as the shell server): it runs after every dispatch,
/// before the clients are served, so its effects reach them with that dispatch's flush.
pub fn run_headless_with(
    core: infynitae_core::Core,
    mut input_port: infynitae_core::InputPort,
    mut output_token: infynitae_core::OutputLifecycleToken,
    options: HeadlessCompositorOptions,
    extension: Option<infynitae_wayland::extension::ExtensionFactory>,
) -> Result<(), Box<dyn std::error::Error>> {
    use infynitae_wayland::session::{Session, SessionOptions};
    use smithay::reexports::calloop::EventLoop;
    use smithay::reexports::wayland_server::Display;

    let display: Display<WaylandState> = Display::new()?;
    let mut state = WaylandState::new(&display.handle(), core);

    let virtual_output = VirtualOutput {
        key: VirtualOutput::key("HEADLESS-0"),
        mode: (options.width, options.height),
        transform: Transform::Normal,
        scale_120: 120,
        refresh_mhz: options.refresh_mhz,
    };
    let output_id = connect(&mut state, &mut output_token, &virtual_output)
        .map_err(|e| format!("Failed to connect headless output: {e:?}"))?;

    // Device ceiling D_max (OB-13, F-SCALE-2, OBJ-29): asked of the renderer, as every backend
    // must. The headless renderer is a fixture and reports the limit it was configured with.
    let renderer = HeadlessRenderer::new(options.device_limit);
    state.report_renderer_limits(&renderer)?;

    let mut event_loop: EventLoop<()> = EventLoop::try_new()?;
    let x_display =
        infynitae_wayland::xwayland::reserve_for_session(options.xwayland_satellite.as_deref())?;
    let mut session = Session::new(
        display,
        &event_loop.handle(),
        &SessionOptions {
            socket_name: options.socket_name,
            exec: options.exec,
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

    println!("[infynitae] Headless compositor running.");
    let mut known_mapped = std::collections::HashSet::new();
    while !session.finished() {
        // An open scale-settlement deadline (INV-SCALE-3) is a wake-up of its own: the loop
        // sleeps until it even if no client and no frame is pending, and has nothing to wake
        // for when no epoch is open (INV-IDLE-1).
        session.wake_at(state.core().next_settlement_deadline());
        let restart = xwayland
            .as_mut()
            .and_then(|x| x.poll(std::time::Instant::now()));
        let timeout = match (session.timeout(), restart) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        event_loop.dispatch(timeout, &mut ())?;
        if let Some(ext) = extension.as_mut() {
            let (fx, changed) = ext.service(state.core_mut());
            state.apply(&fx);
            if changed {
                session.request_frame();
            }
        }
        session.dispatch(&mut state);
        apply_output_configurations(&mut state, &mut output_token);

        let current_mapped = state.core().stage().mapped_elements();
        let new_mapped: Vec<_> = current_mapped
            .iter()
            .copied()
            .filter(|id| !known_mapped.contains(id))
            .collect();

        known_mapped.clear();
        known_mapped.extend(current_mapped);

        if state.core().focus().is_initial_eligible() {
            if let Some(&topmost) = state.core().stage().mapped_elements().last()
                && let Ok(effects) = state
                    .core_mut()
                    .grant_initial_focus(&mut input_port, topmost)
            {
                state.apply(&effects);
                session.flush();
            }
        } else if state.core().focus_on_open() {
            for &new_id in &new_mapped {
                if let Ok(effects) = state.core_mut().set_focus(&mut input_port, Some(new_id)) {
                    state.apply(&effects);
                    session.flush();
                }
            }
        }

        if state.core().teleport_on_open() {
            for &new_id in &new_mapped {
                let port = &mut input_port;
                let _ = state.with_effects(|core, fx| core.recenter_element(port, new_id, fx));
                session.request_frame();
                session.flush();
            }
        }

        // Settle the scale epochs whose deadline has passed, on the session clock.
        let now = session.now();
        if state
            .core()
            .next_settlement_deadline()
            .is_some_and(|deadline| now >= deadline)
        {
            let scale_fx = state.core_mut().scale_tick(now);
            state.apply(&scale_fx);
            session.flush();
        }

        if session.frame_due() {
            vblank(
                &mut state,
                output_id,
                infynitae_wayland::presentation_clock_now(),
            );
            session.count_frame();
            session.flush();
        }

        // M16: withheld callbacks of unobserved elements, on a timer armed only while one is
        // pending (INV-IDLE-1)
        let now = infynitae_wayland::presentation_clock_now();
        session.set_fallback_deadline(state.frame_fallback(now), now);
        session.set_idle_deadline(state.idle_notify_check(now), now);
        let anim_due = state.cursor_animation_due(now);
        session.set_cursor_animation_due(anim_due);
        session.flush();
    }

    println!("[infynitae] Headless compositor shutting down...");
    // the satellite first: it is a client of the session, and its display goes with it
    drop(xwayland);
    session.shutdown_with(&mut state);
    let _ = disconnect(&mut state, &mut output_token, output_id);
    session.flush();
    println!("[infynitae] Headless compositor exited cleanly.");
    Ok(())
}
