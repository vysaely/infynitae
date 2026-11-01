//! The session's processes (session process ownership, 2026-10-16): everything the compositor
//! started, and everything those started, is ended with the session — and nothing else.
//!
//! The boundary is this process's own subtree. The compositor is a child subreaper
//! (`PR_SET_CHILD_SUBREAPER`), so a descendant whose parent exits — a double-forked "daemon", a
//! process that called `setsid` — is re-parented to the compositor, not to init or to the
//! desktop's service manager, and stays inside the boundary. A process is a member only while
//! the kernel lists it as a child of the compositor or of another member
//! (`/proc/<pid>/task/<tid>/children`); each one is pinned by a pidfd opened while that parent
//! link held, and signals go through the pidfd, so a recycled pid can never be signalled. No
//! process group, session id, name or user is used to choose a target: a process of the
//! desktop or of another application is never a member.
//!
//! Contract for code in this process: a child spawned by anything other than the session must be
//! registered (`register_spawned`, and `forget_spawned` once reaped) by its spawner, or the orphan
//! reaper may take its exit status (`ECHILD` for the owner's `wait`). The session's spawner and the
//! satellite supervisor do this; so must a test that hosts a session and spawns clients itself.
//!
//! Outside the boundary by construction: processes started for a client by another service
//! (D-Bus activation, the user's service manager). The sessions this compositor starts run
//! with D-Bus disabled.

use std::collections::{BTreeMap, BTreeSet};
use std::os::fd::OwnedFd;
use std::process::Child;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use smithay::reexports::rustix::event::{PollFd, PollFlags, poll};
use smithay::reexports::rustix::process::{
    Pid, PidfdFlags, Signal, WaitId, WaitIdOptions, getpid, pidfd_open, pidfd_send_signal,
    set_child_subreaper, waitid,
};

/// Children this process spawned whose owner holds a `Child` and reaps it. Every other child is
/// an adopted orphan, reaped here.
static OWNED: Mutex<BTreeSet<i32>> = Mutex::new(BTreeSet::new());

/// Recent reaps by [`reap_adopted`]: which pid, when. A child reaped here that someone still
/// holds a `Child` for (a launch, `launch.rs`) gets `ECHILD` from `try_wait`; this says when it
/// exited.
static REAPED: Mutex<Vec<(i32, Instant)>> = Mutex::new(Vec::new());
/// How long a reap is remembered.
const REAPED_KEPT: Duration = Duration::from_secs(60);

/// When [`reap_adopted`] reaped `pid`, if it did in the last minute.
pub(crate) fn reaped_at(pid: u32) -> Option<Instant> {
    REAPED
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|(p, _)| *p == pid as i32)
        .map(|(_, t)| *t)
}

/// A child was spawned by an owner that reaps it (`Session::spawn`, the satellite supervisor).
/// Prefer [`spawn_owned`]: registering after `spawn` leaves a window in which a child that exits
/// at once can be reaped by a reaper on another thread.
pub fn register_spawned(pid: u32) {
    OWNED.lock().unwrap().insert(pid as i32);
}

/// Spawns `command` and registers the child as owned atomically with respect to
/// [`reap_adopted`] (both hold the registry lock), so the orphan reaper never takes the exit
/// status of a child its owner will wait for.
pub fn spawn_owned(command: &mut std::process::Command) -> std::io::Result<Child> {
    let mut owned = OWNED.lock().unwrap();
    let child = command.spawn()?;
    owned.insert(child.id() as i32);
    Ok(child)
}

/// Its owner reaped it.
pub fn forget_spawned(pid: u32) {
    OWNED.lock().unwrap().remove(&(pid as i32));
}

/// Makes this process the reaper of its orphaned descendants.
pub fn become_subreaper() -> std::io::Result<()> {
    set_child_subreaper(Some(getpid()))?;
    Ok(())
}

/// The direct children of `pid`, over all its threads (a re-parented orphan is the child of
/// one of the reaper's threads).
pub(crate) fn children_of(pid: i32) -> Vec<i32> {
    let mut out = Vec::new();
    let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) else {
        return out;
    };
    for task in tasks.flatten() {
        if let Ok(list) = std::fs::read_to_string(task.path().join("children")) {
            out.extend(
                list.split_whitespace()
                    .filter_map(|p| p.parse::<i32>().ok()),
            );
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// `(state, ppid)` of `pid` from `/proc/<pid>/stat` (the fields after the command name).
fn stat_of(pid: i32) -> Option<(char, i32)> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let mut rest = stat.get(stat.rfind(')')? + 2..)?.split(' ');
    let state = rest.next()?.chars().next()?;
    let ppid = rest.next()?.parse().ok()?;
    Some((state, ppid))
}

/// A member of the session's subtree, pinned.
struct Member {
    pid: i32,
    fd: OwnedFd,
}

/// The session's members: the given `roots` (the session's own children) and every child of
/// this process that no owner registered (orphans it adopted), with all their descendants. A
/// child another owner registered (the satellite, a test's own client) is not a member, nor is
/// anything below it. Each member is pinned by a pidfd opened while it was still the child of its
/// listed parent (a pid recycled in between has another parent and is skipped).
fn members(roots: &[i32]) -> Vec<Member> {
    let me = getpid().as_raw_nonzero().get();
    // Held while the direct children are listed and filtered: a `spawn_owned` on another thread
    // is then entirely before this scan (its child registered) or after it (its child not yet
    // listed); copying the registry first would let a child spawned in between pass as an
    // adopted orphan.
    let owned = OWNED.lock().unwrap();
    let mut out = Vec::new();
    let mut parents = Vec::new();
    for pid in children_of(me) {
        if !(roots.contains(&pid) || !owned.contains(&pid)) {
            continue;
        }
        let Some(p) = Pid::from_raw(pid) else {
            continue;
        };
        let Ok(fd) = pidfd_open(p, PidfdFlags::empty()) else {
            continue;
        };
        if stat_of(pid).is_some_and(|(_, ppid)| ppid == me) {
            out.push(Member { pid, fd });
            parents.push(pid);
        }
    }
    drop(owned);
    while let Some(parent) = parents.pop() {
        for pid in children_of(parent) {
            let Some(p) = Pid::from_raw(pid) else {
                continue;
            };
            let Ok(fd) = pidfd_open(p, PidfdFlags::empty()) else {
                continue;
            };
            if stat_of(pid).is_some_and(|(_, ppid)| ppid == parent) {
                out.push(Member { pid, fd });
                parents.push(pid);
            }
        }
    }
    out
}

/// Reaps exited children of this process that no owner holds (orphans it adopted). Returns how
/// many were reaped.
pub fn reap_adopted() -> usize {
    let me = getpid().as_raw_nonzero().get();
    // held for the whole scan: a `spawn_owned` in progress completes its registration first
    let owned = OWNED.lock().unwrap();
    let mut reaped = 0;
    for pid in children_of(me) {
        if owned.contains(&pid) || !stat_of(pid).is_some_and(|(s, _)| s == 'Z') {
            continue;
        }
        if let Some(p) = Pid::from_raw(pid)
            && waitid(
                WaitId::Pid(p),
                WaitIdOptions::EXITED | WaitIdOptions::NOHANG,
            )
            .is_ok()
        {
            reaped += 1;
            let now = Instant::now();
            let mut log = REAPED.lock().unwrap();
            log.retain(|(_, t)| now.duration_since(*t) < REAPED_KEPT);
            log.push((pid, now));
        }
    }
    reaped
}

/// What ending the subtree took.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Teardown {
    /// Members that got SIGTERM (direct children and every descendant).
    pub terminated: usize,
    /// Members still alive after the grace period, given SIGKILL.
    pub killed: usize,
    /// Members left alive after the final bounded wait (should be 0; a process stuck in an
    /// uninterruptible sleep can outlive SIGKILL for a while).
    pub survivors: usize,
}

/// Ends the session's processes (`members`): SIGTERM to every member (members appearing during the
/// grace period get it too), up to `grace` for all of them to exit, then SIGKILL to those left,
/// then a bounded wait (`grace` again at most). `tick` runs on every pass of the waits, so the
/// owner can keep answering its clients while they quit. `children` (the owner's `Child`
/// handles) are reaped through their handles, adopted orphans here. Nothing outside the subtree
/// is touched.
pub fn end_session_tree(
    children: &mut Vec<Child>,
    grace: Duration,
    tick: &mut dyn FnMut(),
) -> Teardown {
    let mut report = Teardown::default();
    let mut signalled: BTreeMap<i32, OwnedFd> = BTreeMap::new();
    let reap = |children: &mut Vec<Child>| {
        children.retain_mut(|child| match child.try_wait() {
            Ok(None) => true,
            _ => {
                forget_spawned(child.id());
                false
            }
        });
        reap_adopted();
    };
    let alive = |members: &[Member]| {
        members
            .iter()
            .filter(|m| stat_of(m.pid).is_some_and(|(s, _)| s != 'Z'))
            .count()
    };
    // A process signalled once is followed through its pidfd: it counts as alive until the kernel
    // reports its exit. A scan of `/proc/*/children` alone can miss a process while the kernel
    // re-parents it (its parent just exited; the list is not guaranteed complete), and the grace
    // period then ended early — seen as "5 killed" 1 ms after SIGTERM.
    let signalled_alive =
        |signalled: &BTreeMap<i32, OwnedFd>| signalled.values().filter(|fd| !exited(fd)).count();
    let roots = |children: &[Child]| children.iter().map(|c| c.id() as i32).collect::<Vec<_>>();
    let start = Instant::now();
    loop {
        reap(children);
        let members = members(&roots(children));
        for m in members {
            if let std::collections::btree_map::Entry::Vacant(slot) = signalled.entry(m.pid) {
                let _ = pidfd_send_signal(&m.fd, Signal::TERM);
                report.terminated += 1;
                slot.insert(m.fd);
            }
        }
        if (alive(&self::members(&roots(children))) == 0 && signalled_alive(&signalled) == 0)
            || start.elapsed() >= grace
        {
            break;
        }
        tick();
        std::thread::sleep(Duration::from_millis(5));
    }
    // every live member found now, and every signalled one that has not exited (by its pidfd)
    for m in members(&roots(children)) {
        signalled.entry(m.pid).or_insert(m.fd);
    }
    for (pid, fd) in &signalled {
        if !exited(fd) {
            eprintln!(
                "[infynitae] Session process {pid} did not exit within the grace period, sending SIGKILL"
            );
            let _ = pidfd_send_signal(fd, Signal::KILL);
            report.killed += 1;
        }
    }
    let end = Instant::now() + grace;
    loop {
        reap(children);
        let live = alive(&members(&roots(children))).max(signalled_alive(&signalled));
        if (live == 0 && children.is_empty()) || Instant::now() >= end {
            report.survivors = live;
            break;
        }
        tick();
        std::thread::sleep(Duration::from_millis(5));
    }
    report
}

/// Whether the process behind `fd` has exited (its pidfd is readable once it has; a zombie counts
/// as exited). Immune to pid reuse.
fn exited(fd: &OwnedFd) -> bool {
    let mut fds = [PollFd::new(fd, PollFlags::IN)];
    matches!(poll(&mut fds, Some(&Default::default())), Ok(n) if n > 0)
}

/// Super+Delete (owner; Super+Shift+C from 2026-10-21), first step: pins a client's process by a pidfd, while
/// its connection still stands (so a pid reused after it died can never be signalled). Only a
/// process running as this user and other than this one is pinned (a client in this process — a
/// test's probe — is only disconnected).
pub fn pin_client_process(pid: i32, uid: u32) -> Option<std::os::fd::OwnedFd> {
    if pid <= 0 || pid as u32 == std::process::id() || uid != getuid_raw() {
        return None;
    }
    pidfd_open(Pid::from_raw(pid)?, PidfdFlags::empty()).ok()
}

/// The last step: SIGKILL to the pinned process. Returns whether it was sent.
pub fn kill_pinned(fd: &std::os::fd::OwnedFd) -> bool {
    pidfd_send_signal(fd, Signal::KILL).is_ok()
}

fn getuid_raw() -> u32 {
    smithay::reexports::rustix::process::getuid().as_raw()
}

/// Whether `pid` runs xwayland-satellite (its executable's name): the one client all X11 windows
/// share, which a force-close of one window must not end.
pub fn is_xwayland_satellite(pid: i32) -> bool {
    std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .and_then(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().starts_with("xwayland-satellite"))
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    /// Super+Delete's kill: a process of this user is pinned and killed; this process and a
    /// process of another uid are never pinned (a client in this process is only disconnected).
    #[test]
    fn a_client_process_is_pinned_then_killed_and_this_process_never() {
        use std::os::unix::process::ExitStatusExt;
        assert!(pin_client_process(std::process::id() as i32, getuid_raw()).is_none());
        assert!(pin_client_process(0, getuid_raw()).is_none());
        // owned by this test: a session shut down by another test in this process leaves it alone
        let mut child = spawn_owned(Command::new("sleep").arg("30")).unwrap();
        let pid = child.id() as i32;
        assert!(
            pin_client_process(pid, getuid_raw().wrapping_add(1)).is_none(),
            "another uid"
        );
        let fd = pin_client_process(pid, getuid_raw()).expect("pinned");
        assert!(kill_pinned(&fd));
        let status = child.wait().unwrap();
        forget_spawned(pid as u32);
        assert_eq!(status.signal(), Some(9), "{status:?}");
    }

    use super::*;
    use std::process::Command;

    /// `/proc/<pid>/stat` parsing survives a command name with spaces and parentheses. (The
    /// subtree itself is tested through the real binary, `crates/infynitae/tests/shutdown.rs`:
    /// in a test binary every test's children would be members of the same subtree.)
    #[test]
    fn stat_parsing_survives_odd_command_names() {
        // the kernel's command name is the executable's file name
        let dir = std::env::temp_dir().join(format!("infynitae-stat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let odd = dir.join("x) (y ) z");
        std::os::unix::fs::symlink("/bin/sleep", &odd).unwrap();
        // owned by this test: a session shut down by another test in this process leaves it alone
        let mut child = spawn_owned(Command::new(&odd).arg("5")).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        let comm = std::fs::read_to_string(format!("/proc/{}/comm", child.id())).unwrap();
        assert_eq!(comm.trim(), "x) (y ) z");
        let (state, ppid) = stat_of(child.id() as i32).unwrap();
        assert_eq!(ppid, getpid().as_raw_nonzero().get());
        assert!(state != 'Z', "{state}");
        child.kill().unwrap();
        child.wait().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
