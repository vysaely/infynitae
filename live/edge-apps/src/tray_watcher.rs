//! The edge as the session's tray watcher (org.kde.StatusNotifierWatcher), as the C edge was:
//! tray apps (Discord, Steam, Telegram...) register their item here, and the Background card
//! reads the list back over the bus like it reads any other watcher's. The edge also registers
//! itself as a host, with this watcher or with another one.
//!
//! The name is asked for in the queue and may be taken over: a bar or desktop that serves the
//! watcher (waybar, KDE) keeps it, and the edge gets it when that one quits. Items watch the
//! watcher's name and register again with each new owner (by the spec), so nothing is moved over.

use futures_core::Stream;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
#[cfg(test)]
use zbus::connection::Builder;
use zbus::fdo::{DBusProxy, RequestNameFlags, RequestNameReply};
use zbus::message::Header;
use zbus::object_server::SignalEmitter;
use zbus::Connection;

pub const NAME: &str = "org.kde.StatusNotifierWatcher";
pub const PATH: &str = "/StatusNotifierWatcher";
/// where an item registered by its bus name alone serves (the spec's default)
const ITEM_PATH: &str = "/StatusNotifierItem";

#[derive(Default)]
struct State {
    /// "bus/path", in the order they registered
    items: Vec<String>,
    /// the unique names of the hosts' connections
    hosts: Vec<String>,
}

impl State {
    /// Forgets what a bus name that went away served: its items (returned) and its host.
    fn drop_name(&mut self, name: &str) -> Vec<String> {
        self.hosts.retain(|h| h != name);
        let (gone, kept) = std::mem::take(&mut self.items)
            .into_iter()
            .partition(|e| item_bus(e) == name);
        self.items = kept;
        gone
    }
}

fn item_bus(entry: &str) -> &str {
    entry.find('/').map_or(entry, |i| &entry[..i])
}

/// The "bus/path" an item registered as: an object path is on the caller's connection, a bus
/// name is at the default path (the two forms apps use).
fn item_entry(service: &str, sender: Option<&str>) -> Option<String> {
    if service.starts_with('/') {
        sender
            .filter(|s| !s.is_empty())
            .map(|s| format!("{s}{service}"))
    } else if service.is_empty() {
        None
    } else {
        Some(format!("{service}{ITEM_PATH}"))
    }
}

struct Watcher {
    state: Arc<Mutex<State>>,
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(|p| p.into_inner())
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    async fn register_status_notifier_item(
        &self,
        service: String,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        let sender = hdr.sender().map(|s| s.as_str());
        let Some(entry) = item_entry(&service, sender) else {
            return Err(zbus::fdo::Error::InvalidArgs(format!(
                "not a bus name or object path: {service:?}"
            )));
        };
        let new = {
            let mut st = lock(&self.state);
            let new = !st.items.contains(&entry);
            if new {
                st.items.push(entry.clone());
            }
            new
        };
        if new {
            Self::status_notifier_item_registered(&emitter, &entry).await?;
        }
        Ok(())
    }

    async fn register_status_notifier_host(
        &self,
        _service: String,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        // kept by the caller's connection, which is what goes away when the host quits (some
        // hosts pass a service name they do not own)
        let Some(sender) = hdr.sender().map(|s| s.to_string()) else {
            return Ok(());
        };
        let new = {
            let mut st = lock(&self.state);
            let new = !st.hosts.contains(&sender);
            if new {
                st.hosts.push(sender);
            }
            new
        };
        if new {
            Self::status_notifier_host_registered(&emitter).await?;
        }
        Ok(())
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        lock(&self.state).items.clone()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        !lock(&self.state).hosts.is_empty()
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    async fn status_notifier_item_registered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_item_unregistered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_host_registered(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

/// Registers the edge as a host with whoever serves the watcher now.
async fn register_host(conn: &Connection, host: &str) {
    let res = conn
        .call_method(
            Some(NAME),
            PATH,
            Some(NAME),
            "RegisterStatusNotifierHost",
            &(host,),
        )
        .await;
    if let Err(e) = res {
        eprintln!("infynitae-edge-apps: tray: registering as a host: {e}");
    }
}

/// Serves the watcher on the session bus; the connection it returns keeps it served.
pub fn start() -> Result<Connection, String> {
    let addr = zbus::Address::session().map_err(|e| e.to_string())?;
    let state = Arc::new(Mutex::new(State::default()));
    let conn = crate::dbus::build_on(&addr, |b| {
        b.serve_at(
            PATH,
            Watcher {
                state: state.clone(),
            },
        )
    })
    .map_err(|e| e.to_string())?;
    start_connected(conn, state)
}

/// `start` on any bus (the tests run a private one).
#[cfg(test)]
pub fn start_on(builder: Builder<'_>) -> Result<Connection, String> {
    let state = Arc::new(Mutex::new(State::default()));
    let conn = zbus::block_on(
        builder
            .serve_at(
                PATH,
                Watcher {
                    state: state.clone(),
                },
            )
            .build(),
    )
    .map_err(|e| e.to_string())?;
    start_connected(conn, state)
}

fn start_connected(conn: Connection, state: Arc<Mutex<State>>) -> Result<Connection, String> {
    let e = |e: zbus::Error| e.to_string();
    let host = format!("org.kde.StatusNotifierHost-{}", std::process::id());
    let (mut changes, reply) = zbus::block_on(async {
        // subscribed before the names are asked for, so no item that goes away is missed
        let dbus = DBusProxy::new(&conn).await?;
        let changes = dbus.receive_name_owner_changed().await?;
        conn.request_name_with_flags(host.as_str(), RequestNameFlags::DO_NOT_QUEUE)
            .await?;
        let reply = conn
            .request_name_with_flags(NAME, RequestNameFlags::ALLOW_REPLACEMENT)
            .await?;
        Ok::<_, zbus::Error>((changes, reply))
    })
    .map_err(e)?;
    if matches!(reply, RequestNameReply::InQueue) {
        eprintln!("infynitae-edge-apps: tray: another watcher serves; the edge uses it");
    }

    let thread_conn = conn.clone();
    std::thread::Builder::new()
        .name("tray-watcher".into())
        .spawn(move || {
            zbus::block_on(async move {
                // here, not before the edge starts: another watcher may be slow to answer
                register_host(&thread_conn, &host).await;
                while let Some(sig) =
                    std::future::poll_fn(|cx| Pin::new(&mut changes).poll_next(cx)).await
                {
                    let Ok(args) = sig.args() else {
                        continue;
                    };
                    let name = args.name().as_str();
                    if name == NAME {
                        // a new watcher (another one, or the edge itself once that one quit)
                        if args.new_owner().is_some() {
                            register_host(&thread_conn, &host).await;
                        }
                        continue;
                    }
                    if args.new_owner().is_some() {
                        continue;
                    }
                    let gone = lock(&state).drop_name(name);
                    for entry in gone {
                        let res = thread_conn
                            .emit_signal(
                                None::<zbus::names::BusName>,
                                PATH,
                                NAME,
                                "StatusNotifierItemUnregistered",
                                &(entry.as_str(),),
                            )
                            .await;
                        if let Err(e) = res {
                            eprintln!("infynitae-edge-apps: tray: signal: {e}");
                        }
                    }
                }
            });
        })
        .map_err(|e| e.to_string())?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_item_registers_by_path_or_by_bus_name() {
        assert_eq!(
            item_entry("/org/ayatana/NotificationItem/discord", Some(":1.42")).as_deref(),
            Some(":1.42/org/ayatana/NotificationItem/discord")
        );
        assert_eq!(
            item_entry("org.kde.StatusNotifierItem-7-1", Some(":1.42")).as_deref(),
            Some("org.kde.StatusNotifierItem-7-1/StatusNotifierItem")
        );
        assert_eq!(item_entry("", Some(":1.42")), None);
        assert_eq!(item_entry("/StatusNotifierItem", None), None);
    }

    #[test]
    fn a_name_that_goes_away_takes_its_items_and_its_host() {
        let mut st = State {
            items: vec![
                ":1.5/StatusNotifierItem".into(),
                ":1.6/org/ayatana/NotificationItem/a".into(),
                ":1.5/org/ayatana/NotificationItem/b".into(),
            ],
            hosts: vec![":1.5".into(), ":1.9".into()],
        };
        assert_eq!(
            st.drop_name(":1.5"),
            vec![
                ":1.5/StatusNotifierItem".to_string(),
                ":1.5/org/ayatana/NotificationItem/b".to_string()
            ]
        );
        assert_eq!(st.items, vec![":1.6/org/ayatana/NotificationItem/a"]);
        assert_eq!(st.hosts, vec![":1.9"]);
        assert!(
            st.drop_name(":1.50").is_empty(),
            "a longer name is another one"
        );
    }
}
