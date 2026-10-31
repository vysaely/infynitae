//! Virtual input as a real device: `zwlr_virtual_pointer_v1` (the remote desktop portal,
//! wayvnc, wlrctl) and virtual keyboards that type with the seat's own keymap
//! (`zwp_virtual_keyboard_v1`, in `input_method`). Their events are queued here
//! ([`WaylandState::take_virtual_input`]) and each backend hands them to Core through its
//! InputPort, like a mouse or a keyboard plugged in: the shortcuts and the focus rules apply to
//! them as to any other. While locked, pointer events go nowhere (as a real pointer's do) and a
//! virtual keyboard types nothing (owner, 2026-10-25: only your own keys reach the lock screen);
//! a key it held is still released.
//!
//! A pointer's absolute motion lands on its output (`create_virtual_pointer_with_output`), else
//! on the box around all of them, as wlroots maps it; Core takes it as the motion from where the
//! pointer is. Scrolling is gathered until `frame`. Buttons and keys still held when the device
//! goes away are released.

use std::collections::HashMap;
use std::sync::Mutex;

use infynitae_core::PhysicalEvent;
use infynitae_core::input::{Axis, AxisSource};
use infynitae_core::output::OutputId;
use smithay::reexports::wayland_server::backend::{ClientId, GlobalId, ObjectId};
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, WEnum,
};
use wayland_protocols_wlr::virtual_pointer::v1::server::{
    zwlr_virtual_pointer_manager_v1::{self, ZwlrVirtualPointerManagerV1},
    zwlr_virtual_pointer_v1::{self, ZwlrVirtualPointerV1},
};

use crate::WaylandState;

/// One virtual device's event, for the backend to hand to Core.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VirtualInput {
    /// Relative motion, logical px.
    Motion {
        delta: (f64, f64),
    },
    /// Absolute motion: `at` in 0..=1 of the output (or of the box around all outputs).
    Absolute {
        output: Option<OutputId>,
        at: (f64, f64),
    },
    Button {
        button: u32,
        pressed: bool,
    },
    Axis(Axis),
    /// An evdev key of a virtual keyboard with the seat's keymap.
    Key {
        key: u32,
        pressed: bool,
    },
}

pub struct VirtualInputState {
    #[expect(
        dead_code,
        reason = "owns the zwlr_virtual_pointer_manager_v1 global for the state's lifetime"
    )]
    global: GlobalId,
    pub(crate) queue: Vec<VirtualInput>,
    /// The seat's keymap as text: a virtual keyboard uploading the same one is a real keyboard.
    pub(crate) seat_keymap: Option<String>,
    /// The virtual keyboards with the seat's keymap, and the keys each holds.
    pub(crate) keyboards: HashMap<ObjectId, Vec<u32>>,
}

impl VirtualInputState {
    pub fn new(dh: &DisplayHandle, seat_keymap: Option<String>, gate: crate::trust::Gate) -> Self {
        Self {
            global: dh
                .create_global::<WaylandState, ZwlrVirtualPointerManagerV1, crate::trust::Gate>(
                    2, gate,
                ),
            queue: Vec::new(),
            seat_keymap,
            keyboards: HashMap::new(),
        }
    }

    /// A virtual keyboard uploaded `keymap` (its text as received): whether it is the seat's, so
    /// its keys are a real keyboard's from now on.
    pub(crate) fn keyboard_keymap(&mut self, keyboard: ObjectId, keymap: &[u8]) -> bool {
        let seat = self
            .seat_keymap
            .as_deref()
            .is_some_and(|seat| is_seat_keymap(seat, keymap));
        if seat {
            self.keyboards.entry(keyboard).or_default();
        } else {
            self.release_keyboard(&keyboard);
        }
        seat
    }

    /// A key of a virtual keyboard: `true` if it is a real keyboard's (queued for Core, or
    /// dropped while locked unless it releases a held key); `false` for smithay's path.
    pub(crate) fn keyboard_key(
        &mut self,
        keyboard: &ObjectId,
        key: u32,
        pressed: bool,
        locked: bool,
    ) -> bool {
        let Some(held) = self.keyboards.get_mut(keyboard) else {
            return false;
        };
        let was_held = held.contains(&key);
        held.retain(|k| *k != key);
        if pressed && !locked {
            held.push(key);
        }
        if (pressed && !locked) || (!pressed && was_held) {
            self.queue.push(VirtualInput::Key { key, pressed });
        }
        true
    }

    /// Whether `keyboard` is a real keyboard (its modifiers come from its keys, not its requests).
    pub(crate) fn is_seat_keyboard(&self, keyboard: &ObjectId) -> bool {
        self.keyboards.contains_key(keyboard)
    }

    /// The keyboard went away, or stopped using the seat's keymap: its held keys are released.
    pub(crate) fn release_keyboard(&mut self, keyboard: &ObjectId) {
        for key in self.keyboards.remove(keyboard).unwrap_or_default() {
            self.queue.push(VirtualInput::Key {
                key,
                pressed: false,
            });
        }
    }
}

/// The layout and options the seat's keyboard uses, once chosen (`usable_keymap_names`): every
/// client the compositor starts gets them (`apply_keymap_env`), so apps that build their own
/// keymap from `XKB_DEFAULT_*` (kitty) never differ from the seat's.
static SEAT_KEYMAP_NAMES: std::sync::OnceLock<(String, String)> = std::sync::OnceLock::new();

/// The configured layout and options when they make a keymap; else the layout alone; else `us`,
/// with a warning. An empty layout (`XKB_DEFAULT_LAYOUT=""`, a console keymap name such as
/// `de-latin1` taken for one) would make none: the seat would have no keyboard, and an app given
/// it types nothing (seen 2026-10-26 on a ThinkPad).
pub fn usable_keymap_names(layout: &str, options: &str) -> (String, String) {
    use smithay::input::keyboard::xkb;
    let compiles = |layout: &str, options: &str| {
        let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let options = (!options.is_empty()).then(|| options.to_string());
        xkb::Keymap::new_from_names(
            &ctx,
            "evdev",
            "",
            layout,
            "",
            options,
            xkb::COMPILE_NO_FLAGS,
        )
        .is_some()
    };
    let (layout, options) = (layout.trim(), options.trim());
    if !layout.is_empty() && compiles(layout, options) {
        return (layout.to_string(), options.to_string());
    }
    if !layout.is_empty() && !options.is_empty() && compiles(layout, "") {
        eprintln!(
            "[infynitae] Keyboard options {options:?} make no keymap: layout {layout:?} without them"
        );
        return (layout.to_string(), String::new());
    }
    eprintln!(
        "[infynitae] Keyboard layout {layout:?} makes no keymap: using \"us\" (set XKB_DEFAULT_LAYOUT)"
    );
    ("us".to_string(), String::new())
}

/// Records the seat's layout and options for the clients started from now on (once).
pub(crate) fn set_seat_keymap_names(layout: &str, options: &str) {
    let _ = SEAT_KEYMAP_NAMES.set((layout.to_string(), options.to_string()));
}

/// Gives `command` the seat's layout and options: `XKB_DEFAULT_LAYOUT`/`_OPTIONS` for the apps,
/// and `INFYNITAE_XKB_LAYOUT`/`_OPTIONS`, which the session scripts keep over the config's
/// (`live/config.sh`). Nothing before the seat's keyboard exists.
pub(crate) fn apply_keymap_env(command: &mut std::process::Command) {
    let Some((layout, options)) = SEAT_KEYMAP_NAMES.get() else {
        return;
    };
    command
        .env("XKB_DEFAULT_LAYOUT", layout)
        .env("INFYNITAE_XKB_LAYOUT", layout)
        .env("INFYNITAE_XKB_OPTIONS", options);
    if options.is_empty() {
        command.env_remove("XKB_DEFAULT_OPTIONS");
    } else {
        command.env("XKB_DEFAULT_OPTIONS", options);
    }
}

/// The text of the keymap `layout` and `options` compile to: what the seat's keyboard sends.
pub(crate) fn keymap_text(layout: &str, options: Option<String>) -> Option<String> {
    use smithay::input::keyboard::xkb;
    let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
    let keymap =
        xkb::Keymap::new_from_names(&ctx, "", "", layout, "", options, xkb::COMPILE_NO_FLAGS)?;
    Some(keymap.get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1))
}

/// Whether the keymap a client uploaded (`text`, as received) is the seat's.
pub(crate) fn is_seat_keymap(seat: &str, text: &[u8]) -> bool {
    use smithay::input::keyboard::xkb;
    let end = text.iter().position(|b| *b == 0).unwrap_or(text.len());
    let Ok(text) = std::str::from_utf8(&text[..end]) else {
        return false;
    };
    let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
    xkb::Keymap::new_from_string(
        &ctx,
        text.to_string(),
        xkb::KEYMAP_FORMAT_TEXT_V1,
        xkb::COMPILE_NO_FLAGS,
    )
    .is_some_and(|k| k.get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1) == seat)
}

impl WaylandState {
    /// The virtual devices' events since the last call, in order.
    pub fn take_virtual_input(&mut self) -> Vec<VirtualInput> {
        std::mem::take(&mut self.virtual_input.queue)
    }

    /// A virtual pointer's event as Core's physical input at `time_us`; `None` for a key (the
    /// backend sends it with the seat's key path), while locked, or with no screen to land on.
    pub fn virtual_pointer_event(&self, ev: VirtualInput, time_us: u64) -> Option<PhysicalEvent> {
        if self.is_locked() {
            return None;
        }
        Some(match ev {
            VirtualInput::Motion { delta } => PhysicalEvent::PointerMotion {
                delta,
                delta_unaccel: delta,
                time_us,
            },
            VirtualInput::Absolute { output, at } => {
                let delta = self.absolute_delta(output, at)?;
                PhysicalEvent::PointerMotion {
                    delta,
                    delta_unaccel: delta,
                    time_us,
                }
            }
            VirtualInput::Button { button, pressed } => PhysicalEvent::PointerButton {
                button,
                pressed,
                modifiers: self.seat_modifiers(),
                time_us,
            },
            VirtualInput::Axis(axis) => PhysicalEvent::PointerAxis { axis, time_us },
            VirtualInput::Key { .. } => return None,
        })
    }

    /// The motion from the pointer to `at` of `output` (or of the box around all outputs), in
    /// the arrangement's logical px.
    fn absolute_delta(&self, output: Option<OutputId>, at: (f64, f64)) -> Option<(f64, f64)> {
        let core = self.core();
        let rect = |o: &infynitae_core::output::OutputModel| {
            let (w, h) = o.logical_extent();
            (o.arrangement.0, o.arrangement.1, w, h)
        };
        let (x, y, w, h) = match output {
            Some(id) => rect(core.output(id)?),
            None => core.outputs().iter().map(rect).reduce(|a, b| {
                let (x0, y0) = (a.0.min(b.0), a.1.min(b.1));
                let (x1, y1) = ((a.0 + a.2).max(b.0 + b.2), (a.1 + a.3).max(b.1 + b.3));
                (x0, y0, x1 - x0, y1 - y0)
            })?,
        };
        let target = (
            x + (at.0.clamp(0.0, 1.0) * w).min(w - 1.0),
            y + (at.1.clamp(0.0, 1.0) * h).min(h - 1.0),
        );
        let now = match core.pointer() {
            infynitae_core::pointer::PointerState::Located { output, p } => {
                let o = core.output(output)?;
                (o.arrangement.0 + p.0, o.arrangement.1 + p.1)
            }
            infynitae_core::pointer::PointerState::NoOutput => return None,
        };
        Some((target.0 - now.0, target.1 - now.1))
    }
}

/// A virtual pointer: its output, the scroll gathered until `frame`, the buttons it holds.
pub struct VirtualPointerData {
    output: Option<OutputId>,
    inner: Mutex<PointerInner>,
}

#[derive(Default)]
struct PointerInner {
    axis: Option<Axis>,
    held: Vec<u32>,
}

impl GlobalDispatch<ZwlrVirtualPointerManagerV1, crate::trust::Gate> for WaylandState {
    fn can_view(client: Client, gate: &crate::trust::Gate) -> bool {
        gate.allows(&client)
    }
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZwlrVirtualPointerManagerV1>,
        _global_data: &crate::trust::Gate,
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZwlrVirtualPointerManagerV1, ()> for WaylandState {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &ZwlrVirtualPointerManagerV1,
        request: zwlr_virtual_pointer_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        use zwlr_virtual_pointer_manager_v1::Request;
        let (id, output) = match request {
            Request::CreateVirtualPointer { id, .. } => (id, None),
            Request::CreateVirtualPointerWithOutput { id, output, .. } => {
                (id, output.and_then(|o| state.output_id_from_wl(&o)))
            }
            Request::Destroy => return,
            _ => return,
        };
        data_init.init(
            id,
            VirtualPointerData {
                output,
                inner: Mutex::new(PointerInner::default()),
            },
        );
    }
}

impl Dispatch<ZwlrVirtualPointerV1, VirtualPointerData> for WaylandState {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &ZwlrVirtualPointerV1,
        request: zwlr_virtual_pointer_v1::Request,
        data: &VirtualPointerData,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        use zwlr_virtual_pointer_v1::Request;
        let queue = &mut state.virtual_input.queue;
        let mut inner = data.inner.lock().unwrap();
        let axis_of = |inner: &mut PointerInner| {
            *inner.axis.get_or_insert(Axis {
                source: AxisSource::Continuous,
                amount: (0.0, 0.0),
                v120: (0, 0),
            })
        };
        let vertical =
            |a: &WEnum<smithay::reexports::wayland_server::protocol::wl_pointer::Axis>| {
                matches!(
                a,
                WEnum::Value(
                    smithay::reexports::wayland_server::protocol::wl_pointer::Axis::VerticalScroll
                )
            )
            };
        match request {
            Request::Motion { dx, dy, .. } => queue.push(VirtualInput::Motion { delta: (dx, dy) }),
            Request::MotionAbsolute {
                x,
                y,
                x_extent,
                y_extent,
                ..
            } => {
                if x_extent > 0 && y_extent > 0 {
                    queue.push(VirtualInput::Absolute {
                        output: data.output,
                        at: (
                            f64::from(x) / f64::from(x_extent),
                            f64::from(y) / f64::from(y_extent),
                        ),
                    });
                }
            }
            Request::Button {
                button,
                state: pressed,
                ..
            } => {
                let pressed = matches!(
                    pressed,
                    WEnum::Value(smithay::reexports::wayland_server::protocol::wl_pointer::ButtonState::Pressed)
                );
                inner.held.retain(|b| *b != button);
                if pressed {
                    inner.held.push(button);
                }
                queue.push(VirtualInput::Button { button, pressed });
            }
            Request::Axis { axis, value, .. } => {
                let mut a = axis_of(&mut inner);
                if vertical(&axis) {
                    a.amount.1 += value;
                } else {
                    a.amount.0 += value;
                }
                inner.axis = Some(a);
            }
            Request::AxisDiscrete {
                axis,
                value,
                discrete,
                ..
            } => {
                let mut a = axis_of(&mut inner);
                a.source = AxisSource::Wheel;
                if vertical(&axis) {
                    a.amount.1 += value;
                    a.v120.1 += discrete * 120;
                } else {
                    a.amount.0 += value;
                    a.v120.0 += discrete * 120;
                }
                inner.axis = Some(a);
            }
            Request::AxisSource { axis_source } => {
                let mut a = axis_of(&mut inner);
                a.source = match axis_source {
                    WEnum::Value(
                        smithay::reexports::wayland_server::protocol::wl_pointer::AxisSource::Wheel,
                    ) => AxisSource::Wheel,
                    _ => AxisSource::Continuous,
                };
                inner.axis = Some(a);
            }
            Request::Frame => {
                if let Some(a) = inner.axis.take()
                    && (a.amount != (0.0, 0.0) || a.v120 != (0, 0))
                {
                    queue.push(VirtualInput::Axis(a));
                }
            }
            Request::AxisStop { .. } | Request::Destroy => {}
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: ClientId,
        _resource: &ZwlrVirtualPointerV1,
        data: &VirtualPointerData,
    ) {
        let held = std::mem::take(&mut data.inner.lock().unwrap().held);
        for button in held {
            state.virtual_input.queue.push(VirtualInput::Button {
                button,
                pressed: false,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seat_keymap_is_recognised_and_another_is_not() {
        let us = keymap_text("us", None).expect("us compiles");
        let mut sent = us.clone().into_bytes();
        sent.push(0);
        assert!(
            is_seat_keymap(&us, &sent),
            "as a client sends it back, NUL-terminated"
        );
        let de = keymap_text("de", None).expect("de compiles");
        assert!(!is_seat_keymap(&us, de.as_bytes()));
        assert!(!is_seat_keymap(&us, b"not a keymap"));
    }
}

#[cfg(test)]
mod keymap_names_tests {
    use super::usable_keymap_names;

    #[test]
    fn a_layout_that_makes_no_keymap_is_us_and_a_good_one_is_kept() {
        assert_eq!(
            usable_keymap_names("", ""),
            ("us".into(), String::new()),
            "empty"
        );
        assert_eq!(
            usable_keymap_names("de-latin1", ""),
            ("us".into(), String::new()),
            "a console keymap name"
        );
        assert_eq!(
            usable_keymap_names(" us,ru ", "grp:alt_shift_toggle"),
            ("us,ru".into(), "grp:alt_shift_toggle".into()),
            "a real one, kept"
        );
    }
}
