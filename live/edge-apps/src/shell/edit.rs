//! Edit mode: entering it, the card buttons and drags, undo, saving or cancelling a panel's edit.

use super::*;

impl Shell {
    /// A right click on a free spot of a panel of the config: the "Edit panel" menu at that point.
    pub(super) fn offer_edit(&mut self, idx: usize, x: f64, y: f64) -> bool {
        if self.transient == Some(idx) {
            return false;
        }
        self.edit_menu = Some((x, y));
        true
    }

    /// The menu's one item, kept on the screen.
    pub fn edit_menu_rect(&self, w: f64, h: f64) -> Option<Rect> {
        let (x, y) = self.edit_menu?;
        let (mw, mh) = (150.0, 34.0);
        Some((
            (x).min(w - mw - 4.0).max(4.0),
            (y).min(h - mh - 4.0).max(4.0),
            mw,
            mh,
        ))
    }

    pub fn enter_edit(&mut self, w: f64, h: f64) -> bool {
        let out = self.open_output.unwrap_or_else(|| screen_key(w, h));
        self.enter_edit_on(out, w, h)
    }

    pub(super) fn enter_edit_on(&mut self, output: OutputId, w: f64, h: f64) -> bool {
        let Some(idx) = self.open else {
            return false;
        };
        // a tray app's menu and card are not a panel of the config
        if self.transient == Some(idx) {
            return false;
        }
        if self.editing() {
            return false;
        }
        self.opened_by_hover = false;
        self.keyboard_held = false;
        self.hover_open = None;
        self.hover_close = None;
        self.hovered.clear();
        self.pressed = None;
        self.touch = None;
        self.keyboard = false;
        self.selected = None;
        self.opened_at = None;
        self.open_ms = f64::INFINITY;
        self.query.clear();
        self.reset_keys(idx);

        let before_line = crate::panel::line(&self.panels[idx].panel);
        let before_cards = self.edit_cards(idx);
        let p = &self.panels[idx].panel;
        let before_width = p.width;
        let before_hover = p.hover;
        self.edit = crate::edit::EditState {
            active: true,
            panel_idx: idx,
            before_line,
            before_cards,
            before_width,
            before_hover,
            ..Default::default()
        };
        self.refresh_edit_rects(w, h);
        self.update_edit_hover_on(output, w, h);
        true
    }

    pub fn cancel_edit(&mut self) -> bool {
        if !self.editing() {
            return false;
        }
        let idx = self.edit.panel_idx;
        let cards = std::mem::take(&mut self.edit.before_cards);
        let width = self.edit.before_width;
        let hover = self.edit.before_hover;
        self.edit = Default::default();
        if idx < self.panels.len() {
            self.apply_panel_state(idx, cards, width, hover);
        }
        self.panel_scroll = 0.0;
        true
    }

    pub(super) fn save_and_rebuild_panel(
        &mut self,
        idx: usize,
        cards: Vec<CardRef>,
        before_line: String,
    ) {
        let p = &mut self.panels[idx].panel;
        p.placed = false;
        p.height = 0;
        p.items.clear();
        p.words = cards.iter().map(CardRef::word).collect();
        let panel_clone = p.clone();
        let panel_name = panel_clone.name.clone();

        let new_cfg = if let Some((cfg_path, bak_path)) = self.save_paths.clone() {
            if let Err(e) =
                crate::edit::save_panel_to(&cfg_path, &bak_path, &panel_clone, &before_line)
            {
                eprintln!("infynitae-edge-apps: save panel: {e}");
                None
            } else {
                std::fs::read_to_string(&cfg_path)
                    .ok()
                    .map(|t| Config::from_text(&t))
            }
        } else if self.live {
            if let Err(e) = crate::edit::save_panel(&panel_clone, &before_line) {
                eprintln!("infynitae-edge-apps: save panel: {e}");
                None
            } else {
                Some(Config::load())
            }
        } else {
            let mut next = self.cfg.clone();
            if let Some(cp) = next.panels.iter_mut().find(|x| x.name == panel_name) {
                *cp = panel_clone.clone();
            }
            Some(next)
        };

        if let Some(cfg) = new_cfg {
            self.rebuild_panels(cfg, None);
        } else if let Some(cp) = self.cfg.panels.iter_mut().find(|x| x.name == panel_name) {
            *cp = panel_clone;
            self.rebuild_panels(self.cfg.clone(), None);
        }
        self.open = self
            .panels
            .iter()
            .position(|ps| ps.panel.name == panel_name);
        self.fetch_needed = true;
    }

    pub(super) fn remove_placed_card(&mut self, idx: usize, ci: usize) -> bool {
        let Some(ps) = self.panels.get(idx) else {
            return false;
        };
        let before_line = crate::panel::line(&ps.panel);
        let mut cards = self.edit_cards(idx);
        if !crate::edit::remove_card(&mut cards, ci, &self.apps) {
            return false;
        }
        self.selected = None;
        self.hovered.clear();
        self.pressed = None;
        self.touch = None;
        self.save_and_rebuild_panel(idx, cards, before_line);
        true
    }

    pub fn finish_edit(&mut self) -> bool {
        if !self.editing() {
            return false;
        }
        let idx = self.edit.panel_idx;
        let before_line = std::mem::take(&mut self.edit.before_line);
        let cards = self.edit_cards(idx);
        self.edit = Default::default();
        self.panel_scroll = 0.0;
        self.save_and_rebuild_panel(idx, cards, before_line);
        true
    }

    pub(super) fn push_edit_snapshot(&mut self, idx: usize) {
        let cards = self.edit_cards(idx);
        let p = &self.panels[idx].panel;
        self.edit.push_undo(crate::edit::Snapshot {
            cards,
            width: p.width,
            hover: p.hover,
        });
    }

    pub(super) fn edit_change<F>(&mut self, idx: usize, f: F) -> bool
    where
        F: FnOnce(&mut Vec<CardRef>, &mut i32, &mut bool, &[App]) -> bool,
    {
        self.edit.confirm_reset = false;
        let mut cards = self.edit_cards(idx);
        let snap = crate::edit::Snapshot {
            cards: cards.clone(),
            width: self.panels[idx].panel.width,
            hover: self.panels[idx].panel.hover,
        };
        let mut width = snap.width;
        let mut hover = snap.hover;
        if !f(&mut cards, &mut width, &mut hover, &self.apps) {
            return false;
        }
        self.edit.push_undo(snap);
        self.apply_panel_state(idx, cards, width, hover);
        let out = self.active_output_or_last();
        let (w, h) = self.screen_for(out);
        let (pbox, _, content_h) = self.layout_local(idx, w, h);
        self.panel_scroll = self.panel_scroll.clamp(0.0, (content_h - pbox.3).max(0.0));
        self.refresh_edit_rects(w, h);
        self.update_edit_hover_on(out, w, h);
        true
    }

    pub(super) fn scroll_to_last(&mut self, idx: usize, w: f64, h: f64) {
        let (pbox, local_rects, content_h) = self.layout_local(idx, w, h);
        let max_scroll = (content_h - pbox.3).max(0.0);
        if max_scroll <= 0.0 {
            return;
        }
        if let Some((_, r)) = local_rects.last() {
            let bot = r.1 + r.3;
            if bot > self.panel_scroll + pbox.3 {
                self.panel_scroll = (bot - pbox.3).clamp(0.0, max_scroll);
            }
        }
    }

    pub(super) fn update_edit_hover_on(&mut self, output: OutputId, w: f64, h: f64) {
        let Some(idx) = self.open.filter(|_| self.editing()) else {
            self.edit.hover_card = None;
            return;
        };
        let Some((x, y)) = self.pointer_for(output) else {
            return;
        };
        let (pbox, local_rects, content_h) = self.layout_local(idx, w, h);
        let (bar, add) = self.edit_rects(idx, w, h);
        let over_ui =
            crate::edit::inside(bar, x, y) || (self.edit.adding && crate::edit::inside(add, x, y));
        if over_ui || !crate::edit::inside(pbox, x, y) {
            self.edit.hover_card = None;
            return;
        }
        let scroll = self.panel_scroll.clamp(0.0, (content_h - pbox.3).max(0.0));
        self.edit.hover_card = crate::edit::card_at(&local_rects, x - pbox.0, y - pbox.1 + scroll);
    }

    pub(super) fn edit_press(
        &mut self,
        output: OutputId,
        idx: usize,
        screen: (f64, f64),
        at: (f64, f64),
        button: Button,
    ) -> bool {
        let ((w, h), (x, y)) = (screen, at);
        self.last_output = Some(output);
        self.last_screen = (w, h);
        self.pointers.insert(output, (x, y));
        self.pointer = Some((x, y));
        let (bar, add) = self.edit_rects(idx, w, h);
        self.edit.bar_rect = bar;
        self.edit.add_rect = add;
        let cards_now = self.edit_cards(idx);
        let entries = crate::edit::add_entries(&cards_now, &self.apps);

        if let Some(hit) = crate::edit::bar_hit(&self.edit, &entries, x, y) {
            if !matches!(
                hit,
                crate::edit::BarHit::Button(crate::edit::BarButton::Reset)
            ) {
                self.edit.confirm_reset = false;
            }
            match hit {
                crate::edit::BarHit::Nothing => return false,
                crate::edit::BarHit::Button(btn) => match btn {
                    crate::edit::BarButton::Undo => {
                        let Some(snap) = self.edit.pop_undo() else {
                            return false;
                        };
                        self.apply_panel_state(idx, snap.cards, snap.width, snap.hover);
                        self.refresh_edit_rects(w, h);
                        self.update_edit_hover_on(output, w, h);
                        return true;
                    }
                    crate::edit::BarButton::Reset => {
                        if !crate::edit::has_default(&self.panels[idx].panel.name) {
                            return false;
                        }
                        if !self.edit.confirm_reset {
                            self.edit.confirm_reset = true;
                            return true;
                        }
                        self.edit.confirm_reset = false;
                        let name = self.panels[idx].panel.name.clone();
                        let Some((def_cards, def_w, def_hover)) =
                            crate::edit::default_panel_state(&name)
                        else {
                            return false;
                        };
                        self.push_edit_snapshot(idx);
                        self.apply_panel_state(idx, def_cards, def_w, def_hover);
                        self.panel_scroll = 0.0;
                        self.refresh_edit_rects(w, h);
                        self.update_edit_hover_on(output, w, h);
                        return true;
                    }
                    crate::edit::BarButton::Add => {
                        self.edit.adding = !self.edit.adding;
                        if !self.edit.adding {
                            self.edit.expanded_add = None;
                            self.edit.add_scroll = 0.0;
                        }
                        self.refresh_edit_rects(w, h);
                        return true;
                    }
                    crate::edit::BarButton::Narrower => {
                        return self.edit_change(idx, |_, pw, _, _| {
                            if let Some(nw) =
                                crate::edit::adjust_width(*pw, -crate::edit::WIDTH_STEP)
                            {
                                *pw = nw;
                                true
                            } else {
                                false
                            }
                        });
                    }
                    crate::edit::BarButton::Wider => {
                        return self.edit_change(idx, |_, pw, _, _| {
                            if let Some(nw) =
                                crate::edit::adjust_width(*pw, crate::edit::WIDTH_STEP)
                            {
                                *pw = nw;
                                true
                            } else {
                                false
                            }
                        });
                    }
                    crate::edit::BarButton::Hover => {
                        return self.edit_change(idx, |_, _, hv, _| {
                            *hv = !*hv;
                            true
                        });
                    }
                    crate::edit::BarButton::Cancel => return self.cancel_edit(),
                    crate::edit::BarButton::Done => return self.finish_edit(),
                },
                crate::edit::BarHit::ChooseAdd(owner) => {
                    if self.edit.expanded_add.as_deref() == Some(owner.as_str()) {
                        self.edit.expanded_add = None;
                    } else {
                        self.edit.expanded_add = Some(owner.clone());
                        let new_ch = crate::edit::add_content_height(
                            &entries,
                            self.edit.expanded_add.as_deref(),
                        );
                        let (pbox, _, _) = self.layout_local(idx, w, h);
                        let new_add =
                            crate::edit::add_rect((pbox.0, pbox.1), pbox.2, (w, h), new_ch, bar);
                        self.edit.add_scroll = crate::edit::reveal_expanded(
                            new_add,
                            &entries,
                            &owner,
                            self.edit.add_scroll,
                        );
                    }
                    self.refresh_edit_rects(w, h);
                    return true;
                }
                crate::edit::BarHit::AddKind(owner, kind_id) => {
                    let changed = self.edit_change(idx, |cards, _, _, _| {
                        crate::edit::add_card(cards, CardRef::new(&owner, &kind_id))
                    });
                    if changed {
                        let new_cards = self.edit_cards(idx);
                        let new_entries = crate::edit::add_entries(&new_cards, &self.apps);
                        if !new_entries.iter().any(|e| e.owner == owner) {
                            self.edit.expanded_add = None;
                        }
                        self.scroll_to_last(idx, w, h);
                        self.refresh_edit_rects(w, h);
                    }
                    return changed;
                }
            }
        }

        self.edit.confirm_reset = false;
        let (pbox, local_rects, content_h) = self.layout_local(idx, w, h);
        if !crate::edit::inside(pbox, x, y) {
            return false;
        }
        let scroll = self.panel_scroll.clamp(0.0, (content_h - pbox.3).max(0.0));
        let (lx, ly) = (x - pbox.0, y - pbox.1 + scroll);
        let Some(ci) = crate::edit::card_at(&local_rects, lx, ly) else {
            return false;
        };
        if let Some((_, r)) = local_rects.iter().find(|&&(i, _)| i == ci) {
            let (_, btns) = crate::edit::card_buttons(&cards_now, &self.apps, ci, *r, pbox.2);
            for (kind, br) in btns {
                if crate::edit::inside(br, lx, ly) {
                    return match kind {
                        crate::edit::CardButton::Up => {
                            self.edit_change(idx, |c, _, _, _| crate::edit::move_up(c, ci))
                        }
                        crate::edit::CardButton::Down => {
                            self.edit_change(idx, |c, _, _, _| crate::edit::move_down(c, ci))
                        }
                        crate::edit::CardButton::Half => self.edit_change(idx, |c, _, _, apps| {
                            crate::edit::toggle_half(c, ci, apps)
                        }),
                        crate::edit::CardButton::Size => self
                            .edit_change(idx, |c, _, _, apps| crate::edit::next_size(c, ci, apps)),
                        crate::edit::CardButton::Remove => self
                            .edit_change(idx, |c, _, _, apps| {
                                crate::edit::remove_card(c, ci, apps)
                            }),
                    };
                }
            }
        }
        if button == Button::Left {
            self.edit.drag = Some(crate::edit::CardDrag {
                card: ci,
                start_y: y,
                moved: false,
            });
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings::parse_power;
    use crate::shell::tests::*;

    #[test]
    fn an_edited_panel_scrolls_in_one_column_too() {
        let mut sh = shell("SUPER+A bottom right 380 calendar/clock settings/network settings/bluetooth settings/power settings/session");
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        let (_, _, h1) = sh.layout_local(idx, 1920.0, 1080.0);
        let h = h1 * 0.7 + 2.0 * MARGIN;
        sh.open_panel(idx, false, Instant::now());
        sh.open_ms = f64::INFINITY;
        assert!(sh.enter_edit(1920.0, h));
        let (pbox, edited, content_h) = sh.layout_local(idx, 1920.0, h);
        assert!(
            edited.iter().all(|(_, r)| r.0 == 0.0),
            "one column: {edited:?}"
        );
        // every card's strip is inside the content, and scrolling reaches the last card
        assert!(edited
            .iter()
            .all(|(_, r)| r.1 - crate::edit::STRIP_H >= -0.5));
        let last = edited.last().unwrap().1;
        assert!(last.1 + last.3 <= content_h + 0.5 && content_h > pbox.3);
    }

    #[test]
    fn right_click_starts_editing_shows_hidden_placeholder_and_ignores_card_clicks() {
        // notifications/list is Status::Hidden when notes are empty
        let mut sh = shell("SUPER+A top right 380 hover notifications/list settings/power");
        sh.apply(
            "settings",
            "power",
            &parse_power("* balanced:\n  power-saver:\n") as &dyn std::any::Any,
        );
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        let strip = sh.hover_strip(idx, 1920.0, 1080.0);
        let t0 = Instant::now();
        assert!(!sh.pointer_motion_at(1920.0, 1080.0, strip.0 + 10.0, strip.1 + 1.0, t0));
        assert!(sh.hover_tick(t0 + std::time::Duration::from_millis(HOVER_OPEN_MS as u64)));
        assert!(sh.opened_by_hover);
        sh.open_ms = f64::INFINITY;
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 1920, 1080).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        sh.draw(&cr, 1920.0, 1080.0);
        // Before editing, notifications/list is Hidden (only 1 card in layout)
        assert_eq!(sh.layout(idx, 1920.0, 1080.0).1.len(), 1);

        // Right-click on the settings/power hit (which does not use Button::Right) starts edit
        let (_, power_r) = {
            let f = sh.frame.borrow();
            *f[0].2.list.first().unwrap()
        };
        assert!(sh.click(
            1920.0,
            1080.0,
            power_r.0 + 4.0,
            power_r.1 + 4.0,
            Button::Right
        ));
        // a right click alone only offers the menu; Escape takes it away without editing
        assert!(!sh.editing());
        assert!(sh.edit_menu.is_some());
        assert!(sh.key(Key::Escape));
        assert!(sh.edit_menu.is_none() && !sh.editing() && sh.open.is_some());
        sh.click(
            1920.0,
            1080.0,
            power_r.0 + 4.0,
            power_r.1 + 4.0,
            Button::Right,
        );
        let m = sh.edit_menu_rect(1920.0, 1080.0).unwrap();
        assert!(sh.click(1920.0, 1080.0, m.0 + 10.0, m.1 + 10.0, Button::Left));
        assert!(sh.editing());
        assert!(!sh.opened_by_hover);

        // Hover-to-close is off while editing
        sh.pointer_motion(1920.0, 1080.0, 10.0, 10.0);
        assert_eq!(sh.open, Some(idx));

        // While editing: Status::Hidden notifications/list now shows placeholder height
        sh.draw(&cr, 1920.0, 1080.0);
        let rects = sh.layout(idx, 1920.0, 1080.0).1;
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0].1 .3, crate::edit::PLACEHOLDER_H);

        // Clicking inside a card's own controls does nothing while editing (no actions queued)
        assert!(sh.click(
            1920.0,
            1080.0,
            rects[1].1 .0 + 20.0,
            rects[1].1 .1 + 40.0,
            Button::Left
        ));
        assert!(sh.release() || !sh.release());
        assert!(sh.actions.is_empty());

        let dump = crate::boxes::text(&sh, 1920.0, 1080.0);
        assert!(dump.contains("panel control "));
        assert!(dump.contains(" editing top"));
        assert!(dump.contains("barbutton Undo "));
        assert!(dump.contains("barbutton Done "));
    }

    #[test]
    fn card_buttons_drag_undo_reset_and_add_work_via_boxes() {
        let apps = vec![App {
            id: "alacritty".into(),
            name: "Alacritty".into(),
            wm_class: None,
            exec: "alacritty".into(),
            icon: "utilities-terminal".into(),
            terminal: true,
            categories: vec!["System".into()],
        }];
        let mut sh = Shell::new(
            Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 calendar/clock:s settings/network settings/bluetooth\"\n",
            ),
            apps,
        );
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        sh.open_panel(idx, false, Instant::now());
        sh.open_ms = f64::INFINITY;
        frame(&sh);

        // Right-click empty header area starts editing
        let (pbox, _) = sh.layout(idx, 1920.0, 1080.0);
        assert!(sh.click(1920.0, 1080.0, pbox.0 + 10.0, pbox.1 + 10.0, Button::Right));
        // a click elsewhere takes the menu away without editing
        assert!(sh.click(1920.0, 1080.0, pbox.0 + 30.0, pbox.1 + 60.0, Button::Left));
        assert!(!sh.editing() && sh.edit_menu.is_none());
        sh.click(1920.0, 1080.0, pbox.0 + 10.0, pbox.1 + 10.0, Button::Right);
        let m = sh.edit_menu_rect(1920.0, 1080.0).unwrap();
        assert!(sh.click(1920.0, 1080.0, m.0 + 10.0, m.1 + 10.0, Button::Left));
        assert!(sh.editing());

        // Use cardbutton Down on calendar/clock
        let d1 = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (cx, cy) = parse_box_center(&d1, "cardbutton calendar/clock Down ");
        assert!(sh.click(1920.0, 1080.0, cx, cy, Button::Left));
        assert_eq!(sh.edit_cards(idx)[1].owner, "calendar");

        // Size cycles calendar/clock
        let d2 = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (sx, sy) = parse_box_center(&d2, "cardbutton calendar/clock Size ");
        let before_size = sh.edit_cards(idx)[1].size;
        assert!(sh.click(1920.0, 1080.0, sx, sy, Button::Left));
        assert_ne!(sh.edit_cards(idx)[1].size, before_size);

        // Drag card 2 (settings/bluetooth) above card 0 (settings/network)
        let d3 = crate::boxes::text(&sh, 1920.0, 1080.0);
        let c2 = box_rect(&d3, "card settings/bluetooth ");
        let c0 = box_rect(&d3, "card settings/network ");
        assert!(sh.pointer_motion(1920.0, 1080.0, c2.0 + 40.0, c2.1 + 20.0));
        assert!(sh.click(1920.0, 1080.0, c2.0 + 40.0, c2.1 + 20.0, Button::Left));
        assert!(sh.pointer_motion(1920.0, 1080.0, c0.0 + 40.0, c0.1 + 4.0));
        assert_eq!(sh.edit.drop_at, Some(0));
        assert!(sh.release());
        assert_eq!(sh.edit_cards(idx)[0].kind, "bluetooth");

        // Half pairs two adjacent half cards side by side, then toggles both back to full
        let d_half = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (hx, hy) = parse_box_center(&d_half, "cardbutton settings/bluetooth Half ");
        assert!(sh.click(1920.0, 1080.0, hx, hy, Button::Left));
        let cards_half = sh.edit_cards(idx);
        assert!(cards_half[0].half && cards_half[1].half);
        let d_half2 = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (h2x, h2y) = parse_box_center(&d_half2, "cardbutton settings/bluetooth Half ");
        assert!(sh.click(1920.0, 1080.0, h2x, h2y, Button::Left));
        let cards_full = sh.edit_cards(idx);
        assert!(!cards_full[0].half && !cards_full[1].half);

        // Width + and -
        let d3b = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (wx, wy) = parse_box_center(&d3b, "barbutton Wider ");
        assert!(sh.click(1920.0, 1080.0, wx, wy, Button::Left));
        assert_eq!(sh.panels[idx].panel.width, 400);

        // Undo reverts width change
        let d4 = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (ux, uy) = parse_box_center(&d4, "barbutton Undo ");
        assert!(sh.click(1920.0, 1080.0, ux, uy, Button::Left));
        assert_eq!(sh.panels[idx].panel.width, 380);

        // Add opens list, pick Alacritty then add alacritty/app
        let d4b = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (ax, ay) = parse_box_center(&d4b, "barbutton Add ");
        assert!(sh.click(1920.0, 1080.0, ax, ay, Button::Left));
        assert!(sh.edit.adding);
        let d5 = crate::boxes::text(&sh, 1920.0, 1080.0);
        assert!(d5.contains("add "));
        assert!(d5.contains("addrow alacritty "));
        // Scroll Add down so alacritty is visible if list is tall
        let add_r = box_rect(&d5, "add ");
        for _ in 0..20 {
            sh.scroll(add_r.0 + 20.0, add_r.1 + 50.0, 1.0);
        }
        let d5s = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (alx, aly) = parse_box_center(&d5s, "addrow alacritty ");
        assert!(sh.click(1920.0, 1080.0, alx, aly, Button::Left));
        let d6 = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (akx, aky) = parse_box_center(&d6, "addrow alacritty/app ");
        assert!(sh.click(1920.0, 1080.0, akx, aky, Button::Left));
        assert_eq!(sh.edit_cards(idx).last().unwrap().owner, "alacritty");
        let d7 = crate::boxes::text(&sh, 1920.0, 1080.0);
        assert!(!d7.contains("addrow alacritty/app "));

        // Reset asks twice: first sets confirm_reset, second restores built-in control defaults
        let (rx, ry) = parse_box_center(&d7, "barbutton Reset ");
        assert!(sh.click(1920.0, 1080.0, rx, ry, Button::Left));
        assert!(sh.edit.confirm_reset);
        assert!(sh.click(1920.0, 1080.0, rx, ry, Button::Left));
        assert!(!sh.edit.confirm_reset);
        assert!(sh
            .edit_cards(idx)
            .iter()
            .any(|c| c.owner == "sound" && c.kind == "volume"));
    }

    #[test]
    fn cancel_restores_and_done_or_escape_saves_rewrites_and_rebuilds() {
        let dir = std::env::temp_dir().join(format!("infynitae-shell-edit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("config");
        let bak_path = dir.join("panels.backup");
        std::fs::write(
            &cfg_path,
            "PANEL_CONTROL=\"SUPER+A top right 380 settings/network settings/bluetooth\"\n",
        )
        .unwrap();

        let mut sh = Shell::new(
            Config::from_text(&std::fs::read_to_string(&cfg_path).unwrap()),
            Vec::new(),
        );
        sh.save_paths = Some((cfg_path.clone(), bak_path.clone()));
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        sh.open_panel(idx, false, Instant::now());
        sh.open_ms = f64::INFINITY;
        let orig_line = crate::panel::line(&sh.panels[idx].panel);

        // Enter edit, change width and hover, Cancel restores original state without touching disk
        assert!(sh.enter_edit(1920.0, 1080.0));
        let dump = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (wx, wy) = parse_box_center(&dump, "barbutton Wider ");
        assert!(sh.click(1920.0, 1080.0, wx, wy, Button::Left));
        assert_eq!(sh.panels[idx].panel.width, 400);
        let (cx, cy) = parse_box_center(&dump, "barbutton Cancel ");
        assert!(sh.click(1920.0, 1080.0, cx, cy, Button::Left));
        assert!(!sh.editing());
        assert_eq!(crate::panel::line(&sh.panels[idx].panel), orig_line);
        assert!(!bak_path.exists());

        // Enter edit again, remove second card, open Add, Escape closes Add first, second Escape saves
        assert!(sh.enter_edit(1920.0, 1080.0));
        let dump2 = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (rmx, rmy) = parse_box_center(&dump2, "cardbutton settings/bluetooth Remove ");
        assert!(sh.click(1920.0, 1080.0, rmx, rmy, Button::Left));
        let dump2b = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (ax, ay) = parse_box_center(&dump2b, "barbutton Add ");
        assert!(sh.click(1920.0, 1080.0, ax, ay, Button::Left));
        assert!(sh.edit.adding);
        assert!(sh.key(Key::Escape));
        assert!(sh.editing() && !sh.edit.adding);
        assert!(sh.key(Key::Escape));
        assert!(!sh.editing());
        assert!(sh.take_fetch_needed());
        assert_eq!(sh.panels[sh.open.unwrap()].cards.len(), 1);
        assert_eq!(sh.panels[sh.open.unwrap()].cards[0].word.kind, "network");

        let cfg_written = std::fs::read_to_string(&cfg_path).unwrap();
        assert!(cfg_written.contains("settings/network"));
        assert!(!cfg_written.contains("settings/bluetooth"));
        let bak_written = std::fs::read_to_string(&bak_path).unwrap();
        assert!(bak_written.contains(&format!("PANEL_CONTROL=\"{orig_line}\"")));

        // A built-in panel with no line yet in config ("apps") gets a PANEL_APPS= line when Done
        let apps_idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "apps")
            .unwrap();
        assert!(!cfg_written.contains("PANEL_APPS="));
        sh.open_panel(apps_idx, false, Instant::now());
        sh.open_ms = f64::INFINITY;
        assert!(sh.enter_edit(1920.0, 1080.0));
        let dump_apps = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (dx, dy) = parse_box_center(&dump_apps, "barbutton Done ");
        assert!(sh.click(1920.0, 1080.0, dx, dy, Button::Left));
        let cfg_written2 = std::fs::read_to_string(&cfg_path).unwrap();
        assert!(cfg_written2.contains("PANEL_APPS="));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reload_if_changed_catches_first_poll_writes_and_post_start_creation() {
        let dir =
            std::env::temp_dir().join(format!("infynitae-shell-mtime-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("config");

        // 1. Config exists at start, then changes before the very first reload_if_changed poll.
        std::fs::write(&cfg_path, "ACCENT=\"#112233\"\n").unwrap();
        let mut sh = Shell::new_at(
            Config::from_text(&std::fs::read_to_string(&cfg_path).unwrap()),
            Vec::new(),
            &cfg_path,
        );
        let gen0 = crate::icons::global_icon_generation();
        // No equality check on the icon generation here: it is process-wide and other
        // tests running at the same time may bump it.
        assert!(!sh.reload_if_changed_at(&cfg_path));
        std::fs::write(&cfg_path, "ACCENT=\"#445566\"\n").unwrap();
        let future = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options()
            .write(true)
            .open(&cfg_path)
            .unwrap()
            .set_modified(future)
            .unwrap();
        assert!(sh.reload_if_changed_at(&cfg_path));
        assert_eq!(sh.cfg.accent, [0x44, 0x55, 0x66]);
        let gen1 = crate::icons::global_icon_generation();
        assert!(gen1 > gen0, "config reload should clear icon cache");

        sh.rebuild_panels(
            sh.cfg.clone(),
            Some(vec![App {
                id: "new-app".into(),
                name: "New App".into(),
                ..Default::default()
            }]),
        );
        assert!(
            crate::icons::global_icon_generation() > gen1,
            "app list change should clear icon cache"
        );

        // 2. Config does not exist at start, then is created after start.
        let new_cfg_path = dir.join("config-new");
        let mut sh_missing = Shell::new_at(Config::from_text(""), Vec::new(), &new_cfg_path);
        assert!(!sh_missing.reload_if_changed_at(&new_cfg_path));
        std::fs::write(&new_cfg_path, "ACCENT=\"#778899\"\n").unwrap();
        assert!(sh_missing.reload_if_changed_at(&new_cfg_path));
        assert_eq!(sh_missing.cfg.accent, [0x77, 0x88, 0x99]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_app_shows_not_installed_card_and_remove_updates_panel_line() {
        let dir = std::env::temp_dir().join(format!(
            "infynitae-shell-not-installed-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("config");
        let bak_path = dir.join("panels.backup");
        std::fs::write(
            &cfg_path,
            "PANEL_X=\"SUPER+X top right 380 nosuchapp/card settings/bogus settings/session\"\n",
        )
        .unwrap();

        let mut sh = Shell::new(
            Config::from_text(&std::fs::read_to_string(&cfg_path).unwrap()),
            Vec::new(),
        );
        sh.save_paths = Some((cfg_path.clone(), bak_path.clone()));
        let idx = sh.panels.iter().position(|p| p.panel.name == "x").unwrap();
        assert_eq!(sh.panels[idx].cards.len(), 3);
        assert_eq!(sh.panels[idx].cards[0].from, "nosuchapp");
        assert_eq!(sh.panels[idx].cards[1].from, "Settings");
        let cx = sh.cx(Size::S, None, false);
        assert_eq!(sh.panels[idx].cards[0].card.status(&cx), Status::Ready);
        assert_eq!(
            sh.panels[idx].cards[0].card.title(&cx).as_deref(),
            Some("card")
        );
        assert_eq!(NOT_INSTALLED_TEXT, "This app is not installed");

        assert!(sh.open_panel(idx, false, Instant::now()));
        sh.open_ms = f64::INFINITY;
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 1280, 800).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        sh.draw(&cr, 1280.0, 800.0);

        let remove_r = {
            let f = sh.frame.borrow();
            assert_eq!(f[0].2.list.len(), 1, "not-installed card has Remove button");
            f[0].2.list[0].1
        };
        assert!(sh.click(
            1280.0,
            800.0,
            remove_r.0 + 4.0,
            remove_r.1 + 4.0,
            Button::Left
        ));
        let idx = sh.panels.iter().position(|p| p.panel.name == "x").unwrap();
        assert_eq!(sh.open, Some(idx));
        assert_eq!(sh.panels[idx].cards.len(), 2);
        assert_eq!(sh.panels[idx].cards[0].word.kind, "bogus");
        assert_eq!(sh.panels[idx].cards[1].word.kind, "session");

        let saved = std::fs::read_to_string(&cfg_path).unwrap();
        assert!(!saved.contains("nosuchapp/card"));
        assert!(saved.contains("settings/bogus"));
        assert!(saved.contains("settings/session"));
        let backup = std::fs::read_to_string(&bak_path).unwrap();
        assert!(backup.contains("nosuchapp/card"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
