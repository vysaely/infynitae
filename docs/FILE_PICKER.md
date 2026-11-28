# Infynitae file picker (design)

When an app asks to open or save a file today, Infynitae hands the request to
`xdg-desktop-portal-gtk` (`live/env.sh:215`). That window uses GTK's layout,
its own sidebar, and its own shortcuts, so it looks unlike the edge, settings,
lock screen, and screenshot tool.

Infynitae already runs its own portal (`live/portal/infynitae-portal.c`) for
global shortcuts, screen sharing, remote desktop, screenshots, and dark/light
accent settings. This document designs a file picker owned by Infynitae so our
portal can answer `org.freedesktop.portal.FileChooser` (`OpenFile`,
`SaveFile`, `SaveFiles`) with a window that matches the rest of the desktop.

An interactive drawing of the Open and Save windows using the default theme
colours is in [file-picker.html](mockups/file-picker.html). The portal picker
(stage 1, below) and the full file manager (stage 2, [FILE_MANAGER.md](FILE_MANAGER.md))
share one crate (`live/files/`) and one binary (`infynitae-files`): `--portal`
opens a single picker dialog, and running `infynitae-files` directly opens the
file manager. No code is written yet; the owner reviews this design first.

---

## Part 1: What you see and how it works

### The window

One window for opening files, choosing a folder, and saving files. It opens
centred over the app that asked for it, with the same 8 px corners, 1 px
border, font, and colours as the edge and settings.

- **Left column (Places).** Three groups, in order:
  1. **Recent.** Files opened or saved lately, read from the same list the
     launcher searches (`~/.local/share/recently-used.xbel`), newest first.
     Each row shows the file name and its folder below it. Files picked or
     saved in our picker are added to this list too, so they keep showing up
     in Super+W.
  2. **Home folders.** `Home`, then the folders from
     `~/.config/user-dirs.dirs` that exist (`Documents`, `Downloads`,
     `Pictures`, `Videos`, `Music`, `Desktop`), followed by any saved folder
     bookmarks (`~/.config/gtk-3.0/bookmarks`).
  3. **Drives.** Drives named by Roman numeral (`I`, `II`, `III`, `IV`, `V`,
     `VI` and up, per [DRIVES.md](DRIVES.md)), sorted in numerical order,
     followed by any USB drive mounted under `/run/media/<you>/`. Each mounted
     drive shows a thin 3 px usage bar. An internal partition in `/etc/fstab`
     that has not been mounted since boot (such as a second disk or Windows
     partition) still appears in muted text and mounts when you click it.
- **Top bar.**
  - **Back, Forward, Up** buttons (16 px line icons).
  - **Path bar (breadcrumbs and type-a-path).** Shows the current folder as
    clickable parts (`Home / Documents / Notes`). Clicking a part jumps there.
    Clicking the empty space at the right of the bar, or pressing `Ctrl+L`,
    turns it into a text field where you can type or paste a path (`~` works,
    `Tab` completes folder names, `Enter` opens it, `Escape` leaves it).
  - **Search.** Typing in the search box (`Ctrl+F`) filters the current folder
    as you type. A switch next to the box changes scope from **This folder**
    to **Everywhere**, which queries `plocate` in the background (the same way
    Super+W searches files; each keystroke stops the older search).
  - **View and tools.** Four small controls at the right of the bar: switch
    between **List** and **Grid** view, toggle the **Preview** pane, show or
    hide dot-files (`Ctrl+H`), and **New folder** (`Ctrl+Shift+N`), which opens
    an inline name box above the list instead of a second pop-up window.
- **Main file area.**
  - **List view** (default): folders first, then files. Columns for Name,
    Modified, and Size; click a column header to sort or reverse it. Rows are
    34 px tall with a 16 px line icon, faint highlight under the pointer, and
    the theme's accent tint on selected rows.
  - **Grid view:** square tiles with image thumbnails for pictures and line
    icons for other files, useful in `Pictures`, `Downloads`, and wallpapers.
    Thumbnails load in the background so opening a large folder never stalls.
  - Empty folders and empty searches say so plainly ("Empty folder", "No files
    match this filter").
- **Right column (Preview, collapsible).** Hidden or shown with one button or
  `Alt+P` (and hidden automatically if the window is narrowed below 680 px).
  - Pictures (PNG, JPEG, WebP, GIF): scaled picture, pixel size, file size,
    modified date.
  - Plain text, code, and config files: the first lines of the file in your
    monospace font, file size, modified date.
  - Folders: item count and modified date.
  - Everything else: file type, size, modified date.
  - Multiple files selected: count and combined size.
- **Bottom bar.**
  - **Save mode (`SaveFile`).** A `Name` field pre-filled with the app's
    suggested name, with the name selected before the dot (so typing `budget`
    over `untitled.pdf` gives `budget.pdf`). Saving over an existing file
    shows an inline confirmation bar in the theme's alert colour (`report.pdf
    already exists. Replace it?` with `Cancel` and `Replace`) instead of a
    separate pop-up.
  - **Save multiple files (`SaveFiles`).** Picks a destination folder and lists
    the file names the app wants to save; if any name already exists in that
    folder, an inline notice asks whether to rename the colliding files
    (`name (2).ext`) or replace them.
  - **Extra app options (`choices`).** Some apps pass extra controls, such as
    text encoding or a "Read-only" checkbox. When an app passes them, they
    appear in one row above the buttons. When an app passes none (most
    apps), that row is not drawn.
  - **Filters.** On the left of the button row, only when the app supplies
    filters (for example `Images (*.png, *.jpg)`). An `All files` option is
    always added at the end so you are never locked out of picking a file with
    an unusual extension.
  - **Buttons.** On the right: `Cancel` and the action button (`Open`, `Save`,
    `Select folder`, or the label the app gave), filled in the accent colour.
    The action button stays inactive until a valid file or folder is chosen.

### Keyboard use

Every part of the window works without a mouse.

| Key | Action |
| :--- | :--- |
| `ArrowUp` / `ArrowDown` (and `Left` / `Right` in Grid) | Move selection in the file list |
| `Shift` + Arrow, `Space`, `Ctrl+A` | Extend selection, toggle item, select all (when `multiple` is allowed) |
| `Enter` | Open highlighted folder, or accept selected file(s) / save |
| `Backspace` or `Alt+Up` | Go to parent folder (when no text box has focus) |
| `Alt+Left` / `Alt+Right` | Back / forward in folder history |
| `Ctrl+L` | Edit the path bar (`Tab` completes, `Enter` opens, `Escape` cancels) |
| `Ctrl+F` | Focus the search box (`Escape` clears search and returns to the list) |
| `Ctrl+H` | Show or hide hidden files (names starting with `.`) |
| `Ctrl+Shift+N` | Create a new folder inside the current folder |
| `Ctrl+1` / `Ctrl+2` | Switch to List / Grid view |
| `Alt+P` | Show or hide the Preview pane |
| `Tab` / `Shift+Tab` | Move focus between Places, File list, Name field (Save), Filter, Buttons |
| `Escape` | Close an open menu, inline bar, search, or path edit first; otherwise cancel and close the window |

### Matching the edge and settings look

Infynitae already resolves all desktop colours in one place
(`live/edge-apps/src/theme.rs`, written by `live/settings.sh` to
`~/.cache/infynitae/look`). The picker reads that file on startup, the same
way `live/shot-rs/src/look.rs` does, using every token in it:

| Token in `~/.cache/infynitae/look` | Shipped default | Used for |
| :--- | :--- | :--- |
| `LOOK_CARD` | `#111317` | Window background (opaque so text stays legible over any app) |
| `LOOK_EDGE` | `#333b54` (90%) | 1 px dividers, input outlines, `Cancel` button outline |
| `LOOK_TEXT` | `#eef2f8` | File names, headings, active icons |
| `LOOK_MUTED` | `#a7abb0` | Dates, sizes, paths, section names, inactive icons |
| `LOOK_ACCENT` | `#7aa2f7` | Active place, selected row tint, primary button fill, focus outline, drive bars |
| `LOOK_ON_ACCENT` | `#111317` | Label on the filled primary button |
| `LOOK_ACCENT_TEXT` | `#7aa2f7` | Accent text when readable at 3:1 contrast on `LOOK_CARD` |
| `LOOK_ALERT` | `#ed6a6f` | Overwrite warning, permission or write errors |
| `LOOK_TRACK` | `#eef2f8` (12%) | Empty half of drive usage bars, switches that are off |
| `LOOK_CORNERS` | `8` px | Window outer corners (drawn by the compositor from `WINDOW_CORNERS`) |
| `LOOK_SMALL_RADIUS` | `4` px | Buttons, fields, hover rows, selection boxes, preview frame |
| `LOOK_FONT` | `sans-serif` | All UI text (`TERMINAL_FONT` for text file previews) |

Type sizes match `live/edge-apps/src/draw.rs`: 15 px semibold window title,
13 px names and controls, 12 px secondary states and buttons, 11 px muted
notes and column headers. Line icons are 16 px with a 1.6 px stroke
(`live/edge-apps/src/ui.rs`). No gradients, drop shadows, emoji, or
hard-coded colours. Switching to light mode, a wallpaper accent, or any
`THEME_SCHEME` changes the file picker on the next open.

### Which apps use it, and which do not

Only apps that ask `xdg-desktop-portal` for a file dialog show our window.
Other apps draw their own dialog inside their own process, so the desktop
cannot intercept them without toolkit settings.

| App group | Uses our picker right away? | What you see and what can change |
| :--- | :--- | :--- |
| **Flatpak apps** | Yes | The Flatpak sandbox routes every file dialog through the portal. |
| **Firefox (native)** | Only with a pref | Defaults to GTK's built-in picker. Setting `widget.use-xdg-desktop-portal.file-picker = 1` in Firefox's profile switches it to ours; `live/settings.sh` already manages Firefox theme files per profile and can set this if you want. |
| **Qt 5 / Qt 6 apps (native)** | Only with a qt6ct option | `live/env.sh:35-39` sets `QT_QPA_PLATFORMTHEME="qt6ct"`. `live/settings.sh:1185` currently writes `standard_dialogs=default` in `~/.config/qt6ct/qt6ct.conf` (and `qt5ct.conf`); changing that line to `standard_dialogs=xdgdesktopportal` routes Qt's `QFileDialog` through the portal. |
| **GTK 3 and GTK 4 apps (native)** | No by default | Unsandboxed GTK 3 apps use GTK's built-in chooser unless `GTK_USE_PORTAL=1` is set; even then, older GTK 3 apps that call `GtkFileChooserDialog` directly instead of `GtkFileChooserNative` (such as GIMP 2) still draw GTK's own window. GTK 4 dropped `GTK_USE_PORTAL` and only forces the portal outside a sandbox when `GDK_DEBUG=portals` is set. GTK's own windows still keep Infynitae's colours via `gtk.css`. |
| **Chromium and Electron apps** (Discord, VS Code, Obsidian) | Mixed | Flatpak builds use the portal. Native builds vary by Chromium/Electron version: newer versions detect `xdg-desktop-portal` or honour `GTK_USE_PORTAL=1`; older Electron builds link GTK 3 directly and ignore the portal. |
| **Steam** | No | Steam draws its own file dialogs (CEF / VGUI / GTK) and does not call the portal. |
| **Wine and Proton** (including `live/windows-run`) | No | Windows programs call Win32 `GetOpenFileName` / `IFileOpenDialog`, which Wine draws itself showing `C:\` and `Z:\`. Programs under `live/windows-run` also have no access to the session D-Bus ([SECURITY.md](SECURITY.md)). |
| **Blender, Godot, Java, Tk, games** | No | Draw their own file browsers inside the app window. |

If our picker fails to start or exits unexpectedly, `infynitae-portal` falls
back to `xdg-desktop-portal-gtk` so you can still open or save your work. A
config key (`FILE_PICKER="gtk"`) switches back to the GTK picker at any time.

### Questions for the owner before writing code

1. **Firefox and Qt wiring:** Should `live/settings.sh` turn on the portal file
   picker in Firefox profiles (`user.js`) and `qt6ct.conf` / `qt5ct.conf`
   (`standard_dialogs=xdgdesktopportal`) automatically once stage 1 is ready,
   or keep that behind a config option?
2. **Native GTK apps (`GTK_USE_PORTAL=1` / `GDK_DEBUG=portals`):** Leave unset
   by default (safer for older GTK 3 apps with custom preview widgets, and
   avoids setting a GTK debug flag globally) or opt into portal dialogs in
   `live/env.sh`?
3. **Preview pane default:** Start hidden (narrower window, `Alt+P` to open)
   or open by default when the window is wide enough (920 px)?
4. **Pictures folder default view:** Always start in List view, or switch to
   Grid view automatically when opening `~/Pictures` or when the app filters
   for images?

---

## Part 2: Technical design

### 1. Portal interface (`org.freedesktop.impl.portal.FileChooser`)

`infynitae-portal` owns `org.freedesktop.impl.portal.desktop.infynitae` at
`/org/freedesktop/portal/desktop` (`live/portal/infynitae-portal.c:26-27`). We
add `live/portal/file-chooser.c` and `live/portal/file-chooser.h`, registered
in `on_bus_acquired` next to `remote_desktop_init`, `screenshot_init`, and
`portal_settings_init` (`infynitae-portal.c:735-739`).

Only `xdg-desktop-portal` may call it, enforced by the existing
`from_portal(c, sender)` check (`live/portal/remote-desktop.c:749`).

#### Methods and Request cancellation

All three methods on `org.freedesktop.impl.portal.FileChooser` share the D-Bus
signature `(osssa{sv}) -> (ua{sv})`:

```
OpenFile (o handle, s app_id, s parent_window, s title, a{sv} options) -> (u response, a{sv} results)
SaveFile (o handle, s app_id, s parent_window, s title, a{sv} options) -> (u response, a{sv} results)
SaveFiles(o handle, s app_id, s parent_window, s title, a{sv} options) -> (u response, a{sv} results)
```

Response codes follow `org.freedesktop.portal.Request`: `0` = user picked,
`1` = user cancelled, `2` = ended another way (closed by caller or error).

While a request is open, `file-chooser.c` exports
`org.freedesktop.impl.portal.Request` (single method `Close()`) on `handle`
and keeps a table of active requests (`handle -> RequestEntry { inv, reg_id,
child, cancellable, ... }`). Multiple apps may have pickers open at the same
time. When `xdg-desktop-portal` calls `Request.Close()` on `handle` (for
example, the calling app closed), `file-chooser.c` sends `SIGTERM` to the
picker process, unregisters `handle`, and completes the D-Bus invocation with
response `2`.

#### Options and results

| Key | D-Bus type | Methods | Handling |
| :--- | :--- | :--- | :--- |
| `accept_label` | `s` | all | Strip GTK mnemonic underscores (`_Open` -> `Open`, `__` -> `_`); fall back to `Open`, `Select folder`, or `Save`. |
| `modal` | `b` | all | Default `true`. Passed to the UI; with `xdg_foreign` parent set, the compositor places the picker centred over the parent. |
| `multiple` | `b` | `OpenFile` | Default `false`. Enables Shift/Ctrl multi-select and `Ctrl+A`. |
| `directory` | `b` | `OpenFile` | Default `false`. Switches accept action to choosing folder(s); entering a folder still navigates, and accepting with no child selected picks the current folder. |
| `filters` | `a(sa(us))` | `OpenFile`, `SaveFile` | Each entry is `(label, [(kind, pattern)...])` where `kind == 0` is a case-sensitive glob (`*.png`, `*.[jJ][pP][gG]`) and `kind == 1` is a MIME type (`image/png`, `image/*`). Always append an `All files` choice unless one matching `*` is already present. |
| `current_filter` | `(sa(us))` | `OpenFile`, `SaveFile` | Initial active filter. If `filters` is empty, apply this filter unconditionally as the sole filter (plus `All files`). Returned in `results["current_filter"]` when the user accepts. |
| `choices` | `a(ssa(ss)s)` | all | Each entry is `(id, label, [(opt_id, opt_label)...], initial)`. Empty option list = boolean switch (`"true"` / `"false"`). Non-empty list = inline selector. Returned in `results["choices"]` as `a(ss)`. |
| `current_folder` | `ay` | all | Nul-terminated raw filesystem path bytes. Strip trailing `\0`; if it exists and is a directory, start there. |
| `current_name` | `s` | `SaveFile` | Initial text in the `Name` box (strip any `/` so a caller cannot sneak path traversal into the filename field). |
| `current_file` | `ay` | `SaveFile` | Nul-terminated raw path of an existing file being saved. Split into starting folder (takes precedence over `current_folder` if valid) and initial filename (takes precedence over `current_name`). |
| `files` | `aay` | `SaveFiles` | Array of nul-terminated filenames. Picker chooses a folder; output `uris` contains one `file://` URI per input filename in the same order. |
| `uris` (out) | `as` | all | Normalized `file://` URIs, percent-encoded from raw path bytes. `SaveFile` returns exactly one URI. |
| `writable` (out) | `b` | `OpenFile` | Returned as `true` by default (unless a read-only choice is added later), so sandboxed editors receive write access through the document portal. |

#### MIME and glob filter matching

Apps frequently send MIME filters such as `(1, "image/*")` or `(1, "text/plain")`
alongside globs. Instead of reading file contents on every directory open or
linking a C MIME library, the Rust picker loads the system `shared-mime-info`
tables already present on Arch Linux:

- `/usr/share/mime/globs2` (glob pattern -> MIME type, weighted)
- `/usr/share/mime/subclasses` (subtype -> parent MIME type, so
  `application/x-shellscript` and `text/markdown` match `text/plain`)
- `/usr/share/mime/aliases` (equivalent names such as `text/xml` and
  `application/xml`)

Directories always stay visible regardless of the active filter so the user
can navigate into them.

### 2. Where the code lives and what it reuses

Two pieces:

1. **C bridge in `live/portal/` (`file-chooser.c`, `file-chooser.h`).**
   Follows `live/portal/screenshot.c` (112 lines) and `live/portal/settings.c`
   (220 lines). Does no drawing and no filesystem browsing; it validates the
   caller, exports `Request`, converts the D-Bus `GVariant` arguments to JSON,
   spawns the Rust UI asynchronously, and turns its stdout JSON back into a
   D-Bus reply. Built into `infynitae-portal` by `live/build-helpers.sh`.
2. **Standalone Rust crate `live/files/` (binary `infynitae-files`).**
   Shared between the portal picker (`infynitae-files --portal`) and the file
   manager (`infynitae-files`, designed in [FILE_MANAGER.md](FILE_MANAGER.md)).
   Structured like `live/shot-rs/` and `live/edge-apps/`: its own `Cargo.toml`,
   `Cargo.lock`, and `[workspace]`, built by `build_rust` in
   `live/build-helpers.sh`, installed to `~/.local/bin/infynitae-files`
   with `.previous` rollback support, and checked in `.github/workflows/ci.yml`
   (fmt, clippy, `cargo-machete`, tests, `cargo-audit`, `verify-theme-colours`).

#### Why a standalone crate instead of inside `live/edge-apps`

`live/edge-apps` is a long-running `wlr-layer-shell` daemon (one instance per
session) that runs without `NO_NEW_PRIVS` because it launches user apps and
terminals ([SECURITY.md](SECURITY.md)). A file picker has the opposite shape:

- It opens as a regular resizable **`xdg_toplevel`** window so the compositor
  can parent it over the calling app, round its corners with `WINDOW_CORNERS`,
  and let the user move or resize it.
- Two apps can open file pickers at the same time; one short-lived process per
  dialog keeps them isolated and holds 0 bytes of memory when no dialog is
  open.
- A slow USB stick, hung network mount, or malformed image cannot freeze the
  edge panels or notification server.
- Because `infynitae-portal` spawns it, the picker inherits
  `PR_SET_NO_NEW_PRIVS` and empty inheritable capabilities automatically.

#### Code and stack reused from `live/edge-apps` and `live/shot-rs`

The crate uses the same Wayland and drawing stack already built and audited in
`live/edge-apps/Cargo.toml` and `live/shot-rs/Cargo.toml`:
`smithay-client-toolkit` 0.20 (using `xdg_shell` instead of `wlr_layer`),
`wayland-client` 0.31, `wayland-protocols` 0.32 (`zxdg_importer_v2`,
`wp_fractional_scale_v1`, `wp_viewporter`, `wp_cursor_shape_v1`, and in
stage 3 `zwp_text_input_v3`), `cairo-rs` 0.20 (`png` feature), `pango` 0.20,
`pangocairo` 0.20, `serde` + `serde_json`, `libc`.

Parts to mirror or share with `live/edge-apps` and `live/shot-rs`:

| Concern | Existing source | How `live/files` uses it |
| :--- | :--- | :--- |
| Theme tokens | `live/shot-rs/src/look.rs` (reads 6 keys) and `live/edge-apps/src/theme.rs` (prints 21 `LOOK_*` keys) | Read all 21 keys from `$XDG_CACHE_HOME/infynitae/look` with `config/config` defaults if missing. |
| Drawing primitives | `live/edge-apps/src/draw.rs`, `src/ui.rs` | Same `round_rect`, `bar`, Pango font/ellipsize helpers, `SIZE_*` sizes, `LIGHT_ALPHA` (0.08), `CHOSEN_ALPHA` (0.16), 16 px / 1.6 px line icons. |
| Roman-numeral drives | `live/edge-apps/src/cards/files.rs` (`parse_fstab`, `parse_mountinfo_and_fstab`, `roman_to_number`, `sort_drives`, `statvfs_sizes`) | Same `/etc/fstab` + `/proc/self/mountinfo` + `/dev/disk/by-label` + `/run/media/<user>/` rules, extended to keep unmounted `x-gvfs-show` entries whose `UUID=` exists in `/dev/disk/by-uuid/` and mount them via `udisksctl mount` on click. |
| Home folders, bookmarks, recent files | `live/edge-apps/src/cards/files.rs` (`parse_user_dirs`, `parse_gtk_bookmarks`), `src/cards/search.rs` (`parse_xbel`) | Same XDG order and `recently-used.xbel` parsing, plus atomic write-back (temp file + rename) to add picked/saved files to `recently-used.xbel`. |
| Background file search | `live/edge-apps/src/locate.rs` (`Locator`) | Same worker-thread `plocate -i -b -l 40` pattern where the next keystroke kills the previous child. |

Stage 1 needs **no new third-party crate** beyond what `live/edge-apps` and
`live/shot-rs` already lock (PNG previews work through `cairo-rs`'s existing
`png` feature). Stage 2 adds JPEG/WebP/GIF thumbnail decoding; the two options
are the `image` crate (default features disabled, only `png`, `jpeg`, `webp`,
`gif` enabled) or `gdk-pixbuf2` (already in `live/packages.txt:10`). The
`image` crate avoids pulling GObject into the Rust UI and stays inside
`cargo-audit`'s nightly checks.

If a third Rust helper later needs `look` or `drives`, those small pure-Rust
modules can move into a shared path crate under `live/`; until then, keeping
`live/files` independent avoids churning `live/edge-apps` and
`live/shot-rs`.

### 3. Process launch, parenting, and URI handoff

```
App (native or Flatpak)
  |  org.freedesktop.portal.FileChooser.{OpenFile,SaveFile,SaveFiles}
  v
xdg-desktop-portal  (rewrites /run/user/<uid>/doc/... -> host path on input)
  |  org.freedesktop.impl.portal.FileChooser.*
  v
infynitae-portal  (live/portal/file-chooser.c)
  |-- checks from_portal(conn, sender)
  |-- registers org.freedesktop.impl.portal.Request on `handle`
  |-- spawns ~/.local/bin/infynitae-files --portal
  |     stdin  <- one JSON object (method, title, parent_window, options;
  |                raw byte paths base64-encoded so non-UTF-8 survives)
  |     stdout -> one JSON object ({ "response": 0|1, "uris": [...],
  |                "choices": [...], "current_filter": ... })
  |-- waits asynchronously in GLib main loop (never blocks shortcuts/screencast)
  |-- on non-zero exit / spawn error -> forwards call to org.freedesktop.impl.portal.desktop.gtk
  v
xdg-desktop-portal  (for sandboxed apps, registers host URIs in xdg-document-portal)
  |  org.freedesktop.portal.Request::Response
  v
App
```

Why JSON over stdin/stdout pipes instead of command-line arguments or temp
files:

- Filenames, filter lists, and `SaveFiles` lists can be long and must not be
  exposed in `/proc/<pid>/cmdline`.
- Non-UTF-8 Linux path bytes (`ay` in D-Bus) round-trip as base64 without
  loss.
- No temporary files remain on disk if the picker or portal exits abruptly.
- Manual and VM testing can drive the binary directly with
  `infynitae-files --portal < req.json` or simple CLI flags.

#### Window parenting (`parent_window`)

On Wayland, apps export their toplevel via `xdg-foreign` (`zxdg_exporter_v2`)
and pass `parent_window = "wayland:<handle>"`. Infynitae's compositor already
implements `xdg_foreign` (`crates/infynitae-wayland/src/handlers.rs:1413,1458`),
and its placement rule centres child toplevels over their parent window
([WINDOWS.md](WINDOWS.md)).

When `parent_window` starts with `"wayland:"`, `infynitae-files --portal` binds
`zxdg_importer_v2`, calls `import_toplevel(handle)`, and invokes
`set_parent_of(&xdg_surface)` before the first commit. When `parent_window` is
empty or starts with `"x11:"` (Xwayland apps cannot be imported by a Wayland
client through `zxdg_importer_v2`), the picker opens as a regular unparented
toplevel placed by `PLACEMENT_POLICY`.

#### Privileges and mounting unmounted drives

`infynitae-portal` drops privileges at startup (`prctl(PR_SET_NO_NEW_PRIVS, 1)`
in `infynitae-portal.c:1031` and `setpriv --no-new-privs --inh-caps=-all` in
`live/env.sh:243`). The spawned `infynitae-files --portal` inherits those
restrictions, so it can never escalate privileges.

Clicking an unmounted Roman-numeral drive in the sidebar mounts it through
UDisks2 (`udisksctl mount -b /dev/disk/by-uuid/<uuid>` or a direct D-Bus call
to `org.freedesktop.UDisks2`). `udisksctl` is an ordinary unprivileged D-Bus
client, not a SUID binary, and `/etc/polkit-1/rules.d/50-infynitae-drive-names.rules`
authorizes `filesystem-fstab` mounts for the active local session without a
password ([DRIVES.md](DRIVES.md), [SECURITY.md](SECURITY.md)), so mounting
works under `NO_NEW_PRIVS`.

### 4. Sandboxed apps (Flatpak document portal)

`infynitae-files --portal` runs on the host session outside the calling app's
sandbox, so it sees the user's real home folders and drives `I`–`VI`.

`xdg-desktop-portal` owns the boundary with `xdg-document-portal`
(`org.freedesktop.portal.Documents`), so our picker does not talk to the
document portal directly:

- **Input paths:** If a sandboxed app passes a `current_folder` or
  `current_file` inside `/run/user/<uid>/doc/<id>/...`, `xdg-desktop-portal`
  translates it back to the host path before calling our backend.
- **Output URIs:** Our backend returns canonical `file://` URIs on the host
  (absolute, no `.` or `..`, percent-encoded from the raw path bytes). For a
  sandboxed app, `xdg-desktop-portal` registers each path in
  `xdg-document-portal` and hands the app a FUSE path under
  `/run/user/<uid>/doc/<id>/...` granting access to those files alone.
- **`OpenFile` write access:** Our backend returns `writable = true` unless
  read-only is chosen, so sandboxed editors can save changes back to a file
  the user opened.
- **`SaveFile` and `SaveFiles`:** The target file need not exist yet, but its
  parent directory must exist and be writable on the host; the picker checks
  this before accepting so `xdg-document-portal` does not fail after the window
  closes.

### 5. Portal routing and fallback to GTK

`xdg-desktop-portal-gtk` stays installed (`live/packages.txt:14`): it still
answers `AppChooser`, `Notification`, and `Settings` keys Infynitae does not
override, and it is also the fallback file picker.

Fallback works at two points:

1. **Session startup / config (`live/env.sh` and `config/config`).**
   Add `FILE_PICKER="auto"` (options: `auto`, `infynitae`, `gtk`) to
   `config/config`. Just like `live/env.sh:168-186` already probes
   `infynitae-portal` for `ScreenCast` and `Settings` so a script still works
   with an older binary installed before an update, `live/env.sh` checks that
   both `org.freedesktop.impl.portal.FileChooser` is in
   `~/.local/bin/infynitae-portal` and `~/.local/bin/infynitae-files` is
   executable (cached in an env var so `Super+key` presses do not grep the
   binary).
   - When available and `FILE_PICKER != "gtk"`: `infynitae.portal` includes
     `org.freedesktop.impl.portal.FileChooser;` and `infynitae-portals.conf`
     sets `org.freedesktop.impl.portal.FileChooser=infynitae;gtk`.
   - Otherwise: `infynitae.portal` omits it and `infynitae-portals.conf` sets
     `org.freedesktop.impl.portal.FileChooser=gtk`.
   - `infynitae-reload portal` already restarts `xdg-desktop-portal.service`
     whenever either file changes (`live/infynitae-reload:215-226`).
2. **Runtime fallback (`live/portal/file-chooser.c`).**
   `xdg-desktop-portal` picks one `FileChooser` backend at startup and does not
   re-try the second backend in `infynitae;gtk` if a call fails at runtime.
   Therefore, if `infynitae-files` cannot be spawned, dies from a signal,
   exits with a non-zero code, or returns invalid JSON, `file-chooser.c` logs
   the failure to `~/.local/state/infynitae/portal.log` and forwards the exact
   D-Bus call (`OpenFile`, `SaveFile`, or `SaveFiles`, same arguments) to
   `org.freedesktop.impl.portal.desktop.gtk` at
   `/org/freedesktop/portal/desktop`, relaying `Request.Close()` if needed and
   returning GTK's reply.

### 6. Size estimate

Measured sizes of existing helpers in this repository for scale:
`live/portal/screenshot.c` is 112 lines, `live/portal/settings.c` is 220 lines,
`live/edge-apps/src/cards/files.rs` is 2,018 lines (1,282 code + 736 tests),
and `live/shot-rs/src/` is 6,491 lines total.

| Component | Path | Estimated lines |
| :--- | :--- | :--- |
| Portal C bridge + Request table + GTK fallback | `live/portal/file-chooser.{c,h}`, edits to `infynitae-portal.c`, `env.sh`, `build-helpers.sh`, `config/config` | 300–400 |
| Stage 1 Rust core (protocol JSON, state, navigation, filters, choices, MIME tables, `SaveFiles` collisions) | `live/files/src/{portal/,mime.rs}` | 900–1,200 |
| Stage 1 places & filesystem (drives I–VI, XDG folders, `recently-used.xbel` read/write, worker thread) | `live/files/src/{places,fs_list,recent}.rs` | 400–550 |
| Stage 1 theme, layout, Cairo/Pango rendering, text inputs | `live/files/src/{look,draw,ui,text_input}.rs` | 800–1,050 |
| Stage 1 Wayland client (`xdg_toplevel`, `xdg_foreign`, shm buffers, keyboard/pointer/clipboard) | `live/files/src/wayland.rs` | 550–750 |
| Stage 2 (grid view, async thumbnails, preview pane, `plocate` search, unmounted drive mount, bookmarks) | `live/files/src/{grid,preview,thumbs,locate}.rs` | 1,000–1,400 |
| Stage 3 (per-app state memory, `zwp_text_input_v3`, fractional scale, optional Firefox/Qt wiring) | `live/files/src/` + `live/settings.sh` | 500–800 |
| Tests (Rust unit tests, `tools/test-portal-file-chooser`, `tools/vm-edge/pfilepicker.py`) | `live/files/`, `tools/` | 1,000–1,400 |
| **Total (picker stages, code + tests)** | | **5,450–7,550** |

Stage 1 alone (code without tests) is roughly **3,000–4,000 lines**. Additional
file manager modules built on top of this shared crate are sized in
[FILE_MANAGER.md](FILE_MANAGER.md).

### 7. Stages

#### Stage 1: smallest useful daily picker
- `live/portal/file-chooser.{c,h}`: `OpenFile`, `SaveFile`, `SaveFiles`,
  `from_portal` check, `Request.Close()` cancellation, async wait, GTK runtime
  fallback, `live/env.sh` probe and `FILE_PICKER` config key.
- `live/files/`: `xdg_toplevel` with `xdg_foreign` parent attachment,
  21-token theme loader (`~/.cache/infynitae/look`), Places sidebar (Recent,
  Home + XDG folders, mounted drives I–VI), List view with sorting, breadcrumb
  and `Ctrl+L` type-a-path with `Tab` completion, current-folder search
  (`Ctrl+F`), New folder (`Ctrl+Shift+N`), hidden files (`Ctrl+H`), filters
  (globs + `/usr/share/mime`), portal `choices`, Save filename field with
  inline overwrite bar, `SaveFiles` folder picker with collision suffix/prompt,
  full keyboard navigation, atomic update of `recently-used.xbel`.
- Unit tests, `tools/test-portal-file-chooser`, and `tools/vm-edge/pfilepicker.py`.

#### Stage 2: visual browsing and whole-computer search
- Grid view (`Ctrl+2`) with background image thumbnail decoder (PNG, JPEG,
  WebP, GIF) and bounded memory cache, cancelled immediately on folder switch.
- Collapsible Preview pane (`Alt+P`): image preview with dimensions, 16 KiB
  capped UTF-8 text preview, folder item count, metadata for other files.
- **Everywhere** search mode using `plocate` (cancellable worker thread like
  `live/edge-apps/src/locate.rs`).
- Unmounted Roman-numeral partitions from `/etc/fstab` shown muted in Places
  and mounted on click via UDisks2; GTK bookmarks (`~/.config/gtk-3.0/bookmarks`)
  included in Places.

#### Stage 3: per-app memory, IME, and wider app reach
- Remember window size and last-used folder per `app_id` in
  `$XDG_STATE_HOME/infynitae/file-picker.json` (used only when the app omits
  `current_folder`).
- `zwp_text_input_v3` for IME/non-Latin typing, `wp_fractional_scale_v1` +
  `wp_viewporter`, `wp_cursor_shape_v1`, mouse back/forward buttons.
- Optional wiring for Firefox (`widget.use-xdg-desktop-portal.file-picker`),
  Qt (`standard_dialogs=xdgdesktopportal` in `qt6ct.conf` / `qt5ct.conf`), and
  tested `GTK_USE_PORTAL=1` toggle if the owner wants them.

### 8. Risks

1. **Non-UTF-8 filenames.** Linux paths are raw byte arrays (`ay` on D-Bus).
   Any Rust code that converts paths through `String` alone would corrupt or
   drop non-UTF-8 files. Mitigation: carry raw `OsString` / `Vec<u8>`
   internally, base64-encode raw bytes across the portal JSON pipe, display
   lossy UTF-8 in the UI, and percent-encode the original bytes into `file://`
   URIs.
2. **Slow or huge directories.** Opening `/usr/bin`, a folder with 50,000
   files, a slow USB drive, or an unreachable network mount on the UI thread
   would freeze repainting and input. Mitigation: run `read_dir`, `stat`,
   thumbnail decoding, and preview reads on worker threads keyed by a
   generation number so switching folders cancels stale work; show a plain
   `Loading` line while waiting; cap preview text reads at 16 KiB and folder
   item counts at 1,000 (`1,000+ items`).
3. **Untrusted files in Preview and Thumbnails.** A user opening `~/Downloads`
   may hover or scroll past a crafted image file. Mitigation: cap decoded
   dimensions (for example 64 megapixels / 256 MiB decompressed limit), never
   shell out to external converters, skip SVG rendering in the picker (show
   text/metadata instead), and add `cargo-fuzz` targets in `fuzz/` for the
   JSON request parser, `recently-used.xbel`, and glob/MIME parsers.
4. **Concurrent dialogs and caller aborts.** Two apps can open dialogs at once,
   or a caller can quit while the dialog is open. Mitigation: track active
   requests in a hash table by `handle`, export `Request` per handle, never
   block GLib's main loop, and `SIGTERM` the matching child on `Request.Close`.
5. **Runtime GTK fallback compatibility.** Forwarding an `impl.portal` D-Bus
   call from `infynitae-portal` to `org.freedesktop.impl.portal.desktop.gtk`
   assumes Arch's `xdg-desktop-portal-gtk` accepts session-bus calls from
   another host process without a `from_portal`-style sender check. Mitigation:
   verify this explicitly in the VM against Arch's packaged
   `xdg-desktop-portal-gtk` during stage 1; if upstream ever restricts the
   sender, log the error clearly and rely on `FILE_PICKER="gtk"` +
   `infynitae-reload portal`.
6. **`GTK_USE_PORTAL=1` regressions.** Some older GTK 3 apps embed custom
   widgets into `GtkFileChooserDialog` or misbehave when forced onto the
   portal. Mitigation: do not set `GTK_USE_PORTAL=1` globally in stage 1; keep
   it opt-in until tested across the owner's GTK apps.
7. **Theme and drive logic drift.** Copying `look` or `drives` logic from
   `live/edge-apps` risks divergence if `live/settings.sh` or `docs/DRIVES.md`
   changes later. Mitigation: test the `LOOK_*` parser against the exact
   output of `infynitae-edge-apps --print-theme` and keep the `fstab` parsing
   contract shared with `tools/test-automount` / `live/edge-apps` tests (or
   extract a small shared crate once stage 1 lands).
8. **Symlink loops and save overwrites.** Following symlinks during folder
   counting or search can loop; saving without checking the resolved target
   can overwrite silently. Mitigation: do not follow directory symlinks during
   counts, resolve `SaveFile` target before checking existence, and require
   explicit confirmation before replacing any existing file.

### 9. Testing plan

#### Automated tests (run in CI without a screen)

1. **`live/files` unit tests (`cargo test --locked`, new CI job):**
   - Portal JSON serialization/deserialization, including non-UTF-8 raw path
     bytes (`ay` -> base64 -> percent-encoded `file://` URI), mnemonic `_`
     stripping, `current_file` splitting, and `SaveFiles` collision renaming.
   - Glob (`kind = 0`, case-sensitive) and MIME (`kind = 1`, wildcards and
     `/usr/share/mime/subclasses` inheritance) filtering.
   - Places discovery on temporary fixtures: fake `fstab` with drives `I`–`VI`
     (mounted and unmounted), `mountinfo`, `/run/media`, `user-dirs.dirs`,
     `bookmarks`, and `recently-used.xbel` read + atomic write-back.
   - State-machine keyboard tests (navigation, `multiple` selection, `directory`
     mode, `Ctrl+L` path edit and `Tab` completion, `New folder`, `Save`
     overwrite bar, `Escape` priority order) with no Wayland connection.
   - Theme loading from a temporary `look` file and missing-file fallback;
     `tools/verify-theme-colours` confirms zero colour literals outside
     `look.rs`.
2. **Portal D-Bus test (`tools/test-portal-file-chooser`, CI "C helpers" job):**
   - Runs `infynitae-portal` in a private `dbus-run-session` with a stand-in
     `HOME`, `XDG_RUNTIME_DIR`, and stub `infynitae-files` binary
     (matching `tools/test-portal-screencast` and `tools/test-portal-settings`).
   - Checks: direct callers other than `xdg-desktop-portal` get `AccessDenied`;
     `OpenFile`, `SaveFile`, and `SaveFiles` forward all options and return
     `uris`, `choices`, `current_filter`, and `writable`; cancel returns `1`;
     `Request.Close()` terminates the stub and returns `2`; stub crash
     triggers the fallback path (tested against a stub `desktop.gtk` bus
     name).

#### Real desktop tests in the test VM (`tools/vm-edge/pfilepicker.py`)

Runs inside `~/.cache/infynitae-vm/run/` with real `/dev/uinput` keyboard and
mouse input, following `tools/vm-edge/pscreencast.py` and `pask.py`:

- **Direct portal client (Python `Gio`):** calls `org.freedesktop.portal.FileChooser`
  (`OpenFile` single, multiple, directory, filters, choices; `SaveFile` new
  and overwrite; `SaveFiles`) and asserts the returned URIs, filter, and
  choices match the driven clicks/keys.
- **Zenity:** `GTK_USE_PORTAL=1 GDK_DEBUG=portals zenity --file-selection`
  (open, `--multiple`, `--directory`, `--save --confirm-overwrite`,
  `--file-filter='Images | *.png'`; setting both env vars covers GTK 3 and
  GTK 4 zenity builds), verifying zenity prints the exact path chosen in our
  window.
- **KDialog:** `QT_QPA_PLATFORMTHEME=xdgdesktopportal kdialog --getopenfilename`
  and `--getsavefilename` verifying Qt's portal bridge works end to end.
- **GTK 4 test app:** small Python `gi` script (`Gtk.FileDialog` with
  `GDK_DEBUG=portals`, like `tools/vm-edge/ptoolkits.py`), checking via
  `infynitae msg snapshot` that the picker opens centred over the GTK 4 parent
  window (`xdg_foreign`).
- **Firefox:** with `widget.use-xdg-desktop-portal.file-picker = 1`, triggers
  `Ctrl+O` (Open File) and `Ctrl+S` (Save Page As), picks/saves a real file,
  and checks the saved file exists on disk.
- **Flatpak document portal (when a Flatpak runtime is present in the VM):**
  verifies a sandboxed caller receives a `/run/user/<uid>/doc/...` URI and can
  read/write the chosen file while still denied access to neighbour files.
- **Fallback and isolation checks:** verifies `xdg-desktop-portal-gtk` is not
  spawned during normal runs, then replaces the picker binary with a crashing
  stub and confirms the GTK fallback dialog appears instead of hanging the app.
  Saves screenshots of Open and Save for visual inspection (colours, 8 px
  corners, contrast).
