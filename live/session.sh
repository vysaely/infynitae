#!/bin/sh
# Infynitae session start script
# Executed by the compositor's --exec argument.
umask 077

# Started outside a compositor (a console login that runs this file by mistake): start the session
# itself, never the apps with no screen to show them on
if [ -z "$WAYLAND_DISPLAY" ] && [ -x "$HOME/infynitae/live/infynitae-session" ]; then
    echo "[infynitae] session.sh runs inside the compositor; starting infynitae-session instead" >&2
    exec "$HOME/infynitae/live/infynitae-session" "$@"
fi

# 0. The compositor's own thread runs ahead of apps (nice -10, the most the gamemode group's limit
# allows without root): if it is starved, every window stutters with it. Only its main thread is
# raised; the apps this script starts stay at nice 0.
# With real-time rights (`ulimit -r` 2 or more: the realtime group the installer adds the user
# to, from realtime-privileges) the same thread also gets round-robin real-time priority 2, as
# KWin does through rtkit: a busy CPU (a build, a training run) can no longer delay a frame or
# input. Low on purpose: audio (PipeWire, about 88) and the kernel's own threads still come
# first, and the kernel's real-time limit keeps 5% for the rest. Both with reset-on-fork: what
# the compositor starts later (every app opened by key, the satellite, its own threads) begins at
# normal priority. Without it those inherited real-time priority 2 and competed with the
# compositor (seen 2026-11-22: kitty, qemu, the edge and the satellite at RR 2).
if [ "$(cat /proc/$PPID/comm 2>/dev/null)" = "infynitae" ]; then
    renice -n -10 -p "$PPID" >/dev/null 2>&1 || true
    if command -v chrt >/dev/null 2>&1; then
        chrt -R -r -p 2 "$PPID" >/dev/null 2>&1 || chrt -R -o -p 0 "$PPID" >/dev/null 2>&1 || true
    fi
fi

# the state directory every log below writes to (a fresh install has none: the logs and the
# commands redirected into them failed)
mkdir -p "${XDG_STATE_HOME:-$HOME/.local/state}/infynitae"

# 1. Environment & Config (env.sh already sources config.sh and runs settings.sh apply once)
if [ -f "$HOME/infynitae/live/env.sh" ]; then
    . "$HOME/infynitae/live/env.sh"
elif [ -f "$HOME/infynitae/live/config.sh" ]; then
    . "$HOME/infynitae/live/config.sh"
fi

if ! command -v infy_bin >/dev/null 2>&1; then
    infy_bin() {
        for _infy_b in "$@"; do
            if [ -x "$HOME/.local/bin/$_infy_b" ]; then echo "$HOME/.local/bin/$_infy_b"; return 0;
            elif [ -x "$HOME/infynitae/live/$_infy_b" ]; then echo "$HOME/infynitae/live/$_infy_b"; return 0;
            elif command -v "$_infy_b" >/dev/null 2>&1; then command -v "$_infy_b"; return 0; fi
        done
        return 1
    }
fi
if ! command -v infy_on >/dev/null 2>&1; then
    infy_on() {
        case "${1:-}" in 1|[Tt][Rr][Uu][Ee]|[Yy][Ee][Ss]|[Oo][Nn]) return 0 ;; *) return 1 ;; esac
    }
fi

# 1a. The password store for apps runs before the lock screen asks for the password, so that
# password opens it (/etc/pam.d/infynitae), as a login does on other desktops; apps reach it
# through D-Bus. infynitae-keyring once its switch has been made (its tools/switch.sh enables its
# user service), else gnome-keyring; never both (they would race for the same D-Bus name).
if systemctl --user -q is-enabled infynitae-keyring.service 2>/dev/null; then
    systemctl --user start infynitae-keyring.service >/dev/null 2>&1 || true
elif command -v gnome-keyring-daemon >/dev/null 2>&1; then
    gnome-keyring-daemon --start --components=secrets >/dev/null 2>&1 || true
fi

# 1b. Start Screen (replacing the TTY prompt with the lock screen). First, before any window can
# open: nothing else starts until the compositor has confirmed the lock. If it cannot be engaged
# (no locker installed, the locker failed), the session ends rather than stay unlocked.
# Signed in at the login screen a moment ago (INFYNITAE_SIGNED_IN): START_LOCKED does not ask again.
if { infy_on "${START_LOCKED:-0}" && [ "${INFYNITAE_SIGNED_IN:-}" != "1" ]; } || [ "$1" = "--lock" ] \
    || [ "${INFYNITAE_RESTART_LOCKED:-}" = "1" ]; then
    LOCK_BIN=$(infy_bin infynitae-lock || true)
    LOCK_LOG="$HOME/.local/state/infynitae/lock.log"
    mkdir -p "$HOME/.local/state/infynitae"
    LOCKED=false
    if [ -n "$LOCK_BIN" ]; then
        "$LOCK_BIN" > "$LOCK_LOG" 2>&1 &
        LOCK_PID=$!
        # the locker prints this line once the compositor confirmed the lock (up to 15 s)
        i=0
        while [ "$i" -lt 150 ]; do
            if grep -q "Screen locked" "$LOCK_LOG" 2>/dev/null; then
                LOCKED=true
                break
            fi
            kill -0 "$LOCK_PID" 2>/dev/null || break
            sleep 0.1
            i=$((i + 1))
        done
        if [ "$LOCKED" != "true" ]; then
            sleep 0.2
            if grep -q "Screen locked" "$LOCK_LOG" 2>/dev/null; then
                LOCKED=true
            fi
        fi
    fi
    if [ "$LOCKED" != "true" ]; then
        echo "[infynitae] START_LOCKED: the lock screen could not be engaged (${LOCK_BIN:-infynitae-lock not installed}); ending the session" >&2
        # the compositor is this script's parent; on the console its supervisor must not restart it
        if ! tr '\0' ' ' < "/proc/$PPID/cmdline" 2>/dev/null | grep -q -- "--nested"; then
            : > "$HOME/.local/state/infynitae/stop"
        fi
        kill -TERM "$PPID" 2>/dev/null
        exit 1
    fi
fi

# 1c. When memory runs out, the kernel ends an app before the desktop. A process cannot lower its
# out-of-memory score below the one it started with (that needs root), so the desktop keeps the
# score it was given (the compositor, this script so far, the lock screen) and what this script
# starts from here on gets 300: the kernel then counts 30% of the memory against an app
# (oom_score_adj; launch.sh does the same for what a shortcut opens).
DESKTOP_OOM=""
read -r DESKTOP_OOM < /proc/self/oom_score_adj 2>/dev/null || true
if [ -n "$DESKTOP_OOM" ] && [ "$DESKTOP_OOM" -lt 300 ]; then
    echo 300 > /proc/self/oom_score_adj 2>/dev/null || true
fi

# 2. Synchronize Wayland & D-Bus environment for Portals, PipeWire, and screensharing
if command -v dbus-update-activation-environment >/dev/null 2>&1; then
    dbus-update-activation-environment --systemd WAYLAND_DISPLAY XDG_CURRENT_DESKTOP DISPLAY XAUTHORITY 2>/dev/null || true
fi
systemctl --user start infynitae-shutdown-guard.service 2>/dev/null || true
# a compositor restart (Ctrl+Alt+Backspace after an update) ends the wlroots portal, and systemd
# starts it again before the new display exists: three failures in a second, then it gives up for the
# session, and screenshots and remote desktop through the portal stop working (2026-11-15: 64 times
# in 3 days). Started again now that the display is there.
if systemctl --user is-failed --quiet xdg-desktop-portal-wlr.service 2>/dev/null; then
    systemctl --user reset-failed xdg-desktop-portal-wlr.service 2>/dev/null || true
    systemctl --user start --no-block xdg-desktop-portal-wlr.service 2>/dev/null || true
fi

# 2b. Screen sharing: the portal's config for the screens present (infynitae-reload screenshare)
RELOAD_BIN=$(infy_bin infynitae-reload || true)
if [ -n "$RELOAD_BIN" ]; then
    "$RELOAD_BIN" screenshare >/dev/null 2>&1 || true
fi

# 3. Native Polkit authentication agent (for sudo / system prompts in GUI)
case "${POLKIT_AGENT:-auto}" in
    none|off|false|0|FALSE|False) ;;
    auto|"")
        if [ -x "$HOME/.local/bin/infynitae-polkit" ]; then
            "$HOME/.local/bin/infynitae-polkit" >/dev/null 2>&1 &
        fi
        ;;
    *)
        if [ -x "$POLKIT_AGENT" ]; then
            "$POLKIT_AGENT" >/dev/null 2>&1 &
        fi
        ;;
esac

# 4. Wallpaper & Canvas Background (Configured or Auto-Detected)
WP_BIN=$(infy_bin infynitae-wallpaper || true)
if [ -n "$WP_BIN" ]; then
    if [ -n "$WALLPAPER" ]; then
        "$WP_BIN" "$WALLPAPER" "${WALLPAPER_MODE:-fill}" &
    else
        "$WP_BIN" &
    fi
fi

# 4b. The edge: window strip, control panel and clipboard (ENABLE_EDGE)
if infy_on "${ENABLE_EDGE:-true}"; then
    EDGE_BIN=$(infy_bin infynitae-edge infynitae-island || true)
    if [ -n "$EDGE_BIN" ]; then
        "$EDGE_BIN" >/dev/null 2>&1 &
    fi
fi

# 6. Screen recorder and replay buffer (the last 20 minutes; a supervisor restarts it)
if infy_on "${ENABLE_REPLAY_BUFFER:-false}"; then
    RECORDER_BIN=$(infy_bin infynitae-recorder || true)
    if [ -n "$RECORDER_BIN" ]; then
        "$RECORDER_BIN" supervisor >/dev/null 2>&1 &
    fi
fi

# 6b. Clipboard persistence and history
if infy_on "${CLIPBOARD_PERSIST:-true}"; then
    if command -v wl-clip-persist >/dev/null 2>&1; then
        wl-clip-persist --clipboard regular >/dev/null 2>&1 &
    fi
fi

if infy_on "${CLIPBOARD_HISTORY:-true}"; then
    if command -v cliphist >/dev/null 2>&1 && command -v wl-paste >/dev/null 2>&1; then
        wl-paste --type text --watch cliphist store >/dev/null 2>&1 &
        wl-paste --type image --watch cliphist store >/dev/null 2>&1 &
    fi
fi

# 6c. Theme sync: the config's colors and fonts to mako, fuzzel, GTK, Qt and terminals
# (skip if env.sh already applied it in this session)
if [ -z "${INFYNITAE_SETTINGS_APPLIED:-}" ] && [ -f "$HOME/infynitae/live/settings.sh" ]; then
    export INFYNITAE_SETTINGS_APPLIED="1"
    sh "$HOME/infynitae/live/settings.sh" apply >/dev/null 2>&1 || true
fi

# Notifications: NOTIFICATIONS picks the daemon ("auto": the edge when it runs (started above, it
# serves them itself), else mako, swaync, dunst, fnott, the first installed; "edge": the edge;
# "none": none; or a command). Started here so the choice wins over D-Bus activation.
NOTIFY_CMD=""
edge_notifies() {
    infy_on "${ENABLE_EDGE:-true}" \
        && case "$(readlink "$HOME/.local/bin/infynitae-edge" 2>/dev/null)" in
            infynitae-edge-rs | infynitae-edge-apps) true ;; *) false ;; esac
}
case "${NOTIFICATIONS:-auto}" in
    none|off|false|edge) ;;
    auto)
        if ! edge_notifies; then
            for d in mako swaync dunst fnott; do
                if command -v "$d" >/dev/null 2>&1; then NOTIFY_CMD="$d"; break; fi
            done
        fi
        ;;
    *) NOTIFY_CMD="$NOTIFICATIONS" ;;
esac
if [ -n "$NOTIFY_CMD" ]; then
    sh -c "$NOTIFY_CMD" >/dev/null 2>&1 &
fi

# 6c2. The last crash (infynitae-session wrote a report): said once, in a notification
CRASH_NOTE="$HOME/.local/state/infynitae/crashed"
if [ -f "$CRASH_NOTE" ]; then
    IFS="$(printf '\t')" read -r CRASH_REPORT CRASH_WHAT < "$CRASH_NOTE"
    rm -f "$CRASH_NOTE"
    if command -v notify-send >/dev/null 2>&1; then
        CRASH_REPORT=$(printf '%s' "$CRASH_REPORT" | sed "s|^$HOME|~|")
        # once the daemon started above is up
        (sleep 2; notify-send -a Infynitae -u critical "Infynitae restarted after a crash" \
            "$CRASH_WHAT
Report: $CRASH_REPORT") &
    fi
fi

# 6c3. Settings the config has that Infynitae skipped (unknown, misspelled, unreadable): each keeps
# its default, and a notification names them once the daemon is up
if [ -x "$HOME/infynitae/live/config-check" ]; then
    rm -f "$HOME/.local/state/infynitae/config-problems"
    (sleep 2; "$HOME/infynitae/live/config-check" --notify >/dev/null) &
fi

# 6d2. KDE apps' list of apps (Open With), from the menu file settings.sh provides
for b in kbuildsycoca6 kbuildsycoca5; do
    if command -v "$b" >/dev/null 2>&1; then
        "$b" >/dev/null 2>&1 &
        break
    fi
done

# 6e. ~/Documents, ~/Downloads, ~/Pictures … exist and apps know them (xdg-user-dirs)
if [ ! -f "$HOME/.config/user-dirs.dirs" ] && command -v xdg-user-dirs-update >/dev/null 2>&1; then
    xdg-user-dirs-update >/dev/null 2>&1 || true
fi

# 6e2. The on-screen keyboard (squeekboard, OSK) opens when a text field is pressed: the
# compositor asks (launch.sh osk). It runs hidden from the start, as the input method: without
# one the compositor never hears that a field is active. Its own showing, for every active
# field, stays off. OSK="off": it starts only when the panel's switch asks for it.
if command -v squeekboard >/dev/null 2>&1; then
    gsettings set org.gnome.desktop.a11y.applications screen-keyboard-enabled false 2>/dev/null || true
    pkill -x squeekboard 2>/dev/null || true
    case "${OSK:-auto}" in
        off|false|no|never) ;;
        *) squeekboard >/dev/null 2>&1 & ;;
    esac
fi

# 6e3. The battery's charge limit (BATTERY_LIMIT: a percentage, or off), where it is supported
case "${BATTERY_LIMIT:-off}" in
    off|false|no|"") ;;
    *) sh "$HOME/infynitae/live/battery-limit" "$BATTERY_LIMIT" >/dev/null 2>&1 || true ;;
esac

# 6e4. One watcher of the system bus, infynitae-bus-watch, for two things: USB drives and SD cards
# mount when plugged in (infynitae-automount, through udisks2: AUTOMOUNT), and the screen locks
# before the machine sleeps (the lid, the power button), so it wakes locked (infynitae-sleep-lock:
# LOCK_WHEN_AWAY, fallback SLEEP_LOCK). Installed before 2026-11-25, without the watcher, each
# script watches the bus itself.
AUTOMOUNT_BIN=""
SLEEP_LOCK_BIN=""
if infy_on "${AUTOMOUNT:-true}"; then
    AUTOMOUNT_BIN=$(infy_bin infynitae-automount || true)
    if [ -z "$AUTOMOUNT_BIN" ] && command -v udiskie >/dev/null 2>&1; then
        udiskie --no-tray --notify >/dev/null 2>&1 &
    fi
fi
if infy_on "${LOCK_WHEN_AWAY:-${SLEEP_LOCK:-true}}"; then
    SLEEP_LOCK_BIN=$(infy_bin infynitae-sleep-lock || true)
fi
BUS_WATCH_BIN=$(infy_bin infynitae-bus-watch || true)
if [ -z "$BUS_WATCH_BIN" ] && [ -n "$AUTOMOUNT_BIN" ]; then
    "$AUTOMOUNT_BIN" >/dev/null 2>&1 &
    AUTOMOUNT_BIN=""
fi
if [ -n "$AUTOMOUNT_BIN$SLEEP_LOCK_BIN" ]; then
    # the lock screen it starts keeps the desktop's out-of-memory score (1c); infynitae-automount
    # gives what it opens an app's
    (
        [ -n "$DESKTOP_OOM" ] && echo "$DESKTOP_OOM" > /proc/self/oom_score_adj 2>/dev/null
        [ -n "$BUS_WATCH_BIN" ] || exec "$SLEEP_LOCK_BIN"
        set --
        [ -n "$AUTOMOUNT_BIN" ] && set -- --udisks "$AUTOMOUNT_BIN"
        [ -n "$SLEEP_LOCK_BIN" ] && set -- "$@" --sleep "$SLEEP_LOCK_BIN"
        exec "$BUS_WATCH_BIN" "$@"
    ) >/dev/null 2>&1 &
fi

# 6f. Screen auto-rotation on convertible laptops (AUTO_ROTATE; needs iio-sensor-proxy)
case "${AUTO_ROTATE:-auto}" in
    false|0|off|no|FALSE|False|OFF|Off|NO|No) ;;
    *)
        if command -v monitor-sensor >/dev/null 2>&1; then
            for r in "$HOME/infynitae/live/autorotate" "$HOME/.local/bin/autorotate" "$(dirname "$0")/autorotate"; do
                if [ -x "$r" ]; then
                    "$r" >/dev/null 2>&1 &
                    break
                fi
            done
        fi
        ;;
esac

# 6g. RustDesk's service decides X11 or Wayland once per login, while this session is still a
# text console, and then guesses X11: no Wayland display for its windows (the "someone is
# connected" window never opens). Restarted now that the session is Wayland, it looks again.
# Needs live/remote/50-infynitae-rustdesk.rules installed; without it nothing happens.
if systemctl is-active --quiet rustdesk.service 2>/dev/null; then
    if systemctl restart --no-ask-password rustdesk.service 2>>"${XDG_STATE_HOME:-$HOME/.local/state}/infynitae/rustdesk.log"; then
        echo "$(date '+%F %T') restarted rustdesk.service for this Wayland session" >>"${XDG_STATE_HOME:-$HOME/.local/state}/infynitae/rustdesk.log"
    fi
fi

# 7. User autostart script
if [ -x "$HOME/.config/infynitae/autostart.sh" ]; then
    "$HOME/.config/infynitae/autostart.sh" &
fi
# 7a. Apps that asked to start with the session (~/.config/autostart, the XDG way)
[ -f "$HOME/infynitae/live/xdg-autostart" ] && sh "$HOME/infynitae/live/xdg-autostart"

# 7b. Session restore & watcher
RESTORE_BIN=$(infy_bin infynitae-restore || true)
RESTORE_SESSION=false
# restart: the watcher runs as with true; restore and check skip a snapshot from before this boot
if infy_on "${SESSION_RESTORE:-restart}" || [ "${SESSION_RESTORE:-restart}" = restart ]; then
    if [ -n "$RESTORE_BIN" ]; then
        # check: a saved window whose app can be launched (else the terminal below starts).
        # The watcher starts once the restore is done: a snapshot taken while windows are
        # still coming back would overwrite session.json with half the session.
        if "$RESTORE_BIN" check >/dev/null 2>&1; then
            RESTORE_SESSION=true
            { "$RESTORE_BIN" restore; exec "$RESTORE_BIN" watch; } >/dev/null 2>&1 &
        else
            "$RESTORE_BIN" watch >/dev/null 2>&1 &
        fi
    fi
fi

# 8. Launch default terminal on the infinite canvas, unless the session came back or
# START_TERMINAL is off
if [ "$RESTORE_SESSION" = "true" ]; then
    exit 0
fi
if ! infy_on "${START_TERMINAL:-true}"; then
    exit 0
fi

resolve_terminal() {
    if [ -n "$TERMINAL" ] && [ "$TERMINAL" != "auto" ]; then
        echo "$TERMINAL"
        return
    fi
    for t in alacritty foot kitty wezterm ghostty konsole xfce4-terminal gnome-terminal urxvt xterm; do
        if command -v "$t" >/dev/null 2>&1; then
            echo "$t"
            return
        fi
    done
    echo ""
}

TERM_BIN=$(resolve_terminal)
if [ -n "$TERM_BIN" ]; then
    if [ -x "$TERM_BIN" ]; then
        exec "$TERM_BIN"
    else
        exec sh -c "exec $TERM_BIN"
    fi
else
    echo "[infynitae] Warning: No terminal emulator found. Please install alacritty, foot, or kitty." >&2
fi
