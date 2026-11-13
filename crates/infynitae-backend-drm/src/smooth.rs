//! Compositor-side zoom scaling and rounded window corners (`SCALING_FILTER`, `WINDOW_CORNERS`).
//!
//! - At 1:1 (within 2% area tolerance), surfaces take the plain path (with rounded corners when
//!   set) and direct scanout is untouched.
//! - Zoomed in (`enlarges`): `sharp` uses nearest-neighbor, `smooth` uses bilinear, and
//!   `fsr` / `fsr-sharp` run AMD FidelityFX Super Resolution 1 (EASU edge-adaptive upscale into a
//!   cached offscreen texture, followed by RCAS sharpening on the output frame).
//! - Zoomed out (`shrinks`, except legacy `fast`): a cached halving box-filter pyramid per element
//!   commit brings the texture within 2x of destination size before a final area-averaging pass.

use std::collections::HashMap;
use std::sync::Arc;

use infynitae_wayland::ZoomFilter;
use infynitae_wayland::convert::{OutputElement, Rounding};
use infynitae_wayland::log_line;
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::surface::WaylandSurfaceTexture;
use smithay::backend::renderer::element::{Element, Id, Kind, RenderElement, UnderlyingStorage};
use smithay::backend::renderer::gles::{
    GlesError, GlesFrame, GlesPixelProgram, GlesRenderer, GlesTexProgram, GlesTexture, Uniform,
    UniformName, UniformType,
};
use smithay::backend::renderer::utils::{CommitCounter, DamageSet, OpaqueRegions};
use smithay::backend::renderer::{Bind, Frame, Offscreen, Renderer, Texture};
use smithay::utils::{Buffer as BufferCoords, Physical, Point, Rectangle, Scale, Size, Transform};

// Based on AMD FidelityFX Super Resolution 1.0 (ffx_fsr1.h)
//
// Copyright (c) 2021 Advanced Micro Devices, Inc. All rights reserved.
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.  IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

/// Minimum source and destination pixel dimensions for GPU scaling passes (skips 1x1 spacers).
const MIN_SCALE_TEX_DIM: i32 = 4;

/// Maximum unclipped EASU offscreen texture dimension. Windows larger than this on an axis are
/// clipped to the visible output slice (with `OVERSIZED_PAD` pixels of margin).
const MAX_EASU_DIM: i32 = 4096;

/// Extra margin (in physical pixels) around the visible output slice for oversized FSR windows.
const OVERSIZED_PAD: i32 = 16;

/// Area ratio above which another 2x2 halving mip level is generated (`0.5x` has ratio `4.0`).
const MIP_AREA_THRESHOLD: f64 = 3.85;

/// Number of `render` turns an unseen cached texture survives before eviction.
const MAX_IDLE_GENERATIONS: u64 = 120;

/// Hard cap on cached elements per table so many closed windows cannot grow VRAM without bound.
const MAX_CACHE_ENTRIES: usize = 64;

/// Rounded-rect SDF shared across rounded corner, area-average, and RCAS shaders.
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

/// Common GLSL helper to recover normalized valid-texel bounds `[uv_lo, uv_hi]` from Smithay's
/// `tex_matrix` and destination rect size, preventing edge wraparound bleed under crops/transforms.
const UV_BOUNDS_FN: &str = r#"
void valid_uv_bounds(mat3 m, vec2 dst_size, vec2 inv_tex, out vec2 uv_lo, out vec2 uv_hi) {
    vec2 c0 = (m * vec3(0.0, 0.0, 1.0)).xy;
    vec2 c1 = (m * vec3(dst_size.x, 0.0, 1.0)).xy;
    vec2 c2 = (m * vec3(0.0, dst_size.y, 1.0)).xy;
    vec2 c3 = (m * vec3(dst_size.x, dst_size.y, 1.0)).xy;
    uv_lo = min(min(c0, c1), min(c2, c3)) + 0.5 * inv_tex;
    uv_hi = max(max(c0, c1), max(c2, c3)) - 0.5 * inv_tex;
    vec2 mid = 0.5 * (uv_lo + uv_hi);
    uv_lo = min(uv_lo, mid);
    uv_hi = max(uv_hi, mid);
}
"#;

/// Final 4x4 derivative area-averaging shader template (compiled plain and with `ROUND_CORNERS`).
const AVG_TEMPLATE: &str = r#"#version 100
//_DEFINES_
#extension GL_OES_standard_derivatives : enable
#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif
//_EXTRA_DEFINES_

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
uniform vec2 u_inv_tex;
uniform vec2 u_dst_size;
varying vec2 v_coords;

#if defined(ROUND_CORNERS)
uniform vec2 u_offset;
uniform vec2 u_window;
uniform vec4 u_radii;
//_SHAPE_
#endif

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

//_UV_BOUNDS_

void main() {
    vec2 uv_lo;
    vec2 uv_hi;
    valid_uv_bounds(tex_matrix, u_dst_size, u_inv_tex, uv_lo, uv_hi);

    vec2 dx = dFdx(v_coords);
    vec2 dy = dFdy(v_coords);
    vec4 color = vec4(0.0);
    for (int i = 0; i < 4; i++) {
        for (int j = 0; j < 4; j++) {
            vec2 o = (vec2(float(i), float(j)) + 0.5) / 4.0 - 0.5;
            vec2 uv = clamp(v_coords + dx * o.x + dy * o.y, uv_lo, uv_hi);
            vec4 s = texture2D(tex, uv);
#if defined(NO_ALPHA)
            s.a = 1.0;
#endif
            color += s;
        }
    }
    color /= 16.0;
    color *= alpha;

#if defined(ROUND_CORNERS)
    vec2 b = v_coords - vec2(tex_matrix[2][0], tex_matrix[2][1]);
    float m00 = tex_matrix[0][0];
    float m01 = tex_matrix[0][1];
    float m10 = tex_matrix[1][0];
    float m11 = tex_matrix[1][1];
    float det = m00 * m11 - m10 * m01;
    vec2 pos = vec2(m11 * b.x - m10 * b.y, -m01 * b.x + m00 * b.y) / det;
    float inside = corner_inside(pos + u_offset, u_window, u_radii);
    color *= inside;
#endif

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
}
"#;

/// Exact 2x2 box-filter downsample shader for building cached mip levels.
const DOWNSAMPLE_SHADER: &str = r#"#version 100
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
uniform vec2 u_inv_tex;
uniform vec2 u_dst_size;
varying vec2 v_coords;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

//_UV_BOUNDS_

vec4 sample_clamped(vec2 uv, vec2 lo, vec2 hi) {
    vec4 s = texture2D(tex, clamp(uv, lo, hi));
#if defined(NO_ALPHA)
    s.a = 1.0;
#endif
    return s;
}

void main() {
    vec2 uv_lo;
    vec2 uv_hi;
    valid_uv_bounds(tex_matrix, u_dst_size, u_inv_tex, uv_lo, uv_hi);

    vec2 h = 0.5 * u_inv_tex;
    vec4 c0 = sample_clamped(v_coords + vec2(-h.x, -h.y), uv_lo, uv_hi);
    vec4 c1 = sample_clamped(v_coords + vec2( h.x, -h.y), uv_lo, uv_hi);
    vec4 c2 = sample_clamped(v_coords + vec2(-h.x,  h.y), uv_lo, uv_hi);
    vec4 c3 = sample_clamped(v_coords + vec2( h.x,  h.y), uv_lo, uv_hi);
    gl_FragColor = 0.25 * (c0 + c1 + c2 + c3) * alpha;
}
"#;

/// AMD FidelityFX Super Resolution 1 (MIT) EASU (Edge-Adaptive Spatial Upsampling) in GLSL ES 1.00.
const EASU_SHADER: &str = r#"#version 100
//_DEFINES_
#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif

// Based on AMD FidelityFX Super Resolution 1.0 (ffx_fsr1.h), MIT License:
// Copyright (c) 2021 Advanced Micro Devices, Inc. All rights reserved.

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
uniform vec2 u_inv_tex;
uniform vec2 u_dst_size;
varying vec2 v_coords;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

//_UV_BOUNDS_

vec4 easu_fetch(vec2 p0, vec2 off, vec2 lo, vec2 hi) {
    vec4 c = texture2D(tex, clamp(p0 + off * u_inv_tex, lo, hi));
#if defined(NO_ALPHA)
    c.a = 1.0;
#endif
    return c;
}

float easu_luma(vec4 c) {
    return 0.5 * c.g + 0.25 * (c.r + c.b);
}

// Accumulates a structure-tensor double-angle orientation vector and edge monotonicity length.
// Using (gx^2 - gy^2, 2*gx*gy) keeps opposing gradients across 1 px UI stems additive rather
// than cancelling to zero.
void easu_set(
    inout vec2 v2_dir,
    inout float len,
    float w,
    float lt,
    float ll,
    float lc,
    float lr,
    float lb
) {
    float dx = lr - ll;
    float dy = lb - lt;
    float len_x = max(abs(lr - lc), abs(lc - ll));
    float ax = 0.0;
    if (len_x > 1e-5) {
        ax = clamp(abs(dx) / len_x, 0.0, 1.0);
        ax *= ax;
    }
    float len_y = max(abs(lb - lc), abs(lc - lt));
    float ay = 0.0;
    if (len_y > 1e-5) {
        ay = clamp(abs(dy) / len_y, 0.0, 1.0);
        ay *= ay;
    }
    float g2 = dx * dx + dy * dy;
    if (g2 > 1e-6) {
        v2_dir += vec2(dx * dx - dy * dy, 2.0 * dx * dy) * (w / sqrt(g2));
    }
    len += w * max(ax, ay);
}

// Evaluates AMD FSR 1's polynomial approximation to a 2-lobe Lanczos kernel along rotated edge axes.
void easu_tap(
    inout vec4 aC,
    inout float aW,
    vec2 off,
    vec2 f,
    vec2 dir,
    vec2 scale,
    float lob,
    float clp,
    vec4 c
) {
    vec2 d = off - f;
    vec2 v = vec2(d.x * dir.x + d.y * dir.y, d.x * (-dir.y) + d.y * dir.x);
    v *= scale;
    float d2 = min(dot(v, v), clp);
    float wB = (2.0 / 5.0) * d2 - 1.0;
    float wA = lob * d2 - 1.0;
    wB *= wB;
    wA *= wA;
    wB = (25.0 / 16.0) * wB - (25.0 / 16.0 - 1.0);
    float w = wB * wA;
    aC += c * w;
    aW += w;
}

void main() {
    vec2 uv_lo;
    vec2 uv_hi;
    valid_uv_bounds(tex_matrix, u_dst_size, u_inv_tex, uv_lo, uv_hi);

    vec2 pp = v_coords / u_inv_tex - vec2(0.5);
    vec2 fp = floor(pp);
    vec2 f = pp - fp;
    vec2 p0 = (fp + vec2(0.5)) * u_inv_tex;

    // 12-tap cross stencil around (f=0,0, g=1,0, j=0,1, k=1,1)
    vec4 b = easu_fetch(p0, vec2( 0.0, -1.0), uv_lo, uv_hi);
    vec4 c = easu_fetch(p0, vec2( 1.0, -1.0), uv_lo, uv_hi);
    vec4 e = easu_fetch(p0, vec2(-1.0,  0.0), uv_lo, uv_hi);
    vec4 cf = easu_fetch(p0, vec2(0.0,  0.0), uv_lo, uv_hi);
    vec4 g = easu_fetch(p0, vec2( 1.0,  0.0), uv_lo, uv_hi);
    vec4 h = easu_fetch(p0, vec2( 2.0,  0.0), uv_lo, uv_hi);
    vec4 i = easu_fetch(p0, vec2(-1.0,  1.0), uv_lo, uv_hi);
    vec4 j = easu_fetch(p0, vec2( 0.0,  1.0), uv_lo, uv_hi);
    vec4 k = easu_fetch(p0, vec2( 1.0,  1.0), uv_lo, uv_hi);
    vec4 l = easu_fetch(p0, vec2( 2.0,  1.0), uv_lo, uv_hi);
    vec4 n = easu_fetch(p0, vec2( 0.0,  2.0), uv_lo, uv_hi);
    vec4 o = easu_fetch(p0, vec2( 1.0,  2.0), uv_lo, uv_hi);

    float lb = easu_luma(b);
    float lc = easu_luma(c);
    float le = easu_luma(e);
    float lf = easu_luma(cf);
    float lg = easu_luma(g);
    float lh = easu_luma(h);
    float li = easu_luma(i);
    float lj = easu_luma(j);
    float lk = easu_luma(k);
    float ll = easu_luma(l);
    float ln = easu_luma(n);
    float lo = easu_luma(o);

    vec2 v2_dir = vec2(0.0);
    float len = 0.0;
    easu_set(v2_dir, len, (1.0 - f.x) * (1.0 - f.y), lb, le, lf, lg, lj);
    easu_set(v2_dir, len, f.x * (1.0 - f.y),         lc, lf, lg, lh, lk);
    easu_set(v2_dir, len, (1.0 - f.x) * f.y,         lf, li, lj, lk, ln);
    easu_set(v2_dir, len, f.x * f.y,                 lg, lj, lk, ll, lo);

    vec2 dir = vec2(1.0, 0.0);
    float v2_len = dot(v2_dir, v2_dir);
    if (v2_len > 1e-6) {
        vec2 u2 = v2_dir * inversesqrt(v2_len);
        float dx = sqrt(max(0.5 * (1.0 + u2.x), 0.0));
        float dy = sqrt(max(0.5 * (1.0 - u2.x), 0.0));
        dir = vec2(dx, u2.y < 0.0 ? -dy : dy);
    } else {
        len = 0.0;
    }

    vec2 k_scale = vec2(1.0 + 0.35 * len, 1.0 - 0.35 * len);
    float lob = 0.5 + (0.21 - 0.5) * len;
    float clp = 1.0 / lob;

    vec4 aC = vec4(0.0);
    float aW = 0.0;
    easu_tap(aC, aW, vec2( 0.0, -1.0), f, dir, k_scale, lob, clp, b);
    easu_tap(aC, aW, vec2( 1.0, -1.0), f, dir, k_scale, lob, clp, c);
    easu_tap(aC, aW, vec2(-1.0,  0.0), f, dir, k_scale, lob, clp, e);
    easu_tap(aC, aW, vec2( 0.0,  0.0), f, dir, k_scale, lob, clp, cf);
    easu_tap(aC, aW, vec2( 1.0,  0.0), f, dir, k_scale, lob, clp, g);
    easu_tap(aC, aW, vec2( 2.0,  0.0), f, dir, k_scale, lob, clp, h);
    easu_tap(aC, aW, vec2(-1.0,  1.0), f, dir, k_scale, lob, clp, i);
    easu_tap(aC, aW, vec2( 0.0,  1.0), f, dir, k_scale, lob, clp, j);
    easu_tap(aC, aW, vec2( 1.0,  1.0), f, dir, k_scale, lob, clp, k);
    easu_tap(aC, aW, vec2( 2.0,  1.0), f, dir, k_scale, lob, clp, l);
    easu_tap(aC, aW, vec2( 0.0,  2.0), f, dir, k_scale, lob, clp, n);
    easu_tap(aC, aW, vec2( 1.0,  2.0), f, dir, k_scale, lob, clp, o);

    vec4 mn4 = min(min(cf, g), min(j, k));
    vec4 mx4 = max(max(cf, g), max(j, k));
    vec4 color = aW > 1e-5 ? clamp(aC / aW, mn4, mx4) : cf;
    gl_FragColor = color * alpha;
}
"#;

/// AMD FidelityFX Super Resolution 1 (MIT) RCAS (Robust Contrast-Adaptive Sharpening) template.
/// Compiled plain and with `ROUND_CORNERS`; reads the 1:1 RGBA offscreen EASU texture.
const RCAS_TEMPLATE: &str = r#"#version 100
//_DEFINES_
//_EXTRA_DEFINES_

// Based on AMD FidelityFX Super Resolution 1.0 (ffx_fsr1.h), MIT License:
// Copyright (c) 2021 Advanced Micro Devices, Inc. All rights reserved.

#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
uniform sampler2D tex;

uniform float alpha;
uniform highp mat3 tex_matrix;
uniform vec2 u_inv_tex;
uniform float u_sharp;
varying vec2 v_coords;

#if defined(ROUND_CORNERS)
uniform vec2 u_offset;
uniform vec2 u_window;
uniform vec4 u_radii;
//_SHAPE_
#endif

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

float rcas_luma(vec3 c) {
    return 0.5 * c.g + 0.25 * (c.r + c.b);
}

void main() {
    vec2 lo = 0.5 * u_inv_tex;
    vec2 hi = max(lo, vec2(1.0) - 0.5 * u_inv_tex);

    vec4 e = texture2D(tex, clamp(v_coords, lo, hi));
    vec3 b = texture2D(tex, clamp(v_coords + vec2(0.0, -u_inv_tex.y), lo, hi)).rgb;
    vec3 d = texture2D(tex, clamp(v_coords + vec2(-u_inv_tex.x, 0.0), lo, hi)).rgb;
    vec3 f = texture2D(tex, clamp(v_coords + vec2( u_inv_tex.x, 0.0), lo, hi)).rgb;
    vec3 h = texture2D(tex, clamp(v_coords + vec2(0.0,  u_inv_tex.y), lo, hi)).rgb;

    float b_l = rcas_luma(b);
    float d_l = rcas_luma(d);
    float f_l = rcas_luma(f);
    float h_l = rcas_luma(h);
    float e_l = rcas_luma(e.rgb);
    float mx_l = max(max(max(b_l, d_l), max(f_l, h_l)), e_l);
    float mn_l = min(min(min(b_l, d_l), min(f_l, h_l)), e_l);
    float range_l = max(mx_l - mn_l, 0.0);

    // High-frequency noise attenuation and micro-contrast deadzone for clean flat UI panes.
    float nz = 0.25 * (b_l + d_l + f_l + h_l) - e_l;
    float noise_atten = 1.0 - 0.5 * clamp(abs(nz) / max(range_l, 1e-4), 0.0, 1.0);
    noise_atten *= clamp(range_l * 48.0, 0.0, 1.0);

    // Non-negativity and upper-bound algebraic limits against actual 4-neighbor sum so gray AA
    // pixels sitting between pure black strokes and pure white backgrounds still sharpen cleanly
    // without clipping.
    vec3 sum4 = b + d + f + h;
    float limit = 0.1875 * clamp(u_sharp, 0.0, 1.0) * noise_atten;
    vec3 k_low = e.rgb / max(sum4, vec3(1e-5));
    vec3 k_high = (vec3(1.0) - e.rgb) / max(vec3(4.0) - sum4, vec3(1e-5));
    vec3 k_chan = min(min(k_low, k_high), vec3(limit));
    float k = min(k_chan.r, min(k_chan.g, k_chan.b));
    float w = -k;

    vec3 sharp_rgb = (e.rgb + w * sum4) / max(1.0 + 4.0 * w, 1e-4);
    // Keep premultiplied alpha invariant 0 <= rgb <= a so transparent edges never halo.
    vec4 color = vec4(clamp(sharp_rgb, vec3(0.0), vec3(e.a)), e.a) * alpha;

#if defined(ROUND_CORNERS)
    vec2 cb = v_coords - vec2(tex_matrix[2][0], tex_matrix[2][1]);
    float m00 = tex_matrix[0][0];
    float m01 = tex_matrix[0][1];
    float m10 = tex_matrix[1][0];
    float m11 = tex_matrix[1][1];
    float det = m00 * m11 - m10 * m01;
    vec2 pos = vec2(m11 * cb.x - m10 * cb.y, -m01 * cb.x + m00 * cb.y) / det;
    float inside = corner_inside(pos + u_offset, u_window, u_radii);
    color *= inside;
#endif

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
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

/// Compiles the corner shaders for `renderer` (at start and after a new renderer); whether they
/// are ready.
pub fn init_corners(renderer: &mut GlesRenderer) -> bool {
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
    let ready = corners.is_some();
    CORNERS.with(|c| *c.borrow_mut() = corners);
    ready
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

/// Compiled GPU texture shaders for zoomed-out downsampling and zoomed-in FSR 1.
#[derive(Clone, Debug)]
pub struct ZoomPrograms {
    pub avg: GlesTexProgram,
    pub avg_round: GlesTexProgram,
    pub downsample: GlesTexProgram,
    pub easu: GlesTexProgram,
    pub rcas: GlesTexProgram,
    pub rcas_round: GlesTexProgram,
}

impl ZoomPrograms {
    /// Compiles all zoom shader programs for `renderer`.
    pub fn compile(renderer: &mut GlesRenderer) -> Result<Self, GlesError> {
        let tex_dst_uniforms = [
            UniformName::new("u_inv_tex", UniformType::_2f),
            UniformName::new("u_dst_size", UniformType::_2f),
        ];
        let shape_uniform_names = [
            UniformName::new("u_offset", UniformType::_2f),
            UniformName::new("u_window", UniformType::_2f),
            UniformName::new("u_radii", UniformType::_4f),
        ];

        let avg_base = AVG_TEMPLATE.replace("//_UV_BOUNDS_", UV_BOUNDS_FN);
        let avg = renderer.compile_custom_texture_shader(
            avg_base.replace("//_EXTRA_DEFINES_", ""),
            &tex_dst_uniforms,
        )?;
        let mut avg_round_uniforms = tex_dst_uniforms.to_vec();
        avg_round_uniforms.extend_from_slice(&shape_uniform_names);
        let avg_round = renderer.compile_custom_texture_shader(
            avg_base
                .replace("//_EXTRA_DEFINES_", "#define ROUND_CORNERS")
                .replace("//_SHAPE_", SHAPE),
            &avg_round_uniforms,
        )?;

        let downsample = renderer.compile_custom_texture_shader(
            DOWNSAMPLE_SHADER.replace("//_UV_BOUNDS_", UV_BOUNDS_FN),
            &tex_dst_uniforms,
        )?;
        let easu = renderer.compile_custom_texture_shader(
            EASU_SHADER.replace("//_UV_BOUNDS_", UV_BOUNDS_FN),
            &tex_dst_uniforms,
        )?;

        let rcas_uniforms = [
            UniformName::new("u_inv_tex", UniformType::_2f),
            UniformName::new("u_sharp", UniformType::_1f),
        ];
        let rcas = renderer.compile_custom_texture_shader(
            RCAS_TEMPLATE.replace("//_EXTRA_DEFINES_", ""),
            &rcas_uniforms,
        )?;
        let mut rcas_round_uniforms = rcas_uniforms.to_vec();
        rcas_round_uniforms.extend_from_slice(&shape_uniform_names);
        let rcas_round = renderer.compile_custom_texture_shader(
            RCAS_TEMPLATE
                .replace("//_EXTRA_DEFINES_", "#define ROUND_CORNERS")
                .replace("//_SHAPE_", SHAPE),
            &rcas_round_uniforms,
        )?;

        Ok(Self {
            avg,
            avg_round,
            downsample,
            easu,
            rcas,
            rcas_round,
        })
    }
}

/// Compiles `ZoomPrograms` for `renderer` or logs why custom zoom shaders are unavailable.
pub fn zoom_programs(renderer: &mut GlesRenderer) -> Option<Arc<ZoomPrograms>> {
    match ZoomPrograms::compile(renderer) {
        Ok(p) => Some(Arc::new(p)),
        Err(e) => {
            log_line!("[infynitae] Zoom scaling shaders unavailable: {e:?}");
            None
        }
    }
}

/// Exact bit-pattern key for `Rectangle<f64, BufferCoords>` so fractional crops trigger rebuilds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SrcKey {
    loc_x: u64,
    loc_y: u64,
    w: u64,
    h: u64,
}

impl SrcKey {
    fn from_rect(src: Rectangle<f64, BufferCoords>) -> Self {
        Self {
            loc_x: src.loc.x.to_bits(),
            loc_y: src.loc.y.to_bits(),
            w: src.size.w.to_bits(),
            h: src.size.h.to_bits(),
        }
    }
}

struct MipEntry {
    commit: CommitCounter,
    src: SrcKey,
    levels: Vec<GlesTexture>,
    last_used_gen: u64,
}

struct FsrEntry {
    commit: CommitCounter,
    src: SrcKey,
    transform: Transform,
    dst_size: Size<i32, Physical>,
    slice_local: Rectangle<i32, Physical>,
    easu: GlesTexture,
    last_used_gen: u64,
}

/// Per-window GPU texture caches for zoomed-out halving mipmaps and zoomed-in EASU outputs.
#[derive(Default)]
pub struct ZoomCache {
    generation: u64,
    mips: HashMap<Id, MipEntry>,
    fsr: HashMap<Id, FsrEntry>,
}

impl ZoomCache {
    /// Drops all cached textures (must be called on GPU renderer recovery).
    pub fn clear(&mut self) {
        self.mips.clear();
        self.fsr.clear();
    }

    /// Drops cached zoomed-out mip pyramids (e.g. when switching to `SCALING_FILTER="fast"`).
    pub fn clear_mips(&mut self) {
        self.mips.clear();
    }

    /// Drops cached zoomed-in EASU textures (e.g. when switching away from FSR).
    pub fn clear_fsr(&mut self) {
        self.fsr.clear();
    }

    /// Advances the GC epoch and evicts textures unseen for `MAX_IDLE_GENERATIONS`.
    pub fn gc(&mut self) {
        self.generation = self.generation.saturating_add(1);
        let epoch = self.generation;
        self.mips
            .retain(|_, e| e.last_used_gen.saturating_add(MAX_IDLE_GENERATIONS) >= epoch);
        self.fsr
            .retain(|_, e| e.last_used_gen.saturating_add(MAX_IDLE_GENERATIONS) >= epoch);
        trim_map_oldest(&mut self.mips, |e| e.last_used_gen);
        trim_map_oldest(&mut self.fsr, |e| e.last_used_gen);
    }
}

fn trim_map_oldest<V>(map: &mut HashMap<Id, V>, last_gen: impl Fn(&V) -> u64) {
    while map.len() > MAX_CACHE_ENTRIES {
        let Some(oldest) = map
            .iter()
            .min_by_key(|(_, v)| last_gen(v))
            .map(|(k, _)| k.clone())
        else {
            break;
        };
        map.remove(&oldest);
    }
}

/// Whether a draw shrinks its source: more buffer pixels than screen pixels (by area, so a
/// rotated buffer counts the same).
pub fn shrinks(src: Rectangle<f64, BufferCoords>, dst: Rectangle<i32, Physical>) -> bool {
    src.size.w * src.size.h > f64::from(dst.size.w) * f64::from(dst.size.h) * 1.02
}

/// Whether a draw enlarges its source: more screen pixels than buffer pixels (by area).
pub fn enlarges(src: Rectangle<f64, BufferCoords>, dst: Rectangle<i32, Physical>) -> bool {
    f64::from(dst.size.w) * f64::from(dst.size.h) > src.size.w * src.size.h * 1.02
}

fn eligible_size(src: Rectangle<f64, BufferCoords>, dst: Rectangle<i32, Physical>) -> bool {
    src.size.w >= f64::from(MIN_SCALE_TEX_DIM)
        && src.size.h >= f64::from(MIN_SCALE_TEX_DIM)
        && dst.size.w >= MIN_SCALE_TEX_DIM
        && dst.size.h >= MIN_SCALE_TEX_DIM
}

/// Computes the element-local EASU render slice in `0..dst.size`.
///
/// Windows within `MAX_EASU_DIM` keep the whole rect so panning stays 100% cached; larger
/// destinations clip to the visible output rectangle plus `OVERSIZED_PAD`. Returns `None` when
/// completely outside the padded viewport.
fn compute_fsr_slice(
    dst: Rectangle<i32, Physical>,
    output_size: (i32, i32),
) -> Option<Rectangle<i32, Physical>> {
    if dst.size.w <= 0 || dst.size.h <= 0 {
        return None;
    }
    if dst.size.w <= MAX_EASU_DIM && dst.size.h <= MAX_EASU_DIM {
        return Some(Rectangle::from_size(dst.size));
    }
    let viewport = Rectangle::new(
        (-OVERSIZED_PAD, -OVERSIZED_PAD).into(),
        (
            output_size.0 + OVERSIZED_PAD * 2,
            output_size.1 + OVERSIZED_PAD * 2,
        )
            .into(),
    );
    let visible = dst.intersection(viewport)?;
    if visible.is_empty() {
        return None;
    }
    let local = Rectangle::new(visible.loc - dst.loc, visible.size);
    if local.size.w > MAX_EASU_DIM || local.size.h > MAX_EASU_DIM {
        let cw = local.size.w.min(MAX_EASU_DIM);
        let ch = local.size.h.min(MAX_EASU_DIM);
        Some(Rectangle::new(local.loc, (cw, ch).into()))
    } else {
        Some(local)
    }
}

/// Plans the `(mip_w, mip_h)` sizes needed to halve `src_size` until within `MIP_AREA_THRESHOLD`
/// of `dst_size`.
fn mip_chain_sizes(
    src_size: (i32, i32),
    dst_size: Size<i32, Physical>,
) -> Vec<Size<i32, BufferCoords>> {
    let dst_area = f64::from(dst_size.w) * f64::from(dst_size.h);
    let mut out = Vec::new();
    let (mut cur_w, mut cur_h) = src_size;
    while out.len() < 12 && cur_w > 2 && cur_h > 2 {
        let cur_area = f64::from(cur_w) * f64::from(cur_h);
        if cur_area <= dst_area * MIP_AREA_THRESHOLD {
            break;
        }
        cur_w = (cur_w + 1) / 2;
        cur_h = (cur_h + 1) / 2;
        out.push(Size::from((cur_w, cur_h)));
    }
    out
}

fn surface_texture(element: &OutputElement<GlesRenderer>) -> Option<&GlesTexture> {
    match element {
        OutputElement::Surface(surf) => match surf.inner().texture() {
            WaylandSurfaceTexture::Texture(tex) => Some(tex),
            WaylandSurfaceTexture::SolidColor(_) => None,
        },
        _ => None,
    }
}

#[derive(Clone)]
enum ScaleMode {
    /// 1:1, cursor, border ring, solid element, or `sharp`/`smooth`/`fast` without shaders.
    Plain,
    /// Zoomed-out area-averaging directly from the client's source texture (`<= ~2x` shrink).
    Avg {
        src_tex_size: Size<i32, BufferCoords>,
    },
    /// Zoomed-out area-averaging via cached halving mip level (`mip`).
    Mip {
        mip: GlesTexture,
        use_avg_shader: bool,
    },
    /// Zoomed-in AMD FSR 1 (`EASU` offscreen texture + `RCAS` pass).
    Fsr {
        easu: GlesTexture,
        slice_local: Rectangle<i32, Physical>,
        sharp: f32,
    },
}

impl ScaleMode {
    fn is_plain(&self) -> bool {
        matches!(self, Self::Plain)
    }

    fn damage_halo(&self) -> i32 {
        match self {
            Self::Plain => 0,
            Self::Avg { .. } | Self::Mip { .. } => 2,
            Self::Fsr { .. } => 4,
        }
    }
}

struct ElementTex<'a> {
    id: &'a Id,
    commit: CommitCounter,
    tex: &'a GlesTexture,
    src: Rectangle<f64, BufferCoords>,
    transform: Transform,
    dst: Rectangle<i32, Physical>,
}

fn prepare_fsr_texture(
    renderer: &mut GlesRenderer,
    programs: &ZoomPrograms,
    cache: &mut ZoomCache,
    elem: &ElementTex<'_>,
    slice_local: Rectangle<i32, Physical>,
) -> Option<GlesTexture> {
    let src_key = SrcKey::from_rect(elem.src);
    let epoch = cache.generation;
    if let Some(entry) = cache.fsr.get_mut(elem.id)
        && entry.commit == elem.commit
        && entry.src == src_key
        && entry.transform == elem.transform
        && entry.dst_size == elem.dst.size
        && entry.slice_local == slice_local
    {
        entry.last_used_gen = epoch;
        return Some(entry.easu.clone());
    }

    let buf_size: Size<i32, BufferCoords> = (slice_local.size.w, slice_local.size.h).into();
    let mut easu_tex: GlesTexture = match renderer.create_buffer(Fourcc::Abgr8888, buf_size) {
        Ok(t) => t,
        Err(e) => {
            log_line!("[infynitae] FSR offscreen buffer {buf_size:?} failed: {e:?}");
            return None;
        }
    };
    let render_res: Result<(), GlesError> = (|| {
        let mut target = renderer.bind(&mut easu_tex)?;
        let mut frame = renderer.render(
            &mut target,
            (slice_local.size.w, slice_local.size.h).into(),
            Transform::Normal,
        )?;
        let tex_size = elem.tex.size();
        let uniforms = [
            Uniform::new(
                "u_inv_tex",
                (1.0 / tex_size.w as f32, 1.0 / tex_size.h as f32),
            ),
            Uniform::new(
                "u_dst_size",
                (elem.dst.size.w as f32, elem.dst.size.h as f32),
            ),
        ];
        let virtual_dst = Rectangle::new(
            (-slice_local.loc.x, -slice_local.loc.y).into(),
            elem.dst.size,
        );
        let damage_rect = Rectangle::new(slice_local.loc, slice_local.size);
        frame.render_texture_from_to(
            elem.tex,
            elem.src,
            virtual_dst,
            &[damage_rect],
            &[damage_rect],
            elem.transform,
            1.0,
            Some(&programs.easu),
            &uniforms,
        )?;
        let _sync = frame.finish()?;
        Ok(())
    })();

    if let Err(e) = render_res {
        log_line!("[infynitae] FSR EASU pass failed: {e:?}");
        return None;
    }

    cache.fsr.insert(
        elem.id.clone(),
        FsrEntry {
            commit: elem.commit,
            src: src_key,
            transform: elem.transform,
            dst_size: elem.dst.size,
            slice_local,
            easu: easu_tex.clone(),
            last_used_gen: epoch,
        },
    );
    Some(easu_tex)
}

fn prepare_mip_texture(
    renderer: &mut GlesRenderer,
    programs: &ZoomPrograms,
    cache: &mut ZoomCache,
    elem: &ElementTex<'_>,
) -> Option<GlesTexture> {
    let desired = mip_chain_sizes(
        (
            elem.src.size.w.round().max(1.0) as i32,
            elem.src.size.h.round().max(1.0) as i32,
        ),
        elem.dst.size,
    );
    if desired.is_empty() {
        return None;
    }

    let src_key = SrcKey::from_rect(elem.src);
    let epoch = cache.generation;
    let mut levels = match cache.mips.get_mut(elem.id) {
        Some(entry) if entry.commit == elem.commit && entry.src == src_key => {
            entry.last_used_gen = epoch;
            if entry.levels.len() >= desired.len() {
                return Some(entry.levels[desired.len() - 1].clone());
            }
            std::mem::take(&mut entry.levels)
        }
        _ => Vec::new(),
    };

    for &next_size in desired.iter().skip(levels.len()) {
        let mut next_tex: GlesTexture = match renderer.create_buffer(Fourcc::Abgr8888, next_size) {
            Ok(t) => t,
            Err(e) => {
                log_line!("[infynitae] Mip buffer {next_size:?} failed: {e:?}");
                return levels.last().cloned();
            }
        };
        let prev_tex = levels.last().unwrap_or(elem.tex);
        let prev_src = match levels.last() {
            Some(prev) => {
                let s = prev.size();
                Rectangle::from_size((f64::from(s.w), f64::from(s.h)).into())
            }
            None => elem.src,
        };
        let render_res: Result<(), GlesError> = (|| {
            let mut target = renderer.bind(&mut next_tex)?;
            let mut frame = renderer.render(
                &mut target,
                (next_size.w, next_size.h).into(),
                Transform::Normal,
            )?;
            let pts = prev_tex.size();
            let uniforms = [
                Uniform::new("u_inv_tex", (1.0 / pts.w as f32, 1.0 / pts.h as f32)),
                Uniform::new("u_dst_size", (next_size.w as f32, next_size.h as f32)),
            ];
            let rect = Rectangle::from_size((next_size.w, next_size.h).into());
            frame.render_texture_from_to(
                prev_tex,
                prev_src,
                rect,
                &[rect],
                &[rect],
                Transform::Normal,
                1.0,
                Some(&programs.downsample),
                &uniforms,
            )?;
            let _sync = frame.finish()?;
            Ok(())
        })();
        if let Err(e) = render_res {
            log_line!("[infynitae] Mip downsample pass failed: {e:?}");
            return levels.last().cloned();
        }
        levels.push(next_tex);
    }

    let chosen = levels.get(desired.len() - 1).cloned()?;
    cache.mips.insert(
        elem.id.clone(),
        MipEntry {
            commit: elem.commit,
            src: src_key,
            levels,
            last_used_gen: epoch,
        },
    );
    Some(chosen)
}

/// Prepares `elements` for rendering/screencopy on an output of `output_size` at zoom `k`,
/// running any missing cached mip downsample or FSR EASU passes into `cache`.
#[allow(clippy::too_many_arguments)]
pub fn prepare_elements(
    renderer: &mut GlesRenderer,
    rounding: &HashMap<Id, Rounding>,
    programs: Option<&Arc<ZoomPrograms>>,
    cache: &mut ZoomCache,
    elements: Vec<OutputElement<GlesRenderer>>,
    k: f64,
    output_size: (i32, i32),
    filter: ZoomFilter,
) -> Vec<Smoothed<OutputElement<GlesRenderer>>> {
    let scale = Scale::from(k);
    elements
        .into_iter()
        .map(|elem| {
            let round = rounding.get(Element::id(&elem)).copied();
            let mut smoothed = Smoothed::new(elem, programs.cloned()).rounded(round);
            let Some(progs) = programs else {
                return smoothed;
            };
            if smoothed.inner.kind() == Kind::Cursor
                || smoothed.round.as_ref().is_some_and(|r| r.border.is_some())
            {
                return smoothed;
            }
            let src = smoothed.inner.src();
            let dst = smoothed.inner.geometry(scale);
            if !eligible_size(src, dst) {
                return smoothed;
            }
            let Some(src_tex) = surface_texture(&smoothed.inner) else {
                return smoothed;
            };
            let src_tex_size = src_tex.size();
            let id = Element::id(&smoothed.inner).clone();
            let commit = Element::current_commit(&smoothed.inner);
            let transform = smoothed.inner.transform();

            let elem = ElementTex {
                id: &id,
                commit,
                tex: src_tex,
                src,
                transform,
                dst,
            };

            if filter.is_fsr() && enlarges(src, dst) {
                if let Some(slice_local) = compute_fsr_slice(dst, output_size)
                    && let Some(easu) =
                        prepare_fsr_texture(renderer, progs, cache, &elem, slice_local)
                {
                    smoothed.mode = ScaleMode::Fsr {
                        easu,
                        slice_local,
                        sharp: filter.rcas_sharpness(),
                    };
                }
            } else if filter.averages_zoom_out() && shrinks(src, dst) {
                if let Some(mip) = prepare_mip_texture(renderer, progs, cache, &elem) {
                    let ms = mip.size();
                    let mip_src = Rectangle::from_size((f64::from(ms.w), f64::from(ms.h)).into());
                    let use_avg_shader = shrinks(mip_src, dst);
                    smoothed.mode = ScaleMode::Mip {
                        mip,
                        use_avg_shader,
                    };
                } else {
                    smoothed.mode = ScaleMode::Avg { src_tex_size };
                }
            }

            smoothed
        })
        .collect()
}

/// An element drawn with cached zoomed-out averaging or FSR 1 upscaling when zoomed, and plain
/// (or rounded) filtering at 1:1.
pub struct Smoothed<E> {
    inner: E,
    programs: Option<Arc<ZoomPrograms>>,
    mode: ScaleMode,
    /// A rounded window's surface, or a piece of its border (`corners.rs`).
    round: Option<Rounding>,
}

impl<E> Smoothed<E> {
    pub fn new(inner: E, programs: Option<Arc<ZoomPrograms>>) -> Self {
        Self {
            inner,
            programs,
            mode: ScaleMode::Plain,
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
        let damage = self.inner.damage_since(scale, commit);
        let halo = self.mode.damage_halo();
        if halo == 0 || damage.is_empty() {
            return damage;
        }
        let bounds = Rectangle::from_size(self.inner.geometry(scale).size);
        damage
            .into_iter()
            .filter_map(|r| {
                Rectangle::new(
                    (r.loc.x - halo, r.loc.y - halo).into(),
                    (r.size.w + halo * 2, r.size.h + halo * 2).into(),
                )
                .intersection(bounds)
                .filter(|r| !r.is_empty())
            })
            .collect()
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
        if let Some(round) = &self.round
            && let Some((width, color)) = round.border
        {
            let drawn = CORNERS.with(|c| {
                let c = c.borrow();
                let corners = c.as_ref()?;
                let mut uniforms = shape_uniforms(round, dst);
                uniforms.push(Uniform::new("u_width", width));
                let [r, g, b, a] = color.components();
                uniforms.push(Uniform::new("u_color", (r, g, b, a)));
                Some(frame.render_pixel_shader_to(
                    &corners.ring,
                    Rectangle::from_size((f64::from(dst.size.w), f64::from(dst.size.h)).into()),
                    dst,
                    Size::from((dst.size.w, dst.size.h)),
                    Some(damage),
                    self.inner.alpha(),
                    &uniforms,
                ))
            });
            if let Some(drawn) = drawn {
                return drawn;
            }
        }

        match &self.mode {
            ScaleMode::Fsr {
                easu,
                slice_local,
                sharp,
            } => {
                if let Some(progs) = &self.programs {
                    let slice_bounds = Rectangle::from_size(slice_local.size);
                    let shift_to_slice =
                        |rects: &[Rectangle<i32, Physical>]| -> Vec<Rectangle<i32, Physical>> {
                            rects
                                .iter()
                                .filter_map(|r| {
                                    Rectangle::new(r.loc - slice_local.loc, r.size)
                                        .intersection(slice_bounds)
                                        .filter(|r| !r.is_empty())
                                })
                                .collect()
                        };
                    let local_damage = shift_to_slice(damage);
                    if local_damage.is_empty() {
                        return Ok(());
                    }
                    let local_opaque = shift_to_slice(opaque_regions);
                    let dst_slice = Rectangle::new(dst.loc + slice_local.loc, slice_local.size);
                    let es = easu.size();
                    let easu_src = Rectangle::from_size((f64::from(es.w), f64::from(es.h)).into());
                    let mut uniforms = vec![
                        Uniform::new("u_inv_tex", (1.0 / es.w as f32, 1.0 / es.h as f32)),
                        Uniform::new("u_sharp", *sharp),
                    ];
                    let prog = if let Some(round) = &self.round {
                        uniforms.extend(shape_uniforms(round, dst_slice));
                        &progs.rcas_round
                    } else {
                        &progs.rcas
                    };
                    return frame.render_texture_from_to(
                        easu,
                        easu_src,
                        dst_slice,
                        &local_damage,
                        &local_opaque,
                        Transform::Normal,
                        self.inner.alpha(),
                        Some(prog),
                        &uniforms,
                    );
                }
            }
            ScaleMode::Mip {
                mip,
                use_avg_shader,
            } => {
                let ms = mip.size();
                let mip_src = Rectangle::from_size((f64::from(ms.w), f64::from(ms.h)).into());
                let override_prog = if *use_avg_shader {
                    self.programs.as_ref().map(|progs| {
                        let mut uniforms = vec![
                            Uniform::new("u_inv_tex", (1.0 / ms.w as f32, 1.0 / ms.h as f32)),
                            Uniform::new("u_dst_size", (dst.size.w as f32, dst.size.h as f32)),
                        ];
                        if let Some(round) = &self.round {
                            uniforms.extend(shape_uniforms(round, dst));
                            (progs.avg_round.clone(), uniforms)
                        } else {
                            (progs.avg.clone(), uniforms)
                        }
                    })
                } else if let Some(round) = &self.round {
                    CORNERS.with(|c| {
                        c.borrow()
                            .as_ref()
                            .map(|corners| (corners.round.clone(), shape_uniforms(round, dst)))
                    })
                } else {
                    None
                };

                if let Some((prog, uniforms)) = override_prog {
                    return frame.render_texture_from_to(
                        mip,
                        mip_src,
                        dst,
                        damage,
                        opaque_regions,
                        self.inner.transform(),
                        self.inner.alpha(),
                        Some(&prog),
                        &uniforms,
                    );
                }
                return frame.render_texture_from_to(
                    mip,
                    mip_src,
                    dst,
                    damage,
                    opaque_regions,
                    self.inner.transform(),
                    self.inner.alpha(),
                    None,
                    &[],
                );
            }
            ScaleMode::Avg { src_tex_size } => {
                if let Some(progs) = &self.programs {
                    let mut uniforms = vec![
                        Uniform::new(
                            "u_inv_tex",
                            (1.0 / src_tex_size.w as f32, 1.0 / src_tex_size.h as f32),
                        ),
                        Uniform::new("u_dst_size", (dst.size.w as f32, dst.size.h as f32)),
                    ];
                    let prog = if let Some(round) = &self.round {
                        uniforms.extend(shape_uniforms(round, dst));
                        progs.avg_round.clone()
                    } else {
                        progs.avg.clone()
                    };
                    frame.override_default_tex_program(prog, uniforms);
                    let drawn = self.inner.draw(frame, src, dst, damage, opaque_regions);
                    frame.clear_tex_program_override();
                    return drawn;
                }
            }
            ScaleMode::Plain => {}
        }

        if let Some(round) = &self.round {
            let drawn = CORNERS.with(|c| {
                let c = c.borrow();
                let corners = c.as_ref()?;
                frame.override_default_tex_program(
                    corners.round.clone(),
                    shape_uniforms(round, dst),
                );
                let res = self.inner.draw(frame, src, dst, damage, opaque_regions);
                frame.clear_tex_program_override();
                Some(res)
            });
            if let Some(drawn) = drawn {
                return drawn;
            }
        }
        self.inner.draw(frame, src, dst, damage, opaque_regions)
    }

    fn underlying_storage(&self, renderer: &mut GlesRenderer) -> Option<UnderlyingStorage<'_>> {
        // Direct scanout is untouched for unrounded 1:1 / plain windows.
        if self.round.is_some() || !self.mode.is_plain() {
            return None;
        }
        self.inner.underlying_storage(renderer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_renderer() -> Option<GlesRenderer> {
        use smithay::backend::allocator::gbm::GbmDevice;
        use smithay::backend::drm::DrmDeviceFd;
        use smithay::utils::DeviceFd;

        std::fs::OpenOptions::new()
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
    }

    fn readback_rgba(
        renderer: &mut GlesRenderer,
        target: &mut smithay::backend::renderer::gles::GlesTarget<'_>,
        size: (i32, i32),
    ) -> Vec<u8> {
        use smithay::backend::renderer::ExportMem;
        let mapping = renderer
            .copy_framebuffer(target, Rectangle::from_size(size.into()), Fourcc::Abgr8888)
            .expect("readback");
        renderer.map_texture(&mapping).expect("map").to_vec()
    }

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

    #[test]
    fn only_an_enlarging_draw_is_upscaled() {
        let src = |w, h| Rectangle::<f64, BufferCoords>::from_size((w, h).into());
        let dst = |w, h| Rectangle::<i32, Physical>::from_size((w, h).into());
        assert!(enlarges(src(800.0, 600.0), dst(1600, 1200)), "2x zoomed in");
        assert!(
            enlarges(src(800.0, 600.0), dst(1200, 900)),
            "1.5x zoomed in"
        );
        assert!(!enlarges(src(800.0, 600.0), dst(800, 600)), "1:1 fast path");
        assert!(
            !enlarges(src(800.0, 600.0), dst(804, 600)),
            "within 2% tolerance"
        );
        assert!(!enlarges(src(800.0, 600.0), dst(400, 300)), "zoomed out");
        assert!(
            enlarges(src(600.0, 800.0), dst(1600, 1200)),
            "rotated zoomed in"
        );
        assert!(
            !eligible_size(src(1.0, 1.0), dst(800, 600)),
            "1x1 spacer skipped"
        );
        assert!(eligible_size(src(64.0, 64.0), dst(128, 128)));
    }

    #[test]
    fn fsr_slice_and_mip_chain_handle_oversized_and_deep_zoom() {
        let normal = Rectangle::new((100, 50).into(), (3840, 2160).into());
        assert_eq!(
            compute_fsr_slice(normal, (1920, 1080)),
            Some(Rectangle::from_size((3840, 2160).into())),
            "<= 4096 caches full window"
        );

        let huge = Rectangle::new((-1000, -500).into(), (7680, 4320).into());
        assert_eq!(
            compute_fsr_slice(huge, (1920, 1080)),
            Some(Rectangle::new((984, 484).into(), (1952, 1112).into())),
            "oversized clips to padded output slice"
        );

        let offscreen = Rectangle::new((5000, 0).into(), (6000, 6000).into());
        assert_eq!(compute_fsr_slice(offscreen, (1920, 1080)), None);

        assert!(
            mip_chain_sizes((1920, 1080), (1440, 810).into()).is_empty(),
            "0.75x needs no mip"
        );
        assert_eq!(
            mip_chain_sizes((1920, 1080), (960, 540).into()),
            vec![Size::from((960, 540))],
            "0.5x builds L1"
        );
        assert_eq!(
            mip_chain_sizes((1920, 1080), (480, 270).into()),
            vec![Size::from((960, 540)), Size::from((480, 270))],
            "0.25x builds L1, L2"
        );
        assert_eq!(
            mip_chain_sizes((1920, 1080), (240, 135).into()),
            vec![
                Size::from((960, 540)),
                Size::from((480, 270)),
                Size::from((240, 135)),
            ],
            "0.125x builds L1..L3"
        );
    }

    #[test]
    fn zoom_programs_compile_on_gles() {
        let Some(mut renderer) = test_renderer() else {
            eprintln!(
                "no /dev/dri/renderD128 GPU render node in environment: shader compile test skipped"
            );
            return;
        };
        let progs = ZoomPrograms::compile(&mut renderer).expect(
            "all zoom shaders (avg, avg_round, downsample, easu, rcas, rcas_round) compile",
        );
        assert!(init_corners(&mut renderer));
        drop(progs);
    }

    /// On the GPU (the first render node; skipped without one): one-pixel white lines every 4
    /// pixels, drawn at 1/4 and 1/8 via the cached downsample pyramid + averaging shader.
    #[test]
    fn a_window_drawn_at_a_quarter_and_eighth_keeps_its_thin_lines() {
        use smithay::backend::renderer::TextureFilter;
        use smithay::backend::renderer::gles::GlesRenderbuffer;
        use smithay::backend::renderer::{ImportMem, Renderer};

        let Some(mut renderer) = test_renderer() else {
            eprintln!(
                "no /dev/dri/renderD128 GPU render node in environment: zoomed-out test skipped"
            );
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
        let progs = ZoomPrograms::compile(&mut renderer).expect("shaders compile");
        let _ = renderer.upscale_filter(TextureFilter::Nearest);
        let id = Id::new();
        let src_rect = Rectangle::from_size((w as f64, h as f64).into());

        // Compare plain sampling vs cached mipmap draw at 1/4 and 1/8.
        let draw_plain = |renderer: &mut GlesRenderer, dw: i32, dh: i32| -> Vec<f64> {
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
                Frame::render_texture_from_to(
                    &mut frame,
                    &texture,
                    src_rect,
                    dst,
                    &[dst],
                    &[],
                    Transform::Normal,
                    1.0,
                )
                .unwrap();
                frame.finish().unwrap().wait().unwrap();
            }
            readback_rgba(renderer, &mut target, (dw, dh))
                .chunks(4)
                .map(|p| f64::from(p[0]) / 255.0)
                .collect()
        };

        let mut cache = ZoomCache::default();
        let draw_mip = |renderer: &mut GlesRenderer,
                        cache: &mut ZoomCache,
                        dw: i32,
                        dh: i32,
                        commit: usize|
         -> Vec<f64> {
            let mut cc = CommitCounter::default();
            for _ in 0..commit {
                cc.increment();
            }
            let dst = Rectangle::<i32, Physical>::from_size((dw, dh).into());
            let elem = ElementTex {
                id: &id,
                commit: cc,
                tex: &texture,
                src: src_rect,
                transform: Transform::Normal,
                dst,
            };
            let mip = prepare_mip_texture(renderer, &progs, cache, &elem).expect("mip level built");
            let mut buffer: GlesRenderbuffer = renderer
                .create_buffer(Fourcc::Abgr8888, (dw, dh).into())
                .expect("buffer");
            let mut target = renderer.bind(&mut buffer).expect("bind");
            {
                let mut frame = renderer
                    .render(&mut target, (dw, dh).into(), Transform::Normal)
                    .expect("frame");
                frame.clear([0.0, 0.0, 0.0, 1.0].into(), &[dst]).unwrap();
                let ms = mip.size();
                Frame::render_texture_from_to(
                    &mut frame,
                    &mip,
                    Rectangle::from_size((f64::from(ms.w), f64::from(ms.h)).into()),
                    dst,
                    &[dst],
                    &[],
                    Transform::Normal,
                    1.0,
                )
                .unwrap();
                frame.finish().unwrap().wait().unwrap();
            }
            readback_rgba(renderer, &mut target, (dw, dh))
                .chunks(4)
                .map(|p| f64::from(p[0]) / 255.0)
                .collect()
        };

        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        let plain_q = draw_plain(&mut renderer, 64, 16);
        let mip_q = draw_mip(&mut renderer, &mut cache, 64, 16, 1);
        assert!(
            mip_q.iter().all(|v| (v - 0.25).abs() < 0.04),
            "1/4 cached mip keeps every pixel ~0.25, got {:.3}..{:.3}",
            mip_q.iter().cloned().fold(1.0, f64::min),
            mip_q.iter().cloned().fold(0.0, f64::max)
        );
        assert!(
            (mean(&plain_q) - 0.25).abs() > 0.1,
            "control: plain filtering loses 1px lines at 1/4 (mean {:.3})",
            mean(&plain_q)
        );

        // Reuse cached pyramid when deepening to 1/8 with the same CommitCounter.
        let mip_e = draw_mip(&mut renderer, &mut cache, 32, 8, 1);
        assert!(
            mip_e.iter().all(|v| (v - 0.25).abs() < 0.04),
            "1/8 cached mip keeps every pixel ~0.25, got {:.3}..{:.3}",
            mip_e.iter().cloned().fold(1.0, f64::min),
            mip_e.iter().cloned().fold(0.0, f64::max)
        );
        assert_eq!(cache.mips.get(&id).unwrap().levels.len(), 3);

        // Changing CommitCounter invalidates and rebuilds at the requested depth.
        let mip_q2 = draw_mip(&mut renderer, &mut cache, 64, 16, 2);
        assert_eq!(cache.mips.get(&id).unwrap().levels.len(), 2);
        assert!(mip_q2.iter().all(|v| (v - 0.25).abs() < 0.04));
    }

    #[test]
    fn fsr_and_fsr_sharp_upscale_edges_without_overshoot_or_alpha_halo() {
        use smithay::backend::renderer::TextureFilter;
        use smithay::backend::renderer::gles::GlesRenderbuffer;
        use smithay::backend::renderer::{ImportMem, Renderer};

        let Some(mut renderer) = test_renderer() else {
            eprintln!(
                "no /dev/dri/renderD128 GPU render node in environment: FSR GPU test skipped"
            );
            return;
        };
        let progs = ZoomPrograms::compile(&mut renderer).expect("shaders compile");

        // 32x32 input: left half white opaque, right half black opaque, plus a transparent
        // bottom band with premultiplied alpha=0 on white/black border to check alpha invariant.
        let (sw, sh) = (32, 32);
        let mut pixels = vec![0u8; sw * sh * 4];
        for y in 0..sh {
            for x in 0..sw {
                let idx = (y * sw + x) * 4;
                if y >= 24 {
                    // Semitransparent premultiplied foreground on left, empty on right
                    if x < 16 {
                        pixels[idx..idx + 4].copy_from_slice(&[128, 128, 128, 128]);
                    } else {
                        pixels[idx..idx + 4].copy_from_slice(&[0, 0, 0, 0]);
                    }
                } else if x < 16 {
                    pixels[idx..idx + 4].copy_from_slice(&[255, 255, 255, 255]);
                } else {
                    pixels[idx..idx + 4].copy_from_slice(&[0, 0, 0, 255]);
                }
            }
        }
        let tex = renderer
            .import_memory(
                &pixels,
                Fourcc::Abgr8888,
                (sw as i32, sh as i32).into(),
                false,
            )
            .expect("texture");

        let (dw, dh) = (128, 128);
        let dst = Rectangle::<i32, Physical>::from_size((dw, dh).into());
        let src_rect = Rectangle::from_size((sw as f64, sh as f64).into());

        // Measure middle opaque edge row (y = 32 -> src y = 8) transition slope across Smooth, Fsr, FsrSharp.
        let measure_edge_slope = |renderer: &mut GlesRenderer, mode: ZoomFilter| -> (f64, bool) {
            let mut cache = ZoomCache::default();
            let mut buf: GlesRenderbuffer = renderer
                .create_buffer(Fourcc::Abgr8888, (dw, dh).into())
                .expect("buf");
            let mut alpha_ok = true;
            let row = if mode.is_fsr() {
                let id = Id::new();
                let elem = ElementTex {
                    id: &id,
                    commit: CommitCounter::default(),
                    tex: &tex,
                    src: src_rect,
                    transform: Transform::Normal,
                    dst,
                };
                let easu = prepare_fsr_texture(renderer, &progs, &mut cache, &elem, dst)
                    .expect("easu pass");
                let mut target = renderer.bind(&mut buf).expect("bind");
                {
                    let mut frame = renderer
                        .render(&mut target, (dw, dh).into(), Transform::Normal)
                        .expect("frame");
                    frame.clear([0.0, 0.0, 0.0, 0.0].into(), &[dst]).unwrap();
                    let uniforms = [
                        Uniform::new("u_inv_tex", (1.0 / dw as f32, 1.0 / dh as f32)),
                        Uniform::new("u_sharp", mode.rcas_sharpness()),
                    ];
                    frame.override_default_tex_program(progs.rcas.clone(), uniforms.to_vec());
                    Frame::render_texture_from_to(
                        &mut frame,
                        &easu,
                        Rectangle::from_size((f64::from(dw), f64::from(dh)).into()),
                        dst,
                        &[dst],
                        &[],
                        Transform::Normal,
                        1.0,
                    )
                    .unwrap();
                    frame.clear_tex_program_override();
                    frame.finish().unwrap().wait().unwrap();
                }
                readback_rgba(renderer, &mut target, (dw, dh))
            } else {
                let _ = renderer.upscale_filter(if mode.uses_nearest_upscale() {
                    TextureFilter::Nearest
                } else {
                    TextureFilter::Linear
                });
                let mut target = renderer.bind(&mut buf).expect("bind");
                {
                    let mut frame = renderer
                        .render(&mut target, (dw, dh).into(), Transform::Normal)
                        .expect("frame");
                    frame.clear([0.0, 0.0, 0.0, 0.0].into(), &[dst]).unwrap();
                    Frame::render_texture_from_to(
                        &mut frame,
                        &tex,
                        src_rect,
                        dst,
                        &[dst],
                        &[],
                        Transform::Normal,
                        1.0,
                    )
                    .unwrap();
                    frame.finish().unwrap().wait().unwrap();
                }
                readback_rgba(renderer, &mut target, (dw, dh))
            };

            for px in row.as_chunks::<4>().0 {
                if px[0] > px[3] || px[1] > px[3] || px[2] > px[3] {
                    alpha_ok = false;
                }
            }
            let r = 32usize * dw as usize * 4;
            let row_vals: Vec<f64> = (0..dw as usize)
                .map(|x| f64::from(row[r + x * 4]) / 255.0)
                .collect();
            let max_step = row_vals
                .windows(2)
                .map(|w| (w[0] - w[1]).abs())
                .fold(0.0_f64, f64::max);
            (max_step, alpha_ok)
        };

        let (smooth_step, _) = measure_edge_slope(&mut renderer, ZoomFilter::Smooth);
        let (fsr_step, fsr_alpha_ok) = measure_edge_slope(&mut renderer, ZoomFilter::Fsr);
        let (fsr_sharp_step, fsr_sharp_alpha_ok) =
            measure_edge_slope(&mut renderer, ZoomFilter::FsrSharp);

        assert!(
            fsr_alpha_ok && fsr_sharp_alpha_ok,
            "RCAS kept premultiplied alpha invariant"
        );
        assert!(
            fsr_step > smooth_step * 1.15,
            "FSR edge step {fsr_step:.3} must be steeper than bilinear {smooth_step:.3}"
        );
        assert!(
            fsr_sharp_step >= fsr_step,
            "FSR sharp ({fsr_sharp_step:.3}) must be at least as sharp as FSR ({fsr_step:.3})"
        );
    }

    #[test]
    fn one_to_one_fast_path_has_plain_mode_and_zero_halo() {
        let src_1080 = Rectangle::<f64, BufferCoords>::from_size((1920.0, 1080.0).into());
        let dst_1080 = Rectangle::<i32, Physical>::from_size((1920, 1080).into());
        let dst_tol = Rectangle::<i32, Physical>::from_size((1928, 1084).into());
        assert!(!enlarges(src_1080, dst_1080));
        assert!(!shrinks(src_1080, dst_1080));
        assert!(!enlarges(src_1080, dst_tol));
        assert!(!shrinks(src_1080, dst_tol));

        assert!(ScaleMode::Plain.is_plain());
        assert_eq!(ScaleMode::Plain.damage_halo(), 0);
        let avg = ScaleMode::Avg {
            src_tex_size: Size::from((1920, 1080)),
        };
        assert!(!avg.is_plain());
        assert_eq!(avg.damage_halo(), 2);

        let mut cache = ZoomCache::default();
        cache.gc();
        assert!(cache.fsr.is_empty() && cache.mips.is_empty());
    }
}
