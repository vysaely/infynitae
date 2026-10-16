//! EXPERIMENTAL — application probe for the first-application milestone (stage 1).
//!
//! Runs the production Core and adapter with the headless backend in this process, listening on
//! a real Wayland socket, starts ONE client program, delivers a fixed script of keyboard and
//! pointer events through the backend's input port (the same entry points the T1 tests use),
//! composites the output with the software renderer at fixed points and writes the frames out.
//! No window, no GPU, no host display: what is composited is what Infynitae would hand its
//! renderer, not what a GPU would show.
//!
//! usage: app_probe <out-dir> <socket-name> [--click X,Y] -- <client> [args...]
//! The caller provides the environment (private XDG_RUNTIME_DIR, no session bus, no DISPLAY).

use std::fs::File;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use infynitae_backend_headless::{
    VirtualOutput, connect, disconnect, keyboard_key, pointer_button, pointer_motion, vblank,
};
use infynitae_core::Core;
use infynitae_core::input::bindings::Modifiers;
use infynitae_core::navigation::PersistedCameras;
use infynitae_core::output::Transform;
use infynitae_wayland::session::{Session, SessionOptions};
use infynitae_wayland::{WaylandState, convert};
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::backend::renderer::{Bind, ExportMem, Offscreen};
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::Display;
use smithay::utils::Rectangle;

const SIZE: (i32, i32) = (1280, 800);
// evdev codes
const KEY_7: u32 = 8;
const KEY_MINUS: u32 = 12;
const KEY_2: u32 = 3;
const KEY_ENTER: u32 = 28;
const KEY_LEFTCTRL: u32 = 29;
const KEY_Q: u32 = 16;
const BTN_LEFT: u32 = 0x110;

enum Step {
    Capture(String),
    Key(u32, bool),
    /// Move the pointer to an output position (logical px) by one relative motion.
    MoveTo(f64, f64),
    Button(bool),
    /// From here on the client is expected to exit by itself.
    ExpectExit,
}

fn tap(at: u64, key: u32) -> [(u64, Step); 2] {
    [(at, Step::Key(key, true)), (at + 40, Step::Key(key, false))]
}

/// `MS:capture:NAME`, `MS:tap:KEY`, `MS:down:KEY`, `MS:up:KEY`, `MS:click:X,Y`, `MS:move:X,Y`,
/// `MS:press:X,Y` (move, then hold the left button), `MS:release`, `MS:expect-exit`, separated
/// by `;`. MS is milliseconds after the first window is mapped.
fn parse_script(text: &str) -> Result<Vec<(u64, Step)>, Box<dyn std::error::Error>> {
    let mut out = Vec::new();
    for part in text.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let mut f = part.splitn(3, ':');
        let at: u64 = f.next().ok_or("time")?.parse()?;
        let verb = f.next().ok_or("verb")?;
        let arg = f.next().unwrap_or("");
        let xy = || -> Result<(f64, f64), Box<dyn std::error::Error>> {
            let (x, y) = arg.split_once(',').ok_or("X,Y")?;
            Ok((x.parse()?, y.parse()?))
        };
        match verb {
            "capture" => out.push((at, Step::Capture(arg.to_string()))),
            "tap" => out.extend(tap(at, arg.parse()?)),
            "down" => out.push((at, Step::Key(arg.parse()?, true))),
            "up" => out.push((at, Step::Key(arg.parse()?, false))),
            "move" => {
                let (x, y) = xy()?;
                out.push((at, Step::MoveTo(x, y)));
            }
            "click" => {
                let (x, y) = xy()?;
                out.push((at, Step::MoveTo(x, y)));
                out.push((at + 150, Step::Button(true)));
                out.push((at + 200, Step::Button(false)));
            }
            "press" => {
                let (x, y) = xy()?;
                out.push((at, Step::MoveTo(x, y)));
                out.push((at + 150, Step::Button(true)));
            }
            "release" => out.push((at, Step::Button(false))),
            "expect-exit" => out.push((at, Step::ExpectExit)),
            other => return Err(format!("unknown step `{other}`").into()),
        }
    }
    out.sort_by_key(|(at, _)| *at);
    Ok(out)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let split = args
        .iter()
        .position(|a| a == "--")
        .ok_or("missing -- <client>")?;
    let (opts, client) = (&args[..split], &args[split + 1..]);
    let out_dir = std::path::PathBuf::from(&opts[0]);
    let socket = opts[1].clone();
    let option = |name: &str| {
        opts.iter()
            .position(|a| a == name)
            .and_then(|i| opts.get(i + 1))
    };

    // (milliseconds after the client's window is first mapped, step)
    let script = match option("--script") {
        Some(text) => parse_script(text)?,
        None => {
            let mut script: Vec<(u64, Step)> = vec![(1500, Step::Capture("01-mapped".into()))];
            script.extend(tap(2000, KEY_7));
            script.extend(tap(2200, KEY_MINUS));
            script.extend(tap(2400, KEY_2));
            script.push((3000, Step::Capture("02-typed-7-minus-2".into())));
            script.extend(tap(3200, KEY_ENTER));
            script.push((4000, Step::Capture("03-after-enter".into())));
            script.push((5590, Step::ExpectExit));
            script.push((5600, Step::Key(KEY_LEFTCTRL, true)));
            script.push((5650, Step::Key(KEY_Q, true)));
            script.push((5700, Step::Key(KEY_Q, false)));
            script.push((5750, Step::Key(KEY_LEFTCTRL, false)));
            script
        }
    };
    let limit = option("--seconds").map_or(Ok(20), |s| s.parse::<u64>())?;

    let (core, mut input, _shell, mut token) = Core::new(PersistedCameras::default());
    let display: Display<WaylandState> = Display::new()?;
    let mut state = WaylandState::new(&display.handle(), core);
    let output = VirtualOutput {
        key: VirtualOutput::key("PROBE-0"),
        mode: (SIZE.0 as u32, SIZE.1 as u32),
        transform: Transform::Normal,
        scale_120: 120,
        refresh_mhz: 60_000,
    };
    let output_id = connect(&mut state, &mut token, &output).map_err(|e| format!("{e:?}"))?;

    let mut event_loop: EventLoop<()> = EventLoop::try_new()?;
    let mut session = Session::new(
        display,
        &event_loop.handle(),
        &SessionOptions {
            socket_name: Some(socket.clone()),
            exec: None,
            refresh_mhz: 60_000,
            duration: Some(Duration::from_secs(limit)),
            max_frames: None,
            x_display: None,
            x_authority: None,
        },
    )?;

    let mut renderer = PixmanRenderer::new()?;
    let mut image = renderer.create_buffer(Fourcc::Argb8888, SIZE.into())?;

    let mut child = Command::new(&client[0])
        .args(&client[1..])
        .env("WAYLAND_DISPLAY", &socket)
        .stdout(Stdio::from(File::create(out_dir.join("client.out"))?))
        .stderr(Stdio::from(File::create(out_dir.join("client.err"))?))
        .spawn()?;
    let mut log = File::create(out_dir.join("probe.log"))?;
    let started = Instant::now();
    macro_rules! note {
        ($($a:tt)*) => {{
            let line = format!("[{:>6} ms] {}", started.elapsed().as_millis(), format!($($a)*));
            println!("{line}");
            writeln!(log, "{line}")?;
        }};
    }
    note!("client pid {} started: {:?}", child.id(), client);

    let mut mapped_at: Option<Instant> = None;
    let mut next = 0usize;
    let mut expect_exit = false;
    let mut exit_status = None;
    // the Core pointer starts at the centre of the first output
    let mut pointer = (SIZE.0 as f64 / 2.0, SIZE.1 as f64 / 2.0);
    let mut time_us = 1_000_000u64;

    while !session.finished() {
        let wait = session.timeout().map_or(Duration::from_millis(10), |t| {
            t.min(Duration::from_millis(10))
        });
        event_loop.dispatch(Some(wait), &mut ())?;
        session.dispatch(&mut state);

        if state.core().focus().is_initial_eligible()
            && let Some(&topmost) = state.core().stage().mapped_elements().last()
            && let Ok(effects) = state.core_mut().grant_initial_focus(&mut input, topmost)
        {
            state.apply(&effects);
            session.flush();
            note!("initial keyboard focus granted to {topmost:?}");
        }

        if mapped_at.is_none() && !state.core().stage().mapped_elements().is_empty() {
            mapped_at = Some(Instant::now());
            note!(
                "first element mapped: {:?}",
                state.core().stage().mapped_elements()
            );
        }

        while let Some(t0) = mapped_at
            && next < script.len()
            && t0.elapsed() >= Duration::from_millis(script[next].0)
        {
            time_us += 20_000;
            match &script[next].1 {
                Step::Capture(name) => {
                    let mut target = renderer.bind(&mut image)?;
                    convert::render_output(&mut state, &mut renderer, &mut target, output_id, 0)
                        .map_err(|e| format!("render_output: {e:?}"))?;
                    let mapping = renderer.copy_framebuffer(
                        &target,
                        Rectangle::from_size(SIZE.into()),
                        Fourcc::Argb8888,
                    )?;
                    let bytes = renderer.map_texture(&mapping)?;
                    let mut ppm = File::create(out_dir.join(format!("{name}.ppm")))?;
                    write!(ppm, "P6\n{} {}\n255\n", SIZE.0, SIZE.1)?;
                    let mut rgb = Vec::with_capacity(bytes.len() / 4 * 3);
                    for px in bytes.as_chunks::<4>().0 {
                        // ARGB8888 little-endian: B, G, R, A
                        rgb.extend_from_slice(&[px[2], px[1], px[0]]);
                    }
                    ppm.write_all(&rgb)?;
                    let entries = state
                        .core_mut()
                        .render_list(output_id)
                        .map_err(|e| format!("{e:?}"))?
                        .entries
                        .len();
                    note!("captured {name}: {entries} render-list entries");
                    note!("  protocol fullscreen: {:?}", state.core().fullscreen());
                    for id in state.core().stage().mapped_elements() {
                        let anchor = state.core().stage().anchor(id);
                        let geometry = state.core().stage().tree(id).map(|t| t.geometry.size);
                        let surfaces = state
                            .core()
                            .stage()
                            .tree(id)
                            .map_or(0, |t| t.surfaces.len());
                        note!(
                            "  {id:?}: anchor {anchor:?}, window geometry {geometry:?}, {surfaces} surface(s), observers {:?}, keyboard focus {}",
                            state.core().membership().observers(id),
                            state.core().focus().target() == Some(id)
                        );
                    }
                }
                Step::Key(key, pressed) => {
                    keyboard_key(&mut state, &mut input, *key, *pressed, time_us)
                        .map_err(|e| format!("{e:?}"))?;
                    note!("key {key} {}", if *pressed { "down" } else { "up" });
                }
                Step::MoveTo(x, y) => {
                    let d = (x - pointer.0, y - pointer.1);
                    pointer_motion(&mut state, &mut input, d, d, time_us)
                        .map_err(|e| format!("{e:?}"))?;
                    pointer = (*x, *y);
                    note!("pointer moved to ({x}, {y})");
                }
                Step::Button(pressed) => {
                    pointer_button(
                        &mut state,
                        &mut input,
                        BTN_LEFT,
                        *pressed,
                        Modifiers::default(),
                        time_us,
                    )
                    .map_err(|e| format!("{e:?}"))?;
                    note!("left button {}", if *pressed { "down" } else { "up" });
                }
                Step::ExpectExit => {
                    expect_exit = true;
                    note!("delivering Ctrl+Q; the client is expected to exit by itself");
                }
            }
            session.request_frame();
            session.flush();
            next += 1;
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

        if exit_status.is_none()
            && let Some(status) = child.try_wait()?
        {
            exit_status = Some(status);
            note!(
                "client exited: {status} ({})",
                if expect_exit {
                    "after the quit shortcut"
                } else {
                    "BEFORE the quit shortcut"
                }
            );
        }
        if exit_status.is_some() {
            // let the adapter see the disconnect, then look at what is left
            for _ in 0..20 {
                event_loop.dispatch(Some(Duration::from_millis(10)), &mut ())?;
                session.dispatch(&mut state);
            }
            break;
        }
    }

    let core = state.core();
    note!(
        "after client exit: elements on the Stage {}, mapped {}, membership rows {}, keyboard focus {:?}",
        core.stage().z_order().len(),
        core.stage().mapped_elements().len(),
        core.membership().rows().len(),
        core.focus().target()
    );
    if exit_status.is_none() {
        note!("client still running at the end of the session: terminating it (NOT a clean close)");
        let _ = child.kill();
        let _ = child.wait();
    }
    session.shutdown();
    let _ = disconnect(&mut state, &mut token, output_id);
    session.flush();
    drop(session);
    drop(event_loop);
    let rt = std::env::var("XDG_RUNTIME_DIR")?;
    let left: Vec<_> = std::fs::read_dir(&rt)?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name())
        .collect();
    note!("runtime directory after shutdown: {left:?}");
    Ok(())
}
