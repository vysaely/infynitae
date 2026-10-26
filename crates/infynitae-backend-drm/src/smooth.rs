//! Windows drawn smaller than their buffer (zoomed out) are averaged over the area each screen
//! pixel covers, instead of sampled at one point: text and thin lines stay legible and do not
//! shimmer while the camera moves. GLES's plain linear filter reads 4 texels per pixel, so
//! below half size it drops most of the window; Smithay builds no mipmaps.
//!
//! The shader takes 4×4 samples across the pixel's footprint in the texture (its derivatives),
//! each bilinear. It runs only for a draw whose source is larger than its destination: at 100%,
//! zoomed in, and for a fullscreen game (direct scanout) nothing changes. `SCALING_FILTER="fast"`
//! turns it off.

use smithay::backend::renderer::element::{Element, Id, Kind, RenderElement, UnderlyingStorage};
use smithay::backend::renderer::gles::{GlesError, GlesFrame, GlesRenderer, GlesTexProgram};
use smithay::backend::renderer::utils::{CommitCounter, DamageSet, OpaqueRegions};
use smithay::utils::{Buffer as BufferCoords, Physical, Point, Rectangle, Scale, Transform};

/// The texture shader that averages the footprint (Smithay's texture shader contract: `tex`,
/// `alpha`, `v_coords`, the `//_DEFINES_` line).
const SHADER: &str = r#"#version 100
//_DEFINES_
#extension GL_OES_standard_derivatives : enable
#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif

#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
#if defined(EXTERNAL)
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif

uniform float alpha;
varying vec2 v_coords;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

void main() {
    // the texture area this pixel covers, sampled on a 4x4 grid across it
    vec2 dx = dFdx(v_coords);
    vec2 dy = dFdy(v_coords);
    vec4 color = vec4(0.0);
    for (int i = 0; i < 4; i++) {
        for (int j = 0; j < 4; j++) {
            vec2 o = (vec2(float(i), float(j)) + 0.5) / 4.0 - 0.5;
            color += texture2D(tex, v_coords + dx * o.x + dy * o.y);
        }
    }
    color /= 16.0;

#if defined(NO_ALPHA)
    color = vec4(color.rgb, 1.0) * alpha;
#else
    color = color * alpha;
#endif

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
}
"#;

/// The averaging shader, or `None` (logged) where the GPU cannot compile it: then shrinking is
/// plain linear filtering, as before.
pub fn program(renderer: &mut GlesRenderer) -> Option<GlesTexProgram> {
    match renderer.compile_custom_texture_shader(SHADER, &[]) {
        Ok(p) => Some(p),
        Err(e) => {
            eprintln!("[infynitae] Smooth zoomed-out windows unavailable: {e:?}");
            None
        }
    }
}

/// Whether a draw shrinks its source: more buffer pixels than screen pixels (by area, so a
/// rotated buffer counts the same).
pub fn shrinks(src: Rectangle<f64, BufferCoords>, dst: Rectangle<i32, Physical>) -> bool {
    src.size.w * src.size.h > f64::from(dst.size.w) * f64::from(dst.size.h) * 1.02
}

/// An element drawn with the averaging shader when it is shrunk; otherwise as it is.
pub struct Smoothed<E> {
    inner: E,
    program: Option<GlesTexProgram>,
}

impl<E> Smoothed<E> {
    pub fn new(inner: E, program: Option<GlesTexProgram>) -> Self {
        Self { inner, program }
    }
}

impl<E: Element> Element for Smoothed<E> {
    fn id(&self) -> &Id {
        self.inner.id()
    }
    fn current_commit(&self) -> CommitCounter {
        self.inner.current_commit()
    }
    fn location(&self, scale: Scale<f64>) -> Point<i32, Physical> {
        self.inner.location(scale)
    }
    fn src(&self) -> Rectangle<f64, BufferCoords> {
        self.inner.src()
    }
    fn transform(&self) -> Transform {
        self.inner.transform()
    }
    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.inner.geometry(scale)
    }
    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        self.inner.damage_since(scale, commit)
    }
    fn opaque_regions(&self, scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        self.inner.opaque_regions(scale)
    }
    fn alpha(&self) -> f32 {
        self.inner.alpha()
    }
    fn kind(&self) -> Kind {
        self.inner.kind()
    }
}

impl<E: RenderElement<GlesRenderer>> RenderElement<GlesRenderer> for Smoothed<E> {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, BufferCoords>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), GlesError> {
        match &self.program {
            Some(program) if shrinks(src, dst) => {
                frame.override_default_tex_program(program.clone(), Vec::new());
                let drawn = self.inner.draw(frame, src, dst, damage, opaque_regions);
                frame.clear_tex_program_override();
                drawn
            }
            _ => self.inner.draw(frame, src, dst, damage, opaque_regions),
        }
    }

    fn underlying_storage(&self, renderer: &mut GlesRenderer) -> Option<UnderlyingStorage<'_>> {
        // direct scanout sees the window's own buffer, as without the wrapper
        self.inner.underlying_storage(renderer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_shrinking_draw_is_averaged() {
        let src = |w, h| Rectangle::<f64, BufferCoords>::from_size((w, h).into());
        let dst = |w, h| Rectangle::<i32, Physical>::from_size((w, h).into());
        assert!(shrinks(src(800.0, 600.0), dst(400, 300)), "zoomed out");
        assert!(!shrinks(src(800.0, 600.0), dst(800, 600)), "100%");
        assert!(!shrinks(src(800.0, 600.0), dst(1600, 1200)), "zoomed in");
        assert!(
            shrinks(src(600.0, 800.0), dst(400, 300)),
            "a rotated buffer"
        );
        assert!(
            !shrinks(src(801.0, 600.0), dst(800, 600)),
            "rounding is not shrinking"
        );
    }

    /// On the GPU (the first render node; skipped without one): one-pixel white lines every 4
    /// pixels, drawn at a quarter of their size. Plain filtering reads between the lines and
    /// they vanish; averaged, every pixel shows its quarter of white.
    #[test]
    fn a_window_drawn_at_a_quarter_keeps_its_thin_lines() {
        use smithay::backend::allocator::Fourcc;
        use smithay::backend::allocator::gbm::GbmDevice;
        use smithay::backend::drm::DrmDeviceFd;
        use smithay::backend::renderer::gles::GlesRenderbuffer;
        use smithay::backend::renderer::{Bind, ExportMem, Frame, ImportMem, Offscreen, Renderer};
        use smithay::utils::DeviceFd;

        let Some(mut renderer) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/dri/renderD128")
            .ok()
            .and_then(|f| {
                GbmDevice::new(DrmDeviceFd::new(DeviceFd::from(
                    std::os::fd::OwnedFd::from(f),
                )))
                .ok()
            })
            .and_then(|gbm| crate::gles_renderer(&gbm).ok())
        else {
            eprintln!("no GPU render node: skipped");
            return;
        };
        let (w, h) = (256, 64);
        let mut pixels = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in (0..w).step_by(4) {
                pixels[(y * w + x) * 4..][..4].copy_from_slice(&[255, 255, 255, 255]);
            }
            for x in 0..w {
                pixels[(y * w + x) * 4 + 3] = 255;
            }
        }
        let texture = renderer
            .import_memory(
                &pixels,
                Fourcc::Abgr8888,
                (w as i32, h as i32).into(),
                false,
            )
            .expect("texture");
        let program = program(&mut renderer).expect("the shader compiles");
        let (dw, dh) = (64, 16);
        let mut draw = |smooth: bool| -> Vec<f64> {
            let mut buffer: GlesRenderbuffer = renderer
                .create_buffer(Fourcc::Abgr8888, (dw, dh).into())
                .expect("buffer");
            let mut target = renderer.bind(&mut buffer).expect("bind");
            {
                let mut frame = renderer
                    .render(&mut target, (dw, dh).into(), Transform::Normal)
                    .expect("frame");
                let dst = Rectangle::<i32, Physical>::from_size((dw, dh).into());
                frame.clear([0.0, 0.0, 0.0, 1.0].into(), &[dst]).unwrap();
                if smooth {
                    frame.override_default_tex_program(program.clone(), Vec::new());
                }
                Frame::render_texture_from_to(
                    &mut frame,
                    &texture,
                    Rectangle::from_size((w as f64, h as f64).into()),
                    dst,
                    &[dst],
                    &[],
                    Transform::Normal,
                    1.0,
                )
                .unwrap();
                frame.finish().unwrap().wait().unwrap();
            }
            let mapping = renderer
                .copy_framebuffer(
                    &target,
                    Rectangle::from_size((dw, dh).into()),
                    Fourcc::Abgr8888,
                )
                .expect("read back");
            renderer
                .map_texture(&mapping)
                .expect("map")
                .chunks(4)
                .map(|p| f64::from(p[0]) / 255.0)
                .collect()
        };
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        let plain = draw(false);
        let smooth = draw(true);
        assert!(
            smooth.iter().all(|v| (v - 0.25).abs() < 0.08),
            "averaged: every pixel about a quarter white, got {:.2}..{:.2}",
            smooth.iter().cloned().fold(1.0, f64::min),
            smooth.iter().cloned().fold(0.0, f64::max)
        );
        eprintln!(
            "plain mean {:.3}, averaged mean {:.3}",
            mean(&plain),
            mean(&smooth)
        );
        assert!(
            (mean(&plain) - 0.25).abs() > 0.1,
            "the control: plain filtering loses the lines (mean {:.3})",
            mean(&plain)
        );
    }
}
