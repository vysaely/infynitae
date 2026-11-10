//! The edge's part of ~/.config/infynitae/config, read exactly as the C edge reads it
//! (live/edge/infynitae-edge.c, load_position_config and panel_line), so both print the same
//! panels (`--print-panels`, tools/test-edge-panel-lines) and either can replace the other.

use std::path::PathBuf;

/// A module of a panel, by its word in the config. The C edge's modules first, in its order; the
/// ones after `Session` are the new design's (the C edge skips their words).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Module {
    Search,
    Favorites,
    Apps,
    Windows,
    Clipboard,
    Clock,
    Volume,
    Media,
    Wifi,
    Bluetooth,
    Dnd,
    Night,
    Keyboard,
    Osk,
    Power,
    Replay,
    Tray,
    Notifications,
    Session,
    Calendar,
    Memory,
    System,
}

/// Every word a module answers to; the first one of a module is the one written back.
const WORDS: &[(&str, Module)] = &[
    ("search", Module::Search),
    ("favorites", Module::Favorites),
    ("favourites", Module::Favorites),
    ("apps", Module::Apps),
    ("windows", Module::Windows),
    ("clipboard", Module::Clipboard),
    ("clock", Module::Clock),
    ("volume", Module::Volume),
    ("wifi", Module::Wifi),
    ("bluetooth", Module::Bluetooth),
    ("dnd", Module::Dnd),
    ("power", Module::Power),
    ("replay", Module::Replay),
    ("tray", Module::Tray),
    ("media", Module::Media),
    ("nightlight", Module::Night),
    ("night", Module::Night),
    ("keyboard", Module::Keyboard),
    ("osk", Module::Osk),
    ("notifications", Module::Notifications),
    ("session", Module::Session),
    ("calendar", Module::Calendar),
    ("memory", Module::Memory),
    ("system", Module::System),
];

impl Module {
    pub fn of_word(w: &str) -> Option<Module> {
        WORDS
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(w))
            .map(|&(_, m)| m)
    }

    pub fn word(self) -> &'static str {
        WORDS
            .iter()
            .find(|&&(_, m)| m == self)
            .map(|&(k, _)| k)
            .unwrap_or("")
    }

    pub fn is_switch(self) -> bool {
        matches!(
            self,
            Module::Wifi
                | Module::Bluetooth
                | Module::Dnd
                | Module::Night
                | Module::Keyboard
                | Module::Osk
                | Module::Power
                | Module::Replay
        )
    }

    /// Its looks, smallest first, and the one it has when none is written.
    pub fn looks(self) -> (&'static [&'static str], usize) {
        if self.is_switch() {
            (&["small", "tile", "wide"], 1)
        } else if self == Module::Search {
            (&["hidden", "bar"], 1)
        } else if self == Module::Clock {
            (&["time", "big", "calendar"], 0)
        } else {
            (&[], 0)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

impl Side {
    fn word(self) -> &'static str {
        match self {
            Side::Top => "top",
            Side::Bottom => "bottom",
            Side::Left => "left",
            Side::Right => "right",
        }
    }
    pub fn horizontal(self) -> bool {
        matches!(self, Side::Top | Side::Bottom)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub module: Module,
    /// index into `module.looks()`, None: its default
    pub look: Option<usize>,
    /// placed by hand: x, y, w, h inside the panel
    pub rect: Option<(i32, i32, i32, i32)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Panel {
    pub name: String,
    pub side: Side,
    /// along the edge: -1 the start, 0 the middle, 1 the end
    pub align: i8,
    pub width: i32,
    pub height: i32,
    pub hover: bool,
    /// its SUPER+letter, lowercase
    pub key: Option<char>,
    pub items: Vec<Item>,
    pub placed: bool,
    /// the desktop's widgets (`WIDGETS`): the whole screen at 0,0, under the windows, always shown
    /// (docs/EDGE_RUST.md stage 5)
    pub desktop: bool,
}

impl Panel {
    fn new(
        name: &str,
        side: Side,
        align: i8,
        key: Option<char>,
        hover: bool,
        mods: &[Module],
    ) -> Panel {
        let mut p = Panel {
            name: name.to_string(),
            side,
            align,
            width: 0,
            height: 0,
            hover,
            key,
            items: Vec::new(),
            placed: false,
            desktop: false,
        };
        p.set_items(
            mods.iter()
                .map(|&m| Item {
                    module: m,
                    look: None,
                    rect: None,
                })
                .collect(),
        );
        p
    }

    /// each module once, in order, at most 16 (as the C edge)
    fn set_items(&mut self, items: Vec<Item>) {
        self.items.clear();
        for it in items {
            if self.items.len() < 16 && !self.items.iter().any(|x| x.module == it.module) {
                self.items.push(it);
            }
        }
    }

    pub fn has(&self, m: Module) -> bool {
        self.items.iter().any(|i| i.module == m)
    }

    /// The desktop's widgets from a WIDGETS line: `module[:look]@x,y,wxh ...` in screen pixels.
    /// A widget without its place is left out (Add in edit mode gives one).
    pub fn widgets(value: &str) -> Option<Panel> {
        let mut p = Panel::new("widgets", Side::Top, -1, None, false, &[]);
        p.apply(value);
        p.items.retain(|i| i.rect.is_some());
        p.desktop = true;
        p.placed = true;
        (!p.items.is_empty()).then_some(p)
    }

    /// The desktop with no widgets (to add some to).
    pub fn no_widgets() -> Panel {
        let mut p = Panel::new("widgets", Side::Top, -1, None, false, &[]);
        p.desktop = true;
        p.placed = true;
        p
    }

    /// The panel as its PANEL_ line value (the C edge's panel_value); the desktop's widgets as
    /// their WIDGETS value (the widgets and their places only).
    pub fn value(&self) -> String {
        if self.desktop {
            return self
                .items
                .iter()
                .filter_map(|it| {
                    let (x, y, w, h) = it.rect.filter(|r| r.2 > 0 && r.3 > 0)?;
                    let (names, def) = it.module.looks();
                    let word = match it.look {
                        Some(l) if !names.is_empty() && l != def => {
                            format!("{}:{}", it.module.word(), names[l])
                        }
                        _ => it.module.word().to_string(),
                    };
                    Some(format!("{word}@{x},{y},{w}x{h}"))
                })
                .collect::<Vec<_>>()
                .join(" ");
        }
        let along = if self.align == 0 {
            "center"
        } else if self.side.horizontal() {
            if self.align < 0 {
                "left"
            } else {
                "right"
            }
        } else if self.align < 0 {
            "top"
        } else {
            "bottom"
        };
        let mut out = match self.key {
            Some(k) => format!("SUPER+{}", k.to_ascii_uppercase()),
            None => "none".to_string(),
        };
        out += &format!(" {} {}", self.side.word(), along);
        if self.placed && self.width > 0 && self.height > 0 {
            out += &format!(" {}x{}", self.width, self.height);
        } else if self.width > 0 {
            out += &format!(" {}", self.width);
        }
        out += if self.hover { " hover" } else { " nohover" };
        for it in &self.items {
            let (names, def) = it.module.looks();
            match it.look {
                Some(l) if !names.is_empty() && l != def => {
                    out += &format!(" {}:{}", it.module.word(), names[l])
                }
                _ => out += &format!(" {}", it.module.word()),
            }
            if let (true, Some((x, y, w, h))) =
                (self.placed, it.rect.filter(|r| r.2 > 0 && r.3 > 0))
            {
                out += &format!("@{x},{y},{w}x{h}");
            }
        }
        out
    }

    /// Applies a PANEL_ line's value: what it does not say stays (the C edge's panel_line).
    fn apply(&mut self, value: &str) {
        // words are apart by spaces or commas, but a place's commas (after @) belong to it
        let mut buf = String::with_capacity(value.len());
        let mut in_place = false;
        for c in value.chars() {
            match c {
                '@' => {
                    in_place = true;
                    buf.push(c)
                }
                ' ' | '\t' => {
                    in_place = false;
                    buf.push(' ')
                }
                ',' if !in_place => buf.push(' '),
                _ => buf.push(c),
            }
        }
        let mut items: Vec<Item> = Vec::new();
        let mut place: Vec<String> = Vec::new();
        for raw in buf.split_whitespace() {
            let (w_look, at) = match raw.split_once('@') {
                Some((a, b)) => (a, Some(b)),
                None => (raw, None),
            };
            let (w, look) = match w_look.split_once(':') {
                Some((a, b)) => (a, Some(b)),
                None => (w_look, None),
            };
            if w.is_empty() {
                continue;
            }
            if let Some(m) = Module::of_word(w) {
                if items.len() < 16 {
                    let (names, _) = m.looks();
                    let look =
                        look.and_then(|l| names.iter().position(|n| n.eq_ignore_ascii_case(l)));
                    let rect = at
                        .and_then(parse_rect)
                        .filter(|&(_, _, w, h)| w >= 24 && h >= 20);
                    items.push(Item {
                        module: m,
                        look,
                        rect,
                    });
                }
                continue;
            }
            if let Some((pw, ph)) = parse_size(w) {
                if (160..=4000).contains(&pw) {
                    self.width = pw;
                }
                self.height = if (60..=4000).contains(&ph) { ph } else { 0 };
            } else if w.contains('+') {
                let last = w.rsplit('+').next().unwrap_or("");
                let mut cs = last.chars();
                self.key = match (cs.next(), cs.next()) {
                    (Some(c), None)
                        if c.is_ascii_alphabetic() && w.to_ascii_lowercase().contains("super") =>
                    {
                        Some(c.to_ascii_lowercase())
                    }
                    _ => None,
                };
            } else if w.eq_ignore_ascii_case("none") {
                self.key = None;
            } else if w.eq_ignore_ascii_case("hover") {
                self.hover = true;
            } else if w.eq_ignore_ascii_case("nohover") {
                self.hover = false;
            } else if w.starts_with(|c: char| c.is_ascii_digit()) {
                let px = leading_int(w);
                if (160..=2000).contains(&px) {
                    self.width = px;
                }
            } else if place.len() < 4
                && [
                    "top", "bottom", "left", "right", "center", "centre", "middle",
                ]
                .iter()
                .any(|k| k.eq_ignore_ascii_case(w))
            {
                place.push(w.to_ascii_lowercase());
            }
        }
        // the first edge word is the edge, the next where along it
        let mut side_set = false;
        for w in &place {
            if matches!(w.as_str(), "center" | "centre" | "middle") {
                self.align = 0;
                continue;
            }
            let s = match w.as_str() {
                "top" => Side::Top,
                "bottom" => Side::Bottom,
                "left" => Side::Left,
                _ => Side::Right,
            };
            if !side_set {
                self.side = s;
                self.align = 0;
                side_set = true;
                continue;
            }
            if self.side.horizontal() && matches!(s, Side::Left | Side::Right) {
                self.align = if s == Side::Left { -1 } else { 1 };
            }
            if !self.side.horizontal() && matches!(s, Side::Top | Side::Bottom) {
                self.align = if s == Side::Top { -1 } else { 1 };
            }
        }
        if !items.is_empty() {
            self.set_items(items);
            self.placed = self.items.iter().any(|i| i.rect.is_some());
            if !self.placed {
                for i in &mut self.items {
                    i.rect = None;
                }
            }
        }
    }
}

fn leading_int(s: &str) -> i32 {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().unwrap_or(0)
}

/// "361x708" (the C edge's sscanf "%d%c%d" with an x)
fn parse_size(w: &str) -> Option<(i32, i32)> {
    let (a, b) = w.split_once(['x', 'X'])?;
    if a.is_empty() || !a.chars().all(|c| c.is_ascii_digit() || c == '-') {
        return None;
    }
    let b = leading_int_signed(b)?;
    Some((a.parse().ok()?, b))
}

fn leading_int_signed(s: &str) -> Option<i32> {
    let t: String = s
        .chars()
        .enumerate()
        .take_while(|&(i, c)| c.is_ascii_digit() || (i == 0 && c == '-'))
        .map(|(_, c)| c)
        .collect();
    t.parse().ok()
}

/// "8,8,344x34"
fn parse_rect(s: &str) -> Option<(i32, i32, i32, i32)> {
    let mut parts = s.splitn(3, ',');
    let x = parts.next()?.parse().ok()?;
    let y = parts.next()?.parse().ok()?;
    let (w, h) = parse_size(parts.next()?)?;
    Some((x, y, w, h))
}

/// A config value without its quotes and what follows them (the C edge's config_text).
pub fn value_text(v: &str) -> String {
    let v = v.trim_end_matches(['\n', '\r']);
    if let Some(rest) = v.strip_prefix('"') {
        rest.split('"').next().unwrap_or("").to_string()
    } else {
        v.split_whitespace().next().unwrap_or("").to_string()
    }
}

/// What the edge reads from the config.
#[derive(Clone, Debug)]
pub struct Config {
    pub panels: Vec<Panel>,
    /// WIDGETS: the desktop's widgets, None without the line or with no placed widget
    pub widgets: Option<Panel>,
    pub favorites: String,
    pub snap: bool,
    pub overlap: bool,
    pub surface: [u8; 3],
    pub text: [u8; 3],
    pub accent: [u8; 3],
    pub hair: Option<[u8; 3]>,
    /// CORNERS (SQUARE_CORNERS="true" before it: 0)
    pub corners: f64,
    /// THEME_SCHEME
    pub scheme: String,
    /// CARD_OPACITY, 0..1
    pub opacity: f64,
    /// SWITCH_LOOK
    pub switch_look: String,
}

pub fn config_path() -> PathBuf {
    // the C edge reads $HOME/.config, not XDG_CONFIG_HOME; the same here
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config/infynitae/config")
}

pub fn parse_hex(v: &str) -> Option<[u8; 3]> {
    let t = value_text(v);
    let h = t.strip_prefix('#')?;
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let n = u32::from_str_radix(h, 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

fn truthy(v: &str) -> bool {
    let t = value_text(v).to_ascii_lowercase();
    matches!(t.as_str(), "true" | "1" | "yes" | "on")
}

impl Config {
    pub fn load() -> Config {
        Config::from_text(&std::fs::read_to_string(config_path()).unwrap_or_default())
    }

    pub fn from_text(text: &str) -> Config {
        let mut side = Side::Top;
        let mut ctrl_align: i8 = 1;
        let mut c = Config {
            panels: Vec::new(),
            widgets: None,
            favorites: String::new(),
            snap: true,
            overlap: false,
            surface: [0x11, 0x13, 0x17],
            text: [0xee, 0xf2, 0xf8],
            accent: [0x33, 0x3b, 0x54],
            hair: None,
            corners: 8.0,
            scheme: "yours".into(),
            opacity: 0.90, // 90 % until CARD_BLUR exists: 70 % without blur reads as see-through
            switch_look: "rows".into(),
        };
        let mut ctrl = vec![
            Module::Clock,
            Module::Volume,
            Module::Media,
            Module::Wifi,
            Module::Bluetooth,
            Module::Dnd,
            Module::Night,
            Module::Keyboard,
            Module::Osk,
            Module::Power,
            Module::Replay,
            Module::Tray,
            Module::Notifications,
            Module::Session,
        ];
        let mut lines: Vec<(String, String)> = Vec::new();
        let mut corners_set = false;
        for raw in text.lines() {
            let p = raw.trim_start_matches([' ', '\t']);
            let Some((key, v)) = p.split_once('=') else {
                continue;
            };
            match key {
                "EDGE_POSITION" | "ISLAND_EDGE" => {
                    match value_text(v).to_ascii_lowercase().as_str() {
                        "top" => side = Side::Top,
                        "bottom" => side = Side::Bottom,
                        _ => {}
                    }
                }
                "EDGE_CONTROL_SIDE" | "ISLAND_CONTROL_SIDE" => {
                    match value_text(v).to_ascii_lowercase().as_str() {
                        "left" => ctrl_align = -1,
                        "right" => ctrl_align = 1,
                        _ => {}
                    }
                }
                "FAVORITES" => c.favorites = value_text(v),
                "PANELS_SNAP" | "PANELS_OVERLAP" => {
                    let t = value_text(v).to_ascii_lowercase();
                    let on = !matches!(t.as_str(), "false" | "off" | "no" | "0");
                    if key == "PANELS_SNAP" {
                        c.snap = on
                    } else {
                        c.overlap = on
                    }
                }
                "SURFACE_COLOR" => c.surface = parse_hex(v).unwrap_or(c.surface),
                "BORDER_INACTIVE" => c.hair = parse_hex(v).or(c.hair),
                "BORDER_ACTIVE" => c.accent = parse_hex(v).unwrap_or(c.accent),
                "TEXT_COLOR" => c.text = parse_hex(v).unwrap_or(c.text),
                "SQUARE_CORNERS" if !corners_set => {
                    if truthy(v) {
                        c.corners = 0.0
                    }
                }
                "CORNERS" => {
                    if let Ok(n) = value_text(v).parse::<f64>() {
                        c.corners = n.clamp(0.0, 28.0);
                        corners_set = true;
                    }
                }
                "THEME_SCHEME" => c.scheme = value_text(v).to_ascii_lowercase(),
                "CARD_OPACITY" => {
                    if let Ok(n) = value_text(v).trim_end_matches('%').parse::<f64>() {
                        c.opacity = (n / 100.0).clamp(0.2, 1.0);
                    }
                }
                "SWITCH_LOOK" => c.switch_look = value_text(v).to_ascii_lowercase(),
                "CONTROL_MODULES" => {
                    // the control panel's modules before PANEL_CONTROL; "apps" there was the tray
                    let t = value_text(v);
                    let mods: Vec<Module> = t
                        .split([' ', ',', '\t'])
                        .filter(|w| !w.is_empty())
                        .filter_map(|w| {
                            if w.eq_ignore_ascii_case("apps") {
                                Some(Module::Tray)
                            } else {
                                Module::of_word(w)
                            }
                        })
                        .collect();
                    if !mods.is_empty() {
                        ctrl = mods;
                    }
                }
                "WIDGETS" => c.widgets = Panel::widgets(&value_text(v)),
                _ if key.starts_with("PANEL_") && lines.len() < 24 => {
                    lines.push((key["PANEL_".len()..].to_string(), v.to_string()));
                }
                _ => {}
            }
        }
        use Module::*;
        c.panels.push(Panel::new(
            "apps",
            side,
            0,
            Some('w'),
            false,
            &[Search, Favorites, Apps],
        ));
        c.panels.push(Panel::new(
            "control",
            side,
            ctrl_align,
            Some('a'),
            true,
            &ctrl,
        ));
        c.panels.push(Panel::new(
            "windows",
            side,
            0,
            Some('l'),
            true,
            &[Search, Windows],
        ));
        c.panels.push(Panel::new(
            "clipboard",
            side,
            0,
            Some('v'),
            false,
            &[Search, Clipboard],
        ));
        for (raw_name, value) in lines {
            if raw_name.is_empty()
                || !raw_name
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            {
                continue;
            }
            let name: String = raw_name
                .chars()
                .take(23)
                .collect::<String>()
                .to_ascii_lowercase();
            let idx = match c.panels.iter().position(|p| p.name == name) {
                Some(i) => i,
                None if c.panels.len() < 12 => {
                    c.panels.push(Panel::new(&name, side, 0, None, false, &[]));
                    c.panels.len() - 1
                }
                None => continue,
            };
            c.panels[idx].apply(&value_text(&value));
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_panels_follow_the_edge_and_the_control_side() {
        let c = Config::from_text("EDGE_POSITION=\"bottom\"\nEDGE_CONTROL_SIDE=\"right\"\n");
        let lines: Vec<String> = c
            .panels
            .iter()
            .map(|p| format!("{} {}", p.name, p.value()))
            .collect();
        assert_eq!(
            lines[0],
            "apps SUPER+W bottom center nohover search favorites apps"
        );
        assert_eq!(
            lines[1],
            "control SUPER+A bottom right hover clock volume media wifi bluetooth dnd nightlight keyboard osk power replay tray notifications session"
        );
        assert_eq!(
            lines[2],
            "windows SUPER+L bottom center hover search windows"
        );
        assert_eq!(
            lines[3],
            "clipboard SUPER+V bottom center nohover search clipboard"
        );
    }

    #[test]
    fn a_placed_panel_keeps_its_size_and_rectangles() {
        let line = "PANEL_CONTROL=\"SUPER+A bottom right 361x708 hover clock@8,8,344x34 volume@8,48,344x32 wifi@8,86,169x46\"\n";
        let c = Config::from_text(&format!("EDGE_POSITION=\"bottom\"\n{line}"));
        assert_eq!(
            c.panels[1].value(),
            "SUPER+A bottom right 361x708 hover clock@8,8,344x34 volume@8,48,344x32 wifi@8,86,169x46"
        );
    }

    #[test]
    fn a_line_changes_only_what_it_says_and_new_names_make_panels() {
        let c = Config::from_text(
            "PANEL_CONTROL=\"left\"\nPANEL_MEDIA=\"SUPER+M right center 320 volume tray\"\n",
        );
        assert_eq!(c.panels[1].side, Side::Left);
        assert_eq!(c.panels[1].key, Some('a'));
        assert!(c.panels[1].has(Module::Session));
        assert_eq!(
            c.panels[4].value(),
            "SUPER+M right center 320 nohover volume tray"
        );
    }

    #[test]
    fn looks_commas_and_unknown_words() {
        let c =
            Config::from_text("PANEL_X=\"none top left clock:big,wifi:small search:bar bogus\"\n");
        assert_eq!(
            c.panels[4].value(),
            "none top left nohover clock:big wifi:small search"
        );
    }

    #[test]
    fn theme_settings_with_their_older_names() {
        let c = Config::from_text(
            "SQUARE_CORNERS=\"true\"\nBORDER_ACTIVE=\"#cc70a9\"\nCARD_OPACITY=\"55\"\n",
        );
        assert_eq!(c.corners, 0.0);
        assert_eq!(c.accent, [0xcc, 0x70, 0xa9]);
        assert!((c.opacity - 0.55).abs() < 1e-9);
        let c = Config::from_text("CORNERS=\"12\"\nSQUARE_CORNERS=\"true\"\n");
        assert_eq!(c.corners, 12.0);
    }
}
