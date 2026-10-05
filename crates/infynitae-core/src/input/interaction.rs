//! OBJ-26: the interaction state machine (M22), CameraPan only (M6). Pure state and transitions;
//! `entry::physical_input` applies the camera changes they decide.

use crate::geom::CanvasPos;
use crate::output::OutputId;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Interaction {
    /// No compositor interaction: the pointer is client-routed.
    #[default]
    Idle,
    /// Direct-manipulation camera pan (M6) on `output`'s camera.
    CameraPan {
        output: OutputId,
        camera: usize,
        /// The press that acquired the pan; only its release ends it.
        initiator: u32,
        /// `C_start` (F-PAN-1).
        start: CanvasPos,
        /// Σ dp_phys / (s_O · z_C) in Canvas px, summed in f64 from the gesture origin.
        sum: (f64, f64),
    },
    /// The pan's output was removed (C4.6): motion is consumed until the initiator is released.
    Orphaned { initiator: u32 },
}

impl Interaction {
    /// Whether a compositor interaction owns the pointer (KeepFocus, INV-INPUT-3).
    pub fn active(&self) -> bool {
        !matches!(self, Interaction::Idle)
    }

    pub fn initiator(&self) -> Option<u32> {
        match *self {
            Interaction::Idle => None,
            Interaction::CameraPan { initiator, .. } | Interaction::Orphaned { initiator } => {
                Some(initiator)
            }
        }
    }

    /// F-PAN-1 accumulation of one motion event on the pan output: the logical delta becomes
    /// `dp_phys = dp_logical · s_O`, and the term `dp_phys / (s_O · z_C)` is added to the f64
    /// sum. Returns the camera, start and new sum, or `None` when no pan is active.
    pub(crate) fn pan_motion(
        &mut self,
        delta: (f64, f64),
        scale: f64,
        zoom: f64,
    ) -> Option<(usize, CanvasPos, (f64, f64))> {
        let Interaction::CameraPan {
            camera, start, sum, ..
        } = self
        else {
            return None;
        };
        let term = |d: f64| (d * scale) / (scale * zoom);
        sum.0 += term(delta.0);
        sum.1 += term(delta.1);
        Some((*camera, *start, *sum))
    }

    /// C4.6: the pan's output was removed.
    pub(crate) fn orphan_if_on(&mut self, removed: OutputId) {
        if let Interaction::CameraPan {
            output, initiator, ..
        } = *self
            && output == removed
        {
            *self = Interaction::Orphaned { initiator };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pan_terms_are_logical_deltas_divided_by_zoom_only() {
        let mut i = Interaction::CameraPan {
            output: OutputId::for_test(1),
            camera: 0,
            initiator: 0x112,
            start: CanvasPos { x: 0, y: 0 },
            sum: (0.0, 0.0),
        };
        // s_O = 1.5, z_C = 1: dp_phys = 15, term = 15 / 1.5 = 10 logical = 10 Canvas px
        assert_eq!(
            i.pan_motion((10.0, -4.0), 1.5, 1.0).unwrap().2,
            (10.0, -4.0)
        );
        assert_eq!(i.pan_motion((2.5, 0.0), 2.0, 1.0).unwrap().2, (12.5, -4.0));
        assert_eq!(Interaction::Idle.initiator(), None);
        i.orphan_if_on(OutputId::for_test(2));
        assert!(matches!(i, Interaction::CameraPan { .. }));
        i.orphan_if_on(OutputId::for_test(1));
        assert_eq!(i, Interaction::Orphaned { initiator: 0x112 });
        assert!(i.active());
    }
}
