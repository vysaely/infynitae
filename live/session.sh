#!/bin/sh
# Infynitae Native Session Starter (Barebones & Fast)
# Executed by the compositor's --exec argument.
umask 077

# 1. Environment & Config
if [ -f "$HOME/infynitae/live/env.sh" ]; then
    . "$HOME/infynitae/live/env.sh"
fi
if [ -f "$HOME/.config/infynitae/config" ]; then
    . "$HOME/.config/infynitae/config"
fi

# 2. Synchronize Wayland & D-Bus environment for Portals, PipeWire, and screensharing
if command -v dbus-update-activation-environment >/dev/null 2>&1; then
    dbus-update-activation-environment --systemd WAYLAND_DISPLAY XDG_CURRENT_DESKTOP DISPLAY XAUTHORITY 2>/dev/null || true
fi

# 3. Native Polkit authentication agent (for sudo / system prompts in GUI)
if [ -x "$HOME/.local/bin/infynitae-polkit" ]; then
    "$HOME/.local/bin/infynitae-polkit" >/dev/null 2>&1 &
elif [ -n "$POLKIT_AGENT" ] && [ -x "$POLKIT_AGENT" ]; then
    "$POLKIT_AGENT" >/dev/null 2>&1 &
fi

# 4. Wallpaper if explicitly configured in ~/.config/infynitae/config
if [ -n "$WALLPAPER" ]; then
    if [ -x "$HOME/.local/bin/infynitae-wallpaper" ]; then
        "$HOME/.local/bin/infynitae-wallpaper"
    elif [ -x "$HOME/infynitae/live/wallpaper/infynitae-wallpaper" ]; then
        "$HOME/infynitae/live/wallpaper/infynitae-wallpaper"
    elif command -v infynitae-wallpaper >/dev/null 2>&1; then
        infynitae-wallpaper
    fi
fi

# 5. User autostart script (blank canvas for user's future ricing)
if [ -x "$HOME/.config/infynitae/autostart.sh" ]; then
    "$HOME/.config/infynitae/autostart.sh" &
fi

# 6. Launch default terminal on the infinite canvas
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
