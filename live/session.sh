#!/bin/sh
# Infynitae Native Session Starter (Barebones & Fast)
# Executed by the compositor's --exec argument.
umask 077

# 1. Environment & Config
if [ -f "$HOME/infynitae/live/env.sh" ]; then
    . "$HOME/infynitae/live/env.sh"
fi
if [ -f "$HOME/infynitae/live/config.sh" ]; then
    . "$HOME/infynitae/live/config.sh"
fi

# 1b. Start Screen (replacing the TTY prompt with the lock screen). First, before any window can
# open: nothing else starts until the compositor has confirmed the lock. If it cannot be engaged
# (no locker installed, the locker failed), the session ends rather than stay unlocked.
if [ "$START_LOCKED" = "1" ] || [ "$START_LOCKED" = "true" ] || [ "$1" = "--lock" ]; then
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

# 2b. Screen sharing: the portal's config for the screens present (infynitae-reload screenshare)
for r in "$HOME/.local/bin/infynitae-reload" "$HOME/infynitae/live/infynitae-reload"; do
    if [ -x "$r" ]; then
        "$r" screenshare >/dev/null 2>&1 || true
        break
    fi
done

# 3. Native Polkit authentication agent (for sudo / system prompts in GUI)
if [ -x "$HOME/.local/bin/infynitae-polkit" ]; then
    "$HOME/.local/bin/infynitae-polkit" >/dev/null 2>&1 &
elif [ -n "$POLKIT_AGENT" ] && [ -x "$POLKIT_AGENT" ]; then
    "$POLKIT_AGENT" >/dev/null 2>&1 &
fi

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

# 8. Launch default terminal on the infinite canvas
if [ "$RESTORE_SESSION" = "true" ]; then
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
    exec $TERM_BIN
else
    echo "[infynitae] Warning: No terminal emulator found. Please install alacritty, foot, or kitty." >&2
fi
