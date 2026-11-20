//! infynitae-sandbox-socket SOCKET APP_ID ENGINE -- COMMAND [ARGS...]
//!
//! Opens the Wayland socket SOCKET for one sandboxed program through security-context-v1
//! (docs/SECURITY.md): the compositor serves it, and every client that connects through it is
//! sandboxed with this app id and sandbox engine, which the program inside cannot change. Then
//! runs COMMAND (the sandbox, which mounts SOCKET as its Wayland display) and exits with its
//! status. The compositor stops listening once the pipe's write end closes, which is when this
//! process ends, however it ends; the socket file is removed when COMMAND has ended.

use std::io::PipeWriter;
use std::os::fd::AsFd;
use std::os::unix::net::UnixListener;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::wp::security_context::v1::client::wp_security_context_manager_v1::WpSecurityContextManagerV1;
use wayland_protocols::wp::security_context::v1::client::wp_security_context_v1::WpSecurityContextV1;

struct State;

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as wayland_client::Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
wayland_client::delegate_noop!(State: ignore WpSecurityContextManagerV1);
wayland_client::delegate_noop!(State: ignore WpSecurityContextV1);

/// The socket file, removed when dropped.
struct SocketFile(PathBuf);

impl Drop for SocketFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Makes the listener and registers it; the returned pipe end keeps it open.
fn listen(socket: &Path, app_id: &str, engine: &str) -> Result<(SocketFile, PipeWriter), String> {
    let conn = Connection::connect_to_env()
        .map_err(|e| format!("cannot reach the compositor (WAYLAND_DISPLAY): {e}"))?;
    let (globals, mut queue) =
        registry_queue_init::<State>(&conn).map_err(|e| format!("the compositor: {e}"))?;
    let qh = queue.handle();
    // a sandboxed client is never offered the manager: a sandbox cannot make another
    let manager: WpSecurityContextManagerV1 = globals.bind(&qh, 1..=1, ()).map_err(|_| {
        "the compositor offers no security-context-v1 to this program (it may be sandboxed itself)"
            .to_string()
    })?;
    let listener = UnixListener::bind(socket).map_err(|e| format!("{}: {e}", socket.display()))?;
    let file = SocketFile(socket.to_path_buf());
    let (close_read, close_write) = std::io::pipe().map_err(|e| format!("a pipe: {e}"))?;
    let context = manager.create_listener(listener.as_fd(), close_read.as_fd(), &qh, ());
    context.set_sandbox_engine(engine.to_string());
    context.set_app_id(app_id.to_string());
    context.set_instance_id(std::process::id().to_string());
    context.commit();
    context.destroy();
    manager.destroy();
    // the requests and their descriptors reach the compositor, and a protocol error comes back
    // here rather than after the sandbox started
    queue
        .roundtrip(&mut State)
        .map_err(|e| format!("the compositor refused the listener: {e}"))?;
    Ok((file, close_write))
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    let (socket, app_id, engine, command) = match args {
        [socket, app_id, engine, dashes, command @ ..] if dashes == "--" && !command.is_empty() => {
            (socket, app_id, engine, command)
        }
        _ => {
            return Err(
                "usage: infynitae-sandbox-socket SOCKET APP_ID ENGINE -- COMMAND [ARGS...]".into(),
            )
        }
    };
    let (_file, _open) = listen(Path::new(socket), app_id, engine)?;
    // the pipe's ends are close-on-exec: the sandbox does not inherit them
    let status = Command::new(&command[0])
        .args(&command[1..])
        .status()
        .map_err(|e| format!("{}: {e}", command[0]))?;
    let code = status
        .code()
        .or_else(|| status.signal().map(|s| 128 + s))
        .unwrap_or(1);
    Ok(ExitCode::from(code.clamp(0, 255) as u8))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("infynitae-sandbox-socket: {e}");
            ExitCode::from(1)
        }
    }
}
