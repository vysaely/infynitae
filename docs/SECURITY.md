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

## While the screen is locked

Nothing captures (a capture shows the lock screen or is refused), no virtual keyboard or input
method reaches anything, and only the lock screen gets keys. Only the program that locked can
unlock, and only an allowed locker may take the lock. If the lock screen's program dies while
locked, the session stays locked and the compositor starts the lock screen again itself.
