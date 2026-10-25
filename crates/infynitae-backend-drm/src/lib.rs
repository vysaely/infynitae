//! Infynitae Native console backend: the compositor owns the display and the input devices of a
//! seat, started from a text console (FOUNDATION_ARCHITECTURE C1.1; owner decisions T1–T9 of
//! 2026-10-22, `~/infynitae/TTY_BACKEND_DECISIONS_PROPOSAL_2026-10-21.md`).
//!
//! - **Session (T2):** libseat opens the GPU and the input devices without root; a VT switch pauses
//!   the session (input suspended, DRM master dropped) and resumes it.
//! - **Rendering (T3):** GLES on GBM/EGL through Smithay's `DrmCompositor`, page-flipped on vblank.
//!   Client dmabufs and shm buffers are imported on the GPU render node.
//! - **Screens (T5):** every connector connected at start, at its preferred mode, on the primary
//!   GPU. Hotplug is not handled yet.
//! - **Input (T4):** libinput on the seat; the keyboard layout from `XKB_DEFAULT_*` (xkbcommon's
//!   defaults otherwise).
//! - **Safety (T6):** Ctrl+Alt+Backspace quits; Ctrl+Alt+F1…F12 switch VT. Both are taken before
//!   Core sees the key, so no binding or client can swallow them.
//! - **Cursor (T8):** drawn by the compositor (`infynitae_wayland::cursor`).

#![deny(unsafe_code)]

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use infynitae_core::output::{OutputDescriptor, OutputId, OutputKey, Transform};
use infynitae_core::{Core, InputPort, OutputLifecycleToken, PhysicalEvent};
use infynitae_wayland::output_management::{HeadChange, HeadInfo, HeadMode};
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
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::backend::renderer::{Bind, ExportMem, Offscreen, Texture};
use smithay::backend::renderer::{Renderer, TextureFilter};
use smithay::backend::session::libseat::LibSeatSession;
use smithay::backend::session::{Event as SeatEvent, Session as _};
use smithay::output::OutputModeSource;
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::drm::control::{Device as _, ModeTypeFlags, connector, crtc};
use smithay::reexports::input::Libinput;
use smithay::reexports::rustix::fs::OFlags;
use smithay::reexports::wayland_server::Display;
use smithay::utils::{DeviceFd, Rectangle};
use smithay::wayland::shm::with_buffer_contents_mut;

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
    /// Path to xwayland-satellite to supervise; `None`: no X11.
    pub xwayland_satellite: Option<PathBuf>,
    /// Output scale ×120 for every screen (`None`: 120, i.e. 1.0).
    pub scale_120: Option<u32>,
    /// Scaling texture filter ("nearest" for 0% blur, "linear" for smooth).
    pub scaling_filter: Option<String>,
    /// Direct scanout for fullscreen apps and games.
    pub direct_scanout: Option<bool>,
    /// Variable Refresh Rate (VRR / Adaptive Sync): always on, off, or `None` (auto) only while
    /// the screen shows a fullscreen window (games, fullscreen video). Always-on VRR on the
    /// desktop makes video and cursor pacing uneven and flickers on some monitors.
    pub vrr: Option<bool>,
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

/// Validates an output scale factor, returning its 1/120 representation if within 0.25..=8.0. Pure.
pub fn validate_scale(scale: f64) -> Option<u32> {
    if (0.25..=8.0).contains(&scale) {
        Some((scale * 120.0).round().max(1.0) as u32)
    } else {
        None
    }
}

/// Returns whether a configured name matches a connector's kernel name or legacy debug name.
/// The comparison is exact and case-insensitive. Pure.
pub fn matches_connector_name(configured: &str, kernel_name: &str, debug_name: &str) -> bool {
    let conf = configured.trim();
    conf.eq_ignore_ascii_case(kernel_name) || conf.eq_ignore_ascii_case(debug_name)
}

/// Calculates the origin offset required to render a requested screencopy region into a buffer.
pub fn screencopy_region_offset(region: Option<(i32, i32, i32, i32)>) -> (i32, i32) {
    region.map(|(x, y, _, _)| (x, y)).unwrap_or((0, 0))
}

/// Time remaining until the inactivity timeout expires. None when idle blanking is disabled,
/// already active, or inhibited. Pure.
pub fn idle_timeout_remaining(
    idle_sleep_timeout_min: u32,
    dpms_active: bool,
    idle_inhibited: bool,
    last_user_activity: std::time::Instant,
    now: std::time::Instant,
) -> Option<Duration> {
    if !dpms_active && idle_sleep_timeout_min > 0 && !idle_inhibited {
        let total = Duration::from_secs(u64::from(idle_sleep_timeout_min) * 60);
        let elapsed = now.saturating_duration_since(last_user_activity);
        Some(total.saturating_sub(elapsed))
    } else {
        None
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

/// The mode to use: preferred mode's resolution at highest refresh (or custom mode/rate if configured).
pub fn choose_mode(modes: &[ModeInfo]) -> Option<usize> {
    choose_mode_custom(modes, None, None)
}

/// The mode to use with optional custom target resolution and refresh rate.
pub fn choose_mode_custom(
    modes: &[ModeInfo],
    target_size: Option<(u16, u16)>,
    target_rate: Option<u32>,
) -> Option<usize> {
    let size = target_size.or_else(|| {
        modes
            .iter()
            .find(|m| m.preferred)
            .or_else(|| modes.first())
            .map(|m| m.size)
    })?;
    let matching_size: Vec<(usize, &ModeInfo)> = modes
        .iter()
        .enumerate()
        .filter(|(_, m)| m.size == size)
        .collect();

    if let Some(rate) = target_rate
        && let Some(&(idx, _)) = matching_size
            .iter()
            .min_by_key(|(_, m)| (m.refresh_mhz as i64 - rate as i64).abs())
    {
        return Some(idx);
    }

    matching_size
        .into_iter()
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
    /// The queued frame was drawn while locked (no window in it).
    queued_locked: bool,
    /// What the screen shows now has no window: a locked frame reached its vblank, or the
    /// screen is blank.
    shows_locked: bool,
    screencopy_texture: Option<GlesTexture>,
    /// Page flips refused in a row (logged at the first and every 100th).
    flip_failures: u32,
    /// Direct scanout is off for this screen until then: a frame scanning out a client's
    /// buffer was refused.
    no_scanout_until: Option<std::time::Instant>,
    /// The monitor and driver support VRR on this connector.
    vrr_supported: bool,
    /// VRR is on now.
    vrr_on: bool,
    /// Screen power mode turned off by output power management.
    power_off: bool,
    /// The connector's modes, as display settings tools see them and as DRM sets them.
    modes: Vec<(HeadMode, smithay::reexports::drm::control::Mode)>,
}

/// DRM per-screen gamma ramp state.
struct DrmScreenGamma {
    crtc: crtc::Handle,
    gamma_size: u32,
    original_gamma: (Vec<u16>, Vec<u16>, Vec<u16>),
    current_gamma: Option<(Vec<u16>, Vec<u16>, Vec<u16>)>,
}

/// DRM backend implementation of `infynitae_wayland::gamma::GammaHandler`.
struct DrmGammaHandler {
    drm_fd: DrmDeviceFd,
    screens: Arc<Mutex<HashMap<OutputId, DrmScreenGamma>>>,
}

impl infynitae_wayland::gamma::GammaHandler for DrmGammaHandler {
    fn gamma_size(&self, output: OutputId) -> Option<u32> {
        let map = self.screens.lock().unwrap();
        map.get(&output).map(|s| s.gamma_size).filter(|&s| s > 0)
    }

    fn set_gamma(
        &mut self,
        output: OutputId,
        red: &[u16],
        green: &[u16],
        blue: &[u16],
    ) -> Result<(), infynitae_wayland::gamma::GammaSetError> {
        let mut map = self.screens.lock().unwrap();
        let Some(screen) = map.get_mut(&output) else {
            return Err(infynitae_wayland::gamma::GammaSetError);
        };
        self.drm_fd
            .set_gamma(screen.crtc, red, green, blue)
            .map_err(|_| infynitae_wayland::gamma::GammaSetError)?;
        screen.current_gamma = Some((red.to_vec(), green.to_vec(), blue.to_vec()));
        Ok(())
    }

    fn reset_gamma(&mut self, output: OutputId) {
        let mut map = self.screens.lock().unwrap();
        if let Some(screen) = map.get_mut(&output) {
            screen.current_gamma = None;
            let (r, g, b) = &screen.original_gamma;
            let _ = self.drm_fd.set_gamma(screen.crtc, r, g, b);
        }
    }
}

/// DRM backend implementation of `infynitae_wayland::output_power::OutputPowerHandler`.
struct DrmOutputPowerHandler {
    pending: Arc<Mutex<Vec<(OutputId, infynitae_wayland::output_power::OutputPowerMode)>>>,
}

impl infynitae_wayland::output_power::OutputPowerHandler for DrmOutputPowerHandler {
    fn set_mode(
        &mut self,
        output: OutputId,
        mode: infynitae_wayland::output_power::OutputPowerMode,
    ) {
        self.pending.lock().unwrap().push((output, mode));
    }
}

/// Carries out a display configuration (`wlr-output-management`): the new modes on the
/// hardware first, then Core's descriptors and positions. Everything or nothing: a mode the
/// hardware refuses, or a configuration Core refuses, sets every changed screen back.
fn apply_output_configuration(data: &mut LoopData, changes: &[HeadChange]) -> bool {
    let mut changed: Vec<(usize, smithay::reexports::drm::control::Mode)> = Vec::new();
    let revert = |data: &mut LoopData,
                  changed: &[(usize, smithay::reexports::drm::control::Mode)]| {
        for &(i, old) in changed {
            let s = &mut data.screens[i];
            let _ = s.compositor.use_mode(old);
            s.size = (i32::from(old.size().0), i32::from(old.size().1));
            s.compositor
                .set_output_mode_source(OutputModeSource::Static {
                    size: s.size.into(),
                    scale: smithay::utils::Scale::from(s.k),
                    transform: smithay::utils::Transform::Normal,
                });
        }
    };
    for c in changes {
        let Some(m) = c.mode else { continue };
        let Some(i) = data.screens.iter().position(|s| s.id == c.output) else {
            revert(data, &changed);
            return false;
        };
        let s = &mut data.screens[i];
        let Some(&(_, mode)) = s.modes.iter().find(|(h, _)| *h == m) else {
            revert(data, &changed);
            return false;
        };
        let old = s.compositor.pending_mode();
        if s.compositor.use_mode(mode).is_err() {
            revert(data, &changed);
            return false;
        }
        changed.push((i, old));
        s.size = (i32::from(mode.size().0), i32::from(mode.size().1));
        s.compositor
            .set_output_mode_source(OutputModeSource::Static {
                size: s.size.into(),
                scale: smithay::utils::Scale::from(s.k),
                transform: smithay::utils::Transform::Normal,
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
    data.session.request_frame();
    true
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
    gamma_screens: Arc<Mutex<HashMap<OutputId, DrmScreenGamma>>>,
    pending_power: Arc<Mutex<Vec<(OutputId, infynitae_wayland::output_power::OutputPowerMode)>>>,
    libinput: Libinput,
    scale_120: u32,
    /// The session is active (not switched away).
    active: bool,
    quit: bool,
    /// Keys taken by the console (T6): their releases are not passed on either.
    taken: Vec<u32>,
    /// Pointer buttons physically down (every button event, locked or not).
    buttons_down: Vec<u32>,
    /// Active keyboard devices for hardware LED synchronization (Caps Lock, Num Lock).
    keyboards: Vec<smithay::reexports::input::Device>,
    /// All active input devices (keyboards, mice, touchpads) for live config updates.
    input_devices: Vec<smithay::reexports::input::Device>,
    /// Last absolute pointer logical position, used to compute delta for absolute devices (e.g. RustDesk, tablets).
    last_abs_pointer: Option<(f64, f64)>,
    /// Set of known mapped element IDs for detecting newly mapped windows.
    known_mapped: HashSet<infynitae_core::stage::ElementId>,
    /// Direct scanout for primary and overlay planes.
    direct_scanout: bool,
    config_path: Option<PathBuf>,
    last_config_check: std::time::Instant,
    last_config_mtime: Option<std::time::SystemTime>,
    last_user_activity: std::time::Instant,
    dpms_active: bool,
    idle_sleep_timeout_min: u32,
    idle_auto_lock: bool,
    lock_process: Option<std::process::Child>,
    /// The VRR setting (`ConsoleOptions::vrr`).
    vrr_mode: Option<bool>,
    /// For timers set from the render path (a refused page flip's retry).
    loop_handle: smithay::reexports::calloop::LoopHandle<'static, LoopData>,
}

/// The device limit Core is told: assumed, not queried, as in the nested backend (OB-52).
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
    // ext-session-lock: the console draws locked frames and routes keys through lock_key
    state.enable_session_lock();

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
        .or_else(|| {
            smithay::backend::udev::primary_gpu(&seat_name)
                .ok()
                .flatten()
        })
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
    // clients allocate on the GPU this compositor draws with (its render node), not a guess
    if let Ok(node) = smithay::backend::drm::DrmNode::from_path(&gpu) {
        let node = node
            .node_with_type(smithay::backend::drm::NodeType::Render)
            .and_then(Result::ok)
            .unwrap_or(node);
        // every format and modifier the renderer imports, not only linear
        let formats: Vec<_> = smithay::backend::renderer::ImportDma::dmabuf_formats(&renderer)
            .iter()
            .copied()
            .collect();
        println!(
            "[infynitae] Client buffers on GPU node {node} ({} formats/modifiers)",
            formats.len()
        );
        state.set_dmabuf_device_formats(node.dev_id(), formats);
    }

    // Scaling filter: nearest-neighbor filtering on upscaled surfaces by default
    let use_nearest = !matches!(options.scaling_filter.as_deref(), Some("linear"));
    if use_nearest && let Err(e) = renderer.upscale_filter(TextureFilter::Nearest) {
        eprintln!("[infynitae] Warning: failed to set upscale filter to Nearest: {e:?}");
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

    // Read display arrangement and per-monitor configs from ~/.config/infynitae/config
    let mut output_order: Vec<String> = Vec::new();
    let mut output_layout_reverse = false;
    let mut custom_scales: HashMap<String, u32> = HashMap::new();
    let mut custom_modes: HashMap<String, (u16, u16)> = HashMap::new();
    let mut custom_rates: HashMap<String, u32> = HashMap::new();
    let mut idle_timeout_min = 15u32;
    let mut idle_auto_lock = true;

    if let Ok(home) = std::env::var("HOME") {
        let config_path = format!("{home}/.config/infynitae/config");
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            for line in content.lines() {
                let line_clean = line.split('#').next().unwrap_or("").trim();
                if let Some((k, v)) = line_clean.split_once('=') {
                    let key = k.trim();
                    let val = v.trim().trim_matches(|c| c == '"' || c == '\'').trim();
                    if key == "OUTPUT_ORDER" {
                        output_order = val
                            .split(',')
                            .map(|s| s.trim().to_lowercase())
                            .filter(|s| !s.is_empty())
                            .collect();
                    } else if key == "OUTPUT_LAYOUT" && val.eq_ignore_ascii_case("right-to-left") {
                        output_layout_reverse = true;
                    } else if key == "IDLE_SLEEP_TIMEOUT_MIN" {
                        if let Ok(m) = val.parse::<u32>() {
                            idle_timeout_min = m;
                        }
                    } else if key == "IDLE_AUTO_LOCK" {
                        idle_auto_lock =
                            !matches!(val.to_lowercase().as_str(), "0" | "false" | "off" | "no");
                    } else if key.starts_with("OUTPUT_") && key.ends_with("_SCALE") {
                        let conn_name = key
                            .trim_start_matches("OUTPUT_")
                            .trim_end_matches("_SCALE")
                            .replace('_', "-");
                        if let Ok(s) = val.parse::<f64>() {
                            if let Some(s_120) = validate_scale(s) {
                                custom_scales.insert(conn_name.to_lowercase(), s_120);
                            } else {
                                eprintln!(
                                    "[infynitae] OUTPUT_{}_SCALE={s} outside accepted range (0.25..8.0), ignoring",
                                    conn_name
                                );
                            }
                        }
                    } else if key.starts_with("OUTPUT_") && key.ends_with("_RATE") {
                        let conn_name = key
                            .trim_start_matches("OUTPUT_")
                            .trim_end_matches("_RATE")
                            .replace('_', "-");
                        if let Ok(r) = val.parse::<f64>() {
                            custom_rates
                                .insert(conn_name.to_lowercase(), (r * 1000.0).round() as u32);
                        }
                    } else if key.starts_with("OUTPUT_") && key.ends_with("_MODE") {
                        let conn_name = key
                            .trim_start_matches("OUTPUT_")
                            .trim_end_matches("_MODE")
                            .replace('_', "-");
                        if let Some((w_s, h_s)) = val.split_once('x')
                            && let (Ok(w), Ok(h)) =
                                (w_s.trim().parse::<u16>(), h_s.trim().parse::<u16>())
                        {
                            custom_modes.insert(conn_name.to_lowercase(), (w, h));
                        }
                    }
                }
            }
        }
    }

    // every connected connector at its preferred mode, each on a CRTC of its own (T5)
    let resources = drm.resource_handles()?;
    let mut used: Vec<crtc::Handle> = Vec::new();
    let mut screens = Vec::new();
    let gamma_screens: Arc<Mutex<HashMap<OutputId, DrmScreenGamma>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let pending_power: Arc<
        Mutex<Vec<(OutputId, infynitae_wayland::output_power::OutputPowerMode)>>,
    > = Arc::new(Mutex::new(Vec::new()));

    let mut connected_connectors = Vec::new();
    for &handle in resources.connectors() {
        if let Ok(conn) = drm.get_connector(handle, true)
            && conn.state() == connector::State::Connected
        {
            let name = format!("{}-{}", conn.interface().as_str(), conn.interface_id());
            let debug_name = format!("{:?}-{}", conn.interface(), conn.interface_id());
            connected_connectors.push((handle, conn, name, debug_name));
        }
    }

    if !output_order.is_empty() {
        connected_connectors.sort_by_key(|(_, _, name, debug_name)| {
            output_order
                .iter()
                .position(|o| matches_connector_name(o, name, debug_name))
                .unwrap_or(usize::MAX)
        });
    } else if output_layout_reverse {
        connected_connectors.reverse();
    }

    for (handle, conn, name, debug_name) in connected_connectors {
        let n_lower = name.to_lowercase();
        let dbg_lower = debug_name.to_lowercase();
        let target_size = custom_modes
            .get(&n_lower)
            .or_else(|| custom_modes.get(&dbg_lower))
            .copied();
        let target_rate = custom_rates
            .get(&n_lower)
            .or_else(|| custom_rates.get(&dbg_lower))
            .copied();
        let conn_scale = custom_scales
            .get(&n_lower)
            .or_else(|| custom_scales.get(&dbg_lower))
            .copied()
            .unwrap_or(scale_120);

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
                eprintln!(
                    "[infynitae] Screen {name}: requested custom mode {:?} @ {:?} not found, falling back to preferred mode",
                    target_size, target_rate
                );
                choose_mode(&infos)
            } else {
                None
            }
        });
        let Some(mode) = mode_idx.map(|i| conn.modes()[i]) else {
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
        let (w, h) = mode.size();
        let refresh = refresh_mhz(
            mode.clock(),
            mode.hsync().2,
            mode.vsync().2,
            mode.vrefresh(),
        );
        println!(
            "[infynitae] Screen {name}: {w}x{h} @ {}.{:03} Hz (scale: {:.2}x)",
            refresh / 1000,
            refresh % 1000,
            conn_scale as f64 / 120.0
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
            scale_120: conn_scale,
            refresh_mhz: refresh,
        };
        let (id, fx) = state
            .core_mut()
            .output_connected(&mut output_token, descriptor)
            .map_err(|e| format!("output {name}: {e:?}"))?;
        state.apply(&fx);
        let k = state.core().view(id).map_or(1.0, |v| v.k);
        let size = (i32::from(w), i32::from(h));
        let mut compositor = Compositor::new(
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

        let vrr_supported = compositor
            .vrr_supported(handle)
            .is_ok_and(|s| !matches!(s, smithay::backend::drm::VrrSupport::NotSupported));
        let mut vrr_on = false;
        match (vrr_supported, options.vrr) {
            (false, _) => {}
            (true, Some(true)) => {
                vrr_on = compositor.use_vrr(true).is_ok();
                println!("[infynitae] Screen {name}: VRR always on");
            }
            (true, Some(false)) => println!("[infynitae] Screen {name}: VRR off"),
            (true, None) => println!("[infynitae] Screen {name}: VRR while a window is fullscreen"),
        }
        screens.push(Screen {
            id,
            crtc,
            size,
            compositor,
            k,
            pending: false,
            queued_locked: false,
            shows_locked: false,
            screencopy_texture: None,
            flip_failures: 0,
            no_scanout_until: None,
            vrr_supported,
            vrr_on,
            power_off: false,
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
        });
        // VRR is not reported: it follows fullscreen windows (or the config), not the tools
        let (mm_w, mm_h) = conn.size().unwrap_or((0, 0));
        let screen = screens.last().expect("just pushed");
        state.set_output_head_info(
            id,
            HeadInfo {
                modes: screen.modes.iter().map(|(h, _)| *h).collect(),
                physical_size: Some((mm_w as i32, mm_h as i32)),
                adaptive_sync: None,
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
    }
    if screens.is_empty() {
        return Err("no connected screen".into());
    }
    state.set_gamma_handler(Arc::new(Mutex::new(DrmGammaHandler {
        drm_fd: drm.device_fd().clone(),
        screens: gamma_screens.clone(),
    })));
    state.set_output_power_handler(Arc::new(Mutex::new(DrmOutputPowerHandler {
        pending: pending_power.clone(),
    })));
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
                    s.shows_locked = false;
                }
                // Re-apply client's gamma ramp after VT switch
                {
                    let gamma_map = data.gamma_screens.lock().unwrap();
                    for screen in gamma_map.values() {
                        if let Some((r, g, b)) = &screen.current_gamma {
                            let _ = data.drm.device_fd().set_gamma(screen.crtc, r, g, b);
                        }
                    }
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
                    if !data.dpms_active {
                        s.shows_locked = s.queued_locked;
                    }
                    let id = s.id;
                    let now = infynitae_wayland::presentation_clock_now();
                    data.state.frame_tick(id, now);
                    confirm_lock(data);
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
        seat,
        input_port,
        output_token,
        drm,
        renderer,
        screens,
        gamma_screens: gamma_screens.clone(),
        pending_power: pending_power.clone(),
        libinput,
        scale_120,
        active: true,
        quit: false,
        taken: Vec::new(),
        buttons_down: Vec::new(),
        keyboards: Vec::new(),
        input_devices: Vec::new(),
        last_abs_pointer: None,
        known_mapped: HashSet::new(),
        direct_scanout: options.direct_scanout.unwrap_or(true),
        config_path: cfg_path,
        last_config_check: std::time::Instant::now(),
        last_config_mtime: initial_mtime,
        last_user_activity: std::time::Instant::now(),
        dpms_active: false,
        idle_sleep_timeout_min: idle_timeout_min,
        idle_auto_lock,
        lock_process: None,
        vrr_mode: options.vrr,
        loop_handle: handle.clone(),
    };

    data.state.reload_config();
    if let Some(ds) = data.state.direct_scanout {
        data.direct_scanout = ds;
    }

    println!("[infynitae] Compositor running. Ctrl+Alt+Backspace quits.");
    while !data.quit && !data.session.finished() {
        data.session
            .wake_at(data.state.core().next_settlement_deadline());
        let restart = xwayland
            .as_mut()
            .and_then(|x| x.poll(std::time::Instant::now()));
        let has_screencopy = data
            .screens
            .iter()
            .any(|s| data.state.screencopy.has_pending(s.id));
        if has_screencopy {
            data.session.request_frame();
        }
        let idle_timeout = idle_timeout_remaining(
            data.idle_sleep_timeout_min,
            data.dpms_active,
            data.state.is_idle_inhibited(),
            data.last_user_activity,
            std::time::Instant::now(),
        );
        let timeout = [data.session.timeout(), restart, idle_timeout]
            .into_iter()
            .flatten()
            .min();
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
                    if let Some(ds) = data.state.direct_scanout {
                        data.direct_scanout = ds;
                    }
                    if let Ok(content) = std::fs::read_to_string(path) {
                        for line in content.lines() {
                            let line_clean = line.split('#').next().unwrap_or("").trim();
                            if let Some((k, v)) = line_clean.split_once('=') {
                                let key = k.trim();
                                let val = v.trim().trim_matches(|c| c == '"' || c == '\'').trim();
                                if key == "IDLE_SLEEP_TIMEOUT_MIN" {
                                    if let Ok(m) = val.parse::<u32>() {
                                        data.idle_sleep_timeout_min = m;
                                    }
                                } else if key == "IDLE_AUTO_LOCK" {
                                    data.idle_auto_lock = !matches!(
                                        val.to_lowercase().as_str(),
                                        "0" | "false" | "off" | "no"
                                    );
                                }
                            }
                        }
                    }
                    let input_cfg = data.state.input_config().clone();
                    for dev in &mut data.input_devices {
                        configure_input_device(dev, &input_cfg);
                    }
                    data.session.request_frame();
                } else if data.last_config_mtime.is_none() {
                    data.last_config_mtime = Some(mtime);
                }
            }

            // Idle sleep and auto-lock check (skipped when idle is inhibited)
            if !data.dpms_active
                && data.idle_sleep_timeout_min > 0
                && !data.state.is_idle_inhibited()
                && data.last_user_activity.elapsed()
                    >= Duration::from_secs((data.idle_sleep_timeout_min as u64) * 60)
            {
                println!(
                    "[infynitae] Inactivity timeout reached ({} min). Entering DPMS sleep / screen blanking...",
                    data.idle_sleep_timeout_min
                );
                data.dpms_active = true;
                for s in &mut data.screens {
                    let _ = s.compositor.clear();
                    s.pending = false;
                    s.shows_locked = true;
                }
                data.state.set_all_outputs_power_mode(
                    infynitae_wayland::output_power::OutputPowerMode::Off,
                );
                if data.idle_auto_lock {
                    let is_running = data
                        .lock_process
                        .as_mut()
                        .is_some_and(|p| p.try_wait().ok().flatten().is_none());
                    if !is_running {
                        println!("[infynitae] Auto-locking session with infynitae-lock...");
                        let child = std::process::Command::new("infynitae-lock")
                            .spawn()
                            .or_else(|_| {
                                if let Ok(home) = std::env::var("HOME") {
                                    std::process::Command::new(format!(
                                        "{home}/.local/bin/infynitae-lock"
                                    ))
                                    .spawn()
                                } else {
                                    Err(std::io::Error::new(
                                        std::io::ErrorKind::NotFound,
                                        "not found",
                                    ))
                                }
                            });
                        if let Ok(c) = child {
                            data.lock_process = Some(c);
                        }
                    }
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
        for c in data.state.take_output_configurations() {
            let ok = apply_output_configuration(&mut data, &c.changes);
            data.state.finish_output_configuration(c, ok);
        }
        // Process client requests to change screen power save modes
        let pending_power: Vec<_> = data.pending_power.lock().unwrap().drain(..).collect();
        for (output_id, mode) in pending_power {
            if let Some(s) = data.screens.iter_mut().find(|s| s.id == output_id) {
                match mode {
                    infynitae_wayland::output_power::OutputPowerMode::Off if !s.power_off => {
                        s.power_off = true;
                        let _ = s.compositor.clear();
                        s.pending = false;
                        s.shows_locked = true;
                    }
                    infynitae_wayland::output_power::OutputPowerMode::On if s.power_off => {
                        s.power_off = false;
                        let _ = s.compositor.reset_state();
                        s.pending = false;
                        s.shows_locked = false;
                        let gamma_map = data.gamma_screens.lock().unwrap();
                        if let Some(screen) = gamma_map.get(&output_id)
                            && let Some((r, g, b)) = &screen.current_gamma
                        {
                            let _ = data.drm.device_fd().set_gamma(screen.crtc, r, g, b);
                        }
                        data.session.request_frame();
                    }
                    _ => {}
                }
            }
        }
        if let Some(keys) = data.state.take_unlock_releases() {
            let now_us = infynitae_wayland::presentation_clock_now().as_micros() as u64;
            for key in keys {
                core_key(&mut data, key, false, now_us);
            }
            // buttons Core holds that were released while locked: Core sees their release
            let released: Vec<u32> = data
                .state
                .core()
                .buttons()
                .pressed()
                .iter()
                .map(|(b, _)| *b)
                .filter(|b| !data.buttons_down.contains(b))
                .collect();
            for button in released {
                let phys = PhysicalEvent::PointerButton {
                    button,
                    pressed: false,
                    modifiers: data.state.seat_modifiers(),
                    time_us: now_us,
                };
                let port = &mut data.input_port;
                let _ = data
                    .state
                    .with_effects(|core, fx| core.physical_input(port, phys, fx));
            }
            data.state.finish_unlock();
        }

        let current_mapped = data.state.core().stage().mapped_elements();
        let new_mapped: Vec<_> = current_mapped
            .iter()
            .copied()
            .filter(|id| !data.known_mapped.contains(id))
            .collect();

        data.known_mapped.clear();
        data.known_mapped.extend(current_mapped);
        // which new windows the user opened, decided before any of them takes the focus
        let (focus_user_only, teleport_user_only) = data.state.open_policy_user_only();
        let by_user: Vec<bool> = new_mapped
            .iter()
            .map(|&id| {
                (focus_user_only || teleport_user_only)
                    && data.state.opened_by_user(id, data.last_user_activity)
            })
            .collect();

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
            for (&new_id, &user) in new_mapped.iter().zip(&by_user) {
                if focus_user_only && !user {
                    continue;
                }
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
            for (&new_id, &user) in new_mapped.iter().zip(&by_user) {
                // a child window opens over its parent, already in view: the camera stays
                if (teleport_user_only && !user)
                    || data.state.core().toplevel_parent(new_id).is_some()
                {
                    continue;
                }
                let port = &mut data.input_port;
                let _ = data
                    .state
                    .with_effects(|core, fx| core.recenter_element(port, new_id, fx));
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

        let has_screencopy = data
            .screens
            .iter()
            .any(|s| data.state.screencopy.has_pending(s.id));
        if (data.session.frame_due() || has_screencopy) && data.active {
            render(&mut data);
        }

        let now = infynitae_wayland::presentation_clock_now();
        let deadline = data.state.frame_fallback(now);
        data.session.set_fallback_deadline(deadline, now);
        let idle_deadline = data.state.idle_notify_check(now);
        data.session.set_idle_deadline(idle_deadline, now);
        data.session.flush();
    }

    println!("[infynitae] Shutting down compositor...");
    drop(xwayland);
    data.session.shutdown_with(&mut data.state);
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
    let locked = data.state.is_locked();
    let clear = if locked {
        convert::LOCKED_CLEAR
    } else {
        data.state.clear_color()
    };
    for i in 0..data.screens.len() {
        let id = data.screens[i].id;
        let size = data.screens[i].size;

        // Screencopy frame capture if requested (Discord, OBS, WebRTC, RustDesk).
        // Screencopy renders to an offscreen buffer and does not depend on DRM page flip / vblank.
        // Handling this first ensures pending screencopy requests are consumed immediately,
        // preventing the event loop from busy-spinning while DRM page flip is pending.
        serve_captures(
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

        if data.dpms_active || data.screens[i].power_off {
            // While DPMS or power-off is active, do not render or queue frames to the screen.
            // Blanked screens count as showing no window.
            let s = &mut data.screens[i];
            s.shows_locked = true;
            let now = infynitae_wayland::presentation_clock_now();
            data.state.frame_tick(id, now);
            continue;
        }

        // locked: only the lock surface (lock.rs); never a window
        let frame = match convert::lock_elements(&data.state, &mut data.renderer, id) {
            Ok(Some(f)) => Ok(f),
            Ok(None) => convert::output_elements(&mut data.state, &mut data.renderer, id)
                .map_err(|e| format!("{e:?}")),
            Err(e) => Err(format!("{e:?}")),
        };
        let (elements, k) = match frame {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[infynitae] Frame elements: {e}");
                continue;
            }
        };
        // VRR auto: on while this screen shows a fullscreen window
        let want_vrr = data
            .vrr_mode
            .unwrap_or_else(|| data.state.core().presentation(id).is_some());
        let s = &mut data.screens[i];
        if s.vrr_supported && s.vrr_on != want_vrr && s.compositor.use_vrr(want_vrr).is_ok() {
            s.vrr_on = want_vrr;
        }
        if s.k != k {
            s.compositor
                .set_output_mode_source(OutputModeSource::Static {
                    size: s.size.into(),
                    scale: smithay::utils::Scale::from(k),
                    transform: smithay::utils::Transform::Normal,
                });
            s.k = k;
        }
        // Direct scanout: a fullscreen client's buffer on the primary plane (games). Overlay
        // planes only on request: amdgpu refused page flips (EINVAL, hundreds per Electron
        // start) for viewport-scaled subsurfaces there, although Smithay's test commit passed.
        // The cursor is drawn by the compositor, never on the cursor plane.
        let scanout_banned = s
            .no_scanout_until
            .is_some_and(|t| std::time::Instant::now() < t);
        let frame_flags = match (
            data.direct_scanout && !scanout_banned,
            data.state.overlay_planes,
        ) {
            (false, _) => FrameFlags::empty(),
            (true, false) => FrameFlags::ALLOW_PRIMARY_PLANE_SCANOUT,
            (true, true) => {
                FrameFlags::ALLOW_PRIMARY_PLANE_SCANOUT | FrameFlags::ALLOW_OVERLAY_PLANE_SCANOUT
            }
        };
        let queued = match s.compositor.render_frame(
            &mut data.renderer,
            &elements,
            clear,
            frame_flags,
        ) {
            Ok(result) if !result.is_empty => {
                use smithay::backend::drm::compositor::PrimaryPlaneElement;
                let scanout = matches!(result.primary_element, PrimaryPlaneElement::Element(_));
                let (overlays, cursor) = (
                    result.overlay_elements.len(),
                    result.cursor_element.is_some(),
                );
                match s.compositor.queue_frame(()) {
                    Ok(()) => {
                        s.flip_failures = 0;
                        s.queued_locked = locked;
                        true
                    }
                    Err(e) => {
                        s.flip_failures += 1;
                        if s.flip_failures % 100 == 1 {
                            eprintln!(
                                "[infynitae] Page flip refused ({} in a row): primary={} overlays={overlays} cursor={cursor} elements={}: {e:?}",
                                s.flip_failures,
                                if scanout {
                                    "client buffer"
                                } else {
                                    "composited"
                                },
                                elements.len()
                            );
                            // which element the kernel refused, if the refusal names a
                            // rectangle (`journalctl -k` with drm.debug)
                            let geometries: Vec<_> = elements
                                .iter()
                                .map(|e| {
                                    smithay::backend::renderer::element::Element::geometry(
                                        e,
                                        smithay::utils::Scale::from(k),
                                    )
                                })
                                .collect();
                            eprintln!("[infynitae] Refused frame's elements: {geometries:?}");
                        }
                        if scanout {
                            // the client's buffer may be what the kernel refuses: compose for a while
                            s.no_scanout_until =
                                Some(std::time::Instant::now() + Duration::from_secs(5));
                        }
                        if s.flip_failures.is_multiple_of(3) {
                            let _ = s.compositor.reset_state();
                        }
                        // retry after a refresh period, not at once (a busy loop that stalled
                        // the whole machine while Discord's updater drew)
                        let crtc = s.crtc;
                        let period = data
                            .state
                            .core()
                            .output(id)
                            .map(|m| m.refresh_mhz)
                            .filter(|r| *r > 0)
                            .map_or(Duration::from_millis(16), |r| {
                                Duration::from_nanos(1_000_000_000_000 / u64::from(r))
                            });
                        s.pending = true;
                        let _ = data.loop_handle.insert_source(
                            smithay::reexports::calloop::timer::Timer::from_duration(period),
                            move |_, _, data| {
                                if let Some(s) = data.screens.iter_mut().find(|s| s.crtc == crtc) {
                                    s.pending = false;
                                }
                                data.session.request_frame();
                                smithay::reexports::calloop::timer::TimeoutAction::Drop
                            },
                        );
                        false
                    }
                }
            }
            Ok(_) => {
                // nothing changed: the screen already shows these elements
                if locked {
                    s.shows_locked = true;
                }
                false
            }
            Err(e) => {
                eprintln!("[infynitae] Render: {e:?}");
                false
            }
        };
        let s = &mut data.screens[i];
        if s.flip_failures > 0 && !queued {
            // waiting for the retry timer (above); its frame callbacks complete as usual
            let now = infynitae_wayland::presentation_clock_now();
            data.state.frame_tick(id, now);
            continue;
        }
        s.pending = queued;
        if queued {
            data.session.count_frame();
        } else {
            let now = infynitae_wayland::presentation_clock_now();
            data.state.frame_tick(id, now);
        }
    }
    confirm_lock(data);
    data.session.flush();
}

/// Locked, and no screen shows a window any more: the lock client is told it holds the lock.
/// A frame counts once its vblank has come (a queued or refused one is not on screen).
fn confirm_lock(data: &mut LoopData) {
    if data.state.is_locked() && data.screens.iter().all(|s| s.shows_locked) {
        data.state.lock_frame_drawn();
    }
}

/// Serves the captures waiting for `output_id`: the screen's (its frame, as shown), and the
/// windows captured alone (each drawn by itself, `convert::window_capture_elements`).
fn serve_captures(
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
    let (windows, pending): (Vec<_>, Vec<_>) = state
        .screencopy
        .take_pending(output_id)
        .into_iter()
        .partition(|p| p.window.is_some());
    for req in windows {
        let Some(window) = req.window else { continue };
        match convert::window_capture_elements(state, renderer, window) {
            Ok(Some((elements, k, size))) => {
                // a window's own transparency is kept
                let mut texture = None;
                handle_screencopy(
                    renderer,
                    &mut texture,
                    vec![req],
                    &elements,
                    size,
                    k,
                    smithay::backend::renderer::Color32F::TRANSPARENT,
                );
            }
            _ => req.fail(),
        }
    }
    if pending.is_empty() {
        return;
    }

    let needs_cursor = !state.screencopy.hide_cursor() && pending.iter().any(|p| p.overlay_cursor);
    // locked: a capture shows the lock screen, never a window
    let clear = if state.is_locked() {
        convert::LOCKED_CLEAR
    } else {
        clear
    };
    let frame = match convert::lock_elements(state, renderer, output_id) {
        Ok(Some(f)) => Ok(f),
        Ok(None) => convert::output_elements_screencopy(state, renderer, output_id, needs_cursor)
            .map_err(|e| format!("{e:?}")),
        Err(e) => Err(format!("{e:?}")),
    };
    let (elements, k) = match frame {
        Ok(res) => res,
        Err(e) => {
            eprintln!("[screencopy] Failed to get elements: {e}");
            for p in pending {
                p.fail();
            }
            return;
        }
    };
    handle_screencopy(
        renderer,
        screencopy_texture,
        pending,
        &elements,
        size,
        k,
        clear,
    );
}

/// Renders `elements` (`size` px at `k`) into each waiting capture's buffer: a dmabuf on the GPU,
/// a shm buffer through a copy (U-008).
#[allow(unsafe_code)]
fn handle_screencopy<E>(
    renderer: &mut GlesRenderer,
    screencopy_texture: &mut Option<GlesTexture>,
    pending: Vec<infynitae_wayland::screencopy::PendingFrame>,
    elements: &[E],
    size: (i32, i32),
    k: f64,
    clear: smithay::backend::renderer::Color32F,
) where
    E: smithay::backend::renderer::element::RenderElement<GlesRenderer>,
{
    let now = infynitae_wayland::presentation_clock_now();

    // -------------------------------------------------------------------------
    // 1. Direct DMA-BUF GPU Render
    // If the client supplied a linux_dmabuf (PipeWire, OBS, gpu-screen-recorder),
    // import it as a GPU texture and render the composite scene directly onto it.
    // -------------------------------------------------------------------------
    let mut remaining = Vec::new();

    for req in pending {
        if let Ok(dmabuf) = smithay::wayland::dmabuf::get_dmabuf(&req.buffer) {
            use smithay::backend::renderer::ImportDma;
            let mut dmabuf_rendered = false;
            if let Ok(mut dmabuf_tex) = renderer.import_dmabuf(dmabuf, None)
                && let Ok(mut dmabuf_fb) = renderer.bind(&mut dmabuf_tex)
            {
                let target_size = req.region.map(|(_, _, w, h)| (w, h)).unwrap_or(size);
                let (rx, ry) = screencopy_region_offset(req.region);
                let mut damage_tracker = OutputDamageTracker::new(
                    target_size,
                    smithay::utils::Scale::from(k),
                    smithay::utils::Transform::Normal,
                );
                let render_res = if rx != 0 || ry != 0 {
                    let relocated: Vec<_> = elements
                        .iter()
                        .map(|e| {
                            smithay::backend::renderer::element::utils::RelocateRenderElement::from_element(
                                e,
                                smithay::utils::Point::from((-rx, -ry)),
                                smithay::backend::renderer::element::utils::Relocate::Relative,
                            )
                        })
                        .collect();
                    damage_tracker.render_output(renderer, &mut dmabuf_fb, 0, &relocated, clear)
                } else {
                    damage_tracker.render_output(renderer, &mut dmabuf_fb, 0, elements, clear)
                };
                if render_res.is_ok() {
                    dmabuf_rendered = true;
                    req.ready((target_size.0 as u32, target_size.1 as u32), now);
                }
            }
            if !dmabuf_rendered {
                // Non-GPU-importable dmabufs fail: CPU fallback into client mmaps risks SIGBUS
                req.fail();
            }
        } else {
            remaining.push(req);
        }
    }

    if remaining.is_empty() {
        // All requests were fulfilled via direct GPU DMA-BUF rendering.
        return;
    }

    // -------------------------------------------------------------------------
    // 2. Fallback for wl_shm or CPU memory buffers
    // Executed when at least one client requested a CPU or shm buffer.
    // -------------------------------------------------------------------------
    if screencopy_texture
        .as_ref()
        .is_none_or(|t| t.width() != size.0 as u32 || t.height() != size.1 as u32)
    {
        match Offscreen::<GlesTexture>::create_buffer(renderer, Fourcc::Argb8888, size.into()) {
            Ok(b) => *screencopy_texture = Some(b),
            Err(e) => {
                eprintln!("[screencopy] Failed to create offscreen buffer: {e:?}");
                for p in remaining {
                    p.fail();
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
            for p in remaining {
                p.fail();
            }
            return;
        }
    };

    let mut damage_tracker = OutputDamageTracker::new(
        size,
        smithay::utils::Scale::from(k),
        smithay::utils::Transform::Normal,
    );
    if let Err(e) = damage_tracker.render_output(renderer, &mut framebuffer, 0, elements, clear) {
        eprintln!("[screencopy] Failed to render output: {e:?}");
        for p in remaining {
            p.fail();
        }
        return;
    }

    let mapping = match ExportMem::copy_framebuffer(
        renderer,
        &framebuffer,
        Rectangle::from_size(size.into()),
        Fourcc::Argb8888,
    ) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[screencopy] Failed to copy framebuffer: {e:?}");
            for p in remaining {
                p.fail();
            }
            return;
        }
    };

    let slice = match ExportMem::map_texture(renderer, &mapping) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[screencopy] Failed to map texture: {e:?}");
            for p in remaining {
                p.fail();
            }
            return;
        }
    };

    let src_stride = (size.0 * 4) as usize;

    for req in remaining {
        let (rx, ry) = req
            .region
            .map(|(x, y, _, _)| (x.max(0) as usize, y.max(0) as usize))
            .unwrap_or((0, 0));

        let copy_result = with_buffer_contents_mut(&req.buffer, |ptr, len, buffer_data| {
            if buffer_data.format
                != smithay::reexports::wayland_server::protocol::wl_shm::Format::Argb8888
                && buffer_data.format
                    != smithay::reexports::wayland_server::protocol::wl_shm::Format::Xrgb8888
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

        if matches!(copy_result, Ok(true)) {
            let target_size = req.region.map(|(_, _, w, h)| (w, h)).unwrap_or(size);
            req.ready((target_size.0 as u32, target_size.1 as u32), now);
        } else {
            req.fail();
        }
    }
}

fn handle_input(data: &mut LoopData, event: InputEvent<LibinputInputBackend>) {
    data.last_user_activity = std::time::Instant::now();
    if data.dpms_active {
        println!("[infynitae] User input detected: waking up screens from DPMS sleep");
        data.dpms_active = false;
        for s in &mut data.screens {
            let _ = s.compositor.reset_state();
            s.pending = false;
            s.shows_locked = false;
        }
        // Re-apply client's gamma ramp after DPMS wake
        {
            let gamma_map = data.gamma_screens.lock().unwrap();
            for screen in gamma_map.values() {
                if let Some((r, g, b)) = &screen.current_gamma {
                    let _ = data.drm.device_fd().set_gamma(screen.crtc, r, g, b);
                }
            }
        }
        data.state
            .set_all_outputs_power_mode(infynitae_wayland::output_power::OutputPowerMode::On);
        data.session.request_frame();
    }
    if let InputEvent::PointerButton { event } = &event {
        let button = event.button_code();
        data.buttons_down.retain(|b| *b != button);
        if matches!(event.state(), ButtonState::Pressed) {
            data.buttons_down.push(button);
        }
    }
    // locked: pointer and touch go nowhere (keys go to the lock surface, below); a button
    // Core holds and that is released meanwhile gets its release after the unlock
    if data.state.is_locked()
        && !matches!(
            event,
            InputEvent::Keyboard { .. }
                | InputEvent::DeviceAdded { .. }
                | InputEvent::DeviceRemoved { .. }
        )
    {
        return;
    }
    match event {
        InputEvent::Keyboard { event } => {
            let key = event.key_code().raw().saturating_sub(8);
            let pressed = matches!(event.state(), KeyState::Pressed);
            // T6: the console's own keys, before Core (and so before any binding or client)
            if pressed {
                let m = data.state.seat_modifiers();
                match console_key(key, m.ctrl, m.alt) {
                    Some(ConsoleKey::Quit) if data.state.is_locked() => {
                        // quitting would restart the session unlocked: not while locked
                        println!("[infynitae] Ctrl+Alt+Backspace ignored: the session is locked");
                        data.taken.push(key);
                        return;
                    }
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
            // the session lock takes keys before Core (lock.rs)
            if !data.state.lock_key(key, pressed, time_us) {
                core_key(data, key, pressed, time_us);
            }
            update_keyboard_leds(&data.state, &mut data.keyboards, &mut dev);
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
            let scale = screen
                .and_then(|s| data.state.core().output(s.id).map(|o| o.scale))
                .unwrap_or_else(|| (f64::from(data.scale_120) / 120.0).max(0.001));
            let delta =
                calculate_abs_pointer_delta_scale((target_phys_x, target_phys_y), current_p, scale);
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
        InputEvent::TouchDown { event } => {
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
            let scale = screen
                .and_then(|s| data.state.core().output(s.id).map(|o| o.scale))
                .unwrap_or_else(|| (f64::from(data.scale_120) / 120.0).max(0.001));
            let delta =
                calculate_abs_pointer_delta_scale((target_phys_x, target_phys_y), current_p, scale);
            data.last_abs_pointer = Some((target_phys_x / scale, target_phys_y / scale));
            let time_us = Event::time(&event);
            if delta.0.abs() > 0.0001 || delta.1.abs() > 0.0001 {
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
            let btn_phys = PhysicalEvent::PointerButton {
                button: 272, // BTN_LEFT
                pressed: true,
                modifiers: data.state.seat_modifiers(),
                time_us,
            };
            let port = &mut data.input_port;
            let _ = data
                .state
                .with_effects(|core, fx| core.physical_input(port, btn_phys, fx));
        }
        InputEvent::TouchMotion { event } => {
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
            let scale = screen
                .and_then(|s| data.state.core().output(s.id).map(|o| o.scale))
                .unwrap_or_else(|| (f64::from(data.scale_120) / 120.0).max(0.001));
            let delta =
                calculate_abs_pointer_delta_scale((target_phys_x, target_phys_y), current_p, scale);
            data.last_abs_pointer = Some((target_phys_x / scale, target_phys_y / scale));
            let time_us = Event::time(&event);
            if delta.0.abs() > 0.0001 || delta.1.abs() > 0.0001 {
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
        InputEvent::TouchUp { event } => {
            let time_us = Event::time(&event);
            let btn_phys = PhysicalEvent::PointerButton {
                button: 272, // BTN_LEFT
                pressed: false,
                modifiers: data.state.seat_modifiers(),
                time_us,
            };
            let port = &mut data.input_port;
            let _ = data
                .state
                .with_effects(|core, fx| core.physical_input(port, btn_phys, fx));
        }
        InputEvent::TouchCancel { event } => {
            let time_us = Event::time(&event);
            let btn_phys = PhysicalEvent::PointerButton {
                button: 272, // BTN_LEFT
                pressed: false,
                modifiers: data.state.seat_modifiers(),
                time_us,
            };
            let port = &mut data.input_port;
            let _ = data
                .state
                .with_effects(|core, fx| core.physical_input(port, btn_phys, fx));
        }
        InputEvent::DeviceAdded { mut device } => {
            configure_input_device(&mut device, data.state.input_config());
            let is_kb =
                device.has_capability(smithay::reexports::input::DeviceCapability::Keyboard);
            let is_ptr =
                device.has_capability(smithay::reexports::input::DeviceCapability::Pointer);
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
                data.keyboards.push(device.clone());
            }
            data.input_devices.push(device);
        }
        InputEvent::DeviceRemoved { device } => {
            data.keyboards.retain(|k| k.sysname() != device.sysname());
            data.input_devices
                .retain(|d| d.sysname() != device.sysname());
            data.last_abs_pointer = None;
        }
        _ => {}
    }
}

/// A key for Core: the seat's XKB state first (through the input token), then Core's intake.
fn core_key(data: &mut LoopData, key: u32, pressed: bool, time_us: u64) {
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
/// `(abs_x, abs_y)` transformed to screen pixels and normalized by `scale`.
pub fn calculate_abs_pointer_delta_scale(
    abs_px: (f64, f64),
    current_p: (f64, f64),
    scale: f64,
) -> (f64, f64) {
    let scale = scale.max(0.001);
    let target_logical = (abs_px.0 / scale, abs_px.1 / scale);
    (
        target_logical.0 - current_p.0,
        target_logical.1 - current_p.1,
    )
}

/// Calculates the logical pointer delta needed to move from `current_p` to an absolute position
/// `(abs_x, abs_y)` transformed to screen pixels and normalized by `scale_120`.
pub fn calculate_abs_pointer_delta(
    abs_px: (f64, f64),
    current_p: (f64, f64),
    scale_120: u32,
) -> (f64, f64) {
    let scale = (f64::from(scale_120) / 120.0).max(0.001);
    calculate_abs_pointer_delta_scale(abs_px, current_p, scale)
}

fn configure_input_device(
    device: &mut smithay::reexports::input::Device,
    config: &infynitae_wayland::InputDeviceConfig,
) {
    use smithay::reexports::input::AccelProfile;

    // 1. Mouse Acceleration Profile (flat = raw 1:1, adaptive = curve)
    let env_profile = std::env::var("MOUSE_ACCEL_PROFILE").ok();
    let profile = config
        .mouse_accel_profile
        .as_deref()
        .or(env_profile.as_deref());
    if let Some(profile_str) = profile {
        let desired = match profile_str.to_lowercase().as_str() {
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
    let speed = config.mouse_accel_speed.or_else(|| {
        std::env::var("MOUSE_ACCEL_SPEED")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
    });
    if let Some(speed) = speed {
        let clamped = speed.clamp(-1.0, 1.0);
        let _ = device.config_accel_set_speed(clamped);
    }

    // 3. Touchpad Tap-to-click
    let tap = config.touchpad_tap.or_else(|| {
        std::env::var("TOUCHPAD_TAP")
            .ok()
            .map(|t| matches!(t.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
    });
    if let Some(enable) = tap
        && device.config_tap_finger_count() > 0
    {
        let _ = device.config_tap_set_enabled(enable);
    }

    // 4. Touchpad Natural Scrolling
    let nat = config.touchpad_natural_scroll.or_else(|| {
        std::env::var("TOUCHPAD_NATURAL_SCROLL")
            .ok()
            .map(|t| matches!(t.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
    });
    if let Some(enable) = nat
        && device.config_scroll_has_natural_scroll()
    {
        let _ = device.config_scroll_set_natural_scroll_enabled(enable);
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

    #[test]
    fn pointer_motion_absolute_delta_maps_to_target_with_output_scale() {
        // 1.5x scale
        let delta = calculate_abs_pointer_delta_scale((1500.0, 900.0), (0.0, 0.0), 1.5);
        assert_eq!(delta, (1000.0, 600.0));

        // 2.0x scale
        let delta = calculate_abs_pointer_delta_scale((1920.0, 1080.0), (500.0, 300.0), 2.0);
        assert_eq!(delta, (460.0, 240.0));
    }

    #[test]
    fn idle_timeout_remaining_calculates_sleep_and_honors_inhibitions() {
        let start = std::time::Instant::now();
        // 10 minutes timeout, 3 minutes elapsed -> 7 minutes remaining
        let remaining =
            idle_timeout_remaining(10, false, false, start, start + Duration::from_secs(3 * 60));
        assert_eq!(remaining, Some(Duration::from_secs(7 * 60)));

        // 10 minutes timeout, 12 minutes elapsed -> 0 remaining (saturating)
        let remaining = idle_timeout_remaining(
            10,
            false,
            false,
            start,
            start + Duration::from_secs(12 * 60),
        );
        assert_eq!(remaining, Some(Duration::ZERO));

        // When DPMS is already active -> None (no wakeups while idle)
        assert_eq!(idle_timeout_remaining(10, true, false, start, start), None);

        // When idle is inhibited -> None
        assert_eq!(idle_timeout_remaining(10, false, true, start, start), None);

        // When timeout is 0 (disabled) -> None
        assert_eq!(idle_timeout_remaining(0, false, false, start, start), None);
    }

    #[test]
    fn screencopy_region_origin_offset() {
        assert_eq!(
            screencopy_region_offset(Some((100, 200, 800, 600))),
            (100, 200)
        );
        assert_eq!(screencopy_region_offset(None), (0, 0));
        assert_eq!(screencopy_region_offset(Some((0, 0, 1920, 1080))), (0, 0));
    }

    #[test]
    fn connector_name_matching_accepts_kernel_and_debug_names_exact_only() {
        // Kernel name match
        assert!(matches_connector_name("DP-1", "DP-1", "DisplayPort-1"));
        assert!(matches_connector_name("dp-1", "DP-1", "DisplayPort-1"));
        assert!(matches_connector_name("HDMI-A-1", "HDMI-A-1", "HDMIA-1"));
        assert!(matches_connector_name("hdmi-a-1", "HDMI-A-1", "HDMIA-1"));

        // Legacy debug name match
        assert!(matches_connector_name(
            "DisplayPort-1",
            "DP-1",
            "DisplayPort-1"
        ));
        assert!(matches_connector_name(
            "displayport-1",
            "DP-1",
            "DisplayPort-1"
        ));
        assert!(matches_connector_name("HDMIA-1", "HDMI-A-1", "HDMIA-1"));
        assert!(matches_connector_name("hdmia-1", "HDMI-A-1", "HDMIA-1"));

        // No substring false positives ("displayport-1" must not match "displayport-10")
        assert!(!matches_connector_name(
            "displayport-1",
            "DP-10",
            "DisplayPort-10"
        ));
        assert!(!matches_connector_name("dp-1", "DP-10", "DisplayPort-10"));
        assert!(!matches_connector_name(
            "DisplayPort-10",
            "DP-1",
            "DisplayPort-1"
        ));

        // Mismatched connectors
        assert!(!matches_connector_name("HDMI-A-1", "DP-1", "DisplayPort-1"));
    }

    #[test]
    fn scale_validation_accepts_range_and_rejects_outside() {
        assert_eq!(validate_scale(1.0), Some(120));
        assert_eq!(validate_scale(0.25), Some(30));
        assert_eq!(validate_scale(8.0), Some(960));
        assert_eq!(validate_scale(1.5), Some(180));

        // Outside range (0.25..=8.0)
        assert_eq!(validate_scale(0.24), None);
        assert_eq!(validate_scale(0.0), None);
        assert_eq!(validate_scale(-1.0), None);
        assert_eq!(validate_scale(8.01), None);
        assert_eq!(validate_scale(10.0), None);
        assert_eq!(validate_scale(f64::NAN), None);
        assert_eq!(validate_scale(f64::INFINITY), None);
    }

    #[test]
    fn unrecognised_custom_mode_falls_back_to_preferred() {
        let m = |w, h, hz: u32, preferred| ModeInfo {
            size: (w, h),
            refresh_mhz: hz * 1000,
            preferred,
        };
        let modes = [
            m(1920, 1080, 60, true),
            m(1920, 1080, 144, false),
            m(1280, 720, 60, false),
        ];
        // Custom 4K mode that is not supported returns None
        assert_eq!(choose_mode_custom(&modes, Some((3840, 2160)), None), None);
        // Fallback to choose_mode succeeds and picks preferred resolution at highest refresh
        let fallback =
            choose_mode_custom(&modes, Some((3840, 2160)), None).or_else(|| choose_mode(&modes));
        assert_eq!(fallback, Some(1));
    }
}
