//! Interactive resize: the track from the user's drag to the client's answering commit.

use super::*;

impl Core {
    /// Super+right drag starts on `id`: the size and the anchor at the press are kept, so the
    /// corner opposite the dragged one stays put whatever size the client draws.
    pub(crate) fn begin_resize(
        &mut self,
        id: ElementId,
        left: bool,
        top: bool,
    ) -> Option<(i32, i32)> {
        let (anchor, tree) = (self.stage.anchor(id)?, self.stage.tree(id)?);
        let size = (tree.geometry.size.w, tree.geometry.size.h);
        self.resized_out_of_maximize(id);
        self.resize = Some(ResizeTrack {
            element: id,
            left,
            top,
            start_anchor: anchor,
            start_size: size,
            target: size,
            sent: size,
            awaiting: false,
            released: false,
            unanswered: 0,
        });
        Some(size)
    }

    /// One motion of the resize: the size the pointer asks for. At most one configure is
    /// outstanding: a newer size waits for the client's commit of the last one.
    pub(crate) fn resize_to(&mut self, size: (i32, i32), out: &mut Vec<Effect>) {
        let Some(r) = self.resize.as_mut() else {
            return;
        };
        r.target = size;
        if !r.awaiting && r.sent != size {
            r.sent = size;
            r.awaiting = true;
            out.push(Effect::ToplevelConfigureSize {
                element: r.element,
                size: LocalSize {
                    w: size.0,
                    h: size.1,
                },
            });
        }
    }

    /// The drag ended: one last configure, without `resizing`, at the last size asked for. The
    /// track stays until the client has drawn it (the anchor follows that commit).
    pub(crate) fn end_resize(&mut self, out: &mut Vec<Effect>) {
        let Some(r) = self.resize.as_mut() else {
            return;
        };
        r.released = true;
        r.unanswered = if r.awaiting { 2 } else { 1 };
        r.sent = r.target;
        r.awaiting = true;
        out.push(Effect::ToplevelConfigureSize {
            element: r.element,
            size: LocalSize {
                w: r.target.0,
                h: r.target.1,
            },
        });
    }

    /// A commit of the resized window: the anchor moves so that the corner opposite the dragged
    /// one stays where it was at the press; then the next size waits, if the pointer asked for
    /// another one meanwhile. Returns the anchor to write.
    pub(super) fn resize_committed(
        &mut self,
        id: ElementId,
        size: LocalSize,
        out: &mut Vec<Effect>,
    ) -> Option<CanvasPoint> {
        let r = self.resize.as_mut().filter(|r| r.element == id)?;
        let anchor = CanvasPoint {
            x: r.start_anchor.x
                + if r.left {
                    i64::from(r.start_size.0 - size.w)
                } else {
                    0
                },
            y: r.start_anchor.y
                + if r.top {
                    i64::from(r.start_size.1 - size.h)
                } else {
                    0
                },
        };
        r.awaiting = false;
        if r.released {
            r.unanswered = r.unanswered.saturating_sub(1);
            if (size.w, size.h) == r.sent || r.unanswered == 0 {
                self.resize = None;
            }
        } else if r.target != r.sent {
            let target = r.target;
            self.resize_to(target, out);
        }
        Some(anchor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::tests::*;

    /// Super+right drag: release while an in-flight configure is awaited keeps the track until
    /// the commit that answers the last configure sent, so intermediate commits do not drop the
    /// track and move the fixed corner.
    #[test]
    fn resize_track_kept_until_commit_answering_last_configure() {
        let mut c = centred((1280, 800), 120, None);
        let (id, start_a) = placed(&mut c, 600, 400);
        let mut fx = Vec::new();

        c.begin_resize(id, true, true);
        c.resize_to((800, 600), &mut fx);
        assert_eq!(
            fx,
            vec![Effect::ToplevelConfigureSize {
                element: id,
                size: LocalSize { w: 800, h: 600 },
            }]
        );
        fx.clear();

        c.resize_to((700, 500), &mut fx);
        assert!(fx.is_empty(), "motion while awaiting configure is queued");

        c.end_resize(&mut fx);
        assert_eq!(
            fx,
            vec![Effect::ToplevelConfigureSize {
                element: id,
                size: LocalSize { w: 700, h: 500 },
            }]
        );
        fx.clear();

        c.toplevel_committed(id, sized(800, 600, true)).unwrap();
        let a800 = c.stage().anchor(id).unwrap();
        assert_eq!(a800.x + 800, start_a.x + 600);
        assert_eq!(a800.y + 600, start_a.y + 400);

        c.toplevel_committed(id, sized(700, 500, true)).unwrap();
        let a700 = c.stage().anchor(id).unwrap();
        assert_eq!(a700.x + 700, start_a.x + 600);
        assert_eq!(a700.y + 500, start_a.y + 400);
    }

    /// A client may answer the last configure with another size (a terminal's cell grid): the
    /// track still ends with that answer, and a later resize of the client's own moves nothing.
    #[test]
    fn resize_track_ends_when_the_last_configure_is_answered_with_another_size() {
        let mut c = centred((1280, 800), 120, None);
        let (id, start_a) = placed(&mut c, 600, 400);
        let mut fx = Vec::new();
        c.begin_resize(id, true, true);
        c.resize_to((800, 600), &mut fx);
        c.resize_to((700, 500), &mut fx);
        c.end_resize(&mut fx);
        // answers to the 800 and the 700 configure, the second snapped to 696x496
        c.toplevel_committed(id, sized(800, 600, true)).unwrap();
        c.toplevel_committed(id, sized(696, 496, true)).unwrap();
        let a = c.stage().anchor(id).unwrap();
        assert_eq!((a.x + 696, a.y + 496), (start_a.x + 600, start_a.y + 400));
        assert!(c.resize.is_none(), "the track outlived the answers");
        // the client grows by itself: its anchor stays
        c.toplevel_committed(id, sized(900, 700, true)).unwrap();
        assert_eq!(c.stage().anchor(id), Some(a));
    }

    /// Resize track is cleared when its element unmaps or is destroyed, and does not snap
    /// the anchor on a subsequent remap.
    #[test]
    fn resize_track_cleared_on_unmap() {
        let mut c = centred((1280, 800), 120, None);
        let (id, _start_a) = placed(&mut c, 600, 400);

        c.begin_resize(id, true, true);
        assert!(c.toplevel_configure_state(id).resizing);

        // Window moves to a different location
        let mut fx = Vec::new();
        let moved_anchor = CanvasPoint { x: 500, y: 500 };
        c.place_by_shell(id, moved_anchor, &mut fx).unwrap();
        assert_eq!(c.stage().anchor(id), Some(moved_anchor));

        c.toplevel_committed(id, sized(600, 400, false)).unwrap();
        assert!(
            !c.toplevel_configure_state(id).resizing,
            "resizing cleared on unmap"
        );

        c.toplevel_committed(id, sized(600, 400, false)).unwrap(); // awaiting first buffer
        c.toplevel_committed(id, sized(600, 400, true)).unwrap(); // mapped
        assert_eq!(c.stage().anchor(id), Some(moved_anchor));

        // A further commit does not snap the anchor back to start_anchor.
        c.toplevel_committed(id, sized(800, 600, true)).unwrap();
        assert_eq!(
            c.stage().anchor(id),
            Some(moved_anchor),
            "anchor must not snap back to stale resize start_anchor"
        );
    }
}
