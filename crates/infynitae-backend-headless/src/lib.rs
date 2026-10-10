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
        }
    }
}

/// Runs a headless compositor session.
///
/// Connects a virtual output, opens a Wayland listening socket, accepts clients, ticks
/// virtual vblank frames, and exits cleanly when requested or when duration/frame limits are reached.
pub fn run_headless(
    core: infynitae_core::Core,
    mut input_port: infynitae_core::InputPort,
    mut output_token: infynitae_core::OutputLifecycleToken,
    options: HeadlessCompositorOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    use smithay::reexports::calloop::generic::Generic;
    use smithay::reexports::calloop::{EventLoop, Interest, Mode, PostAction};
    use smithay::reexports::wayland_server::Display;
    use smithay::wayland::socket::ListeningSocketSource;
    use std::sync::Arc;
    use std::time::Instant;

    let mut display: Display<WaylandState> = Display::new()?;
    let mut state = WaylandState::new(&display.handle(), core);

    let socket_source = match options.socket_name.as_deref() {
        Some(name) => ListeningSocketSource::with_name(name)?,
        None => ListeningSocketSource::new_auto()?,
    };
    let socket_name = socket_source.socket_name().to_string_lossy().to_string();
    println!("[infynitae] Headless listening on Wayland socket: {socket_name}");
    println!("[infynitae] To connect clients, set WAYLAND_DISPLAY={socket_name}");

    let virtual_output = VirtualOutput {
        key: VirtualOutput::key("HEADLESS-0"),
        mode: (options.width, options.height),
        transform: Transform::Normal,
        scale_120: 120,
        refresh_mhz: options.refresh_mhz,
    };
    let output_id = connect(&mut state, &mut output_token, &virtual_output)
        .map_err(|e| format!("Failed to connect headless output: {e:?}"))?;

    // Spawn client if requested
    let mut children = Vec::new();
    if let Some(cmd) = &options.exec {
        println!("[infynitae] Spawning initial client: {cmd}");
        let mut parts = cmd.split_whitespace();
        if let Some(bin) = parts.next() {
            let mut command = std::process::Command::new(bin);
            command.args(parts);
            command.env("WAYLAND_DISPLAY", &socket_name);
            match command.spawn() {
                Ok(child) => {
                    println!("[infynitae] Client spawned with PID {}", child.id());
                    children.push(child);
                }
                Err(e) => {
                    eprintln!("[infynitae] Failed to spawn {cmd}: {e}");
                }
            }
        }
    }

    struct HeadlessData {
        display: Display<WaylandState>,
        children: Vec<std::process::Child>,
    }

    let mut event_loop: EventLoop<HeadlessData> = EventLoop::try_new()?;
    event_loop
        .handle()
        .insert_source(socket_source, |stream, _, data| {
            let _ = data
                .display
                .handle()
                .insert_client(stream, Arc::new(infynitae_wayland::ClientState::default()));
        })
        .map_err(|e| format!("Failed to register socket: {e}"))?;

    // Insert display client poll FD into calloop (Defect D: event loop starvation)
    let display_fd = display.backend().poll_fd().try_clone_to_owned()?;
    event_loop
        .handle()
        .insert_source(
            Generic::new(display_fd, Interest::READ, Mode::Level),
            |_event, _metadata, _data| Ok(PostAction::Continue),
        )
        .map_err(|e| format!("Failed to register display event source: {e}"))?;

    // Ingest renderer device ceiling D_max = 16384 (OB-23, F-SCALE-2)
    state.core_mut().set_renderer_device_max(16384);

    let mut data = HeadlessData { display, children };
    let start_time = Instant::now();
    let period = vblank_period(options.refresh_mhz);
    let mut frame_count = 0u64;
    let mut last_vblank = Instant::now();

    println!("[infynitae] Headless compositor running.");
    loop {
        let elapsed = last_vblank.elapsed();
        let mut timeout = period.saturating_sub(elapsed);
        let now_ms = start_time.elapsed().as_millis() as u64;
        if let Some(deadline) = state.core().next_settlement_deadline() {
            let remain = Duration::from_millis(deadline.saturating_sub(now_ms));
            timeout = timeout.min(remain);
        }
        event_loop.dispatch(Some(timeout), &mut data)?;

        // Non-blocking child process reaping (Defect E)
        data.children.retain_mut(|child| {
            match child.try_wait() {
                Ok(Some(status)) => {
                    println!(
                        "[infynitae] Headless child process {} exited with status {}",
                        child.id(),
                        status
                    );
                    false // remove reaped child
                }
                Ok(None) => true,
                Err(e) => {
                    eprintln!("[infynitae] Failed to wait on child {}: {}", child.id(), e);
                    false
                }
            }
        });

        if let Some(max_dur) = options.duration
            && start_time.elapsed() >= max_dur
        {
            break;
        }
        if let Some(max_frames) = options.max_frames
            && frame_count >= max_frames
        {
            break;
        }

        let _ = data.display.dispatch_clients(&mut state);
        let _ = data.display.flush_clients();

        // Initial focus grant on topmost mapped element if seat is eligible (Defect A: INV-INPUT-8)
        if state.core().focus().is_initial_eligible()
            && let Some(&topmost) = state.core().stage().mapped_elements().last()
            && let Ok(effects) = state
                .core_mut()
                .grant_initial_focus(&mut input_port, topmost)
        {
            state.apply(&effects);
            let _ = data.display.flush_clients();
        }

        // Advance scale settlement timer
        let now_ms = start_time.elapsed().as_millis() as u64;
        let scale_fx = state.core_mut().scale_tick(now_ms);
        if !scale_fx.is_empty() {
            state.apply(&scale_fx);
            let _ = data.display.flush_clients();
        }

        if last_vblank.elapsed() >= period {
            vblank(&mut state, output_id, start_time.elapsed());
            frame_count += 1;
            last_vblank = Instant::now();
            let _ = data.display.flush_clients();
        }
    }

    // Terminate and reap any remaining children gracefully (Task A)
    terminate_children(&mut data.children, Duration::from_millis(500));

    println!("[infynitae] Headless compositor shutting down...");
    let _ = disconnect(&mut state, &mut output_token, output_id);
    let _ = data.display.flush_clients();
    println!("[infynitae] Headless compositor exited cleanly.");
    Ok(())
}

/// Gracefully terminates and reaps compositor-owned child processes.
///
/// Implements a bounded two-stage shutdown policy:
/// 1. Reaps any children that have already exited.
/// 2. Sends `SIGTERM` to remaining live children.
/// 3. Polls with non-blocking checks up to `grace_period` (default ~500ms).
/// 4. Escalates to `SIGKILL` only for children that remain alive after grace interval.
/// 5. Blocks to reap all direct child processes, preventing zombies.
///
/// Correctly handles `ESRCH` (process exited in race), already-exited children,
/// and repeated invocations.
pub fn terminate_children(children: &mut Vec<std::process::Child>, grace_period: Duration) {
    use smithay::reexports::rustix::io::Errno;
    use smithay::reexports::rustix::process::{Pid, Signal, kill_process};
    use std::time::Instant;

    // 1. Initial non-blocking reap for children that already exited normally
    children.retain_mut(|child| {
        match child.try_wait() {
            Ok(Some(_status)) => false, // exited and reaped
            Ok(None) => true,           // still running
            Err(_) => false,
        }
    });

    if children.is_empty() {
        return;
    }

    // 2. Send SIGTERM to all remaining living children
    for child in children.iter_mut() {
        let pid_raw = child.id() as i32;
        if let Some(pid) = Pid::from_raw(pid_raw) {
            match kill_process(pid, Signal::TERM) {
                Ok(()) => {}
                Err(Errno::SRCH) => {} // Process already exited (normal race)
                Err(e) => {
                    eprintln!("[infynitae] Failed to send SIGTERM to child {pid_raw}: {e}");
                }
            }
        }
    }

    // 3 & 4. Bounded grace wait (drain interval)
    let start = Instant::now();
    let poll_interval = Duration::from_millis(10);
    while start.elapsed() < grace_period && !children.is_empty() {
        children.retain_mut(|child| {
            match child.try_wait() {
                Ok(Some(_status)) => false, // reaped during grace
                Ok(None) => true,           // still running
                Err(_) => false,
            }
        });
        if children.is_empty() {
            break;
        }
        std::thread::sleep(poll_interval);
    }

    // 5 & 6. Escalate to SIGKILL for any stubborn children and reap all remaining
    for child in children.iter_mut() {
        println!(
            "[infynitae] Child {} did not exit within grace period, escalating to SIGKILL",
            child.id()
        );
        let _ = child.kill();
        let _ = child.wait();
    }
    children.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn test_terminate_children_normal_exit() {
        let mut children = vec![Command::new("true").spawn().unwrap()];
        std::thread::sleep(Duration::from_millis(50));
        terminate_children(&mut children, Duration::from_millis(500));
        assert!(children.is_empty());
    }

    #[test]
    fn test_terminate_children_sigterm_handling() {
        let mut children = vec![Command::new("sleep").arg("10").spawn().unwrap()];
        let start = std::time::Instant::now();
        terminate_children(&mut children, Duration::from_millis(1000));
        let elapsed = start.elapsed();
        assert!(children.is_empty());
        assert!(
            elapsed < Duration::from_millis(500),
            "SIGTERM took {:?}",
            elapsed
        );
    }

    #[test]
    fn test_terminate_children_ignored_sigterm_escalates_to_sigkill() {
        let mut children = vec![
            Command::new("sh")
                .args(["-c", "trap '' TERM; while true; do sleep 1; done"])
                .spawn()
                .unwrap(),
        ];
        // Allow shell process to start and install trap handler
        std::thread::sleep(Duration::from_millis(50));
        let start = std::time::Instant::now();
        terminate_children(&mut children, Duration::from_millis(100));
        let elapsed = start.elapsed();
        assert!(children.is_empty());
        assert!(
            elapsed >= Duration::from_millis(80),
            "escalated too quickly: {:?}",
            elapsed
        );
        assert!(
            elapsed < Duration::from_millis(500),
            "escalation hung: {:?}",
            elapsed
        );
    }

    #[test]
    fn test_terminate_children_repeated_shutdown_idempotent() {
        let mut children = vec![Command::new("sleep").arg("10").spawn().unwrap()];
        terminate_children(&mut children, Duration::from_millis(500));
        assert!(children.is_empty());
        terminate_children(&mut children, Duration::from_millis(500));
        assert!(children.is_empty());
    }

    #[test]
    fn test_terminate_children_zombie_prevention() {
        let child = Command::new("sleep").arg("10").spawn().unwrap();
        let pid = child.id();
        let mut children = vec![child];
        terminate_children(&mut children, Duration::from_millis(500));
        assert!(children.is_empty());

        let status_path = format!("/proc/{pid}/status");
        if let Ok(status) = std::fs::read_to_string(&status_path) {
            for line in status.lines() {
                if line.starts_with("State:") {
                    assert!(!line.contains('Z'), "process {pid} left as zombie: {line}");
                }
            }
        }
    }
}
