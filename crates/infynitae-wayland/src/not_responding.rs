//! "<app> is not responding" (owner, 2026-11-22): a window asked to close (Super+Shift+C, the
//! window menu, the window list) gets one `xdg_wm_base.ping` with the request. If it is still
//! there after [`WAIT`] and its client has not answered the ping, the edge asks whether to end
//! it (`launch.sh not-responding`). Nothing waits on the app and nothing pings a window that was
//! not asked to close: the check is a deadline the loop wakes for.
//!
//! An X11 window's ping goes to xwayland-satellite, which answers for itself: a frozen X11 app is
//! never asked about here; Super+Shift+Delete ends it.

use std::time::Duration;

use infynitae_core::stage::ElementId;

/// How long a window asked to close has to go away or answer the ping.
pub const WAIT: Duration = Duration::from_secs(5);

/// The windows asked to close whose client has not answered yet; `C` names the client.
pub struct Watches<C> {
    pending: Vec<(ElementId, C, Duration)>,
}

impl<C> Default for Watches<C> {
    fn default() -> Self {
        Watches {
            pending: Vec::new(),
        }
    }
}

impl<C: PartialEq> Watches<C> {
    /// `element` of `client` was asked to close at `now`; a second request waits anew.
    pub fn start(&mut self, element: ElementId, client: C, now: Duration) {
        self.pending.retain(|(e, ..)| *e != element);
        self.pending.push((element, client, now + WAIT));
    }

    /// `client` answered a ping: none of its windows is asked about.
    pub fn answered(&mut self, client: &C) {
        self.pending.retain(|(_, c, _)| c != client);
    }

    /// The windows whose wait ended by `now` and that are still there (`exists`), each once.
    pub fn due(&mut self, now: Duration, exists: impl Fn(ElementId) -> bool) -> Vec<ElementId> {
        let mut out = Vec::new();
        self.pending.retain(|(e, _, at)| {
            if *at > now {
                return exists(*e);
            }
            if exists(*e) {
                out.push(*e);
            }
            false
        });
        out
    }

    /// When the next wait ends; `None` when nothing waits.
    pub fn deadline(&self) -> Option<Duration> {
        self.pending.iter().map(|(_, _, at)| *at).min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(v: u64) -> Duration {
        Duration::from_millis(v)
    }

    fn ids() -> (ElementId, ElementId) {
        let id = |n| {
            infynitae_core::stage::ElementRef::from_raw(1, n)
                .unwrap()
                .element
        };
        (id(1), id(2))
    }

    #[test]
    fn a_window_still_there_unanswered_is_due_once() {
        let (a, _) = ids();
        let mut w = Watches::default();
        w.start(a, 1u32, ms(1000));
        assert_eq!(w.deadline(), Some(ms(1000) + WAIT));
        assert!(w.due(ms(1000) + WAIT - ms(1), |_| true).is_empty());
        assert_eq!(w.due(ms(1000) + WAIT, |_| true), [a]);
        assert!(
            w.due(ms(1000) + WAIT * 2, |_| true).is_empty(),
            "asked once"
        );
        assert_eq!(w.deadline(), None);
    }

    #[test]
    fn an_answer_or_a_closed_window_asks_nothing() {
        let (a, b) = ids();
        let mut w = Watches::default();
        w.start(a, 1u32, ms(0));
        w.start(b, 2u32, ms(0));
        w.answered(&1);
        assert_eq!(w.due(WAIT, |e| e != b), Vec::<ElementId>::new());
        assert_eq!(w.deadline(), None);
    }

    #[test]
    fn a_window_gone_before_its_deadline_stops_waking_the_loop() {
        let (a, _) = ids();
        let mut w = Watches::default();
        w.start(a, 1u32, ms(0));
        assert!(w.due(ms(10), |_| false).is_empty());
        assert_eq!(w.deadline(), None);
    }

    #[test]
    fn a_second_request_waits_anew() {
        let (a, _) = ids();
        let mut w = Watches::default();
        w.start(a, 1u32, ms(0));
        w.start(a, 1u32, ms(3000));
        assert!(w.due(WAIT, |_| true).is_empty());
        assert_eq!(w.due(ms(3000) + WAIT, |_| true), [a]);
    }
}
