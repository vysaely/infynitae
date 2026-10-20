//! INC-11 exit tests on the production path of the shell protocol: the production Core and the
//! production server over real Unix sockets (C7.3, INV-SHELL-1/2, INV-STAGE-3, OB-35). Elements
//! are made through Core's protocol entries (the adapter's calls), so no Wayland client is needed;
//! the binary's end-to-end test (crates/infynitae/tests/shell.rs) covers the adapter and the loop.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

use infynitae_core::Core;
use infynitae_core::geom::{LocalPoint, LocalRect, LocalSize};
use infynitae_core::navigation::PersistedCameras;
use infynitae_core::output::{OutputDescriptor, OutputKey, Transform};
use infynitae_core::scale::InteractionKind;
use infynitae_core::snapshot::{
    InputRegion, SnapshotSurface, SurfaceKey, SurfaceRole, SurfaceTree,
};
use infynitae_core::stage::ElementId;
use infynitae_ipc::ShellServer;
use infynitae_ipc::json::{self, Value};

fn win(k: u64, w: i32, h: i32, mapped: bool) -> SurfaceTree {
    SurfaceTree {
        geometry: LocalRect {
            loc: LocalPoint::default(),
            size: LocalSize { w, h },
        },
        surfaces: vec![SnapshotSurface {
            key: SurfaceKey::from_raw(std::num::NonZeroU64::new(k).unwrap()),
            role: SurfaceRole::Root,
            size: if mapped {
                LocalSize { w, h }
            } else {
                LocalSize::default()
            },
            mapped,
            input: InputRegion::Infinite,
            frame_requested: false,
        }],
    }
}

/// A Core with one 1920×1080 output, and a server with nothing connected.
fn setup() -> (Core, ShellServer, u64) {
    let (mut core, _input, shell, mut token) = Core::new(PersistedCameras::default());
    let (o, _) = core
        .output_connected(
            &mut token,
            OutputDescriptor {
                key: OutputKey {
                    device: "test".into(),
                    connector: "T-1".into(),
                    make: "Test".into(),
                    model: "Test".into(),
                    serial: None,
                },
                mode: (1920, 1080),
                transform: Transform::Normal,
                scale_120: 120,
                refresh_mhz: 60_000,
            },
        )
        .unwrap();
    let server = ShellServer::new(shell, Arc::new(|| {}));
    (core, server, o.get())
}

fn map_win(core: &mut Core, k: u64, w: i32, h: i32) -> ElementId {
    let id = core.toplevel_created();
    core.toplevel_committed(id, win(k, w, h, false)).unwrap();
    core.toplevel_committed(id, win(k, w, h, true)).unwrap();
    id
}

/// A connected shell: its end of a socket pair, read line by line.
struct Shell {
    w: UnixStream,
    r: BufReader<UnixStream>,
}

fn connect(server: &mut ShellServer) -> Shell {
    let (a, b) = UnixStream::pair().unwrap();
    server.connect(a);
    b.set_read_timeout(Some(Duration::from_millis(5))).unwrap();
    Shell {
        r: BufReader::new(b.try_clone().unwrap()),
        w: b,
    }
}

impl Shell {
    fn send(&mut self, line: &str) {
        self.w.write_all(line.as_bytes()).unwrap();
        self.w.write_all(b"\n").unwrap();
    }

    /// The lines that arrive while `server` is serviced, until `done` says enough (bounded).
    fn read_until(
        &mut self,
        server: &mut ShellServer,
        core: &mut Core,
        done: impl Fn(&[Value]) -> bool,
    ) -> Vec<Value> {
        let mut got = Vec::new();
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end && !done(&got) {
            server.service(core);
            let mut line = String::new();
            match self.r.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => got.push(json::parse(line.trim_end()).expect("the server sends JSON")),
                Err(_) => {} // timeout: service again
            }
        }
        got
    }

    /// Sends a request and returns everything up to and including its reply (events first).
    fn request(&mut self, server: &mut ShellServer, core: &mut Core, line: &str) -> Vec<Value> {
        self.send(line);
        self.read_until(server, core, |got| {
            got.last().is_some_and(|v| v.get("ok").is_some())
        })
    }

    fn reply(&mut self, server: &mut ShellServer, core: &mut Core, line: &str) -> Value {
        self.request(server, core, line).pop().expect("a reply")
    }
}

fn gen_of(v: &Value) -> u64 {
    v.get("generation").and_then(Value::as_u64).unwrap()
}

fn eref(core: &Core, id: ElementId) -> String {
    format!("[{},{}]", core.stage().instance().get(), id.get())
}

/// C7.2 Queries: the snapshot holds the outputs (with their camera), the elements bottom to top
/// with title and app_id, the focus and the attention.
#[test]
fn a_snapshot_is_a_derived_copy_of_core() {
    let (mut core, mut server, o) = setup();
    let a = map_win(&mut core, 1, 400, 300);
    let b = map_win(&mut core, 2, 300, 200);
    core.toplevel_metadata(b, Some("Terminal"), Some("org.example.Term"))
        .unwrap();
    let mut shell = connect(&mut server);
    let reply = shell.reply(&mut server, &mut core, r#"{"id":1,"cmd":"snapshot"}"#);
    assert_eq!(reply.get("id").and_then(Value::as_u64), Some(1));
    assert_eq!(reply.get("ok"), Some(&Value::Bool(true)));
    let s = reply.get("snapshot").unwrap();
    assert_eq!(
        s.get("instance").and_then(Value::as_u64),
        Some(core.stage().instance().get())
    );
    let outputs = s.get("outputs").and_then(Value::as_array).unwrap();
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0].get("id").and_then(Value::as_u64), Some(o));
    assert_eq!(outputs[0].get("name").and_then(Value::as_str), Some("T-1"));
    assert!(
        outputs[0]
            .get("camera")
            .and_then(|c| c.get("zoom"))
            .is_some()
    );
    let elements = s.get("elements").and_then(Value::as_array).unwrap();
    let ids: Vec<String> = elements
        .iter()
        .map(|e| e.get("id").unwrap().to_json())
        .collect();
    assert_eq!(ids, [eref(&core, a), eref(&core, b)], "bottom to top");
    assert_eq!(
        elements[1].get("title").and_then(Value::as_str),
        Some("Terminal")
    );
    assert_eq!(
        elements[1].get("app_id").and_then(Value::as_str),
        Some("org.example.Term")
    );
    assert_eq!(elements[0].get("title"), Some(&Value::Null));
    assert_eq!(
        elements[0].get("size").map(Value::to_json).as_deref(),
        Some("[400,300]")
    );
}

/// C7.3: commands run in the order sent; each reply carries its request id and the generation
/// that resulted; a request that fails changes nothing and moves no generation (atomic).
#[test]
fn commands_run_in_order_and_failures_change_nothing() {
    let (mut core, mut server, o) = setup();
    let w = map_win(&mut core, 1, 400, 300);
    let mut shell = connect(&mut server);
    let g0 = gen_of(&shell.reply(&mut server, &mut core, r#"{"id":"a","cmd":"snapshot"}"#));
    // three commands in one write: executed in this order
    shell.send(&format!(
        r#"{{"id":1,"cmd":"navigate","output":{o},"to":{{"canvas":[100,100]}}}}
{{"id":2,"cmd":"navigate","output":{o},"to":{{"delta":[50,-50]}}}}
{{"id":3,"cmd":"move_element","element":{},"to":[7.5,8.5]}}"#,
        eref(&core, w)
    ));
    let got = shell.read_until(&mut server, &mut core, |g| {
        g.iter().filter(|v| v.get("ok").is_some()).count() == 3
    });
    let replies: Vec<&Value> = got.iter().filter(|v| v.get("ok").is_some()).collect();
    let ids: Vec<u64> = replies
        .iter()
        .filter_map(|r| r.get("id")?.as_u64())
        .collect();
    assert_eq!(ids, [1, 2, 3]);
    let gens: Vec<u64> = replies.iter().map(|r| gen_of(r)).collect();
    assert!(
        gens[0] > g0 && gens[1] > gens[0] && gens[2] > gens[1],
        "{gens:?}"
    );
    let q = 1i64 << 16;
    assert_eq!(
        core.cameras().center(0),
        Some(infynitae_core::geom::CanvasPos {
            x: 150 * q,
            y: 50 * q
        })
    );
    assert_eq!(
        core.stage().anchor(w),
        Some(infynitae_core::geom::CanvasPoint { x: 8, y: 8 }),
        "R3: 7.5 → 8, 8.5 → 8 (ties to even)"
    );

    let before = core.cameras().center(0);
    let g = gens[2];
    for (line, code) in [
        (
            format!(r#"{{"id":4,"cmd":"navigate","output":{o},"to":{{"canvas":[1e12,0]}}}}"#),
            "invalid_argument",
        ),
        (
            format!(
                r#"{{"id":4,"cmd":"navigate","output":{},"to":{{"canvas":[0,0]}}}}"#,
                o + 7
            ),
            "unknown_output",
        ),
        (r#"{"id":4,"cmd":"navigate"}"#.to_owned(), "invalid_request"),
        (r#"{"id":4,"cmd":"teleport"}"#.to_owned(), "unknown_command"),
        ("{not json".to_owned(), "parse_error"),
        (
            r#"{"id":4,"cmd":"viewport_restore","output":1e30}"#.to_owned(),
            "invalid_request",
        ),
    ] {
        let r = shell.reply(&mut server, &mut core, &line);
        assert_eq!(r.get("ok"), Some(&Value::Bool(false)), "{line}");
        assert_eq!(r.get("error").and_then(Value::as_str), Some(code), "{line}");
    }
    assert_eq!(core.cameras().center(0), before);
    let r = shell.reply(&mut server, &mut core, r#"{"id":5,"cmd":"snapshot"}"#);
    assert_eq!(gen_of(&r), g, "no failed request moved the generation");
}

/// INV-STAGE-3: a destroyed element's reference and another instance's fail with stale_element; a
/// later element never answers to them.
#[test]
fn stale_references_fail_and_never_alias() {
    let (mut core, mut server, o) = setup();
    let w = map_win(&mut core, 1, 400, 300);
    let old = eref(&core, w);
    core.toplevel_destroyed(w);
    let fresh = map_win(&mut core, 2, 400, 300);
    assert_ne!(eref(&core, fresh), old);
    let mut shell = connect(&mut server);
    let other = core.stage().instance().get() ^ 1;
    for line in [
        format!(r#"{{"id":1,"cmd":"focus","element":{old}}}"#),
        format!(r#"{{"id":1,"cmd":"viewport_fit","output":{o},"element":{old}}}"#),
        format!(
            r#"{{"id":1,"cmd":"focus","element":[{other},{}]}}"#,
            fresh.get()
        ),
    ] {
        let r = shell.reply(&mut server, &mut core, &line);
        assert_eq!(
            r.get("error").and_then(Value::as_str),
            Some("stale_element"),
            "{line}"
        );
    }
    assert_eq!(core.focus().target(), None);
}

/// INV-SHELL-2: events carry increasing generations; a subscriber that missed some re-syncs with a
/// Snapshot and the events whose generation is greater than the snapshot's.
#[test]
fn subscribers_get_ordered_events_and_resync_by_snapshot() {
    let (mut core, mut server, o) = setup();
    let mut sub = connect(&mut server);
    let mut cmd = connect(&mut server);
    let r = sub.reply(&mut server, &mut core, r#"{"id":1,"cmd":"subscribe"}"#);
    let g0 = gen_of(&r);
    assert!(r.get("snapshot").is_some());
    let w = map_win(&mut core, 1, 400, 300);
    cmd.reply(
        &mut server,
        &mut core,
        &format!(r#"{{"id":2,"cmd":"navigate","output":{o},"to":{{"canvas":[10,10]}}}}"#),
    );
    let events = sub.read_until(&mut server, &mut core, |g| {
        g.iter()
            .any(|e| e.get("event").and_then(Value::as_str) == Some("output_changed"))
    });
    let names: Vec<&str> = events
        .iter()
        .filter_map(|e| e.get("event")?.as_str())
        .collect();
    assert!(names.contains(&"element_added"), "{names:?}");
    assert!(names.contains(&"output_changed"), "{names:?}");
    let gens: Vec<u64> = events.iter().map(gen_of).collect();
    assert!(
        gens.windows(2).all(|p| p[0] <= p[1]) && gens[0] > g0,
        "{gens:?}"
    );
    // a snapshot now, then one more change: the subscriber's next events are newer than it
    let snap = gen_of(&sub.reply(&mut server, &mut core, r#"{"id":3,"cmd":"snapshot"}"#));
    core.toplevel_metadata(w, Some("x"), None).unwrap();
    let later = sub.read_until(&mut server, &mut core, |g| !g.is_empty());
    assert_eq!(
        later[0].get("event").and_then(Value::as_str),
        Some("element_changed")
    );
    assert!(gen_of(&later[0]) > snap);
}

/// C7.3 reconnect and shell crash: the compositor keeps nothing of a closed connection; a stream it
/// left open is ended (its epoch settles); a new connection starts from a Snapshot.
#[test]
fn a_closed_connection_ends_its_stream_and_a_reconnect_resyncs() {
    let (mut core, mut server, _o) = setup();
    let w = map_win(&mut core, 1, 400, 300);
    let mut shell = connect(&mut server);
    let line = format!(
        r#"{{"id":1,"cmd":"move_element","element":{},"to":[5,5],"stream":"begin"}}"#,
        eref(&core, w)
    );
    let r = shell.reply(&mut server, &mut core, &line);
    assert_eq!(r.get("ok"), Some(&Value::Bool(true)));
    let open = |core: &Core| {
        core.scale_state(w)
            .is_some_and(|s| s.epoch.open && s.epoch.kind == Some(InteractionKind::ShellStream))
    };
    assert!(open(&core));
    drop(shell); // the shell crashed
    let end = Instant::now() + Duration::from_secs(5);
    while server.connections() > 0 && Instant::now() < end {
        server.service(&mut core);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(server.connections(), 0);
    assert!(!open(&core), "the stream's epoch settled");
    assert_eq!(
        core.stage().anchor(w),
        Some(infynitae_core::geom::CanvasPoint { x: 5, y: 5 }),
        "nothing rolled back"
    );
    let mut again = connect(&mut server);
    let r = again.reply(&mut server, &mut core, r#"{"id":1,"cmd":"snapshot"}"#);
    assert!(r.get("snapshot").is_some());
}

/// A shell that stops reading is disconnected once its queue is full; the compositor never waits
/// for it. A line longer than the limit ends its connection too.
#[test]
fn a_shell_that_does_not_read_or_floods_is_disconnected() {
    let (mut core, mut server, o) = setup();
    let silent = connect(&mut server);
    let mut sub_line = silent.w.try_clone().unwrap();
    sub_line
        .write_all(b"{\"id\":1,\"cmd\":\"subscribe\"}\n")
        .unwrap();
    let mut cmd = connect(&mut server);
    let _ = cmd.reply(&mut server, &mut core, r#"{"id":0,"cmd":"snapshot"}"#);
    assert_eq!(server.connections(), 2);
    // each navigate is one output_changed event for the subscriber, which never reads
    let mut n = 0;
    let end = Instant::now() + Duration::from_secs(20);
    while server.connections() == 2 && Instant::now() < end {
        n += 1;
        cmd.send(&format!(
            r#"{{"id":{n},"cmd":"navigate","output":{o},"to":{{"canvas":[{n},0]}}}}"#
        ));
        server.service(&mut core);
        if n % 64 == 0 {
            // let the reader catch up; the replies to `cmd` are read and dropped
            let _ = cmd.read_until(&mut server, &mut core, |g| g.len() >= 64);
        }
    }
    assert_eq!(
        server.connections(),
        1,
        "the silent subscriber was dropped after {n} events"
    );
    drop(silent);
    let mut flood = connect(&mut server);
    let long = "x".repeat(infynitae_ipc::protocol::MAX_LINE + 10);
    let _ = flood.w.write_all(long.as_bytes());
    let end = Instant::now() + Duration::from_secs(5);
    while server.connections() == 2 && Instant::now() < end {
        server.service(&mut core);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        server.connections(),
        1,
        "the flooding connection was closed"
    );
}

/// OB-35 (its T1 criterion): an attention mark is an event; it does not survive unmap or destroy,
/// and the shell is told so by an event.
#[test]
fn attention_marks_end_with_unmap_and_destroy_and_the_shell_is_told() {
    let (mut core, mut server, _o) = setup();
    let a = map_win(&mut core, 1, 400, 300);
    let b = map_win(&mut core, 2, 300, 200);
    let mut sub = connect(&mut server);
    sub.reply(&mut server, &mut core, r#"{"id":1,"cmd":"subscribe"}"#);
    let attention = |events: &[Value]| -> Option<String> {
        events
            .iter()
            .rfind(|e| e.get("event").and_then(Value::as_str) == Some("attention"))
            .map(|e| e.get("elements").unwrap().to_json())
    };
    for (victim, end) in [(a, "unmap"), (b, "destroy")] {
        core.activation_request(victim, false).unwrap();
        let ev = sub.read_until(&mut server, &mut core, |g| attention(g).is_some());
        assert_eq!(
            attention(&ev),
            Some(format!("[{}]", eref(&core, victim))),
            "{end}"
        );
        if end == "unmap" {
            core.toplevel_committed(victim, win(1, 400, 300, false))
                .unwrap();
        } else {
            core.toplevel_destroyed(victim);
        }
        let ev = sub.read_until(&mut server, &mut core, |g| attention(g).is_some());
        assert_eq!(
            attention(&ev).as_deref(),
            Some("[]"),
            "no mark survives the {end}"
        );
        assert!(core.focus().attention().is_empty());
    }
}
