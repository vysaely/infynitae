//! A window that exactly fills a screen is treated as fullscreen (FIT_IS_FULLSCREEN), though it never
//! asked to be: its frames can go to the screen as they are, it may tear, the game mode and VRR
//! apply. On an endless canvas this is often how a game or a video sits: maximized, or moved and
//! zoomed to fit. The protocol's fullscreen state is untouched, so the app is told nothing and
//! nothing about it changes when it stops fitting: one pan, and it is an ordinary window again.
//!
//! It is decided on each frame from what that frame shows. wlroots' scene graph does the like for
//! scanout alone (it only asks whether one buffer covers the screen); here tearing and the rest follow.

use std::collections::HashMap;

use infynitae_core::output::OutputId;
use infynitae_core::projection::View;
use infynitae_core::render::FrameEntry;
use infynitae_core::stage::ElementId;

use crate::WaylandState;

/// The frontmost window, when its main surface covers exactly the view, at no zoom but the screen's
/// and with nothing popped up over it. A real fullscreen window is not asked about here.
pub fn fills_the_view(entries: &[FrameEntry], view: &View) -> Option<ElementId> {
    if view.only.is_some() || view.k != view.scale {
        return None;
    }
    let first = entries.first()?;
    let (w, h) = view.size;
    let exact = |e: &FrameEntry| {
        e.rect.x0 == 0 && e.rect.y0 == 0 && e.rect.x1 == i64::from(w) && e.rect.y1 == i64::from(h)
    };
    // front to back, popups first: it must be its first surface, and its only one there
    let own = entries
        .iter()
        .filter(|e| e.element == first.element)
        .count();
    (exact(first) && own == 1).then_some(first.element)
}

/// Whether a format has no alpha: what is behind the window can't show through it.
pub fn is_opaque(format: smithay::backend::allocator::Fourcc) -> bool {
    use smithay::backend::allocator::Fourcc as F;
    matches!(
        format,
        F::Xrgb8888
            | F::Xbgr8888
            | F::Xrgb2101010
            | F::Xbgr2101010
            | F::Xrgb16161616f
            | F::Xbgr16161616f
    )
}

#[derive(Debug)]
pub struct Fitted {
    pub(crate) enabled: bool,
}

impl Default for Fitted {
    fn default() -> Self {
        Fitted { enabled: true }
    }
}

impl WaylandState {
    /// The window that fills `output` without being fullscreen, as of the last frame.
    pub fn fitted_window(&self, output: OutputId) -> Option<ElementId> {
        self.fitted_windows.get(&output).copied()
    }

    /// The window that has `output` to itself: fullscreen, or filling it.
    pub fn exclusive_window(&self, output: OutputId) -> Option<ElementId> {
        self.core
            .presentation(output)
            .filter(|_| !self.core.is_presentation_suppressed(output))
            .or_else(|| self.fitted_window(output))
    }

    pub(crate) fn fit_counts_as_fullscreen(&self, window: ElementId, _output: OutputId) -> bool {
        self.fitted.enabled
            && !self.is_locked()
            && !self.is_element_transparent(window)
            && self
                .window_buffer_format(window)
                .is_some_and(|f| is_opaque(f.code))
    }

    pub(crate) fn set_fitted_window(&mut self, output: OutputId, window: Option<ElementId>) {
        let was = match window {
            Some(w) => self.fitted_windows.insert(output, w),
            None => self.fitted_windows.remove(&output),
        };
        if was != window {
            match window {
                Some(w) => println!(
                    "[infynitae] Window {w:?} fills screen {output:?}: treated as fullscreen"
                ),
                None => println!(
                    "[infynitae] Window {:?} no longer fills screen {output:?}",
                    was.unwrap()
                ),
            }
        }
    }
}

pub(crate) type FittedWindows = HashMap<OutputId, ElementId>;

#[cfg(test)]
mod tests {
    use smithay::backend::allocator::Fourcc as F;

    #[test]
    fn only_a_format_without_alpha_is_opaque() {
        assert!(super::is_opaque(F::Xrgb8888));
        assert!(super::is_opaque(F::Xbgr2101010));
        assert!(!super::is_opaque(F::Argb8888));
        assert!(!super::is_opaque(F::Abgr2101010));
    }
}
