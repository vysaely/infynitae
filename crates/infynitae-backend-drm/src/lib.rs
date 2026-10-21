//! Infynitae Native console backend: the compositor owns the display and the input devices of a
//! seat, started from a text console (FOUNDATION_ARCHITECTURE C1.1; owner decisions T1–T9 of
//! 2026-10-22, `~/infynitae/TTY_BACKEND_DECISIONS_PROPOSAL_2026-10-21.md`). EXPERIMENTAL, like the
//! rest of the runtime until INC-13.
//!
//! - **Session (T2):** libseat opens the GPU and the input devices without root; a VT switch pauses
//!   the session (input suspended, DRM master dropped) and resumes it.
//! - **Rendering (T3):** GLES on GBM/EGL through Smithay's `DrmCompositor`, page-flipped on vblank.
//!   Clients' dmabufs are not imported yet (INC-13): shm clients only.
//! - **Screens (T5):** every connector connected at start, at its preferred mode, on the primary
//!   GPU. Hotplug is not handled yet.
//! - **Input (T4):** libinput on the seat; the keyboard layout from `XKB_DEFAULT_*` (xkbcommon's
//!   defaults otherwise).
//! - **Safety (T6):** Ctrl+Alt+Backspace quits; Ctrl+Alt+F1…F12 switch VT. Both are taken before
//!   Core sees the key, so no binding or client can swallow them.
//! - **Cursor (T8):** drawn by the compositor (`infynitae_wayland::cursor`).

#![deny(unsafe_code)]

use std::error::Error;
use std::path::PathBuf;
use std::time::Duration;

use infynitae_core::output::{OutputDescriptor, OutputId, OutputKey, Transform};
use infynitae_core::{Core, InputPort, OutputLifecycleToken, PhysicalEvent};
use infynitae_wayland::session::{Session, SessionOptions};
use infynitae_wayland::{WaylandState, convert};

use smithay::backend::allocator::Fourcc;
use smithay::backend::allocator::gbm::{GbmAllocator, GbmBufferFlags, GbmDevice};
use smithay::backend::drm::compositor::{DrmCompositor, FrameFlags};
use smithay::backend::drm::exporter::gbm::GbmFramebufferExporter;
use smithay::backend::drm::{DrmDevice, DrmDeviceFd, DrmEvent};
use smithay::backend::egl::{EGLContext, EGLDisplay};
use smithay::backend::input::{
    Axis, AxisSource, ButtonState, Event, InputEvent, KeyState, KeyboardKeyEvent, PointerAxisEvent,
    PointerButtonEvent, PointerMotionEvent,
};
use smithay::backend::libinput::{LibinputInputBackend, LibinputSessionInterface};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::session::libseat::LibSeatSession;
use smithay::backend::session::{Event as SeatEvent, Session as _};
use smithay::output::OutputModeSource;
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::drm::control::{Device as _, ModeTypeFlags, connector, crtc};
use smithay::reexports::input::Libinput;
use smithay::reexports::rustix::fs::OFlags;
use smithay::reexports::wayland_server::Display;
use smithay::utils::DeviceFd;

/// Evdev codes of the keys the backend takes for itself (T6).
const KEY_BACKSPACE: u32 = 14;
const KEY_F1: u32 = 59;
const KEY_F10: u32 = 68;
const KEY_F11: u32 = 87;
const KEY_F12: u32 = 88;

/// Options of the console compositor.
#[derive(Clone, Debug, Default)]
pub struct ConsoleOptions {
    /// Wayland socket name; `None` picks a free one.
    pub socket_name: Option<String>,
    /// A command started once the compositor is ready.
    pub exec: Option<String>,
    /// Exit after this long (tests, automation).
    pub duration: Option<Duration>,
    /// Exit after this many frames.
    pub max_frames: Option<u64>,
    /// xwayland-satellite to supervise (INC-12, experimental); `None`: no X11.
    pub xwayland_satellite: Option<PathBuf>,
    /// Output scale ×120 for every screen (`None`: 120, i.e. 1.0).
    pub scale_120: Option<u32>,
}

/// What the console takes before Core sees a key (T6), given the evdev code and whether Ctrl and
/// Alt are held: quit, switch to a VT, or nothing. Pure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleKey {
    Quit,
    SwitchVt(i32),
}

pub fn console_key(key: u32, ctrl: bool, alt: bool) -> Option<ConsoleKey> {
    if !(ctrl && alt) {
        return None;
    }
    match key {
        KEY_BACKSPACE => Some(ConsoleKey::Quit),
        KEY_F1..=KEY_F10 => Some(ConsoleKey::SwitchVt((key - KEY_F1 + 1) as i32)),
        KEY_F11 => Some(ConsoleKey::SwitchVt(11)),
        KEY_F12 => Some(ConsoleKey::SwitchVt(12)),
        _ => None,
    }
}

/// A mode's refresh in mHz, from its pixel clock (kHz) and totals; the rounded `vrefresh` when the
/// totals are unknown. Pure.
pub fn refresh_mhz(clock_khz: u32, htotal: u16, vtotal: u16, vrefresh: u32) -> u32 {
    let total = u64::from(htotal) * u64::from(vtotal);
    if total == 0 {
        return vrefresh.saturating_mul(1000);
    }
    (u64::from(clock_khz) * 1_000_000 / total).min(u64::from(u32::MAX)) as u32
}

/// One mode as the choice needs it: size, refresh (mHz), whether the monitor prefers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeInfo {
    pub size: (u16, u16),
    pub refresh_mhz: u32,
    pub preferred: bool,
}

/// The mode to use (T5, refined after the first console run, 2026-10-22: a 165 Hz monitor
/// prefers 1920×1080 at 60 Hz): the preferred mode's resolution (the first mode's without one),
/// at the highest refresh the monitor offers for it. `None` without modes. Pure.
pub fn choose_mode(modes: &[ModeInfo]) -> Option<usize> {
    let size = modes
        .iter()
        .find(|m| m.preferred)
        .or_else(|| modes.first())?
        .size;
    modes
        .iter()
        .enumerate()
        .filter(|(_, m)| m.size == size)
        .max_by_key(|(_, m)| m.refresh_mhz)
        .map(|(i, _)| i)
}

type Compositor =
    DrmCompositor<GbmAllocator<DrmDeviceFd>, GbmFramebufferExporter<DrmDeviceFd>, (), DrmDeviceFd>;

/// One screen: its Core output, its CRTC and its compositor.
struct Screen {
    id: OutputId,
    crtc: crtc::Handle,
    size: (i32, i32),
    compositor: Compositor,
    /// The k the compositor's mode source was set with.
    k: f64,
    /// A frame is queued and its vblank has not come yet.
    pending: bool,
}

struct LoopData {
    state: WaylandState,
    session: Session,
    seat: LibSeatSession,
    input_port: InputPort,
    output_token: OutputLifecycleToken,
    drm: DrmDevice,
    renderer: GlesRenderer,
    screens: Vec<Screen>,
    libinput: Libinput,
    scale_120: u32,
    /// The session is active (not switched away).
    active: bool,
    quit: bool,
    /// Keys taken by the console (T6): their releases are not passed on either.
    taken: Vec<u32>,
}

/// The device limit Core is told: assumed, not queried, as in the nested backend (OB-52, INC-13).
struct AssumedGlesLimits;

impl infynitae_wayland::limits::RendererLimits for AssumedGlesLimits {
    fn max_buffer_side(&self) -> Result<u32, infynitae_wayland::limits::LimitQueryError> {
        Ok(infynitae_core::scale::DEFAULT_D_MAX)
    }
}

/// The GLES renderer on the GPU's GBM device. Smithay marks its EGL display and renderer
/// constructors as not safe to call unchecked (U-007, `docs/foundation/UNSAFE_INVENTORY.md`).
#[allow(unsafe_code)]
fn gles_renderer(gbm: &GbmDevice<DrmDeviceFd>) -> Result<GlesRenderer, Box<dyn Error>> {
    // SAFETY: the display is made from a GBM device that stays open for the renderer's lifetime
    // (the device and its fd are owned by the loop data, which outlives the renderer); EGL is
    // used on this thread only.
    let display = unsafe { EGLDisplay::new(gbm.clone()) }?;
    let context = EGLContext::new(&display)?;
    // SAFETY: the context was just made and is current on no other thread; the renderer takes it
    // over and is used on this thread only.
    let renderer = unsafe { GlesRenderer::new(context) }?;
    Ok(renderer)
}

/// Runs the compositor on the console, with an optional loop extension (the shell server). Blocks
/// until Ctrl+Alt+Backspace, a signal or a run limit.
pub fn run_console_with(
    core: Core,
    input_port: InputPort,
    mut output_token: OutputLifecycleToken,
    options: ConsoleOptions,
    extension: Option<infynitae_wayland::extension::ExtensionFactory>,
) -> Result<(), Box<dyn Error>> {
    let mut event_loop: EventLoop<LoopData> = EventLoop::try_new()?;
    let display: Display<WaylandState> = Display::new()?;
    let mut state = WaylandState::new(&display.handle(), core);
    state.set_console_target();

    // the seat first: without it nothing can be opened
    let (mut seat, seat_notifier) = LibSeatSession::new()
        .map_err(|e| format!("no seat (is this a console you are logged in on?): {e}"))?;
    let seat_name = seat.seat();
    println!("[infynitae] Seat {seat_name}");

    let gpu = smithay::backend::udev::primary_gpu(&seat_name)?
        .or_else(|| {
            smithay::backend::udev::all_gpus(&seat_name)
                .ok()
                .and_then(|g| g.into_iter().next())
        })
        .ok_or("no GPU on this seat")?;
    println!("[infynitae] GPU {}", gpu.display());
    let fd = seat.open(
        &gpu,
        OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
    )?;
    let fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let (mut drm, drm_notifier) = DrmDevice::new(fd.clone(), true)?;
    let gbm = GbmDevice::new(fd.clone())?;
    let renderer = gles_renderer(&gbm)?;
    let allocator = GbmAllocator::new(
        gbm.clone(),
        GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
    );
    let exporter = GbmFramebufferExporter::new(gbm.clone(), None);
    let render_formats = renderer
        .egl_context()
        .dmabuf_render_formats()
        .iter()
        .copied()
        .collect::<Vec<_>>();
    let scale_120 = options.scale_120.unwrap_or(120);

    // every connected connector at its preferred mode, each on a CRTC of its own (T5)
    let resources = drm.resource_handles()?;
    let mut used: Vec<crtc::Handle> = Vec::new();
    let mut screens = Vec::new();
    for &handle in resources.connectors() {
        let conn = drm.get_connector(handle, true)?;
        if conn.state() != connector::State::Connected {
            continue;
        }
        let infos: Vec<ModeInfo> = conn
            .modes()
            .iter()
            .map(|m| ModeInfo {
                size: m.size(),
                refresh_mhz: refresh_mhz(m.clock(), m.hsync().2, m.vsync().2, m.vrefresh()),
                preferred: m.mode_type().contains(ModeTypeFlags::PREFERRED),
            })
            .collect();
        let Some(mode) = choose_mode(&infos).map(|i| conn.modes()[i]) else {
            continue;
        };
        let crtc = conn
            .encoders()
            .iter()
            .filter_map(|e| drm.get_encoder(*e).ok())
            .flat_map(|e| resources.filter_crtcs(e.possible_crtcs()))
            .find(|c| !used.contains(c));
        let Some(crtc) = crtc else {
            println!("[infynitae] No free CRTC for {:?}", conn.interface());
            continue;
        };
        used.push(crtc);
        let name = format!("{:?}-{}", conn.interface(), conn.interface_id());
        let (w, h) = mode.size();
        let refresh = refresh_mhz(
            mode.clock(),
            mode.hsync().2,
            mode.vsync().2,
            mode.vrefresh(),
        );
        println!(
            "[infynitae] Screen {name}: {w}x{h} @ {}.{:03} Hz",
            refresh / 1000,
            refresh % 1000
        );
        let surface = drm.create_surface(crtc, mode, &[handle])?;
        let descriptor = OutputDescriptor {
            key: OutputKey {
                device: gpu.display().to_string(),
                connector: name.clone(),
                make: "Unknown".into(),
                model: "Unknown".into(),
                serial: None,
            },
            mode: (u32::from(w), u32::from(h)),
            transform: Transform::Normal,
            scale_120,
            refresh_mhz: refresh,
        };
        let (id, fx) = state
            .core_mut()
            .output_connected(&mut output_token, descriptor)
            .map_err(|e| format!("output {name}: {e:?}"))?;
        state.apply(&fx);
        let k = state.core().view(id).map_or(1.0, |v| v.k);
        let size = (i32::from(w), i32::from(h));
        let compositor = Compositor::new(
            OutputModeSource::Static {
                size: size.into(),
                scale: smithay::utils::Scale::from(k),
                transform: smithay::utils::Transform::Normal,
            },
            surface,
            None,
            allocator.clone(),
            exporter.clone(),
            [Fourcc::Xrgb8888, Fourcc::Argb8888],
            render_formats.iter().copied(),
            drm.cursor_size(),
            Some(gbm.clone()),
        )
        .map_err(|e| format!("screen {name}: {e:?}"))?;
        screens.push(Screen {
            id,
            crtc,
            size,
            compositor,
            k,
            pending: false,
        });
    }
    if screens.is_empty() {
        return Err("no connected screen".into());
    }
    state.report_renderer_limits(&AssumedGlesLimits)?;

    let mut libinput = Libinput::new_with_udev(LibinputSessionInterface::from(seat.clone()));
    libinput
        .udev_assign_seat(&seat_name)
        .map_err(|()| "libinput could not take the seat")?;

    let x_display =
        infynitae_wayland::xwayland::reserve_for_session(options.xwayland_satellite.as_deref())?;
    let session = Session::new(
        display,
        &event_loop.handle(),
        &SessionOptions {
            socket_name: options.socket_name.clone(),
            exec: options.exec.clone(),
            refresh_mhz: screens
                .iter()
                .filter_map(|s| state.core().output(s.id).map(|m| m.refresh_mhz))
                .max()
                .unwrap_or(60_000),
            duration: options.duration,
            max_frames: options.max_frames,
            x_display: x_display.as_ref().map(|d| d.name()),
            x_authority: x_display
                .as_ref()
                .and_then(|d| d.authority_path().map(|p| p.to_path_buf())),
        },
    )?;
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

    let handle = event_loop.handle();
    handle
        .insert_source(seat_notifier, |event, _, data| match event {
            SeatEvent::PauseSession => {
                println!("[infynitae] Session paused (VT switched away)");
                data.libinput.suspend();
                data.drm.pause();
                data.active = false;
            }
            SeatEvent::ActivateSession => {
                println!("[infynitae] Session resumed");
                if data.libinput.resume().is_err() {
                    eprintln!("[infynitae] libinput did not resume");
                }
                if let Err(e) = data.drm.activate(false) {
                    eprintln!("[infynitae] DRM did not resume: {e}");
                }
                for s in &mut data.screens {
                    let _ = s.compositor.reset_state();
                    s.pending = false;
                }
                data.active = true;
                data.session.request_frame();
            }
        })
        .map_err(|e| format!("seat events: {e}"))?;
    handle
        .insert_source(drm_notifier, |event, _, data| match event {
            DrmEvent::VBlank(crtc) => {
                if let Some(s) = data.screens.iter_mut().find(|s| s.crtc == crtc) {
                    let _ = s.compositor.frame_submitted();
                    s.pending = false;
                    let id = s.id;
                    let now = infynitae_wayland::presentation_clock_now();
                    data.state.frame_tick(id, now);
                    data.session.request_frame();
                }
            }
            DrmEvent::Error(e) => eprintln!("[infynitae] DRM error: {e}"),
        })
        .map_err(|e| format!("DRM events: {e}"))?;
    handle
        .insert_source(
            LibinputInputBackend::new(libinput.clone()),
            |event, _, data| {
                handle_input(data, event);
                data.session.request_frame();
            },
        )
        .map_err(|e| format!("input events: {e}"))?;

    let mut data = LoopData {
        state,
        session,
        seat,
        input_port,
        output_token,
        drm,
        renderer,
        screens,
        libinput,
        scale_120,
        active: true,
        quit: false,
        taken: Vec::new(),
    };

    println!("[infynitae] Compositor running. Ctrl+Alt+Backspace quits.");
    while !data.quit && !data.session.finished() {
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
        if let Some(ext) = extension.as_mut() {
            let (fx, changed) = ext.service(data.state.core_mut());
            data.state.apply(&fx);
            if changed {
                data.session.request_frame();
            }
        }
        data.session.dispatch(&mut data.state);

        if data.state.core().focus().is_initial_eligible()
            && let Some(&topmost) = data.state.core().stage().mapped_elements().last()
            && let Ok(effects) = data
                .state
                .core_mut()
                .grant_initial_focus(&mut data.input_port, topmost)
        {
            data.state.apply(&effects);
            data.session.flush();
        }

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

        if data.session.frame_due() && data.active {
            render(&mut data);
        }

        let now = infynitae_wayland::presentation_clock_now();
        let deadline = data.state.frame_fallback(now);
        data.session.set_fallback_deadline(deadline, now);
        data.session.flush();
    }

    println!("[infynitae] Shutting down compositor...");
    drop(xwayland);
    data.session.shutdown();
    for s in std::mem::take(&mut data.screens) {
        if let Ok(fx) = data
            .state
            .core_mut()
            .output_disconnected(&mut data.output_token, s.id)
        {
            data.state.apply(&fx);
        }
    }
    data.session.flush();
    println!("[infynitae] Compositor exited cleanly.");
    Ok(())
}

/// One frame on every screen that is not waiting for its vblank. A screen whose frame changed
/// nothing completes its frame callbacks now; a queued one at its vblank.
fn render(data: &mut LoopData) {
    let clear = data.state.clear_color();
    for i in 0..data.screens.len() {
        if data.screens[i].pending {
            continue;
        }
        let id = data.screens[i].id;
        let (elements, k) = match convert::output_elements(&mut data.state, &mut data.renderer, id)
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[infynitae] Frame elements: {e:?}");
                continue;
            }
        };
        let s = &mut data.screens[i];
        if s.k != k {
            s.compositor
                .set_output_mode_source(OutputModeSource::Static {
                    size: s.size.into(),
                    scale: smithay::utils::Scale::from(k),
                    transform: smithay::utils::Transform::Normal,
                });
            s.k = k;
        }
        let queued = match s.compositor.render_frame(
            &mut data.renderer,
            &elements,
            clear,
            FrameFlags::DEFAULT,
        ) {
            Ok(result) if !result.is_empty => match s.compositor.queue_frame(()) {
                Ok(()) => true,
                Err(e) => {
                    eprintln!("[infynitae] Queue frame: {e:?}");
                    false
                }
            },
            Ok(_) => false,
            Err(e) => {
                eprintln!("[infynitae] Render: {e:?}");
                false
            }
        };
        s.pending = queued;
        if queued {
            data.session.count_frame();
        } else {
            let now = infynitae_wayland::presentation_clock_now();
            data.state.frame_tick(id, now);
        }
    }
    data.session.flush();
}

fn handle_input(data: &mut LoopData, event: InputEvent<LibinputInputBackend>) {
    match event {
        InputEvent::Keyboard { event } => {
            let key = event.key_code().raw().saturating_sub(8);
            let pressed = matches!(event.state(), KeyState::Pressed);
            // T6: the console's own keys, before Core (and so before any binding or client)
            if pressed {
                let m = data.state.seat_modifiers();
                match console_key(key, m.ctrl, m.alt) {
                    Some(ConsoleKey::Quit) => {
                        println!("[infynitae] Ctrl+Alt+Backspace: quitting");
                        data.quit = true;
                        data.taken.push(key);
                        return;
                    }
                    Some(ConsoleKey::SwitchVt(vt)) => {
                        if let Err(e) = data.seat.change_vt(vt) {
                            eprintln!("[infynitae] VT {vt}: {e}");
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
        InputEvent::PointerButton { event } => {
            let phys = PhysicalEvent::PointerButton {
                button: event.button_code(),
                pressed: matches!(event.state(), ButtonState::Pressed),
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
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T6: Ctrl+Alt+Backspace quits and Ctrl+Alt+F1…F12 switch VT, with both modifiers held; any
    /// other key, or a missing modifier, is the session's.
    #[test]
    fn the_console_keys_need_ctrl_and_alt() {
        assert_eq!(
            console_key(KEY_BACKSPACE, true, true),
            Some(ConsoleKey::Quit)
        );
        assert_eq!(
            console_key(KEY_F1, true, true),
            Some(ConsoleKey::SwitchVt(1))
        );
        assert_eq!(
            console_key(KEY_F10, true, true),
            Some(ConsoleKey::SwitchVt(10))
        );
        assert_eq!(
            console_key(KEY_F11, true, true),
            Some(ConsoleKey::SwitchVt(11))
        );
        assert_eq!(
            console_key(KEY_F12, true, true),
            Some(ConsoleKey::SwitchVt(12))
        );
        assert_eq!(console_key(KEY_BACKSPACE, true, false), None);
        assert_eq!(console_key(KEY_BACKSPACE, false, true), None);
        assert_eq!(console_key(KEY_F1, false, false), None);
        assert_eq!(
            console_key(30, true, true),
            None,
            "Ctrl+Alt+A is the session's"
        );
    }

    /// A 165 Hz monitor that prefers 1920×1080 at 60 Hz runs at 165 Hz, at the preferred
    /// resolution (not a larger mode's); without a preferred mode, the first mode's resolution.
    #[test]
    fn the_preferred_resolution_at_its_highest_refresh() {
        let m = |w, h, hz: u32, preferred| ModeInfo {
            size: (w, h),
            refresh_mhz: hz * 1000,
            preferred,
        };
        let modes = [
            m(1920, 1080, 60, true),
            m(2560, 1440, 144, false),
            m(1920, 1080, 165, false),
            m(1920, 1080, 144, false),
            m(1280, 720, 240, false),
        ];
        assert_eq!(choose_mode(&modes), Some(2));
        let unflagged = [
            m(1920, 1080, 60, false),
            m(1920, 1080, 120, false),
            m(1280, 720, 240, false),
        ];
        assert_eq!(choose_mode(&unflagged), Some(1));
        assert_eq!(choose_mode(&[]), None);
    }

    /// The refresh from the pixel clock: 1920×1080 CEA timing (2200 × 1125 at 148.5 MHz) is
    /// exactly 60 Hz; 59.94 Hz (148.352 MHz) is not rounded up; unknown totals fall back to
    /// `vrefresh`.
    #[test]
    fn refresh_comes_from_the_pixel_clock() {
        assert_eq!(refresh_mhz(148_500, 2200, 1125, 60), 60_000);
        assert_eq!(refresh_mhz(148_352, 2200, 1125, 60), 59_940);
        assert_eq!(refresh_mhz(0, 0, 0, 75), 75_000);
    }
}
