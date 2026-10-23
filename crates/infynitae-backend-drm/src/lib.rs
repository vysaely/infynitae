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

use std::collections::HashSet;
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
    AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputEvent, KeyState,
    KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
};
use smithay::backend::libinput::{LibinputInputBackend, LibinputSessionInterface};
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::backend::renderer::{Renderer, TextureFilter};
use smithay::backend::renderer::{Bind, ExportMem, Offscreen, Texture};
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::session::libseat::LibSeatSession;
use smithay::backend::session::{Event as SeatEvent, Session as _};
use smithay::output::OutputModeSource;
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::drm::control::{Device as _, ModeTypeFlags, connector, crtc};
use smithay::reexports::input::Libinput;
use smithay::reexports::rustix::fs::OFlags;
use smithay::reexports::wayland_server::Display;
use smithay::reexports::wayland_server::Resource;
use smithay::utils::{DeviceFd, Rectangle};
use smithay::wayland::shm::with_buffer_contents_mut;
use infynitae_wayland::screencopy::Flags;

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
    /// Scaling texture filter ("nearest" for 0% blur, "linear" for smooth).
    pub scaling_filter: Option<String>,
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
    screencopy_texture: Option<GlesTexture>,
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
    /// Active keyboard devices for hardware LED synchronization (Caps Lock, Num Lock).
    keyboards: Vec<smithay::reexports::input::Device>,
    /// Last absolute pointer logical position, used to compute delta for absolute devices (e.g. RustDesk, tablets).
    last_abs_pointer: Option<(f64, f64)>,
    /// Set of known mapped element IDs for detecting newly mapped windows.
    known_mapped: HashSet<infynitae_core::stage::ElementId>,
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

    let gpu = std::env::var("INFYNITAE_DRM_DEVICE")
        .or_else(|_| std::env::var("WLR_DRM_DEVICES"))
        .or_else(|_| std::env::var("DRM_DEVICE"))
        .ok()
        .map(PathBuf::from)
        .or_else(|| smithay::backend::udev::primary_gpu(&seat_name).ok().flatten())
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
    let mut renderer = gles_renderer(&gbm)?;

    // Zero-blur scaling: use Nearest-neighbor filtering on upscaled surfaces by default
    let use_nearest = match options.scaling_filter.as_deref() {
        Some("linear") => false,
        _ => true,
    };
    if use_nearest {
        if let Err(e) = renderer.upscale_filter(TextureFilter::Nearest) {
            eprintln!("[infynitae] Warning: failed to set upscale filter to Nearest: {e:?}");
        }
    }
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
            screencopy_texture: None,
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
        keyboards: Vec::new(),
        last_abs_pointer: None,
        known_mapped: HashSet::new(),
    };

    println!("[infynitae] Compositor running. Ctrl+Alt+Backspace quits.");
    while !data.quit && !data.session.finished() {
        data.session
            .wake_at(data.state.core().next_settlement_deadline());
        let restart = xwayland
            .as_mut()
            .and_then(|x| x.poll(std::time::Instant::now()));
        let has_screencopy = data.screens.iter().any(|s| data.state.screencopy.has_pending(s.id));
        if has_screencopy {
            data.session.request_frame();
        }
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
                let _ = data.state.with_effects(|core, fx| {
                    core.recenter_element(port, new_id, fx)
                });
                data.session.request_frame();
                data.session.flush();
            }
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

        let has_screencopy = data.screens.iter().any(|s| data.state.screencopy.has_pending(s.id));
        if (data.session.frame_due() || has_screencopy) && data.active {
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
        let id = data.screens[i].id;
        let size = data.screens[i].size;

        // Screencopy frame capture if requested (Discord, OBS, WebRTC, RustDesk).
        // Screencopy renders to an offscreen buffer and does not depend on DRM page flip / vblank.
        // Handling this first ensures pending screencopy requests are consumed immediately,
        // preventing the event loop from busy-spinning while DRM page flip is pending.
        handle_screencopy(
            &mut data.state,
            &mut data.renderer,
            &mut data.screens[i].screencopy_texture,
            id,
            size,
            clear,
        );

        if data.screens[i].pending {
            continue;
        }

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
            FrameFlags::empty(),
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

#[allow(unsafe_code)]
fn handle_screencopy(
    state: &mut WaylandState,
    renderer: &mut GlesRenderer,
    screencopy_texture: &mut Option<GlesTexture>,
    output_id: OutputId,
    size: (i32, i32),
    clear: smithay::backend::renderer::Color32F,
) {
    if !state.screencopy.has_pending(output_id) {
        return;
    }
    let pending = state.screencopy.take_pending(output_id);
    if pending.is_empty() {
        return;
    }

    let needs_cursor = !state.screencopy.hide_cursor() && pending.iter().any(|p| p.overlay_cursor);
    let (elements, k) = match convert::output_elements_opt(state, renderer, output_id, needs_cursor) {
        Ok(res) => res,
        Err(e) => {
            eprintln!("[screencopy] Failed to get elements: {e:?}");
            for p in pending {
                p.frame.failed();
            }
            return;
        }
    };

    if screencopy_texture.as_ref().is_none_or(|t| t.width() != size.0 as u32 || t.height() != size.1 as u32) {
        match Offscreen::<GlesTexture>::create_buffer(renderer, Fourcc::Argb8888, size.into()) {
            Ok(b) => *screencopy_texture = Some(b),
            Err(e) => {
                eprintln!("[screencopy] Failed to create offscreen buffer: {e:?}");
                for p in pending {
                    p.frame.failed();
                }
                return;
            }
        }
    }

    let offscreen = screencopy_texture.as_mut().unwrap();
    let mut framebuffer = match renderer.bind(offscreen) {
        Ok(fb) => fb,
        Err(e) => {
            eprintln!("[screencopy] Failed to bind offscreen buffer: {e:?}");
            for p in pending {
                p.frame.failed();
            }
            return;
        }
    };

    let mut damage_tracker = OutputDamageTracker::new(size, smithay::utils::Scale::from(k), smithay::utils::Transform::Normal);
    if let Err(e) = damage_tracker.render_output(renderer, &mut framebuffer, 0, &elements, clear) {
        eprintln!("[screencopy] Failed to render output: {e:?}");
        for p in pending {
            p.frame.failed();
        }
        return;
    }

    let mapping = match ExportMem::copy_framebuffer(renderer, &framebuffer, Rectangle::from_size(size.into()), Fourcc::Argb8888) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[screencopy] Failed to copy framebuffer: {e:?}");
            for p in pending {
                p.frame.failed();
            }
            return;
        }
    };

    let slice = match ExportMem::map_texture(renderer, &mapping) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[screencopy] Failed to map texture: {e:?}");
            for p in pending {
                p.frame.failed();
            }
            return;
        }
    };

    let now = infynitae_wayland::presentation_clock_now();
    let tv_sec_hi = (now.as_secs() >> 32) as u32;
    let tv_sec_lo = (now.as_secs() & 0xFFFFFFFF) as u32;
    let tv_nsec = now.subsec_nanos();

    let src_stride = (size.0 * 4) as usize;

    for req in pending {
        let (rx, ry) = req
            .region
            .map(|(x, y, _, _)| (x.max(0) as usize, y.max(0) as usize))
            .unwrap_or((0, 0));

        let copy_result = with_buffer_contents_mut(&req.buffer, |ptr, len, buffer_data| {
            if buffer_data.format != smithay::reexports::wayland_server::protocol::wl_shm::Format::Argb8888
                && buffer_data.format != smithay::reexports::wayland_server::protocol::wl_shm::Format::Xrgb8888
            {
                return false;
            }
            let h = (buffer_data.height.max(0) as usize).min(size.1 as usize);
            let w_bytes = (buffer_data.width.max(0) as usize).min(size.0 as usize) * 4;
            let dest_stride = buffer_data.stride.max(0) as usize;
            let offset = buffer_data.offset.max(0) as usize;

            if dest_stride < w_bytes {
                return false;
            }
            let total_required = match dest_stride
                .checked_mul(h.saturating_sub(1))
                .and_then(|v| v.checked_add(w_bytes))
                .and_then(|v| v.checked_add(offset))
            {
                Some(bytes) => bytes,
                None => return false,
            };
            if total_required > len {
                return false;
            }

            // SAFETY: bounds verified: `dest_stride >= w_bytes`, `total_required <= len`,
            // and `src_offset + w_bytes <= slice.len()`.
            unsafe {
                let dest_base = ptr.add(offset);
                for row in 0..h {
                    let src_row_idx = (ry + row).min(size.1.max(0) as usize);
                    let src_offset = src_row_idx * src_stride + rx * 4;
                    if src_offset + w_bytes <= slice.len() {
                        let src_row = &slice[src_offset..(src_offset + w_bytes)];
                        let dest_row = dest_base.add(row * dest_stride);
                        std::ptr::copy_nonoverlapping(src_row.as_ptr(), dest_row, w_bytes);
                    }
                }
            }
            true
        });

        let mut copied = matches!(copy_result, Ok(true));
        if !copied {
            // If not wl_shm, try linux-dmabuf buffer (used by xdg-desktop-portal-wlr with PipeWire)
            if let Ok(dmabuf) = smithay::wayland::dmabuf::get_dmabuf(&req.buffer) {
                use smithay::backend::renderer::ImportDma;
                if let Ok(mut dmabuf_tex) = renderer.import_dmabuf(dmabuf, None) {
                    if let Ok(mut dmabuf_fb) = renderer.bind(&mut dmabuf_tex) {
                        let mut damage_tracker = OutputDamageTracker::new(size, 1.0, smithay::utils::Transform::Normal);
                        if damage_tracker.render_output(renderer, &mut dmabuf_fb, 0, &elements, clear).is_ok() {
                            copied = true;
                        }
                    }
                }

                if !copied {
                    use smithay::backend::allocator::Buffer;
                    use smithay::backend::allocator::dmabuf::{DmabufMappingMode, DmabufSyncFlags};

                    if let Ok(mapping) = dmabuf.map_plane(0, DmabufMappingMode::WRITE) {
                        let _ = dmabuf.sync_plane(0, DmabufSyncFlags::WRITE | DmabufSyncFlags::START);
                        let ptr = mapping.ptr() as *mut u8;
                        let len = mapping.length();
                        let dest_stride = dmabuf.strides().next().unwrap_or((size.0 * 4) as u32) as usize;
                        let offset = dmabuf.offsets().next().unwrap_or(0) as usize;
                        let h = (dmabuf.size().h.max(0) as usize).min(size.1 as usize);
                        let w_bytes = (dmabuf.size().w.max(0) as usize).min(size.0 as usize) * 4;

                        if dest_stride >= w_bytes {
                            let total_required = dest_stride
                                .checked_mul(h.saturating_sub(1))
                                .and_then(|v| v.checked_add(w_bytes))
                                .and_then(|v| v.checked_add(offset));
                            if let Some(total) = total_required
                                && total <= len
                            {
                                unsafe {
                                    let dest_base = ptr.add(offset);
                                    for row in 0..h {
                                        let src_row_idx = (ry + row).min(size.1.max(0) as usize);
                                        let src_offset = src_row_idx * src_stride + rx * 4;
                                        if src_offset + w_bytes <= slice.len() {
                                            let src_row = &slice[src_offset..(src_offset + w_bytes)];
                                            let dest_row = dest_base.add(row * dest_stride);
                                            std::ptr::copy_nonoverlapping(src_row.as_ptr(), dest_row, w_bytes);
                                        }
                                    }
                                }
                                let _ = dmabuf.sync_plane(0, DmabufSyncFlags::WRITE | DmabufSyncFlags::END);
                                copied = true;
                            }
                        }
                    }
                }
            }
        }

        if copied {
            if req.with_damage && req.frame.version() >= 2 {
                req.frame.damage(0, 0, size.0 as u32, size.1 as u32);
            }
            req.frame.flags(Flags::empty());
            req.frame.ready(tv_sec_hi, tv_sec_lo, tv_nsec);
        } else {
            req.frame.failed();
        }
    }
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
            let mut dev = Event::device(&event);
            if let Ok(token) = data
                .state
                .core()
                .verify_input_port(&data.input_port, key, pressed)
            {
                let (modifiers, mods_changed) = data.state.sync_seat_key(token);
                update_keyboard_leds(&data.state, &mut data.keyboards, &mut dev);
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
            let (target_phys_x, target_phys_y) = match screen {
                Some(s) => (event.x_transformed(s.size.0), event.y_transformed(s.size.1)),
                None => (event.x(), event.y()),
            };
            let delta = calculate_abs_pointer_delta(
                (target_phys_x, target_phys_y),
                current_p,
                data.scale_120,
            );
            let scale = (f64::from(data.scale_120) / 120.0).max(0.001);
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
            configure_input_device(&mut device);
            let is_kb = device.has_capability(smithay::reexports::input::DeviceCapability::Keyboard);
            let is_ptr = device.has_capability(smithay::reexports::input::DeviceCapability::Pointer);
            println!(
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
                data.keyboards.push(device);
            }
        }
        InputEvent::DeviceRemoved { device } => {
            data.keyboards.retain(|k| k.sysname() != device.sysname());
            data.last_abs_pointer = None;
        }
        _ => {}
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
/// `(abs_x, abs_y)` transformed to screen pixels and normalized by `scale_120`.
pub fn calculate_abs_pointer_delta(
    abs_px: (f64, f64),
    current_p: (f64, f64),
    scale_120: u32,
) -> (f64, f64) {
    let scale = (f64::from(scale_120) / 120.0).max(0.001);
    let target_logical = (abs_px.0 / scale, abs_px.1 / scale);
    (target_logical.0 - current_p.0, target_logical.1 - current_p.1)
}

fn configure_input_device(device: &mut smithay::reexports::input::Device) {
    use smithay::reexports::input::AccelProfile;

    // 1. Mouse Acceleration Profile (flat = raw 1:1, adaptive = curve)
    if let Ok(profile) = std::env::var("MOUSE_ACCEL_PROFILE") {
        let desired = match profile.to_lowercase().as_str() {
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
    if let Ok(speed_str) = std::env::var("MOUSE_ACCEL_SPEED")
        && let Ok(speed) = speed_str.parse::<f64>()
    {
        let clamped = speed.clamp(-1.0, 1.0);
        let _ = device.config_accel_set_speed(clamped);
    }

    // 3. Touchpad Tap-to-click
    if let Ok(tap) = std::env::var("TOUCHPAD_TAP") {
        let enable = matches!(tap.to_lowercase().as_str(), "1" | "true" | "yes" | "on");
        if device.config_tap_finger_count() > 0 {
            let _ = device.config_tap_set_enabled(enable);
        }
    }

    // 4. Touchpad Natural Scrolling
    if let Ok(nat) = std::env::var("TOUCHPAD_NATURAL_SCROLL") {
        let enable = matches!(nat.to_lowercase().as_str(), "1" | "true" | "yes" | "on");
        if device.config_scroll_has_natural_scroll() {
            let _ = device.config_scroll_set_natural_scroll_enabled(enable);
        }
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
}
