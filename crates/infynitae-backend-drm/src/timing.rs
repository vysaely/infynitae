//! Loop turn timing: the slow-turn and latency lines of the session log.

use super::*;

/// Where a turn of the loop spent its time, for a turn long enough to be felt as a stutter
/// ([`slow_turn_threshold`]): the session log names its slowest parts. "Loop busy" said only how
/// long (553 ms on the owner's PC, 2026-11-06), not where.
pub(super) struct TurnClock {
    pub(super) last: Option<std::time::Instant>,
    pub(super) phases: Vec<(&'static str, Duration)>,
    /// The surface slowest to make into an element this turn (`take_slowest_surface`).
    pub(super) surface: Option<String>,
}

/// Fallback slow-turn threshold when no physical screen is connected.
const SLOW_TURN_FALLBACK: Duration = Duration::from_millis(8);

/// One refresh of the fastest connected screen, or [`SLOW_TURN_FALLBACK`] when none is attached.
pub(super) fn slow_turn_threshold(screens: &[Screen]) -> Duration {
    screens
        .iter()
        .map(|s| s.schedule.period())
        .min()
        .unwrap_or(SLOW_TURN_FALLBACK)
}

impl TurnClock {
    pub(super) fn new(start: std::time::Instant) -> Self {
        Self {
            last: Some(start),
            phases: Vec::with_capacity(20),
            surface: None,
        }
    }

    /// Disabled clock: allocates nothing and takes no timestamps.
    pub(super) fn off() -> Self {
        Self {
            last: None,
            phases: Vec::new(),
            surface: None,
        }
    }

    pub(super) fn is_enabled(&self) -> bool {
        self.last.is_some()
    }

    /// The part of the turn since the last mark is `name`.
    pub(super) fn mark(&mut self, name: &'static str) {
        let Some(last) = self.last else {
            return;
        };
        let now = std::time::Instant::now();
        self.phases.push((name, now.duration_since(last)));
        self.last = Some(now);
    }

    /// The parts of a turn of `busy` at least `threshold`, slowest first, those of 1 ms or more;
    /// the rest is time before the first mark or between marks this turn did not reach.
    pub(super) fn slow_parts(&self, busy: Duration, threshold: Duration) -> Option<String> {
        if self.last.is_none() || busy < threshold {
            return None;
        }
        let mut phase_totals: HashMap<&'static str, Duration> = HashMap::new();
        for &(name, d) in &self.phases {
            *phase_totals.entry(name).or_default() += d;
        }
        let mut parts: Vec<(&'static str, Duration)> = phase_totals
            .into_iter()
            .filter(|(_, d)| *d >= Duration::from_millis(1))
            .collect();
        parts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        let marked: Duration = self.phases.iter().map(|(_, d)| *d).sum();
        let mut out: Vec<String> = parts
            .iter()
            .map(|(n, d)| format!("{n} {:.1} ms", d.as_secs_f64() * 1000.0))
            .collect();
        let rest = busy.saturating_sub(marked);
        if rest >= Duration::from_millis(1) {
            out.push(format!("other {:.1} ms", rest.as_secs_f64() * 1000.0));
        }
        let surface = self
            .surface
            .as_ref()
            .map_or_else(String::new, |s| format!("; slowest surface {s}"));
        Some(format!(
            "[infynitae] Slow turn {:.1} ms: {}{surface}",
            busy.as_secs_f64() * 1000.0,
            out.join(", ")
        ))
    }
}

/// Holds the slowest turn of each 1 s window so a 7 ms hiccup earlier in the second never hides a
/// 40 ms stall later in the same second.
#[derive(Debug, Default)]
pub(super) struct SlowTurnLog {
    pub(super) window_started: Option<std::time::Instant>,
    pub(super) worst: Option<(Duration, String)>,
}

impl SlowTurnLog {
    pub(super) fn record_at(
        &mut self,
        clock: &TurnClock,
        busy: Duration,
        threshold: Duration,
        now: std::time::Instant,
    ) -> Option<String> {
        let flushed = self.take_due_at(now);
        if let Some(line) = clock.slow_parts(busy, threshold) {
            match self.window_started {
                Some(_) => {
                    if self.worst.as_ref().is_none_or(|(prev, _)| busy > *prev) {
                        self.worst = Some((busy, line));
                    }
                }
                None => {
                    self.window_started = Some(now);
                    self.worst = Some((busy, line));
                }
            }
        }
        flushed
    }

    pub(super) fn take_due_at(&mut self, now: std::time::Instant) -> Option<String> {
        let ws = self.window_started?;
        if now.duration_since(ws) < Duration::from_secs(1) {
            return None;
        }
        self.window_started = None;
        self.worst.take().map(|(_, line)| line)
    }

    pub(super) fn flush(&mut self) -> Option<String> {
        self.window_started = None;
        self.worst.take().map(|(_, line)| line)
    }

    pub(super) fn remaining(&self, now: std::time::Instant) -> Option<Duration> {
        self.window_started
            .map(|ws| Duration::from_secs(1).saturating_sub(now.saturating_duration_since(ws)))
    }
}

/// LATENCY_STATS: every 10 s, each screen's input-to-screen times and missed refreshes, in the
/// session log.
pub(super) fn latency_summary(data: &mut LoopData) {
    if !data.state.latency_stats {
        return;
    }
    let now = std::time::Instant::now();
    if data
        .latency_logged
        .is_some_and(|t| now.duration_since(t) < std::time::Duration::from_secs(10))
    {
        return;
    }
    data.latency_logged = Some(now);
    let busy = std::mem::take(&mut data.loop_busy_max);
    if busy >= Duration::from_millis(5) {
        log_line!(
            "[infynitae] Loop busy: longest turn {:.1} ms",
            busy.as_secs_f64() * 1000.0
        );
    }
    for s in &mut data.screens {
        let (frames, misses) = s.schedule.counts();
        let (pause_frames, pause_misses) = s.schedule.pause_counts();
        let (pauses, paused) = s.schedule.pauses();
        if let Some((p50, p95, max, n)) = s.latency.take_summary() {
            let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
            let (direct_m, comp_m) = s.schedule.margins();
            let margins = if data.state.latency_mode
                == infynitae_wayland::frame_schedule::LatencyMode::Direct
            {
                format!(
                    " (direct {:.2}, composited {:.2})",
                    ms(direct_m),
                    ms(comp_m)
                )
            } else {
                String::new()
            };
            log_line!(
                "[infynitae] Latency {:?}: input to screen p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms ({n} frames); budget {:.2} ms, margin {:.2} ms{margins}, {misses} of {frames} frames in a run missed their refresh ({pause_misses} of {pause_frames} after pause); paused {pauses} times{}",
                s.id,
                ms(p50),
                ms(p95),
                ms(max),
                ms(s.schedule.budget().unwrap_or_default()),
                ms(s.schedule.margin()),
                if paused { " (now)" } else { "" },
            );
            if let (Some(sub), Some(flip)) = s.schedule.take_timings() {
                log_line!(
                    "[infynitae] Frame timing {:?}: refresh to submit p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms; submit to refresh p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms",
                    s.id,
                    ms(sub.0),
                    ms(sub.1),
                    ms(sub.2),
                    ms(flip.0),
                    ms(flip.1),
                    ms(flip.2),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn turn_clock_marks_only_when_enabled() {
        use std::time::{Duration, Instant};
        let threshold = super::SLOW_TURN_FALLBACK;
        let mut off = super::TurnClock::off();
        assert!(!off.is_enabled());
        off.mark("config reload");
        off.mark("cursor");
        assert!(off.phases.is_empty());
        assert_eq!(off.phases.capacity(), 0);
        assert_eq!(off.slow_parts(Duration::from_millis(50), threshold), None);
        let mut log = super::SlowTurnLog::default();
        assert_eq!(
            log.record_at(&off, Duration::from_millis(50), threshold, Instant::now()),
            None
        );
        assert_eq!(log.flush(), None);

        let mut on = super::TurnClock::new(Instant::now());
        assert!(on.is_enabled());
        on.mark("config reload");
        on.mark("cursor");
        assert_eq!(
            on.phases.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            ["config reload", "cursor"]
        );
    }

    #[test]
    fn a_slow_turn_names_its_slowest_parts() {
        use std::time::Duration;
        let threshold = super::SLOW_TURN_FALLBACK;
        let mut c = super::TurnClock::new(std::time::Instant::now());
        c.phases = vec![
            ("client requests", Duration::from_millis(15)),
            ("background priority", Duration::from_micros(500)),
            ("render", Duration::from_millis(40)),
        ];
        assert_eq!(c.slow_parts(Duration::from_millis(7), threshold), None);
        assert_eq!(
            c.slow_parts(Duration::from_micros(60_500), threshold)
                .as_deref(),
            Some(
                "[infynitae] Slow turn 60.5 ms: render 40.0 ms, client requests 15.0 ms, other 5.0 ms"
            )
        );
    }

    /// A slow "layers above" or "windows" says which surface took the time.
    #[test]
    fn a_slow_turn_names_its_slowest_surface() {
        use std::time::Duration;
        let mut c = super::TurnClock::new(std::time::Instant::now());
        c.phases = vec![("layers above", Duration::from_millis(7))];
        c.surface = Some("infynitae-edge-apps, shm 1920x1080 buffer, 6.9 ms".into());
        assert_eq!(
            c.slow_parts(Duration::from_millis(7), super::SLOW_TURN_FALLBACK),
            None
        );
        assert_eq!(
            c.slow_parts(Duration::from_millis(8), super::SLOW_TURN_FALLBACK)
                .as_deref(),
            Some(
                "[infynitae] Slow turn 8.0 ms: layers above 7.0 ms, other 1.0 ms; slowest surface infynitae-edge-apps, shm 1920x1080 buffer, 6.9 ms"
            )
        );
    }

    #[test]
    fn slow_turn_aggregates_repeated_phases() {
        use std::time::Duration;
        let mut c = super::TurnClock::new(std::time::Instant::now());
        c.phases = vec![
            ("element building", Duration::from_millis(3)),
            ("scanout decision", Duration::from_millis(2)),
            ("element building", Duration::from_millis(4)),
            ("render_frame", Duration::from_millis(5)),
        ];
        assert_eq!(
            c.slow_parts(Duration::from_millis(16), super::SLOW_TURN_FALLBACK)
                .as_deref(),
            Some(
                "[infynitae] Slow turn 16.0 ms: element building 7.0 ms, render_frame 5.0 ms, scanout decision 2.0 ms, other 2.0 ms"
            )
        );
    }

    #[test]
    fn slow_turn_reports_worst_of_each_second() {
        use std::time::{Duration, Instant};
        let threshold = Duration::from_nanos(6_060_606); // 165 Hz
        let mut log = super::SlowTurnLog::default();
        let t0 = Instant::now();

        let mut mild = super::TurnClock::new(t0);
        mild.phases = vec![("capture readback", Duration::from_millis(7))];
        let mut worst = super::TurnClock::new(t0);
        worst.phases = vec![("render_frame", Duration::from_millis(40))];
        let mut tail = super::TurnClock::new(t0);
        tail.phases = vec![("flush", Duration::from_millis(10))];

        // Earlier slow turn opens the 1 s window without immediately printing.
        assert_eq!(
            log.record_at(&mild, Duration::from_millis(7), threshold, t0),
            None
        );
        // Slower turn within the same second replaces it; a milder one after does not.
        assert_eq!(
            log.record_at(
                &worst,
                Duration::from_millis(40),
                threshold,
                t0 + Duration::from_millis(100),
            ),
            None
        );
        assert_eq!(
            log.record_at(
                &tail,
                Duration::from_millis(10),
                threshold,
                t0 + Duration::from_millis(200),
            ),
            None
        );
        // After 1 second, the slowest turn of that second is emitted.
        let due = log.take_due_at(t0 + Duration::from_millis(1000)).unwrap();
        assert!(due.starts_with("[infynitae] Slow turn 40.0 ms: render_frame 40.0 ms"));
        assert_eq!(log.take_due_at(t0 + Duration::from_millis(1100)), None);
    }

    #[test]
    fn slow_turn_threshold_matches_one_refresh() {
        use std::time::Duration;
        let period_165hz = Duration::from_nanos(6_060_606);
        let mut c = super::TurnClock::new(std::time::Instant::now());
        c.phases = vec![("render_frame", Duration::from_millis(6))];
        assert_eq!(c.slow_parts(Duration::from_millis(6), period_165hz), None);
        assert!(
            c.slow_parts(Duration::from_micros(6_100), period_165hz)
                .is_some()
        );
        assert_eq!(super::slow_turn_threshold(&[]), super::SLOW_TURN_FALLBACK);
    }
}
