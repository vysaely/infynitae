# The edge: design

What the edge (panels and desktop widgets) looks like and how it behaves. The owner approved this design on
2026-11-09 (claude.ai canvas "Infynitae widget modules", three boards) and the panel edit mode on 2026-11-10
(canvas "Infynitae panel edit mode"). The canvases live on a claude.ai account other sessions may not reach,
so their content is kept here:

- `widget-modules/1-desktop-and-panel.webp`: a 1920x1080 screen. Left, modules placed on the desktop under the
  windows (clock + weather, calendar with events, music); right, the same kind of modules stacked in a panel
  (quick settings, volume and brightness, system, memory, background apps, notifications).
- `widget-modules/2-music-sizes.webp`: the music module small, medium and large.
- `widget-modules/3-every-other-module.webp`: search, favorites, apps, windows, clipboard, power buttons, Wi-Fi.
- `panel-edit-mode/board.html` (+ `canvas.json`): the edit mode, interactive on the canvas; the markup and
  its script hold the exact sizes and the editing rules.

The pictures are drawn at twice the screen's size; the numbers below are screen pixels.

## One style for every module

- **A card per module**: a dark see-through card (the theme's surface colour, about 75-90% opaque, the
  wallpaper showing through), a 1 px outline a little lighter than the card, corners from `CORNERS`
  (8 px in the pictures). Padding 18 px left and right, 16 px top and bottom. 12 px between cards.
- **A title in 15 px semibold** at the top left. On the same line, right-aligned, at most one quiet hint in
  12 px muted text: a key ("Super+L"), a count ("Queue 12"), a total ("19.6 of 32 GB"), a state ("On"), or
  an action word ("Clear all", "Right click: more").
- **Content is text**: 13 px for names, 11-12 px muted for second lines and states. Small line icons
  (16-18 px, 2 px stroke) where an icon says something; never an icon on its own without a word, except the
  transport buttons and the power buttons (which have their word under them).
- **Thin bars**: 3-4 px tall, rounded, the track faint, the filled part in the accent. Sliders are the same
  bar with a small round knob.
- **The accent only on what is on, active or chosen**: a switch that is on (its icon and its state text), the
  playing song, today in the calendar, the selected row, the play/pause button, a running count ("2 new").
  Everything else is text colour or muted. **No filled blocks**: no tiles, no coloured squares behind
  switches.
- Colours, corners and the scheme come from the theme (`docs/EDGE_RUST.md`, "One look, one set of settings");
  nothing hard-coded, the accent is whatever the system theme says.
- Every number and name is real (no placeholders in the running edge). A module with nothing to show hides
  itself; a module whose data is still loading shows its title and one muted line ("Loading"), never "...".
- Every button on a card works (skip a song, close an app that eats memory, flip the month, connect to a
  network). Hover shows a faint row highlight; the pointer is a hand on what can be clicked.

## The modules

### Quick settings (`switches`)
Title "Quick settings", hint "Right click: more". A two-column list; each switch is an icon, its name
("Wi-Fi", "Bluetooth", "Do not disturb", "Night light", "Replay", "Power mode", "Keyboard", "Screen keys")
and under it its state in 11 px ("HomeNet", "Headphones", "Off", "Last 20 min", "Balanced", "US"). On: icon
and state in the accent. Click flips it (or steps it: power mode, keyboard layout); right click opens its
child card (networks, devices, durations).

### Volume and brightness (`sound`, `brightness`)
A line icon and "Volume" left, "60%" right; under it the slider. Brightness the same (only where a screen's
brightness can be set). Last line: "Output: Headphones" left, "Change" right (opens the output list).

### System (`system`)
Title "System", hint the GPU's name ("RX 7800 XT"). Three columns: GPU %, VRAM GB, Temp C; each a muted
11 px label, the value in 22 px semibold, a thin bar under. Optional last line: what runs on the GPU
("Training V17f · ends 18:20") with an action ("Pause").

### Memory (`memory`)
Title "Memory", hint "19.6 of 32 GB" (the used number in text colour). A thin bar. Then the apps that use the
most: name left, size right, an "x" to close it. Last line in the accent: "Close background apps · frees 2.4 GB"
(never the compositor, the session, the edge, training, the VM, terminals).

### Running in the background (`tray`)
Title "Running in the background", hint "Right click: menu". The tray as words in two columns: the app's name
and its status in 11 px ("online", "2 new" in the accent, "ready", "playing", "3 accounts"). Click activates,
right click opens its menu.

### Notifications (`notifications`)
Title "Notifications", hint "Clear all". Each: app name 11 px muted and age right ("2 min"), the summary in
13 px, the body in 12 px muted. Click opens, swipe or x removes.

### Clock (`clock`)
Small: "12:16" 18 px semibold left, "Tue 10 Nov" right. Medium/large (desktop): the time very large
(about 64 px, tight letter spacing), the date under it in 16 px, "Next alarm 07:30" with an alarm icon; on
the right, when set, the weather (icon, "12 C", "Partly cloudy", the city).

### Calendar (`calendar`)
Title: the month ("November"), arrows right. Weekday letters muted, days in 13 px, days of other months
dim, today as the accent rounded square, a small accent dot under days with events. Below the month, the
coming events: an accent bar, the title, the day and time muted. Last line: "+ Click a day to add an event or
reminder".

### Music (`media`), three sizes (`2-music-sizes.webp`)
- Small (a corner or a narrow panel): cover 48 px, title + artist, play/pause (accent square) and next.
- Medium: cover 60 px, title, "Artist · Melodyae"; the line being sung now and the next line (lyrics, when
  the player gives them); the progress bar; shuffle, previous, play/pause (accent), next, repeat; last line
  "Next: Song · Artist" left, "Queue 12" right.
- Large (resized bigger in edit mode): cover 92 px, title 22 px, "Artist · Album", "FLAC · 44.1 kHz ·
  Melodyae"; the line before, the line now (bigger), the next line; progress with times; the transport;
  "Up next" with "12 songs · 41 min" and the queue as rows (cover 30 px, song, artist, length). Taller shows
  more songs; it scrolls; a click plays that song. The queue is inside the card, never a pop-out.

### Search (`search`)
A bar card: magnifier icon, "Search apps, windows, files, settings" muted, the key ("Super+W") right.

### Favorites (`favorites`)
Title "Favorites", hint "Right click an app to pin". App icons 40 px in a row with the name under.

### Apps (`apps`)
Title "Apps", hint "Recent first". Rows: app icon 28 px, the name 14 px; "running" muted right for open apps.
The selected row has the faint highlight.

### Windows (`windows`)
Title "Windows", hint "Super+L" (the window list's key). Rows: the title in 14 px, the app in 11 px muted
under it, an "x" right to close; the focused window marked by a thin accent bar on its left.

### Clipboard (`clipboard`)
Title "Clipboard", hint "Super+V". Rows: a line icon for the kind (link, pinned, image, text), the text cut
to one line, the age or "pinned" right.

### Power buttons (`session`)
A card with five buttons in a row: Lock, Sleep, Log out, Restart, Shut down; a line icon over the word.

### Wi-Fi (child of the Wi-Fi switch, or a module)
Title "Wi-Fi", "On" right in the accent. Networks: signal icon, name ("HomeNet" in the accent when
connected), state right ("connected", "saved", "locked", "open"); "Hidden network..." last. A line under:
"DNS: automatic" left, "VPN: off" right (each opens its own small list).

## The panel and the desktop

- **One kind of module, two places**: the same modules placed on the desktop (under the windows, `WIDGETS`)
  or stacked in a panel (`PANEL_` lines, over the windows, opened by key or by hovering the screen edge).
- A panel is a column of cards, 380 px wide by default, at the screen edge it is set to, 24 px from the
  edges. Every card only as tall as what it shows. A panel taller than the screen scrolls.
- Cards can be half width (two side by side) where the module allows it (clock, music, calendar, system,
  memory).

## Editing (`panel-edit-mode/board.html`)

- Right click an empty spot on a panel (or a widget) to edit. The panel keeps its real look: cards stay cards,
  Quick settings stays one card.
- A bar above the cards: "Editing Control", Undo, Reset, Add, Done (the accent); a Width row with narrower /
  wider (steps of 20 px).
- Each card gets a small bar on top: its name, move up, move down, Half / Full (where allowed), its sizes
  (S / M / L, the chosen one in the accent), and an x to take it out. Cards get an accent outline.
- Quick settings shows an x beside each switch to take it out, and the taken-out ones as dashed "+ Name"
  buttons to put back.
- Add opens a list beside the panel: each module not in the panel with its name, a one-line description and
  an Add button; "Every module is in the panel." when none is left.
- Nothing is placed by pixels in a panel: order, half/full and size only, so nothing overlaps and no gap is
  left. (Desktop widgets keep free placement.)
- Done, Escape or closing the panel saves the `PANEL_` line; Undo steps back through the changes of this edit.
