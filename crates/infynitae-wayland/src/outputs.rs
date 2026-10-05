//! Output globals, membership application, damage trackers and frame completion (INC-2).
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
}

impl WaylandState {
    /// Applies Core's effects in order (C1.2 `WaylandState::apply`).
    pub fn apply(&mut self, effects: &[Effect]) {
        for effect in effects {
            match *effect {
                Effect::InitialConfigure(id) => {
                    if let Some(t) = self.window(id).and_then(|w| w.toplevel()) {
                        t.send_configure();
                    }
                }
                Effect::ToplevelConfigure(id) => {
                    if let Some(t) = self.window(id).and_then(|w| w.toplevel()) {
                        let activated = self.core.focus().target() == Some(id);
                        t.with_pending_state(|s| {
                            if activated {
                                s.states.set(xdg_toplevel::State::Activated);
                            } else {
                                s.states.unset(xdg_toplevel::State::Activated);
                            }
                        });
                        t.send_configure();
                    }
                }
                Effect::KeyboardFocus(target) => {
                    let serial = SERIAL_COUNTER.next_serial();
                    let target_surface = target.and_then(|id| {
                        self.window(id)
                            .and_then(|w| w.toplevel().map(|t| t.wl_surface().clone()))
                    });
                    let kb = self.keyboard_handle();
                    self.popups.cleanup();
                    if let Some(mut g) = self.popup_grab.take()
                        && !g.has_ended()
                    {
                        if self.ordered_preemption_strategy == PreemptionStrategy::Naive {
                            // NEG-1: naive set_focus without ungrabbing popup
                            self.popup_grab = Some(g);
                        } else {
                            g.ungrab(PopupUngrabStrategy::All);
                            if self.ordered_preemption_strategy == PreemptionStrategy::PointerFirst
                            {
                                // NEG-2: pointer-first unset order (W-SM-15 violation)
                                if self.pointer_has_grab(g.serial()) {
                                    self.pointer_unset_grab(serial, 0);
                                }
                                if kb.has_grab(g.serial()) {
                                    self.keyboard_unset_grab();
                                }
                            } else {
                                // Canonical ORDERED (INV-INPUT-10):
                                // 1. PopupGrab::ungrab(All) (already done above)
                                // 2. keyboard.unset_grab
                                if kb.has_grab(g.serial()) {
                                    self.keyboard_unset_grab();
                                }
                                // 3. pointer.unset_grab (only if pointer has this grab)
                                if self.pointer_has_grab(g.serial()) {
                                    self.pointer_unset_grab(serial, 0);
                                }
                            }
                        }
                    }
                    kb.set_focus(self, target_surface, serial);
                }
                Effect::DismissPopupGrab(_) => {
                    self.popups.cleanup();
                    if let Some(mut g) = self.popup_grab.take()
                        && !g.has_ended()
                    {
                        g.ungrab(PopupUngrabStrategy::All);
                        let kb = self.keyboard_handle();
                        if kb.has_grab(g.serial()) {
                            self.keyboard_unset_grab();
                        }
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
                Effect::OutputAdded(id) => self.output_added(id),
                Effect::OutputChanged(id) => self.output_changed(id),
                Effect::OutputRemoved(id) => self.output_removed(id),
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
                ref pointer => {
                    let applied = self.apply_pointer(pointer);
                    debug_assert!(applied, "unhandled effect {pointer:?}");
                }
            }
        }
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
            tracker: convert::damage_tracker(&model, &view),
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
        h.tracker = convert::damage_tracker(&model, &view);
    }

    fn output_removed(&mut self, id: OutputId) {
        if let Some(i) = self.outputs.iter().position(|h| h.id == id) {
            let h = self.outputs.remove(i);
            self.dh.remove_global::<WaylandState>(h.global);
        }
    }

    /// A vblank of `output` at `now`: the requested frame callbacks of each element complete
    /// once, and its pending presentation feedback is presented with `sync_output` = `output`
    /// (INV-FRAME-1, INV-FRAME-2, INV-FRAME-5). `elements` comes from `Core::frame_tick`.
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
}
