//! Infynitae Native nested development backend (winit window, its input and GL target)
//! (FOUNDATION_ARCHITECTURE C1.1).

#![deny(unsafe_code)]

use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use infynitae_core::output::{OutputDescriptor, OutputId, OutputKey, Transform};
use infynitae_core::{Core, InputPort, OutputLifecycleToken, PhysicalEvent};
use infynitae_wayland::{ClientState, WaylandState, convert};

use smithay::backend::input::{
    AbsolutePositionEvent, ButtonState, Event, InputEvent, KeyState, KeyboardKeyEvent,
    PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::winit::{self, WinitEvent, WinitGraphicsBackend};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{EventLoop, Interest, Mode, PostAction};
use smithay::reexports::wayland_server::Display;
use smithay::reexports::winit::dpi::LogicalSize;
use smithay::reexports::winit::window::Window as WinitWindow;
use smithay::wayland::socket::ListeningSocketSource;

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
    display: Display<WaylandState>,
    input_port: InputPort,
    output_token: OutputLifecycleToken,
    output_id: OutputId,
    output_key: OutputKey,
    backend: WinitGraphicsBackend<GlesRenderer>,
    start_time: Instant,
    last_abs_pos: Option<(f64, f64)>,
    scale_120: u32,
    last_frame: Instant,
    frame_count: u64,
    children: Vec<std::process::Child>,
    running: Arc<AtomicBool>,
    options: WinitCompositorOptions,
}

/// Runs the nested compositor with Winit backend.
///
/// Takes ownership of `Core` and the composition root's authority ports (`InputPort`,
/// `OutputLifecycleToken`), wires the nested window, listening socket, renderer and
/// event loop, and blocks until the window is closed or a termination signal is received.
pub fn run_nested(
    core: Core,
    input_port: InputPort,
    mut output_token: OutputLifecycleToken,
    options: WinitCompositorOptions,
) -> Result<(), Box<dyn Error>> {
    let running = Arc::new(AtomicBool::new(true));
    let mut event_loop: EventLoop<LoopData> = EventLoop::try_new()?;

    // 1. Initialize Wayland server display
    let mut display: Display<WaylandState> = Display::new()?;
    let mut state = WaylandState::new(&display.handle(), core);

    // 2. Initialize Wayland listening socket
    let socket_source = match options.socket_name.as_deref() {
        Some(name) => ListeningSocketSource::with_name(name)?,
        None => ListeningSocketSource::new_auto()?,
    };
    let socket_name = socket_source.socket_name().to_string_lossy().to_string();
    println!("[infynitae] Listening on Wayland display socket: {socket_name}");
    println!("[infynitae] To connect clients, set WAYLAND_DISPLAY={socket_name}");

    // Insert listening socket into calloop
    event_loop
        .handle()
        .insert_source(socket_source, |stream, _, data| {
            let client_res = data
                .display
                .handle()
                .insert_client(stream, Arc::new(ClientState::default()));
            match client_res {
                Ok(client) => {
                    println!("[infynitae] Client connected: {client:?}");
                }
                Err(e) => {
                    eprintln!("[infynitae] Failed to insert client: {e}");
                }
            }
        })
        .map_err(|e| format!("Failed to register listening socket: {e}"))?;

    // Insert display client poll FD into calloop (Defect D: event loop starvation)
    let display_fd = display.backend().poll_fd().try_clone_to_owned()?;
    event_loop
        .handle()
        .insert_source(
            Generic::new(display_fd, Interest::READ, Mode::Level),
            |_event, _metadata, _data| Ok(PostAction::Continue),
        )
        .map_err(|e| format!("Failed to register display event source: {e}"))?;

    // 3. Initialize Winit backend & window
    let window_attributes = WinitWindow::default_attributes()
        .with_inner_size(LogicalSize::new(
            options.width as f64,
            options.height as f64,
        ))
        .with_title(&options.title)
        .with_visible(true);

    let (backend, winit_event_loop) =
        winit::init_from_attributes::<GlesRenderer>(window_attributes)?;

    // 4. Connect initial output in Core via OutputLifecycleToken
    let output_key = OutputKey {
        device: "winit".into(),
        connector: "WINIT-0".into(),
        make: "Infynitae".into(),
        model: "Nested".into(),
        serial: None,
    };
    let descriptor = OutputDescriptor {
        key: output_key.clone(),
        mode: (options.width, options.height),
        transform: Transform::Normal,
        scale_120: 120, // 1.0x scale
        refresh_mhz: options.refresh_mhz,
    };
    let (output_id, effects) = state
        .core_mut()
        .output_connected(&mut output_token, descriptor)
        .map_err(|e| format!("Failed to connect output: {e:?}"))?;
    state.apply(&effects);

    // Device ceiling D_max (OB-13, F-SCALE-2, OBJ-29). PROVISIONAL: the EC-16 device fact, not
    // a value discovered from this GLES renderer; the host GPU's real limit may be lower. Runtime
    // discovery for the nested backend is not implemented (OB-13 residual).
    let fx = state
        .core_mut()
        .set_renderer_device_max(infynitae_core::scale::DEFAULT_D_MAX)
        .map_err(|e| format!("Invalid renderer device limit: {e:?}"))?;
    state.apply(&fx);

    // 5. Spawn client command if requested
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

    // 6. Assemble LoopData
    let mut data = LoopData {
        state,
        display,
        input_port,
        output_token,
        output_id,
        output_key,
        backend,
        start_time: Instant::now(),
        last_abs_pos: None,
        scale_120: 120,
        last_frame: Instant::now(),
        frame_count: 0,
        children,
        running: running.clone(),
        options,
    };

    // 7. Insert WinitEventLoop into calloop
    event_loop
        .handle()
        .insert_source(winit_event_loop, |event, _, data| {
            handle_winit_event(data, event);
        })
        .map_err(|e| format!("Failed to register winit event source: {e}"))?;

    // 8. Event loop runner
    println!("[infynitae] Compositor running. Press Ctrl+C or close window to exit.");
    let target_frame_duration = vblank_period(data.options.refresh_mhz);

    while running.load(Ordering::SeqCst) {
        // Run calloop with timeout to pace frames
        let elapsed = data.last_frame.elapsed();
        let timeout = target_frame_duration.saturating_sub(elapsed);

        event_loop.dispatch(Some(timeout), &mut data)?;

        // Non-blocking child process reaping (Defect E)
        data.children.retain_mut(|child| {
            match child.try_wait() {
                Ok(Some(status)) => {
                    println!(
                        "[infynitae] Child process {} exited with status {}",
                        child.id(),
                        status
                    );
                    false // remove reaped child
                }
                Ok(None) => true, // still running
                Err(e) => {
                    eprintln!("[infynitae] Failed to wait on child {}: {}", child.id(), e);
                    false
                }
            }
        });

        // Check test duration limit if configured
        if let Some(max_dur) = data.options.duration
            && data.start_time.elapsed() >= max_dur
        {
            break;
        }

        // Check frame limit if configured
        if let Some(max_frames) = data.options.max_frames
            && data.frame_count >= max_frames
        {
            break;
        }

        // Dispatch Wayland client requests
        let _ = data.display.dispatch_clients(&mut data.state);
        let _ = data.display.flush_clients();

        // Initial focus grant on topmost mapped element if seat is eligible (Defect A: INV-INPUT-8)
        if data.state.core().focus().is_initial_eligible()
            && let Some(&topmost) = data.state.core().stage().mapped_elements().last()
            && let Ok(effects) = data
                .state
                .core_mut()
                .grant_initial_focus(&mut data.input_port, topmost)
        {
            data.state.apply(&effects);
            let _ = data.display.flush_clients();
        }

        // Advance scale settlement timer
        let now_ms = data.start_time.elapsed().as_millis() as u64;
        let scale_fx = data.state.core_mut().scale_tick(now_ms);
        if !scale_fx.is_empty() {
            data.state.apply(&scale_fx);
            let _ = data.display.flush_clients();
        }

        // Render frame if frame interval has elapsed (Defect C)
        if data.last_frame.elapsed() >= target_frame_duration {
            render_and_present(&mut data);
        }
    }

    // Terminate and reap any remaining children gracefully (Task A)
    terminate_children(&mut data.children, Duration::from_millis(500));

    // 9. Clean shutdown: disconnect output
    println!("[infynitae] Shutting down compositor...");
    if let Ok(effects) = data
        .state
        .core_mut()
        .output_disconnected(&mut data.output_token, data.output_id)
    {
        data.state.apply(&effects);
    }
    let _ = data.display.flush_clients();

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
            let scale_120 = (scale_factor * 120.0).round().max(1.0) as u32;
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
            data.running.store(false, Ordering::SeqCst);
        }
        WinitEvent::Input(input_event) => {
            handle_input_event(data, input_event);
        }
        WinitEvent::Redraw => {
            // Prevent duplicate presentation: only render if target frame duration has elapsed (Defect C)
            let target_frame_duration = vblank_period(data.options.refresh_mhz);
            if data.last_frame.elapsed() >= target_frame_duration {
                render_and_present(data);
            }
        }
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
                if let Ok(effects) = data
                    .state
                    .core_mut()
                    .physical_input(&mut data.input_port, phys)
                {
                    data.state.apply(&effects);
                }
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
            if let Ok(effects) = data
                .state
                .core_mut()
                .physical_input(&mut data.input_port, phys)
            {
                data.state.apply(&effects);
            }
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
                if let Ok(effects) = data
                    .state
                    .core_mut()
                    .physical_input(&mut data.input_port, phys)
                {
                    data.state.apply(&effects);
                }
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
            if let Ok(effects) = data
                .state
                .core_mut()
                .physical_input(&mut data.input_port, phys)
            {
                data.state.apply(&effects);
            }
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
            if let Ok(effects) = data
                .state
                .core_mut()
                .physical_input(&mut data.input_port, phys)
            {
                data.state.apply(&effects);
            }
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
        return;
    };

    let damage = match render_res {
        Ok(Some(d)) => Some(d),
        Ok(None) => None,
        Err(e) => {
            eprintln!("[infynitae] Render error: {e:?}");
            return;
        }
    };

    // Submit buffer to GPU/host when damaged
    if let Some(damage_rects) = &damage {
        if let Err(e) = data.backend.submit(Some(damage_rects)) {
            eprintln!("[infynitae] Submit error: {e:?}");
            return;
        }
        data.frame_count += 1;
    }

    // Complete frame callbacks and presentation feedback for clocked elements
    let now = data.start_time.elapsed();
    let elements = data.state.core().frame_tick(data.output_id);
    data.state.complete_frames(data.output_id, &elements, now);
    let _ = data.display.flush_clients();

    data.last_frame = Instant::now();
}

/// Calculates logical pointer motion delta from an absolute physical position sample,
/// establishing a baseline on entry/re-entry without warping (Defect B).
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

/// Normalizes relative physical motion delta by scale factor (Defect B).
pub fn normalize_rel_motion(delta: (f64, f64), scale_120: u32) -> (f64, f64) {
    let scale = (scale_120 as f64 / 120.0).max(0.001);
    (delta.0 / scale, delta.1 / scale)
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

        // Re-entry from completely different position: baseline established, no multi-hundred-pixel warp!
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
    fn test_terminate_children_normal_exit() {
        let mut children = vec![std::process::Command::new("true").spawn().unwrap()];
        std::thread::sleep(Duration::from_millis(50));
        terminate_children(&mut children, Duration::from_millis(500));
        assert!(children.is_empty());
    }

    #[test]
    fn test_terminate_children_sigterm_handling() {
        let mut children = vec![
            std::process::Command::new("sleep")
                .arg("10")
                .spawn()
                .unwrap(),
        ];
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
            std::process::Command::new("sh")
                .args(["-c", "trap '' TERM; while true; do sleep 1; done"])
                .spawn()
                .unwrap(),
        ];
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
        let mut children = vec![
            std::process::Command::new("sleep")
                .arg("10")
                .spawn()
                .unwrap(),
        ];
        terminate_children(&mut children, Duration::from_millis(500));
        assert!(children.is_empty());
        terminate_children(&mut children, Duration::from_millis(500));
        assert!(children.is_empty());
    }

    #[test]
    fn test_terminate_children_zombie_prevention() {
        let child = std::process::Command::new("sleep")
            .arg("10")
            .spawn()
            .unwrap();
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
