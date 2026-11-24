//! Pointer input: motion and the hover dwell, clicks and releases, wheel and touchpad scrolling, drag autoscroll.

use super::*;

impl Shell {
    /// The pointer moved: a hover panel opens from its strip after a short dwell and closes after
    /// a grace time when left; the light follows the hit under it; a held button drags. True when
    /// something must be drawn again.
    #[cfg(test)]
    pub fn pointer_motion(&mut self, w: f64, h: f64, x: f64, y: f64) -> bool {
        self.pointer_motion_at(w, h, x, y, Instant::now())
    }

    #[cfg(test)]
    pub fn pointer_motion_at(&mut self, w: f64, h: f64, x: f64, y: f64, now: Instant) -> bool {
        self.pointer_motion_on(screen_key(w, h), w, h, x, y, now)
    }

    pub fn pointer_motion_on(
        &mut self,
        output: OutputId,
        w: f64,
        h: f64,
        x: f64,
        y: f64,
        now: Instant,
    ) -> bool {
        if self
            .touch
            .is_some_and(|t| t.mode == TouchMode::DragHit && t.output == output)
        {
            return false;
        }
        if self.ask.ask.on {
            if self.ask_output.is_some_and(|id| id != output) {
                return false;
            }
            self.last_output = Some(output);
            self.last_screen = (w, h);
            return crate::ask::hover(&mut self.ask, w, h, x, y);
        }
        if self.window_menu.is_some() {
            // the pointer moved: the keyboard's selection hides (the light follows the pointer)
            let hid = std::mem::take(&mut self.keyboard);
            self.pointer = Some((x, y));
            self.last_output = Some(output);
            let now = self.hit_at_on(output, (x, y));
            let changed = now != self.hovered.get(&output).copied();
            match now {
                Some(h) => self.hovered.insert(output, h),
                None => self.hovered.remove(&output),
            };
            return changed || hid;
        }
        let Some(idx) = self.open else {
            self.last_output = Some(output);
            self.last_screen = (w, h);
            self.pointers.insert(output, (x, y));
            self.keyboard = false;
            let hit_strip = (0..self.panels.len()).find(|&i| {
                if !self.panels[i].panel.hover {
                    return false;
                }
                let s = self.hover_strip(i, w, h);
                x >= s.0 && x <= s.0 + s.2 && y >= s.1 && y <= s.1 + s.3
            });
            if let Some(i) = hit_strip {
                match self.hover_open {
                    Some((prev_out, prev_i, t0)) if prev_out == output && prev_i == i => {
                        if now.saturating_duration_since(t0).as_secs_f64() * 1000.0 >= HOVER_OPEN_MS
                        {
                            self.hover_open = None;
                            return self.open_panel_on(Some(output), i, true, now);
                        }
                    }
                    _ => self.hover_open = Some((output, i, now)),
                }
            } else if self.hover_open.is_some_and(|(o, _, _)| o == output) {
                self.hover_open = None;
            }
            return false;
        };
        if self.open_output.is_some_and(|id| id != output) {
            self.pointers.insert(output, (x, y));
            return false;
        }
        self.last_output = Some(output);
        self.last_screen = (w, h);
        // the pointer moved: the keyboard's selection hides (the light follows the pointer)
        let hid = std::mem::take(&mut self.keyboard);
        if self.editing() {
            self.pointers.insert(output, (x, y));
            self.pointer = Some((x, y));
            if let Some(mut d) = self.edit.drag {
                if !d.moved && (y - d.start_y).abs() >= 6.0 {
                    d.moved = true;
                    self.edit.hover_card = None;
                }
                self.edit.drag = Some(d);
                if d.moved {
                    let (pbox, local_rects, content_h) = self.layout_local(idx, w, h);
                    let scroll = self.panel_scroll.clamp(0.0, (content_h - pbox.3).max(0.0));
                    let cards = self.edit_cards(idx);
                    self.edit.drop_at = crate::edit::drop_index(
                        &local_rects,
                        cards.len(),
                        d.card,
                        x - pbox.0,
                        y - pbox.1 + scroll,
                    );
                    return true;
                }
                return false;
            }
            let prev = self.edit.hover_card;
            self.update_edit_hover_on(output, w, h);
            let (bar, add) = self.edit_rects(idx, w, h);
            let over_bar = crate::edit::inside(bar, x, y)
                || (self.edit.adding && crate::edit::inside(add, x, y));
            return self.edit.hover_card != prev || over_bar || hid;
        }
        if self.opened_by_hover {
            let b = self.hover_box(idx, w, h);
            if x < b.0 || x > b.0 + b.2 || y < b.1 || y > b.1 + b.3 {
                self.hover_close.get_or_insert(now);
                self.pointers.remove(&output);
                self.pointer = None;
                let lit = self.hovered.remove(&output).is_some();
                return lit || hid;
            }
            self.hover_close = None;
        }
        self.pointers.insert(output, (x, y));
        self.pointer = Some((x, y));
        if let Some((po, ci, hit)) = self.pressed {
            if po == output {
                return self.card_event(idx, ci, &Event::Drag { hit, at: (x, y) });
            }
        }
        let hit_now = self.hit_at_on(output, (x, y));
        let changed = hit_now != self.hovered.get(&output).copied();
        match hit_now {
            Some(h) => {
                self.hovered.insert(output, h);
            }
            None => {
                self.hovered.remove(&output);
            }
        }
        changed || hid
    }

    #[cfg(test)]
    pub fn pointer_leave_at(&mut self, now: Instant) -> bool {
        let out = self.active_output_or_last();
        self.pointer_leave_on(out, now)
    }

    pub fn pointer_leave_on(&mut self, output: OutputId, now: Instant) -> bool {
        if self.ask.ask.on || self.touch.is_some_and(|t| t.output == output) {
            return false;
        }
        self.pointers.remove(&output);
        if self.hover_open.is_some_and(|(o, _, _)| o == output) {
            self.hover_open = None;
        }
        if self.open_output.is_some_and(|id| id != output) {
            return false;
        }
        self.pointer = None;
        if self.editing() {
            return self.edit.hover_card.take().is_some();
        }
        let lit = self.hovered.remove(&output).is_some();
        if self.opened_by_hover && self.open.is_some() {
            self.hover_close.get_or_insert(now);
            return lit;
        }
        lit
    }

    /// Advances hover dwell / close grace timers; returns true if a panel opened or closed.
    pub fn hover_tick(&mut self, now: Instant) -> bool {
        if self.ask.ask.on || self.editing() {
            self.hover_open = None;
            self.hover_close = None;
            return false;
        }
        if self.open.is_none() {
            if let Some((out, i, t0)) = self.hover_open {
                if now.saturating_duration_since(t0).as_secs_f64() * 1000.0 >= HOVER_OPEN_MS {
                    self.hover_open = None;
                    return self.open_panel_on(Some(out), i, true, now);
                }
            }
            return false;
        }
        if self.opened_by_hover {
            if let Some(t0) = self.hover_close {
                if now.saturating_duration_since(t0).as_secs_f64() * 1000.0 >= HOVER_CLOSE_MS {
                    self.hover_close = None;
                    return self.close_at(now);
                }
            }
        }
        false
    }

    pub fn hover_timeout(&self, now: Instant) -> Option<std::time::Duration> {
        if self.ask.ask.on || self.editing() {
            return None;
        }
        if self.open.is_none() {
            self.hover_open.map(|(_, _, t0)| {
                std::time::Duration::from_secs_f64(HOVER_OPEN_MS / 1000.0)
                    .saturating_sub(now.saturating_duration_since(t0))
            })
        } else if self.opened_by_hover {
            self.hover_close.map(|t0| {
                std::time::Duration::from_secs_f64(HOVER_CLOSE_MS / 1000.0)
                    .saturating_sub(now.saturating_duration_since(t0))
            })
        } else {
            None
        }
    }

    /// A button went down: on a hit, the card's Press; an unused right click starts editing; outside
    /// an open panel, it closes.
    #[cfg(test)]
    pub fn click(&mut self, w: f64, h: f64, x: f64, y: f64, button: Button) -> bool {
        self.click_on(screen_key(w, h), w, h, x, y, button)
    }

    pub fn click_on(
        &mut self,
        output: OutputId,
        w: f64,
        h: f64,
        x: f64,
        y: f64,
        button: Button,
    ) -> bool {
        if self.visible_output().is_some_and(|id| id != output) {
            return false;
        }
        if self.touch.is_some() {
            let _ = self.touch_cancel();
        }
        self.click_inner(output, w, h, x, y, button)
    }

    pub(super) fn click_inner(
        &mut self,
        output: OutputId,
        w: f64,
        h: f64,
        x: f64,
        y: f64,
        button: Button,
    ) -> bool {
        self.last_output = Some(output);
        self.last_screen = (w, h);
        if self.ask.ask.on {
            let raw = match button {
                Button::Left => 0x110,
                Button::Right => 0x111,
                Button::Middle => 0x112,
            };
            return crate::ask::click(&mut self.ask, w, h, x, y, raw);
        }
        if let Some(wm) = self.window_menu {
            let r = self.window_menu_rect(w, h).unwrap();
            if x < r.0 || x > r.0 + r.2 || y < r.1 || y > r.1 + r.3 {
                self.window_menu = None;
                self.hovered.clear();
                self.selected = None;
                self.keyboard = false;
                return true;
            }
            if button == Button::Left {
                if let Some((action, _, _)) = self
                    .hit_at_on(output, (x, y))
                    .and_then(|(_, hit)| wm.items().get(hit as usize).copied())
                {
                    self.actions.push((
                        Action::ShellCommand(wm.command_line(action)),
                        "window/menu".into(),
                        "windows",
                        &[],
                    ));
                    self.window_menu = None;
                    self.hovered.clear();
                    self.selected = None;
                    self.keyboard = false;
                    return true;
                }
            }
            return false;
        }
        let Some(idx) = self.open else {
            return false;
        };
        if self.edit_menu.is_some() {
            let item = self.edit_menu_rect(w, h);
            self.edit_menu = None;
            if button == Button::Left
                && item.is_some_and(|r| x >= r.0 && x <= r.0 + r.2 && y >= r.1 && y <= r.1 + r.3)
            {
                return self.enter_edit_on(output, w, h);
            }
            return true;
        }
        if self.editing() {
            return self.edit_press(output, idx, (w, h), (x, y), button);
        }
        let ((ox, oy, pw, ph), _) = self.layout(idx, w, h);
        if x < ox || x > ox + pw || y < oy || y > oy + ph {
            return self.close();
        }
        self.hover_close = None;
        let Some((ci, hit)) = self.hit_at_on(output, (x, y)) else {
            if button == Button::Right {
                return self.offer_edit(idx, x, y);
            }
            return false;
        };
        if button == Button::Left {
            self.pressed = Some((output, ci, hit));
        }
        let used = self.card_event(
            idx,
            ci,
            &Event::Press {
                hit,
                button,
                at: (x, y),
            },
        );
        if !used && button == Button::Right && self.open == Some(idx) {
            return self.offer_edit(idx, x, y);
        }
        used
    }

    #[cfg(test)]
    pub fn release(&mut self) -> bool {
        let out = self
            .pressed
            .map(|(o, _, _)| o)
            .unwrap_or_else(|| self.active_output_or_last());
        self.release_on(out)
    }

    pub fn release_on(&mut self, output: OutputId) -> bool {
        if self.visible_output().is_some_and(|id| id != output) {
            return false;
        }
        if self
            .touch
            .is_some_and(|t| t.mode == TouchMode::DragHit && t.output == output)
        {
            return false;
        }
        self.release_inner(output)
    }

    pub(super) fn release_inner(&mut self, output: OutputId) -> bool {
        let Some(idx) = self.open else {
            return false;
        };
        if self.editing() {
            let Some(d) = self.edit.drag.take() else {
                return false;
            };
            let at = self.edit.drop_at.take();
            if d.moved {
                let changed = if let Some(to) = at {
                    self.edit_change(idx, |c, _, _, _| crate::edit::move_card(c, d.card, to))
                } else {
                    false
                };
                if !changed {
                    let (w, h) = self.screen_for(output);
                    self.update_edit_hover_on(output, w, h);
                }
                return true;
            }
            return false;
        }
        let Some((po, ci, hit)) = self.pressed else {
            return false;
        };
        if po != output {
            return false;
        }
        self.pressed = None;
        self.card_event(idx, ci, &Event::Release { hit })
    }

    pub(super) fn scroll_panel(
        &mut self,
        output: OutputId,
        idx: usize,
        screen: (f64, f64),
        at: (f64, f64),
        step: f64,
    ) -> bool {
        let ((w, h), (x, y)) = (screen, at);
        if self.editing() {
            let (pbox, _, content_h) = self.layout_local(idx, w, h);
            let (bar, add) = self.edit_rects(idx, w, h);
            if self.edit.adding && crate::edit::inside(add, x, y) {
                let cards = self.edit_cards(idx);
                let entries = crate::edit::add_entries(&cards, &self.apps);
                let total_h =
                    crate::edit::add_content_height(&entries, self.edit.expanded_add.as_deref());
                let view_h = crate::edit::add_viewport(add).3;
                let max = (total_h - view_h).max(0.0);
                let next = (self.edit.add_scroll + step).clamp(0.0, max);
                if (next - self.edit.add_scroll).abs() > 0.01 {
                    self.edit.add_scroll = next;
                    return true;
                }
                return false;
            }
            if crate::edit::inside(bar, x, y) {
                return false;
            }
            if crate::edit::inside(pbox, x, y) {
                let max = (content_h - pbox.3).max(0.0);
                let next = (self.panel_scroll + step).clamp(0.0, max);
                if (next - self.panel_scroll).abs() > 0.01 {
                    self.panel_scroll = next;
                    self.update_edit_hover_on(output, w, h);
                    return true;
                }
            }
            return false;
        }
        let (pbox, _, content_h) = self.layout_local(idx, w, h);
        if crate::edit::inside(pbox, x, y) {
            let max = (content_h - pbox.3).max(0.0);
            let next = (self.panel_scroll + step).clamp(0.0, max);
            if (next - self.panel_scroll).abs() > 0.01 {
                self.panel_scroll = next;
                return true;
            }
        }
        false
    }

    /// Scrolls by `dy` pixels (1:1 touchpad movement) at `(x, y)` on `output`.
    pub fn scroll_on(&mut self, output: OutputId, w: f64, h: f64, x: f64, y: f64, dy: f64) -> bool {
        self.scroll_axis_on(output, (w, h), (x, y), dy, 0.0)
    }

    /// Scrolls by `notches` discrete wheel steps at `(x, y)` on `output` (one notch = 36 px).
    pub fn scroll_notches_on(
        &mut self,
        output: OutputId,
        w: f64,
        h: f64,
        x: f64,
        y: f64,
        notches: f64,
    ) -> bool {
        self.scroll_axis_on(
            output,
            (w, h),
            (x, y),
            notches * crate::card::SCROLL_NOTCH_PX,
            notches,
        )
    }

    /// Scrolls by `dy` pixels, also telling notch-based cards when `notches` came from discrete steps.
    pub fn scroll_axis_on(
        &mut self,
        output: OutputId,
        screen: (f64, f64),
        at: (f64, f64),
        dy: f64,
        notches: f64,
    ) -> bool {
        if self.visible_output().is_some_and(|id| id != output) {
            return false;
        }
        let Some(idx) = self.open else {
            return false;
        };
        if !self.editing() {
            if let Some(ci) = self.card_at_on(output, at) {
                let hit = self
                    .hit_at_on(output, at)
                    .filter(|(c, _)| *c == ci)
                    .map(|(_, h)| h);
                if self.card_event(idx, ci, &Event::Scroll { hit, dy, notches }) {
                    return true;
                }
            }
        }
        self.scroll_panel(output, idx, screen, at, dy)
    }

    #[cfg(test)]
    pub fn scroll(&mut self, x: f64, y: f64, dy: f64) -> bool {
        let (w, h) = self.open_screen();
        let out = self
            .open_output
            .or(self.last_output)
            .unwrap_or_else(|| screen_key(w, h));
        self.scroll_on(out, w, h, x, y, dy)
    }

    #[cfg(test)]
    pub fn scroll_notches(&mut self, x: f64, y: f64, notches: f64) -> bool {
        let (w, h) = self.open_screen();
        let out = self
            .open_output
            .or(self.last_output)
            .unwrap_or_else(|| screen_key(w, h));
        self.scroll_notches_on(out, w, h, x, y, notches)
    }

    pub(super) fn drag_autoscroll(&mut self, output: OutputId, w: f64, h: f64, dt_ms: f64) -> bool {
        let Some(idx) = self.open else {
            return false;
        };
        if !self.editing() || !self.edit.drag.is_some_and(|d| d.moved) {
            return false;
        }
        let Some((x, y)) = self.pointer_for(output) else {
            return false;
        };
        let (pbox, local_rects, content_h) = self.layout_local(idx, w, h);
        let max_scroll = (content_h - pbox.3).max(0.0);
        if max_scroll <= 0.0 || x < pbox.0 || x > pbox.0 + pbox.2 {
            return false;
        }
        let edge = 36.0;
        let speed = if y < pbox.1 + edge {
            -((1.0 - ((y - pbox.1).max(0.0) / edge)) * 420.0)
        } else if y > pbox.1 + pbox.3 - edge {
            (1.0 - ((pbox.1 + pbox.3 - y).max(0.0) / edge)) * 420.0
        } else {
            0.0
        };
        if speed == 0.0 {
            return false;
        }
        let next = (self.panel_scroll + speed * (dt_ms / 1000.0)).clamp(0.0, max_scroll);
        if (next - self.panel_scroll).abs() < 0.01 {
            return false;
        }
        self.panel_scroll = next;
        let drag_idx = self.edit.drag.unwrap().card;
        let cards = self.edit_cards(idx);
        self.edit.drop_at = crate::edit::drop_index(
            &local_rects,
            cards.len(),
            drag_idx,
            x - pbox.0,
            y - pbox.1 + next,
        );
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings::parse_power;
    use crate::shell::tests::*;

    #[test]
    fn scale_1_5_scrolls_and_hits_in_logical_pixels() {
        // A 1920x1080 display at scale 1.5 has a 1280x720 logical viewport and a 1920x1080 device buffer.
        let mut sh = shell(
            "SUPER+A bottom right 380 calendar/clock:m settings/network settings/bluetooth settings/power settings/session windows/list",
        );
        sh.apply(
            "settings",
            "power",
            &parse_power("* balanced:\n  power-saver:\n  performance:\n") as &dyn std::any::Any,
        );
        sh.windows = (1..=6)
            .map(|i| Window {
                id: i,
                title: format!("Win {i}"),
                app_id: "firefox".into(),
                focused: i == 1,
            })
            .collect();
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        // At 1920x1080 logical (scale 1.0) the cards fit; at 1280x720 logical (scale 1.5 on a
        // 1920x1080 panel) the same 380 px column scrolls, in logical px.
        let (one, _, _) = sh.layout_local(idx, 1920.0, 1080.0);
        assert_eq!(one.2, 380.0);
        let (lw, lh) = (1280.0, 720.0);
        let (pbox, local_rects, content_h) = sh.layout_local(idx, lw, lh);
        assert_eq!(pbox.2, 380.0);
        assert!((pbox.0 + pbox.2 - (lw - MARGIN)).abs() < 0.5);
        assert!(content_h > pbox.3 && local_rects.iter().all(|(_, r)| r.0 == 0.0));

        let out: OutputId = 15;
        assert!(sh.open_panel_on(Some(out), idx, false, Instant::now()));
        sh.open_ms = f64::INFINITY;
        // scrolled to the end: the window list (the last card) is in view
        sh.panel_scroll = content_h - pbox.3;
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 1920, 1080).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        cr.scale(1.5, 1.5);
        assert_eq!(sh.draw_on(out, &cr, lw, lh), Some(pbox));
        let frames = sh.frame_hits(Some(out));
        // the window list is the last card
        let win_hit = frames
            .last()
            .and_then(|(_, _, hits)| hits.list.iter().find(|(id, _)| *id == 1).map(|h| h.1))
            .expect("first window row hit, in view");
        assert!(win_hit.1 >= pbox.1 - 0.5 && win_hit.1 + win_hit.3 <= pbox.1 + pbox.3 + 0.5);
        assert!(sh.click_on(out, lw, lh, win_hit.0 + 4.0, win_hit.1 + 4.0, Button::Left));
        assert!(sh
            .actions
            .iter()
            .any(|(a, ..)| matches!(a, Action::Focus(1))));
    }

    #[test]
    fn a_click_on_a_mode_queues_its_command_and_a_click_outside_closes() {
        let mut sh = shell("SUPER+A bottom right 380 settings/power");
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        sh.apply(
            "settings",
            "power",
            &parse_power("* balanced:\n  power-saver:\n  performance:\n") as &dyn std::any::Any,
        );
        assert!(sh.open_panel(idx, false, Instant::now()));
        sh.open_ms = f64::INFINITY;
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 1920, 1080).unwrap();
        let cr = cairo::Context::new(&surface).unwrap();
        sh.draw(&cr, 1920.0, 1080.0);
        // the third word, Performance, of the choice: find it among the hits
        let (ci, r) = {
            let f = sh.frame.borrow();
            let (ci, _, hits) = &f[0];
            (*ci, hits.list.iter().find(|(id, _)| *id == 2).unwrap().1)
        };
        assert_eq!(ci, 0);
        assert!(sh.click(1920.0, 1080.0, r.0 + 5.0, r.1 + 5.0, Button::Left));
        assert_eq!(sh.actions.len(), 1);
        assert_eq!(
            sh.actions[0].0,
            Action::Run("powerprofilesctl set 'performance' >/dev/null 2>&1".into())
        );
        assert_eq!(
            (sh.actions[0].1.as_str(), sh.actions[0].2),
            ("settings/power", "settings")
        );
        assert!(
            sh.click(1920.0, 1080.0, 10.0, 10.0, Button::Left),
            "outside: closes"
        );
        assert!(sh.open.is_none());
    }

    #[test]
    fn a_hover_panel_opens_from_its_strip_and_closes_when_left() {
        let mut sh = shell("SUPER+A bottom right 380 hover settings/power");
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        let s = sh.hover_strip(idx, 1920.0, 1080.0);
        let t0 = Instant::now();
        assert!(!sh.pointer_motion_at(1920.0, 1080.0, s.0 + 10.0, s.1 + 1.0, t0));
        assert!(sh.open.is_none());
        let t1 = t0 + std::time::Duration::from_millis(HOVER_OPEN_MS as u64);
        assert!(sh.hover_tick(t1));
        assert_eq!(sh.open, Some(idx));
        assert!(sh.opened_by_hover);
        assert!(!sh.pointer_motion_at(1920.0, 1080.0, 100.0, 100.0, t1));
        assert_eq!(sh.open, Some(idx));
        assert!(
            sh.hover_tick(t1 + std::time::Duration::from_millis(HOVER_CLOSE_MS as u64)),
            "left past grace: closed"
        );
        assert!(sh.open.is_none());
    }

    #[test]
    fn hover_dwell_ignores_fast_pass_and_grace_cancels_on_reentry() {
        let mut sh = shell("SUPER+A bottom right 380 hover settings/power");
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        let s = sh.hover_strip(idx, 1920.0, 1080.0);
        let t0 = Instant::now();

        // Fast pass through the 2 px strip (leaves within 40 ms) does not open.
        assert!(!sh.pointer_motion_at(1920.0, 1080.0, s.0 + 10.0, s.1 + 1.0, t0));
        assert!(sh.hover_timeout(t0).is_some());
        assert!(!sh.hover_tick(t0 + std::time::Duration::from_millis(40)));
        assert!(!sh.pointer_motion_at(
            1920.0,
            1080.0,
            s.0 + 10.0,
            s.1 - 20.0,
            t0 + std::time::Duration::from_millis(50)
        ));
        assert!(sh
            .hover_timeout(t0 + std::time::Duration::from_millis(50))
            .is_none());
        assert!(!sh.hover_tick(t0 + std::time::Duration::from_millis(300)));
        assert!(sh.open.is_none());

        // Leaving via pointer_leave_at while dwelling also cancels opening.
        let t_enter = t0 + std::time::Duration::from_millis(400);
        sh.pointer_motion_at(1920.0, 1080.0, s.0 + 10.0, s.1 + 1.0, t_enter);
        sh.pointer_leave_at(t_enter + std::time::Duration::from_millis(30));
        assert!(!sh.hover_tick(t_enter + std::time::Duration::from_millis(200)));
        assert!(sh.open.is_none());

        // Resting in the strip for HOVER_OPEN_MS opens it.
        let t_dwell = t_enter + std::time::Duration::from_millis(300);
        sh.pointer_motion_at(1920.0, 1080.0, s.0 + 10.0, s.1 + 1.0, t_dwell);
        assert!(!sh.hover_tick(t_dwell + std::time::Duration::from_millis(119)));
        assert!(sh.hover_tick(t_dwell + std::time::Duration::from_millis(120)));
        assert_eq!(sh.open, Some(idx));

        // Leave the hover box, then return within HOVER_CLOSE_MS: stays open.
        let t_out = t_dwell + std::time::Duration::from_millis(200);
        let b = sh.hover_box(idx, 1920.0, 1080.0);
        sh.pointer_motion_at(1920.0, 1080.0, 50.0, 50.0, t_out);
        assert!(!sh.hover_tick(t_out + std::time::Duration::from_millis(200)));
        assert_eq!(sh.open, Some(idx));
        sh.pointer_motion_at(
            1920.0,
            1080.0,
            b.0 + 10.0,
            b.1 + 10.0,
            t_out + std::time::Duration::from_millis(210),
        );
        assert!(!sh.hover_tick(t_out + std::time::Duration::from_millis(500)));
        assert_eq!(sh.open, Some(idx));

        // Leave again (via pointer_leave_at) and stay out past HOVER_CLOSE_MS: closes.
        let t_leave = t_out + std::time::Duration::from_millis(600);
        sh.pointer_leave_at(t_leave);
        assert!(!sh.hover_tick(t_leave + std::time::Duration::from_millis(249)));
        assert!(sh.hover_tick(t_leave + std::time::Duration::from_millis(250)));
        assert!(sh.open.is_none());
    }

    #[test]
    fn two_screen_sizes_keep_separate_frames_pointers_and_clicks() {
        let mut sh = shell("SUPER+A top right 380 settings/power");
        sh.apply(
            "settings",
            "power",
            &parse_power("* balanced:\n  power-saver:\n  performance:\n") as &dyn std::any::Any,
        );
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        assert!(sh.open_panel(idx, false, Instant::now()));
        sh.open_ms = f64::INFINITY;

        // Draw first on 1920x1080, then on 1280x800 (different right-aligned x coordinates).
        let surf1 = cairo::ImageSurface::create(cairo::Format::ARgb32, 1920, 1080).unwrap();
        let cr1 = cairo::Context::new(&surf1).unwrap();
        sh.draw(&cr1, 1920.0, 1080.0);
        let hit_1920 = sh.frame.borrow()[0].2.list[2].1;
        assert!(hit_1920.0 > 1500.0);

        let surf2 = cairo::ImageSurface::create(cairo::Format::ARgb32, 1280, 800).unwrap();
        let cr2 = cairo::Context::new(&surf2).unwrap();
        sh.draw(&cr2, 1280.0, 800.0);
        let hit_1280 = sh.frame.borrow()[0].2.list[2].1;
        assert!(hit_1280.0 < 1280.0);

        // Pointer motion on 1920x1080 must not highlight 1280x800.
        assert!(sh.pointer_motion(1920.0, 1080.0, hit_1920.0 + 4.0, hit_1920.1 + 4.0));
        assert!(sh.pointer_for(screen_key(1920.0, 1080.0)).is_some());
        assert_eq!(sh.pointer_for(screen_key(1280.0, 800.0)), None);

        // Clicking with 1920x1080 coordinates hits the card drawn at 1920x1080, not closed by 1280x800's box.
        assert!(sh.click(
            1920.0,
            1080.0,
            hit_1920.0 + 4.0,
            hit_1920.1 + 4.0,
            Button::Left
        ));
        assert_eq!(sh.open, Some(idx));
        assert!(
            sh.actions
                .iter()
                .any(|(a, _, _, _)| matches!(a, Action::Run(cmd) if cmd.contains("performance"))),
            "expected performance action, got {:?}",
            sh.actions.iter().map(|(a, _, _, _)| a).collect::<Vec<_>>()
        );
    }

    #[test]
    fn scroll_moves_touchpad_pixels_one_to_one_and_wheel_notch_by_36px() {
        // B3: pixel values move 1:1 and discrete wheel notches move 36 px.
        let mut sh = Shell::new(
            Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 settings/power settings/session settings/sound settings/network settings/bluetooth\"\n",
            ),
            Vec::new(),
        );
        sh.apply(
            "settings",
            "power",
            &parse_power("* balanced:\n  power-saver:\n  performance:\n") as &dyn std::any::Any,
        );
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        assert!(sh.open_panel(idx, false, Instant::now()));
        sh.open_ms = f64::INFINITY;
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 600, 300).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let pbox = sh.draw(&cr, 600.0, 300.0).unwrap();
        let (_, _, content_h) = sh.layout_local(idx, 600.0, 300.0);
        assert!(content_h > pbox.3 + 80.0);

        let sx = pbox.0 + 10.0;
        let sy = pbox.1 + 10.0;
        assert!(sh.scroll(sx, sy, 2.5));
        assert!((sh.panel_scroll - 2.5).abs() < 1e-6);
        assert!(sh.scroll_notches(sx, sy, 1.0));
        assert!((sh.panel_scroll - 38.5).abs() < 1e-6);
    }

    #[test]
    fn scroll_handles_touchpad_sizes_and_card_to_panel_handover() {
        // F3: touchpad-sized pixel values (0.5, 2.5, 3.5, 15), 1 notch (36 px),
        // and handover from a scrollable card's own scroll to panel_scroll.
        let mut sh = Shell::new(
            Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 settings/power windows/list settings/session\"\n",
            ),
            Vec::new(),
        );
        sh.apply(
            "settings",
            "power",
            &parse_power("* balanced:\n  power-saver:\n  performance:\n") as &dyn std::any::Any,
        );
        sh.windows = (1..=10)
            .map(|i| Window {
                id: i,
                title: format!("Win {i}"),
                app_id: "kitty".into(),
                focused: i == 1,
            })
            .collect();
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        assert!(sh.open_panel(idx, false, Instant::now()));
        sh.open_ms = f64::INFINITY;

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 600, 340).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let pbox = sh.draw(&cr, 600.0, 340.0).unwrap();
        let (_, _, content_h) = sh.layout_local(idx, 600.0, 340.0);
        assert!(content_h > pbox.3 + 80.0);

        // Over settings/power (non-scrolling card): pixel amounts apply 1:1.
        let power_pt = {
            let f = sh.frame.borrow();
            let r = f[0].1;
            (r.0 + 10.0, r.1 + 10.0)
        };
        for dy in [0.5, 2.5, 3.5, 15.0] {
            let before = sh.panel_scroll;
            assert!(sh.scroll(power_pt.0, power_pt.1, dy));
            assert!(
                (sh.panel_scroll - (before + dy)).abs() < 1e-6,
                "scroll({dy}) moved {} instead of {dy}",
                sh.panel_scroll - before
            );
        }
        assert!((sh.panel_scroll - 21.5).abs() < 1e-6);
        assert!(sh.scroll_notches(power_pt.0, power_pt.1, 1.0));
        assert!((sh.panel_scroll - 57.5).abs() < 1e-6);

        // Reset panel scroll and test card -> panel handover on windows/list (10 windows, 6 visible -> max_scroll = 4).
        sh.panel_scroll = 0.0;
        sh.draw(&cr, 600.0, 340.0);
        let win_pt = {
            let f = sh.frame.borrow();
            let r = f[1].1;
            (r.0 + 10.0, r.1 + 40.0)
        };
        for _ in 0..4 {
            assert!(sh.scroll_notches(win_pt.0, win_pt.1, 1.0));
            assert_eq!(sh.panel_scroll, 0.0, "card consumes notches until bottom");
        }
        // At card's bottom, next notch hands over to panel_scroll.
        assert!(sh.scroll_notches(win_pt.0, win_pt.1, 1.0));
        assert!((sh.panel_scroll - 36.0).abs() < 1e-6);
    }
}
