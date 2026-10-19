//! SIGTERM, SIGINT and SIGCHLD delivered to the event loop through a self-pipe.
//!
//! Owner decision 2026-10-19: the orderly-shutdown and process-ownership design is approved,
//! `signal-hook` is rejected; the self-pipe is re-implemented on `libc`, which is already in the
//! locked graph (no new crate). The behaviour is the design's:
//! - a signal only writes one byte to the stream a [`Registration`] holds; the loop watches the
//!   other end, so nothing but an async-signal-safe `send` happens in the handler;
//! - no signal is blocked, so the session's children inherit no blocked mask, and `exec` resets
//!   a handled signal to its default action in them;
//! - once every registration of a signal has ended, the handler stays installed and does
//!   nothing: a later signal is ignored rather than taking its default action in the middle of
//!   the teardown (`repeated_signals_shut_down_once`).
//!
//! Several sessions may be registered at once (tests run sessions on parallel threads); each
//! signal has [`SLOTS`] registration slots. A registration's write end is never closed: a handler
//! that read a descriptor just before its registration ended can then never write into a
//! descriptor number the process has since reused; `MSG_NOSIGNAL | MSG_DONTWAIT` keep the write
//! from raising SIGPIPE or blocking once the loop's end is gone. The `unsafe` sites are listed in
//! UNSAFE_INVENTORY (U-005, U-006).

use std::io;
use std::os::fd::{AsRawFd, IntoRawFd};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

pub(crate) const SIGTERM: i32 = libc::SIGTERM;
pub(crate) const SIGINT: i32 = libc::SIGINT;
pub(crate) const SIGCHLD: i32 = libc::SIGCHLD;

/// Registrations per signal that may be live at once.
const SLOTS: usize = 16;

const fn empty() -> [AtomicI32; SLOTS] {
    [const { AtomicI32::new(-1) }; SLOTS]
}

/// The write ends to notify, per signal (SIGTERM, SIGINT, SIGCHLD); −1 is a free slot.
static WRITE_ENDS: [[AtomicI32; SLOTS]; 3] = [empty(), empty(), empty()];
/// Whether the handler is installed for the signal.
static INSTALLED: [AtomicBool; 3] = [const { AtomicBool::new(false) }; 3];

fn index(signal: i32) -> Option<usize> {
    match signal {
        libc::SIGTERM => Some(0),
        libc::SIGINT => Some(1),
        libc::SIGCHLD => Some(2),
        _ => None,
    }
}

/// The signal handler: one byte to every registered write end, nothing else. It touches only
/// atomics and `send`, both async-signal-safe, and preserves `errno` for the interrupted code.
#[allow(unsafe_code)]
extern "C" fn on_signal(signal: libc::c_int) {
    let Some(i) = index(signal) else { return };
    // SAFETY: `__errno_location` returns this thread's errno; `send` is async-signal-safe and is
    // given a one-byte buffer that lives for the call. A descriptor is never closed after it was
    // registered (module doc), so `fd` is an open socket of this process or −1 (skipped).
    unsafe {
        let errno = *libc::__errno_location();
        for slot in &WRITE_ENDS[i] {
            let fd = slot.load(Ordering::SeqCst);
            if fd >= 0 {
                let byte = 0u8;
                libc::send(
                    fd,
                    (&byte as *const u8).cast(),
                    1,
                    libc::MSG_NOSIGNAL | libc::MSG_DONTWAIT,
                );
            }
        }
        *libc::__errno_location() = errno;
    }
}

/// Installs [`on_signal`] for `signal` once per process (SA_RESTART: interrupted system calls are
/// restarted, as with the handler this replaces).
#[allow(unsafe_code)]
fn install(signal: i32, i: usize) -> io::Result<()> {
    if INSTALLED[i].load(Ordering::SeqCst) {
        return Ok(());
    }
    // SAFETY: a zeroed `sigaction` is a valid starting value (no flags, empty mask); the handler
    // is an `extern "C" fn(c_int)` that does only async-signal-safe work; `sigemptyset` and
    // `sigaction` receive pointers to locals that live for the calls.
    let rc = unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
        action.sa_flags = libc::SA_RESTART;
        libc::sigemptyset(&mut action.sa_mask);
        libc::sigaction(signal, &action, std::ptr::null_mut())
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    INSTALLED[i].store(true, Ordering::SeqCst);
    Ok(())
}

/// One stream notified on a signal; dropping it ends the notification.
pub(crate) struct Registration {
    signal: usize,
    slot: usize,
}

impl Drop for Registration {
    fn drop(&mut self) {
        WRITE_ENDS[self.signal][self.slot].store(-1, Ordering::SeqCst);
    }
}

/// From now on `signal` writes one byte to `notify` (the write end of a stream whose other end
/// the event loop watches), until the returned registration is dropped.
pub(crate) fn register(signal: i32, notify: UnixStream) -> io::Result<Registration> {
    let i = index(signal).ok_or_else(|| io::Error::other(format!("signal {signal} not served")))?;
    notify.set_nonblocking(true)?;
    let fd = notify.as_raw_fd();
    let slot = WRITE_ENDS[i]
        .iter()
        .position(|s| {
            s.compare_exchange(-1, fd, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        })
        .ok_or_else(|| {
            io::Error::other(format!(
                "more than {SLOTS} registrations of signal {signal}"
            ))
        })?;
    if let Err(e) = install(signal, i) {
        WRITE_ENDS[i][slot].store(-1, Ordering::SeqCst);
        return Err(e);
    }
    // never closed (module doc): the descriptor stays this process's for its lifetime
    let _ = notify.into_raw_fd();
    Ok(Registration { signal: i, slot })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn a_signal_writes_one_byte_to_every_registered_stream_and_none_after_the_registration_ends() {
        let (mut a, a_notify) = UnixStream::pair().unwrap();
        let (mut b, b_notify) = UnixStream::pair().unwrap();
        a.set_nonblocking(true).unwrap();
        b.set_nonblocking(true).unwrap();
        let ra = register(SIGCHLD, a_notify).unwrap();
        let rb = register(SIGCHLD, b_notify).unwrap();
        on_signal(SIGCHLD);
        let mut buf = [0u8; 8];
        assert_eq!(a.read(&mut buf).unwrap(), 1);
        assert_eq!(b.read(&mut buf).unwrap(), 1);
        drop(ra);
        on_signal(SIGCHLD);
        assert!(
            a.read(&mut buf).is_err(),
            "an ended registration is not notified"
        );
        assert_eq!(
            b.read(&mut buf).unwrap(),
            1,
            "the other registration still is"
        );
        drop(rb);
    }
}
