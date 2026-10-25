//! Off-screen frame policy (M16; OBJ-17 FrameLiveness; INV-FRAME-1, INV-FRAME-3, INV-FRAME-4,
//! INV-IDLE-1; ADR-06).
//!
//! An observed element's callbacks complete on its clock output's vblank (`Core::frame_tick`).
//! An unobserved element's callbacks are withheld, except:
//! - the **kick**: before a non-initial configure, `popup_done` or an input event reaches the
//!   client, its withheld callbacks complete once (`Core::frame_kick`, called by the adapter for
//!   every effect it applies, before applying it);
//! - the **fallback**: at most one completion per period P, measured from the element's last
//!   completion, on a compositor timer that is armed only while an unobserved element has a
//!   pending callback (`Core::frame_fallback`, `Core::frame_fallback_deadline`).
//! - the **capture**: a capture of the window alone (sharing one window) completes its callbacks
//!   when it is drawn (`Core::frame_captured`), so a shared window off screen draws at the rate
//!   the share takes frames, not at the fallback's (owner, 2026-10-25: "like 2 fps").
//!
//! Core reads no clock. A completion Core decides without being handed the time (a vblank, a
//! kick) is stamped with the time of the next `frame_fallback` evaluation, which is never earlier
//! than the completion itself, so the period is never shortened (AUD-24); at worst the fallback
//! comes one evaluation late. That is OBJ-17's reconstruction rule ("reset to now").
//!
//! "Pending" is the snapshot's `frame_requested` (OBJ-05: committed callbacks not yet completed),
//! taken at every commit and cleared here when Core decides a completion. Smithay's throttling
//! state is not used.

use std::time::Duration;

use crate::snapshot::SurfaceKey;
use crate::stage::ElementId;

/// Default off-screen fallback period P: finite and > 0 (INV-FRAME-3, PROV-4).
pub const DEFAULT_FALLBACK_PERIOD: Duration = Duration::from_secs(1);

/// The fallback period must be finite and greater than zero (INV-FRAME-3, PROV-4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidFallbackPeriod;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Liveness {
    /// A committed callback has not completed (OBJ-05 `frame_requested`).
    requested: bool,
    /// The completion the fallback period is measured from; `None` until the next evaluation
    /// stamps it.
    last: Option<Duration>,
}

/// OBJ-17, one entry per element that ever had a pending callback, sorted by element.
#[derive(Debug)]
pub(crate) struct FrameLiveness {
    period: Duration,
    entries: Vec<(ElementId, Liveness)>,
    /// The surface that last received pointer focus (the recipient of buttons and axes, which
    /// carry no target). Kept as a surface, not resolved: whether its element must be kicked is
    /// decided when an event reaches it, from the element's state at that moment.
    pub(crate) pointer_surface: Option<SurfaceKey>,
    /// The element whose surface last received keyboard focus.
    pub(crate) keyboard_recipient: Option<ElementId>,
}

impl Default for FrameLiveness {
    fn default() -> Self {
        FrameLiveness {
            period: DEFAULT_FALLBACK_PERIOD,
            entries: Vec::new(),
            pointer_surface: None,
            keyboard_recipient: None,
        }
    }
}

impl FrameLiveness {
    pub(crate) fn period(&self) -> Duration {
        self.period
    }

    pub(crate) fn set_period(&mut self, period: Duration) -> Result<(), InvalidFallbackPeriod> {
        if period.is_zero() {
            return Err(InvalidFallbackPeriod);
        }
        self.period = period;
        Ok(())
    }

    fn find(&self, element: ElementId) -> Result<usize, usize> {
        self.entries.binary_search_by_key(&element, |(e, _)| *e)
    }

    /// A commit of `element` whose surface tree has (`requested`) or has no pending callback.
    pub(crate) fn committed(&mut self, element: ElementId, requested: bool) {
        match self.find(element) {
            Ok(i) => self.entries[i].1.requested = requested,
            Err(i) if requested => self.entries.insert(
                i,
                (
                    element,
                    Liveness {
                        requested,
                        last: None,
                    },
                ),
            ),
            Err(_) => {}
        }
    }

    pub(crate) fn requested(&self, element: ElementId) -> bool {
        self.find(element)
            .is_ok_and(|i| self.entries[i].1.requested)
    }

    /// Core decided a completion of `element`'s callbacks (vblank, kick or capture): nothing is pending
    /// and the period restarts at the next evaluation. A vblank of an element with nothing
    /// pending completes nothing and leaves the last completion where it was.
    pub(crate) fn completed(&mut self, element: ElementId) {
        if let Ok(i) = self.find(element)
            && self.entries[i].1.requested
        {
            self.entries[i].1 = Liveness {
                requested: false,
                last: None,
            };
        }
    }

    pub(crate) fn destroyed(&mut self, element: ElementId) {
        if let Ok(i) = self.find(element) {
            self.entries.remove(i);
        }
        if self.keyboard_recipient == Some(element) {
            self.keyboard_recipient = None;
        }
    }

    /// The kick (INV-FRAME-3 (a)): whether `element`'s withheld callbacks complete before the
    /// event that is about to reach its client. Only an unobserved element with a pending
    /// callback is kicked; the completion is recorded.
    pub(crate) fn kick(&mut self, element: ElementId, observed: bool) -> bool {
        if observed || !self.requested(element) {
            return false;
        }
        self.completed(element);
        true
    }

    /// The pending, unobserved element whose surface tree satisfies `contains`, if any. Only
    /// elements with a pending callback are examined.
    pub(crate) fn pending_where(
        &self,
        observed: impl Fn(ElementId) -> bool,
        contains: impl Fn(ElementId) -> bool,
    ) -> Option<ElementId> {
        self.entries
            .iter()
            .filter(|(e, l)| l.requested && !observed(*e))
            .map(|(e, _)| *e)
            .find(|e| contains(*e))
    }

    /// The fallback evaluation at `now` (INV-FRAME-3 (b)): unstamped completions are stamped
    /// with `now`; every unobserved element with a pending callback whose last completion is at
    /// least P ago completes, and is appended to `due`.
    pub(crate) fn evaluate(
        &mut self,
        now: Duration,
        observed: impl Fn(ElementId) -> bool,
        due: &mut Vec<ElementId>,
    ) {
        let period = self.period;
        for (e, l) in &mut self.entries {
            let last = *l.last.get_or_insert(now);
            if l.requested && !observed(*e) && now >= last.saturating_add(period) {
                *l = Liveness {
                    requested: false,
                    last: Some(now),
                };
                due.push(*e);
            }
        }
    }

    /// When the fallback timer must fire: the earliest `last + P` over unobserved elements with
    /// a pending callback; `Duration::ZERO` (at once) if one of them is not stamped yet; `None`
    /// when there is no such element, and then no timer is armed (INV-IDLE-1).
    pub(crate) fn deadline(&self, observed: impl Fn(ElementId) -> bool) -> Option<Duration> {
        self.entries
            .iter()
            .filter(|(e, l)| l.requested && !observed(*e))
            .map(|(_, l)| {
                l.last
                    .map_or(Duration::ZERO, |last| last.saturating_add(self.period))
            })
            .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::{InstanceId, Stage};

    fn ids(n: usize) -> Vec<ElementId> {
        let mut stage = Stage::new(InstanceId(1));
        (0..n).map(|_| stage.create()).collect()
    }

    const S: Duration = Duration::from_secs(1);

    #[test]
    fn nothing_pending_arms_no_timer() {
        let e = ids(2);
        let mut f = FrameLiveness::default();
        assert_eq!(f.deadline(|_| false), None);
        f.committed(e[0], false);
        assert_eq!(f.deadline(|_| false), None, "a commit without a callback");
        f.committed(e[0], true);
        assert_eq!(
            f.deadline(|_| true),
            None,
            "an observed element completes on its clock output, not on the timer"
        );
    }

    #[test]
    fn fallback_once_per_period_from_the_last_completion() {
        let e = ids(1)[0];
        let mut f = FrameLiveness::default();
        let mut due = Vec::new();
        f.committed(e, true);
        assert_eq!(
            f.deadline(|_| false),
            Some(Duration::ZERO),
            "not stamped yet"
        );
        f.evaluate(10 * S, |_| false, &mut due);
        assert!(due.is_empty(), "stamped now: the period starts here");
        assert_eq!(f.deadline(|_| false), Some(11 * S));
        f.evaluate(11 * S - Duration::from_nanos(1), |_| false, &mut due);
        assert!(due.is_empty());
        f.evaluate(11 * S, |_| false, &mut due);
        assert_eq!(due, vec![e]);
        assert_eq!(
            f.deadline(|_| false),
            None,
            "nothing pending after completion"
        );
        // a new request right after the fallback waits a whole period
        due.clear();
        f.committed(e, true);
        f.evaluate(11 * S + Duration::from_millis(1), |_| false, &mut due);
        assert!(due.is_empty());
        assert_eq!(f.deadline(|_| false), Some(12 * S));
    }

    #[test]
    fn a_kick_restarts_the_period() {
        let e = ids(1)[0];
        let mut f = FrameLiveness::default();
        let mut due = Vec::new();
        f.committed(e, true);
        f.evaluate(Duration::ZERO, |_| false, &mut due);
        assert!(f.kick(e, false));
        assert!(!f.kick(e, false), "a kick completes once");
        f.committed(e, true);
        // the kick happened before this evaluation; it is stamped here, not earlier
        f.evaluate(S - Duration::from_millis(1), |_| false, &mut due);
        assert!(due.is_empty(), "no fallback within P of the kick (AUD-24)");
        assert_eq!(
            f.deadline(|_| false),
            Some(2 * S - Duration::from_millis(1))
        );
    }

    /// The period runs from the last completion that completed something: a vblank of an
    /// element with nothing pending does not move it.
    #[test]
    fn an_empty_vblank_is_not_a_completion() {
        let e = ids(1)[0];
        let mut f = FrameLiveness::default();
        let mut due = Vec::new();
        f.committed(e, true);
        f.completed(e);
        f.evaluate(10 * S, |_| true, &mut due);
        f.completed(e);
        f.evaluate(10 * S + Duration::from_millis(17), |_| true, &mut due);
        f.committed(e, true);
        assert_eq!(f.deadline(|_| false), Some(11 * S));
    }

    #[test]
    fn observed_elements_are_never_kicked_or_fallen_back() {
        let e = ids(1)[0];
        let mut f = FrameLiveness::default();
        let mut due = Vec::new();
        f.committed(e, true);
        assert!(!f.kick(e, true));
        f.evaluate(100 * S, |_| true, &mut due);
        assert!(due.is_empty());
        assert!(
            f.requested(e),
            "still pending for the clock output's vblank"
        );
    }

    #[test]
    fn destruction_forgets_the_element() {
        let e = ids(1)[0];
        let mut f = FrameLiveness::default();
        f.committed(e, true);
        f.keyboard_recipient = Some(e);
        f.destroyed(e);
        assert_eq!(f.deadline(|_| false), None);
        assert_eq!(f.keyboard_recipient, None);
    }

    #[test]
    fn the_period_is_finite_and_positive() {
        let mut f = FrameLiveness::default();
        assert_eq!(f.period(), DEFAULT_FALLBACK_PERIOD);
        assert_eq!(f.set_period(Duration::ZERO), Err(InvalidFallbackPeriod));
        assert_eq!(f.set_period(Duration::from_millis(250)), Ok(()));
        assert_eq!(f.period(), Duration::from_millis(250));
    }
}
