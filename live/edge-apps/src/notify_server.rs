//! The edge as the session's notification server (org.freedesktop.Notifications, Desktop
//! Notifications Specification 1.2), in place of mako. zbus runs the object on its own executor
//! its own executor thread; each Notify / CloseNotification is handed to the edge's loop through a
//! calloop channel (`Incoming`), and the edge's answers (`Outgoing`) go back as the spec's signals.
//!
//! The name is asked for without queueing: when another daemon holds it (mako started by the
//! session or by D-Bus activation, a NOTIFICATIONS command), the edge does not serve, and that
//! daemon keeps working as before.

use crate::notify::{clean_body, clean_line, Incoming, Note, Outgoing};
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use zbus::fdo::RequestNameReply;
use zbus::zvariant::{OwnedValue, Value};

pub const NAME: &str = "org.freedesktop.Notifications";
pub const PATH: &str = "/org/freedesktop/Notifications";

struct Server {
    tx: Sender<Incoming>,
    next: AtomicU32,
    /// the id a synchronous tag got (the volume keys' next one replaces it)
    sync_ids: Mutex<HashMap<String, u32>>,
}

/// A whole number from a hint of any integer type (senders differ: urgency is a byte by the spec,
/// some send an int).
fn int(v: &Value) -> Option<i64> {
    match v {
        Value::U8(x) => Some(*x as i64),
        Value::I16(x) => Some(*x as i64),
        Value::U16(x) => Some(*x as i64),
        Value::I32(x) => Some(*x as i64),
        Value::U32(x) => Some(*x as i64),
        Value::I64(x) => Some(*x),
        Value::U64(x) => i64::try_from(*x).ok(),
        _ => None,
    }
}

fn text(v: &Value) -> Option<String> {
    match v {
        Value::Str(s) => Some(s.to_string()),
        _ => None,
    }
}

fn flag(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        other => int(other).is_some_and(|n| n != 0),
    }
}

/// A Notify call as the edge keeps it (`id` already chosen).
#[allow(clippy::too_many_arguments)]
pub fn note_from(
    id: u32,
    app_name: &str,
    summary: &str,
    body: &str,
    actions: &[String],
    hints: &HashMap<String, Value>,
    expire_timeout: i32,
    time: String,
) -> Note {
    let h = |k: &str| hints.get(k);
    let urgency = h("urgency").and_then(int).unwrap_or(1).clamp(0, 2) as u8;
    let value = h("value").and_then(int).map(|v| v.clamp(0, 100) as i32);
    let sync = [
        "x-canonical-private-synchronous",
        "synchronous",
        "private-synchronous",
    ]
    .iter()
    .find_map(|k| h(k).and_then(text))
    .filter(|s| !s.is_empty());
    let mut app = clean_line(app_name);
    if app.is_empty() {
        app = h("desktop-entry")
            .and_then(text)
            .map(|s| clean_line(&s))
            .unwrap_or_default();
    }
    // [key, label, key, label, ...]; an odd one left over is dropped
    let actions = actions
        .iter()
        .step_by(2)
        .zip(actions.iter().skip(1).step_by(2))
        .map(|(k, l)| (k.clone(), clean_line(l)))
        .filter(|(k, _)| !k.is_empty())
        .take(4)
        .collect();
    let expire = match expire_timeout {
        0 => None,
        t if t > 0 => Some(Some(Duration::from_millis(t as u64))),
        _ => Some(None),
    };
    Note {
        id,
        app,
        summary: clean_line(summary),
        body: clean_body(body),
        actions,
        urgency,
        value,
        sync,
        expire,
        transient: h("transient").is_some_and(flag),
        resident: h("resident").is_some_and(flag),
        time,
    }
}

impl Server {
    /// The id for a notification: the one it replaces, the one its synchronous tag had, or a new one.
    fn id_for(&self, replaces_id: u32, sync: Option<&str>) -> u32 {
        let mut map = self.sync_ids.lock().unwrap_or_else(|p| p.into_inner());
        let id = if replaces_id != 0 {
            replaces_id
        } else if let Some(id) = sync.and_then(|s| map.get(s).copied()) {
            id
        } else {
            loop {
                let id = self.next.fetch_add(1, Ordering::Relaxed);
                if id != 0 {
                    break id;
                }
            }
        };
        if let Some(s) = sync {
            map.insert(s.to_string(), id);
        }
        id
    }
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Server {
    fn get_capabilities(&self) -> Vec<String> {
        [
            "actions",
            "body",
            "persistence",
            "x-canonical-private-synchronous",
            "synchronous",
            "private-synchronous",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        _app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let hints: HashMap<String, Value> = hints
            .iter()
            .filter_map(|(k, v)| v.try_clone().ok().map(|v| (k.clone(), Value::from(v))))
            .collect();
        let sync = [
            "x-canonical-private-synchronous",
            "synchronous",
            "private-synchronous",
        ]
        .iter()
        .find_map(|k| hints.get(*k).and_then(text))
        .filter(|s| !s.is_empty());
        let id = self.id_for(replaces_id, sync.as_deref());
        let time = chrono::Local::now().format("%H:%M").to_string();
        let note = note_from(
            id,
            &app_name,
            &summary,
            &body,
            &actions,
            &hints,
            expire_timeout,
            time,
        );
        let _ = self.tx.send(Incoming::Notify(note));
        id
    }

    fn close_notification(&self, id: u32) {
        let _ = self.tx.send(Incoming::Close(id));
    }

    fn get_server_information(&self) -> (String, String, String, String) {
        (
            "Infynitae".into(),
            "Infynitae".into(),
            env!("CARGO_PKG_VERSION").into(),
            "1.2".into(),
        )
    }
}

/// Serves org.freedesktop.Notifications on the session bus; the connection it returns keeps it
/// served (and sends the signals). An error when the name is held by another daemon or there is
/// no session bus: the edge then does not serve.
pub fn start(tx: Sender<Incoming>) -> Result<zbus::blocking::Connection, String> {
    let e = |e: zbus::Error| e.to_string();
    let server = Server {
        tx,
        next: AtomicU32::new(1),
        sync_ids: Mutex::new(HashMap::new()),
    };
    let conn = zbus::blocking::connection::Builder::session()
        .map_err(e)?
        .serve_at(PATH, server)
        .map_err(e)?
        .build()
        .map_err(e)?;
    let ask = || {
        matches!(
            conn.request_name_with_flags(NAME, zbus::fdo::RequestNameFlags::DoNotQueue.into()),
            Ok(RequestNameReply::PrimaryOwner) | Ok(RequestNameReply::AlreadyOwner)
        )
    };
    if ask() {
        return Ok(conn);
    }
    // held: by a mako that D-Bus started by itself (a notification sent while the edge was not
    // running, e.g. restarted by an update); the config asks for the edge, so that mako goes and
    // the name is asked for again. Any other daemon is someone's choice and stays.
    let holder = owner_program(&conn);
    if holder.as_deref() == Some("mako") {
        if let Some(pid) = owner_pid(&conn) {
            unsafe { libc::kill(pid as i32, libc::SIGTERM) };
            for _ in 0..20 {
                std::thread::sleep(Duration::from_millis(50));
                if ask() {
                    return Ok(conn);
                }
            }
        }
    }
    Err(format!(
        "{NAME} is held by {}",
        holder.unwrap_or_else(|| "another daemon".into())
    ))
}

/// The process holding the notifications name.
fn owner_pid(conn: &zbus::blocking::Connection) -> Option<u32> {
    let dbus = zbus::blocking::fdo::DBusProxy::new(conn).ok()?;
    let name = zbus::names::BusName::try_from(NAME).ok()?;
    dbus.get_connection_unix_process_id(name).ok()
}

/// Its program's name (/proc/PID/comm).
fn owner_program(conn: &zbus::blocking::Connection) -> Option<String> {
    let pid = owner_pid(conn)?;
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|s| s.trim().to_string())
}

/// Sends what the edge decided as the spec's signals.
pub fn send(conn: &zbus::blocking::Connection, out: Vec<Outgoing>) {
    for o in out {
        let _ = match o {
            Outgoing::Action(id, key) => conn.emit_signal(
                None::<zbus::names::BusName>,
                PATH,
                NAME,
                "ActionInvoked",
                &(id, key),
            ),
            Outgoing::Closed(id, reason) => conn.emit_signal(
                None::<zbus::names::BusName>,
                PATH,
                NAME,
                "NotificationClosed",
                &(id, reason),
            ),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hints(pairs: &[(&str, Value<'static>)]) -> HashMap<String, Value<'static>> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn a_volume_key_notification_becomes_a_level_with_its_tag() {
        let h = hints(&[
            ("x-canonical-private-synchronous", Value::from("volume")),
            ("value", Value::I32(55)),
        ]);
        let n = note_from(3, "Audio", "Volume: 55%", "", &[], &h, 1200, "12:00".into());
        assert_eq!(n.value, Some(55));
        assert_eq!(n.sync.as_deref(), Some("volume"));
        assert_eq!(n.expire, Some(Some(Duration::from_millis(1200))));
    }

    #[test]
    fn hints_of_any_integer_type_and_the_spec_s_timeouts() {
        let h = hints(&[("urgency", Value::U8(2)), ("transient", Value::Bool(true))]);
        let n = note_from(1, "", "s", "b", &[], &h, -1, "t".into());
        assert_eq!(n.urgency, 2);
        assert!(n.transient);
        assert_eq!(n.expire, Some(None), "-1: the server's default");
        let h = hints(&[
            ("urgency", Value::I32(0)),
            ("desktop-entry", Value::from("firefox")),
        ]);
        let n = note_from(1, "", "s", "b", &[], &h, 0, "t".into());
        assert_eq!(n.urgency, 0);
        assert_eq!(n.app, "firefox", "no app name: the desktop entry");
        assert_eq!(n.expire, None, "0: until dismissed");
    }

    #[test]
    fn actions_come_in_pairs() {
        let a: Vec<String> = ["default", "Open", "reply", "Reply", "odd"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let n = note_from(1, "a", "s", "b", &a, &HashMap::new(), -1, "t".into());
        assert_eq!(
            n.actions,
            vec![
                ("default".into(), "Open".into()),
                ("reply".into(), "Reply".into())
            ]
        );
    }

    #[test]
    fn ids_replace_and_follow_the_synchronous_tag() {
        let (tx, _rx) = smithay_client_toolkit::reexports::calloop::channel::channel();
        let s = Server {
            tx,
            next: AtomicU32::new(1),
            sync_ids: Mutex::new(HashMap::new()),
        };
        assert_eq!(s.id_for(0, None), 1);
        assert_eq!(s.id_for(0, Some("volume")), 2);
        assert_eq!(s.id_for(0, Some("volume")), 2, "the same tag, the same id");
        assert_eq!(s.id_for(9, None), 9, "replaces_id wins");
        assert_eq!(s.id_for(0, None), 3);
    }
}
