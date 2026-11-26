//! Worker threads: the sources' fetch schedule and workers, the app list refresh, the per-card command queue.

use super::*;

/// What comes back to the loop from a worker thread.
pub(super) enum Fetched {
    /// Result from a source's worker along with the `Source` object so its state (such as CPU
    /// `/proc/stat` deltas) persists across fetches while the panel stays open.
    Source(&'static str, &'static str, Data, Box<dyn Source>),
    /// One-off result from `Action::Run` after a card command finishes.
    Data(&'static str, &'static str, Data),
    /// Exit status and bounded last stderr line from a card's shell command.
    CommandDone(String, CommandResult),
    /// Installed apps re-read on a worker thread along with the folder mtimes statted before the
    /// read started.
    Apps(Vec<crate::app::App>, Vec<Option<std::time::SystemTime>>),
}

pub(super) type SourceId = (&'static str, &'static str);

pub(super) struct SourceEntry {
    pub(super) last: Instant,
    pub(super) every: Option<Duration>,
    pub(super) source: Option<Box<dyn Source>>,
}

/// When each source on the open panel last ran, its interval (`None` = once per panel opening),
/// and its live `Source` object kept between fetches while the panel remains open.
#[derive(Default)]
pub(super) struct FetchSchedule {
    pub(super) open_panel: Option<usize>,
    pub(super) records: HashMap<SourceId, SourceEntry>,
    pub(super) in_flight: HashMap<SourceId, Option<Duration>>,
}

impl FetchSchedule {
    pub(super) fn sync_panel(&mut self, open: Option<usize>, sources: &[SourceId]) {
        if self.open_panel != open {
            self.open_panel = open;
            self.records.clear();
        }
        if open.is_none() {
            return;
        }
        self.records.retain(|id, _| sources.contains(id));
    }

    pub(super) fn take_due(
        &mut self,
        open: Option<usize>,
        sources: &[SourceId],
        force: bool,
        now: Instant,
        mut make: impl FnMut(&'static str, &'static str) -> Option<Box<dyn Source>>,
    ) -> Vec<(SourceId, Box<dyn Source>)> {
        self.sync_panel(open, sources);
        if open.is_none() {
            return Vec::new();
        }
        let mut due_list = Vec::new();
        for &id in sources {
            if let Some(&every) = self.in_flight.get(&id) {
                // Remember in-flight interval across reopen so finishing before close-reopen settle
                // does not schedule a duplicate fetch for the same opening.
                self.records.entry(id).or_insert(SourceEntry {
                    last: now,
                    every,
                    source: None,
                });
                continue;
            }
            let due = match self.records.get(&id) {
                None => true,
                Some(_) if force => true,
                Some(entry) => entry
                    .every
                    .is_some_and(|every| now.saturating_duration_since(entry.last) >= every),
            };
            if !due {
                continue;
            }
            let (src, every) = match self.records.get_mut(&id) {
                Some(entry) => {
                    let src = match entry.source.take() {
                        Some(s) => s,
                        None => match make(id.0, id.1) {
                            Some(s) => s,
                            None => continue,
                        },
                    };
                    let every = src.every();
                    entry.last = now;
                    entry.every = every;
                    (src, every)
                }
                None => {
                    let Some(src) = make(id.0, id.1) else {
                        continue;
                    };
                    let every = src.every();
                    self.records.insert(
                        id,
                        SourceEntry {
                            last: now,
                            every,
                            source: None,
                        },
                    );
                    (src, every)
                }
            };
            self.in_flight.insert(id, every);
            due_list.push((id, src));
        }
        due_list
    }

    pub(super) fn mark_done(&mut self, id: SourceId, src: Option<Box<dyn Source>>, now: Instant) {
        self.in_flight.remove(&id);
        if self.open_panel.is_some() {
            if let Some(entry) = self.records.get_mut(&id) {
                entry.last = now;
                if let Some(s) = src {
                    entry.source = Some(s);
                }
            }
        }
    }

    pub(super) fn next_timeout(&self, sources: &[SourceId], now: Instant) -> Option<Duration> {
        self.open_panel?;
        sources
            .iter()
            .filter(|id| !self.in_flight.contains_key(*id))
            .filter_map(|id| {
                let entry = self.records.get(id)?;
                let every = entry.every?;
                let due_at = entry.last.checked_add(every)?;
                Some(due_at.saturating_duration_since(now))
            })
            .min()
    }
}

pub(super) struct SourceWorker {
    pub(super) tx: std::sync::mpsc::Sender<Box<dyn Source>>,
    #[cfg(test)]
    pub(super) handle: Option<std::thread::JoinHandle<()>>,
}

pub(super) fn spawn_worker(id: SourceId, fetch_tx: Sender<Fetched>) -> SourceWorker {
    let (tx, rx) = std::sync::mpsc::channel::<Box<dyn Source>>();
    let (prov, key) = id;
    let run = move || {
        while let Ok(mut src) = rx.recv() {
            let data = src.fetch();
            if fetch_tx
                .send(Fetched::Source(prov, key, data, src))
                .is_err()
            {
                break;
            }
        }
    };
    let _handle = match std::thread::Builder::new()
        .name(format!("edge-{prov}-{key}"))
        .spawn(run)
    {
        Ok(h) => Some(h),
        Err(e) => {
            eprintln!("infynitae-edge-apps: spawn worker {prov}/{key}: {e}");
            None
        }
    };
    SourceWorker {
        tx,
        #[cfg(test)]
        handle: _handle,
    }
}

/// Tracks desktop-entry folder modification times so `app::load` runs on a worker thread only
/// when a panel opens and at least one folder's modification time changed.
#[derive(Default)]
pub(super) struct AppRefresh {
    pub(super) dirs: Vec<std::path::PathBuf>,
    pub(super) last_mtimes: Vec<Option<std::time::SystemTime>>,
    pub(super) last_open: Option<usize>,
    pub(super) in_flight: bool,
}

impl AppRefresh {
    pub(super) fn new(
        dirs: Vec<std::path::PathBuf>,
        last_mtimes: Vec<Option<std::time::SystemTime>>,
    ) -> Self {
        Self {
            dirs,
            last_mtimes,
            last_open: None,
            in_flight: false,
        }
    }

    pub(super) fn sync_closed(&mut self) {
        self.last_open = None;
    }

    pub(super) fn check_open(
        &mut self,
        open: Option<usize>,
    ) -> Option<(Vec<std::path::PathBuf>, Vec<Option<std::time::SystemTime>>)> {
        let Some(idx) = open else {
            self.last_open = None;
            return None;
        };
        if self.last_open == Some(idx) {
            return None;
        }
        self.last_open = Some(idx);
        if self.in_flight || self.dirs.is_empty() {
            return None;
        }
        let mtimes = crate::app::dir_mtimes(&self.dirs);
        if mtimes == self.last_mtimes {
            return None;
        }
        self.in_flight = true;
        Some((self.dirs.clone(), mtimes))
    }

    pub(super) fn mark_done(&mut self, mtimes: Vec<Option<std::time::SystemTime>>, applied: bool) {
        self.in_flight = false;
        if applied {
            self.last_mtimes = mtimes;
        }
    }
}

pub(super) fn spawn_apps_worker(
    dirs: Vec<std::path::PathBuf>,
    mtimes: Vec<Option<std::time::SystemTime>>,
    fetch_tx: Sender<Fetched>,
) -> bool {
    match std::thread::Builder::new()
        .name("edge-apps-scan".into())
        .spawn(move || {
            let apps = crate::app::load(&dirs);
            let _ = fetch_tx.send(Fetched::Apps(apps, mtimes));
        }) {
        Ok(_) => true,
        Err(e) => {
            eprintln!("infynitae-edge-apps: spawn apps worker: {e}");
            false
        }
    }
}

/// Per-card queue for `Action::Run`: runs one command at a time and keeps distinct waiting
/// commands in FIFO order; an identical waiting command replaces its existing waiting copy.
#[derive(Default)]
pub(super) struct CommandQueue {
    pub(super) active: HashMap<String, VecDeque<String>>,
}

impl CommandQueue {
    /// Enqueues `cmd` for `card`. Returns `true` when the caller must start the worker.
    pub(super) fn enqueue(&mut self, card: &str, cmd: String) -> bool {
        if let Some(waiting) = self.active.get_mut(card) {
            if let Some(slot) = waiting.iter_mut().find(|w| *w == &cmd) {
                *slot = cmd;
            } else {
                waiting.push_back(cmd);
            }
            false
        } else {
            self.active.insert(card.to_string(), VecDeque::new());
            true
        }
    }

    pub(super) fn pop_next(&mut self, card: &str) -> Option<String> {
        let waiting = self.active.get_mut(card)?;
        match waiting.pop_front() {
            Some(next) => Some(next),
            None => {
                self.active.remove(card);
                None
            }
        }
    }

    #[cfg(test)]
    pub(super) fn set_busy(&mut self, card: &str) {
        self.active.entry(card.to_string()).or_default();
    }

    #[cfg(test)]
    pub(super) fn waiting(&self, card: &str) -> Vec<String> {
        self.active
            .get(card)
            .map(|q| q.iter().cloned().collect())
            .unwrap_or_default()
    }
}

pub(super) fn run_queued_with<F>(
    queued: &std::sync::Mutex<CommandQueue>,
    card: &str,
    first: String,
    mut run: F,
) where
    F: FnMut(&str),
{
    let mut cmd = first;
    loop {
        run(&cmd);
        let mut q = queued.lock().unwrap_or_else(|p| p.into_inner());
        match q.pop_next(card) {
            Some(next) => cmd = next,
            None => break,
        }
    }
}

pub(super) fn run_sh_result_exec(cmd: &str) -> CommandResult {
    match Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
    {
        Ok(out) => CommandResult {
            command: cmd.to_string(),
            status: out.status.code(),
            stderr_line: crate::card::last_stderr_line(&out.stderr),
        },
        Err(e) => {
            eprintln!("infynitae-edge-apps: run {cmd}: {e}");
            CommandResult {
                command: cmd.to_string(),
                status: None,
                stderr_line: crate::card::last_stderr_line(e.to_string().as_bytes()),
            }
        }
    }
}

pub(super) fn run_sh_result(cmd: &str) -> CommandResult {
    // a test never runs it from run_actions: it would change the machine's settings
    #[cfg(test)]
    return CommandResult {
        command: cmd.to_string(),
        status: Some(0),
        stderr_line: String::new(),
    };
    #[cfg(not(test))]
    run_sh_result_exec(cmd)
}

pub(super) fn refresh_sources(tx: &Sender<Fetched>, prov: &'static str, keys: &[&'static str]) {
    for &key in keys {
        if let Some(mut src) = crate::provider::by_id(prov).and_then(|p| p.source(key)) {
            let _ = tx.send(Fetched::Data(prov, key, src.fetch()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wayland::tests::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[test]
    fn none_interval_fetches_once_until_reopen_or_switch() {
        let mut sched = FetchSchedule::default();
        let once_calls = Arc::new(AtomicUsize::new(0));
        let specs = [(("about", "info"), &once_calls, None)];
        let ids = [("about", "info")];
        let t0 = Instant::now();

        assert_eq!(
            run_due(&mut sched, Some(0), &specs, false, t0),
            vec![("about", "info")]
        );
        assert_eq!(once_calls.load(Ordering::Relaxed), 1);
        sched.mark_done(("about", "info"), None, t0 + Duration::from_millis(10));

        // While the same panel stays open, it never fetches again and needs no wakeup timeout.
        assert_eq!(sched.next_timeout(&ids, t0 + Duration::from_secs(1)), None);
        assert!(run_due(
            &mut sched,
            Some(0),
            &specs,
            false,
            t0 + Duration::from_secs(60)
        )
        .is_empty());
        assert_eq!(once_calls.load(Ordering::Relaxed), 1);

        // Closing and reopening the same panel fetches once again.
        assert!(run_due(
            &mut sched,
            None,
            &specs,
            false,
            t0 + Duration::from_secs(61)
        )
        .is_empty());
        assert_eq!(sched.next_timeout(&ids, t0 + Duration::from_secs(61)), None);
        assert_eq!(
            run_due(
                &mut sched,
                Some(0),
                &specs,
                false,
                t0 + Duration::from_secs(62)
            ),
            vec![("about", "info")]
        );
        assert_eq!(once_calls.load(Ordering::Relaxed), 2);
        sched.mark_done(("about", "info"), None, t0 + Duration::from_secs(62));

        // Switching directly to another panel also treats the new opening as fresh.
        assert_eq!(
            run_due(
                &mut sched,
                Some(1),
                &specs,
                false,
                t0 + Duration::from_secs(63)
            ),
            vec![("about", "info")]
        );
        assert_eq!(once_calls.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn honours_each_periodic_source_interval_and_next_timeout() {
        let mut sched = FetchSchedule::default();
        let once_calls = Arc::new(AtomicUsize::new(0));
        let fast_calls = Arc::new(AtomicUsize::new(0));
        let slow_calls = Arc::new(AtomicUsize::new(0));
        let specs = [
            (("about", "info"), &once_calls, None),
            (("audio", "sink"), &fast_calls, Some(Duration::from_secs(2))),
            (
                ("weather", "now"),
                &slow_calls,
                Some(Duration::from_secs(10)),
            ),
        ];
        let ids: Vec<SourceId> = specs.iter().map(|(id, _, _)| *id).collect();
        let t0 = Instant::now();

        // Opening fetches all three once; while all three are in flight no timer wakeup is needed.
        let initial = run_due(&mut sched, Some(0), &specs, false, t0);
        assert_eq!(initial, ids);
        assert_eq!(sched.next_timeout(&ids, t0), None);

        // Complete at distinct instants; next_timeout tracks the earliest remaining deadline.
        let t_done_fast = t0 + Duration::from_millis(50);
        sched.mark_done(("about", "info"), None, t0 + Duration::from_millis(20));
        sched.mark_done(("audio", "sink"), None, t_done_fast);
        assert_eq!(
            sched.next_timeout(&ids, t_done_fast),
            Some(Duration::from_secs(2))
        );

        let t_done_slow = t0 + Duration::from_millis(300);
        sched.mark_done(("weather", "now"), None, t_done_slow);
        assert_eq!(
            sched.next_timeout(&ids, t_done_slow),
            Some(Duration::from_millis(1750))
        );

        // Before 2 s elapses from fast's completion, nothing is due.
        assert!(run_due(
            &mut sched,
            Some(0),
            &specs,
            false,
            t_done_fast + Duration::from_millis(1999),
        )
        .is_empty());

        // At 2 s from fast's completion, only fast runs; slow and None stay idle.
        let t_tick1 = t_done_fast + Duration::from_secs(2);
        assert_eq!(
            run_due(&mut sched, Some(0), &specs, false, t_tick1),
            vec![("audio", "sink")]
        );
        assert_eq!(once_calls.load(Ordering::Relaxed), 1);
        assert_eq!(fast_calls.load(Ordering::Relaxed), 2);
        assert_eq!(slow_calls.load(Ordering::Relaxed), 1);

        // While fast is in flight again, next_timeout looks at slow's deadline.
        assert_eq!(
            sched.next_timeout(&ids, t_tick1),
            Some(Duration::from_secs(10) - (t_tick1 - t_done_slow))
        );
        sched.mark_done(("audio", "sink"), None, t_tick1 + Duration::from_millis(20));

        // After 10 s from slow's completion, slow becomes due alongside any due fast interval.
        let t_tick_slow = t_done_slow + Duration::from_secs(10);
        assert_eq!(
            run_due(&mut sched, Some(0), &specs, false, t_tick_slow),
            vec![("audio", "sink"), ("weather", "now")]
        );
        assert_eq!(once_calls.load(Ordering::Relaxed), 1);
        assert_eq!(fast_calls.load(Ordering::Relaxed), 3);
        assert_eq!(slow_calls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn skips_in_flight_reopen_duplicates_and_honours_force() {
        let mut sched = FetchSchedule::default();
        let once_calls = Arc::new(AtomicUsize::new(0));
        let fast_calls = Arc::new(AtomicUsize::new(0));
        let specs = [
            (("about", "info"), &once_calls, None),
            (("audio", "sink"), &fast_calls, Some(Duration::from_secs(2))),
        ];
        let ids: Vec<SourceId> = specs.iter().map(|(id, _, _)| *id).collect();
        let t0 = Instant::now();

        assert_eq!(run_due(&mut sched, Some(0), &specs, false, t0), ids);

        // In-flight sources are never overlapped, even on force.
        assert!(run_due(
            &mut sched,
            Some(0),
            &specs,
            true,
            t0 + Duration::from_millis(10)
        )
        .is_empty());

        // Close and reopen while `about/info` (None) is still in flight, then complete it.
        assert!(run_due(
            &mut sched,
            None,
            &specs,
            false,
            t0 + Duration::from_millis(20)
        )
        .is_empty());
        assert!(run_due(
            &mut sched,
            Some(0),
            &specs,
            false,
            t0 + Duration::from_millis(30)
        )
        .is_empty());
        sched.mark_done(("about", "info"), None, t0 + Duration::from_millis(40));
        sched.mark_done(("audio", "sink"), None, t0 + Duration::from_millis(40));

        // On later ticks in this reopened panel, `about/info` stays single-shot.
        assert!(run_due(
            &mut sched,
            Some(0),
            &specs,
            false,
            t0 + Duration::from_millis(500)
        )
        .is_empty());
        assert_eq!(once_calls.load(Ordering::Relaxed), 1);

        // Forced fetch runs both `None` and not-yet-due periodic sources immediately.
        assert_eq!(
            run_due(
                &mut sched,
                Some(0),
                &specs,
                true,
                t0 + Duration::from_millis(600)
            ),
            ids
        );
        assert_eq!(once_calls.load(Ordering::Relaxed), 2);
        assert_eq!(fast_calls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn removed_source_fetches_again_if_re_added() {
        let mut sched = FetchSchedule::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let full = [(("about", "info"), &calls, None)];
        let empty: [(SourceId, &Arc<AtomicUsize>, Option<Duration>); 0] = [];
        let t0 = Instant::now();

        assert_eq!(
            run_due(&mut sched, Some(0), &full, false, t0),
            vec![("about", "info")]
        );
        sched.mark_done(("about", "info"), None, t0 + Duration::from_millis(10));

        // Card removed from the open panel drops its record; re-adding fetches once again.
        assert!(run_due(
            &mut sched,
            Some(0),
            &empty,
            false,
            t0 + Duration::from_secs(1)
        )
        .is_empty());
        assert_eq!(
            run_due(
                &mut sched,
                Some(0),
                &full,
                false,
                t0 + Duration::from_secs(2)
            ),
            vec![("about", "info")]
        );
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }

    struct InstanceSource {
        calls: usize,
    }

    impl Source for InstanceSource {
        fn fetch(&mut self) -> Data {
            self.calls += 1;
            Box::new((std::thread::current().id(), self.calls))
        }
    }

    #[test]
    fn keeps_source_instance_across_fetches_in_one_opening() {
        let mut sched = FetchSchedule::default();
        let ids = [("monitor", "cpu")];
        let mut make_calls = 0usize;
        let t0 = Instant::now();

        let mut due1 = sched.take_due(Some(0), &ids, false, t0, |_, _| {
            make_calls += 1;
            Some(Box::new(InstanceSource { calls: 0 }))
        });
        assert_eq!(due1.len(), 1);
        let (id1, mut src1) = due1.pop().unwrap();
        let (_, n1) = *src1
            .fetch()
            .downcast::<(std::thread::ThreadId, usize)>()
            .unwrap();
        assert_eq!(n1, 1);
        assert_eq!(make_calls, 1);
        let t_done1 = t0 + Duration::from_millis(10);
        sched.mark_done(id1, Some(src1), t_done1);

        let t1 = t_done1 + Duration::from_secs(2);
        let mut due2 = sched.take_due(Some(0), &ids, false, t1, |_, _| {
            make_calls += 1;
            Some(Box::new(InstanceSource { calls: 0 }))
        });
        assert_eq!(due2.len(), 1);
        let (id2, mut src2) = due2.pop().unwrap();
        let (_, n2) = *src2
            .fetch()
            .downcast::<(std::thread::ThreadId, usize)>()
            .unwrap();
        assert_eq!(n2, 2, "second fetch must reuse the same Source instance");
        assert_eq!(
            make_calls, 1,
            "source must not be rebuilt while panel stays open"
        );
        sched.mark_done(id2, Some(src2), t1 + Duration::from_millis(10));

        // Closing and reopening builds a fresh Source.
        assert!(sched
            .take_due(None, &[], false, t1 + Duration::from_secs(1), |_, _| None)
            .is_empty());
        let mut due3 = sched.take_due(Some(0), &ids, false, t1 + Duration::from_secs(2), |_, _| {
            make_calls += 1;
            Some(Box::new(InstanceSource { calls: 0 }))
        });
        let (_, mut src3) = due3.pop().unwrap();
        let (_, n3) = *src3
            .fetch()
            .downcast::<(std::thread::ThreadId, usize)>()
            .unwrap();
        assert_eq!(n3, 1);
        assert_eq!(make_calls, 2);
    }

    #[test]
    fn worker_reuses_one_thread_while_open_and_stops_on_close() {
        type WorkerMsg = (std::thread::ThreadId, usize, Box<dyn Source>);
        let (fetch_tx, fetch_rx) = channel::channel::<Fetched>();
        let mut event_loop: EventLoop<Option<WorkerMsg>> = EventLoop::try_new().unwrap();
        event_loop
            .handle()
            .insert_source(fetch_rx, |evt, _, out| {
                if let channel::Event::Msg(Fetched::Source(_, _, data, src)) = evt {
                    let (tid, n) = *data.downcast::<(std::thread::ThreadId, usize)>().unwrap();
                    *out = Some((tid, n, src));
                }
            })
            .unwrap();

        let mut worker = spawn_worker(("monitor", "cpu"), fetch_tx);
        let src: Box<dyn Source> = Box::new(InstanceSource { calls: 0 });
        worker.tx.send(src).unwrap();
        let mut res = None;
        while res.is_none() {
            event_loop
                .dispatch(Some(Duration::from_millis(100)), &mut res)
                .unwrap();
        }
        let (tid1, n1, src) = res.take().unwrap();
        assert_eq!(n1, 1);

        worker.tx.send(src).unwrap();
        while res.is_none() {
            event_loop
                .dispatch(Some(Duration::from_millis(100)), &mut res)
                .unwrap();
        }
        let (tid2, n2, _src) = res.take().unwrap();
        assert_eq!(n2, 2);
        assert_eq!(tid1, tid2, "same worker thread serves both fetches");

        let handle = worker.handle.take().unwrap();
        drop(worker);
        handle.join().unwrap();
    }

    #[test]
    fn cpu_source_reports_percent_on_second_fetch_of_one_opening() {
        let dir =
            std::env::temp_dir().join(format!("infynitae-edge-apps-f1-cpu-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("stat"), "cpu  100 0 50 850 0 0 0 0\n").unwrap();
        std::env::set_var("INFYNITAE_PROC_DIR", &dir);

        let mut sched = FetchSchedule::default();
        let ids = [("monitor", "cpu")];
        let t0 = Instant::now();
        let make = |p, k| crate::provider::by_id(p).and_then(|prov| prov.source(k));

        let mut due1 = sched.take_due(Some(0), &ids, false, t0, make);
        assert_eq!(due1.len(), 1);
        let (id1, mut src1) = due1.pop().unwrap();
        let d1 = src1
            .fetch()
            .downcast::<crate::cards::monitor::CpuData>()
            .unwrap();
        assert_eq!(d1.busy_pct, None);
        let t_done1 = t0 + Duration::from_millis(10);
        sched.mark_done(id1, Some(src1), t_done1);

        std::fs::write(dir.join("stat"), "cpu  180 0 70 1150 0 0 0 0\n").unwrap();
        let t1 = t_done1 + Duration::from_secs(2);
        let mut due2 = sched.take_due(Some(0), &ids, false, t1, make);
        assert_eq!(due2.len(), 1);
        let (id2, mut src2) = due2.pop().unwrap();
        let d2 = src2
            .fetch()
            .downcast::<crate::cards::monitor::CpuData>()
            .unwrap();
        assert_eq!(d2.busy_pct, Some(25));
        sched.mark_done(id2, Some(src2), t1 + Duration::from_millis(10));

        std::env::remove_var("INFYNITAE_PROC_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_config_source_fetches_once_per_open_with_no_timer() {
        let mut sched = FetchSchedule::default();
        let ids = [("settings", "config")];
        let t0 = Instant::now();
        let make = |p, k| crate::provider::by_id(p).and_then(|prov| prov.source(k));

        let mut due1 = sched.take_due(Some(0), &ids, false, t0, make);
        assert_eq!(due1.len(), 1);
        let (id, mut src) = due1.pop().unwrap();
        assert_eq!(src.every(), None);
        let _ = src.fetch();
        sched.mark_done(id, Some(src), t0 + Duration::from_millis(10));

        assert_eq!(sched.next_timeout(&ids, t0 + Duration::from_secs(1)), None);
        assert!(sched
            .take_due(Some(0), &ids, false, t0 + Duration::from_secs(60), make)
            .is_empty());
    }

    #[test]
    fn apps_refresh_skips_when_closed_and_reloads_on_worker_when_opened() {
        let dir = std::env::temp_dir().join(format!(
            "infynitae-wayland-apps-refresh-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let apps_dir = dir.join("applications");
        std::fs::create_dir_all(&apps_dir).unwrap();

        let dirs = vec![apps_dir.clone()];
        let initial_mtimes = crate::app::dir_mtimes(&dirs);
        let mut sh = Shell::new(
            crate::config::Config::from_text("PANEL_APPS=\"SUPER+A top right 380 apps/apps\"\n"),
            crate::app::load(&dirs),
        );
        sh.live = true;
        sh.set_app_dirs_with_mtimes(dirs.clone(), initial_mtimes.clone());
        let mut refresh = AppRefresh::new(dirs, initial_mtimes);

        // Add a .desktop file while all panels are closed.
        std::fs::write(
            apps_dir.join("hello.desktop"),
            "[Desktop Entry]\nName=Hello\nExec=hello\n",
        )
        .unwrap();
        let t1 = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::open(&apps_dir)
            .unwrap()
            .set_modified(t1)
            .unwrap();

        assert!(
            refresh.check_open(None).is_none(),
            "nothing runs while all panels are closed"
        );
        assert!(sh.apps.is_empty());

        // Panel opens: detects changed folder mtime and schedules one worker load.
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "apps")
            .unwrap();
        assert!(sh.open_panel(idx, false, Instant::now()));
        let (scan_dirs, scan_mtimes) = refresh.check_open(Some(idx)).expect("should trigger load");
        assert!(
            refresh.check_open(Some(idx)).is_none(),
            "same opening does not stat or reload again"
        );

        type AppsMsg = (Vec<crate::app::App>, Vec<Option<std::time::SystemTime>>);
        let (fetch_tx, fetch_rx) = channel::channel::<Fetched>();
        let mut event_loop: EventLoop<Option<AppsMsg>> = EventLoop::try_new().unwrap();
        event_loop
            .handle()
            .insert_source(fetch_rx, |evt, _, out| {
                if let channel::Event::Msg(Fetched::Apps(apps, mtimes)) = evt {
                    *out = Some((apps, mtimes));
                }
            })
            .unwrap();

        assert!(spawn_apps_worker(scan_dirs, scan_mtimes, fetch_tx));
        let mut received = None;
        while received.is_none() {
            event_loop
                .dispatch(Some(Duration::from_millis(100)), &mut received)
                .unwrap();
        }
        let (apps, done_mtimes) = received.unwrap();
        refresh.mark_done(done_mtimes, true);
        assert!(sh.apply_apps(apps));
        assert_eq!(sh.open, Some(idx));
        assert_eq!(
            sh.apps.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
            vec!["hello"]
        );

        // Closing and reopening with no folder change does not trigger another load.
        refresh.sync_closed();
        assert!(refresh.check_open(Some(idx)).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn per_card_queue_keeps_distinct_waiting_commands_in_order() {
        let (_mock, conn) = MockWayland::start(
            vec![MockOutputSpec {
                width: 1280,
                height: 800,
                scale: 1,
                fractional_120: 120,
            }],
            false,
        );
        let sh = Shell::new(crate::config::Config::from_text(""), Vec::new());
        let (fetch_tx, _fetch_rx) = channel::channel::<Fetched>();
        let (mut app, _eq) = init_app(&conn, sh, fetch_tx).unwrap();

        // Simulate an active worker for this card so follow-up commands wait in the queue.
        app.queued.lock().unwrap().set_busy("settings/bluetooth");
        app.shell.actions.push((
            Action::Run("bluetoothctl pair A".into()),
            "settings/bluetooth".into(),
            "settings",
            &[],
        ));
        app.shell.actions.push((
            Action::Run("bluetoothctl connect B".into()),
            "settings/bluetooth".into(),
            "settings",
            &[],
        ));
        // A duplicate of an already waiting command replaces its waiting copy without changing order.
        app.shell.actions.push((
            Action::Run("bluetoothctl pair A".into()),
            "settings/bluetooth".into(),
            "settings",
            &[],
        ));
        // Background actions bypass the per-card queue.
        app.shell.actions.push((
            Action::RunBackground("bluetoothctl --timeout 15 scan on >&2".into()),
            "settings/bluetooth".into(),
            "settings",
            &[],
        ));
        app.run_actions();

        assert_eq!(
            app.queued.lock().unwrap().waiting("settings/bluetooth"),
            vec![
                "bluetoothctl pair A".to_string(),
                "bluetoothctl connect B".to_string(),
            ]
        );
        assert!(
            app.ran
                .contains(&"bluetoothctl --timeout 15 scan on >&2".to_string()),
            "background scan should still run when dispatched: {:?}",
            app.ran
        );

        // Drain via run_queued_with to verify sequential FIFO execution and background independence.
        let q = std::sync::Arc::new(std::sync::Mutex::new(CommandQueue::default()));
        assert!(q.lock().unwrap().enqueue("card", "first".into()));
        assert!(!q.lock().unwrap().enqueue("card", "second".into()));
        assert!(!q.lock().unwrap().enqueue("card", "third".into()));
        assert!(!q.lock().unwrap().enqueue("card", "second".into()));
        // Re-sending the currently running command queues a new run after waiting ones.
        assert!(!q.lock().unwrap().enqueue("card", "first".into()));

        let (gate_tx, gate_rx) = std::sync::mpsc::channel();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (log_tx, log_rx) = std::sync::mpsc::channel();
        let q_worker = q.clone();
        let log_q = log_tx.clone();
        let worker = std::thread::spawn(move || {
            run_queued_with(&q_worker, "card", "first".into(), |c| {
                if c == "first" && started_tx.send(()).is_ok() {
                    let _ = gate_rx.recv();
                }
                let _ = log_q.send(c.to_string());
            });
        });

        started_rx.recv().unwrap();
        // While the queue worker is blocked on "first", an outside background run completes first.
        log_tx.send("scan".to_string()).unwrap();
        gate_tx.send(()).unwrap();
        gate_tx.send(()).unwrap();
        worker.join().unwrap();

        let executed: Vec<String> = log_rx.try_iter().collect();
        assert_eq!(executed, vec!["scan", "first", "second", "third", "first"]);
    }

    #[test]
    fn command_runner_keeps_exit_status_last_stderr_line_and_delivers_command_done() {
        let long_err = run_sh_result_exec(
            "printf 'ignored first line\\n\\033[31m%s\\033[0m\\n\\n' \"$(printf 'e%.0s' $(seq 1 250))\" >&2; exit 4",
        );
        assert_eq!(long_err.status, Some(4));
        assert_eq!(long_err.stderr_line.len(), crate::card::MAX_STDERR_BYTES);
        assert!(long_err.stderr_line.chars().all(|c| c == 'e'));

        let ok = run_sh_result_exec("echo noise; echo ok >&2");
        assert_eq!(ok.status, Some(0));
        assert_eq!(ok.stderr_line, "ok");

        // Run a real queued worker with two commands and verify CommandDone messages arrive in order.
        let q = std::sync::Arc::new(std::sync::Mutex::new(CommandQueue::default()));
        let cmd1 = "echo 'Error: Connection activation failed: Secrets were required, but not provided.' >&2; exit 4";
        let cmd2 = "true";
        assert!(q.lock().unwrap().enqueue("settings/network", cmd1.into()));
        assert!(!q.lock().unwrap().enqueue("settings/network", cmd2.into()));

        let (tx, rx) = channel::channel::<Fetched>();
        let q_w = q.clone();
        std::thread::spawn(move || {
            run_queued_with(&q_w, "settings/network", cmd1.into(), |c| {
                let res = run_sh_result_exec(c);
                let _ = tx.send(Fetched::CommandDone("settings/network".into(), res));
            });
        });

        let mut results = Vec::new();
        while results.len() < 2 {
            if let Ok(Fetched::CommandDone(card, res)) = rx.recv() {
                assert_eq!(card, "settings/network");
                results.push(res);
            }
        }
        assert_eq!(results[0].status, Some(4));
        assert_eq!(
            results[0].stderr_line,
            "Error: Connection activation failed: Secrets were required, but not provided."
        );
        assert_eq!(results[1].status, Some(0));

        let mut sh = Shell::new(
            crate::config::Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 settings/network\"\n",
            ),
            Vec::new(),
        );
        let now = Instant::now();
        assert!(sh.command_done("settings/network", &results[0], now));
        assert!(sh.card_deadline().is_some());
        assert!(sh.command_done("settings/network", &results[1], now));
        assert_eq!(sh.card_deadline(), None);
    }

    /// The launcher's file search: plocate runs off the loop, the next keystroke kills the run
    /// before (only its answer reaches the card), and closing the panel kills the one in flight.
    #[test]
    fn the_launcher_s_file_search_is_cancelled_by_the_next_keystroke_and_by_closing() {
        let pid_file = crate::locate::tests::stub("harness", "")
            .parent()
            .unwrap()
            .join("pid");
        let plocate = crate::locate::tests::stub(
            "harness",
            &format!(
                r#"if [ "$6" = slow ]; then echo $$ > {}; exec sleep 30; fi; echo "/home/a/$6.txt""#,
                pid_file.display()
            ),
        );
        let wait_pid = || {
            let t0 = Instant::now();
            while !pid_file.exists() && t0.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(5));
            }
            let pid = std::fs::read_to_string(&pid_file).unwrap();
            std::fs::remove_file(&pid_file).unwrap();
            format!("/proc/{}", pid.trim())
        };
        let located = |rx: &channel::Channel<Fetched>, within: Duration| {
            let t0 = Instant::now();
            while t0.elapsed() < within {
                if let Ok(Fetched::Data("apps", "located", data)) = rx.try_recv() {
                    return Some(data);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            None
        };

        let (_mock, conn) = MockWayland::start(
            vec![MockOutputSpec {
                width: 1280,
                height: 800,
                scale: 1,
                fractional_120: 120,
            }],
            false,
        );
        let sh = Shell::new(crate::config::Config::from_text(""), Vec::new());
        let (fetch_tx, fetch_rx) = channel::channel::<Fetched>();
        let (mut app, _eq) = init_app(&conn, sh, fetch_tx).unwrap();
        app.locator = crate::locate::Locator::new(plocate.as_os_str());
        let target = app.key_target_output();
        assert!(app.shell.toggle_named_on(target, "apps"));

        app.shell.type_text("slow");
        app.run_actions();
        let slow = wait_pid();
        app.shell.type_text("fast");
        app.run_actions();
        let data = located(&fetch_rx, Duration::from_secs(5)).expect("the answer for fast");
        assert!(
            !std::path::Path::new(&slow).exists(),
            "the run for the keystroke before still goes"
        );
        assert!(app.shell.apply("apps", "located", data.as_ref()));
        assert!(
            located(&fetch_rx, Duration::from_millis(300)).is_none(),
            "only one answer"
        );
        // no app or other kind matches: Enter opens the file found
        assert!(app.shell.key(crate::shell::Key::Enter));
        assert!(
            app.shell
                .actions
                .iter()
                .any(|(a, ..)| *a == Action::OpenUri("/home/a/fast.txt".into())),
            "{:?}",
            app.shell
                .actions
                .iter()
                .map(|(a, ..)| a)
                .collect::<Vec<_>>()
        );
        app.shell.actions.clear();

        // closing while one runs kills it, and its answer never comes
        app.shell.close();
        assert!(app.shell.toggle_named_on(target, "apps"));
        app.shell.type_text("slow");
        app.run_actions();
        let slow = wait_pid();
        app.shell.close();
        app.fetch_due(false);
        assert!(
            !std::path::Path::new(&slow).exists(),
            "plocate outlived the panel"
        );
        assert!(located(&fetch_rx, Duration::from_millis(300)).is_none());
        let _ = std::fs::remove_dir_all(plocate.parent().unwrap());
    }
}
