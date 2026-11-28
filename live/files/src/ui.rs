//! Cairo/Pango renderer and hit-test layout for the portal file picker matching
//! `docs/mockups/file-picker.html`. Every colour and radius comes from `Look`.

use crate::draw::{self, Icon, Text, SIZE_BODY, SIZE_NOTE, SIZE_SMALL, SIZE_TITLE};
use crate::fs_list::{format_modified, format_size, FileEntry, SortColumn, SortOrder};
use crate::look::Look;
use crate::portal::mode::{Focus, OpenMenu, OverwritePrompt, PickerMode, PickerState, PlaceTarget};
use crate::preview::PreviewContent;
use crate::text_input::TextInput;
use cairo::Context;
use std::path::PathBuf;

pub const DEFAULT_W: u32 = 920;
pub const DEFAULT_H: u32 = 580;
pub const MIN_W: u32 = 520;
pub const MIN_H: u32 = 400;

const HEADER_H: f64 = 44.0;
const TOOLBAR_H: f64 = 46.0;
const COL_HEAD_H: f64 = 28.0;
const ROW_H: f64 = 34.0;
const RECENT_ROW_H: f64 = 44.0;
const FOOTER_H: f64 = 50.0;
const SAVE_ROW_H: f64 = 46.0;
const CHOICES_ROW_H: f64 = 42.0;
const INLINE_BAR_H: f64 = 42.0;
const ERROR_BAR_H: f64 = 34.0;
const PREVIEW_MIN_WIN_W: f64 = 680.0;
const PREVIEW_W: f64 = 224.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HitAction {
    WindowDrag,
    CloseBtn,
    BackBtn,
    ForwardBtn,
    UpBtn,
    Breadcrumb(PathBuf),
    PathBarEdit,
    PathInput,
    SearchInput,
    SearchClear,
    TogglePreview,
    ToggleHidden,
    NewFolderBtn,
    NewFolderInput,
    NewFolderCreate,
    NewFolderCancel,
    Place(PlaceTarget),
    SortCol(SortColumn),
    FileRow(usize),
    OverwriteCancel,
    OverwriteReplace,
    OverwriteRename,
    SaveNameInput,
    ChoiceBool(usize),
    ChoiceMenuBtn(usize),
    ChoiceMenuItem(usize, String),
    FilterBtn,
    FilterItem(usize),
    CancelBtn,
    AcceptBtn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorKind {
    Default,
    Pointer,
    Text,
}

#[derive(Clone, Debug)]
pub struct HitTarget {
    pub rect: Rect,
    pub action: HitAction,
    pub cursor: CursorKind,
}

#[derive(Clone, Debug, Default)]
pub struct LayoutHits {
    pub targets: Vec<HitTarget>,
    pub list_rect: Option<Rect>,
    pub places_rect: Option<Rect>,
    pub list_max_scroll: f64,
    pub places_max_scroll: f64,
    pub row_height: f64,
    pub text_boxes: Vec<(Focus, Rect, f64, bool)>,
}

impl LayoutHits {
    fn push(&mut self, rect: Rect, action: HitAction, cursor: CursorKind) {
        self.targets.push(HitTarget {
            rect,
            action,
            cursor,
        });
    }

    /// Topmost hit target under `(px, py)` (later targets like dropdown menus win).
    pub fn hit_test(&self, px: f64, py: f64) -> Option<&HitTarget> {
        self.targets.iter().rev().find(|t| t.rect.contains(px, py))
    }
}

pub fn render(
    cr: &Context,
    look: &Look,
    state: &PickerState,
    w: f64,
    h: f64,
    hover_pos: Option<(f64, f64)>,
) -> LayoutHits {
    let mut hits = LayoutHits::default();
    let is_hovered = |r: Rect| hover_pos.is_some_and(|(x, y)| r.contains(x, y));

    draw::with_font(&look.font, || {
        // Opaque window background.
        draw::set(cr, look.window_bg());
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();

        // 1. Header
        let head_r = Rect::new(0.0, 0.0, w, HEADER_H);
        hits.push(head_r, HitAction::WindowDrag, CursorKind::Default);

        let close_r = Rect::new(w - 14.0 - 28.0, (HEADER_H - 28.0) * 0.5, 28.0, 28.0);
        if is_hovered(close_r) {
            draw::fill_round_rect(
                cr,
                close_r.x,
                close_r.y,
                close_r.w,
                close_r.h,
                look.small_radius,
                look.hover_fill(),
            );
        }
        draw::icon(
            cr,
            Icon::Close,
            close_r.x + 6.0,
            close_r.y + 6.0,
            if is_hovered(close_r) {
                look.text
            } else {
                look.muted
            },
        );
        hits.push(close_r, HitAction::CloseBtn, CursorKind::Pointer);

        let mut title_right = close_r.x - 10.0;
        if !state.app_id.is_empty() {
            let app_t = Text {
                size: SIZE_SMALL,
                bold: false,
                color: look.muted,
                text: &state.app_id,
            };
            let aw = draw::text_right(cr, title_right, (HEADER_H - 16.0) * 0.5, &app_t);
            title_right -= aw + 12.0;
        }
        draw::text_max(
            cr,
            16.0,
            (HEADER_H - 20.0) * 0.5,
            (title_right - 16.0).max(40.0),
            &Text {
                size: SIZE_TITLE,
                bold: true,
                color: look.text,
                text: &state.title,
            },
        );
        draw::hline(cr, 0.0, HEADER_H - 1.0, w, look.edge);

        // 2. Toolbar
        let tb_y = HEADER_H;
        draw_toolbar(cr, look, state, w, tb_y, hover_pos, &mut hits);
        draw::hline(cr, 0.0, tb_y + TOOLBAR_H - 1.0, w, look.edge);

        // 3. Reserve bottom sections (footer, optional choices, optional save row, optional overwrite bar)
        let mut bottom_y = h - FOOTER_H;
        let footer_r = Rect::new(0.0, bottom_y, w, FOOTER_H);

        let choices_r = if !state.choices.is_empty() {
            bottom_y -= CHOICES_ROW_H;
            Some(Rect::new(0.0, bottom_y, w, CHOICES_ROW_H))
        } else {
            None
        };

        let save_r = match &state.mode {
            PickerMode::SaveFile | PickerMode::SaveFiles { .. } => {
                bottom_y -= SAVE_ROW_H;
                Some(Rect::new(0.0, bottom_y, w, SAVE_ROW_H))
            }
            PickerMode::OpenFile { .. } => None,
        };

        let overwrite_r = if state.overwrite.is_some() {
            bottom_y -= INLINE_BAR_H;
            Some(Rect::new(0.0, bottom_y, w, INLINE_BAR_H))
        } else {
            None
        };

        // 4. Body: Places | Main List | Optional Preview
        let body_y = tb_y + TOOLBAR_H;
        let body_h = (bottom_y - body_y).max(60.0);
        let places_w = if w < 700.0 { 148.0 } else { 184.0 };
        let show_preview = state.preview_open && w >= PREVIEW_MIN_WIN_W;
        let preview_w = if show_preview { PREVIEW_W } else { 0.0 };
        let main_w = (w - places_w - preview_w).max(180.0);

        let places_r = Rect::new(0.0, body_y, places_w, body_h);
        draw_places(cr, look, state, places_r, hover_pos, &mut hits);
        draw::vline(cr, places_w - 1.0, body_y, body_h, look.edge);

        let main_r = Rect::new(places_w, body_y, main_w, body_h);
        draw_main_area(cr, look, state, main_r, hover_pos, &mut hits);

        if show_preview {
            let prev_r = Rect::new(places_w + main_w, body_y, preview_w, body_h);
            draw::vline(cr, prev_r.x, body_y, body_h, look.edge);
            draw_preview_pane(cr, look, state, prev_r);
        }

        // 5. Bottom sections
        if let Some(r) = overwrite_r {
            draw::hline(cr, 0.0, r.y, w, look.alert);
            draw_overwrite_bar(cr, look, state, r, hover_pos, &mut hits);
        }
        if let Some(r) = save_r {
            draw::hline(cr, 0.0, r.y, w, look.edge);
            draw_save_row(cr, look, state, r, &mut hits);
        }
        let mut choice_btn_rects: Vec<(usize, Rect)> = Vec::new();
        if let Some(r) = choices_r {
            draw::hline(cr, 0.0, r.y, w, look.edge);
            choice_btn_rects = draw_choices_row(cr, look, state, r, hover_pos, &mut hits);
        }
        draw::hline(cr, 0.0, footer_r.y, w, look.edge);
        let filter_btn_rect = draw_footer(cr, look, state, footer_r, hover_pos, &mut hits);

        // 6. Overlay dropdown menus on top
        match state.open_menu {
            Some(OpenMenu::Filter) => {
                if let Some(anchor) = filter_btn_rect {
                    draw_filter_menu(cr, look, state, anchor, hover_pos, &mut hits);
                }
            }
            Some(OpenMenu::Choice(ci)) => {
                if let Some(&(_, anchor)) = choice_btn_rects.iter().find(|(idx, _)| *idx == ci) {
                    draw_choice_menu(cr, look, state, ci, anchor, hover_pos, &mut hits);
                }
            }
            None => {}
        }
    });

    hits
}

fn draw_toolbar(
    cr: &Context,
    look: &Look,
    state: &PickerState,
    w: f64,
    y: f64,
    hover_pos: Option<(f64, f64)>,
    hits: &mut LayoutHits,
) {
    let btn_y = y + (TOOLBAR_H - 28.0) * 0.5;
    let field_y = y + (TOOLBAR_H - 30.0) * 0.5;
    let mut x = 12.0;

    // Back / Forward / Up
    let nav_items = [
        (Icon::Back, state.can_go_back(), HitAction::BackBtn),
        (Icon::Forward, state.can_go_forward(), HitAction::ForwardBtn),
        (Icon::Up, state.can_go_up(), HitAction::UpBtn),
    ];
    for (ic, enabled, action) in nav_items {
        let r = Rect::new(x, btn_y, 28.0, 28.0);
        draw_icon_button(cr, look, r, ic, enabled, false, hover_pos);
        if enabled {
            hits.push(r, action, CursorKind::Pointer);
        }
        x += 30.0;
    }
    x += 6.0;

    // Right-side tool buttons: Preview, Hidden, New folder
    let tools_w = 3.0 * 30.0;
    let tools_x = w - 12.0 - tools_w + 2.0;
    let tool_items = [
        (
            Icon::Preview,
            true,
            state.preview_open,
            HitAction::TogglePreview,
        ),
        (
            Icon::Hidden,
            true,
            state.show_hidden,
            HitAction::ToggleHidden,
        ),
        (
            Icon::NewFolder,
            !state.in_recent_view,
            state.new_folder.is_some(),
            HitAction::NewFolderBtn,
        ),
    ];
    for (i, (ic, enabled, pressed, action)) in tool_items.into_iter().enumerate() {
        let r = Rect::new(tools_x + i as f64 * 30.0, btn_y, 28.0, 28.0);
        draw_icon_button(cr, look, r, ic, enabled, pressed, hover_pos);
        if enabled {
            hits.push(r, action, CursorKind::Pointer);
        }
    }

    // Search field
    let search_w = if w < 700.0 { 140.0 } else { 200.0 };
    let search_x = tools_x - 8.0 - search_w;
    let search_r = Rect::new(search_x, field_y, search_w, 30.0);
    let search_focused = state.focus == Focus::Search;
    draw::fill_round_rect(
        cr,
        search_r.x,
        search_r.y,
        search_r.w,
        search_r.h,
        look.small_radius,
        look.box_fill(),
    );
    draw::stroke_round_rect(
        cr,
        search_r.x,
        search_r.y,
        search_r.w,
        search_r.h,
        look.small_radius,
        if search_focused {
            look.accent
        } else {
            look.edge
        },
    );
    draw::icon(
        cr,
        Icon::Search,
        search_r.x + 7.0,
        search_r.y + 7.0,
        look.muted,
    );
    let clear_space = if state.search_input.is_empty() {
        6.0
    } else {
        26.0
    };
    let search_text_r = Rect::new(
        search_r.x + 26.0,
        search_r.y,
        (search_r.w - 26.0 - clear_space).max(20.0),
        search_r.h,
    );
    draw_text_input_inner(
        cr,
        look,
        &state.search_input,
        search_text_r,
        SIZE_SMALL,
        search_focused,
        "Search folder",
        false,
    );
    hits.push(search_r, HitAction::SearchInput, CursorKind::Text);
    hits.text_boxes
        .push((Focus::Search, search_text_r, SIZE_SMALL, false));

    if !state.search_input.is_empty() {
        let cr_btn = Rect::new(search_r.x + search_r.w - 24.0, search_r.y + 3.0, 22.0, 24.0);
        if hover_pos.is_some_and(|(px, py)| cr_btn.contains(px, py)) {
            draw::fill_round_rect(
                cr,
                cr_btn.x,
                cr_btn.y,
                cr_btn.w,
                cr_btn.h,
                look.small_radius,
                look.hover_fill(),
            );
        }
        draw::icon(cr, Icon::Close, cr_btn.x + 3.0, cr_btn.y + 4.0, look.muted);
        hits.push(cr_btn, HitAction::SearchClear, CursorKind::Pointer);
    }

    // Path bar (breadcrumbs or Ctrl+L edit field)
    let path_w = (search_x - 8.0 - x).max(120.0);
    let path_r = Rect::new(x, field_y, path_w, 30.0);
    draw::fill_round_rect(
        cr,
        path_r.x,
        path_r.y,
        path_r.w,
        path_r.h,
        look.small_radius,
        look.box_fill(),
    );
    draw::stroke_round_rect(
        cr,
        path_r.x,
        path_r.y,
        path_r.w,
        path_r.h,
        look.small_radius,
        if state.path_editing {
            look.accent
        } else {
            look.edge
        },
    );

    if state.path_editing {
        let inner = Rect::new(path_r.x + 8.0, path_r.y, path_r.w - 16.0, path_r.h);
        draw_text_input_inner(
            cr,
            look,
            &state.path_input,
            inner,
            SIZE_SMALL,
            state.focus == Focus::PathEdit,
            "/path/to/folder",
            true,
        );
        hits.push(path_r, HitAction::PathInput, CursorKind::Text);
        hits.text_boxes
            .push((Focus::PathEdit, inner, SIZE_SMALL, true));
    } else {
        // Clicking empty space in the path bar enters edit mode.
        hits.push(path_r, HitAction::PathBarEdit, CursorKind::Text);
        let crumbs = state.breadcrumbs();
        let _ = cr.save();
        cr.rectangle(
            path_r.x + 4.0,
            path_r.y + 1.0,
            path_r.w - 8.0,
            path_r.h - 2.0,
        );
        cr.clip();

        // Measure total width and drop leading crumbs if the path is deeper than the bar.
        let mut widths: Vec<f64> = Vec::with_capacity(crumbs.len());
        for (i, (label, _)) in crumbs.iter().enumerate() {
            let t = Text {
                size: SIZE_SMALL,
                bold: i + 1 == crumbs.len(),
                color: look.text,
                text: label,
            };
            widths.push(draw::measure(cr, &t).0 + 12.0);
        }
        let sep_w = 12.0;
        let max_inner = path_r.w - 16.0;
        let mut start = 0usize;
        let total_w = |s: usize| -> f64 {
            let count = crumbs.len() - s;
            widths[s..].iter().sum::<f64>() + count.saturating_sub(1) as f64 * sep_w
        };
        while start + 1 < crumbs.len() && total_w(start) > max_inner {
            start += 1;
        }

        let mut cx = path_r.x + 6.0;
        for i in start..crumbs.len() {
            let (ref label, ref path) = crumbs[i];
            let is_last = i + 1 == crumbs.len();
            if i > start {
                draw::text(
                    cr,
                    cx + 3.0,
                    path_r.y + 7.0,
                    &Text {
                        size: SIZE_NOTE,
                        bold: false,
                        color: look.muted,
                        text: "/",
                    },
                );
                cx += sep_w;
            }
            let bw = widths[i].min((path_r.x + path_r.w - 6.0 - cx).max(24.0));
            let br = Rect::new(cx, path_r.y + 3.0, bw, 24.0);
            let hov = hover_pos.is_some_and(|(px, py)| br.contains(px, py));
            if hov && !path.as_os_str().is_empty() {
                draw::fill_round_rect(
                    cr,
                    br.x,
                    br.y,
                    br.w,
                    br.h,
                    look.small_radius,
                    look.hover_fill(),
                );
            }
            draw::text_max(
                cr,
                br.x + 6.0,
                br.y + 4.0,
                (br.w - 12.0).max(8.0),
                &Text {
                    size: SIZE_SMALL,
                    bold: is_last,
                    color: if is_last || hov {
                        look.text
                    } else {
                        look.muted
                    },
                    text: label,
                },
            );
            if !path.as_os_str().is_empty() {
                hits.push(br, HitAction::Breadcrumb(path.clone()), CursorKind::Pointer);
            }
            cx += bw;
        }
        let _ = cr.restore();
    }
}

fn draw_icon_button(
    cr: &Context,
    look: &Look,
    r: Rect,
    ic: Icon,
    enabled: bool,
    pressed: bool,
    hover_pos: Option<(f64, f64)>,
) {
    let hov = enabled && hover_pos.is_some_and(|(px, py)| r.contains(px, py));
    if pressed {
        draw::fill_round_rect(
            cr,
            r.x,
            r.y,
            r.w,
            r.h,
            look.small_radius,
            look.chosen_fill(),
        );
    } else if hov {
        draw::fill_round_rect(cr, r.x, r.y, r.w, r.h, look.small_radius, look.hover_fill());
    }
    let col = if !enabled {
        look.muted.alpha(look.muted.3 * 0.42)
    } else if pressed {
        look.accent_text
    } else if hov {
        look.text
    } else {
        look.muted
    };
    draw::icon(cr, ic, r.x + 6.0, r.y + 6.0, col);
}

fn draw_places(
    cr: &Context,
    look: &Look,
    state: &PickerState,
    r: Rect,
    hover_pos: Option<(f64, f64)>,
    hits: &mut LayoutHits,
) {
    hits.places_rect = Some(r);
    let _ = cr.save();
    cr.rectangle(r.x, r.y, r.w - 1.0, r.h);
    cr.clip();

    let pad = 8.0;
    let item_w = r.w - pad * 2.0 - 1.0;
    let mut y = r.y + 8.0 - state.places_scroll;
    let start_y = y;
    let mut item_idx = 0usize;

    // 1. Recent
    draw_place_heading(cr, look, r.x + pad + 8.0, y, "Recent");
    y += 22.0;
    {
        let row_r = Rect::new(r.x + pad, y, item_w, 30.0);
        let active = state.in_recent_view;
        let kbd = state.focus == Focus::Places && state.place_cursor == item_idx;
        let hov = hover_pos.is_some_and(|(px, py)| row_r.contains(px, py) && r.contains(px, py));
        draw_place_row_bg(cr, look, row_r, active, hov, kbd);
        draw::icon(
            cr,
            Icon::Clock,
            row_r.x + 8.0,
            row_r.y + 7.0,
            if active { look.accent_text } else { look.muted },
        );
        draw::text_max(
            cr,
            row_r.x + 30.0,
            row_r.y + 6.0,
            item_w - 36.0,
            &Text {
                size: SIZE_BODY,
                bold: active,
                color: if active { look.accent_text } else { look.text },
                text: "Recent",
            },
        );
        if row_r.y + row_r.h > r.y && row_r.y < r.y + r.h {
            hits.push(
                row_r,
                HitAction::Place(PlaceTarget::Recent),
                CursorKind::Pointer,
            );
        }
        y += 32.0;
        item_idx += 1;
    }

    // 2. Home folders
    if !state.places.folders.is_empty() {
        y += 6.0;
        draw_place_heading(cr, look, r.x + pad + 8.0, y, "Folders");
        y += 22.0;
        for (fi, folder) in state.places.folders.iter().enumerate() {
            let row_r = Rect::new(r.x + pad, y, item_w, 30.0);
            let active = !state.in_recent_view && state.current_dir == folder.path;
            let kbd = state.focus == Focus::Places && state.place_cursor == item_idx;
            let hov =
                hover_pos.is_some_and(|(px, py)| row_r.contains(px, py) && r.contains(px, py));
            draw_place_row_bg(cr, look, row_r, active, hov, kbd);
            let ic = if folder.is_home {
                Icon::Home
            } else {
                Icon::Folder
            };
            draw::icon(
                cr,
                ic,
                row_r.x + 8.0,
                row_r.y + 7.0,
                if active { look.accent_text } else { look.muted },
            );
            draw::text_max(
                cr,
                row_r.x + 30.0,
                row_r.y + 6.0,
                item_w - 36.0,
                &Text {
                    size: SIZE_BODY,
                    bold: active,
                    color: if active { look.accent_text } else { look.text },
                    text: &folder.name,
                },
            );
            if row_r.y + row_r.h > r.y && row_r.y < r.y + r.h {
                hits.push(
                    row_r,
                    HitAction::Place(PlaceTarget::Folder(fi)),
                    CursorKind::Pointer,
                );
            }
            y += 32.0;
            item_idx += 1;
        }
    }

    // 3. Drives (I-VI and USB)
    if !state.places.drives.is_empty() {
        y += 6.0;
        draw_place_heading(cr, look, r.x + pad + 8.0, y, "Drives");
        y += 22.0;
        for (di, drive) in state.places.drives.iter().enumerate() {
            let row_h = if drive.total_bytes > 0 { 40.0 } else { 30.0 };
            let row_r = Rect::new(r.x + pad, y, item_w, row_h);
            let active = !state.in_recent_view && state.current_dir == drive.mount_point;
            let kbd = state.focus == Focus::Places && state.place_cursor == item_idx;
            let hov =
                hover_pos.is_some_and(|(px, py)| row_r.contains(px, py) && r.contains(px, py));
            draw_place_row_bg(cr, look, row_r, active, hov, kbd);

            let ic = if drive.removable {
                Icon::Usb
            } else {
                Icon::Drive
            };
            draw::icon(
                cr,
                ic,
                row_r.x + 8.0,
                row_r.y + 7.0,
                if active { look.accent_text } else { look.muted },
            );

            let free = drive.free_short();
            let free_w = if !free.is_empty() {
                draw::text_right(
                    cr,
                    row_r.x + row_r.w - 8.0,
                    row_r.y + 8.0,
                    &Text {
                        size: SIZE_NOTE,
                        bold: false,
                        color: look.muted,
                        text: &free,
                    },
                ) + 6.0
            } else {
                0.0
            };

            draw::text_max(
                cr,
                row_r.x + 30.0,
                row_r.y + 6.0,
                (item_w - 38.0 - free_w).max(20.0),
                &Text {
                    size: SIZE_BODY,
                    bold: active,
                    color: if active { look.accent_text } else { look.text },
                    text: &drive.name,
                },
            );

            if drive.total_bytes > 0 {
                draw::bar(
                    cr,
                    look,
                    row_r.x + 30.0,
                    row_r.y + 29.0,
                    (item_w - 38.0).max(20.0),
                    drive.used_fraction(),
                    look.accent,
                );
            }

            if row_r.y + row_r.h > r.y && row_r.y < r.y + r.h {
                hits.push(
                    row_r,
                    HitAction::Place(PlaceTarget::Drive(di)),
                    CursorKind::Pointer,
                );
            }
            y += row_h + 2.0;
            item_idx += 1;
        }
    }

    let content_h = (y - start_y) + 16.0;
    hits.places_max_scroll = (content_h - r.h).max(0.0);
    let _ = cr.restore();
}

fn draw_place_heading(cr: &Context, look: &Look, x: f64, y: f64, label: &str) {
    draw::text(
        cr,
        x,
        y + 3.0,
        &Text {
            size: SIZE_NOTE,
            bold: false,
            color: look.muted,
            text: label,
        },
    );
}

fn draw_place_row_bg(
    cr: &Context,
    look: &Look,
    r: Rect,
    active: bool,
    hovered: bool,
    kbd_focused: bool,
) {
    if active {
        draw::fill_round_rect(
            cr,
            r.x,
            r.y,
            r.w,
            r.h,
            look.small_radius,
            look.chosen_fill(),
        );
    } else if hovered {
        draw::fill_round_rect(cr, r.x, r.y, r.w, r.h, look.small_radius, look.hover_fill());
    }
    if kbd_focused {
        draw::stroke_round_rect(cr, r.x, r.y, r.w, r.h, look.small_radius, look.accent);
    }
}

fn draw_main_area(
    cr: &Context,
    look: &Look,
    state: &PickerState,
    r: Rect,
    hover_pos: Option<(f64, f64)>,
    hits: &mut LayoutHits,
) {
    let mut y = r.y;

    // Inline New folder bar (Ctrl+Shift+N)
    if let Some(ref nf) = state.new_folder {
        let bar_r = Rect::new(r.x, y, r.w, INLINE_BAR_H);
        draw::set(cr, look.box_fill());
        cr.rectangle(bar_r.x, bar_r.y, bar_r.w, bar_r.h);
        let _ = cr.fill();

        let lbl_w = draw::text(
            cr,
            bar_r.x + 12.0,
            bar_r.y + 13.0,
            &Text {
                size: SIZE_SMALL,
                bold: false,
                color: look.muted,
                text: "New folder",
            },
        );
        let cancel_r = Rect::new(bar_r.x + bar_r.w - 12.0 - 64.0, bar_r.y + 7.0, 64.0, 28.0);
        let create_r = Rect::new(cancel_r.x - 8.0 - 68.0, bar_r.y + 7.0, 68.0, 28.0);
        let inp_x = bar_r.x + 12.0 + lbl_w + 10.0;
        let inp_r = Rect::new(
            inp_x,
            bar_r.y + 7.0,
            (create_r.x - 10.0 - inp_x).max(60.0),
            28.0,
        );
        draw::fill_round_rect(
            cr,
            inp_r.x,
            inp_r.y,
            inp_r.w,
            inp_r.h,
            look.small_radius,
            look.window_bg(),
        );
        draw::stroke_round_rect(
            cr,
            inp_r.x,
            inp_r.y,
            inp_r.w,
            inp_r.h,
            look.small_radius,
            look.accent,
        );
        let inner = Rect::new(inp_r.x + 8.0, inp_r.y, inp_r.w - 16.0, inp_r.h);
        draw_text_input_inner(
            cr,
            look,
            nf,
            inner,
            SIZE_SMALL,
            state.focus == Focus::NewFolder,
            "Folder name",
            false,
        );
        hits.push(inp_r, HitAction::NewFolderInput, CursorKind::Text);
        hits.text_boxes
            .push((Focus::NewFolder, inner, SIZE_SMALL, false));

        draw_button(
            cr,
            look,
            create_r,
            "Create",
            ButtonStyle::Primary,
            !nf.as_str().trim().is_empty(),
            false,
            hover_pos,
        );
        hits.push(create_r, HitAction::NewFolderCreate, CursorKind::Pointer);
        draw_button(
            cr,
            look,
            cancel_r,
            "Cancel",
            ButtonStyle::Secondary,
            true,
            false,
            hover_pos,
        );
        hits.push(cancel_r, HitAction::NewFolderCancel, CursorKind::Pointer);

        y += INLINE_BAR_H;
        draw::hline(cr, r.x, y - 1.0, r.w, look.edge);
    }

    // Inline error alert bar
    if let Some(ref err) = state.error_msg {
        let err_r = Rect::new(r.x, y, r.w, ERROR_BAR_H);
        draw::set(cr, look.alert_fill());
        cr.rectangle(err_r.x, err_r.y, err_r.w, err_r.h);
        let _ = cr.fill();
        draw::text_max(
            cr,
            err_r.x + 12.0,
            err_r.y + 9.0,
            (err_r.w - 24.0).max(20.0),
            &Text {
                size: SIZE_SMALL,
                bold: false,
                color: look.alert,
                text: err,
            },
        );
        y += ERROR_BAR_H;
        draw::hline(cr, r.x, y - 1.0, r.w, look.alert);
    }

    // Column headers
    let show_mod = r.w >= 340.0;
    let size_w = 76.0;
    let mod_w = if show_mod { 116.0 } else { 0.0 };
    let head_r = Rect::new(r.x, y, r.w, COL_HEAD_H);
    let name_w = (r.w - 28.0 - size_w - mod_w - if show_mod { 16.0 } else { 8.0 }).max(80.0);
    let col_name_x = r.x + 14.0;
    let col_mod_x = col_name_x + name_w + 8.0;
    let col_size_right = r.x + r.w - 14.0;

    let sort_arrow = |col: SortColumn| -> &'static str {
        if state.sort_col != col {
            ""
        } else if state.sort_order == SortOrder::Asc {
            " ↑"
        } else {
            " ↓"
        }
    };

    let name_hdr = format!("Name{}", sort_arrow(SortColumn::Name));
    let name_hdr_r = Rect::new(col_name_x, head_r.y, name_w, COL_HEAD_H);
    draw::text(
        cr,
        col_name_x,
        head_r.y + 7.0,
        &Text {
            size: SIZE_NOTE,
            bold: false,
            color: if state.sort_col == SortColumn::Name {
                look.accent_text
            } else {
                look.muted
            },
            text: &name_hdr,
        },
    );
    hits.push(
        name_hdr_r,
        HitAction::SortCol(SortColumn::Name),
        CursorKind::Pointer,
    );

    if show_mod {
        let mod_hdr = format!("Modified{}", sort_arrow(SortColumn::Modified));
        let mod_hdr_r = Rect::new(col_mod_x, head_r.y, mod_w, COL_HEAD_H);
        draw::text(
            cr,
            col_mod_x,
            head_r.y + 7.0,
            &Text {
                size: SIZE_NOTE,
                bold: false,
                color: if state.sort_col == SortColumn::Modified {
                    look.accent_text
                } else {
                    look.muted
                },
                text: &mod_hdr,
            },
        );
        hits.push(
            mod_hdr_r,
            HitAction::SortCol(SortColumn::Modified),
            CursorKind::Pointer,
        );
    }

    let size_hdr = format!("Size{}", sort_arrow(SortColumn::Size));
    let size_hdr_r = Rect::new(col_size_right - size_w, head_r.y, size_w, COL_HEAD_H);
    draw::text_right(
        cr,
        col_size_right,
        head_r.y + 7.0,
        &Text {
            size: SIZE_NOTE,
            bold: false,
            color: if state.sort_col == SortColumn::Size {
                look.accent_text
            } else {
                look.muted
            },
            text: &size_hdr,
        },
    );
    hits.push(
        size_hdr_r,
        HitAction::SortCol(SortColumn::Size),
        CursorKind::Pointer,
    );

    y += COL_HEAD_H;
    draw::hline(cr, r.x, y - 1.0, r.w, look.edge);

    // Scrollable file list
    let list_r = Rect::new(r.x, y, r.w, (r.y + r.h - y).max(20.0));
    hits.list_rect = Some(list_r);
    let row_h = if state.in_recent_view {
        RECENT_ROW_H
    } else {
        ROW_H
    };
    hits.row_height = row_h;

    let _ = cr.save();
    cr.rectangle(list_r.x, list_r.y, list_r.w, list_r.h);
    cr.clip();

    if state.loading {
        draw::text_center(
            cr,
            list_r.x + list_r.w * 0.5,
            list_r.y + 36.0,
            &Text {
                size: SIZE_SMALL,
                bold: false,
                color: look.muted,
                text: "Loading",
            },
        );
    } else if state.visible.is_empty() {
        let msg = if state.in_recent_view {
            "No recent files"
        } else if !state.search_input.is_empty() || !state.filters.is_empty() {
            if state.raw_entries.is_empty() {
                "Empty folder"
            } else {
                "No files match this filter"
            }
        } else {
            "Empty folder"
        };
        draw::text_center(
            cr,
            list_r.x + list_r.w * 0.5,
            list_r.y + 36.0,
            &Text {
                size: SIZE_SMALL,
                bold: false,
                color: look.muted,
                text: msg,
            },
        );
    } else {
        let pad_top = 4.0;
        let total_h = pad_top * 2.0 + state.visible.len() as f64 * row_h;
        hits.list_max_scroll = (total_h - list_r.h).max(0.0);

        let first_idx = ((state.list_scroll - pad_top).max(0.0) / row_h).floor() as usize;
        let max_vis = (list_r.h / row_h).ceil() as usize + 2;
        let end_idx = (first_idx + max_vis).min(state.visible.len());

        for vi in first_idx..end_idx {
            let entry = &state.raw_entries[state.visible[vi]];
            let ry = list_r.y + pad_top + vi as f64 * row_h - state.list_scroll;
            let row_r = Rect::new(list_r.x + 6.0, ry, list_r.w - 12.0, row_h);
            let is_sel = state.selected.contains(&vi);
            let is_cur = state.cursor == Some(vi) && state.focus == Focus::FileList;
            let hov =
                hover_pos.is_some_and(|(px, py)| row_r.contains(px, py) && list_r.contains(px, py));

            if is_sel {
                draw::fill_round_rect(
                    cr,
                    row_r.x,
                    row_r.y,
                    row_r.w,
                    row_r.h,
                    look.small_radius,
                    look.chosen_fill(),
                );
            } else if hov {
                draw::fill_round_rect(
                    cr,
                    row_r.x,
                    row_r.y,
                    row_r.w,
                    row_r.h,
                    look.small_radius,
                    look.hover_fill(),
                );
            }
            if is_cur {
                draw::stroke_round_rect(
                    cr,
                    row_r.x,
                    row_r.y,
                    row_r.w,
                    row_r.h,
                    look.small_radius,
                    look.accent,
                );
            }

            let ic = entry_icon(entry, &state.mime_db);
            let ic_col = if is_sel { look.accent_text } else { look.muted };
            draw::icon(cr, ic, col_name_x, row_r.y + (row_h - 16.0) * 0.5, ic_col);

            let text_x = col_name_x + 25.0;
            let text_w = (name_w - 25.0).max(30.0);
            let name_col = if is_sel { look.accent_text } else { look.text };
            if let Some(ref sub) = entry.subtitle {
                draw::text_max(
                    cr,
                    text_x,
                    row_r.y + 5.0,
                    text_w,
                    &Text {
                        size: SIZE_BODY,
                        bold: is_sel,
                        color: name_col,
                        text: &entry.display_name,
                    },
                );
                draw::text_max(
                    cr,
                    text_x,
                    row_r.y + 23.0,
                    text_w,
                    &Text {
                        size: SIZE_NOTE,
                        bold: false,
                        color: look.muted,
                        text: sub,
                    },
                );
            } else {
                draw::text_max(
                    cr,
                    text_x,
                    row_r.y + (row_h - 17.0) * 0.5,
                    text_w,
                    &Text {
                        size: SIZE_BODY,
                        bold: is_sel,
                        color: name_col,
                        text: &entry.display_name,
                    },
                );
            }

            if show_mod {
                let m = format_modified(entry.modified_secs);
                draw::text_max(
                    cr,
                    col_mod_x,
                    row_r.y + (row_h - 15.0) * 0.5,
                    mod_w,
                    &Text {
                        size: SIZE_SMALL,
                        bold: false,
                        color: look.muted,
                        text: &m,
                    },
                );
            }

            if !entry.is_dir {
                let sz = format_size(entry.size);
                draw::text_right(
                    cr,
                    col_size_right,
                    row_r.y + (row_h - 15.0) * 0.5,
                    &Text {
                        size: SIZE_SMALL,
                        bold: false,
                        color: look.muted,
                        text: &sz,
                    },
                );
            }

            hits.push(row_r, HitAction::FileRow(vi), CursorKind::Pointer);
        }
    }

    let _ = cr.restore();
}

fn entry_icon(entry: &FileEntry, mime_db: &crate::mime::MimeDb) -> Icon {
    if entry.is_dir {
        Icon::Folder
    } else if mime_db.is_previewable_image(&entry.mime) {
        Icon::Image
    } else if mime_db.is_previewable_text(&entry.mime) {
        Icon::TextFile
    } else {
        Icon::File
    }
}

fn draw_preview_pane(cr: &Context, look: &Look, state: &PickerState, r: Rect) {
    let _ = cr.save();
    cr.rectangle(r.x + 1.0, r.y, r.w - 1.0, r.h);
    cr.clip();

    let pad = 12.0;
    let x = r.x + pad;
    let cw = (r.w - pad * 2.0).max(40.0);
    let mut y = r.y + pad;

    match &state.preview {
        PreviewContent::Empty => {
            draw::text_center(
                cr,
                r.x + r.w * 0.5,
                r.y + 42.0,
                &Text {
                    size: SIZE_SMALL,
                    bold: false,
                    color: look.muted,
                    text: "Select a file to preview",
                },
            );
        }
        PreviewContent::Loading { name } => {
            draw_preview_box_frame(cr, look, x, y, cw, 132.0);
            draw::text_center(
                cr,
                x + cw * 0.5,
                y + 58.0,
                &Text {
                    size: SIZE_SMALL,
                    bold: false,
                    color: look.muted,
                    text: "Loading",
                },
            );
            y += 142.0;
            draw::text_max(
                cr,
                x,
                y,
                cw,
                &Text {
                    size: SIZE_BODY,
                    bold: true,
                    color: look.text,
                    text: name,
                },
            );
        }
        PreviewContent::Image {
            name,
            image,
            type_label,
            size,
            modified_secs,
        } => {
            let box_h = 132.0;
            draw_preview_box_frame(cr, look, x, y, cw, box_h);
            if image.thumb_w > 0 && image.thumb_h > 0 {
                let stride = (image.thumb_w as i32) * 4;
                let data = image.bgra.clone();
                if let Ok(surf) = cairo::ImageSurface::create_for_data(
                    data,
                    cairo::Format::ARgb32,
                    image.thumb_w as i32,
                    image.thumb_h as i32,
                    stride,
                ) {
                    let inner_w = cw - 4.0;
                    let inner_h = box_h - 4.0;
                    let scale = (inner_w / image.thumb_w as f64)
                        .min(inner_h / image.thumb_h as f64)
                        .min(1.0);
                    let dw = image.thumb_w as f64 * scale;
                    let dh = image.thumb_h as f64 * scale;
                    let ix = x + (cw - dw) * 0.5;
                    let iy = y + (box_h - dh) * 0.5;
                    let _ = cr.save();
                    draw::round_rect(
                        cr,
                        x + 1.0,
                        y + 1.0,
                        cw - 2.0,
                        box_h - 2.0,
                        (look.small_radius - 1.0).max(0.0),
                    );
                    cr.clip();
                    cr.translate(ix, iy);
                    cr.scale(scale, scale);
                    let _ = cr.set_source_surface(&surf, 0.0, 0.0);
                    let _ = cr.paint();
                    let _ = cr.restore();
                }
            }
            y += box_h + 10.0;
            y += draw_preview_title(cr, look, x, y, cw, name);
            let dims = format!("{} × {}", image.orig_w, image.orig_h);
            let sz = format_size(*size);
            let m = format_modified(*modified_secs);
            draw_preview_kv(
                cr,
                look,
                x,
                y,
                cw,
                &[
                    ("Dimensions", dims.as_str()),
                    ("Type", type_label.as_str()),
                    ("Size", sz.as_str()),
                    ("Modified", m.as_str()),
                ],
            );
        }
        PreviewContent::Text {
            name,
            snippet,
            type_label,
            size,
            modified_secs,
        } => {
            let box_h = 152.0;
            draw_preview_box_frame(cr, look, x, y, cw, box_h);
            draw::with_font(&look.mono_font, || {
                draw::text_wrapped_clipped(
                    cr,
                    (x + 8.0, y + 8.0, cw - 16.0, box_h - 16.0),
                    SIZE_NOTE,
                    look.text,
                    snippet,
                );
            });
            y += box_h + 10.0;
            y += draw_preview_title(cr, look, x, y, cw, name);
            let sz = format_size(*size);
            let m = format_modified(*modified_secs);
            draw_preview_kv(
                cr,
                look,
                x,
                y,
                cw,
                &[
                    ("Type", type_label.as_str()),
                    ("Size", sz.as_str()),
                    ("Modified", m.as_str()),
                ],
            );
        }
        PreviewContent::Folder {
            name,
            item_count,
            capped,
            modified_secs,
        } => {
            let box_h = 96.0;
            draw_preview_box_frame(cr, look, x, y, cw, box_h);
            draw::icon(
                cr,
                Icon::Folder,
                x + cw * 0.5 - 8.0,
                y + box_h * 0.5 - 8.0,
                look.muted,
            );
            y += box_h + 10.0;
            y += draw_preview_title(cr, look, x, y, cw, name);
            let items_str = if *capped {
                "1,000+ items".to_string()
            } else if *item_count == 0 {
                "Empty folder".to_string()
            } else if *item_count == 1 {
                "1 item".to_string()
            } else {
                format!("{item_count} items")
            };
            let m = format_modified(*modified_secs);
            draw_preview_kv(
                cr,
                look,
                x,
                y,
                cw,
                &[
                    ("Type", "Folder"),
                    ("Contents", items_str.as_str()),
                    ("Modified", m.as_str()),
                ],
            );
        }
        PreviewContent::File {
            name,
            type_label,
            size,
            modified_secs,
        } => {
            let box_h = 96.0;
            draw_preview_box_frame(cr, look, x, y, cw, box_h);
            draw::icon(
                cr,
                Icon::File,
                x + cw * 0.5 - 8.0,
                y + box_h * 0.5 - 8.0,
                look.muted,
            );
            y += box_h + 10.0;
            y += draw_preview_title(cr, look, x, y, cw, name);
            let sz = format_size(*size);
            let m = format_modified(*modified_secs);
            draw_preview_kv(
                cr,
                look,
                x,
                y,
                cw,
                &[
                    ("Type", type_label.as_str()),
                    ("Size", sz.as_str()),
                    ("Modified", m.as_str()),
                ],
            );
        }
        PreviewContent::Multiple { count, total_size } => {
            let title = format!("{count} items selected");
            y += draw_preview_title(cr, look, x, y, cw, &title);
            let sz = format_size(*total_size);
            draw_preview_kv(cr, look, x, y, cw, &[("Total size", sz.as_str())]);
        }
    }

    let _ = cr.restore();
}

fn draw_preview_box_frame(cr: &Context, look: &Look, x: f64, y: f64, w: f64, h: f64) {
    draw::fill_round_rect(cr, x, y, w, h, look.small_radius, look.box_fill());
    draw::stroke_round_rect(cr, x, y, w, h, look.small_radius, look.edge);
}

fn draw_preview_title(cr: &Context, look: &Look, x: f64, y: f64, w: f64, name: &str) -> f64 {
    draw::text_max(
        cr,
        x,
        y,
        w,
        &Text {
            size: SIZE_BODY,
            bold: true,
            color: look.text,
            text: name,
        },
    );
    24.0
}

fn draw_preview_kv(cr: &Context, look: &Look, x: f64, mut y: f64, w: f64, rows: &[(&str, &str)]) {
    for &(k, v) in rows {
        if v.is_empty() {
            continue;
        }
        draw::text(
            cr,
            x,
            y,
            &Text {
                size: SIZE_SMALL,
                bold: false,
                color: look.muted,
                text: k,
            },
        );
        draw::text_right(
            cr,
            x + w,
            y,
            &Text {
                size: SIZE_SMALL,
                bold: false,
                color: look.text,
                text: v,
            },
        );
        y += 20.0;
    }
}

fn draw_overwrite_bar(
    cr: &Context,
    look: &Look,
    state: &PickerState,
    r: Rect,
    hover_pos: Option<(f64, f64)>,
    hits: &mut LayoutHits,
) {
    let Some(ref prompt) = state.overwrite else {
        return;
    };
    draw::set(cr, look.alert_fill());
    cr.rectangle(r.x, r.y + 1.0, r.w, r.h - 1.0);
    let _ = cr.fill();

    let btn_y = r.y + (r.h - 28.0) * 0.5;
    let mut right_x = r.x + r.w - 14.0;

    let replace_r = Rect::new(right_x - 76.0, btn_y, 76.0, 28.0);
    draw_button(
        cr,
        look,
        replace_r,
        "Replace",
        ButtonStyle::Danger,
        true,
        false,
        hover_pos,
    );
    hits.push(replace_r, HitAction::OverwriteReplace, CursorKind::Pointer);
    right_x = replace_r.x - 8.0;

    let msg = match prompt {
        OverwritePrompt::Single { name, .. } => format!("{name} already exists. Replace it?"),
        OverwritePrompt::Multi { colliding, .. } => {
            let rename_r = Rect::new(right_x - 124.0, btn_y, 124.0, 28.0);
            draw_button(
                cr,
                look,
                rename_r,
                "Rename colliding",
                ButtonStyle::Secondary,
                true,
                false,
                hover_pos,
            );
            hits.push(rename_r, HitAction::OverwriteRename, CursorKind::Pointer);
            right_x = rename_r.x - 8.0;
            if colliding.len() == 1 {
                format!("{} already exists in this folder.", colliding[0])
            } else {
                format!("{} files already exist in this folder.", colliding.len())
            }
        }
    };

    let cancel_r = Rect::new(right_x - 68.0, btn_y, 68.0, 28.0);
    draw_button(
        cr,
        look,
        cancel_r,
        "Cancel",
        ButtonStyle::Secondary,
        true,
        false,
        hover_pos,
    );
    hits.push(cancel_r, HitAction::OverwriteCancel, CursorKind::Pointer);

    draw::text_max(
        cr,
        r.x + 14.0,
        r.y + (r.h - 16.0) * 0.5,
        (cancel_r.x - 12.0 - (r.x + 14.0)).max(40.0),
        &Text {
            size: SIZE_SMALL,
            bold: false,
            color: look.alert,
            text: &msg,
        },
    );
}

fn draw_save_row(cr: &Context, look: &Look, state: &PickerState, r: Rect, hits: &mut LayoutHits) {
    match &state.mode {
        PickerMode::SaveFile => {
            let lbl_w = draw::text(
                cr,
                r.x + 14.0,
                r.y + (r.h - 15.0) * 0.5,
                &Text {
                    size: SIZE_SMALL,
                    bold: false,
                    color: look.muted,
                    text: "Name",
                },
            );
            let inp_x = r.x + 14.0 + lbl_w + 10.0;
            let inp_r = Rect::new(
                inp_x,
                r.y + (r.h - 30.0) * 0.5,
                (r.x + r.w - 14.0 - inp_x).max(80.0),
                30.0,
            );
            let focused = state.focus == Focus::SaveName;
            draw::fill_round_rect(
                cr,
                inp_r.x,
                inp_r.y,
                inp_r.w,
                inp_r.h,
                look.small_radius,
                look.box_fill(),
            );
            draw::stroke_round_rect(
                cr,
                inp_r.x,
                inp_r.y,
                inp_r.w,
                inp_r.h,
                look.small_radius,
                if focused { look.accent } else { look.edge },
            );
            let inner = Rect::new(inp_r.x + 10.0, inp_r.y, inp_r.w - 20.0, inp_r.h);
            draw_text_input_inner(
                cr,
                look,
                &state.save_input,
                inner,
                SIZE_BODY,
                focused,
                "File name",
                false,
            );
            hits.push(inp_r, HitAction::SaveNameInput, CursorKind::Text);
            hits.text_boxes
                .push((Focus::SaveName, inner, SIZE_BODY, false));
        }
        PickerMode::SaveFiles { files } => {
            let summary = format!("Saving {} file(s): {}", files.len(), files.join(", "));
            draw::text_max(
                cr,
                r.x + 14.0,
                r.y + (r.h - 16.0) * 0.5,
                (r.w - 28.0).max(40.0),
                &Text {
                    size: SIZE_SMALL,
                    bold: false,
                    color: look.muted,
                    text: &summary,
                },
            );
        }
        PickerMode::OpenFile { .. } => {}
    }
}

fn draw_choices_row(
    cr: &Context,
    look: &Look,
    state: &PickerState,
    r: Rect,
    hover_pos: Option<(f64, f64)>,
    hits: &mut LayoutHits,
) -> Vec<(usize, Rect)> {
    let mut x = r.x + 14.0;
    let mut btn_rects = Vec::new();
    for (ci, ch) in state.choices.iter().enumerate() {
        let lbl_w = draw::text(
            cr,
            x,
            r.y + (r.h - 15.0) * 0.5,
            &Text {
                size: SIZE_SMALL,
                bold: false,
                color: look.muted,
                text: &ch.label,
            },
        );
        x += lbl_w + 8.0;

        if ch.options.is_empty() {
            // Boolean switch (30 x 16)
            let on = state.choice_values.get(&ch.id).is_some_and(|v| v == "true");
            let sw_r = Rect::new(x, r.y + (r.h - 16.0) * 0.5, 30.0, 16.0);
            draw::fill_round_rect(
                cr,
                sw_r.x,
                sw_r.y,
                sw_r.w,
                sw_r.h,
                8.0,
                if on { look.accent } else { look.track },
            );
            let knob_x = if on { sw_r.x + 16.0 } else { sw_r.x + 2.0 };
            draw::fill_round_rect(
                cr,
                knob_x,
                sw_r.y + 2.0,
                12.0,
                12.0,
                6.0,
                if on { look.on_accent } else { look.text },
            );
            hits.push(sw_r, HitAction::ChoiceBool(ci), CursorKind::Pointer);
            x += 46.0;
        } else {
            let cur_id = state
                .choice_values
                .get(&ch.id)
                .map(String::as_str)
                .unwrap_or("");
            let cur_label = ch
                .options
                .iter()
                .find(|o| o.id == cur_id)
                .map(|o| o.label.as_str())
                .unwrap_or(cur_id);
            let tw = draw::measure(
                cr,
                &Text {
                    size: SIZE_SMALL,
                    bold: false,
                    color: look.text,
                    text: cur_label,
                },
            )
            .0;
            let bw = (tw + 38.0).clamp(80.0, 220.0);
            let br = Rect::new(x, r.y + (r.h - 28.0) * 0.5, bw, 28.0);
            let hov = hover_pos.is_some_and(|(px, py)| br.contains(px, py));
            if hov {
                draw::fill_round_rect(
                    cr,
                    br.x,
                    br.y,
                    br.w,
                    br.h,
                    look.small_radius,
                    look.hover_fill(),
                );
            }
            draw::stroke_round_rect(cr, br.x, br.y, br.w, br.h, look.small_radius, look.edge);
            draw::text_max(
                cr,
                br.x + 10.0,
                br.y + 6.0,
                bw - 32.0,
                &Text {
                    size: SIZE_SMALL,
                    bold: false,
                    color: look.text,
                    text: cur_label,
                },
            );
            draw::icon(
                cr,
                Icon::ChevronDown,
                br.x + br.w - 22.0,
                br.y + 6.0,
                look.muted,
            );
            hits.push(br, HitAction::ChoiceMenuBtn(ci), CursorKind::Pointer);
            btn_rects.push((ci, br));
            x += bw + 16.0;
        }
    }
    btn_rects
}

fn draw_footer(
    cr: &Context,
    look: &Look,
    state: &PickerState,
    r: Rect,
    hover_pos: Option<(f64, f64)>,
    hits: &mut LayoutHits,
) -> Option<Rect> {
    let btn_y = r.y + (r.h - 30.0) * 0.5;
    let mut filter_rect = None;

    if let Some(active) = state.filters.active_filter() {
        let tw = draw::measure(
            cr,
            &Text {
                size: SIZE_SMALL,
                bold: false,
                color: look.text,
                text: &active.label,
            },
        )
        .0;
        let bw = (tw + 40.0).clamp(110.0, (r.w * 0.45).max(110.0));
        let fr = Rect::new(r.x + 14.0, btn_y, bw, 30.0);
        let hov = hover_pos.is_some_and(|(px, py)| fr.contains(px, py));
        let focused = state.focus == Focus::FilterBtn;
        if hov {
            draw::fill_round_rect(
                cr,
                fr.x,
                fr.y,
                fr.w,
                fr.h,
                look.small_radius,
                look.hover_fill(),
            );
        }
        draw::stroke_round_rect(
            cr,
            fr.x,
            fr.y,
            fr.w,
            fr.h,
            look.small_radius,
            if focused { look.accent } else { look.edge },
        );
        draw::text_max(
            cr,
            fr.x + 10.0,
            fr.y + 7.0,
            bw - 32.0,
            &Text {
                size: SIZE_SMALL,
                bold: false,
                color: look.text,
                text: &active.label,
            },
        );
        draw::icon(
            cr,
            Icon::ChevronDown,
            fr.x + fr.w - 22.0,
            fr.y + 7.0,
            look.muted,
        );
        hits.push(fr, HitAction::FilterBtn, CursorKind::Pointer);
        filter_rect = Some(fr);
    }

    let accept_tw = draw::measure(
        cr,
        &Text {
            size: SIZE_SMALL,
            bold: true,
            color: look.on_accent,
            text: &state.accept_label,
        },
    )
    .0;
    let accept_w = (accept_tw + 28.0).max(76.0);
    let accept_r = Rect::new(r.x + r.w - 14.0 - accept_w, btn_y, accept_w, 30.0);
    let can_accept = state.can_accept();
    draw_button(
        cr,
        look,
        accept_r,
        &state.accept_label,
        ButtonStyle::Primary,
        can_accept,
        state.focus == Focus::AcceptBtn,
        hover_pos,
    );
    if can_accept {
        hits.push(accept_r, HitAction::AcceptBtn, CursorKind::Pointer);
    }

    let cancel_w = 76.0;
    let cancel_r = Rect::new(accept_r.x - 8.0 - cancel_w, btn_y, cancel_w, 30.0);
    draw_button(
        cr,
        look,
        cancel_r,
        "Cancel",
        ButtonStyle::Secondary,
        true,
        state.focus == Focus::CancelBtn,
        hover_pos,
    );
    hits.push(cancel_r, HitAction::CancelBtn, CursorKind::Pointer);

    filter_rect
}

fn draw_filter_menu(
    cr: &Context,
    look: &Look,
    state: &PickerState,
    anchor: Rect,
    hover_pos: Option<(f64, f64)>,
    hits: &mut LayoutHits,
) {
    let items: Vec<(String, bool, HitAction)> = state
        .filters
        .filters
        .iter()
        .enumerate()
        .map(|(i, f)| {
            (
                f.label.clone(),
                i == state.filters.active,
                HitAction::FilterItem(i),
            )
        })
        .collect();
    draw_popup_menu(cr, look, anchor, &items, hover_pos, hits);
}

fn draw_choice_menu(
    cr: &Context,
    look: &Look,
    state: &PickerState,
    ci: usize,
    anchor: Rect,
    hover_pos: Option<(f64, f64)>,
    hits: &mut LayoutHits,
) {
    let Some(ch) = state.choices.get(ci) else {
        return;
    };
    let cur = state
        .choice_values
        .get(&ch.id)
        .map(String::as_str)
        .unwrap_or("");
    let items: Vec<(String, bool, HitAction)> = ch
        .options
        .iter()
        .map(|o| {
            (
                o.label.clone(),
                o.id == cur,
                HitAction::ChoiceMenuItem(ci, o.id.clone()),
            )
        })
        .collect();
    draw_popup_menu(cr, look, anchor, &items, hover_pos, hits);
}

fn draw_popup_menu(
    cr: &Context,
    look: &Look,
    anchor: Rect,
    items: &[(String, bool, HitAction)],
    hover_pos: Option<(f64, f64)>,
    hits: &mut LayoutHits,
) {
    if items.is_empty() {
        return;
    }
    let item_h = 28.0;
    let pad = 4.0;
    let menu_h = items.len() as f64 * item_h + pad * 2.0;
    let mut menu_w = anchor.w.max(200.0);
    for (label, _, _) in items {
        let tw = draw::measure(
            cr,
            &Text {
                size: SIZE_SMALL,
                bold: false,
                color: look.text,
                text: label,
            },
        )
        .0;
        menu_w = menu_w.max((tw + 44.0).min(360.0));
    }
    let menu_r = Rect::new(anchor.x, (anchor.y - 4.0 - menu_h).max(8.0), menu_w, menu_h);
    draw::fill_round_rect(
        cr,
        menu_r.x,
        menu_r.y,
        menu_r.w,
        menu_r.h,
        look.small_radius,
        look.window_bg(),
    );
    draw::stroke_round_rect(
        cr,
        menu_r.x,
        menu_r.y,
        menu_r.w,
        menu_r.h,
        look.small_radius,
        look.edge,
    );

    for (i, (label, selected, action)) in items.iter().enumerate() {
        let ir = Rect::new(
            menu_r.x + pad,
            menu_r.y + pad + i as f64 * item_h,
            menu_r.w - pad * 2.0,
            item_h,
        );
        let hov = hover_pos.is_some_and(|(px, py)| ir.contains(px, py));
        if *selected {
            draw::fill_round_rect(
                cr,
                ir.x,
                ir.y,
                ir.w,
                ir.h,
                look.small_radius,
                look.chosen_fill(),
            );
        } else if hov {
            draw::fill_round_rect(
                cr,
                ir.x,
                ir.y,
                ir.w,
                ir.h,
                look.small_radius,
                look.hover_fill(),
            );
        }
        draw::text_max(
            cr,
            ir.x + 8.0,
            ir.y + 6.0,
            ir.w - 30.0,
            &Text {
                size: SIZE_SMALL,
                bold: *selected,
                color: if *selected {
                    look.accent_text
                } else {
                    look.text
                },
                text: label,
            },
        );
        if *selected {
            draw::icon(
                cr,
                Icon::Check,
                ir.x + ir.w - 22.0,
                ir.y + 6.0,
                look.accent_text,
            );
        }
        hits.push(ir, action.clone(), CursorKind::Pointer);
    }
}

#[derive(Clone, Copy)]
enum ButtonStyle {
    Primary,
    Secondary,
    Danger,
}

#[allow(clippy::too_many_arguments)]
fn draw_button(
    cr: &Context,
    look: &Look,
    r: Rect,
    label: &str,
    style: ButtonStyle,
    enabled: bool,
    focused: bool,
    hover_pos: Option<(f64, f64)>,
) {
    let hov = enabled && hover_pos.is_some_and(|(px, py)| r.contains(px, py));
    let alpha = if enabled { 1.0 } else { 0.42 };
    match style {
        ButtonStyle::Primary => {
            draw::fill_round_rect(
                cr,
                r.x,
                r.y,
                r.w,
                r.h,
                look.small_radius,
                look.accent.alpha(look.accent.3 * alpha),
            );
            if hov {
                draw::fill_round_rect(cr, r.x, r.y, r.w, r.h, look.small_radius, look.hover_fill());
            }
            if focused {
                draw::stroke_round_rect(
                    cr,
                    r.x - 1.0,
                    r.y - 1.0,
                    r.w + 2.0,
                    r.h + 2.0,
                    look.small_radius,
                    look.text,
                );
            }
            draw::text_center(
                cr,
                r.x + r.w * 0.5,
                r.y + (r.h - 15.0) * 0.5,
                &Text {
                    size: SIZE_SMALL,
                    bold: true,
                    color: look.on_accent.alpha(look.on_accent.3 * alpha),
                    text: label,
                },
            );
        }
        ButtonStyle::Danger => {
            draw::fill_round_rect(cr, r.x, r.y, r.w, r.h, look.small_radius, look.alert);
            if hov {
                draw::fill_round_rect(cr, r.x, r.y, r.w, r.h, look.small_radius, look.hover_fill());
            }
            draw::text_center(
                cr,
                r.x + r.w * 0.5,
                r.y + (r.h - 15.0) * 0.5,
                &Text {
                    size: SIZE_SMALL,
                    bold: true,
                    color: look.on_accent,
                    text: label,
                },
            );
        }
        ButtonStyle::Secondary => {
            if hov {
                draw::fill_round_rect(cr, r.x, r.y, r.w, r.h, look.small_radius, look.hover_fill());
            }
            draw::stroke_round_rect(
                cr,
                r.x,
                r.y,
                r.w,
                r.h,
                look.small_radius,
                if focused { look.accent } else { look.edge },
            );
            draw::text_center(
                cr,
                r.x + r.w * 0.5,
                r.y + (r.h - 15.0) * 0.5,
                &Text {
                    size: SIZE_SMALL,
                    bold: false,
                    color: look.text.alpha(look.text.3 * alpha),
                    text: label,
                },
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_text_input_inner(
    cr: &Context,
    look: &Look,
    ti: &TextInput,
    r: Rect,
    size: f64,
    focused: bool,
    placeholder: &str,
    mono: bool,
) {
    let font = if mono { &look.mono_font } else { &look.font };
    draw::with_font(font, || {
        let _ = cr.save();
        cr.rectangle(r.x, r.y, r.w.max(0.0), r.h.max(0.0));
        cr.clip();

        let text_y = r.y + (r.h - (size + 4.0)) * 0.5;
        let s = ti.as_str();
        if s.is_empty() {
            if !placeholder.is_empty() {
                draw::text(
                    cr,
                    r.x,
                    text_y,
                    &Text {
                        size,
                        bold: false,
                        color: look.muted,
                        text: placeholder,
                    },
                );
            }
            if focused {
                draw::set(cr, look.accent);
                cr.rectangle(r.x.round(), r.y + 5.0, 1.5, (r.h - 10.0).max(8.0));
                let _ = cr.fill();
            }
            let _ = cr.restore();
            return;
        }

        let caret_x = draw::text_index_x(cr, size, s, ti.cursor());
        let scroll = (caret_x - (r.w - 4.0)).max(0.0);

        if focused {
            if let Some((a, b)) = ti.selection() {
                let sx0 = draw::text_index_x(cr, size, s, a) - scroll;
                let sx1 = draw::text_index_x(cr, size, s, b) - scroll;
                draw::set(cr, look.chosen_fill());
                cr.rectangle(
                    r.x + sx0,
                    r.y + 4.0,
                    (sx1 - sx0).max(2.0),
                    (r.h - 8.0).max(8.0),
                );
                let _ = cr.fill();
            }
        }

        draw::text(
            cr,
            r.x - scroll,
            text_y,
            &Text {
                size,
                bold: false,
                color: look.text,
                text: s,
            },
        );

        if focused {
            draw::set(cr, look.accent);
            cr.rectangle(
                (r.x + caret_x - scroll).round(),
                r.y + 5.0,
                1.5,
                (r.h - 10.0).max(8.0),
            );
            let _ = cr.fill();
        }

        let _ = cr.restore();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mime::MimeDb;
    use crate::places::Places;
    use crate::portal::protocol::{PortalOptions, PortalRequest};
    use std::sync::Arc;

    #[test]
    fn renders_open_and_save_windows_with_hit_targets() {
        let surf =
            cairo::ImageSurface::create(cairo::Format::ARgb32, DEFAULT_W as i32, DEFAULT_H as i32)
                .unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let look = Look::default();

        let req = PortalRequest {
            method: "SaveFile".into(),
            app_id: "org.mozilla.firefox".into(),
            parent_window: String::new(),
            title: "Save As".into(),
            options: PortalOptions {
                current_name: Some("report.pdf".into()),
                ..Default::default()
            },
        };
        let (state, _) = PickerState::from_request(
            req,
            Places::default(),
            Arc::new(MimeDb::default()),
            None,
            Vec::new(),
        );
        let hits = render(&cr, &look, &state, DEFAULT_W as f64, DEFAULT_H as f64, None);
        assert!(hits
            .targets
            .iter()
            .any(|t| t.action == HitAction::CancelBtn));
        assert!(hits
            .targets
            .iter()
            .any(|t| t.action == HitAction::AcceptBtn));
        assert!(hits
            .targets
            .iter()
            .any(|t| t.action == HitAction::SaveNameInput));
        assert!(hits.list_rect.is_some());
    }
}
