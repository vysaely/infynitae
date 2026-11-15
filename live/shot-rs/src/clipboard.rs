use std::fs::{File, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::os::fd::AsRawFd;
use std::path::Path;

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1, ext_data_control_manager_v1, ext_data_control_offer_v1,
    ext_data_control_source_v1,
};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1, zwlr_data_control_manager_v1, zwlr_data_control_offer_v1,
    zwlr_data_control_source_v1,
};

struct ClipboardState {
    png_bytes: Vec<u8>,
    done: bool,
}

impl ClipboardState {
    fn write_offer(&self, mime_type: &str, fd: std::os::fd::OwnedFd) {
        if mime_type != "image/png" {
            return;
        }
        let mut file = File::from(fd);
        if let Err(e) = write_all_ignoring_pipe(&mut file, &self.png_bytes) {
            eprintln!("infynitae-shot: the clipboard reader got a cut-off picture: {e}");
        }
    }
}

/// Writes all of `data`, waiting while the reader's pipe is full: readers may hand over a
/// non-blocking pipe (wl-clip-persist does), whose 64 KiB buffer fills before a screenshot is
/// written, and giving up there left the clipboard holding the first 64 KiB of the PNG.
fn write_all_ignoring_pipe(file: &mut File, mut data: &[u8]) -> io::Result<()> {
    while !data.is_empty() {
        match file.write(data) {
            Ok(0) => return Ok(()),
            Ok(n) => data = &data[n..],
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == ErrorKind::WouldBlock => wait_writable(file)?,
            Err(e) if e.kind() == ErrorKind::BrokenPipe => return Ok(()),
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Waits until `file` takes more data (or its reader went away, which the next write reports).
fn wait_writable(file: &File) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let mut pfd = libc::pollfd {
        fd: file.as_raw_fd(),
        events: libc::POLLOUT,
        revents: 0,
    };
    loop {
        // SAFETY: one valid pollfd for the call's duration
        let r = unsafe { libc::poll(&mut pfd, 1, 10_000) };
        match r {
            1.. => return Ok(()),
            0 => {
                return Err(io::Error::new(
                    ErrorKind::TimedOut,
                    "the reader stopped reading for 10 s",
                ))
            }
            _ if io::Error::last_os_error().kind() == ErrorKind::Interrupted => continue,
            _ => return Err(io::Error::last_os_error()),
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for ClipboardState {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for ClipboardState {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ext_data_control_manager_v1::ExtDataControlManagerV1, ()> for ClipboardState {
    fn event(
        _: &mut Self,
        _: &ext_data_control_manager_v1::ExtDataControlManagerV1,
        _: ext_data_control_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ext_data_control_device_v1::ExtDataControlDeviceV1, ()> for ClipboardState {
    fn event(
        state: &mut Self,
        device: &ext_data_control_device_v1::ExtDataControlDeviceV1,
        event: ext_data_control_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            // what the clipboard holds (our own PNG, or another program's): not read here
            ext_data_control_device_v1::Event::DataOffer { id } => id.destroy(),
            ext_data_control_device_v1::Event::Finished => {
                device.destroy();
                state.done = true;
            }
            _ => {}
        }
    }

    // the device announces each clipboard's contents as a new offer object (opcode 0): without
    // this, wayland-client panics on the first one (found in the VM, 2026-11-13)
    wayland_client::event_created_child!(ClipboardState, ext_data_control_device_v1::ExtDataControlDeviceV1, [
        ext_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ext_data_control_offer_v1::ExtDataControlOfferV1, ())
    ]);
}

impl Dispatch<ext_data_control_offer_v1::ExtDataControlOfferV1, ()> for ClipboardState {
    fn event(
        _: &mut Self,
        _: &ext_data_control_offer_v1::ExtDataControlOfferV1,
        _: ext_data_control_offer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ext_data_control_source_v1::ExtDataControlSourceV1, ()> for ClipboardState {
    fn event(
        state: &mut Self,
        source: &ext_data_control_source_v1::ExtDataControlSourceV1,
        event: ext_data_control_source_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_data_control_source_v1::Event::Send { mime_type, fd } => {
                state.write_offer(&mime_type, fd);
            }
            ext_data_control_source_v1::Event::Cancelled => {
                source.destroy();
                state.done = true;
            }
            _ => {}
        }
    }
}

impl Dispatch<zwlr_data_control_manager_v1::ZwlrDataControlManagerV1, ()> for ClipboardState {
    fn event(
        _: &mut Self,
        _: &zwlr_data_control_manager_v1::ZwlrDataControlManagerV1,
        _: zwlr_data_control_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwlr_data_control_device_v1::ZwlrDataControlDeviceV1, ()> for ClipboardState {
    fn event(
        state: &mut Self,
        device: &zwlr_data_control_device_v1::ZwlrDataControlDeviceV1,
        event: zwlr_data_control_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_data_control_device_v1::Event::DataOffer { id } => id.destroy(),
            zwlr_data_control_device_v1::Event::Finished => {
                device.destroy();
                state.done = true;
            }
            _ => {}
        }
    }

    wayland_client::event_created_child!(ClipboardState, zwlr_data_control_device_v1::ZwlrDataControlDeviceV1, [
        zwlr_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (zwlr_data_control_offer_v1::ZwlrDataControlOfferV1, ())
    ]);
}

impl Dispatch<zwlr_data_control_offer_v1::ZwlrDataControlOfferV1, ()> for ClipboardState {
    fn event(
        _: &mut Self,
        _: &zwlr_data_control_offer_v1::ZwlrDataControlOfferV1,
        _: zwlr_data_control_offer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwlr_data_control_source_v1::ZwlrDataControlSourceV1, ()> for ClipboardState {
    fn event(
        state: &mut Self,
        source: &zwlr_data_control_source_v1::ZwlrDataControlSourceV1,
        event: zwlr_data_control_source_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_data_control_source_v1::Event::Send { mime_type, fd } => {
                state.write_offer(&mime_type, fd);
            }
            zwlr_data_control_source_v1::Event::Cancelled => {
                source.destroy();
                state.done = true;
            }
            _ => {}
        }
    }
}

/// Prints `output_path` to stdout, offers `png_bytes` as `image/png` via data-control, and forks
/// a background child to serve paste requests until another client claims the clipboard.
///
/// The parent exits immediately via `_exit(0)` once the selection is registered so callers
/// unblock while keeping the Wayland connection alive in the child.
pub fn publish_and_fork_clipboard(output_path: &Path, png_bytes: Vec<u8>) -> ! {
    // Print and flush the written path before forking or redirecting descriptors.
    println!("{}", output_path.display());
    let _ = io::stdout().flush();
    let _ = io::stderr().flush();

    let Ok(conn) = Connection::connect_to_env() else {
        std::process::exit(0);
    };
    let Ok((globals, mut queue)) = registry_queue_init::<ClipboardState>(&conn) else {
        std::process::exit(0);
    };
    let qh = queue.handle();
    let mut state = ClipboardState {
        png_bytes,
        done: false,
    };

    let Ok(seat) = globals.bind::<wl_seat::WlSeat, _, _>(&qh, 1..=9, ()) else {
        eprintln!("infynitae-shot: clipboard unavailable (missing wl_seat)");
        std::process::exit(0);
    };

    enum ActiveControl {
        Ext {
            device: ext_data_control_device_v1::ExtDataControlDeviceV1,
            source: ext_data_control_source_v1::ExtDataControlSourceV1,
        },
        Wlr {
            device: zwlr_data_control_device_v1::ZwlrDataControlDeviceV1,
            source: zwlr_data_control_source_v1::ZwlrDataControlSourceV1,
        },
    }

    let control = if let Ok(mgr) =
        globals.bind::<ext_data_control_manager_v1::ExtDataControlManagerV1, _, _>(&qh, 1..=1, ())
    {
        let device = mgr.get_data_device(&seat, &qh, ());
        let source = mgr.create_data_source(&qh, ());
        source.offer("image/png".to_string());
        device.set_selection(Some(&source));
        ActiveControl::Ext { device, source }
    } else if let Ok(mgr) =
        globals.bind::<zwlr_data_control_manager_v1::ZwlrDataControlManagerV1, _, _>(&qh, 1..=2, ())
    {
        let device = mgr.get_data_device(&seat, &qh, ());
        let source = mgr.create_data_source(&qh, ());
        source.offer("image/png".to_string());
        device.set_selection(Some(&source));
        ActiveControl::Wlr { device, source }
    } else {
        eprintln!("infynitae-shot: clipboard unavailable (no data-control global)");
        std::process::exit(0);
    };

    if queue.roundtrip(&mut state).is_err() || state.done {
        std::process::exit(0);
    }

    // Must run single-threaded after all prior Wayland/Cairo work before spawning child.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        eprintln!("infynitae-shot: failed to fork clipboard daemon");
        std::process::exit(0);
    }
    if pid > 0 {
        // Exit without dropping `conn` or flushing stdio buffers twice.
        unsafe { libc::_exit(0) };
    }

    unsafe {
        libc::setsid();
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    if let Ok(devnull) = OpenOptions::new().read(true).write(true).open("/dev/null") {
        let fd = devnull.as_raw_fd();
        unsafe {
            libc::dup2(fd, 0);
            libc::dup2(fd, 1);
            libc::dup2(fd, 2);
        }
    }

    while !state.done {
        if queue.blocking_dispatch(&mut state).is_err() {
            break;
        }
    }
    match control {
        ActiveControl::Ext { device, source } => {
            source.destroy();
            device.destroy();
        }
        ActiveControl::Wlr { device, source } => {
            source.destroy();
            device.destroy();
        }
    }
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::fd::FromRawFd;

    /// A screenshot larger than a pipe's buffer reaches a reader that handed over a
    /// non-blocking pipe whole (it used to stop at 64 KiB).
    #[test]
    fn whole_picture_through_a_nonblocking_pipe() {
        let mut fds = [0; 2];
        // SAFETY: pipe2 fills two descriptors we then own
        assert_eq!(
            unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) },
            0
        );
        let (mut reader, mut writer) =
            unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) };
        let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let expected = data.clone();
        let slow_reader = std::thread::spawn(move || {
            let mut got = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => return got,
                    Ok(n) => got.extend_from_slice(&buf[..n]),
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(2))
                    }
                    Err(e) => panic!("{e}"),
                }
            }
        });
        write_all_ignoring_pipe(&mut writer, &data).unwrap();
        drop(writer);
        assert_eq!(slow_reader.join().unwrap(), expected);
    }
}
