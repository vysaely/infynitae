//! Infynitae Native console backend: the compositor owns the display and the input devices of a
//! seat, started from a text console (FOUNDATION_ARCHITECTURE C1.1; owner decisions T1–T9 of
//! 2026-10-22, `~/infynitae/TTY_BACKEND_DECISIONS_PROPOSAL_2026-10-21.md`).
//!
//! - **Session (T2):** libseat opens the GPU and the input devices without root; a VT switch pauses
//!   the session (input suspended, DRM master dropped) and resumes it.
//! - **Rendering (T3):** GLES on GBM/EGL through Smithay's `DrmCompositor`, page-flipped on vblank.
//!   Client dmabufs and shm buffers are imported on the GPU render node.
//! - **Screens (T5):** every connected connector, at its preferred mode, on the primary GPU.
//!   Hotplug: a screen plugged in, unplugged or woken is found once the GPU's udev changes have
//!   been quiet for a moment (`rescan_screens`); starting with no screen waits for one.
//! - **Input (T4):** libinput on the seat; the keyboard layout from `XKB_DEFAULT_*` (xkbcommon's
//!   defaults otherwise).
//! - **Safety (T6):** Ctrl+Alt+Backspace quits; Ctrl+Alt+F1…F12 switch VT. Both are taken before
//!   Core sees the key, so no binding or client can swallow them.
//! - **Cursor (T8):** drawn by the compositor (`infynitae_wayland::cursor`).

#![deny(unsafe_code)]

mod capture;
mod edid;
mod input;
mod render;
mod scanout_log;
mod screens;
mod smooth;
mod timing;

pub use input::{calculate_abs_pointer_delta_scale, parse_lock_when_away, resolve_touchpad_tap};
pub use screens::{VIRTUAL_SCREEN_SIZE, virtual_screen_change};

use capture::{
    CaptureSeen, CopyWorker, PendingCopy, deliver_copies, serve_captures, watch_copy_sync,
};
use input::{
    apply_virtual_input, configure_input_device, core_key, core_touch, dispatch_libinput_event,
};
use render::{confirm_lock, render, sync_scaling_filter, watch_frame_waits};
use screens::{
    HOTPLUG_SETTLE, ScreenFactory, apply_display_change, apply_output_configuration,
    connect_screen, rescan_screens, update_virtual_screen,
};
use timing::{SlowTurnLog, TurnClock, latency_summary, slow_turn_threshold};

use std::collections::{HashMap, HashSet, VecDeque};
use std::error::Error;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use infynitae_core::output::{OutputDescriptor, OutputId, OutputKey, Transform};
use infynitae_core::{Core, InputPort, OutputLifecycleToken, PhysicalEvent};
use infynitae_wayland::display_settings::{DisplayChange, parse_transform};
use infynitae_wayland::output_management::{HeadChange, HeadInfo, HeadMode};
use infynitae_wayland::session::{Session, SessionOptions};
use infynitae_wayland::{FrameDone, WaylandState, convert, log_line};

use smithay::backend::allocator::Fourcc;
use smithay::backend::allocator::gbm::{GbmAllocator, GbmBufferFlags, GbmDevice};
use smithay::backend::drm::compositor::{DrmCompositor, FrameFlags};
use smithay::backend::drm::exporter::gbm::GbmFramebufferExporter;
use smithay::backend::drm::{DrmDevice, DrmDeviceFd, DrmEvent};
use smithay::backend::egl::fence::EGLFence;
use smithay::backend::egl::{EGLContext, EGLDisplay};
use smithay::backend::input::{
    AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, GestureBeginEvent,
    GestureEndEvent, GesturePinchUpdateEvent, GestureSwipeUpdateEvent, InputEvent, KeyState,
    KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent, PointerMotionEvent, ProximityState,
    Switch, SwitchState, SwitchToggleEvent, TabletToolButtonEvent, TabletToolEvent,
    TabletToolProximityEvent, TabletToolTipEvent, TabletToolTipState,
};
use smithay::backend::libinput::{LibinputInputBackend, LibinputSessionInterface};
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::gles::{Capability, GlesRenderer, GlesTexture};
use smithay::backend::renderer::sync::SyncPoint;
use smithay::backend::renderer::{Bind, ExportMem, Offscreen, Texture};
use smithay::backend::renderer::{Renderer, TextureFilter};
use smithay::backend::session::libseat::LibSeatSession;
use smithay::backend::session::{Event as SeatEvent, Session as _};
use smithay::output::OutputModeSource;
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::calloop::{EventLoop, Interest, Mode, PostAction};
use smithay::reexports::drm::control::{Device, ModeTypeFlags, connector, crtc, property};
use smithay::reexports::input::Libinput;
use smithay::reexports::input::event::touch::TouchEventSlot;
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
    /// Scaling texture filter ("sharp" / "nearest", "smooth" / "linear", "fsr", "fsr-sharp", "fast").
    pub scaling_filter: Option<String>,
    /// Direct scanout for fullscreen apps and games.
    pub direct_scanout: Option<bool>,
    /// Variable Refresh Rate (VRR / Adaptive Sync): always on, off, or `None` (auto) only while
    /// the screen shows a fullscreen window (games, fullscreen video). Always-on VRR on the
    /// desktop makes video and cursor pacing uneven and flickers on some monitors.
    pub vrr: Option<bool>,
    /// Start locked (`--locked`): locked before any client runs, until a lock client unlocks.
    pub locked: bool,
    /// `VIRTUAL_SCREEN`: with no screen connected (a monitor off or unplugged), a virtual one
    /// stands in, so remote desktops and captures still have a screen.
    pub virtual_screen: bool,
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

/// Maps a DRM connector `"panel orientation"` enum name to the Core output transform that shows
/// the image upright. Returns `None` for `"Unknown"` or unrecognised strings. Pure.
pub fn panel_orientation_transform(orientation: &str) -> Option<Transform> {
    match orientation.trim().to_ascii_lowercase().as_str() {
        "normal" => Some(Transform::Normal),
        "upside down" => Some(Transform::R180),
        "left side up" => Some(Transform::R90),
        "right side up" => Some(Transform::R270),
        _ => None,
    }
}

/// Picks the initial transform for a connector: an explicit per-output setting (`OUTPUT_<NAME>_TRANSFORM`)
/// wins; otherwise the DRM `"panel orientation"` property is used; otherwise `Normal`. Pure.
pub fn resolve_output_transform(
    custom: Option<Transform>,
    panel_orientation: Option<&str>,
) -> Transform {
    custom
        .or_else(|| panel_orientation.and_then(panel_orientation_transform))
        .unwrap_or(Transform::Normal)
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

/// Time remaining until the next logind idle-inhibit background query is due.
/// Starts 5 s before the idle timeout expires so the answer is ready without
/// blocking the main loop. None when idle blanking is disabled, already active,
/// or inhibited by Wayland. Pure.
pub fn logind_query_remaining(
    idle_sleep_timeout_min: u32,
    dpms_active: bool,
    idle_inhibited: bool,
    last_user_activity: std::time::Instant,
    now: std::time::Instant,
) -> Option<Duration> {
    if !dpms_active && idle_sleep_timeout_min > 0 && !idle_inhibited {
        let total = Duration::from_secs(u64::from(idle_sleep_timeout_min) * 60);
        let lead = Duration::from_secs(5).min(total / 2);
        let query_at = total.saturating_sub(lead);
        let elapsed = now.saturating_duration_since(last_user_activity);
        Some(query_at.saturating_sub(elapsed))
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

/// The mode to use with optional custom target resolution and refresh rate, chosen as a config
/// reload chooses it (`display_settings::choose_mode`).
pub fn choose_mode_custom(
    modes: &[ModeInfo],
    target_size: Option<(u16, u16)>,
    target_rate: Option<u32>,
) -> Option<usize> {
    let heads: Vec<HeadMode> = modes
        .iter()
        .map(|m| HeadMode {
            size: (i32::from(m.size.0), i32::from(m.size.1)),
            refresh_mhz: m.refresh_mhz as i32,
            preferred: m.preferred,
        })
        .collect();
    infynitae_wayland::display_settings::choose_mode(&heads, target_size, target_rate)
}

type Compositor =
    DrmCompositor<GbmAllocator<DrmDeviceFd>, GbmFramebufferExporter<DrmDeviceFd>, (), DrmDeviceFd>;

/// One screen: its Core output, its CRTC and its compositor.
struct Screen {
    id: OutputId,
    /// The connector it is shown on (hotplug finds it by this).
    connector: connector::Handle,
    /// Connector name (e.g. `eDP-1`, `HDMI-A-1`) for matching `WL_OUTPUT` touch devices.
    connector_name: String,
    /// Panel physical dimensions in mm `(width, height)`, `(0, 0)` when unknown.
    physical_size: (u32, u32),
    crtc: crtc::Handle,
    size: (i32, i32),
    compositor: Compositor,
    /// The k the compositor's mode source was set with.
    k: f64,
    /// The rotation it is drawn through (`OUTPUT_<NAME>_TRANSFORM`, display settings tools).
    transform: smithay::utils::Transform,
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
    /// Async page flips the kernel refused here in a row (a frame that also moved the cursor
    /// plane is one); at [`TEARING_REFUSALS`] no tearing for [`TEARING_RETRY`].
    tearing_refused: u32,
    /// When tearing is tried again after too many refusals.
    tearing_retry_at: Option<std::time::Instant>,
    /// Whether the session log last said tearing was on.
    tearing_logged: bool,
    /// Current fullscreen game window (`exclusive_window`) and refusal reasons already logged for it.
    last_tearing_game: Option<infynitae_core::stage::ElementId>,
    tearing_refusals_logged: Vec<String>,
    /// The monitor and driver support VRR on this connector.
    vrr_supported: bool,
    /// VRR is on now.
    vrr_on: bool,
    /// This screen's VRR mode: `VRR_ENABLED` until a display settings tool changes it (always
    /// on, off, or `None`: while a window is fullscreen).
    vrr_mode: Option<bool>,
    /// Screen power mode turned off by output power management.
    power_off: bool,
    /// The connector's modes, as display settings tools see them and as DRM sets them.
    modes: Vec<(HeadMode, smithay::reexports::drm::control::Mode)>,
    /// When to draw its next frame: just in time for its next refresh (LATENCY).
    schedule: infynitae_wayland::frame_schedule::FrameSchedule,
    /// Whether this screen spares the GPU (`GPU_RELIEF`).
    relief: infynitae_wayland::frame_schedule::FrameRelief,
    /// Whether `GPU_RELIEF` is currently acting on this screen.
    relief_on: bool,
    /// A frame had nothing to show since the last refresh.
    empty_since_refresh: bool,
    /// Whether its last frame went to the screen without being drawn.
    scanned_out: bool,
    /// A frame wanted, held back until then (presentation clock).
    deferred: Option<std::time::Duration>,
    /// Input-to-screen times (LATENCY_STATS).
    latency: infynitae_wayland::frame_schedule::LatencyStats,
    /// The last logged (fullscreen shown, its buffer scanned out) pair: a change is logged, so
    /// the log says whether a game's frames reach the screen as they are or are composited.
    scanout_logged: Option<(bool, bool)>,
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

struct LoopData {
    state: WaylandState,
    session: Session,
    seat: LibSeatSession,
    input_port: InputPort,
    output_token: OutputLifecycleToken,
    drm: DrmDevice,
    /// Before `renderer`: dropped first, so its thread is done reading the renderer's mappings.
    capture_worker: CopyWorker,
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
    /// Which device holds which button or key down: (device sysname, code, is a key). A device
    /// that goes away (unplugged, a macro's virtual mouse closed) with one down never sends its
    /// release; it is released for it, or every click went to the window that had the press
    /// (owner, 2026-11-15: "I can interact only with AEAM").
    held_by_device: Vec<(String, u32, bool)>,
    /// Active keyboard devices for hardware LED synchronization (Caps Lock, Num Lock).
    keyboards: Vec<smithay::reexports::input::Device>,
    /// All active input devices (keyboards, mice, touchpads) for live config updates.
    input_devices: Vec<smithay::reexports::input::Device>,
    /// Active touch slots: `(seat_slot, device sysname, output)` so motions stay on the touch's
    /// initial output and unplugging one touchscreen cancels its slots.
    touch_slots: Vec<(u32, String, OutputId)>,
    /// Last absolute pointer logical position, used to compute delta for absolute devices (e.g. RustDesk, tablets).
    last_abs_pointer: Option<(f64, f64)>,
    /// Set of known mapped element IDs for detecting newly mapped windows.
    known_mapped: HashSet<infynitae_core::stage::ElementId>,
    /// Last seen stage map generation for caching mapped window set.
    last_mapped_generation: u64,
    /// Direct scanout for primary and overlay planes.
    direct_scanout: bool,
    /// Compiled shaders for zoomed-out downsampling and zoomed-in FSR 1; `None` if unavailable.
    zoom_programs: Option<std::sync::Arc<smooth::ZoomPrograms>>,
    /// Per-window cached downsample mip pyramids and EASU offscreen textures.
    zoom_cache: smooth::ZoomCache,
    /// `SCALING_FILTER`, updated live on config reload and reused after a GPU reset.
    scaling_filter: infynitae_wayland::ZoomFilter,
    /// When the renderer was last made again after a GPU reset (at most once a second: GPU
    /// memory still full makes the next one fail too).
    renderer_recovered_at: Option<std::time::Instant>,
    config_path: Option<PathBuf>,
    last_config_check: std::time::Instant,
    last_config_mtime: Option<std::time::SystemTime>,
    last_user_activity: std::time::Instant,
    /// The longest turn of the loop since the last LATENCY_STATS summary.
    loop_busy_max: Duration,
    /// The last LATENCY_STATS summary.
    latency_logged: Option<std::time::Instant>,
    /// Slowest turn of each 1 s window, flushed when the second ends.
    slow_turn: SlowTurnLog,
    /// Whether the immediately preceding loop turn took at least one refresh period.
    last_turn_slow: bool,
    /// Capture streams and one-shot clients already logged with their path (`dmabuf` vs `shm`).
    capture_log: CaptureSeen<(
        Option<smithay::reexports::wayland_server::backend::ClientId>,
        Option<infynitae_wayland::screencopy::Consumer>,
    )>,
    dpms_active: bool,
    idle_sleep_timeout_min: u32,
    idle_auto_lock: bool,
    lock_process: Option<std::process::Child>,
    logind_inhibit: infynitae_wayland::idle_inhibit::LogindInhibitWatcher,
    /// The VRR setting (`ConsoleOptions::vrr`), each screen's until a tool changes it.
    vrr_mode: Option<bool>,
    /// For timers set from the render path (a refused page flip's retry).
    loop_handle: smithay::reexports::calloop::LoopHandle<'static, LoopData>,
    /// Sets up screens connected later.
    factory: ScreenFactory,
    /// The connectors are compared with the screens then (a display change settled; hotplug).
    rescan_at: Option<std::time::Instant>,
    /// The laptop's lid is closed: its panel is left out while another screen is connected.
    lid_closed: bool,
    /// Captures into memory whose GPU copy was started and waits for its GPU fence before mapping
    /// (`deliver_copies`), instead of blocking the loop on `map_texture`.
    capture_copies: Vec<PendingCopy>,
    /// Screens whose `wl_shm` capture was deferred while a readback on that screen was in flight.
    capture_readback_deferred: Vec<OutputId>,
    /// `ConsoleOptions::virtual_screen`.
    virtual_screen_enabled: bool,
    /// The virtual screen standing in while no screen is connected.
    virtual_screen: Option<VirtualScreen>,
}

/// A screen with no monitor: drawn only for captures (remote desktops, screen sharing), its
/// frames paced by the session's frame timer.
struct VirtualScreen {
    id: OutputId,
    screencopy_texture: Option<GlesTexture>,
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
    // lost, not aborted, when the GPU resets or refuses its work (out of GPU memory): the loop
    // then makes a new renderer and every window stays open (`renderer_lost`)
    // and scheduled ahead of other GPU work (a game, a training run): the compositor's frames are
    // small, and a frame queued behind a long job misses its refresh. The kernel grants high
    // priority to the display's master, which this process is; otherwise it falls back.
    let context = EGLContext::new_lose_context_on_reset_with_priority(
        &display,
        smithay::backend::egl::context::ContextPriority::High,
    )?;
    log_line!("[infynitae] GPU context priority: {:?}", context.priority());
    // SAFETY: the context was just made and is current on no other thread; the renderer takes it
    // over and is used on this thread only.
    let renderer = unsafe { GlesRenderer::new(context) }?;
    Ok(renderer)
}

/// Whether the renderer's context is lost: the GPU reset, or refused its work (out of GPU
/// memory). Its frames then draw nothing, and a new renderer is made (`recover_renderer`).
/// Asked after every frame: one GL call (U-009, `docs/foundation/UNSAFE_INVENTORY.md`).
#[allow(unsafe_code)]
fn renderer_lost(renderer: &mut GlesRenderer) -> bool {
    renderer
        .with_context(|gl| {
            if !gl.GetGraphicsResetStatus.is_loaded() {
                return false;
            }
            // SAFETY: `with_context` made the renderer's context current on this thread; the
            // call takes no arguments and only reads the context's reset status.
            let status = unsafe { gl.GetGraphicsResetStatus() };
            status != smithay::backend::renderer::gles::ffi::NO_ERROR
        })
        .unwrap_or(false)
}

/// At the idle timeout: whether logind holds an idle inhibitor (an app over D-Bus, through
/// infynitae-portal's org.freedesktop.ScreenSaver, or `systemd-inhibit --what=idle`). Uses the
/// last known answer delivered through the channel from the background query thread. If so,
/// the timeout is asked again in a minute, not at every loop.
fn logind_inhibits_idle_now(data: &mut LoopData) -> bool {
    logind_inhibits_idle_decision(
        &data.logind_inhibit,
        data.idle_sleep_timeout_min,
        &mut data.last_user_activity,
    )
}

pub(crate) fn logind_inhibits_idle_decision(
    inhibit: &infynitae_wayland::idle_inhibit::LogindInhibitWatcher,
    idle_sleep_timeout_min: u32,
    last_user_activity: &mut std::time::Instant,
) -> bool {
    if !inhibit.is_inhibited() {
        return false;
    }
    let timeout = Duration::from_secs(u64::from(idle_sleep_timeout_min) * 60);
    let recheck = timeout.saturating_sub(Duration::from_secs(60));
    *last_user_activity = std::time::Instant::now()
        .checked_sub(recheck)
        .unwrap_or_else(std::time::Instant::now);
    true
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
    if options.locked {
        state.start_locked();
        log_line!("[infynitae] Started locked: waiting for the lock screen");
    }

    // the seat first: without it nothing can be opened
    let (mut seat, seat_notifier) = LibSeatSession::new()
        .map_err(|e| format!("no seat (is this a console you are logged in on?): {e}"))?;
    let seat_name = seat.seat();
    log_line!("[infynitae] Seat {seat_name}");

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
    log_line!("[infynitae] GPU {}", gpu.display());
    let fd = seat.open(
        &gpu,
        OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
    )?;
    let fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let (mut drm, drm_notifier) = DrmDevice::new(fd.clone(), true)?;
    let gbm = GbmDevice::new(fd.clone())?;
    let mut renderer = gles_renderer(&gbm)?;
    // clients allocate on the GPU this compositor draws with (its render node), not a guess
    let render_node = smithay::backend::drm::DrmNode::from_path(&gpu)
        .ok()
        .map(|node| {
            node.node_with_type(smithay::backend::drm::NodeType::Render)
                .and_then(Result::ok)
                .unwrap_or(node)
        });
    if let Some(node) = render_node {
        // every format and modifier the renderer imports, not only linear
        let formats: Vec<_> = smithay::backend::renderer::ImportDma::dmabuf_formats(&renderer)
            .iter()
            .copied()
            .collect();
        log_line!(
            "[infynitae] Client buffers on GPU node {node} ({} formats/modifiers)",
            formats.len()
        );
        state.set_dmabuf_device_formats(node.dev_id(), formats);
        // Smithay never says which GPU a client's buffer belongs to, and without that the screen
        // refuses it silently. Clients are told to allocate on this one.
        state.set_dmabuf_mark(Box::new(move |dmabuf| dmabuf.set_node(node)));
    }

    // SCALING_FILTER: sharp (default), smooth, fsr, fsr-sharp, or fast.
    let filter =
        infynitae_wayland::ZoomFilter::parse(options.scaling_filter.as_deref().unwrap_or("sharp"));
    state.scaling_filter = filter;
    if filter.uses_nearest_upscale()
        && let Err(e) = renderer.upscale_filter(TextureFilter::Nearest)
    {
        log_line!("[infynitae] Warning: failed to set upscale filter to Nearest: {e:?}");
    }
    let zoom_programs = smooth::zoom_programs(&mut renderer);
    let corners_ready = smooth::init_corners(&mut renderer);
    let allocator = GbmAllocator::new(
        gbm.clone(),
        GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
    );
    // Which GPU's client buffers may go to the screen as they are. Smithay refuses every one
    // without saying so when this is None: no window was ever scanned out (2026-11-03).
    let exporter = GbmFramebufferExporter::new(gbm.clone(), render_node);
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
    let mut custom_transforms: HashMap<String, Transform> = HashMap::new();
    let mut idle_timeout_min = 15u32;
    let mut idle_auto_lock = true;

    if let Ok(home) = std::env::var("HOME") {
        let config_path = format!("{home}/.config/infynitae/config");
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            idle_auto_lock = parse_lock_when_away(&content);
            for (k, v) in content.lines().filter_map(infynitae_wayland::config_line) {
                let key = k.trim();
                let val = v.trim();
                if key == "OUTPUT_ORDER" {
                    output_order = infynitae_wayland::parse_list(val);
                } else if key == "OUTPUT_LAYOUT" && val.eq_ignore_ascii_case("right-to-left") {
                    output_layout_reverse = true;
                } else if key == "IDLE_SLEEP_TIMEOUT_MIN" {
                    if let Ok(m) = val.parse::<u32>() {
                        idle_timeout_min = m;
                    }
                } else if key.starts_with("OUTPUT_") && key.ends_with("_SCALE") {
                    let conn_name = key
                        .trim_start_matches("OUTPUT_")
                        .trim_end_matches("_SCALE")
                        .replace('_', "-");
                    if let Ok(s) = val.parse::<f64>() {
                        if let Some(s_120) = validate_scale(s) {
                            custom_scales.insert(conn_name.to_lowercase(), s_120);
                        } else {
                            log_line!(
                                "[infynitae] OUTPUT_{}_SCALE={s} outside accepted range (0.25..8.0), ignoring",
                                conn_name
                            );
                        }
                    }
                } else if key.starts_with("OUTPUT_") && key.ends_with("_TRANSFORM") {
                    let conn_name = key
                        .trim_start_matches("OUTPUT_")
                        .trim_end_matches("_TRANSFORM")
                        .replace('_', "-");
                    match parse_transform(val) {
                        Some(t) => {
                            custom_transforms.insert(conn_name.to_lowercase(), t);
                        }
                        None => log_line!(
                            "[infynitae] OUTPUT_{conn_name}_TRANSFORM={val}: not a rotation (normal, 90, 180, 270, flipped, flipped-90, flipped-180, flipped-270), ignoring"
                        ),
                    }
                } else if key.starts_with("OUTPUT_") && key.ends_with("_RATE") {
                    let conn_name = key
                        .trim_start_matches("OUTPUT_")
                        .trim_end_matches("_RATE")
                        .replace('_', "-");
                    if let Ok(r) = val.parse::<f64>() {
                        custom_rates.insert(conn_name.to_lowercase(), (r * 1000.0).round() as u32);
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

    // every connected connector at its preferred mode, each on a CRTC of its own (T5)
    let resources = drm.resource_handles()?;
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
        if output_layout_reverse {
            log_line!("[infynitae] OUTPUT_ORDER overrides OUTPUT_LAYOUT");
        }
        connected_connectors.sort_by_key(|(_, _, name, debug_name)| {
            output_order
                .iter()
                .position(|o| matches_connector_name(o, name, debug_name))
                .unwrap_or(usize::MAX)
        });
    } else if output_layout_reverse {
        connected_connectors.reverse();
    }

    let factory = ScreenFactory {
        gpu: gpu.clone(),
        gbm: gbm.clone(),
        allocator,
        exporter,
        render_formats,
        custom_modes,
        custom_rates,
        custom_scales,
        custom_transforms,
        scale_120,
        vrr: options.vrr,
    };
    for (handle, conn, _, _) in connected_connectors {
        if let Some(screen) = connect_screen(
            &factory,
            &mut drm,
            &mut state,
            &mut output_token,
            &gamma_screens,
            &screens,
            handle,
            &conn,
        )? {
            screens.push(screen);
        }
    }
    if screens.is_empty() {
        // a monitor that is off or asleep at boot can report no connection: wait for it (hotplug)
        log_line!("[infynitae] No connected screen yet: waiting for one");
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
    // the edge says what captures the screen from this file (CAPTURE_INDICATOR="edge")
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        state.capture_indicator().set_file(
            std::path::PathBuf::from(dir)
                .join("infynitae")
                .join("capturing"),
        );
    }
    // the desktop journal: what captured, read the clipboard, typed or locked, and who
    if let Some(dir) = infynitae_wayland::journal::default_dir()
        && let Err(e) = state.open_journal(&dir)
    {
        log_line!("[infynitae] Journal: cannot open {}: {e}", dir.display());
    }
    // the shortcut actions get this session's socket and X display too (INV-XWAYLAND-1)
    state.set_client_env(
        session.socket_name(),
        x_display.as_ref().map(|d| d.name()),
        x_display
            .as_ref()
            .and_then(|d| d.authority_path().map(|p| p.to_path_buf())),
    );
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
                log_line!("[infynitae] Session paused (VT switched away)");
                if !data.touch_slots.is_empty() {
                    data.touch_slots.clear();
                    core_touch(data, PhysicalEvent::TouchCancel);
                    data.state.cancel_touches();
                }
                data.libinput.suspend();
                data.drm.pause();
                data.active = false;
                // a frame held back would come due with nothing to draw it, and the loop would spin
                for s in &mut data.screens {
                    s.deferred = None;
                }
            }
            SeatEvent::ActivateSession => {
                log_line!("[infynitae] Session resumed");
                if data.libinput.resume().is_err() {
                    log_line!("[infynitae] libinput did not resume");
                }
                if let Err(e) = data.drm.activate(false) {
                    log_line!("[infynitae] DRM did not resume: {e}");
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
                // screens may have come or gone while away
                data.rescan_at = Some(std::time::Instant::now());
                data.session.request_frame();
            }
        })
        .map_err(|e| format!("seat events: {e}"))?;
    let (logind_inhibit, inhibit_channel) =
        infynitae_wayland::idle_inhibit::LogindInhibitWatcher::new();
    handle
        .insert_source(inhibit_channel, |event, _, data| {
            if let smithay::reexports::calloop::channel::Event::Msg(inhibited) = event {
                data.logind_inhibit.update(inhibited);
            }
        })
        .map_err(|e| format!("idle inhibit channel: {e}"))?;
    // the capture copy thread wakes the loop to hand its captures over
    let (copy_ping, copy_wake) = smithay::reexports::calloop::ping::make_ping()
        .map_err(|e| format!("capture copy wake: {e}"))?;
    handle
        .insert_source(copy_wake, |_, _, _| {})
        .map_err(|e| format!("capture copy wake: {e}"))?;
    let capture_worker =
        CopyWorker::new(copy_ping).map_err(|e| format!("capture copy thread: {e}"))?;
    handle
        .insert_source(drm_notifier, |event, meta, data| match event {
            DrmEvent::VBlank(crtc) => {
                if let Some(s) = data.screens.iter_mut().find(|s| s.crtc == crtc) {
                    let _ = s.compositor.frame_submitted();
                    s.pending = false;
                    // the refresh's own time (the kernel's), else now
                    let shown = match meta.as_ref().map(|m| m.time) {
                        Some(smithay::backend::drm::DrmEventTime::Monotonic(t)) => t,
                        _ => infynitae_wayland::presentation_clock_now(),
                    };
                    s.schedule.vblank(shown);
                    s.latency.shown(shown);
                    data.session.frame_refreshed();
                    s.empty_since_refresh = false;
                    let id = s.id;
                    if data.state.gpu_relief {
                        let hz = data
                            .state
                            .core()
                            .output(id)
                            .map_or(60, |m| u64::from(m.refresh_mhz / 1000));
                        let (on, changed) = s.relief.step(s.schedule.counts(), hz);
                        s.relief_on = on;
                        if changed {
                            data.state.core_mut().set_frame_relief(id, on);
                            log_line!(
                                "[infynitae] GPU relief on screen {id:?}: {}",
                                if on {
                                    "refreshes missed, windows out of use slow down"
                                } else {
                                    "over"
                                }
                            );
                        }
                    } else {
                        s.relief_on = false;
                        data.state.core_mut().set_frame_relief(id, false);
                    }
                    if !data.dpms_active {
                        s.shows_locked = s.queued_locked;
                    }
                    let id = s.id;
                    // a window the camera fits to the screen is spared by relief like a fullscreen one
                    let fitted = data.state.fitted_window(id);
                    data.state.core_mut().set_relief_spared(id, fitted);
                    data.state.frame_tick(id, shown);
                    confirm_lock(data);
                    data.session.request_frame();
                }
            }
            DrmEvent::Error(e) => log_line!("[infynitae] DRM error: {e}"),
        })
        .map_err(|e| format!("DRM events: {e}"))?;
    handle
        .insert_source(
            Generic::new(libinput.clone(), Interest::READ, Mode::Level),
            |_, _, data| {
                // input runs inside the loop's wait, which the turn's clock leaves out: timed here
                // so a device that is slow to add shows as a slow turn too
                let began = std::time::Instant::now();
                let mut turn = if data.state.latency_stats {
                    TurnClock::new(began)
                } else {
                    TurnClock::off()
                };
                // libinput opens a new device and reads its udev properties in here
                let _ = data.libinput.dispatch();
                turn.mark("libinput");
                while let Some(event) = data.libinput.next() {
                    let added = matches!(
                        event,
                        smithay::reexports::input::Event::Device(
                            smithay::reexports::input::event::DeviceEvent::Added(_)
                        )
                    );
                    dispatch_libinput_event(data, event);
                    turn.mark(if added {
                        "device setup"
                    } else {
                        "input events"
                    });
                }
                if turn.is_enabled() {
                    let now = std::time::Instant::now();
                    let threshold = slow_turn_threshold(&data.screens);
                    let busy = now.saturating_duration_since(began);
                    data.loop_busy_max = data.loop_busy_max.max(busy);
                    if let Some(line) = data.slow_turn.record_at(&turn, busy, threshold, now) {
                        log_line!("{line}");
                    }
                }
                data.session.request_frame();
                Ok(PostAction::Continue)
            },
        )
        .map_err(|e| format!("input events: {e}"))?;
    // hotplug: the GPU's udev "change" events (a monitor plugged, unplugged, woken)
    let gpu_dev = drm.device_id();
    match smithay::backend::udev::UdevBackend::new(&seat_name) {
        Ok(udev) => {
            handle
                .insert_source(udev, move |event, _, data| {
                    if let smithay::backend::udev::UdevEvent::Changed { device_id } = event
                        && device_id == gpu_dev
                    {
                        data.rescan_at = Some(std::time::Instant::now() + HOTPLUG_SETTLE);
                    }
                })
                .map_err(|e| format!("udev events: {e}"))?;
        }
        Err(e) => log_line!("[infynitae] No udev monitor, screens are not hotplugged: {e}"),
    }

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
        capture_worker,
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
        held_by_device: Vec::new(),
        keyboards: Vec::new(),
        input_devices: Vec::new(),
        touch_slots: Vec::new(),
        last_abs_pointer: None,
        known_mapped: HashSet::new(),
        last_mapped_generation: u64::MAX,
        loop_busy_max: Duration::ZERO,
        direct_scanout: options.direct_scanout.unwrap_or(true),
        zoom_programs,
        zoom_cache: smooth::ZoomCache::default(),
        scaling_filter: filter,
        renderer_recovered_at: None,
        config_path: cfg_path,
        last_config_check: std::time::Instant::now(),
        last_config_mtime: initial_mtime,
        last_user_activity: std::time::Instant::now(),
        latency_logged: None,
        slow_turn: SlowTurnLog::default(),
        last_turn_slow: false,
        capture_log: CaptureSeen::default(),
        dpms_active: false,
        idle_sleep_timeout_min: idle_timeout_min,
        idle_auto_lock,
        lock_process: None,
        logind_inhibit,
        vrr_mode: options.vrr,
        loop_handle: handle.clone(),
        factory,
        rescan_at: None,
        lid_closed: false,
        capture_copies: Vec::new(),
        capture_readback_deferred: Vec::new(),
        virtual_screen_enabled: options.virtual_screen,
        virtual_screen: None,
    };
    data.state.corners_drawn = corners_ready;
    update_virtual_screen(&mut data);

    data.state.reload_config();
    sync_scaling_filter(&mut data);
    if let Some(ds) = data.state.direct_scanout {
        data.direct_scanout = ds;
    }

    data.state.enable_frame_waits();
    let device = data.drm.device_fd().clone();
    data.state.enable_explicit_sync(&device);
    if data.state.latency_stats {
        scanout_log::install();
    }

    log_line!("[infynitae] Compositor running. Ctrl+Alt+Backspace quits.");
    data.session.follow_display();
    let mut turn_began = std::time::Instant::now();
    let mut turn_started = data.state.latency_stats.then_some(turn_began);
    let mut turn = turn_started.map_or_else(TurnClock::off, TurnClock::new);
    while !data.quit && !data.session.finished() {
        data.session
            .wake_at(data.state.core().next_settlement_deadline());
        let restart = xwayland
            .as_mut()
            .and_then(|x| x.poll(std::time::Instant::now()));
        let has_screencopy = data
            .screens
            .iter()
            .map(|s| s.id)
            .chain(data.virtual_screen.as_ref().map(|v| v.id))
            .any(|id| data.state.screencopy.has_pending(id));
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
        let query_due = logind_query_remaining(
            data.idle_sleep_timeout_min,
            data.dpms_active,
            data.state.is_idle_inhibited(),
            data.last_user_activity,
            std::time::Instant::now(),
        );
        if query_due == Some(Duration::ZERO) {
            data.logind_inhibit.query_async(std::time::Instant::now());
        }
        let capture_mark = data
            .state
            .capture_indicator()
            .due(std::time::Instant::now());
        let rescan = data
            .rescan_at
            .map(|t| t.saturating_duration_since(std::time::Instant::now()));
        let background = data.state.background().due(std::time::Instant::now());
        let timeout = [
            data.session.timeout(),
            restart,
            idle_timeout,
            query_due.filter(|d| !d.is_zero()),
            capture_mark,
            rescan,
            background,
        ]
        .into_iter()
        .flatten()
        .min();
        // time spent in the loop's own turn, every wait left out: a stall of this process alone,
        // where "refresh to submit" also counts a quiet screen
        let now = std::time::Instant::now();
        let busy = now.saturating_duration_since(turn_began);
        let threshold = slow_turn_threshold(&data.screens);
        data.last_turn_slow = busy >= threshold;
        if turn_started.is_some() {
            data.loop_busy_max = data.loop_busy_max.max(busy);
            turn.surface = data.state.take_slowest_surface();
            if let Some(line) = data.slow_turn.record_at(&turn, busy, threshold, now) {
                log_line!("{line}");
            }
        }
        let slow_flush = data.slow_turn.remaining(std::time::Instant::now());
        let timeout = [timeout, slow_flush].into_iter().flatten().min();
        event_loop.dispatch(timeout, &mut data)?;
        turn_began = std::time::Instant::now();
        turn_started = data.state.latency_stats.then_some(turn_began);
        turn = turn_started.map_or_else(TurnClock::off, TurnClock::new);
        if data.active
            && data
                .rescan_at
                .is_some_and(|t| t <= std::time::Instant::now())
        {
            data.rescan_at = None;
            rescan_screens(&mut data);
            data.session.request_frame();
        }
        turn.mark("screen rescan");
        // apps out of sight yield the CPU; game mode ends with the game's fullscreen
        data.state.background_update(std::time::Instant::now());
        turn.mark("background priority");
        // the capture indicator came or went: every screen draws it, or no longer
        if data
            .state
            .capture_indicator()
            .update(std::time::Instant::now())
        {
            data.session.request_frame();
        }
        // hidden by a fullscreen window, a capture is announced instead
        if data.state.capture_notice(std::time::Instant::now()) {
            data.state.announce_hidden_capture();
        }
        turn.mark("capture indicator");
        // Watches ~/.config/infynitae/config for changes and reloads settings
        if data.last_config_check.elapsed() >= Duration::from_millis(250) {
            data.last_config_check = std::time::Instant::now();
            if let Some(path) = &data.config_path
                && let Ok(meta) = std::fs::metadata(path)
                && let Ok(mtime) = meta.modified()
            {
                if data.last_config_mtime.is_some() && data.last_config_mtime != Some(mtime) {
                    data.last_config_mtime = Some(mtime);
                    log_line!(
                        "[infynitae] Config change detected (~/.config/infynitae/config). Live reloading..."
                    );
                    data.state.reload_config();
                    turn.mark("config reload");
                    data.state.config_changed();
                    turn.mark("config-check");
                    if let Some(ds) = data.state.direct_scanout {
                        data.direct_scanout = ds;
                    }
                    if let Ok(content) = std::fs::read_to_string(path) {
                        data.idle_auto_lock = parse_lock_when_away(&content);
                        for (k, v) in content.lines().filter_map(infynitae_wayland::config_line) {
                            let key = k.trim();
                            let val = v.trim();
                            if key == "IDLE_SLEEP_TIMEOUT_MIN"
                                && let Ok(m) = val.parse::<u32>()
                            {
                                data.idle_sleep_timeout_min = m;
                            }
                        }
                    }
                    let input_cfg = data.state.input_config().clone();
                    for dev in &mut data.input_devices {
                        configure_input_device(dev, &input_cfg);
                    }
                    sync_scaling_filter(&mut data);
                    data.session.request_frame();
                    turn.mark("config second read");
                } else if data.last_config_mtime.is_none() {
                    data.last_config_mtime = Some(mtime);
                }
            }
            turn.mark("config watch");

            // Idle sleep and auto-lock check (skipped when idle is inhibited)
            if !data.dpms_active
                && data.idle_sleep_timeout_min > 0
                && !data.state.is_idle_inhibited()
                && data.last_user_activity.elapsed()
                    >= Duration::from_secs((data.idle_sleep_timeout_min as u64) * 60)
                && !logind_inhibits_idle_now(&mut data)
            {
                log_line!(
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
                // `clear` waits for the kernel to turn each screen off (a blocking modeset)
                turn.mark("screen off");
                if data.idle_auto_lock {
                    // an error here is a child someone else reaped: it is not running, or the screen
                    // would never lock again after the first failure
                    let is_running = data
                        .lock_process
                        .as_mut()
                        .is_some_and(|p| matches!(p.try_wait(), Ok(None)));
                    if !is_running && !data.state.is_locked() {
                        log_line!("[infynitae] Auto-locking session with infynitae-lock...");
                        // locked first, by the compositor itself: if the locker fails to start or
                        // never takes the lock, waking the screen shows black, not the desktop, and
                        // the locker is started again (`restart_dead_locker`)
                        data.state.start_locked();
                        // this process's own environment names no display: without this the lock
                        // client could not connect, and the screen blanked unlocked (2026-11-04)
                        let display = data.session.socket_name().to_string();
                        let child = std::process::Command::new("infynitae-lock")
                            .env("WAYLAND_DISPLAY", &display)
                            .spawn()
                            .or_else(|_| {
                                if let Ok(home) = std::env::var("HOME") {
                                    std::process::Command::new(format!(
                                        "{home}/.local/bin/infynitae-lock"
                                    ))
                                    .env("WAYLAND_DISPLAY", &display)
                                    .spawn()
                                } else {
                                    Err(std::io::Error::new(
                                        std::io::ErrorKind::NotFound,
                                        "not found",
                                    ))
                                }
                            });
                        match child {
                            Ok(c) => data.lock_process = Some(c),
                            Err(e) => log_line!("[infynitae] Auto-lock could not start: {e}"),
                        }
                    }
                    turn.mark("auto-lock");
                }
            }
        }
        turn.mark("idle check");
        if let Some(ext) = extension.as_mut() {
            ext.set_locked(data.state.is_locked());
            if let Some(entries) = data.state.clipboard_listing() {
                ext.set_clipboard(&entries);
            }
            let (fx, changed) = ext.service(data.state.core_mut());
            data.session.set_extension_wake(ext.wake_at());
            if ext.take_lock_request() {
                data.state.lock_now();
            }
            for ask in ext.take_desk_asks() {
                data.state.desk_ask(ask);
            }
            data.state.apply(&fx);
            if changed {
                data.session.request_frame();
            }
        }
        turn.mark("shell socket");
        // commits timed for now apply, whoever holds them (a hidden window, a screen not drawing)
        if data
            .state
            .apply_due_timed_commits(infynitae_wayland::presentation_clock_now())
        {
            data.session.request_frame();
        }
        data.session.dispatch(&mut data.state);
        turn.mark("client requests");
        // a new buffer is what a rest must not hold back: it goes to the very next refresh
        if data.state.take_new_buffer() {
            data.session.frame_wake();
        }
        watch_frame_waits(&mut data);
        for c in data.state.take_output_configurations() {
            let ok = apply_output_configuration(&mut data, &c.changes);
            data.state.finish_output_configuration(c, ok);
        }
        if let Some(change) = data.state.take_display_change() {
            apply_display_change(&mut data, &change);
        }
        apply_virtual_input(&mut data);
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

        turn.mark("outputs, input, unlock");
        let current_generation = data.state.core().stage().map_generation();
        let new_mapped = if current_generation != data.last_mapped_generation {
            data.last_mapped_generation = current_generation;
            let current_mapped = data.state.core().stage().mapped_elements();
            let new_mapped: Vec<_> = current_mapped
                .iter()
                .copied()
                .filter(|id| !data.known_mapped.contains(id))
                .collect();

            data.known_mapped.clear();
            data.known_mapped.extend(current_mapped);
            new_mapped
        } else {
            Vec::new()
        };
        // which new windows the user opened, decided before any of them takes the focus
        let (focus_user_only, teleport_user_only) = data.state.open_policy_user_only();
        let priority = data.state.open_policy_priority();
        let by_user: Vec<bool> = new_mapped
            .iter()
            .map(|&id| {
                (focus_user_only || teleport_user_only || priority)
                    && data.state.opened_by_user(id, data.last_user_activity)
            })
            .collect();
        // FOCUS_ON_OPEN=priority: the windows that may not take the focus now, decided against
        // the focus as it is before any of them takes it
        let held: Vec<bool> = new_mapped
            .iter()
            .zip(&by_user)
            .map(|(&id, &user)| priority && data.state.open_focus_held(id, user))
            .collect();

        if data.state.core().focus().is_initial_eligible() {
            if let Some(topmost) = data.state.core().stage().top_mapped()
                && let Ok(effects) = data
                    .state
                    .core_mut()
                    .grant_initial_focus(&mut data.input_port, topmost)
            {
                data.state.apply(&effects);
                data.session.flush();
            }
        } else if data.state.core().focus_on_open() {
            for ((&new_id, &user), &hold) in new_mapped.iter().zip(&by_user).zip(&held) {
                // a window the rules keep from the focus (one the user did not open, or one held
                // while the focus is busy) opens without it and asks for attention: Super+Tab
                // goes to it first, the edge marks it (owner, 2026-10-31). Never through an
                // activation, which FOCUS_ON_ACTIVATE="always" would turn into a focus.
                if (focus_user_only && !user) || hold {
                    data.state.core_mut().opened_without_focus(new_id);
                    continue;
                }
                // one the user opened, or a dialog, comes out over a fullscreen window
                let same_client = data
                    .state
                    .core()
                    .focus()
                    .target()
                    .is_some_and(|f| data.state.started_by_app_of(new_id, f));
                let shown =
                    user || data.state.core().toplevel_parent(new_id).is_some() || same_client;
                let core = data.state.core_mut();
                let result = if shown {
                    core.show_opened(&mut data.input_port, new_id)
                } else {
                    core.set_focus(&mut data.input_port, Some(new_id))
                };
                if let Ok(effects) = result {
                    data.state.apply(&effects);
                    data.session.flush();
                }
            }
        }

        let teleport = data.state.core().teleport_on_open();
        for ((&new_id, &user), &hold) in new_mapped.iter().zip(&by_user).zip(&held) {
            // a held window moves nothing: it waits out of the way
            if hold {
                continue;
            }
            let port = &mut data.input_port;
            // a child window opens over its parent, already in view: the camera stays. A window
            // the camera does not come to is brought into the view when it is mostly outside it
            // (owner, 2026-11-09: a folder picker an app opened 1,620 px under the view)
            if teleport
                && !(teleport_user_only && !user)
                && data.state.core().toplevel_parent(new_id).is_none()
            {
                let _ = data
                    .state
                    .with_effects(|core, fx| core.recenter_element(port, new_id, fx));
            } else {
                let _ = data
                    .state
                    .with_effects(|core, fx| core.bring_into_view(port, new_id, fx));
            }
            data.session.request_frame();
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

        turn.mark("new windows");
        if !data.capture_copies.is_empty() && data.active {
            let mut woken = Vec::new();
            deliver_copies(
                &mut data.renderer,
                &mut data.capture_copies,
                &mut data.capture_worker,
                &mut woken,
            );
            for id in woken {
                if let Some(pos) = data
                    .capture_readback_deferred
                    .iter()
                    .position(|&deferred_id| deferred_id == id)
                {
                    data.capture_readback_deferred.swap_remove(pos);
                    data.state.screencopy.wake_waiting(id);
                }
            }
            turn.mark("capture copies");
            data.session.flush();
        }
        let has_screencopy = data
            .screens
            .iter()
            .map(|s| s.id)
            .chain(data.virtual_screen.as_ref().map(|v| v.id))
            .any(|id| data.state.screencopy.has_pending(id));
        // a frame held back for its start time (LATENCY) is drawn when it comes
        let now = infynitae_wayland::presentation_clock_now();
        let deferred_due = data
            .screens
            .iter()
            .any(|s| s.deferred.is_some_and(|t| t <= now));
        // a zoom glide draws on every refresh until it rests
        if data.state.core().zoom_gliding() {
            data.session.request_frame();
        }
        // a commit timed for a coming refresh goes into the frame started in the refresh before it
        if data.screens.iter().any(|s| {
            data.state
                .next_timed_commit(Some(s.id))
                .is_some_and(|t| s.schedule.start_to_show_at(t) <= now)
        }) {
            data.session.request_frame();
        }
        if (data.session.frame_due() || has_screencopy || deferred_due) && data.active {
            // what this turn's input made goes to the clients first: a game reads its mouse motion
            // now, not after the compositor's own drawing (review 2026-11-12)
            data.session.flush();
            turn.mark("flush");
            render(&mut data, &mut turn);
        }
        let now = infynitae_wayland::presentation_clock_now();
        let next_deferred = data.screens.iter().filter_map(|s| s.deferred).min();
        // a timed commit: the start of the frame it goes into, and its own time (applied then
        // whatever the screens do)
        let next_timed = data
            .screens
            .iter()
            .filter_map(|s| {
                let t = data.state.next_timed_commit(Some(s.id))?;
                Some(s.schedule.start_to_show_at(t)).filter(|start| *start > now)
            })
            .chain(data.state.next_timed_commit(None))
            .min();
        let deadline = match (next_deferred, next_timed) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        data.session.set_render_deadline(deadline, now);
        latency_summary(&mut data);
        turn.mark("latency summary");

        let now = infynitae_wayland::presentation_clock_now();
        let deadline = data.state.frame_fallback(now);
        data.session.set_fallback_deadline(deadline, now);
        turn.mark("frame fallback");
        let idle_deadline = data.state.idle_notify_check(now);
        data.session.set_idle_deadline(idle_deadline, now);
        turn.mark("idle notify");
        let asked = data.state.not_responding_check(now);
        data.session.set_not_responding_deadline(asked, now);
        turn.mark("not responding");
        let anim_due = data.state.cursor_animation_due(now);
        data.session.set_cursor_animation_due(anim_due);
        turn.mark("cursor animation");
        data.session.flush();
        turn.mark("flush");
    }

    if let Some(line) = data.slow_turn.flush() {
        log_line!("{line}");
    }
    log_line!("[infynitae] Shutting down compositor...");
    data.state.background_restore_all();
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
    log_line!("[infynitae] Compositor exited.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On the GPU (the first render node; skipped without one): the console renderer's context
    /// is made to be lost, not aborted, on a GPU reset (the display offers the extension), and a
    /// healthy one reports no reset after drawing.
    #[test]
    fn the_console_renderer_survives_a_gpu_reset_and_a_healthy_one_is_not_lost() {
        use smithay::backend::allocator::Fourcc;
        use smithay::backend::allocator::gbm::GbmDevice;
        use smithay::backend::renderer::gles::GlesRenderbuffer;
        use smithay::backend::renderer::{Bind, Frame, Offscreen, Renderer};
        use smithay::utils::DeviceFd;

        let Some(mut renderer) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/dri/renderD128")
            .ok()
            .and_then(|f| {
                GbmDevice::new(DrmDeviceFd::new(DeviceFd::from(
                    std::os::fd::OwnedFd::from(f),
                )))
                .ok()
            })
            .and_then(|gbm| gles_renderer(&gbm).ok())
        else {
            eprintln!("no GPU render node: skipped");
            return;
        };
        assert!(
            renderer
                .egl_context()
                .display()
                .extensions()
                .iter()
                .any(|e| e == "EGL_EXT_create_context_robustness"),
            "the display can make a context that is lost on reset"
        );
        let mut buffer: GlesRenderbuffer =
            Offscreen::create_buffer(&mut renderer, Fourcc::Abgr8888, (16, 16).into())
                .expect("buffer");
        {
            let mut target = renderer.bind(&mut buffer).expect("bind");
            let mut frame = renderer
                .render(
                    &mut target,
                    (16, 16).into(),
                    smithay::utils::Transform::Normal,
                )
                .expect("frame");
            let all = smithay::utils::Rectangle::from_size((16, 16).into());
            frame
                .clear([0.0, 0.0, 0.0, 1.0].into(), &[all])
                .expect("clear");
            frame.finish().expect("finish").wait().expect("wait");
        }
        assert!(
            !renderer_lost(&mut renderer),
            "a healthy context is not lost"
        );
    }

    /// T6: Ctrl+Alt+Backspace quits and Ctrl+Alt+F1…F12 switch VT, with both modifiers held; any
    /// other key, or a missing modifier, is the session's.
    #[test]
    fn panel_orientation_maps_kernel_values_and_lets_custom_override() {
        assert_eq!(
            panel_orientation_transform("Normal"),
            Some(Transform::Normal)
        );
        assert_eq!(
            panel_orientation_transform("Upside Down"),
            Some(Transform::R180)
        );
        assert_eq!(
            panel_orientation_transform("Left Side Up"),
            Some(Transform::R90)
        );
        assert_eq!(
            panel_orientation_transform("Right Side Up"),
            Some(Transform::R270)
        );
        assert_eq!(panel_orientation_transform("Unknown"), None);
        assert_eq!(panel_orientation_transform(""), None);

        assert_eq!(
            resolve_output_transform(None, Some("Right Side Up")),
            Transform::R270
        );
        assert_eq!(
            resolve_output_transform(None, Some("Left Side Up")),
            Transform::R90
        );
        assert_eq!(
            resolve_output_transform(None, Some("Upside Down")),
            Transform::R180
        );
        assert_eq!(
            resolve_output_transform(None, Some("Normal")),
            Transform::Normal
        );
        assert_eq!(
            resolve_output_transform(None, Some("Unknown")),
            Transform::Normal
        );
        assert_eq!(resolve_output_transform(None, None), Transform::Normal);
        assert_eq!(
            resolve_output_transform(Some(Transform::Normal), Some("Right Side Up")),
            Transform::Normal
        );
        assert_eq!(
            resolve_output_transform(Some(Transform::R90), Some("Right Side Up")),
            Transform::R90
        );
    }

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
    fn logind_query_remaining_schedules_lead_and_respects_inhibitions() {
        let start = std::time::Instant::now();
        // 10 minutes timeout (600 s): query is due 5 s before timeout (595 s)
        let due = logind_query_remaining(10, false, false, start, start);
        assert_eq!(due, Some(Duration::from_secs(595)));

        // At 595 s elapsed: query is due immediately (0 remaining)
        let due = logind_query_remaining(10, false, false, start, start + Duration::from_secs(595));
        assert_eq!(due, Some(Duration::ZERO));

        // When DPMS is already active -> None
        assert_eq!(logind_query_remaining(10, true, false, start, start), None);

        // When idle is inhibited -> None
        assert_eq!(logind_query_remaining(10, false, true, start, start), None);

        // When timeout is 0 (disabled) -> None
        assert_eq!(logind_query_remaining(0, false, false, start, start), None);
    }

    #[test]
    fn logind_inhibits_idle_decision_runs_no_process() {
        let (mut watcher, _rx) = infynitae_wayland::idle_inhibit::LogindInhibitWatcher::new();
        let mut activity = std::time::Instant::now();
        let t0 = activity;

        // Not inhibited: returns false, activity timestamp unchanged
        assert!(!logind_inhibits_idle_decision(&watcher, 10, &mut activity));
        assert_eq!(activity, t0);

        // Inhibited: returns true, adjusts activity so timeout rechecks in 60 s
        watcher.update(true);
        assert!(logind_inhibits_idle_decision(&watcher, 10, &mut activity));
        assert!(activity < t0, "activity pushed back for recheck");

        // Evaluates in-memory without spawning any process on the main loop
        assert!(t0.elapsed() < Duration::from_millis(5));
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
