# Polish plan: one look, plain words, less code, less waiting

Written 2026-11-13 from a read of branch `feat/edge-apps` (commit `4cb8855`). A review and a
plan; no code was changed. Nothing was built or run for it: every statement comes from reading
the files named, and from the owner's own `~/.local/state/infynitae/session.log` (the session
started 2026-11-12 22:37). "Not confirmed" marks what reading alone could not settle.

Sizes: S is under a day for one worker, M one to three days, L more. Paths are from the
repository root. `settings.sh` is `live/settings.sh`; `lock.c` is `live/lock/infynitae-lock.c`;
`polkit.c` is `live/polkit/infynitae-polkit.c`; `wl/` is `crates/infynitae-wayland/src/`;
`drm` is `crates/infynitae-backend-drm/src/lib.rs`; `edge/` is `live/edge-apps/src/`.

## What the compositor draws, so the scope is clear

The compositor draws windows, their borders and rounded corners, the cursor, the canvas colour
and the capture dot (`wl/convert.rs:749-823`, `1073`). It draws no title bars, no on-screen
display and no notifications of its own: volume and brightness are `notify-send` calls from
`live/launch.sh:374-503`, shown by the edge's notification server or by mako
(`docs/COMPONENTS.md:23`). The lock screen, wallpaper, password prompt and portal are separate C
programs. So "one look" is a question of five drawing parts: the compositor (borders, corners),
the edge, the lock screen, the password prompt, and the files `settings.sh` writes for GTK, Qt,
Firefox, terminals, mako and fuzzel.

---

## 1. One look

### 1.1 The shared values

The edge is the largest surface and the one the owner has been choosing by eye, so its values
are the reference. Everything else moves to them.

| Value | Is | Where it is defined today |
| :--- | :--- | :--- |
| **card** | fill of a panel card, dialog, pop-up, lock field | `edge/theme.rs:84`, from `SURFACE_COLOR` or the scheme |
| **edge** | the 1 px line around a card | `edge/theme.rs:86`, `133-136` |
| **text** | main text | `edge/theme.rs:87` |
| **muted** | secondary text: text mixed 32% toward the surface | `edge/theme.rs:142` |
| **accent** | what is on, chosen or asks for attention | `edge/theme.rs:91`, from `BORDER_ACTIVE` |
| **on accent** | text on an accent fill | `edge/theme.rs:144` |
| **accent as text** | the accent where it must read on a card (3:1) | `edge/theme.rs:95-97`, `110-116` |
| **track** | empty part of a bar or switch: text at 12% | `edge/theme.rs:146` |
| **pointed at** | text at 8% under the pointer | `edge/ui.rs:24-28` |
| **chosen** | accent at 16% behind the chosen list item and a list's value | `edge/ui.rs:580`, `378-379` |
| **corner** | `CORNERS`, else `WINDOW_CORNERS`, else 8 | `edge/config.rs:737-747` |
| **small corner** | corner x 0.55, rounded: buttons, fields, list rows | `edge/theme.rs:182-184` |
| **pill** | half the height: switches, sliders | `edge/ui.rs:280`, `463` |
| **spacing** | 12 between cards and parts, 18 x 16 inside a card, 24 from the screen's edge | `edge/draw.rs:7-9`, `edge/shell.rs:17-21` |
| **type** | 15 semibold title, 13 body, 12 controls and secondary, 11 notes, in px | `edge/draw.rs:133`, `edge/ui.rs:239`, `332`, `252` |
| **rows** | 40 px row, 32 px list row, 16 px icon, 30 x 16 switch | `edge/ui.rs:15-21` |
| **time** | 220 ms for a list, 200 ms for a panel, 180 ms for a pop-up | `edge/ui.rs:18`, `edge/shell.rs:23`, `edge/notify.rs:37` |

No alert colour exists in this set. `docs/NO_SLOP.md` names the tokens as surface, card, text,
muted and accent, and says the accent is what asks for attention. See item 1.6.

### 1.2 The look is resolved in one place but only a third of it is handed on

- **Wrong.** The edge prints 16 lines of resolved look (`edge/theme.rs:155-179`: card, edge,
  text, muted, accent, on accent, track, each with opacity, plus both corners). `settings.sh`
  reads them (`settings.sh:30-66`) and then saves only five to `~/.cache/infynitae/look`
  (`settings.sh:70-76`: corners, card, edge, text, accent). "Accent as text" is not printed at
  all. The lock screen, password prompt and screenshot helper read that file
  (`lock.c:342-361`, `polkit.c:259-281`, `live/shot-rs/src/look.rs:35-43`) and each works out
  the missing values again: the prompt computes its own small corner (`polkit.c:285`) and its
  own muted text as text at 62% (`polkit.c:301`), the lock uses text at 45% to 66%
  (`lock.c:766`, `873`).
- **Why it matters.** Secondary text is a different grey in each part, and a change to the
  edge's rules reaches the others only by hand.
- **Change.** `shell_lines` also prints `LOOK_ACCENT_TEXT` and `LOOK_FONT`; `look_save` writes
  every line it read; the three readers take muted, small corner and accent-as-text from the
  file and keep their built-in defaults only for a missing file.
- **Size** S. **Risk** low: readers ignore lines they do not know (`lock.c:351-360`).

### 1.3 A colour scheme reaches panels but not apps or windows

- **Wrong.** `THEME_SCHEME` other than "yours" changes the card and accent in the edge
  (`edge/theme.rs:124-131`), and through the look file mako, fuzzel, the lock and the prompt
  (`settings.sh:420-422`, `433`, `475`). GTK, Qt, Firefox and the terminals are written from
  the raw `SURFACE_COLOR`, `TEXT_COLOR` and `BORDER_ACTIVE` (`settings.sh:349-371`, `503`,
  `524`, `604-623`, `675-691`). The compositor's borders read `BORDER_ACTIVE` and
  `BORDER_INACTIVE` directly (`wl/lib.rs:1393-1401`). The config says so itself: the scheme is
  "the colours of the panels, widgets, notifications and menus" (`config/config:198-199`).
- **Why it matters.** With "tonal" or "vibrant" the panels are tinted and every app window,
  its border and the terminal keep the untinted colours: two palettes on one screen.
- **Change.** `settings.sh` writes GTK, Qt, Firefox and terminal files from `LOOK_CARD`,
  `LOOK_TEXT`, `LOOK_ACCENT` and `LOOK_EDGE`. For the window border the compositor reads the
  look file's accent and edge when the scheme is not "yours" (a file read; no new dependency,
  so `tools/verify-architecture`'s list for the adapter, lines 80-92, is untouched).
- **Size** M. **Risk** medium: every themed app changes colour for anyone not on "yours";
  `tools/test-settings-sync`, `test-qt-theme`, `test-firefox-theme` and `test-mako-config`
  hold the current output and need new expected values.

### 1.4 Schemes are dark only, and "light" reaches apps only

- **Wrong.** The five built schemes fix the card's lightness at 9% (`edge/theme.rs:125-129`)
  and take `TEXT_COLOR` unchanged (`edge/theme.rs:121`, `141`). `COLOR_SCHEME="prefer-light"`
  is read only for GTK and the portal (`settings.sh:338`, `live/portal/settings.c:98-102`).
  Colours from the wallpaper are dark only, by its own note
  (`live/wallpaper/theme-colors.h:12`).
- **Why it matters.** Choosing light apps leaves panels, lock and prompt dark; a hand-made
  light theme (dark `TEXT_COLOR`) with any built scheme gives dark text on a 9% card.
- **Change.** The resolver takes light or dark as an input: light gives a card near 96%
  lightness, dark text, and the same contrast checks (`edge/theme.rs:105-116`). Detail in
  section 2, group C.
- **Size** M. **Risk** medium: a light edge has never been looked at; needs pictures from
  `--render` (`edge/main.rs:34`) reviewed by the owner before it is offered.

### 1.5 The edge's own kit still has the parts the owner chose against

The owner's choice today: a list's value is a soft accent block like the chosen option in the
open list; no outlined boxes; no big two-button choices.

| Part | File:line | State | Change |
| :--- | :--- | :--- | :--- |
| `ui::value_box` | `edge/ui.rs:356-402` | follows the choice | drop the unused `_changed` argument (`ui.rs:362`) |
| `ui::list_row` | `edge/ui.rs:565-622` | follows it (accent 16%) | none |
| `ui::choice` | `edge/ui.rs:404-454` | outlined boxes side by side, accent 18%; 4 callers: `cards/settings.rs:949`, `settings_rows.rs:524`, `settings_look.rs:1838`, `3021` | each caller becomes a row whose value opens a list; then delete `choice` |
| `ui::button` | `edge/ui.rs:329-352` | a 1 px outline; 15 callers in 10 card files | no outline: the "pointed at" fill at rest (text 8%), deeper when pointed at; same height and text |
| hand-drawn lines | 80 `stroke()` calls outside `ui.rs`, most in `settings_look.rs` (11), `session.rs` (9), `music.rs` (8) | not checked one by one | review each: icon strokes stay, outlines around controls go |
| type sizes | 11.5 px 15 times, 12.5 three, 13.5 twice, 14 and 10.5 once each, beside the four in 1.1 | counts from `grep 'size: '` over `edge/` | fold into 11, 12, 13, 15 and name them as constants in `draw.rs` |
| fills | 22 different opacities in `.alpha(...)` calls; 0.16 and 0.18 both mean "chosen" (`ui.rs:433`, `580`) | counts from grep | named constants for pointed-at, chosen, track; one value each |

- **Why it matters.** The same control looks different from card to card, which is what the
  owner sees as "not one thing".
- **Size** M (the kit S, the callers M). **Risk** low in code, visible everywhere: the render
  pictures (`tools/test-edge-render`) and the VM picker and settings scripts
  (`tools/vm-edge/ppicker.py`, `psettings.py`) are the check.

### 1.6 The lock screen

| What | File:line | Diverges how | Change |
| :--- | :--- | :--- | :--- |
| wrong password, Caps Lock | `lock.c:814`, `843`, `880`, `883` | two fixed colours not from the theme | accent as text plus the words already there; or one alert role added to the theme (owner decides, see below) |
| on-screen keyboard keys | `lock.c:535` | a fixed grey-blue | card and edge from the look file |
| keyboard font | `lock.c:528`, `552` | "Sans 14" where the rest says "sans-serif" | the look's font |
| type sizes | `lock.c:763-767`, `853-884` | in points (11, 10.5, 13.5), the edge in px | px: 13 field, 11 notes, 15 date; the clock stays large |
| background without a wallpaper | `lock.c:706-712` | a radial wash of the surface colour | flat canvas colour (`NO_SLOP.md`: no gradient washes) |
| field corner | `lock.c:105`, `356-360` | the full corner | small corner, as fields elsewhere |

Owner decision needed: errors in the accent with words (what the password prompt already
does, `polkit.c:305-308`), or one alert colour in the theme. The plan assumes the first: it
adds nothing.

- **Size** S. **Risk** low; `tools/test-lock-render` draws every state without locking.

### 1.7 The password prompt

- **Wrong.** A GTK 3 window with its own style sheet (`polkit.c:285-345`): font "Sans"
  (`:293`), sizes 13/11/12 px, two outlined buttons side by side (`:320-342`), padding 6 x 10
  and 6 x 14. It is the only part that needs GTK 3 to build
  (`live/build-helpers.sh:45`).
- **Change, step 1 (S).** Take muted and small corner from the look file; no outline on the
  buttons; the confirm button is the soft accent block.
- **Change, step 2 (M).** Ask through the edge's question card, which already takes a secret
  (`edge/ask.rs:1`, `edge/main.rs:178`), keeping the GTK window as the fallback when no
  edge runs; later drop it. Then the prompt is drawn by the same code as every panel.
- **Risk** step 1 low. Step 2 medium: a password crosses a pipe to another process; the VM
  script `tools/vm-edge/pmic.py` covers the prompt and `pask.py` the secret.

### 1.8 What apps get from `settings.sh`

| What | File:line | Diverges how | Change |
| :--- | :--- | :--- | :--- |
| GTK button and field corner | `settings.sh:327`, `387`, `394` | three quarters of the corner (8 gives 6) where the edge and prompt use 0.55 (8 gives 4) | `LOOK_SMALL_RADIUS` |
| GTK hover | `settings.sh:389` | text at 14%, the edge 8% | 8% |
| GTK list selection | `settings.sh:397-399` | a solid accent fill for rows and text selection alike | rows: accent 16% with normal text; text selection stays solid |
| GTK animations | `settings.sh:339` | forced off, while the edge animates | section 2, group E |
| mako | `settings.sh:430`, `438-441` | 10 pt font, margin 12, padding 12 x 14; the edge's own pop-ups are 360 wide with margin 24 (`edge/notify.rs:25-29`) | margin 24, padding 16 x 18, the look's font. Only seen when `NOTIFICATIONS` is not the edge (`config/config:575-580`) |
| fuzzel | `settings.sh:466`, `479` | 11 pt; selection accent at 18% | the look's font; 16% |
| terminal palette | `settings.sh:505-512` | seven fixed colours, only blue follows the accent (`:509`) | leave: red must stay red in a terminal. Say so in the config comment |
| terminal selection | `settings.sh:527`, `563` | fixed `#262b38` | accent mixed 30% into the surface |
| terminal padding | `settings.sh:499`, `529`, `546` | 8, 6, 8 | 8 |
| fallback colours | `settings.sh:44-45` | `#1e2030`, `#e6e6e6`, opacity `f7` differ from the defaults at `:105-109` and `CARD_OPACITY` 90 | the same defaults |

- **Size** S. **Risk** low; visible in every GTK app, so one before/after picture for the owner.

### 1.9 The compositor's own colours

- `wl/convert.rs:693` `CANVAS_CLEAR` is (0.11, 0.11, 0.13); the config's default canvas is
  `#0b0f14` (`config/config:151`). Seen only before the config is read. Make them equal. S.
- `wl/lib.rs:803` the default inactive border has opacity 0.8; a value read from the config has
  1.0 (`wl/lib.rs:2239`). Pick one. S.
- `wl/convert.rs:1073` the capture dot is a fixed red. It is a warning mark documented as red
  (`config/config:719`); leave it, as the one fixed colour.
- Corner limits differ: windows 0 to 24 (`wl/lib.rs:1354`, `1541`), the edge and
  `settings.sh` 0 to 28 (`edge/config.rs:739`, `settings.sh:41`), the config says 0 to 24 and
  0 to 28 (`config/config:190`, `196`). One limit, 24. S.

### 1.10 Font

`FONT_NAME` reaches GTK and Qt only (`settings.sh:336`, `719`). Everything Infynitae draws
names "sans-serif" itself: `edge/draw.rs:10`, `lock.c:763`, `polkit.c:293`,
`settings.sh:430`, `466`, `live/shot-rs/src/draw.rs:86`. With the default they agree; with a
chosen font, apps change and the desktop does not. Change in section 2, group D. S.

---

## 2. One setting, many parts

The rule for every group: the person sees one row. Old keys keep working, unchanged, as
overrides written only in the file. A config without the new key behaves exactly as today,
because the new key's missing value is worked out from the old ones. Keys shown on no card
are listed in `FILE_ONLY` (`edge/cards/settings_rows.rs:173`), and a test there fails when a
key is on neither a card nor that list (`settings_rows.rs:1046-1061`), so the split is
enforced.

The Appearance card has 13 rows today (`edge/cards/settings_look.rs:795-917`), Games 9
(`settings_games.rs:24-105`), Lock and sleep 6 (`settings_lock.rs:26-83`).

### A. Corners: already one, with loose ends

- **One setting:** "Corners" (`WINDOW_CORNERS`, the row at `settings_look.rs:822-823`).
- **Drives:** windows (`wl/lib.rs:1352-1360`, `1541-1543`), the edge
  (`edge/config.rs:737-747`), the look file (`settings.sh:34-43`) and from it GTK
  (`settings.sh:327`), mako (`:437`), fuzzel (`:486`), the lock (`lock.c:356-360`), the prompt
  (`polkit.c:276-280`), the screenshot helper (`live/shot-rs/src/look.rs:41`).
- **Overrides in the file:** `CORNERS` (other corners for Infynitae's own parts,
  `config/config:192-196`) and `SQUARE_CORNERS` (`:176`). Both are file-only already
  (`settings_look.rs:2`).
- **Left to do:** one limit (item 1.9); GTK takes the small corner (item 1.8).
- **Migration:** none.

### B. Accent: one colour under a border's name

- **Today:** `BORDER_ACTIVE` is "border of the focused window, and the accent colour apps
  use" (`config/config:161-162`). The Settings row already says "Accent"
  (`settings_look.rs:804-805`).
- **One setting:** `ACCENT`. Drives the focused border (`wl/lib.rs:1393-1397`), the edge
  (`edge/config.rs:735`), apps through the portal (`live/portal/settings.c:103-106`), GTK
  (`settings.sh:353`, `371`), Qt (`:687`), Firefox (`:615-623`), folders
  (`settings.sh:216`), terminals (`:509`, `:526`), lock and prompt through the look file.
- **Override in the file:** `BORDER_ACTIVE`, for a focused border that differs from the accent.
- **Migration:** no `ACCENT` line means `ACCENT` is `BORDER_ACTIVE`. Not a rename in
  `live/config-renames`: the old key keeps its own meaning.
- **Do it after 1.2 and 1.3:** then only the resolver, the portal and the compositor read
  the key, not nine places in four languages.
- **Card:** one row, as now. **Size** S once 1.3 is in. **Risk** low.

### C. Colours: six rows to three

- **Today:** "Apps look" (`COLOR_SCHEME`), "Panels' colours" (`THEME_SCHEME`), "Background"
  (`SURFACE_COLOR`), "Text" (`TEXT_COLOR`), "Other windows' border" (`BORDER_INACTIVE`),
  "Behind the windows" (`CANVAS_COLOR`), "Colours from the wallpaper"
  (`THEME_FROM_WALLPAPER`), "Apps follow these colours" (`THEME_SYNC`):
  `settings_look.rs:795-917`.
- **One setting each, three rows:**
  1. **"Dark or light"** (`COLOR_SCHEME`, kept). Drives apps as now and, new, the resolver's
     card and text when they are not set by hand (item 1.4).
  2. **"Colours"** (`THEME_SCHEME`, kept; the row is renamed from "Panels' colours"). Drives
     everything after item 1.3, not panels alone. "yours" stays the value that means "use
     the colours in the file as they are".
  3. **"Colours from the wallpaper"** (kept).
- **Overrides in the file:** `SURFACE_COLOR`, `TEXT_COLOR`, `BORDER_INACTIVE`,
  `CANVAS_COLOR`. With "yours" they are used as written, as today. With a built scheme the
  canvas and the inactive border are worked out from the card (the wallpaper helper already
  has that rule: canvas darker than the surface, border the surface lightened toward the
  accent, `live/wallpaper/theme-colors.h:9-11`).
- **`THEME_SYNC`** moves to the file: it is a switch for people who keep their own GTK or
  terminal files, and each managed file can already be kept by deleting its "managed by
  infynitae" line (`settings.sh:342-345`).
- **Migration:** existing configs have `THEME_SCHEME="yours"` or no line, and "yours" reads
  the four colour keys as today (`edge/theme.rs:130-131`). Nothing changes until the person
  picks another value.
- **Card:** Appearance goes from 13 rows to 8: Dark or light, Accent, Colours, Colours from
  the wallpaper, Corners, Card opacity, Window border, Folders in the accent. The colour
  pickers for background and text stay reachable under "yours" (the any-colour tab from
  commit `94de896`) or in the file.
- **Size** M (with 1.3 and 1.4). **Risk** medium, as 1.3 and 1.4.

### D. Font: one family; size is not one setting

- **One setting:** "Font" (`FONT_NAME`, kept). Drives GTK and Qt as now
  (`settings.sh:336`, `719`) and, new, the look file's `LOOK_FONT`, read by the edge
  (`edge/draw.rs:10`), lock, prompt, mako, fuzzel and the screenshot helper (item 1.10).
- **Stays separate, for a reason:** `TERMINAL_FONT` (`config/config:327-329`): a terminal
  needs a fixed-width font and the interface does not.
- **Not one setting:** `FONT_SIZE` is in points and scales app text (`config/config:332`).
  The edge's layout is in fixed px (`edge/ui.rs:15-21`); scaling its text without its rows
  would clip. The one size for everything is `OUTPUT_SCALE` (`config/config:398`), which
  already has per-screen overrides (`:467-468`). Leave `FONT_SIZE` for apps and say so in
  its comment.
- **Migration:** none; the default "sans-serif" is what is hard-coded today.
- **Card:** "Fonts, icons, cursor" keeps its rows. **Size** S. **Risk** low: a font with wide
  letters can clip fixed-width labels; the render pictures show it.

### E. Motion: no setting exists, and the parts disagree

- **Today:** GTK animations are forced off (`settings.sh:339`); the edge animates lists,
  panels and pop-ups (`edge/ui.rs:18`, `edge/shell.rs:23-26`, `edge/notify.rs:37`); the
  wallpaper's depth settles over about a second (`config/config:388-391`). No key covers
  them (no `MOTION` or `ANIMATION` key in `config/config`).
- **One setting:** `MOTION="on"` or `"off"`. Drives the edge's durations (off: zero),
  `gtk-enable-animations`, and the wallpaper's settle.
- **Migration:** a missing `MOTION` keeps today's mix exactly (edge on, GTK off). Writing the
  key is what makes them agree.
- **Owner decision:** `NO_SLOP.md` says no setting nobody asked to change. If the owner
  does not want the switch, the lean answer is a fixed choice: turn GTK's animations on to
  match the edge, no key. The plan assumes the fixed choice. **Size** S. **Risk** low.

### F. Games: nine rows to three

- **Today:** `GAME_MODE`, `GAME_QUIET`, `TEARING`, `GPU_RELIEF`, `BACKGROUND_PRIORITY`,
  `LATENCY`, `DIRECT_SCANOUT`, `FIT_IS_FULLSCREEN`, `HARDWARE_CURSOR`
  (`settings_games.rs:24-105`).
- **One setting:** "Game mode" (`GAME_MODE`, kept). On: the CPU governor through gamemode
  (`config/config:272-274`), notifications held (`GAME_QUIET`, `live/launch.sh:121-125`),
  hidden windows slowed when the GPU is full (`GPU_RELIEF`, `config/config:284-288`).
- **Rows kept:** Game mode, Tearing in fullscreen, Lower priority for apps out of sight (it
  also applies with no game running, `config/config:291-293`).
- **Overrides in the file:** `GAME_QUIET` and `GPU_RELIEF` (each can still be set to false on
  its own), and the tuning keys `LATENCY`, `DIRECT_SCANOUT`, `FIT_IS_FULLSCREEN`,
  `HARDWARE_CURSOR`: their defaults are the measured best, and the row names needed GPU
  words to explain them.
- **Migration:** all keys read as today; only the card changes. **Size** S. **Risk** low.

### G. Lock: two rows to one

- **Today:** "Lock when the screens go off" (`IDLE_AUTO_LOCK`) and "Lock before sleep"
  (`SLEEP_LOCK`): `settings_lock.rs:43-53`.
- **One setting:** "Lock when away" (new key `LOCK_WHEN_AWAY`). Drives the idle lock
  (`drm:1737-1740`, `1775`) and `live/infynitae-sleep-lock`.
- **Overrides in the file:** the two old keys. **Migration:** a missing `LOCK_WHEN_AWAY`
  means each old key decides for itself, as today.
- **Stays separate:** `START_LOCKED` (the login, a different moment).
- **Size** S. **Risk** low, but it is the lock: `tools/test-sleep-lock` and a VM round.

### H. Already one setting, leave alone

- **Cursor theme and size:** read by the compositor (`wl/lib.rs:1192-1201`, `1586-1601`)
  and passed to X11, GTK and Qt by `settings.sh:299-300`, `311-318`, `334-335`. Two rows for
  two different things.
- **Keyboard repeat:** delay and rate, read by the compositor (`wl/lib.rs:587-612`,
  `1480-1490`) and exported for apps (`live/env.sh:124-125`).
- **Recording quality:** `RECORD_FPS`, `RECORD_CODEC`, `RECORD_AUDIO` already serve both the
  replay and a recording (`config/config:817-825`).
- **Which program does a job** (`LAUNCHER`, `CONTROL_MENU`, `MENU_PICKER`,
  `CLIPBOARD_PICKER`, `NOTIFICATIONS`, `POLKIT_AGENT`, `SCREEN_LOCKER`): all "auto" by
  default, all file-only already (`settings_rows.rs:189-204`), and `ENABLE_EDGE="false"`
  already moves the "auto" ones to their fallbacks (`config/config:587-591`).

### Where one value is wrong

- Window corner and control corner: a 4 px button inside an 8 px card is the design.
- Interface font and terminal font; app text size and the edge's sizes (group D).
- Card opacity (`CARD_OPACITY`, 90) and terminal opacity (`settings.sh:502`, 97): text in a
  terminal needs the more solid background.
- Canvas and surface: the canvas is deliberately darker, so windows stand off it.
- Tearing and variable refresh: different trade-offs, chosen per game.
- No spacing setting: spacing is part of the design, fixed in 1.1.

---

## 3. Wording

The setting names on the edge's cards are already plain ("A new window opens", "Stop
charging at"). The worst texts are in the scripts and the password prompt.

| Where | Now | Replace with |
| :--- | :--- | :--- |
| `live/launch.sh:656` | "Idle Inhibit: ON" / "Stay Awake enabled (display sleep suppressed)" | "Stay awake is on" / "The screens stay on until you turn it off" |
| `live/launch.sh:662` | "Idle Inhibit: OFF" / "Display sleep restored (normal timeout)" | "Stay awake is off" / "The screens turn off after the usual time" |
| `live/launch.sh:627`, `633` | "Privacy Mode: ON" / "Window hidden from screen share & capture"; "Privacy Mode: OFF" | "This window is hidden from captures" / "This window shows in captures again" |
| `live/launch.sh:645`, `650` | "Tearing: ON" / "Tearing: OFF" | "Tearing is on for this window" / "Tearing is off for this window" |
| `live/launch.sh:262` | "Infynitae Reloaded" / "Configuration re-applied from ~/.config/infynitae/config" | "Settings reloaded" |
| `live/launch.sh:276`, `285`, `313`, `349` | "Screenshot Saved", "Region Screenshot Saved" | "Screenshot saved" for both, with the file |
| `live/record/infynitae-recorder:340`, `388` | "Clip Saved" | "Clip saved" |
| `live/record/infynitae-recorder:345` | "Replay Buffer Inactive" / "No replay buffer directory found." | "The replay is off" / "Turn it on in Settings, Screenshots and recording" |
| `live/record/infynitae-recorder:358`, `373` | "Replay Buffer Empty" / "Still buffering first segment…", "Waiting for segments..." | "Nothing to save yet" / "The replay started a moment ago. Try again in a few seconds." |
| `live/record/infynitae-recorder:423`, `436` | "Recording Started", "Recording Saved" | "Recording" / "Recording saved" |
| `polkit.c:442`, `458` | "Authentication Required — Infynitae", "Authentication required" | "Password needed" |
| `polkit.c:463` | "Privilege escalation requested." / "Action: org.…" | the program's own message; without one, "A program asks to change the system". The action id stays as the quiet second line, without "Action:" |
| `polkit.c:507` | "Authenticate" | "Allow" |
| `polkit.c:194` and `lock.c:881` | "Authentication failed. Try again." and "Wrong password, try again" | "Wrong password, try again" in both |
| `settings.sh:853` | "==> Settings synchronized successfully across all toolkits." | "Applied." |
| `config/config:364-366` | "Synchronize the palette above to Mako, Fuzzel, GTK 3/4, Qt…" | "Apps follow these colours (GTK, Qt with qt6ct, Firefox, terminals, mako, fuzzel)", the card's own words (`settings_look.rs:917`) |
| `config/config:354-360` | "Primary text color across panels…" ("color") | "colour", as the rest of the file (`:198`, `:370`) |
| `live/infynitae-automount:190` | "Unlock it with: udisksctl unlock -b /dev/…" | ask for the passphrase with the edge's question card (the script already reports "Wrong passphrase", `:206`); the command only when no edge runs |

Words that mean two things:

- **"fit"** is the wallpaper's mode (`settings_look.rs:940-941`), the camera's zoom to a
  window (`config/config:133-137`) and `FIT_IS_FULLSCREEN`. Rename the wallpaper row to
  "Placement".
- **"control menu", "control panel", "Quick settings"** for Super+A and its content
  (`config/config:562`, `:39`, `:844`; `edge/cards/quick.rs:15`). Use "control panel" for
  the panel, "Quick settings" for the card, and "control menu" only for the fuzzel fallback
  script.
- **Capital letters:** the scripts use Title Case, the edge sentence case. `NO_SLOP.md`
  asks for sentence case; the table above applies it.

Row names that still need a hardware word, all moved to the file by section 2 group F:
"Cursor on the GPU's layer" (`settings_games.rs:105`), "Fullscreen straight to the screen"
(`:87`), "When frames are drawn" (`:73`). Others to rename: "Cards" for opacity
(`settings_look.rs:845-846`) to "Card opacity"; "Apps look" (`:795-796`) to "Dark or light";
"Recorded by" (`settings_recording.rs:96`) to "Recorder".

**Size** S for the table. **Risk** low; `tools/test-launch-actions` may hold some of these
texts (not confirmed) and would need the new ones.

---

## 4. Lean

### 4.1 Remove now (no behaviour changes)

| What | Evidence | Size |
| :--- | :--- | :--- |
| `live/lock/xdg-shell*` and `live/lock/wlr-layer-shell*` (six files, 5,232 lines) | the lock includes only `ext-session-lock-v1-protocol.h` (`lock.c:36`) and is built from two sources (`live/build-helpers.sh:46-47`); nothing else names these files; they are byte-for-byte copies of the wallpaper's (`cmp`) | S |
| `live/edge-apps/src/config.rs.orig` | a tracked leftover of a patch (`git ls-files`) | S |
| `#![allow(dead_code)]` for the whole new edge | `edge/main.rs:8`, with a comment that the Wayland side "comes next" (`:4-7`); it exists (`edge/wayland.rs`, 1,557 lines). Remove the line, then delete what the compiler names. Also `edge/draw.rs:114` and `ui.rs:362` | S to M, not known until built |
| stale statements | `crates/infynitae-wayland/Cargo.toml` says "no [patch]" while the root has one (`Cargo.toml:27-28`); `docs/COMPONENTS.md:22` says notifications default to mako, the config says the edge (`config/config:575-580`); `docs/COMPONENTS.md:25` and `config/config:593-597` describe three edges | S |

### 4.2 Remove when the new edge replaces the old ones

Blocking first: the desktop widgets and the `calendar`, `memory` and `system` modules exist
only in the Rust edge (`config/config:619`, `656-661`). The owner decides whether widgets are
ported or dropped before `live/edge-rs` goes. Editing a panel ("Add a card") is also still
open in the new edge (`docs/EDGE_APPS.md:3`).

| Goes | Evidence |
| :--- | :--- |
| `live/edge` (the C edge, 6,101 lines plus headers) and `live/edge-rs` (1.1 MB of source) | both frozen (`live/edge/FROZEN`, `live/edge-rs/FROZEN`) |
| their builds | `live/build-helpers.sh:53-55`, `79` |
| `live/edge-pick` and `EDGE_IMPL` | `live/edge-pick:11-31`, `config/config:593-597`, `wl/lib.rs:1188-1190`, `settings.sh:47-49`, `live/infynitae-reload:206-207`, `258-259`, `live/session.sh:184-185`. Retire the key with a "-" line in `live/config-renames` |
| CI jobs `old-edges-frozen` and `edge-rs` | `.github/workflows/ci.yml:76-103`, `107-130` |
| tests of the old edges | `tools/test-edge-rs`, `tools/test-edge-pick`; `tools/test-edge-app-search`, `test-edge-config-value` and `test-edge-child-signals` name no edge binary and most likely compile the C edge's headers (`live/edge/*.h`): not confirmed, check before deleting |
| old names in the new edge | its runtime files are called `infynitae-edge-rs.ask`, `.features`, `.ask-cancel` (`edge/ask.rs:36-39`), and `live/infynitae-format-drive:53-54` reads two features files |
| installed binaries | `infynitae-edge-c` 212 KB and `infynitae-edge-rs` 3.6 MB in `~/.local/bin` (`ls`, today) |

Also in `~/.local/bin`, outside the repository: `infynitae.before-frames` (11 MB) and
`infynitae-wallpaper.before-apps`, backups from earlier trials. The owner can delete them by
hand once the current builds are trusted.

### 4.3 Config parsing: eight readers, three sets of rules

| Reader | File:line | How |
| :--- | :--- | :--- |
| session scripts | `live/config.sh:8-21` | the file is run as shell |
| `settings.sh` | `settings.sh:79-81` | run as shell, without `config.sh`'s check for a broken line and without the renames (`live/config.sh:24-32`) |
| `settings.sh set` | `settings.sh:140-150`, `804-848` | awk, twice |
| `edge-pick` | `live/edge-pick:11` | sed |
| compositor | `wl/lib.rs:1996` `config_line`, used at `wl/lib.rs:1162`, `drm:1726`, and by `crates/infynitae/src/main.rs:392`, `restore.rs:792`, `wl/screencopy.rs:159` | Rust, line by line |
| new edge | `edge/config.rs:590`, `640` | Rust, its own |
| lock, prompt, portal, wallpaper | `lock.c:373-390`, `polkit.c:215-227`, `live/portal/settings.c:98`, `live/wallpaper/infynitae-wallpaper.c` | C, four copies |
| screenshot helper | `live/shot-rs/src/look.rs:133` | Rust, a third hex parser |

What goes wrong today: a switch written `"ON"` or `"True"` is on for the compositor, which
lowers the case (`wl/lib.rs:1375-1378`), and off for the shell, which accepts six exact
spellings (`live/config.sh:82-87`); `GAME_QUIET` is compared with the exact word `true`
(`live/launch.sh:125`). The compositor alone has 39 copies of the yes/no match
(`grep` over `crates/*/src`).

- **Change.** Not one shared library: the compositor's crates may not gain dependencies
  (`tools/verify-architecture:80-92`) and the helpers are in three languages. Instead:
  1. One yes/no function in `wl/lib.rs` beside `config_line`, used by all 39 places. S.
  2. `infy_on` lowers the case; `launch.sh` uses it. S.
  3. One file of cases (`tools/config-cases`: a line, the key and value it must give, on or
     off) that the compositor's test, the edge's test and a shell test all read. A new
     spelling then fails in whichever reader disagrees. S.
  4. After 1.2 the lock, prompt and screenshot helper read only the look file, and their
     config parsers go. S.
- **Risk** low. A config line with `$(...)` is still run by the shell readers and read as
  text by the others; that difference stays and should be said in the config's header.

### 4.4 Changing a setting does everything

- **Wrong.** The edge changes a setting by starting `settings.sh` twice (`log`, then `set`:
  `edge/cards/settings_rows.rs:151-157`). `set` then runs the whole apply with `--force`
  (`settings.sh:851-852`): GTK 3 and 4, mako, fuzzel, three terminals, every Firefox
  profile, Qt 5 and 6 with 17 awk starts for the palette (`settings.sh:670-690`), the
  folder icons, for any key, also `TOUCHPAD_TAP`.
- **Why it matters.** Process starts and file writes on every click in Settings, and the
  compositor's reload that follows stalls it (item 5.4).
- **Change.** `set` takes the old value and logs it itself (one start); the apply runs only
  when the key is one of the look keys; the Qt palette is one awk call.
- **Size** S to M. **Risk** medium: a key missing from the look list would not reach apps
  until the next login; `tools/test-settings-actions` and `test-settings-sync` cover it.

### 4.5 Builds and binaries

- Three Rust programs outside the workspace each have their own lock file and target
  folder and compile the same cairo, pango, zbus and Wayland client crates
  (`live/edge-apps/Cargo.toml`, `live/edge-rs/Cargo.toml` with identical dependencies,
  `live/shot-rs/Cargo.toml`; `live/build-helpers.sh:79-81`). After 4.2 two remain: put
  them in one workspace under `live/` with one target folder, so an install compiles the
  shared crates once. It is a second workspace; the root one that
  `tools/verify-architecture` checks is untouched. S.
- The compositor is built with full link-time optimisation but not stripped
  (`Cargo.toml:33-35`); it is 11.3 MB installed, the edge 4.1 MB with `strip = true`
  (`live/edge-apps/Cargo.toml`). `strip = "debuginfo"` keeps function names for crash
  reports. The size gain is not measured. S.
- The password prompt is the only user of GTK 3 at build time (`live/build-helpers.sh:45`);
  item 1.7 step 2 removes it.
- `live/control-menu` (406 lines) is the fuzzel fallback for `ENABLE_EDGE="false"`. It is
  not dead; keep it for PCs without the edge.

---

## 5. Speed and resource use

From `~/.local/state/infynitae/session.log`, read today, screen at 165 Hz (6.06 ms a
refresh):

- "input to screen p50 11.52 ms, p95 12.09 ms (155 frames)"; other 10 s windows show p50
  6.0 to 11.2 ms.
- "refresh to submit p50 0.06 ms … submit to refresh p50 5.98 ms"; "budget 0.13 ms".
- "715 of 41069 frames missed their refresh" (1.7%).
- 206 "Slow turn" lines: 70 name element building (8 to 10 ms each), at least 38 name
  captures (8.4 to 8.9 ms), 10 screen rescan (about 9 ms), 9 "config and idle" (two of them
  92 ms).

Ranked by expected gain. None of the gains is measured; each item says how to measure it.

### 5.1 Desktop frames are drawn 6 ms before they are shown

- **Wrong.** `LATENCY="direct"` draws at once unless a game is scanned out
  (`config/config:234-241`; `drm:2180-2181`). The log shows the result: a frame is handed
  over 0.06 ms after a refresh and waits 5.98 ms for the next, while drawing it takes
  0.13 ms. Whatever the mouse did in those 6 ms is not in the picture.
- **Why it matters.** It is most of the 6 to 11.5 ms between hand and screen on the desktop.
- **Change.** In "direct", composited frames also start late when the measured budget is
  small (under 1 ms, say) and no refresh was missed lately. The scheduler already widens
  its margin on a miss, narrows it again and pauses itself when misses persist
  (`wl/frame_schedule.rs:296-314`, `65-78`). The config warns that late drawing misses more
  while the GPU is busy (`config/config:238-240`), so it stays off while `GPU_RELIEF` is
  acting or a game is on another screen.
- **Expected gain:** up to about 4 ms off input to screen on the desktop (an estimate from
  the two numbers above).
- **Measure:** `LATENCY_STATS` lines before and after, same desktop use: input-to-screen
  p50 and the missed count.
- **Size** M. **Risk** medium: more missed refreshes if the guard is wrong. The pure logic
  is unit-tested in `wl/frame_schedule.rs:590-750`; add cases there first.

### 5.2 The recorder's captures run on the compositor's one thread

- **Wrong.** `serve_captures` draws and reads back on the main thread before each frame
  (`drm:2136-2151`); at least 38 slow turns of 8.4 to 8.9 ms name it. Already item 2 in
  `docs/research/PERFORMANCE-LINUX.md:229`.
- **Change.** As written there: the recorder takes dmabuf frames and encodes on the GPU;
  until then, 30 frames a second while game mode is on.
- **Size** M to L. **Risk** medium (two programs and a protocol path).

### 5.3 Element building stalls for 8 to 10 ms

- **Wrong.** 70 slow turns name "element building" (`drm:2231`), the step that builds each
  frame's list (`wl/convert.rs:749-823`). The cause is not confirmed. Open since the
  earlier review (`docs/research/PERFORMANCE-LINUX.md:234`).
- **Change, step 1 (S).** Finer marks inside it: cursor, layers above, windows, layers
  below, and the wrap at `drm:2218-2230`. One session then names the part.
- **Step 2.** Fix what the marks show. Size not known.
- **Risk** step 1 none beyond log lines (only written when a turn is slow, `drm:3077`).

### 5.4 Each settings change stalls the compositor

- **Wrong.** Nine slow turns name "config and idle", two at 92 ms: about 15 refreshes
  without a frame. That step holds the config reload (`drm:1712-1746`, `wl/lib.rs:1070`
  onward), a second read of the same file (`drm:1726-1741`) and the start of `config-check`
  (`wl/outputs.rs:342-344`). Which of them is slow is not confirmed.
- **Change.** A mark after each of the three; then move the slow one off the turn or skip
  it when its keys did not change. With 4.4 there are also fewer writes.
- **Size** S to find, S to M to fix. **Risk** low.

### 5.5 The edge fetches everything every 2 s

- **Wrong.** Each source says how often it wants to be read, or "once per opening"
  (`edge/card.rs:18-24`; `None` at `cards/settings_about.rs:144`, `settings_display.rs:237`,
  `settings_look.rs:234` and others). The loop ignores it: while a panel is open, every
  source of every card on it is fetched every 2 s, each on a new thread
  (`edge/wayland.rs:536-557`); `every()` has no caller outside tests. The About card's
  source starts four programs (`cards/settings_about.rs`, four `Command::new`).
- **Why it matters.** An open Settings panel starts programs every 2 s for data that does
  not change. A closed edge wakes for nothing, which is right (`edge/wayland.rs:1380-1381`).
- **Change.** Keep the last fetch time per source and honour `every()`.
- **Size** S. **Risk** low; a unit test with a counting source, and
  `tools/vm-edge/psettings.py`.

### 5.6 Smaller, in order

- The edge's theme follows a config change only while a panel is open
  (`edge/wayland.rs:531-535`): a pop-up shown with the edge closed keeps the old colours
  until a panel opens. Reload on the next draw as well. S.
- Real-time priority for the compositor's thread: not in the code (no `SCHED_RR` in
  `crates/` or `live/`), still open as item 5 of `PERFORMANCE-LINUX.md:232`. S plus one
  `sudo` step.
- `screen rescan` takes about 9 ms on ten turns (`drm:1686-1695`). Likely hotplug only; not
  confirmed. Look after 5.3.
- `live/launch.sh` loads `env.sh` and the config on each shortcut; `tools/test-launch-fast`
  guards it in CI (`.github/workflows/ci.yml:57`). Not measured here; leave.
- The compositor checks the config's time every 250 ms, only on turns it takes anyway
  (`drm:1712-1716`), and the background check is deadline-driven (`wl/background.rs:132-152`).
  Nothing to gain.

Already done since the earlier review: clients are flushed before drawing (`drm:2026-2029`)
and the margin shrinks a whole step after 240 good frames (`wl/frame_schedule.rs:65-69`).

---

## 6. Smithay

### 6.1 What it is here

Smithay 0.7.0 is vendored at `vendor/smithay-0.7.0` with four local patches (`patches/0001`
to `0004`, 362 lines of patch text): pointer constraints on leave, tearing page flips, a lost
GPU context, context priority. `tools/verify-architecture` pins the version, checks every
vendored file against `enforcement/smithay-0.7.0.sha256`, and lists the patched files' hashes
by hand (lines 32-53, 329-373). Infynitae already goes around the parts of Smithay that did
not fit: its own placement instead of `Space`, its own hit test, its own render tree and popup
order (`docs/foundation/FOUNDATION_BLOCKERS.md`, W-SM-1, 2, 5, 11, 12, 20). What is used is the
protocol code and the DRM, EGL and input backends: a toolkit, as the owner says.

### 6.2 Where it limits Infynitae, and the small patch for each

| Limit | Evidence | Patch | Size | Upstream |
| :--- | :--- | :--- | :--- | :--- |
| Unused `error` imports: warnings in every build | `use tracing::{…error…}` with no `error!(` in `backend/drm/surface/gbm.rs:19`, `backend/session/libseat.rs:28`, `backend/winit/mod.rs:27`, `input/keyboard/mod.rs:15`, `wayland/seat/keyboard.rs:3` (found by grep, not by building) | remove five words | S | yes; whether upstream already has it is not confirmed |
| Wrong error code for a buffer before configure | W-SM-18 (`FOUNDATION_BLOCKERS.md:376-380`); upstream commit `641d2ad` | backport that commit; the tests then accept only the right code | S | already upstream |
| A constraint's lifetime is private, so the adapter guesses "persistent" and cleans up after | W-SM-21 (`:395-399`) | a three-line accessor; the workaround goes | S | yes |
| A dead client's popup grab stays until the next pointer event | W-SM-19 (`:382-386`) | drop the grab on disconnect | M | yes |
| Why a frame was not scanned out is only in trace events, so Infynitae carries `tracing` and a 147-line subscriber | `crates/infynitae-backend-drm/Cargo.toml` comment; `crates/infynitae-backend-drm/src/scanout_log.rs` | return the reason from `render_frame` (`vendor/…/backend/drm/compositor/mod.rs:1690`) | M | yes, as an API addition |
| `configure_bounds(0,0)` cannot be left out | W-SM-17 (`:370-374`) | none needed: bounds are always set | - | - |

Not limits of Smithay: `wp_fifo`, `wp_commit_timing` and explicit sync are in the vendored
tree (`vendor/smithay-0.7.0/src/wayland/fifo`, `commit_timing`, `drm_syncobj`) and are not
offered by Infynitae yet (no use in `crates/`). That is adapter work, no patch
(`docs/research/PERFORMANCE-LINUX.md:242`, `245`).

Speed: nothing read here shows Smithay as the cause of the stalls in section 5. Item 5.3's
marks come first; a patch for speed needs a number that points into `vendor/`.

### 6.3 Keeping patches small, listed and upstreamable

1. One patch, one reason, one file in `patches/`, numbered. Its header says what failed,
   the owner's decision and date, and the upstream link or "not sent, because".
2. A list at `patches/README.md`: number, files touched, lines, reason, upstream state.
   Today that list is spread over comments in `tools/verify-architecture:38-53` and
   `Cargo.toml:24-28`.
3. Each patch comes with a test in `crates/infynitae-backend-headless/tests/` that fails
   without it, except build-only ones like the imports.
4. Each patch updates `enforcement/smithay-0.7.0.sha256` and the hash tables in
   `tools/verify-architecture`. A later step, M: replace the hand-kept tables with one
   check that the vendored tree equals 0.7.0 from crates.io plus `patches/000*.patch`
   applied in order. Then a patch is one file and one list line.
5. Send each upstream when written. A merged one disappears at the next pin update.
6. A limit: when the set passes about ten patches or 600 lines, move the pin to a newer
   Smithay instead of adding more. Whether a newer release exists was not checked.

### 6.4 Recommendation

Yes, make your own improvements, in the form above: small numbered patches to the vendored
copy, each tested, listed and sent upstream. Do not fork Smithay into the tree as Infynitae's
own code, and do not replace it: the parts in use (protocol objects, DRM and EGL, libinput,
libseat) are large, hardware-facing, and maintained by others, and nothing found argues that
they hold Infynitae back. The first three rows of 6.2 are a good first batch: they are a few
lines each, one is already upstream, and they remove a workaround and the warnings. This
changes the standing rule ("no Smithay patch now", `FOUNDATION_BLOCKERS.md:397`) from "only
when forced" to "when it removes a workaround or a defect in a few lines", which is the
owner's decision to record in `docs/DECISIONS.md`.

---

## 7. Build order

Owner decisions first, since they change what is built: errors in the accent or an alert
colour (1.6); motion as a fixed choice or a switch (group E); widgets ported or dropped
(4.2); the patch policy (6.4).

State on 2026-11-13, 18:00: batches 1 to 5 are merged on this branch (`git log`: `0e4cddb`,
`afafd69`, `a3580f1`, `e9e822b`, `8a468e6`, `1acfc2b`, `99a5e88` to `b362bc8`). Line numbers in
sections 1 to 6 are from before those merges; section 8 was read after them.

Each batch is for one worker and lists the files it owns. Batches in one wave touch no file
in common. CI means every job of `.github/workflows/ci.yml` passes; the jobs that matter are
named. The VM scripts are in `tools/vm-edge/` and run in the test VM only, never on this PC.

### Wave 1: no decision needed, lowest risk, in parallel

| # | Batch | Files owned | Verify |
| :--- | :--- | :--- | :--- |
| 1 | Dead files and stale statements (4.1, without the dead-code flag) | `live/lock/xdg-shell*`, `live/lock/wlr-layer-shell*`, `live/edge-apps/src/config.rs.orig`, `docs/COMPONENTS.md`, the comment in `crates/infynitae-wayland/Cargo.toml` | CI `helpers` (builds the lock, `test-lock-render`), `verify-architecture` |
| 2 | The edge honours each source's interval (5.5, 5.6 first point) | `live/edge-apps/src/wayland.rs`, `card.rs` | a unit test with a counting source; CI `edge-apps`; VM `psettings.py`, `ptime.py` |
| 3 | Wording in scripts (section 3) | `live/launch.sh`, `live/record/infynitae-recorder`, `live/infynitae-automount`, `tools/test-launch-actions` if it holds texts | CI `helpers` |
| 4 | Compositor: marks for 5.3 and 5.4, one yes/no function, default colours and corner limit (4.3 step 1, 1.9) | `crates/infynitae-wayland/src/lib.rs`, `convert.rs`, `crates/infynitae-backend-drm/src/lib.rs`, `crates/infynitae-backend-winit/src/lib.rs` | CI `workspace-build`, `verify-architecture`, `verify-test-efficacy`; then one owner session and the new log lines |
| 5 | Smithay first batch (6.2 rows 1 to 3) and the list | `vendor/smithay-0.7.0/…`, `patches/0005` to `0007`, `patches/README.md`, `enforcement/smithay-0.7.0.sha256`, `tools/verify-architecture`, the two adapter spots W-SM-18 and W-SM-21 name | `tools/verify-architecture` in CI; headless tests `t1_popup_policy`, `t1_constraint_lifetime` |

Batches 4 and 5 both touch the adapter: batch 5's adapter edits (the W-SM-21 workaround) wait
for batch 4 to merge, or batch 5 ships the vendored patches first and the workaround's removal
second.

### Wave 2: the look, in parallel

| # | Batch | Files owned | Verify |
| :--- | :--- | :--- | :--- |
| 6 | The whole look handed on; apps written from it; `set` does less (1.2, 1.3 for apps, 1.8, 4.4) | `live/edge-apps/src/theme.rs`, `live/settings.sh`, `live/config.sh`, `tools/test-settings-sync`, `test-settings-actions`, `test-mako-config`, `test-qt-theme`, `test-firefox-theme`, new `tools/config-cases` | those tests in CI `helpers`; `theme.rs` unit tests |
| 7 | The edge's kit and its callers (1.5), then the dead-code flag (4.1) | `live/edge-apps/src/ui.rs`, `draw.rs`, `main.rs`, `cards/*.rs` | CI `edge-apps`; `tools/test-edge-render`; VM `ppicker.py`, `psettings.py`, `pfluid.py`; pictures to the owner |
| 8 | Lock and prompt in the shared look (1.6, 1.7 step 1) | `live/lock/infynitae-lock.c`, `live/polkit/infynitae-polkit.c`, `tools/test-lock-render` | CI `helpers`; VM `pmic.py` |

Batch 8 reads the new look lines when they are there and falls back when not, so it does
not wait for batch 6. Batch 7 does not touch `theme.rs`; batch 6 does not touch `cards/`.

### Wave 3: after waves 1 and 2

| # | Batch | Files owned | Verify |
| :--- | :--- | :--- | :--- |
| 9 | One setting, many parts (section 2: B, C, D, F, G; E as decided) and light (1.4) | `config/config`, `live/edge-apps/src/config.rs`, `theme.rs`, `cards/settings_look.rs`, `settings_games.rs`, `settings_lock.rs`, `settings_rows.rs`, `live/config-check`, `live/portal/settings.c` | the two card tests at `settings_rows.rs:999` and `1046`; `tools/test-config-defaults`, `test-config-check`, `test-portal-settings`; VM `psettings.py`; a test that an old config gives the same look lines as before |
| 10 | Late start for desktop frames (5.1) and the fix for whatever 5.3 and 5.4 named | `crates/infynitae-wayland/src/frame_schedule.rs`, `crates/infynitae-backend-drm/src/lib.rs`, `convert.rs` as needed | unit tests in `frame_schedule.rs`; a full VM round; then the owner's trial with `LATENCY_STATS`, comparing the two log lines quoted in section 5 |
| 11 | Compositor borders from the look for built schemes (1.3, second half) | `crates/infynitae-wayland/src/lib.rs` | after batch 10, same files; headless tests; `verify-architecture` |
| 12 | Password prompt through the edge's question (1.7 step 2) | `live/polkit/infynitae-polkit.c`, `live/build-helpers.sh` | VM `pmic.py`, `pask.py` |

### Wave 4: when the owner makes the new edge the default

| # | Batch | Files owned | Verify |
| :--- | :--- | :--- | :--- |
| 13 | Delete the old edges and what serves them (4.2); one workspace under `live/` (4.5) | `live/edge`, `live/edge-rs`, `live/edge-pick`, `live/build-helpers.sh`, `.github/workflows/ci.yml`, `tools/test-edge-*`, `live/config-renames`, the `EDGE_IMPL` lines named in 4.2, `live/edge-apps/src/ask.rs`, `live/infynitae-format-drive` | CI; every VM script against the one edge; `tools/test-config-renames` |
| 14 | Recorder on dmabuf (5.2) | `live/record/*`, `serve_captures` in the DRM backend | VM `pcards.py` (saves a real clip); "captures" slow turns gone from the log |
| 15 | Smithay second batch (6.2 rows 4 and 5) and the simpler vendor check (6.3 point 4) | `vendor/`, `patches/`, `tools/verify-architecture`, `crates/infynitae-backend-drm/src/scanout_log.rs` | `tools/verify-architecture` with its seeded negative controls; headless tests |

The first things to do, for the most gain at the least risk: batches 1, 2 and 3 (deletions,
one loop fix, texts), then batch 4's marks, because two of the three largest speed items
cannot be fixed until one session's log names their cause.

### Wave 5: beyond Windows and other desktops (section 8)

Order: the measuring tools first, because every item in section 8 is judged by them. Then by
gain for risk. Batches marked "after N" touch the same files as batch N and wait for it.

| # | Batch | Size | Files owned | Verify |
| :--- | :--- | :--- | :--- | :--- |
| 16 | Measuring tools (8.0): the frame and motion test client, the wakeup counter, the zoom sharpness script | M | new `tools/frame-probe/`, `tools/wakeups`, `tools/vm-edge/pzoom.py` | each tool run in the VM prints its numbers; CI builds the client in `helpers` |
| 17 | Uploads at commit time, changed keys only on a settings change (8.1 steps 2 and 3), after one owner session with the marks | S to M | `crates/infynitae-wayland/src/handlers.rs`, `lib.rs` (`reload_config`) | "Slow turn" lines naming windows or config reload gone from one session's log; headless tests |
| 10 | (already listed) Late start for desktop frames, now with its own learned margin (8.2) | M | `frame_schedule.rs`, `drm` | unit tests; batch 16's client; the "Latency" log line |
| 18 | Captures never wait for the GPU (8.1 step 1), after 10 | M | the capture functions of `drm` (`serve_captures` to `deliver_copies`) | "captures" and "capture copies" gone from slow turns; VM `pshot.py`, `pcards.py` |
| 19 | Zoom stage 1: apps redraw at the next whole scale after a zoom, pixel budget, sharp upscale (8.7), after 11 | M | `crates/infynitae-core/src/scale.rs`, `wl/lib.rs` (the two scale setters), `crates/infynitae-backend-drm/src/smooth.rs`, `config/config` | scale unit tests in `scale.rs`; headless `t1_scale`; VM `pzoom.py` numbers before and after |
| 20 | Canvas: back, places, places in the window search (8.6) | M | `crates/infynitae-core/src/navigation.rs`, `entry.rs`, `input/keys.rs`, `crates/infynitae-ipc/src/protocol.rs`, `crates/infynitae/src/msg.rs`, `live/edge-apps/src/cards/windows.rs` | core unit tests; `crates/infynitae-ipc/tests/t1_shell.rs`; a VM script that jumps and comes back |
| 21 | Game profiles (8.4 B), after 9 | S to M | `wl/background.rs`, `tearing.rs`, `live/launch.sh` (`game-mode`), `config/config`, `cards/settings_games.rs` | unit tests in `background.rs` and `tearing.rs`; `tools/test-launch-actions`; one play session |
| 22 | Idle: fix what batch 16's counter names (8.5) | S | `crates/infynitae/src/restore.rs` and what the counter shows | `tools/wakeups` before and after, screen left alone for 60 s |
| 23 | Keeper stage 1: the session keeps the socket and the apps (8.3) | M | `live/infynitae-session`, `wl/session.rs`, `wl/ownership.rs`, `live/session.sh` | VM: kill the compositor, count windows whose process is the same; `tools/test-session` |
| 24 | Zoom stage 2: reduced copies for far zoom-out, only if `pzoom.py` shows aliasing below one eighth (8.7), after 18 and 19 | M | `smooth.rs`, `drm` | `pzoom.py` at 1/16 and 1/32 |
| 25 | HDR stage 0 and 1: read the screen's HDR block, colour management for a fullscreen game shown directly (8.4 A) | L | `crates/infynitae-backend-drm/src/edid.rs`, new `wl/color.rs`, `drm` scanout path, possibly `patches/0008` | the monitor's own HDR indicator; an HDR test video; headless protocol tests |
| 26 | Keeper stage 2: the proxy that holds every app (8.3) | L | new crate `live/keep/` | VM: every VM script passes with apps behind it; kill and update tests; batch 16's client with and without it |
| 27 | Zoom glides (8.8 stage 1): a target and a per-refresh approach in Core for the wheel, fit, overview, slots and back; the pointer's canvas point held from exact values | M | `crates/infynitae-core/src/entry.rs`, `navigation.rs`, `lib.rs` (one tick entry), `drm` (calls the tick each refresh), `crates/infynitae-backend-headless/tests/` (new `t1_zoom_glide.rs`) | the headless test's jump numbers (8.8, measured); `tools/verify-architecture` (the camera is still written only in Core); after 10 and 20, same files |
| 28 | Zoom as one picture (8.8 stage 2): the canvas drawn once on one grid while a zoom runs and shown through a fractional source rectangle; three-frame fade at the hand-over | M | `drm` (render path), `crates/infynitae-backend-drm/src/smooth.rs`, `wl/convert.rs` (a canvas-only element list) | headless: two windows' edges keep their distance through a whole zoom; `frame-probe` shows no missed refresh over 100 zooms; `pzoom.py` at rest unchanged; after 18 and 27 |
| 29 | Sharper pictures fade in; scale goes down only after a rest (8.8 stage 3) | S | `crates/infynitae-core/src/scale.rs`, `wl/convert.rs` (a window's last picture kept for the fade), `drm` | unit tests in `scale.rs` (no flip when the zoom hovers at a scale's edge); VM `pzoom.py` with a screenshot every frame of the settle; with or after 19 |
| 30 | Names on far windows (8.8, the new way): the edge labels windows too small to read, at rest only | S to M | `live/edge-apps/src/wayland.rs`, new `live/edge-apps/src/labels.rs` | `--render` picture with fixed data; a VM script that zooms out and reads the labels' boxes; after 28 |

Batches 16, 17, 20, 22 and 23 share no files and can run together. 19 and 21 each wait for
one earlier batch. 25 and 26 are large and start only when the owner asks for them.
Batches 27 to 29 are one lane in the compositor, in that order; 30 is the edge alone. Batch 24's
reduced copies are also what batch 30's far windows are drawn from, so 24 moves before 30.

---

## 8. Beyond Windows and other Linux desktops

Read on 2026-11-13 after the wave 1 merges. Seven items. Each says where the code stands,
the design chosen and why, what it costs, how it is measured against Windows, GNOME and KDE
on this PC, and the risk. Statements about other systems are from memory, were not checked
today, and are marked so. No number below is a promise: the targets are what the
measurements are held against.

### 8.0 The measuring tools (batch 16)

Three small tools, since Infynitae's own log lines cannot be read on another desktop.

- **`tools/frame-probe`**: one small Wayland client. It draws a counter every frame and reads
  `wp_presentation` feedback, so a jump of more than one in the refresh sequence is a missed
  refresh. It also draws a square at the pointer and pairs each motion event's time with
  the presentation time of the frame that showed it: motion to screen, as an app sees it.
  The same binary runs on Infynitae, GNOME and KDE (all three offer presentation feedback;
  from memory). On Windows the same two numbers come from PresentMon (dropped frames) and a
  240 fps phone camera (`docs/research/PERFORMANCE-LINUX.md:215`).
- **`tools/wakeups`**: a shell script that reads `voluntary_ctxt_switches` from
  `/proc/<pid>/task/*/status` twice, 60 s apart, for the compositor and every helper, and
  prints wakeups per second each. It works for `kwin_wayland` and `gnome-shell` too.
- **`tools/vm-edge/pzoom.py`**: section 8.7.

Cost: about 300 lines of C for the client (the helpers already carry generated protocol
files, e.g. `live/record/wlr-screencopy-protocol.c`), 30 of shell. No new dependency.
GNOME and KDE numbers need those desktops installed on this PC or started from a USB stick;
whether they are installed was not checked.

### 8.1 Never miss a frame

**Where it stands.** The running session (started 16:00:34) reported "2477 of 177930 frames
missed their refresh" when read at 18:13: about 1,100 an hour, 1.4% of the frames drawn. A
miss is a frame shown more than half a refresh after the one it was meant for
(`wl/frame_schedule.rs:295`). Everything runs on one thread: input, client requests, captures,
drawing (`drm:1625-2047`). The marks from batch 4 now split a slow turn into frame list,
cursor, layers above, windows, layers below, smooth wrap (`wl/convert.rs`
`output_elements_marked`; `drm:2219`, `2242`) and config reload, config-check, config second
read (`drm:1725-1750`). The compositor running today was built at 15:59, before the marks, so
no session has named the slow part yet.

**Design.** Three steps, each small, none a second thread.

1. *Captures never wait for the GPU* (batch 18). A capture into a client's GPU buffer is one
   more draw and no readback (`drm:2751`). A capture into shared memory draws off screen,
   starts a readback (`drm:2835`) and maps it at the start of the next turn (`drm:2885`,
   `2126`); mapping waits until the GPU has finished. Change: map only once the frame's GPU
   fence has signalled (Smithay hands out the fence: `vendor/…/renderer/sync/mod.rs:29-36`,
   `gles/mod.rs:2302`), and start at most one readback per screen per turn. Our own recorder
   moves to GPU buffers (batch 14).
2. *Uploads at commit time* (batch 17). A client's shared-memory buffer is uploaded to the
   GPU when the frame's elements are built (`wl/convert.rs:572-594`, `from_surface`), not when
   it is committed (`wl/handlers.rs:447` only records it). Upload on commit instead, so the
   work is done before the frame's start time. Only if the marks name the windows stage.
3. *A settings change applies only what changed* (batch 17). Today a change re-applies every
   key, starts `config-check` and reads the file a second time, all in one turn
   (`drm:1719-1750`, `wl/lib.rs:1116` onward). Keep the last parsed values and apply the keys
   that differ; move the second read into the first.

**Why this and not threads.** The GL context and the Wayland state belong to the loop's
thread; a render thread means sharing both, which is a rewrite of the backend and a new
class of bugs. "Cached element building" is not chosen either: an element list reused across
frames is wrong the moment a surface commits, and nothing measured says building the list
(as opposed to uploading inside it) is slow. The three steps remove the waits that have been
seen (captures 8.4 to 8.9 ms, config 92 ms, section 5) and leave the loop single-threaded.

**Cost.** About 150 lines in `drm`, 60 in `wl/`. Runtime: less work, none added.

**Measured.** Infynitae: the counter in the "Latency" line (`drm:3143`), as missed per hour
of desktop use with no game running, and the count of "Slow turn" lines. Target: under 20 an
hour (today about 1,100) and no slow turn outside a screen being plugged in. Other desktops:
`frame-probe`'s gap count over the same hour of the same use; Windows: PresentMon.

**Risk.** Step 1 low (a capture arrives a turn later). Step 2 medium: an upload on commit is
wasted for a window that commits twice per refresh or is not in view; do it only for
surfaces in the last frame. Step 3 medium: a key missed in the comparison is not applied
until restart; the yes/no cases file from 4.3 and the headless config tests cover it.

### 8.2 Lowest input-to-screen delay on the desktop

**Where it stands.** The scheduler that starts a frame as late as it can still make the
refresh exists and learns: render times of the last 128 frames, their 95th percentile, a
margin that widens 0.75 ms on a miss and narrows after 240 good frames, a pause when misses
persist (`wl/frame_schedule.rs:60-80`, `209-222`, `295-320`). One exists per screen
(`drm:2181-2183`). With the default `LATENCY="direct"` it is used only while a game is shown
directly; a composited desktop frame is drawn at once (`wl/frame_schedule.rs:48-54`). Today's
log: "input to screen p50 11.57 ms … budget 0.15 ms".

**Design.** In "direct", composited frames are scheduled too, with a margin of their own.
The schedule keeps two margins per screen, one for frames shown directly and one for
composited ones, so a game's misses do not widen the desktop's margin or the reverse. The
composited margin starts at 1.5 ms and follows the same widen, narrow and pause rules. Late
start is skipped while `GPU_RELIEF` is acting or the last turn was slow.

**Why this.** The alternative is a fixed "draw N ms before the refresh" as KWin and Mutter
do (from memory; the module's own header notes KWin predicting 11 ms for 2 ms of work,
`wl/frame_schedule.rs:8-9`). A fixed lead is either too early (delay) or too late (misses)
on a PC whose GPU is sometimes training a model. A margin learned from this screen's own
misses adapts to both. It reuses the code that exists; the change is the second margin and
one condition.

**Cost.** About 60 lines in `frame_schedule.rs`, 10 in `drm`. Runtime: one more timer wake
per drawn frame.

**Measured.** The "Latency" line: input-to-screen p50 and p95, target p50 under 4 ms on the
165 Hz screen with the missed count no higher than before. `frame-probe`'s motion-to-screen
on Infynitae, GNOME and KDE. Across to Windows only the camera test compares: 20 clicks
filmed at 240 fps, about 4 ms a camera frame.

**Risk.** Medium. Step 8.1 must land first or with it: once frames start late, any stall in
the turn is a miss where today it is only a late start. Unit tests in
`frame_schedule.rs:451` onward get cases for the two margins.

### 8.3 Apps survive a compositor crash or update

**Where it stands.**

- The compositor makes the listening socket itself (`wl/session.rs:343-346`) and is the
  parent of everything in the session: on exit it ends every process it started
  (`wl/ownership.rs:1-12`; the log's "Session processes ended … 70 terminated",
  `wl/session.rs:703`).
- `live/infynitae-session` restarts a compositor that crashed or quit (`:96`, `:138`).
- After a restart `infynitae restore` reopens the saved apps where they were
  (`crates/infynitae/src/restore.rs:8-11`). That is reopening, not surviving: unsaved work
  in an app is lost.
- `QT_WAYLAND_RECONNECT=1` is set (`live/env.sh:38-39`), but a Qt app is ended with the
  session on a clean exit and loses its parent on a crash. Whether any app has ever
  reconnected here is not confirmed.
- An update installs the new binary and says it "runs from its next start"
  (`update.sh:114`, `138`).

**What exists elsewhere (from memory, not checked today).** KWin keeps the socket in a
wrapper process and Qt and some patched toolkits reconnect; other apps die. `waypipe` and
`wprs` are proxies that sit between apps and a compositor and track protocol state, built
for remote use. No drop-in tool keeps every local app across a compositor restart.

**What Smithay allows.** It accepts a client from any connected stream (`insert_client`,
`wl/session.rs:351`), so the socket can be owned by another process without a patch. Its
socket type has no constructor from an existing socket (`vendor/…/wayland/socket.rs:65`,
`80`), so Infynitae accepts on the inherited one itself. All client state lives in the
compositor's memory and nothing can save or hand it over, so a restarted compositor cannot
"take clients back" by itself. Something has to replay each client's state to it.

**Design, in two stages.**

*Stage 1, the keeper holds the socket and the apps (batch 23, M).* `infynitae-session`
already outlives the compositor. It creates the socket and passes it to each compositor it
starts; the session's apps are started under it, not under the compositor; a crash no longer
orphans them and an update's restart no longer ends them. Apps that can reconnect (Qt 6) come
back by themselves; the others see their connection close and exit, and `restore` reopens
them as today. The process-ownership rule (`wl/ownership.rs`) moves to the keeper with its
tests.

*Stage 2, the proxy (batch 26, L).* A separate program, `infynitae-keep`: a Wayland server to
the apps and one client to the compositor. It keeps each app's objects (surfaces, roles,
buffers, their sizes and states) and forwards requests and events. Buffers pass as file
descriptors, so no pixels are copied and a game can still be shown directly. When the
compositor goes away it holds the apps' connections, reconnects to the new one and creates
every surface again from what it kept. `xwayland-satellite` runs behind it, so X11 apps and
games survive too. The edge, wallpaper and lock connect directly as today; they restart in
under a second.

**Why two stages, and why a proxy.** Reconnecting inside each toolkit (KDE's way) can never
cover every app: GTK, Electron, SDL and Wine would each need it. Splitting the compositor so
the protocol side never restarts does not help an update, which replaces that side too. A
proxy is the only design that covers every app without changing them, and it is also the
largest and riskiest piece in this plan, so stage 1 is built first: it is needed by stage 2
anyway and already saves Qt apps.

**Size and risk, plainly.** Stage 2 is every protocol Infynitae offers to normal apps,
handled twice (as server and as client) with state kept: core, xdg-shell, viewporter,
fractional scale, shared memory and GPU buffers, clipboard and drag, pointer constraints and
relative motion, text input, tearing, content type, presentation feedback, activation. My
estimate is 8,000 to 15,000 lines of Rust and months, not weeks; it is not measured. Risks:
(1) one bug in it breaks every app, so it ships off, behind `KEEP_APPS`, a trial switch that
goes when it is the default; (2) each message crosses one more socket, added delay not
known until measured; (3) the compositor knows apps by their process
(`wl/session.rs:355-357`, trust and background priority), and behind a proxy every app is
the proxy, so the proxy must pass the real process on over a private channel; (4) an app
keeps running blind for the seconds the compositor is away and may time out.

**Can `infynitae-update` restart the compositor without closing windows?** After stage 1:
only Qt 6 windows stay; the rest are reopened in place. After stage 2: yes for every app
behind the proxy, and the update can offer "restart now" instead of "from its next start".
Before either: no.

**Measured.** In the VM: open ten apps of different toolkits, kill the compositor, count the
windows whose process id is unchanged and the seconds until the desktop is back. Same test
on KDE (expect Qt apps only) and GNOME (expect none; from memory). Windows restarts its
compositor with all windows kept, so stage 2 reaches Windows and passes both Linux desktops.
Delay added by the proxy: `frame-probe` and the "Latency" line with and without it.

### 8.4 Games

#### A. HDR

**Where it stands.** Nothing. Smithay 0.7 has no colour management module (the list of
`vendor/smithay-0.7.0/src/wayland/`) and sets no HDR property on the screen; variable refresh
is the only such property it handles (`vendor/…/drm/surface/atomic.rs:567-633`). Infynitae
asks for 8-bit screen buffers (`drm:1001`). The renderer does know 10-bit and half-float
formats (`vendor/…/renderer/gles/format.rs:22-23`). The colour management protocol's
description is in the locked `wayland-protocols` 0.32.13 (`Cargo.lock:1883-1884`; its
`protocols/staging/color-management`); whether the server bindings are built in this
feature set is not confirmed. The EDID reader takes make, model and serial only
(`crates/infynitae-backend-drm/src/edid.rs:3-9`), so whether this monitor does HDR is not
known to the compositor. X11 has no HDR path (from memory), so a game through
`xwayland-satellite` cannot be HDR whatever the compositor does; it needs Proton's Wayland
driver (`docs/research/PERFORMANCE-LINUX.md:43`).

**Design.** Passthrough first.

- *Stage 0 (S).* Read the EDID's HDR block and log what the screen says it can do.
- *Stage 1 (L).* Offer the colour management protocol with the smallest useful set: a
  surface may say its content is HDR10 (PQ, BT.2020) with its mastering values. While such
  a window is fullscreen and shown directly, set the screen's HDR metadata and colour space
  and hand the game's buffer through untouched; set them back when it stops. Setting the two
  screen properties needs either the DRM device handle Smithay exposes
  (`vendor/…/drm/surface/mod.rs:191`) or a small patch beside `use_vrr`, listed as P8.
- *Stage 2 (L, not planned now).* The whole desktop drawn in HDR with SDR windows mapped
  into it.

**Why passthrough.** Composited HDR needs a 10-bit or float screen buffer, a colour
conversion in every draw and tone mapping, in a renderer that has none of it: a large change
to the path every frame takes. A fullscreen game shown directly already skips the renderer
(`DIRECT_SCANOUT`), so HDR for that case touches only the protocol and two screen
properties. It covers the one use the owner named (games).

**Cost.** Stage 1: about 600 to 900 lines (protocol objects are most of it). Runtime: none
outside an HDR game.

**Measured.** The monitor's own HDR indicator turns on with the game and off after; an HDR
test pattern shows detail above SDR white; `drm_info` shows the metadata set. Against
others: KDE and GNOME have composited HDR and Windows has it with Auto HDR (from memory), so
this reaches parity for fullscreen games and stays behind for HDR video in a window. Not a
place to claim a lead.

**Risk.** High for its size: wrong metadata gives a washed-out or dark picture, and an
overlay drawn over the game (a notification) forces compositing, where stage 1 must drop
back to SDR cleanly. `GAME_QUIET` already holds notifications during a game.

#### B. Per-game profiles

**Where it stands.** A game is noticed when it fills the screen (`wl/background.rs:182-221`):
game mode starts, notifications are held if it "plays like a game" (`:226-243`), and both
end 5 s after it leaves (`:38`, `:156`). The hook runs `launch.sh game-mode on|off`
(`wl/background.rs:245-251`, `live/launch.sh:107-141`). Tearing can be forced or refused per
window, but only until the window closes (`wl/tearing.rs:34-37`, `113-139`). A per-app choice
that is remembered in the config exists for keep-on-top and privacy
(`wl/lib.rs:1657`, `live/launch.sh:597-607`).

**Design.** One list, in the form `GLOBAL_SHORTCUTS` already uses:
`GAME_PROFILES="steam_app_730=notear,power:performance cs2=composited"`. Words: `tear` or
`notear`, `direct` or `composited`, `quiet` or `loud`, `power:performance|balanced|saver`.
A game with no entry gets the global settings, as today. The profile is applied where game
mode starts and undone where it ends, the pair that exists. "Automatic" means nobody writes
the list by hand: Super+Shift+O on a game stores its tearing word for that app through
`remember_for_app`, and the Games card lists the games seen with their words.

**Why this.** The alternative is a file per game or a profile editor. One list keyed by app
id is what the config already does three times, needs no new file format, and the start and
end hooks exist. Variable refresh and the latency mode are left out: nobody asked to change
them per game.

**Cost.** About 150 lines in `wl/`, 20 in `launch.sh`, a list on the Games card. Runtime: a
lookup when a game starts.

**Measured.** Function, not speed: start a game with a profile, check tearing, the power
mode (`powerprofilesctl get`) and held notifications; quit, check all three are back.
Windows and the other desktops have no per-game compositor profile (from memory; GPU vendor
panels do part of it), so the measure is that it works and restores.

**Risk.** Low. A wrong app id means no profile, which is today's behaviour. X11 games all
come through one satellite process, but each window still has its own app id; that the id is
stable per game (e.g. `steam_app_<n>`) is not confirmed for every launcher.

### 8.5 Idle efficiency

**Where it stands.** Read from `/proc` today over 63 s while the owner was using the PC (so
not idle): compositor 134 wakeups a second, Xwayland 61, the edge 2.0, `infynitae restore
watch` 0.24, and 0.00 for the portal, password agent, wallpaper, automount, sleep-lock,
recorder supervisor, clipboard tools and the session script. `gpu-screen-recorder`, the
replay the owner turned on, woke 891 times a second. A true idle reading has not been taken.

From the code, at idle the compositor sleeps until the earliest of a short list of
deadlines, each `None` when there is nothing to wait for (`drm:1665-1677`,
`wl/session.rs:504-529`); the config's time is looked at only on turns it takes anyway
(`drm:1713`). What can still wake it with a still screen:

- `restore watch` asks for the window list every 15 s (`crates/infynitae/src/restore.rs:23`).
- an animated cursor (`wl/session.rs:512`).
- any client that commits or sends a request, including the edge.
- the logind question every 45 s while idle sleep is on (`wl/idle_inhibit.rs:71`).

The edge's 2 a second with, as far as known, no panel open is not explained by its loop,
which has no timeout when closed (`live/edge-apps/src/wayland.rs:1536-1538`); window title
changes reaching it from the compositor are one candidate. Not confirmed.

**Design.** Measure first (batch 16), then remove what the counter names (batch 22). Known
now: `restore watch` saves on a change, using the shell socket's `subscribe`
(`crates/infynitae-ipc/src/protocol.rs:156`), not every 15 s. The replay is the owner's
choice and outside the count, but its cost is shown on its Settings row so the choice is
informed.

**Why this.** There is no polling loop left in the helpers to redesign; the scripts block on
`gdbus monitor` or `wait` (`live/infynitae-automount:546`, `live/infynitae-sleep-lock:145`,
`live/record/infynitae-recorder:302`). What remains is found by counting, not by reading.

**Cost.** `restore watch` on events: about 40 lines. Others not known until measured.

**Measured.** `tools/wakeups`, screen left alone for 60 s, no app drawing: target under 1 a
second for the compositor and 0 for every helper. The same script on `kwin_wayland` and
`gnome-shell` with their default panels. Across to Windows: CPU package power at idle
(`/sys/class/powercap` on Linux, HWiNFO on Windows), same screen brightness and refresh
rate. `powertop` is optional and is not in `live/packages.txt` (not checked).

**Risk.** Low. Saving the session on change must still be throttled, or a window being
dragged writes the file every frame.

### 8.6 The canvas: what only Infynitae can do

**Where it stands.** The camera can go to a window slot or a cluster (Super+1…9,
`crates/infynitae-core/src/input/keys.rs:77-80`), to the last two windows (Super+Tab), and to
an overview and back (`entry.rs:1385`). A history of 256 camera positions exists, but only a
fit pushes to it and "restore" pops it (`navigation.rs:32`, `380-383`; `entry.rs:1828`). The
shell socket's `navigate` takes a point and a zoom (`crates/infynitae-ipc/src/protocol.rs:159-187`);
the command line passes no zoom (`crates/infynitae/src/msg.rs:206-228`). Windows can be
searched and jumped to in the edge's Windows panel (`live/edge-apps/src/cards/windows.rs:1-4`)
and in the overview's menu (`msg.rs:324` onward). Nothing names a place.

**Design.** Three additions on what exists.

1. *Back.* Every jump (slot, Super+Tab, a pick in the window search, `navigate`, the
   overview) pushes the camera to the history that is there; one key walks back. The key is
   the owner's to choose (`BIND_BACK`).
2. *Places.* `PLACE_<NAME>="x,y,zoom [CHORD]"`, a line like `PANEL_<NAME>`.
   `infynitae msg place NAME` saves where the camera is; the chord, or the name in the
   window search, goes there. A place is where a task lives: "mail", "game", "work".
3. *One search.* Places are rows in the Windows panel's list beside windows, and typing
   while the overview is shown opens that panel with what was typed. The fuzzel menu in
   `msg.rs` stays only as the fallback without the edge.

**Why this.** Workspaces (GNOME, KDE, Windows) are a fixed row of screens; a place on an
endless canvas is a point and a zoom, which needs no new object in the core, only a stored
camera value and the navigate that exists. A separate "task" object with its own window
list was the alternative: more state to keep right, and the canvas already groups windows by
where they are (clusters, `keys.rs:79-80`).

**Cost.** About 80 lines in core (history pushes, one key), 60 in the adapter and `msg.rs`,
80 in the Windows card. Runtime: none.

**Measured.** Key presses and time to reach a given window among 20, by the same person:
Infynitae's search and places against Alt+Tab and the task view on Windows and the overview
on GNOME and KDE. The time from key to frame is in the VM script `ptime.py` already.

**Risk.** Low. A place saved on one screen size shows a different area on another; store the
centre and the zoom, as the session file does (`restore.rs:4-5`).

### 8.7 Zoom quality at any distance

**Where it stands.**

- Each window has a wanted scale: the largest, over the screens showing it, of screen scale
  times camera zoom. The window is told the smallest step of a ladder that covers it,
  through the fractional scale and buffer scale hints
  (`crates/infynitae-core/src/scale.rs:3-13`; `wl/convert.rs:1618-1630`). During a zoom
  nothing is sent; one hint goes out when it settles (`scale.rs:48-52`, "after"), and only
  at whole scales (`wl/lib.rs:1098-1114`).
- That machinery is off by default: `MAX_APP_SCALE="1"` (`config/config:412-416`;
  `wl/lib.rs:98-101`: a terminal re-flowed its text while zooming, before "after" existed).
  So zooming in today enlarges the pixels the app drew at 1x, with the nearest-pixel filter
  (`drm:1190-1197`): text turns blocky.
- Zoomed out, each screen pixel averages a 4 x 4 grid of samples over the area it covers
  (`crates/infynitae-backend-drm/src/smooth.rs:1-9`, `53-64`), which is exact down to about
  one eighth size. Smithay builds no smaller copies of textures (`smooth.rs:4`; no
  `GenerateMipmap` in `vendor/…/renderer/gles`). The camera can zoom far past one eighth
  (`crates/infynitae-core/src/geom.rs:23`).
- The only ceiling on a window's scale is the GPU's largest texture, 16384
  (`scale.rs:31-33`). A 2560 x 1440 window at 2x is 59 MB a buffer, at 3x 133 MB, and apps
  keep two or three.

**How others do it (from memory, not checked today).** KDE's zoom and macOS's zoom enlarge
the finished screen picture, so text is soft or blocky at any zoom above 1. Hyprland's zoom
does the same. niri's overview and GNOME's draw live windows smaller with the GPU's linear
filter, GNOME with smaller texture copies. None asks apps to redraw for the zoom.

**The design: draw at the next whole scale up, average down.**

1. *Zoomed in, settled.* The window is asked to redraw at the next whole scale at or above
   what it needs (zoom 1.4 gives 2, zoom 2.3 gives 3). The compositor averages that down to
   the exact zoom with the shader that exists. Text is drawn by the app's own font code at
   more pixels than the screen shows and then reduced, so it is sharp at every zoom in
   between, and the app's layout does not change because whole scales keep its logical size.
   Default `MAX_APP_SCALE` becomes 3.
2. *While the zoom moves.* Nothing is asked of apps; the picture they last drew is scaled.
   The hint goes out once, 300 ms after the zoom stops (`scale.rs:29`). This is "after",
   already built and tested (`scale.rs:1495`).
   The sharper picture is faded in, not swapped, and a window goes back down a scale only
   after a rest (section 8.8, causes 5 and design point 4).
3. *A pixel budget.* A window is never asked for more pixels than four times the screen's.
   This joins the texture limit in the same place (`scale.rs:6-8`). Only windows in view are
   asked at all, since the wanted scale counts only screens showing the window.
4. *Past the ceiling* (zoom above 3, or a window over budget), the enlarged picture uses a
   sharp-edge filter: nearest inside each source pixel, blended across one screen pixel at
   its border. About ten shader lines beside the existing ones in `smooth.rs`; it replaces
   plain nearest, which staircases at uneven zooms.
5. *Zoomed out to one eighth.* Unchanged: the 4 x 4 average.
6. *Farther out* (stage 2, batch 24, only if measured to alias). Each window in view keeps
   a quarter-size copy made with the same shader, redrawn when the window commits, at most
   ten times a second; the screen draw samples the copy. That is clean to 1/32. A copy of
   a 2560 x 1440 window is 0.9 MB.

**Why this and not the others.**

- *Exact fractional scales* (ask for 1.4 at zoom 1.4) give the fewest pixels, but apps round
  their sizes at fractional scales, so text and layout shift at each new zoom; the owner
  already rejected that for terminals. Whole scales cost more memory and never shift.
- *Enlarging the finished picture* (KDE, macOS, Hyprland) costs nothing and cannot be sharp:
  the detail was never drawn.
- *Smaller texture copies for every window, always* (the GNOME way) needs a Smithay patch,
  does not work for buffers that come straight from a game or video decoder, and costs GPU
  time on every commit of every window for a case, far zoom-out, that is rare. A copy only
  for windows in view at that distance costs nothing otherwise.

**Cost.** Stage 1: the default, about 30 lines for the budget, 10 of shader, 20 to choose
it. GPU: a window redrawn at 2x or 3x is the app's cost, paid once per zoom; the average
down is the shader that runs today when zoomed out. Memory is bounded by item 3: at most
four screens' worth of pixels per window in view.

**Measured: `tools/vm-edge/pzoom.py`.** A test window shows black text at 10, 12 and 16 px
and a pattern of 1 px lines. The script sets the zoom to 0.03, 0.06, 0.125, 0.25, 0.5, 0.75,
1.5, 2, 2.7 and 4 through `navigate`, waits for it to settle, and takes a screenshot. Two
numbers per zoom: for zoom above 1, the width in screen pixels over which a letter's edge
goes from 10% to 90% of its contrast (sharp is under 1.5); for zoom below 1, the difference
from a reference made by averaging the 1x picture on the CPU (the line pattern must become
even grey, with no bands). The same window and zooms on KDE's zoom, and Windows' Magnifier
by hand, give the comparison: their edge widths grow with the zoom, Infynitae's must not.

**Risk.** Medium. Apps that ignore the scale hints stay at 1x and get item 4. A terminal
changes its buffer at the settle; with whole scales its rows and columns must stay the same,
which is the case to test first (foot, kitty, alacritty). Memory rises for people who work
zoomed in; the budget bounds it and `MAX_APP_SCALE="1"` remains for those who want none of
it.

### 8.8 Zoom without anything jumping

Read on 2026-11-13 after wave 2 and batch 16 were merged (`ff7fa5f`). Nothing was run.

#### What moves today, and why

1. **The zoom is a step, not a motion.** Each wheel event multiplies the zoom by 1.15 per
   notch and writes the camera at once (`crates/infynitae-core/src/entry.rs:2251-2288`).
   Nothing in Core, the adapter or the DRM backend interpolates a camera (no match for
   animation or easing in their sources, apart from the cursor's own frames). Fit, the
   overview, zoom reset and the window slots write their target the same way
   (`entry.rs:2292-2330`, `1401`; `navigation.rs:445`). The screen shows a 15% jump in one
   refresh, then stands still until the next notch.
2. **Steps follow input events, not refreshes.** A notch lands in whichever refresh it
   arrives before. Two notches in one refresh give a 32% jump, none gives a still frame. A
   touchpad's fine scroll (`axis.amount`, `entry.rs:2253-2256`) and a touch pinch
   (`crates/infynitae-core/src/touch.rs:468`) are finer but paced the same way.
3. **Every edge is rounded to a whole pixel, again at every step.** A window's four edges
   are each `round((x - R)·k) - C` (`crates/infynitae-core/src/projection.rs:231-233`,
   `134-141`), and the reference they are rounded against is set again on every zoom change
   (`crates/infynitae-core/src/lib.rs:2098-2100`; kept only while panning,
   `projection.rs:1292`). So from one step to the next a window's width, and the gap between
   two windows, can each change by one pixel in a different direction: windows shift against
   each other and thin lines shimmer.
4. **The point under the pointer moves by up to half a pixel per step.** The new centre is
   computed so the canvas point under the pointer stays (`entry.rs:2270-2284`), but the
   camera is then snapped to the pixel grid (`projection.rs:226-229`). The error does not
   add up (each step starts from the stored centre), but it is there on every step.
5. **The sharper picture arrives in one frame.** With `MAX_APP_SCALE` above 1, an app is
   told its new scale 300 ms after the zoom stops (`crates/infynitae-core/src/scale.rs:29`,
   `48-52`) and its next buffer replaces the old one whole. Section 8.7 makes that the
   default, so it would add this pop for everyone. With `ZOOM_SHARPEN="live"` it happens
   at each step and terminals re-flow (`wl/lib.rs:98-101`).
6. **The border does not scale, the corners do.** The corner radius is multiplied by the
   zoom (`wl/convert.rs:971-977`); the border's width is multiplied by the screen's scale
   only and never goes under one pixel (`wl/convert.rs:1017`). Zoomed out, a 1 px border
   stays 1 px around a corner that got smaller; it reads as the border jumping thicker.
7. **Panels and the cursor.** Layer surfaces are outside the canvas and do not move
   (`wl/layers.rs:1-4`). The cursor keeps its size. Neither is a cause. Whether the edge
   redraws when window titles change during a zoom is not confirmed.

The far-zoom shimmer (smaller than one eighth) is section 8.7's item 6, not repeated here.

#### The design

1. **A target and a glide, in Core.** The wheel, a pinch, fit, the overview, the slots and
   "back" set a target (zoom and the canvas point to hold). Once per refresh Core moves the
   camera toward it: the logarithm of the zoom closes a fixed share of the remaining
   distance per unit of time (time constant 50 ms: 95% there in 150 ms). A new notch while
   it glides only moves the target, so ten fast notches are one motion. A finger on a
   touchpad or screen is followed directly (no lag behind a finger), and released into the
   same glide for the last part. The tick is one entry in Core called by the backend each
   refresh while a glide runs: the camera is still written only in Core, which
   `tools/verify-architecture` enforces (probes at lines 145 and 160).
2. **The pointer's canvas point is exact.** The point is taken when the glide starts and
   kept until the pointer moves; each frame's centre is computed from it and the frame's
   zoom, from unrounded values. Rounding happens once, when the frame is drawn.
3. **While a glide runs, the canvas is one picture.** The windows, borders and corners are
   drawn into one off-screen picture on one pixel grid, the finer of the start's and the
   target's, exactly as a resting frame is drawn. The screen then shows that picture through
   a source rectangle with fractional edges, which the renderer already takes (source
   rectangles are `f64`, `wl/convert.rs:127`; off-screen pictures are already made for
   captures, `drm:2801`). Every window, border and corner goes through the same single
   transform, so nothing can move against anything else, and positions are finer than a
   pixel. Windows stay live: the picture is redrawn each frame. Panels and the cursor are
   drawn on top as now. The picture is at most twice the screen per axis; a longer zoom-out
   changes grid on the way, behind the same fade as point 5.
4. **Apps are asked for nothing during a glide, and rarely after.** No logical size is ever
   changed by a zoom (already true: only scale hints are sent, `wl/convert.rs:1618-1630`).
   The scale hint goes out when the zoom has rested 300 ms, whole scales only (8.7). Going
   up a scale happens when the zoom needs it; going down only after 2 s at rest and when the
   need is at least half a scale lower, so a zoom hovering around 2.0 never flips. The
   policy already forbids going down during a zoom (`scale.rs:14-17`); this adds the rest.
5. **Hand-overs are faded.** Two moments change the pixel grid: the first and last frame of
   a glide (the one picture against the normal frame, at most half a pixel apart per edge),
   and an app's sharper buffer arriving. Both are a three-frame fade (18 ms at 165 Hz) for
   the grid and a six-frame fade for the sharper buffer, from a kept copy of the window's
   last picture (a window can already be drawn alone into a texture,
   `wl/convert.rs` `window_capture_elements`). The copy is dropped after the fade, or after
   500 ms if the app never answers.
6. **Borders scale with the zoom**, like corners, and fade out below one pixel instead of
   staying at one.

**Why this and not the alternatives.**

- *Placing each window at fractional positions* would need Smithay's draw call to take a
  fractional destination; it takes whole pixels (`wl/convert.rs:126`). That is a patch to
  the renderer's vertex stage and still rounds each window separately. One picture uses
  only what exists and rounds once.
- *A fixed-length animation with an ease curve* restarts or stutters when a second notch
  comes mid-way. An approach toward a moving target has no start or end to collide with.
- *Momentum after the hand stops* (the canvas coasting on) is left out: the owner asked for
  nothing to move that he did not move.

**What it costs at 165 Hz.** At rest: nothing; the normal path, and a fullscreen game is
still shown directly. During a glide (about 150 ms after the last input):

- Cheap: the glide's arithmetic (a few floating-point operations per refresh), the fades
  (one extra textured rectangle for a few frames).
- GPU: the canvas is drawn as now, into a picture of one to four times the screen's pixels,
  plus one full-screen draw of that picture. On a 2560 x 1440 screen the extra draw is
  3.7 million pixels a frame, 608 million a second; the picture itself is up to 14.7 million
  pixels a frame in the worst case (a long zoom-out). Not measured. The dedicated GPU here
  draws a whole desktop frame in 0.15 ms (section 8.1's log line), so there is room; on an
  integrated GPU the cap drops to 1.5 times per axis.
- Memory: one picture of at most four screens (59 MB at this size) while a glide runs, and
  one kept copy per window in view for under a second after a settle.
- Every refresh of a glide is a full redraw, so 8.1 (no stalls) matters more: a stall
  during a glide is a visible hitch.

#### A new way to zoom

Two things, both made of the pieces above.

1. **The overview is zoom-out, and every jump is a glide.** With point 1 in Core, Super+Tab,
   the slots, fit, "back" (8.6) and the overview all travel instead of cutting. The overview
   stops being a mode: it is the glide to the zoom that shows everything, and any zoom-in
   from it lands where the pointer is. One key zooms to the window under the pointer and the
   same key goes back (fit and the history exist, `navigation.rs:380-401`). Cost: none
   beyond batch 27.
2. **Names on far windows.** When a window is drawn smaller than about 160 px high its text
   cannot be read, and averaging cannot fix that. At rest, the edge draws each such
   window's icon and title beside it at normal text size: the canvas far out reads as named
   cards with live pictures, and near it is the real windows. The edge already has the text
   renderer, the icons and the window list; the positions come from the shell socket's
   snapshot, which it can subscribe to (`crates/infynitae-ipc/src/protocol.rs:156`). Labels
   fade in 100 ms after the zoom rests and are hidden the moment it moves, so they never
   trail behind the canvas.

**Why these.** Text in the compositor would need a font renderer there, a new dependency in
crates that are held to a short list (`tools/verify-architecture:80-92`); the edge has one.
Replacing far windows with cards entirely (no live picture) throws away what the canvas
shows best, the windows themselves. Pinch and wheel "with momentum" is the coasting the
owner did not ask for; the glide already gives the wheel a smooth finish.

#### How smooth is measured

- **Jump, in a test** (`t1_zoom_glide.rs`, headless, a fake clock at 165 Hz): a wheel zoom
  of ten notches and a fit across the canvas. Per frame: the screen position of the held
  canvas point (ideal: constant) and of two windows' edges. Targets: the held point moves
  under 0.05 px in any frame; the distance between the two windows' edges, divided by the
  zoom, is constant to 0.05 px during the glide; the zoom's step ratio from frame to frame
  never grows (no speeding up after it starts slowing).
- **Pacing, on the real screen:** `tools/frame-probe` in a window while a script zooms 100
  times: no missed refresh, and the "Latency" line's missed count unchanged.
- **The hand-over:** screenshots of the last glide frame and the first resting frame;
  no edge more than half a pixel apart (compared as images), and the fade covers it.
- **Sharpness at rest:** `tools/vm-edge/pzoom.py` gives the same numbers as before 8.8.
- **Against others:** the same ten-notch zoom filmed at 240 fps on KDE's zoom, Hyprland's
  and macOS's (from memory, each scales the finished screen, so nothing shifts but text is
  soft) and on today's Infynitae (15% steps). The claim to prove: as steady as theirs
  during the motion, and sharp at rest where theirs is not.

#### Batches

27 to 30 in wave 5's table. Sizes: 27 M, 28 M, 29 S, 30 S to M. Batch 27 alone removes
causes 1, 2 and 4 and gives the gliding overview; 28 removes cause 3; 29 removes cause 5
and must ship with batch 19, so the new default never shows the pop; cause 6 is ten lines
in batch 28. Risk: 28 is the one that changes the render path, and only while a glide runs,
so a fault there shows as a wrong picture for a fraction of a second and the resting frame
is untouched. The headless tests compare the last glide frame with the resting frame so a
fault cannot pass unseen.
