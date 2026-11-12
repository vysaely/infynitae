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
use smithay::backend::renderer::gles::{
    GlesError, GlesFrame, GlesPixelProgram, GlesRenderer, GlesTexProgram, Uniform, UniformName,
    UniformType,
};
use smithay::utils::Size;

use infynitae_wayland::corners::Rounding;
use smithay::backend::renderer::utils::{CommitCounter, DamageSet, OpaqueRegions};
use smithay::utils::{Buffer as BufferCoords, Physical, Point, Rectangle, Scale, Transform};

use infynitae_wayland::log_line;

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

/// The rounded-rect shape both corner shaders share (WINDOW_CORNERS, `corners.rs`): how much of
/// the pixel at `p` (pixels from the rect's top left) is inside a rect of `size` whose corners
/// (top left, top right, bottom right, bottom left) have radii `r`, with an anti-aliased edge.
/// The curve is a gentle squircle (exponent 2.6; a circle is 2, iOS about 5): softer than an arc,
/// still a desktop corner.
const SHAPE: &str = r#"
float corner_inside(vec2 p, vec2 size, vec4 r) {
    bool left = p.x < size.x * 0.5;
    bool top = p.y < size.y * 0.5;
    float rad = top ? (left ? r.x : r.y) : (left ? r.w : r.z);
    // distance to the nearest straight edge, for the straight parts
    float edge = min(min(p.x, p.y), min(size.x - p.x, size.y - p.y));
    float straight = clamp(edge + 0.5, 0.0, 1.0);
    if (rad <= 0.0) {
        return straight;
    }
    vec2 c = vec2(left ? rad : size.x - rad, top ? rad : size.y - rad);
    vec2 q = vec2(left ? c.x - p.x : p.x - c.x, top ? c.y - p.y : p.y - c.y);
    if (q.x <= 0.0 || q.y <= 0.0) {
        return straight;
    }
    vec2 u = q / rad;
    float n = 2.6;
    float d = pow(pow(u.x, n) + pow(u.y, n), 1.0 / n);
    return clamp((1.0 - d) * rad + 0.5, 0.0, 1.0) * straight;
}
"#;

/// A window's own surfaces, cut to its rounded corners. The pixel's place in the drawn rect comes
/// from inverting the texture matrix the vertex stage used, so any crop, scale or buffer
/// transform is right; `u_offset` places that rect within the window.
const ROUND_TEX: &str = r#"#version 100
//_DEFINES_
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
uniform highp mat3 tex_matrix;
uniform vec2 u_offset;
uniform vec2 u_window;
uniform vec4 u_radii;
varying vec2 v_coords;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif
//_SHAPE_
void main() {
    vec2 b = v_coords - vec2(tex_matrix[2][0], tex_matrix[2][1]);
    float m00 = tex_matrix[0][0];
    float m01 = tex_matrix[0][1];
    float m10 = tex_matrix[1][0];
    float m11 = tex_matrix[1][1];
    float det = m00 * m11 - m10 * m01;
    vec2 pos = vec2(m11 * b.x - m10 * b.y, -m01 * b.x + m00 * b.y) / det;
    float inside = corner_inside(pos + u_offset, u_window, u_radii);

    vec4 color = texture2D(tex, v_coords);
#if defined(NO_ALPHA)
    color = vec4(color.rgb, 1.0) * alpha;
#else
    color = color * alpha;
#endif
    color = color * inside;

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif
    gl_FragColor = color;
}
"#;

/// A window's border as a ring around its rounded geometry: inside the outer shape (the window
/// grown by the border width, radii grown with it) and outside the window's own shape.
const RING: &str = r#"
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
uniform vec2 size;
uniform float alpha;
uniform vec2 u_offset;
uniform vec2 u_window;
uniform vec4 u_radii;
uniform float u_width;
uniform vec4 u_color;
varying vec2 v_coords;
#if defined(DEBUG_FLAGS)
uniform float tint;
#endif
//_SHAPE_
void main() {
    vec2 p = v_coords * size + u_offset;
    vec4 grown = vec4(
        u_radii.x > 0.0 ? u_radii.x + u_width : 0.0,
        u_radii.y > 0.0 ? u_radii.y + u_width : 0.0,
        u_radii.z > 0.0 ? u_radii.z + u_width : 0.0,
        u_radii.w > 0.0 ? u_radii.w + u_width : 0.0);
    float outer = corner_inside(p + vec2(u_width), u_window + vec2(2.0 * u_width), grown);
    float inner = corner_inside(p, u_window, u_radii);
    float a = outer * (1.0 - inner);
    vec4 color = u_color * a * alpha;
#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif
    gl_FragColor = color;
}
"#;

/// The corner shaders, compiled once (`init_corners`); `None` where the GPU cannot: then windows
/// are square, as before.
pub struct Corners {
    round: GlesTexProgram,
    ring: GlesPixelProgram,
}

thread_local! {
    static CORNERS: std::cell::RefCell<Option<Corners>> = const { std::cell::RefCell::new(None) };
}

/// Compiles the corner shaders for `renderer` (at start and after a new renderer).
pub fn init_corners(renderer: &mut GlesRenderer) {
    let shared = [
        UniformName::new("u_offset", UniformType::_2f),
        UniformName::new("u_window", UniformType::_2f),
        UniformName::new("u_radii", UniformType::_4f),
    ];
    let mut ring_names = shared.to_vec();
    ring_names.push(UniformName::new("u_width", UniformType::_1f));
    ring_names.push(UniformName::new("u_color", UniformType::_4f));
    let round =
        renderer.compile_custom_texture_shader(ROUND_TEX.replace("//_SHAPE_", SHAPE), &shared);
    let ring = renderer.compile_custom_pixel_shader(RING.replace("//_SHAPE_", SHAPE), &ring_names);
    let corners = match (round, ring) {
        (Ok(round), Ok(ring)) => Some(Corners { round, ring }),
        (r, g) => {
            log_line!(
                "[infynitae] Rounded window corners unavailable (square instead): {:?} {:?}",
                r.err(),
                g.err()
            );
            None
        }
    };
    CORNERS.with(|c| *c.borrow_mut() = corners);
}

fn shape_uniforms(round: &Rounding, dst: Rectangle<i32, Physical>) -> Vec<Uniform<'static>> {
    let [a, b, c, d] = round.radii;
    vec![
        Uniform::new(
            "u_offset",
            (
                (dst.loc.x - round.window.loc.x) as f32,
                (dst.loc.y - round.window.loc.y) as f32,
            ),
        ),
        Uniform::new(
            "u_window",
            (round.window.size.w as f32, round.window.size.h as f32),
        ),
        Uniform::new("u_radii", (a, b, c, d)),
    ]
}

/// The four corner squares of a rounded window, in the coordinates of an element at `at`: what
/// may show through there, so not opaque.
fn corner_squares(round: &Rounding, at: Point<i32, Physical>) -> [Rectangle<i32, Physical>; 4] {
    let w = round.window;
    let r = |v: f32| v.ceil() as i32;
    let [tl, tr, br, bl] = round.radii;
    let o = |x: i32, y: i32| Point::<i32, Physical>::from((x - at.x, y - at.y));
    [
        Rectangle::new(o(w.loc.x, w.loc.y), (r(tl), r(tl)).into()),
        Rectangle::new(
            o(w.loc.x + w.size.w - r(tr), w.loc.y),
            (r(tr), r(tr)).into(),
        ),
        Rectangle::new(
            o(w.loc.x + w.size.w - r(br), w.loc.y + w.size.h - r(br)),
            (r(br), r(br)).into(),
        ),
        Rectangle::new(
            o(w.loc.x, w.loc.y + w.size.h - r(bl)),
            (r(bl), r(bl)).into(),
        ),
    ]
}

/// The averaging shader, or `None` (logged) where the GPU cannot compile it: then shrinking is
/// plain linear filtering, as before.
pub fn program(renderer: &mut GlesRenderer) -> Option<GlesTexProgram> {
    match renderer.compile_custom_texture_shader(SHADER, &[]) {
        Ok(p) => Some(p),
        Err(e) => {
            log_line!("[infynitae] Smooth zoomed-out windows unavailable: {e:?}");
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
    /// A rounded window's surface, or a piece of its border (`corners.rs`).
    round: Option<Rounding>,
}

impl<E> Smoothed<E> {
    pub fn new(inner: E, program: Option<GlesTexProgram>) -> Self {
        Self {
            inner,
            program,
            round: None,
        }
    }

    /// Drawn with rounded corners (`round` from the frame's `WaylandState::rounding`), when the
    /// corner shaders compiled.
    pub fn rounded(mut self, round: Option<Rounding>) -> Self {
        let ready = CORNERS.with(|c| c.borrow().is_some());
        self.round = round.filter(|r| ready && r.is_round());
        self
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
        match &self.round {
            // a border piece is mostly see-through around the curve
            Some(r) if r.border.is_some() => OpaqueRegions::default(),
            // the window's corners let what is behind show
            Some(r) => {
                let at = self.inner.geometry(scale).loc;
                let corners = corner_squares(r, at);
                self.inner
                    .opaque_regions(scale)
                    .iter()
                    .flat_map(|rect| rect.subtract_rects(corners.iter().copied()))
                    .collect()
            }
            None => self.inner.opaque_regions(scale),
        }
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
        if let Some(round) = &self.round {
            let drawn = CORNERS.with(|c| {
                let c = c.borrow();
                let corners = c.as_ref()?;
                let mut uniforms = shape_uniforms(round, dst);
                Some(match round.border {
                    // a border piece: the ring's part inside this rect
                    Some((width, color)) => {
                        let local: Vec<_> = damage.to_vec();
                        uniforms.push(Uniform::new("u_width", width));
                        let [r, g, b, a] = color.components();
                        uniforms.push(Uniform::new("u_color", (r, g, b, a)));
                        frame.render_pixel_shader_to(
                            &corners.ring,
                            Rectangle::from_size(
                                (f64::from(dst.size.w), f64::from(dst.size.h)).into(),
                            ),
                            dst,
                            Size::from((dst.size.w, dst.size.h)),
                            Some(&local),
                            self.inner.alpha(),
                            &uniforms,
                        )
                    }
                    None => {
                        frame.override_default_tex_program(corners.round.clone(), uniforms);
                        let drawn = self.inner.draw(frame, src, dst, damage, opaque_regions);
                        frame.clear_tex_program_override();
                        drawn
                    }
                })
            });
            if let Some(drawn) = drawn {
                return drawn;
            }
        }
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
        // a rounded window is drawn by the GPU (its corners cut); never one that fills the
        // screen, which is square, so direct scanout is untouched. Otherwise the window's own
        // buffer, as without the wrapper
        if self.round.is_some() {
            return None;
        }
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
