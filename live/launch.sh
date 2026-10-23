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
if [ -z "$WAYLAND_DISPLAY" ] && [ -n "$XDG_RUNTIME_DIR" ]; then
    for wsock in "$XDG_RUNTIME_DIR"/wayland-[0-9]*; do
        if [ -S "$wsock" ]; then
            export WAYLAND_DISPLAY="${wsock##*/}"
            break
        fi
    done
fi

# Automatically resolve X11 DISPLAY and XAUTHORITY if not already set
if [ -z "$DISPLAY" ]; then
    for xsock in /tmp/.X11-unix/X*; do
        if [ -S "$xsock" ]; then
            num="${xsock##*/X}"
            export DISPLAY=":$num"
            break
        fi
    done
fi
if [ -z "$XAUTHORITY" ]; then
    if [ -n "$XDG_RUNTIME_DIR" ]; then
        AUTH_FILE=$(ls -t "$XDG_RUNTIME_DIR"/infynitae-xauth-* 2>/dev/null | head -1)
        if [ -f "$AUTH_FILE" ]; then
            export XAUTHORITY="$AUTH_FILE"
        fi
    fi
    if [ -z "$XAUTHORITY" ] && [ -f "$HOME/.Xauthority" ]; then
        export XAUTHORITY="$HOME/.Xauthority"
    fi
fi

# Allow complete user-script overrides in ~/.config/infynitae/launch.sh if present
USER_SCRIPT="$HOME/.config/infynitae/launch.sh"
if [ -x "$USER_SCRIPT" ]; then
    exec "$USER_SCRIPT" "$ACTION"
fi

# Universal terminal resolution (respects config if specified, or auto-detects kitty/alacritty/foot/etc.)
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

TERM_CMD=$(resolve_terminal)

case "$ACTION" in
    terminal)
        # Super+Q: Terminal emulator (auto-detects kitty, alacritty, foot, or uses $TERMINAL)
        if [ -n "$TERM_CMD" ]; then
            exec $TERM_CMD
        else
            echo "[infynitae] Error: No terminal emulator found. Please install kitty, alacritty, or foot." >&2
        fi
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
                if [ "${ENABLE_QUICKSHELL:-false}" = "true" ] && command -v qs >/dev/null 2>&1 && [ -f "$HOME/infynitae/live/quickshell/shell.qml" ]; then
                    exec qs ipc -p "$HOME/infynitae/live/quickshell/shell.qml" call island toggle
                elif command -v fuzzel >/dev/null 2>&1; then
                    exec fuzzel
                elif command -v rofi >/dev/null 2>&1; then
                    exec rofi -show drun
                elif command -v tofi-drun >/dev/null 2>&1; then
                    exec tofi-drun
                elif command -v wofi >/dev/null 2>&1; then
                    exec wofi --show drun
                elif command -v qs >/dev/null 2>&1 && [ -f "$HOME/infynitae/live/quickshell/shell.qml" ]; then
                    exec qs ipc -p "$HOME/infynitae/live/quickshell/shell.qml" call island toggle
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
        # Super+Shift+Tab: Overview (Quickshell Canvas Radar if enabled, or fast Fuzzel window switcher)
        if [ "${ENABLE_QUICKSHELL:-false}" = "true" ] && command -v qs >/dev/null 2>&1 && [ -f "$HOME/infynitae/live/quickshell/shell.qml" ]; then
            exec qs ipc -p "$HOME/infynitae/live/quickshell/shell.qml" call overview toggle
        elif [ -x "$HOME/infynitae/live/overview-menu.py" ]; then
            exec "$HOME/infynitae/live/overview-menu.py"
        elif command -v fuzzel >/dev/null 2>&1; then
            exec fuzzel
        fi
        ;;
    reload|reload-shell|restart-shell)
        # Super+Shift+BackSpace: Live reload settings, shell UI, and desktop environment
        # 1. Apply all settings from ~/.config/infynitae/config to GTK, Qt, cursors, environment
        if [ -x "$HOME/infynitae/live/settings.sh" ]; then
            "$HOME/infynitae/live/settings.sh" apply >/dev/null 2>&1 || true
        elif command -v infynitae-settings >/dev/null 2>&1; then
            infynitae-settings apply >/dev/null 2>&1 || true
        fi

        # 2. Restart Quickshell Island / HUD if running
        if pidof qs >/dev/null 2>&1; then
            killall qs 2>/dev/null || true
        fi

        # 3. Reload third-party bars/daemons if running
        if pidof waybar >/dev/null 2>&1; then
            killall -SIGUSR2 waybar 2>/dev/null || true
        fi
        if pidof mako >/dev/null 2>&1; then
            makoctl reload 2>/dev/null || true
        fi

        # 4. Notify user if notification service is present
        if command -v notify-send >/dev/null 2>&1; then
            notify-send -a Infynitae -u low "Infynitae Reloaded" "Configuration re-applied from ~/.config/infynitae/config" 2>/dev/null || true
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
    region-screenshot)
        # Super+Shift+R: Interactive region screenshot (slurp + grim)
        SHOT_DIR="${SCREENSHOT_DIR:-$HOME/Pictures/Screenshots}"
        mkdir -p "$SHOT_DIR"
        SHOT_FILE="$SHOT_DIR/region-$(date +%Y%m%d-%H%M%S).png"
        if command -v slurp >/dev/null 2>&1 && command -v grim >/dev/null 2>&1; then
            GEOM=$(slurp)
            if [ -n "$GEOM" ]; then
                grim -g "$GEOM" "$SHOT_FILE"
                if [ -f "$SHOT_FILE" ] && command -v wl-copy >/dev/null 2>&1; then
                    wl-copy < "$SHOT_FILE"
                fi
                if [ -f "$SHOT_FILE" ] && command -v notify-send >/dev/null 2>&1; then
                    notify-send -a Infynitae "Region Screenshot Saved" "$SHOT_FILE"
                fi
            fi
        elif command -v spectacle >/dev/null 2>&1; then
            spectacle -r -b -n -o "$SHOT_FILE" 2>/dev/null || true
            if [ -f "$SHOT_FILE" ] && command -v wl-copy >/dev/null 2>&1; then
                wl-copy < "$SHOT_FILE"
            fi
        fi
        ;;
    browser)
        # Web browser
        exec "${BROWSER:-firefox}"
        ;;
    volume-up)
        # Volume Up: PipeWire wpctl / amixer + synchronous OSD
        if command -v wpctl >/dev/null 2>&1; then
            wpctl set-volume -l 1.5 @DEFAULT_AUDIO_SINK@ 5%+
            VOL=$(wpctl get-volume @DEFAULT_AUDIO_SINK@ 2>/dev/null | awk '{print int($2 * 100)}')
            if [ -n "$VOL" ] && command -v notify-send >/dev/null 2>&1; then
                notify-send -a "Audio" -h string:x-canonical-private-synchronous:volume -h "int:value:$VOL" -t 1200 "Volume: ${VOL}%"
            fi
        elif command -v amixer >/dev/null 2>&1; then
            amixer set Master 5%+ >/dev/null 2>&1
        fi
        ;;
    volume-down)
        # Volume Down: PipeWire wpctl / amixer + synchronous OSD
        if command -v wpctl >/dev/null 2>&1; then
            wpctl set-volume -l 1.5 @DEFAULT_AUDIO_SINK@ 5%-
            VOL=$(wpctl get-volume @DEFAULT_AUDIO_SINK@ 2>/dev/null | awk '{print int($2 * 100)}')
            if [ -n "$VOL" ] && command -v notify-send >/dev/null 2>&1; then
                notify-send -a "Audio" -h string:x-canonical-private-synchronous:volume -h "int:value:$VOL" -t 1200 "Volume: ${VOL}%"
            fi
        elif command -v amixer >/dev/null 2>&1; then
            amixer set Master 5%- >/dev/null 2>&1
        fi
        ;;
    volume-mute)
        # Volume Mute: PipeWire wpctl / amixer + synchronous OSD
        if command -v wpctl >/dev/null 2>&1; then
            wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle
            MUTED=$(wpctl get-volume @DEFAULT_AUDIO_SINK@ 2>/dev/null | grep -i MUTED)
            if [ -n "$MUTED" ]; then
                MSG="Muted"
            else
                VOL=$(wpctl get-volume @DEFAULT_AUDIO_SINK@ 2>/dev/null | awk '{print int($2 * 100)}')
                MSG="Unmuted (${VOL}%)"
            fi
            if command -v notify-send >/dev/null 2>&1; then
                notify-send -a "Audio" -h string:x-canonical-private-synchronous:volume -t 1200 "$MSG"
            fi
        elif command -v amixer >/dev/null 2>&1; then
            amixer set Master toggle >/dev/null 2>&1
        fi
        ;;
    brightness-up)
        # Brightness Up: brightnessctl / light + synchronous OSD
        if command -v brightnessctl >/dev/null 2>&1; then
            brightnessctl set +5% >/dev/null 2>&1
            CUR=$(brightnessctl get 2>/dev/null)
            MAX=$(brightnessctl max 2>/dev/null)
            if [ -n "$CUR" ] && [ -n "$MAX" ] && [ "$MAX" -gt 0 ]; then
                PCT=$((CUR * 100 / MAX))
                if command -v notify-send >/dev/null 2>&1; then
                    notify-send -a "Display" -h string:x-canonical-private-synchronous:brightness -h "int:value:$PCT" -t 1200 "Brightness: ${PCT}%"
                fi
            fi
        elif command -v light >/dev/null 2>&1; then
            light -A 5
        fi
        ;;
    brightness-down)
        # Brightness Down: brightnessctl / light + synchronous OSD
        if command -v brightnessctl >/dev/null 2>&1; then
            brightnessctl set 5%- >/dev/null 2>&1
            CUR=$(brightnessctl get 2>/dev/null)
            MAX=$(brightnessctl max 2>/dev/null)
            if [ -n "$CUR" ] && [ -n "$MAX" ] && [ "$MAX" -gt 0 ]; then
                PCT=$((CUR * 100 / MAX))
                if command -v notify-send >/dev/null 2>&1; then
                    notify-send -a "Display" -h string:x-canonical-private-synchronous:brightness -h "int:value:$PCT" -t 1200 "Brightness: ${PCT}%"
                fi
            fi
        elif command -v light >/dev/null 2>&1; then
            light -U 5
        fi
        ;;
    media-play-pause)
        if command -v playerctl >/dev/null 2>&1; then
            playerctl play-pause
        fi
        ;;
    media-next)
        if command -v playerctl >/dev/null 2>&1; then
            playerctl next
        fi
        ;;
    media-prev)
        if command -v playerctl >/dev/null 2>&1; then
            playerctl previous
        fi
        ;;
    lock-screen)
        # Super+L: Lock screen (hyprlock, swaylock, gtklock, waylock)
        LOCKER="${SCREEN_LOCKER:-auto}"
        case "$LOCKER" in
            auto|"")
                if command -v hyprlock >/dev/null 2>&1; then
                    exec hyprlock
                elif command -v swaylock >/dev/null 2>&1; then
                    exec swaylock -f -c 000000
                elif command -v gtklock >/dev/null 2>&1; then
                    exec gtklock
                elif command -v waylock >/dev/null 2>&1; then
                    exec waylock
                fi
                ;;
            *)
                exec $LOCKER
                ;;
        esac
        ;;
    mute|toggle-mute)
        # Trigger Discord / App Global Shortcuts portal (in-app mute only, zero OS hardware muting)
        if [ -S "$XDG_RUNTIME_DIR/infynitae/shortcuts.sock" ]; then
            python3 -c "import socket; s=socket.socket(socket.AF_UNIX, socket.SOCK_STREAM); s.connect('$XDG_RUNTIME_DIR/infynitae/shortcuts.sock'); s.sendall(b'mute'); s.close()" 2>/dev/null || true
        fi
        ;;
    deafen|toggle-deafen)
        # Trigger Discord / App Global Shortcuts portal (in-app deafen only, zero OS hardware muting)
        if [ -S "$XDG_RUNTIME_DIR/infynitae/shortcuts.sock" ]; then
            python3 -c "import socket; s=socket.socket(socket.AF_UNIX, socket.SOCK_STREAM); s.connect('$XDG_RUNTIME_DIR/infynitae/shortcuts.sock'); s.sendall(b'deafen'); s.close()" 2>/dev/null || true
        fi
        ;;
    *)
        echo "Unknown action: $ACTION" >&2
        exit 1
        ;;
esac
