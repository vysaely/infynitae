# Security: who may do what

How Infynitae decides what an app may do beyond drawing its own windows. The decisions behind
it are in [DECISIONS.md](DECISIONS.md) ("Security and trust", 2026-10-31).

## Two kinds of app

- **Sandboxed apps** (Flatpak, and any sandbox that uses the `security-context-v1` protocol)
  connect through a listener their sandbox asked the compositor for. That connection carries
  the sandbox's name and the app's id, which the app inside cannot change. The sandbox is the
  boundary that matters: a sandboxed app is denied every power below unless you list its app id.
- **Desktop (native) apps** connect to the ordinary Wayland socket. Infynitae knows one by its
  real program file: the file its process runs (device and inode), never a name the program
  reports or a file name it could copy. A native app already runs with your rights (it can read
  your files and run commands), so for native apps the lists are a guard against mistakes and
  surprises more than against attacks.
- **Infynitae's own helpers** (the edge, the lock screen, the wallpaper, the portal, the
  recorder, the polkit agent, as installed in `~/.local/bin`) always have what they need.
- The files are looked up when the session starts and when the config changes. An update writes
  a helper (or a listed program) anew, as another file; when such a program is refused, its
  name or path on the list is looked up again, so an updated lock screen can still lock the
  session without restarting it. A program the lists do not name is never looked up again.

## The powers and their lists

Each power is a group of Wayland protocols. An app that is not allowed one never sees those
protocols at all: for that app they do not exist, and asking for one ends its connection.

| Setting | What it allows | Default |
|---|---|---|
| `ALLOW_CAPTURE` | Capturing the screen or a single window directly | every native app |
| `ALLOW_CLIPBOARD` | Reading and setting the clipboard at any time, without focus | every native app |
| `ALLOW_INPUT` | Typing and moving the mouse as if you did (virtual keyboard, virtual pointer) | every native app |
| `ALLOW_INPUT_METHOD` | Being the input method, which sees every key | every native app |
| `ALLOW_WINDOWS` | Listing every window, and focusing or closing any | every native app |
| `ALLOW_DISPLAY` | Screen colours (night light), modes and power | every native app |
| `ALLOW_LAYERS` | Panels, overlays and notifications above or below every window | every native app |
| `ALLOW_LOCK` | Locking the session | only Infynitae's lock and the `SCREEN_LOCKER` command |

A list holds words separated by spaces:

- `native`: every desktop app.
- An app id, such as `com.obsproject.Studio`: that sandboxed app.
- A program name, such as `grim`: the program of that name in `PATH`, `~/.local/bin`, `/usr/lib`
  or `/usr/libexec`, by its file. A program started through an interpreter (a Python script) is
  the interpreter's file, so list the interpreter's path or keep `native`.
- An absolute path, such as `/opt/obs/bin/obs`: that file.

The defaults name the usual tools as well (`grim`, `wl-copy`, `wtype`, `kanshi`, `mako`…), so
taking `native` out of a list keeps them working:

```sh
ALLOW_CAPTURE="grim wf-recorder obs com.obsproject.Studio"   # only these may capture
ALLOW_INPUT=""                                               # no app may type for you
```

A change applies to apps that connect after you save the config; an app that is already
running keeps what it had until it restarts. `live/config-check` names a word it cannot use.

Without any list, a sandboxed app still has everything an ordinary app has: its own windows,
the clipboard while it has the keyboard focus, and drag and drop.

## Screen sharing and remote desktop: you are asked

An app that shares the screen through the portal (a video call's Share button) or wants the
keyboard and mouse (a remote desktop) goes through Infynitae's portal, which asks you first:
this time, always, or deny.

- "Always" is remembered per app id, the id the sandbox gives the app, in
  `~/.local/state/infynitae/screenshare-allowed` and `remote-desktop-allowed` (one id per line).
  To forget a choice, delete that app's line (or the file); the app is asked again next time.
- An app outside a sandbox reaches the portal without an id, so it cannot be told apart from
  any other such app: it is asked every time, never remembered.
- Only xdg-desktop-portal may call Infynitae's portal, so the app id it passes is the one the
  portal checked; another program calling it directly is refused.
- `SCREENSHARE_ASK="false"` shares without asking (the old behaviour); `"true"` asks and also
  always lets you pick the screen or window.
- While anything captures the screen, the edge's background apps module says "Screen capture" and
  which app (`CAPTURE_INDICATOR="edge"`, the default since 2026-11-12, the owner's choice), or a red dot
  shows in the corner of every screen (`"dot"`). The edge's line is seen only when a panel with that
  module is open. Only the replay buffer Infynitae itself started is left out; it is known by a private
  socket the compositor gives it, never by a program name.

## While the screen is locked

Nothing captures (a capture shows the lock screen or is refused), no virtual keyboard or input
method reaches anything, and only the lock screen gets keys. Only the program that locked can
unlock, and only an allowed locker may take the lock. If the lock screen's program dies while
locked, the session stays locked and the compositor starts the lock screen again itself.
Before the machine sleeps, the sleep lock asks the compositor whether the session is locked
(`infynitae msg snapshot`, its `locked` field), never whether a process of some name runs; a
lock screen that does not confirm in time holds the sleep back as long as logind allows.

## The desktop journal: what happened, where and how

`~/.local/state/infynitae/journal/journal.tsv` (`$XDG_STATE_HOME` when set) records what the
compositor and Infynitae's helpers see, one line per event, and is never sent anywhere.
`infynitae msg journal` prints it; `--since 2026-11-25` (any start of a time) and `--app grim`
(any part of the program) narrow it down.

A line has six fields separated by tabs: the time in UTC, the event, the program as its real
file (the path `/proc/<pid>/exe` points to; for a sandboxed app its app id), its pid, what it was
about, and the outcome. An empty field is not known.

| Event | Written by | When |
| :--- | :--- | :--- |
| `capture-start`, `capture-stop` | compositor | a program starts capturing a screen or window, and 3 s after its last frame |
| `clipboard-reader` | compositor | a program makes a data-control device: it can read every copy without focus |
| `virtual-keyboard`, `virtual-pointer` | compositor | a program makes one |
| `input-method` | compositor | a program becomes the input method |
| `global-refused` | compositor | a program is refused a power its list does not give it; once per program and power per session |
| `session-lock`, `session-unlock` | compositor | the session is locked and by whom, unlocked, or an unlock is refused |
| `unlock-failed` | lock screen | a wrong password |
| `polkit` | password prompt | a program asks for a system action: allowed, denied (each wrong password), cancelled |
| `drive-added`, `drive-mounted` | automount | a removable drive is plugged in, and each mount or failed mount |
| `autostart-added`, `autostart-removed` | session start | an autostart entry, or `autostart.sh`, is new or gone since the last session |

Its size is capped: at 1 MiB (1,048,576 bytes) the file becomes `journal.1.tsv` and the one
before is deleted, so the two never hold more than 2 MiB (a line is about 150 bytes: some
14,000 events). Only the compositor rotates; the helpers append.

## Windows and Android programs

A Windows program (`.exe`, `.msi`) opened with `infynitae-windows-run` runs with Proton (through
umu-run) inside a bubblewrap sandbox; an Android app (`.apk`) opened with `infynitae-android-run`
runs in Waydroid. Both connect to Infynitae through `infynitae-sandbox-socket`, a
security-context-v1 listener, so they are sandboxed apps (above): no capture, clipboard without
focus, typing for you, window list, display, layers or lock unless you list their app id,
`infynitae.windows.NAME` (NAME is the program's C: drive) or `infynitae.android`.

What a Windows program sees, and nothing else of the machine:

- `/usr`, `/etc` and `/sys`, read-only (the system's programs and settings, as any user sees them)
- its own C: drive, `~/.local/share/infynitae/windows/NAME`, read and write; NAME is the file's
  name, so an installer and the program it installs share one, and the program's shortcuts in
  the app list start it with the same one
- `~/Windows`, read and write: the one folder shared with your other files
- the file it was opened with, read-only
- Proton, read-only: the newest in Steam's `compatibilitytools.d`, else the UMU-Proton umu
  fetched; and umu's runtime, where the program's writes vanish when it ends
- the graphics card (`/dev/dri`), sound (the PipeWire and PulseAudio sockets), the network, and
  its own empty `/tmp`, home folder and `/run`

It does not see the rest of your home folder (`~/.ssh`, browser profiles, documents), the
session bus (D-Bus), the X11 display, other processes, or other programs' C: drives. Its
environment starts empty. `--offline` takes the network away too; `tools/test-windows-run`
checks these with bubblewrap itself.

What this does not protect against:

- A bug in the Linux kernel or the graphics driver: the program talks to both directly.
- Sound: the PipeWire socket lets a program record the microphone and every app's sound.
- The network: the program can reach anything your machine can, the local network too, unless
  it runs with `--offline`.
- `--x11`, for a program that fails on Wayland: X11 programs can read each other's windows and
  keystrokes, so such a program can watch every other X11 app (games, older apps through
  Xwayland).
- What you put in `~/Windows`, and what the program keeps in its own C: drive: a program you
  ran once can change its own C: drive for the next start.
- The first start of a new C: drive runs umu and Proton with their runtime writable, so they can
  set it up; the program itself never runs that way.
- Waydroid: its container manager (`waydroid-container`) runs as root, and Android runs in an
  LXC container it starts; a bug there is a bug as root. Android apps are sandboxed to
  Infynitae only when `infynitae-android-run` started the Waydroid session. Waydroid's own app
  entries (`waydroid app launch`) start a session of their own while none runs, and that one
  connects as a desktop app.

## Data kept on disk

- The screen-sharing log (`~/.local/state/infynitae/screenshare.log`) names screens and counts
  windows; it never records a window's title.
- Clipboard history (`CLIPBOARD_HISTORY`, cliphist) is kept on disk. A copied password stays out
  of it only when the password manager marks it as sensitive and both wl-clipboard (2.2 or
  newer, `CLIPBOARD_STATE`) and cliphist honour that mark; if yours do not, turn the history off
  if you copy passwords.

## Drives named in /etc/fstab

Drives named by Roman numeral (docs/DRIVES.md) are lines in `/etc/fstab`, written by
`/usr/local/lib/infynitae-drive-name` as root. The rule `/etc/polkit-1/rules.d/50-infynitae-drive-names.rules`
allows two things without a password, only for a session that is local and active (the person at
the computer, not SSH or another seat):

- running that one program through pkexec. It takes a UUID, a numeral, a filesystem type and a
  kind, checks each against a fixed list or character set, writes only between its two marker
  lines and refuses a UUID or a numeral that a line outside them has.
- udisks' `filesystem-fstab` action: mounting and unmounting an fstab line marked `x-udisks-auth`
  (those of the block). Without the rule udisks asks for the administrator password. With it, a
  partition inside the PC that got a numeral mounts without the password that udisks asks before
  mounting a system disk.

## Updates

`infynitae-update` pulls the source with git, only forward, then installs some of its files as
root (the lock screen's PAM file, the session entry) and builds it.

- With `UPDATE_SIGNING_KEY` set in the config (a GnuPG fingerprint, or an SSH key as git prints
  it), a new version is taken only when its commit carries a good signature by that key;
  otherwise the update says so and builds the source as it was. Unset (the default), updates
  work as before.
- Your password is asked once at the start. The steps that need root run before the build, and
  sudo's permission is ended (`sudo -k`) before the build starts, so nothing the build runs can
  use it.
- `install.sh` can be run from a download (`curl | bash`); reading the script first, or cloning
  and checking the commit's signature, is the careful way.
- `infynitae-update --rollback` swaps each `~/.local/bin/<name>` back with its saved
  `<name>.previous` copy without `sudo` (see [RECOVERY.md](RECOVERY.md)).

## Helper privilege dropping and input fuzzing

Session helpers that never need privilege escalation drop `NO_NEW_PRIVS` and clear inheritable
capabilities (`CapInh: 0`) where they start (`setpriv --no-new-privs --inh-caps=-all` in
`live/session.sh`, `live/env.sh`, `live/infynitae-reload`, and `XwaylandSupervisor::spawn`; plus
`prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0)` directly inside compiled C helpers). Once set, the kernel
forbids gaining privileges through SUID/SGID binaries or file capabilities across any later `exec`.

Note on `systemd-run`: `systemd-run --user --scope` rejects `NoNewPrivileges=yes`, and transient
user services lose shell redirection logs (`helpers.log`, `portal.log`), `$!` PID tracking in
`live/env.sh`, `oom_score_adj=300` from `live/session.sh`, and compatibility outside a live user
systemd bus, so `setpriv` + `prctl` are used in place without adding extra supervisor processes.

| Helper | Drops `NO_NEW_PRIVS` + `CapInh` | Why |
|---|---|---|
| `infynitae-portal` | Yes (`prctl` + `setpriv`) | Wayland/D-Bus portal; never escalates |
| `infynitae-wallpaper` | Yes (`prctl` + `setpriv`) | Draws wallpaper and runs `mpvpaper`; never escalates |
| `infynitae-record-stream` | Yes (`prctl`) | Reads screencopy frames to stdout for `ffmpeg` |
| `infynitae-xres-pid` | Yes (`prctl`) | Reads `_NET_WM_PID` / X-Resource client PIDs |
| `xwayland-satellite` | Yes (`setpriv` in `XwaylandSupervisor`) | X11 bridge; forks `Xwayland` unprivileged |
| `wl-clip-persist`, `wl-paste` | Yes (`setpriv`) | Clipboard persistence and `cliphist store` watch |
| `mako`, `swaync`, `dunst`, `fnott` | Yes (`setpriv`) | Fallback notification daemons |
| `squeekboard` | Yes (`setpriv`) | On-screen keyboard |
| `kbuildsycoca6`, `kbuildsycoca5` | Yes (`setpriv`) | Rebuilds KDE app cache at login |
| `autorotate` | Yes (`setpriv`) | Reads `monitor-sensor` and calls `infynitae msg` |
| `infynitae-polkit` | No | Invokes `polkit-agent-helper-1` / PAM authentication |
| `infynitae-lock`, `infynitae-sleep-lock` | No (`PR_SET_DUMPABLE=0` only) | PAM invokes SGID/SUID `unix_chkpwd` for unlock |
| `infynitae-automount`, `udiskie` | No | Invokes `pkexec` (`infynitae-drive-name`) and `udisksctl` |
| `infynitae-disk-guard` | No | Invokes `pkexec /usr/local/lib/infynitae-disk-fix` |
| `infynitae-recorder` | No | Invokes `gpu-screen-recorder` (`gsr-kms-server` `cap_sys_admin`) |
| `infynitae-edge` (`infynitae-edge-apps`) | No | Spawns user desktop apps, terminals (`launch.sh`), `xdg-open` |
| `infynitae-restore` | No | Re-launches saved user terminals and desktop apps |

Input-reading paths (`config_line` in `infynitae-wayland`, `parse_request` / `json::parse` in
`infynitae-ipc`, and `clipboard_entry` in `live/edge-apps`) have standalone `cargo-fuzz` targets
in `fuzz/` (run weekly in CI) plus deterministic crash and seed corpus unit tests in each crate.
