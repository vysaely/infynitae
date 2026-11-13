//! Installed apps from their desktop entries (docs/EDGE_APPS.md section 4), as today's edge reads them
//! (`live/edge-rs/src/modules/apps.rs` 84-250), plus the categories and the terminal flag that the
//! providers' claims and `Action::Launch` need.

use std::path::{Path, PathBuf};

/// An installed program, from its desktop entry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct App {
    /// the entry's file name without ".desktop": "steam", "org.qbittorrent.qBittorrent"
    pub id: String,
    pub name: String,
    pub icon: String,
    /// Exec= with the field codes taken out
    pub exec: String,
    /// Categories= split on ';'
    pub categories: Vec<String>,
    pub wm_class: Option<String>,
    /// Terminal=true: started inside the terminal
    pub terminal: bool,
}

/// Takes `%f`, `%u`, `%F`, `%U` and the other field codes out of an Exec line; `%%` is `%`.
pub fn clean_exec(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('%') => out.push('%'),
                Some(_) => {
                    while out.ends_with(' ') {
                        out.pop();
                    }
                }
                None => break,
            }
        } else {
            out.push(c);
        }
    }
    out.trim().to_string()
}

/// One desktop entry's text as an app; None when it is hidden (NoDisplay, Hidden) or has no name or
/// command. `id` is its file name without ".desktop".
pub fn parse_entry(id: &str, text: &str) -> Option<App> {
    let mut in_entry = false;
    let (mut hidden, mut terminal) = (false, false);
    let mut app = App {
        id: id.to_string(),
        ..Default::default()
    };
    let mut exec = String::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim();
        match k.trim() {
            "NoDisplay" | "Hidden" if v == "true" => hidden = true,
            "Name" if app.name.is_empty() => app.name = v.to_string(),
            "Exec" if exec.is_empty() => exec = v.to_string(),
            "Icon" if app.icon.is_empty() => app.icon = v.to_string(),
            "Categories" if app.categories.is_empty() => {
                app.categories = v
                    .split(';')
                    .filter(|c| !c.is_empty())
                    .map(str::to_string)
                    .collect()
            }
            "StartupWMClass" if app.wm_class.is_none() && !v.is_empty() => {
                app.wm_class = Some(v.to_string())
            }
            "Terminal" => terminal = v == "true",
            _ => {}
        }
    }
    app.exec = clean_exec(&exec);
    app.terminal = terminal;
    (!hidden && !app.name.is_empty() && !app.exec.is_empty()).then_some(app)
}

/// The desktop entries in `dirs`, first folder first: a file name seen once (even hidden) hides the
/// same name in later folders. Sorted by name.
pub fn load(dirs: &[PathBuf]) -> Vec<App> {
    let mut apps: Vec<App> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut entries: Vec<_> = rd.filter_map(Result::ok).collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let fname = e.file_name();
            let Some(file) = fname.to_str() else {
                continue;
            };
            let Some(id) = file.strip_suffix(".desktop").filter(|i| !i.is_empty()) else {
                continue;
            };
            if seen.iter().any(|s| s == id) {
                continue;
            }
            seen.push(id.to_string());
            if let Some(app) = std::fs::read_to_string(e.path())
                .ok()
                .and_then(|t| parse_entry(id, &t))
            {
                apps.push(app);
            }
        }
    }
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

/// The folders desktop entries are found in, in the order they are read.
pub fn dirs() -> Vec<PathBuf> {
    let home = std::env::var("HOME").ok().filter(|s| !s.is_empty());
    let mut d = Vec::new();
    match std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|s| !s.is_empty())
    {
        Some(dh) => d.push(PathBuf::from(dh).join("applications")),
        None => {
            if let Some(h) = &home {
                d.push(Path::new(h).join(".local/share/applications"))
            }
        }
    }
    if let Some(h) = &home {
        d.push(Path::new(h).join(".local/share/flatpak/exports/share/applications"));
    }
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    for p in data_dirs.split(':').filter(|p| !p.is_empty()) {
        d.push(Path::new(p).join("applications"));
    }
    d.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_gives_its_name_command_categories_and_terminal_flag() {
        let a = parse_entry(
            "btop",
            "[Desktop Entry]\nName=btop++\nExec=btop %f\nIcon=btop\nCategories=System;Monitor;ConsoleOnly;\nTerminal=true\n[Desktop Action x]\nName=Other\n",
        )
        .unwrap();
        assert_eq!(a.name, "btop++");
        assert_eq!(a.exec, "btop");
        assert_eq!(a.categories, vec!["System", "Monitor", "ConsoleOnly"]);
        assert!(a.terminal);
        assert!(parse_entry("x", "[Desktop Entry]\nName=X\nExec=x\nNoDisplay=true\n").is_none());
        assert!(parse_entry("x", "[Desktop Entry]\nName=X\n").is_none());
    }

    #[test]
    fn the_first_folder_hides_the_same_entry_later() {
        let t = std::env::temp_dir().join(format!("edge-apps-test-{}", std::process::id()));
        let (a, b) = (t.join("a"), t.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(
            a.join("steam.desktop"),
            "[Desktop Entry]\nName=Steam\nExec=steam\nNoDisplay=true\n",
        )
        .unwrap();
        std::fs::write(
            b.join("steam.desktop"),
            "[Desktop Entry]\nName=Steam\nExec=steam\n",
        )
        .unwrap();
        std::fs::write(
            b.join("mpv.desktop"),
            "[Desktop Entry]\nName=mpv\nExec=mpv %U\nCategories=AudioVideo;Player;\n",
        )
        .unwrap();
        let apps = load(&[a, b]);
        let _ = std::fs::remove_dir_all(&t);
        assert_eq!(
            apps.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
            vec!["mpv"]
        );
    }
}
