//! Which new windows the user opened (owner, 2026-10-24): with FOCUS_ON_OPEN / TELEPORT_ON_OPEN
//! set to "user", only those take the focus and move the camera; a window an app opens by itself
//! (a popup window, a second app window, a link opened in a running browser) appears quietly.
//!
//! A window is the user's when
//! 1. its process carries the `INFYNITAE_LAUNCH` tag of a launch from a shortcut (Super+Q/W/E,
//!    the launcher, a config `EXEC_` binding) that started or ended (a launcher exits as it
//!    starts the app) less than [`TAGGED`] ago: the tag is inherited, so the app a launcher
//!    starts has it too, even after the launcher is gone;
//! 2. any such launch started or ended less than [`ANY_LAUNCH`] ago: a single-instance app
//!    (an open browser, file manager) shows the new window from its old process, and X11
//!    windows all belong to xwayland-satellite;
//! 3. the user typed or clicked less than [`TYPED`] ago and the window is a child window
//!    (`xdg_toplevel` parent) of the focused one, or its process descends from the focused
//!    window's (a command run in the focused terminal).

use std::process::{Child, Command};
use std::time::{Duration, Instant};

use infynitae_core::stage::ElementId;
use smithay::reexports::wayland_server::Resource;

use crate::WaylandState;

/// A tagged window counts when its launch is this recent.
pub const TAGGED: Duration = Duration::from_secs(15);
/// Any window counts when a launch is this recent.
pub const ANY_LAUNCH: Duration = Duration::from_secs(3);
/// A window of a child of the focused window counts when the user acted this recently.
pub const TYPED: Duration = Duration::from_secs(5);
/// The environment variable a launch's processes carry.
const TAG: &str = "INFYNITAE_LAUNCH";

/// One launch from a shortcut: its process, when it started, when it ended.
struct Launch {
    id: u64,
    child: Child,
    started: Instant,
    ended: Option<Instant>,
}

impl Launch {
    /// The launch's latest moment: now while its process runs, else its end.
    fn moment(&mut self, now: Instant) -> Instant {
        if self.ended.is_none() && !matches!(self.child.try_wait(), Ok(None)) {
            self.ended = Some(now);
        }
        self.ended.unwrap_or(now).max(self.started)
    }
}

#[derive(Default)]
pub(crate) struct Launches {
    next: u64,
    list: Vec<Launch>,
}

impl Launches {
    /// Starts `cmd` as a launch from a shortcut, tagged.
    pub(crate) fn spawn(&mut self, cmd: &mut Command) -> std::io::Result<()> {
        self.next += 1;
        let id = self.next;
        let child = cmd.env(TAG, id.to_string()).spawn()?;
        self.list.push(Launch {
            id,
            child,
            started: Instant::now(),
            ended: None,
        });
        Ok(())
    }

    /// Ended launches older than any rule are forgotten.
    fn prune(&mut self, now: Instant) {
        self.list.retain_mut(|l| now.duration_since(l.moment(now)) < TAGGED);
    }
}

/// The `INFYNITAE_LAUNCH` tag in a process's environment.
fn tag_of(pid: u32) -> Option<u64> {
    tag_in(&std::fs::read(format!("/proc/{pid}/environ")).ok()?)
}

/// The tag in an environment block (`/proc/<pid>/environ`: NUL-separated `KEY=value`). Pure.
fn tag_in(env: &[u8]) -> Option<u64> {
    env.split(|b| *b == 0)
        .find_map(|kv| kv.strip_prefix(format!("{TAG}=").as_bytes()))
        .and_then(|v| std::str::from_utf8(v).ok()?.parse().ok())
}

/// A process's parent.
fn parent_of(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // pid (comm) state ppid …: comm may hold spaces and parentheses
    stat.rsplit_once(')')?.1.split_whitespace().nth(1)?.parse().ok()
}

/// `pid` is `ancestor` or one of its descendants.
fn descends_from(mut pid: u32, ancestor: u32) -> bool {
    for _ in 0..64 {
        if pid == ancestor {
            return true;
        }
        match parent_of(pid) {
            Some(p) if p > 1 => pid = p,
            _ => return false,
        }
    }
    false
}

impl WaylandState {
    /// The process of the client that shows `element`.
    pub(crate) fn client_pid(&self, element: ElementId) -> Option<u32> {
        let surface = self.window_surface(element)?;
        let creds = surface.client()?.get_credentials(&self.dh).ok()?;
        u32::try_from(creds.pid).ok()
    }

    /// Whether the user opened the new window `element` (module docs), given when they last
    /// typed or clicked.
    pub fn opened_by_user(&mut self, element: ElementId, last_input: Instant) -> bool {
        let now = Instant::now();
        self.launches.prune(now);
        let typed = now.duration_since(last_input) < TYPED;
        // a dialog or child window of the focused window, right after the user acted on it
        let focused = self.core.focus().target().filter(|f| *f != element);
        if typed && focused.is_some() && self.core.toplevel_parent(element) == focused {
            return true;
        }
        let Some(pid) = self.client_pid(element) else {
            return false;
        };
        let tag = tag_of(pid);
        for l in &mut self.launches.list {
            let age = now.duration_since(l.moment(now));
            if age < ANY_LAUNCH || (tag == Some(l.id) && age < TAGGED) {
                return true;
            }
        }
        typed
            && focused
                .and_then(|f| self.client_pid(f))
                .is_some_and(|focused| focused != pid && descends_from(pid, focused))
    }

    /// `pid` belongs to a launch from a shortcut (rule 1): a launcher started from Super+W, or
    /// what it started.
    pub(crate) fn is_launch_process(&mut self, pid: u32) -> bool {
        let now = Instant::now();
        let Some(tag) = tag_of(pid) else {
            return false;
        };
        self.launches
            .list
            .iter_mut()
            .any(|l| l.id == tag && now.duration_since(l.moment(now)) < TAGGED)
    }

    /// FOCUS_ON_OPEN / TELEPORT_ON_OPEN = "user": only windows the user opened.
    pub fn open_policy_user_only(&self) -> (bool, bool) {
        (self.focus_open_user_only, self.teleport_open_user_only)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tag is found in an environment block, and descent through the parent chain. No
    /// process is started: this binary's ownership tests end every child of the process.
    #[test]
    fn the_tag_is_read_and_descent_is_followed() {
        assert_eq!(tag_in(b"A=1\0INFYNITAE_LAUNCH=7\0B=2\0"), Some(7));
        assert_eq!(tag_in(b"INFYNITAE_LAUNCHER=7\0"), None, "a longer name is another variable");
        assert_eq!(tag_in(b"INFYNITAE_LAUNCH=x\0"), None);
        let me = std::process::id();
        let parent = parent_of(me).expect("the test has a parent");
        assert!(descends_from(me, parent));
        assert!(descends_from(me, me));
        assert!(!descends_from(parent, me));
        assert_eq!(tag_of(me), None, "this process was not launched");
    }
}
