# The edge from apps (2026-11-13)

State (2026-11-13): stage 1 (panels, system cards, launcher, windows, clipboard, notification server,
ask prompt, panel editing and Add a card in `edit.rs`), stage 2 app cards (calendar, monitor, music,
Steam, chat, files, simple), and the Settings cards (`docs/EDGE_SETTINGS.md`, including
`settings/keys`) are built in `live/edge-apps` with VM scripts in `tools/vm-edge/` (`papps.py`,
`papps2.py`, `pask.py`, `pedit.py`, `psettings.py`, `pkeys.py`); desktop widgets (`WIDGETS`) are not
yet built there. `EDGE_IMPL="apps"` picks it; `live/edge-rs` stays the default until it switches
over.

The rest of this document is the plan as written before building. The owner asked for the edge to be reworked from zero: first map the
apps, then every card in a panel comes from an app. You add an app, then pick one of its cards. Known
apps give rich cards; any other app gives a simple card (open it, its windows, its notifications).
Today's modules move to the app they belong to: the switches to Settings, volume to Sound, the clock to
Calendar, notifications to Notifications.

This document is the architecture. The look is in `docs/design/apps-edge/system-cards.html` (three
versions of the system cards) and `docs/design/apps-edge/app-cards.html` (the app cards in version 1).
The rules of `docs/NO_SLOP.md` and `docs/design/EDGE_DESIGN.md` ("One style for every module") still hold.

Line numbers below are in `live/edge-rs/src/` on `main` at d455039, unless a branch is named
(`origin/feat/edge-fluid` 4fdce9e, `origin/feat/edge-notify` c95a830, `origin/feat/net-icon` 92dac21).

## 1. The model

```
App       an installed program: one desktop entry (id = its file name without ".desktop")
Provider  code that knows a kind of app and what cards it can give (rich), or the fallback (simple)
Card      one thing drawn in a panel: a kind from a provider, a size, its data, its input
Panel     an edge of the screen with placed cards in order (a PANEL_ line)
```

- An **app** is found on disk (section 4). Infynitae's own parts that have no desktop entry (Settings,
  Sound, Calendar, Notifications, the app list, the window list, the clipboard) are providers that are
  listed as apps without one.
- A **provider** claims apps: by id, by id prefix, or by categories. One provider can claim several apps
  (Music claims every `AudioVideo;Player`), and gives the same card kinds for each. An app nobody claims
  goes to the `simple` provider.
- A **card** placed in a panel names its owner and its kind: `steam/games`, `settings/network`. The owner
  is an app id (the card is tied to that app) or a provider id (the card uses whichever claimed app fits:
  `music/playing` shows the player that is playing, as today's `media` does).
- A **panel** is today's panel: key, edge, place along it, width, hover, then cards in order; each card
  full or half width and in one of its sizes. Nothing is placed by pixels.

### How a panel is stored

The same `PANEL_<NAME>` lines in `~/.config/infynitae/config`. A card is one word:

```
owner/kind[=argument][:size][@half]

PANEL_CONTROL="SUPER+A top right hover calendar/clock sound/volume music/playing settings/network
               settings/bluetooth settings/power settings/quick=dnd,nightlight,keyboard,replay
               steam/games:m notifications/list settings/session"
```

- `owner`: a provider id (`settings`, `music`) or a desktop entry id (`steam`, `org.qbittorrent.qBittorrent`).
  A provider id wins when both exist; the installed entry is then written with `.desktop` at its end.
- `kind`: one of the provider's card kinds.
- `=argument`: only where a kind needs one. Today only `settings/quick` (which switches, in order).
- `:size`: `s`, `m` or `l`; left out: the kind's default. `@half`: half the panel's width, as now.

Old words (no `/`) are still read and mapped. An old line is rewritten in the new form the first time the
panel is edited and saved; the line from before goes to `~/.config/infynitae/panels.backup` as now.

| Old word | Becomes | Note |
|---|---|---|
| `wifi`, `network` | `settings/network` | looks `small`, `tile` to `:s`; `wide` to `:m` |
| `bluetooth` | `settings/bluetooth` | same |
| `power` | `settings/power` | same |
| `dnd` `nightlight` `night` `keyboard` `osk` `replay` | one `settings/quick=...` | at the place of the first of them, the switches in the order written |
| `volume` | `sound/volume` | |
| `clock`, `clock:big` | `calendar/clock:s`, `calendar/clock:m` | |
| `clock:calendar`, `calendar` | `calendar/month` | |
| `notifications` | `notifications/list` | |
| `session` | `settings/session` | |
| `media` | `music/playing` | |
| `memory` | `monitor/memory` | |
| `system` | `monitor/gpu` | |
| `drives` | `files/drives` | |
| `search` `favorites` `apps` | `apps/search` `apps/favorites` `apps/list` | looks `hidden`, `bar` kept as `:s`, `:m` |
| `windows` | `windows/list` | |
| `clipboard` | `clipboard/history` | |
| `tray` | `apps/background` | |

`module@x,y,WxH` (the older editor) is read in order as now (`config.rs:511`). The `WIDGETS` line takes
the same card words with its places (`steam/games@60,60,380x234`).

Known limit: the Rust edge of today (`EDGE_IMPL="rust"`) does not know words with `/`. Switching back to
it after a panel was saved by the new edge loses the new cards from that panel until the backup line is
put back. `live/config-check` must learn the new word form in the same change that adds it.

## 2. The crate

`live/edge-apps/`, its own crate and lockfile as `live/edge-rs` is, binary `infynitae-edge-apps`,
picked by `EDGE_IMPL="apps"` in `live/edge-pick`. Same libraries: smithay-client-toolkit 0.20, cairo and
pango 0.20, zbus 5 (blocking), chrono, libc, signal-hook-registry. No new dependency.

```
live/edge-apps/src/
  main.rs            the command line (section 9)
  app.rs             App, desktop entries, icons            (from edge-rs modules/apps.rs:84-250, 548-1055)
  provider.rs        Provider, Claim, Listed, CardKind, the registry, the old-word table
  card.rs            Card, Source, Cx, Hits, Event, Action, Status, Size
  panel.rs           PANEL_ lines, layout (column, halves), panel geometry   (from config.rs, main.rs:24-295)
  theme.rs, draw.rs  copied as they are, with their tests
  ui.rs              the shared parts every card draws with (section 3.5)
  shell.rs           the state machine: open panel, hover, keyboard, edit mode, the animation clock
  wayland.rs         surfaces, buffers, input, signals, the worker channel   (section 5)
  ask.rs             the ask prompt, copied as it is                (edge-rs modules/ask.rs)
  notify.rs, notify_server.rs   copied from origin/feat/edge-notify
  cards/settings.rs  network, bluetooth, power, quick, session
  cards/sound.rs  cards/calendar.rs  cards/notifications.rs  cards/apps.rs  cards/windows.rs  cards/clipboard.rs
  cards/monitor.rs  cards/music.rs  cards/steam.rs  cards/chat.rs  cards/files.rs  cards/simple.rs
```

`shell.rs` and every card are plain state: no Wayland, no threads, no process spawned from them. Only
`wayland.rs` and the sources do I/O. That is what makes cards testable without a display.

## 3. The provider API

All of it lives in `provider.rs` and `card.rs`. A worker writes one file in `cards/` and adds one line to
the registry.

### 3.1 Apps and providers

```rust
/// An installed program, from its desktop entry.
pub struct App {
    pub id: String,                 // "steam", "org.qbittorrent.qBittorrent"
    pub name: String,               // Name=
    pub icon: String,               // Icon=
    pub exec: String,               // Exec= with the field codes taken out
    pub categories: Vec<String>,    // Categories= split on ';'
    pub wm_class: Option<String>,   // StartupWMClass=
    pub terminal: bool,             // Terminal=true
}

/// How a provider says which apps are its own.
pub enum Claim {
    /// the desktop entry id, exactly
    Id(&'static str),
    /// ids that start with this ("org.telegram.desktop": the owner's entry is
    /// org.telegram.desktop._8cd7908c5e89c6139d26e96aae2e8a4a)
    IdPrefix(&'static str),
    /// every one of these is in the entry's Categories ("AudioVideo" and "Player")
    Categories(&'static [&'static str]),
}

/// Whether the provider itself is offered in "Add a card", beside the apps it claims.
pub enum Listed {
    /// Infynitae's own part with no desktop entry: Settings, Sound, Calendar
    Always,
    /// only while no installed app is claimed (the system monitor works from /proc without one)
    WithoutApp,
    /// only through its apps (Steam, Music) or never by name (simple)
    Never,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size { S, M, L }

/// One kind of card a provider gives.
pub struct CardKind {
    pub id: &'static str,              // "network"; the word after the '/'
    pub title: &'static str,           // "Network"; the header, and the name in "Add a card"
    pub about: &'static str,           // one line for "Add a card": what it shows and does
    pub sizes: &'static [Size],        // smallest first, at least one
    pub default: Size,
    pub half: bool,                    // may sit at half the panel's width
    pub sources: &'static [&'static str],  // the data it needs, by the provider's source keys
}

pub trait Provider: Sync {
    /// The word before the '/' when a card is not tied to one app: "settings", "music".
    fn id(&self) -> &'static str;
    /// Its name where it is listed without an app: "Settings", "System monitor".
    fn name(&self) -> &'static str;
    fn claims(&self) -> &'static [Claim];
    fn listed(&self) -> Listed;
    fn cards(&self) -> &'static [CardKind];
    /// A new card of `kind` (an id from `cards()`); None for a kind or an argument it does not know.
    fn card(&self, kind: &str, argument: Option<&str>) -> Option<Box<dyn Card>>;
    /// The source behind `key` (a key from some CardKind::sources); None for a key it does not know.
    fn source(&self, key: &str) -> Option<Box<dyn Source>>;
}

/// Every provider, in the order claims are tried. `simple` is last and takes what is left.
pub static PROVIDERS: &[&dyn Provider] = &[
    &cards::settings::Settings, &cards::sound::Sound, &cards::calendar::Calendar,
    &cards::notifications::Notifications, /* ... */ &cards::simple::Simple,
];
```

The first provider in `PROVIDERS` with a matching claim owns the app. `Claim::Id` and `IdPrefix` are
tried over all providers before any `Categories` claim, so an app claimed by name is never taken by a
category (LACT's entry says only `Categories=System;`, so the monitor claims it by id).

### 3.2 Data: sources run off the UI thread

```rust
/// What a source hands back: the provider's own type, read by its cards.
pub type Data = Box<dyn std::any::Any + Send>;

/// Fetches one piece of data. Lives on a worker thread; may block (a command, D-Bus, a file).
pub trait Source: Send {
    /// A failure is part of the data (NetworkData::NoService), not an Err: the card shows it.
    fn fetch(&mut self) -> Data;
    /// How often while a card that needs it is shown. None: once each time the panel opens.
    fn every(&self) -> Option<std::time::Duration> {
        Some(std::time::Duration::from_secs(2))
    }
}
```

- A source is known by `(provider id, key)`. Two cards that need the same key share one source and one
  fetch.
- The core starts a source when a panel (or the desktop's widgets) shows a card that lists its key, and
  stops asking when none does. A closed edge fetches nothing (the "zero cost when closed" rule of
  `docs/EDGE_RUST.md`).
- Each source has one worker thread while in use. `fetch` results go to the event loop through one
  `calloop::channel::Sender<(SourceId, Data)>`; the loop calls `Card::apply` on each card that lists the
  key and redraws when one returns true. This is today's `spawn_fetch` and `fetch_rx`
  (`modules/mod.rs:159`, `wayland.rs:2021` in `run_loop`) with typed data in place of command output keyed
  by a string.
- After a command a card asked for (`Action::Run`) ends, the core fetches that card's sources again at
  once, as signal K does today (`wayland.rs:1216-1226`).
- Test data: `INFYNITAE_EDGE_RENDER_DATA` keeps working. A source that runs a command goes through
  `card::run_sh(key, cmd)` (today's `modules/mod.rs:125`), which returns the fixture's value for `key`
  when that variable is set. A source that reads files takes its root folder as a field, so a test points
  it at a temporary folder (as `memory.rs:72` does with `/proc`).

Three things are not sources. They are kept by the core because they must run while the edge is closed
or belong to the Wayland connection, and every card may read them through `Cx`:

- `cx.apps`: the installed apps (section 4).
- `cx.windows`: open windows from `zwlr_foreign_toplevel_manager_v1` (`wayland.rs:1767-1890`): id, title,
  app id, focused.
- `cx.notes`: the notifications kept by the edge's own server (`origin/feat/edge-notify`,
  `modules/notify.rs:40` `Note`, at most 50, `MAX_HISTORY`).

### 3.3 The card

```rust
pub type Rect = (f64, f64, f64, f64);   // x, y, w, h

/// What a card can read while it measures, draws or handles input. Fixed in tests.
pub struct Cx<'a> {
    pub theme: &'a Theme,                    // colours, corners (theme.rs:65)
    pub now: chrono::NaiveDateTime,
    pub size: Size,                          // the size this card is placed in
    pub app: Option<&'a App>,                // the app the card is tied to; None for a provider's own card
    pub apps: &'a [App],
    pub windows: &'a [Window],
    pub notes: &'a [Note],
    pub list_above: bool,                    // the panel is at the bottom edge: lists open over their row
    pub editing: bool,
}

pub enum Status {
    Ready,
    /// no data yet: the core draws the header and "Loading"
    Loading,
    /// nothing to show now: no card at all (a placeholder with the title while the panel is edited)
    Hidden,
    /// it cannot work, said in one sentence: "NetworkManager is not running". The core draws the
    /// header and this line in muted text
    Missing(String),
}

pub trait Card {
    /// New data for one of its sources. `data` is the provider's type: downcast_ref it.
    /// True when what the card shows changed.
    fn apply(&mut self, key: &str, data: &dyn std::any::Any) -> bool;

    fn status(&self, cx: &Cx) -> Status;

    /// The header's title when it is not the kind's ("November 2026" for the month).
    fn title(&self, _cx: &Cx) -> Option<String> { None }

    /// The body's height at `width` (the header and the card's padding are the core's). Includes a
    /// list opened in place at its present height.
    fn height(&self, cx: &Cx, width: f64) -> f64;

    /// Draws the body into `body`. Every clickable part is added to `hits`.
    fn draw(&self, cr: &cairo::Context, cx: &Cx, body: Rect, hits: &mut Hits);

    /// Input on one of its hits. What should happen outside the card goes to `out`.
    /// True when the card must be drawn again.
    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool;

    /// Moves its animations on by `dt_ms`. True while one is still running.
    fn tick(&mut self, _dt_ms: f64) -> bool { false }
}
```

The core draws the card's box (`draw.rs:33`) and its header: the title at the left, and at the right the
name of the app the card comes from (`cx.app`'s name, or the provider's name). The card draws only the
body. So every header looks the same, and a design change to it is one place.

**App not installed.** A card does not handle this. When a placed card names an app id that is not in
`cx.apps`: if another installed app is claimed by the same provider, the card is shown for that app (and
the word is rewritten at the next save); otherwise the core draws the header with the id as the app name,
the line "This app is not installed" and a Remove button that takes the card out of the panel line.
`Status::Missing` is for the other case: the app is there but what the card needs is not (no
NetworkManager, no Bluetooth adapter, `powerprofilesctl` absent, Steam has no library yet).

### 3.4 Hits and input

```rust
pub struct Hits { /* the pointer, the card's origin, the hits of this frame */ }

impl Hits {
    /// Registers a clickable rectangle (body coordinates) under the card's own `id`.
    /// True when the pointer is over it: draw its light. A hit added later lies on top and takes the
    /// click (a More button added after its row).
    pub fn add(&mut self, id: u32, rect: Rect) -> bool;
    /// True when the keyboard's selection is on `id`.
    pub fn selected(&self, id: u32) -> bool;
}

pub enum Button { Left, Right, Middle }

pub enum Event<'a> {
    Press { hit: u32, button: Button, at: (f64, f64) },
    Drag { hit: u32, at: (f64, f64) },      // the pointer moved with the button held on `hit`
    Release { hit: u32 },
    Scroll { hit: Option<u32>, dy: f64 },   // None: over the card but on no hit
    Text(&'a str),                          // what was typed, for a card with a search field
}

/// What a card asks the core to do. Cards never start a process or touch a window themselves.
pub enum Action {
    Run(String),                    // sh -c, off the UI thread, one at a time per card (mod.rs:191)
    Launch(String),                 // an app by id: its Exec line, noted in the recent apps (apps.rs:1130)
    OpenUri(String),                // "steam://rungameid/730", a folder
    Focus(u32), CloseWindow(u32),   // a window from cx.windows
    Dismiss(u32),                   // a notification from cx.notes
    Invoke(u32, String),            // a notification's action
    ClosePanel,
}
```

- Hover: `Hits::add` is today's `hot` (`modules/mod.rs:1226`). The shell keeps the hits of the last frame
  and redraws on pointer motion only when the pointer goes from one hit to another (`main.rs:2144-2147`).
- Keyboard: arrows move the selection to the nearest hit in that direction, Enter is `Press` with
  `Button::Left` on it, Shift+Enter with `Button::Right`. A card gets keyboard use without code of its own.
  The selection is drawn only after a key was used (`origin/feat/edge-fluid` `modules/mod.rs:305-308`).
- Touch: a tap is a left press and release, a hold is a right press (`wayland.rs:768-880`,
  `TouchTracker`).
- The pointer shape is a hand over any hit.

### 3.5 Shared parts (`ui.rs`)

Cards are built from these, so they look and feel the same. Each draws with the theme and registers its
own hit.

```rust
/// A row: icon, name, second line (state), and room at the right. Returns the row's hover.
pub fn row(cr: &Context, cx: &Cx, hits: &mut Hits, id: u32, r: Rect, row: &Row) -> bool;
pub struct Row<'a> { pub icon: Option<Icon>, pub name: &'a str, pub state: &'a str, pub on: bool }

/// The switch at a row's right end. On: the accent.
pub fn switch(cr: &Context, cx: &Cx, hits: &mut Hits, id: u32, row: Rect, on: bool);

/// "More" at the right of a row while the row is hovered or selected, "Less" while its list is open.
pub fn more(cr: &Context, cx: &Cx, hits: &mut Hits, id: u32, row: Rect, open: bool);

/// A slider (the 3 px bar of draw.rs:115 with a knob); 6 px while hovered or dragged.
pub fn slider(cr: &Context, cx: &Cx, hits: &mut Hits, id: u32, r: Rect, part: f64);
pub fn slider_part(r: Rect, at: (f64, f64)) -> f64;   // the value under the pointer, 0 to 1

/// A choice of a few words in one line (power mode). Returns nothing; each word is hit `first_id + i`.
pub fn choice(cr: &Context, cx: &Cx, hits: &mut Hits, first_id: u32, r: Rect, words: &[&str], chosen: usize);

/// A text button ("Search again", "Remove").
pub fn button(cr: &Context, cx: &Cx, hits: &mut Hits, id: u32, r: Rect, word: &str);

/// A list that opens in place under (or over) a row.
pub struct List { pub open: bool, reveal: f64 }
impl List {
    pub fn toggle(&mut self);
    /// 220 ms, eased out (origin/feat/edge-fluid child.rs:106, LIST_MS). True while moving.
    pub fn tick(&mut self, dt_ms: f64) -> bool;
    /// Its height now, for `rows` rows of 32 px: add it to Card::height.
    pub fn height(&self, rows: usize) -> f64;
    /// Where the row and the list go inside `r` (the row's place with the list's height added):
    /// the list under the row, or over it when `above` (cx.list_above), so the row stays under the
    /// pointer in a panel at the bottom of the screen.
    pub fn split(&self, r: Rect, rows: usize, above: bool) -> (Rect, Rect);
}
```

A card with a list keeps a `List`, calls `toggle` on the More hit, returns `list.tick(dt)` from
`Card::tick`, and adds `list.height(n)` in `Card::height`. The core does the rest: while any `tick`
returns true it draws on every frame callback, and the panel's height follows `height`.

Icons are line icons drawn with cairo paths in `ui.rs` (`Icon::Wired`, `Icon::Wifi(level)`,
`Icon::Bluetooth`...), 16 px, 1.6 px stroke, in the text colour or the accent. App icons come from the
icon theme through `app::draw_icon` (today's `apps.rs:1001`).

### 3.6 A whole card, as an example

The Power mode card, complete except for imports. A worker's card has this shape.

```rust
pub struct PowerData { pub modes: Vec<String>, pub active: String }   // empty modes: no power-profiles-daemon

struct PowerSource;
impl Source for PowerSource {
    fn fetch(&mut self) -> Data {
        Box::new(parse_power(&run_sh("power", "powerprofilesctl list 2>/dev/null")))
    }
}

#[derive(Default)]
struct PowerCard { data: Option<PowerData> }

impl Card for PowerCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        let (true, Some(d)) = (key == "power", data.downcast_ref::<PowerData>()) else { return false };
        let changed = self.data.as_ref() != Some(d);
        self.data = Some(d.clone());
        changed
    }
    fn status(&self, _: &Cx) -> Status {
        match &self.data {
            None => Status::Loading,
            Some(d) if d.modes.is_empty() => Status::Missing("This PC has no power modes".into()),
            Some(_) => Status::Ready,
        }
    }
    fn height(&self, _: &Cx, _: f64) -> f64 { 32.0 }
    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let Some(d) = &self.data else { return };
        let words: Vec<&str> = d.modes.iter().map(|m| mode_word(m)).collect();
        let chosen = d.modes.iter().position(|m| *m == d.active).unwrap_or(0);
        ui::choice(cr, cx, hits, 0, body, &words, chosen);
    }
    fn event(&mut self, ev: &Event, _: &Cx, out: &mut Vec<Action>) -> bool {
        let (Event::Press { hit, button: Button::Left, .. }, Some(d)) = (ev, &mut self.data) else { return false };
        let Some(mode) = d.modes.get(*hit as usize).cloned() else { return false };
        out.push(Action::Run(format!("powerprofilesctl set {mode}")));
        d.active = mode;          // shown at once; the fetch after the command confirms or corrects it
        true
    }
}
```

## 4. Finding the apps

Reuse `modules/apps.rs`: `default_app_dirs` (215), `load_apps_from_dirs` (177), `parse_desktop_entry`
(107), `clean_exec` (84). The folders, first wins for the same file name:

1. `$XDG_DATA_HOME/applications` (default `~/.local/share/applications`)
2. `~/.local/share/flatpak/exports/share/applications`
3. each of `$XDG_DATA_DIRS` (default `/usr/local/share:/usr/share`) + `/applications`
4. `/var/lib/flatpak/exports/share/applications`

An entry with `NoDisplay=true` or `Hidden=true` is not an app (and hides the same file name in a later
folder), as now. To add: read `Categories=` and `Terminal=`. Today's parser reads neither and the edge runs the `Exec`
line as it is; `btop.desktop` is `Terminal=true`, so `Action::Launch` must start such an app inside the
terminal that `live/launch.sh` picks for Super+Q (`launch.sh:62`).

When: once when the edge starts, then again each time a panel opens if any of those folders' modification
time changed (a stat per folder, no watcher thread). Snap's folder (`/var/lib/snapd/desktop/applications`)
is in `XDG_DATA_DIRS` where snap is set up; nothing extra is done for it.

Claims, from the owner's PC on 2026-11-13 (read from the entries there):

| App | Entry id | Categories | Goes to |
|---|---|---|---|
| btop++ | `btop` | System;Monitor;ConsoleOnly | `monitor` by `Categories(["System","Monitor"])` |
| LACT | `io.github.ilya_zlobintsev.LACT` | System | `monitor` by `Id` |
| Melodyae | `melodyae` | AudioVideo;Audio;Player | `music` by `Categories(["AudioVideo","Player"])` |
| mpv | `mpv` | AudioVideo;Audio;Video;Player;TV | `music`, same claim |
| Steam | `steam` | Network;FileTransfer;Game | `steam` by `Id("steam")`, `Id("com.valvesoftware.Steam")` |
| Discord | `discord` | Network;InstantMessaging | `chat` by `Id("discord")`, `Id("com.discordapp.Discord")` |
| Telegram | `org.telegram.desktop._8cd7...` | Chat;Network;InstantMessaging;Qt | `chat` by `IdPrefix("org.telegram.desktop")` |
| PCManFM-Qt | `pcmanfm-qt` | FileManager;Utility;Core;Qt | `files` by `Categories(["FileManager"])` |
| Firefox, kitty, qBittorrent, RustDesk, AEAM, Faugus, SongRec | | | `simple` |

The Steam shortcuts in `~/.local/share/applications` ("Counter-Strike 2.desktop" and so on) are apps too
and go to `simple`; the Steam card lists the games without them.

"Add a card" lists: first the providers with `Listed::Always` and those with `Listed::WithoutApp` that
have no app (under "Infynitae"), then the claimed apps ("Rich cards"), then the rest ("Simple card"),
each group by name. Picking one shows its kinds (`CardKind::title`, `about`) with an Add button each.

## 5. Surfaces, the animation clock, input, damage

Kept as in `live/edge-rs/src/wayland.rs`, which was measured and fixed over several rounds. The new
`wayland.rs` starts as a copy with the module calls replaced by the card calls.

- **Surfaces**: one layer surface per output over the whole output, Top layer, anchored to all four
  edges, exclusive zone -1, no keyboard until a panel is open (`wayland.rs:1115-1160`). A second one on the
  Bottom layer on the first output for the desktop's widgets, made only when there is a `WIDGETS` line
  (`wayland.rs:1163-1173`). A surface the size of the panel was considered and not chosen: a list opening
  in place would need a new size from the compositor on each frame of its animation, and the hover strips
  and "a click outside closes" need the whole output anyway.
- **Input region**: closed, only the hover strips; opened by hover, the panel and the way to it; opened
  by key, the whole output so a click outside closes it (`wayland.rs:173-235`).
- **Under a fullscreen window**: the closed surface is unmapped (no buffer), so the window can be shown
  directly (`wayland.rs:237-262`; the reason is in the comment at 160-164).
- **Buffers and damage**: two shm buffers per surface; each frame clears and redraws the panel's box and
  damages that box, the box this buffer held before, and the box last committed (`wayland.rs:319-343`,
  `403-711`, damage at `692-702`). Known cost, kept for now: a hover change redraws the whole panel, not
  one card. Not measured on the new cards; if a VM measurement shows it matters, the step is a clip to the
  cards whose `apply`, `event` or `tick` returned true.
- **Frame pacing**: draw only after the compositor's frame callback (`frame_pending`, `wayland.rs:1290-1321`);
  when both buffers are still held, commit unchanged and draw on the callback (`wayland.rs:414-422`).
- **The animation clock**: as `World::tick` on `origin/feat/edge-fluid` (`modules/mod.rs:1333-1379`, called
  from the frame callback at `wayland.rs:1310-1332`). In the new edge `Shell::tick(now) -> bool` does three
  things: the cards' arrival after a panel opens (200 ms each, 28 ms between, at most 8 spaced; drawn by the
  core with `push_group` and `paint_with_alpha`, `modules/mod.rs:1472-1492` there), every shown card's
  `Card::tick(dt)`, and the notification popups' arrival. A step is capped at 50 ms so a stalled frame does
  not jump to the end. While it returns true the surface is marked dirty on each frame callback; when it
  returns false no more frames are asked for, and the input region is set once for the settled size.
- **Timers**: none while closed. Open: sources by their `every`. Widgets: the clock wakes the edge on the
  minute (`wayland.rs:109`), sources only for the kinds placed.
- **Pointer, keyboard, touch**: `PointerHandler::pointer_frame` (`wayland.rs:1571-1636`) hands motion,
  press, release and axis to the shell; the shell finds the card and the hit and calls `Card::event`.
- **Popups**: the notification popups are drawn on the same surface, as on `origin/feat/edge-notify`.

## 6. Building order

### Stage 1: the core and the system cards (Claude)

1. The crate, `app.rs`, `provider.rs`, `card.rs`, `panel.rs` with the old-word table, `ui.rs`, `--render`,
   `--print-panels`. Done when: an old `PANEL_CONTROL` line prints as new words, and the Power mode card
   above renders from fixed data in CI.
2. `wayland.rs`, `shell.rs`, `ask.rs`, the signals and files of section 9. Done when: a panel opens by key
   and by hover in the VM, and `tools/test-edge-ask` passes against the new binary.
3. `settings/network`, `settings/bluetooth`, `settings/power`. Data and commands as today
   (`switches.rs:19-41`, `child.rs:358-630`, the network state and icon of `origin/feat/net-icon`
   `switches.rs:147-180`, `865-911`): `nmcli`, `bluetoothctl`, `powerprofilesctl`.
   - Network, S: one row (icon by state, "Network", state: "Wired", the Wi-Fi network's name, "Not
     connected", "Off"), the switch (Wi-Fi radio), More. M: the list always open. The list: the joined
     network first, the others by signal with a lock where secured, "Hidden network", "Search again".
     On a PC with no Wi-Fi device (the owner's: `wifi:unavailable`) the switch and More are not shown.
   - Bluetooth, S: row, switch, More. The list: paired devices (connected first), "Find new devices"
     (scans 15 s, found ones with Pair, then "Pairing", then connected or the failure in words).
   - Power mode: the choice of the modes `powerprofilesctl list` gives.
4. `sound/volume` (`volume.rs`, the outputs and per-app levels of `child.rs:204-320`), `settings/quick`
   (do not disturb, night light, keyboard layout, screen keys, replay; `switches.rs`), `notifications/list`
   with the server (`origin/feat/edge-notify`), `settings/session` (`session.rs`).
5. `apps/*`, `windows/list`, `clipboard/history`, `calendar/clock`: today's modules moved onto the card
   trait so the four built-in panels work. Edit mode with "Add a card" by app.
6. `simple.rs` (the brief is below; it is small and the core's "Add a card" needs it).

Stage 1 is done when the four built-in panels work in the VM with `EDGE_IMPL="apps"` and the VM scripts
of `tools/vm-edge` pass against it.

### Stage 2 and later: app cards (agy workers)

One worker per card file. Each brief names the file, the claims, the kinds, where the data is, what a
click does, and what is not known. Every card follows section 7 (tests). No card may add a dependency.

**Calendar** (`cards/calendar.rs`, provider `calendar`, `Listed::Always`, no claims). The clock exists
from stage 1; this adds `month`: sizes M (the month) and L (the month and the coming events), half
allowed. Title: the month and year, with previous and next arrows; weekday letters, days, other months'
days dim, today marked with the accent, as `docs/design/EDGE_DESIGN.md` "Calendar" and today's
`modules/calendar.rs` (month arithmetic `shifted_month`, tests there). No source for the month: it reads
`cx.now`; the core redraws on the minute. Events are a later step and have no data source decided yet
(today's `parse_calendar_events`, `calendar.rs:31`, reads lines nobody writes): L shows the month only
until one exists, and nothing about events is drawn. First week day: Monday, unless the locale's
`LC_TIME` says otherwise (`locale first_weekday`; to check in the VM).

**System monitor** (`cards/monitor.rs`, provider `monitor`, `Listed::WithoutApp`, claims
`Categories(["System","Monitor"])`, `Id("io.github.ilya_zlobintsev.LACT")`). Kinds: `overview` (S: CPU %,
GPU %, memory used, one line each with a thin bar; M: the same plus CPU and GPU temperature), `memory` and
`gpu` (today's `modules/memory.rs` and `modules/system.rs` moved, with their parsers and tests). Sources,
all files, the root folders as fields for tests: `/proc/stat` (CPU %: the difference between two reads, so
the first fetch shows no number), `/proc/meminfo`, `/proc/[pid]/status` for the top apps (`memory.rs:19`),
`/sys/class/drm/card*/device/gpu_busy_percent`, `mem_info_vram_used`, `mem_info_vram_total` (amdgpu; these
are what `system.rs:121` looks for), `hwmon*/temp1_input` under the GPU's device and under
`/sys/class/hwmon` for the CPU (`k10temp`, `coretemp` by the `name` file). A value that cannot be read is
not shown (no "0", no "N/A"); NVIDIA and Intel GPUs have other files and are not covered until someone can
test on one. A click on the header or a row: `Action::Launch(cx.app)`. `every`: 2 s.

**Music** (`cards/music.rs`, provider `music`, `Listed::Never`, claims
`Categories(["AudioVideo","Player"])`). Kind `playing`, sizes S, M, L as "Music, three sizes" in
`docs/design/EDGE_DESIGN.md`; today's `modules/media.rs` (2070 lines: MPRIS over zbus, Melodyae's queue
and lyrics through `app.melodyae.Extras`, cover loading) is the code to move. Which player: for
`music/playing`, the one playing, else the last one that played; for `melodyae/playing`, the MPRIS name
whose `DesktopEntry` property equals the app id (`org.mpris.MediaPlayer2` root interface), compared without
case. Not known without trying: whether every player sets `DesktopEntry` (mpv needs its mpris plugin to
appear on D-Bus at all). When the tied app has no MPRIS name on the bus, the card shows the app's name
and an Open button, nothing else. A browser playing audio also appears on MPRIS and can be shown by
`music/playing`. `Status::Hidden` for `music/playing` when no player exists.

**Steam** (`cards/steam.rs`, provider `steam`, `Listed::Never`, claims `Id("steam")`,
`Id("com.valvesoftware.Steam")`). Kind `games`: S 3 rows, M 6, L every game, scrolling. Source (`every`:
None, once per open): the Steam folder is the first that exists of `~/.local/share/Steam`,
`~/.steam/steam`, `~/.var/app/com.valvesoftware.Steam/.local/share/Steam`; read
`steamapps/libraryfolders.vdf`, take every `"path"` value; in each `<path>/steamapps/` read every
`appmanifest_<id>.acf` and take `"appid"`, `"name"`, `"StateFlags"`, `"LastPlayed"` (seconds since 1970;
"0" when never played). The format is quoted keys and values, tab separated, in braces; a small parser of
our own, no crate. Leave out tools by name: names starting with "Proton ", "Steam Linux Runtime",
"Steamworks Common Redistributables" (the owner's library has 8 such entries among 18, all with
`LastPlayed` 0; a never-played game also has 0, so the number cannot be the filter). A tool with another
name will show as a game: known gap. Order: `LastPlayed` descending, never played last by name. Each row:
the name, "Last played" as a day ("Yesterday", "Sun 8 Nov"), and on hover a Play button:
`Action::OpenUri(format!("steam://rungameid/{appid}"))`. A cover, if used at M and L:
`<Steam>/appcache/librarycache/<appid>/library_600x900.jpg` (present on the owner's PC for 730; other
games keep it in a subfolder with a hash for a name, and older clients used
`librarycache/<appid>_library_600x900.jpg`); no cover found: the row without one. Not known without
trying: how to tell a game is running. The window's app id may be `steam_app_<appid>` for Proton games and
something else for native ones; until that is checked in real use, no "running" mark is drawn.
`Status::Missing("Steam has no games installed")` when no manifest is left after the filter.

**Discord and Telegram** (`cards/chat.rs`, provider `chat`, `Listed::Never`, claims `Id("discord")`,
`Id("com.discordapp.Discord")`, `IdPrefix("org.telegram.desktop")`). Kind `messages`, S (3) and M (8).
No source of its own. It shows, from `cx.notes`, the notifications of this app, newest first, grouped by
their summary (the sender or the channel), with an x on hover (`Action::Dismiss`) and a click that invokes
the default action (`Action::Invoke(id, "default")`, which makes the app show that chat). Under them, from
`cx.windows`, the app's windows by title; a click focuses (`Action::Focus`). No notification and no window:
one row, "Open Discord" (`Action::Launch`). Matching: a note belongs to the app when its `desktop-entry`
hint equals the app id, else when its app name equals the app's name without case (the server keeps one
field from both, `origin/feat/edge-notify` `notify_server.rs:83`); a window belongs by
`app_matches_window_id` (`apps.rs:548`). What cannot be had: neither app gives an unread count the edge can
read. Discord shows what it sends as notifications only, and sends none while its window is focused or
when its own notification setting is off, so the card can be empty while messages are unread; the card
never shows a number of unread messages. Telegram may send its count on the
`com.canonical.Unity.LauncherEntry` D-Bus signal; this was not checked here and is not part of this card
until someone watches the bus with Telegram running. Not checked either: whether Discord sets the
`desktop-entry` hint (the name fallback covers it if it sends "discord").

**Files** (`cards/files.rs`, provider `files`, `Listed::WithoutApp`, claims
`Categories(["FileManager"])`). Kinds: `drives` (today's `modules/drives.rs` moved: `/proc/self/mountinfo`,
`/etc/fstab` entries with `x-gvfs-show`, names from `x-gvfs-name=` so the owner's drives read I to V, used
and total, eject for removable ones) and `folders`, S (5) and M (10): the home folder's XDG folders
(`xdg-user-dir` names that exist), then the bookmarks of `~/.config/gtk-3.0/bookmarks` (one `file://` URI
and an optional name per line; PCManFM-Qt and the GTK file choosers share this file; it is empty on the
owner's PC today), then recent folders: the parent folders of the newest entries of
`~/.local/share/recently-used.xbel` (`<bookmark href="file://..." modified="...">`), each folder once, only
those that still exist. Honest limit: that file is written by GTK programs and by some Qt ones; folders
opened only in PCManFM-Qt may not appear. A click: `Action::OpenUri(folder)`, opened by the tied app, or
the default file manager for `files/...`. `every`: None for folders, 2 s for drives while shown.

**Simple card** (`cards/simple.rs`, provider `simple`, last in `PROVIDERS`, claims everything left).
Kind `app`, S and M. S: one row, the app's icon and name, "Open" on hover, and under the name "2 windows"
or "Not running". M: that row, then its windows from `cx.windows` by title (click focuses, x closes), then
its notifications from `cx.notes` (as the chat card, 3 at most). No source. A click on the first row:
focus its newest window when it has one, else `Action::Launch`.

## 7. Tests every card must have

Run by CI in a job "Edge (apps)" copied from the `edge-rs` job (`.github/workflows`, fmt, clippy with
`-D warnings`, `cargo test --locked`, then the shell scripts). Never on the owner's PC.

1. **Parsing**, unit tests in the card's file, on text copied from a real machine (as
   `parses_real_nmcli_outputs`, `switches.rs:835`): each source's parser with a usual output, an empty
   one, and a broken one. For file sources: a temporary folder laid out like the real one.
2. **State**: `apply` then `status` for Loading, Ready, Hidden and Missing; `event` for each hit, asserting
   on the `Vec<Action>` and on the card's new state (a click on Performance pushes
   `Run("powerprofilesctl set performance")` and shows it chosen).
3. **Height**: `height` equals what `draw` uses, for each size, with a list closed, half open and open.
4. **Render**: `infynitae-edge-apps --render NAME WxH OUT.png` with `INFYNITAE_EDGE_RENDER_DATA` and
   `INFYNITAE_EDGE_RENDER_TIME` fixed, compared as `tools/test-edge-render` compares today. One picture per
   size and per state that looks different (for Network: wired, Wi-Fi joined, not connected, off, list
   open). `INFYNITAE_EDGE_RENDER_POINTER` puts the pointer on a hit to check the light and More.
5. **A VM test with real input** in `tools/vm-edge/p<card>.py`, in the form of `pcards.py`: open the panel
   with its key through the virtual keyboard, read `INFYNITAE_EDGE_BOXES`, move the virtual mouse onto the
   card, click each control, and check the effect outside the edge (`nmcli radio wifi` says disabled; a
   window got focus; `steam://` reached a stub handler). It prints PASS or FAIL per check and a SUMMARY
   line. The report of a card says which of these ran and quotes their output; "tested" is not written for
   one that did not run.

A fix to a card comes with a test that failed before it.

## 8. What is drawn where: the split of work

| Part | Who draws and decides |
|---|---|
| The card's box, header (title, app name), padding, arrival, Loading, Missing, not installed | the core |
| The body | the card, with `ui.rs` parts |
| Panel layout, halves, scrolling a panel taller than the screen, edit mode, "Add a card" | the core |
| What a click does outside the edge | the core, from the card's `Action`s |
| Colours and corners | `Theme` only (`theme.rs:65`, from `THEME_SCHEME`, `CORNERS`, `CARD_OPACITY`); a card has no colour of its own |

## 9. Compatibility

Other parts call the edge by its command line, signals and files. The new crate keeps each one; a part
that calls `infynitae-edge` must not be able to tell which edge answered.

| What | Used by | How the new crate keeps it |
|---|---|---|
| `infynitae-edge ask [--title-stdin] [--text] [--secret] [--default TEXT] [TITLE]`; choices on standard input, the answer on standard output; exit 0 answered, 1 cancelled, 3 no edge running (2: wrong use) | infynitae-keyring's permission and password cards, `live/infynitae-format-drive`, `live/screenshare-chooser` (as the picker it is given; on `main` the script does not name the edge itself) | `modules/ask.rs` is copied whole with its tests (`run_client` at 196, the limits at 23-27, the card's drawing and keys). The prompt is not a card: the shell draws it over everything, queues requests, and closes it when the asker is gone, as now (`main.rs:678-696`, `wayland.rs:903-956`). `tools/test-edge-ask` runs against the new binary |
| The ask files in `$XDG_RUNTIME_DIR`: `infynitae-edge-rs.ask`, `.ask.<pid>` pipes, `.ask-cancel`, and `infynitae-edge-rs.features` holding the line `ask` | `live/infynitae-format-drive:53-54` reads the features file to decide between the edge and fuzzel | the same file names (only one edge runs at a time); the features file is written at start and removed at exit (`wayland.rs:1936`, `2098`) |
| `infynitae-edge panel NAME`, `control`, `apps`, `clipboard`, `toggle` / `center` / `open` (the windows panel), `widgets`, `stop` | the compositor's keys through `live/launch.sh`, `live/session.sh`, `live/infynitae-reload` | `parse_args` (`main.rs:311-336`) is copied with its tests; a second call finds the running edge by `infynitae-edge-rs.pid` (`wayland.rs:71-85`; its check of the process name accepts any `infynitae-edge-*`), writes the panel's name to `infynitae-edge-rs.panel` and sends SIGRTMIN+2 (`wayland.rs:88-97`) |
| Signals: SIGUSR1 windows, SIGUSR2 control, SIGRTMIN clipboard, SIGRTMIN+1 apps, SIGRTMIN+2 the panel named in the file, SIGRTMIN+3 an ask request or a refresh, SIGTERM / SIGINT stop | the same callers, and scripts that refresh the open panel after changing something | `setup_signal_pipe` (`wayland.rs:1894-1921`) and `handle_signal_byte` (`wayland.rs:1187-1232`) copied; "refresh" fetches the open panel's sources again |
| `--print-panels` | `tools/test-edge-panel-lines`, `tools/test-edge-rs`, `tools/vm-edge/pfeel.py` | prints the panels as now, the cards as new words; an old line prints mapped. The test's expected output changes in the same commit, old-word lines kept as inputs |
| `--print-theme` | `live/settings.sh` (the look file `~/.cache/infynitae/look`: LOOK_CARD, LOOK_EDGE, LOOK_TEXT, LOOK_ACCENT, LOOK_CORNERS...) | `theme.rs` copied; same lines (`theme.rs:121-146`) |
| `--render NAME WxH FILE`, `--render widgets WxH FILE`, with `INFYNITAE_EDGE_RENDER_DATA`, `_TIME`, `_POINTER`, `_EDIT`, `_ADD`, `_ASK`, `_SEARCH`, `_SELECT`, `_SUB`, `_RECT` | `tools/test-edge-render`, `tools/test-edge-rs` | the same arguments and variables (`main.rs:2194-2470`). The fixture keys stay as listed at the top of `modules/mod.rs` (wifi, bluetooth, power, volume...), since the stage 1 sources run the same commands. `_SUB` (a list open) names a card kind |
| `INFYNITAE_EDGE_BOXES`: after each drawing a file with `panel NAME x y w h editing|locked SIDE`, one `module WORD x y w h` per card, `bar`, `barbutton`, the open list's and the add sheet's boxes; `closed` when closed | `tools/vm-edge/*.py` | `dump_boxes` (`modules/mod.rs:1016`) kept. For a card that came from an old word the line keeps `module <old word>` so today's VM scripts pass unchanged; every card also gets `card OWNER/KIND x y w h`, and each hit `hit OWNER/KIND ID x y w h` so new scripts click controls by id |
| The notification server: `org.freedesktop.Notifications` on the session bus (Notify, CloseNotification, GetCapabilities, GetServerInformation; the signals NotificationClosed and ActionInvoked), the name asked for without queueing so another daemon that holds it keeps working | every app that sends notifications; `live/session.sh` and `live/infynitae-reload` on that branch decide whether to start mako | `notify_server.rs` and `modules/notify.rs` copied from `origin/feat/edge-notify` with their tests; started in `run_loop` as there (`wayland.rs:2138`). Its history is `cx.notes`, read by the Notifications, chat and simple cards. That branch is not merged to `main` yet: if it changes before it merges, the copy follows it |
| `PANEL_<NAME>`, `WIDGETS`, `FAVORITES`, `CONTROL_MODULES`, `EDGE_POSITION`, `EDGE_CONTROL_SIDE`, `PANELS_SNAP`, `PANELS_OVERLAP`, the look settings | the owner's config | read as now (`config.rs:623-790`); section 1 for the card words |
| The layer names `infynitae-edge` and `infynitae-widgets` | the compositor's rules for layers | the same names (`wayland.rs:1121-1125`) |

## 10. Decisions made here, and what is still open

Decided in this document (the owner was not asked; each can be changed before stage 1 starts):

- Cards are stored in the same `PANEL_` lines as `owner/kind` words, not in a new file.
- A card can be tied to "whichever app of this kind" by naming the provider (`music/playing`).
- Network, Bluetooth and Power mode are cards of their own; the other switches share one Quick settings
  card.
- The tray (`apps/background`) stays as a card of the app list for now. With a simple card for every app
  it may no longer be needed; that is the owner's call after using stage 1.
- The header of every card is drawn by the core.
- The look: `system-cards.html` shows three versions. Version 1 (rows and switches) is the one the app
  cards page and the sizes in this document assume, because it keeps what was already chosen (More on
  hover, lists in place under a row) and its header has room for the app's name. The owner picks; versions
  2 and 3 change only `ui.rs` and the core's header, not the card trait.

Open, cannot be known without trying: marked in the briefs (Steam's running games and covers, players'
`DesktopEntry`, Telegram's count, Discord's hint, the first week day, recent folders from Qt apps).
