//! Session D-Bus helpers: one cached session connection for card fetches, and pre-connected
//! Unix sockets handed to `Builder::unix_stream` with `guid=` verification.

use std::os::unix::net::{SocketAddr, UnixStream};
use std::sync::{Arc, Mutex};
use zbus::address::transport::{Transport, UnixSocket};
use zbus::connection::Builder;
use zbus::{Address, Connection};

/// Cache of one shared session bus connection for card data fetches.
struct SessionCache(Option<(String, Connection)>);

impl SessionCache {
    fn get_or_connect(&mut self, addr: &Address) -> zbus::Result<Connection> {
        let key = addr.to_string();
        if let Some((cached_key, conn)) = &self.0 {
            if *cached_key == key && !conn.is_closed() {
                return Ok(conn.clone());
            }
        }
        let conn = build_on(addr, |b| b)?;
        self.0 = Some((key, conn.clone()));
        Ok(conn)
    }
}

static SESSION: Mutex<SessionCache> = Mutex::new(SessionCache(None));

/// Returns the shared session bus connection for card fetches, connecting once if needed.
pub fn session() -> zbus::Result<Connection> {
    let addr = Address::session()?;
    let mut cache = SESSION.lock().unwrap_or_else(|p| p.into_inner());
    cache.get_or_connect(&addr)
}

/// Returns a `Builder` for `addr`, pre-connecting Unix sockets on the calling thread.
pub fn builder_for(addr: &Address) -> zbus::Result<Builder<'static>> {
    let Transport::Unix(unix) = addr.transport() else {
        return Ok(Builder::address(addr.clone()));
    };
    let sock_addr = unix_socket_addr(unix.path())?;
    let stream = UnixStream::connect_addr(&sock_addr)
        .map_err(|e| zbus::Error::Connection(Arc::new(e), Box::new(addr.clone())))?;
    Ok(Builder::unix_stream(stream))
}

/// Builds a connection to `addr`, applying `configure` and verifying any `guid=` in `addr`.
pub fn build_on<F>(addr: &Address, configure: F) -> zbus::Result<Connection>
where
    F: FnOnce(Builder<'static>) -> Builder<'static>,
{
    let builder = configure(builder_for(addr)?);
    let conn = zbus::block_on(builder.build())?;
    verify_guid(&conn, addr)?;
    Ok(conn)
}

/// Builds a session bus connection, applying `configure`.
pub fn build_session<F>(configure: F) -> zbus::Result<Connection>
where
    F: FnOnce(Builder<'static>) -> Builder<'static>,
{
    let addr = Address::session()?;
    build_on(&addr, configure)
}

fn verify_guid(conn: &Connection, addr: &Address) -> zbus::Result<()> {
    if let Some(expected) = addr.guid() {
        if conn.server_guid().as_str() != expected.as_str() {
            return Err(zbus::Error::Handshake(format!(
                "Server GUID mismatch: expected {expected}, got {}",
                conn.server_guid()
            )));
        }
    }
    Ok(())
}

fn unix_socket_addr(path: &UnixSocket) -> zbus::Result<SocketAddr> {
    match path {
        UnixSocket::File(p) => SocketAddr::from_pathname(p).map_err(Into::into),
        #[cfg(target_os = "linux")]
        UnixSocket::Abstract(name) => {
            use std::os::linux::net::SocketAddrExt;
            SocketAddr::from_abstract_name(name.as_encoded_bytes()).map_err(Into::into)
        }
        _ => Err(zbus::Error::Unsupported),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixListener;
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Bus {
        daemon: Child,
        address: String,
        dir: PathBuf,
    }

    impl Bus {
        fn start(tag: &str) -> Option<Self> {
            static SEQ: AtomicUsize = AtomicUsize::new(0);
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "infynitae-edge-apps-dbus-{tag}-{}-{n}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let spawned = Command::new("dbus-daemon")
                .arg("--session")
                .arg("--nofork")
                .arg("--print-address=1")
                .arg(format!("--address=unix:dir={}", dir.display()))
                .stdout(Stdio::piped())
                .spawn();
            let mut daemon = match spawned {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("skipped: no dbus-daemon to run a private bus: {e}");
                    let _ = std::fs::remove_dir_all(&dir);
                    return None;
                }
            };
            let mut address = String::new();
            BufReader::new(daemon.stdout.take().unwrap())
                .read_line(&mut address)
                .unwrap();
            let address = address.trim().to_string();
            assert!(!address.is_empty(), "dbus-daemon printed no address");
            Some(Self {
                daemon,
                address,
                dir,
            })
        }

        fn addr(&self) -> Address {
            Address::try_from(self.address.as_str()).unwrap()
        }
    }

    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.daemon.kill();
            let _ = self.daemon.wait();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn builder_for_supports_abstract_socket_and_rejects_dir_address() {
        let dir_addr = Address::try_from("unix:dir=/tmp").unwrap();
        assert!(matches!(
            builder_for(&dir_addr),
            Err(zbus::Error::Unsupported)
        ));

        #[cfg(target_os = "linux")]
        {
            use std::os::linux::net::SocketAddrExt;
            let abs_name = format!("infynitae-edge-apps-test-{}", std::process::id());
            let sock_addr = SocketAddr::from_abstract_name(abs_name.as_bytes()).unwrap();
            let listener = UnixListener::bind_addr(&sock_addr).unwrap();
            let addr = Address::try_from(format!("unix:abstract={abs_name}").as_str()).unwrap();
            assert!(builder_for(&addr).is_ok());
            drop(listener);
        }
    }

    #[test]
    fn build_on_verifies_guid_and_session_cache_reuses_one_connection() {
        let Some(bus1) = Bus::start("cache-1") else {
            return;
        };
        let addr1 = bus1.addr();

        // Wrong GUID in address fails handshake verification.
        let wrong_guid = "00000000000000000000000000000000";
        let path_part = bus1
            .address
            .split(',')
            .find(|p| p.starts_with("unix:path="))
            .unwrap();
        let bad_addr =
            Address::try_from(format!("{path_part},guid={wrong_guid}").as_str()).unwrap();
        assert!(matches!(
            build_on(&bad_addr, |b| b),
            Err(zbus::Error::Handshake(_))
        ));

        let mut cache = SessionCache(None);
        let c1 = cache.get_or_connect(&addr1).unwrap();
        let c2 = cache.get_or_connect(&addr1).unwrap();
        assert_eq!(
            c1.unique_name().unwrap().as_str(),
            c2.unique_name().unwrap().as_str(),
            "card fetches on the same bus must reuse one connection"
        );

        // Changing bus address opens a connection to the new bus.
        let Some(bus2) = Bus::start("cache-2") else {
            return;
        };
        let c3 = cache.get_or_connect(&bus2.addr()).unwrap();
        assert_ne!(c1.server_guid(), c3.server_guid());
    }
}
