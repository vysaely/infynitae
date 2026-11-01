//! When to draw a screen's next frame: as late before its next refresh as the frame can still
//! make it (LATENCY). A frame drawn right after a refresh waits almost a whole refresh before it
//! is shown; drawn just in time, it shows the newest input and the newest client buffers.
//!
//! The prediction is measured, not padded: the render times of recent frames (their 95th
//! percentile, and never less than the slowest of the last few), plus a safety margin that grows
//! by a step when a frame misses its refresh and shrinks slowly while frames make it. Other
//! compositors that schedule late overestimate badly (KWin measured at 11 ms predicted for 2 ms
//! actual, 2026), and the overestimate is latency.
//!
//! All times are on the presentation clock (CLOCK_MONOTONIC, the DRM vblank timestamps' clock).

use std::time::Duration;

/// How much latency to trade for safety (LATENCY in the config).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LatencyMode {
    /// Draw at once, as before: no late scheduling.
    Off,
    /// The margin starts at 1 ms and never goes below 0.5 ms.
    Balanced,
    /// The margin starts at 0.5 ms and may go down to 0.25 ms.
    Lowest,
}

impl LatencyMode {
    /// From the config value; unknown values are `Balanced`.
    pub fn parse(v: &str) -> LatencyMode {
        match v.to_ascii_lowercase().as_str() {
            "off" | "false" | "0" | "no" => LatencyMode::Off,
            "lowest" | "low" | "min" => LatencyMode::Lowest,
            _ => LatencyMode::Balanced,
        }
    }

    fn margins(self) -> (Duration, Duration) {
        match self {
            LatencyMode::Off => (Duration::ZERO, Duration::ZERO),
            LatencyMode::Balanced => (Duration::from_micros(1000), Duration::from_micros(500)),
            LatencyMode::Lowest => (Duration::from_micros(500), Duration::from_micros(250)),
        }
    }
}

/// Recent render times kept for the prediction.
const SAMPLES: usize = 128;
/// The slowest of this many latest frames is a floor for the prediction (a sudden slow frame).
const RECENT: usize = 8;
/// A missed refresh widens the margin by this much.
const MISS_STEP: Duration = Duration::from_micros(750);
/// Frames made in a row before the margin shrinks by one tenth of a step.
const SHRINK_AFTER: u32 = 240;
/// The margin never exceeds this.
const MARGIN_MAX: Duration = Duration::from_millis(4);
/// Refreshes drawn at once after a miss at the largest margin: frames keep missing although the
/// margin is as wide as it goes, so the render times measured do not tell when a frame is really
/// ready (a GPU still busy after the submission); late scheduling stops for a while.
const PAUSE_REFRESHES: u32 = 600;

/// One screen's schedule.
#[derive(Debug)]
pub struct FrameSchedule {
    mode: LatencyMode,
    period: Duration,
    /// The last refresh seen (a vblank's timestamp).
    last_vblank: Option<Duration>,
    /// The refresh the frame in flight was drawn for.
    target: Option<Duration>,
    samples: [Duration; SAMPLES],
    filled: usize,
    next: usize,
    margin: Duration,
    made_in_a_row: u32,
    misses: u64,
    frames: u64,
    /// Refreshes left drawn at once (PAUSE_REFRESHES).
    paused: u32,
    pauses: u64,
    /// When the frame in flight was submitted.
    submitted: Option<Duration>,
    /// From a refresh to the frame submitted after it, since the last summary.
    to_submit: Vec<Duration>,
    /// From a frame's submission to the refresh that showed it, since the last summary.
    to_flip: Vec<Duration>,
}

/// p50, p95 and max.
pub type Percentiles = (Duration, Duration, Duration);

/// The [`Percentiles`] of `v`, which it empties; `None` when it is empty.
fn percentiles(v: &mut Vec<Duration>) -> Option<Percentiles> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    let at = |q: usize| v[((v.len() * q).div_ceil(100)).saturating_sub(1)];
    let summary = (at(50), at(95), v[v.len() - 1]);
    v.clear();
    Some(summary)
}

/// Longest list of timings kept between summaries (a summary comes every 10 s).
const TIMINGS_KEPT: usize = 10_000;

impl FrameSchedule {
    /// A schedule for a screen refreshing every `period`.
    pub fn new(mode: LatencyMode, period: Duration) -> FrameSchedule {
        FrameSchedule {
            mode,
            period,
            last_vblank: None,
            target: None,
            samples: [Duration::ZERO; SAMPLES],
            filled: 0,
            next: 0,
            margin: mode.margins().0,
            made_in_a_row: 0,
            misses: 0,
            frames: 0,
            paused: 0,
            pauses: 0,
            submitted: None,
            to_submit: Vec::new(),
            to_flip: Vec::new(),
        }
    }

    pub fn set_mode(&mut self, mode: LatencyMode) {
        if mode != self.mode {
            self.mode = mode;
            self.margin = mode.margins().0;
        }
    }

    pub fn set_period(&mut self, period: Duration) {
        self.period = period;
    }

    /// The predicted time a frame needs, from start to submitted: the 95th percentile of the
    /// recent ones, at least the slowest of the last few, plus the margin; capped below the
    /// period. `None` before any frame was measured.
    pub fn budget(&self) -> Option<Duration> {
        if self.filled == 0 {
            return None;
        }
        let mut v: Vec<Duration> = self.samples[..self.filled].to_vec();
        v.sort_unstable();
        let p95 = v[((v.len() * 95).div_ceil(100)).saturating_sub(1)];
        let recent = (0..RECENT.min(self.filled))
            .map(|i| self.samples[(self.next + SAMPLES - 1 - i) % SAMPLES])
            .max()
            .unwrap_or_default();
        let b = p95.max(recent) + self.margin;
        Some(b.min(self.period.mul_f64(0.9)))
    }

    /// When the next frame should start, given a frame is wanted at `now`: just in time for
    /// the next refresh that can still be made. `None`: draw now (late scheduling off, nothing
    /// measured yet, no refresh seen, or no time to wait).
    pub fn start_at(&self, now: Duration) -> Option<Duration> {
        if self.mode == LatencyMode::Off || self.paused > 0 {
            return None;
        }
        let (last, budget) = (self.last_vblank?, self.budget()?);
        let period = self.period.max(Duration::from_micros(1));
        // the first refresh after `now` that the frame can still make
        let mut refresh = last + period;
        while refresh < now + budget {
            refresh += period;
        }
        let start = refresh - budget;
        (start > now).then_some(start)
    }

    /// [`Self::start_at`] for a frame that may already be held back until `held`: once that
    /// time has come, it is drawn now. The loop wakes a little after the time it asked for, and
    /// from there the refresh looks missed; asking again would push the frame back a whole
    /// refresh, every time, and nothing would be drawn again.
    pub fn start_for(&self, now: Duration, held: Option<Duration>) -> Option<Duration> {
        if held.is_some_and(|t| t <= now) {
            return None;
        }
        self.start_at(now)
    }

    /// A frame started at `started` was submitted at `submitted`; it is meant for the next
    /// refresh after it.
    pub fn rendered(&mut self, started: Duration, submitted: Duration) {
        let took = submitted.saturating_sub(started);
        self.samples[self.next] = took;
        self.next = (self.next + 1) % SAMPLES;
        self.filled = (self.filled + 1).min(SAMPLES);
        self.submitted = Some(submitted);
        if let Some(last) = self.last_vblank
            && self.to_submit.len() < TIMINGS_KEPT
        {
            self.to_submit.push(submitted.saturating_sub(last));
        }
        self.target = self.last_vblank.map(|last| {
            let period = self.period.max(Duration::from_micros(1));
            let mut r = last + period;
            while r < submitted {
                r += period;
            }
            r
        });
    }

    /// The refresh at `time` showed the frame in flight: a frame that came a refresh or more
    /// after the one it was meant for missed it, and the margin widens.
    /// A miss at the widest margin stops late scheduling for PAUSE_REFRESHES; frames drawn at
    /// once meanwhile leave the margin alone.
    pub fn vblank(&mut self, time: Duration) {
        let paused = self.paused > 0;
        self.paused = self.paused.saturating_sub(1);
        if let Some(submitted) = self.submitted.take()
            && self.to_flip.len() < TIMINGS_KEPT
        {
            self.to_flip.push(time.saturating_sub(submitted));
        }
        if let Some(target) = self.target.take() {
            self.frames += 1;
            let missed = time > target + self.period / 2;
            if missed {
                self.misses += 1;
                self.made_in_a_row = 0;
            }
            let late_scheduled = !paused && self.mode != LatencyMode::Off;
            if late_scheduled && missed {
                if self.margin >= MARGIN_MAX {
                    self.paused = PAUSE_REFRESHES;
                    self.pauses += 1;
                    self.margin = self.mode.margins().0;
                } else {
                    self.margin = (self.margin + MISS_STEP).min(MARGIN_MAX);
                }
            } else if late_scheduled {
                self.made_in_a_row += 1;
                if self.made_in_a_row >= SHRINK_AFTER {
                    self.made_in_a_row = 0;
                    let floor = self.mode.margins().1;
                    self.margin = self.margin.saturating_sub(MISS_STEP / 10).max(floor);
                }
            }
        }
        self.last_vblank = Some(time);
    }

    /// Where a frame's time went since the last summary, each as p50, p95 and max: from a refresh
    /// to the frame's submission, and from the submission to the refresh that showed it. A missed
    /// refresh is late in one of the two: the first is this process, the second the kernel and
    /// GPU. Resets.
    pub fn take_timings(&mut self) -> (Option<Percentiles>, Option<Percentiles>) {
        (
            percentiles(&mut self.to_submit),
            percentiles(&mut self.to_flip),
        )
    }

    /// Frames shown, and how many missed the refresh they were drawn for.
    pub fn counts(&self) -> (u64, u64) {
        (self.frames, self.misses)
    }

    pub fn margin(&self) -> Duration {
        self.margin
    }

    /// How often late scheduling stopped for a while (PAUSE_REFRESHES), and whether it is
    /// stopped now.
    pub fn pauses(&self) -> (u64, bool) {
        (self.pauses, self.paused > 0)
    }
}

/// Input-to-screen times: from an input event to the refresh that first showed a frame drawn
/// after it (LATENCY_STATS logs a summary).
#[derive(Debug, Default)]
pub struct LatencyStats {
    /// The earliest input not drawn yet.
    pending: Option<Duration>,
    /// The earliest input in the frame in flight.
    in_flight: Option<Duration>,
    samples: Vec<Duration>,
}

impl LatencyStats {
    /// An input event arrived at `time`.
    pub fn input(&mut self, time: Duration) {
        if self.pending.is_none() {
            self.pending = Some(time);
        }
    }

    /// A frame was drawn: it carries the inputs before it.
    pub fn drawn(&mut self) {
        if self.in_flight.is_none() {
            self.in_flight = self.pending.take();
        }
    }

    /// The refresh at `time` showed that frame.
    pub fn shown(&mut self, time: Duration) {
        if let Some(t) = self.in_flight.take()
            && self.samples.len() < 100_000
        {
            self.samples.push(time.saturating_sub(t));
        }
    }

    /// p50, p95, max and the count since the last summary; resets.
    pub fn take_summary(&mut self) -> Option<(Duration, Duration, Duration, usize)> {
        if self.samples.is_empty() {
            return None;
        }
        let mut v = std::mem::take(&mut self.samples);
        v.sort_unstable();
        let at = |q: usize| v[((v.len() * q).div_ceil(100)).saturating_sub(1)];
        Some((at(50), at(95), *v.last().unwrap(), v.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: Duration = Duration::from_nanos(6_060_606); // 165 Hz
    fn ms(v: f64) -> Duration {
        Duration::from_secs_f64(v / 1000.0)
    }

    fn warmed(mode: LatencyMode, took: f64) -> FrameSchedule {
        let mut s = FrameSchedule::new(mode, P);
        for _ in 0..20 {
            s.rendered(Duration::ZERO, ms(took));
        }
        s
    }

    #[test]
    fn timings_say_where_a_frames_time_went_and_reset() {
        let mut s = FrameSchedule::new(LatencyMode::Off, P);
        s.vblank(ms(100.0));
        // submitted 1 ms after the refresh, shown 11 ms later: it waited out a whole refresh
        s.rendered(ms(100.5), ms(101.0));
        s.vblank(ms(112.0));
        let (submit, flip) = s.take_timings();
        assert_eq!(submit.map(|t| t.2.as_micros()), Some(1000));
        assert_eq!(flip.map(|t| t.2.as_micros()), Some(11000));
        assert_eq!(s.take_timings(), (None, None), "a summary resets them");
        // a refresh with no frame in flight is not a frame's time
        s.vblank(ms(118.0));
        assert_eq!(s.take_timings().1, None);
    }

    #[test]
    fn a_frame_starts_just_in_time_for_the_next_refresh() {
        let mut s = warmed(LatencyMode::Balanced, 1.0);
        s.vblank(ms(100.0));
        // wanted right after the refresh: wait until next refresh - (1 ms + 1 ms margin)
        let start = s.start_at(ms(100.2)).expect("a later start");
        let want = ms(100.0) + P - ms(2.0);
        assert!(
            start.abs_diff(want) < Duration::from_micros(5),
            "{start:?} vs {want:?}"
        );
    }

    #[test]
    fn too_late_for_this_refresh_means_the_next_one_or_now() {
        let mut s = warmed(LatencyMode::Balanced, 1.0);
        s.vblank(ms(100.0));
        // 1 ms before the next refresh: this one can't be made, so in time for the one after
        let now = ms(100.0) + P - ms(1.0);
        let start = s.start_at(now).expect("the following refresh");
        assert!(start > now && start < now + P);
        // exactly at the start point: draw now
        let at = ms(100.0) + P - ms(2.0);
        assert_eq!(s.start_at(at), None);
    }

    #[test]
    fn a_held_frame_woken_late_is_drawn_not_pushed_back() {
        let mut s = warmed(LatencyMode::Balanced, 1.0);
        s.vblank(ms(100.0));
        let held = s.start_at(ms(100.2)).expect("held back");
        let woke = held + Duration::from_micros(80);
        // asked afresh, that moment is past this refresh's start: the next refresh
        assert!(s.start_at(woke).is_some());
        // but the frame held for it is drawn now
        assert_eq!(s.start_for(woke, Some(held)), None);
        // woken early by something else: still held until its time
        let early = held - ms(1.0);
        let again = s.start_for(early, Some(held)).expect("still held");
        assert!(again.abs_diff(held) < Duration::from_micros(5));
    }

    #[test]
    fn nothing_is_delayed_when_off_or_before_anything_is_known() {
        let mut s = warmed(LatencyMode::Off, 1.0);
        s.vblank(ms(100.0));
        assert_eq!(s.start_at(ms(100.2)), None, "off");
        let s = FrameSchedule::new(LatencyMode::Balanced, P);
        assert_eq!(s.start_at(ms(100.2)), None, "no refresh, no measurement");
    }

    #[test]
    fn a_sudden_slow_frame_raises_the_prediction_at_once() {
        let mut s = warmed(LatencyMode::Balanced, 1.0);
        let before = s.budget().unwrap();
        s.rendered(Duration::ZERO, ms(3.0));
        assert!(s.budget().unwrap() >= ms(3.0) + s.margin());
        assert!(s.budget().unwrap() > before);
    }

    #[test]
    fn a_missed_refresh_widens_the_margin_and_made_ones_shrink_it_slowly() {
        let mut s = warmed(LatencyMode::Balanced, 1.0);
        s.vblank(ms(100.0));
        let m0 = s.margin();
        // drawn for the next refresh, shown a whole refresh later
        s.rendered(ms(104.0), ms(105.0));
        s.vblank(ms(100.0) + P * 2);
        assert_eq!(s.counts(), (1, 1));
        assert_eq!(s.margin(), m0 + MISS_STEP);
        // many frames made: back down, never below the mode's floor
        let mut t = ms(100.0) + P * 2;
        for _ in 0..(SHRINK_AFTER * 20) {
            s.rendered(t + P - ms(2.0), t + P - ms(1.0));
            t += P;
            s.vblank(t);
        }
        assert_eq!(s.margin(), Duration::from_micros(500));
    }

    #[test]
    fn misses_at_the_widest_margin_stop_late_scheduling_for_a_while() {
        let mut s = warmed(LatencyMode::Balanced, 1.0);
        let mut t = ms(100.0);
        s.vblank(t);
        // every frame shown a refresh late, whatever the margin
        let mut n = 0;
        while s.pauses().0 == 0 {
            s.rendered(t + ms(1.0), t + ms(2.0));
            t += P * 2;
            s.vblank(t);
            n += 1;
            assert!(n < 20, "never paused");
        }
        assert!(s.pauses().1);
        assert_eq!(s.start_at(t + ms(0.2)), None, "drawn at once while paused");
        for _ in 0..PAUSE_REFRESHES {
            t += P;
            s.vblank(t);
        }
        assert!(!s.pauses().1);
        assert!(s.start_at(t + ms(0.2)).is_some(), "late again afterwards");
        assert_eq!(
            s.margin(),
            Duration::from_micros(1000),
            "from the start margin"
        );
    }

    #[test]
    fn the_budget_never_takes_the_whole_period() {
        let s = warmed(LatencyMode::Balanced, 50.0);
        assert!(s.budget().unwrap() <= P.mul_f64(0.9));
    }

    #[test]
    fn latency_runs_from_the_first_input_to_the_refresh_that_shows_it() {
        let mut l = LatencyStats::default();
        l.input(ms(10.0));
        l.input(ms(11.0));
        l.drawn();
        l.shown(ms(14.0));
        l.input(ms(20.0));
        l.drawn();
        l.shown(ms(22.0));
        let (p50, _, max, n) = l.take_summary().unwrap();
        assert_eq!(n, 2);
        assert_eq!(max, ms(4.0));
        assert!(p50 == ms(2.0) || p50 == ms(4.0));
        assert!(l.take_summary().is_none(), "reset");
    }
}
