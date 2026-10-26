//! Touch (`wl_touch`): the fingers on a touchscreen, each in its slot. A finger's first contact is
//! hit-tested like a click (layer surfaces above the Canvas, then the Canvas, then below it) and
//! the surface it lands on keeps that finger until it lifts: its motion is delivered there even
//! outside the surface, as `wl_touch` requires. A contact on empty canvas goes to no client.
//!
//! The location of a later motion is found the way the first contact's was: on the Canvas, the
//! position unprojected through the output's view (a window's surface coordinates are Canvas
//! units); on a layer surface, the position less the surface's place on the output (its events
//! are surface-local, never zoomed).

use crate::output::OutputId;
use crate::protocol::PointerTarget;

/// How a finger's later positions become the location delivered to its surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Anchor {
    /// A surface on the Canvas: the position unprojected through the view.
    Canvas,
    /// A layer surface: `offset` is its origin on the output (output logical px).
    Layer { offset: (f64, f64) },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TouchPoint {
    pub output: OutputId,
    pub target: PointerTarget,
    pub anchor: Anchor,
}

/// The fingers down on a surface, by slot, and the ones on empty canvas.
#[derive(Clone, Debug, Default)]
pub(crate) struct Touches {
    points: Vec<(u32, TouchPoint)>,
    pub canvas: Option<CanvasGesture>,
}

/// Fingers on empty canvas move their output's camera: the Canvas point under their centroid
/// stays under it (one finger pans), and with two or more the zoom follows their spread. The grab
/// is taken again whenever a finger joins or lifts, so the view never jumps.
#[derive(Clone, Debug)]
pub(crate) struct CanvasGesture {
    pub output: OutputId,
    pub camera: usize,
    /// Each finger's position, output logical px.
    pub fingers: Vec<(u32, (f64, f64))>,
    /// `None`: to be taken from the camera at the next motion.
    pub grab: Option<Grab>,
}

/// Where a gesture took hold: the Canvas point under the centroid (Canvas px), the zoom then,
/// and the fingers' spread then.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Grab {
    anchor: (f64, f64),
    zoom: f64,
    spread: f64,
}

/// The fingers' centroid and their mean distance from it (0 for one finger).
fn centroid_spread(fingers: &[(u32, (f64, f64))]) -> ((f64, f64), f64) {
    let n = fingers.len().max(1) as f64;
    let c = fingers
        .iter()
        .fold((0.0, 0.0), |a, (_, p)| (a.0 + p.0 / n, a.1 + p.1 / n));
    let spread = fingers
        .iter()
        .map(|(_, p)| (p.0 - c.0).hypot(p.1 - c.1))
        .sum::<f64>()
        / n;
    (c, spread)
}

impl Grab {
    /// The grab of `fingers` on a camera at `center` (Canvas px) and `zoom`, on an output of
    /// logical size `extent`.
    pub(crate) fn take(
        fingers: &[(u32, (f64, f64))],
        center: (f64, f64),
        zoom: f64,
        extent: (f64, f64),
    ) -> Grab {
        let (c, spread) = centroid_spread(fingers);
        Grab {
            anchor: (
                center.0 + (c.0 - extent.0 / 2.0) / zoom,
                center.1 + (c.1 - extent.1 / 2.0) / zoom,
            ),
            zoom,
            spread,
        }
    }

    /// The camera's zoom and centre (Canvas px) that keep the anchor under the fingers now.
    pub(crate) fn camera(
        &self,
        fingers: &[(u32, (f64, f64))],
        extent: (f64, f64),
    ) -> (f64, (f64, f64)) {
        let (c, spread) = centroid_spread(fingers);
        let zoom = if fingers.len() >= 2 && self.spread >= 1.0 {
            (self.zoom * spread / self.spread).clamp(crate::geom::ZOOM_MIN, crate::geom::ZOOM_MAX)
        } else {
            self.zoom
        };
        (
            zoom,
            (
                self.anchor.0 - (c.0 - extent.0 / 2.0) / zoom,
                self.anchor.1 - (c.1 - extent.1 / 2.0) / zoom,
            ),
        )
    }
}

impl Touches {
    pub(crate) fn get(&self, slot: u32) -> Option<TouchPoint> {
        self.points
            .iter()
            .find(|(s, _)| *s == slot)
            .map(|(_, p)| *p)
    }

    pub(crate) fn insert(&mut self, slot: u32, point: TouchPoint) {
        self.points.retain(|(s, _)| *s != slot);
        self.points.push((slot, point));
    }

    /// The finger lifted: whether it was on a surface. A finger on the canvas lets go of it.
    pub(crate) fn remove(&mut self, slot: u32) -> bool {
        if let Some(g) = &mut self.canvas
            && g.fingers.iter().any(|(s, _)| *s == slot)
        {
            g.fingers.retain(|(s, _)| *s != slot);
            g.grab = None;
            if g.fingers.is_empty() {
                self.canvas = None;
            }
        }
        let before = self.points.len();
        self.points.retain(|(s, _)| *s != slot);
        self.points.len() != before
    }

    /// Every finger is forgotten: whether any was on a surface.
    pub(crate) fn clear(&mut self) -> bool {
        self.canvas = None;
        let any = !self.points.is_empty();
        self.points.clear();
        any
    }

    /// Whether a finger is down on `output`.
    pub(crate) fn on_output(&self, output: OutputId) -> bool {
        self.points.iter().any(|(_, p)| p.output == output)
            || self.canvas.as_ref().is_some_and(|g| g.output == output)
    }

    /// A finger on empty canvas of `output`, whose camera is `camera`: it joins the gesture there.
    /// `false` while a gesture holds another output's canvas (the finger is ignored).
    pub(crate) fn canvas_down(
        &mut self,
        slot: u32,
        output: OutputId,
        camera: usize,
        p: (f64, f64),
    ) -> bool {
        let g = self.canvas.get_or_insert_with(|| CanvasGesture {
            output,
            camera,
            fingers: Vec::new(),
            grab: None,
        });
        if g.output != output {
            return false;
        }
        g.fingers.push((slot, p));
        g.grab = None;
        true
    }

    /// Whether `slot` is a finger on the canvas.
    pub(crate) fn on_canvas(&self, slot: u32) -> bool {
        self.canvas
            .as_ref()
            .is_some_and(|g| g.fingers.iter().any(|(s, _)| *s == slot))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXTENT: (f64, f64) = (1920.0, 1080.0);

    #[test]
    fn one_finger_keeps_its_canvas_point_under_it() {
        let g = Grab::take(&[(0, (100.0, 100.0))], (0.0, 0.0), 2.0, EXTENT);
        let (zoom, center) = g.camera(&[(0, (150.0, 80.0))], EXTENT);
        assert_eq!(zoom, 2.0, "one finger never zooms");
        // the finger moved (+50, -20) px on screen: the camera moved the other way, /zoom
        assert_eq!(center, (-25.0, 10.0));
    }

    #[test]
    fn a_pinch_zooms_by_the_spread_around_the_centroid() {
        let start = [(0, (860.0, 540.0)), (1, (1060.0, 540.0))];
        let g = Grab::take(&start, (500.0, 300.0), 1.0, EXTENT);
        let (zoom, center) = g.camera(&[(0, (760.0, 540.0)), (1, (1160.0, 540.0))], EXTENT);
        assert_eq!(zoom, 2.0, "twice the spread: twice the zoom");
        assert_eq!(
            center,
            (500.0, 300.0),
            "the centroid stayed: so did its Canvas point"
        );
        // the anchor stays under the centroid when the fingers also move
        let moved = [(0, (700.0, 600.0)), (1, (1100.0, 600.0))];
        let (zoom, center) = g.camera(&moved, EXTENT);
        let under = (
            center.0 + (900.0 - EXTENT.0 / 2.0) / zoom,
            center.1 + (600.0 - EXTENT.1 / 2.0) / zoom,
        );
        assert_eq!(under, (500.0, 300.0));
    }

    #[test]
    fn the_zoom_stays_in_its_domain() {
        let g = Grab::take(
            &[(0, (0.0, 0.0)), (1, (2.0, 0.0))],
            (0.0, 0.0),
            crate::geom::ZOOM_MAX,
            EXTENT,
        );
        let (zoom, _) = g.camera(&[(0, (0.0, 0.0)), (1, (1000.0, 0.0))], EXTENT);
        assert_eq!(zoom, crate::geom::ZOOM_MAX);
    }

    #[test]
    fn a_lift_lets_go_and_the_last_ends_the_gesture() {
        let mut t = Touches::default();
        let o = OutputId::for_test(1);
        assert!(t.canvas_down(0, o, 0, (0.0, 0.0)));
        assert!(t.canvas_down(1, o, 0, (10.0, 0.0)));
        assert!(
            !t.canvas_down(2, OutputId::for_test(2), 1, (0.0, 0.0)),
            "another output's canvas"
        );
        t.canvas.as_mut().unwrap().grab =
            Some(Grab::take(&[(0, (0.0, 0.0))], (0.0, 0.0), 1.0, EXTENT));
        assert!(!t.remove(1), "a canvas finger was on no surface");
        assert_eq!(
            t.canvas.as_ref().unwrap().grab,
            None,
            "the grab is taken again"
        );
        assert!(t.on_output(o));
        t.remove(0);
        assert!(t.canvas.is_none());
    }
}
