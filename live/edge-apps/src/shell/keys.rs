//! Keyboard input: typing a query, the keys that move the selection, scroll it into view and press it.

use super::*;

impl Shell {
    /// What was typed, as if typed now (renders and tests).
    pub fn type_text(&mut self, q: &str) {
        if let Some(idx) = self.open {
            self.query = q.to_string();
            self.send_text(idx);
        }
    }

    /// A new panel's typing starts empty, and its cards hear so (a search shows everything again).
    pub(super) fn reset_keys(&mut self, idx: usize) {
        self.query.clear();
        self.selected = None;
        self.keyboard = false;
        for ci in 0..self.panels[idx].cards.len() {
            self.card_event(idx, ci, &Event::Text(""));
        }
    }

    /// The query to every card of the open panel; the selection starts over. True when one
    /// changed.
    pub(super) fn send_text(&mut self, idx: usize) -> bool {
        self.selected = None;
        let q = self.query.clone();
        let mut changed = false;
        for ci in 0..self.panels[idx].cards.len() {
            changed |= self.card_event(idx, ci, &Event::Text(&q));
            if self.open != Some(idx) {
                break;
            }
        }
        changed
    }

    /// Every hit of the last frame in order (the cards top to bottom, each card's in the order it
    /// added them), once each.
    pub(super) fn hit_order(&self) -> Vec<(usize, u32, Rect)> {
        let mut out: Vec<(usize, u32, Rect)> = Vec::new();
        let frames = self.frames.borrow();
        let list: &Vec<(usize, Rect, Hits)> = if let Some(id) = self.open_output {
            if let Some(f) = frames.get(&id) {
                &f.hits
            } else {
                &self.frame.borrow()
            }
        } else {
            &self.frame.borrow()
        };
        for (ci, _, hits) in list.iter() {
            for (id, r) in &hits.list {
                if !out.iter().any(|(c, i, _)| c == ci && i == id) {
                    out.push((*ci, *id, *r));
                }
            }
        }
        out
    }

    pub(super) fn scroll_to_hit(&mut self, idx: usize, r: Rect, is_first: bool) {
        let ((w, h), frame_scroll) = {
            let frames = self.frames.borrow();
            if let Some(f) = self.open_output.and_then(|id| frames.get(&id)) {
                (f.screen, f.scroll)
            } else {
                (self.frame_screen.get(), self.frame_scroll.get())
            }
        };
        let (pbox, _, content_h) = self.layout_local(idx, w, h);
        let max_scroll = (content_h - pbox.3).max(0.0);
        if max_scroll <= 0.0 {
            self.panel_scroll = 0.0;
            return;
        }
        let cur_scroll = self.panel_scroll.clamp(0.0, max_scroll);
        let ry = r.1 + frame_scroll - cur_scroll;
        let top = pbox.1;
        let bot = pbox.1 + pbox.3;
        let mut next = cur_scroll;
        if ry + r.3 > bot {
            next = cur_scroll + (ry + r.3 - bot);
        }
        let ry_after = r.1 + frame_scroll - next;
        if ry_after < top {
            next = cur_scroll + (ry - top);
        }
        if is_first && (r.1 + frame_scroll + r.3) <= bot {
            next = 0.0;
        }
        self.panel_scroll = next.clamp(0.0, max_scroll);
    }

    /// A key while a panel is open. True when something must be drawn again.
    pub fn key(&mut self, k: Key) -> bool {
        // the question first: it takes the keyboard while it asks, panel or not
        if self.ask.ask.on {
            let (sym, ch) = match k {
                Key::Escape => (0xff1b, None),
                Key::Enter => (0xff0d, None),
                Key::Up => (0xff52, None),
                Key::Down => (0xff54, None),
                Key::Left => (0xff51, None),
                Key::Right => (0xff53, None),
                Key::Home => (0xff50, None),
                Key::End => (0xff57, None),
                Key::Tab => (0xff09, None),
                Key::BackTab => (0xfe20, None),
                Key::Backspace => (0xff08, None),
                Key::Char(c) => (0, Some(c)),
                Key::PageUp | Key::PageDown | Key::Delete | Key::Super(_) => return false,
            };
            return crate::ask::key(&mut self.ask, sym, ch);
        }
        if let Some(wm) = self.window_menu {
            return match k {
                Key::Escape => {
                    self.window_menu = None;
                    self.hovered.clear();
                    self.selected = None;
                    self.keyboard = false;
                    true
                }
                Key::Up | Key::Down | Key::Tab | Key::BackTab | Key::Home | Key::End => {
                    let len = wm.items().len() as u32;
                    let at = self.selected.map(|(_, h)| h);
                    let next = match k {
                        Key::Home => 0,
                        Key::End => len - 1,
                        _ => match (at, self.keyboard) {
                            (Some(i), false) => i,
                            (None, _) => {
                                if matches!(k, Key::Up | Key::BackTab) {
                                    len - 1
                                } else {
                                    0
                                }
                            }
                            (Some(i), true) => {
                                if matches!(k, Key::Up | Key::BackTab) {
                                    (i + len - 1) % len
                                } else {
                                    (i + 1) % len
                                }
                            }
                        },
                    };
                    self.keyboard = true;
                    self.selected = Some((0, next));
                    true
                }
                Key::Enter => {
                    let idx = self
                        .selected
                        .filter(|_| self.keyboard)
                        .map(|(_, h)| h)
                        .or_else(|| {
                            self.last_output
                                .and_then(|o| self.hovered.get(&o))
                                .map(|&(_, h)| h)
                        });
                    if let Some((action, _, _)) =
                        idx.and_then(|hit| wm.items().get(hit as usize).copied())
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
                        true
                    } else {
                        false
                    }
                }
                _ => false,
            };
        }
        if self.shortcuts.open {
            return match k {
                Key::Escape => self.close_shortcuts(),
                Key::Backspace => {
                    if self.shortcuts.query.pop().is_some() {
                        self.shortcuts.scroll = 0.0;
                        true
                    } else {
                        false
                    }
                }
                Key::Char(c) => {
                    if c.is_control() {
                        return false;
                    }
                    self.shortcuts.query.push(c);
                    self.shortcuts.scroll = 0.0;
                    true
                }
                Key::Tab | Key::BackTab => {
                    self.keyboard = true;
                    self.selected = if self.selected == Some((0, crate::shortcuts::HIT_CHANGE)) {
                        None
                    } else {
                        Some((0, crate::shortcuts::HIT_CHANGE))
                    };
                    true
                }
                Key::Enter => {
                    let selected_change =
                        self.keyboard && self.selected == Some((0, crate::shortcuts::HIT_CHANGE));
                    let hovered_change = self
                        .last_output
                        .and_then(|o| self.hovered.get(&o))
                        .is_some_and(|&(c, h)| c == 0 && h == crate::shortcuts::HIT_CHANGE);
                    if selected_change || hovered_change {
                        self.open_settings_keys(self.shortcuts.output)
                    } else {
                        false
                    }
                }
                Key::Up | Key::Down | Key::PageUp | Key::PageDown | Key::Home | Key::End => {
                    let (w, h) = self.open_screen();
                    let max =
                        crate::shortcuts::max_scroll(w, h, &self.cfg.keys, &self.shortcuts.query);
                    if max <= 0.0 {
                        return false;
                    }
                    let view_h = crate::shortcuts::viewport(crate::shortcuts::rect(
                        w,
                        h,
                        &self.cfg.keys,
                        &self.shortcuts.query,
                    ))
                    .3;
                    let page = (view_h - crate::ui::LIST_ROW_H).max(crate::ui::LIST_ROW_H);
                    let next = match k {
                        Key::Home => 0.0,
                        Key::End => max,
                        Key::PageUp => (self.shortcuts.scroll - page).clamp(0.0, max),
                        Key::PageDown => (self.shortcuts.scroll + page).clamp(0.0, max),
                        Key::Up => (self.shortcuts.scroll - crate::ui::LIST_ROW_H).clamp(0.0, max),
                        Key::Down => {
                            (self.shortcuts.scroll + crate::ui::LIST_ROW_H).clamp(0.0, max)
                        }
                        _ => self.shortcuts.scroll,
                    };
                    if (next - self.shortcuts.scroll).abs() > 0.01 {
                        self.shortcuts.scroll = next;
                        true
                    } else {
                        false
                    }
                }
                Key::Super(_) | Key::Left | Key::Right | Key::Delete => false,
            };
        }
        let Some(idx) = self.open else {
            return false;
        };
        if self.editing() {
            return match k {
                Key::Escape => {
                    if self.edit.adding {
                        self.edit.adding = false;
                        self.edit.expanded_add = None;
                        self.edit.add_scroll = 0.0;
                        true
                    } else {
                        self.finish_edit()
                    }
                }
                Key::PageUp | Key::PageDown | Key::Home | Key::End => {
                    let out = self.active_output_or_last();
                    let (w, h) = self.open_screen();
                    let (pbox, _, content_h) = self.layout_local(idx, w, h);
                    let max = (content_h - pbox.3).max(0.0);
                    let next = match k {
                        Key::Home => 0.0,
                        Key::End => max,
                        Key::PageUp => {
                            let page = (pbox.3 - crate::ui::ROW_H).max(crate::ui::ROW_H);
                            (self.panel_scroll - page).clamp(0.0, max)
                        }
                        _ => {
                            let page = (pbox.3 - crate::ui::ROW_H).max(crate::ui::ROW_H);
                            (self.panel_scroll + page).clamp(0.0, max)
                        }
                    };
                    if (next - self.panel_scroll).abs() > 0.01 {
                        self.panel_scroll = next;
                        self.update_edit_hover_on(out, w, h);
                        true
                    } else {
                        false
                    }
                }
                Key::Super(c) => {
                    let c = c.to_ascii_lowercase();
                    let out = self.open_output;
                    self.finish_edit();
                    if self
                        .open
                        .is_some_and(|i| self.panels[i].panel.key == Some(c))
                    {
                        self.close()
                    } else if let Some(other) =
                        self.panels.iter().position(|p| p.panel.key == Some(c))
                    {
                        self.open_panel_on(out, other, false, Instant::now())
                    } else {
                        true
                    }
                }
                _ => false,
            };
        }
        if k == Key::Escape && self.edit_menu.take().is_some() {
            return true;
        }
        match k {
            Key::Escape => {
                if self.query.is_empty() {
                    self.close()
                } else {
                    self.query.clear();
                    self.send_text(idx);
                    true
                }
            }
            Key::Super(c) => {
                let c = c.to_ascii_lowercase();
                if self.panels[idx].panel.key == Some(c) {
                    self.close()
                } else if let Some(other) = self.panels.iter().position(|p| p.panel.key == Some(c))
                {
                    self.open_panel_on(self.open_output, other, false, Instant::now())
                } else {
                    false
                }
            }
            Key::Up | Key::Down | Key::Tab | Key::BackTab | Key::Home | Key::End => {
                let order = self.hit_order();
                if order.is_empty() {
                    return false;
                }
                let at = self
                    .selected
                    .and_then(|(c, h)| order.iter().position(|(oc, oh, _)| *oc == c && *oh == h));
                // the first arrow while the selection is hidden shows it where it is
                let next = match k {
                    Key::Home => 0,
                    Key::End => order.len() - 1,
                    _ => match (at, self.keyboard) {
                        (Some(i), false) => i,
                        (None, _) => {
                            if matches!(k, Key::Up | Key::BackTab) {
                                order.len() - 1
                            } else {
                                0
                            }
                        }
                        (Some(i), true) => {
                            if matches!(k, Key::Up | Key::BackTab) {
                                (i + order.len() - 1) % order.len()
                            } else {
                                (i + 1) % order.len()
                            }
                        }
                    },
                };
                self.keyboard = true;
                self.selected = Some((order[next].0, order[next].1));
                let top_of_col = next == 0
                    || (order[next].0 != order[next - 1].0
                        && order[next].2 .1 < order[next - 1].2 .1);
                self.scroll_to_hit(idx, order[next].2, top_of_col);
                true
            }
            Key::Left | Key::Right => {
                let (true, Some((ci, hit))) = (self.keyboard, self.selected) else {
                    return false;
                };
                let dir = if k == Key::Left { -1 } else { 1 };
                self.card_event(idx, ci, &Event::Step { hit, dir })
            }
            Key::PageUp | Key::PageDown => {
                let (w, h) = self.open_screen();
                let (pbox, _, content_h) = self.layout_local(idx, w, h);
                let max = (content_h - pbox.3).max(0.0);
                if max <= 0.0 {
                    return false;
                }
                let page = (pbox.3 - crate::ui::ROW_H).max(crate::ui::ROW_H);
                let delta = if k == Key::PageDown { page } else { -page };
                let next = (self.panel_scroll + delta).clamp(0.0, max);
                if (next - self.panel_scroll).abs() > 0.01 {
                    self.panel_scroll = next;
                    true
                } else {
                    false
                }
            }
            Key::Delete => {
                let (true, Some((ci, hit))) = (self.keyboard, self.selected) else {
                    return false;
                };
                self.card_event(idx, ci, &Event::Delete { hit })
            }
            Key::Enter => {
                if let (true, Some((ci, hit))) = (self.keyboard, self.selected) {
                    let r = self
                        .hit_order()
                        .into_iter()
                        .find(|(c, h, _)| *c == ci && *h == hit)
                        .map_or((0.0, 0.0, 0.0, 0.0), |(_, _, r)| r);
                    let frame_scroll = self
                        .open_output
                        .and_then(|id| self.frames.borrow().get(&id).map(|f| f.scroll))
                        .unwrap_or_else(|| self.frame_scroll.get());
                    let dy = frame_scroll - self.panel_scroll;
                    let ev = Event::Press {
                        hit,
                        button: Button::Left,
                        at: (r.0 + r.2 / 2.0, r.1 + dy + r.3 / 2.0),
                    };
                    let changed = self.card_event(idx, ci, &ev);
                    if self.open == Some(idx) {
                        self.card_event(idx, ci, &Event::Release { hit });
                    }
                    return changed;
                }
                // nothing selected: the first card that has a default takes it
                for ci in 0..self.panels[idx].cards.len() {
                    if self.card_event(idx, ci, &Event::Enter) {
                        return true;
                    }
                    if self.open != Some(idx) {
                        return true;
                    }
                }
                false
            }
            Key::Backspace => {
                if self.query.pop().is_some() {
                    self.send_text(idx);
                    true
                } else {
                    false
                }
            }
            Key::Char(c) => {
                if c.is_control() {
                    return false;
                }
                self.query.push(c);
                self.send_text(idx);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings::parse_power;
    use crate::shell::tests::*;

    #[test]
    fn typing_filters_and_enter_takes_the_first_match() {
        let (mut sh, _) = windows_shell();
        frame(&sh);
        for c in "ste".chars() {
            assert!(sh.key(Key::Char(c)));
        }
        assert_eq!(sh.query, "ste");
        assert!(sh.key(Key::Enter));
        let acts: Vec<&Action> = sh.actions.iter().map(|(a, ..)| a).collect();
        assert_eq!(acts, vec![&Action::Focus(3)]);
        // the Focus closes the panel through the core, ClosePanel already did
        assert!(sh.open.is_none());
        assert!(sh.query.is_empty());
    }

    #[test]
    fn escape_clears_the_query_first_then_closes() {
        let (mut sh, _) = windows_shell();
        sh.key(Key::Char('x'));
        assert!(sh.key(Key::Backspace));
        assert!(sh.query.is_empty());
        assert!(!sh.key(Key::Backspace));
        sh.key(Key::Char('x'));
        assert!(sh.key(Key::Escape));
        assert!(sh.open.is_some());
        assert!(sh.key(Key::Escape));
        assert!(sh.open.is_none());
    }

    #[test]
    fn arrows_show_the_selection_then_move_it_and_enter_presses_it() {
        let (mut sh, _) = windows_shell();
        frame(&sh);
        let order = sh.hit_order();
        assert!(order.len() >= 2);
        // the first arrow shows it on the first hit, not past it
        assert!(sh.key(Key::Down));
        assert!(sh.keyboard);
        assert_eq!(sh.selected, Some((order[0].0, order[0].1)));
        sh.key(Key::Down);
        assert_eq!(sh.selected, Some((order[1].0, order[1].1)));
        sh.key(Key::Up);
        sh.key(Key::Up);
        // wraps to the last
        let last = order.last().unwrap();
        assert_eq!(sh.selected, Some((last.0, last.1)));
        // the pointer moving hides it
        assert!(sh.pointer_motion(1280.0, 800.0, 1100.0, 60.0));
        assert!(!sh.keyboard);
        // first row, Enter: its window
        sh.key(Key::Tab);
        sh.selected = Some((order[0].0, order[0].1));
        assert!(sh.key(Key::Enter));
        let acts: Vec<&Action> = sh.actions.iter().map(|(a, ..)| a).collect();
        assert_eq!(acts, vec![&Action::Focus(1)]);
    }

    #[test]
    fn super_with_another_panels_key_switches_to_it() {
        let mut sh = Shell::new(
            Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 windows/list\"\nPANEL_CLIP=\"SUPER+K top center 380 clipboard/history\"\n",
            ),
            Vec::new(),
        );
        let a = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        let v = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "clip")
            .unwrap();
        sh.open_panel(a, false, Instant::now());
        assert!(sh.key(Key::Super('K')));
        assert_eq!(sh.open, Some(v));
        assert!(sh.key(Key::Super('k')));
        assert!(sh.open.is_none());
    }

    #[test]
    fn keyboard_navigation_scrolls_selected_hit_into_panel_box() {
        let mut sh = shell("SUPER+A top right 380 settings/power settings/session windows/list");
        sh.apply(
            "settings",
            "power",
            &parse_power("* balanced:\n  power-saver:\n  performance:\n") as &dyn std::any::Any,
        );
        sh.windows = (1..=6)
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

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 600, 400).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let pbox = sh.draw(&cr, 600.0, 400.0).unwrap();
        let (_, _, content_h) = sh.layout_local(idx, 600.0, 400.0);
        assert!(
            content_h > pbox.3,
            "precondition: panel is taller than screen"
        );

        // Move Down repeatedly without redrawing in between until past the fold.
        let order = sh.hit_order();
        let past_fold = order
            .iter()
            .position(|(_, _, r)| r.1 + r.3 > pbox.1 + pbox.3)
            .expect("at least one hit starts below the fold");
        for _ in 0..=past_fold {
            assert!(sh.key(Key::Down));
        }
        assert!(
            sh.panel_scroll > 0.0,
            "panel_scroll should advance past the fold"
        );
        sh.draw(&cr, 600.0, 400.0);
        let (ci, hit) = sh.selected.unwrap();
        let r = sh
            .hit_order()
            .into_iter()
            .find(|(c, h, _)| *c == ci && *h == hit)
            .unwrap()
            .2;
        assert!(
            r.1 >= pbox.1 && r.1 + r.3 <= pbox.1 + pbox.3,
            "selected rect {r:?} must lie inside pbox {pbox:?}"
        );
    }

    #[test]
    fn tall_scrolled_panel_with_hidden_and_half_cards_walks_all_hits_in_view() {
        let apps = vec![
            App {
                id: "app1".into(),
                name: "App One".into(),
                exec: "true".into(),
                ..Default::default()
            },
            App {
                id: "app2".into(),
                name: "App Two".into(),
                exec: "true".into(),
                ..Default::default()
            },
        ];
        // Includes a hidden card (notifications/list with no notes), a half-width pair,
        // and multiple tall cards that overflow a 400 px high screen.
        let mut sh = Shell::new(
            Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 notifications/list app1/app@half app2/app@half settings/power settings/session windows/list\"\n",
            ),
            apps,
        );
        sh.apply(
            "settings",
            "power",
            &parse_power("* balanced:\n  power-saver:\n  performance:\n") as &dyn std::any::Any,
        );
        sh.windows = (1..=6)
            .map(|i| Window {
                id: i,
                title: format!("Window {i}"),
                app_id: "firefox".into(),
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

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 600, 400).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let pbox = sh.draw(&cr, 600.0, 400.0).unwrap();
        let (_, layout_rects) = sh.layout(idx, 600.0, 400.0);
        // notifications/list (index 0) is Hidden, app1/app and app2/app share one row (half-width).
        assert_eq!(layout_rects[0].0, 1);
        assert_eq!(layout_rects[0].1 .1, layout_rects[1].1 .1);

        let total_hits = sh.hit_order().len();
        assert!(total_hits >= 10);
        let mut saw_scrolled = false;
        for step in 0..total_hits {
            assert!(sh.key(Key::Down));
            sh.draw(&cr, 600.0, 400.0);
            if sh.panel_scroll > 0.0 {
                saw_scrolled = true;
            }
            let (ci, hit) = sh.selected.unwrap();
            let r = sh
                .hit_order()
                .into_iter()
                .find(|(c, h, _)| *c == ci && *h == hit)
                .expect("selected hit present in frame")
                .2;
            assert!(
                r.0 >= pbox.0
                    && r.0 + r.2 <= pbox.0 + pbox.2
                    && r.1 >= pbox.1
                    && r.1 + r.3 <= pbox.1 + pbox.3,
                "step {step}: selected rect {r:?} not inside pbox {pbox:?}"
            );
        }
        assert!(saw_scrolled, "walking all hits must scroll past the fold");

        // Wrapping past the last hit returns to the first hit and scrolls back to top.
        assert!(sh.key(Key::Down));
        sh.draw(&cr, 600.0, 400.0);
        assert_eq!(sh.panel_scroll, 0.0);
        let first = sh.hit_order()[0];
        assert_eq!(sh.selected, Some((first.0, first.1)));
    }

    #[test]
    fn left_and_right_step_selected_volume_slider_and_choice_strip() {
        // B6: Left/Right step selected slider and choice strip.
        use crate::cards::sound::{Brightness, VolumeData};
        let mut sh = shell("SUPER+A top right 380 sound/volume settings/power");
        sh.apply(
            "sound",
            "volume",
            &VolumeData {
                level: Some(50),
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

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 1280, 800).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        sh.draw(&cr, 1280.0, 800.0);

        // Card 0 hits: 0 = mute, 1 = volume slider.
        let order = sh.hit_order();
        assert!(order.iter().any(|(c, h, _)| *c == 0 && *h == 1));
        assert!(sh.key(Key::Down)); // hit 0 (mute)
        assert!(sh.key(Key::Down)); // hit 1 (volume slider)
        assert_eq!(sh.selected, Some((0, 1)));

        assert!(sh.key(Key::Right));
        assert!(sh
            .actions
            .iter()
            .any(|(a, ..)| matches!(a, Action::Run(cmd) if cmd.contains("55%"))));
        sh.actions.clear();

        assert!(sh.key(Key::Left));
        assert!(sh
            .actions
            .iter()
            .any(|(a, ..)| matches!(a, Action::Run(cmd) if cmd.contains("50%"))));
        sh.actions.clear();

        // Move to card 1 (power mode choice strip): step Right from balanced -> performance.
        assert!(sh.key(Key::Down));
        assert_eq!(sh.selected.unwrap().0, 1);
        assert!(sh.key(Key::Right));
        assert!(sh
            .actions
            .iter()
            .any(|(a, ..)| matches!(a, Action::Run(cmd) if cmd.contains("performance"))));
    }

    #[test]
    fn home_end_and_page_keys_navigate_and_scroll_panel() {
        // B6: Home/End pick first/last hit; PageUp/PageDown scroll by height minus one row.
        let mut sh = Shell::new(
            Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 settings/power settings/session windows/list\"\n",
            ),
            Vec::new(),
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

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 600, 360).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let pbox = sh.draw(&cr, 600.0, 360.0).unwrap();
        let (_, _, content_h) = sh.layout_local(idx, 600.0, 360.0);
        let max_scroll = (content_h - pbox.3).max(0.0);
        assert!(max_scroll > 50.0);

        let order = sh.hit_order();
        assert!(sh.key(Key::End));
        assert!(sh.keyboard);
        let last = order.last().unwrap();
        assert_eq!(sh.selected, Some((last.0, last.1)));
        assert!(sh.panel_scroll > 0.0);

        assert!(sh.key(Key::Home));
        assert_eq!(sh.selected, Some((order[0].0, order[0].1)));
        assert_eq!(sh.panel_scroll, 0.0);

        let expected_page = (pbox.3 - crate::ui::ROW_H)
            .max(crate::ui::ROW_H)
            .min(max_scroll);
        assert!(sh.key(Key::PageDown));
        assert!((sh.panel_scroll - expected_page).abs() < 1e-6);
        assert!(sh.key(Key::PageUp));
        assert_eq!(sh.panel_scroll, 0.0);
    }

    #[test]
    fn window_menu_home_end_and_touch_tap() {
        let ev = r#"{"event":"window_menu","element":[42,7],"position":[320.5,180.0],"on_top":true,"maximized":false,"fullscreen":false,"generation":3}"#;
        let wm = parse_window_menu_event(ev).expect("parsed window_menu event");
        let mut sh = Shell::new(Config::from_text(""), Vec::new());
        assert!(sh.open_window_menu(wm));
        frame(&sh);

        assert!(sh.key(Key::End));
        assert!(sh.keyboard);
        assert_eq!(sh.selected, Some((0, 4)));
        assert!(sh.key(Key::Home));
        assert_eq!(sh.selected, Some((0, 0)));

        let dump = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (x4, y4) = parse_box_center(&dump, "hit window/menu 4 ");
        assert!(sh.touch_down_on(1, 1920.0, 1080.0, 0, x4, y4));
        assert!(sh.touch_up(0));
        assert!(!sh.active());
        assert_eq!(
            std::mem::take(&mut sh.actions)[0].0,
            Action::ShellCommand("{\"id\":1,\"cmd\":\"close\",\"element\":[42,7]}\n".into())
        );
    }

    #[test]
    fn delete_only_acts_where_card_has_remove_action() {
        // B6: Delete runs remove/dismiss/close where supported and does nothing otherwise.
        let apps = vec![App {
            id: "kitty".into(),
            name: "Kitty".into(),
            exec: "kitty".into(),
            ..Default::default()
        }];
        let mut sh = Shell::new(
            Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 windows/list notifications/list apps/list\"\n",
            ),
            apps,
        );
        let mut win_cfg = crate::cards::settings_rows::ConfigData::default();
        win_cfg
            .now
            .insert("PLACE_DEV".into(), "10,20,1.0 Dev".into());
        sh.apply("windows", "config", &win_cfg as &dyn std::any::Any);
        sh.windows = vec![Window {
            id: 42,
            title: "Term".into(),
            app_id: "kitty".into(),
            focused: true,
        }];
        sh.notes = vec![crate::card::Note {
            id: 7,
            app: "Mail".into(),
            summary: "New mail".into(),
            body: String::new(),
            actions: Vec::new(),
            urgency: 1,
            time: "12:00".into(),
        }];
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

        // Card 0: windows/list -> hit 1 = Window 42, hit 2 = Close button, hit 3 = Place "Dev".
        assert!(sh.key(Key::Down));
        assert_eq!(sh.selected.unwrap().0, 0);
        assert!(sh.key(Key::Delete));
        assert_eq!(
            sh.actions.iter().map(|(a, ..)| a).collect::<Vec<_>>(),
            vec![&Action::CloseWindow(42)]
        );
        sh.actions.clear();

        // Advance to Place row (hit 3) on card 0: Delete must do nothing.
        while sh.selected.is_some_and(|(c, h)| c == 0 && h < 3) {
            assert!(sh.key(Key::Down));
        }
        assert_eq!(sh.selected, Some((0, 3)));
        assert!(!sh.key(Key::Delete));
        assert!(sh.actions.is_empty());

        // Advance to Card 1 (notifications/list): Delete dismisses note 7.
        while sh.selected.is_some_and(|(c, _)| c < 1) {
            assert!(sh.key(Key::Down));
        }
        assert_eq!(sh.selected.unwrap().0, 1);
        assert!(sh.key(Key::Delete));
        assert_eq!(
            sh.actions.iter().map(|(a, ..)| a).collect::<Vec<_>>(),
            vec![&Action::Dismiss(7)]
        );
        sh.actions.clear();

        // Advance to Card 2 (apps/list): Delete must do nothing.
        while sh.selected.is_some_and(|(c, _)| c < 2) {
            assert!(sh.key(Key::Down));
        }
        assert_eq!(sh.selected.unwrap().0, 2);
        assert!(!sh.key(Key::Delete));
        assert!(sh.actions.is_empty());
    }
}
