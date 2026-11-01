//! Infynitae Native Smithay adapter (FOUNDATION_ARCHITECTURE C1.2).
//!
//! Bridges Wayland protocols (compositor, xdg-shell, layer-shell, outputs, seat, presentation-time,
//! fractional scale, activation, and data control) to Core. Surface commits extract snapshots for
//! Core, Core effects drive protocol responses in order, and render elements from Core render lists
//! are presented through damage trackers.
//!
//! Every Smithay handler receives `&mut` protocol state. That state holds the Core but never an
//! authority port or the output-lifecycle token: those types are `!Send` and the state is asserted
//! `Send` below, so storing one here is a compile error (C7.4, E0277). tools/verify-architecture
//! also rejects any mention of the authority entry points in this crate's sources.

#![deny(unsafe_code)]

pub mod background;
pub mod capture_indicator;
pub mod convert;
pub mod cursor;
pub mod extension;
pub mod frame_schedule;
mod frames;
pub mod gamma;
mod handlers;
pub mod idle_inhibit;
pub mod idle_notify;
mod image_copy;
mod input_method;
mod keyboard;
mod launch;
mod layers;
pub mod limits;
mod lock;
pub mod output_management;
pub mod output_power;
mod outputs;
pub mod ownership;
mod pointer;
pub mod registry;
pub mod screencopy;
pub mod session;
mod shortcuts_inhibit;
mod signals;
mod snapshot;
pub mod tearing;
pub mod trust;
pub mod virtual_input;
mod wlr_toplevels;
mod x11_keys;
pub mod xwayland;

pub use handlers::ACTIVATION_TOKENS_KEPT;
pub use keyboard::PreemptionStrategy;

use infynitae_core::Core;
use infynitae_core::stage::ElementId;
use smithay::desktop::{PopupManager, Window};
use smithay::input::{Seat, SeatState};
use smithay::reexports::wayland_server::backend::{
    ClientData, ClientId, DisconnectReason, GlobalId, ObjectId,
};
use smithay::reexports::wayland_server::{DisplayHandle, Resource};
use smithay::wayland::compositor::{CompositorClientState, CompositorState};
use smithay::wayland::dmabuf::{DmabufFeedbackBuilder, DmabufState};
use smithay::wayland::fractional_scale::FractionalScaleManagerState;
use smithay::wayland::output::OutputManagerState;
use smithay::wayland::pointer_constraints::PointerConstraintsState;
use smithay::wayland::presentation::PresentationState;
use smithay::wayland::relative_pointer::RelativePointerManagerState;
use smithay::wayland::selection::data_device::DataDeviceState;
use smithay::wayland::selection::primary_selection::PrimarySelectionState;
use smithay::wayland::shell::wlr_layer::WlrLayerShellState;
use smithay::wayland::shell::xdg::XdgShellState;
use smithay::wayland::shm::ShmState;
use smithay::wayland::viewporter::ViewporterState;
use smithay::wayland::xdg_activation::XdgActivationState;

/// `clk_id` announced by `wp_presentation`: CLOCK_MONOTONIC on Linux. Frame times handed to
/// `complete_frames` are on this clock.
const CLOCK_MONOTONIC: u32 = 1;

/// The current time on the clock `wp_presentation` announces. A backend that presents in real
/// time takes the frame time it hands to `complete_frames` from here and from nowhere else, so
/// the timestamps clients receive are on the clock they were told (a lockstep test supplies
/// virtual frame times instead).
pub fn presentation_clock_now() -> std::time::Duration {
    smithay::utils::Clock::<smithay::utils::Monotonic>::new()
        .now()
        .into()
}

pub use idle_notify::IdleNotifyState;

/// MAX_APP_SCALE when the config has none (owner, 2026-10-31): zooming only scales what apps
/// drew; they never re-render for it (a HiDPI screen still gets its own scale). Re-rendering
/// made a terminal re-flow its text as you zoomed.
pub const DEFAULT_MAX_APP_SCALE: f64 = 1.0;
pub use registry::SurfaceRegistry;

/// Per-client data the composition root attaches when inserting a client.
#[derive(Debug, Default)]
pub struct ClientState {
    compositor: CompositorClientState,
    /// Whether the client is xwayland-satellite (its surfaces are X11 windows), once asked.
    x11: std::sync::OnceLock<bool>,
    /// Who the client is, for the privileged globals (`trust`). Unset: the compositor's own
    /// connection (an internal socket pair, a test), which is trusted.
    identity: std::sync::OnceLock<trust::Identity>,
    /// Connected through the recorder's own socket (`session::recorder_socket_path`, M2): the
    /// replay buffer Infynitae started, left out of the capture mark.
    recorder: bool,
}

impl ClientState {
    /// A client whose identity is known when it connects (a sandbox's listener).
    pub fn with_identity(identity: trust::Identity) -> ClientState {
        let state = ClientState::default();
        let _ = state.identity.set(identity);
        state
    }

    /// Sets the identity of a client that connected to the ordinary socket (once; before its
    /// first request is read).
    pub fn identify(&self, identity: trust::Identity) {
        let _ = self.identity.set(identity);
    }

    /// A client of the recorder's own socket.
    pub fn recorder() -> ClientState {
        ClientState {
            recorder: true,
            ..ClientState::default()
        }
    }

    pub fn is_recorder(&self) -> bool {
        self.recorder
    }

    pub fn identity(&self) -> Option<&trust::Identity> {
        self.identity.get()
    }
}

impl ClientData for ClientState {
    fn initialized(&self, _: ClientId) {}
    fn disconnected(&self, _: ClientId, _: DisconnectReason) {}
}

/// An element's protocol handle (C2.2: handles stay in the adapter).
struct ElementHandle {
    id: ElementId,
    window: Window,
}

/// Configuration for input devices (pointers, mice, touchpads) loaded from ~/.config/infynitae/config.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InputDeviceConfig {
    pub mouse_accel_profile: Option<String>,
    pub mouse_accel_speed: Option<f64>,
    pub touchpad_tap: Option<bool>,
    pub touchpad_natural_scroll: Option<bool>,
}

/// Protocol state handed to every Smithay handler.
pub struct WaylandState {
    core: Core,
    dh: DisplayHandle,
    compositor: CompositorState,
    #[expect(
        dead_code,
        reason = "owns the xdg-output manager global for the state's lifetime"
    )]
    output_manager: OutputManagerState,
    #[expect(
        dead_code,
        reason = "owns the wp_presentation global for the state's lifetime"
    )]
    presentation: PresentationState,
    outputs: Vec<outputs::OutputHandle>,
    pub(crate) disabled_outputs: Vec<(GlobalId, std::time::Instant)>,
    /// The compositor started a process of its own (a shortcut, a launch, the wallpaper): the
    /// session reaps exits from then on (SIGCHLD), also without an `-e` command.
    pub(crate) spawned_children: bool,
    /// What X11 apps hear while a Wayland window has focus (XWAYLAND_KEYS, `x11_keys`).
    pub(crate) x11_keys: x11_keys::X11Keys,
    /// What the backend's framebuffer is (`set_host_window_target`).
    pub(crate) target: convert::RenderTarget,
    /// The cursor clients asked for, drawn when the target draws one (`cursor`).
    cursor: smithay::input::pointer::CursorImageStatus,
    /// When the current cursor shape was set (for animated cursor pacing).
    cursor_epoch: std::time::Duration,
    /// The focus (surface and origin) and position of the last `wl_pointer.motion` delivered,
    /// and whether pointer events wait for their `wl_pointer.frame` (`pointer`).
    pointer_sent: pointer::Sent,
    /// Core's `PointerMotion` effects applied, counted before a repeat is dropped (for tests).
    pointer_motions: u64,
    arrow: cursor::Arrow,
    capture_indicator: capture_indicator::CaptureIndicator,
    /// Apps out of sight yield the CPU; the game gets game mode (`background`).
    background: background::Background,
    pub(crate) cursor_theme: std::sync::Mutex<Option<cursor::CursorThemeManager>>,
    pub(crate) config_home: Option<std::path::PathBuf>,
    pub(crate) xcursor_path: Option<String>,
    xdg_shell: XdgShellState,
    shm: ShmState,
    popups: PopupManager,
    /// The number of xdg popups created so far: each popup's creation number orders the
    /// popups of its toplevel in the snapshot (xdg-shell stacking; see `snapshot`).
    popups_created: u64,
    seats: SeatState<WaylandState>,
    /// The single seat, with pointer and keyboard.
    seat: Seat<WaylandState>,
    pub(crate) popup_grab: Option<smithay::desktop::PopupGrab<WaylandState>>,
    pub(crate) nested_grab_keep_pointer: bool,
    pub(crate) ordered_preemption_strategy: keyboard::PreemptionStrategy,
    #[expect(
        dead_code,
        reason = "owns the zwp_pointer_constraints_v1 global for the state's lifetime"
    )]
    pointer_constraints: PointerConstraintsState,
    #[expect(
        dead_code,
        reason = "owns the zwp_relative_pointer_manager_v1 global for the state's lifetime"
    )]
    relative_pointer: RelativePointerManagerState,
    #[expect(
        dead_code,
        reason = "owns the wp_fractional_scale_manager_v1 global for the state's lifetime"
    )]
    fractional_scale_manager: FractionalScaleManagerState,
    #[expect(
        dead_code,
        reason = "owns the wp_viewporter global for the state's lifetime"
    )]
    viewporter: ViewporterState,
    /// `wp_cursor_shape_v1`: a client names a cursor shape; the compositor draws it from the
    /// user's cursor theme (`cursor`), the same as a named cursor.
    #[expect(
        dead_code,
        reason = "owns the wp_cursor_shape_manager_v1 global for the state's lifetime"
    )]
    cursor_shape: smithay::wayland::cursor_shape::CursorShapeManagerState,
    /// `zxdg_exporter_v2` / `zxdg_importer_v2`: a portal dialog (file chooser, screen-share
    /// picker) takes another client's window as its parent, as `xdg_toplevel.set_parent` does.
    xdg_foreign: smithay::wayland::xdg_foreign::XdgForeignState,
    /// `wp_single_pixel_buffer_manager_v1`: one-colour buffers (scaled by a viewport), drawn as
    /// solid colour by every renderer here.
    #[expect(
        dead_code,
        reason = "owns the wp_single_pixel_buffer_manager_v1 global for the state's lifetime"
    )]
    single_pixel_buffer: smithay::wayland::single_pixel_buffer::SinglePixelBufferState,
    /// `ext_foreign_toplevel_list_v1`: the window list for bars and task switchers. A window is
    /// listed while it is mapped; Smithay keeps only weak handles, so the strong ones are here.
    foreign_toplevel_list: smithay::wayland::foreign_toplevel_list::ForeignToplevelListState,
    /// Apps keeping the screens on (`idle_inhibit`).
    idle_inhibit: idle_inhibit::IdleInhibit,
    /// Fullscreen games' frames shown at once (`tearing`).
    tearing: tearing::Tearing,
    /// Each finger's last location sent (`touch`: a cancel needs it).
    touch_last: Vec<(u32, infynitae_core::geom::CanvasPos)>,
    foreign_toplevels: std::collections::HashMap<
        ElementId,
        smithay::wayland::foreign_toplevel_list::ForeignToplevelHandle,
    >,
    /// Input methods, text input and virtual keyboards (`input_method`).
    pub(crate) input_method: input_method::InputMethodState,
    /// `zwlr_output_manager_v1`: display settings tools (`output_management`).
    pub(crate) output_management: output_management::OutputManagementState,
    /// `zwlr_foreign_toplevel_manager_v1`: task bars (`wlr_toplevels`).
    wlr_toplevels: wlr_toplevels::WlrToplevelsState,
    /// `zwp_keyboard_shortcuts_inhibit_manager_v1` (`shortcuts_inhibit`).
    shortcuts_inhibit: shortcuts_inhibit::ShortcutsInhibitState,
    /// `wp_content_type_v1`: a surface says it shows a photo, a video or a game (`content_type`).
    #[expect(
        dead_code,
        reason = "owns the wp_content_type_manager_v1 global for the state's lifetime"
    )]
    content_type: smithay::wayland::content_type::ContentTypeState,
    /// `wp_alpha_modifier_v1`: a surface's opacity multiplier, applied where its render element
    /// is built (Smithay's surface element reads it).
    #[expect(
        dead_code,
        reason = "owns the wp_alpha_modifier_v1 global for the state's lifetime"
    )]
    alpha_modifier: smithay::wayland::alpha_modifier::AlphaModifierState,
    /// `xdg_system_bell_v1`: an app rings the bell; accepted and ignored (no sound or flash yet).
    #[expect(
        dead_code,
        reason = "owns the xdg_system_bell_v1 global for the state's lifetime"
    )]
    system_bell: smithay::wayland::xdg_system_bell::XdgSystemBellState,
    /// `wl_data_device_manager`: clipboard selection and drag-and-drop offers (2026-10-16).
    /// Selections are Smithay protocol objects; who sees and sets them follows Core's
    /// keyboard focus (`SeatHandler::focus_changed`).
    data_device: DataDeviceState,
    /// `zwp_primary_selection_device_manager_v1`: the primary selection, same focus rule.
    primary_selection: PrimarySelectionState,
    /// `zwlr_data_control_manager_v1` and `ext_data_control_manager_v1` (2026-10-25): clipboard
    /// tools (wl-copy, clipboard managers) set and read selections without a window of their own.
    /// Without them wl-copy mapped a window on every copy, which the open-focus rule could take
    /// for one the user opened (the camera jumped to it on every text selection).
    pub(crate) wlr_data_control: smithay::wayland::selection::wlr_data_control::DataControlState,
    pub(crate) ext_data_control: smithay::wayland::selection::ext_data_control::DataControlState,
    /// The client that set the clipboard and the primary selection (Smithay accepts a selection
    /// only from the keyboard-focused client). A selection whose owner's connection is gone is
    /// cleared, so other clients are told it is gone (Smithay does not do it by itself).
    selection_owners: [Option<ClientId>; 2],
    selection_sources: [Option<ObjectId>; 2],
    pub(crate) data_control_owner: Option<(
        smithay::wayland::selection::SelectionTarget,
        Option<ClientId>,
        Option<ObjectId>,
    )>,
    /// A client started a drag (Smithay installed its DnD grab); `refuse_client_drags` ends it.
    drag_started: bool,
    /// `xdg_activation_v1` (B10.7, INV-INPUT-9, W-SM-8).
    activation: XdgActivationState,
    /// Who may use the privileged globals (`trust`, `ALLOW_*`).
    pub(crate) trust: trust::SharedPolicy,
    /// `wp_security_context_manager_v1`: sandboxes (Flatpak) connect their apps through it.
    _security_context: smithay::wayland::security_context::SecurityContextState,
    /// Listeners a sandbox created, for the session loop to serve (`Session::dispatch`).
    pub(crate) security_listeners: Vec<(
        smithay::wayland::security_context::SecurityContextListenerSource,
        smithay::wayland::security_context::SecurityContext,
    )>,
    /// `zwlr_layer_shell_v1` (L1–L8) and its surfaces, in creation order.
    layer_shell: WlrLayerShellState,
    layers: Vec<layers::LayerHandle>,
    registry: SurfaceRegistry,
    elements: Vec<ElementHandle>,
    pub(crate) serial_outputs:
        std::collections::VecDeque<(smithay::utils::Serial, infynitae_core::output::OutputId)>,
    pub screencopy: screencopy::ScreencopyState,
    pub gamma: gamma::GammaState,
    pub output_power: output_power::OutputPowerState,
    /// Virtual pointers and seat-keymap virtual keyboards, as real input (`virtual_input`).
    pub(crate) virtual_input: virtual_input::VirtualInputState,
    /// `ext-image-copy-capture-v1` output capture (`image_copy`); frames join `screencopy`'s queue.
    #[expect(
        dead_code,
        reason = "owns the ext-image-copy-capture globals for the state's lifetime"
    )]
    image_copy: image_copy::ImageCopyState,
    pub dmabuf: DmabufState,
    /// The linux-dmabuf global, replaced when the backend names its GPU (`set_dmabuf_main_device`).
    dmabuf_global: Option<smithay::wayland::dmabuf::DmabufGlobal>,
    /// The GPU clients allocate on (`set_dmabuf_main_device`), also offered to capture sessions.
    pub(crate) dmabuf_main_dev: Option<libc::dev_t>,
    /// Frames waiting for the GPU to finish them, for the backend (`take_frame_waits`); `None`:
    /// this backend does not wait.
    frame_waits: Option<Vec<handlers::FrameWait>>,
    /// The formats clients may allocate (`set_dmabuf_device_formats`).
    dmabuf_formats: Vec<smithay::backend::allocator::Format>,
    /// Those the screens can show as they are (`add_dmabuf_scanout_formats`).
    dmabuf_scanout_formats: Vec<smithay::backend::allocator::Format>,
    pub(crate) popup_pointer_grab_serial: Option<smithay::utils::Serial>,
    /// Install serial of the popup keyboard grab (the latest level of the chain), the identity by
    /// which teardown releases exactly that grab (A1).
    pub(crate) popup_keyboard_grab_serial: Option<smithay::utils::Serial>,
    /// The caller-owned effect buffer of C14.3 for the hot entries (input, commit): cleared,
    /// not freed, between events.
    effects: infynitae_core::protocol::EffectBuf,
    /// Reused storage: the elements one frame tick completes.
    ticked: Vec<infynitae_core::stage::ElementId>,
    #[expect(
        dead_code,
        reason = "owns the zxdg_decoration_manager_v1 global for the state's lifetime"
    )]
    xdg_decoration: smithay::wayland::shell::xdg::decoration::XdgDecorationState,
    pub border_width: i32,
    pub border_ignore_csd: bool,
    /// CLIP_TO_GEOMETRY: a window's surfaces are drawn only inside its window geometry, not in the
    /// margins apps keep around it (stale pixels there showed as black blocks beside Discord).
    pub clip_to_geometry: bool,
    /// SQUARE_CORNERS: every window is told it is tiled on all four sides, so apps that round
    /// their corners and draw shadows draw a plain rectangle instead.
    pub square_corners: bool,
    /// LATENCY: when each screen's frame is drawn (frame_schedule).
    pub latency_mode: crate::frame_schedule::LatencyMode,
    /// LATENCY_STATS: input-to-screen times in the session log every 10 s.
    pub latency_stats: bool,
    pub border_active: smithay::backend::renderer::Color32F,
    pub border_inactive: smithay::backend::renderer::Color32F,
    pub border_ids: std::collections::HashMap<
        infynitae_core::stage::ElementId,
        [smithay::backend::renderer::element::Id; 4],
    >,
    /// Each window's border colour as last drawn, and its commit counter: bumped when the colour
    /// changes (focus moved), so the damage tracker repaints the border.
    pub border_commits: std::collections::HashMap<
        infynitae_core::stage::ElementId,
        (
            smithay::backend::renderer::Color32F,
            smithay::backend::renderer::utils::CommitCounter,
        ),
    >,
    pub rendered_elements: Vec<infynitae_core::stage::ElementId>,
    pub input_config: InputDeviceConfig,
    pub direct_scanout: Option<bool>,
    /// Client buffers may also go on overlay planes (`OVERLAY_PLANES`, off by default: amdgpu
    /// refused page flips for Electron's viewport-scaled subsurfaces there).
    pub overlay_planes: bool,
    /// The cursor on the GPU's cursor plane (`HARDWARE_CURSOR`, on by default): it moves without
    /// a redraw, and a fullscreen game with a cursor still goes to the screen directly.
    pub hardware_cursor: bool,
    pub wallpaper: Option<String>,
    pub wallpaper_mode: Option<String>,
    /// The config was read once: a wallpaper change after it restarts the wallpaper (the
    /// session script starts the first one).
    wallpaper_loaded: bool,
    pub keyboard_repeat_delay: i32,
    pub keyboard_repeat_rate: i32,
    pub private_elements: std::collections::HashSet<infynitae_core::stage::ElementId>,
    pub privacy_apps: Vec<String>,
    pub privacy_titles: Vec<String>,
    pub wine_transparency: bool,
    pub force_transparent_apps: Vec<String>,
    pub manual_idle_inhibit: bool,
    /// `ext_idle_notifier_v1`: user idle notifications for external idle listeners (swayidle).
    pub idle_notify: IdleNotifyState,
    /// Current virtual time for test harnesses driving the adapter in virtual time.
    pub(crate) current_time: Option<std::time::Duration>,
    /// The config's `EXEC_*` commands, by the index Core's `Effect::Exec` names.
    pub(crate) exec_commands: Vec<String>,
    /// Processes started from shortcuts, to tell the windows the user opened (`launch`).
    pub(crate) launches: launch::Launches,
    /// FOCUS_ON_OPEN / TELEPORT_ON_OPEN = "user" (the default): only windows the user opened.
    pub(crate) focus_open_user_only: bool,
    pub(crate) teleport_open_user_only: bool,
    /// FOCUS_ON_OPEN = "priority": new windows take the focus unless it is busy
    /// (`open_focus_held`).
    pub focus_open_priority: bool,
    /// The control menu's Do Not Disturb flag file (`do_not_disturb`); None without a runtime dir.
    pub dnd_flag: Option<std::path::PathBuf>,
    /// FOCUS_ON_ACTIVATE = "user" (the default): only tokens from the user's launches or the app
    /// itself move the focus (`request_activation`).
    pub(crate) activate_user_only: bool,
    /// `ext-session-lock-v1`, when the backend enforces it (`enable_session_lock`).
    session_lock: Option<lock::SessionLock>,
    // probe builds only: the Send assertion must be the one and only error
    #[cfg(feature = "probe_store_port_in_state")]
    leaked_port: infynitae_core::InputPort,
    #[cfg(feature = "probe_store_output_token_in_state")]
    leaked_token: infynitae_core::OutputLifecycleToken,
}

/// C7.4 / EC-15: protocol state must be `Send`.
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<WaylandState>();
};

impl WaylandState {
    /// The backend draws into a window of a host compositor through GL (the nested Winit
    /// backend): its framebuffer's rows run bottom-up, so every output is drawn flipped, and the
    /// Canvas is cleared to an opaque colour (a transparent clear lets the host's desktop show
    /// through). The headless backend keeps the default: top-down rows, a transparent clear.
    pub fn set_host_window_target(&mut self) {
        self.set_target(convert::RenderTarget {
            flip_y: true,
            clear: convert::CANVAS_CLEAR,
            draw_cursor: false,
        });
    }

    /// The backend owns the display (the console backend): nobody else draws the pointer, so the
    /// compositor does (T8); the Canvas is opaque. The framebuffer's rows run top-down (Smithay's
    /// DRM compositor renders into its own swapchain the right way up).
    pub fn set_console_target(&mut self) {
        self.screencopy.enable_capture();
        self.set_target(convert::RenderTarget {
            flip_y: false,
            clear: convert::CANVAS_CLEAR,
            draw_cursor: true,
        });
    }

    fn set_target(&mut self, target: convert::RenderTarget) {
        self.target = target;
        for h in &mut self.outputs {
            if let (Some(model), Some(view)) = (self.core.output(h.id), self.core.view(h.id)) {
                h.tracker = convert::damage_tracker(model, &view, self.target);
                h.tracker_k = view.k;
            }
        }
    }

    /// Removes disabled output globals whose delay has elapsed.
    pub fn prune_disabled_outputs(&mut self) {
        let now = std::time::Instant::now();
        self.disabled_outputs.retain(|(global, deadline)| {
            if *deadline <= now {
                self.dh.remove_global::<WaylandState>(global.clone());
                false
            } else {
                true
            }
        });
    }

    /// How many absolute motions Core has sent, counting those the adapter then dropped as a
    /// repeat of what the client has (for tests: a motion Core should not have sent is seen even
    /// when it changed nothing on the wire).
    pub fn pointer_motions(&self) -> u64 {
        self.pointer_motions
    }

    /// Number of disabled output globals awaiting removal (for tests).
    pub fn disabled_outputs_count(&self) -> usize {
        self.disabled_outputs.len()
    }

    /// Forces expiration of disabled outputs' removal delays (for tests).
    pub fn expire_disabled_outputs(&mut self) {
        let past = std::time::Instant::now() - std::time::Duration::from_secs(1);
        for (_, deadline) in &mut self.disabled_outputs {
            *deadline = past;
        }
    }
}

impl WaylandState {
    /// Creates the globals (`wl_compositor` v6 and `wl_subcompositor`, `wl_shm`, `xdg_wm_base`,
    /// `zxdg_output_manager_v1`, `wp_presentation`, `wl_seat` with pointer and keyboard,
    /// `zwp_relative_pointer_manager_v1`). Output globals follow Core's effects.
    pub fn new(display: &DisplayHandle, core: Core) -> Self {
        let mut seats = SeatState::new();
        let mut seat = seats.new_wl_seat(display, "seat0");
        seat.add_pointer();
        let mut repeat_delay = std::env::var("KEYBOARD_REPEAT_DELAY")
            .ok()
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(200);
        let mut repeat_rate = std::env::var("KEYBOARD_REPEAT_RATE")
            .ok()
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(35);
        let mut xkb_layout = std::env::var("XKB_DEFAULT_LAYOUT").unwrap_or_default();
        let mut xkb_options = std::env::var("XKB_DEFAULT_OPTIONS").unwrap_or_default();

        if let Ok(home) = std::env::var("HOME") {
            let config_path = format!("{home}/.config/infynitae/config");
            if let Ok(content) = std::fs::read_to_string(config_path) {
                for (key, val) in content.lines().filter_map(config_line) {
                    let key = key.trim();
                    let val = val.trim();
                    match key {
                        "KEYBOARD_REPEAT_DELAY" => {
                            if let Ok(d) = val.parse::<i32>() {
                                repeat_delay = d;
                            }
                        }
                        "KEYBOARD_REPEAT_RATE" => {
                            if let Ok(r) = val.parse::<i32>() {
                                repeat_rate = r;
                            }
                        }
                        "XKB_DEFAULT_LAYOUT" => {
                            if !val.is_empty() {
                                xkb_layout = val.to_string();
                            }
                        }
                        "XKB_DEFAULT_OPTIONS" if !val.is_empty() => {
                            xkb_options = val.to_string();
                        }
                        _ => {}
                    }
                }
            }
        }

        // a layout that makes no keymap (empty, misspelled) is never the seat's: `us` then
        let (xkb_layout, xkb_options) =
            virtual_input::usable_keymap_names(&xkb_layout, &xkb_options);
        virtual_input::set_seat_keymap_names(&xkb_layout, &xkb_options);
        let seat_keymap = virtual_input::keymap_text(
            &xkb_layout,
            (!xkb_options.is_empty()).then(|| xkb_options.clone()),
        );
        let xkb_config = smithay::input::keyboard::XkbConfig {
            layout: &xkb_layout,
            options: if xkb_options.is_empty() {
                None
            } else {
                Some(xkb_options)
            },
            ..Default::default()
        };
        seat.add_keyboard(xkb_config, repeat_delay, repeat_rate)
            .expect("the seat has a keyboard");
        let capabilities = convert::wm_capabilities(core.toplevel_capabilities());

        // the privileged globals are offered only to the clients the policy allows (`trust`)
        let trust = trust::SharedPolicy::default();
        if let Ok(home) = std::env::var("HOME")
            && let Ok(content) = std::fs::read_to_string(format!("{home}/.config/infynitae/config"))
        {
            trust.set(trust_policy(std::path::Path::new(&home), &content));
        }
        use trust::Capability as Cap;
        // the primary selection exists before the data-control globals that also serve it
        let primary_selection = PrimarySelectionState::new::<Self>(display);
        let wlr_data_control =
            smithay::wayland::selection::wlr_data_control::DataControlState::new::<Self, _>(
                display,
                Some(&primary_selection),
                trust.gate(Cap::Clipboard).filter(),
            );
        let ext_data_control =
            smithay::wayland::selection::ext_data_control::DataControlState::new::<Self, _>(
                display,
                Some(&primary_selection),
                trust.gate(Cap::Clipboard).filter(),
            );
        let mut state = WaylandState {
            core,
            dh: display.clone(),
            compositor: CompositorState::new_v6::<Self>(display),
            output_manager: OutputManagerState::new_with_xdg_output::<Self>(display),
            presentation: PresentationState::new::<Self>(display, CLOCK_MONOTONIC),
            outputs: Vec::new(),
            disabled_outputs: Vec::new(),
            spawned_children: false,
            x11_keys: Default::default(),
            target: convert::RenderTarget::default(),
            cursor: smithay::input::pointer::CursorImageStatus::default_named(),
            cursor_epoch: std::time::Duration::ZERO,
            pointer_sent: pointer::Sent::default(),
            pointer_motions: 0,
            arrow: cursor::Arrow::default(),
            capture_indicator: capture_indicator::CaptureIndicator::default(),
            background: background::Background::default(),
            cursor_theme: std::sync::Mutex::new(None),
            config_home: None,
            xcursor_path: None,
            // only what Core's toplevel-request policy can grant is announced (M2)
            xdg_shell: XdgShellState::new_with_capabilities::<Self>(display, capabilities),
            shm: ShmState::new::<Self>(display, []),
            popups: PopupManager::default(),
            popups_created: 0,
            seats,
            seat,
            popup_grab: None,
            popup_pointer_grab_serial: None,
            popup_keyboard_grab_serial: None,
            nested_grab_keep_pointer: true,
            ordered_preemption_strategy: keyboard::PreemptionStrategy::Ordered,
            pointer_constraints: PointerConstraintsState::new::<Self>(display),
            relative_pointer: RelativePointerManagerState::new::<Self>(display),
            fractional_scale_manager: FractionalScaleManagerState::new::<Self>(display),
            viewporter: ViewporterState::new::<Self>(display),
            cursor_shape: smithay::wayland::cursor_shape::CursorShapeManagerState::new::<Self>(
                display,
            ),
            xdg_foreign: smithay::wayland::xdg_foreign::XdgForeignState::new::<Self>(display),
            single_pixel_buffer: smithay::wayland::single_pixel_buffer::SinglePixelBufferState::new::<
                Self,
            >(display),
            foreign_toplevel_list:
                smithay::wayland::foreign_toplevel_list::ForeignToplevelListState::new_with_filter::<
                    Self,
                >(display, trust.gate(Cap::Windows).filter()),
            idle_inhibit: idle_inhibit::IdleInhibit::new(display),
            tearing: tearing::Tearing::new(display),
            touch_last: Vec::new(),
            foreign_toplevels: std::collections::HashMap::new(),
            shortcuts_inhibit: shortcuts_inhibit::ShortcutsInhibitState::new(display),
            wlr_toplevels: wlr_toplevels::WlrToplevelsState::new(display, trust.gate(Cap::Windows)),
            output_management: output_management::OutputManagementState::new(
                display,
                trust.gate(Cap::Display),
            ),
            input_method: input_method::InputMethodState::new(display, &trust),
            content_type: smithay::wayland::content_type::ContentTypeState::new::<Self>(display),
            alpha_modifier: smithay::wayland::alpha_modifier::AlphaModifierState::new::<Self>(
                display,
            ),
            system_bell: smithay::wayland::xdg_system_bell::XdgSystemBellState::new::<Self>(
                display,
            ),
            data_device: DataDeviceState::new::<Self>(display),
            primary_selection,
            wlr_data_control,
            ext_data_control,
            selection_owners: [None, None],
            selection_sources: [None, None],
            data_control_owner: None,
            drag_started: false,
            activation: XdgActivationState::new::<Self>(display),
            _security_context: smithay::wayland::security_context::SecurityContextState::new::<
                Self,
                _,
            >(display, trust::SharedPolicy::unsandboxed),
            security_listeners: Vec::new(),
            layer_shell: WlrLayerShellState::new_with_filter::<Self, _>(
                display,
                trust.gate(Cap::Layers).filter(),
            ),
            layers: Vec::new(),
            registry: SurfaceRegistry::default(),
            elements: Vec::new(),
            serial_outputs: std::collections::VecDeque::new(),
            screencopy: screencopy::ScreencopyState::new(display, trust.gate(Cap::Capture)),
            gamma: gamma::GammaState::new(display, trust.gate(Cap::Display)),
            output_power: output_power::OutputPowerState::new(display, trust.gate(Cap::Display)),
            virtual_input: virtual_input::VirtualInputState::new(
                display,
                seat_keymap,
                trust.gate(Cap::Input),
            ),
            image_copy: image_copy::ImageCopyState::new(display, trust.gate(Cap::Capture)),
            dmabuf: DmabufState::new(),
            dmabuf_global: None,
            dmabuf_main_dev: None,
            frame_waits: None,
            dmabuf_formats: Vec::new(),
            dmabuf_scanout_formats: Vec::new(),
            effects: infynitae_core::protocol::EffectBuf::new(),
            ticked: Vec::new(),
            xdg_decoration: smithay::wayland::shell::xdg::decoration::XdgDecorationState::new::<Self>(
                display,
            ),
            // none until the config asks for one (config/config: BORDER_WIDTH="1")
            border_width: 0,
            border_ignore_csd: true,
            clip_to_geometry: true,
            square_corners: true,
            latency_mode: crate::frame_schedule::LatencyMode::Off,
            latency_stats: false,
            border_active: smithay::backend::renderer::Color32F::new(0.48, 0.64, 0.97, 1.0),
            border_inactive: smithay::backend::renderer::Color32F::new(0.20, 0.23, 0.33, 0.8),
            border_ids: std::collections::HashMap::new(),
            border_commits: std::collections::HashMap::new(),
            rendered_elements: Vec::new(),
            input_config: InputDeviceConfig::default(),
            direct_scanout: None,
            overlay_planes: false,
            hardware_cursor: true,
            wallpaper: None,
            wallpaper_mode: None,
            wallpaper_loaded: false,
            keyboard_repeat_delay: repeat_delay,
            keyboard_repeat_rate: repeat_rate,
            private_elements: std::collections::HashSet::new(),
            privacy_apps: Vec::new(),
            privacy_titles: Vec::new(),
            wine_transparency: true,
            force_transparent_apps: Vec::new(),
            manual_idle_inhibit: false,
            idle_notify: IdleNotifyState::new(display),
            current_time: None,
            exec_commands: Vec::new(),
            launches: launch::Launches::default(),
            focus_open_user_only: false,
            focus_open_priority: true,
            dnd_flag: std::env::var_os("XDG_RUNTIME_DIR")
                .map(|d| std::path::PathBuf::from(d).join("infynitae/do-not-disturb")),
            teleport_open_user_only: true,
            activate_user_only: true,
            session_lock: None,
            // probe builds only: the Send assertion must be the one and only error
            #[cfg(feature = "probe_store_port_in_state")]
            leaked_port: unreachable!(),
            #[cfg(feature = "probe_store_output_token_in_state")]
            leaked_token: unreachable!(),
            trust,
        };
        // a guess until the backend names its GPU (the console does)
        if let Some(dev) = guess_dmabuf_device() {
            state.set_dmabuf_main_device(dev);
        }
        state
    }

    /// The GPU clients should allocate their buffers on (linux-dmabuf feedback main device): the
    /// render node the backend draws with. Replaces the global made with the previous device.
    pub fn set_dmabuf_main_device(&mut self, dev: libc::dev_t) {
        self.set_dmabuf_device_formats(dev, DMABUF_FORMATS.to_vec());
    }

    /// [`set_dmabuf_main_device`](Self::set_dmabuf_main_device) with the formats and modifiers
    /// the backend's renderer imports (tiled and compressed layouts render far faster than
    /// linear ones); an empty list keeps the linear formats.
    pub fn set_dmabuf_device_formats(
        &mut self,
        dev: libc::dev_t,
        formats: Vec<smithay::backend::allocator::Format>,
    ) {
        let formats = if formats.is_empty() {
            DMABUF_FORMATS.to_vec()
        } else {
            formats
        };
        self.dmabuf_formats = formats;
        self.dmabuf_main_dev = Some(dev);
        self.publish_dmabuf_global();
    }

    /// The formats a screen's main plane shows as they are. Clients hear of them first, so a
    /// fullscreen game allocates buffers the screen can take directly (direct scanout) and not
    /// a compressed layout only the GPU can read, which must be redrawn every frame.
    pub fn add_dmabuf_scanout_formats(
        &mut self,
        plane: impl IntoIterator<Item = smithay::backend::allocator::Format>,
    ) {
        let new = scanout_tranche(&self.dmabuf_formats, plane);
        let before = self.dmabuf_scanout_formats.len();
        for format in new {
            if !self.dmabuf_scanout_formats.contains(&format) {
                self.dmabuf_scanout_formats.push(format);
            }
        }
        if self.dmabuf_scanout_formats.len() != before {
            println!(
                "[infynitae] Client buffers: {} formats/modifiers the screen shows directly",
                self.dmabuf_scanout_formats.len()
            );
            self.publish_dmabuf_global();
        }
    }

    /// From now on a client's frame is applied only once the GPU has finished it; the backend
    /// takes the frames to watch with [`Self::take_frame_waits`].
    pub fn enable_frame_waits(&mut self) {
        self.frame_waits = Some(Vec::new());
    }

    /// The frames that began to wait since the last call.
    pub fn take_frame_waits(&mut self) -> Vec<handlers::FrameWait> {
        self.frame_waits
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// A frame of `client` is finished: what waited for it is applied.
    pub fn frame_ready(&mut self, client: &smithay::reexports::wayland_server::Client) {
        use smithay::wayland::compositor::CompositorHandler;
        let dh = self.dh.clone();
        self.client_compositor_state(client)
            .blocker_cleared(self, &dh);
    }

    /// The format and modifier of the buffer `id` shows, when it is a dmabuf.
    pub fn window_buffer_format(
        &self,
        id: ElementId,
    ) -> Option<smithay::backend::allocator::Format> {
        use smithay::backend::allocator::Buffer as _;
        let surface = self.window_surface(id)?;
        smithay::wayland::compositor::with_states(&surface, |states| {
            let state = states
                .data_map
                .get::<smithay::backend::renderer::utils::RendererSurfaceStateUserData>()?
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let dmabuf = smithay::wayland::dmabuf::get_dmabuf(state.buffer()?).ok()?;
            Some(dmabuf.format())
        })
    }

    /// (Re)creates the linux-dmabuf global from the device and formats known now.
    fn publish_dmabuf_global(&mut self) {
        use smithay::reexports::wayland_protocols::wp::linux_dmabuf::zv1::server::zwp_linux_dmabuf_feedback_v1::TrancheFlags;
        let Some(dev) = self.dmabuf_main_dev else {
            return;
        };
        let formats = self.dmabuf_formats.clone();
        let mut builder = DmabufFeedbackBuilder::new(dev, formats.iter().copied());
        if !self.dmabuf_scanout_formats.is_empty() {
            builder = builder.add_preference_tranche(
                dev,
                Some(TrancheFlags::Scanout),
                self.dmabuf_scanout_formats.iter().copied(),
            );
        }
        let global = match builder.build() {
            Ok(feedback) => self
                .dmabuf
                .create_global_with_default_feedback::<Self>(&self.dh, &feedback),
            Err(_) => self.dmabuf.create_global::<Self>(&self.dh, formats),
        };
        if let Some(old) = self.dmabuf_global.replace(global) {
            self.dmabuf.destroy_global::<Self>(&self.dh, old);
        }
    }

    pub fn set_border_config(
        &mut self,
        width: i32,
        active: smithay::backend::renderer::Color32F,
        inactive: smithay::backend::renderer::Color32F,
    ) {
        self.border_width = width.max(0);
        self.border_active = active;
        self.border_inactive = inactive;
    }

    /// Whether an element's toplevel negotiated Client-Side Decorations (CSD).
    pub fn is_csd_element(&self, element: infynitae_core::stage::ElementId) -> bool {
        use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode;
        self.window(element)
            .and_then(|w| w.toplevel())
            .map(|t| t.current_state().decoration_mode == Some(Mode::ClientSide))
            .unwrap_or(false)
    }

    pub fn core(&self) -> &Core {
        &self.core
    }

    /// The composition root and backends reach Core through the state, as handlers do.
    pub fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    /// One Core entry into the state's reusable [`EffectBuf`](infynitae_core::protocol::EffectBuf)
    /// (C14.3), whose effects are then applied in order: `f` calls an `_into` entry of Core. On
    /// an error nothing is applied. The buffer is cleared, not freed, so a steady stream of
    /// events allocates nothing for its effects (INV-ALLOC-1).
    pub fn with_effects<R, E>(
        &mut self,
        f: impl FnOnce(&mut Core, &mut infynitae_core::protocol::EffectBuf) -> Result<R, E>,
    ) -> Result<R, E> {
        let mut fx = std::mem::take(&mut self.effects);
        fx.clear();
        let result = f(&mut self.core, &mut fx);
        if result.is_ok() {
            self.apply(&fx);
            if self.core.take_physical_input_activity() {
                let now = self.current_time.unwrap_or_else(presentation_clock_now);
                self.notify_activity(now);
            }
        }
        self.effects = fx;
        result
    }

    /// A vblank of `output`: Core's frame clock decides which observed elements complete (into
    /// reused storage), and their callbacks complete and their feedback is presented.
    pub fn frame_tick(
        &mut self,
        output: infynitae_core::output::OutputId,
        now: std::time::Duration,
    ) {
        let mut ticked = std::mem::take(&mut self.ticked);
        ticked.clear();
        self.core.frame_tick_into(output, &mut ticked);
        self.complete_frames(output, &ticked, now);
        self.ticked = ticked;
        self.complete_layer_frames(output, now);
        self.complete_lock_frames(output, now);
        if self.target.draw_cursor
            && let Some(o) = self.smithay_output(output).cloned()
        {
            self.complete_cursor_frames(&o, now);
        }
        if let Some(o) = self.smithay_output(output).cloned() {
            self.complete_input_method_frames(&o, now);
        }
    }

    /// The colour the empty Canvas is cleared to on this backend's target.
    pub fn clear_color(&self) -> smithay::backend::renderer::Color32F {
        self.target.clear
    }

    /// Sets the colour the empty Canvas is cleared to on this backend's target.
    pub fn set_clear_color(&mut self, clear: smithay::backend::renderer::Color32F) {
        self.target.clear = clear;
    }

    /// Reloads configuration from ~/.config/infynitae/config into Core and compositor state.
    /// `MAX_APP_SCALE`: how far apps render sharper as you zoom in (the scale policy's ceiling,
    /// PROV-1). Never below a screen's own scale, at most 8; absent or invalid,
    /// [`DEFAULT_MAX_APP_SCALE`].
    fn set_max_app_scale(&mut self, value: Option<f64>) {
        use infynitae_core::scale::ScalePolicyConfig;
        let screens = self
            .core
            .outputs()
            .iter()
            .map(|o| o.scale)
            .fold(1.0, f64::max);
        let h_cfg = value
            .filter(|v| v.is_finite() && *v > 0.0)
            .unwrap_or(DEFAULT_MAX_APP_SCALE)
            .clamp(screens, 8.0_f64.max(screens));
        let config = ScalePolicyConfig {
            h_cfg,
            ..*self.core.scale_config()
        };
        if config != *self.core.scale_config() {
            match self.core.set_scale_config(config) {
                Ok(fx) => self.apply(&fx),
                Err(e) => eprintln!("[infynitae] MAX_APP_SCALE {h_cfg} refused: {e:?}"),
            }
        }
    }

    /// `ZOOM_SHARPEN`: "after" (apps re-render once a zoom settles, at whole scales only) or
    /// "live" (at each rung while zooming, the ladder's finer steps).
    fn set_zoom_sharpen(&mut self, after: bool) {
        use infynitae_core::scale::{DEFAULT_R, ScalePolicyConfig};
        let config = ScalePolicyConfig {
            sharpen_after: after,
            // a ratio of 2 leaves the whole scales (and the screens' own) as the only rungs
            r: if after { 2.0 } else { DEFAULT_R },
            ..*self.core.scale_config()
        };
        if config != *self.core.scale_config() {
            match self.core.set_scale_config(config) {
                Ok(fx) => self.apply(&fx),
                Err(e) => eprintln!("[infynitae] ZOOM_SHARPEN refused: {e:?}"),
            }
        }
    }

    pub fn reload_config(&mut self) {
        let mut new_cursor_theme: Option<String> = None;
        let mut new_cursor_size: Option<u32> = None;
        let home = self
            .config_home
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .or_else(|| std::env::var("HOME").ok());
        if let Some(ref home) = home {
            let config_path = format!("{home}/.config/infynitae/config");
            if let Ok(content) = std::fs::read_to_string(config_path) {
                // for the clients that connect from now on
                self.trust
                    .set(trust_policy(std::path::Path::new(home), &content));
                let mut new_wallpaper: Option<String> = None;
                let mut new_wallpaper_mode: Option<String> = None;
                let mut free_cursor_on_super = true;
                let mut bind_lines: Vec<(String, String)> = Vec::new();
                let mut exec_lines: Vec<(String, String)> = Vec::new();
                let mut max_app_scale: Option<f64> = None;
                let mut x11_keys_mode = x11_keys::Mode::default();
                // "after" unless the config says "live" (a config from before the setting too)
                let mut zoom_sharpen_after = true;
                let (mut background_priority, mut game_mode) = (true, true);
                let mut tearing = true;
                for line in content.lines() {
                    if let Some((key, val)) = config_line(line) {
                        let (key, val) = (key.as_str(), val.as_str());
                        if let Some(name) = key.strip_prefix("BIND_") {
                            bind_lines.push((name.to_ascii_uppercase(), val.to_string()));
                            continue;
                        }
                        if key.starts_with("EXEC_") {
                            exec_lines.push((key.to_string(), val.to_string()));
                            continue;
                        }
                        match key {
                            "CURSOR_THEME" => {
                                let trimmed = val.trim();
                                if !trimmed.is_empty() {
                                    new_cursor_theme = Some(trimmed.to_string());
                                }
                            }
                            "CURSOR_SIZE" => {
                                if let Ok(sz) = val.parse::<u32>() {
                                    new_cursor_size = Some(sz);
                                }
                            }
                            "PAN_MODE" => match val.to_lowercase().as_str() {
                                "rts" => self
                                    .core
                                    .set_pan_mode(infynitae_core::navigation::PanMode::Rts),
                                "free" => self
                                    .core
                                    .set_pan_mode(infynitae_core::navigation::PanMode::Free),
                                _ => self
                                    .core
                                    .set_pan_mode(infynitae_core::navigation::PanMode::Grab),
                            },
                            "SCREENCOPY_CURSOR" => {
                                let hide = matches!(
                                    val.to_lowercase().as_str(),
                                    "0" | "false" | "off" | "no"
                                );
                                self.screencopy.set_hide_cursor(hide);
                            }
                            "MAX_APP_SCALE" => {
                                max_app_scale = val.parse::<f64>().ok();
                            }
                            "BACKGROUND_PRIORITY" => {
                                background_priority = !matches!(
                                    val.to_lowercase().as_str(),
                                    "0" | "false" | "off" | "no"
                                );
                            }
                            "TEARING" => {
                                tearing = !matches!(
                                    val.to_lowercase().as_str(),
                                    "0" | "false" | "off" | "no"
                                );
                            }
                            "GAME_MODE" => {
                                game_mode = !matches!(
                                    val.to_lowercase().as_str(),
                                    "0" | "false" | "off" | "no"
                                );
                            }
                            "CAPTURE_INDICATOR" => {
                                self.capture_indicator.enabled = !matches!(
                                    val.to_lowercase().as_str(),
                                    "0" | "false" | "off" | "no"
                                );
                            }
                            "FOCUS_FOLLOWS_MOUSE" => match val.to_lowercase().as_str() {
                                "strict" => {
                                    self.core.set_focus_follows_mouse(true);
                                    self.core.set_focus_follows_mouse_strict(true);
                                }
                                "0" | "false" | "off" | "no" | "click" => {
                                    self.core.set_focus_follows_mouse(false);
                                    self.core.set_focus_follows_mouse_strict(false);
                                }
                                _ => {
                                    self.core.set_focus_follows_mouse(true);
                                    self.core.set_focus_follows_mouse_strict(false);
                                }
                            },
                            "FOCUS_CYCLE_NAVIGATE" => {
                                let enabled = !matches!(
                                    val.to_lowercase().as_str(),
                                    "0" | "false" | "off" | "no"
                                );
                                self.core.set_focus_cycle_navigate(enabled);
                            }
                            "FOCUS_ON_OPEN" => {
                                let enabled = !matches!(
                                    val.to_lowercase().as_str(),
                                    "0" | "false" | "off" | "no"
                                );
                                self.core.set_focus_on_open(enabled);
                                self.focus_open_user_only = val.eq_ignore_ascii_case("user");
                                self.focus_open_priority = val.eq_ignore_ascii_case("priority");
                            }
                            "TELEPORT_ON_OPEN" => {
                                let enabled = !matches!(
                                    val.to_lowercase().as_str(),
                                    "0" | "false" | "off" | "no"
                                );
                                self.core.set_teleport_on_open(enabled);
                                self.teleport_open_user_only = val.eq_ignore_ascii_case("user");
                            }
                            "SUPER_ENTER_ACTION" => {
                                if val.eq_ignore_ascii_case("fit") {
                                    self.core.set_enter_mode(infynitae_core::EnterMode::Fit);
                                } else {
                                    self.core
                                        .set_enter_mode(infynitae_core::EnterMode::Maximize);
                                }
                            }
                            "FULLSCREEN_PAN" => {
                                let enabled = !matches!(
                                    val.to_lowercase().as_str(),
                                    "0" | "false" | "off" | "no" | "exclusive"
                                );
                                self.core.set_fullscreen_pan(enabled);
                            }
                            "PLACEMENT_POLICY" => match val.to_lowercase().as_str() {
                                "center" | "centred" | "centered" => {
                                    self.core.set_placement_policy(
                                        infynitae_core::PlacementPolicy::CentredInView,
                                    );
                                }
                                "origin" => {
                                    self.core.set_placement_policy(
                                        infynitae_core::PlacementPolicy::OriginAtCameraCentre,
                                    );
                                }
                                _ => {
                                    self.core.set_placement_policy(
                                        infynitae_core::PlacementPolicy::ClusterTiling,
                                    );
                                }
                            },
                            "CANVAS_COLOR" => {
                                if let Some(c) = crate::parse_color_hex(val) {
                                    self.set_clear_color(c);
                                }
                            }
                            "BORDER_WIDTH" => {
                                if let Ok(w) = val.parse::<i32>() {
                                    self.border_width = w.max(0);
                                }
                            }
                            "LATENCY" => {
                                self.latency_mode = crate::frame_schedule::LatencyMode::parse(val);
                            }
                            "LATENCY_STATS" => {
                                self.latency_stats = matches!(
                                    val.to_lowercase().as_str(),
                                    "1" | "true" | "on" | "yes"
                                );
                            }
                            "SQUARE_CORNERS" => {
                                self.square_corners = !matches!(
                                    val.to_lowercase().as_str(),
                                    "0" | "false" | "off" | "no"
                                );
                            }
                            "CLIP_TO_GEOMETRY" => {
                                self.clip_to_geometry = !matches!(
                                    val.to_lowercase().as_str(),
                                    "0" | "false" | "off" | "no"
                                );
                            }
                            "BORDER_IGNORE_CSD" => {
                                self.border_ignore_csd = !matches!(
                                    val.to_lowercase().as_str(),
                                    "0" | "false" | "off" | "no"
                                );
                            }
                            "BORDER_ACTIVE" => {
                                if let Some(c) = crate::parse_color_hex(val) {
                                    self.border_active = c;
                                }
                            }
                            "BORDER_INACTIVE" => {
                                if let Some(c) = crate::parse_color_hex(val) {
                                    self.border_inactive = c;
                                }
                            }
                            "MOUSE_ACCEL_PROFILE" => {
                                self.input_config.mouse_accel_profile = Some(val.to_string());
                            }
                            "MOUSE_ACCEL_SPEED" => {
                                if let Ok(s) = val.parse::<f64>() {
                                    self.input_config.mouse_accel_speed = Some(s.clamp(-1.0, 1.0));
                                }
                            }
                            "TOUCHPAD_TAP" => {
                                self.input_config.touchpad_tap = Some(matches!(
                                    val.to_lowercase().as_str(),
                                    "1" | "true" | "yes" | "on"
                                ));
                            }
                            "TOUCHPAD_NATURAL_SCROLL" => {
                                self.input_config.touchpad_natural_scroll = Some(matches!(
                                    val.to_lowercase().as_str(),
                                    "1" | "true" | "yes" | "on"
                                ));
                            }
                            "XWAYLAND_KEYS" => x11_keys_mode = x11_keys::Mode::parse(val),
                            "ZOOM_SHARPEN" => {
                                zoom_sharpen_after = !val.eq_ignore_ascii_case("live")
                            }
                            "DIRECT_SCANOUT" => {
                                self.direct_scanout = Some(matches!(
                                    val.to_lowercase().as_str(),
                                    "1" | "true" | "yes" | "on"
                                ));
                            }
                            "FOCUS_ON_ACTIVATE" => {
                                use infynitae_core::input::focus::ActivationPolicy;
                                let v = val.to_lowercase();
                                self.activate_user_only = v == "user";
                                self.core.set_activation_policy(match v.as_str() {
                                    "never" | "0" | "false" | "off" => {
                                        ActivationPolicy::AttentionOnly
                                    }
                                    _ => ActivationPolicy::Strict,
                                });
                            }
                            "SUPER_FREES_CURSOR" => {
                                free_cursor_on_super = !matches!(
                                    val.to_lowercase().as_str(),
                                    "0" | "false" | "off" | "no"
                                );
                            }
                            "HARDWARE_CURSOR" => {
                                self.hardware_cursor = !matches!(
                                    val.to_lowercase().as_str(),
                                    "0" | "false" | "off" | "no"
                                );
                            }
                            "OVERLAY_PLANES" => {
                                self.overlay_planes = matches!(
                                    val.to_lowercase().as_str(),
                                    "1" | "true" | "yes" | "on"
                                );
                            }
                            "WALLPAPER" => {
                                let trimmed = val.trim();
                                if !trimmed.is_empty() {
                                    new_wallpaper = Some(trimmed.to_string());
                                }
                            }
                            "WALLPAPER_MODE" => {
                                let trimmed = val.trim();
                                if !trimmed.is_empty() {
                                    new_wallpaper_mode = Some(trimmed.to_string());
                                }
                            }
                            "KEYBOARD_REPEAT_DELAY" => {
                                if let Ok(d) = val.parse::<i32>() {
                                    self.keyboard_repeat_delay = d;
                                    if let Some(kb) = self.seat.get_keyboard() {
                                        kb.change_repeat_info(self.keyboard_repeat_rate, d);
                                    }
                                }
                            }
                            "KEYBOARD_REPEAT_RATE" => {
                                if let Ok(r) = val.parse::<i32>() {
                                    self.keyboard_repeat_rate = r;
                                    if let Some(kb) = self.seat.get_keyboard() {
                                        kb.change_repeat_info(r, self.keyboard_repeat_delay);
                                    }
                                }
                            }
                            "PRIVACY_APPS" => {
                                self.privacy_apps = val
                                    .split(',')
                                    .map(|s| s.trim().to_lowercase())
                                    .filter(|s| !s.is_empty())
                                    .collect();
                            }
                            "PRIVACY_TITLES" => {
                                self.privacy_titles = val
                                    .split(',')
                                    .map(|s| s.trim().to_lowercase())
                                    .filter(|s| !s.is_empty())
                                    .collect();
                            }
                            "WINE_TRANSPARENCY" => {
                                self.wine_transparency = !matches!(
                                    val.to_lowercase().as_str(),
                                    "0" | "false" | "off" | "no"
                                );
                            }
                            "FORCE_TRANSPARENT_APPS" => {
                                self.force_transparent_apps = val
                                    .split(',')
                                    .map(|s| s.trim().to_lowercase())
                                    .filter(|s| !s.is_empty())
                                    .collect();
                            }
                            _ => {}
                        }
                    }
                }
                let (bindings, commands, problems) = user_key_bindings(&bind_lines, &exec_lines);
                for p in problems {
                    eprintln!("[infynitae] Config key binding ignored: {p}");
                }
                self.core.set_key_bindings(bindings);
                self.core.set_free_cursor_on_super(free_cursor_on_super);
                self.exec_commands = commands;
                self.set_max_app_scale(max_app_scale);
                self.x11_keys.mode = x11_keys_mode;
                self.set_zoom_sharpen(zoom_sharpen_after);
                self.background.game_mode = game_mode;
                self.set_tearing_enabled(tearing);
                self.background.enabled = background_priority;
                if !background_priority {
                    self.background_restore_all();
                }
                let first_load = !std::mem::replace(&mut self.wallpaper_loaded, true);
                if new_wallpaper != self.wallpaper || new_wallpaper_mode != self.wallpaper_mode {
                    self.wallpaper = new_wallpaper;
                    self.wallpaper_mode = new_wallpaper_mode;
                    // infynitae-reload finds this session's socket (the compositor itself has no
                    // WAYLAND_DISPLAY on the console) and expands the path; at the first load the
                    // session script starts the wallpaper
                    if !first_load {
                        let home = std::env::var("HOME").unwrap_or_default();
                        let bin = format!("{home}/.local/bin/infynitae-reload");
                        let reload = if std::path::Path::new(&bin).exists() {
                            bin
                        } else {
                            format!("{home}/infynitae/live/infynitae-reload")
                        };
                        let _ = std::process::Command::new(reload)
                            .arg("wallpaper")
                            .stdout(std::process::Stdio::null())
                            .spawn();
                        self.spawned_children = true;
                    }
                }
            }
        }
        let theme_name = new_cursor_theme
            .or_else(|| std::env::var("CURSOR_THEME").ok())
            .or_else(|| std::env::var("XCURSOR_THEME").ok())
            .unwrap_or_else(|| "default".to_string());
        let cursor_size = new_cursor_size
            .or_else(|| {
                std::env::var("CURSOR_SIZE")
                    .ok()
                    .and_then(|s| s.parse().ok())
            })
            .or_else(|| {
                std::env::var("XCURSOR_SIZE")
                    .ok()
                    .and_then(|s| s.parse().ok())
            })
            .unwrap_or(24);
        let changed = match self.cursor_theme.lock() {
            Ok(guard) => match guard.as_ref() {
                Some(mgr) => mgr.theme_name() != theme_name || mgr.size() != cursor_size,
                None => true,
            },
            Err(_) => true,
        };
        if changed {
            let home_path = self
                .config_home
                .as_ref()
                .map(|p| p.to_path_buf())
                .or_else(|| std::env::var("HOME").ok().map(std::path::PathBuf::from));
            let xcursor_path = self
                .xcursor_path
                .clone()
                .or_else(|| std::env::var("XCURSOR_PATH").ok());
            let paths =
                cursor::theme_search_paths_with(home_path.as_deref(), xcursor_path.as_deref());
            self.set_cursor_theme_with_search_paths(&theme_name, cursor_size, paths);
        }
    }

    /// Current input device configuration.
    pub fn input_config(&self) -> &InputDeviceConfig {
        &self.input_config
    }

    pub fn registry(&self) -> &SurfaceRegistry {
        &self.registry
    }

    /// The protocol handle of an element (render oracles and later increments).
    pub fn window(&self, id: ElementId) -> Option<&Window> {
        self.elements.iter().find(|e| e.id == id).map(|e| &e.window)
    }

    /// The root `wl_surface` of a window, whether native Wayland (XDG) or XWayland (X11).
    pub fn window_surface(
        &self,
        id: ElementId,
    ) -> Option<smithay::reexports::wayland_server::protocol::wl_surface::WlSurface> {
        use smithay::wayland::seat::WaylandFocus;
        self.window(id)
            .and_then(|w| w.wl_surface().map(|s| s.into_owned()))
    }

    /// Elements in creation order.
    pub fn elements(&self) -> impl Iterator<Item = ElementId> + '_ {
        self.elements.iter().map(|e| e.id)
    }

    /// Whether an element is marked private (omitted/transparent during screencopy, screenshare, RustDesk).
    pub fn is_element_private(&self, element: infynitae_core::stage::ElementId) -> bool {
        if self.private_elements.contains(&element) {
            return true;
        }
        if let Some(meta) = self.core.stage().meta(element) {
            if let Some(app_id) = &meta.app_id {
                let id_l = app_id.to_lowercase();
                if self.privacy_apps.iter().any(|p| id_l.contains(p)) {
                    return true;
                }
            }
            if let Some(title) = &meta.title {
                let t_l = title.to_lowercase();
                if self.privacy_titles.iter().any(|t| t_l.contains(t)) {
                    return true;
                }
            }
        }
        false
    }

    /// Toggles the privacy capture exemption on the currently focused window.
    pub fn toggle_privacy_focused(&mut self) {
        if let Some(focused) = self.core.focus().target() {
            if self.private_elements.contains(&focused) {
                self.private_elements.remove(&focused);
                println!("[infynitae] Privacy capture exemption DISABLED for window {focused:?}");
                self.spawn_action("notify-privacy-off");
            } else {
                self.private_elements.insert(focused);
                println!(
                    "[infynitae] Privacy capture exemption ENABLED for window {focused:?} (hidden from screenshots and screenshare)"
                );
                self.spawn_action("notify-privacy-on");
            }
        }
    }

    /// Toggles stay-awake idle sleep inhibition (blocks DPMS monitor sleep and auto-lock).
    pub fn toggle_idle_inhibit(&mut self) {
        self.manual_idle_inhibit = !self.manual_idle_inhibit;
        if self.manual_idle_inhibit {
            println!("[infynitae] Idle sleep & auto-lock INHIBITED (Stay Awake mode)");
            self.spawn_action("notify-idle-inhibit-on");
        } else {
            println!("[infynitae] Idle sleep & auto-lock RESTORED to normal timeout");
            self.spawn_action("notify-idle-inhibit-off");
        }
    }

    /// Whether idle sleep & screen auto-locking should be inhibited: stay-awake mode
    /// (Super+Shift+I), or an app showing a video or a game (`idle_inhibit`).
    pub fn is_idle_inhibited(&self) -> bool {
        self.manual_idle_inhibit || self.app_inhibits_idle()
    }

    /// Sets the current virtual time for test harnesses driving the adapter in virtual time.
    pub fn set_current_time(&mut self, now: std::time::Duration) {
        self.current_time = Some(now);
    }

    /// Earliest deadline among active, pending idle notifications (INV-IDLE-1).
    pub fn idle_notify_deadline(&self) -> Option<std::time::Duration> {
        let inhibited = self.is_idle_inhibited();
        self.idle_notify
            .notifications
            .iter()
            .filter(|n| n.resource.is_alive())
            .filter(|n| !n.is_idled && (n.ignore_inhibitor || !inhibited))
            .map(|n| self.idle_notify.last_activity + n.timeout)
            .min()
    }

    /// Evaluates idle notifications against `now`: sends `idled` once when timeout has passed,
    /// sends `resumed` if an idled notification became inhibited. Returns the next pending deadline.
    pub fn idle_notify_check(&mut self, now: std::time::Duration) -> Option<std::time::Duration> {
        self.idle_notify
            .notifications
            .retain(|n| n.resource.is_alive());
        let inhibited = self.is_idle_inhibited();
        for n in &mut self.idle_notify.notifications {
            if !n.ignore_inhibitor && inhibited {
                if n.is_idled {
                    n.resource.resumed();
                    n.is_idled = false;
                }
            } else {
                let deadline = self.idle_notify.last_activity + n.timeout;
                if now >= deadline && !n.is_idled {
                    n.resource.idled();
                    n.is_idled = true;
                }
            }
        }
        self.idle_notify_deadline()
    }

    /// Physical input occurred: resets the timer to `now` and sends `resumed` to all idled notifications.
    pub fn notify_activity(&mut self, now: std::time::Duration) {
        self.idle_notify.last_activity = now;
        self.idle_notify
            .notifications
            .retain(|n| n.resource.is_alive());
        for n in &mut self.idle_notify.notifications {
            if n.is_idled {
                n.resource.resumed();
                n.is_idled = false;
            }
        }
    }

    /// Whether an element should force transparent blending (Wine layered windows, colorkeys).
    pub fn is_element_transparent(&self, element: infynitae_core::stage::ElementId) -> bool {
        if !self.wine_transparency {
            return false;
        }
        if let Some(meta) = self.core.stage().meta(element) {
            if let Some(app_id) = &meta.app_id {
                let id_l = app_id.to_lowercase();
                if id_l.contains("wine")
                    || id_l.ends_with(".exe")
                    || self.force_transparent_apps.iter().any(|a| id_l.contains(a))
                {
                    return true;
                }
            }
            if let Some(title) = &meta.title {
                let t_l = title.to_lowercase();
                if self.force_transparent_apps.iter().any(|a| t_l.contains(a)) {
                    return true;
                }
            }
        }
        false
    }

    /// Returns the current modifier state from Smithay's XKB seat state (OBJ-27).
    pub fn seat_modifiers(&self) -> infynitae_core::input::bindings::Modifiers {
        self.seat
            .get_keyboard()
            .map(|kb| convert::modifiers_from_smithay(&kb.modifier_state()))
            .unwrap_or_default()
    }

    /// Returns whether the seat's keyboard currently has an active grab.
    pub fn keyboard_is_grabbed(&self) -> bool {
        self.keyboard_handle().is_grabbed()
    }

    /// Returns the currently focused surface of the seat's keyboard, if any.
    pub fn keyboard_current_focus(
        &self,
    ) -> Option<smithay::reexports::wayland_server::protocol::wl_surface::WlSurface> {
        self.keyboard_handle().current_focus()
    }

    /// Returns Smithay's raw XKB modifier state for testing and observation.
    pub fn keyboard_modifier_state(&self) -> smithay::input::keyboard::ModifiersState {
        self.keyboard_handle().modifier_state()
    }

    /// Updates Smithay's XKB state for the physical key event carried by `token` and returns the
    /// resulting modifiers and whether the modifier state changed, without forwarding the key to
    /// any client. The token is consumed; a token minted by another Core mutates nothing (R2-02).
    /// Only a logical transition reaches XKB (R2-04, single implementation for every backend,
    /// C3): a press of a key Core already holds, or a release of a key Core does not hold, is a
    /// duplicate/orphan that Core will absorb, so the seat must not see it either.
    pub fn sync_seat_key(
        &mut self,
        token: infynitae_core::InputAuthToken,
    ) -> (infynitae_core::input::bindings::Modifiers, bool) {
        if token.core_id() != self.core.id() {
            return (self.seat_modifiers(), false);
        }
        let (key, pressed) = (token.key(), token.pressed());
        let is_down = self.core.is_key_down(key);
        if pressed == is_down {
            return (self.seat_modifiers(), false);
        }
        use smithay::backend::input::KeyState;
        use smithay::input::keyboard::Keycode;

        let kb = self.keyboard_handle();
        let code = Keycode::new(key + 8);
        let state = if pressed {
            KeyState::Pressed
        } else {
            KeyState::Released
        };
        let (_, mods_changed) =
            kb.input_intercept(self, code, state, |_state, _mods, _sym| None::<()>);
        let modifiers = convert::modifiers_from_smithay(&kb.modifier_state());
        (modifiers, mods_changed)
    }

    /// Returns the seat keyboard's active LED indicator state (Caps Lock, Num Lock, Scroll Lock).
    pub fn keyboard_led_state(&self) -> smithay::input::keyboard::LedState {
        self.keyboard_handle().led_state()
    }

    /// Returns the number of interaction serials currently held in the provenance ring buffer.
    pub fn serial_outputs_len(&self) -> usize {
        self.serial_outputs.len()
    }

    /// Records the causal output on which a user interaction serial occurred (D-B6 Rule 1 provenance).
    pub(crate) fn record_serial_output(
        &mut self,
        serial: smithay::utils::Serial,
        output: infynitae_core::output::OutputId,
    ) {
        if self.serial_outputs.len() >= 64 {
            self.serial_outputs.pop_front();
        }
        self.serial_outputs.push_back((serial, output));
    }

    /// Retrieves the causal output mapped to a serial, if known.
    pub(crate) fn serial_output(
        &self,
        serial: smithay::utils::Serial,
    ) -> Option<infynitae_core::output::OutputId> {
        self.serial_outputs
            .iter()
            .rev()
            .find(|(s, _)| *s == serial)
            .map(|(_, o)| *o)
    }

    /// Registers the backend gamma handler for querying and setting output gamma ramps.
    pub fn set_gamma_handler(
        &mut self,
        handler: std::sync::Arc<std::sync::Mutex<dyn gamma::GammaHandler>>,
    ) {
        self.gamma.set_handler(handler);
    }

    /// Registers the backend output power handler for blanking / waking displays.
    pub fn set_output_power_handler(
        &mut self,
        handler: std::sync::Arc<std::sync::Mutex<dyn output_power::OutputPowerHandler>>,
    ) {
        self.output_power.set_handler(handler);
    }

    /// Returns the current power mode of an output (defaults to `Mode::On`).
    pub fn output_power_mode(
        &self,
        output: infynitae_core::output::OutputId,
    ) -> output_power::OutputPowerMode {
        self.output_power.mode(output)
    }

    /// Sets the power mode for a specific output and notifies listeners and backend.
    pub fn set_output_power_mode(
        &mut self,
        output: infynitae_core::output::OutputId,
        mode: output_power::OutputPowerMode,
    ) {
        self.output_power.set_mode(output, mode);
    }

    /// Sets the power mode for all known outputs (e.g. idle sleep / wake on input).
    pub fn set_all_outputs_power_mode(&mut self, mode: output_power::OutputPowerMode) {
        let ids: Vec<infynitae_core::output::OutputId> =
            self.outputs.iter().map(|h| h.id).collect();
        for id in ids {
            self.output_power.set_mode(id, mode);
        }
    }
}

/// One `KEY=value` line of the config, which the session scripts source as shell: the value is
/// read as one shell word (`"…"` with `\"` escapes, `'…'`, or bare up to a blank), so a `#` or a
/// quote inside it is kept, and a ` # comment` after it is not. `None` for a comment or a line
/// without `=`. Pure.
/// The `ALLOW_*` lists of a config's text (`trust`); a key it lacks keeps its default.
fn trust_policy(home: &std::path::Path, content: &str) -> trust::Policy {
    let lines: Vec<(String, String)> = content.lines().filter_map(config_line).collect();
    let mut lists: Vec<(trust::Capability, String)> = lines
        .iter()
        .filter_map(|(k, v)| trust::Capability::from_key(k.trim()).map(|c| (c, v.clone())))
        .collect();
    // the locker SCREEN_LOCKER names may hold the lock too
    let locker = lines
        .iter()
        .rev()
        .find(|(k, _)| k == "SCREEN_LOCKER")
        .and_then(|(_, v)| v.split_whitespace().next().map(str::to_string))
        .filter(|l| l != "auto");
    if let Some(locker) = locker {
        match lists
            .iter_mut()
            .rev()
            .find(|(c, _)| *c == trust::Capability::Lock)
        {
            Some((_, list)) => list.push_str(&format!(" {locker}")),
            None => lists.push((
                trust::Capability::Lock,
                format!("{} {locker}", trust::Capability::Lock.default_list()),
            )),
        }
    }
    trust::Policy::from_lists(Some(home), lists.iter().map(|(c, v)| (*c, v.as_str())))
}

pub fn config_line(line: &str) -> Option<(String, String)> {
    let line = line.trim_start();
    if line.starts_with('#') {
        return None;
    }
    let line = line.strip_prefix("export ").unwrap_or(line);
    let (key, value) = line.split_once('=')?;
    let mut out = String::new();
    let mut chars = value.trim_start().chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => break,
            '\'' => out.extend(chars.by_ref().take_while(|c| *c != '\'')),
            '"' => {
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some(e @ ('"' | '\\' | '$' | '`')) => out.push(e),
                            Some(e) => {
                                out.push('\\');
                                out.push(e);
                            }
                            None => out.push('\\'),
                        },
                        c => out.push(c),
                    }
                }
            }
            '\\' => out.extend(chars.next()),
            c => out.push(c),
        }
    }
    Some((key.trim().to_string(), out.trim().to_string()))
}

/// The user's key bindings from config lines: `BIND_<ACTION>="CHORD[,CHORD]"` gives an action
/// its own chords (its built-in chord is freed), `BIND_<ACTION>="none"` frees it without a new
/// one; `EXEC_<NAME>="CHORD:command"` runs a command. Returns the bindings, the commands (by
/// `Exec` index) and a description of each line that was not understood, or that names a chord
/// an earlier line already bound (the earlier line keeps it). Pure.
pub fn user_key_bindings(
    binds: &[(String, String)],
    execs: &[(String, String)],
) -> (
    infynitae_core::input::keys::UserBindings,
    Vec<String>,
    Vec<String>,
) {
    use infynitae_core::input::keys::{ACTION_NAMES, KeyAction, UserBindings, parse_chord};
    let mut out = UserBindings::default();
    let mut commands = Vec::new();
    let mut problems = Vec::new();
    // who bound each chord, for the duplicate report
    let mut bound_by: Vec<(infynitae_core::input::keys::Chord, String)> = Vec::new();
    let mut taken = |chord, line: String, problems: &mut Vec<String>, text: &str| {
        if let Some((_, first)) = bound_by.iter().find(|(c, _)| *c == chord) {
            problems.push(format!("{line}: \"{text}\" is already bound by {first}"));
            return true;
        }
        bound_by.push((chord, line));
        false
    };
    for (name, value) in binds {
        let Some(&(_, action)) = ACTION_NAMES.iter().find(|(n, _)| n == name) else {
            problems.push(format!("BIND_{name}: no such action"));
            continue;
        };
        out.moved.push(action);
        if value.eq_ignore_ascii_case("none") || value.is_empty() {
            continue;
        }
        for chord_text in value.split(',') {
            match parse_chord(chord_text) {
                Some(chord) => {
                    if !taken(
                        chord,
                        format!("BIND_{name}"),
                        &mut problems,
                        chord_text.trim(),
                    ) {
                        out.chords.push((chord, Some(action)));
                    }
                }
                None => problems.push(format!("BIND_{name}: \"{chord_text}\" is not a key chord")),
            }
        }
    }
    for (name, value) in execs {
        let Some((chord_text, command)) = value.split_once(':') else {
            problems.push(format!("{name}: expected \"CHORD:command\""));
            continue;
        };
        let (Some(chord), command) = (parse_chord(chord_text), command.trim()) else {
            problems.push(format!("{name}: \"{chord_text}\" is not a key chord"));
            continue;
        };
        if command.is_empty() {
            problems.push(format!("{name}: no command"));
            continue;
        }
        if commands.len() >= usize::from(u16::MAX) {
            problems.push(format!("{name}: too many EXEC_ commands"));
            continue;
        }
        if taken(chord, name.clone(), &mut problems, chord_text.trim()) {
            continue;
        }
        out.chords
            .push((chord, Some(KeyAction::Exec(commands.len() as u16))));
        commands.push(command.to_string());
    }
    (out, commands, problems)
}

/// The dmabuf formats offered when the backend names none: linear only, which any GPU can
/// import and scan out.
const DMABUF_FORMATS: [smithay::backend::allocator::Format; 2] = [
    smithay::backend::allocator::Format {
        code: smithay::backend::allocator::Fourcc::Argb8888,
        modifier: smithay::backend::allocator::Modifier::Linear,
    },
    smithay::backend::allocator::Format {
        code: smithay::backend::allocator::Fourcc::Xrgb8888,
        modifier: smithay::backend::allocator::Modifier::Linear,
    },
];

/// A DRM device for backends that do not name theirs: the first render node, else the first
/// card node.
/// The formats of a plane that clients may also allocate, in the clients' order.
fn scanout_tranche<F: PartialEq + Copy>(
    render: &[F],
    plane: impl IntoIterator<Item = F>,
) -> Vec<F> {
    let plane: Vec<F> = plane.into_iter().collect();
    render
        .iter()
        .copied()
        .filter(|f| plane.contains(f))
        .collect()
}

fn guess_dmabuf_device() -> Option<libc::dev_t> {
    use std::os::unix::fs::MetadataExt;
    let mut nodes: Vec<std::path::PathBuf> = std::fs::read_dir("/dev/dri")
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    nodes.sort();
    let pick = |prefix: &str| {
        nodes.iter().find_map(|p| {
            let name = p.file_name()?.to_string_lossy().into_owned();
            if name.starts_with(prefix) {
                std::fs::metadata(p).ok().map(|m| m.rdev() as libc::dev_t)
            } else {
                None
            }
        })
    };
    pick("renderD").or_else(|| pick("card"))
}

/// Parses a hex color `#RRGGBB` or `#RRGGBBAA` into a `Color32F`.
pub fn parse_color_hex(hex: &str) -> Option<smithay::backend::renderer::Color32F> {
    let s = hex
        .trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .trim_start_matches('#');
    if !s.is_ascii() {
        return None;
    }
    if s.len() == 6 {
        let r = u8::from_str_radix(&s[0..2], 16).ok()? as f32 / 255.0;
        let g = u8::from_str_radix(&s[2..4], 16).ok()? as f32 / 255.0;
        let b = u8::from_str_radix(&s[4..6], 16).ok()? as f32 / 255.0;
        Some(smithay::backend::renderer::Color32F::new(r, g, b, 1.0))
    } else if s.len() == 8 {
        let r = u8::from_str_radix(&s[0..2], 16).ok()? as f32 / 255.0;
        let g = u8::from_str_radix(&s[2..4], 16).ok()? as f32 / 255.0;
        let b = u8::from_str_radix(&s[4..6], 16).ok()? as f32 / 255.0;
        let a = u8::from_str_radix(&s[6..8], 16).ok()? as f32 / 255.0;
        Some(smithay::backend::renderer::Color32F::new(r, g, b, a))
    } else {
        None
    }
}

/// INV-ARCH-2: `unsafe` is denied here (lint `unsafe_code`).
#[cfg(feature = "probe_unsafe_block")]
pub fn probe_unsafe_block() {
    unsafe {}
}

#[cfg(test)]
mod key_binding_config_tests {
    use super::user_key_bindings;
    use infynitae_core::input::keys::{KeyAction, parse_chord};

    fn kv(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// BIND_ moves actions (several chords allowed) or frees them with "none"; EXEC_ adds
    /// commands in order; lines that cannot be understood are reported, not applied.
    #[test]
    fn config_lines_become_bindings_and_commands() {
        let (b, cmds, problems) = user_key_bindings(
            &kv(&[
                ("TERMINAL", "SUPER+RETURN,SUPER+T"),
                ("LAUNCHER", "none"),
                ("TELEPORT", "SUPER+X"),
                ("LOCK", "SUPER+NOPE"),
            ]),
            &kv(&[
                ("EXEC_BROWSER", "SUPER+B: firefox --new-window"),
                ("EXEC_BAD", "firefox"),
            ]),
        );
        assert_eq!(
            b.chords,
            vec![
                (
                    parse_chord("SUPER+RETURN").unwrap(),
                    Some(KeyAction::SpawnTerminal)
                ),
                (
                    parse_chord("SUPER+T").unwrap(),
                    Some(KeyAction::SpawnTerminal)
                ),
                (parse_chord("SUPER+B").unwrap(), Some(KeyAction::Exec(0))),
            ]
        );
        assert_eq!(
            b.moved,
            vec![
                KeyAction::SpawnTerminal,
                KeyAction::ToggleLauncher,
                KeyAction::SpawnLockScreen
            ]
        );
        assert_eq!(cmds, vec!["firefox --new-window".to_string()]);
        assert_eq!(problems.len(), 3, "{problems:?}");
    }

    /// A chord bound by two lines stays with the first, and the second is reported.
    #[test]
    fn a_chord_bound_twice_is_reported() {
        let (b, cmds, problems) = user_key_bindings(
            &kv(&[("CLOSE", "SUPER+Q")]),
            &kv(&[("EXEC_X", "SUPER+Q:foo"), ("EXEC_Y", "SUPER+Y:bar")]),
        );
        assert_eq!(
            b.chords
                .iter()
                .filter(|(c, _)| *c == parse_chord("SUPER+Q").unwrap())
                .count(),
            1
        );
        assert_eq!(cmds, vec!["bar".to_string()]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("EXEC_X") && problems[0].contains("BIND_CLOSE"));
    }

    /// Config lines are read as the shell reads them: quotes and `#` inside a value are kept,
    /// a trailing comment is not.
    #[test]
    fn config_values_are_read_as_shell_words() {
        use super::config_line;
        let v = |l: &str| config_line(l).map(|(_, v)| v);
        assert_eq!(
            v(r#"EXEC_NOTE="SUPER+N:sh -c 'notify-send hi'""#).as_deref(),
            Some("SUPER+N:sh -c 'notify-send hi'")
        );
        assert_eq!(
            v(r#"EXEC_WEB="SUPER+B:firefox https://x.org/#top"  # docs"#).as_deref(),
            Some("SUPER+B:firefox https://x.org/#top")
        );
        assert_eq!(v("PAN_MODE=rts # comment").as_deref(), Some("rts"));
        assert_eq!(v("BG='#1e1e2e'").as_deref(), Some("#1e1e2e"));
        assert_eq!(v(r#"X="a \"b\" c""#).as_deref(), Some(r#"a "b" c"#));
        assert_eq!(
            config_line("export KEY=1"),
            Some(("KEY".to_string(), "1".to_string()))
        );
        assert_eq!(config_line("# EXEC_X=\"SUPER+X:foo\""), None);
        assert_eq!(config_line("no equals"), None);
    }

    #[test]
    fn parse_color_hex_handles_non_ascii_and_malformed() {
        use super::parse_color_hex;
        assert!(parse_color_hex("aé123").is_none());
        assert!(parse_color_hex("#aé1234").is_none());
        assert!(parse_color_hex("#12345é").is_none());
        assert!(parse_color_hex("").is_none());
        assert!(parse_color_hex("invalid").is_none());

        let c = parse_color_hex("#ff0000").unwrap();
        assert_eq!((c.r(), c.g(), c.b(), c.a()), (1.0, 0.0, 0.0, 1.0));

        let c2 = parse_color_hex("#00ff0080").unwrap();
        assert_eq!((c2.r(), c2.g(), c2.b()), (0.0, 1.0, 0.0));
        assert!((c2.a() - 128.0 / 255.0).abs() < 1e-4);
    }
}

#[cfg(test)]
mod scanout_tranche_tests {
    /// Only what the renderer can also read is offered: a plane's format no client could be
    /// composited from would break the window the moment it leaves fullscreen.
    #[test]
    fn a_scanout_tranche_is_what_both_the_plane_and_the_renderer_take() {
        assert_eq!(super::scanout_tranche(&[1, 2, 3, 4], [9, 4, 2]), [2, 4]);
        assert!(super::scanout_tranche(&[1, 2], [3]).is_empty());
    }
}
