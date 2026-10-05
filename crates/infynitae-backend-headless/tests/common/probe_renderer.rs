//! Recording renderer for the EE-1 transform matrix (test fixture), ported from the E1R harness
//! (experiments/b8-harness/src/bin/xtransform.rs, evidence EE-1): a CPU rasteriser of the true
//! panel size with the GLES2 renderer's viewport/scissor semantics (smithay-0.7.0
//! src/backend/renderer/gles/mod.rs:2044-2090). `render(output_size, dst_transform)` records the
//! viewport; element coordinates live in `transform_size(output_size)` space and are mapped into
//! the viewport by `dst_transform`. Handedness is not claimed. Not production code.

#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::sync::SyncPoint;
use smithay::backend::renderer::{
    Color32F, ContextId, DebugFlags, Frame, Renderer, RendererSuper, Texture, TextureFilter,
};
use smithay::utils::{Buffer, Physical, Rectangle, Size, Transform};

pub const CLEAR: u32 = 1;

#[derive(Debug)]
pub struct ProbeError;
impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "probe error")
    }
}
impl std::error::Error for ProbeError {}

#[derive(Debug, Clone)]
pub struct ProbeTex;
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
pub struct Fb {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u32>,
    pub lost: u64,
    pub render_calls: Vec<(Size<i32, Physical>, Transform)>,
    pub cur: Option<(Size<i32, Physical>, Transform)>,
    pub first_hit: Option<usize>,
}
impl Fb {
    pub fn new(w: i32, h: i32) -> Self {
        Fb {
            w,
            h,
            px: vec![0; (w * h) as usize],
            lost: 0,
            render_calls: vec![],
            cur: None,
            first_hit: None,
        }
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
pub struct ProbeRenderer {
    pub fb: Rc<RefCell<Fb>>,
}
#[derive(Debug)]
pub struct ProbeFrame {
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
        Ok(ProbeFrame {
            fb: self.fb.clone(),
            t: dst_transform,
        })
    }
    fn wait(&mut self, _: &SyncPoint) -> Result<(), ProbeError> {
        Ok(())
    }
}
pub fn cid(c: Color32F) -> u32 {
    c.r() as u32
}
impl Frame for ProbeFrame {
    type Error = ProbeError;
    type TextureId = ProbeTex;
    fn context_id(&self) -> ContextId<ProbeTex> {
        CTX.clone()
    }
    fn clear(
        &mut self,
        color: Color32F,
        at: &[Rectangle<i32, Physical>],
    ) -> Result<(), ProbeError> {
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
