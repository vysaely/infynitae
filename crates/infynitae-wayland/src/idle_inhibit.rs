//! `zwp_idle_inhibit_manager_v1`: a video player, a browser playing video, a game, keeps the
//! screens on and the session unlocked while it shows. An inhibitor counts while its surface is
//! part of a mapped window and the session is not locked: locking lets the screens go dark as
//! always, whatever an app asks.
//!
//! Apps that ask over D-Bus instead (org.freedesktop.ScreenSaver: X11 apps, VLC, Steam) and
//! `systemd-inhibit --what=idle` reach the backend through logind (`logind_idle_inhibited`).

use smithay::reexports::wayland_server::DisplayHandle;
use smithay::reexports::wayland_server::Resource;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::wayland::idle_inhibit::{IdleInhibitHandler, IdleInhibitManagerState};

use crate::WaylandState;

pub struct IdleInhibit {
    #[expect(
        dead_code,
        reason = "owns the zwp_idle_inhibit_manager_v1 global for the state's lifetime"
    )]
    manager: IdleInhibitManagerState,
    /// The surfaces with an inhibitor, as clients made them (dead ones dropped when read).
    surfaces: Vec<WlSurface>,
}

impl IdleInhibit {
    pub fn new(dh: &DisplayHandle) -> Self {
        Self {
            manager: IdleInhibitManagerState::new::<WaylandState>(dh),
            surfaces: Vec::new(),
        }
    }
}

impl IdleInhibitHandler for WaylandState {
    fn inhibit(&mut self, surface: WlSurface) {
        self.idle_inhibit.surfaces.retain(|s| s.is_alive());
        if !self.idle_inhibit.surfaces.contains(&surface) {
            self.idle_inhibit.surfaces.push(surface);
        }
    }

    fn uninhibit(&mut self, surface: WlSurface) {
        self.idle_inhibit.surfaces.retain(|s| *s != surface);
    }
}

impl WaylandState {
    /// Whether an app's inhibitor keeps the screens on now: its surface alive, in a mapped
    /// window, and the session unlocked.
    pub fn app_inhibits_idle(&self) -> bool {
        !self.is_locked()
            && self.idle_inhibit.surfaces.iter().any(|s| {
                s.is_alive()
                    && self
                        .registry
                        .key(s)
                        .is_some_and(|k| self.core.stage().element_containing_surface(k).is_some())
            })
    }
}

smithay::delegate_idle_inhibit!(WaylandState);

/// Whether logind holds an idle inhibitor (`BlockInhibited` names "idle"): an app that asked over
/// D-Bus (org.freedesktop.ScreenSaver, through infynitae-portal), or `systemd-inhibit
/// --what=idle`. Asked through busctl, logind's public interface (its files say not to parse
/// them); the backend asks only when the screens would go off, at most once a minute.
pub fn logind_idle_inhibited() -> bool {
    std::process::Command::new("busctl")
        .args([
            "get-property",
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
            "BlockInhibited",
        ])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .is_some_and(|o| block_inhibited_names_idle(&String::from_utf8_lossy(&o.stdout)))
}

/// busctl's `s "sleep:idle:handle-lid-switch"`: whether "idle" is one of the names. Pure.
fn block_inhibited_names_idle(reply: &str) -> bool {
    reply
        .trim()
        .trim_start_matches('s')
        .trim()
        .trim_matches('"')
        .split(':')
        .any(|w| w == "idle")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_is_found_among_the_inhibited_names() {
        assert!(block_inhibited_names_idle("s \"sleep:idle\"\n"));
        assert!(block_inhibited_names_idle("s \"idle\""));
        assert!(!block_inhibited_names_idle("s \"sleep:handle-lid-switch\""));
        assert!(!block_inhibited_names_idle("s \"\""));
        assert!(
            !block_inhibited_names_idle("s \"idler\""),
            "the whole name only"
        );
    }
}
