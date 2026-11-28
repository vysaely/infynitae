# Infynitae file manager (design)

Today Infynitae relies on an external file manager: `FILE_MANAGER` in
`~/.config/infynitae/config` (PCManFM-Qt on the owner's machine, or Thunar
installed by `live/install-lib.sh:108` when no other manager is present). An
external manager brings its own sidebar, its own shortcuts, its own copy
dialogs, and a different visual style from the edge, settings, lock screen, and
screenshot tool.

Instead of maintaining a separate file picker and file manager, **one program
(`infynitae-files`)** in one Rust crate (`live/files/`) handles both:

- **Stage 1 (portal picker):** `infynitae-files --portal` answers Open and Save
  requests from apps through `infynitae-portal` ([FILE_PICKER.md](FILE_PICKER.md)).
- **Stage 2 (file manager):** `infynitae-files` run directly opens the full
  file manager described below.

An interactive mock-up using the shipped default theme colours is in
[file-manager.html](mockups/file-manager.html), built to match
[file-picker.html](mockups/file-picker.html). No code is written yet; the owner
reviews this design first.

---

## Part 1: What you see and how it works

### One window, built on the picker

The file manager keeps the layout from [FILE_PICKER.md](FILE_PICKER.md): the
same 8 px window corners, 1 px borders, fonts, line icons, and colours from
`~/.cache/infynitae/look`.

- **Left column (Places).** Recent files, Home and XDG folders (`Documents`,
  `Downloads`, `Pictures`, `Videos`, `Music`, `Desktop`), saved folder
  bookmarks (shared with the picker and the edge's Folders card), **Trash**,
  and **Drives** (`I`, `II`, `III`, `IV`, `V`, `VI` and up per
  [DRIVES.md](DRIVES.md), plus USB sticks under `/run/media/<you>/`).
- **Top bar.** Back, Forward, Up, clickable breadcrumbs (`Ctrl+L` to type a
  path with `Tab` completion), Search (`Ctrl+F`, switching between **Here** and
  **All** via `plocate`), List and Grid views (`Ctrl+1` / `Ctrl+2`), Split view
  (`F3`), Preview pane (`Alt+P`), hidden files (`Ctrl+H`), and New folder
  (`Ctrl+Shift+N`).
- **Main file area.** Folders first, sorted by Name (natural number order so
  `track 2.flac` comes before `track 10.flac`), Modified, Size, or Type. Click
  any column header to sort or reverse it.
- **Right column (Preview, collapsible).** Pictures, first lines of text files,
  folder counts, and details for the selected item, hidden automatically on
  narrow windows.

### What the manager adds to the picker

- **Tabs and split view.**
  - A tab strip sits along the top (`Ctrl+T` opens a tab, `Ctrl+W` closes it,
    `Ctrl+Shift+T` reopens the last closed tab, `Alt+1`–`9` jumps to a tab, and
    middle-clicking any folder or Place opens it in a new tab).
  - **Split view (`F3` or toolbar button)** splits the file area into two
    side-by-side folders in the same window. Each side keeps its own folder,
    history, view mode, and selection. `Tab` switches sides, and the active
    side's path bar carries the accent border so you always see where keys and
    pastes go.
- **Copy, move, rename, and undo.**
  - `Ctrl+C` copies, `Ctrl+X` cuts (cut rows turn dimmed until pasted or
    replaced), `Ctrl+V` pastes, and `F2` renames directly inside the row with
    the name selected before the extension.
  - Copied files go onto the desktop clipboard so you can paste them into
    another window, Firefox, Discord, or a terminal.
  - `Ctrl+Z` undoes your last rename, move, move-to-trash, or empty new folder.
    Right after an undoable action, a short bar at the bottom offers an `Undo`
    button for a few seconds.
- **Trash and restore.**
  - `Delete` moves selected items to Trash. On another drive (such as Drive
    `III`, `IV`, or a USB stick), items move into that drive's own trash folder
    by instant rename instead of copying gigabytes back to your home drive.
  - Clicking **Trash** in Places shows deleted items with their original folder
    and deletion date, plus a top bar with **Restore** and **Empty trash**.
    **Empty trash** asks for confirmation first, showing the item count and
    total size.
  - `Shift+Delete` deletes permanently and always shows an inline confirmation
    bar in the alert colour (`Cancel` first, `Delete` second).
- **Drag and drop with other apps.**
  - Drag files into folders, across tabs (hover 0.7 s over a tab or folder to
    open it), between split panes, onto Places (drop on **Trash** to trash;
    drop a folder under **Bookmarks** to pin it), or out to Firefox, Discord,
    media players, and editors.
  - Drop files from other apps into any open folder.
  - Dropping on the same drive moves; dropping on another drive copies. Hold
    `Ctrl` to copy, `Shift` to move, or `Alt` to pick from a menu (`Move here`,
    `Copy here`, `Link here`, `Cancel`).
- **Progress panel for large copies.**
  - Copying something large, like a 336 GB music library, runs in the
    background so the window stays responsive and you can browse other tabs.
  - A docked panel opens above the status bar showing the source and target,
    current file name, thin accent progress bar, bytes done out of total, item
    count, smoothed transfer speed, estimated time left, **Pause / Resume**,
    and **Cancel**. You can fold it down to a single summary line while it
    runs.
  - If you close the window while a transfer is active, an inline bar warns you
    before anything stops.
  - Two large transfers writing to the same physical disk queue one after the
    other so disk speed stays steady; transfers to different drives run side by
    side.
- **Name conflict prompt.**
  - When a transfer hits a file with the same name at the destination, that job
    pauses (other tabs and unrelated transfers keep working) and shows an
    inline comparison bar with both files' size, modified date, and plain
    difference (`Incoming is newer and 1.2 MB larger`).
  - Actions: **Replace**, **Keep both** (adds `(2)` or lets you type another
    name), **Skip**, and **Cancel**, plus an **Apply to all** switch for the
    remaining conflicts in that transfer.
  - Copying a folder onto an existing folder with the same name offers
    **Merge** (keeping unrelated files already inside and asking only where
    names collide), **Keep both**, or **Skip**.
- **Right-click menu, Open with, and Properties.**
  - Double-click or `Enter` opens a file with its default app.
  - Right-click offers **Open**, **Open with...** (runs Infynitae's existing
    `infynitae-open-with`, including setting a new default), **Cut**, **Copy**,
    **Paste**, **Rename**, **Move to trash**, **Open terminal here** (opens
    your `TERMINAL` in that folder), **Add to Places** (on folders), and
    **Properties** (`Alt+Enter`).
  - **Properties** opens a clean card showing editable name, file type, path,
    exact size (summing folders in the background), dates, symlink target,
    default app with a **Change** button, and simple permissions including an
    **Allow running as a program** switch.
- **Drives: mount, unmount, eject, and format.**
  - Each mounted drive shows free space and a 3 px usage bar. At 90% full or
    higher (such as Drive `II` at 97%), the bar switches from the accent colour
    to the alert colour so tight space is visible without pop-ups.
  - Clicking an unmounted internal drive (such as Drive `VI`) mounts it through
    UDisks2 without a password prompt (per [DRIVES.md](DRIVES.md)) and opens
    it.
  - Removable drives show an inline **Eject** button (and **Eject** on
    right-click) that calls `infynitae-automount eject` to unmount, lock
    encrypted partitions, and power the stick off. Mounted non-system internal
    partitions offer **Unmount** on right-click.
  - Right-clicking a removable drive offers **Format drive...** only when
    `infynitae-format-drive --can` confirms it is safe to format, then launches
    `infynitae-format-drive`. Internal and system drives never show it.
- **Thumbnails and shared bookmarks.**
  - Image thumbnails generate on background threads and save in
    `~/.cache/thumbnails/`, so Grid view loads quickly on return and shares the
    same cache with the file picker and other Linux apps.
  - Bookmarks read and write `~/.config/gtk-3.0/bookmarks`, so pinning or
    removing a folder updates the file picker and the edge's Folders card too.

### How your data stays safe

- **Same-drive moves never copy data.** Moving a folder inside the same drive
  uses a single atomic rename. It finishes at once and uses zero extra disk
  space, even on a drive at 97% full like Drive `II`.
- **Cross-drive moves never delete before verifying.** Moving files from one
  drive to another copies each file into a hidden temporary file in the target
  folder, checks the byte count and checksum after flushing to disk, swaps the
  temporary file into its final name, and only then removes the original. If a
  transfer fails, gets cancelled, or loses power midway, the original file is
  still intact and the partial temporary file is deleted.
- **Replacing a file never wipes the old copy early.** Overwriting an existing
  file writes to a temporary file first and renames it over the old file only
  after the full copy lands on disk. If the copy stops halfway, the old file
  stays untouched.
- **Full disk protection (Drive `II` at 97% full).** Before copying starts, the
  manager sums the transfer size and checks free space on the target drive (and
  checks the 4 GB single-file limit on FAT32 USB drives). If the drive does not
  have room, it says exactly how much is needed and how much is free before
  writing a byte. If another app fills the drive mid-copy, the partial
  temporary file is removed right away to free its space, the job pauses, and
  you can clear room and click **Retry**, **Skip**, or **Cancel**.
- **Unplugged drive mid-copy.** If a USB stick is pulled out during a transfer,
  the job stops at once instead of hanging the window, deletes any temporary
  file on the mounted side, leaves every unfinished original untouched, and
  lists how many files copied and how many did not. Tabs open on the removed
  drive step back to Home with a plain notice.
- **Permission errors.** Unreadable or read-only files are never skipped
  silently. The job pauses, names the exact file, and offers **Skip**, **Skip
  all unreadable**, **Retry**, or **Cancel**. Copying onto exFAT, FAT32, or
  NTFS drives (which do not store Linux permissions) skips permission-bit
  copies without interrupting you.

### Keyboard shortcuts

All picker keys from [FILE_PICKER.md](FILE_PICKER.md) still work (`Ctrl+L`,
`Ctrl+F`, `Ctrl+H`, `Ctrl+Shift+N`, `Ctrl+1`/`2`, `Alt+P`, `Alt+Left/Right/Up`,
`Backspace`, `Escape`). The manager adds:

| Key | Action |
| :--- | :--- |
| `Ctrl+T` / `Ctrl+W` / `Ctrl+Shift+T` | New tab / close tab / reopen closed tab |
| `Ctrl+Tab` / `Ctrl+Shift+Tab`, `Alt+1`–`9` | Next or previous tab / jump to tab |
| `F3`, `Tab` (in split view) | Toggle split view / switch active pane |
| `F2` | Rename selected item inline |
| `Ctrl+C` / `Ctrl+X` / `Ctrl+V` | Copy / cut / paste |
| `Delete` / `Shift+Delete` | Move to Trash / delete permanently (confirms) |
| `Ctrl+Z` | Undo last rename, move, trash, or empty new folder |
| `Alt+Enter` | Open Properties for selected item |
| `Ctrl+D` | Add current folder to Places bookmarks |
| `F5` (or `Ctrl+R`) | Refresh current folder |
| `Shift+F10` (or Menu key) | Open right-click menu at selected item |

### Trying it beside PCManFM-Qt

Building `infynitae-files` does not touch your current setup. PCManFM-Qt stays
your `FILE_MANAGER` and your default folder app (`inode/directory`) until you
test `infynitae-files` and decide to switch. Switching takes one selection in
Settings -> Apps -> Files, and switching back to PCManFM-Qt takes the same one
step.

### Questions for the owner before writing code

1. **Cross-drive move verification:** Should cross-drive moves read every file
   back from the destination drive to verify its checksum by default (catches
   bad USB sticks, takes extra time on 336 GB), or read-back verify on
   removable drives only and rely on byte count plus `fsync` on internal drives?
2. **Closing the window during a long copy:** Should closing the last window
   ask `Stop transfer and close` vs `Keep copying in a small progress window`,
   or refuse to close until you cancel or finish?
3. **Click behaviour:** Standard double-click to open files and folders (single
   click selects), matching PCManFM-Qt?
4. **Video and PDF thumbnails:** Built-in image thumbnails only (PNG, JPEG,
   WebP, GIF), or also run `ffmpegthumbnailer` and `pdftoppm` (already in
   `live/packages-optional.txt:10`) in a `NO_NEW_PRIVS` subprocess when
   installed?

---

## Part 2: Technical design

### 1. One Rust crate, two modes (`live/files/`)

Both the portal picker and the file manager compile into one binary
(`~/.local/bin/infynitae-files`) from a standalone crate at `live/files/` (own
`Cargo.toml`, `Cargo.lock`, `[workspace]`, built by `build_rust infynitae-files
"$LIVE/files"` in `live/build-helpers.sh` with `.previous` rollback).

`live/file-manager/` already exists in this repository and holds
`infynitae-format-drive.desktop` for PCManFM-Qt (`live/install-lib.sh:658`).
Naming the crate `live/files/` avoids colliding with that folder while PCManFM-Qt
remains supported, and matches `live/edge-apps/src/cards/files.rs` and the
`Files` label in `live/edge-apps/src/cards/settings_apps.rs:60`.

#### Entry modes and privilege boundary

| Invocation | Process model | Privileges (`docs/SECURITY.md:211-243`) |
| :--- | :--- | :--- |
| `infynitae-files --portal` | Spawned by `infynitae-portal` (`live/portal/file-chooser.c`), one isolated short-lived process per dialog, stdin/stdout JSON. Never connects to the manager socket. | Inherits `PR_SET_NO_NEW_PRIVS` and `CapInh: 0` from `infynitae-portal`. Never launches user apps (`gio launch`, `TERMINAL`). |
| `infynitae-files [PATH_OR_URI...]` | Normal user app. Connects to `$XDG_RUNTIME_DIR/infynitae/files.sock` (mode `0600`) if running, or starts the session instance. Supports `--new-window`, `--tab`, `--select <PATH>`, and `--standalone` (bypasses socket for tests). | Normal user process (same as `infynitae-edge-apps`, `SECURITY.md:241`), because it opens user apps via `gio open` / `infynitae-open-with`, runs `TERMINAL`, and invokes `infynitae-automount` / `infynitae-format-drive`. |

Running manager windows in one session process (each window a normal
`xdg_toplevel` on the canvas) lets background transfer jobs, undo history,
clipboard ownership, and the bounded thumbnail memory cache work across windows
without a separate daemon. If a stale socket file exists after a crash, startup
probes it with `connect()`, unlinks on `ECONNREFUSED`, and binds a fresh socket.

#### Module layout inside `live/files/src/`

```
live/files/src/
  main.rs            # CLI dispatch: --portal vs manager mode
  # --- Shared by picker (stage 1) and manager (stage 2) ---
  look.rs            # 21 LOOK_* tokens from ~/.cache/infynitae/look + defaults
  draw.rs            # Cairo/Pango text, round_rect, bar, SIZE_* constants
  ui.rs              # Shared widgets: buttons, menus, inline bars, switches, icons
  text_input.rs      # Editable text field: cursor, selection, clipboard, Tab complete
  places.rs          # Drives I-XXXIX (fstab + mountinfo + by-uuid + /run/media), XDG dirs, bookmarks
  recent.rs          # ~/.local/share/recently-used.xbel read + atomic write
  mime.rs            # /usr/share/mime/{globs2,subclasses,aliases} + plain type names
  fs_list.rs         # Async read_dir + stat worker (generation-cancelled), natural sort, .hidden
  thumbs.rs          # Freedesktop Thumbnail Spec 0.8.0 disk cache + bounded RAM LRU + image decoder
  preview.rs         # Collapsible right-side preview pane (image, 16 KiB text, folder count)
  locate.rs          # Cancellable plocate worker thread (mirrors live/edge-apps/src/locate.rs)
  wayland.rs         # SCTK 0.20 xdg_toplevel, shm buffers, input, clipboard, fractional scale, IME
  # --- Portal picker mode (stage 1, docs/FILE_PICKER.md) ---
  portal/
    protocol.rs      # Stdin/stdout JSON, base64 raw path bytes, percent-encoded file:// URIs
    filters.rs       # Glob (kind=0) + MIME (kind=1) matcher
    mode.rs          # OpenFile, SaveFile, SaveFiles, choices, zxdg_importer_v2 parenting
  # --- File manager mode (stage 2) ---
  manager/
    app.rs           # Multi-window, tabs, split view, single-instance IPC socket, inotify watcher
    ops.rs           # Safe copy/move/rename/delete engine (pure fs logic, no Wayland dependency)
    trash.rs         # Freedesktop Trash Spec 1.0 (trash, restore, empty, directorysizes)
    undo.rs          # Bounded 50-entry in-memory undo stack
    dnd.rs           # wl_data_device drag source + drop target (text/uri-list, drag icon surface)
    drives.rs        # /proc/self/mountinfo poll watcher, udisksctl, automount eject, format-drive
    launch.rs        # gio open, infynitae-open-with, TERMINAL resolution
    overlay.rs       # Context menu, Properties card, Progress panel, Conflict bar
```

#### Dependencies

Shared with `live/edge-apps/Cargo.toml` and `live/shot-rs/Cargo.toml`:
`smithay-client-toolkit` 0.20, `wayland-client` 0.31, `wayland-protocols` 0.32,
`cairo-rs` 0.20 (`png` feature), `pango` 0.20, `pangocairo` 0.20, `serde` 1.0,
`serde_json` 1.0, `libc` 0.2, `chrono` 0.4.

New crates for `live/files/Cargo.toml`:
- `image` (`default-features = false`, features `["png", "jpeg", "webp", "gif"]`)
  for safe in-process image thumbnails and previews (already proposed in
  [FILE_PICKER.md](FILE_PICKER.md)).
- `sha2` (already audited in root `Cargo.lock:1443`) for cross-drive move
  read-back verification.

Things built directly on `libc` or ~60 lines of Rust with no extra crate:
- Directory live refresh: Linux `inotify` (`libc::inotify_init1`,
  `inotify_add_watch`), one watch per currently open tab/pane directory (never
  recursive).
- Mount/unmount live refresh: `libc::poll` with `POLLPRI | POLLERR` on an open
  descriptor of `/proc/self/mountinfo`, which Linux signals on every mount
  table change.
- Thumbnail cache filename hash: ~60 lines of pure-Rust MD5 (RFC 1321), used
  only because Freedesktop Thumbnail Spec 0.8.0 mandates MD5 of the canonical
  `file://` URI for interoperability with `~/.cache/thumbnails/`.

### 2. Safe copy, move, rename, and delete engine (`manager/ops.rs`)

`ops.rs` has no Wayland or Cairo dependency. It accepts a `JobSpec`, runs on a
worker thread, reads `AtomicBool` pause/cancel flags, and communicates
progress, conflict questions, and recoverable errors over channels.

#### Pre-flight checks (before writing a byte)

1. **Self-containment check:** Before copying or moving a directory `src` into
   `dst_dir`, walk `dst_dir` up to `/` comparing `(st_dev, st_ino)` against
   `src`. Reject if any ancestor matches (catches symlinks and bind mounts that
   string prefix checks miss).
2. **Tree scan:** Walk `src` without following directory symlinks (`O_NOFOLLOW`
   / `symlink_metadata`). Record regular files, symlinks, directories, and
   count skipped special files (FIFOs, Unix sockets, block/char devices) so the
   job never blocks on `open()` of a FIFO.
3. **Free-space calculation:** Query `statvfs(dst_dir)`. Round each regular
   file's size up to `f_frsize` (accounting for cluster waste when copying
   thousands of small cover-art files to FAT/exFAT). Compare against
   `f_bavail * f_frsize` (space available to non-root users). When replacing an
   existing file on the same destination filesystem, require free space for net
   growth plus the largest single replaced file (since the old file stays intact
   until the temporary file finishes). If free space is short, abort before
   writing and report needed vs available bytes. If a copy would leave less than
   1 GiB or 2% free on the target drive (critical on btrfs roots guarded by
   `live/infynitae-disk-guard`), show an inline warning before proceeding.
4. **Filesystem limits check:** Query `statfs(dst_dir)`.
   - On `MSDOS_SUPER_MAGIC` (`0x4d44`, FAT32), reject any file larger than
     `4,294,967,295` bytes before starting, naming the file and suggesting
     exFAT or another drive.
   - On FAT32, exFAT (`0x2011bab0`), and NTFS (`0x5346544e` / `0x7366746e` /
     `fuseblk`), flag names containing illegal characters (`< > : " \ | ? *` or
     trailing dot/space) and detect case-only collisions (`Cover.jpg` vs
     `cover.jpg`), offering Rename, Auto-replace characters with `_`, or Skip.

#### Same-filesystem move

Check `stat(src_parent).st_dev == stat(dst_dir).st_dev`. Attempt
`renameat2(AT_FDCWD, src, AT_FDCWD, dst, RENAME_NOREPLACE)`.

- Succeeds in O(1) with zero extra disk space (critical on Drive `II` at 97%).
- Returns `EEXIST`: route to the conflict handler; if the user selects
  **Replace**, use plain `rename()` (or for directory-on-directory, merge
  children recursively).
- Returns `EXDEV` (bind mounts, separate btrfs subvolumes on one block device):
  fall through to the cross-filesystem pipeline.

#### Copy and cross-filesystem move per-file pipeline

Every regular file copy (and every file inside a cross-filesystem move) follows
this sequence:

1. Open source with `O_RDONLY | O_NOFOLLOW`. Record `(src_size, src_mtime)`.
2. Create a hidden temporary file in the destination directory:
   `.infynitae-part-<pid>-<seq>` with `O_CREAT | O_EXCL | O_WRONLY` (mode
   `0600` initially, or masked to source mode on completion). Because it sits
   in the destination directory, step 8 is atomic on the same filesystem.
3. Transfer data in 1 MiB chunks:
   - Same-FS copy: try `ioctl(dst_fd, FICLONE, src_fd)` first for instant
     copy-on-write reflinks on btrfs/XFS; else `copy_file_range` in 1 MiB
     chunks.
   - Cross-FS copy/move: `read()` / `write()` 1 MiB chunks while updating a
     streaming `Sha256` digest of source bytes.
   - Between every chunk: check cancel (immediate abort) and pause (wait on
     condvar; resumes at exact offset). Emit progress every 100 ms with an EWMA
     throughput estimate over the last 3 seconds.
4. Check `bytes_written == bytes_read`. For cross-drive moves, re-`fstat` the
   source descriptor: if size or `mtime` changed while reading, refuse to
   delete the source and warn the user that the source file changed mid-move.
5. Preserve timestamps (`futimens`) and permissions (`fchmod`). On non-POSIX
   mounts (`vfat`, `exfat`, `ntfs3`, `fuseblk`), ignore `EPERM`/`EINVAL`/
   `ENOTSUP` from `fchmod`/`futimens` (logged once per job, never prompting).
6. Flush data and metadata to the storage device via `File::sync_all()`
   (`fsync`). Writeback failures (`EIO`, delayed `ENOSPC`, USB disconnect)
   surface here while the source and any replaced destination file are still
   untouched.
7. **Cross-drive move verification:** Issue
   `posix_fadvise(dst_fd, 0, 0, POSIX_FADV_DONTNEED)` so the kernel drops cached
   pages, re-read the temporary file from start to end, and compare its
   `Sha256` digest against step 3. On mismatch, unlink the temporary file, keep
   the source untouched, and pause with a corruption alert. (Subject to Owner
   Question 1 for internal vs removable drives.)
8. **Atomic publish:** Close `dst_fd`. Publish via
   `renameat2(..., RENAME_NOREPLACE)` for new or Keep-both targets, or
   `rename()` when the user explicitly approved Replace. Then `fsync()` the
   destination directory descriptor.
9. **Source removal (cross-drive move only):** `unlink()` the source file only
   after step 8 returns `0`. For moved directory trees, recreate directories
   top-down and `rmdir()` source directories bottom-up only when empty; if any
   file inside was skipped or failed, its parent chain stays intact at the
   source.
10. **Cleanup on any error or cancel before step 8:** Close and `unlink()`
    `.infynitae-part-<pid>-<seq>` immediately so neither partial files nor
    wasted disk space remain.

Symlinks are copied by reading the link target (`readlink`) and calling
`symlink()` at a temporary name then `rename()`, never following the target.

#### Disk-queue scheduler

For jobs larger than 64 MiB, resolve the destination's physical parent block
device from `/sys/dev/block/<maj>:<min>` (following the `../` parent device for
partitions). Jobs writing to the same physical disk queue sequentially by
default (preventing HDD/USB thrashing on 336 GB transfers), with a manual
`Start now` button on queued jobs. Transfers targeting different disks run in
parallel.

#### Error handling matrix

| Condition | Syscall error | Behaviour |
| :--- | :--- | :--- |
| Full disk mid-copy | `ENOSPC`, `EDQUOT` | Unlink `.infynitae-part-*` immediately; pause job; keep source and old destination intact; offer **Retry**, **Skip**, **Cancel**. |
| Drive unplugged / bad sector | `EIO`, `ENODEV`, `ESTALE`, `EROFS` | Stop job immediately (no retry loop); unlink temp file if destination still reachable; keep unfinished sources intact; report exact counts done vs remaining; step tabs on that mount back to Home. |
| Permission denied | `EACCES`, `EPERM` | Pause job naming exact path and operation (read/write); offer **Skip**, **Skip all unreadable**, **Retry**, **Cancel**. |
| Conflict (same name) | `EEXIST` | Pause job; show inline comparison bar with **Replace**, **Keep both** (with editable name), **Skip**, **Cancel**, and **Apply to all**. Folders offer **Merge** instead of wiping existing contents. |

### 3. Freedesktop Trash Specification 1.0 (`manager/trash.rs`)

Implements the Freedesktop Trash Specification 1.0 so files trashed in
`infynitae-files`, PCManFM-Qt, Thunar, or GTK apps are interoperable:

1. **Trash directory resolution per filesystem (`st_dev`):**
   - If the file's `st_dev` matches `$XDG_DATA_HOME` (default
     `~/.local/share`): use `$XDG_DATA_HOME/Trash`.
   - Otherwise, find the mount point `$topdir` from `/proc/self/mountinfo`:
     - If `$topdir/.Trash` exists, is a real directory (`lstat`, not a symlink),
       and has the sticky bit set (`mode & 0o1000 != 0`): use
       `$topdir/.Trash/<uid>`.
     - Otherwise create/use `$topdir/.Trash-<uid>`.
   - Create `files/` and `info/` subdirectories with mode `0700`. If creation
     fails (`EACCES`, `EROFS`, `ENOSPC`), never silently copy across drives and
     never delete silently: show an inline alert offering **Cancel** or
     **Delete permanently**.
2. **Atomic trash move:**
   - Choose a unique entry name (`<name>`, then `<name>.2`, `<name>.3`, ...) by
     creating `info/<entry>.trashinfo` with `O_CREAT | O_EXCL | O_WRONLY`
     (`0600`).
   - Write `[Trash Info]\nPath=<percent-encoded path>\nDeletionDate=YYYY-MM-DDTHH:MM:SS\n`
     (path relative to `$topdir` for non-home mounts, per spec) and `fsync()`.
   - Move source via `rename(src, files/<entry>)`. If `rename()` fails, unlink
     `info/<entry>.trashinfo` and report the error.
   - Update `directorysizes` cache file when trashing a directory (format:
     `<size> <mtime> <percent-encoded name>\n`, rewritten atomically via temp
     file + rename).
3. **Restore:**
   - Parse `info/<entry>.trashinfo`, decode `Path=`, reject any `..` component
     or path escaping `$topdir` for non-home trash.
   - Create missing parent directories if needed (after confirming if missing).
   - If the target exists, run the same conflict prompt (Replace / Keep both /
     Skip).
   - `rename(files/<entry>, target)` first, and unlink `info/<entry>.trashinfo`
     only after the rename succeeds.
4. **Empty trash:**
   - Iterates every mounted trash directory known to Places (home trash plus
     mounted drives' `.Trash-<uid>`).
   - Deletes `files/<entry>` completely first, then unlinks
     `info/<entry>.trashinfo` so an interrupted Empty never leaves invisible
     orphans under `files/`.

### 4. Undo stack (`manager/undo.rs`)

In-memory ring of the last 50 operations per manager session (cleared on exit):

- `Rename { from, to, dev, ino }`: checks `to` still exists with matching
  `(dev, ino)` and `from` does not exist, then `renameat2(..., RENAME_NOREPLACE)`.
- `MoveBatch { items: Vec<(orig, dest)> }`: moves each item back (using `ops.rs`
  for cross-drive moves, recreating removed empty parent folders if needed, and
  triggering conflict checks instead of overwriting).
- `TrashBatch { items: Vec<(trash_dir, entry, orig_path)> }`: calls Trash
  restore for every item from that delete action in one `Ctrl+Z`.
- `CreateDir { path }`: runs `rmdir(path)` only; refuses with a notice if files
  were placed inside it.

Permanent delete (`Shift+Delete`) is never pushed to the undo stack. Copying is
excluded from `Ctrl+Z` so undoing never risks deleting a copied file the user
edited afterwards.

### 5. Drag and drop and clipboard (`manager/dnd.rs`, `wayland.rs`)

Infynitae's compositor already implements `wl_data_device_manager` drag and
drop (`crates/infynitae-wayland/src/handlers.rs:1139-1168`, `DataDeviceHandler`
and `ClientDndGrabHandler`, plus `drag_icon` compositing in
`crates/infynitae-wayland/src/convert.rs:1584` and `cursor.rs:769`).

- **Clipboard (`Ctrl+C` / `Ctrl+X` / `Ctrl+V`):**
  - Offers `x-special/gnome-copied-files` (`copy\nfile://...` or
    `cut\nfile://...`), `text/uri-list` (RFC 2483 `\r\n`-separated `file://`
    URIs), and `text/plain;charset=utf-8` (newline-separated host paths for
    terminals/editors).
  - On `Ctrl+V`, reads `x-special/gnome-copied-files` first (to know copy vs
    cut), falling back to `text/uri-list` (treated as copy).
- **Outgoing drag (`wl_data_device.start_drag`):**
  - Starts after pointer moves 6 px with left button held on a selected item.
  - Creates a `wl_data_source` offering `text/uri-list` and
    `text/plain;charset=utf-8` with v3 actions `Copy | Move | Ask`.
  - Attaches a small themed `wl_surface` as the `drag_icon` showing the item
    name or count (`3 items`) drawn with `LOOK_CARD`, `LOOK_ACCENT`, and
    `LOOK_TEXT`.
- **Incoming drop (`wl_data_offer`):**
  - Accepts `text/uri-list` only for `file://` URIs (ignoring `#` comment lines
    per RFC 2483, stripping `file://localhost` host prefix, decoding raw
    percent-encoded bytes into `OsString`). Non-file URIs (`https://`) are
    declined.
  - Default action: `Move` when source and destination share `st_dev`, `Copy`
    across filesystems; `Ctrl` forces `Copy`, `Shift` forces `Move`, `Alt`
    opens an inline `Move here / Copy here / Link here / Cancel` bar at drop.
  - Hovering a folder row or tab for 700 ms opens it automatically during drag.

### 6. Freedesktop Thumbnail Managing Standard 0.8.0 (`thumbs.rs`)

Shared between the manager and the picker's Grid view:

- **Cache directories:** `$XDG_CACHE_HOME/thumbnails/normal/` (128x128 PNG),
  `$XDG_CACHE_HOME/thumbnails/large/` (256x256 PNG, used when
  `OUTPUT_SCALE > 1` or previewing), and
  `$XDG_CACHE_HOME/thumbnails/fail/infynitae-files-<version>/`, all created
  with mode `0700`.
- **Key and metadata:** Filename is `<md5(canonical_file_uri)>.png` (lowercase
  hex). Written PNGs include `tEXt` chunks `Thumb::URI`, `Thumb::MTime` (unix
  seconds), `Thumb::Size`, `Thumb::Image::Width`, and `Thumb::Image::Height`.
- **Lookup and atomic write:** Verify `Thumb::MTime` (and `Thumb::Size` if
  present) matches `stat()`. Write new thumbnails to a temp file in the same
  cache directory with mode `0600` and `rename()` into place. Never write disk
  thumbnails for files inside `$XDG_CACHE_HOME/thumbnails/` or inside Trash.
- **Safety limits:** Bounded in-memory LRU cache (64 MiB of Cairo surfaces),
  worker pool capped at 2 threads, generation-cancelled on folder switch,
  decode limit 64 megapixels / 256 MiB decompressed (`FILE_PICKER.md:535`).

### 7. Desktop integration and migration from PCManFM-Qt

#### Reusing existing Infynitae helpers

| Action in `infynitae-files` | Existing helper | How it runs |
| :--- | :--- | :--- |
| Open file (`Enter` / double-click) | `live/xdg-open` (20 lines) | `gio open <path>`, falling back to `infynitae-open-with` if no app handles the type. |
| Right-click **Open with...** / Properties **Change** | `live/infynitae-open-with` (63 lines) | Spawns `infynitae-open-with <path>` directly (uses `MENU_PICKER`, marks default, writes `~/.config/mimeapps.list` on "Set the default"). |
| Removable drive **Eject** | `live/infynitae-automount` (758 lines) | Steps open tabs off the mount first (and blocks if an active transfer targets it), then runs `infynitae-automount eject <device>` (`live/infynitae-automount:144-148`: unmounts, locks LUKS, powers off, notifies). |
| Fixed partition **Mount** / **Unmount** | `udisksctl` + `/etc/polkit-1/rules.d/50-infynitae-drive-names.rules` | `udisksctl mount -b /dev/disk/by-uuid/<uuid>` / `udisksctl unmount --no-user-interaction -b <device>` (never `power-off` on fixed disks). |
| Removable drive **Format drive...** | `live/infynitae-format-drive` (204 lines) | Shown only when `infynitae-format-drive --can <mount_point>` prints `true`; clicking spawns `infynitae-format-drive <mount_point>`. |
| **Open terminal here** | `live/config.sh` + `TERMINAL` | Resolves `TERMINAL` the same way `live/launch.sh` does, with `current_dir` set to the folder. |

#### How Edge cards, launcher search, and `inode/directory` open it

Every existing folder entry point in Infynitae already routes through
`xdg-open` -> `gio open` -> `inode/directory` in `~/.config/mimeapps.list`:

- **Edge Files -> Drives card** (`live/edge-apps/src/cards/files.rs:745`) emits
  `Action::OpenUri(drive.mount_point)`.
- **Edge Files -> Folders card** (`live/edge-apps/src/cards/files.rs:1278`)
  emits `Action::OpenUri(f.path)`.
- **Edge Launcher search (`Super+W`)**
  (`live/edge-apps/src/cards/search.rs:479`) emits `Action::OpenUri(path)` for
  Clips, Recordings, Recent, and `plocate` Files.
- **`live/edge-apps/src/wayland.rs:665-668`** dispatches `Action::OpenUri` by
  running `xdg-open <uri>` (`live/xdg-open:9` -> `gio open`).
- **USB mount notifications** (`live/infynitae-automount:347`) run
  `xdg-open "$mount_path"` when you click **Open**.
- **`Super+E`** (`live/launch.sh:175-209`) launches `FILE_MANAGER` from
  `~/.config/infynitae/config`.

Wiring `infynitae-files` into all of them takes three small additions:

1. **Desktop entry** at `live/applications/infynitae-files.desktop`, installed
   to `~/.local/share/applications/infynitae-files.desktop`:
   ```ini
   [Desktop Entry]
   Type=Application
   Name=Files
   Comment=Browse folders, drives, and trash
   Exec=infynitae-files %U
   Icon=system-file-manager
   Terminal=false
   Categories=System;FileTools;FileManager;
   MimeType=inode/directory;
   StartupWMClass=infynitae-files
   ```
   Because `Categories` contains `FileManager`, Settings -> Apps -> Files
   (`live/edge-apps/src/cards/settings_apps.rs:58-65`) lists it automatically
   and the Edge `Files` provider (`live/edge-apps/src/cards/files.rs:20`,
   `Claim::Categories(&["FileManager"])`) attaches Drives and Folders cards to
   it.
2. **Syncing `inode/directory` when picking Files in Settings -> Apps:** Mirror
   `browser_default_command` (`live/edge-apps/src/cards/settings_apps.rs:313-319`)
   for `FILE_MANAGER` so choosing a file manager in Settings writes both
   `FILE_MANAGER="..."` and `xdg-mime default <id>.desktop inode/directory`
   (or `gio mime inode/directory <id>.desktop`). Selecting `Files` or
   `PCManFM-Qt` in Settings then switches `Super+E`, Edge cards, Launcher
   search, mount notifications, and third-party apps together.
3. **Optional search & Drives card polish (stage 2c):**
   - In `live/edge-apps/src/cards/search.rs`, add `Shift+Enter` / right-click
     "Show in folder" on file results calling `infynitae-files --select <path>`
     when `infynitae-files` is active.
   - In `live/edge-apps/src/cards/files.rs:348-356`, recognise `# removable:`
     fstab entries (such as Drive `V` at `/mnt/V`) as removable so the Edge
     Drives card shows the Eject icon for Roman-numeral USB sticks too.
   - Optional D-Bus `org.freedesktop.FileManager1` activation service
     (`ShowFolders`, `ShowItems`, `ShowItemProperties`), installed only when
     `infynitae-files` is chosen as default so Firefox and Chromium "Show in
     folder" highlight the downloaded file inside its folder.

#### What stays PCManFM-Qt until `infynitae-files` is ready

- `FILE_MANAGER="pcmanfm-qt"` in the owner's `~/.config/infynitae/config`
  remains untouched (`config/config` rule: updates never rewrite user values).
- `inode/directory` in `~/.config/mimeapps.list` remains `pcmanfm-qt.desktop`
  (`live/install-lib.sh:728` never overwrites an existing default).
- `~/.local/share/file-manager/actions/infynitae-format-drive.desktop`
  (`live/install-lib.sh:656-659`) remains installed so PCManFM-Qt keeps its
  **Format drive...** action.
- `live/launch.sh:180-193` (`auto` fallback order),
  `live/edge-apps/src/cards/settings_apps.rs:63` (`auto_order`),
  `live/install-lib.sh:95` (`other_file_manager()`), and
  `live/packages-optional.txt:10` (`files thunar ...`) stay unchanged until
  stage 2 passes all VM checks and the owner decides to promote
  `infynitae-files` for fresh installs.
- `live/settings.sh:666` (`FOLDER_COLOR` Papirus folder tinting) stays intact
  for GTK/Qt apps and PCManFM-Qt.

### 8. Stages and size estimate

Reference sizes measured in this repository:
`live/edge-apps/src/cards/files.rs` is 2,018 lines, `live/shot-rs/src/` is
6,491 lines, `live/infynitae-automount` is 758 lines,
`live/infynitae-format-drive` is 204 lines, and `live/infynitae-open-with` is
63 lines.

| Stage | Scope | Paths | Estimated lines |
| :--- | :--- | :--- | :--- |
| **Stage 1 (picker core)** | Portal `FileChooser` (`OpenFile`, `SaveFile`, `SaveFiles`), shared Wayland/Cairo/theme/Places/list/grid/preview/`plocate` core (sized in [FILE_PICKER.md](FILE_PICKER.md)) | `live/portal/file-chooser.{c,h}`, `live/files/src/` | 5,450–7,550 (incl. picker tests) |
| **Stage 2a (safe daily manager)** | Manager entry, single-instance IPC socket, tabs, `inotify` refresh, inline rename (`F2`), clipboard copy/cut/paste, safe `ops.rs` engine (pre-flight space + FAT32 checks, temp-file atomic publish, `fsync`, cross-FS verify, pause/cancel, disk queue), progress panel, conflict bar, Freedesktop Trash 1.0 + Trash view + Restore/Empty, undo stack, right-click menu (`infynitae-open-with`, terminal), bookmark write-back, `.desktop` file | `live/files/src/manager/{app,ops,trash,undo,launch,overlay}.rs`, `live/applications/infynitae-files.desktop` | 2,200–2,900 code + 900–1,200 tests |
| **Stage 2b (split, DnD, drives, thumbnails)** | Split view (`F3`), `wl_data_device` drag and drop (`text/uri-list`, drag icon, 700 ms spring-load), Properties overlay (async folder size, permissions, default app), live `mountinfo` poll + mount/unmount/`infynitae-automount eject`/`infynitae-format-drive`, Freedesktop Thumbnail Spec 0.8.0 disk cache (shared with picker) | `live/files/src/manager/{dnd,drives}.rs`, `live/files/src/thumbs.rs` | 1,800–2,400 code + 600–800 tests |
| **Stage 2c (polish and switchover)** | Settings -> Apps `FILE_MANAGER` updating `inode/directory`, `search.rs` `--select`, optional `org.freedesktop.FileManager1`, optional `NO_NEW_PRIVS` `ffmpegthumbnailer`/`pdftoppm` support, promoting default in `launch.sh` / `install-lib.sh` once approved | `live/files/src/`, `live/edge-apps/src/cards/{settings_apps,search,files}.rs`, `live/launch.sh`, `live/install-lib.sh` | 500–800 code + 300–500 tests |
| **Total manager additions (Stages 2a–2c)** | | | **6,300–8,600** (4,500–6,100 code + 1,800–2,500 tests) |
| **Combined total (Stages 1 + 2, code + tests)** | | | **11,750–16,150** |

### 9. Risks and explicit non-goals

1. **Data loss during interrupted cross-drive move or overwrite.** Mitigation:
   copy to hidden temp file in destination -> verify -> `fsync` -> atomic
   `rename` -> `fsync(dir)` -> unlink source; fault-injection unit tests fail
   every step deliberately and assert the original survives intact.
2. **Silent corruption on removable USB flash drives.** Mitigation:
   `POSIX_FADV_DONTNEED` + `Sha256` read-back check before deleting the source
   on cross-drive moves.
3. **Full disk and btrfs metadata exhaustion (Drive `II` at 97% full).**
   Mitigation: cluster-aligned `statvfs` pre-flight check, accounting for peak
   temp-file space on Replace, immediate unlink of `.infynitae-part-*` on
   `ENOSPC`, low-headroom warning below 1 GiB / 2%, and alert-coloured drive
   usage bars at >= 90%.
4. **FAT32 / exFAT / NTFS pitfalls on Linux libraries (such as 336 GB of music).**
   4 GiB FAT32 file limit, illegal characters (`< > : " \ | ? *`),
   case-insensitive filename collisions (`Cover.jpg` vs `cover.jpg`), and
   `EPERM` on `chmod`. Mitigation: pre-flight checks for size and characters,
   case-collision prompt, and treating non-POSIX `fchmod` failures as expected
   on FAT/exFAT/NTFS mounts.
5. **Manager holding a removable drive busy on Eject.** Mitigation: never call
   `chdir()` into viewed folders; before calling `infynitae-automount eject`,
   check for active jobs targeting that mount and navigate open tabs off it.
6. **Symlink loops, hardlinks, and special files.** Mitigation: `O_NOFOLLOW` on
   regular file reads, `(st_dev, st_ino)` ancestor walk for self-copy checks,
   hardlink dedup (`HashSet<(u64, u64)>`) during Properties folder size totals,
   and skipping FIFOs/sockets/devices with a clear notice.
7. **Untrusted media and filenames in `Downloads` / Trash.** Mitigation: raw
   `OsString`/`Vec<u8>` paths throughout, 64 MP / 256 MiB decode caps in
   `thumbs.rs`, `0700`/`0600` permissions on thumbnail cache, and `cargo-fuzz`
   targets in `fuzz/` for `.trashinfo`, `text/uri-list`, and bookmark parsing.
8. **Cross-toolkit and Xwayland Drag & Drop quirks.** Mitigation: handle both
   `\r\n` and `\n`, `#` comment lines, and `file://localhost/` in
   `text/uri-list`; verify Wayland-to-Wayland and Wayland-to-Xwayland drops in
   the VM test suite.
9. **Explicitly out of scope for Stages 1–2 (keeps the crate small):** virtual
   network filesystems (`smb://`, `sftp://`, `mtp://` via GVFS), built-in
   archive extraction/creation UI, batch regex renamer, and git status
   overlays. PCManFM-Qt or terminal tools remain available when those are
   needed.

### 10. Testing plan

#### Automated unit tests (`cargo test --locked` in `live/files/`, no screen)

1. **Copy and move engine (`manager/ops.rs`, running on temp directories):**
   - Nested directory trees, empty folders, symlinks (valid and dangling,
     recreated as symlinks without dereferencing), non-UTF-8 filenames,
     self-copy rejection (direct and through a symlink), natural name sorting.
   - Forced cross-filesystem move path: verifies temp file created in target
     directory, `fsync` + read-back checksum run, atomic publish completes,
     source file unlinked only after publish, source folders removed bottom-up
     only when empty.
   - Fault injection via mockable filesystem trait at the I/O boundary:
     - `ENOSPC` mid-file: asserts `.infynitae-part-*` is unlinked, existing
       target on Replace is untouched, source is untouched, job pauses with
       Retry/Skip/Cancel.
     - `EIO` / `ENODEV` mid-file: asserts job halts without retry spin, reports
       exact finished vs unfinished counts, leaves unfinished sources intact.
     - Read-back checksum mismatch on cross-drive move: asserts temp file is
       unlinked, source is NOT deleted, and corruption error is raised.
     - `EACCES` on source or destination: asserts pause with Skip / Skip all /
       Retry / Cancel; `EPERM` from `fchmod` on simulated `exfat` mount does
       not interrupt the transfer.
     - Source modified mid-move: asserts source is NOT deleted.
   - Conflict handling: Replace, Keep both (`name (2).ext`, `name (3).ext`),
     Skip, Folder Merge (preserves unrelated destination files), Apply to all.
   - Pause/resume and Cancel: pause between chunks, resume produces
     byte-identical output; cancel mid-file deletes `.infynitae-part-*` and
     leaves source intact.
   - Pre-flight guards: free-space shortfall rejection, FAT32 > 4 GiB rejection,
     FAT/exFAT illegal character detection, case-only collision detection.
2. **Freedesktop Trash 1.0 (`manager/trash.rs`):**
   - `.trashinfo` syntax, percent-encoding, collision numbering
     (`foo.2.trashinfo`), `directorysizes` updates, Restore (including missing
     parent and destination collision), Empty Trash deletion order (`files/`
     before `info/`), `.Trash` sticky-bit and non-symlink checks vs
     `.Trash-<uid>`.
3. **Undo stack (`manager/undo.rs`):**
   - Undo rename (and safe refusal when old name was recreated), undo move
     batch, undo trash batch, undo empty new folder (and safe refusal once a
     file is inside).
4. **Thumbnails, DnD parsing, and Places (`thumbs.rs`, `dnd.rs`, `places.rs`):**
   - Reference MD5 URI keys from Freedesktop Thumbnail Spec 0.8.0, `tEXt`
     chunks, stale `Thumb::MTime` invalidation, `0700`/`0600` permissions,
     recursion guard inside cache dir.
   - `text/uri-list` and `x-special/gnome-copied-files` parsing (CRLF, `#`
     comments, `file://localhost/`, non-UTF-8 percent-encoded bytes).
   - `~/.config/gtk-3.0/bookmarks` atomic read/write and `# removable:` drive
     classification.
   - `tools/verify-theme-colours` confirms zero colour literals outside
     `look.rs`.

#### Real desktop VM tests (`tools/vm-edge/pfilemanager.py`)

Runs inside `~/.cache/infynitae-vm/run/` with real `/dev/uinput` keyboard and
mouse input, following `tools/vm-edge/pcards.py` and `psearch.py`:

- **Entry points:** `Super+E` with `FILE_MANAGER="infynitae-files"`, clicking a
  drive and folder in the Edge Files cards, and opening a folder from `Super+W`
  search with `inode/directory=infynitae-files.desktop`.
- **Tabs and split view:** `Ctrl+T`, middle-click folder into new tab, `Ctrl+W`,
  `F3` split view, `Tab` switching active pane.
- **File operations and undo:** Inline `F2` rename + `Ctrl+Z` undo; `Ctrl+C` /
  `Ctrl+V` conflict prompt exercising Replace, Keep both, Skip, and Apply to
  all; `Delete` to Trash, view in Trash, **Restore**, and **Empty trash**.
- **Real cross-filesystem and full-disk tests (loopback ext4 images in VM):**
  - Mount a 32 MiB ext4 loopback image at `/mnt/V` and a tiny 6 MiB ext4
    loopback image at `/mnt/VI`.
  - Run a real cross-filesystem move from `~/` to `/mnt/V` (triggers real
    `EXDEV`), pausing and resuming via the progress panel, and verifying source
    removal and destination checksum.
  - Attempt a 10 MiB copy onto the 6 MiB `/mnt/VI` mount (verifies pre-flight
    full-disk block), then fill `/mnt/VI` mid-copy to verify real `ENOSPC`
    pause, partial-file cleanup, and retry after freeing space.
  - Trash a file on `/mnt/V` and verify `/mnt/V/.Trash-<uid>` is used (instant
    same-FS rename, not copied to `~/.local/share/Trash`), then restore it.
- **Drag and drop:** Drag a file from one `infynitae-files` window to another
  and onto a GTK 4 drop-target script (`wl_data_device`, `text/uri-list`).
- **Helpers:** Right-click -> **Open with...** launches `infynitae-open-with`;
  **Format drive...** does not appear on `/` or non-removable mounts.
- **Visual check:** Save screenshots of tabs, split view, progress panel,
  conflict prompt, Trash, and right-click menu to verify theme colours, 8 px
  corners, and contrast.
