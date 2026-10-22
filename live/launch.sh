#!/bin/sh
# Universal action launcher for Infynitae
# Dispatches shortcuts: terminal (Super+Q), file-manager (Super+E), launcher (Super+W)
ACTION="$1"

# Initialize decoupled standalone desktop environment
if [ -f "$HOME/infynitae/live/env.sh" ]; then
    . "$HOME/infynitae/live/env.sh"
fi

# Load user configuration if present
if [ -f "$HOME/.config/infynitae/config" ]; then
    . "$HOME/.config/infynitae/config"
fi

# Automatically resolve the current Infynitae Wayland display if not already set
if [ -z "$WAYLAND_DISPLAY" ]; then
    export WAYLAND_DISPLAY=$(ls -t "$XDG_RUNTIME_DIR"/wayland-* 2>/dev/null | head -1 | xargs -r basename)
fi

# Allow complete user-script overrides in ~/.config/infynitae/launch.sh if present
USER_SCRIPT="$HOME/.config/infynitae/launch.sh"
if [ -x "$USER_SCRIPT" ]; then
    exec "$USER_SCRIPT" "$ACTION"
fi

TERM_CMD="${TERMINAL:-foot}"

case "$ACTION" in
    terminal)
        # Super+Q: Terminal emulator (configurable via ~/.config/infynitae/config)
        exec $TERM_CMD
        ;;
    file-manager|files)
        # Super+E: File explorer (configurable via ~/.config/infynitae/config)
        FM="${FILE_MANAGER:-auto}"
        case "$FM" in
            auto|"")
                if command -v yazi >/dev/null 2>&1; then
                    exec $TERM_CMD -e yazi
                elif command -v lf >/dev/null 2>&1; then
                    exec $TERM_CMD -e lf
                elif command -v thunar >/dev/null 2>&1; then
                    exec thunar
                elif command -v nemo >/dev/null 2>&1; then
                    exec nemo
                elif command -v dolphin >/dev/null 2>&1; then
                    exec dolphin --new-window
                else
                    exec $TERM_CMD -e sh -c "ls -la; exec \$SHELL"
                fi
                ;;
            yazi)
                exec $TERM_CMD -e yazi
                ;;
            lf)
                exec $TERM_CMD -e lf
                ;;
            ranger)
                exec $TERM_CMD -e ranger
                ;;
            dolphin)
                exec dolphin --new-window
                ;;
            *)
                exec $FM
                ;;
        esac
        ;;
    launcher|island)
        # Super+W: Application launcher (configurable via ~/.config/infynitae/config)
        L="${LAUNCHER:-auto}"
        case "$L" in
            auto|"")
                if command -v qs >/dev/null 2>&1 && [ -f "$HOME/infynitae/live/quickshell/shell.qml" ]; then
                    exec qs ipc -p "$HOME/infynitae/live/quickshell/shell.qml" call island toggle
                elif command -v fuzzel >/dev/null 2>&1; then
                    exec fuzzel
                elif command -v rofi >/dev/null 2>&1; then
                    exec rofi -show drun
                elif command -v tofi-drun >/dev/null 2>&1; then
                    exec tofi-drun
                elif command -v wofi >/dev/null 2>&1; then
                    exec wofi --show drun
                fi
                ;;
            quickshell|qs)
                exec qs ipc -p "$HOME/infynitae/live/quickshell/shell.qml" call island toggle
                ;;
            fuzzel)
                exec fuzzel
                ;;
            rofi)
                exec rofi -show drun
                ;;
            tofi)
                exec tofi-drun
                ;;
            wofi)
                exec wofi --show drun
                ;;
            *)
                exec $L
                ;;
        esac
        ;;
    overview|radar)
        # Super+Shift+Tab: Quickshell Canvas Overview / Radar
        exec qs ipc -p "$HOME/infynitae/live/quickshell/shell.qml" call overview toggle
        ;;
    reload|reload-shell|restart-shell)
        # Super+Shift+R: Reload Quickshell UI and live scripts
        killall qs 2>/dev/null || true
        if command -v notify-send >/dev/null 2>&1; then
            notify-send -a Infynitae "Shell Reloaded" "Quickshell supervisor restarted UI" 2>/dev/null || true
        fi
        ;;
    screenshot)
        # Super+Shift+S / PrintScreen: Screenshot dispatcher
        SHOT_DIR="${SCREENSHOT_DIR:-$HOME/Pictures/Screenshots}"
        mkdir -p "$SHOT_DIR"
        SHOT_FILE="$SHOT_DIR/shot-$(date +%Y%m%d-%H%M%S).png"
        if [ -n "$SCREENSHOT_CMD" ]; then
            exec $SCREENSHOT_CMD "$SHOT_FILE"
        elif command -v grim >/dev/null 2>&1; then
            grim "$SHOT_FILE"
            if [ -f "$SHOT_FILE" ] && command -v wl-copy >/dev/null 2>&1; then
                wl-copy < "$SHOT_FILE"
            fi
            if [ -f "$SHOT_FILE" ] && command -v notify-send >/dev/null 2>&1; then
                notify-send -a Infynitae "Screenshot Saved" "$SHOT_FILE"
            fi
        elif command -v spectacle >/dev/null 2>&1; then
            spectacle -b -n -o "$SHOT_FILE" 2>/dev/null || true
            if [ -f "$SHOT_FILE" ] && command -v wl-copy >/dev/null 2>&1; then
                wl-copy < "$SHOT_FILE"
            fi
        fi
        ;;
    browser)
        # Web browser
        exec "${BROWSER:-firefox}"
        ;;
    *)
        echo "Unknown action: $ACTION" >&2
        exit 1
        ;;
esac
