#!/bin/sh
# Infynitae Native Session Starter (Barebones & Fast)
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
if [ "$(cat /proc/$PPID/comm 2>/dev/null)" = "infynitae" ]; then
    renice -n -10 -p "$PPID" >/dev/null 2>&1 || true
fi

# 1. Environment & Config
if [ -f "$HOME/infynitae/live/env.sh" ]; then
    . "$HOME/infynitae/live/env.sh"
fi
if [ -f "$HOME/infynitae/live/config.sh" ]; then
    . "$HOME/infynitae/live/config.sh"
fi

# 1a. The password store for apps (gnome-keyring) runs before the lock screen asks for the
# password, so that password opens it (/etc/pam.d/infynitae), as a login does on other desktops;
# apps reach it through D-Bus
if command -v gnome-keyring-daemon >/dev/null 2>&1; then
    gnome-keyring-daemon --start --components=secrets >/dev/null 2>&1 || true
fi

# 1b. Start Screen (replacing the TTY prompt with the lock screen). First, before any window can
# open: nothing else starts until the compositor has confirmed the lock. If it cannot be engaged
# (no locker installed, the locker failed), the session ends rather than stay unlocked.
if [ "$START_LOCKED" = "1" ] || [ "$START_LOCKED" = "true" ] || [ "$1" = "--lock" ] \
    || [ "${INFYNITAE_RESTART_LOCKED:-}" = "1" ]; then
    LOCK_BIN=""
    if [ -x "$HOME/.local/bin/infynitae-lock" ]; then
        LOCK_BIN="$HOME/.local/bin/infynitae-lock"
    elif [ -x "$HOME/infynitae/live/lock/infynitae-lock" ]; then
        LOCK_BIN="$HOME/infynitae/live/lock/infynitae-lock"
    elif command -v infynitae-lock >/dev/null 2>&1; then
        LOCK_BIN="infynitae-lock"
    fi
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

# 2. Synchronize Wayland & D-Bus environment for Portals, PipeWire, and screensharing
if command -v dbus-update-activation-environment >/dev/null 2>&1; then
    dbus-update-activation-environment --systemd WAYLAND_DISPLAY XDG_CURRENT_DESKTOP DISPLAY XAUTHORITY 2>/dev/null || true
fi
systemctl --user start infynitae-shutdown-guard.service 2>/dev/null || true

# 2b. Screen sharing: the portal's config for the screens present (infynitae-reload screenshare)
for r in "$HOME/.local/bin/infynitae-reload" "$HOME/infynitae/live/infynitae-reload"; do
    if [ -x "$r" ]; then
        "$r" screenshare >/dev/null 2>&1 || true
        break
    fi
done

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
WP_BIN=""
if [ -x "$HOME/.local/bin/infynitae-wallpaper" ]; then
    WP_BIN="$HOME/.local/bin/infynitae-wallpaper"
elif [ -x "$HOME/infynitae/live/wallpaper/infynitae-wallpaper" ]; then
    WP_BIN="$HOME/infynitae/live/wallpaper/infynitae-wallpaper"
elif command -v infynitae-wallpaper >/dev/null 2>&1; then
    WP_BIN="infynitae-wallpaper"
fi

if [ -n "$WP_BIN" ]; then
    if [ -n "$WALLPAPER" ]; then
        "$WP_BIN" "$WALLPAPER" "${WALLPAPER_MODE:-fill}" &
    else
        "$WP_BIN" &
    fi
fi

# 4b. Native Dynamic Island & Top-Edge Touch Hybrid Shell (ENABLE_ISLAND)
case "${ENABLE_ISLAND:-true}" in
    0|false|no|off|FALSE|False) ;;
    *)
        if [ -x "$HOME/.local/bin/infynitae-island" ]; then
            "$HOME/.local/bin/infynitae-island" >/dev/null 2>&1 &
        fi
        ;;
esac


# 6. Native Screen Recorder & 20-minute Replay Buffer (Invincible background watchdog)
if [ "${ENABLE_REPLAY_BUFFER:-false}" = "true" ] || [ "${ENABLE_REPLAY_BUFFER:-false}" = "1" ]; then
    if [ -x "$HOME/.local/bin/infynitae-recorder" ]; then
        "$HOME/.local/bin/infynitae-recorder" supervisor >/dev/null 2>&1 &
    elif [ -x "$HOME/infynitae/live/record/infynitae-recorder" ]; then
        "$HOME/infynitae/live/record/infynitae-recorder" supervisor >/dev/null 2>&1 &
    fi
fi

# 6b. Clipboard persistence and history
case "${CLIPBOARD_PERSIST:-true}" in
    1|true|yes|on|TRUE|True)
        if command -v wl-clip-persist >/dev/null 2>&1; then
            wl-clip-persist --clipboard regular >/dev/null 2>&1 &
        fi
        ;;
esac

case "${CLIPBOARD_HISTORY:-true}" in
    1|true|yes|on|TRUE|True)
        if command -v cliphist >/dev/null 2>&1 && command -v wl-paste >/dev/null 2>&1; then
            wl-paste --type text --watch cliphist store >/dev/null 2>&1 &
            wl-paste --type image --watch cliphist store >/dev/null 2>&1 &
        fi
        ;;
esac

# 6c. Synchronize unified desktop theme & settings (Mako, Fuzzel, GTK, Qt, Terminals)
if [ -f "$HOME/infynitae/live/settings.sh" ]; then
    sh "$HOME/infynitae/live/settings.sh" apply >/dev/null 2>&1 || true
fi

# Notifications: NOTIFICATIONS picks the daemon ("auto": mako, swaync, dunst, fnott, the first
# installed; "none": none; or a command). Started here so the choice wins over D-Bus activation.
if [ -f "$HOME/infynitae/live/patch-discord-shortcuts.py" ] && command -v python3 >/dev/null 2>&1; then
    python3 "$HOME/infynitae/live/patch-discord-shortcuts.py" >/dev/null 2>&1 &
fi
NOTIFY_CMD=""
case "${NOTIFICATIONS:-auto}" in
    none|off|false) ;;
    auto)
        for d in mako swaync dunst fnott; do
            if command -v "$d" >/dev/null 2>&1; then NOTIFY_CMD="$d"; break; fi
        done
        ;;
    *) NOTIFY_CMD="$NOTIFICATIONS" ;;
esac
if [ -n "$NOTIFY_CMD" ]; then
    $NOTIFY_CMD >/dev/null 2>&1 &
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

# 6c2. Settings the config has that Infynitae skipped (unknown, misspelled, unreadable): each keeps
# its default, and a notification names them once the daemon is up
if [ -x "$HOME/infynitae/live/config-check" ]; then
    rm -f "$HOME/.local/state/infynitae/config-problems"
    (sleep 2; "$HOME/infynitae/live/config-check" --notify >/dev/null) &
fi

# 6d. USB drives and SD cards mount when plugged in (udiskie, through udisks2): AUTOMOUNT
case "${AUTOMOUNT:-true}" in
    1|true|yes|on|TRUE|True)
        if command -v udiskie >/dev/null 2>&1; then
            udiskie --no-tray --notify >/dev/null 2>&1 &
        fi
        ;;
esac

# 6d2. KDE apps' list of apps (Open With), from the menu file settings.sh provides
for b in kbuildsycoca6 kbuildsycoca5; do
    if command -v "$b" >/dev/null 2>&1; then
        "$b" >/dev/null 2>&1 &
        break
    fi
done

# 6e. ~/Documents, ~/Downloads, ~/Pictures … exist and apps know them (xdg-user-dirs)
if command -v xdg-user-dirs-update >/dev/null 2>&1; then
    xdg-user-dirs-update >/dev/null 2>&1 || true
fi

# 6e2. The on-screen keyboard (squeekboard, OSK): it shows for a text field. "always" runs it
# from the start; "auto" runs it in tablet mode only (the compositor's tablet-mode switch,
# launch.sh): running, it is an input method, and with one apps such as terminals take typed
# text from it instead of the keyboard's keys
case "${OSK:-auto}" in
    always|on|true)
        if command -v squeekboard >/dev/null 2>&1; then
            gsettings set org.gnome.desktop.a11y.applications screen-keyboard-enabled true 2>/dev/null || true
            squeekboard >/dev/null 2>&1 &
        fi
        ;;
esac

# 6e3. The battery's charge limit (BATTERY_LIMIT: a percentage, or off), where it is supported
case "${BATTERY_LIMIT:-off}" in
    off|false|no|"") ;;
    *) sh "$HOME/infynitae/live/battery-limit" "$BATTERY_LIMIT" >/dev/null 2>&1 || true ;;
esac

# 6e2. The screen locks before the machine sleeps (the lid, the power button): it wakes locked (SLEEP_LOCK)
case "${SLEEP_LOCK:-true}" in
    0|false|no|off|FALSE|False) ;;
    *)
        for s in "$HOME/.local/bin/infynitae-sleep-lock" "$HOME/infynitae/live/infynitae-sleep-lock"; do
            if [ -x "$s" ]; then
                "$s" >/dev/null 2>&1 &
                break
            fi
        done
        ;;
esac

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

# 7. User autostart script (blank canvas for user's future ricing)
if [ -x "$HOME/.config/infynitae/autostart.sh" ]; then
    "$HOME/.config/infynitae/autostart.sh" &
fi

# 7b. Session restore & watcher
RESTORE_BIN=""
if [ -x "$HOME/.local/bin/infynitae-restore" ]; then
    RESTORE_BIN="$HOME/.local/bin/infynitae-restore"
elif [ -x "$HOME/infynitae/live/infynitae-restore" ]; then
    RESTORE_BIN="$HOME/infynitae/live/infynitae-restore"
elif [ -x "$(dirname "$0")/infynitae-restore" ]; then
    RESTORE_BIN="$(dirname "$0")/infynitae-restore"
elif command -v infynitae-restore >/dev/null 2>&1; then
    RESTORE_BIN="infynitae-restore"
fi

RESTORE_SESSION=false
case "${SESSION_RESTORE:-true}" in
    1|true|yes|on|TRUE|True)
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
        ;;
esac

# 8. Launch default terminal on the infinite canvas, unless the session came back or
# START_TERMINAL is off
if [ "$RESTORE_SESSION" = "true" ]; then
    exit 0
fi
case "${START_TERMINAL:-true}" in
    0|false|no|off|FALSE|False) exit 0 ;;
esac

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
    exec $TERM_BIN
else
    echo "[infynitae] Warning: No terminal emulator found. Please install alacritty, foot, or kitty." >&2
fi
