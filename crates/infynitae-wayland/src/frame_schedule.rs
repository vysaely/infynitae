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
    /// Direct scanout (`Balanced`: 1 ms start, 0.5 ms floor) and composited desktop frames (1.5 ms
    /// start, 0.5 ms floor) both schedule late with independent margins and budgets.
    Direct,
}

impl LatencyMode {
    /// From the config value; unknown values are `Balanced`.
    pub fn parse(v: &str) -> LatencyMode {
        if crate::parse_bool(v) == Some(false) {
            LatencyMode::Off
        } else if v.eq_ignore_ascii_case("lowest")
            || v.eq_ignore_ascii_case("low")
            || v.eq_ignore_ascii_case("min")
        {
            LatencyMode::Lowest
        } else if v.eq_ignore_ascii_case("direct") {
            LatencyMode::Direct
        } else {
            LatencyMode::Balanced
        }
    }

    /// `(start, floor)` margins for `scanned_out` (`true` = direct scanout, `false` = composited).
    fn margins_for(self, scanned_out: bool) -> (Duration, Duration) {
        match self {
            LatencyMode::Direct if !scanned_out => {
                (Duration::from_micros(1500), Duration::from_micros(500))
            }
            LatencyMode::Direct => LatencyMode::Balanced.margins_for(true),
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
/// Frames made in a row before the margin shrinks by a whole step: from the widest back to the
/// narrowest in about seven seconds at 165 Hz (a tenth of a step took about a minute, so a few early
/// misses cost a game most of `LATENCY="direct"`'s gain for that long; review 2026-11-12).
const SHRINK_AFTER: u32 = 240;
/// After the mode changes (a game starts being shown directly) this many refreshes do not widen the
/// margin: the first frames carry the switch's own work, not the game's.
const SETTLE_REFRESHES: u32 = 30;
/// The margin never exceeds this.
const MARGIN_MAX: Duration = Duration::from_millis(4);
/// Refreshes drawn at once after a miss at the largest margin: frames keep missing although the
/// margin is as wide as it goes, so the render times measured do not tell when a frame is really
/// ready (a GPU still busy after the submission); late scheduling stops for a while.
const PAUSE_REFRESHES: u32 = 600;
/// Composited saturation detection: pause at >= 5% misses over 100 non-settling frames (or >= 2 in
/// 40 while recovering), doubling the pause up to ~16x and resetting after 600 clean frames.
const SAT_WINDOW: u32 = 100;
const SAT_MISSES: u32 = 5;
const RECOVERY_WINDOW: u32 = 40;
const RECOVERY_MISSES: u32 = 2;
const PAUSE_MAX: u32 = PAUSE_REFRESHES * 16;
const PAUSE_RESET_AFTER: u32 = 600;
/// How long after a refresh its successors' times are trusted for [`FrameSchedule`]'s guard.
const GUARD_TRUSTED: Duration = Duration::from_secs(1);
/// A guarded frame starts this long after the refresh it waited for.
const AFTER_REFRESH: Duration = Duration::from_micros(100);

/// Render times and margin state for one screen path (direct scanout or composited).
#[derive(Debug, Clone, Copy)]
struct Lane {
    samples: [Duration; SAMPLES],
    filled: usize,
    next: usize,
    margin: Duration,
    made_in_a_row: u32,
    /// Refreshes left in which a miss does not widen the margin (SETTLE_REFRESHES after a switch).
    settling: u32,
    /// Refreshes left drawn at once (PAUSE_REFRESHES, or doubled while saturated).
    paused: u32,
    pauses: u64,
    /// Recent non-settling late-scheduled misses (bit 0 = newest).
    miss_bits: u128,
    pause_hold: u32,
    clean_since_pause: u32,
}

impl Lane {
    fn new(start_margin: Duration) -> Self {
        Self {
            samples: [Duration::ZERO; SAMPLES],
            filled: 0,
            next: 0,
            margin: start_margin,
            made_in_a_row: 0,
            settling: 0,
            paused: 0,
            pauses: 0,
            miss_bits: 0,
            pause_hold: PAUSE_REFRESHES,
            clean_since_pause: 0,
        }
    }

    fn set_margin_for_mode(&mut self, start_margin: Duration) {
        self.margin = start_margin;
        self.settling = SETTLE_REFRESHES;
    }

    fn record_sample(&mut self, took: Duration) {
        self.samples[self.next] = took;
        self.next = (self.next + 1) % SAMPLES;
        self.filled = (self.filled + 1).min(SAMPLES);
    }

    fn budget(&self, period: Duration) -> Option<Duration> {
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
        Some(b.min(period.mul_f64(0.9)))
    }

    fn vblank_outcome(
        &mut self,
        late_scheduled: bool,
        missed: bool,
        (start_margin, floor): (Duration, Duration),
        backoff: bool,
    ) {
        if missed {
            self.made_in_a_row = 0;
        }
        let settling = self.settling > 0;
        self.settling = self.settling.saturating_sub(1);
        if late_scheduled && missed && settling {
            // the switch's own frames: counted as missed, the margin left alone
        } else if late_scheduled {
            let sat_mask = (1u128 << SAT_WINDOW) - 1;
            let rec_mask = (1u128 << RECOVERY_WINDOW) - 1;
            self.miss_bits = ((self.miss_bits << 1) | u128::from(missed)) & sat_mask;
            if missed {
                self.clean_since_pause = 0;
                let saturated = backoff
                    && (self.miss_bits.count_ones() >= SAT_MISSES
                        || (self.pause_hold > PAUSE_REFRESHES
                            && (self.miss_bits & rec_mask).count_ones() >= RECOVERY_MISSES));
                if self.margin >= MARGIN_MAX || saturated {
                    self.paused = if backoff {
                        self.pause_hold
                    } else {
                        PAUSE_REFRESHES
                    };
                    self.pauses += 1;
                    self.margin = start_margin;
                    self.miss_bits = 0;
                    if backoff {
                        self.pause_hold = (self.pause_hold * 2).min(PAUSE_MAX);
                    }
                } else {
                    self.margin = (self.margin + MISS_STEP).min(MARGIN_MAX);
                }
            } else {
                self.made_in_a_row += 1;
                if self.made_in_a_row >= SHRINK_AFTER {
                    self.made_in_a_row = 0;
                    self.margin = self.margin.saturating_sub(MISS_STEP).max(floor);
                }
                if backoff && self.pause_hold > PAUSE_REFRESHES {
                    self.clean_since_pause += 1;
                    if self.clean_since_pause >= PAUSE_RESET_AFTER {
                        self.pause_hold = PAUSE_REFRESHES;
                        self.clean_since_pause = 0;
                    }
                }
            }
        }
    }
}

/// One screen's schedule.
#[derive(Debug)]
pub struct FrameSchedule {
    mode: LatencyMode,
    period: Duration,
    /// Whether this screen is currently scanning out directly (`true`) or compositing (`false`).
    scanned_out: bool,
    /// Temporary bypass for this frame (composited desktop under `Direct` when guards trigger).
    skip_late: bool,
    /// The last refresh seen (a vblank's timestamp).
    last_vblank: Option<Duration>,
    /// The refresh the frame in flight was drawn for.
    target: Option<Duration>,
    /// Independent render history and margin for scanned-out frames (`true`).
    direct: Lane,
    /// Independent render history and margin for composited frames (`false`).
    composited: Lane,
    /// Which path drew the frame currently in flight.
    in_flight_scanned_out: bool,
    /// Whether late scheduling was bypassed for the frame currently in flight.
    in_flight_skipped: bool,
    misses: u64,
    frames: u64,
    /// Frames whose target came after a quiet gap (`>= 4 * period`): counted apart from runs and
    /// never used to widen the margin or feed GPU relief.
    pause_misses: u64,
    pause_frames: u64,
    /// Whether the frame currently in flight was submitted after a quiet gap.
    in_flight_after_pause: bool,
    /// LATENCY off: how much of a refresh before its end a frame no longer starts, and waits for
    /// the next instead (FRAME_GUARD, percent).
    guard_percent: u32,
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
            scanned_out: false,
            skip_late: false,
            last_vblank: None,
            target: None,
            direct: Lane::new(mode.margins_for(true).0),
            composited: Lane::new(mode.margins_for(false).0),
            in_flight_scanned_out: false,
            in_flight_skipped: false,
            misses: 0,
            frames: 0,
            pause_misses: 0,
            pause_frames: 0,
            in_flight_after_pause: false,
            guard_percent: 0,
            submitted: None,
            to_submit: Vec::new(),
            to_flip: Vec::new(),
        }
    }

    pub fn set_mode(&mut self, mode: LatencyMode) {
        if mode != self.mode {
            self.mode = mode;
            self.direct.set_margin_for_mode(mode.margins_for(true).0);
            self.composited
                .set_margin_for_mode(mode.margins_for(false).0);
        }
    }

    /// Selects whether the screen currently scans out (`true`) or composes (`false`). Switching
    /// paths enters settling so transition frames do not widen the new path's margin.
    pub fn set_scanned_out(&mut self, scanned_out: bool) {
        if scanned_out != self.scanned_out {
            self.scanned_out = scanned_out;
            self.lane_mut(scanned_out).settling = SETTLE_REFRESHES;
        }
    }

    /// Bypasses late scheduling for the next frame while keeping both lanes' state intact.
    pub fn set_skip_late(&mut self, skip: bool) {
        self.skip_late = skip;
    }

    fn active(&self) -> &Lane {
        if self.scanned_out {
            &self.direct
        } else {
            &self.composited
        }
    }

    fn lane(&self, scanned_out: bool) -> &Lane {
        if scanned_out {
            &self.direct
        } else {
            &self.composited
        }
    }

    fn lane_mut(&mut self, scanned_out: bool) -> &mut Lane {
        if scanned_out {
            &mut self.direct
        } else {
            &mut self.composited
        }
    }

    pub fn set_period(&mut self, period: Duration) {
        self.period = period;
    }

    pub fn period(&self) -> Duration {
        self.period
    }

    pub fn set_guard_percent(&mut self, percent: u32) {
        self.guard_percent = percent.min(90);
    }

    /// How long until the next refresh, from the last real one (so it never slides against the
    /// display); one period when none was seen lately.
    pub fn until_next_refresh(&self, now: Duration) -> Duration {
        let period = self.period.max(Duration::from_micros(1));
        match self.last_vblank {
            Some(last) if now >= last => {
                let done = (now - last).as_nanos() % period.as_nanos();
                period - Duration::from_nanos(done as u64)
            }
            _ => period,
        }
    }

    /// LATENCY off: a frame that would start too close to the refresh's end waits until just after
    /// it (with the newest buffers then), as a late start only misses the refresh and blocks the
    /// next frame too. Only near a refresh seen lately: long after the last one its time is a guess.
    fn guarded_start(&self, now: Duration) -> Option<Duration> {
        let last = self.last_vblank?;
        if self.guard_percent == 0 || now < last || now - last > GUARD_TRUSTED {
            return None;
        }
        let left = self.until_next_refresh(now);
        (left < self.period * self.guard_percent / 100).then(|| now + left + AFTER_REFRESH)
    }

    /// The predicted time a frame needs, from start to submitted: the 95th percentile of the
    /// recent ones, at least the slowest of the last few, plus the margin; capped below the
    /// period. `None` before any frame was measured.
    pub fn budget(&self) -> Option<Duration> {
        self.active().budget(self.period)
    }

    /// When the next frame should start, given a frame is wanted at `now`: just in time for
    /// the next refresh that can still be made. `None`: draw now (late scheduling off, skipped,
    /// nothing measured yet, no refresh seen, or no time to wait).
    pub fn start_at(&self, now: Duration) -> Option<Duration> {
        if self.mode == LatencyMode::Off {
            return self.guarded_start(now);
        }
        let lane = self.active();
        if self.skip_late || lane.paused > 0 {
            return self.guarded_start(now);
        }
        let last = self.last_vblank?;
        let Some(budget) = lane.budget(self.period) else {
            return self.guarded_start(now);
        };
        let period = self.period.max(Duration::from_micros(1));
        // the first refresh after `now` that the frame can still make
        let mut refresh = last + period;
        while refresh < now + budget {
            refresh += period;
        }
        let start = refresh - budget;
        if start > now {
            Some(start)
        } else {
            self.guarded_start(now)
        }
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
        let scanned_out = self.scanned_out;
        self.lane_mut(scanned_out).record_sample(took);
        self.in_flight_scanned_out = scanned_out;
        self.in_flight_skipped = self.skip_late;
        self.submitted = Some(submitted);
        // a frame long after the last refresh is a screen that was quiet, not a late frame
        let in_run = self
            .last_vblank
            .is_some_and(|last| submitted.saturating_sub(last) < self.period * 4);
        self.in_flight_after_pause = !in_run;
        if let Some(last) = self.last_vblank
            && self.to_submit.len() < TIMINGS_KEPT
            && in_run
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
    /// after the one it was meant for missed it, and the active path's margin widens.
    /// A miss at the widest margin stops late scheduling on that path for PAUSE_REFRESHES;
    /// frames drawn at once meanwhile leave its margin alone.
    pub fn vblank(&mut self, time: Duration) {
        let in_flight_direct = self.in_flight_scanned_out;
        let paused = self.lane(in_flight_direct).paused > 0;
        self.direct.paused = self.direct.paused.saturating_sub(1);
        self.composited.paused = self.composited.paused.saturating_sub(1);
        if let Some(submitted) = self.submitted.take()
            && self.to_flip.len() < TIMINGS_KEPT
        {
            self.to_flip.push(time.saturating_sub(submitted));
        }
        let after_pause = std::mem::take(&mut self.in_flight_after_pause);
        let skipped = std::mem::take(&mut self.in_flight_skipped);
        if let Some(target) = self.target.take() {
            let missed = time > target + self.period / 2;
            if after_pause {
                self.pause_frames += 1;
                if missed {
                    self.pause_misses += 1;
                }
            } else {
                self.frames += 1;
                if missed {
                    self.misses += 1;
                }
                let late_scheduled = !paused && !skipped && self.mode != LatencyMode::Off;
                let bounds = self.mode.margins_for(in_flight_direct);
                self.lane_mut(in_flight_direct).vblank_outcome(
                    late_scheduled,
                    missed,
                    bounds,
                    !in_flight_direct,
                );
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

    /// In-run frames shown, and how many missed the refresh they were drawn for.
    pub fn counts(&self) -> (u64, u64) {
        (self.frames, self.misses)
    }

    /// Frames drawn after a quiet pause (`>= 4 * period`), and how many missed their guessed target.
    pub fn pause_counts(&self) -> (u64, u64) {
        (self.pause_frames, self.pause_misses)
    }

    pub fn margin(&self) -> Duration {
        self.active().margin
    }

    /// `(direct, composited)` margins kept per screen path.
    pub fn margins(&self) -> (Duration, Duration) {
        (self.direct.margin, self.composited.margin)
    }

    /// How often late scheduling stopped for a while (PAUSE_REFRESHES), and whether the active
    /// path is stopped now.
    pub fn pauses(&self) -> (u64, bool) {
        (
            self.direct.pauses + self.composited.pauses,
            self.active().paused > 0,
        )
    }
}

/// Whether a screen spares the GPU: its refreshes are being missed. Missing about one in twenty for
/// a second starts it; it holds for a while, longer each time it had to start again at once, since
/// the misses stop while it works and say nothing of whether the load has gone.
#[derive(Debug, Default)]
pub struct FrameRelief {
    /// The counts at the start of this second.
    mark: (u64, u64),
    /// The refreshes it still holds for.
    left: u64,
    /// Its last hold, in seconds.
    hold: u64,
    /// The refreshes since it last ended.
    since: u64,
}

const RELIEF_HOLD: u64 = 30;
const RELIEF_HOLD_MAX: u64 = 300;
const RELIEF_AGAIN: u64 = 5;

impl FrameRelief {
    /// A refresh, with the schedule's counts ([`FrameSchedule::counts`]) and the screen's rate.
    /// Returns whether to spare the GPU, and whether that changed.
    pub fn step(&mut self, (frames, misses): (u64, u64), hz: u64) -> (bool, bool) {
        let was = self.left > 0;
        if was {
            self.left -= 1;
        } else {
            self.since = self.since.saturating_add(1);
        }
        let done = frames.saturating_sub(self.mark.0);
        if done >= hz.max(1) {
            let missed = misses.saturating_sub(self.mark.1);
            self.mark = (frames, misses);
            if missed * 20 >= done {
                if !was {
                    self.hold = if self.hold > 0 && self.since < RELIEF_AGAIN * hz {
                        (self.hold * 2).min(RELIEF_HOLD_MAX)
                    } else {
                        RELIEF_HOLD
                    };
                }
                self.left = self.hold * hz;
            }
        }
        let on = self.left > 0;
        if was && !on {
            self.since = 0;
        }
        (on, on != was)
    }
}

/// Maximum time a pending input stays attached across empty renders while waiting for an app frame.
const PENDING_MAX_AGE: Duration = Duration::from_millis(100);

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
        if self
            .pending
            .is_none_or(|t| time.saturating_sub(t) > PENDING_MAX_AGE)
        {
            self.pending = Some(time);
        }
    }

    /// A frame was drawn at `now`: it carries pending inputs within [`PENDING_MAX_AGE`].
    pub fn drawn(&mut self, now: Duration) {
        self.expire_pending(now);
        if self.in_flight.is_none() {
            self.in_flight = self.pending.take();
        }
    }

    /// Expires a pending input older than [`PENDING_MAX_AGE`] on an empty render turn.
    pub fn expire_pending(&mut self, now: Duration) {
        if self
            .pending
            .is_some_and(|t| now.saturating_sub(t) > PENDING_MAX_AGE)
        {
            self.pending = None;
        }
    }

    /// Drops any pending input unconditionally (e.g. while powered off).
    pub fn drop_pending(&mut self) {
        self.pending = None;
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
    #[test]
    fn direct_schedules_both_paths_with_independent_starting_margins() {
        use super::LatencyMode;
        assert_eq!(LatencyMode::parse("direct"), LatencyMode::Direct);
        let s = FrameSchedule::new(LatencyMode::Direct, P);
        assert_eq!(s.margins(), (ms(1.0), ms(1.5)));
        assert_eq!(s.margin(), ms(1.5), "starts composited at 1.5 ms");
        let off = FrameSchedule::new(LatencyMode::Off, P);
        assert_eq!(off.margins(), (Duration::ZERO, Duration::ZERO));
        let bal = FrameSchedule::new(LatencyMode::Balanced, P);
        assert_eq!(bal.margins(), (ms(1.0), ms(1.0)));
    }

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
    fn missed_refreshes_start_the_relief_and_it_holds_longer_each_time_it_comes_back() {
        let mut r = FrameRelief::default();
        // one second at 100 Hz with 4 missed: nothing
        let mut seen = (false, false);
        for f in 1..=100 {
            seen = r.step((f, 4), 100);
        }
        assert_eq!(seen, (false, false));
        // the next with 5 of 100 missed: on, for 30 s
        for f in 101..=199 {
            r.step((f, 4), 100);
        }
        assert_eq!(r.step((200, 9), 100), (true, true));
        for f in 201..3200 {
            assert!(r.step((f, 9), 100).0, "still held at {f}");
        }
        assert_eq!(r.step((3200, 9), 100), (false, true));
        // missing again at once: 60 s now
        for f in 3201..3300 {
            r.step((f, 9), 100);
        }
        assert_eq!(r.step((3300, 29), 100), (true, true));
        for f in 3301..9300 {
            assert!(r.step((f, 29), 100).0, "still held at {f}");
        }
        assert_eq!(r.step((9300, 29), 100), (false, true));
        // a long time later: back to 30 s
        for f in 9301..12000 {
            r.step((f, 29), 100);
        }
        for f in 12000..12099 {
            r.step((f, 29), 100);
        }
        assert_eq!(r.step((12100, 49), 100), (true, true));
        assert_eq!(r.hold, RELIEF_HOLD);
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

    /// From the widest margin back to the narrowest in seconds, not a minute: a whole step per
    /// SHRINK_AFTER refreshes made (review 2026-11-12).
    #[test]
    fn the_margin_comes_back_from_its_widest_in_seconds() {
        let mut s = warmed(LatencyMode::Balanced, 1.0);
        let mut t = ms(100.0);
        s.vblank(t);
        // widened to 3.5 ms by four misses
        for _ in 0..4 {
            s.rendered(t + ms(1.0), t + ms(2.0));
            t += P * 2;
            s.vblank(t);
        }
        assert!(s.margin() >= Duration::from_micros(3500));
        let steps =
            (s.margin() - Duration::from_micros(500)).as_micros() / MISS_STEP.as_micros() + 1;
        for _ in 0..(SHRINK_AFTER as u128 * steps) {
            s.rendered(t + P - ms(2.0), t + P - ms(1.0));
            t += P;
            s.vblank(t);
        }
        assert_eq!(s.margin(), Duration::from_micros(500));
        // at 165 Hz that is under ten seconds
        assert!(SHRINK_AFTER as u128 * steps < 165 * 10);
    }

    /// The refreshes right after the mode changes (a game starts going to the screen directly) do not
    /// widen the margin: they carry the switch's own work.
    #[test]
    fn misses_right_after_the_mode_changes_leave_the_margin_alone() {
        let mut s = warmed(LatencyMode::Off, 1.0);
        s.set_mode(LatencyMode::Balanced);
        let m0 = s.margin();
        let mut t = ms(100.0);
        // no frame in flight at this refresh: it does not count toward settling
        s.vblank(t);
        for _ in 0..SETTLE_REFRESHES {
            s.rendered(t + ms(1.0), t + ms(2.0));
            t += P * 2;
            s.vblank(t);
        }
        assert_eq!(s.margin(), m0, "settling: no widening");
        assert!(s.counts().1 > 0, "still counted as missed");
        s.rendered(t + ms(1.0), t + ms(2.0));
        t += P * 2;
        s.vblank(t);
        assert!(s.margin() > m0, "after settling a miss widens it");
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
        l.drawn(ms(12.0));
        l.shown(ms(14.0));
        l.input(ms(20.0));
        l.drawn(ms(21.0));
        l.shown(ms(22.0));
        let (p50, _, max, n) = l.take_summary().unwrap();
        assert_eq!(n, 2);
        assert_eq!(max, ms(4.0));
        assert!(p50 == ms(2.0) || p50 == ms(4.0));
        assert!(l.take_summary().is_none(), "reset");
    }

    #[test]
    fn the_guard_holds_a_late_frame_until_just_after_the_refresh() {
        let mut s = FrameSchedule::new(LatencyMode::Off, P);
        s.set_guard_percent(30);
        s.vblank(Duration::from_secs(10));
        let start = Duration::from_secs(10);
        assert_eq!(
            s.start_at(start + P / 10),
            None,
            "early in the refresh: at once"
        );
        assert_eq!(s.start_at(start + P * 69 / 100), None);
        let held = s.start_at(start + P * 80 / 100).expect("too late: held");
        assert!(held > start + P && held < start + P + P / 10);
    }

    #[test]
    fn no_guard_without_a_percent_or_a_recent_refresh() {
        let mut s = FrameSchedule::new(LatencyMode::Off, P);
        s.vblank(Duration::from_secs(10));
        assert_eq!(s.start_at(Duration::from_secs(10) + P * 95 / 100), None);
        s.set_guard_percent(30);
        assert_eq!(s.start_at(Duration::from_secs(12) + P * 95 / 100), None);
    }

    #[test]
    fn the_next_refresh_comes_from_the_last_real_one() {
        let mut s = FrameSchedule::new(LatencyMode::Off, P);
        let t = Duration::from_secs(5);
        s.vblank(t);
        assert_eq!(s.until_next_refresh(t + P / 4), P - P / 4);
        assert_eq!(s.until_next_refresh(t + P * 3 + P / 4), P - P / 4);
    }

    #[test]
    fn a_quiet_screen_is_not_a_late_frame() {
        let mut s = FrameSchedule::new(LatencyMode::Off, P);
        let t = Duration::from_secs(5);
        s.vblank(t);
        s.rendered(t + P * 100, t + P * 100);
        assert_eq!(s.take_timings().0, None);
    }

    #[test]
    fn misses_after_a_pause_do_not_widen_the_margin_or_count_as_in_run() {
        let mut s = warmed(LatencyMode::Balanced, 1.0);
        let t = Duration::from_secs(10);
        s.vblank(t);
        let m0 = s.margin();

        // First frame after a quiet gap (> 4 * P) whose guessed target misses: recorded as after-pause
        // without changing counts() or widening the margin.
        let wake = t + P * 20;
        s.rendered(wake, wake + ms(1.0));
        s.vblank(wake + P * 2);
        assert_eq!(s.counts(), (0, 0));
        assert_eq!(s.pause_counts(), (1, 1));
        assert_eq!(s.margin(), m0);

        // Next frame is in a run (< 4 * P) and misses: counts in-run and widens margin.
        let run = wake + P * 2 + ms(1.0);
        s.rendered(run, run + ms(1.0));
        s.vblank(wake + P * 4);
        assert_eq!(s.counts(), (1, 1));
        assert_eq!(s.pause_counts(), (1, 1));
        assert_eq!(s.margin(), m0 + MISS_STEP);
    }

    #[test]
    fn dropped_pending_input_is_not_charged_to_a_later_frame() {
        let mut l = LatencyStats::default();
        // Frame already in flight is kept while an idle input after it is dropped.
        l.input(ms(10.0));
        l.drawn(ms(10.5));
        l.input(ms(11.0));
        l.drop_pending();
        l.shown(ms(15.0));

        // Later frame without another dropped input starts fresh from its own input.
        l.input(ms(1000.0));
        l.drawn(ms(1001.0));
        l.shown(ms(1003.0));

        let (_, _, max, n) = l.take_summary().unwrap();
        assert_eq!(n, 2);
        assert_eq!(max, ms(5.0));
    }

    #[test]
    fn direct_keeps_game_and_desktop_margins_and_budgets_separate() {
        let mut s = FrameSchedule::new(LatencyMode::Direct, P);
        // Warm composited (desktop) path with 2.0 ms frames -> budget 2.0 + 1.5 = 3.5 ms.
        for _ in 0..20 {
            s.rendered(Duration::ZERO, ms(2.0));
        }
        // Switch to direct scanout and warm past settling with 0.2 ms frames -> budget 0.2 + 1.0 = 1.2 ms.
        s.set_scanned_out(true);
        let mut t = ms(100.0);
        s.vblank(t);
        for _ in 0..(SETTLE_REFRESHES + 16) {
            s.rendered(t + P - ms(1.0), t + P - ms(0.8));
            t += P;
            s.vblank(t);
        }
        assert_eq!(s.margins(), (ms(1.0), ms(1.5)));
        let direct_budget = s.budget().unwrap();
        assert!(
            direct_budget.abs_diff(ms(1.2)) < Duration::from_micros(5),
            "{direct_budget:?}"
        );

        // Switching back to composited keeps the 2.0 ms desktop samples and 1.5 ms margin.
        s.set_scanned_out(false);
        let composited_budget = s.budget().unwrap();
        assert!(
            composited_budget.abs_diff(ms(3.5)) < Duration::from_micros(5),
            "{composited_budget:?}"
        );
        // Run out composited settling with good frames.
        for _ in 0..SETTLE_REFRESHES {
            s.rendered(t + P - ms(3.0), t + P - ms(1.0));
            t += P;
            s.vblank(t);
        }

        // A miss on composited widens only composited margin; direct margin stays 1.0 ms.
        s.rendered(t + ms(1.0), t + ms(3.0));
        t += P * 2;
        s.vblank(t);
        assert_eq!(s.margins(), (ms(1.0), ms(1.5) + MISS_STEP));

        // Pushing composited all the way to pause stops only composited late start; direct still schedules late.
        while !s.pauses().1 {
            s.rendered(t + ms(1.0), t + ms(3.0));
            t += P * 2;
            s.vblank(t);
        }
        assert_eq!(s.start_at(t + ms(0.2)), None, "composited is paused");
        s.set_scanned_out(true);
        assert!(
            s.start_at(t + ms(0.2)).is_some(),
            "direct path is not paused by composited misses"
        );
        assert_eq!(s.margins().0, ms(1.0), "direct margin untouched");
    }

    #[test]
    fn skipped_composited_frames_draw_at_once_and_leave_margin_alone() {
        let mut s = warmed(LatencyMode::Direct, 1.0);
        let mut t = ms(100.0);
        s.vblank(t);
        assert!(
            s.start_at(t + ms(0.2)).is_some(),
            "composited late at 1.5 ms"
        );
        assert_eq!(s.margin(), ms(1.5));

        s.set_skip_late(true);
        assert_eq!(s.start_at(t + ms(0.2)), None, "guard forces immediate draw");
        assert_eq!(s.start_for(t + ms(0.2), None), None);
        // A miss while guarded does not widen or pause the composited margin.
        s.rendered(t + ms(1.0), t + ms(2.0));
        t += P * 2;
        s.vblank(t);
        assert_eq!(s.margin(), ms(1.5));
        assert_eq!(s.counts(), (1, 1));

        s.set_skip_late(false);
        assert!(s.start_at(t + ms(0.2)).is_some(), "late start resumes");
    }

    #[test]
    fn skipped_or_paused_composited_frames_keep_frame_guard_protection() {
        let mut s = warmed(LatencyMode::Direct, 1.0);
        s.set_guard_percent(20);
        let t = ms(100.0);
        s.vblank(t);

        s.set_skip_late(true);
        let near_end = t + P - ms(0.3);
        assert_eq!(
            s.start_at(near_end),
            Some(t + P + AFTER_REFRESH),
            "skipped frame 0.3 ms before vblank waits until after refresh"
        );
        assert_eq!(
            s.start_at(t + ms(0.5)),
            None,
            "skipped frame early in refresh draws at once"
        );

        s.set_skip_late(false);
        s.composited.paused = 10;
        assert_eq!(
            s.start_at(near_end),
            Some(t + P + AFTER_REFRESH),
            "paused composited frame near vblank still gets guarded start"
        );

        let mut cold = FrameSchedule::new(LatencyMode::Direct, P);
        cold.set_guard_percent(20);
        cold.vblank(t);
        assert_eq!(
            cold.start_at(near_end),
            Some(t + P + AFTER_REFRESH),
            "unwarmed frame without budget near vblank still gets guarded start"
        );
    }

    #[test]
    fn composited_gpu_saturation_backs_off_escalates_and_keeps_direct_game_untouched() {
        let mut s = warmed(LatencyMode::Direct, 1.0);
        let mut t = ms(100.0);
        s.vblank(t);

        // 1 miss every 16 frames (~6.25% rate) saturates the desktop lane without hitting MARGIN_MAX first.
        for frame in 1..=100 {
            let miss = frame % 16 == 0;
            s.rendered(t + ms(0.5), t + ms(1.5));
            t += if miss { P * 2 } else { P };
            s.vblank(t);
            if s.pauses().1 {
                break;
            }
        }
        assert_eq!(
            s.pauses(),
            (1, true),
            "desktop backed off at >=5% miss rate"
        );

        for _ in 0..PAUSE_REFRESHES {
            assert_eq!(s.start_at(t + ms(0.2)), None, "paused");
            t += P;
            s.vblank(t);
        }
        assert!(!s.pauses().1, "first pause lasted PAUSE_REFRESHES");

        // Renewed misses during recovery escalate the pause to 2x.
        for _ in 0..RECOVERY_MISSES {
            s.rendered(t + ms(1.0), t + ms(2.0));
            t += P * 2;
            s.vblank(t);
        }
        assert_eq!(s.pauses(), (2, true), "escalated pause");
        for _ in 0..PAUSE_REFRESHES {
            t += P;
            s.vblank(t);
        }
        assert!(s.pauses().1, "second pause holds for 2x PAUSE_REFRESHES");
        for _ in 0..PAUSE_REFRESHES {
            t += P;
            s.vblank(t);
        }
        assert!(!s.pauses().1, "second pause ends at 2x PAUSE_REFRESHES");

        // 600 clean composited frames reset escalation back to 1x PAUSE_REFRESHES.
        for _ in 0..PAUSE_RESET_AFTER {
            s.rendered(t + P - ms(3.0), t + P - ms(1.0));
            t += P;
            s.vblank(t);
        }
        assert_eq!(s.composited.pause_hold, PAUSE_REFRESHES);

        // Switching to direct scanout keeps the fullscreen game lane untouched.
        s.set_scanned_out(true);
        for _ in 0..SETTLE_REFRESHES {
            s.rendered(t + P - ms(2.0), t + P - ms(1.0));
            t += P;
            s.vblank(t);
        }
        assert!(s.start_at(t + ms(0.2)).is_some());
        // Direct lane still requires MARGIN_MAX (5 misses) and keeps a flat PAUSE_REFRESHES hold.
        for _ in 0..5 {
            s.rendered(t + ms(1.0), t + ms(2.0));
            t += P * 2;
            s.vblank(t);
        }
        assert_eq!(s.direct.pauses, 1);
        assert_eq!(s.direct.paused, PAUSE_REFRESHES);
        for _ in 0..PAUSE_REFRESHES {
            t += P;
            s.vblank(t);
        }
        for _ in 0..5 {
            s.rendered(t + ms(1.0), t + ms(2.0));
            t += P * 2;
            s.vblank(t);
        }
        assert_eq!(s.direct.pauses, 2);
        assert_eq!(
            s.direct.paused, PAUSE_REFRESHES,
            "direct fullscreen game lane never escalates pause hold"
        );
    }

    #[test]
    fn pending_input_stays_across_empty_renders_up_to_limit() {
        let mut l = LatencyStats::default();
        // Kept across empty renders within 100 ms until the app commits a frame.
        l.input(ms(10.0));
        l.expire_pending(ms(20.0));
        l.expire_pending(ms(105.0));
        l.drawn(ms(108.0));
        l.shown(ms(112.0));
        let (_, _, max, n) = l.take_summary().unwrap();
        assert_eq!(n, 1);
        assert_eq!(max, ms(102.0));

        // Expired after 100 ms of empty renders, so unrelated later frames do not count it.
        l.input(ms(200.0));
        l.expire_pending(ms(301.0));
        l.drawn(ms(310.0));
        l.shown(ms(316.0));
        assert!(l.take_summary().is_none());

        // Also expired on drawn() if > 100 ms passed without empty renders.
        l.input(ms(400.0));
        l.drawn(ms(550.0));
        l.shown(ms(556.0));
        assert!(l.take_summary().is_none());

        // Stale pending > 100 ms old is replaced by a fresh input rather than suppressing it.
        l.input(ms(600.0));
        l.input(ms(750.0));
        l.drawn(ms(752.0));
        l.shown(ms(755.0));
        let (_, _, max, n) = l.take_summary().unwrap();
        assert_eq!(n, 1);
        assert_eq!(max, ms(5.0));
    }
}
