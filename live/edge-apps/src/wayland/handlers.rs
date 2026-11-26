//! Wayland events: compositor, outputs, layer surfaces, seat, keyboard, pointer, touch, fractional scale and the toplevel list.

use super::*;

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        let factor = new_factor.max(1);
        let mut need_sync = false;
        if let Some(os) = self
            .outputs
            .iter_mut()
            .find(|o| o.layer.wl_surface() == surface)
        {
            if os.integer_scale != factor {
                os.integer_scale = factor;
                if os.viewport.is_none() {
                    os.mapped_closed = false;
                    if self.shell.visible_on(os.id) {
                        os.dirty = true;
                        need_sync = true;
                    } else {
                        os.apply_mode(qh, &self.compositor, &self.shell);
                    }
                }
            }
        }
        let mut osd_changed = false;
        for pl in self
            .popups
            .iter_mut()
            .chain(self.widget_layer.iter_mut())
            .chain(self.osd_layer.iter_mut())
            .filter(|p| p.layer.wl_surface() == surface)
        {
            if pl.integer_scale != factor {
                pl.integer_scale = factor;
                if pl.viewport.is_none()
                    && pl.render(qh, &self.compositor, &self.shell)
                    && pl.kind == SurfaceKind::Osd
                {
                    osd_changed = true;
                }
            }
        }
        if osd_changed {
            self.dump_osd_boxes();
        }
        if need_sync {
            self.sync_all();
        }
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        let is_output_surface = self.outputs.iter().any(|o| o.layer.wl_surface() == surface);
        if is_output_surface {
            // an animation running (arrival, leaving, a list opening, drag autoscroll): draw again
            let was_visible = self.shell.visible();
            let animating =
                was_visible && (self.shell.tick(Instant::now()) || self.shell.wants_frame());
            let ended = self.was_animating && !animating;
            let closed_finished = was_visible && !self.shell.visible();
            self.was_animating = animating;
            if let Some(os) = self
                .outputs
                .iter_mut()
                .find(|o| o.layer.wl_surface() == surface)
            {
                os.frame_pending = false;
                if animating && self.shell.visible_on(os.id) {
                    os.dirty = true;
                }
                if os.dirty && self.shell.visible_on(os.id) {
                    os.render_if_needed(qh, &self.shell);
                } else if os.clear_pending && !self.shell.visible_on(os.id) {
                    os.clear_and_commit(qh, &self.shell);
                } else if os.release_pending && !self.shell.visible_on(os.id) {
                    os.release_shown_buffer();
                }
            }
            if ended || closed_finished {
                // the panel's size settled or its leaving fade ended: input region / clear follows
                self.sync_all();
            }
        }
        if let Some(pl) = self
            .popups
            .as_mut()
            .filter(|p| p.layer.wl_surface() == surface)
        {
            pl.frame_pending = false;
            pl.render(qh, &self.compositor, &self.shell);
        }
        if let Some(ol) = self
            .osd_layer
            .as_mut()
            .filter(|p| p.layer.wl_surface() == surface)
        {
            ol.frame_pending = false;
            if ol.again && ol.render(qh, &self.compositor, &self.shell) {
                self.dump_osd_boxes();
            }
        }
        if self
            .widget_layer
            .as_ref()
            .is_some_and(|l| l.layer.wl_surface() == surface)
        {
            // drawn again only while a card's animation runs (or a draw waited for this frame)
            let running = self.shell.widgets_tick(Instant::now());
            if let Some(wl) = self.widget_layer.as_mut() {
                wl.frame_pending = false;
                if running || wl.again {
                    wl.render(qh, &self.compositor, &self.shell);
                }
            }
        }
    }
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, qh: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.create_output_surface(output.clone());
        self.sync_output_scale(&output, qh);
        // the first screen plugged in: the widgets on it at once
        self.sync_widgets();
    }
    fn update_output(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.sync_output_scale(&output, qh);
    }
    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.remove_output_matching(|o| o.output == output);
        if self.popups.as_ref().is_some_and(|p| p.output == output) {
            // the next pop-up makes the strip again, on the first screen left
            self.popups = None;
            self.popup_touch = None;
        }
        if self.osd_layer.as_ref().is_some_and(|o| o.output == output) {
            self.osd_layer = None;
        }
        if self
            .widget_layer
            .as_ref()
            .is_some_and(|w| w.output == output)
        {
            // the widgets go to the first screen left
            self.widget_layer = None;
            self.widget_touch = None;
            self.sync_widgets();
        }
    }
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        self.remove_output_matching(|o| &o.layer == layer);
        if self.popups.as_ref().is_some_and(|p| &p.layer == layer) {
            self.popups = None;
            self.popup_touch = None;
        }
        if self.osd_layer.as_ref().is_some_and(|o| &o.layer == layer) {
            self.osd_layer = None;
        }
        if self
            .widget_layer
            .as_ref()
            .is_some_and(|w| &w.layer == layer)
        {
            self.widget_layer = None;
            self.widget_touch = None;
        }
    }
    fn configure(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        if let Some(os) = self.outputs.iter_mut().find(|o| &o.layer == layer) {
            os.width = configure.new_size.0.max(1);
            os.height = configure.new_size.1.max(1);
            os.configured = true;
            os.apply_mode(qh, &self.compositor, &self.shell);
        }
        let mut osd_changed = false;
        if let Some(pl) = self
            .popups
            .iter_mut()
            .chain(self.widget_layer.iter_mut())
            .chain(self.osd_layer.iter_mut())
            .find(|p| &p.layer == layer)
        {
            pl.width = configure.new_size.0.max(1);
            pl.height = configure.new_size.1.max(1);
            pl.configured = true;
            pl.awaiting_configure = false;
            if pl.render(qh, &self.compositor, &self.shell) && pl.kind == SurfaceKind::Osd {
                osd_changed = true;
            }
        }
        if osd_changed {
            self.dump_osd_boxes();
        }
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = self.seat_state.get_keyboard(qh, &seat, None).ok();
        }
        if capability == Capability::Pointer && self.pointer.is_none() {
            if let Ok(ptr) = self.seat_state.get_pointer(qh, &seat) {
                if let Some(mgr) = &self.cursor_shape_mgr {
                    self.cursor_device = Some(mgr.get_pointer(&ptr, qh, ()));
                }
                self.pointer = Some(ptr);
            }
        }
        if capability == Capability::Touch && self.touch.is_none() {
            self.touch = self.seat_state.get_touch(qh, &seat).ok();
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard {
            if let Some(kb) = self.keyboard.take() {
                kb.release();
            }
        }
        if capability == Capability::Pointer {
            if let Some(dev) = self.cursor_device.take() {
                dev.destroy();
            }
            if let Some(p) = self.pointer.take() {
                p.release();
            }
        }
        if capability == Capability::Touch {
            if let Some(t) = self.touch.take() {
                t.release();
            }
            self.popup_touch = None;
            if self.shell.touch_cancel() {
                self.sync_all();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

/// What a key press means to the open panel.
pub(super) fn key_of(event: &KeyEvent, m: &Modifiers) -> Option<crate::shell::Key> {
    use crate::shell::Key;
    Some(match event.keysym {
        Keysym::Escape => Key::Escape,
        Keysym::Up | Keysym::KP_Up => Key::Up,
        Keysym::Down | Keysym::KP_Down => Key::Down,
        Keysym::Left | Keysym::KP_Left => Key::Left,
        Keysym::Right | Keysym::KP_Right => Key::Right,
        Keysym::Home | Keysym::KP_Home => Key::Home,
        Keysym::End | Keysym::KP_End => Key::End,
        Keysym::Page_Up | Keysym::KP_Page_Up => Key::PageUp,
        Keysym::Page_Down | Keysym::KP_Page_Down => Key::PageDown,
        Keysym::Delete | Keysym::KP_Delete => Key::Delete,
        Keysym::Tab if m.shift => Key::BackTab,
        Keysym::ISO_Left_Tab => Key::BackTab,
        Keysym::Tab => Key::Tab,
        Keysym::Return | Keysym::KP_Enter => Key::Enter,
        Keysym::BackSpace => Key::Backspace,
        k if m.logo => {
            let c = k.key_char()?;
            if !c.is_ascii_alphabetic() {
                return None;
            }
            Key::Super(c)
        }
        _ if m.ctrl || m.alt => return None,
        k => {
            let c = event
                .utf8
                .as_deref()
                .and_then(|s| s.chars().next())
                .or_else(|| k.key_char())?;
            Key::Char(c)
        }
    })
}

impl KeyboardHandler for App {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        self.keyboard_output = self
            .outputs
            .iter()
            .find(|o| o.layer.wl_surface() == surface)
            .map(|o| o.id);
        if self.keyboard_output.is_some()
            && self.shell.open.is_some()
            && !self.shell.opened_by_hover
            && !self.shell.keyboard_held
        {
            self.shell.keyboard_held = true;
            self.sync_all();
        }
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if let Some(id) = self
            .outputs
            .iter()
            .find(|o| o.layer.wl_surface() == surface)
            .map(|o| o.id)
        {
            if self.keyboard_output == Some(id) {
                self.keyboard_output = None;
            }
        }
    }
    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        if let Some(k) = key_of(&event, &self.modifiers) {
            if self.shell.key(k) {
                self.sync_all();
            }
        }
    }
    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        // held: arrows, paging, Backspace and characters repeat; Escape, Enter, Delete and Super do not
        if let Some(k) = key_of(&event, &self.modifiers).filter(|k| {
            !matches!(
                k,
                crate::shell::Key::Escape
                    | crate::shell::Key::Enter
                    | crate::shell::Key::Delete
                    | crate::shell::Key::Super(_)
            )
        }) {
            if self.shell.key(k) {
                self.sync_all();
            }
        }
    }
    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }
    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.modifiers = modifiers;
    }
}

/// Converts a Wayland vertical axis event into `(pixels, notches)`.
/// Discrete wheel events (`value120` or `discrete`) scale by 36 px per notch; continuous touchpad
/// events pass `absolute` pixels 1:1 with `0.0` notches.
pub(crate) fn scroll_from_axis(vertical: &AxisScroll) -> (f64, f64) {
    if vertical.value120 != 0 {
        let notches = f64::from(vertical.value120) / 120.0;
        (notches * crate::card::SCROLL_NOTCH_PX, notches)
    } else if vertical.discrete != 0 {
        let notches = f64::from(vertical.discrete);
        (notches * crate::card::SCROLL_NOTCH_PX, notches)
    } else {
        (vertical.absolute, 0.0)
    }
}

impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        let mut changed = false;
        let mut pop_changed = false;
        let mut widgets_changed = false;
        for event in events {
            if let PointerEventKind::Enter { serial } = event.kind {
                if let Some(dev) = &self.cursor_device {
                    dev.set_shape(serial, wp_cursor_shape_device_v1::Shape::Default);
                }
            }
            if let Some(pl) = self
                .popups
                .as_mut()
                .filter(|p| p.layer.wl_surface() == &event.surface)
            {
                // the pop-ups: pointing holds one's time; a click on its x, a button, or the card
                use crate::notify::{hit, popup_at, Hit};
                let w = pl.width as f64;
                let now = Instant::now();
                let n = &mut self.shell.notify;
                let before = pl.pointer.and_then(|p| hit(n, w, p));
                match event.kind {
                    PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                        pl.pointer = Some(event.position);
                        let over = popup_at(n, w, event.position);
                        pop_changed |= n.hover(over, now) || hit(n, w, event.position) != before;
                    }
                    PointerEventKind::Leave { .. } => {
                        pl.pointer = None;
                        pop_changed |= n.hover(None, now) || before.is_some();
                    }
                    PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                        pop_changed |= match hit(n, w, event.position) {
                            Some(Hit::Dismiss(id)) => n.dismiss(id),
                            Some(Hit::Action(id, key)) => n.invoke(id, &key),
                            Some(Hit::Open(id)) => n.invoke(id, "default"),
                            None => false,
                        };
                    }
                    _ => {}
                }
                continue;
            }
            if self
                .widget_layer
                .as_ref()
                .is_some_and(|l| l.layer.wl_surface() == &event.surface)
            {
                let (x, y) = event.position;
                let sh = &mut self.shell;
                widgets_changed |= match event.kind {
                    PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                        sh.widget_motion(x, y)
                    }
                    PointerEventKind::Leave { .. } => sh.widget_leave(),
                    PointerEventKind::Press { button, .. } => match button {
                        BTN_LEFT => sh.widget_press(x, y, Button::Left),
                        BTN_RIGHT => sh.widget_press(x, y, Button::Right),
                        BTN_MIDDLE => sh.widget_press(x, y, Button::Middle),
                        _ => false,
                    },
                    PointerEventKind::Release { .. } => sh.widget_release(),
                    PointerEventKind::Axis { .. } => false,
                };
                continue;
            }
            let Some((os_id, w, h)) = self
                .outputs
                .iter()
                .find(|o| o.layer.wl_surface() == &event.surface && o.configured)
                .map(|o| (o.id, o.width as f64, o.height as f64))
            else {
                continue;
            };
            let (x, y) = event.position;
            let now = Instant::now();
            changed |= match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    self.current_pointer_output = Some(os_id);
                    self.last_pointer_output = Some(os_id);
                    self.shell.pointer_motion_on(os_id, w, h, x, y, now)
                }
                PointerEventKind::Leave { .. } => {
                    if self.current_pointer_output == Some(os_id) {
                        self.current_pointer_output = None;
                    }
                    self.shell.pointer_leave_on(os_id, now)
                }
                PointerEventKind::Press { button, .. } => {
                    let b = match button {
                        BTN_LEFT => Button::Left,
                        BTN_RIGHT => Button::Right,
                        BTN_MIDDLE => Button::Middle,
                        _ => continue,
                    };
                    self.shell.click_on(os_id, w, h, x, y, b)
                }
                PointerEventKind::Release { .. } => self.shell.release_on(os_id),
                PointerEventKind::Axis { vertical, .. } => {
                    let (dy, notches) = scroll_from_axis(&vertical);
                    if notches != 0.0 {
                        self.shell.scroll_notches_on(os_id, w, h, x, y, notches)
                    } else {
                        dy.abs() > 0.0 && self.shell.scroll_on(os_id, w, h, x, y, dy)
                    }
                }
            };
        }
        if pop_changed {
            self.sync_popups();
        }
        if widgets_changed {
            self.redraw_widgets();
        }
        if changed || !self.shell.actions.is_empty() {
            self.sync_all();
        }
    }
}

impl TouchHandler for App {
    fn down(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_touch::WlTouch,
        _: u32,
        _: u32,
        surface: wl_surface::WlSurface,
        id: i32,
        position: (f64, f64),
    ) {
        if self
            .popups
            .as_ref()
            .is_some_and(|p| p.layer.wl_surface() == &surface)
        {
            if self.popup_touch.is_none() {
                self.popup_touch = Some((id, position));
            }
            return;
        }
        if self
            .widget_layer
            .as_ref()
            .is_some_and(|l| l.layer.wl_surface() == &surface)
        {
            // one finger on the widgets at a time, as the left button
            if self.widget_touch.is_none() {
                self.widget_touch = Some(id);
                self.shell.widgets.pointer = Some(position);
                if self
                    .shell
                    .widget_press(position.0, position.1, Button::Left)
                {
                    self.redraw_widgets();
                }
                if !self.shell.actions.is_empty() {
                    self.sync_all();
                }
            }
            return;
        }
        let Some((os_id, w, h)) = self
            .outputs
            .iter()
            .find(|o| o.layer.wl_surface() == &surface && o.configured)
            .map(|o| (o.id, o.width as f64, o.height as f64))
        else {
            return;
        };
        if self
            .shell
            .touch_down_on(os_id, w, h, id, position.0, position.1)
            || !self.shell.actions.is_empty()
        {
            self.sync_all();
        }
    }

    fn up(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_touch::WlTouch,
        _: u32,
        _: u32,
        id: i32,
    ) {
        if let Some((_, start)) = self.popup_touch.take().filter(|(pid, _)| *pid == id) {
            let pop_changed = if let Some(pl) = self.popups.as_mut() {
                use crate::notify::{hit, Hit};
                let w = pl.width as f64;
                let n = &mut self.shell.notify;
                match hit(n, w, start) {
                    Some(Hit::Dismiss(nid)) => n.dismiss(nid),
                    Some(Hit::Action(nid, key)) => n.invoke(nid, &key),
                    Some(Hit::Open(nid)) => n.invoke(nid, "default"),
                    None => false,
                }
            } else {
                false
            };
            if pop_changed {
                self.sync_popups();
            }
            return;
        }
        if self.widget_touch == Some(id) {
            self.widget_touch = None;
            // a finger lifted leaves no hover light
            if self.shell.widget_release() | self.shell.widget_leave() {
                self.redraw_widgets();
            }
            if !self.shell.actions.is_empty() {
                self.sync_all();
            }
            return;
        }
        if self.shell.touch_up(id) || !self.shell.actions.is_empty() {
            self.sync_all();
        }
    }

    fn motion(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_touch::WlTouch,
        _: u32,
        id: i32,
        position: (f64, f64),
    ) {
        if let Some((pid, start)) = self.popup_touch {
            if pid == id {
                let dx = position.0 - start.0;
                let dy = position.1 - start.1;
                if dx * dx + dy * dy >= 64.0 {
                    self.popup_touch = None;
                }
                return;
            }
        }
        if self.widget_touch == Some(id) {
            if self.shell.widget_motion(position.0, position.1) {
                self.redraw_widgets();
            }
            if !self.shell.actions.is_empty() {
                self.sync_all();
            }
            return;
        }
        if self.shell.touch_motion(id, position.0, position.1) {
            self.sync_all();
        }
    }

    fn shape(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_touch::WlTouch,
        _: i32,
        _: f64,
        _: f64,
    ) {
    }

    fn orientation(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_touch::WlTouch,
        _: i32,
        _: f64,
    ) {
    }

    fn cancel(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_touch::WlTouch) {
        self.popup_touch = None;
        if self.widget_touch.take().is_some()
            && (self.shell.widget_cancel() | self.shell.widget_leave())
        {
            self.redraw_widgets();
        }
        if self.shell.touch_cancel() {
            self.sync_all();
        }
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_compositor!(App);
delegate_output!(App);
delegate_shm!(App);
delegate_seat!(App);
delegate_keyboard!(App);
delegate_pointer!(App);
delegate_touch!(App);
delegate_layer!(App);
delegate_registry!(App);
delegate_noop!(App: ignore WpViewporter);
delegate_noop!(App: ignore WpViewport);
delegate_noop!(App: ignore WpFractionalScaleManagerV1);
delegate_noop!(App: ignore WpCursorShapeManagerV1);
delegate_noop!(App: ignore WpCursorShapeDeviceV1);

impl Dispatch<WpFractionalScaleV1, wl_surface::WlSurface> for App {
    fn event(
        state: &mut Self,
        _: &WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        surface: &wl_surface::WlSurface,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wp_fractional_scale_v1::Event::PreferredScale { scale } = event else {
            return;
        };
        if scale == 0 {
            return;
        }
        let mut need_sync = false;
        if let Some(os) = state
            .outputs
            .iter_mut()
            .find(|o| o.layer.wl_surface() == surface)
        {
            if os.fractional_scale_120 != Some(scale) {
                os.fractional_scale_120 = Some(scale);
                os.mapped_closed = false;
                if state.shell.visible_on(os.id) {
                    os.dirty = true;
                    need_sync = true;
                } else {
                    os.apply_mode(qh, &state.compositor, &state.shell);
                }
            }
        }
        let mut osd_changed = false;
        if let Some(pl) = state
            .popups
            .iter_mut()
            .chain(state.widget_layer.iter_mut())
            .chain(state.osd_layer.iter_mut())
            .find(|p| p.layer.wl_surface() == surface)
        {
            if pl.fractional_scale_120 != Some(scale) {
                pl.fractional_scale_120 = Some(scale);
                if pl.render(qh, &state.compositor, &state.shell) && pl.kind == SurfaceKind::Osd {
                    osd_changed = true;
                }
            }
        }
        if osd_changed {
            state.dump_osd_boxes();
        }
        if need_sync {
            state.sync_all();
        }
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for App {
    fn event(
        state: &mut Self,
        _: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } = event {
            let id = state.next_toplevel_id;
            state.next_toplevel_id += 1;
            state.toplevels.push(Toplevel {
                id,
                handle: toplevel,
                title: String::new(),
                app_id: String::new(),
                focused: false,
                fullscreen: false,
                outputs: Vec::new(),
            });
        }
    }

    smithay_client_toolkit::reexports::client::event_created_child!(
        App,
        ZwlrForeignToplevelManagerV1,
        [zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ())]
    );
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for App {
    fn event(
        state: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwlr_foreign_toplevel_handle_v1::{Event as E, State as S};
        let t = state.toplevels.iter_mut().find(|t| &t.handle == handle);
        match event {
            E::Title { title } => {
                if let Some(t) = t {
                    t.title = title
                }
            }
            E::AppId { app_id } => {
                if let Some(t) = t {
                    t.app_id = app_id
                }
            }
            E::State { state: raw } => {
                let has = |s: S| {
                    raw.as_chunks::<4>()
                        .0
                        .iter()
                        .any(|&c| u32::from_ne_bytes(c) == s as u32)
                };
                let (active, fullscreen) = (has(S::Activated), has(S::Fullscreen));
                if let Some(t) = t {
                    t.focused = active;
                    if t.fullscreen != fullscreen {
                        t.fullscreen = fullscreen;
                        state.fullscreen_changed = true;
                    }
                }
            }
            E::OutputEnter { output } => {
                if let Some(t) = t {
                    if !t.outputs.contains(&output) {
                        t.outputs.push(output);
                        state.fullscreen_changed |= t.fullscreen;
                    }
                }
            }
            E::OutputLeave { output } => {
                if let Some(t) = t {
                    let before = t.outputs.len();
                    t.outputs.retain(|o| o != &output);
                    state.fullscreen_changed |= t.fullscreen && t.outputs.len() != before;
                }
            }
            E::Done => {
                if std::mem::take(&mut state.fullscreen_changed) {
                    state.apply_modes();
                }
                state.sync_windows();
            }
            E::Closed => {
                handle.destroy();
                let was_fullscreen = state
                    .toplevels
                    .iter()
                    .any(|t| &t.handle == handle && t.fullscreen);
                state.toplevels.retain(|t| &t.handle != handle);
                if was_fullscreen {
                    state.apply_modes();
                }
                state.sync_windows();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wayland::tests::*;

    #[test]
    fn headless_two_outputs_shows_panel_on_one_output_and_routes_click() {
        let (mock, conn) = MockWayland::start(
            vec![
                MockOutputSpec {
                    width: 1920,
                    height: 1080,
                    scale: 1,
                    fractional_120: 120,
                },
                MockOutputSpec {
                    width: 1280,
                    height: 800,
                    scale: 2,
                    fractional_120: 240,
                },
            ],
            false,
        );
        let mut sh = Shell::new(
            crate::config::Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 hover settings/power\"\n",
            ),
            Vec::new(),
        );
        sh.apply(
            "settings",
            "power",
            &crate::cards::settings::parse_power("* balanced:\n  performance:\n")
                as &dyn std::any::Any,
        );
        let (fetch_tx, _fetch_rx) = channel::channel::<Fetched>();
        let (mut app, mut eq) = init_app(&conn, sh, fetch_tx).unwrap();
        eq.roundtrip(&mut app).unwrap();
        assert_eq!(app.outputs.len(), 2);

        let sid1 = surface_wl_id(&app.outputs[0]);
        let sid2 = surface_wl_id(&app.outputs[1]);
        let out2_id = app.outputs[1].id;
        let ctrl_idx = app
            .shell
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();

        // Open by hover on Output 2's 2 px strip with injected Instants (no sleep).
        let strip2 = app.shell.hover_strip(ctrl_idx, 1280.0, 800.0);
        let t0 = Instant::now();
        assert!(!app.shell.pointer_motion_on(
            out2_id,
            1280.0,
            800.0,
            strip2.0 + 10.0,
            strip2.1 + 1.0,
            t0
        ));
        assert!(app
            .shell
            .hover_tick(t0 + Duration::from_millis(crate::shell::HOVER_OPEN_MS as u64)));
        assert_eq!(app.shell.open, Some(ctrl_idx));
        assert_eq!(app.shell.open_output, Some(out2_id));
        app.shell.open_ms = f64::INFINITY;
        app.shell.opened_at = None;
        app.sync_all();
        eq.roundtrip(&mut app).unwrap();

        // Output 2 draws at 2560x1600; Output 1 stays clear with only its 2 px hover strips.
        let c1 = mock.last_commit(sid1);
        let c2 = mock.last_commit(sid2);
        assert_eq!(c2.buffer_size, Some((2560, 1600)));
        assert!(surface_has_nonzero_pixels(&mut app.outputs[1]));
        assert!(!surface_has_nonzero_pixels(&mut app.outputs[0]));
        assert_eq!(c1.keyboard_interactivity, 0); // KeyboardInteractivity::None
        let expected_strips1: Vec<(i32, i32, i32, i32)> = (0..app.shell.panels.len())
            .filter(|&i| app.shell.panels[i].panel.hover)
            .map(|i| {
                let s = app.shell.hover_strip(i, 1920.0, 1080.0);
                (
                    s.0.floor() as i32,
                    s.1.floor() as i32,
                    s.2.ceil() as i32,
                    s.3.ceil() as i32,
                )
            })
            .collect();
        assert_eq!(c1.input_rects, expected_strips1);

        // Pointer event on Output 1 (while Output 2 shows the panel) does not close or click Output 2.
        let s1 = expected_strips1[0];
        mock.send_pointer_move(sid1, f64::from(s1.0) + 10.0, f64::from(s1.1) + 1.0, true);
        mock.send_pointer_click(BTN_LEFT);
        eq.roundtrip(&mut app).unwrap();
        assert_eq!(app.shell.open, Some(ctrl_idx));
        assert_eq!(app.shell.open_output, Some(out2_id));
        assert!(app.shell.actions.is_empty());

        // Click the second power choice ("performance") on Output 2 in logical coordinates.
        let perf_rect = app.shell.frame_hits(Some(out2_id))[0].2.list[1].1;
        mock.send_pointer_move(sid2, perf_rect.0 + 5.0, perf_rect.1 + 5.0, true);
        mock.send_pointer_click(BTN_LEFT);
        eq.roundtrip(&mut app).unwrap();
        assert!(
            app.shell.actions.iter().any(|(a, _, _, _)| matches!(
                a,
                Action::Run(cmd) if cmd.contains("performance")
            )) || app.ran.iter().any(|c| c.contains("performance")),
            "click on Output 2 must trigger performance action"
        );
        app.shell.actions.clear();

        // Close, focus a toplevel on Output 2, and open by key/signal -> opens on Output 2 only.
        assert!(app.shell.close_at(t0));
        app.shell
            .tick(t0 + Duration::from_millis(crate::shell::LEAVE_MS as u64 + 10));
        app.shell
            .tick(t0 + Duration::from_millis(crate::shell::LEAVE_MS as u64 + 60));
        app.sync_all();
        eq.roundtrip(&mut app).unwrap();

        mock.send_focused_toplevel(1);
        eq.roundtrip(&mut app).unwrap();
        app.handle_signal_byte(b'C');
        app.shell.open_ms = f64::INFINITY;
        app.shell.opened_at = None;
        eq.roundtrip(&mut app).unwrap();
        app.outputs[1].dirty = true;
        app.sync_all();
        eq.roundtrip(&mut app).unwrap();

        assert_eq!(app.shell.open_output, Some(out2_id));
        assert!(surface_has_nonzero_pixels(&mut app.outputs[1]));
        assert!(!surface_has_nonzero_pixels(&mut app.outputs[0]));
    }

    #[test]
    fn headless_widgets_surface_only_with_a_line_under_the_windows_taking_clicks_on_widgets() {
        let spec = || {
            vec![MockOutputSpec {
                width: 1280,
                height: 800,
                scale: 1,
                fractional_120: 120,
            }]
        };
        let layers = |mock: &MockWayland| {
            let mut v: Vec<(u32, String)> = mock
                .state
                .lock()
                .unwrap()
                .layer_kind
                .values()
                .cloned()
                .collect();
            v.sort();
            v
        };
        // no WIDGETS line: the panels' surface only (Top layer), nothing for widgets
        let (mock, conn) = MockWayland::start(spec(), false);
        let sh = Shell::new(crate::config::Config::from_text(""), Vec::new());
        let (fetch_tx, _fetch_rx) = channel::channel::<Fetched>();
        let (mut app, mut eq) = init_app(&conn, sh, fetch_tx).unwrap();
        app.sync_all();
        eq.roundtrip(&mut app).unwrap();
        assert!(app.widget_layer.is_none());
        assert_eq!(layers(&mock), vec![(2, "infynitae-edge".to_string())]);
        drop(app);
        drop(mock);

        // with the line: the widgets' surface on the Bottom layer, the pointer on the widgets only
        let (mock, conn) = MockWayland::start(spec(), false);
        let mut sh = Shell::new(
            crate::config::Config::from_text(
                "WIDGETS=\"power@100,100,380x150 clock@600,100,300x100\"\n",
            ),
            Vec::new(),
        );
        sh.apply(
            "settings",
            "power",
            &crate::cards::settings::parse_power("* balanced:\n  performance:\n")
                as &dyn std::any::Any,
        );
        let (fetch_tx, _fetch_rx) = channel::channel::<Fetched>();
        let (mut app, mut eq) = init_app(&conn, sh, fetch_tx).unwrap();
        app.sync_all();
        eq.roundtrip(&mut app).unwrap();
        eq.roundtrip(&mut app).unwrap();
        assert_eq!(
            layers(&mock),
            vec![
                (1, "infynitae-widgets".to_string()),
                (2, "infynitae-edge".to_string())
            ]
        );
        let sid = {
            use smithay_client_toolkit::reexports::client::Proxy;
            app.widget_layer
                .as_ref()
                .expect("widgets' surface")
                .layer
                .wl_surface()
                .id()
                .protocol_id()
        };
        let c = mock.last_commit(sid);
        assert_eq!(c.buffer_size, Some((1280, 800)));
        assert_eq!(c.keyboard_interactivity, 0);
        assert_eq!(
            c.input_rects,
            vec![(100, 100, 380, 150), (600, 100, 300, 100)]
        );
        // no panel open: the widgets' source is fetched all the same
        assert_eq!(app.shell.open, None);
        assert!(app.workers.contains_key(&("settings", "power")));

        // a click on the second power mode reaches the card; one beside the widgets does nothing
        mock.send_pointer_move(sid, 50.0, 50.0, true);
        mock.send_pointer_click(BTN_LEFT);
        eq.roundtrip(&mut app).unwrap();
        assert!(app.ran.is_empty());
        let perf = app.shell.widget_frame()[0].1.list[1].1;
        mock.send_pointer_move(sid, perf.0 + 5.0, perf.1 + 5.0, false);
        mock.send_pointer_click(BTN_LEFT);
        eq.roundtrip(&mut app).unwrap();
        assert!(
            app.ran.iter().any(|c| c.contains("performance")),
            "{:?}",
            app.ran
        );

        // the line taken out: the surface goes
        app.shell
            .rebuild_panels(crate::config::Config::from_text(""), None);
        app.sync_all();
        assert!(app.widget_layer.is_none());
        assert!(app.workers.is_empty());
    }

    #[test]
    fn scroll_from_axis_distinguishes_discrete_value120_and_touchpad_pixels() {
        let disc = AxisScroll {
            discrete: 1,
            absolute: 10.0,
            ..Default::default()
        };
        assert_eq!(scroll_from_axis(&disc), (36.0, 1.0));

        let v120 = AxisScroll {
            value120: 120,
            absolute: 15.0,
            ..Default::default()
        };
        assert_eq!(scroll_from_axis(&v120), (36.0, 1.0));

        let half_v120 = AxisScroll {
            value120: 60,
            ..Default::default()
        };
        assert_eq!(scroll_from_axis(&half_v120), (18.0, 0.5));

        let pad_small = AxisScroll {
            absolute: 2.5,
            ..Default::default()
        };
        assert_eq!(scroll_from_axis(&pad_small), (2.5, 0.0));

        let pad_tiny = AxisScroll {
            absolute: -0.5,
            ..Default::default()
        };
        assert_eq!(scroll_from_axis(&pad_tiny), (-0.5, 0.0));
    }

    #[test]
    fn key_of_maps_navigation_editing_and_delete_keys() {
        use crate::shell::Key;
        let mods = Modifiers::default();
        let mk = |keysym| KeyEvent {
            raw_code: 0,
            keysym,
            utf8: None,
            time: 0,
        };
        assert_eq!(key_of(&mk(Keysym::Left), &mods), Some(Key::Left));
        assert_eq!(key_of(&mk(Keysym::Right), &mods), Some(Key::Right));
        assert_eq!(key_of(&mk(Keysym::Home), &mods), Some(Key::Home));
        assert_eq!(key_of(&mk(Keysym::End), &mods), Some(Key::End));
        assert_eq!(key_of(&mk(Keysym::Page_Up), &mods), Some(Key::PageUp));
        assert_eq!(key_of(&mk(Keysym::Page_Down), &mods), Some(Key::PageDown));
        assert_eq!(key_of(&mk(Keysym::Delete), &mods), Some(Key::Delete));
    }

    #[test]
    fn headless_two_outputs_touch_and_axis_route_to_open_output() {
        use crate::cards::settings_rows::ConfigData;
        let (mock, conn) = MockWayland::start(
            vec![
                MockOutputSpec {
                    width: 1920,
                    height: 1080,
                    scale: 1,
                    fractional_120: 120,
                },
                MockOutputSpec {
                    width: 1280,
                    height: 320,
                    scale: 1,
                    fractional_120: 120,
                },
            ],
            false,
        );
        let mut sh = Shell::new(
            crate::config::Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 settings/power settings/input settings/session\"\n",
            ),
            Vec::new(),
        );
        sh.apply(
            "settings",
            "power",
            &crate::cards::settings::parse_power("* balanced:\n  power-saver:\n  performance:\n")
                as &dyn std::any::Any,
        );
        let mut input_cfg = ConfigData::default();
        input_cfg
            .default
            .insert("KEYBOARD_REPEAT_DELAY".into(), "400".into());
        sh.apply("settings", "config", &input_cfg as &dyn std::any::Any);

        let (fetch_tx, _fetch_rx) = channel::channel::<Fetched>();
        let (mut app, mut eq) = init_app(&conn, sh, fetch_tx).unwrap();
        eq.roundtrip(&mut app).unwrap();
        assert!(app.touch.is_some());

        let sid1 = surface_wl_id(&app.outputs[0]);
        let sid2 = surface_wl_id(&app.outputs[1]);
        let out2_id = app.outputs[1].id;
        assert!(app.shell.toggle_named_on(Some(out2_id), "control"));
        app.shell.open_ms = f64::INFINITY;
        app.shell.opened_at = None;
        app.sync_all();
        eq.roundtrip(&mut app).unwrap();

        let pbox2 = app.shell.shown_box(1280.0, 320.0).unwrap();

        // Pointer axis on Output 1 (panel is on Output 2) is ignored.
        mock.send_pointer_move(sid1, 100.0, 100.0, true);
        mock.send_pointer_axis_value120(120, 10.0);
        eq.roundtrip(&mut app).unwrap();
        assert_eq!(app.shell.panel_scroll, 0.0);

        // Pointer axis on Output 2: continuous pixels 1:1, then 1 value120 notch = 36 px.
        mock.send_pointer_move(sid2, pbox2.0 + 10.0, pbox2.1 + 10.0, true);
        mock.send_pointer_axis_continuous(2.5);
        eq.roundtrip(&mut app).unwrap();
        assert!((app.shell.panel_scroll - 2.5).abs() < 1e-3);
        mock.send_pointer_axis_value120(120, 15.0);
        eq.roundtrip(&mut app).unwrap();
        assert!((app.shell.panel_scroll - 38.5).abs() < 1e-3);

        // Touch on Output 1 does nothing.
        mock.send_touch_down(sid1, 0, 100.0, 100.0);
        mock.send_touch_up(0);
        eq.roundtrip(&mut app).unwrap();
        assert!(app.shell.open.is_some());
        assert!(app.shell.actions.is_empty());

        // Touch tap on Output 2 ("performance" choice) triggers its command.
        app.shell.panel_scroll = 0.0;
        app.outputs[1].dirty = true;
        app.sync_all();
        eq.roundtrip(&mut app).unwrap();
        let perf_r = app.shell.frame_hits(Some(out2_id))[0].2.list[2].1;
        mock.send_touch_down(sid2, 1, perf_r.0 + 4.0, perf_r.1 + 4.0);
        mock.send_touch_up(1);
        eq.roundtrip(&mut app).unwrap();
        assert!(
            app.shell
                .actions
                .iter()
                .any(|(a, ..)| matches!(a, Action::Run(cmd) if cmd.contains("performance")))
                || app.ran.iter().any(|c| c.contains("performance"))
        );
        app.shell.actions.clear();
        app.ran.clear();

        // Touch drag on slider followed by cancel clears state without committing.
        let slider_r = {
            let hits = app.shell.frame_hits(Some(out2_id));
            hits[1]
                .2
                .list
                .iter()
                .find(|(id, _)| hits[1].2.is_draggable(*id))
                .expect("input slider draggable")
                .1
        };
        let iy = slider_r.1 + slider_r.3 / 2.0;
        mock.send_touch_down(sid2, 2, slider_r.0 + slider_r.2 * 0.2, iy);
        mock.send_touch_motion(2, slider_r.0 + slider_r.2 * 0.8, iy);
        mock.send_touch_cancel();
        eq.roundtrip(&mut app).unwrap();
        assert!(app.shell.actions.is_empty());
        assert!(
            app.ran.is_empty(),
            "a cancelled drag commits nothing: {:?}",
            app.ran
        );
    }

    #[test]
    fn headless_pointer_enter_sets_default_cursor_shape_when_offered() {
        // 1. Compositor offers wp_cursor_shape_manager_v1: pointer enter sets Shape::Default.
        let (mock, conn) = MockWayland::start_with(
            vec![MockOutputSpec {
                width: 1280,
                height: 800,
                scale: 1,
                fractional_120: 120,
            }],
            false,
            true,
        );
        let sh = Shell::new(
            crate::config::Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 settings/session\"\n",
            ),
            Vec::new(),
        );
        let (fetch_tx, _fetch_rx) = channel::channel::<Fetched>();
        let (mut app, mut eq) = init_app(&conn, sh, fetch_tx).unwrap();
        eq.roundtrip(&mut app).unwrap();
        eq.roundtrip(&mut app).unwrap();
        assert!(app.cursor_device.is_some());

        let sid = surface_wl_id(&app.outputs[0]);
        mock.send_pointer_move(sid, 100.0, 50.0, true);
        eq.roundtrip(&mut app).unwrap();
        eq.roundtrip(&mut app).unwrap();
        let shapes = mock.state.lock().unwrap().cursor_shapes.clone();
        assert_eq!(shapes.len(), 1);
        assert_eq!(
            shapes[0].1,
            wp_cursor_shape_device_v1::Shape::Default as u32
        );

        // Subsequent motion does not re-send set_shape; leaving and entering again sends it with the new serial.
        mock.send_pointer_move(sid, 120.0, 60.0, false);
        eq.roundtrip(&mut app).unwrap();
        eq.roundtrip(&mut app).unwrap();
        assert_eq!(mock.state.lock().unwrap().cursor_shapes.len(), 1);

        mock.send_pointer_leave(sid);
        mock.send_pointer_move(sid, 140.0, 70.0, true);
        eq.roundtrip(&mut app).unwrap();
        eq.roundtrip(&mut app).unwrap();
        let shapes = mock.state.lock().unwrap().cursor_shapes.clone();
        assert_eq!(shapes.len(), 2);
        assert!(shapes[1].0 > shapes[0].0);
        assert_eq!(
            shapes[1].1,
            wp_cursor_shape_device_v1::Shape::Default as u32
        );

        // 2. Compositor does not offer wp_cursor_shape_manager_v1: leaves cursor as is without error.
        let (mock_no, conn_no) = MockWayland::start_with(
            vec![MockOutputSpec {
                width: 1280,
                height: 800,
                scale: 1,
                fractional_120: 120,
            }],
            false,
            false,
        );
        let sh_no = Shell::new(crate::config::Config::from_text(""), Vec::new());
        let (fetch_tx_no, _fetch_rx_no) = channel::channel::<Fetched>();
        let (mut app_no, mut eq_no) = init_app(&conn_no, sh_no, fetch_tx_no).unwrap();
        eq_no.roundtrip(&mut app_no).unwrap();
        eq_no.roundtrip(&mut app_no).unwrap();
        assert!(app_no.cursor_device.is_none());
        let sid_no = surface_wl_id(&app_no.outputs[0]);
        mock_no.send_pointer_move(sid_no, 50.0, 20.0, true);
        eq_no.roundtrip(&mut app_no).unwrap();
        eq_no.roundtrip(&mut app_no).unwrap();
        assert!(mock_no.state.lock().unwrap().cursor_shapes.is_empty());
    }
}
