#!/bin/sh
# Started by Infynitae's --exec inside the compositor (WAYLAND_DISPLAY already points at it).

# Initialize decoupled standalone desktop environment
if [ -f "$HOME/infynitae/live/env.sh" ]; then
    . "$HOME/infynitae/live/env.sh"
fi

# Load user application and session preferences
if [ -f "$HOME/.config/infynitae/config" ]; then
    . "$HOME/.config/infynitae/config"
fi

# Start polkit authentication agent if present
for agent in \
    /usr/lib/hyprpolkitagent \
    /usr/lib/polkit-gnome/polkit-gnome-authentication-agent-1 \
    /usr/lib/polkit-kde-authentication-agent-1; do
    if [ -x "$agent" ]; then
        "$agent" >/dev/null 2>&1 &
        break
    fi
done

# Supervised Quickshell Dynamic Island & Radar HUD
# If quickshell exits (e.g. killed via Super+Shift+R or syntax error during live editing),
# it respawns automatically without taking down the compositor or any client windows.
LOG_DIR="$HOME/.local/state/infynitae"
mkdir -p "$LOG_DIR"
(
    sleep 0.1
    while true; do
        if command -v qs >/dev/null 2>&1; then
            qs -p "$HOME/infynitae/live/quickshell/shell.qml" >> "$LOG_DIR/quickshell.log" 2>&1
        fi
        sleep 1
    done
) &

# Launch default terminal
exec "${TERMINAL:-foot}"
