//! Which processes have a sound stream running, playing or recording (background priority: an
//! app doing either keeps full priority out of sight). PipeWire's PulseAudio server lists them,
//! as the edge's Sound card reads them (`pactl`). One thread runs `pactl subscribe`, which sleeps
//! until a stream comes, goes or changes, and then lists the streams again; nothing runs while
//! nothing changes. Without `pactl` (or with PipeWire stopped) no app counts as playing.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::log_line;

/// How long after the watcher ended (PipeWire restarted, `pactl` missing) it may start again.
const RESTART_AFTER: Duration = Duration::from_secs(60);

/// The running streams: for each, its process and that process's ancestors, nearest first.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Running {
    pub playing: Vec<Vec<u32>>,
    pub recording: Vec<Vec<u32>>,
}

impl Running {
    /// Whether a stream of `app`'s own tree runs: the nearest windowed process above the
    /// stream's (or the stream's own) is `app` (a terminal that started a player is not playing).
    pub fn of(chains: &[Vec<u32>], app: u32, windowed: impl Fn(u32) -> bool) -> bool {
        chains
            .iter()
            .any(|chain| chain.iter().copied().find(|&p| windowed(p)) == Some(app))
    }
}

/// The watcher thread's state, shared with the main loop.
#[derive(Default)]
struct Shared {
    running: Running,
    ended: bool,
}

/// The watcher, started at the first need.
#[derive(Default)]
pub struct Streams {
    shared: Option<Arc<Mutex<Shared>>>,
    started_at: Option<Instant>,
}

impl Streams {
    /// The streams running now, as last listed; starts the watcher first if it is not running
    /// (again [`RESTART_AFTER`] after it ended).
    pub fn running(&mut self, now: Instant) -> Running {
        let ended = self
            .shared
            .as_ref()
            .is_none_or(|s| s.lock().map_or(true, |s| s.ended));
        if ended
            && self
                .started_at
                .is_none_or(|t| now.duration_since(t) >= RESTART_AFTER)
        {
            self.start(now);
        }
        self.shared
            .as_ref()
            .and_then(|s| s.lock().ok().map(|s| s.running.clone()))
            .unwrap_or_default()
    }

    fn start(&mut self, now: Instant) {
        let first = self.started_at.is_none();
        self.started_at = Some(now);
        let shared = Arc::new(Mutex::new(Shared::default()));
        self.shared = Some(shared.clone());
        let child = crate::ownership::spawn_owned(
            Command::new("pactl")
                .arg("subscribe")
                .env("LC_ALL", "C")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null()),
        );
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                if first {
                    log_line!("[infynitae] Background priority: no pactl ({e}); sound is not seen");
                }
                shared.lock().map(|mut s| s.ended = true).ok();
                return;
            }
        };
        let Some(out) = child.stdout.take() else {
            return;
        };
        let spawned = std::thread::Builder::new()
            .name("infynitae-streams".into())
            .spawn(move || {
                watch(BufReader::new(out), &shared, list_running);
                let _ = child.kill();
                let _ = child.wait();
                crate::ownership::forget_spawned(child.id());
                if let Ok(mut s) = shared.lock() {
                    s.running = Running::default();
                    s.ended = true;
                }
            });
        if let Err(e) = spawned {
            log_line!("[infynitae] Background priority: no stream thread ({e}); sound is not seen");
        }
    }
}

/// Lists the streams at once and again after each burst of stream events, until `pactl
/// subscribe` ends.
fn watch<R: std::io::Read>(
    mut events: BufReader<R>,
    shared: &Mutex<Shared>,
    list: impl Fn() -> Running,
) {
    let mut dirty = true;
    let mut line = String::new();
    loop {
        // the events already read come first: a burst (a stream's new and its changes) is one
        // listing
        if dirty && events.buffer().is_empty() {
            dirty = false;
            let running = list();
            match shared.lock() {
                Ok(mut s) => s.running = running,
                Err(_) => return,
            }
        }
        line.clear();
        match events.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => dirty |= line.contains("sink-input") || line.contains("source-output"),
        }
    }
}

/// The streams running now, from `pactl list`.
fn list_running() -> Running {
    Running {
        playing: chains(&list("sink-inputs")),
        recording: chains(&list("source-outputs")),
    }
}

/// `pactl list <what>`'s output; empty if it failed.
fn list(what: &str) -> String {
    Command::new("pactl")
        .args(["list", what])
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// The process ids of the streams in a `pactl list sink-inputs` or `source-outputs` that run
/// (not corked).
pub fn running_pids(out: &str) -> Vec<u32> {
    let mut pids = Vec::new();
    let mut stream: Option<(bool, Option<u32>)> = None;
    let done = |s: Option<(bool, Option<u32>)>, pids: &mut Vec<u32>| {
        if let Some((false, Some(pid))) = s {
            pids.push(pid);
        }
    };
    for line in out.lines() {
        if !line.starts_with(char::is_whitespace) {
            done(stream.take(), &mut pids);
            if line.starts_with("Sink Input #") || line.starts_with("Source Output #") {
                stream = Some((false, None));
            }
            continue;
        }
        let Some((corked, pid)) = stream.as_mut() else {
            continue;
        };
        let line = line.trim();
        if let Some(v) = line.strip_prefix("Corked:") {
            *corked = v.trim() != "no";
        } else if let Some(v) = line.strip_prefix("application.process.id = ") {
            *pid = v.trim().trim_matches('"').parse().ok();
        }
    }
    done(stream, &mut pids);
    pids
}

/// Each running stream's process and its ancestors, nearest first.
fn chains(out: &str) -> Vec<Vec<u32>> {
    running_pids(out)
        .into_iter()
        .map(|pid| {
            let mut chain = vec![pid];
            while let Some((_, ppid)) = crate::ownership::stat_of(*chain.last().unwrap() as i32)
                && ppid > 1
                && chain.len() < 64
            {
                chain.push(ppid as u32);
            }
            chain
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// From `LC_ALL=C pactl list sink-inputs` on PipeWire 1.2 (two streams, trimmed): one playing,
    /// one paused.
    const SINK_INPUTS: &str = "Sink Input #63
\tDriver: PipeWire
\tOwner Module: n/a
\tClient: 61
\tSink: 52
\tCorked: no
\tMute: no
\tProperties:
\t\tapplication.name = \"Firefox\"
\t\tapplication.process.id = \"4242\"
\t\tmedia.name = \"AudioStream\"

Sink Input #71
\tDriver: PipeWire
\tCorked: yes
\tProperties:
\t\tapplication.name = \"spotify\"
\t\tapplication.process.id = \"5151\"
";

    #[test]
    fn only_streams_not_corked_count() {
        assert_eq!(running_pids(SINK_INPUTS), vec![4242]);
        assert_eq!(running_pids(""), Vec::<u32>::new());
        let recording =
            "Source Output #9\n\tCorked: no\n\tProperties:\n\t\tapplication.process.id = \"77\"\n";
        assert_eq!(running_pids(recording), vec![77]);
    }

    /// A stream of a browser's content process is the browser's; a player started from a
    /// terminal is the player's, never the terminal's.
    #[test]
    fn a_stream_belongs_to_the_nearest_app_with_windows() {
        let chains = vec![vec![300, 200, 100]];
        let windowed = |p| p == 200 || p == 100;
        assert!(Running::of(&chains, 200, windowed));
        assert!(!Running::of(&chains, 100, windowed), "the terminal above");
        assert!(!Running::of(&chains, 300, windowed), "no window of its own");
    }

    /// The first listing comes before any event; a burst of stream events read at once is one
    /// more; an event of another kind is none. The sound server is not asked (`list` counts).
    #[test]
    fn a_burst_of_events_is_one_listing() {
        let count = |events: &str| {
            let shared = Mutex::new(Shared::default());
            let n = std::cell::Cell::new(0);
            watch(BufReader::new(events.as_bytes()), &shared, || {
                n.set(n.get() + 1);
                Running {
                    playing: vec![vec![n.get()]],
                    recording: vec![],
                }
            });
            (n.get(), shared.into_inner().unwrap().running.playing)
        };
        assert_eq!(count(""), (1, vec![vec![1]]), "listed at once");
        assert_eq!(count("Event 'change' on client #3\n"), (1, vec![vec![1]]));
        // a reader of one byte at a time sees each event alone: the start, then each stream event
        let burst = "Event 'new' on sink-input #1\nEvent 'change' on source-output #1\n";
        assert_eq!(
            count(burst).0,
            2,
            "read in one go: the start, then one for the burst"
        );
        let shared = Mutex::new(Shared::default());
        let n = std::cell::Cell::new(0);
        watch(
            BufReader::with_capacity(1, burst.as_bytes()),
            &shared,
            || {
                n.set(n.get() + 1);
                Running::default()
            },
        );
        assert_eq!(n.get(), 3);
    }
}
