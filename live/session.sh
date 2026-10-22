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

# Start Quickshell Dynamic Island launcher in background
(sleep 0.1 && qs -p "$HOME/infynitae/live/quickshell/shell.qml" > "$HOME/infynitae/live/quickshell.log" 2>&1) &

# Launch default terminal
exec "${TERMINAL:-foot}"
