//! Infynitae Native headless backend: virtual outputs, virtual vblank and the test renderer, so
//! tests exercise the production core and adapter (FOUNDATION_ARCHITECTURE C1.1, NB-12).
//!
//! INC-2 state: any number of virtual outputs connected, reconfigured and disconnected through
//! the output-lifecycle token (startup and hotplug alike); virtual vblank per output driving
//! Core's frame clock; frame production through the adapter's damage trackers. No input
//! injection yet (INC-3).

#![deny(unsafe_code)]

use std::time::Duration;

use infynitae_core::input::Axis;
use infynitae_core::input::bindings::Modifiers;
use infynitae_core::output::{OutputDescriptor, OutputId, OutputKey, Transform};
use infynitae_core::render::FrameEntry;
use infynitae_core::stage::ElementId;
use infynitae_core::{EntryError, FrameError, InputPort, OutputLifecycleToken, PhysicalEvent};
use infynitae_wayland::{WaylandState, convert};
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
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
    let effects = state.core_mut().physical_input(port, ev)?;
    state.apply(&effects);
    Ok(())
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
    let elements = state.core().frame_tick(output);
    state.complete_frames(output, &elements, now);
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
/// elements with the test renderer, front to back.
pub fn render_frame(
    state: &mut WaylandState,
    renderer: &mut DummyRenderer,
    output: OutputId,
) -> Result<Vec<(FrameEntry, WaylandSurfaceRenderElement<DummyRenderer>)>, RenderError> {
    let entries = state
        .core_mut()
        .render_list(output)
        .map_err(RenderError::Frame)?
        .entries
        .clone();
    convert::render_elements(renderer, state.registry(), &entries).map_err(RenderError::Renderer)
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
        }
    }
}

/// Runs a headless compositor session: a virtual output, a listening Wayland socket, and a
/// virtual vblank on the session's frame timer. Returns when a run limit is reached.
pub fn run_headless(
    core: infynitae_core::Core,
    mut input_port: infynitae_core::InputPort,
    mut output_token: infynitae_core::OutputLifecycleToken,
    options: HeadlessCompositorOptions,
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
    let mut session = Session::new(
        display,
        &event_loop.handle(),
        &SessionOptions {
            socket_name: options.socket_name,
            exec: options.exec,
            refresh_mhz: options.refresh_mhz,
            duration: options.duration,
            max_frames: options.max_frames,
        },
    )?;

    println!("[infynitae] Headless compositor running.");
    while !session.finished() {
        // An open scale-settlement deadline (INV-SCALE-3) is a wake-up of its own: the loop
        // sleeps until it even if no client and no frame is pending, and has nothing to wake
        // for when no epoch is open (INV-IDLE-1).
        session.wake_at(state.core().next_settlement_deadline());
        event_loop.dispatch(session.timeout(), &mut ())?;
        session.dispatch(&mut state);

        // Initial focus grant on topmost mapped element if seat is eligible (INV-INPUT-8)
        if state.core().focus().is_initial_eligible()
            && let Some(&topmost) = state.core().stage().mapped_elements().last()
            && let Ok(effects) = state
                .core_mut()
                .grant_initial_focus(&mut input_port, topmost)
        {
            state.apply(&effects);
            session.flush();
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
    }

    println!("[infynitae] Headless compositor shutting down...");
    session.shutdown();
    let _ = disconnect(&mut state, &mut output_token, output_id);
    session.flush();
    println!("[infynitae] Headless compositor exited cleanly.");
    Ok(())
}
