//! Session lock, `ext-session-lock-v1` (2026-10-24). While the session is locked, each output
//! shows only its lock surface (over black), and every key goes to the lock surface: the backend
//! hands keys to [`WaylandState::lock_key`] before Core, touches to [`WaylandState::lock_touch`]
//! (a finger reaches the lock surface of the screen it touched: the lock screen's own on-screen
//! keyboard, for tablets), and pointer input to nothing, so no binding, window or other client
//! sees input. Core is not told about the lock.
//!
//! Core keeps its own record of held keys. A key Core holds when the lock starts (Super of
//! Super+L) is released while Core does not listen: its release only updates the seat's XKB
//! state (the lock surface never saw it pressed), and after the unlock the backend passes Core
//! those releases ([`WaylandState::take_unlock_releases`]) while no surface has the keyboard,
//! then gives the keyboard back ([`WaylandState::finish_unlock`]). A key pressed while locked
//! and released after the unlock only updates XKB too, so no modifier sticks and no client is
//! told of a release it never saw pressed.
//!
//! Only the lock holder unlocks. Smithay 0.7 calls `unlock` for an `unlock_and_destroy` on any
//! lock object, a refused one too (it posts `invalid_unlock` and goes on), so `unlock` only
//! notes the request; after the dispatch, [`WaylandState::settle_unlock`] ends the lock if the
//! holder's lock object is gone while its client is still connected (a client that destroyed
//! its own confirmed lock). A lock surface is taken only from the holder's client.
//!
//! A lock outlives its client (the protocol's rule): when the lock client dies, the screen stays
//! black and locked, and a new lock client may take the lock over. A session started locked
//! ([`WaylandState::start_locked`]) begins in that state: locked, with no lock client yet. Only backends that draw it
//! offer the global ([`WaylandState::enable_session_lock`]).
//!
//! Core's focus changes while locked move no keyboard focus, and no popup may take a grab: the
//! keyboard returns to Core's focus at [`WaylandState::finish_unlock`]. A pointer button Core
//! holds and that is released while locked gets its release after the unlock (the backend).

use std::collections::HashSet;

use smithay::backend::input::KeyState;
use smithay::delegate_session_lock;
use smithay::desktop::PopupUngrabStrategy;
use smithay::input::keyboard::{FilterResult, Keycode};
use smithay::reexports::wayland_protocols::ext::session_lock::v1::server::ext_session_lock_v1::ExtSessionLockV1;
use smithay::reexports::wayland_server::Resource;
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::protocol::wl_output::WlOutput;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::SERIAL_COUNTER;
use smithay::wayland::session_lock::{
    LockSurface, SessionLockHandler, SessionLockManagerState, SessionLocker,
};

use infynitae_core::PhysicalEvent;
use infynitae_core::output::OutputId;

use crate::WaylandState;

/// Where a touchscreen event goes ([`WaylandState::lock_touch_route`]).
pub(crate) enum LockTouchRoute {
    /// Not the lock's: Core's.
    Core,
    /// The lock takes it and nobody gets it (locked, a finger on no lock surface).
    Dropped,
    /// A finger lands on this lock surface.
    Down(WlSurface),
    /// A finger on a lock surface moves, lifts, or the frame or cancel that ends it.
    Lock,
}

/// Evdev codes are below this (KEY_MAX + 1).
const KEY_COUNT: u32 = 0x300;

pub(crate) struct SessionLock {
    manager: SessionLockManagerState,
    locked: bool,
    /// The lock object of the client holding the lock: another lock is refused while it lives.
    holder: Option<ExtSessionLockV1>,
    /// The holder's client.
    holder_client: Option<ClientId>,
    /// `unlock` was called during the last dispatch: [`WaylandState::settle_unlock`] decides.
    unlock_requested: bool,
    /// A lock request confirmed once a locked frame has been drawn.
    pending: Option<SessionLocker>,
    surfaces: Vec<(OutputId, LockSurface)>,
    /// Keys whose press went to the lock surface.
    lock_keys: HashSet<u32>,
    /// Keys physically down (every key passes `lock_key`).
    phys_down: HashSet<u32>,
    /// Fingers that landed on a lock surface: they stay with it until they lift.
    fingers: Vec<u32>,
    /// Unlocked: Core's releases and the keyboard's return are still to come.
    just_unlocked: bool,
    /// When the compositor last started the locker because the holder died (M4).
    relaunched: Option<std::time::Instant>,
    /// How many times it did (for the tests).
    relaunches: u32,
}

/// How long a locker the compositor started has to take the lock before it is started again.
const RELAUNCH_AFTER: std::time::Duration = std::time::Duration::from_secs(5);

impl WaylandState {
    /// Offers `ext_session_lock_manager_v1`. Only for a backend that draws locked frames and
    /// routes its keys through [`WaylandState::lock_key`].
    pub fn enable_session_lock(&mut self) {
        if self.session_lock.is_none() {
            self.session_lock = Some(SessionLock {
                manager: SessionLockManagerState::new::<Self, _>(
                    &self.dh,
                    self.trust.gate(crate::trust::Capability::Lock).filter(),
                ),
                locked: false,
                holder: None,
                holder_client: None,
                unlock_requested: false,
                pending: None,
                surfaces: Vec::new(),
                lock_keys: HashSet::new(),
                phys_down: HashSet::new(),
                fingers: Vec::new(),
                just_unlocked: false,
                relaunched: None,
                relaunches: 0,
            });
        }
    }

    /// Starts the session locked, before any client connects (`--locked`): black and locked as
    /// after a lock client's death, until a lock client takes the lock and unlocks it. No window,
    /// binding or command runs unlocked between the start and the lock screen's arrival.
    pub fn start_locked(&mut self) {
        self.enable_session_lock();
        if let Some(l) = self.session_lock.as_mut() {
            l.locked = true;
        }
        let kb = self.keyboard_handle();
        kb.set_focus(self, None, SERIAL_COUNTER.next_serial());
    }

    /// The session is locked: only lock surfaces are drawn and given input.
    pub fn is_locked(&self) -> bool {
        self.session_lock.as_ref().is_some_and(|l| l.locked)
    }

    /// A physical key, before Core. Returns `true` when the lock took it (Core must not see it).
    pub fn lock_key(&mut self, key: u32, pressed: bool, time_us: u64) -> bool {
        let Some(l) = self.session_lock.as_mut() else {
            return false;
        };
        if pressed {
            l.phys_down.insert(key);
        } else {
            l.phys_down.remove(&key);
        }
        let code = Keycode::new(key + 8);
        let state = if pressed {
            KeyState::Pressed
        } else {
            KeyState::Released
        };
        if l.locked {
            if pressed {
                l.lock_keys.insert(key);
            } else if !l.lock_keys.remove(&key) {
                // down since before the lock: the lock surface never saw it pressed
                let kb = self.keyboard_handle();
                kb.input_intercept(self, code, state, |_, _, _| None::<()>);
                return true;
            }
            let target = l
                .surfaces
                .iter()
                .find(|(_, s)| s.alive())
                .map(|(_, s)| s.wl_surface().clone());
            let kb = self.keyboard_handle();
            if kb.current_focus() != target {
                kb.set_focus(self, target, SERIAL_COUNTER.next_serial());
            }
            // a grab (an input method's, taken during the lock) would get the key instead: the
            // password goes to the lock surface only (`input_method`)
            if kb.is_grabbed() {
                self.keyboard_unset_grab();
            }
            kb.input::<(), _>(
                self,
                code,
                state,
                SERIAL_COUNTER.next_serial(),
                (time_us / 1000) as u32,
                |_, _, _| FilterResult::Forward,
            );
            return true;
        }
        if !pressed && l.lock_keys.remove(&key) {
            // pressed for the lock surface: XKB saw the press, so it must see the release
            let kb = self.keyboard_handle();
            kb.input_intercept(self, code, state, |_, _, _| None::<()>);
            return true;
        }
        false
    }

    /// Where a touchscreen event goes while the lock may hold it (`PhysicalEvent::Touch*`, in
    /// the touched screen's logical px); [`WaylandState::lock_touch`] (`pointer`) delivers it.
    /// While locked a finger goes to the lock surface of the screen it touched and nothing else
    /// sees it; a finger that landed there stays with it until it lifts, past an unlock.
    pub(crate) fn lock_touch_route(&mut self, ev: &PhysicalEvent) -> LockTouchRoute {
        let Some(l) = self.session_lock.as_mut() else {
            return LockTouchRoute::Core;
        };
        let locked = l.locked;
        let unheld = if locked {
            LockTouchRoute::Dropped
        } else {
            LockTouchRoute::Core
        };
        match *ev {
            PhysicalEvent::TouchDown { slot, output, .. } => {
                if !locked {
                    return LockTouchRoute::Core;
                }
                let Some(surface) = l
                    .surfaces
                    .iter()
                    .find(|(o, s)| *o == output && s.alive())
                    .map(|(_, s)| s.wl_surface().clone())
                else {
                    return LockTouchRoute::Dropped;
                };
                l.fingers.retain(|s| *s != slot);
                l.fingers.push(slot);
                LockTouchRoute::Down(surface)
            }
            PhysicalEvent::TouchMotion { slot, .. } => {
                if l.fingers.contains(&slot) {
                    LockTouchRoute::Lock
                } else {
                    unheld
                }
            }
            PhysicalEvent::TouchUp { slot, .. } => {
                if l.fingers.contains(&slot) {
                    l.fingers.retain(|s| *s != slot);
                    LockTouchRoute::Lock
                } else {
                    unheld
                }
            }
            PhysicalEvent::TouchFrame => {
                if locked || !l.fingers.is_empty() {
                    LockTouchRoute::Lock
                } else {
                    LockTouchRoute::Core
                }
            }
            PhysicalEvent::TouchCancel => {
                if !std::mem::take(&mut l.fingers).is_empty() {
                    LockTouchRoute::Lock
                } else {
                    unheld
                }
            }
            _ => LockTouchRoute::Core,
        }
    }

    /// After an unlock, once: the keys Core holds that are no longer down. The backend passes
    /// Core their releases (no surface has the keyboard yet), then calls `finish_unlock`.
    pub fn take_unlock_releases(&mut self) -> Option<Vec<u32>> {
        let l = self.session_lock.as_mut()?;
        if !std::mem::take(&mut l.just_unlocked) {
            return None;
        }
        let phys_down = l.phys_down.clone();
        Some(
            (0..KEY_COUNT)
                .filter(|k| self.core.is_key_down(*k) && !phys_down.contains(k))
                .collect(),
        )
    }

    /// The end of an unlock: the keyboard returns to the window Core has focused.
    pub fn finish_unlock(&mut self) {
        if self.is_locked() {
            return;
        }
        // focus changes while locked were held back: a layer surface holding the keyboard (L5)
        // has it before the focused window
        let target = match self.core.focus().layer() {
            Some(layer) => self.layer_surface(layer),
            None => self
                .core
                .focus()
                .target()
                .and_then(|id| self.window_surface(id)),
        };
        let kb = self.keyboard_handle();
        kb.set_focus(self, target, SERIAL_COUNTER.next_serial());
    }

    /// The backend drew a locked frame on every output: a waiting lock is confirmed now, when no
    /// window is on screen any more.
    pub fn lock_frame_drawn(&mut self) {
        if let Some(locker) = self.session_lock.as_mut().and_then(|l| l.pending.take()) {
            locker.lock();
        }
    }

    /// After each client dispatch: while locked, a lock holder whose client is gone is replaced
    /// by the compositor itself, which starts the locker (`launch.sh lock-screen`, the
    /// `SCREEN_LOCKER`), and again every `RELAUNCH_AFTER` until one holds the lock (M4, owner
    /// 2026-10-31). The session stays locked meanwhile. Returns whether it started one.
    pub fn restart_dead_locker(&mut self, now: std::time::Instant) -> bool {
        let Some(l) = self.session_lock.as_ref() else {
            return false;
        };
        if !l.locked || l.holder.is_none() {
            return false;
        }
        let mut connected = false;
        if let Some(id) = l.holder_client.clone() {
            self.dh
                .backend_handle()
                .with_all_clients(|c| connected |= c == id);
        }
        if connected
            || l.relaunched
                .is_some_and(|t| now.saturating_duration_since(t) < RELAUNCH_AFTER)
        {
            return false;
        }
        if let Some(l) = self.session_lock.as_mut() {
            l.relaunched = Some(now);
            l.relaunches += 1;
        }
        eprintln!("[infynitae] Session lock: the locker is gone while locked; starting it again");
        self.spawn_action("lock-screen");
        true
    }

    /// How many times the compositor started the locker after the holder died.
    pub fn locker_restarts(&self) -> u32 {
        self.session_lock.as_ref().map_or(0, |l| l.relaunches)
    }

    /// After each client dispatch: an unlock asked for during it ends the lock only if the holder
    /// destroyed its confirmed lock (its lock object is gone, its client still connected). An
    /// `unlock_and_destroy` on a refused lock, or the holder's death, leaves the session locked.
    pub fn settle_unlock(&mut self) {
        let Some(l) = self.session_lock.as_mut() else {
            return;
        };
        if !std::mem::take(&mut l.unlock_requested) || !l.locked {
            return;
        }
        let holder_destroyed = l.holder.as_ref().is_some_and(|h| !h.is_alive());
        let confirmed = l.pending.is_none();
        let mut connected = false;
        if let Some(id) = l.holder_client.clone() {
            self.dh
                .backend_handle()
                .with_all_clients(|c| connected |= c == id);
        }
        if !(holder_destroyed && confirmed && connected) {
            eprintln!("[infynitae] Session lock: unlock refused (not the lock holder's)");
            return;
        }
        let Some(l) = self.session_lock.as_mut() else {
            return;
        };
        l.locked = false;
        l.holder = None;
        l.holder_client = None;
        l.pending = None;
        l.surfaces.clear();
        l.just_unlocked = true;
        // nobody has the keyboard until Core has its releases (`finish_unlock`)
        let kb = self.keyboard_handle();
        kb.set_focus(self, None, SERIAL_COUNTER.next_serial());
    }

    /// The live lock surface of `output`, if its lock client made one.
    pub fn lock_surface(&self, output: OutputId) -> Option<WlSurface> {
        self.session_lock
            .as_ref()?
            .surfaces
            .iter()
            .find_map(|(o, s)| (*o == output && s.alive()).then(|| s.wl_surface().clone()))
    }

    /// A vblank of `output`: its lock surface's frame callbacks complete.
    pub(crate) fn complete_lock_frames(&mut self, output: OutputId, now: std::time::Duration) {
        if !self.is_locked() {
            return;
        }
        let (Some(surface), Some(o)) = (
            self.lock_surface(output),
            self.smithay_output(output).cloned(),
        ) else {
            return;
        };
        smithay::desktop::utils::send_frames_surface_tree(&surface, &o, now, None, |_, _| {
            Some(o.clone())
        });
    }

    /// The lock surface's size for `output`: the output's logical size.
    fn lock_surface_size(&self, output: OutputId) -> Option<(u32, u32)> {
        let o = self.smithay_output(output)?;
        let mode = o.current_mode()?;
        let scale = o.current_scale().fractional_scale().max(0.001);
        Some((
            (f64::from(mode.size.w) / scale).round().max(1.0) as u32,
            (f64::from(mode.size.h) / scale).round().max(1.0) as u32,
        ))
    }
}

impl SessionLockHandler for WaylandState {
    fn lock_state(&mut self) -> &mut SessionLockManagerState {
        &mut self
            .session_lock
            .as_mut()
            .expect("the lock global exists only with the lock state")
            .manager
    }

    fn lock(&mut self, confirmation: SessionLocker) {
        let Some(l) = self.session_lock.as_mut() else {
            return; // dropping the locker tells the client the lock failed
        };
        if l.holder.as_ref().is_some_and(|h| h.is_alive()) {
            return; // a live lock client holds the lock already
        }
        l.relaunched = None;
        l.holder = Some(confirmation.ext_session_lock().clone());
        l.holder_client = confirmation.ext_session_lock().client().map(|c| c.id());
        l.unlock_requested = false;
        l.surfaces.clear();
        if l.locked {
            // taking over from a lock client that died: the session never stopped being locked
            confirmation.lock();
            return;
        }
        l.locked = true;
        l.pending = Some(confirmation);
        // no window keeps the keyboard, and no popup grab may route keys past the lock
        if let Some(mut g) = self.popup_grab.take()
            && !g.has_ended()
        {
            g.ungrab(PopupUngrabStrategy::All);
        }
        self.keyboard_unset_grab();
        // a finger held on a window is given up: nothing reaches a window while locked
        self.cancel_touches();
        let kb = self.keyboard_handle();
        kb.set_focus(self, None, SERIAL_COUNTER.next_serial());
        // Smithay tells a newly focused surface which forwarded keys are down; keys held now
        // are released while locked without being forwarded, so forget them (nothing is sent:
        // no surface has the keyboard)
        for code in kb.pressed_keys() {
            kb.input_forward(
                self,
                code,
                KeyState::Released,
                SERIAL_COUNTER.next_serial(),
                0,
                false,
            );
        }
    }

    fn unlock(&mut self) {
        // whose request this was is not known here: decided after the dispatch (module docs)
        if let Some(l) = self.session_lock.as_mut() {
            l.unlock_requested = true;
        }
    }

    fn new_surface(&mut self, surface: LockSurface, output: WlOutput) {
        let Some(id) = self.output_id_of(&output) else {
            return;
        };
        // a refused lock's surface would replace the holder's (and get the typed password)
        let holder_client = self.session_lock.as_ref().and_then(|l| {
            l.holder
                .as_ref()
                .filter(|h| h.is_alive())
                .and(l.holder_client.clone())
        });
        let from = surface.wl_surface().client().map(|c| c.id());
        if holder_client.is_none() || from != holder_client {
            return;
        }
        if let Some((w, h)) = self.lock_surface_size(id) {
            surface.with_pending_state(|s| s.size = Some((w, h).into()));
        }
        if let Some(l) = self.session_lock.as_mut() {
            l.surfaces.retain(|(o, s)| *o != id && s.alive());
            l.surfaces.push((id, surface));
        }
    }
}

delegate_session_lock!(WaylandState);
