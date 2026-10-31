//! A loop extension: work the composition root plugs into a backend's event loop that the adapter
//! itself knows nothing about (the trusted shell server). The adapter and the backends see
//! only this trait and a wake-up; what runs inside, and with which authority, is the composition
//! root's (C7.4: the adapter never holds or names the shell's authority).

use std::io;

use infynitae_core::Core;
use infynitae_core::protocol::Effect;
use smithay::reexports::calloop::LoopHandle;
use smithay::reexports::calloop::ping::{Ping, PingSource, make_ping};

/// Runs once after every loop dispatch.
pub trait LoopExtension {
    /// Does what arrived since the last call. Returns Core's effects, for the adapter to apply,
    /// and whether what is shown may have changed (the loop then asks for a frame).
    fn service(&mut self, core: &mut Core) -> (Vec<Effect>, bool);

    /// The session's lock state, which the adapter holds; a backend gives it before `service`.
    fn set_locked(&mut self, _locked: bool) {}
}

/// Wakes the loop from another thread (`Send`, cloneable). A wake-up with nothing behind it costs
/// one loop iteration; none happens unless something calls it (INV-IDLE-1).
#[derive(Clone)]
pub struct Waker(Ping);

impl Waker {
    pub fn wake(&self) {
        self.0.ping();
    }
}

/// The loop side of a [`Waker`], given to the backend with its extension.
pub struct WakeSource(PingSource);

/// A waker and the source a backend registers.
pub fn waker() -> io::Result<(Waker, WakeSource)> {
    let (ping, source) = make_ping()?;
    Ok((Waker(ping), WakeSource(source)))
}

/// An extension and its wake-up source, as a backend's run function takes them.
pub struct Extension {
    pub inner: Box<dyn LoopExtension>,
    pub wake: WakeSource,
}

/// Makes the extension. A backend calls it once its session handles SIGTERM and SIGINT, so a
/// signal during start-up ends the loop and whatever the extension created (a socket) goes with
/// it; an error leaves the compositor running without the extension.
pub type ExtensionFactory = Box<dyn FnOnce() -> io::Result<Extension>>;

/// Calls the factory and registers the extension's wake-up source with the loop.
pub fn start<D: 'static>(
    factory: Option<ExtensionFactory>,
    handle: &LoopHandle<'_, D>,
) -> Option<Box<dyn LoopExtension>> {
    let ext = match factory?() {
        Ok(ext) => ext,
        Err(e) => {
            eprintln!("[infynitae] loop extension not started: {e}");
            return None;
        }
    };
    if let Err(e) = ext.wake.register(handle) {
        eprintln!("[infynitae] loop extension not started: {e}");
        return None;
    }
    Some(ext.inner)
}

impl WakeSource {
    /// Registers the source: a wake-up only ends the loop's wait; the extension runs after the
    /// dispatch like after any other.
    pub fn register<D: 'static>(self, handle: &LoopHandle<'_, D>) -> io::Result<()> {
        handle
            .insert_source(self.0, |_, _, _| {})
            .map(|_| ())
            .map_err(|e| io::Error::other(e.to_string()))
    }
}
