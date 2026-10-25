//! Output globals, membership application, damage trackers and frame completion.
//!
//! Smithay `Output` state is written from Core's OutputModel, one way (C2.1); it is never read
//! back as policy input. Enter/leave are applied only from Core's membership effects
//! (INV-RENDER-3); `Window::output_enter`/`Space` are not used.

use std::time::Duration;

use infynitae_core::output::OutputId;
use infynitae_core::protocol::Effect;
use infynitae_core::stage::ElementId;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::desktop::PopupUngrabStrategy;
use smithay::output::{Output, PhysicalProperties, Subpixel};
use smithay::reexports::wayland_protocols::wp::presentation_time::server::wp_presentation_feedback;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::backend::GlobalId;
use smithay::utils::SERIAL_COUNTER;
use smithay::wayland::presentation::Refresh;

use crate::keyboard::PreemptionStrategy;
use crate::{WaylandState, convert};

/// The adapter's handles for one connected output.
pub(crate) struct OutputHandle {
    pub(crate) id: OutputId,
    pub(crate) output: Output,
    global: GlobalId,
    pub(crate) tracker: OutputDamageTracker,
    /// The `k_O` the tracker was built with: a frame at another k rebuilds it (C12.F15.4).
    pub(crate) tracker_k: f64,
}

impl WaylandState {
    /// Applies Core's effects in order (C1.2 `WaylandState::apply`).
    pub fn apply(&mut self, effects: &[Effect]) {
        self.prune_disabled_outputs();
        for effect in effects {
            self.kick_before(effect);
            match *effect {
                Effect::InitialConfigure(id) | Effect::ToplevelConfigure(id) => {
                    self.configure_toplevel(id, None);
                }
                Effect::ToplevelConfigureSize { element, size } => {
                    self.configure_toplevel(element, Some((size.w, size.h)));
                }
                Effect::CloseRequest(element) => {
                    if let Some(t) = self.window(element).and_then(|w| w.toplevel()) {
                        t.send_close();
                    }
                }
                Effect::ForceClose(element) => self.force_close(element),
                Effect::PreferredScale {
                    element,
                    scale_120,
                    integer_scale,
                } => {
                    if let Some(w) = self.window(element) {
                        convert::send_preferred_scale_to_window(w, scale_120, integer_scale);
                    }
                }
                Effect::KeyboardFocus(target) => {
                    let serial = SERIAL_COUNTER.next_serial();
                    let target_surface = target.and_then(|id| self.window_surface(id));
                    let kb = self.keyboard_handle();
                    self.popups.cleanup();
                    if let Some(mut g) = self.popup_grab.take()
                        && !g.has_ended()
                    {
                        if self.ordered_preemption_strategy == PreemptionStrategy::Naive {
                            // NEG-1: naive set_focus without ungrabbing popup
                            self.popup_grab = Some(g);
                        } else if self.ordered_preemption_strategy
                            == PreemptionStrategy::PointerFirst
                        {
                            g.ungrab(PopupUngrabStrategy::All);
                            // NEG-2: pointer-first unset order (W-SM-15 violation)
                            if let Some(pg_serial) = self.popup_pointer_grab_serial.take() {
                                if self.pointer_has_grab(pg_serial) {
                                    self.pointer_unset_grab(serial, 0);
                                }
                            } else if self.pointer_has_grab(g.serial())
                                || g.previous_serial()
                                    .is_some_and(|ps| self.pointer_has_grab(ps))
                            {
                                self.pointer_unset_grab(serial, 0);
                            }
                            if kb.has_grab(g.serial())
                                || g.previous_serial().is_some_and(|ps| kb.has_grab(ps))
                            {
                                self.keyboard_unset_grab();
                            }
                        } else {
                            // Canonical ORDERED (INV-INPUT-10):
                            // 1. PopupGrab::ungrab(All)
                            g.ungrab(PopupUngrabStrategy::All);
                            // 2. keyboard.unset_grab
                            if kb.has_grab(g.serial())
                                || g.previous_serial().is_some_and(|ps| kb.has_grab(ps))
                            {
                                self.keyboard_unset_grab();
                            }
                            // 3. pointer.unset_grab (only if pointer has this grab)
                            if let Some(pg_serial) = self.popup_pointer_grab_serial.take() {
                                if self.pointer_has_grab(pg_serial) {
                                    self.pointer_unset_grab(serial, 0);
                                }
                            } else if self.pointer_has_grab(g.serial())
                                || g.previous_serial()
                                    .is_some_and(|ps| self.pointer_has_grab(ps))
                            {
                                self.pointer_unset_grab(serial, 0);
                            }
                        }
                    }
                    // locked: the keyboard stays with the lock surface (finish_unlock restores)
                    if !self.is_locked() {
                        kb.set_focus(self, target_surface, serial);
                    }
                }
                Effect::DismissPopupGrab(target) => {
                    let serial = SERIAL_COUNTER.next_serial();
                    self.popups.cleanup();
                    if let Some(mut g) = self.popup_grab.take()
                        && !g.has_ended()
                    {
                        g.ungrab(PopupUngrabStrategy::All);
                        let kb = self.keyboard_handle();
                        if kb.has_grab(g.serial())
                            || g.previous_serial().is_some_and(|ps| kb.has_grab(ps))
                        {
                            self.keyboard_unset_grab();
                        }
                    }
                    if let Some(pg_serial) = self.popup_pointer_grab_serial.take() {
                        if self.pointer_has_grab(pg_serial) {
                            self.set_pan_dismissed_pointer_grab(serial);
                        }
                    } else if self.is_pointer_grabbed() {
                        self.set_pan_dismissed_pointer_grab(serial);
                    }
                    let target_surface = self.window_surface(target);
                    let kb = self.keyboard_handle();
                    if !self.is_locked() {
                        kb.set_focus(self, target_surface, serial);
                    }
                }
                Effect::KeyboardForward {
                    key,
                    pressed,
                    mods_changed,
                    time_us,
                } => {
                    use smithay::backend::input::KeyState;
                    use smithay::input::keyboard::Keycode;

                    let mods = self.seat_modifiers();
                    notify_portal_key(key, pressed, mods);

                    let kb = self.keyboard_handle();
                    let code = Keycode::new(key + 8);
                    let state = if pressed {
                        KeyState::Pressed
                    } else {
                        KeyState::Released
                    };
                    let time_ms = (time_us / 1000) as u32;
                    kb.input_forward(
                        self,
                        code,
                        state,
                        SERIAL_COUNTER.next_serial(),
                        time_ms,
                        mods_changed,
                    );
                }
                Effect::KeyboardIntercept { .. } => {}
                // an output change is news to the focused client: Core's true position after it
                // (M10) is sent even where it repeats the last motion (pointer.rs `Sent`)
                Effect::OutputAdded(id) => {
                    self.pointer_sent = Default::default();
                    self.output_added(id);
                }
                Effect::OutputChanged(id) => {
                    self.pointer_sent = Default::default();
                    self.output_changed(id);
                }
                Effect::OutputRemoved(id) => {
                    self.pointer_sent = Default::default();
                    self.output_removed(id);
                }
                Effect::Enter { output, surface } => {
                    if let (Some(h), Some(s)) =
                        (self.handle(output), self.registry.surface(surface))
                    {
                        h.output.enter(s);
                    }
                }
                Effect::Leave { output, surface } => {
                    // a surface no longer registered is dead: nothing is sent to it
                    if let (Some(h), Some(s)) =
                        (self.handle(output), self.registry.surface(surface))
                    {
                        h.output.leave(s);
                    }
                }
                Effect::ToggleLauncher => {
                    self.spawn_action("launcher");
                }
                Effect::SpawnTerminal => {
                    self.spawn_action("terminal");
                }
                Effect::SpawnFileManager => {
                    self.spawn_action("file-manager");
                }
                Effect::SpawnScreenshot => {
                    self.spawn_action("screenshot");
                }
                Effect::SpawnRegionScreenshot => {
                    self.spawn_action("region-screenshot");
                }
                Effect::ToggleOverview => {
                    self.spawn_action("overview");
                }
                Effect::ReloadShell => {
                    self.reload_config();
                    self.spawn_action("reload");
                }
                Effect::SpawnLockScreen => {
                    self.spawn_action("lock-screen");
                }
                Effect::SpawnClipboardHistory => {
                    self.spawn_action("clipboard-history");
                }
                Effect::SpawnControlMenu => {
                    self.spawn_action("control-menu");
                }
                Effect::Exec(i) => {
                    if let Some(command) = self.exec_commands.get(usize::from(i)).cloned() {
                        self.spawn_action_with("exec", &command);
                    }
                }
                Effect::TogglePrivacy => {
                    self.toggle_privacy_focused();
                }
                Effect::ToggleIdleInhibit => {
                    self.toggle_idle_inhibit();
                }
                Effect::SaveReplay => {
                    self.spawn_action("save-replay");
                }
                Effect::ToggleRecording => {
                    self.spawn_action("record-toggle");
                }
                Effect::VolumeUp => {
                    self.spawn_action("volume-up");
                }
                Effect::VolumeDown => {
                    self.spawn_action("volume-down");
                }
                Effect::VolumeMute => {
                    self.spawn_action("volume-mute");
                }
                Effect::BrightnessUp => {
                    self.spawn_action("brightness-up");
                }
                Effect::BrightnessDown => {
                    self.spawn_action("brightness-down");
                }
                Effect::MediaPlayPause => {
                    self.spawn_action("media-play-pause");
                }
                Effect::MediaNext => {
                    self.spawn_action("media-next");
                }
                Effect::MediaPrev => {
                    self.spawn_action("media-prev");
                }
                ref other => {
                    let applied = self.apply_layer(other) || self.apply_pointer(other);
                    debug_assert!(applied, "unhandled effect {other:?}");
                }
            }
        }
        self.drop_constraints_smithay_destroyed();
    }

    /// Spawns a desktop action via ~/infynitae/live/launch.sh <action>
    pub(crate) fn spawn_action(&mut self, action: &str) {
        self.spawn_action_with(action, "");
    }

    /// [`spawn_action`](Self::spawn_action) with an argument (`exec`: the command to run).
    pub(crate) fn spawn_action_with(&mut self, action: &str, arg: &str) {
        let home = std::env::var("HOME").unwrap_or_default();
        let script = format!("{home}/infynitae/live/launch.sh");
        let mut cmd = std::process::Command::new("sh");
        cmd.arg(script).arg(action);
        if !arg.is_empty() {
            cmd.arg(arg);
        }
        if let Ok(d) = std::env::var("DISPLAY") {
            cmd.env("DISPLAY", d);
        }
        if let Ok(a) = std::env::var("XAUTHORITY") {
            cmd.env("XAUTHORITY", a);
        }
        cmd.env_remove("KDE_FULL_SESSION")
            .env_remove("KDE_SESSION_VERSION")
            .env_remove("KDE_SESSION_UID")
            .env_remove("KDE_APPLICATIONS_AS_SCOPE")
            .env_remove("XDG_MENU_PREFIX");
        // the shortcuts that open apps are launches (`launch`): their windows are the user's
        if matches!(
            action,
            "terminal"
                | "launcher"
                | "file-manager"
                | "exec"
                | "browser"
                | "overview"
                | "clipboard-history"
                | "control-menu"
        ) {
            let _ = self.launches.spawn(&mut cmd);
        } else {
            let _ = cmd.spawn();
        }
        // their exits (and their orphans') are reaped from now on (`Session::dispatch`)
        self.spawned_children = true;
    }

    /// Sends one `xdg_toplevel.configure` whose states and size are Core's at this moment:
    /// `activated` from the focus (INV-PROTO-1), `fullscreen` and the fullscreen size from
    /// OBJ-19 (INV-PRES-1). `offered` is a one-time size offer for a toplevel that is not
    /// fullscreen (the size saved when fullscreen was granted). Every field is written on every
    /// configure, so nothing of an earlier one survives in Smithay's pending state: the adapter
    /// holds no fullscreen state of its own.
    fn configure_toplevel(&mut self, id: ElementId, offered: Option<(i32, i32)>) {
        let Some(t) = self.window(id).and_then(|w| w.toplevel()) else {
            return;
        };
        let activated = self.core.focus().target() == Some(id);
        let state = self.core.toplevel_configure_state(id);
        let size = state.size.map(|s| (s.w, s.h)).or(offered);
        let set = |states: &mut smithay::wayland::shell::xdg::ToplevelStateSet, flag, on: bool| {
            if on {
                states.set(flag);
            } else {
                states.unset(flag);
            }
        };
        t.with_pending_state(|s| {
            set(&mut s.states, xdg_toplevel::State::Activated, activated);
            set(
                &mut s.states,
                xdg_toplevel::State::Fullscreen,
                state.fullscreen.is_some(),
            );
            set(&mut s.states, xdg_toplevel::State::Resizing, state.resizing);
            s.size = size.map(Into::into);
            s.fullscreen_output = None;
        });
        t.send_configure();
    }

    /// The Core output a client's `wl_output` object stands for.
    pub(crate) fn output_id_of(
        &self,
        wl_output: &smithay::reexports::wayland_server::protocol::wl_output::WlOutput,
    ) -> Option<OutputId> {
        let output = Output::from_resource(wl_output)?;
        self.outputs
            .iter()
            .find(|h| h.output == output)
            .map(|h| h.id)
    }

    /// The Smithay output of a connected output (for render oracles and backends).
    pub fn smithay_output(&self, id: OutputId) -> Option<&Output> {
        self.handle(id).map(|h| &h.output)
    }

    fn handle(&self, id: OutputId) -> Option<&OutputHandle> {
        self.outputs.iter().find(|h| h.id == id)
    }

    /// `wl_output.name` must be unique among the outputs: the connector, suffixed on collision.
    fn unique_name(&self, connector: &str) -> String {
        let taken = |n: &str| self.outputs.iter().any(|h| h.output.name() == n);
        if !taken(connector) {
            return connector.to_string();
        }
        (2..)
            .map(|i| format!("{connector}-{i}"))
            .find(|n| !taken(n))
            .expect("unbounded")
    }

    fn output_added(&mut self, id: OutputId) {
        let Some(model) = self.core.output(id).cloned() else {
            return;
        };
        let Some(view) = self.core.view(id) else {
            return;
        };
        let output = Output::new(
            self.unique_name(&model.key.connector),
            PhysicalProperties {
                size: convert::unknown_physical_size(),
                subpixel: Subpixel::Unknown,
                make: model.key.make.clone(),
                model: model.key.model.clone(),
            },
        );
        let mode = convert::output_mode(&model);
        output.change_current_state(
            Some(mode),
            Some(convert::output_transform(model.transform)),
            Some(convert::output_scale(&model)),
            Some(convert::output_location(&model)),
        );
        output.set_preferred(mode);
        let global = output.create_global::<WaylandState>(&self.dh);
        self.outputs.push(OutputHandle {
            id,
            output,
            global,
            tracker: convert::damage_tracker(&model, &view, self.target),
            tracker_k: view.k,
        });
    }

    fn output_changed(&mut self, id: OutputId) {
        let (Some(model), Some(view)) = (self.core.output(id).cloned(), self.core.view(id)) else {
            return;
        };
        let Some(h) = self.outputs.iter_mut().find(|h| h.id == id) else {
            return;
        };
        let mode = convert::output_mode(&model);
        h.output.change_current_state(
            Some(mode),
            Some(convert::output_transform(model.transform)),
            Some(convert::output_scale(&model)),
            Some(convert::output_location(&model)),
        );
        h.output.set_preferred(mode);
        // F-RENDER-1: the tracker is rebuilt when the descriptor or k changes
        h.tracker = convert::damage_tracker(&model, &view, self.target);
        h.tracker_k = view.k;
    }

    fn output_removed(&mut self, id: OutputId) {
        if let Some(i) = self.outputs.iter().position(|h| h.id == id) {
            let h = self.outputs.remove(i);
            self.screencopy.fail_pending_output(id);
            self.dh.disable_global::<WaylandState>(h.global.clone());
            self.disabled_outputs
                .push((h.global, std::time::Instant::now() + Duration::from_secs(5)));
        }
    }

    /// A vblank of `output` at `now`: the requested frame callbacks of each element complete
    /// once, and its pending presentation feedback is presented with `sync_output` = `output`
    /// (INV-FRAME-1, INV-FRAME-2, INV-FRAME-5). `elements` comes from `Core::frame_tick_into`.
    pub fn complete_frames(&mut self, output: OutputId, elements: &[ElementId], now: Duration) {
        let Some(h) = self.handle(output) else { return };
        let (o, refresh) = (
            h.output.clone(),
            self.core.output(output).map(|m| m.refresh_mhz),
        );
        let period = refresh
            .filter(|r| *r > 0)
            .map(|r| Refresh::Fixed(Duration::from_nanos(1_000_000_000_000 / r as u64)))
            .unwrap_or(Refresh::Unknown);
        for id in elements {
            let Some(window) = self.window(*id) else {
                continue;
            };
            let mut feedback = smithay::desktop::utils::OutputPresentationFeedback::new(&o);
            window.take_presentation_feedback(
                &mut feedback,
                |_, _| Some(o.clone()),
                |_, _| wp_presentation_feedback::Kind::empty(),
            );
            feedback.presented::<_, smithay::utils::Monotonic>(
                now,
                period,
                0,
                wp_presentation_feedback::Kind::empty(),
            );
            window.send_frame(&o, now, None, |_, _| Some(o.clone()));
        }
    }

    pub fn output_id_from_wl(
        &self,
        wl: &smithay::reexports::wayland_server::protocol::wl_output::WlOutput,
    ) -> Option<OutputId> {
        self.outputs
            .iter()
            .find(|h| h.output.owns(wl))
            .map(|h| h.id)
    }

    pub fn output_mode_size(&self, id: OutputId) -> Option<(i32, i32)> {
        self.core
            .output(id)
            .map(|m| (m.phys.0 as i32, m.phys.1 as i32))
    }
}

/// Notifies the GlobalShortcuts portal backend if the key is bound by an application (Discord, etc.)
pub(crate) fn notify_portal_key(
    key: u32,
    pressed: bool,
    mods: infynitae_core::input::bindings::Modifiers,
) {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};

    // Cache of bound keys: checked in memory, refreshed at most once per second if file exists
    static LAST_CHECK: AtomicU64 = AtomicU64::new(0);
    static BOUND_KEYS: Mutex<Option<std::collections::HashSet<u32>>> = Mutex::new(None);

    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let bound_keys_path = format!("{runtime}/infynitae/bound_keys");

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let last = LAST_CHECK.load(Ordering::Relaxed);

    if now != last {
        LAST_CHECK.store(now, Ordering::Relaxed);
        if let Ok(content) = std::fs::read_to_string(&bound_keys_path) {
            let mut set = std::collections::HashSet::new();
            for line in content.lines() {
                if let Ok(k) = line.trim().parse::<u32>() {
                    set.insert(k);
                }
            }
            if let Ok(mut lock) = BOUND_KEYS.lock() {
                *lock = Some(set);
            }
        }
    }

    let is_bound = BOUND_KEYS
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|s| s.contains(&key)))
        .unwrap_or(false);

    if is_bound {
        static SENDER: std::sync::OnceLock<std::sync::mpsc::SyncSender<(u32, bool, u32, String)>> =
            std::sync::OnceLock::new();
        let sender = SENDER.get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::sync_channel::<(u32, bool, u32, String)>(128);
            std::thread::Builder::new()
                .name("portal-notify".into())
                .spawn(move || {
                    use std::io::Write;
                    use std::os::unix::net::UnixStream;
                    while let Ok((k, p, m, sock_path)) = rx.recv() {
                        if let Ok(mut stream) = UnixStream::connect(&sock_path) {
                            let _ = stream.set_nonblocking(true);
                            let msg = format!("key:{k}:{}:{m}\n", if p { 1 } else { 0 });
                            let _ = stream.write_all(msg.as_bytes());
                        }
                    }
                })
                .expect("spawn portal notifier thread");
            tx
        });
        let mut mods_mask: u32 = 0;
        if mods.shift {
            mods_mask |= 1 << 0;
        }
        if mods.ctrl {
            mods_mask |= 1 << 1;
        }
        if mods.alt {
            mods_mask |= 1 << 2;
        }
        if mods.logo {
            mods_mask |= 1 << 3;
        }
        let sock_path = format!("{runtime}/infynitae/shortcuts.sock");
        let _ = sender.try_send((key, pressed, mods_mask, sock_path));
    }
}
