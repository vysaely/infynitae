//! OBJ-26: the interaction state machine (M22): the camera pan (M6), and the user's window move
//! and resize (Super+left / Super+right drag, owner request 2026-10-22, "like Hyprland"). Pure
//! state and transitions; `entry::physical_input` applies the changes they decide.

use crate::geom::CanvasPos;
use crate::output::OutputId;
use crate::stage::ElementId;

/// A camera pan nested in a window move: what [`Interaction::CameraPan`] keeps, plus its button.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MovePan {
    pub output: OutputId,
    pub camera: usize,
    /// The pan's own button; its release ends the pan, not the move.
    pub button: u32,
    /// `C_start` (F-PAN-1).
    pub start: CanvasPos,
    /// Σ dp_phys / (s_O · z_C) in Canvas px.
    pub sum: (f64, f64),
}

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
    /// The pan's output was removed (C4.6), or the moved or resized window was destroyed: motion
    /// is consumed until the initiator is released.
    Orphaned { initiator: u32 },
    /// WindowMove (Super+left drag, or the client's `xdg_toplevel.move`): F-DRAG-1 as
    /// decided for OB-34 (owner, 2026-10-22), `anchor = R3(q_now − grab_offset)` — the grabbed
    /// point stays under the pointer, on whichever output it is.
    ElementMove {
        element: ElementId,
        initiator: u32,
        /// `q_press − anchor₀`.
        grab_offset: CanvasPos,
        /// Super+middle pressed during the move (owner request 2026-10-22): the camera pans under
        /// the fixed pointer and the window, still `q_now − grab_offset`, travels with it.
        pan: Option<MovePan>,
        /// Whether the whole cluster moves together (Super+Shift+Left drag).
        is_cluster: bool,
    },
    /// Super+right drag: the window is resized from the corner nearest the pointer at the press
    /// (`left`, `top` say which); the opposite corner stays where it was.
    ElementResize {
        element: ElementId,
        initiator: u32,
        left: bool,
        top: bool,
        /// The window geometry's size at the press.
        start_size: (i32, i32),
        /// The pointer's Canvas position at the press.
        press: CanvasPos,
        /// Whether the whole cluster resizes together (Super+Shift+Right drag).
        is_cluster: bool,
    },
}

impl Interaction {
    /// Whether a compositor interaction owns the pointer (KeepFocus, INV-INPUT-3).
    pub fn active(&self) -> bool {
        !matches!(self, Interaction::Idle)
    }

    pub fn initiator(&self) -> Option<u32> {
        match *self {
            Interaction::Idle => None,
            Interaction::CameraPan { initiator, .. }
            | Interaction::Orphaned { initiator }
            | Interaction::ElementMove { initiator, .. }
            | Interaction::ElementResize { initiator, .. } => Some(initiator),
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
        let (camera, start, sum) = match self {
            Interaction::CameraPan {
                camera, start, sum, ..
            } => (camera, start, sum),
            Interaction::ElementMove {
                pan: Some(MovePan {
                    camera, start, sum, ..
                }),
                ..
            } => (camera, start, sum),
            _ => return None,
        };
        let term = |d: f64| (d * scale) / (scale * zoom);
        sum.0 += term(delta.0);
        sum.1 += term(delta.1);
        Some((*camera, *start, *sum))
    }

    /// The output and camera a pan moves: a plain pan, or one nested in a window move.
    pub fn panning(&self) -> Option<(OutputId, usize)> {
        match *self {
            Interaction::CameraPan { output, camera, .. }
            | Interaction::ElementMove {
                pan: Some(MovePan { output, camera, .. }),
                ..
            } => Some((output, camera)),
            _ => None,
        }
    }

    /// The window a move or resize acts on.
    pub fn element(&self) -> Option<ElementId> {
        match *self {
            Interaction::ElementMove { element, .. }
            | Interaction::ElementResize { element, .. } => Some(element),
            _ => None,
        }
    }

    /// The window being moved or resized was destroyed: the drag is orphaned.
    pub(crate) fn orphan_element(&mut self, destroyed: ElementId) {
        if self.element() == Some(destroyed)
            && let Some(initiator) = self.initiator()
        {
            *self = Interaction::Orphaned { initiator };
        }
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
        // a pan nested in a move ends with its output; the move goes on
        if let Interaction::ElementMove { pan, .. } = self
            && pan.is_some_and(|p| p.output == removed)
        {
            *pan = None;
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
