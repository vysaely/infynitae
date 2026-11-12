//! Rounded window corners (`WINDOW_CORNERS`, logical px; 0 square): the compositor rounds every
//! window it draws on the desktop with the same soft corner, whatever toolkit drew the window,
//! and its border follows the curve. Desktop-sized on purpose (owner 2026-11-12: "the desktop
//! aesthetic, not just copy the phones"): a small radius and a gentle squircle, no blur.
//!
//! Square where rounding would leave a gap: a window that fills the screen (fullscreen or
//! fitted) is not rounded at all, and any corner that touches the screen's edge stays square
//! (owner: "when the app fills the viewport the corners should be straight"), so a window snapped
//! to half the screen meets the edges with no empty space.
//!
//! This module only decides the shape; the DRM backend draws it (`smooth.rs`), other backends
//! draw windows square.

use smithay::backend::renderer::Color32F;
use smithay::utils::{Physical, Rectangle};

/// How a window is rounded this frame: its geometry in output pixels and each corner's radius
/// (top left, top right, bottom right, bottom left; 0 square).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rounding {
    pub window: Rectangle<i32, Physical>,
    pub radii: [f32; 4],
    /// For a border piece: its width and colour (the ring drawn just outside `window`).
    pub border: Option<(f32, Color32F)>,
}

impl Rounding {
    pub fn is_round(&self) -> bool {
        self.radii.iter().any(|r| *r > 0.0)
    }
}

/// Each corner's radius for a window at `g` (x0, y0, x1, y1, output pixels) on a view of
/// `view` pixels, with `radius` pixels for a free corner: 0 for a corner on the view's edge or
/// beyond it.
pub fn corner_radii(g: (i64, i64, i64, i64), view: (i64, i64), radius: f32) -> [f32; 4] {
    if radius <= 0.0 {
        return [0.0; 4];
    }
    let (x0, y0, x1, y1) = g;
    let (w, h) = view;
    // never more than half the window's shorter side
    let r = radius.min(((x1 - x0).min(y1 - y0) as f32 / 2.0).max(0.0));
    let left = x0 <= 0;
    let top = y0 <= 0;
    let right = x1 >= w;
    let bottom = y1 >= h;
    let pick = |edge_a: bool, edge_b: bool| if edge_a || edge_b { 0.0 } else { r };
    [
        pick(top, left),
        pick(top, right),
        pick(bottom, right),
        pick(bottom, left),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_free_window_is_rounded_on_all_four_corners() {
        assert_eq!(
            corner_radii((100, 100, 900, 700), (1920, 1080), 8.0),
            [8.0; 4]
        );
    }

    #[test]
    fn corners_on_the_screens_edge_stay_square() {
        // snapped to the left half: every corner touches an edge
        assert_eq!(corner_radii((0, 0, 960, 1080), (1920, 1080), 8.0), [0.0; 4]);
        // against the top only: the bottom corners stay round
        assert_eq!(
            corner_radii((100, 0, 900, 700), (1920, 1080), 8.0),
            [0.0, 0.0, 8.0, 8.0]
        );
        // partly off the right edge
        assert_eq!(
            corner_radii((1500, 200, 2100, 700), (1920, 1080), 8.0),
            [8.0, 0.0, 0.0, 8.0]
        );
    }

    #[test]
    fn a_small_window_never_gets_more_than_half_its_side() {
        assert_eq!(
            corner_radii((100, 100, 110, 200), (1920, 1080), 8.0),
            [5.0; 4]
        );
    }

    #[test]
    fn zero_is_square() {
        assert_eq!(
            corner_radii((100, 100, 900, 700), (1920, 1080), 0.0),
            [0.0; 4]
        );
    }
}
