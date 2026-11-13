# Settings in the edge (2026-11-13)

A plan, nothing here is built. Infynitae has no settings window: the settings are lines in
`~/.config/infynitae/config` (defaults and comments in `config/config`) and `live/settings.sh` changes
one from a terminal. This document adds cards to the `settings` provider of the new edge
(`docs/EDGE_APPS.md`, `live/edge-apps/src/cards/settings.rs:13`) so the settings people change are
rows, switches and lists, and the rest stays one click away in the file.

The look is `docs/design/apps-edge/settings-cards.html` (version 1 of `system-cards.html`: rows and
switches, More under the pointer, lists in place). `docs/NO_SLOP.md` and `docs/design/EDGE_DESIGN.md`
hold. Line numbers are in the worktree `feat/edge-apps`.

## 1. What is added

| Kind | Title | Config section it covers |
|---|---|---|
| `settings/all` | Settings | the index: one row per card below, each opening in place |
| `settings/appearance` | Appearance | Look (colours, corners), part of Windows & canvas |
| `settings/wallpaper` | Wallpaper | Look (the three WALLPAPER keys) |
| `settings/fonts` | Fonts, icons, cursor | Look (fonts, icon theme, cursor) |
| `settings/display` | Display | Displays & sharpness, night light |
| `settings/input` | Keyboard and mouse | Keyboard & mouse |
| `settings/windows` | Windows | Windows & canvas (behaviour) |
| `settings/defaults` | Apps | Apps (default apps, what starts) |
| `settings/games` | Games | Performance & latency |
| `settings/lock` | Lock and sleep | Lock & idle, battery limit |
| `settings/privacy` | Privacy | Privacy & screen capture, clipboard |
| `settings/recording` | Screenshots and recording | Screen recorder, screenshots |
| `settings/about` | This PC | nothing in the config: what the PC is, updates, the config file |

Already there and unchanged: `network`, `bluetooth`, `power`, `quick`, `session`.

Not cards, and why:

- **Notifications.** The config has two keys: `GAME_QUIET` (a row in Games) and `NOTIFICATIONS` (which
  program shows them; left to the file). Do not disturb is in `settings/quick`. Two rows do not make a
  card; typing "notifications" in the panel finds both (section 5).
- **Sound devices.** `sound/volume` has the output list. The config has no sound key.
- **Keys** (`BIND_*`, `EXEC_*`, `GLOBAL_SHORTCUTS`). Changing a key needs the edge to hear a held chord,
  and `Event` (`card.rs:190`) carries typed text only. Left to the file until the owner asks for it.
- **Panels.** `PANEL_*`, `WIDGETS` and `FAVORITES` are changed by editing a panel, as now.

## 2. Needed before any card (the core and `settings.sh`, one change each)

These are faults found while reading; a card built without them misbehaves.

1. **The edge cannot read what `settings.sh set` writes.** `set` writes `KEY='value'`
   (`settings.sh:780-794`); the edge's `value_text` (`config.rs:590`) strips only double quotes, so it
   reads `'bottom'`. Fix: `value_text` reads single quotes as the compositor's `config_line` does
   (`crates/infynitae-wayland/src/lib.rs:1985`). Test: a config line in each quoting gives the same value.
2. **`set` refuses an empty value** (`settings.sh:766`), so `WALLPAPER`, `TERMINAL_FONT`,
   `XKB_DEFAULT_LAYOUT`, `PRIVACY_APPS` cannot go back to their default `""`. Fix: only a missing second
   argument is an error.
3. **The edge reads the config once, at its start** (`wayland.rs:1252`). A corner or colour changed from
   a card would not show in the panel it was changed in. Fix: on the refresh signal (SIGRTMIN+3) the
   shell loads `Config` again when the file's modification time changed and rebuilds `Theme`; panels
   keep their open state.
4. **`set` always re-applies every toolkit file** (`apply_settings --force`, `settings.sh:809`), also
   for keys only the compositor reads. Not measured. If the VM shows more than 100 ms, add
   `set --no-apply` for those keys; not before.
5. **A source for the config.** `settings` gets the source key `config` (`every`: None; fetched again
   after each `Action::Run`, `docs/EDGE_APPS.md` 3.2): `ConfigData { now: map, default: map }` from
   `~/.config/infynitae/config` and `config.template` beside it. Its root folder is a field, for tests.
6. **Two parts in `ui.rs`**: `value(cr, cx, row, x_right, text)` (a row's value at its right, 12 px,
   muted; in the accent when it differs from the default) and `step(...)` (minus, the number, plus; for
   font and cursor size). Nothing else is new: `row`, `switch`, `more`, `choice`, `slider`, `List`,
   `list_row`, `button` (`ui.rs:209-566`).

## 3. How every row works

One file, `cards/settings_rows.rs`, holds what the cards share; each card file is then a table.

```rust
pub enum Control {
    Switch { on: &'static str, off: &'static str },          // the two values written
    Pick(&'static [(&'static str, &'static str)]),           // (value, word): a list in place
    Words(&'static [(&'static str, &'static str)]),          // at most 3: ui::choice under the name
    Slide { min: f64, max: f64, step: f64, unit: &'static str },
    Step { min: i64, max: i64 },
    Found(&'static str),     // a list from a source key ("fonts"): what this PC has
    Text,                    // asked with `infynitae-edge ask --text --default <now>`
}
pub enum After { Save, Use, Restart, Reload(&'static str) }  // Reload: the infynitae-reload part
pub struct Setting {
    pub key: &'static str, pub name: &'static str, pub says: &'static str,
    pub control: Control, pub after: After, pub more: bool,   // more: only under More (or at M)
}
```

- **Reading.** The value is `config.now[key]`, else `config.default[key]`.
- **Sizes.** S: the rows with `more: false`, then one row "More" that opens the others in place. M: all
  rows. Every card allows S and M, default S, `half: false`.
- **A change** shows at once (as `settings.rs:926` does), then one `Action::Run`:

  ```
  sh "$HOME/infynitae/live/settings.sh" log KEY 'OLD' && \
  sh "$HOME/infynitae/live/settings.sh" set KEY 'NEW' >/dev/null 2>&1 [&& AFTER]
  ```

  the path as `quick.rs:172`, values through `settings::sq` (`settings.rs:98`). `log` is new: it adds
  `time<TAB>KEY<TAB>OLD` to `~/.local/state/infynitae/settings-changes` (the last 50 lines kept); OLD
  is `-` for a key that had no line. `AFTER` by the key's tag in `config/config:10-14`:

  | Tag | After `set` | The row says |
  |---|---|---|
  | `[save]` | nothing: the compositor checks the file every 250 ms (`infynitae-backend-drm/src/lib.rs:1712`) | nothing |
  | `[reload]`, look keys | nothing more: `set` already re-applied GTK, Qt and the terminals; the core's refresh re-reads the theme (2.3) | nothing |
  | `[reload]`, a helper | `sh "$HOME/infynitae/live/infynitae-reload" PART` (the part is in each card's table) | nothing |
  | `[use]` | nothing | nothing |
  | `[restart]` | nothing | "At the next start" in place of the state, in the accent, until the compositor restarts |

- **A slider** writes on release, not while dragged (one line in the log, one apply).
- **Undo.** `settings/about` shows "Undo: KEY was OLD" for the log's last line; a click sets it back
  (or removes the line for `-`: `settings.sh unset KEY`, new, comments the line out) and drops the log
  line. So undo steps back through up to 50 changes, across restarts.
- **Hidden.** A row whose program or hardware is missing is not drawn (the tests are in each card).
  A card left with no row is `Status::Hidden`.
- **Typing** in the panel (`Event::Text`, `shell.rs:557`): a card shows only rows whose name, `says` or
  key contains every typed word, without case, More ignored; none: `Status::Hidden`.
- **Tests**: as `docs/EDGE_APPS.md` section 7. For each card the VM script clicks each control and reads
  the value back with `settings.sh get KEY`, then checks the effect named in its table.

## 4. The cards

Columns: the row's name; control; key; values (word shown = value written where not said); after.
"M" in the first column: only under More.

### `settings/all` (`cards/settings_all.rs`)

One row per card of this section that is not `Hidden`, in the order of section 1: its title, and as
state one line of what is set ("Dark, corners 8 px"; "DP-1 2560x1440 165 Hz"). A click opens that
card's body in place under the row (`ui::List`; one open at a time; the body drawn at S, its own More
working). It owns one `Box<dyn Card>` per kind and passes `apply`, `event`, `tick` through; its
sources are theirs together. Hits: child `i` gets ids `1000 * (i + 1) + child id`. While something is
typed, the rows are replaced by every child's matching rows under the child's title, then "In the
file" (section 5). Sizes: M only. No setting of its own.

### `settings/appearance` (`cards/settings_look.rs`)

| Row | Control | Key | Values | After |
|---|---|---|---|---|
| Apps look | Words | `COLOR_SCHEME` | Dark `prefer-dark`, Light `prefer-light` | look |
| Accent | Text, with a 16 px swatch of the value | `BORDER_ACTIVE` | `#rrggbb`; anything else is refused and the row says "Not a colour" | look |
| Colours from the wallpaper | Pick | `THEME_FROM_WALLPAPER` | Off `false`, The accent `accent`, Everything `all` | look |
| Corners | Slide 0-24 px | `WINDOW_CORNERS` | | look |
| Panels' colours | Pick | `THEME_SCHEME` | Yours, Monochrome, Neutral, Tonal, Vibrant, Expressive (lower case written) | look |
| M Cards | Slide 20-100 % | `CARD_OPACITY` | | look |
| M Background | Text + swatch | `SURFACE_COLOR` | | look |
| M Text | Text + swatch | `TEXT_COLOR` | | look |
| M Window border | Step 0-8 | `BORDER_WIDTH` | | save |
| M Other windows' border | Text + swatch | `BORDER_INACTIVE` | | save |
| M Behind the windows | Text + swatch | `CANVAS_COLOR` | | save |
| M Folders in the accent | Switch | `FOLDER_COLOR` | `accent` / `theme` | look |
| M Apps follow these colours | Switch | `THEME_SYNC` | | look |

The swatch is the setting's own value drawn as data; nothing else on the card has a colour of its own.
Hidden: "Folders in the accent" unless `ICON_THEME` starts with `Papirus`. "Colours from the
wallpaper" without `~/.local/bin/infynitae-wallpaper`. While it is not Off, Accent (and for
Everything also Background, borders, Behind the windows) show "From the wallpaper" and are not
clickable: the next apply would overwrite them (`settings.sh:225-241`). Left to the file: `CORNERS`
(one corner for everything is the rule), `SQUARE_CORNERS`, `BORDER_IGNORE_CSD`, `CLIP_TO_GEOMETRY`,
`WINE_TRANSPARENCY`, `FORCE_TRANSPARENT_APPS`, `SWITCH_LOOK`. Not known without trying: whether
`WINDOW_CORNERS` reaches open windows on save or only on reload (its tag says reload; the compositor
reloads on save).

### `settings/wallpaper` (same file)

| Row | Control | Key | Values | After |
|---|---|---|---|---|
| Picture | Found `wallpapers` | `WALLPAPER` | the file's name shown, its path written; first entry "The first in Pictures/Wallpapers" writes `""` | save (the compositor restarts the wallpaper, `infynitae-wayland/src/lib.rs:453`) |
| Fit | Pick | `WALLPAPER_MODE` | Fill, Fit, Stretch, Center, Tile | save |
| M Depth | Pick | `WALLPAPER_PARALLAX` | Off, Subtle, Strong | save |

Source `wallpapers`: the png, jpg, jpeg, webp files of `~/Pictures/Wallpapers` and
`~/.local/share/wallpapers` (`live/wallpaper/infynitae-wallpaper.c:782`), by name, at most 12 listed and
"Other file" (asked as text). No thumbnails in this stage (decoding images is not in the crate).
Hidden: the whole card without `infynitae-wallpaper`.

### `settings/fonts` (same file)

| Row | Control | Key | Values | After |
|---|---|---|---|---|
| Font | Found `fonts` | `FONT_NAME` | | look |
| Size | Step 6-24 | `FONT_SIZE` | | look |
| Icons | Found `icons` | `ICON_THEME` | | look |
| Cursor | Found `cursors` | `CURSOR_THEME` | | look |
| M Cursor size | Pick | `CURSOR_SIZE` | 16, 24, 32, 48 | look |
| M Terminal font | Found `fonts-mono` | `TERMINAL_FONT` | first entry "Same as Font" writes `""` | look |

Sources (`every`: None): `fonts`: `fc-list : family`, the first name of each line, sorted, once each,
with `sans-serif` and `monospace` first; `fonts-mono`: `fc-list :spacing=100 family`; `icons`: folders
with an `index.theme` holding `Directories=` under `/usr/share/icons`, `~/.local/share/icons`,
`~/.icons`, without `Infynitae-*` (made by `settings.sh:178`) and without cursor-only themes;
`cursors`: folders holding `cursors/` (`settings.sh:852`). Not `settings.sh list fonts`: it keeps only
names matching eight words (`settings.sh:866`). Hidden: Font rows without `fc-list`. Apps take a new
font or cursor at their next start; the row's `says` states it. Not known: which running apps follow
without a restart.

### `settings/display` (`cards/settings_display.rs`)

Source `screens` (`every`: None): `infynitae msg snapshot`, its `outputs` (name, make, model, mode,
scale, transform, refresh_mhz: `crates/infynitae-ipc/src/model.rs:71-79`), and `wlr-randr --json` for
each screen's list of modes when `wlr-randr` is installed.

One block per screen, titled "DP-1  2560x1440  165 Hz" (model as its second line):

| Row | Control | Key (`N`: the name, `-` as `_`) | Values | After |
|---|---|---|---|---|
| Resolution | Found (the screen's modes) | `OUTPUT_N_MODE` | | restart; at once too through `wlr-randr --output NAME --mode WxH` |
| Refresh rate | Found (rates of that mode) | `OUTPUT_N_RATE` | | same, `--mode WxH@R` |
| Scale | Pick | `OUTPUT_N_SCALE`; with one screen `OUTPUT_SCALE` | 1, 1.25, 1.5, 2 | same, `--scale` |
| M Turned | Pick | `OUTPUT_N_TRANSFORM` | Not turned `normal`, 90, 180, 270 | same, `--transform` |

Then, once:

| Row | Control | Key | Values | After |
|---|---|---|---|---|
| Variable refresh rate | Pick | `VRR_ENABLED` | In fullscreen `auto`, On, Off | restart |
| Night light warmth | Slide 3000-6500 K, step 100 | `NIGHT_LIGHT_TEMP` | | save |
| M Order, left to right | Text | `OUTPUT_ORDER` | names with commas | restart |
| M Sharper when zoomed in | Pick | `MAX_APP_SCALE` | Never `1`, 2x, 3x | save |
| M Zoomed windows | Pick | `SCALING_FILTER` | Sharp `nearest`, Smooth `linear`, Fast `fast` | restart |

A mode change through `wlr-randr` is followed by a card "Keep this? 10" in the edge's ask prompt
(`infynitae-edge ask`, choices Keep and Back); no answer in 10 s puts the old mode back and does not
write the key. Hidden: Resolution, Refresh rate and Turned without `wlr-randr` lists (the block then
shows what is set, and Scale writes the key and says "At the next start"); Order with one screen;
Refresh rate with one rate; Variable refresh rate when no mode list says the screen can (not known
how to read that without trying: `wlr-randr --json` has `adaptive_sync` only where the compositor
reports it). Left to the file: `ZOOM_SHARPEN`, `VIRTUAL_SCREEN`, `OUTPUT_LAYOUT`, `AUTO_ROTATE*`, `OSK`.
Not known without trying: whether the compositor's output management takes every mode `wlr-randr`
lists; whether night light re-reads the temperature while on. Brightness stays in `sound/volume`.

### `settings/input` (`cards/settings_input.rs`)

| Row | Control | Key | Values | After |
|---|---|---|---|---|
| Layouts | Text | `XKB_DEFAULT_LAYOUT` | "us,de"; empty: the system's | restart |
| Repeat starts after | Slide 150-600 ms, step 10 | `KEYBOARD_REPEAT_DELAY` | | save |
| Repeat speed | Slide 10-60 per second | `KEYBOARD_REPEAT_RATE` | | save |
| Pointer speed | Slide -1.0 to 1.0, step 0.05 | `MOUSE_ACCEL_SPEED` | | save |
| Acceleration | Switch | `MOUSE_ACCEL_PROFILE` | `adaptive` / `flat` | save |
| Tap to click | Switch | `TOUCHPAD_TAP` | | save |
| Scroll follows the fingers | Switch | `TOUCHPAD_NATURAL_SCROLL` | | save |
| M Super frees a captured pointer | Switch | `SUPER_FREES_CURSOR` | | save |
| M Switch layouts with | Pick | `XKB_DEFAULT_OPTIONS` | None `""`, Alt+Shift `grp:alt_shift_toggle`, Super+Space `grp:win_space_toggle`, Caps Lock `grp:caps_toggle`; a value with other options shows "Set in the file" and is not changed | restart |
| M Keys heard by X11 apps | Pick | `XWAYLAND_KEYS` | None `off`, Modifiers, Shortcuts, All | save |

Hidden: the two touchpad rows unless a device in `/proc/bus/input/devices` has "Touchpad" in its
name (source `touchpad`, a file read; a touchpad named otherwise is missed: known gap, the file still
works). "Switch layouts with" with one layout. Layouts is text in this stage: a list of the 100-odd
names in `/usr/share/X11/xkb/rules/evdev.lst` is a later step.

### `settings/windows` (`cards/settings_windows.rs`)

| Row | Control | Key | Values | After |
|---|---|---|---|---|
| Focus | Pick | `FOCUS_FOLLOWS_MOUSE` | Under the pointer `strict`, Under the pointer, kept over empty space `sloppy`, On a click `click` | save |
| A new window takes the focus | Pick | `FOCUS_ON_OPEN` | Unless you are busy `priority`, Only ones you opened `user`, Always `1`, Never `0` | save |
| A new window opens | Pick | `PLACEMENT_POLICY` | Beside the others `cluster`, In the middle `center`, At the origin `origin` | save |
| Super+Enter | Words | `SUPER_ENTER_ACTION` | Maximize, Fit | save |
| M The view follows a new window | Pick | `TELEPORT_ON_OPEN` | Only ones you opened `user`, Always `1`, Never `0` | save |
| M An app asks for the focus | Pick | `FOCUS_ON_ACTIVATE` | From you `user`, Always, Never | save |
| M Super+Tab moves the view | Switch | `FOCUS_CYCLE_NAVIGATE` | `1` / `0` | save |
| M Dragging the canvas | Pick | `PAN_MODE` | Pointer moves with it `free`, Pointer stays `grab`, Grip `rts` | save |
| M Pan out of fullscreen | Switch | `FULLSCREEN_PAN` | | save |

`KEEP_ON_TOP` is left to its key (Super+Shift+K writes it).

### `settings/defaults` (`cards/settings_apps.rs`)

| Row | Control | Key | Values | After |
|---|---|---|---|---|
| Terminal | Found | `TERMINAL` | "Automatic" `auto`, then `cx.apps` with `TerminalEmulator` in Categories; the app's `exec` written | use |
| Files | Found | `FILE_MANAGER` | `auto`, apps with `FileManager` | use |
| Browser | Found | `BROWSER` | `auto`, apps with `WebBrowser` | use |
| System monitor | Found | `SYSTEM_MONITOR` | `auto`, apps with `System` and `Monitor` | use |
| Reopen apps at login | Switch | `SESSION_RESTORE` | | Reload(`restore`) |
| M Open a terminal when nothing was reopened | Switch | `START_TERMINAL` | | restart |
| M Mount USB drives when plugged in | Switch | `AUTOMOUNT` | | Reload(`automount`) |
| M X11 apps and games | Switch | `XWAYLAND` | | restart |

No source: the lists come from `cx.apps`. A value that is no listed app's `exec` shows as written and
stays (a command of the owner's). "Automatic" shows what it resolves to only where that is cheap to
know; otherwise the word alone. Hidden: a row with no app of its category installed; "Mount USB
drives" without `udisksctl`. Left to the file: `LAUNCHER`, `MENU_PICKER`, `CLIPBOARD_PICKER`,
`CONTROL_MENU`, `POLKIT_AGENT`, `NOTIFICATIONS`, `ENABLE_EDGE`, `EDGE_IMPL`, `EDGE_POSITION`,
`EDGE_CONTROL_SIDE` (a reload of the edge closes the panel; a panel's edge is set by editing it).
Open: `BROWSER` is only what command-line programs run (`live/config.sh`); the browser other apps
open links with is `xdg-settings set default-web-browser`. This row should set both; that is the
owner's call (section 8).

### `settings/games` (`cards/settings_games.rs`)

| Row | Control | Key | Values | After |
|---|---|---|---|---|
| Game mode | Switch | `GAME_MODE` | | save |
| Hold notifications during a game | Switch | `GAME_QUIET` | | save |
| Tearing in fullscreen | Pick | `TEARING` | Always, When the game asks `true`, Never `false` | save |
| Slow hidden windows when the GPU is full | Switch | `GPU_RELIEF` | | save |
| M Lower priority for apps out of sight | Switch | `BACKGROUND_PRIORITY` | | save |
| M When frames are drawn | Pick | `LATENCY` | Direct, At once `off`, Balanced, Lowest | save |
| M Fullscreen straight to the screen | Switch | `DIRECT_SCANOUT` | | save |
| M A window filling the screen counts as fullscreen | Switch | `FIT_IS_FULLSCREEN` | | save |
| M Cursor on the GPU's layer | Switch | `HARDWARE_CURSOR` | | save |

Hidden: Game mode without `gamemoded` on PATH. Left to the file: `FRAME_GUARD`, `OVERLAY_PLANES`,
`LATENCY_STATS`.

### `settings/lock` (`cards/settings_lock.rs`)

| Row | Control | Key | Values | After |
|---|---|---|---|---|
| Screens off after | Pick | `IDLE_SLEEP_TIMEOUT_MIN` | 5, 10, 15, 30, 60 minutes, Never `0`; another number shows as it is | save |
| Lock when the screens go off | Switch | `IDLE_AUTO_LOCK` | | save |
| Lock before sleep | Switch | `SLEEP_LOCK` | | Reload(`sleep-lock`) |
| Stop charging at | Pick | `BATTERY_LIMIT` | Full `off`, 80 %, 90 % | save, then `sh "$HOME/infynitae/live/battery-limit"` |
| M Start at the lock screen | Switch | `START_LOCKED` | | restart |
| M Show "Shutting down" | Switch | `SHUTDOWN_SCREEN` | | use |

Hidden: "Stop charging at" unless a `/sys/class/power_supply/BAT*/charge_control_end_threshold`
exists (`live/battery-limit:12`). Not known without trying: how `battery-limit` is called to apply
now (read its head before building). Left to the file: `SCREEN_LOCKER`.

### `settings/privacy` (`cards/settings_privacy.rs`)

| Row | Control | Key | Values | After |
|---|---|---|---|---|
| When an app shares the screen | Pick | `SCREENSHARE_ASK` | Ask once per app `app`, Always pick what to share `true`, Never ask `false` | Reload(`screenshare`) |
| Apps always allowed to share | a list in place, each with Remove | the lines of `~/.local/state/infynitae/screenshare-allowed` | | at once (the line is deleted) |
| Hidden from captures | a list in place, each with Remove | `PRIVACY_APPS` | | save |
| Show that the screen is captured | Pick | `CAPTURE_INDICATOR` | In the edge `edge`, A dot `dot`, No `false` | save |
| Clipboard history | Switch | `CLIPBOARD_HISTORY` | | Reload(`clipboard`) |
| M Keep the clipboard after an app closes | Switch | `CLIPBOARD_PERSIST` | | Reload(`clipboard`) |
| M Cursor in captures | Switch | `SCREENCOPY_CURSOR` | | save |
| M Remote desktop | Switch | `REMOTE_DESKTOP` | | restart |

Source `shares`: that file's lines. Hidden: the two lists while empty; Clipboard history without
`cliphist` and `wl-paste`; the other clipboard row without `wl-clip-persist`. Left to the file: every
`ALLOW_*` list and `PRIVACY_TITLES` (a wrong word there breaks screenshots or clipboard managers; the
file has the explanation beside each).

### `settings/recording` (`cards/settings_recording.rs`)

| Row | Control | Key | Values | After |
|---|---|---|---|---|
| Replay keeps the last | Pick | `REPLAY_BUFFER_MINUTES` | 2, 5, 10, 20, 30 minutes | Reload(`recorder`) |
| Sound recorded | Pick | `RECORD_AUDIO` | The PC `desktop`, The microphone `mic`, Both, None | Reload(`recorder`) |
| Frames per second | Pick | `RECORD_FPS` | 30, 60, 120 | Reload(`recorder`) |
| Freeze the screen while picking a region | Switch | `FREEZE_SCREENSHOT` | | use |
| Screenshots, Clips, Recordings | three buttons | `SCREENSHOT_DIR`, `CLIPS_DIR`, `RECORDINGS_DIR` | each opens its folder (`Action::OpenUri`); the folders are changed in the file | |
| M Format | Pick | `RECORD_CODEC` | HEVC, AV1, H.264, Automatic | Reload(`recorder`) |
| M Recorded by | Pick | `REPLAY_BACKEND` | Automatic, gpu-screen-recorder, Infynitae | Reload(`recorder`) |

Replay on or off stays in `settings/quick`. The recorder is reloaded only while
`ENABLE_REPLAY_BUFFER` is true (else the key is written and nothing runs). Hidden: the
gpu-screen-recorder choice without it on PATH; the whole card without `infynitae-recorder`. Left to
the file: `REPLAY_DIR`, `SCREENSHOT_CMD`, `RECORD_NOTIFY`.

### `settings/about` (`cards/settings_about.rs`)

Source `pc` (`every`: None), each line left out when its file or program is missing:

| Row | From |
|---|---|
| the PC's name | `/etc/hostname` |
| System | `PRETTY_NAME` of `/etc/os-release`, the kernel from `/proc/sys/kernel/osrelease` |
| Processor | the first `model name` of `/proc/cpuinfo` |
| Memory | `MemTotal` of `/proc/meminfo`, in GB |
| Graphics | `lspci -mm` lines of class VGA or 3D, the device name (not shown without `lspci`) |
| Infynitae | `git -C "$HOME/infynitae" log -1 --format='%h %cs'` |
| Time zone | `timedatectl show -p Timezone --value` |

Then the actions: **Check for updates** (`Action::Run` of `infynitae-update` in the terminal:
`xdg-terminal-exec infynitae-update`), **Undo** (section 3; not drawn with an empty log), **Open the
config file**, **Restart needed** (drawn only when a restart key was changed since the compositor
started: the keys, and "Ctrl+Alt+Backspace restarts; it closes your windows"). Sizes: S (the name,
System, the actions), M (all). Not known without trying: `lspci` names for integrated graphics read
long; cut at the bracket.

## 5. The Settings panel, and the rest of the file

A fifth built-in panel beside the four of `config.rs:775-800`:

```
PANEL_SETTINGS="SUPER+I top center 440 apps/search=settings settings/all settings/about"
```

Super+I is free (Super+Shift+I is stay awake, `crates/infynitae-core/src/input/keys.rs:538`) and is
the key Windows uses. No hover. Any of the cards can also be put in any other panel by "Add a card"
(`settings/display:m` in the control panel).

**Open the config file** (a row of `settings/about`, and the last row of `settings/all`):
`Action::Run("sh \"$HOME/.local/bin/xdg-terminal-exec\" sh \"$HOME/infynitae/live/settings.sh\" edit")`
then `ClosePanel` (`settings.sh:921` runs `$EDITOR`, else nano). Saving the file applies as the tags
say; the cards read it again the next time the panel opens.

**In the file.** While something is typed, `settings/all` ends with the keys of `config.template`
that are on no card and whose name or comment matches: the key, its value, the first comment line. A
click opens the editor at that line (`+N` before the file name, for nano, vim, nvim, vi, micro; other
editors open the file's top). So every setting is found from one place, and the ones without a
control say where they are.

A test in the crate reads `config/config` and fails when a key is neither in a card's table nor in
the list `FILE_ONLY` in `settings_rows.rs`: a new setting cannot be added without deciding where it
goes.

## 6. Building order

1. Section 2 (Claude): the five fixes, `settings_rows.rs`, the two `ui.rs` parts, `log` and `unset`
   in `settings.sh`, the built-in panel. Done when `settings/games` (switches and one Pick only)
   passes its VM script.
2. One agy worker per card file of section 4, in this order of use: appearance + wallpaper + fonts,
   display, input, windows, defaults, lock, privacy, recording, about.
3. `settings/all` and "In the file" last: they need every table.

## 7. Better than the Windows Settings app: the checks

None of these is measured yet. Each is a test that prints a number or PASS.

| Claim | Check |
|---|---|
| It opens at once: no program starts, the edge is running | VM, as `tools/vm-edge/pfeel.py`: Super+I to the first drawn frame, 20 runs, the median and the worst. Target: under 50 ms. Windows' number to compare is not measured here; measure it on the owner's Windows drive before quoting one |
| Every setting is found from one field | For each key of `config.template`: typing the key gives a row or an "In the file" line. Target: all of them. Windows' search does not reach the registry or the old Control Panel pages |
| One place, no second app | The crate test of section 5: zero keys unassigned |
| A change applies without an Apply button | Per `[save]` row in the VM: time from the click to the effect (the compositor's log line or the changed state). Target: under 300 ms (the 250 ms check plus a frame) |
| Every change can be undone | VM: change 10 rows, press Undo 10 times, `cmp` the config with its copy from before. Target: the same bytes |
| It says before, not after, that a restart is needed | Render test: every `After::Restart` row shows "At the next start" after a change; a crate test that the tag in `config/config` and `After` agree for every key |
| It hides what the PC cannot do | VM without `wlr-randr`, `gamemoded`, a battery, a touchpad, `cliphist`: none of those rows is in `INFYNITAE_EDGE_BOXES`; no row says "not available" |
| It costs nothing while closed | `ps` RSS of the edge before the first open and after closing; sources stopped (no `fc-list`, `wlr-randr` in `ps` 5 s after closing) |
| The settings stay one readable file | After one change by a card: `diff` shows one line |

## 8. Decided here, and open

Decided here (the owner was not asked; each can change before step 1):

- One index card (`settings/all`) in a panel of its own, not eleven cards stacked: eleven S cards are
  about 2000 px tall and no panel scrolling was found in `shell.rs`.
- A setting with more than three values is a row with its value at the right and a list in place
  (variant A on the design page); two or three short words are a choice in one line only where it is
  the card's first row. Variant B shows the choice always.
- No Notifications, Sound, Keys or Panels card (section 1).
- Colours are typed as `#rrggbb`; no colour picker.
- Super+I, top centre, 440 px.

Open for the owner:

1. Variant A or B for rows with a few values (the design page shows both for Windows).
2. Should Browser also set the system's default browser (`xdg-settings`), so links from every app
   follow it?
3. A card for keys (`BIND_*`): wanted? It needs chord capture in the core.
4. A colour picker for the accent, or is typing the value enough, given "Colours from the wallpaper"?
5. Wallpaper thumbnails: wanted enough to add image decoding to the edge?
6. Should a display mode change ask "Keep this?" with a 10 s way back, as written, or apply without
   asking?
