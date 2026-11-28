//! The markup step after a pick: arrows, boxes, pen lines, text, pixelated parts and a crop drawn
//! on the capture, undone one at a time, and the picture they make. Wayland stays in wayland.rs:
//! this module takes pointer and key input in the editor surface's buffer pixels and draws.

use std::ops::Range;

use smithay_client_toolkit::seat::keyboard::Keysym;

use crate::draw::rounded_rect;
use crate::geom::{crop_upright_surface, Rect, RectF};
use crate::look::{Color, Look};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Arrow,
    Box,
    Pen,
    Text,
    Pixelate,
    Crop,
}

const TOOLS: [Tool; 6] = [
    Tool::Arrow,
    Tool::Box,
    Tool::Pen,
    Tool::Text,
    Tool::Pixelate,
    Tool::Crop,
];

impl Tool {
    fn label(self) -> &'static str {
        match self {
            Tool::Arrow => "Arrow",
            Tool::Box => "Box",
            Tool::Pen => "Pen",
            Tool::Text => "Text",
            Tool::Pixelate => "Pixelate",
            Tool::Crop => "Crop",
        }
    }

    /// The key that picks it, as typed
    fn key(self) -> &'static str {
        match self {
            Tool::Arrow => "A",
            Tool::Box => "B",
            Tool::Pen => "P",
            Tool::Text => "T",
            Tool::Pixelate => "X",
            Tool::Crop => "C",
        }
    }
}

/// One mark, in the capture's pixels.
#[derive(Debug, Clone, PartialEq)]
pub enum Mark {
    Arrow {
        from: (f64, f64),
        to: (f64, f64),
        color: Color,
        width: f64,
    },
    Box {
        rect: RectF,
        color: Color,
        width: f64,
    },
    Pen {
        points: Vec<(f64, f64)>,
        color: Color,
        width: f64,
    },
    /// `at` is the top-left corner of the text.
    Text {
        at: (f64, f64),
        text: String,
        color: Color,
        size: f64,
    },
    /// Each `block` x `block` square inside `rect` becomes its average colour.
    Pixelate { rect: Rect, block: i32 },
    /// The picture is cut to the last crop.
    Crop { rect: Rect },
}

/// How the editor was left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finish {
    /// Enter: the marked-up picture to the clipboard, no file
    Copy,
    /// Ctrl+S: the marked-up picture to the file and the clipboard
    Save,
    /// Escape: the capture without marks, as without the editor
    Original,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Nothing,
    Redraw,
    Done(Finish),
}

/// A toolbar control under the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Tool(Tool),
    Color(usize),
    Undo,
    Finish(Finish),
}

impl Hit {
    /// The control's name in the INFYNITAE_SHOT_BOXES dump.
    pub fn name(self) -> String {
        match self {
            Hit::Tool(t) => t.label().to_lowercase(),
            Hit::Color(i) => format!("color{}", i + 1),
            Hit::Undo => "undo".to_string(),
            Hit::Finish(Finish::Copy) => "copy".to_string(),
            Hit::Finish(Finish::Save) => "save".to_string(),
            Hit::Finish(Finish::Original) => "original".to_string(),
        }
    }
}

/// Sizes in the editor's logical pixels; marks get them in capture pixels at the zoom they are
/// drawn at, so a line looks as thick on screen whatever the capture's size.
const STROKE: f64 = 3.0;
const TEXT_SIZE: f64 = 20.0;
/// Large enough that 12 to 16 px text under it cannot be read
const PIXEL_BLOCK: f64 = 10.0;
const MARGIN: f64 = 16.0;
const BAR_H: f64 = 36.0;
const BAR_FONT: f64 = 13.0;
const BAR_GAP: f64 = 16.0;
const SWATCH: f64 = 14.0;

/// A drag in progress, in capture pixels.
struct Stroke {
    tool: Tool,
    points: Vec<(f64, f64)>,
}

struct Typing {
    at: (f64, f64),
    text: String,
}

pub struct Editor {
    base: cairo::ImageSurface,
    /// `base` with every mark drawn, before the crop
    composed: cairo::ImageSurface,
    /// `composed` cut and scaled as shown, and where: a frame that scales a 4K capture took
    /// 2.1 ms, one that paints this 0.6 ms (release build)
    shown: Option<(RectF, cairo::ImageSurface)>,
    marks: Vec<Mark>,
    tool: Tool,
    palette: [Color; 3],
    color: usize,
    font: String,
    stroke: Option<Stroke>,
    typing: Option<Typing>,
    buf_w: f64,
    buf_h: f64,
    /// buffer pixels per logical pixel
    ui: f64,
    /// how many rows the toolbar took at the last frame
    bar_rows: usize,
    hits: Vec<(RectF, Hit)>,
}

impl Editor {
    pub fn new(base: cairo::ImageSurface, look: &Look) -> Result<Self, String> {
        let composed = copy_surface(&base)?;
        Ok(Self {
            base,
            composed,
            shown: None,
            marks: Vec::new(),
            tool: Tool::Arrow,
            palette: [look.accent, look.text, look.card],
            color: 0,
            font: look.font.clone(),
            stroke: None,
            typing: None,
            buf_w: 1.0,
            buf_h: 1.0,
            ui: 1.0,
            bar_rows: 1,
            hits: Vec::new(),
        })
    }

    /// The editor surface's buffer size and buffer pixels per logical pixel.
    pub fn set_view(&mut self, buf_w: u32, buf_h: u32, ui: f64) {
        self.buf_w = buf_w.max(1) as f64;
        self.buf_h = buf_h.max(1) as f64;
        self.ui = ui.max(0.1);
    }

    pub fn hits(&self) -> &[(RectF, Hit)] {
        &self.hits
    }

    /// Whether a press at (`x`, `y`) would work a toolbar control; never during a stroke, which
    /// keeps drawing wherever the pointer goes.
    pub fn control_at(&self, x: f64, y: f64) -> bool {
        self.stroke.is_none() && self.hits.iter().any(|(r, _)| contains(*r, x, y))
    }

    /// The part of the capture shown and kept: the last crop, else all of it.
    pub fn crop(&self) -> Rect {
        crop_of(&self.marks, self.base.width(), self.base.height())
    }

    /// Where the capture is shown in buffer pixels, and buffer pixels per capture pixel.
    pub fn frame(&self) -> (RectF, f64) {
        let crop = self.crop();
        let m = MARGIN * self.ui;
        let aw = (self.buf_w - 2.0 * m).max(1.0);
        let rows = self.bar_rows as f64;
        let bar = rows * BAR_H * self.ui + (rows - 1.0) * m / 2.0;
        let ah = (self.buf_h - 3.0 * m - bar).max(1.0);
        let (cw, ch) = (crop.w.max(1) as f64, crop.h.max(1) as f64);
        let fit = (aw / cw).min(ah / ch).min(1.0);
        let (w, h) = (cw * fit, ch * fit);
        // at whole pixels, so that the shown picture is copied without resampling
        let (x, y) = ((m + (aw - w) / 2.0).round(), (m + (ah - h) / 2.0).round());
        (RectF::new(x, y, w, h), fit)
    }

    fn to_image(&self, x: f64, y: f64) -> (f64, f64) {
        let (img, fit) = self.frame();
        let crop = self.crop();
        let px = (x.clamp(img.x, img.right()) - img.x) / fit + crop.x as f64;
        let py = (y.clamp(img.y, img.bottom()) - img.y) / fit + crop.y as f64;
        (px, py)
    }

    /// Capture pixels per logical pixel at the current zoom.
    fn unit(&self) -> f64 {
        self.ui / self.frame().1
    }

    pub fn press(&mut self, x: f64, y: f64) -> Result<Step, String> {
        if let Some(&(_, hit)) = self.hits.iter().find(|(r, _)| contains(*r, x, y)) {
            return self.apply_hit(hit);
        }
        let (img, _) = self.frame();
        if !contains(img, x, y) {
            return Ok(Step::Nothing);
        }
        let at = self.to_image(x, y);
        if self.tool == Tool::Text {
            self.commit_typing()?;
            let size = TEXT_SIZE * self.unit();
            self.typing = Some(Typing {
                at: (at.0, at.1 - size * 0.6),
                text: String::new(),
            });
            return Ok(Step::Redraw);
        }
        self.commit_typing()?;
        self.stroke = Some(Stroke {
            tool: self.tool,
            points: vec![at],
        });
        Ok(Step::Redraw)
    }

    pub fn motion(&mut self, x: f64, y: f64) -> Step {
        let at = self.to_image(x, y);
        let Some(stroke) = &mut self.stroke else {
            return Step::Nothing;
        };
        // a pen keeps every point, the other tools their start and the pointer
        if stroke.tool == Tool::Pen || stroke.points.len() == 1 {
            stroke.points.push(at);
        } else {
            stroke.points[1] = at;
        }
        Step::Redraw
    }

    pub fn release(&mut self, x: f64, y: f64) -> Result<Step, String> {
        if self.stroke.is_none() {
            return Ok(Step::Nothing);
        }
        self.motion(x, y);
        let Some(stroke) = self.stroke.take() else {
            return Ok(Step::Nothing);
        };
        if let Some(mark) = self.mark_from(&stroke) {
            self.push(mark)?;
        }
        Ok(Step::Redraw)
    }

    fn mark_from(&self, stroke: &Stroke) -> Option<Mark> {
        let unit = self.unit();
        let color = self.palette[self.color];
        let first = stroke.points[0];
        let last = *stroke.points.last().unwrap_or(&first);
        let rect = RectF::from_points(first, last);
        // a click is not a mark: under one logical pixel of travel
        let moved = rect.w.max(rect.h) >= unit;
        match stroke.tool {
            Tool::Arrow if moved => Some(Mark::Arrow {
                from: first,
                to: last,
                color,
                width: STROKE * unit,
            }),
            Tool::Box if moved => Some(Mark::Box {
                rect,
                color,
                width: STROKE * unit,
            }),
            Tool::Pen => Some(Mark::Pen {
                points: stroke.points.clone(),
                color,
                width: STROKE * unit,
            }),
            Tool::Pixelate if moved => pixel_rect(rect, self.crop()).map(|rect| Mark::Pixelate {
                rect,
                block: (PIXEL_BLOCK * unit).round().max(4.0) as i32,
            }),
            Tool::Crop if moved => pixel_rect(rect, self.crop()).map(|rect| Mark::Crop { rect }),
            _ => None,
        }
    }

    fn push(&mut self, mark: Mark) -> Result<(), String> {
        apply_mark(&mut self.composed, &mark, &self.font)?;
        self.marks.push(mark);
        self.shown = None;
        Ok(())
    }

    fn commit_typing(&mut self) -> Result<(), String> {
        let Some(typing) = self.typing.take() else {
            return Ok(());
        };
        if typing.text.is_empty() {
            return Ok(());
        }
        let mark = Mark::Text {
            at: typing.at,
            text: typing.text,
            color: self.palette[self.color],
            size: TEXT_SIZE * self.unit(),
        };
        self.push(mark)
    }

    fn undo(&mut self) -> Result<(), String> {
        if self.marks.pop().is_some() {
            self.composed = compose(&self.base, &self.marks, &self.font)?;
            self.shown = None;
        }
        Ok(())
    }

    fn apply_hit(&mut self, hit: Hit) -> Result<Step, String> {
        match hit {
            Hit::Tool(t) => {
                self.commit_typing()?;
                self.tool = t;
            }
            Hit::Color(i) => self.color = i,
            Hit::Undo => {
                if self.typing.take().is_none() {
                    self.undo()?;
                }
            }
            Hit::Finish(f) => {
                if f != Finish::Original {
                    self.commit_typing()?;
                }
                return Ok(Step::Done(f));
            }
        }
        Ok(Step::Redraw)
    }

    /// A key press. `text` is what the key types; `ctrl` whether Control is held.
    pub fn key(&mut self, sym: Keysym, text: Option<&str>, ctrl: bool) -> Result<Step, String> {
        if let Some(typing) = &mut self.typing {
            if !ctrl {
                match sym {
                    Keysym::Return | Keysym::KP_Enter => {
                        self.commit_typing()?;
                        return Ok(Step::Redraw);
                    }
                    Keysym::Escape => {
                        self.typing = None;
                        return Ok(Step::Redraw);
                    }
                    Keysym::BackSpace => {
                        typing.text.pop();
                        return Ok(Step::Redraw);
                    }
                    _ => {}
                }
                let typed: String = text
                    .unwrap_or("")
                    .chars()
                    .filter(|c| !c.is_control())
                    .collect();
                if typed.is_empty() {
                    return Ok(Step::Nothing);
                }
                typing.text.push_str(&typed);
                return Ok(Step::Redraw);
            }
            // a shortcut while typing: Control+Z drops the text, the others keep it first
            if matches!(sym, Keysym::z | Keysym::Z) {
                self.typing = None;
                return Ok(Step::Redraw);
            }
            self.commit_typing()?;
        }

        let hit = if ctrl {
            match sym {
                Keysym::s | Keysym::S => Hit::Finish(Finish::Save),
                Keysym::z | Keysym::Z => Hit::Undo,
                _ => return Ok(Step::Nothing),
            }
        } else {
            match sym {
                Keysym::Return | Keysym::KP_Enter => Hit::Finish(Finish::Copy),
                Keysym::Escape => Hit::Finish(Finish::Original),
                Keysym::_1 => Hit::Color(0),
                Keysym::_2 => Hit::Color(1),
                Keysym::_3 => Hit::Color(2),
                _ => {
                    let Some(key) = text.map(|t| t.to_uppercase()) else {
                        return Ok(Step::Nothing);
                    };
                    match TOOLS.into_iter().find(|t| t.key() == key) {
                        Some(t) => Hit::Tool(t),
                        None => return Ok(Step::Nothing),
                    }
                }
            }
        };
        self.apply_hit(hit)
    }

    /// The marked-up picture: every mark, cut to the crop.
    pub fn result(&self) -> Result<cairo::ImageSurface, String> {
        let crop = self.crop();
        let mut composed = copy_surface(&self.composed)?;
        if let Some(typing) = &self.typing {
            apply_mark(
                &mut composed,
                &Mark::Text {
                    at: typing.at,
                    text: typing.text.clone(),
                    color: self.palette[self.color],
                    size: TEXT_SIZE * self.unit(),
                },
                &self.font,
            )?;
        }
        crop_upright_surface(&composed, crop)
    }

    pub fn into_base(self) -> cairo::ImageSurface {
        self.base
    }

    /// Draws the editor into `dst` (the surface's buffer) and records the toolbar's controls.
    pub fn render(&mut self, dst: &cairo::ImageSurface, look: &Look) -> Result<(), String> {
        let cr = cairo::Context::new(dst).map_err(|e| format!("cairo context failed: {e}"))?;
        let items = self.bar_items(&cr, look);
        let rows = self.bar_rows(&items);
        self.bar_rows = rows.len();
        let (img, fit) = self.frame();
        let crop = self.crop();
        cr.set_operator(cairo::Operator::Source);
        look.card.set_source_with_alpha(&cr, 0.85);
        cr.paint().map_err(|e| format!("cairo paint failed: {e}"))?;
        cr.set_operator(cairo::Operator::Over);

        if !matches!(&self.shown, Some((r, _)) if *r == img) {
            self.shown = Some((img, scaled(&self.composed, crop, img, fit)?));
        }
        if let Some((_, shown)) = &self.shown {
            cr.set_source_surface(shown, img.x, img.y)
                .map_err(|e| format!("cairo source failed: {e}"))?;
            cr.rectangle(img.x, img.y, img.w, img.h);
            cr.fill().map_err(|e| format!("cairo paint failed: {e}"))?;
        }

        cr.save().map_err(|e| format!("cairo save failed: {e}"))?;
        cr.rectangle(img.x, img.y, img.w, img.h);
        cr.clip();
        cr.translate(img.x, img.y);
        cr.scale(fit, fit);
        cr.translate(-crop.x as f64, -crop.y as f64);

        if let Some(stroke) = &self.stroke {
            match self.mark_from(stroke) {
                Some(Mark::Pixelate { rect, .. }) | Some(Mark::Crop { rect }) => {
                    if stroke.tool == Tool::Crop {
                        // outside the new crop dimmed, as the selection overlay dims
                        look.card.set_source_with_alpha(&cr, 0.40);
                        cr.set_fill_rule(cairo::FillRule::EvenOdd);
                        cr.rectangle(crop.x as f64, crop.y as f64, crop.w as f64, crop.h as f64);
                        cr.rectangle(rect.x as f64, rect.y as f64, rect.w as f64, rect.h as f64);
                        cr.fill().map_err(|e| format!("cairo fill failed: {e}"))?;
                    }
                    cr.set_line_width(1.0 / fit);
                    look.accent.set_source_with_alpha(&cr, 1.0);
                    cr.rectangle(rect.x as f64, rect.y as f64, rect.w as f64, rect.h as f64);
                    cr.stroke()
                        .map_err(|e| format!("cairo stroke failed: {e}"))?;
                }
                Some(mark) => draw_vector(&cr, &mark, &self.font)?,
                None => {}
            }
        }
        if let Some(typing) = &self.typing {
            let color = self.palette[self.color];
            let size = TEXT_SIZE * self.unit();
            let layout = text_layout(&cr, &self.font, size, &typing.text);
            let (tw, th) = layout.pixel_size();
            color.set_source_with_alpha(&cr, 1.0);
            cr.move_to(typing.at.0, typing.at.1);
            pangocairo::functions::show_layout(&cr, &layout);
            // the caret after the text
            cr.set_line_width(1.0 / fit);
            cr.move_to(typing.at.0 + tw as f64 + 1.0 / fit, typing.at.1);
            cr.rel_line_to(0.0, th.max(1) as f64);
            cr.stroke()
                .map_err(|e| format!("cairo stroke failed: {e}"))?;
        }
        cr.restore()
            .map_err(|e| format!("cairo restore failed: {e}"))?;

        cr.set_line_width(1.0);
        look.edge.set_source_with_alpha(&cr, 1.0);
        cr.rectangle(img.x - 0.5, img.y - 0.5, img.w + 1.0, img.h + 1.0);
        cr.stroke()
            .map_err(|e| format!("cairo outline failed: {e}"))?;

        self.hits = self.render_bar(&cr, look, &items, &rows)?;
        drop(cr);
        dst.flush();
        Ok(())
    }

    fn bar_items(&self, cr: &cairo::Context, look: &Look) -> Vec<BarItem> {
        let ui = self.ui;
        let mut items: Vec<(Hit, Option<(&str, &str)>)> = TOOLS
            .into_iter()
            .map(|t| (Hit::Tool(t), Some((t.label(), t.key()))))
            .collect();
        items.extend((0..self.palette.len()).map(|i| (Hit::Color(i), None)));
        items.extend([
            (Hit::Undo, Some(("Undo", "Ctrl+Z"))),
            (Hit::Finish(Finish::Copy), Some(("Copy", "Enter"))),
            (Hit::Finish(Finish::Save), Some(("Save", "Ctrl+S"))),
            (Hit::Finish(Finish::Original), Some(("Original", "Esc"))),
        ]);
        items
            .into_iter()
            .map(|(hit, text)| {
                let text = text.map(|(label, key)| {
                    (
                        text_layout(cr, &look.font, BAR_FONT * ui, label),
                        text_layout(cr, &look.font, BAR_FONT * ui, key),
                    )
                });
                let w = match &text {
                    Some((a, b)) => (a.pixel_size().0 + b.pixel_size().0) as f64 + 6.0 * ui,
                    None => SWATCH * ui,
                };
                BarItem { hit, text, w }
            })
            .collect()
    }

    /// The toolbar's rows: as many controls on each as fit the view's width.
    fn bar_rows(&self, items: &[BarItem]) -> Vec<Range<usize>> {
        let gap = BAR_GAP * self.ui;
        let avail = self.buf_w - 2.0 * MARGIN * self.ui;
        let mut rows = Vec::new();
        let (mut start, mut width) = (0, gap);
        for (i, item) in items.iter().enumerate() {
            if i > start && width + item.w + gap > avail {
                rows.push(start..i);
                (start, width) = (i, gap);
            }
            width += item.w + gap;
        }
        rows.push(start..items.len());
        rows
    }

    fn render_bar(
        &self,
        cr: &cairo::Context,
        look: &Look,
        items: &[BarItem],
        rows: &[Range<usize>],
    ) -> Result<Vec<(RectF, Hit)>, String> {
        let ui = self.ui;
        let gap = BAR_GAP * ui;
        let bar_h = BAR_H * ui;
        let m = MARGIN * ui;
        let mut hits = Vec::with_capacity(items.len());
        for (n, row) in rows.iter().enumerate() {
            let row = &items[row.clone()];
            let below = (rows.len() - 1 - n) as f64;
            let total = row.iter().map(|i| i.w + gap).sum::<f64>() + gap;
            let bx = ((self.buf_w - total) / 2.0).max(0.0);
            let by = self.buf_h - m - bar_h - below * (bar_h + m / 2.0);

            look.card.set_source_with_alpha(cr, 1.0);
            rounded_rect(cr, bx, by, total, bar_h, look.small_corners as f64 * ui);
            cr.fill().map_err(|e| format!("cairo fill failed: {e}"))?;

            let mut x = bx + gap;
            for item in row {
                match (&item.text, item.hit) {
                    (Some((label, key)), hit) => {
                        let active = hit == Hit::Tool(self.tool);
                        let th = label.pixel_size().1 as f64;
                        let ty = by + (bar_h - th) / 2.0;
                        let color = if active { look.accent } else { look.text };
                        color.set_source_with_alpha(cr, 1.0);
                        cr.move_to(x, ty);
                        pangocairo::functions::show_layout(cr, label);
                        look.text.set_source_with_alpha(cr, 0.6);
                        cr.move_to(x + label.pixel_size().0 as f64 + 6.0 * ui, ty);
                        pangocairo::functions::show_layout(cr, key);
                    }
                    (None, Hit::Color(i)) => {
                        let s = SWATCH * ui;
                        let sy = by + (bar_h - s) / 2.0;
                        self.palette[i].set_source_with_alpha(cr, 1.0);
                        cr.rectangle(x, sy, s, s);
                        cr.fill().map_err(|e| format!("cairo fill failed: {e}"))?;
                        // the colour in use is outlined; the others have the edge colour around
                        // them so that a swatch of the card's own colour is seen
                        let (ring, lw) = if i == self.color {
                            (look.text, 2.0 * ui)
                        } else {
                            (look.edge, 1.0)
                        };
                        ring.set_source_with_alpha(cr, 1.0);
                        cr.set_line_width(lw);
                        cr.rectangle(x - lw / 2.0, sy - lw / 2.0, s + lw, s + lw);
                        cr.stroke()
                            .map_err(|e| format!("cairo stroke failed: {e}"))?;
                    }
                    (None, _) => {}
                }
                hits.push((RectF::new(x - gap / 2.0, by, item.w + gap, bar_h), item.hit));
                x += item.w + gap;
            }
        }
        Ok(hits)
    }
}

/// A toolbar control: a label and its key, or a colour swatch.
struct BarItem {
    hit: Hit,
    text: Option<(pango::Layout, pango::Layout)>,
    w: f64,
}

/// The `crop` of `src` at `fit`, `img`'s size.
fn scaled(
    src: &cairo::ImageSurface,
    crop: Rect,
    img: RectF,
    fit: f64,
) -> Result<cairo::ImageSurface, String> {
    let out = cairo::ImageSurface::create(
        cairo::Format::ARgb32,
        img.w.ceil().max(1.0) as i32,
        img.h.ceil().max(1.0) as i32,
    )
    .map_err(|e| format!("cairo surface failed: {e}"))?;
    let cr = cairo::Context::new(&out).map_err(|e| format!("cairo context failed: {e}"))?;
    cr.set_operator(cairo::Operator::Source);
    cr.scale(fit, fit);
    cr.set_source_surface(src, -crop.x as f64, -crop.y as f64)
        .map_err(|e| format!("cairo source failed: {e}"))?;
    // cairo's default filter took 56 ms to halve a 4K capture, bilinear 1.8 ms (release build);
    // only the preview is scaled, the saved picture keeps every pixel
    cr.source().set_filter(cairo::Filter::Bilinear);
    cr.paint().map_err(|e| format!("cairo paint failed: {e}"))?;
    drop(cr);
    out.flush();
    Ok(out)
}

fn contains(r: RectF, x: f64, y: f64) -> bool {
    x >= r.x && x < r.right() && y >= r.y && y < r.bottom()
}

/// The whole pixels a dragged rectangle covers, inside `bounds`.
fn pixel_rect(r: RectF, bounds: Rect) -> Option<Rect> {
    let x0 = r.x.floor() as i32;
    let y0 = r.y.floor() as i32;
    let x1 = r.right().ceil() as i32;
    let y1 = r.bottom().ceil() as i32;
    Rect::new(x0, y0, x1 - x0, y1 - y0).clamp_to(bounds)
}

pub fn crop_of(marks: &[Mark], w: i32, h: i32) -> Rect {
    marks
        .iter()
        .rev()
        .find_map(|m| match m {
            Mark::Crop { rect } => Some(*rect),
            _ => None,
        })
        .unwrap_or(Rect::new(0, 0, w, h))
}

fn copy_surface(src: &cairo::ImageSurface) -> Result<cairo::ImageSurface, String> {
    crop_upright_surface(src, Rect::new(0, 0, src.width(), src.height()))
}

/// `base` with `marks` drawn in order, before the crop.
pub fn compose(
    base: &cairo::ImageSurface,
    marks: &[Mark],
    font: &str,
) -> Result<cairo::ImageSurface, String> {
    let mut out = copy_surface(base)?;
    for mark in marks {
        apply_mark(&mut out, mark, font)?;
    }
    Ok(out)
}

fn apply_mark(surface: &mut cairo::ImageSurface, mark: &Mark, font: &str) -> Result<(), String> {
    match mark {
        Mark::Pixelate { rect, block } => pixelate(surface, *rect, *block),
        Mark::Crop { .. } => Ok(()),
        _ => {
            let cr =
                cairo::Context::new(&*surface).map_err(|e| format!("cairo context failed: {e}"))?;
            draw_vector(&cr, mark, font)?;
            drop(cr);
            surface.flush();
            Ok(())
        }
    }
}

fn draw_vector(cr: &cairo::Context, mark: &Mark, font: &str) -> Result<(), String> {
    let fail = |e: cairo::Error| format!("cairo drawing failed: {e}");
    match mark {
        Mark::Arrow {
            from,
            to,
            color,
            width,
        } => {
            let (dx, dy) = (to.0 - from.0, to.1 - from.1);
            let len = dx.hypot(dy).max(1e-6);
            let (ux, uy) = (dx / len, dy / len);
            let head = (width * 4.5).min(len);
            let half = head * 0.6;
            let base = (to.0 - ux * head, to.1 - uy * head);
            color.set_source_with_alpha(cr, 1.0);
            cr.set_line_width(*width);
            cr.set_line_cap(cairo::LineCap::Round);
            cr.move_to(from.0, from.1);
            cr.line_to(base.0, base.1);
            cr.stroke().map_err(fail)?;
            cr.move_to(to.0, to.1);
            cr.line_to(base.0 - uy * half, base.1 + ux * half);
            cr.line_to(base.0 + uy * half, base.1 - ux * half);
            cr.close_path();
            cr.fill().map_err(fail)?;
        }
        Mark::Box { rect, color, width } => {
            color.set_source_with_alpha(cr, 1.0);
            cr.set_line_width(*width);
            cr.set_line_join(cairo::LineJoin::Miter);
            cr.rectangle(rect.x, rect.y, rect.w, rect.h);
            cr.stroke().map_err(fail)?;
        }
        Mark::Pen {
            points,
            color,
            width,
        } => {
            color.set_source_with_alpha(cr, 1.0);
            cr.set_line_width(*width);
            cr.set_line_cap(cairo::LineCap::Round);
            cr.set_line_join(cairo::LineJoin::Round);
            let Some(first) = points.first() else {
                return Ok(());
            };
            cr.move_to(first.0, first.1);
            if points.len() == 1 {
                // a click: a dot
                cr.line_to(first.0, first.1);
            }
            for p in &points[1..] {
                cr.line_to(p.0, p.1);
            }
            cr.stroke().map_err(fail)?;
        }
        Mark::Text {
            at,
            text,
            color,
            size,
        } => {
            let layout = text_layout(cr, font, *size, text);
            color.set_source_with_alpha(cr, 1.0);
            cr.move_to(at.0, at.1);
            pangocairo::functions::show_layout(cr, &layout);
        }
        Mark::Pixelate { .. } | Mark::Crop { .. } => {}
    }
    Ok(())
}

fn text_layout(cr: &cairo::Context, font: &str, size: f64, text: &str) -> pango::Layout {
    let layout = pangocairo::functions::create_layout(cr);
    let mut desc = pango::FontDescription::from_string(font);
    desc.set_absolute_size(size * pango::SCALE as f64);
    layout.set_font_description(Some(&desc));
    layout.set_text(text);
    layout
}

/// Replaces each `block` x `block` square of `rect` (squares start at its top-left corner; the
/// last row and column may be smaller) with the average of its pixels. Averaging throws the
/// detail away, which a blur would not: a blur can be partly undone.
pub fn pixelate(surface: &mut cairo::ImageSurface, rect: Rect, block: i32) -> Result<(), String> {
    let Some(r) = rect.clamp_to(Rect::new(0, 0, surface.width(), surface.height())) else {
        return Ok(());
    };
    let block = block.max(1);
    surface.flush();
    let stride = surface.stride() as usize;
    let mut data = surface
        .data()
        .map_err(|e| format!("failed to map surface for pixelate: {e}"))?;
    let mut y0 = r.y;
    while y0 < r.bottom() {
        let y1 = (y0 + block).min(r.bottom());
        let mut x0 = r.x;
        while x0 < r.right() {
            let x1 = (x0 + block).min(r.right());
            let mut sum = [0u32; 4];
            for y in y0..y1 {
                let row = y as usize * stride;
                for x in x0..x1 {
                    let o = row + x as usize * 4;
                    for (c, s) in sum.iter_mut().enumerate() {
                        *s += data[o + c] as u32;
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            let avg = sum.map(|s| ((s + n / 2) / n) as u8);
            for y in y0..y1 {
                let row = y as usize * stride;
                for x in x0..x1 {
                    let o = row + x as usize * 4;
                    data[o..o + 4].copy_from_slice(&avg);
                }
            }
            x0 = x1;
        }
        y0 = y1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: i32, h: i32, rgb: (f64, f64, f64)) -> cairo::ImageSurface {
        let s = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).unwrap();
        let cr = cairo::Context::new(&s).unwrap();
        cr.set_source_rgb(rgb.0, rgb.1, rgb.2);
        cr.paint().unwrap();
        drop(cr);
        s
    }

    fn px(s: &mut cairo::ImageSurface, x: i32, y: i32) -> [u8; 4] {
        s.flush();
        let stride = s.stride() as usize;
        let d = s.data().unwrap();
        let o = y as usize * stride + x as usize * 4;
        [d[o], d[o + 1], d[o + 2], d[o + 3]]
    }

    fn look() -> Look {
        Look {
            accent: Color::rgb8(0, 255, 0),
            ..Default::default()
        }
    }

    /// 200 x 100 white in a 400 x 300 view at scale 1: shown at full size from (100, 74)
    fn editor() -> Editor {
        let mut e = Editor::new(solid(200, 100, (1.0, 1.0, 1.0)), &look()).unwrap();
        e.set_view(400, 300, 1.0);
        e
    }

    fn drag(e: &mut Editor, from: (f64, f64), to: (f64, f64)) {
        e.press(100.0 + from.0, 74.0 + from.1).unwrap();
        e.motion(100.0 + (from.0 + to.0) / 2.0, 74.0 + (from.1 + to.1) / 2.0);
        e.release(100.0 + to.0, 74.0 + to.1).unwrap();
    }

    fn key(e: &mut Editor, sym: Keysym, text: &str, ctrl: bool) -> Step {
        e.key(sym, (!text.is_empty()).then_some(text), ctrl)
            .unwrap()
    }

    #[test]
    fn pixelate_averages_each_block_and_leaves_the_rest() {
        // columns 0 and 1 white, 2 to 7 black
        let mut s = solid(8, 4, (0.0, 0.0, 0.0));
        {
            let cr = cairo::Context::new(&s).unwrap();
            cr.set_source_rgb(1.0, 1.0, 1.0);
            cr.rectangle(0.0, 0.0, 2.0, 4.0);
            cr.fill().unwrap();
        }
        pixelate(&mut s, Rect::new(0, 0, 4, 4), 4).unwrap();
        // 8 white and 8 black pixels: (8 * 255 + 8) / 16 = 128
        assert_eq!(px(&mut s, 0, 0), [128, 128, 128, 255]);
        assert_eq!(px(&mut s, 3, 3), [128, 128, 128, 255]);
        assert_eq!(px(&mut s, 4, 0), [0, 0, 0, 255]);
    }

    #[test]
    fn pixelate_cuts_blocks_at_the_rect_and_the_picture_edge() {
        let mut s = solid(10, 10, (1.0, 1.0, 1.0));
        {
            let cr = cairo::Context::new(&s).unwrap();
            cr.set_source_rgb(0.0, 0.0, 0.0);
            cr.rectangle(9.0, 0.0, 1.0, 10.0);
            cr.fill().unwrap();
        }
        // from x 6 past the right edge: blocks of 3 are columns 6..9 and 9 alone
        pixelate(&mut s, Rect::new(6, 0, 20, 3), 3).unwrap();
        assert_eq!(px(&mut s, 7, 1), [255, 255, 255, 255]);
        assert_eq!(px(&mut s, 9, 1), [0, 0, 0, 255]);
        assert_eq!(px(&mut s, 5, 1), [255, 255, 255, 255]);
    }

    #[test]
    fn a_dragged_box_is_drawn_in_the_accent_on_the_picture() {
        let mut e = editor();
        assert_eq!(key(&mut e, Keysym::b, "b", false), Step::Redraw);
        assert_eq!(e.tool, Tool::Box);
        drag(&mut e, (10.0, 10.0), (50.0, 50.0));
        assert_eq!(e.marks.len(), 1);
        let mut out = e.result().unwrap();
        assert_eq!((out.width(), out.height()), (200, 100));
        // the box's left side at x 10, its inside untouched
        assert_eq!(px(&mut out, 10, 30), [0, 255, 0, 255]);
        assert_eq!(px(&mut out, 30, 30), [255, 255, 255, 255]);
    }

    #[test]
    fn undo_takes_back_the_last_mark() {
        let mut e = editor();
        key(&mut e, Keysym::b, "b", false);
        drag(&mut e, (10.0, 10.0), (50.0, 50.0));
        key(&mut e, Keysym::p, "p", false);
        drag(&mut e, (100.0, 20.0), (150.0, 20.0));
        assert_eq!(e.marks.len(), 2);
        assert_eq!(key(&mut e, Keysym::z, "\u{1a}", true), Step::Redraw);
        assert_eq!(e.marks.len(), 1);
        let mut out = e.result().unwrap();
        assert_eq!(px(&mut out, 125, 20), [255, 255, 255, 255]);
        assert_eq!(px(&mut out, 10, 30), [0, 255, 0, 255]);
        key(&mut e, Keysym::z, "\u{1a}", true);
        let mut out = e.result().unwrap();
        assert_eq!(px(&mut out, 10, 30), [255, 255, 255, 255]);
    }

    #[test]
    fn crop_cuts_the_result_and_undo_gives_the_whole_back() {
        let mut e = editor();
        key(&mut e, Keysym::c, "c", false);
        drag(&mut e, (10.0, 10.0), (60.0, 40.0));
        let out = e.result().unwrap();
        assert_eq!((out.width(), out.height()), (50, 30));
        // the crop is shown filling the view: 50 x 30 fits at 1:1
        let (img, fit) = e.frame();
        assert_eq!((img.w, img.h, fit), (50.0, 30.0, 1.0));
        key(&mut e, Keysym::z, "\u{1a}", true);
        let out = e.result().unwrap();
        assert_eq!((out.width(), out.height()), (200, 100));
    }

    #[test]
    fn a_mark_after_a_crop_lands_where_it_was_drawn() {
        let mut e = editor();
        key(&mut e, Keysym::c, "c", false);
        drag(&mut e, (100.0, 50.0), (200.0, 100.0));
        key(&mut e, Keysym::b, "b", false);
        let (img, _) = e.frame();
        // a box from the crop's corner: at (100, 50) in the picture, (0, 0) in the result
        e.press(img.x + 0.0, img.y + 0.0).unwrap();
        e.release(img.x + 40.0, img.y + 40.0).unwrap();
        assert_eq!(
            e.marks.last(),
            Some(&Mark::Box {
                rect: RectF::new(100.0, 50.0, 40.0, 40.0),
                color: look().accent,
                width: STROKE,
            })
        );
        let mut out = e.result().unwrap();
        assert_eq!(px(&mut out, 0, 20), [0, 255, 0, 255]);
    }

    #[test]
    fn pixelate_tool_hides_what_is_under_it() {
        // black and white columns one pixel wide
        let base = solid(200, 100, (1.0, 1.0, 1.0));
        {
            let cr = cairo::Context::new(&base).unwrap();
            cr.set_source_rgb(0.0, 0.0, 0.0);
            for x in (0..200).step_by(2) {
                cr.rectangle(x as f64, 0.0, 1.0, 100.0);
            }
            cr.fill().unwrap();
        }
        let mut e = Editor::new(base, &look()).unwrap();
        e.set_view(400, 300, 1.0);
        key(&mut e, Keysym::x, "x", false);
        drag(&mut e, (0.0, 0.0), (40.0, 40.0));
        let mut out = e.result().unwrap();
        // blocks of 10: five black and five white columns each, one grey
        let a = px(&mut out, 0, 0);
        let b = px(&mut out, 1, 0);
        assert_eq!(a, b);
        assert!((a[0] as i32 - 128).abs() <= 1, "{a:?}");
        assert_eq!(px(&mut out, 41, 41), [255, 255, 255, 255]);
    }

    #[test]
    fn typing_takes_the_keys_until_enter_then_enter_copies() {
        let mut e = editor();
        key(&mut e, Keysym::t, "t", false);
        e.press(150.0, 120.0).unwrap();
        for (sym, t) in [(Keysym::H, "H"), (Keysym::i, "i"), (Keysym::c, "c")] {
            assert_eq!(key(&mut e, sym, t, false), Step::Redraw);
        }
        // c typed a letter: the tool stays Text
        assert_eq!(e.tool, Tool::Text);
        key(&mut e, Keysym::BackSpace, "\u{8}", false);
        assert_eq!(key(&mut e, Keysym::Return, "\r", false), Step::Redraw);
        match e.marks.as_slice() {
            [Mark::Text { text, .. }] => assert_eq!(text, "Hi"),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            key(&mut e, Keysym::Return, "\r", false),
            Step::Done(Finish::Copy)
        );
    }

    #[test]
    fn escape_while_typing_drops_the_text_and_stays() {
        let mut e = editor();
        key(&mut e, Keysym::t, "t", false);
        e.press(150.0, 120.0).unwrap();
        key(&mut e, Keysym::a, "a", false);
        assert_eq!(key(&mut e, Keysym::Escape, "\u{1b}", false), Step::Redraw);
        assert!(e.marks.is_empty());
        assert_eq!(
            key(&mut e, Keysym::Escape, "\u{1b}", false),
            Step::Done(Finish::Original)
        );
    }

    #[test]
    fn save_keeps_text_still_being_typed() {
        let mut e = editor();
        key(&mut e, Keysym::t, "t", false);
        e.press(150.0, 120.0).unwrap();
        key(&mut e, Keysym::o, "o", false);
        assert_eq!(
            key(&mut e, Keysym::s, "\u{13}", true),
            Step::Done(Finish::Save)
        );
        assert_eq!(e.marks.len(), 1);
    }

    #[test]
    fn a_click_without_travel_is_no_arrow() {
        let mut e = editor();
        e.press(150.0, 120.0).unwrap();
        e.release(150.0, 120.0).unwrap();
        assert!(e.marks.is_empty());
        drag(&mut e, (10.0, 10.0), (60.0, 60.0));
        assert!(matches!(e.marks.as_slice(), [Mark::Arrow { .. }]));
    }

    #[test]
    fn colour_keys_pick_from_the_look() {
        let mut e = editor();
        key(&mut e, Keysym::_2, "2", false);
        key(&mut e, Keysym::b, "b", false);
        drag(&mut e, (10.0, 10.0), (50.0, 50.0));
        match e.marks.as_slice() {
            [Mark::Box { color, .. }] => assert_eq!(*color, Look::default().text),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_large_capture_is_shown_smaller_and_marks_keep_their_on_screen_size() {
        // 1600 x 800 in a 400 x 300 view: shown at fit 368 / 1600 = 0.23
        let mut e = Editor::new(solid(1600, 800, (1.0, 1.0, 1.0)), &look()).unwrap();
        e.set_view(400, 300, 1.0);
        let (img, fit) = e.frame();
        assert!((fit - 0.23).abs() < 1e-9, "{fit}");
        e.press(img.x + 10.0, img.y + 10.0).unwrap();
        e.release(img.x + 100.0, img.y + 10.0).unwrap();
        match e.marks.as_slice() {
            [Mark::Arrow {
                from, to, width, ..
            }] => {
                assert!((from.0 - 10.0 / fit).abs() < 1e-6);
                assert!((to.0 - 100.0 / fit).abs() < 1e-6);
                assert!((width - STROKE / fit).abs() < 1e-6);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn toolbar_controls_answer_a_click() {
        let mut e = editor();
        let dst = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 300).unwrap();
        e.render(&dst, &look()).unwrap();
        let at = |e: &Editor, want: Hit| {
            let (r, _) = e.hits().iter().find(|(_, h)| *h == want).unwrap();
            (r.x + r.w / 2.0, r.y + r.h / 2.0)
        };
        let (x, y) = at(&e, Hit::Tool(Tool::Crop));
        assert_eq!(e.press(x, y).unwrap(), Step::Redraw);
        assert_eq!(e.tool, Tool::Crop);
        let (x, y) = at(&e, Hit::Finish(Finish::Save));
        assert_eq!(e.press(x, y).unwrap(), Step::Done(Finish::Save));
        // 400 px is too narrow for one row: more, and every control inside the view
        assert!(e.bar_rows > 1);
        for (r, h) in e.hits() {
            assert!(r.x >= 0.0 && r.right() <= 400.0, "{h:?} {r:?}");
        }
    }
}
