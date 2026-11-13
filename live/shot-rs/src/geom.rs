use serde::Deserialize;
use std::io::Cursor;
use std::path::Path;
use wayland_client::protocol::wl_output;

/// Minimum pointer travel (in logical pixels) before a press+move counts as a drag rather than a
/// click.
pub const CLICK_DRAG_THRESHOLD: f64 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(self) -> i32 {
        self.y + self.h
    }

    pub fn contains(self, px: i32, py: i32) -> bool {
        self.w > 0
            && self.h > 0
            && px >= self.x
            && px < self.x + self.w
            && py >= self.y
            && py < self.y + self.h
    }

    pub fn clamp_to(self, bounds: Rect) -> Option<Rect> {
        let x0 = self.x.max(bounds.x);
        let y0 = self.y.max(bounds.y);
        let x1 = self.right().min(bounds.right());
        let y1 = self.bottom().min(bounds.bottom());
        if x1 > x0 && y1 > y0 {
            Some(Rect::new(x0, y0, x1 - x0, y1 - y0))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectF {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl RectF {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    pub fn from_points(a: (f64, f64), b: (f64, f64)) -> Self {
        let x0 = a.0.min(b.0);
        let y0 = a.1.min(b.1);
        let x1 = a.0.max(b.0);
        let y1 = a.1.max(b.1);
        Self::new(x0, y0, x1 - x0, y1 - y0)
    }

    pub fn right(self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(self) -> f64 {
        self.y + self.h
    }

    pub fn intersect(self, other: RectF) -> Option<RectF> {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = self.right().min(other.right());
        let y1 = self.bottom().min(other.bottom());
        if x1 > x0 && y1 > y0 {
            Some(RectF::new(x0, y0, x1 - x0, y1 - y0))
        } else {
            None
        }
    }
}

/// Interactive drag state supporting Space-to-move while dragging in any direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DragSelection {
    press: (f64, f64),
    anchor: (f64, f64),
    current: (f64, f64),
    last_ptr: (f64, f64),
    pub space_held: bool,
    pub dragged: bool,
}

impl DragSelection {
    pub fn begin(x: f64, y: f64) -> Self {
        Self {
            press: (x, y),
            anchor: (x, y),
            current: (x, y),
            last_ptr: (x, y),
            space_held: false,
            dragged: false,
        }
    }

    pub fn update(&mut self, x: f64, y: f64, bounds: RectF) {
        if !self.dragged && (x - self.press.0).hypot(y - self.press.1) >= CLICK_DRAG_THRESHOLD {
            self.dragged = true;
        }

        if self.space_held {
            let rect = self.rect();
            let raw_dx = x - self.last_ptr.0;
            let raw_dy = y - self.last_ptr.1;
            let dx = if rect.w <= bounds.w {
                (rect.x + raw_dx).clamp(bounds.x, bounds.right() - rect.w) - rect.x
            } else {
                0.0
            };
            let dy = if rect.h <= bounds.h {
                (rect.y + raw_dy).clamp(bounds.y, bounds.bottom() - rect.h) - rect.y
            } else {
                0.0
            };
            self.anchor.0 += dx;
            self.anchor.1 += dy;
            self.current.0 += dx;
            self.current.1 += dy;
        } else {
            self.current = (
                x.clamp(bounds.x, bounds.right()),
                y.clamp(bounds.y, bounds.bottom()),
            );
        }

        self.last_ptr = (x, y);
    }

    pub fn rect(self) -> RectF {
        RectF::from_points(self.anchor, self.current)
    }
}

/// Output rotation and reflection matching `wl_output::transform` and Smithay `Transform`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputTransform {
    #[default]
    Normal,
    R90,
    R180,
    R270,
    Flipped,
    Flipped90,
    Flipped180,
    Flipped270,
}

impl OutputTransform {
    pub fn from_wl(t: wl_output::Transform) -> Self {
        match t {
            wl_output::Transform::Normal => Self::Normal,
            wl_output::Transform::_90 => Self::R90,
            wl_output::Transform::_180 => Self::R180,
            wl_output::Transform::_270 => Self::R270,
            wl_output::Transform::Flipped => Self::Flipped,
            wl_output::Transform::Flipped90 => Self::Flipped90,
            wl_output::Transform::Flipped180 => Self::Flipped180,
            wl_output::Transform::Flipped270 => Self::Flipped270,
            _ => Self::Normal,
        }
    }

    pub fn from_snapshot_str(s: &str) -> Self {
        match s {
            "normal" => Self::Normal,
            "90" => Self::R90,
            "180" => Self::R180,
            "270" => Self::R270,
            "flipped" => Self::Flipped,
            "flipped-90" => Self::Flipped90,
            "flipped-180" => Self::Flipped180,
            "flipped-270" => Self::Flipped270,
            _ => Self::Normal,
        }
    }

    /// Upright view pixel dimensions `(w, h)` from physical panel dimensions `(phys_w, phys_h)`.
    pub fn view_size(self, phys_w: u32, phys_h: u32) -> (u32, u32) {
        match self {
            Self::R90 | Self::R270 | Self::Flipped90 | Self::Flipped270 => (phys_h, phys_w),
            _ => (phys_w, phys_h),
        }
    }

    /// Maps upright view pixel `(vx, vy)` back to panel buffer pixel `(px, py)`.
    #[inline]
    pub fn view_to_panel(self, vx: u32, vy: u32, phys_w: u32, phys_h: u32) -> (u32, u32) {
        match self {
            Self::Normal => (vx, vy),
            Self::R90 => (vy, phys_h - 1 - vx),
            Self::R180 => (phys_w - 1 - vx, phys_h - 1 - vy),
            Self::R270 => (phys_w - 1 - vy, vx),
            Self::Flipped => (phys_w - 1 - vx, vy),
            Self::Flipped90 => (vy, vx),
            Self::Flipped180 => (vx, phys_h - 1 - vy),
            Self::Flipped270 => (phys_w - 1 - vy, phys_h - 1 - vx),
        }
    }
}

/// Raw captured frame in physical panel orientation.
pub struct PanelFrame<'a> {
    pub data: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub force_opaque: bool,
    pub y_invert: bool,
    pub transform: OutputTransform,
}

/// Converts a raw captured screencopy buffer in panel orientation and cuts `view_crop` (expressed in
/// upright output pixels) into an `ARgb32` Cairo image surface.
pub fn cut_transformed_capture(
    frame: &PanelFrame<'_>,
    view_crop: Rect,
) -> Result<cairo::ImageSurface, String> {
    let phys_w = frame.width;
    let phys_h = frame.height;
    let src_stride = frame.stride;
    if phys_w == 0 || phys_h == 0 {
        return Err("empty capture dimensions".to_string());
    }
    if view_crop.w <= 0 || view_crop.h <= 0 {
        return Err("empty crop rectangle".to_string());
    }
    let (vw, vh) = frame.transform.view_size(phys_w, phys_h);
    if view_crop.x < 0
        || view_crop.y < 0
        || view_crop.right() > vw as i32
        || view_crop.bottom() > vh as i32
    {
        return Err("crop rectangle out of view bounds".to_string());
    }
    let min_stride = (phys_w as usize) * 4;
    if src_stride < min_stride {
        return Err("source stride smaller than row width".to_string());
    }
    let needed = (phys_h as usize - 1) * src_stride + min_stride;
    if frame.data.len() < needed {
        return Err("source buffer smaller than declared frame".to_string());
    }

    let mut dst = cairo::ImageSurface::create(cairo::Format::ARgb32, view_crop.w, view_crop.h)
        .map_err(|e| format!("cairo surface creation failed: {e}"))?;
    let dst_stride = dst.stride() as usize;

    {
        let mut dst_data = dst
            .data()
            .map_err(|e| format!("failed to map cairo surface: {e}"))?;

        for row in 0..view_crop.h as u32 {
            let vy = view_crop.y as u32 + row;
            let dst_row = row as usize * dst_stride;
            for col in 0..view_crop.w as u32 {
                let vx = view_crop.x as u32 + col;
                let (px, py) = frame.transform.view_to_panel(vx, vy, phys_w, phys_h);
                let src_y = if frame.y_invert { phys_h - 1 - py } else { py };
                let src_off = src_y as usize * src_stride + px as usize * 4;
                let dst_off = dst_row + col as usize * 4;

                dst_data[dst_off] = frame.data[src_off];
                dst_data[dst_off + 1] = frame.data[src_off + 1];
                dst_data[dst_off + 2] = frame.data[src_off + 2];
                dst_data[dst_off + 3] = if frame.force_opaque {
                    0xff
                } else {
                    frame.data[src_off + 3]
                };
            }
        }
    }

    Ok(dst)
}

/// Converts the whole captured panel buffer into an upright `ARgb32` Cairo image surface.
pub fn upright_surface_from_panel(frame: &PanelFrame<'_>) -> Result<cairo::ImageSurface, String> {
    let (vw, vh) = frame.transform.view_size(frame.width, frame.height);
    cut_transformed_capture(frame, Rect::new(0, 0, vw as i32, vh as i32))
}

/// Cuts an output-pixel rectangle from an upright `ARgb32` capture without resampling.
pub fn crop_upright_surface(
    src: &cairo::ImageSurface,
    crop: Rect,
) -> Result<cairo::ImageSurface, String> {
    if crop.w <= 0 || crop.h <= 0 {
        return Err("empty crop rectangle".to_string());
    }
    let sw = src.width();
    let sh = src.height();
    if crop.x < 0 || crop.y < 0 || crop.right() > sw || crop.bottom() > sh {
        return Err("crop rectangle out of surface bounds".to_string());
    }

    // Flush any pending Cairo vector drawing before reading raw surface pixels.
    src.flush();
    let src_stride = src.stride() as usize;
    let mut dst = cairo::ImageSurface::create(cairo::Format::ARgb32, crop.w, crop.h)
        .map_err(|e| format!("cairo surface creation failed: {e}"))?;
    let dst_stride = dst.stride() as usize;
    let row_bytes = crop.w as usize * 4;

    {
        let mut dst_data = dst
            .data()
            .map_err(|e| format!("failed to map destination surface: {e}"))?;
        src.with_data(|src_data| {
            for row in 0..crop.h as usize {
                let sy = crop.y as usize + row;
                let src_off = sy * src_stride + crop.x as usize * 4;
                let dst_off = row * dst_stride;
                dst_data[dst_off..dst_off + row_bytes]
                    .copy_from_slice(&src_data[src_off..src_off + row_bytes]);
            }
        })
        .map_err(|e| format!("failed to read source surface: {e}"))?;
    }

    Ok(dst)
}

/// Input descriptor for cutting and joining across one or more upright output captures.
pub struct CapturedOutputSlice<'a> {
    pub logical_rect: RectF,
    pub scale: f64,
    pub surface: &'a cairo::ImageSurface,
}

/// Cuts `sel_logical` from each intersecting output capture and joins them into one PNG-ready
/// surface. When all intersecting outputs share the same scale, slices are copied 1:1 without
/// resampling; mixed scales target the highest scale among the touched outputs.
pub fn join_selection_across_outputs(
    outputs: &[CapturedOutputSlice<'_>],
    sel_logical: RectF,
) -> Result<cairo::ImageSurface, String> {
    if sel_logical.w <= 0.0 || sel_logical.h <= 0.0 {
        return Err("empty selection".to_string());
    }

    let mut touched = Vec::new();
    let mut target_scale = 0.0_f64;
    for out in outputs {
        if let Some(inter) = sel_logical.intersect(out.logical_rect) {
            if out.scale > target_scale {
                target_scale = out.scale;
            }
            touched.push((out, inter));
        }
    }
    if touched.is_empty() || target_scale <= 0.0 {
        return Err("selection does not intersect any output".to_string());
    }

    // Preserve integer output-pixel fidelity when only one output is selected.
    if touched.len() == 1 {
        let (out, inter) = touched[0];
        let surf_w = out.surface.width();
        let surf_h = out.surface.height();
        let scale_x = surf_w as f64 / out.logical_rect.w.max(1e-6);
        let scale_y = surf_h as f64 / out.logical_rect.h.max(1e-6);
        let x0 = ((inter.x - out.logical_rect.x) * scale_x).round() as i32;
        let y0 = ((inter.y - out.logical_rect.y) * scale_y).round() as i32;
        let x1 = ((inter.right() - out.logical_rect.x) * scale_x).round() as i32;
        let y1 = ((inter.bottom() - out.logical_rect.y) * scale_y).round() as i32;
        if let Some(crop) =
            Rect::new(x0, y0, x1 - x0, y1 - y0).clamp_to(Rect::new(0, 0, surf_w, surf_h))
        {
            return crop_upright_surface(out.surface, crop);
        }
        return Err("empty selection after output clamping".to_string());
    }

    let out_w = (sel_logical.w * target_scale).round() as i32;
    let out_h = (sel_logical.h * target_scale).round() as i32;
    if out_w <= 0 || out_h <= 0 {
        return Err("empty selection in output pixels".to_string());
    }

    let dst = cairo::ImageSurface::create(cairo::Format::ARgb32, out_w, out_h)
        .map_err(|e| format!("cairo surface creation failed: {e}"))?;
    let cr = cairo::Context::new(&dst).map_err(|e| format!("cairo context failed: {e}"))?;
    cr.set_operator(cairo::Operator::Source);

    for (out, inter) in touched {
        let surf_w = out.surface.width();
        let surf_h = out.surface.height();
        let scale_x = surf_w as f64 / out.logical_rect.w.max(1e-6);
        let scale_y = surf_h as f64 / out.logical_rect.h.max(1e-6);

        let sx0 = ((inter.x - out.logical_rect.x) * scale_x).round() as i32;
        let sy0 = ((inter.y - out.logical_rect.y) * scale_y).round() as i32;
        let sx1 = ((inter.right() - out.logical_rect.x) * scale_x).round() as i32;
        let sy1 = ((inter.bottom() - out.logical_rect.y) * scale_y).round() as i32;
        let Some(crop) =
            Rect::new(sx0, sy0, sx1 - sx0, sy1 - sy0).clamp_to(Rect::new(0, 0, surf_w, surf_h))
        else {
            continue;
        };

        let tile = crop_upright_surface(out.surface, crop)?;

        let dx0 = ((inter.x - sel_logical.x) * target_scale).round() as i32;
        let dy0 = ((inter.y - sel_logical.y) * target_scale).round() as i32;
        let dx1 = ((inter.right() - sel_logical.x) * target_scale).round() as i32;
        let dy1 = ((inter.bottom() - sel_logical.y) * target_scale).round() as i32;
        let dw = (dx1 - dx0).max(0);
        let dh = (dy1 - dy0).max(0);
        if dw == 0 || dh == 0 {
            continue;
        }

        let _ = cr.save();
        cr.rectangle(dx0 as f64, dy0 as f64, dw as f64, dh as f64);
        cr.clip();
        cr.translate(dx0 as f64, dy0 as f64);
        if crop.w != dw || crop.h != dh {
            cr.scale(dw as f64 / crop.w as f64, dh as f64 / crop.h as f64);
        }
        cr.set_source_surface(&tile, 0.0, 0.0)
            .map_err(|e| format!("cairo source failed: {e}"))?;
        cr.paint().map_err(|e| format!("cairo paint failed: {e}"))?;
        let _ = cr.restore();
    }

    drop(cr);
    dst.flush();
    Ok(dst)
}

pub fn encode_png_bytes(surface: &cairo::ImageSurface) -> Result<Vec<u8>, String> {
    surface.flush();
    let mut buf = Cursor::new(Vec::new());
    surface
        .write_to_png(&mut buf)
        .map_err(|e| format!("png encode failed: {e}"))?;
    Ok(buf.into_inner())
}

pub fn write_png_file(path: &Path, png_bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, png_bytes).map_err(|e| format!("failed to write {}: {e}", path.display()))
}

// --- Snapshot parsing & world-to-output projection ---

#[derive(Debug, Clone, Deserialize)]
pub struct SnapshotCamera {
    pub center: [f64; 2],
    pub zoom: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SnapshotOutput {
    pub name: String,
    pub mode: [u32; 2],
    pub scale: f64,
    #[serde(default)]
    pub transform: String,
    #[serde(default)]
    pub position: [i32; 2],
    pub camera: Option<SnapshotCamera>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SnapshotElement {
    pub state: String,
    pub anchor: Option<[f64; 2]>,
    pub size: Option<[f64; 2]>,
    #[serde(default)]
    pub minimized: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Snapshot {
    #[serde(default)]
    pub outputs: Vec<SnapshotOutput>,
    #[serde(default)]
    pub elements: Vec<SnapshotElement>,
}

#[derive(Debug, Deserialize)]
struct SnapshotEnvelope {
    ok: Option<bool>,
    error: Option<String>,
    snapshot: Option<Snapshot>,
}

/// Parses `infynitae msg snapshot` output (either full IPC reply envelope or bare snapshot).
pub fn parse_snapshot_json(json: &str) -> Result<Snapshot, String> {
    if let Ok(env) = serde_json::from_str::<SnapshotEnvelope>(json) {
        if let Some(snap) = env.snapshot {
            return Ok(snap);
        }
        if env.ok == Some(false) {
            return Err(format!(
                "snapshot failed: {}",
                env.error.unwrap_or_else(|| "unknown error".to_string())
            ));
        }
    }
    serde_json::from_str::<Snapshot>(json).map_err(|e| format!("invalid snapshot JSON: {e}"))
}

/// Projects one world-space window `(anchor, size)` onto an output of upright pixel size
/// `(view_w, view_h)` and `scale` with `camera = (center, zoom)`, clamping to output pixels.
///
/// Formula in logical coordinates: `((anchor - center) * zoom + (W/2, H/2), size * zoom)` with
/// `W = view_w / scale`, `H = view_h / scale`; multiplied by `scale` into output pixels.
pub fn project_window_to_output(
    anchor: [f64; 2],
    size: [f64; 2],
    camera: &SnapshotCamera,
    view_w: u32,
    view_h: u32,
    scale: f64,
) -> Option<Rect> {
    if camera.zoom <= 0.0 || scale <= 0.0 || size[0] <= 0.0 || size[1] <= 0.0 {
        return None;
    }
    if view_w == 0 || view_h == 0 {
        return None;
    }
    let k = camera.zoom * scale;
    let half_w = view_w as f64 * 0.5;
    let half_h = view_h as f64 * 0.5;

    let left = ((anchor[0] - camera.center[0]) * k + half_w).round() as i32;
    let top = ((anchor[1] - camera.center[1]) * k + half_h).round() as i32;
    let right = (((anchor[0] + size[0]) - camera.center[0]) * k + half_w).round() as i32;
    let bottom = (((anchor[1] + size[1]) - camera.center[1]) * k + half_h).round() as i32;

    let raw = Rect::new(left, top, right - left, bottom - top);
    raw.clamp_to(Rect::new(0, 0, view_w as i32, view_h as i32))
}

/// Returns mapped, visible window rectangles in front-to-back z-order in output pixels for
/// `output_name`, given actual runtime view size and scale.
pub fn windows_on_output_with_size(
    snapshot: &Snapshot,
    output_name: &str,
    view_w: u32,
    view_h: u32,
    scale: f64,
) -> Vec<Rect> {
    let Some(out) = snapshot.outputs.iter().find(|o| o.name == output_name) else {
        return Vec::new();
    };
    let Some(cam) = out.camera.as_ref() else {
        return Vec::new();
    };

    let mut rects = Vec::new();
    // `snapshot.elements` is bottom-to-top Stage z-order; reverse gives front-most first.
    for el in snapshot.elements.iter().rev() {
        if el.state != "mapped" || el.minimized {
            continue;
        }
        let (Some(anchor), Some(size)) = (el.anchor, el.size) else {
            continue;
        };
        if let Some(rect) = project_window_to_output(anchor, size, cam, view_w, view_h, scale) {
            rects.push(rect);
        }
    }
    rects
}

/// Convenience wrapper deriving upright view size and scale directly from `snapshot`'s output.
#[cfg(test)]
pub fn windows_on_output(snapshot: &Snapshot, output_name: &str) -> Vec<Rect> {
    let Some(out) = snapshot.outputs.iter().find(|o| o.name == output_name) else {
        return Vec::new();
    };
    let transform = OutputTransform::from_snapshot_str(&out.transform);
    let (vw, vh) = transform.view_size(out.mode[0], out.mode[1]);
    windows_on_output_with_size(snapshot, output_name, vw, vh, out.scale)
}

/// Picks the front-most window rectangle containing output-pixel point `(px, py)`.
pub fn pick_window_at(windows_front_to_back: &[Rect], px: i32, py: i32) -> Option<Rect> {
    windows_front_to_back
        .iter()
        .copied()
        .find(|r| r.contains(px, py))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_rect_from_snapshot_panned_zoomed_and_clamped() {
        let json = r#"{
            "id": 1,
            "ok": true,
            "snapshot": {
                "outputs": [{
                    "name": "DP-1",
                    "mode": [1920, 1080],
                    "scale": 1.0,
                    "transform": "normal",
                    "position": [0, 0],
                    "camera": { "center": [500.0, 200.0], "zoom": 0.5 }
                }],
                "elements": [
                    { "state": "mapped", "anchor": [-2000.0, -400.0], "size": [1000.0, 800.0], "minimized": false },
                    { "state": "mapped", "anchor": [500.0, 200.0], "size": [400.0, 200.0], "minimized": true },
                    { "state": "unmapped", "anchor": [500.0, 200.0], "size": [400.0, 200.0], "minimized": false },
                    { "state": "mapped", "anchor": [300.0, 0.0], "size": [600.0, 400.0], "minimized": false }
                ]
            }
        }"#;
        let snap = parse_snapshot_json(json).unwrap();
        let wins = windows_on_output(&snap, "DP-1");
        assert_eq!(wins.len(), 2);
        // Front-most (last mapped & non-minimized):
        // left = (300 - 500) * 0.5 + 960 = 860, top = (0 - 200) * 0.5 + 540 = 440,
        // w = 600 * 0.5 = 300, h = 400 * 0.5 = 200.
        assert_eq!(wins[0], Rect::new(860, 440, 300, 200));
        // Back-most clamped on left edge:
        // raw left = (-2000 - 500)*0.5 + 960 = -290, right = -290 + 500 = 210 => clamped x=0, w=210;
        // raw top = (-400 - 200)*0.5 + 540 = 240, h = 400.
        assert_eq!(wins[1], Rect::new(0, 240, 210, 400));

        // Hit test picks front-most.
        assert_eq!(pick_window_at(&wins, 900, 500), Some(wins[0]));
        assert_eq!(pick_window_at(&wins, 100, 300), Some(wins[1]));
        assert_eq!(pick_window_at(&wins, 1800, 900), None);
    }

    #[test]
    fn window_rect_scales_into_output_pixels_on_hidpi_and_rotated_output() {
        // 3840x2160 panel rotated 90 degrees => 2160x3840 view; scale 2.0 => 1080x1920 logical.
        let json = r#"{
            "outputs": [{
                "name": "eDP-1",
                "mode": [3840, 2160],
                "scale": 2.0,
                "transform": "90",
                "position": [0, 0],
                "camera": { "center": [100.0, 200.0], "zoom": 1.0 }
            }],
            "elements": [
                { "state": "mapped", "anchor": [100.0, 200.0], "size": [300.0, 400.0], "minimized": false }
            ]
        }"#;
        let snap = parse_snapshot_json(json).unwrap();
        let wins = windows_on_output(&snap, "eDP-1");
        // Centered on 2160x3840 => half = (1080, 1920), size * 2 = (600, 800).
        assert_eq!(wins, vec![Rect::new(1080, 1920, 600, 800)]);
    }

    #[test]
    fn selection_rect_from_drag_in_all_four_directions() {
        let expected = RectF::new(100.0, 200.0, 50.0, 80.0);
        // Down-right
        assert_eq!(RectF::from_points((100.0, 200.0), (150.0, 280.0)), expected);
        // Up-left
        assert_eq!(RectF::from_points((150.0, 280.0), (100.0, 200.0)), expected);
        // Down-left
        assert_eq!(RectF::from_points((150.0, 200.0), (100.0, 280.0)), expected);
        // Up-right
        assert_eq!(RectF::from_points((100.0, 280.0), (150.0, 200.0)), expected);
    }

    #[test]
    fn space_move_translates_selection_keeping_size_and_clamping() {
        let bounds = RectF::new(0.0, 0.0, 400.0, 300.0);
        let mut drag = DragSelection::begin(100.0, 100.0);
        drag.update(200.0, 150.0, bounds);
        assert!(drag.dragged);
        assert_eq!(drag.rect(), RectF::new(100.0, 100.0, 100.0, 50.0));

        drag.space_held = true;
        drag.update(240.0, 130.0, bounds);
        assert_eq!(drag.rect(), RectF::new(140.0, 80.0, 100.0, 50.0));

        // Clamp against right/bottom wall without shrinking size.
        drag.update(600.0, 600.0, bounds);
        assert_eq!(drag.rect(), RectF::new(300.0, 250.0, 100.0, 50.0));

        // Release space and continue resizing from the moved opposite corner (300, 250).
        drag.space_held = false;
        drag.update(360.0, 280.0, bounds);
        assert_eq!(drag.rect(), RectF::new(300.0, 250.0, 60.0, 30.0));
    }

    #[test]
    fn transform_view_to_panel_bijective_for_all_variants() {
        let (pw, ph) = (7, 5);
        for t in [
            OutputTransform::Normal,
            OutputTransform::R90,
            OutputTransform::R180,
            OutputTransform::R270,
            OutputTransform::Flipped,
            OutputTransform::Flipped90,
            OutputTransform::Flipped180,
            OutputTransform::Flipped270,
        ] {
            let (vw, vh) = t.view_size(pw, ph);
            let mut seen = vec![false; (pw * ph) as usize];
            for vy in 0..vh {
                for vx in 0..vw {
                    let (px, py) = t.view_to_panel(vx, vy, pw, ph);
                    assert!(px < pw && py < ph, "{t:?}: ({px},{py}) out of bounds");
                    let idx = (py * pw + px) as usize;
                    assert!(!seen[idx], "{t:?}: collision at ({px},{py})");
                    seen[idx] = true;
                }
            }
            assert!(seen.into_iter().all(|v| v), "{t:?}: missed pixels");
        }
    }

    #[test]
    fn cut_rect_scale_2_and_90_degree_transform_synthetic_cairo() {
        // Panel is 12x8 pixels (so rotated 90 degrees -> upright view is 8x12).
        // Simulate scale=2 output of logical 4x6 (8x12 output pixels).
        let phys_w = 12_u32;
        let phys_h = 8_u32;
        let src_surf =
            cairo::ImageSurface::create(cairo::Format::ARgb32, phys_w as i32, phys_h as i32)
                .unwrap();
        {
            let cr = cairo::Context::new(&src_surf).unwrap();
            // Fill background dark gray.
            cr.set_source_rgb(0.2, 0.2, 0.2);
            cr.paint().unwrap();
            // Paint distinct colored blocks at known panel pixels:
            // panel x in [2..6), y in [4..8) => red (panel bottom-mid).
            cr.set_source_rgb(1.0, 0.0, 0.0);
            cr.rectangle(2.0, 4.0, 4.0, 4.0);
            cr.fill().unwrap();
            // panel x in [8..10), y in [0..3) => blue (panel top-right).
            cr.set_source_rgb(0.0, 0.0, 1.0);
            cr.rectangle(8.0, 0.0, 2.0, 3.0);
            cr.fill().unwrap();
        }
        src_surf.flush();
        let stride = src_surf.stride() as usize;
        let mut bytes = Vec::new();
        src_surf.with_data(|d| bytes.extend_from_slice(d)).unwrap();

        // Upright view with 90 deg rotation has view_size = (8, 12).
        // Under view_to_panel for R90: (px, py) = (vy, 7 - vx).
        // So panel [2..6) x [4..8) appears at view vy in [2..6), vx in [0..4) (because 7 - vx in [4..8) => vx in 0..=3).
        // And under scale=2, logical rect (0, 1, 2, 2) maps to output-pixel rect (0, 2, 4, 4).
        let scale = 2.0;
        let logical_rect = RectF::new(0.0, 1.0, 2.0, 2.0);
        let view_crop = Rect::new(
            (logical_rect.x * scale) as i32,
            (logical_rect.y * scale) as i32,
            (logical_rect.w * scale) as i32,
            (logical_rect.h * scale) as i32,
        );
        assert_eq!(view_crop, Rect::new(0, 2, 4, 4));

        let mut cut = cut_transformed_capture(
            &PanelFrame {
                data: &bytes,
                width: phys_w,
                height: phys_h,
                stride,
                force_opaque: false,
                y_invert: false,
                transform: OutputTransform::R90,
            },
            view_crop,
        )
        .unwrap();
        assert_eq!((cut.width(), cut.height()), (4, 4));

        let cut_stride = cut.stride() as usize;
        let cut_data = cut.data().unwrap();
        for row in 0..4 {
            for col in 0..4 {
                let off = row * cut_stride + col * 4;
                // Cairo ARgb32 little-endian = [B, G, R, A]
                assert_eq!(
                    &cut_data[off..off + 4],
                    &[0, 0, 255, 255],
                    "pixel ({col},{row}) expected red"
                );
            }
        }
    }

    #[test]
    fn join_selection_across_two_outputs_synthetic_cairo() {
        // Left output: logical (0, 0, 10, 10) at scale 2 -> 20x20 pixels, green (#00ff00).
        // Right output: logical (10, 0, 10, 10) at scale 2 -> 20x20 pixels, blue (#0000ff).
        let left = cairo::ImageSurface::create(cairo::Format::ARgb32, 20, 20).unwrap();
        {
            let cr = cairo::Context::new(&left).unwrap();
            cr.set_source_rgb(0.0, 1.0, 0.0);
            cr.paint().unwrap();
        }
        let right = cairo::ImageSurface::create(cairo::Format::ARgb32, 20, 20).unwrap();
        {
            let cr = cairo::Context::new(&right).unwrap();
            cr.set_source_rgb(0.0, 0.0, 1.0);
            cr.paint().unwrap();
        }

        let slices = [
            CapturedOutputSlice {
                logical_rect: RectF::new(0.0, 0.0, 10.0, 10.0),
                scale: 2.0,
                surface: &left,
            },
            CapturedOutputSlice {
                logical_rect: RectF::new(10.0, 0.0, 10.0, 10.0),
                scale: 2.0,
                surface: &right,
            },
        ];

        // Select across seam: logical x in [8..13), y in [2..5) => 5x3 logical = 10x6 output pixels.
        let mut joined =
            join_selection_across_outputs(&slices, RectF::new(8.0, 2.0, 5.0, 3.0)).unwrap();
        assert_eq!((joined.width(), joined.height()), (10, 6));

        let stride = joined.stride() as usize;
        {
            let data = joined.data().unwrap();
            for row in 0..6 {
                for col in 0..10 {
                    let off = row * stride + col * 4;
                    let expected = if col < 4 {
                        [0, 255, 0, 255] // green from left output (2 logical px * 2 = 4 px)
                    } else {
                        [255, 0, 0, 255] // blue from right output (3 logical px * 2 = 6 px)
                    };
                    assert_eq!(
                        &data[off..off + 4],
                        &expected,
                        "joined pixel ({col},{row}) mismatch"
                    );
                }
            }
        }

        // Verify valid PNG encoding round-trips through Cairo's PNG reader cleanly.
        let png = encode_png_bytes(&joined).unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        let decoded = cairo::ImageSurface::create_from_png(&mut Cursor::new(&png)).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (10, 6));
    }
}
