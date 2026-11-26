//! Files by name for the launcher's search (`Action::Locate`): plocate on a thread of its own, the
//! run before killed by the next keystroke so fast typing never piles up searches. plocate reads
//! its own database, which its daily timer updates; the edge never runs updatedb.

use std::ffi::OsString;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};

/// Paths asked of plocate for one query; the card shows the first few that match every word.
pub const LIMIT: usize = 40;

/// What one plocate run found, for the query it ran with (the card drops an answer to an older one).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Located {
    pub query: String,
    pub paths: Vec<String>,
}

#[derive(Default)]
struct Run {
    cancelled: bool,
    child: Option<Child>,
}

fn lock(run: &Mutex<Run>) -> MutexGuard<'_, Run> {
    run.lock().unwrap_or_else(|p| p.into_inner())
}

pub struct Locator {
    program: OsString,
    run: Option<Arc<Mutex<Run>>>,
}

impl Locator {
    pub fn new(program: impl Into<OsString>) -> Locator {
        Locator {
            program: program.into(),
            run: None,
        }
    }

    /// Kills the run in flight, if any; its answer is never sent.
    pub fn cancel(&mut self) {
        let Some(run) = self.run.take() else { return };
        let mut r = lock(&run);
        r.cancelled = true;
        if let Some(mut child) = r.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Cancels the run before and searches `query` (nothing for an empty one); `done` gets the
    /// answer on the worker thread. plocate not installed: no answer (Settings, Apps says so).
    pub fn start(&mut self, query: &str, done: impl FnOnce(Located) + Send + 'static) {
        self.cancel();
        let terms: Vec<String> = query.split_whitespace().map(str::to_string).collect();
        if terms.is_empty() {
            return;
        }
        let run = Arc::new(Mutex::new(Run::default()));
        self.run = Some(run.clone());
        let program = self.program.clone();
        let query = query.trim().to_string();
        let spawned = std::thread::Builder::new()
            .name("edge-locate".into())
            .spawn(move || {
                // spawned under the lock, so a cancel either comes first or finds the child to kill
                let mut stdout = {
                    let mut r = lock(&run);
                    if r.cancelled {
                        return;
                    }
                    let child = Command::new(&program)
                        .args(["-i", "-b", "-l", &LIMIT.to_string(), "--"])
                        .args(&terms)
                        .stdin(Stdio::null())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::null())
                        .spawn();
                    let mut child = match child {
                        Ok(c) => c,
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
                        Err(e) => {
                            eprintln!("infynitae-edge-apps: plocate: {e}");
                            return;
                        }
                    };
                    let Some(out) = child.stdout.take() else {
                        let _ = child.kill();
                        let _ = child.wait();
                        return;
                    };
                    r.child = Some(child);
                    out
                };
                let mut buf = Vec::new();
                // a read error ends the answer where it stopped (a kill closes the pipe too)
                let _ = stdout.read_to_end(&mut buf);
                let mut child = {
                    let mut r = lock(&run);
                    match r.child.take() {
                        Some(c) if !r.cancelled => c,
                        _ => return,
                    }
                };
                let _ = child.wait();
                done(Located {
                    query,
                    paths: String::from_utf8_lossy(&buf)
                        .lines()
                        .filter(|l| !l.is_empty())
                        .map(str::to_string)
                        .collect(),
                });
            });
        if let Err(e) = spawned {
            eprintln!("infynitae-edge-apps: spawn plocate worker: {e}");
            self.run = None;
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    /// A `plocate` shell script in a folder of its own.
    pub(crate) fn stub(tag: &str, body: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "infynitae-edge-locate-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("plocate");
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    #[test]
    fn the_answer_comes_for_its_query_with_the_words_passed_apart() {
        // the stub prints its arguments one per line: the flags, then each word
        let p = stub("args", r#"for a in "$@"; do printf '%s\n' "$a"; done"#);
        let (tx, rx) = mpsc::channel();
        let mut l = Locator::new(p.as_os_str());
        l.start("  tax  2026 ", move |found| tx.send(found).unwrap());
        let found = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(found.query, "tax  2026");
        assert_eq!(found.paths, ["-i", "-b", "-l", "40", "--", "tax", "2026"]);
    }

    #[test]
    fn the_next_keystroke_kills_the_run_before_and_only_its_answer_comes() {
        // "slow" sleeps as a search over a cold database can; the PID file shows it was killed
        let dir = stub("cancel", "").parent().unwrap().to_path_buf();
        let pid_file = dir.join("pid");
        let p = stub(
            "cancel",
            &format!(
                r#"if [ "$6" = slow ]; then echo $$ > {}; exec sleep 30; fi; echo "/home/a/$6.txt""#,
                pid_file.display()
            ),
        );
        let (tx, rx) = mpsc::channel();
        let mut l = Locator::new(p.as_os_str());
        let tx1 = tx.clone();
        l.start("slow", move |found| tx1.send(found).unwrap());
        let t0 = Instant::now();
        while !pid_file.exists() && t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let pid = std::fs::read_to_string(&pid_file).unwrap();
        let pid = pid.trim();
        let started = Instant::now();
        l.start("fast", move |found| tx.send(found).unwrap());
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "cancel waited for the slow run"
        );
        let found = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(found.query, "fast");
        assert_eq!(found.paths, ["/home/a/fast.txt"]);
        // nothing more: the slow run's answer never comes
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
        assert!(
            !std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "the slow plocate still runs"
        );
    }

    #[test]
    fn an_empty_query_or_a_missing_plocate_gives_no_answer() {
        let (tx, rx) = mpsc::channel::<Located>();
        let mut l = Locator::new("/nonexistent/infynitae-edge-test/plocate");
        let tx1 = tx.clone();
        l.start("notes", move |found| tx1.send(found).unwrap());
        l.start("   ", move |found| tx.send(found).unwrap());
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
    }
}
