//! Touch (`wl_touch`): the fingers on a touchscreen, each in its slot. A finger's first contact is
//! hit-tested like a click (layer surfaces above the Canvas, then the Canvas, then below it) and
//! the surface it lands on keeps that finger until it lifts: its motion is delivered there even
//! outside the surface, as `wl_touch` requires. A contact on empty canvas goes to no client.
//!
//! The location of a later motion is found the way the first contact's was: on the Canvas, the
//! position unprojected through the output's view (a window's surface coordinates are Canvas
//! units); on a layer surface, the position less the surface's place on the output (its events
//! are surface-local, never zoomed).
//!
//! Two fingers move the canvas, also on a window (owner, 2026-10-27): a finger joining one on a
//! window takes it from the app (its touch is cancelled) into the canvas gesture, and a finger
//! landing while fingers are on the canvas joins them. Fingers on a layer surface (the on-screen
//! keyboard types with two) and on a fullscreen window (games) stay theirs. A quick two-finger
//! tap, twice, recenters like Super+Space.
//!
//! Three fingers on a window move or resize it (owner, 2026-10-28): a third finger joining two
//! whose centroid is on a window takes the gesture from the canvas to that window. Moved
//! together, they carry it (the Canvas point under their centroid stays under it); spread or
//! pinched, they resize it, the width by their horizontal spread and the height by their
//! vertical one, its top-left corner staying put. Which is decided once they have moved
//! `WINDOW_SLOP`; the last lift ends it.

use crate::geom::CanvasPoint;
use crate::output::OutputId;
use crate::protocol::PointerTarget;
use crate::stage::ElementId;

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
    /// Where the finger is now, output logical px (it moves to the canvas from there).
    pub at: (f64, f64),
}

/// A two-finger tap: both fingers down and up within this, moving no more than `TAP_SLOP`.
const TAP_MAX_MS: u32 = 250;
/// How far (output logical px) a tapping finger may move.
const TAP_SLOP: f64 = 16.0;
/// The second tap starts within this of the first's end, near it.
const DOUBLE_TAP_GAP_MS: u32 = 350;
const DOUBLE_TAP_DISTANCE: f64 = 80.0;
/// How far (output logical px) three fingers move, or change their spread, before they move or
/// resize their window.
pub(crate) const WINDOW_SLOP: f64 = 24.0;

/// The end of a two-finger tap, waiting for a second.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Tap {
    output: OutputId,
    end_ms: u32,
    centroid: (f64, f64),
}

/// A finger's lift: whether it was on a surface, and where (output, output logical px) a
/// two-finger double tap it ended was.
pub(crate) struct Lift {
    pub on_surface: bool,
    pub double_tap: Option<(OutputId, (f64, f64))>,
}

/// The fingers down on a surface, by slot, and the ones on empty canvas.
#[derive(Clone, Debug, Default)]
pub(crate) struct Touches {
    points: Vec<(u32, TouchPoint)>,
    pub canvas: Option<CanvasGesture>,
    pub window: Option<WindowGesture>,
    last_tap: Option<Tap>,
    /// A window gesture that was resizing ended: its last configure is to be sent.
    resize_done: bool,
}

/// Three or more fingers on a window: they move or resize it.
#[derive(Clone, Debug)]
pub(crate) struct WindowGesture {
    pub output: OutputId,
    pub element: ElementId,
    /// Each finger's position, output logical px.
    pub fingers: Vec<(u32, (f64, f64))>,
    /// `None`: to be taken from the window at the next motion (a finger joined or lifted).
    pub grab: Option<WindowGrab>,
    pub mode: WindowMode,
}

/// What a window gesture does: nothing yet, until its fingers moved `WINDOW_SLOP`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowMode {
    Undecided,
    Move,
    Resize,
}

/// Where a window gesture took hold: the fingers' centroid and extent (the width and height of
/// the box around them, output logical px), and the window's anchor and size then.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WindowGrab {
    pub centroid: (f64, f64),
    pub extent: (f64, f64),
    pub anchor: CanvasPoint,
    pub size: (i32, i32),
}

/// The width and height of the box around `fingers`.
pub(crate) fn extent(fingers: &[(u32, (f64, f64))]) -> (f64, f64) {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (_, p) in fingers {
        (x0, y0, x1, y1) = (x0.min(p.0), y0.min(p.1), x1.max(p.0), y1.max(p.1));
    }
    if fingers.is_empty() {
        (0.0, 0.0)
    } else {
        (x1 - x0, y1 - y0)
    }
}

impl WindowGesture {
    /// The finger `slot` is at `p` now.
    pub(crate) fn follow(&mut self, slot: u32, p: (f64, f64)) {
        if let Some(f) = self.fingers.iter_mut().find(|(s, _)| *s == slot) {
            f.1 = p;
        }
    }

    /// The fingers' centroid and extent now.
    pub(crate) fn shape(&self) -> ((f64, f64), (f64, f64)) {
        Self::centroid_of(&self.fingers)
    }

    /// `fingers`' centroid and extent.
    pub(crate) fn centroid_of(fingers: &[(u32, (f64, f64))]) -> ((f64, f64), (f64, f64)) {
        (centroid_spread(fingers).0, extent(fingers))
    }
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
    /// For the two-finger tap: when it began, where each finger came down, how many were down
    /// at most, and whether one moved further than a tap.
    started_ms: u32,
    downs: Vec<(u32, (f64, f64))>,
    most: usize,
    moved: bool,
}

impl CanvasGesture {
    /// The finger `slot` is at `p` now.
    pub(crate) fn follow(&mut self, slot: u32, p: (f64, f64)) {
        if let Some(f) = self.fingers.iter_mut().find(|(s, _)| *s == slot) {
            f.1 = p;
        }
        if let Some((_, d)) = self.downs.iter().find(|(s, _)| *s == slot)
            && (p.0 - d.0).hypot(p.1 - d.1) > TAP_SLOP
        {
            self.moved = true;
        }
    }
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

    /// The finger is forgotten: whether it was on a surface. A finger on the canvas lets go of it.
    pub(crate) fn remove(&mut self, slot: u32) -> bool {
        self.lift(slot, None).on_surface
    }

    /// The finger lifted at `time_ms`: whether it was on a surface, and the output and place
    /// (output logical px) of a two-finger double tap it ended.
    pub(crate) fn lift(&mut self, slot: u32, time_ms: Option<u32>) -> Lift {
        let mut double_tap = None;
        if let Some(g) = &mut self.window
            && g.fingers.iter().any(|(s, _)| *s == slot)
        {
            g.fingers.retain(|(s, _)| *s != slot);
            g.grab = None;
            if g.fingers.is_empty() {
                self.end_window();
            }
        }
        if let Some(g) = &mut self.canvas
            && g.fingers.iter().any(|(s, _)| *s == slot)
        {
            g.fingers.retain(|(s, _)| *s != slot);
            g.grab = None;
            if g.fingers.is_empty() {
                let g = self.canvas.take().expect("the gesture just seen");
                if let Some(t) = time_ms {
                    double_tap = self.tapped(&g, t);
                }
            }
        }
        let before = self.points.len();
        self.points.retain(|(s, _)| *s != slot);
        Lift {
            on_surface: self.points.len() != before,
            double_tap,
        }
    }

    /// A gesture ended at `end_ms`: a two-finger tap is kept for the next, and the second of two
    /// is a double tap, at their centroid.
    fn tapped(&mut self, g: &CanvasGesture, end_ms: u32) -> Option<(OutputId, (f64, f64))> {
        let tap = g.most == 2
            && g.downs.len() == 2
            && !g.moved
            && end_ms.wrapping_sub(g.started_ms) <= TAP_MAX_MS;
        if !tap {
            self.last_tap = None;
            return None;
        }
        let (centroid, _) = centroid_spread(&g.downs);
        let second = self.last_tap.take().is_some_and(|first| {
            first.output == g.output
                && g.started_ms.wrapping_sub(first.end_ms) <= DOUBLE_TAP_GAP_MS
                && (centroid.0 - first.centroid.0).hypot(centroid.1 - first.centroid.1)
                    <= DOUBLE_TAP_DISTANCE
        });
        if second {
            return Some((g.output, centroid));
        }
        self.last_tap = Some(Tap {
            output: g.output,
            end_ms,
            centroid,
        });
        None
    }

    /// The finger `slot` on a surface is at `p` now.
    pub(crate) fn surface_finger_at(&mut self, slot: u32, p: (f64, f64)) {
        if let Some((_, tp)) = self.points.iter_mut().find(|(s, _)| *s == slot) {
            tp.at = p;
        }
    }

    /// The fingers on windows of `output`, taken from their surfaces with their places, when
    /// every finger down is one (none on a layer surface, none on another output); `None`
    /// otherwise, and nothing changes.
    pub(crate) fn take_window_fingers(
        &mut self,
        output: OutputId,
    ) -> Option<Vec<(u32, (f64, f64))>> {
        if self.points.is_empty()
            || self
                .points
                .iter()
                .any(|(_, p)| p.output != output || p.anchor != Anchor::Canvas)
        {
            return None;
        }
        Some(self.points.drain(..).map(|(s, p)| (s, p.at)).collect())
    }

    /// Every finger is forgotten: whether any was on a surface.
    pub(crate) fn clear(&mut self) -> bool {
        self.canvas = None;
        self.end_window();
        self.last_tap = None;
        let any = !self.points.is_empty();
        self.points.clear();
        any
    }

    /// Whether a finger is down on `output`.
    pub(crate) fn on_output(&self, output: OutputId) -> bool {
        self.points.iter().any(|(_, p)| p.output == output)
            || self.canvas.as_ref().is_some_and(|g| g.output == output)
            || self.window.as_ref().is_some_and(|g| g.output == output)
    }

    /// A finger on empty canvas of `output`, whose camera is `camera`: it joins the gesture there.
    /// `false` while a gesture holds another output's canvas (the finger is ignored).
    pub(crate) fn canvas_down(
        &mut self,
        slot: u32,
        output: OutputId,
        camera: usize,
        p: (f64, f64),
        time_ms: u32,
    ) -> bool {
        let g = self.canvas.get_or_insert_with(|| CanvasGesture {
            output,
            camera,
            fingers: Vec::new(),
            grab: None,
            started_ms: time_ms,
            downs: Vec::new(),
            most: 0,
            moved: false,
        });
        if g.output != output {
            return false;
        }
        g.fingers.push((slot, p));
        g.downs.push((slot, p));
        g.most = g.most.max(g.fingers.len());
        g.grab = None;
        true
    }

    /// Whether `slot` is a finger on the canvas.
    pub(crate) fn on_canvas(&self, slot: u32) -> bool {
        self.canvas
            .as_ref()
            .is_some_and(|g| g.fingers.iter().any(|(s, _)| *s == slot))
    }

    /// Whether `slot` is a finger of the window gesture.
    pub(crate) fn on_window(&self, slot: u32) -> bool {
        self.window
            .as_ref()
            .is_some_and(|g| g.fingers.iter().any(|(s, _)| *s == slot))
    }

    /// The canvas gesture's fingers move or resize `element` from now on (it ends, and no tap
    /// is counted).
    pub(crate) fn canvas_to_window(&mut self, element: ElementId) {
        let Some(g) = self.canvas.take() else {
            return;
        };
        self.last_tap = None;
        self.end_window();
        self.window = Some(WindowGesture {
            output: g.output,
            element,
            fingers: g.fingers,
            grab: None,
            mode: WindowMode::Undecided,
        });
    }

    /// A finger joins the window gesture on `output`: `false` when there is none there.
    pub(crate) fn window_down(&mut self, slot: u32, output: OutputId, p: (f64, f64)) -> bool {
        let Some(g) = self.window.as_mut().filter(|g| g.output == output) else {
            return false;
        };
        g.fingers.push((slot, p));
        g.grab = None;
        true
    }

    /// The window gesture ends (a resize of it waits for its last configure).
    pub(crate) fn end_window(&mut self) {
        if let Some(g) = self.window.take()
            && g.mode == WindowMode::Resize
        {
            self.resize_done = true;
        }
    }

    /// Whether a window gesture's resize ended since this was last asked.
    pub(crate) fn take_resize_done(&mut self) -> bool {
        std::mem::take(&mut self.resize_done)
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
        assert!(t.canvas_down(0, o, 0, (0.0, 0.0), 0));
        assert!(t.canvas_down(1, o, 0, (10.0, 0.0), 0));
        assert!(
            !t.canvas_down(2, OutputId::for_test(2), 1, (0.0, 0.0), 0),
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

    /// Two fingers down at `ms` and up `held` ms later, `moved` px apart from where they landed.
    fn two_finger_tap(
        t: &mut Touches,
        o: OutputId,
        ms: u32,
        held: u32,
        moved: f64,
    ) -> Option<(OutputId, (f64, f64))> {
        t.canvas_down(0, o, 0, (100.0, 100.0), ms);
        t.canvas_down(1, o, 0, (200.0, 100.0), ms + 10);
        t.canvas.as_mut().unwrap().follow(1, (200.0 + moved, 100.0));
        assert_eq!(t.lift(0, Some(ms + held)).double_tap, None);
        t.lift(1, Some(ms + held + 5)).double_tap
    }

    #[test]
    fn two_quick_two_finger_taps_are_a_double_tap_at_their_centre() {
        let mut t = Touches::default();
        let o = OutputId::for_test(1);
        assert_eq!(two_finger_tap(&mut t, o, 1000, 100, 0.0), None, "the first");
        assert_eq!(
            two_finger_tap(&mut t, o, 1300, 100, 5.0),
            Some((o, (150.0, 100.0)))
        );
        assert_eq!(
            two_finger_tap(&mut t, o, 1600, 100, 0.0),
            None,
            "a third starts over"
        );
    }

    #[test]
    fn a_drag_a_slow_tap_or_a_late_second_tap_is_no_double_tap() {
        let o = OutputId::for_test(1);
        let mut t = Touches::default();
        two_finger_tap(&mut t, o, 1000, 100, 0.0);
        assert_eq!(
            two_finger_tap(&mut t, o, 1300, 100, 40.0),
            None,
            "moved: a pan"
        );
        let mut t = Touches::default();
        two_finger_tap(&mut t, o, 1000, 100, 0.0);
        assert_eq!(
            two_finger_tap(&mut t, o, 1300, 600, 0.0),
            None,
            "held: no tap"
        );
        let mut t = Touches::default();
        two_finger_tap(&mut t, o, 1000, 100, 0.0);
        assert_eq!(two_finger_tap(&mut t, o, 2000, 100, 0.0), None, "too late");
        // one finger, then two: the one-finger gesture is no tap
        let mut t = Touches::default();
        t.canvas_down(0, o, 0, (100.0, 100.0), 0);
        t.lift(0, Some(50));
        assert_eq!(two_finger_tap(&mut t, o, 100, 100, 0.0), None);
    }

    fn on_window(o: OutputId, at: (f64, f64)) -> TouchPoint {
        TouchPoint {
            output: o,
            target: PointerTarget {
                surface: crate::snapshot::SurfaceKey::from_raw(
                    std::num::NonZeroU64::new(1).unwrap(),
                ),
                origin: crate::geom::CanvasPoint { x: 0, y: 0 },
            },
            anchor: Anchor::Canvas,
            at,
        }
    }

    #[test]
    fn fingers_on_windows_are_taken_but_never_one_on_a_layer_surface() {
        let o = OutputId::for_test(1);
        let mut t = Touches::default();
        t.insert(0, on_window(o, (10.0, 10.0)));
        t.surface_finger_at(0, (12.0, 14.0));
        assert_eq!(t.take_window_fingers(o), Some(vec![(0, (12.0, 14.0))]));
        assert!(!t.remove(0), "taken: on no surface any more");

        let mut t = Touches::default();
        t.insert(0, on_window(o, (10.0, 10.0)));
        t.insert(
            1,
            TouchPoint {
                anchor: Anchor::Layer { offset: (0.0, 0.0) },
                ..on_window(o, (5.0, 5.0))
            },
        );
        assert_eq!(
            t.take_window_fingers(o),
            None,
            "the on-screen keyboard keeps its fingers"
        );
        assert!(t.get(0).is_some() && t.get(1).is_some(), "nothing changed");
        assert_eq!(Touches::default().take_window_fingers(o), None);
    }
}
