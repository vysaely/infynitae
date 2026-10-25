//! The trusted shell's channel (C7.1): the socket `$XDG_RUNTIME_DIR/infynitae/shell.sock`
//! (directory 0700, socket 0600), admission by `SO_PEERCRED` (the peer's uid must be this
//! process's; owner decision 2026-10-20, D1: checked here, with rustix, so `infynitae-ipc` keeps
//! Core as its only dependency), and the server plugged into the backend's loop.

use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use infynitae_core::protocol::Effect;
use infynitae_core::{Core, ShellPort};
use infynitae_ipc::ShellServer;
use infynitae_wayland::extension::{Extension, ExtensionFactory, LoopExtension, waker};

/// The default socket (C7.1), or `None` without `XDG_RUNTIME_DIR`.
pub fn default_path() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(|d| PathBuf::from(d).join("infynitae/shell.sock"))
}

/// C7.1 admission: the connecting process runs as this process's user. Any process of the same
/// user may connect, exactly as with `hyprctl` (the honest trust statement of C7.1).
pub fn same_user(stream: &UnixStream) -> bool {
    rustix::net::sockopt::socket_peercred(stream)
        .is_ok_and(|cred| cred.uid == rustix::process::getuid())
}

/// The server as a loop extension; the socket file goes with it (after the server, whose drop
/// still connects to it once to end its acceptor).
struct ShellExtension {
    server: ShellServer,
    _socket: SocketFile,
}

struct SocketFile(PathBuf);

impl Drop for SocketFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

impl LoopExtension for ShellExtension {
    fn service(&mut self, core: &mut Core) -> (Vec<Effect>, bool) {
        let done = self.server.service(core);
        (done.effects, done.changed)
    }
}

/// The shell server as the backend starts it (after its signal handlers exist).
pub fn factory(port: ShellPort, path: PathBuf) -> ExtensionFactory {
    Box::new(move || {
        let ext = serve(port, &path)?;
        println!("[infynitae] Shell socket: {}", path.display());
        Ok(ext)
    })
}

/// Binds the socket at `path` and serves it with `port`. A live socket of another compositor at
/// the same path is an error (nothing is removed); a stale one (nobody accepts) is replaced.
pub fn serve(port: ShellPort, path: &Path) -> io::Result<Extension> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other("the shell socket path has no directory"))?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    // the directory may have existed with other permissions
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    if path.exists() {
        if UnixStream::connect(path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                format!("{} is served by another compositor", path.display()),
            ));
        }
        fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    let socket = SocketFile(path.to_path_buf());
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    let (w, wake) = waker()?;
    let mut server = ShellServer::new(port, Arc::new(move || w.wake()));
    server.listen(listener, path.to_path_buf(), Box::new(same_user));
    Ok(Extension {
        inner: Box::new(ShellExtension {
            server,
            _socket: socket,
        }),
        wake,
    })
}
