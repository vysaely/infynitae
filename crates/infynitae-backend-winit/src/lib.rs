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
    PointerAxisEvent, PointerButtonEvent,
};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::winit::{self, WinitEvent, WinitGraphicsBackend};
use smithay::reexports::calloop::EventLoop;
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
            duration: None,
            max_frames: None,
        }
    }
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
    frame_count: u64,
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
    let display: Display<WaylandState> = Display::new()?;
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
        refresh_mhz: 60_000,
    };
    let (output_id, effects) = state
        .core_mut()
        .output_connected(&mut output_token, descriptor)
        .map_err(|e| format!("Failed to connect output: {e:?}"))?;
    state.apply(&effects);

    // 5. Spawn client command if requested
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
        frame_count: 0,
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
    let target_frame_duration = Duration::from_micros(16_667); // ~60 Hz
    let mut last_frame = Instant::now();

    while running.load(Ordering::SeqCst) {
        // Run calloop with timeout to pace frames
        let elapsed = last_frame.elapsed();
        let timeout = target_frame_duration.saturating_sub(elapsed);

        event_loop.dispatch(Some(timeout), &mut data)?;

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

        // Render frame if frame interval has elapsed
        if last_frame.elapsed() >= target_frame_duration {
            render_and_present(&mut data);
            last_frame = Instant::now();
        }
    }

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
            let scale_120 = (scale_factor * 120.0).round().max(1.0) as u32;
            let descriptor = OutputDescriptor {
                key: data.output_key.clone(),
                mode: (width, height),
                transform: Transform::Normal,
                scale_120,
                refresh_mhz: 60_000,
            };
            if let Ok(effects) = data.state.core_mut().output_reconfigured(
                &mut data.output_token,
                data.output_id,
                descriptor,
            ) {
                data.state.apply(&effects);
            }
        }
        WinitEvent::CloseRequested => {
            data.running.store(false, Ordering::SeqCst);
        }
        WinitEvent::Input(input_event) => {
            handle_input_event(data, input_event);
        }
        WinitEvent::Redraw => {
            render_and_present(data);
        }
        _ => {}
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
        InputEvent::PointerMotion { event: _ } => {
            // Winit relative pointer motion
        }
        InputEvent::PointerMotionAbsolute { event } => {
            let pos = (event.x(), event.y());
            let delta = match data.last_abs_pos {
                Some(prev) => (pos.0 - prev.0, pos.1 - prev.1),
                None => (0.0, 0.0),
            };
            data.last_abs_pos = Some(pos);
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
    let damage_slice = {
        if let Ok((renderer, mut framebuffer)) = data.backend.bind() {
            let damage = convert::render_output(
                &mut data.state,
                renderer,
                &mut framebuffer,
                data.output_id,
                age,
            );
            let now = data.start_time.elapsed();
            let elements = data.state.core().frame_tick(data.output_id);
            data.state.complete_frames(data.output_id, &elements, now);
            match damage {
                Ok(Some(d)) => Some(d),
                _ => None,
            }
        } else {
            None
        }
    };

    let damage_ref = damage_slice.as_deref();
    let _ = data.backend.submit(damage_ref);
    data.frame_count += 1;
}
