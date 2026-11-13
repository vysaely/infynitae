//! `infynitae msg`: the session's scripts talk to the running compositor through its shell
//! socket (`$XDG_RUNTIME_DIR/infynitae/shell.sock`, one JSON object per line, C7.3) and to the
//! shortcuts portal's socket, from the binary itself instead of a Python helper.
//!
//! ```text
//! infynitae msg snapshot                        the compositor's state, as JSON
//! infynitae msg focus <instance> <element>      focus a window
//! infynitae msg navigate <x> <y> [zoom]         the first screen's camera to a canvas point
//! infynitae msg navigate element <inst> <elem>  … to a window
//! infynitae msg place <name>                    save the active viewport to config
//! infynitae msg restore [output]                the camera back to where it was
//! infynitae msg shortcut <name>                 a global shortcut (mute, deafen, …) to apps
//! infynitae msg overview                        pick a window from a menu and go to it
//! infynitae msg layers [app]                    every window's surfaces and where they are drawn
//! ```

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use infynitae_ipc::json::{self, Value};

/// How long the compositor has to answer a request.
const TIMEOUT: Duration = Duration::from_secs(2);
/// `capture-chord` waits up to 10 s in the compositor; leave headroom for socket framing.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(12);

/// The runtime directory: `XDG_RUNTIME_DIR`, else the user's own.
fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(format!("/run/user/{}", rustix::process::getuid().as_raw()))
        })
}

/// The compositor's shell socket.
pub fn shell_socket() -> PathBuf {
    runtime_dir().join("infynitae/shell.sock")
}

/// One request on a new connection to `socket`, and its reply; a failure is a reply too
/// (`{"ok": false, "error": …}`), as the Python helper gave it.
pub fn request_at(socket: &std::path::Path, req: &Value) -> Value {
    request_at_timeout(socket, req, TIMEOUT)
}

/// Sends one request with an explicit read/write timeout.
pub fn request_at_timeout(socket: &std::path::Path, req: &Value, timeout: Duration) -> Value {
    let fail = |e: &str| Value::obj([("ok", false.into()), ("error", e.into())]);
    let Ok(mut stream) = UnixStream::connect(socket) else {
        return fail("socket_not_found");
    };
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    if stream
        .write_all(format!("{}\n", req.to_json()).as_bytes())
        .is_err()
    {
        return fail("write_failed");
    }
    let mut line = String::new();
    if BufReader::new(stream).read_line(&mut line).is_err() || line.trim().is_empty() {
        return fail("empty_response");
    }
    json::parse(line.trim()).unwrap_or_else(|_| fail("bad_response"))
}

/// [`request_at`] the compositor's shell socket.
pub fn request(req: &Value) -> Value {
    request_at(&shell_socket(), req)
}

/// A `[instance, element]` id.
fn element(inst: u64, elem: u64) -> Value {
    Value::Arr(vec![inst.into(), elem.into()])
}

/// The first screen's id (1 when the compositor does not say).
fn first_output() -> u64 {
    let snap = request(&Value::obj([
        ("id", 1u64.into()),
        ("cmd", "snapshot".into()),
    ]));
    snap.get("snapshot")
        .and_then(|s| s.get("outputs"))
        .and_then(Value::as_array)
        .and_then(|o| o.first())
        .and_then(|o| o.get("id"))
        .and_then(Value::as_u64)
        .unwrap_or(1)
}

fn usage() -> ExitCode {
    eprintln!(
        "Usage: infynitae msg snapshot | focus <inst> <elem> | navigate <x> <y> [zoom] | navigate element <inst> <elem> | place <name> | restore [output] | shortcut <name> | overview | lock | night-light [on|off] | keyboard-layout | osk [on|off] | capture-chord | layers [app]"
    );
    ExitCode::from(2)
}

fn parse_navigate_args(args: &[String]) -> Option<(Value, Option<f64>)> {
    let num = |i: usize| args.get(i).and_then(|a| a.parse::<u64>().ok());
    if args.get(1).map(String::as_str) == Some("element") {
        return match (num(2), num(3), args.get(4)) {
            (Some(i), Some(e), None) => Some((Value::obj([("element", element(i, e))]), None)),
            _ => None,
        };
    }
    if args.len() > 4 {
        return None;
    }
    let finite = |i: usize| {
        args.get(i)
            .and_then(|a| a.parse::<f64>().ok())
            .filter(|v| v.is_finite())
    };
    let zoom = match args.get(3) {
        Some(_) => Some(finite(3).filter(|z| *z > 0.0)?),
        None => None,
    };
    let (x, y) = (finite(1)?, finite(2)?);
    Some((
        Value::obj([("canvas", Value::Arr(vec![x.into(), y.into()]))]),
        zoom,
    ))
}

fn valid_place_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn snapshot_camera(snap: &Value) -> Option<(f64, f64, f64)> {
    let camera = snap
        .get("snapshot")?
        .get("outputs")?
        .as_array()?
        .first()?
        .get("camera")?;
    let center = camera.get("center")?.as_array()?;
    let x = center.first()?.as_f64()?;
    let y = center.get(1)?.as_f64()?;
    let zoom = camera.get("zoom")?.as_f64()?;
    (x.is_finite() && y.is_finite() && zoom.is_finite() && zoom > 0.0).then_some((x, y, zoom))
}

fn format_num(v: f64) -> String {
    let rounded = (v * 10_000.0).round() / 10_000.0;
    let clean = if rounded == 0.0 { 0.0 } else { rounded };
    let s = format!("{clean:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn extract_place_chord(raw: &str) -> Option<String> {
    let t = raw.trim();
    let inner = if let Some(rest) = t.strip_prefix('"') {
        rest.split_once('"').map_or(rest, |(s, _)| s)
    } else if let Some(rest) = t.strip_prefix('\'') {
        rest.split_once('\'').map_or(rest, |(s, _)| s)
    } else {
        t.split('#').next().unwrap_or("").trim()
    };
    let mut parts = inner.split_whitespace();
    let _coords = parts.next()?;
    let chord: Vec<&str> = parts.collect();
    (!chord.is_empty()).then(|| chord.join(" "))
}

fn upsert_place_in_config(text: &str, name: &str, x: f64, y: f64, zoom: f64) -> (String, String) {
    let key = format!("PLACE_{}", name.to_ascii_uppercase());
    let prefix = format!("{key}=");
    let existing_chord = text.lines().find_map(|line| {
        let rest = line.trim_start().strip_prefix(&prefix)?;
        extract_place_chord(rest)
    });
    let coords = format!("{},{},{}", format_num(x), format_num(y), format_num(zoom));
    let rhs = match existing_chord {
        Some(chord) => format!("\"{coords} {chord}\""),
        None => format!("\"{coords}\""),
    };
    let formatted_line = format!("{key}={rhs}");

    let mut out = Vec::new();
    let mut replaced = false;
    for line in text.lines() {
        if line.trim_start().starts_with(&prefix) {
            if !replaced {
                out.push(formatted_line.clone());
                replaced = true;
            }
        } else {
            out.push(line.to_string());
        }
    }
    if !replaced {
        out.push(formatted_line.clone());
    }
    let mut joined = out.join("\n");
    if text.ends_with('\n') || !replaced {
        joined.push('\n');
    }
    (joined, formatted_line)
}

fn place_config_path() -> PathBuf {
    if let Some(p) = std::env::var_os("CONFIG_FILE").filter(|v| !v.is_empty()) {
        return PathBuf::from(p);
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config/infynitae/config")
}

fn save_place(name: &str) -> ExitCode {
    if !valid_place_name(name) {
        eprintln!("infynitae msg place: name must use letters, digits, or underscore");
        return ExitCode::from(2);
    }
    let snap = request(&Value::obj([
        ("id", 1u64.into()),
        ("cmd", "snapshot".into()),
    ]));
    let Some((x, y, zoom)) = snapshot_camera(&snap) else {
        eprintln!("infynitae msg place: no active camera in snapshot");
        return ExitCode::FAILURE;
    };
    let path = place_config_path();
    if let Some(dir) = path.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        eprintln!("infynitae msg place: {e}");
        return ExitCode::FAILURE;
    }
    let before = std::fs::read_to_string(&path).unwrap_or_default();
    let (after, line) = upsert_place_in_config(&before, name, x, y, zoom);
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("config");
    let tmp = path.with_file_name(format!(".{file_name}.tmp.{}", std::process::id()));
    if let Err(e) = std::fs::write(&tmp, after.as_bytes()) {
        eprintln!("infynitae msg place: {e}");
        return ExitCode::FAILURE;
    }
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        eprintln!("infynitae msg place: {e}");
        return ExitCode::FAILURE;
    }
    println!("{line}");
    ExitCode::SUCCESS
}

fn edge_running_in(runtime: &std::path::Path, proc_root: &std::path::Path) -> bool {
    for file in ["infynitae-edge-rs.pid", "infynitae-edge.pid"] {
        let Ok(text) = std::fs::read_to_string(runtime.join(file)) else {
            continue;
        };
        let Ok(pid) = text.trim().parse::<u32>() else {
            continue;
        };
        if pid <= 1 {
            continue;
        }
        let Ok(comm) = std::fs::read_to_string(proc_root.join(pid.to_string()).join("comm")) else {
            continue;
        };
        let comm = comm.trim();
        if comm == "infynitae-edge" || comm.starts_with("infynitae-edge-") {
            return true;
        }
    }
    false
}

fn capture_chord_from_reply(reply: &Value) -> Result<&str, &str> {
    if reply.get("ok") == Some(&Value::Bool(true))
        && let Some(chord) = reply.get("chord").and_then(Value::as_str)
    {
        return Ok(chord);
    }
    Err(reply
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("capture_failed"))
}

fn capture_chord() -> ExitCode {
    let reply = request_at_timeout(
        &shell_socket(),
        &Value::obj([("id", 1u64.into()), ("cmd", "capture-chord".into())]),
        CAPTURE_TIMEOUT,
    );
    match capture_chord_from_reply(&reply) {
        Ok(chord) => {
            println!("{chord}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

/// Prints a reply; success when it says `"ok": true`.
fn print_reply(reply: &Value) -> ExitCode {
    println!("{}", reply.to_json());
    if reply.get("ok") == Some(&Value::Bool(true)) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// `infynitae msg …` (`args` after "msg").
/// `infynitae msg layers [app]`: per window (of `app`, any case), its geometry, then each surface
/// with its role, offset and size and, under it, the physical rects it is drawn at.
fn layers(app: Option<&str>) -> ExitCode {
    let reply = request(&Value::obj([("id", 1u64.into()), ("cmd", "layers".into())]));
    let Some(windows) = reply
        .get("snapshot")
        .and_then(|s| s.get("windows"))
        .and_then(Value::as_array)
    else {
        eprintln!("infynitae msg: no answer (an older compositor without `layers`?)");
        return ExitCode::FAILURE;
    };
    let text = |v: Option<&Value>| v.map(Value::to_json).unwrap_or_default();
    for w in windows {
        let name = w.get("app_id").and_then(Value::as_str).unwrap_or("?");
        if app.is_some_and(|a| !a.eq_ignore_ascii_case(name)) {
            continue;
        }
        println!(
            "{name} {} anchor {} geometry {}",
            text(w.get("id")),
            text(w.get("anchor")),
            text(w.get("geometry"))
        );
        let drawn = w.get("drawn").and_then(Value::as_array).unwrap_or(&[]);
        for s in w.get("surfaces").and_then(Value::as_array).unwrap_or(&[]) {
            println!(
                "  surface {} {} parent {} offset {} size {} mapped {}",
                text(s.get("key")),
                s.get("role").and_then(Value::as_str).unwrap_or("?"),
                text(s.get("parent")),
                text(s.get("offset")),
                text(s.get("size")),
                text(s.get("mapped"))
            );
            for d in drawn.iter().filter(|d| d.get("key") == s.get("key")) {
                println!(
                    "    drawn on output {} at {} (canvas {} size {})",
                    text(d.get("output")),
                    text(d.get("rect")),
                    text(d.get("origin")),
                    text(d.get("size"))
                );
            }
        }
    }
    ExitCode::SUCCESS
}

pub fn run(args: &[String]) -> ExitCode {
    let num = |i: usize| args.get(i).and_then(|a| a.parse::<u64>().ok());
    let id = ("id", Value::from(1u64));
    match args.first().map(String::as_str) {
        Some("snapshot") => print_reply(&request(&Value::obj([id, ("cmd", "snapshot".into())]))),
        Some("layers") => layers(args.get(1).map(String::as_str)),
        Some("focus") => match (num(1), num(2)) {
            (Some(i), Some(e)) => print_reply(&request(&Value::obj([
                id,
                ("cmd", "focus".into()),
                ("element", element(i, e)),
            ]))),
            _ => usage(),
        },
        Some("navigate") => {
            let Some((to, zoom)) = parse_navigate_args(args) else {
                return usage();
            };
            let output = first_output();
            let req = match zoom {
                Some(z) => Value::obj([
                    id,
                    ("cmd", "navigate".into()),
                    ("output", output.into()),
                    ("to", to),
                    ("zoom", z.into()),
                ]),
                None => Value::obj([
                    id,
                    ("cmd", "navigate".into()),
                    ("output", output.into()),
                    ("to", to),
                ]),
            };
            print_reply(&request(&req))
        }
        Some("place") => match (args.get(1), args.get(2)) {
            (Some(name), None) => save_place(name),
            _ => usage(),
        },
        Some("restore" | "viewport_restore") => {
            let output = num(1).unwrap_or_else(first_output);
            print_reply(&request(&Value::obj([
                id,
                ("cmd", "viewport_restore".into()),
                ("output", output.into()),
            ])))
        }
        Some("shortcut") => match args.get(1) {
            Some(name) => shortcut(name),
            None => usage(),
        },
        Some("overview") => overview(),
        // the session locked in the compositor now, then the lock screen (sleep-lock)
        Some("lock") => print_reply(&request(&Value::obj([id, ("cmd", "lock".into())]))),
        // night light: on, off, or the other way round (no word)
        Some("night-light" | "night_light") => {
            let on = match args.get(1).map(String::as_str) {
                None | Some("toggle") => Value::Null,
                Some("on") => Value::Bool(true),
                Some("off") => Value::Bool(false),
                _ => return usage(),
            };
            print_reply(&request(&Value::obj([
                id,
                ("cmd", "night_light".into()),
                ("on", on),
            ])))
        }
        // the on-screen keyboard: shown, hidden, or the other way round (no word)
        Some("osk") => {
            let on = match args.get(1).map(String::as_str) {
                None | Some("toggle") => Value::Null,
                Some("on" | "show") => Value::Bool(true),
                Some("off" | "hide") => Value::Bool(false),
                _ => return usage(),
            };
            print_reply(&request(&Value::obj([
                id,
                ("cmd", "osk".into()),
                ("on", on),
            ])))
        }
        // the next keyboard layout
        Some("keyboard-layout" | "keyboard_layout") => print_reply(&request(&Value::obj([
            id,
            ("cmd", "keyboard_layout".into()),
        ]))),
        Some("capture-chord" | "capture_chord") if args.len() == 1 => capture_chord(),
        _ => usage(),
    }
}

/// Sends a global shortcut's name to the shortcuts portal (`shortcuts.sock`), which passes it to
/// the apps that registered it (Discord's mute). Nothing listens: nothing to do.
fn shortcut(name: &str) -> ExitCode {
    if let Ok(mut s) = UnixStream::connect(runtime_dir().join("infynitae/shortcuts.sock")) {
        let _ = s.write_all(name.as_bytes());
    }
    ExitCode::SUCCESS
}

/// The windows as menu lines, `[n] app — title`, and each line's window.
pub fn overview_lines(snapshot: &Value) -> Vec<(String, (u64, u64))> {
    let elements = snapshot
        .get("snapshot")
        .and_then(|s| s.get("elements"))
        .and_then(Value::as_array)
        .unwrap_or(&[]);
    elements
        .iter()
        .filter_map(|e| {
            let id = e.get("id")?.as_array()?;
            Some((id.first()?.as_u64()?, id.get(1)?.as_u64()?, e))
        })
        .enumerate()
        .map(|(n, (inst, elem, e))| {
            let app = e
                .get("app_id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or("window");
            let title = e
                .get("title")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or("Untitled");
            (format!("[{}] {app} — {title}", n + 1), (inst, elem))
        })
        .collect()
}

/// The window switcher (Super+Shift+Tab without Quickshell): the windows in a menu (`MENU_PICKER`,
/// else fuzzel, rofi or wofi); the chosen one is focused and the camera goes to it. No windows:
/// the launcher.
fn overview() -> ExitCode {
    use std::process::{Command, Stdio};
    // The edge handles overview interaction directly; keep fuzzel/rofi/wofi as fallback without edge.
    if edge_running_in(&runtime_dir(), std::path::Path::new("/proc")) {
        return ExitCode::SUCCESS;
    }
    let snap = request(&Value::obj([
        ("id", 1u64.into()),
        ("cmd", "snapshot".into()),
    ]));
    let lines = overview_lines(&snap);
    let have = |p: &str| {
        std::env::var_os("PATH")
            .is_some_and(|path| std::env::split_paths(&path).any(|d| d.join(p).is_file()))
    };
    if lines.is_empty() {
        let launcher = std::env::var("LAUNCHER")
            .ok()
            .filter(|l| !l.is_empty() && l != "auto")
            .unwrap_or_else(|| "fuzzel".into());
        let _ = Command::new("sh").args(["-c", &launcher]).status();
        return ExitCode::SUCCESS;
    }
    let picker = std::env::var("MENU_PICKER")
        .ok()
        .filter(|p| !p.trim().is_empty());
    let mut menu = match picker {
        Some(p) => {
            let mut c = Command::new("sh");
            c.args(["-c", &p]);
            c
        }
        None if have("fuzzel") => {
            let mut c = Command::new("fuzzel");
            c.args(["--dmenu", "-p", "Overview: "]);
            c
        }
        None if have("rofi") => {
            let mut c = Command::new("rofi");
            c.args(["-dmenu", "-p", "Overview"]);
            c
        }
        None if have("wofi") => {
            let mut c = Command::new("wofi");
            c.args(["--dmenu", "-p", "Overview: "]);
            c
        }
        None => return ExitCode::FAILURE,
    };
    let Ok(mut child) = menu.stdin(Stdio::piped()).stdout(Stdio::piped()).spawn() else {
        return ExitCode::FAILURE;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let text: Vec<&str> = lines.iter().map(|(l, _)| l.as_str()).collect();
        let _ = stdin.write_all(text.join("\n").as_bytes());
    }
    let Ok(out) = child.wait_with_output() else {
        return ExitCode::FAILURE;
    };
    let chosen = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let Some(&(_, (inst, elem))) = lines.iter().find(|(l, _)| *l == chosen) else {
        return ExitCode::SUCCESS;
    };
    request(&Value::obj([
        ("id", 1u64.into()),
        ("cmd", "focus".into()),
        ("element", element(inst, elem)),
    ]));
    let output = first_output();
    request(&Value::obj([
        ("id", 2u64.into()),
        ("cmd", "navigate".into()),
        ("output", output.into()),
        ("to", Value::obj([("element", element(inst, elem))])),
    ]));
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_overview_lists_each_window_with_its_app_and_title() {
        let snap = json::parse(
            r#"{"ok":true,"snapshot":{"elements":[
                {"id":[624742772932336,3],"app_id":"kitty","title":"~"},
                {"id":[624742772932336,5],"app_id":null,"title":""},
                {"id":"bad"}]}}"#,
        )
        .unwrap();
        assert_eq!(
            overview_lines(&snap),
            vec![
                ("[1] kitty — ~".to_string(), (624742772932336, 3)),
                ("[2] window — Untitled".to_string(), (624742772932336, 5)),
            ]
        );
    }

    #[test]
    fn no_compositor_is_a_failed_reply_not_a_crash() {
        let reply = request_at(
            std::path::Path::new("/nonexistent/infynitae/shell.sock"),
            &Value::obj([("cmd", "snapshot".into())]),
        );
        assert_eq!(reply.get("ok"), Some(&Value::Bool(false)));
    }

    #[test]
    fn capture_chord_parses_replies_and_errors() {
        let ok = Value::obj([
            ("id", 1u64.into()),
            ("ok", true.into()),
            ("generation", 2u64.into()),
            ("chord", "SUPER+SHIFT+Q".into()),
        ]);
        assert_eq!(capture_chord_from_reply(&ok), Ok("SUPER+SHIFT+Q"));

        let cancelled = Value::obj([("ok", false.into()), ("error", "cancelled".into())]);
        assert_eq!(capture_chord_from_reply(&cancelled), Err("cancelled"));

        let timeout = Value::obj([("ok", false.into()), ("error", "timeout".into())]);
        assert_eq!(capture_chord_from_reply(&timeout), Err("timeout"));

        let missing_chord = Value::obj([("ok", true.into())]);
        assert_eq!(
            capture_chord_from_reply(&missing_chord),
            Err("capture_failed")
        );
    }

    #[test]
    fn request_at_timeout_reads_capture_reply_over_unix_socket() {
        let dir = std::env::temp_dir().join(format!("infynitae-msg-cap-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let sock_path = dir.join("shell.sock");
        let _ = std::fs::remove_file(&sock_path);

        let listener = std::os::unix::net::UnixListener::bind(&sock_path).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut req_line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut req_line)
                .unwrap();
            let req = json::parse(req_line.trim()).unwrap();
            assert_eq!(
                req.get("cmd").and_then(Value::as_str),
                Some("capture-chord")
            );
            let reply = infynitae_ipc::protocol::capture_reply(
                req.get("id").cloned().unwrap_or(Value::Null),
                7,
                "SUPER+RETURN",
            );
            writeln!(stream, "{reply}").unwrap();
        });

        let reply = request_at_timeout(
            &sock_path,
            &Value::obj([("id", 9u64.into()), ("cmd", "capture-chord".into())]),
            Duration::from_secs(2),
        );
        server.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(capture_chord_from_reply(&reply), Ok("SUPER+RETURN"));
        assert_eq!(reply.get("id").and_then(Value::as_u64), Some(9));
    }

    #[test]
    fn navigate_args_accept_optional_zoom_and_reject_invalid_values() {
        let s = |v: &[&str]| v.iter().map(|x| (*x).to_string()).collect::<Vec<_>>();
        assert_eq!(
            parse_navigate_args(&s(&["navigate", "120", "-40.5", "1.5"])),
            Some((
                Value::obj([("canvas", Value::Arr(vec![120.0.into(), (-40.5).into()]))]),
                Some(1.5),
            ))
        );
        assert_eq!(
            parse_navigate_args(&s(&["navigate", "0", "0"])),
            Some((
                Value::obj([("canvas", Value::Arr(vec![0.0.into(), 0.0.into()]))]),
                None,
            ))
        );
        assert_eq!(
            parse_navigate_args(&s(&["navigate", "element", "4", "9"])),
            Some((Value::obj([("element", element(4, 9))]), None))
        );
        for bad in [
            &["navigate", "0", "0", "0"][..],
            &["navigate", "0", "0", "-1.0"][..],
            &["navigate", "0", "0", "nan"][..],
            &["navigate", "inf", "0"][..],
            &["navigate", "element", "4", "9", "1.5"][..],
            &["navigate", "1", "2", "1.0", "extra"][..],
        ] {
            assert_eq!(parse_navigate_args(&s(bad)), None, "{bad:?}");
        }
    }

    #[test]
    fn place_extracts_camera_and_upserts_config_preserving_chord() {
        let snap = json::parse(
            r#"{"ok":true,"snapshot":{"outputs":[{"id":1,"camera":{"center":[1250.25,-0.00001],"zoom":1.333333}}]}}"#,
        )
        .unwrap();
        let (x, y, zoom) = snapshot_camera(&snap).unwrap();

        let initial = "# user config\nBORDER=2\nPLACE_WORK=\"0,0,1 SUPER+F1\"\n";
        let (updated, line) = upsert_place_in_config(initial, "work", x, y, zoom);
        assert_eq!(line, "PLACE_WORK=\"1250.25,0,1.3333 SUPER+F1\"");
        assert_eq!(
            updated,
            "# user config\nBORDER=2\nPLACE_WORK=\"1250.25,0,1.3333 SUPER+F1\"\n"
        );

        let (appended, home_line) = upsert_place_in_config(&updated, "Home_2", -300.0, 40.0, 1.0);
        assert_eq!(home_line, "PLACE_HOME_2=\"-300,40,1\"");
        assert_eq!(
            appended,
            "# user config\nBORDER=2\nPLACE_WORK=\"1250.25,0,1.3333 SUPER+F1\"\nPLACE_HOME_2=\"-300,40,1\"\n"
        );

        assert!(valid_place_name("work_1"));
        assert!(!valid_place_name(""));
        assert!(!valid_place_name("my-place"));
    }

    #[test]
    fn edge_running_checks_pid_files_and_proc_comm() {
        let base = std::env::temp_dir().join(format!("infynitae-msg-edge-{}", std::process::id()));
        let rt = base.join("rt");
        let proc_root = base.join("proc");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&rt).unwrap();
        std::fs::create_dir_all(proc_root.join("4242")).unwrap();

        assert!(!edge_running_in(&rt, &proc_root));

        std::fs::write(rt.join("infynitae-edge-rs.pid"), "4242\n").unwrap();
        std::fs::write(proc_root.join("4242/comm"), "other-app\n").unwrap();
        assert!(!edge_running_in(&rt, &proc_root));

        std::fs::write(proc_root.join("4242/comm"), "infynitae-edge-rs\n").unwrap();
        assert!(edge_running_in(&rt, &proc_root));

        let _ = std::fs::remove_dir_all(&base);
    }
}
