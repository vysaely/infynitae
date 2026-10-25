//! E1R / E-F2-01 + E-F3-02: rotated-output dimension handling, headless.
//!
//! Drives Smithay 0.7.0's real `OutputDamageTracker::render_output` with real
//! `SolidColorRenderElement`s into a recording renderer (`ProbeRenderer`) that rasterises into a
//! CPU framebuffer of the true panel size with the GLES2 renderer's viewport/scissor semantics
//! (smithay-0.7.0 src/backend/renderer/gles/mod.rs:2044-2090):
//!   - `glViewport(0,0,output_size)` + scissor: the `output_size` passed to `Renderer::render` is the
//!     physical framebuffer region ("dimensions of the display **before** the dst_transform",
//!     renderer/mod.rs:405-407);
//!   - element (draw) coordinates live in `transform_size(output_size)` space, mapped into the
//!     viewport by `dst_transform`.
//! The element→viewport mapping uses `Transform::transform_rect_in`. Handedness (which of 90/270 is
//! clockwise on glass) is NOT claimed: every check below is invariant under replacing the mapping
//! by its inverse. No GPU, no DRM, no T4 claim.
//!
//! Rules compared (all other formulas identical):
//!   tracker size  SEALED   = mode after transform (PD-F1-04 fed into F-RENDER-1)
//!                 E1R      = physical (untransformed) mode
//!   hit pixel     SEALED   = F-HIT-1 `u = transform_O(p·scale_O)` (both readings: t and t⁻¹)
//!                 E1R      = `u = p·scale_O` (view/element space, the space of F-LATTICE-1 rects)
//! Output: docs/archive/evidence/E1R/xtransform-output.txt
use std::{cell::RefCell, rc::Rc};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            damage::OutputDamageTracker,
            element::{solid::SolidColorRenderElement, Id, Kind},
            sync::SyncPoint,
            Color32F, ContextId, DebugFlags, Frame, Renderer, RendererSuper, Texture, TextureFilter,
        },
    },
    utils::{Buffer, Physical, Point, Rectangle, Scale, Size, Transform},
};

#[derive(Debug)]
struct ProbeError;
impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "probe error")
    }
}
impl std::error::Error for ProbeError {}

#[derive(Debug, Clone)]
struct ProbeTex;
impl Texture for ProbeTex {
    fn width(&self) -> u32 {
        0
    }
    fn height(&self) -> u32 {
        0
    }
    fn format(&self) -> Option<Fourcc> {
        None
    }
}

/// CPU framebuffer of the physical panel; ids are carried in the colour's red channel.
#[derive(Debug)]
struct Fb {
    w: i32,
    h: i32,
    px: Vec<u32>,
    lost: u64,
    render_calls: Vec<(Size<i32, Physical>, Transform)>,
    cur: Option<(Size<i32, Physical>, Transform)>,
    first_hit: Option<usize>,
}
impl Fb {
    fn new(w: i32, h: i32) -> Self {
        Fb { w, h, px: vec![0; (w * h) as usize], lost: 0, render_calls: vec![], cur: None, first_hit: None }
    }
    /// Paint an element-space rect: map into viewport space, scissor to the viewport, then the
    /// panel. Fragments inside the viewport but outside the panel are lost.
    fn paint(&mut self, r: Rectangle<i32, Physical>, id: u32) {
        let (out, t) = self.cur.expect("paint outside a frame");
        let elem = t.transform_size(out);
        let m = t.transform_rect_in(r, &elem);
        for y in m.loc.y.max(0)..(m.loc.y + m.size.h).min(out.h) {
            for x in m.loc.x.max(0)..(m.loc.x + m.size.w).min(out.w) {
                if x < self.w && y < self.h {
                    let idx = (y * self.w + x) as usize;
                    self.px[idx] = id;
                    if self.first_hit.is_none() && id != CLEAR {
                        self.first_hit = Some(idx);
                    }
                } else {
                    self.lost += 1;
                }
            }
        }
    }
}

static CTX: std::sync::LazyLock<ContextId<ProbeTex>> = std::sync::LazyLock::new(ContextId::new);

#[derive(Debug)]
struct ProbeRenderer {
    fb: Rc<RefCell<Fb>>,
}
#[derive(Debug)]
struct ProbeFrame {
    fb: Rc<RefCell<Fb>>,
    t: Transform,
}

impl RendererSuper for ProbeRenderer {
    type Error = ProbeError;
    type TextureId = ProbeTex;
    type Framebuffer<'buffer> = ProbeTex;
    type Frame<'frame, 'buffer>
        = ProbeFrame
    where
        'buffer: 'frame,
        Self: 'frame;
}
impl Renderer for ProbeRenderer {
    fn context_id(&self) -> ContextId<ProbeTex> {
        CTX.clone()
    }
    fn downscale_filter(&mut self, _: TextureFilter) -> Result<(), ProbeError> {
        Ok(())
    }
    fn upscale_filter(&mut self, _: TextureFilter) -> Result<(), ProbeError> {
        Ok(())
    }
    fn set_debug_flags(&mut self, _: DebugFlags) {}
    fn debug_flags(&self) -> DebugFlags {
        DebugFlags::empty()
    }
    fn render<'frame, 'buffer>(
        &'frame mut self,
        _target: &'frame mut ProbeTex,
        output_size: Size<i32, Physical>,
        dst_transform: Transform,
    ) -> Result<ProbeFrame, ProbeError>
    where
        'buffer: 'frame,
    {
        let mut fb = self.fb.borrow_mut();
        fb.render_calls.push((output_size, dst_transform));
        fb.cur = Some((output_size, dst_transform));
        Ok(ProbeFrame { fb: self.fb.clone(), t: dst_transform })
    }
    fn wait(&mut self, _: &SyncPoint) -> Result<(), ProbeError> {
        Ok(())
    }
}
fn cid(c: Color32F) -> u32 {
    c.r() as u32
}
impl Frame for ProbeFrame {
    type Error = ProbeError;
    type TextureId = ProbeTex;
    fn context_id(&self) -> ContextId<ProbeTex> {
        CTX.clone()
    }
    fn clear(&mut self, color: Color32F, at: &[Rectangle<i32, Physical>]) -> Result<(), ProbeError> {
        let mut fb = self.fb.borrow_mut();
        for r in at {
            fb.paint(*r, cid(color));
        }
        Ok(())
    }
    fn draw_solid(
        &mut self,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        color: Color32F,
    ) -> Result<(), ProbeError> {
        let mut fb = self.fb.borrow_mut();
        for d in damage {
            let r = Rectangle::new(dst.loc + d.loc, d.size);
            if let Some(r) = r.intersection(dst) {
                fb.paint(r, cid(color));
            }
        }
        Ok(())
    }
    fn render_texture_from_to(
        &mut self,
        _: &ProbeTex,
        _: Rectangle<f64, Buffer>,
        _: Rectangle<i32, Physical>,
        _: &[Rectangle<i32, Physical>],
        _: &[Rectangle<i32, Physical>],
        _: Transform,
        _: f32,
    ) -> Result<(), ProbeError> {
        unreachable!("solid elements only")
    }
    fn transformation(&self) -> Transform {
        self.t
    }
    fn wait(&mut self, _: &SyncPoint) -> Result<(), ProbeError> {
        Ok(())
    }
    fn finish(self) -> Result<SyncPoint, ProbeError> {
        self.fb.borrow_mut().cur = None;
        Ok(SyncPoint::default())
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Rule {
    Sealed,
    E1r,
}

const CLEAR: u32 = 1;

fn solid(r: Rectangle<i32, Physical>, id: u32) -> SolidColorRenderElement {
    SolidColorRenderElement::new(Id::new(), r, 0usize, Color32F::new(id as f32, 0.0, 0.0, 1.0), Kind::Unspecified)
}

/// One full frame (age 0 ⇒ full damage). Returns framebuffer, damage union, render() size.
fn frame(
    phys: Size<i32, Physical>,
    t: Transform,
    k: f64,
    rule: Rule,
    elems: &[SolidColorRenderElement],
) -> (Fb, Vec<Rectangle<i32, Physical>>, Size<i32, Physical>) {
    let tracker_size = match rule {
        Rule::E1r => phys,
        Rule::Sealed => t.transform_size(phys),
    };
    let fbcell = Rc::new(RefCell::new(Fb::new(phys.w, phys.h)));
    let mut r = ProbeRenderer { fb: fbcell.clone() };
    let mut tracker = OutputDamageTracker::new(tracker_size, Scale::from(k), t);
    let mut target = ProbeTex;
    let res = tracker
        .render_output(&mut r, &mut target, 0, elems, Color32F::new(CLEAR as f32, 0.0, 0.0, 1.0))
        .expect("render_output");
    let damage = res.damage.cloned().unwrap_or_default();
    drop(r);
    let fb = Rc::try_unwrap(fbcell).ok().unwrap().into_inner();
    let vs = fb.render_calls[0].0;
    (fb, damage, vs)
}

/// The view (element-space) extent the architecture derives for O: F-VIEW-1's `mode_O` (after transform).
fn view_phys(phys: Size<i32, Physical>, t: Transform) -> Size<i32, Physical> {
    t.transform_size(phys)
}

struct Checks {
    rows: Vec<String>,
    fails: usize,
}
impl Checks {
    fn check(&mut self, name: &str, ok: bool, detail: String) {
        if !ok {
            self.fails += 1;
        }
        self.rows.push(format!("    [{}] {name}: {detail}", if ok { "ok" } else { "FAIL" }));
    }
}

/// Render checks C1..C4 for one (case, tracker rule).
fn render_checks(phys: Size<i32, Physical>, t: Transform, k: f64, rule: Rule) -> Checks {
    let mut c = Checks { rows: vec![], fails: 0 };
    let v = view_phys(phys, t);
    // front-to-back: 4 non-overlapping edge markers (ids 10..13) over a full-view cover (id 2)
    let elems = vec![
        solid(Rectangle::new((0, 0).into(), (v.w, 1).into()), 10),
        solid(Rectangle::new((0, v.h - 1).into(), (v.w, 1).into()), 11),
        solid(Rectangle::new((0, 1).into(), (1, v.h - 2).into()), 12),
        solid(Rectangle::new((v.w - 1, 1).into(), (1, v.h - 2).into()), 13),
        solid(Rectangle::new((0, 0).into(), v), 2),
    ];
    let (fb, damage, vs) = frame(phys, t, k, rule, &elems);
    c.check(
        "C1 viewport == panel",
        vs == phys,
        format!("render(output_size={}x{}) on panel {}x{}", vs.w, vs.h, phys.w, phys.h),
    );
    let blank = fb.px.iter().filter(|&&p| p == 0 || p == CLEAR).count();
    c.check(
        "C2 view covers panel exactly (no gap, no lost fragment)",
        blank == 0 && fb.lost == 0,
        format!("uncovered panel px={blank}, fragments outside panel={}", fb.lost),
    );
    // each marker must be one full panel edge line (minus corners for the side markers)
    let mut edges_hit = vec![];
    let mut marker_ok = true;
    let mut mdetail = vec![];
    for (id, len) in [(10, v.w), (11, v.w), (12, v.h - 2), (13, v.h - 2)] {
        let pts: Vec<(i32, i32)> = (0..fb.h)
            .flat_map(|y| (0..fb.w).map(move |x| (x, y)))
            .filter(|&(x, y)| fb.px[(y * fb.w + x) as usize] == id)
            .collect();
        let edge = if !pts.is_empty() && pts.iter().all(|p| p.1 == 0) {
            Some("top")
        } else if !pts.is_empty() && pts.iter().all(|p| p.1 == fb.h - 1) {
            Some("bottom")
        } else if !pts.is_empty() && pts.iter().all(|p| p.0 == 0) {
            Some("left")
        } else if !pts.is_empty() && pts.iter().all(|p| p.0 == fb.w - 1) {
            Some("right")
        } else {
            None
        };
        let ok = edge.is_some() && pts.len() as i32 == len;
        marker_ok &= ok;
        if let Some(e) = edge {
            edges_hit.push(e);
        }
        mdetail.push(format!("m{id}:{}px@{}", pts.len(), edge.unwrap_or("none")));
    }
    edges_hit.sort();
    edges_hit.dedup();
    c.check(
        "C3 view edges land on the 4 panel edges, full length",
        marker_ok && edges_hit.len() == 4,
        mdetail.join(" "),
    );
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    let mut area = 0i64;
    for d in &damage {
        x0 = x0.min(d.loc.x);
        y0 = y0.min(d.loc.y);
        x1 = x1.max(d.loc.x + d.size.w);
        y1 = y1.max(d.loc.y + d.size.h);
        area += d.size.w as i64 * d.size.h as i64;
    }
    c.check(
        "C4 full-frame damage == view extent",
        (x0, y0, x1, y1) == (0, 0, v.w, v.h) && area == v.w as i64 * v.h as i64,
        format!("damage bounds [{x0},{y0})..[{x1},{y1}) area {area}; view {}x{}", v.w, v.h),
    );
    c
}

/// C5: render/hit agreement. Tiles (distinct ids) cover the view; the presented tile under the
/// pointer is read from the panel pixel where a 1×1 cursor element at the pointer lands (both go
/// through the real tracker). The hit tile is selected by the hit rule over the same tile rects.
struct HitPrep {
    v: Size<i32, Physical>,
    tw: i32,
    th: i32,
    pts: Vec<(f64, f64)>,
    presented: Vec<Option<u32>>,
}

fn prepare_hit_checks(phys: Size<i32, Physical>, t: Transform, k: f64) -> HitPrep {
    let v = view_phys(phys, t);
    let (tw, th) = (7, 5);
    let mut tiles = vec![];
    let tid = |ix: i32, iy: i32| 100 + (iy * 1000 + ix) as u32;
    let mut iy = 0;
    while iy * th < v.h {
        let mut ix = 0;
        while ix * tw < v.w {
            let r = Rectangle::new((ix * tw, iy * th).into(), (tw.min(v.w - ix * tw), th.min(v.h - iy * th)).into());
            tiles.push(solid(r, tid(ix, iy)));
            ix += 1;
        }
        iy += 1;
    }
    let (scene, _, _) = frame(phys, t, k, Rule::E1r, &tiles);
    // OutputLogical extent (what clients/pointer see) = view / scale
    let (lw, lh) = (v.w as f64 / k, v.h as f64 / k);
    let mut pts: Vec<(f64, f64)> = vec![
        (0.0, 0.0),
        (lw - 1e-6, 0.0),
        (0.0, lh - 1e-6),
        (lw - 1e-6, lh - 1e-6),
        (lw / 2.0, lh / 2.0),
    ];
    let mut s: u64 = 0x9E3779B97F4A7C15;
    for _ in 0..195 {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let a = (s >> 11) as f64 / (1u64 << 53) as f64;
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let b = (s >> 11) as f64 / (1u64 << 53) as f64;
        pts.push((a * lw, b * lh));
    }
    let cur_fb = Rc::new(RefCell::new(Fb::new(phys.w, phys.h)));
    let mut presented = Vec::with_capacity(pts.len());
    for &(px, py) in &pts {
        let e = Point::<f64, Physical>::from((px * k, py * k));
        let cur = Rectangle::new((e.x.floor() as i32, e.y.floor() as i32).into(), (1, 1).into());
        {
            let mut fb = cur_fb.borrow_mut();
            fb.first_hit = None;
            fb.lost = 0;
            fb.render_calls.clear();
            fb.cur = None;
        }
        let mut r = ProbeRenderer { fb: cur_fb.clone() };
        let mut tracker = OutputDamageTracker::new(phys, Scale::from(k), t);
        let mut target = ProbeTex;
        let _ = tracker
            .render_output(&mut r, &mut target, 0, &[solid(cur, 7)], Color32F::new(CLEAR as f32, 0.0, 0.0, 1.0))
            .expect("render_output");
        drop(r);
        let mut fb = cur_fb.borrow_mut();
        let pos = fb.first_hit;
        if let Some(idx) = pos {
            fb.px[idx] = 0;
        }
        presented.push(pos.map(|i| scene.px[i]));
    }
    HitPrep { v, tw, th, pts, presented }
}

fn eval_hit_check(prep: &HitPrep, t: Transform, k: f64, hit: &str) -> (usize, usize, Vec<String>) {
    let tid = |ix: i32, iy: i32| 100 + (iy * 1000 + ix) as u32;
    let mut mism = 0;
    let mut ex = vec![];
    for (i, &(px, py)) in prep.pts.iter().enumerate() {
        let e = Point::<f64, Physical>::from((px * k, py * k));
        let presented = prep.presented[i];
        let u = match hit {
            "E1R p*s" => e,
            "SEALED t(p*s)" => t.transform_point_in(e, &prep.v.to_f64()),
            "SEALED t^-1(p*s)" => t.invert().transform_point_in(e, &prep.v.to_f64()),
            _ => unreachable!(),
        };
        let (ux, uy) = (u.x.floor() as i32, u.y.floor() as i32);
        let hit_tile = if ux >= 0 && uy >= 0 && ux < prep.v.w && uy < prep.v.h {
            Some(tid(ux / prep.tw, uy / prep.th))
        } else {
            None
        };
        if presented.is_none() || presented != hit_tile {
            mism += 1;
            if ex.len() < 2 {
                ex.push(format!("p=({px:.3},{py:.3}) presented={presented:?} hit={hit_tile:?}"));
            }
        }
    }
    (mism, prep.pts.len(), ex)
}

fn main() {
    println!("xtransform — E1R E-F2-01/E-F3-02: rotated-output dimensions through smithay =0.7.0 OutputDamageTracker");
    println!("renderer: ProbeRenderer (CPU raster, GLES2 viewport/scissor semantics); HEADLESS; no T4 claim\n");
    let phys: Size<i32, Physical> = (1920, 1080).into();
    let cases: Vec<(Transform, f64)> = vec![
        (Transform::Normal, 1.0),
        (Transform::_90, 1.0),
        (Transform::_180, 1.0),
        (Transform::_270, 1.0),
        (Transform::_90, 1.25),
        (Transform::_270, 1.25),
        (Transform::_90, 1.5),
        (Transform::_270, 1.5),
        (Transform::Flipped90, 1.25),
        (Transform::Flipped, 1.0),
    ];
    let mut e1r_fail = 0;
    let mut sealed_fail_rot = 0;
    let mut sealed_rot_cases = 0;
    let mut sealed_fail_nonrot = 0;
    let mut hit_sealed_fail_nonnormal = 0;
    let mut hit_nonnormal_cases = 0;
    for &(t, k) in &cases {
        let v = view_phys(phys, t);
        println!(
            "case panel {}x{} transform {:?} scale {k}: view(after transform) {}x{} phys; OutputLogical {:.3}x{:.3}",
            phys.w, phys.h, t, v.w, v.h, v.w as f64 / k, v.h as f64 / k
        );
        let swaps = t.transform_size(phys) != phys;
        for rule in [Rule::E1r, Rule::Sealed] {
            let c = render_checks(phys, t, k, rule);
            println!("  tracker rule {:?}: {} failing check(s)", rule, c.fails);
            for r in &c.rows {
                println!("{r}");
            }
            match rule {
                Rule::E1r => e1r_fail += c.fails,
                Rule::Sealed if swaps => {
                    sealed_rot_cases += 1;
                    if c.fails > 0 {
                        sealed_fail_rot += 1
                    }
                }
                Rule::Sealed => sealed_fail_nonrot += c.fails,
            }
        }
        let prep = prepare_hit_checks(phys, t, k);
        for hit in ["E1R p*s", "SEALED t(p*s)", "SEALED t^-1(p*s)"] {
            let (m, n, ex) = eval_hit_check(&prep, t, k, hit);
            println!("  C5 render/hit agreement, hit rule {hit}: {m}/{n} mismatches {}", ex.join("; "));
            if hit == "E1R p*s" {
                e1r_fail += m;
            } else if t != Transform::Normal {
                hit_nonnormal_cases += 1;
                if m > 0 {
                    hit_sealed_fail_nonnormal += 1;
                }
            }
        }
        println!();
    }
    println!("SUMMARY");
    println!("  E1R rules (tracker=physical mode, hit u=p*s): {e1r_fail} failing checks/mismatches over {} cases", cases.len());
    println!("  SEALED tracker rule on 90/270-class transforms: failing in {sealed_fail_rot}/{sealed_rot_cases} cases (negative control)");
    println!("  SEALED tracker rule on Normal/180/Flipped: {sealed_fail_nonrot} failing checks (expected 0: no swap)");
    println!("  SEALED hit rule (both readings) on non-Normal transforms: failing in {hit_sealed_fail_nonnormal}/{hit_nonnormal_cases}");
    let pass = e1r_fail == 0
        && sealed_fail_rot == sealed_rot_cases
        && sealed_rot_cases > 0
        && sealed_fail_nonrot == 0
        && hit_sealed_fail_nonnormal == hit_nonnormal_cases;
    println!("RESULT: {}", if pass { "PASS (corrected passes; sealed negative controls fail as predicted)" } else { "UNEXPECTED" });
    std::process::exit(if pass { 0 } else { 1 });
}
