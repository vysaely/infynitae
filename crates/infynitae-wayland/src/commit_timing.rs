//! Commit timing (`wp_commit_timing_v1`): a client gives a commit the time it is to be shown at
//! (Mesa's Vulkan for even frame pacing, video players for each frame's time), and the commit is
//! held until then, so it is never shown before that time.
//!
//! A held commit of a window applies just before the window's clock output (INV-FRAME-2) draws the
//! frame that reaches the screen at or after its time ([`WaylandState::apply_timed_commits_for`]),
//! so it is shown at the first refresh it may be. Any held commit whose time has come applies at
//! once ([`WaylandState::apply_due_timed_commits`]): a hidden window's, a layer surface's, or one
//! on a screen that draws nothing then (off, or a flip still pending); it is then shown at the
//! refresh after its time.
//!
//! Smithay's timer object stores the time; it runs unmanaged here, so that the holding hook below
//! also records which surfaces hold a commit.

use std::time::Duration;

use infynitae_core::output::OutputId;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{DisplayHandle, Resource};
use smithay::utils::{Monotonic, Time};
use smithay::wayland::commit_timing::{
    CommitTimerBarrierStateUserData, CommitTimerStateUserData, CommitTimingManagerState,
};
use smithay::wayland::compositor::{add_blocker, with_states};

use crate::WaylandState;

pub(crate) struct CommitTiming {
    _global: CommitTimingManagerState,
    /// The surfaces that hold a timed commit.
    waiting: Vec<WlSurface>,
}

impl CommitTiming {
    pub(crate) fn new(display: &DisplayHandle) -> Self {
        Self {
            _global: CommitTimingManagerState::unmanaged::<WaylandState>(display),
            waiting: Vec::new(),
        }
    }
}

/// The earliest time a commit of `surface` waits for.
fn next_time(surface: &WlSurface) -> Option<Duration> {
    with_states(surface, |states| {
        let barriers = states.data_map.get::<CommitTimerBarrierStateUserData>()?;
        let next = barriers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .next_deadline()?;
        Some(Duration::from(Time::<Monotonic>::from(next)))
    })
}

/// Applies `surface`'s commits timed up to `until`; whether there was one.
fn release(surface: &WlSurface, until: Duration) -> bool {
    with_states(surface, |states| {
        states
            .data_map
            .get::<CommitTimerBarrierStateUserData>()
            .is_some_and(|b| {
                b.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .signal_until(Time::<Monotonic>::from(until))
            })
    })
}

/// The pre-commit hook of every surface: a commit with a time is held until then.
pub(crate) fn hold_timed_commit(state: &mut WaylandState, _: &DisplayHandle, surface: &WlSurface) {
    let time = with_states(surface, |states| {
        states
            .data_map
            .get::<CommitTimerStateUserData>()
            .and_then(|t| t.borrow_mut().timestamp.take())
    });
    let Some(time) = time else {
        return;
    };
    let barrier = with_states(surface, |states| {
        states
            .data_map
            .get_or_insert(CommitTimerBarrierStateUserData::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .register(time)
    });
    add_blocker(surface, barrier);
    let waiting = &mut state.commit_timing.waiting;
    if !waiting.contains(surface) {
        waiting.push(surface.clone());
    }
}

impl WaylandState {
    /// The output whose refreshes show `surface`: its window's clock output; `None` for a hidden
    /// window and a surface that is no window's.
    fn timed_clock(&self, surface: &WlSurface) -> Option<OutputId> {
        let root = self.toplevel_root(surface)?;
        let i = self.element_index_by_root(&root)?;
        self.core.clock_output_of(self.elements[i].id)
    }

    /// Applies the commits timed up to `until` of the surfaces `on` selects; whether any did.
    fn release_timed(&mut self, until: Duration, on: Option<OutputId>) -> bool {
        let mut any = false;
        let mut i = 0;
        while i < self.commit_timing.waiting.len() {
            let surface = &self.commit_timing.waiting[i];
            if !surface.is_alive() {
                self.commit_timing.waiting.swap_remove(i);
                continue;
            }
            if on.is_some() && self.timed_clock(surface) != on {
                i += 1;
                continue;
            }
            if release(surface, until) {
                any = true;
                if let Some(client) = surface.client()
                    && !self.released.contains(&client)
                {
                    self.released.push(client);
                }
            }
            if next_time(&self.commit_timing.waiting[i]).is_none() {
                self.commit_timing.waiting.swap_remove(i);
            } else {
                i += 1;
            }
        }
        self.apply_released();
        any
    }

    /// Just before `output` draws the frame that reaches the screen at `shown_at` (presentation
    /// clock): the commits of the windows it clocks timed up to then apply, and are in that frame.
    pub fn apply_timed_commits_for(&mut self, output: OutputId, shown_at: Duration) {
        if !self.commit_timing.waiting.is_empty() {
            self.release_timed(shown_at, Some(output));
        }
    }

    /// Every commit timed up to `now` applies, whichever surface holds it. A backend calls this
    /// once per loop iteration, before it decides whether to draw, and wakes for
    /// [`Self::next_timed_commit`]`(None)`. Whether one applied: what it shows needs a frame.
    pub fn apply_due_timed_commits(&mut self, now: Duration) -> bool {
        !self.commit_timing.waiting.is_empty() && self.release_timed(now, None)
    }

    /// The earliest time a held commit waits for: of the windows `output` clocks, or of any
    /// surface (`None`).
    pub fn next_timed_commit(&self, output: Option<OutputId>) -> Option<Duration> {
        self.commit_timing
            .waiting
            .iter()
            .filter(|s| output.is_none() || self.timed_clock(s) == output)
            .filter_map(next_time)
            .min()
    }
}

smithay::delegate_commit_timing!(WaylandState);
