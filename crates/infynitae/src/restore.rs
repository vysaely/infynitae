//! `infynitae restore`: saving and restoring the canvas session across restarts (it replaces the
//! Python `infynitae-restore`; `live/infynitae-restore` is now a wrapper around this).
//!
//! - `watch`: every 15 s, the mapped windows (app, title, anchor, size, fullscreen) and each
//!   screen's camera centre to `~/.local/state/infynitae/session.json`. Ended by SIGTERM with no
//!   last snapshot: the session is ending and its windows are already closing, so it would
//!   overwrite the last good file with a partial one.
//! - `restore`: each saved app (found by its desktop file) is launched once, and launched again
//!   while it still has windows to place and nothing came for [`RELAUNCH_AFTER`] (a terminal
//!   opens one window per launch; a browser reopens all its own); for 60 s each new window of a
//!   saved app goes to the next saved place of that app, in order; then the cameras go back.
//! - `check`: exit 0 when the saved session has a window whose app can be launched (a desktop
//!   file whose command is installed), 1 otherwise.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use infynitae_ipc::json::{self, Value};

/// How often `watch` saves the session.
const WATCH_INTERVAL: Duration = Duration::from_secs(15);
/// How long `restore` places windows.
const POLL_TIMEOUT: Duration = Duration::from_secs(60);
const POLL_INTERVAL: Duration = Duration::from_millis(200);
/// An app with windows still to place and none arriving for this long is launched again.
const RELAUNCH_AFTER: Duration = Duration::from_secs(8);

fn state_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join(".local/state/infynitae")
}

fn session_path() -> PathBuf {
    state_dir().join("session.json")
}

fn log_path() -> PathBuf {
    state_dir().join("restore.log")
}

/// The time now as `YYYY-MM-DD HH:MM:SS` UTC.
fn timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // civil from days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// One line in the restore log.
fn log(path: &Path, message: &str) {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "[{}] {message}", timestamp());
    }
}

/// A request to the compositor and its reply (the shell socket; a fake in the tests).
pub trait Ipc {
    fn request(&mut self, req: Value) -> Value;
}

/// The running compositor's shell socket, one connection per request.
struct Socket(u64);

impl Ipc for Socket {
    fn request(&mut self, req: Value) -> Value {
        crate::msg::request(&req)
    }
}

impl Socket {
    fn next(&mut self) -> Value {
        self.0 += 1;
        self.0.into()
    }
}

fn req<const N: usize>(id: u64, cmd: &str, rest: [(&str, Value); N]) -> Value {
    let mut members = vec![
        ("id".to_string(), Value::from(id)),
        ("cmd".to_string(), cmd.into()),
    ];
    members.extend(rest.into_iter().map(|(k, v)| (k.to_string(), v)));
    Value::Obj(members)
}

fn ok(reply: &Value) -> bool {
    reply.get("ok") == Some(&Value::Bool(true))
}

/// Writes `data` to `path` whole or not at all (a temporary file, synced, then renamed).
fn save_file(path: &Path, data: &Value) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("json.tmp.{}", std::process::id()));
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data.to_json().as_bytes())?;
        f.write_all(b"\n")?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// The session in a snapshot reply: its mapped windows that have an app, and each screen's
/// camera centre; `None` for a failed reply.
pub fn session_of(reply: &Value) -> Option<Value> {
    if !ok(reply) {
        return None;
    }
    let snapshot = reply.get("snapshot")?;
    let field = |e: &Value, k: &str| e.get(k).cloned().unwrap_or(Value::Null);
    let windows = snapshot
        .get("elements")
        .and_then(Value::as_array)
        .unwrap_or(&[])
        .iter()
        .filter(|e| e.get("state").and_then(Value::as_str) == Some("mapped"))
        .filter(|e| {
            e.get("app_id")
                .and_then(Value::as_str)
                .is_some_and(|a| !a.is_empty())
        })
        .map(|e| {
            Value::Obj(
                ["app_id", "title", "anchor", "size", "fullscreen"]
                    .iter()
                    .map(|k| (k.to_string(), field(e, k)))
                    .collect(),
            )
        })
        .collect();
    let outputs = snapshot
        .get("outputs")
        .and_then(Value::as_array)
        .unwrap_or(&[])
        .iter()
        .map(|o| {
            let center = o.get("camera").map_or(Value::Null, |c| field(c, "center"));
            Value::obj([("id", field(o, "id")), ("center", center)])
        })
        .collect();
    Some(Value::obj([
        ("windows", Value::Arr(windows)),
        ("outputs", Value::Arr(outputs)),
    ]))
}

/// One snapshot saved to `path`; false (and the file untouched) when the compositor did not
/// answer.
pub fn save_snapshot(path: &Path, ipc: &mut impl Ipc, id: u64) -> bool {
    let reply = ipc.request(req(id, "snapshot", []));
    match session_of(&reply) {
        Some(session) => save_file(path, &session).is_ok(),
        None => false,
    }
}

/// A desktop file's `[Desktop Entry]` keys, lowercased, the first of each.
fn desktop_entry(path: &Path) -> HashMap<String, String> {
    let mut entry = HashMap::new();
    let Ok(text) = std::fs::read(path) else {
        return entry;
    };
    let mut inside = false;
    for line in String::from_utf8_lossy(&text).lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            inside = line == "[Desktop Entry]";
            continue;
        }
        if inside && let Some((k, v)) = line.split_once('=') {
            entry
                .entry(k.trim().to_lowercase())
                .or_insert_with(|| v.trim().to_string());
        }
    }
    entry
}

/// A command line split as a POSIX shell does (quotes, backslashes); `None` when a quote is
/// left open.
pub fn split_command(line: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\n' => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next()? {
                        '\'' => break,
                        c => word.push(c),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next()? {
                        '"' => break,
                        '\\' => match chars.next()? {
                            c @ ('"' | '\\' | '$' | '`') => word.push(c),
                            '\n' => {}
                            c => {
                                word.push('\\');
                                word.push(c);
                            }
                        },
                        c => word.push(c),
                    }
                }
            }
            '\\' => {
                in_word = true;
                match chars.next() {
                    Some('\n') | None => {}
                    Some(c) => word.push(c),
                }
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    Some(words)
}

/// A desktop file's `Exec` as a command: field codes (`%u`, `%F`, …) removed; empty when it does
/// not parse.
pub fn clean_exec(exec: &str) -> Vec<String> {
    let Some(words) = split_command(exec) else {
        return Vec::new();
    };
    words
        .into_iter()
        .filter_map(|w| {
            let mut out = String::new();
            let mut chars = w.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '%'
                    && let Some(&n) = chars.peek()
                    && "uUfFick".contains(n)
                {
                    chars.next();
                    continue;
                }
                out.push(c);
            }
            (!out.is_empty()).then_some(out)
        })
        .collect()
}

/// The desktop files under `dir`, each directory's sorted, depth first.
fn desktop_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = read.flatten().map(|e| e.path()).collect();
    entries.sort();
    let (dirs, files): (Vec<_>, Vec<_>) = entries.into_iter().partition(|p| p.is_dir());
    out.extend(
        files
            .into_iter()
            .filter(|p| p.extension().is_some_and(|e| e == "desktop")),
    );
    for d in dirs {
        desktop_files(&d, out);
    }
}

/// The application directories: the user's, then the system's.
fn default_apps_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    vec![
        home.join(".local/share/applications"),
        PathBuf::from("/usr/share/applications"),
    ]
}

/// The command that starts `app_id`: the desktop file named after it, else the one whose
/// `StartupWMClass` is it, else the one whose `Exec` program is named it (all ignoring case).
pub fn app_command(app_id: &str, apps_dirs: &[PathBuf]) -> Option<Vec<String>> {
    let target = app_id.to_lowercase();
    let target = target
        .strip_suffix(".desktop")
        .unwrap_or(&target)
        .to_string();
    if target.is_empty() {
        return None;
    }
    let mut files = Vec::new();
    for d in apps_dirs {
        desktop_files(d, &mut files);
    }
    let by_name = files.iter().find_map(|f| {
        let base = f.file_stem()?.to_string_lossy().to_lowercase();
        (base == target).then(|| desktop_entry(f).get("exec").cloned())?
    });
    let exec = by_name
        .or_else(|| {
            files.iter().find_map(|f| {
                let e = desktop_entry(f);
                (e.get("startupwmclass").map(|c| c.to_lowercase()) == Some(target.clone()))
                    .then(|| e.get("exec").cloned())?
            })
        })
        .or_else(|| {
            files.iter().find_map(|f| {
                let e = desktop_entry(f);
                let ex = e.get("exec")?;
                let first = clean_exec(ex).into_iter().next()?;
                let bin_base = Path::new(&first)
                    .file_name()?
                    .to_string_lossy()
                    .to_lowercase();
                (bin_base == target).then(|| ex.clone())
            })
        });
    // only an installed app's own desktop file starts it: an app picks its app_id, and a window
    // named after any program on PATH (`poweroff`) must never run it at the next login
    let args = clean_exec(&exec?);
    (!args.is_empty()).then_some(args)
}

/// Starts an app on its own, its output not the restore's (it would flood restore.log).
fn launch(app_id: &str, args: &[String], log_file: &Path) {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let result = Command::new(&args[0])
        .args(&args[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn();
    if let Err(e) = result {
        log(
            log_file,
            &format!("Error launching {app_id} ({args:?}): {e}"),
        );
    }
}

/// Timings of [`restore`] (shorter in the tests).
pub struct Timing {
    pub poll_timeout: Duration,
    pub poll_interval: Duration,
    pub relaunch_after: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            poll_timeout: POLL_TIMEOUT,
            poll_interval: POLL_INTERVAL,
            relaunch_after: RELAUNCH_AFTER,
        }
    }
}

/// The saved session at `session` brought back (module docs).
pub fn restore(
    session: &Path,
    apps_dirs: &[PathBuf],
    ipc: &mut impl Ipc,
    launch: &mut impl FnMut(&str, &[String]),
    log_file: &Path,
    timing: Timing,
) {
    let mut id = 0u64;
    let mut next = || {
        id += 1;
        id
    };
    let Ok(text) = std::fs::read_to_string(session) else {
        log(
            log_file,
            &format!("Session file not found: {}", session.display()),
        );
        return;
    };
    let Ok(data) = json::parse(&text) else {
        log(
            log_file,
            &format!("Failed to load session file {}", session.display()),
        );
        return;
    };
    let empty = Vec::new();
    let list = |k: &str| match data.get(k) {
        Some(Value::Arr(a)) => a,
        _ => &empty,
    };
    let (saved, outputs) = (list("windows"), list("outputs"));
    let navigate_back = |ipc: &mut dyn FnMut(Value), next: &mut dyn FnMut() -> u64| {
        for o in outputs {
            if let (Some(out), Some(center)) =
                (o.get("id").and_then(Value::as_u64), o.get("center"))
                && *center != Value::Null
            {
                log(
                    log_file,
                    &format!("Navigating output {out} to {}", center.to_json()),
                );
                ipc(req(
                    next(),
                    "navigate",
                    [
                        ("output", out.into()),
                        ("to", Value::obj([("canvas", center.clone())])),
                    ],
                ));
            }
        }
    };
    if saved.is_empty() {
        log(log_file, "No saved windows found in session");
        navigate_back(&mut |r| drop(ipc.request(r)), &mut next);
        return;
    }

    // the windows open before anything launches: never moved, and a saved place one of them
    // covers now is not taken back (the window stays where the cluster placement put it)
    let mut placed: HashSet<(u64, u64)> = HashSet::new();
    let mut open: Vec<[f64; 4]> = Vec::new();
    let before = ipc.request(req(next(), "snapshot", []));
    if ok(&before) {
        for e in snapshot_elements(&before) {
            if let Some(key) = element_key(e) {
                placed.insert(key);
            }
            if let Some(r) = rect_of(e) {
                open.push(r);
            }
        }
    }

    struct App {
        cmd: Vec<String>,
        saved: usize,
        launches: usize,
        last: Instant,
    }
    let app_of = |w: &Value| w.get("app_id").and_then(Value::as_str).map(str::to_owned);
    // 1. each saved app once
    let mut apps: HashMap<String, App> = HashMap::new();
    for w in saved {
        let Some(app_id) = app_of(w).filter(|a| !a.is_empty()) else {
            continue;
        };
        let key = app_id.to_lowercase();
        if let Some(app) = apps.get_mut(&key) {
            app.saved += 1;
            continue;
        }
        let Some(cmd) = app_command(&app_id, apps_dirs) else {
            log(log_file, &format!("Skip {app_id}: no desktop file found"));
            continue;
        };
        log(log_file, &format!("Launch {app_id}: {cmd:?}"));
        launch(&app_id, &cmd);
        apps.insert(
            key,
            App {
                cmd,
                saved: 1,
                launches: 1,
                last: Instant::now(),
            },
        );
    }

    // 2. only windows of launched apps can come back
    let mut pending: Vec<&Value> = saved
        .iter()
        .filter(|w| {
            w.get("anchor").is_some_and(|a| *a != Value::Null)
                && app_of(w).is_some_and(|a| apps.contains_key(&a.to_lowercase()))
        })
        .collect();
    let start = Instant::now();
    while !pending.is_empty() && start.elapsed() < timing.poll_timeout {
        let reply = ipc.request(req(next(), "snapshot", []));
        if ok(&reply) {
            for e in snapshot_elements(&reply) {
                let Some((inst, elem)) = element_key(e) else {
                    continue;
                };
                if placed.contains(&(inst, elem))
                    || e.get("state").and_then(Value::as_str) != Some("mapped")
                {
                    continue;
                }
                placed.insert((inst, elem));
                let Some(app_id) = app_of(e).filter(|a| !a.is_empty()) else {
                    continue;
                };
                let key = app_id.to_lowercase();
                // the next saved window of this app, in saved order
                let Some(i) = pending
                    .iter()
                    .position(|w| app_of(w).is_some_and(|a| a.to_lowercase() == key))
                else {
                    continue;
                };
                let target = pending.remove(i);
                if let Some(app) = apps.get_mut(&key) {
                    app.last = Instant::now();
                }
                let covered = rect_of(target).is_some_and(|t| open.iter().any(|o| overlap(&t, o)));
                if covered {
                    log(
                        log_file,
                        &format!(
                            "Keeping {app_id} (element [{inst}, {elem}]) where it opened: a window open before covers its saved place"
                        ),
                    );
                } else if let Some(anchor) = target.get("anchor") {
                    log(
                        log_file,
                        &format!(
                            "Moving {app_id} (element [{inst}, {elem}]) to {}",
                            anchor.to_json()
                        ),
                    );
                    ipc.request(req(
                        next(),
                        "move_element",
                        [
                            ("element", Value::Arr(vec![inst.into(), elem.into()])),
                            ("to", anchor.clone()),
                        ],
                    ));
                }
            }
        }
        // an app with windows still to place and no news for a while: launch it again
        let mut keys: Vec<&String> = apps.keys().collect();
        keys.sort();
        let keys: Vec<String> = keys.into_iter().cloned().collect();
        for key in keys {
            let waiting = pending
                .iter()
                .filter(|w| app_of(w).is_some_and(|a| a.to_lowercase() == key))
                .count();
            let app = apps.get_mut(&key).expect("a key of apps");
            if waiting > 0
                && app.launches < app.saved
                && app.last.elapsed() >= timing.relaunch_after
            {
                app.launches += 1;
                app.last = Instant::now();
                log(
                    log_file,
                    &format!("Launch {key} again ({waiting} window(s) to place)"),
                );
                launch(&key, &app.cmd);
            }
        }
        std::thread::sleep(timing.poll_interval);
    }

    // 3. the cameras back where they were
    navigate_back(&mut |r| drop(ipc.request(r)), &mut next);
}

fn snapshot_elements(reply: &Value) -> &[Value] {
    reply
        .get("snapshot")
        .and_then(|s| s.get("elements"))
        .and_then(Value::as_array)
        .unwrap_or(&[])
}

/// An element's `[instance, element]` id.
fn element_key(e: &Value) -> Option<(u64, u64)> {
    let raw = e.get("id").and_then(Value::as_array)?;
    Some((raw.first()?.as_u64()?, raw.get(1)?.as_u64()?))
}

/// A window's `[x, y, w, h]` from its anchor and size, saved or in a snapshot.
fn rect_of(w: &Value) -> Option<[f64; 4]> {
    let a = w.get("anchor")?.as_array()?;
    let s = w.get("size")?.as_array()?;
    Some([
        a.first()?.as_f64()?,
        a.get(1)?.as_f64()?,
        s.first()?.as_f64()?,
        s.get(1)?.as_f64()?,
    ])
}

fn overlap(a: &[f64; 4], b: &[f64; 4]) -> bool {
    a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
}

/// The saved session has a window whose app can be launched: a desktop file whose command is
/// installed (on `PATH`, or a path that exists).
pub fn restorable(session: &Path, apps_dirs: &[PathBuf]) -> bool {
    let Some(data) = std::fs::read_to_string(session)
        .ok()
        .and_then(|t| json::parse(&t).ok())
    else {
        return false;
    };
    let Some(windows) = data.get("windows").and_then(Value::as_array) else {
        return false;
    };
    let installed = |cmd: &str| {
        if cmd.contains('/') {
            return Path::new(cmd).is_file();
        }
        std::env::var_os("PATH")
            .is_some_and(|path| std::env::split_paths(&path).any(|d| d.join(cmd).is_file()))
    };
    let apps: HashSet<&str> = windows
        .iter()
        .filter_map(|w| w.get("app_id").and_then(Value::as_str))
        .filter(|a| !a.is_empty())
        .collect();
    apps.into_iter()
        .any(|a| app_command(a, apps_dirs).is_some_and(|cmd| installed(&cmd[0])))
}

fn session_restore_enabled() -> bool {
    if let Ok(val) = std::env::var("SESSION_RESTORE") {
        return !matches!(val.to_lowercase().as_str(), "0" | "false" | "off" | "no");
    }
    if let Ok(home) = std::env::var("HOME")
        && let Ok(content) = std::fs::read_to_string(format!("{home}/.config/infynitae/config"))
    {
        for (k, v) in content.lines().filter_map(infynitae_wayland::config_line) {
            if k.trim() == "SESSION_RESTORE" {
                return !matches!(
                    v.trim().to_lowercase().as_str(),
                    "0" | "false" | "off" | "no"
                );
            }
        }
    }
    true
}

/// `infynitae restore watch|save|restore|check` (`args` after "restore").
pub fn run(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("watch") => {
            if !session_restore_enabled() {
                let _ = std::fs::remove_file(session_path());
                return ExitCode::SUCCESS;
            }
            let mut ipc = Socket(0);
            let first_id = ipc.next().as_u64().unwrap_or(1);
            save_snapshot(&session_path(), &mut ipc, first_id);
            // a minute without an answer: the compositor is gone (the session ended, or this was
            // started outside one), and the next session starts its own watcher
            let mut unanswered = 0;
            loop {
                std::thread::sleep(WATCH_INTERVAL);
                if !session_restore_enabled() {
                    let _ = std::fs::remove_file(session_path());
                    return ExitCode::SUCCESS;
                }
                let id = ipc.next().as_u64().unwrap_or(1);
                if save_snapshot(&session_path(), &mut ipc, id) {
                    unanswered = 0;
                } else {
                    unanswered += 1;
                    if unanswered >= 4 {
                        return ExitCode::FAILURE;
                    }
                }
            }
        }
        Some("save") => {
            let path = args.get(1).map_or_else(session_path, PathBuf::from);
            if args.get(1).is_none() && !session_restore_enabled() {
                let _ = std::fs::remove_file(&path);
                return ExitCode::SUCCESS;
            }
            let mut ipc = Socket(0);
            let id = ipc.next().as_u64().unwrap_or(1);
            if save_snapshot(&path, &mut ipc, id) {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Some("restore") => {
            restore(
                &session_path(),
                &default_apps_dirs(),
                &mut Socket(0),
                &mut |app, cmd| launch(app, cmd, &log_path()),
                &log_path(),
                Timing::default(),
            );
            ExitCode::SUCCESS
        }
        Some("check") => {
            let path = args.get(1).map_or_else(session_path, PathBuf::from);
            if restorable(&path, &default_apps_dirs()) {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        _ => {
            eprintln!("Usage: infynitae restore watch | save | restore | check [session.json]");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory for one test.
    fn tmp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("infynitae-restore-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn desktop(dir: &Path, name: &str, body: &str) {
        std::fs::write(
            dir.join(format!("{name}.desktop")),
            format!("[Desktop Entry]\nType=Application\n{body}"),
        )
        .unwrap();
    }

    struct Fake<F: FnMut(&Value) -> Value> {
        calls: Vec<Value>,
        reply: F,
    }

    impl<F: FnMut(&Value) -> Value> Ipc for Fake<F> {
        fn request(&mut self, req: Value) -> Value {
            let r = (self.reply)(&req);
            self.calls.push(req);
            r
        }
    }

    fn quick() -> Timing {
        Timing {
            poll_timeout: Duration::from_millis(400),
            poll_interval: Duration::from_millis(10),
            relaunch_after: Duration::from_millis(50),
        }
    }

    #[test]
    fn desktop_files_are_found_by_name_or_window_class_and_field_codes_go() {
        let d = tmp("lookup");
        desktop(&d, "my-term", "Name=My Terminal\nExec=my-term -e %f %F\n");
        assert_eq!(
            app_command("MY-TERM", std::slice::from_ref(&d)),
            Some(vec!["my-term".into(), "-e".into()])
        );
        desktop(
            &d,
            "special-browser",
            "StartupWMClass=Special-Browser-WM\nExec=browser-bin %u %U %k %c %i --profile default\n",
        );
        assert_eq!(
            app_command("special-browser-wm", std::slice::from_ref(&d)),
            Some(vec![
                "browser-bin".into(),
                "--profile".into(),
                "default".into()
            ])
        );
        assert_eq!(
            app_command("nonexistent-app", std::slice::from_ref(&d)),
            None
        );
        desktop(&d, "bad-quote", "Exec=bad-app \"unclosed quote %u\n");
        assert_eq!(app_command("bad-quote", std::slice::from_ref(&d)), None);
        assert!(clean_exec("bad-app \"unclosed quote").is_empty());
        assert_eq!(
            clean_exec(r#"sh -c "echo 'a b' \"q\"" x\ y"#),
            vec!["sh", "-c", "echo 'a b' \"q\"", "x y"]
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_snapshot_saves_mapped_windows_with_an_app_and_a_failed_one_keeps_the_file() {
        let d = tmp("save");
        let file = d.join("session.json");
        let snapshot = json::parse(
            r#"{"id":1,"ok":true,"snapshot":{"outputs":[{"id":1,"camera":{"center":[500.0,600.0]}}],
            "elements":[
              {"id":[1,1],"state":"mapped","app_id":"demo-app","title":"Window 1","anchor":[10.0,20.0],"size":[100.0,100.0],"fullscreen":null},
              {"id":[1,2],"state":"unmapped","app_id":"demo-app","anchor":[0.0,0.0]},
              {"id":[1,3],"state":"mapped","app_id":null,"anchor":[0.0,0.0]},
              {"id":[1,4],"state":"mapped","app_id":"demo-app","title":"Window 2","anchor":[30.0,40.0],"size":[100.0,100.0],"fullscreen":null}]}}"#,
        )
        .unwrap();
        let mut ipc = Fake {
            calls: vec![],
            reply: |_: &Value| snapshot.clone(),
        };
        assert!(save_snapshot(&file, &mut ipc, 1));
        let saved = json::parse(&std::fs::read_to_string(&file).unwrap()).unwrap();
        let windows = saved.get("windows").and_then(Value::as_array).unwrap();
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].get("anchor").unwrap().to_json(), "[10,20]");
        assert_eq!(windows[1].get("anchor").unwrap().to_json(), "[30,40]");
        assert_eq!(
            saved.get("outputs").and_then(Value::as_array).unwrap()[0]
                .get("center")
                .unwrap()
                .to_json(),
            "[500,600]"
        );

        // demo-app has no desktop file: nothing to launch; with one whose command exists, there is
        let apps = d.join("apps");
        std::fs::create_dir_all(&apps).unwrap();
        assert!(!restorable(&file, std::slice::from_ref(&apps)));
        desktop(&apps, "demo-app", "Name=Demo\nExec=sh -c true\n");
        assert!(restorable(&file, std::slice::from_ref(&apps)));

        let mut failing = Fake {
            calls: vec![],
            reply: |_: &Value| json::parse(r#"{"ok":false}"#).unwrap(),
        };
        assert!(!save_snapshot(&file, &mut failing, 2));
        let kept = json::parse(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(
            kept.get("windows").and_then(Value::as_array).unwrap().len(),
            2,
            "untouched"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn windows_are_placed_in_saved_order_and_unknown_apps_skipped() {
        let d = tmp("restore");
        let apps = d.join("apps");
        std::fs::create_dir_all(&apps).unwrap();
        desktop(&apps, "demo-app", "Name=Demo App\nExec=demo-app-bin\n");
        let file = d.join("session.json");
        std::fs::write(
            &file,
            r#"{"windows":[
              {"app_id":"demo-app","title":"Win 1","anchor":[100.0,200.0]},
              {"app_id":"demo-app","title":"Win 2","anchor":[300.0,400.0]},
              {"app_id":"app-without-desktop","title":"Win 3","anchor":[50.0,50.0]}],
              "outputs":[{"id":1,"center":[1200.0,800.0]}]}"#,
        )
        .unwrap();
        let log_file = d.join("restore.log");
        let snapshot = json::parse(
            r#"{"ok":true,"snapshot":{"elements":[
              {"id":[1,101],"state":"mapped","app_id":"demo-app"},
              {"id":[1,102],"state":"mapped","app_id":"unknown-app"},
              {"id":[1,103],"state":"mapped","app_id":"demo-app"}],"outputs":[]}}"#,
        )
        .unwrap();
        // nothing is open before the launch; then the app's windows are
        let snapshots = std::cell::Cell::new(0);
        let mut ipc = Fake {
            calls: vec![],
            reply: |r: &Value| {
                if r.get("cmd").and_then(Value::as_str) == Some("snapshot") {
                    snapshots.set(snapshots.get() + 1);
                    if snapshots.get() == 1 {
                        json::parse(r#"{"ok":true,"snapshot":{"elements":[],"outputs":[]}}"#)
                            .unwrap()
                    } else {
                        snapshot.clone()
                    }
                } else {
                    json::parse(r#"{"ok":true}"#).unwrap()
                }
            },
        };
        let mut launched = Vec::new();
        restore(
            &file,
            std::slice::from_ref(&apps),
            &mut ipc,
            &mut |a, c| launched.push((a.to_string(), c.to_vec())),
            &log_file,
            quick(),
        );

        assert_eq!(
            launched,
            vec![("demo-app".to_string(), vec!["demo-app-bin".to_string()])]
        );
        assert!(
            std::fs::read_to_string(&log_file)
                .unwrap()
                .contains("Skip app-without-desktop: no desktop file found")
        );
        let moves: Vec<_> = ipc
            .calls
            .iter()
            .filter(|c| c.get("cmd").and_then(Value::as_str) == Some("move_element"))
            .collect();
        assert_eq!(moves.len(), 2);
        assert_eq!(moves[0].get("element").unwrap().to_json(), "[1,101]");
        assert_eq!(moves[0].get("to").unwrap().to_json(), "[100,200]");
        assert_eq!(moves[1].get("element").unwrap().to_json(), "[1,103]");
        assert_eq!(moves[1].get("to").unwrap().to_json(), "[300,400]");
        let navs: Vec<_> = ipc
            .calls
            .iter()
            .filter(|c| c.get("cmd").and_then(Value::as_str) == Some("navigate"))
            .collect();
        assert_eq!(navs.len(), 1);
        assert_eq!(navs[0].get("output").and_then(Value::as_u64), Some(1));
        assert_eq!(
            navs[0].get("to").unwrap().to_json(),
            r#"{"canvas":[1200,800]}"#
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A window open before the restore (a browser a login opened) is never moved, and a saved
    /// place it covers now is not taken back: the restored window stays where it was placed.
    #[test]
    fn windows_open_before_the_restore_are_left_alone_and_not_covered() {
        let d = tmp("open-before");
        let apps = d.join("apps");
        std::fs::create_dir_all(&apps).unwrap();
        desktop(&apps, "term", "Name=Term\nExec=term-bin\n");
        desktop(&apps, "browser", "Name=Browser\nExec=browser-bin\n");
        let file = d.join("session.json");
        std::fs::write(
            &file,
            r#"{"windows":[
              {"app_id":"term","anchor":[-600,-1094],"size":[1237,721]},
              {"app_id":"term","anchor":[2000,0],"size":[800,600]}],"outputs":[]}"#,
        )
        .unwrap();
        let browser = r#"{"id":[1,7],"state":"mapped","app_id":"browser","anchor":[-622,-1105],"size":[1281,743]}"#;
        let old_term = r#"{"id":[1,8],"state":"mapped","app_id":"term","anchor":[5000,5000],"size":[800,600]}"#;
        let windows = std::cell::RefCell::new(vec![
            json::parse(browser).unwrap(),
            json::parse(old_term).unwrap(),
        ]);
        let next = std::cell::Cell::new(100u64);
        let mut ipc = Fake {
            calls: vec![],
            reply: |r: &Value| {
                if r.get("cmd").and_then(Value::as_str) == Some("snapshot") {
                    Value::obj([
                        ("ok", true.into()),
                        (
                            "snapshot",
                            Value::obj([("elements", Value::Arr(windows.borrow().clone()))]),
                        ),
                    ])
                } else {
                    Value::obj([("ok", true.into())])
                }
            },
        };
        restore(
            &file,
            std::slice::from_ref(&apps),
            &mut ipc,
            &mut |app, _| {
                next.set(next.get() + 1);
                windows.borrow_mut().push(Value::obj([
                    ("id", Value::Arr(vec![1u64.into(), next.get().into()])),
                    ("state", "mapped".into()),
                    ("app_id", app.into()),
                ]));
            },
            &d.join("log"),
            quick(),
        );
        let moves: Vec<String> = ipc
            .calls
            .iter()
            .filter(|c| c.get("cmd").and_then(Value::as_str) == Some("move_element"))
            .map(|c| {
                format!(
                    "{} {}",
                    c.get("element").unwrap().to_json(),
                    c.get("to").unwrap().to_json()
                )
            })
            .collect();
        // the first terminal's saved place is under the browser: kept where it opened; the
        // second goes back; neither the browser nor the terminal open before moves
        assert_eq!(moves, vec!["[1,102] [2000,0]".to_string()]);
        assert!(
            std::fs::read_to_string(d.join("log"))
                .unwrap()
                .contains("Keeping term (element [1, 101]) where it opened")
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_terminal_is_launched_per_window_and_a_browser_once() {
        let d = tmp("relaunch");
        let apps = d.join("apps");
        std::fs::create_dir_all(&apps).unwrap();
        for name in ["term", "browser"] {
            desktop(&apps, name, &format!("Name={name}\nExec={name}-bin\n"));
        }
        let file = d.join("session.json");
        std::fs::write(
            &file,
            r#"{"windows":[{"app_id":"term","anchor":[0,0]},{"app_id":"term","anchor":[10,0]},
              {"app_id":"browser","anchor":[0,50]},{"app_id":"browser","anchor":[10,50]}],"outputs":[]}"#,
        )
        .unwrap();
        let windows = std::cell::RefCell::new(Vec::<Value>::new());
        let next = std::cell::Cell::new(200u64);
        let mut launches = Vec::new();
        let mut ipc = Fake {
            calls: vec![],
            reply: |r: &Value| {
                if r.get("cmd").and_then(Value::as_str) == Some("snapshot") {
                    Value::obj([
                        ("ok", true.into()),
                        (
                            "snapshot",
                            Value::obj([
                                ("elements", Value::Arr(windows.borrow().clone())),
                                ("outputs", Value::Arr(vec![])),
                            ]),
                        ),
                    ])
                } else {
                    Value::obj([("ok", true.into())])
                }
            },
        };
        restore(
            &file,
            std::slice::from_ref(&apps),
            &mut ipc,
            &mut |app, _| {
                launches.push(app.to_string());
                for _ in 0..if app == "term" { 1 } else { 2 } {
                    next.set(next.get() + 1);
                    windows.borrow_mut().push(Value::obj([
                        ("id", Value::Arr(vec![1u64.into(), next.get().into()])),
                        ("state", "mapped".into()),
                        ("app_id", app.into()),
                    ]));
                }
            },
            &d.join("log"),
            quick(),
        );
        launches.sort();
        assert_eq!(launches, vec!["browser", "term", "term"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_log_time_is_a_date_and_a_time() {
        let t = timestamp();
        assert_eq!(t.len(), 19, "{t}");
        assert_eq!(&t[4..5], "-");
        assert_eq!(&t[13..14], ":");
    }
}
