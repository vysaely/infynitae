//! Touchscreen input: finger down, motion, up and cancel on a panel.

use super::*;

impl Shell {
    #[cfg(test)]
    pub fn touch_down(&mut self, w: f64, h: f64, id: i32, x: f64, y: f64) -> bool {
        let out = self.open_output.unwrap_or_else(|| screen_key(w, h));
        self.touch_down_on(out, w, h, id, x, y)
    }

    /// Touch finger down on `output` of size `w x h` at `(x, y)`.
    pub fn touch_down_on(
        &mut self,
        output: OutputId,
        w: f64,
        h: f64,
        id: i32,
        x: f64,
        y: f64,
    ) -> bool {
        if self.touch.is_some() {
            return false;
        }
        if self.visible_output().is_some_and(|oid| oid != output) {
            return false;
        }
        if let Some((po, _, _)) = self.pressed {
            let _ = self.release_inner(po);
        }
        if self.edit.drag.is_some() {
            self.edit.drag = None;
            self.edit.drop_at = None;
        }
        if self.ask.ask.on {
            self.last_output = Some(output);
            self.last_screen = (w, h);
            self.touch = Some(TouchState {
                id,
                output,
                w,
                h,
                start: (x, y),
                last: (x, y),
                mode: TouchMode::Pending,
            });
            return crate::ask::hover(&mut self.ask, w, h, x, y);
        }
        if self.window_menu.is_some() || self.shortcuts.open {
            self.last_output = Some(output);
            self.last_screen = (w, h);
            self.touch = Some(TouchState {
                id,
                output,
                w,
                h,
                start: (x, y),
                last: (x, y),
                mode: TouchMode::Pending,
            });
            let hid = std::mem::take(&mut self.keyboard);
            let hovered_now = self.hit_at_on(output, (x, y));
            let changed = hovered_now != self.hovered.get(&output).copied();
            match hovered_now {
                Some(h) => {
                    self.hovered.insert(output, h);
                }
                None => {
                    self.hovered.remove(&output);
                }
            }
            return changed || hid;
        }
        let Some(idx) = self.open else {
            return false;
        };
        self.last_output = Some(output);
        self.last_screen = (w, h);
        let hid = std::mem::take(&mut self.keyboard);
        if !self.editing() && self.edit_menu.is_none() {
            let ((ox, oy, pw, ph), _) = self.layout(idx, w, h);
            if x >= ox && x <= ox + pw && y >= oy && y <= oy + ph {
                if let Some((ci, hit)) = self.hit_at_on(output, (x, y)) {
                    if self.hit_is_draggable_on(output, ci, hit) {
                        self.touch = Some(TouchState {
                            id,
                            output,
                            w,
                            h,
                            start: (x, y),
                            last: (x, y),
                            mode: TouchMode::DragHit,
                        });
                        self.pointers.insert(output, (x, y));
                        self.pointer = Some((x, y));
                        let changed = self.click_inner(output, w, h, x, y, Button::Left);
                        return changed || hid;
                    }
                }
            }
        }
        self.touch = Some(TouchState {
            id,
            output,
            w,
            h,
            start: (x, y),
            last: (x, y),
            mode: TouchMode::Pending,
        });
        let hovered_now = if !self.editing() && self.edit_menu.is_none() {
            self.hit_at_on(output, (x, y))
        } else {
            None
        };
        let changed = hovered_now != self.hovered.get(&output).copied();
        match hovered_now {
            Some(h) => {
                self.hovered.insert(output, h);
            }
            None => {
                self.hovered.remove(&output);
            }
        }
        changed || hid
    }

    /// Touch finger moved to `(x, y)`: drags sliders or vertically scrolls the panel.
    pub fn touch_motion(&mut self, id: i32, x: f64, y: f64) -> bool {
        let Some(mut t) = self.touch.filter(|t| t.id == id) else {
            return false;
        };
        if self.ask.ask.on {
            t.last = (x, y);
            self.touch = Some(t);
            return crate::ask::hover(&mut self.ask, t.w, t.h, x, y);
        }
        if self.window_menu.is_some() {
            if t.mode == TouchMode::Pending {
                let dx = x - t.start.0;
                let dy = y - t.start.1;
                if dx * dx + dy * dy < TOUCH_DRAG_PX * TOUCH_DRAG_PX {
                    t.last = (x, y);
                    self.touch = Some(t);
                    return false;
                }
                t.mode = TouchMode::Moved;
                t.last = (x, y);
                self.touch = Some(t);
                self.pointers.remove(&t.output);
                self.pointer = None;
                return self.hovered.remove(&t.output).is_some();
            }
            t.last = (x, y);
            self.touch = Some(t);
            return false;
        }
        if self.shortcuts.open {
            if t.mode == TouchMode::Pending {
                let dx = x - t.start.0;
                let dy = y - t.start.1;
                if dx * dx + dy * dy < TOUCH_DRAG_PX * TOUCH_DRAG_PX {
                    t.last = (x, y);
                    self.touch = Some(t);
                    return false;
                }
                t.mode = if dy.abs() >= dx.abs() {
                    TouchMode::Scroll
                } else {
                    TouchMode::Moved
                };
                let step = t.last.1 - y;
                t.last = (x, y);
                let mode = t.mode;
                let (w, h) = (t.w, t.h);
                let out = t.output;
                self.touch = Some(t);
                self.pointers.remove(&out);
                self.pointer = None;
                let lit = self.hovered.remove(&out).is_some();
                if mode == TouchMode::Scroll {
                    let r = crate::shortcuts::rect(w, h, &self.cfg.keys, &self.shortcuts.query);
                    if crate::shortcuts::inside(r, x, y) {
                        let max = crate::shortcuts::max_scroll(
                            w,
                            h,
                            &self.cfg.keys,
                            &self.shortcuts.query,
                        );
                        let next = (self.shortcuts.scroll + step).clamp(0.0, max);
                        if (next - self.shortcuts.scroll).abs() > 0.01 {
                            self.shortcuts.scroll = next;
                            return true;
                        }
                    }
                }
                return lit;
            }
            if t.mode == TouchMode::Scroll {
                let step = t.last.1 - y;
                t.last = (x, y);
                let (w, h) = (t.w, t.h);
                self.touch = Some(t);
                let max = crate::shortcuts::max_scroll(w, h, &self.cfg.keys, &self.shortcuts.query);
                let next = (self.shortcuts.scroll + step).clamp(0.0, max);
                if (next - self.shortcuts.scroll).abs() > 0.01 {
                    self.shortcuts.scroll = next;
                    return true;
                }
                return false;
            }
            t.last = (x, y);
            self.touch = Some(t);
            return false;
        }
        let Some(idx) = self.open else {
            self.touch = None;
            return false;
        };
        match t.mode {
            TouchMode::DragHit => {
                t.last = (x, y);
                self.touch = Some(t);
                self.pointers.insert(t.output, (x, y));
                self.pointer = Some((x, y));
                if let Some((po, ci, hit)) = self.pressed {
                    if po == t.output {
                        return self.card_event(idx, ci, &Event::Drag { hit, at: (x, y) });
                    }
                }
                false
            }
            TouchMode::Pending => {
                let dx = x - t.start.0;
                let dy = y - t.start.1;
                if dx * dx + dy * dy < TOUCH_DRAG_PX * TOUCH_DRAG_PX {
                    t.last = (x, y);
                    self.touch = Some(t);
                    return false;
                }
                if dy.abs() >= dx.abs() {
                    t.mode = TouchMode::Scroll;
                    let step = t.last.1 - y;
                    t.last = (x, y);
                    self.touch = Some(t);
                    self.pointers.remove(&t.output);
                    self.pointer = None;
                    let lit = self.hovered.remove(&t.output).is_some();
                    return self.scroll_panel(t.output, idx, (t.w, t.h), t.start, step) || lit;
                }
                t.mode = TouchMode::Moved;
                t.last = (x, y);
                self.touch = Some(t);
                self.pointers.remove(&t.output);
                self.pointer = None;
                self.hovered.remove(&t.output).is_some()
            }
            TouchMode::Scroll => {
                let step = t.last.1 - y;
                t.last = (x, y);
                self.touch = Some(t);
                self.scroll_panel(t.output, idx, (t.w, t.h), t.start, step)
            }
            TouchMode::Moved => {
                t.last = (x, y);
                self.touch = Some(t);
                false
            }
        }
    }

    /// Touch finger lifted: taps when it did not drag, or releases an active slider drag.
    pub fn touch_up(&mut self, id: i32) -> bool {
        let Some(t) = self.touch.take() else {
            return false;
        };
        if t.id != id {
            self.touch = Some(t);
            return false;
        }
        match t.mode {
            TouchMode::DragHit => {
                let mut changed = self.release_inner(t.output);
                changed |= self.pointers.remove(&t.output).is_some();
                changed |= self.pointer.take().is_some();
                changed |= self.hovered.remove(&t.output).is_some();
                changed
            }
            TouchMode::Pending => {
                let mut changed = if self.ask.ask.on {
                    crate::ask::hover(&mut self.ask, t.w, t.h, t.start.0, t.start.1)
                } else {
                    false
                };
                self.pointers.insert(t.output, t.start);
                self.pointer = Some(t.start);
                changed |= self.click_inner(t.output, t.w, t.h, t.start.0, t.start.1, Button::Left);
                changed |= self.release_inner(t.output);
                self.pointers.remove(&t.output);
                self.pointer = None;
                changed |= self.hovered.remove(&t.output).is_some();
                changed |= self.edit.hover_card.take().is_some();
                changed
            }
            TouchMode::Scroll | TouchMode::Moved => {
                self.pointers.remove(&t.output);
                self.pointer = None;
                self.hovered.remove(&t.output).is_some()
            }
        }
    }

    /// Compositor cancelled the active touch sequence: drops pressed/drag state without committing.
    pub fn touch_cancel(&mut self) -> bool {
        let Some(t) = self.touch.take() else {
            return false;
        };
        let mut changed = false;
        if t.mode == TouchMode::DragHit {
            if let (Some(idx), Some((po, ci, hit))) = (self.open, self.pressed.take()) {
                if po == t.output {
                    changed |= self.card_event(idx, ci, &Event::Cancel { hit });
                }
            } else {
                self.pressed = None;
            }
        }
        self.pointers.remove(&t.output);
        self.pointer = None;
        changed |= t.mode == TouchMode::DragHit;
        changed |= self.hovered.remove(&t.output).is_some();
        changed |= self.edit.hover_card.take().is_some();
        changed
    }

    /// Cancels an active touch sequence if it belongs to `output` (e.g. output removed).
    pub fn touch_cancel_on(&mut self, output: OutputId) -> bool {
        if self.touch.is_some_and(|t| t.output == output) {
            self.touch_cancel()
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings::parse_power;
    use crate::shell::tests::*;

    #[test]
    fn touch_tap_toggles_switch_and_clears_hover_without_sticking() {
        // B4: touch down/up on a switch taps it and leaves no pointer/hover stuck.
        use crate::cards::settings_rows::ConfigData;
        let mut sh = shell("SUPER+A top right 380 settings/games");
        let mut data = ConfigData::default();
        data.programs.insert("gamemoded".into());
        data.default.insert("GAME_MODE".into(), "false".into());
        sh.apply("settings", "config", &data as &dyn std::any::Any);
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        assert!(sh.open_panel(idx, false, Instant::now()));
        sh.open_ms = f64::INFINITY;

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 1280, 800).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        sh.draw(&cr, 1280.0, 800.0);

        let switch_r = {
            let f = sh.frame.borrow();
            f[0].2
                .list
                .iter()
                .find(|(id, _)| *id == 101)
                .expect("GAME_MODE switch hit")
                .1
        };
        let sx = switch_r.0 + switch_r.2 / 2.0;
        let sy = switch_r.1 + switch_r.3 / 2.0;

        assert!(sh.touch_down(1280.0, 800.0, 0, sx, sy));
        // Switch has not toggled on down yet (could become a vertical scroll drag).
        assert!(sh.actions.is_empty());
        assert!(sh.touch_up(0));
        assert!(
            sh.actions
                .iter()
                .any(|(a, ..)| matches!(a, Action::Run(cmd) if cmd.contains("GAME_MODE") && cmd.contains("true"))),
            "tap on switch must queue toggle command: {:?}",
            sh.actions
        );
        assert_eq!(sh.pointer, None);
        assert!(sh.hovered.is_empty());
        assert_eq!(sh.pressed, None);
    }

    #[test]
    fn touch_vertical_drag_on_non_draggable_scrolls_panel_without_tapping() {
        // B4: vertical touch drag starting on a switch/button scrolls the panel and does not click.
        use crate::cards::settings_rows::ConfigData;
        let mut sh = Shell::new(
            Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 settings/games settings/power settings/session\"\n",
            ),
            Vec::new(),
        );
        let mut data = ConfigData::default();
        data.programs.insert("gamemoded".into());
        data.default.insert("GAME_MODE".into(), "false".into());
        sh.apply("settings", "config", &data as &dyn std::any::Any);
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

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 600, 320).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        sh.draw(&cr, 600.0, 320.0);

        let switch_r = sh.frame.borrow()[0]
            .2
            .list
            .iter()
            .find(|(id, _)| *id == 101)
            .unwrap()
            .1;
        let sx = switch_r.0 + switch_r.2 / 2.0;
        let sy = switch_r.1 + switch_r.3 / 2.0;

        assert!(sh.touch_down(600.0, 320.0, 0, sx, sy));
        assert!(sh.touch_motion(0, sx, sy - 40.0));
        assert!((sh.panel_scroll - 40.0).abs() < 1e-6);
        let _ = sh.touch_up(0);
        assert!(
            sh.actions.is_empty(),
            "vertical scroll drag must not activate switch"
        );
    }

    #[test]
    fn touch_drag_on_slider_moves_it_and_cancel_clears_state() {
        // B4: touch drag on a slider moves it; touch_cancel drops pressed/drag state without sticking.
        use crate::cards::settings_rows::ConfigData;
        use crate::cards::sound::{Brightness, VolumeData};
        let mut sh = shell("SUPER+A top right 380 sound/volume settings/input");
        sh.apply(
            "sound",
            "volume",
            &VolumeData {
                level: Some(20),
                muted: false,
                output: "Speakers".into(),
                ..Default::default()
            } as &dyn std::any::Any,
        );
        sh.apply(
            "sound",
            "brightness",
            &Brightness(None) as &dyn std::any::Any,
        );
        let mut data = ConfigData::default();
        data.default
            .insert("KEYBOARD_REPEAT_DELAY".into(), "400".into());
        sh.apply("settings", "config", &data as &dyn std::any::Any);
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        assert!(sh.open_panel(idx, false, Instant::now()));
        sh.open_ms = f64::INFINITY;

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 1280, 800).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        sh.draw(&cr, 1280.0, 800.0);

        // 1) Drag volume slider to 75% via touch.
        let vol_r = sh.frame.borrow()[0]
            .2
            .list
            .iter()
            .find(|(id, _)| *id == 1)
            .unwrap()
            .1;
        let sy = vol_r.1 + vol_r.3 / 2.0;
        assert!(sh.touch_down(1280.0, 800.0, 1, vol_r.0 + vol_r.2 * 0.25, sy));
        assert!(sh.touch_motion(1, vol_r.0 + vol_r.2 * 0.75, sy));
        assert!(sh.touch_up(1));
        assert!(sh
            .actions
            .iter()
            .any(|(a, ..)| matches!(a, Action::Run(cmd) if cmd.contains("75%"))));
        assert_eq!(sh.pressed, None);
        assert_eq!(sh.pointer, None);
        sh.actions.clear();

        // 2) Start touch drag on settings/input slider (commits only on release), then touch_cancel.
        let input_slider_r = sh.frame.borrow()[1]
            .2
            .list
            .iter()
            .find(|(id, _)| sh.frame.borrow()[1].2.is_draggable(*id))
            .expect("input slider marked draggable")
            .1;
        let iy = input_slider_r.1 + input_slider_r.3 / 2.0;
        assert!(sh.touch_down(
            1280.0,
            800.0,
            2,
            input_slider_r.0 + input_slider_r.2 * 0.2,
            iy
        ));
        assert!(sh.touch_motion(2, input_slider_r.0 + input_slider_r.2 * 0.9, iy));
        assert!(sh.pressed.is_some());
        assert!(sh.touch_cancel());
        assert_eq!(sh.pressed, None);
        assert_eq!(sh.pointer, None);
        assert!(sh.hovered.is_empty());
        assert!(
            sh.actions.is_empty(),
            "cancelled slider drag must not commit: {:?}",
            sh.actions
        );
        // Subsequent pointer motion only hovers and must not drag or commit the cancelled slider.
        assert!(sh.pointer_motion(1280.0, 800.0, input_slider_r.0 + 10.0, iy));
        assert_eq!(sh.pressed, None);
        assert!(!sh.release());
        assert!(sh.actions.is_empty());
    }

    #[test]
    fn multi_output_scroll_and_touch_only_act_on_visible_output() {
        let mut sh = Shell::new(
            Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 settings/power settings/session settings/sound settings/network\"\n",
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
        let out1: OutputId = 1;
        let out2: OutputId = 2;
        assert!(sh.open_panel_on(Some(out2), idx, false, Instant::now()));
        sh.open_ms = f64::INFINITY;

        let surf2 = cairo::ImageSurface::create(cairo::Format::ARgb32, 600, 300).unwrap();
        let cr2 = cairo::Context::new(&surf2).unwrap();
        let pbox2 = sh.draw_on(out2, &cr2, 600.0, 300.0).unwrap();

        // Scroll and touch on out1 (not showing the panel) do nothing.
        assert!(!sh.scroll_on(out1, 1920.0, 1080.0, 100.0, 100.0, 10.0));
        assert!(!sh.scroll_notches_on(out1, 1920.0, 1080.0, 100.0, 100.0, 1.0));
        assert!(!sh.touch_down_on(out1, 1920.0, 1080.0, 0, 100.0, 100.0));
        assert_eq!(sh.panel_scroll, 0.0);
        assert_eq!(sh.open, Some(idx));

        // Scroll on out2 moves panel_scroll on out2.
        assert!(sh.scroll_on(out2, 600.0, 300.0, pbox2.0 + 10.0, pbox2.1 + 10.0, 5.0));
        assert!((sh.panel_scroll - 5.0).abs() < 1e-6);

        // Touch tap on performance button on out2 activates it.
        sh.draw_on(out2, &cr2, 600.0, 300.0);
        let perf_r = sh.frame_hits(Some(out2))[0].2.list[2].1;
        assert!(sh.touch_down_on(out2, 600.0, 300.0, 0, perf_r.0 + 4.0, perf_r.1 + 4.0));
        assert!(sh.touch_up(0));
        assert!(sh
            .actions
            .iter()
            .any(|(a, ..)| matches!(a, Action::Run(cmd) if cmd.contains("performance"))));
    }
}
