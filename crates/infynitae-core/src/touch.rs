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

/// The fingers down on a surface, by slot.
#[derive(Clone, Debug, Default)]
pub(crate) struct Touches {
    points: Vec<(u32, TouchPoint)>,
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

    /// The finger lifted: whether it was on a surface.
    pub(crate) fn remove(&mut self, slot: u32) -> bool {
        let before = self.points.len();
        self.points.retain(|(s, _)| *s != slot);
        self.points.len() != before
    }

    /// Every finger is forgotten: whether any was on a surface.
    pub(crate) fn clear(&mut self) -> bool {
        let any = !self.points.is_empty();
        self.points.clear();
        any
    }

    /// Whether a finger is down on `output`.
    pub(crate) fn on_output(&self, output: OutputId) -> bool {
        self.points.iter().any(|(_, p)| p.output == output)
    }
}
