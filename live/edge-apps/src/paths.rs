//! Home, runtime, and state paths from environment variables, with one rule for unset or empty
//! values so callers never fall back to paths relative to the current working directory.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// `$HOME`, or `None` when `HOME` is unset or empty.
///
/// Unset and empty `HOME` are treated identically so callers never join a relative path onto an
/// empty `PathBuf` (which would resolve inside the current working directory).
pub fn home() -> Option<PathBuf> {
    home_from(std::env::var_os("HOME").as_deref())
}

pub(crate) fn home_from(val: Option<&OsStr>) -> Option<PathBuf> {
    val.filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// Expands a leading `~`, `$HOME`, or `${HOME}` (exact match or followed by `/`) against `$HOME`,
/// returning the trimmed input as a `PathBuf` when `HOME` is unset or empty or no prefix matches.
pub fn expand_home(raw: &str) -> PathBuf {
    expand_home_with(raw, home().as_deref())
}

/// Like [`expand_home`], using `home` instead of reading `$HOME`. An empty `home` path is treated
/// the same as `None`.
pub fn expand_home_with(raw: &str, home: Option<&Path>) -> PathBuf {
    let trimmed = raw.trim();
    let Some(base) = home.filter(|h| !h.as_os_str().is_empty()).map(clean_base) else {
        return PathBuf::from(trimmed);
    };
    for prefix in ["$HOME", "${HOME}", "~"] {
        if trimmed == prefix {
            return base;
        }
        if let Some(rest) = trimmed
            .strip_prefix(prefix)
            .and_then(|s| s.strip_prefix('/'))
        {
            let rest = rest.trim_end_matches('/');
            return if rest.is_empty() {
                base
            } else {
                base.join(rest)
            };
        }
    }
    PathBuf::from(trimmed)
}

fn clean_base(p: &Path) -> PathBuf {
    match p.to_str() {
        Some(s) => {
            let trimmed = s.trim_end_matches('/');
            if trimmed.is_empty() && s.starts_with('/') {
                PathBuf::from("/")
            } else {
                PathBuf::from(trimmed)
            }
        }
        None => p.to_path_buf(),
    }
}

/// `$XDG_RUNTIME_DIR`, or `/tmp` when unset or empty.
pub fn runtime_dir() -> PathBuf {
    runtime_dir_from(std::env::var_os("XDG_RUNTIME_DIR").as_deref())
}

pub(crate) fn runtime_dir_from(val: Option<&OsStr>) -> PathBuf {
    val.filter(|s| !s.is_empty())
        .map_or_else(|| PathBuf::from("/tmp"), PathBuf::from)
}

/// `name` inside [`runtime_dir`].
pub fn runtime_path(name: &str) -> PathBuf {
    runtime_dir().join(name)
}

/// `$XDG_RUNTIME_DIR` when non-empty, otherwise `$HOME/.cache`, or [`runtime_dir`] (`/tmp`) when
/// both `XDG_RUNTIME_DIR` and `HOME` are unset or empty. Matches `${XDG_RUNTIME_DIR:-$HOME/.cache}`
/// in the shell helpers while avoiding a relative `.cache` path when `HOME` is missing.
pub fn runtime_or_cache_dir() -> PathBuf {
    runtime_or_cache_dir_from(
        std::env::var_os("XDG_RUNTIME_DIR").as_deref(),
        home().as_deref(),
    )
}

pub(crate) fn runtime_or_cache_dir_from(
    xdg_runtime_dir: Option<&OsStr>,
    home: Option<&Path>,
) -> PathBuf {
    if let Some(dir) = xdg_runtime_dir.filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    home.filter(|h| !h.as_os_str().is_empty())
        .map_or_else(|| runtime_dir_from(xdg_runtime_dir), |h| h.join(".cache"))
}

/// `$XDG_STATE_HOME` when non-empty, otherwise `$HOME/.local/state`, or `None` when both
/// `XDG_STATE_HOME` and `HOME` are unset or empty.
pub fn state_dir() -> Option<PathBuf> {
    state_dir_from(
        std::env::var_os("XDG_STATE_HOME").as_deref(),
        home().as_deref(),
    )
}

pub(crate) fn state_dir_from(
    xdg_state_home: Option<&OsStr>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(dir) = xdg_state_home.filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    home.filter(|h| !h.as_os_str().is_empty())
        .map(|h| h.join(".local/state"))
}

/// The Infynitae checkout the desktop scripts run from, as they resolve it: the one install.sh
/// recorded in `~/.config/infynitae/checkout` (its first line), else `~/infynitae`.
pub fn checkout_dir(home: &Path) -> PathBuf {
    std::fs::read_to_string(home.join(".config/infynitae/checkout"))
        .ok()
        .and_then(|s| {
            s.lines()
                .next()
                .filter(|l| !l.is_empty())
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| home.join("infynitae"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_and_empty_home_return_none_and_do_not_make_relative_paths() {
        assert_eq!(home_from(None), None);
        assert_eq!(home_from(Some(OsStr::new(""))), None);
        assert_eq!(
            home_from(Some(OsStr::new("/home/alice"))),
            Some(PathBuf::from("/home/alice"))
        );

        // With unset or empty home, expand_home_with leaves prefixes unexpanded instead of
        // stripping them into CWD-relative paths.
        for no_home in [None, Some(Path::new(""))] {
            assert_eq!(
                expand_home_with("~/Pictures/Shots", no_home),
                PathBuf::from("~/Pictures/Shots")
            );
            assert_eq!(
                expand_home_with("$HOME/.config", no_home),
                PathBuf::from("$HOME/.config")
            );
            assert_eq!(
                expand_home_with("${HOME}/Docs", no_home),
                PathBuf::from("${HOME}/Docs")
            );
            assert_eq!(expand_home_with("~", no_home), PathBuf::from("~"));
            assert_eq!(
                state_dir_from(None, no_home),
                None,
                "state_dir must be None when both XDG_STATE_HOME and HOME are unset/empty"
            );
            assert_eq!(
                state_dir_from(Some(OsStr::new("")), no_home),
                None,
                "empty XDG_STATE_HOME falls back to HOME, yielding None when HOME is missing"
            );
            assert_eq!(
                runtime_or_cache_dir_from(None, no_home),
                PathBuf::from("/tmp")
            );
        }
    }

    #[test]
    fn expand_home_expands_tilde_and_home_vars() {
        let home = Some(Path::new("/home/alice/"));
        assert_eq!(expand_home_with("~", home), PathBuf::from("/home/alice"));
        assert_eq!(expand_home_with("~/", home), PathBuf::from("/home/alice"));
        assert_eq!(
            expand_home_with("$HOME", home),
            PathBuf::from("/home/alice")
        );
        assert_eq!(
            expand_home_with("${HOME}/", home),
            PathBuf::from("/home/alice")
        );
        assert_eq!(
            expand_home_with("  ~/Videos/Clips  ", home),
            PathBuf::from("/home/alice/Videos/Clips")
        );
        assert_eq!(
            expand_home_with("$HOME/Pictures/Shots/", home),
            PathBuf::from("/home/alice/Pictures/Shots")
        );
        assert_eq!(
            expand_home_with("${HOME}/Documents", home),
            PathBuf::from("/home/alice/Documents")
        );
        assert_eq!(
            expand_home_with("/mnt/media/Recordings", home),
            PathBuf::from("/mnt/media/Recordings")
        );
        // Must not expand partial prefix like ~other or $HOME2.
        assert_eq!(
            expand_home_with("~bob/file", home),
            PathBuf::from("~bob/file")
        );
        assert_eq!(
            expand_home_with("$HOME2/file", home),
            PathBuf::from("$HOME2/file")
        );
    }

    #[test]
    fn runtime_and_state_dirs_follow_xdg_fallbacks() {
        assert_eq!(runtime_dir_from(None), PathBuf::from("/tmp"));
        assert_eq!(
            runtime_dir_from(Some(OsStr::new(""))),
            PathBuf::from("/tmp")
        );
        assert_eq!(
            runtime_dir_from(Some(OsStr::new("/run/user/1000"))),
            PathBuf::from("/run/user/1000")
        );

        let home = Some(Path::new("/home/alice"));
        assert_eq!(
            state_dir_from(None, home),
            Some(PathBuf::from("/home/alice/.local/state"))
        );
        assert_eq!(
            state_dir_from(Some(OsStr::new("")), home),
            Some(PathBuf::from("/home/alice/.local/state"))
        );
        assert_eq!(
            state_dir_from(Some(OsStr::new("/custom/state")), home),
            Some(PathBuf::from("/custom/state"))
        );

        assert_eq!(
            runtime_or_cache_dir_from(None, home),
            PathBuf::from("/home/alice/.cache")
        );
        assert_eq!(
            runtime_or_cache_dir_from(Some(OsStr::new("/run/user/1000")), home),
            PathBuf::from("/run/user/1000")
        );
    }

    /// The installer's record, else `~/infynitae`.
    #[test]
    fn the_checkout_is_the_recorded_one_else_home_infynitae() {
        let home = std::env::temp_dir().join(format!("edge-checkout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".config/infynitae")).unwrap();
        assert_eq!(checkout_dir(&home), home.join("infynitae"));
        std::fs::write(home.join(".config/infynitae/checkout"), "").unwrap();
        assert_eq!(checkout_dir(&home), home.join("infynitae"));
        std::fs::write(
            home.join(".config/infynitae/checkout"),
            "/src/my infynitae\n",
        )
        .unwrap();
        assert_eq!(checkout_dir(&home), PathBuf::from("/src/my infynitae"));
        std::fs::remove_dir_all(&home).unwrap();
    }
}
