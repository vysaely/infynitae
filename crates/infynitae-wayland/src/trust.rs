//! Who may use the privileged protocols (owner decision 2026-10-31, `docs/SECURITY.md`).
//!
//! A privileged global (screen capture, clipboard management, virtual input, input method, the
//! window list, the session lock, display control, layer-shell) is advertised to a client only
//! when the policy allows that client the global's capability. A client that is not allowed never
//! sees the global, and a bind to it fails like a bind to a global that does not exist.
//!
//! - A **sandboxed** client (connected through a security-context-v1 listener, as Flatpak does)
//!   is allowed a capability only when its sandbox app id is on that capability's list.
//! - A **native** client is known by its real executable: the device and inode of the file
//!   `/proc/<pid>/exe` points to, never its file name. It is allowed when the list has the word
//!   `native`, or names its program (looked up in `PATH`, `~/.local/bin`, `/usr/lib`,
//!   `/usr/libexec`) or its absolute path.
//! - Infynitae's own helpers are always allowed what they need.
//! - The compositor's own connections (an internal socket pair, the tests) are trusted.
//!
//! A native app already runs with the user's rights, so for native apps the lists guard against
//! accidents more than against attacks; the boundary that matters is the sandbox.

use std::collections::HashSet;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use smithay::reexports::wayland_server::Client;

use crate::ClientState;

/// A group of privileged globals that is allowed or denied as one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Capability {
    /// wlr-screencopy, ext-image-copy-capture (screens and windows).
    Capture,
    /// wlr- and ext-data-control: reading and setting the clipboard without focus.
    Clipboard,
    /// Virtual keyboard and virtual pointer.
    Input,
    /// zwp-input-method (it may grab the keyboard).
    InputMethod,
    /// wlr-foreign-toplevel (list, focus, close windows) and ext-foreign-toplevel-list.
    Windows,
    /// ext-session-lock.
    Lock,
    /// Gamma, output power and output management.
    Display,
    /// wlr-layer-shell: panels, overlays and backgrounds above or below every window.
    Layers,
}

impl Capability {
    pub const ALL: [Capability; 8] = [
        Capability::Capture,
        Capability::Clipboard,
        Capability::Input,
        Capability::InputMethod,
        Capability::Windows,
        Capability::Lock,
        Capability::Display,
        Capability::Layers,
    ];

    /// The config key of its list.
    pub fn key(self) -> &'static str {
        match self {
            Capability::Capture => "ALLOW_CAPTURE",
            Capability::Clipboard => "ALLOW_CLIPBOARD",
            Capability::Input => "ALLOW_INPUT",
            Capability::InputMethod => "ALLOW_INPUT_METHOD",
            Capability::Windows => "ALLOW_WINDOWS",
            Capability::Lock => "ALLOW_LOCK",
            Capability::Display => "ALLOW_DISPLAY",
            Capability::Layers => "ALLOW_LAYERS",
        }
    }

    pub fn from_key(key: &str) -> Option<Capability> {
        Capability::ALL.into_iter().find(|c| c.key() == key)
    }

    /// The list when the config has none (the same as `config/config`): every native app, plus
    /// the usual tools by name so that removing `native` keeps them working. The lock is only
    /// Infynitae's own (M4).
    pub fn default_list(self) -> &'static str {
        match self {
            Capability::Capture => "native grim wf-recorder obs xdg-desktop-portal-wlr",
            Capability::Clipboard => "native wl-copy wl-paste cliphist wl-clip-persist",
            Capability::Input => "native wtype ydotool",
            Capability::InputMethod => "native fcitx5",
            Capability::Windows => "native",
            Capability::Lock => "",
            Capability::Display => "native wlr-randr kanshi wlsunset gammastep",
            Capability::Layers => "native slurp mako swaync fuzzel",
        }
    }

    /// Infynitae's own helpers that always have this capability, by installed name.
    fn helpers(self) -> &'static [&'static str] {
        match self {
            Capability::Capture => &["infynitae-record-stream"],
            Capability::Clipboard => &["xwayland-satellite"],
            Capability::Input => &["infynitae-portal"],
            Capability::InputMethod => &[],
            Capability::Windows => &["infynitae-edge", "infynitae-island"],
            Capability::Lock => &["infynitae-lock"],
            Capability::Display => &[],
            Capability::Layers => &[
                "infynitae-edge",
                "infynitae-island",
                "infynitae-lock",
                "infynitae-wallpaper",
            ],
        }
    }
}

/// A file, by device and inode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileId {
    pub dev: u64,
    pub ino: u64,
}

impl FileId {
    /// The file a path names, following links.
    pub fn of(path: &Path) -> Option<FileId> {
        let m = std::fs::metadata(path).ok()?;
        m.is_file().then(|| FileId {
            dev: m.dev(),
            ino: m.ino(),
        })
    }
}

/// Who a client is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Identity {
    /// Connected through a security-context listener; the metadata is the sandbox's, which the
    /// app inside cannot change.
    Sandboxed {
        engine: Option<String>,
        app_id: Option<String>,
    },
    /// Connected to the ordinary socket: its executable, when it could be read.
    Native {
        exe: Option<FileId>,
        path: Option<PathBuf>,
    },
}

impl Identity {
    /// A native process, by the executable `/proc/<pid>/exe` points to.
    pub fn of_pid(pid: i32) -> Identity {
        let link = PathBuf::from(format!("/proc/{pid}/exe"));
        Identity::Native {
            exe: FileId::of(&link),
            path: std::fs::read_link(&link).ok(),
        }
    }

    /// A short name for logs and the consent store: the sandbox app id or the executable's path.
    pub fn describe(&self) -> String {
        match self {
            Identity::Sandboxed { engine, app_id } => format!(
                "{} (sandboxed by {})",
                app_id.as_deref().unwrap_or("unknown app"),
                engine.as_deref().unwrap_or("unknown")
            ),
            Identity::Native { path, .. } => path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "unknown program".into()),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Rule {
    native: bool,
    apps: HashSet<String>,
    exes: HashSet<FileId>,
}

/// The lists, resolved to app ids and files.
#[derive(Clone, Debug, PartialEq)]
pub struct Policy {
    rules: Vec<Rule>,
}

impl Policy {
    /// The policy from the config's `ALLOW_*` values (a missing key keeps its default), with the
    /// helpers under `home`.
    pub fn from_lists<'a>(
        home: Option<&Path>,
        lists: impl IntoIterator<Item = (Capability, &'a str)>,
    ) -> Policy {
        let mut given: Vec<Option<&str>> = vec![None; Capability::ALL.len()];
        for (cap, list) in lists {
            given[cap as usize] = Some(list);
        }
        let dirs = search_dirs(home);
        let rules = Capability::ALL
            .into_iter()
            .map(|cap| {
                let mut rule = Rule::default();
                for word in given[cap as usize]
                    .unwrap_or(cap.default_list())
                    .split([' ', ',', '\t'])
                    .filter(|w| !w.is_empty())
                {
                    if word == "native" {
                        rule.native = true;
                    } else if word.starts_with('/') {
                        rule.exes.extend(FileId::of(Path::new(word)));
                    } else if word.contains('/') {
                        // a relative path names nothing (`config-check` says so)
                    } else {
                        rule.apps.insert(word.to_string());
                        rule.exes
                            .extend(dirs.iter().filter_map(|d| FileId::of(&d.join(word))));
                    }
                }
                for name in cap.helpers() {
                    rule.exes.extend(
                        helper_paths(home, name)
                            .iter()
                            .filter_map(|p| FileId::of(p)),
                    );
                }
                rule
            })
            .collect();
        Policy { rules }
    }

    /// Whether `identity` has `cap`.
    pub fn allows(&self, identity: &Identity, cap: Capability) -> bool {
        let rule = &self.rules[cap as usize];
        match identity {
            Identity::Sandboxed { app_id, .. } => {
                app_id.as_ref().is_some_and(|a| rule.apps.contains(a))
            }
            Identity::Native { exe, .. } => {
                rule.native || exe.as_ref().is_some_and(|e| rule.exes.contains(e))
            }
        }
    }
}

impl Default for Policy {
    fn default() -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        Policy::from_lists(home.as_deref(), [])
    }
}

/// Where a program name is looked up.
fn search_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .filter(|d| d.is_absolute())
                .collect()
        })
        .unwrap_or_default();
    if let Some(home) = home {
        dirs.push(home.join(".local/bin"));
    }
    for d in ["/usr/local/bin", "/usr/bin", "/usr/lib", "/usr/libexec"] {
        dirs.push(PathBuf::from(d));
    }
    dirs
}

/// A helper as installed (`~/.local/bin`) and as built in the checkout (`~/infynitae/live`).
fn helper_paths(home: Option<&Path>, name: &str) -> Vec<PathBuf> {
    let Some(home) = home else {
        return Vec::new();
    };
    let dir = match name {
        "infynitae-record-stream" => "record",
        n => n.strip_prefix("infynitae-").unwrap_or(n),
    };
    let mut paths = vec![
        home.join(".local/bin").join(name),
        home.join("infynitae/live").join(dir).join(name),
    ];
    if name == "xwayland-satellite" {
        paths.push(PathBuf::from("/usr/bin/xwayland-satellite"));
    }
    paths
}

/// The policy, shared by every gate; a config reload replaces it for the clients that bind after.
#[derive(Clone, Default)]
pub struct SharedPolicy(Arc<RwLock<Policy>>);

impl SharedPolicy {
    pub fn set(&self, policy: Policy) {
        if let Ok(mut p) = self.0.write() {
            *p = policy;
        }
    }

    pub fn gate(&self, cap: Capability) -> Gate {
        Gate {
            cap,
            policy: self.clone(),
        }
    }

    /// Whether `client` has `cap` (its identity from its `ClientState`).
    pub fn allows(&self, client: &Client, cap: Capability) -> bool {
        let Some(state) = client.get_data::<ClientState>() else {
            return false;
        };
        // the replay recorder Infynitae started captures through its own socket
        if state.is_recorder() && cap == Capability::Capture {
            return true;
        }
        let Some(identity) = state.identity() else {
            // the compositor's own connection
            return true;
        };
        self.0.read().is_ok_and(|p| p.allows(identity, cap))
    }

    /// The globals that only native, unsandboxed clients may see: security-context-v1 itself.
    pub fn unsandboxed(client: &Client) -> bool {
        client
            .get_data::<ClientState>()
            .is_some_and(|s| !matches!(s.identity(), Some(Identity::Sandboxed { .. })))
    }
}

/// A privileged global's data: its capability and the policy that decides it (`can_view`).
#[derive(Clone)]
pub struct Gate {
    cap: Capability,
    policy: SharedPolicy,
}

impl Gate {
    pub fn allows(&self, client: &Client) -> bool {
        self.policy.allows(client, self.cap)
    }

    /// A filter for Smithay's `new::<_, F>` constructors.
    pub fn filter(self) -> impl Fn(&Client) -> bool + Send + Sync + 'static {
        move |c| self.allows(c)
    }
}

impl smithay::wayland::security_context::SecurityContextHandler for crate::WaylandState {
    /// A sandbox made a listener for its app: the session loop serves it, and every client that
    /// connects through it is sandboxed with this context's metadata (`Session::dispatch`).
    fn context_created(
        &mut self,
        source: smithay::wayland::security_context::SecurityContextListenerSource,
        context: smithay::wayland::security_context::SecurityContext,
    ) {
        self.security_listeners.push((source, context));
    }
}
smithay::delegate_security_context!(crate::WaylandState);

/// The identity of a client that connected through a sandbox's listener.
pub fn sandboxed(context: &smithay::wayland::security_context::SecurityContext) -> Identity {
    Identity::Sandboxed {
        engine: context.sandbox_engine.clone(),
        app_id: context.app_id.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me() -> Identity {
        Identity::of_pid(std::process::id() as i32)
    }

    fn sandboxed(app: &str) -> Identity {
        Identity::Sandboxed {
            engine: Some("flatpak".into()),
            app_id: Some(app.into()),
        }
    }

    #[test]
    fn a_sandboxed_app_needs_its_app_id_on_the_list() {
        let p = Policy::from_lists(None, [(Capability::Capture, "native org.obs.Studio")]);
        assert!(p.allows(&sandboxed("org.obs.Studio"), Capability::Capture));
        assert!(!p.allows(&sandboxed("org.other.App"), Capability::Capture));
        // the defaults name no sandboxed app
        for cap in Capability::ALL {
            if cap != Capability::Capture {
                assert!(!p.allows(&sandboxed("org.obs.Studio"), cap), "{cap:?}");
            }
        }
        let anon = Identity::Sandboxed {
            engine: None,
            app_id: None,
        };
        assert!(!p.allows(&anon, Capability::Capture));
    }

    #[test]
    fn a_native_app_is_known_by_its_file_not_its_name() {
        let exe = std::fs::read_link("/proc/self/exe").unwrap();
        let path = exe.to_str().unwrap();
        let p = Policy::from_lists(None, [(Capability::Capture, path)]);
        assert!(p.allows(&me(), Capability::Capture));
        // the same name with another file is someone else
        let other = Identity::Native {
            exe: FileId::of(Path::new("/bin/sh")),
            path: Some(exe.clone()),
        };
        assert!(!p.allows(&other, Capability::Capture));
        // the default lock list has no native app
        assert!(!p.allows(&me(), Capability::Lock));
        // `native` allows every native app
        assert!(p.allows(&me(), Capability::Clipboard));
        let tight = Policy::from_lists(None, [(Capability::Clipboard, "wl-copy")]);
        assert!(!tight.allows(&me(), Capability::Clipboard));
    }
}
