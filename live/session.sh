#!/bin/sh
# Started by Infynitae's --exec inside the compositor (WAYLAND_DISPLAY already points at it).
umask 077

# Initialize decoupled standalone desktop environment
if [ -f "$HOME/infynitae/live/env.sh" ]; then
    . "$HOME/infynitae/live/env.sh"
fi

# Load user application and session preferences
if [ -f "$HOME/.config/infynitae/config" ]; then
    . "$HOME/.config/infynitae/config"
fi

# Start polkit authentication agent if present (respects POLKIT_AGENT or autodetects)
if [ -n "$POLKIT_AGENT" ] && [ -x "$POLKIT_AGENT" ]; then
    "$POLKIT_AGENT" >/dev/null 2>&1 &
else
    for agent in \
        "$HOME/.local/bin/infynitae-polkit" \
        "$HOME/.local/bin/infynitae-shell" \
        /usr/lib/hyprpolkitagent \
        /usr/lib/polkit-gnome/polkit-gnome-authentication-agent-1 \
        /usr/lib/polkit-kde-authentication-agent-1; do
        if [ -x "$agent" ]; then
            if [ "$agent" = "$HOME/.local/bin/infynitae-shell" ]; then
                "$agent" polkit >/dev/null 2>&1 &
            else
                "$agent" >/dev/null 2>&1 &
            fi
            break
        fi
    done
fi

# User autostart script if present (e.g. waybar, mako, swaybg, etc.)
if [ -x "$HOME/.config/infynitae/autostart.sh" ]; then
    "$HOME/.config/infynitae/autostart.sh" &
fi

# Start wallpaper if configured in ~/.config/infynitae/config
if [ -n "$WALLPAPER" ]; then
    if [ -x "$HOME/infynitae/live/wallpaper/infynitae-wallpaper.sh" ]; then
        "$HOME/infynitae/live/wallpaper/infynitae-wallpaper.sh" &
    elif command -v infynitae-wallpaper >/dev/null 2>&1; then
        infynitae-wallpaper &
    fi
fi

# Start clipboard persistence daemon if present (wl-clip-persist or cliphist)
if command -v wl-clip-persist >/dev/null 2>&1; then
    wl-clip-persist --clipboard regular >/dev/null 2>&1 &
elif command -v cliphist >/dev/null 2>&1 && command -v wl-paste >/dev/null 2>&1; then
    wl-paste --watch cliphist store >/dev/null 2>&1 &
fi

# Ensure PipeWire and Portal are cleanly synchronized with the new Wayland display
systemctl --user restart pipewire wireplumber xdg-desktop-portal-wlr xdg-desktop-portal >/dev/null 2>&1 || true

# Refresh RustDesk user server to bind cleanly to the fresh Wayland session and PipeWire
if command -v rustdesk >/dev/null 2>&1; then
    (
        sleep 1
        pkill -f "/usr/share/rustdesk/rustdesk --server" >/dev/null 2>&1 || true
    ) &
fi

# Supervised Quickshell Dynamic Island & Radar HUD
# If quickshell exits (e.g. killed via Super+Shift+R or syntax error during live editing),
# it respawns automatically without taking down the compositor or any client windows.
LOG_DIR="$HOME/.local/state/infynitae"
mkdir -p "$LOG_DIR"
if [ "${ENABLE_QUICKSHELL:-true}" != "false" ]; then
    (
        sleep 0.1
        while true; do
            if command -v qs >/dev/null 2>&1 && [ -f "$HOME/infynitae/live/quickshell/shell.qml" ]; then
                qs -p "$HOME/infynitae/live/quickshell/shell.qml" >> "$LOG_DIR/quickshell.log" 2>&1
            fi
            sleep 1
        done
    ) &
fi

# Universal terminal auto-detection (respects user config, or checks kitty, alacritty, foot, etc.)
resolve_terminal() {
    if [ -n "$TERMINAL" ] && [ "$TERMINAL" != "auto" ]; then
        echo "$TERMINAL"
        return
    fi
    for t in kitty alacritty foot wezterm ghostty konsole xfce4-terminal gnome-terminal urxvt xterm; do
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
    echo "[infynitae] Warning: No terminal emulator found (e.g. kitty, alacritty, foot)" >&2
fi
