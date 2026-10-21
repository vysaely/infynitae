//! The pointer drawn by the compositor itself, for a backend that owns the display (the console
//! backend, owner decision T8 2026-10-22): nothing else draws it there. The client's cursor
//! surface when it set one (`wl_pointer.set_cursor`), a built-in arrow when it asked for a named
//! cursor or none was set, nothing when it hid the pointer. A backend inside a host window leaves
//! the pointer to the host.
//!
//! The geometry of what is drawn (the client's surface at its hotspot, the arrow's rects) is
//! `convert`'s (INV-COORD-4); this module keeps what the client asked for.

use smithay::backend::renderer::element::Id;
use smithay::input::pointer::CursorImageStatus;
use smithay::reexports::wayland_server::Resource;

use crate::WaylandState;

/// The built-in arrow's height in rows (`convert::arrow_rects`).
pub(crate) const ARROW_ROWS: i32 = 13;

/// The ids of the arrow's rects (stable across frames, so an unmoved arrow is not damage).
#[derive(Debug)]
pub(crate) struct Arrow {
    pub(crate) outline: Vec<Id>,
    pub(crate) fill: Vec<Id>,
}

impl Default for Arrow {
    fn default() -> Self {
        Arrow {
            outline: (0..ARROW_ROWS).map(|_| Id::new()).collect(),
            fill: (0..ARROW_ROWS).map(|_| Id::new()).collect(),
        }
    }
}

impl WaylandState {
    /// The client asked for a cursor (`SeatHandler::cursor_image`).
    pub(crate) fn set_cursor_status(&mut self, status: CursorImageStatus) {
        self.cursor = status;
    }

    /// The frame callbacks of the cursor surface complete with the output's frame (an animated
    /// cursor advances).
    pub(crate) fn complete_cursor_frames(
        &self,
        output: &smithay::output::Output,
        now: std::time::Duration,
    ) {
        if let CursorImageStatus::Surface(surface) = &self.cursor
            && surface.is_alive()
        {
            smithay::desktop::utils::send_frames_surface_tree(
                surface,
                output,
                now,
                None,
                |_, _| Some(output.clone()),
            );
        }
    }
}
