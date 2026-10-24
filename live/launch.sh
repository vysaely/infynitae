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

# Automatically resolve X11 DISPLAY and XAUTHORITY with auto-healing
if [ -z "$DISPLAY" ] || [ ! -S "/tmp/.X11-unix/X${DISPLAY#*:}" ]; then
    for xsock in /tmp/.X11-unix/X*; do
        if [ -S "$xsock" ]; then
            num="${xsock##*/X}"
            export DISPLAY=":$num"
            break
        fi
    done
fi


LATEST_XAUTH=""
if [ -n "$XDG_RUNTIME_DIR" ]; then
    if [ -f "$XDG_RUNTIME_DIR/infynitae-xauth" ]; then
        LATEST_XAUTH="$XDG_RUNTIME_DIR/infynitae-xauth"
    else
        LATEST_XAUTH=$(ls -t "$XDG_RUNTIME_DIR"/infynitae-xauth-* 2>/dev/null | head -1)
    fi
fi

if [ -z "$XAUTHORITY" ] || [ ! -f "$XAUTHORITY" ]; then
    if [ -n "$LATEST_XAUTH" ] && [ -f "$LATEST_XAUTH" ]; then
        export XAUTHORITY="$LATEST_XAUTH"
    elif [ -f "$HOME/.Xauthority" ]; then
        export XAUTHORITY="$HOME/.Xauthority"
    fi
elif [ -n "$LATEST_XAUTH" ] && [ -f "$LATEST_XAUTH" ] && [ "$XAUTHORITY" != "$LATEST_XAUTH" ]; then
    export XAUTHORITY="$LATEST_XAUTH"
fi

if [ -n "$XAUTHORITY" ] && [ -f "$XAUTHORITY" ]; then
    if [ -n "$XDG_RUNTIME_DIR" ] && [ "$XAUTHORITY" != "$XDG_RUNTIME_DIR/infynitae-xauth" ]; then
        ln -sf "$XAUTHORITY" "$XDG_RUNTIME_DIR/infynitae-xauth" 2>/dev/null || true
    fi
    # never replace a real ~/.Xauthority (another desktop's); only ours, a symlink
    if [ "$XAUTHORITY" != "$HOME/.Xauthority" ] && { [ -L "$HOME/.Xauthority" ] || [ ! -e "$HOME/.Xauthority" ]; }; then
        ln -sf "$XAUTHORITY" "$HOME/.Xauthority" 2>/dev/null || true
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
        # 1. Re-apply settings and restart the wallpaper (infynitae-reload; windows stay open)
        "$HOME/infynitae/live/infynitae-reload" settings wallpaper screenshare >/dev/null 2>&1 || true

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

        # 5. Notify user if notification service is present
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
        # Volume Up: PipeWire wpctl / PulseAudio pactl / amixer + synchronous OSD
        if command -v wpctl >/dev/null 2>&1; then
            wpctl set-volume -l 1.5 @DEFAULT_AUDIO_SINK@ 5%+
            VOL=$(wpctl get-volume @DEFAULT_AUDIO_SINK@ 2>/dev/null | awk '{print int($2 * 100)}')
            if [ -n "$VOL" ] && command -v notify-send >/dev/null 2>&1; then
                notify-send -a "Audio" -h string:x-canonical-private-synchronous:volume -h "int:value:$VOL" -t 1200 "Volume: ${VOL}%"
            fi
        elif command -v pactl >/dev/null 2>&1; then
            pactl set-sink-volume @DEFAULT_SINK@ +5% >/dev/null 2>&1
            VOL=$(pactl get-sink-volume @DEFAULT_SINK@ 2>/dev/null | grep -Po '[0-9]+(?=%)' | head -1)
            if [ -n "$VOL" ] && command -v notify-send >/dev/null 2>&1; then
                notify-send -a "Audio" -h string:x-canonical-private-synchronous:volume -h "int:value:$VOL" -t 1200 "Volume: ${VOL}%"
            fi
        elif command -v amixer >/dev/null 2>&1; then
            amixer set Master 5%+ >/dev/null 2>&1
        fi
        ;;
    volume-down)
        # Volume Down: PipeWire wpctl / PulseAudio pactl / amixer + synchronous OSD
        if command -v wpctl >/dev/null 2>&1; then
            wpctl set-volume -l 1.5 @DEFAULT_AUDIO_SINK@ 5%-
            VOL=$(wpctl get-volume @DEFAULT_AUDIO_SINK@ 2>/dev/null | awk '{print int($2 * 100)}')
            if [ -n "$VOL" ] && command -v notify-send >/dev/null 2>&1; then
                notify-send -a "Audio" -h string:x-canonical-private-synchronous:volume -h "int:value:$VOL" -t 1200 "Volume: ${VOL}%"
            fi
        elif command -v pactl >/dev/null 2>&1; then
            pactl set-sink-volume @DEFAULT_SINK@ -5% >/dev/null 2>&1
            VOL=$(pactl get-sink-volume @DEFAULT_SINK@ 2>/dev/null | grep -Po '[0-9]+(?=%)' | head -1)
            if [ -n "$VOL" ] && command -v notify-send >/dev/null 2>&1; then
                notify-send -a "Audio" -h string:x-canonical-private-synchronous:volume -h "int:value:$VOL" -t 1200 "Volume: ${VOL}%"
            fi
        elif command -v amixer >/dev/null 2>&1; then
            amixer set Master 5%- >/dev/null 2>&1
        fi
        ;;
    volume-mute)
        # Volume Mute: PipeWire wpctl / PulseAudio pactl / amixer + synchronous OSD
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
        elif command -v pactl >/dev/null 2>&1; then
            pactl set-sink-mute @DEFAULT_SINK@ toggle >/dev/null 2>&1
            MUTED=$(pactl get-sink-mute @DEFAULT_SINK@ 2>/dev/null | grep -i yes)
            if [ -n "$MUTED" ]; then
                MSG="Muted"
            else
                VOL=$(pactl get-sink-volume @DEFAULT_SINK@ 2>/dev/null | grep -Po '[0-9]+(?=%)' | head -1)
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
        # Brightness Up: brightnessctl / light / sysfs kernel fallback (Steam Deck, Handhelds, Laptops)
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
            PCT=$(light -G 2>/dev/null | awk '{print int($1)}')
            if [ -n "$PCT" ] && command -v notify-send >/dev/null 2>&1; then
                notify-send -a "Display" -h string:x-canonical-private-synchronous:brightness -h "int:value:$PCT" -t 1200 "Brightness: ${PCT}%"
            fi
        else
            for b in /sys/class/backlight/*; do
                if [ -w "$b/brightness" ] && [ -f "$b/max_brightness" ]; then
                    CUR=$(cat "$b/brightness" 2>/dev/null || echo 0)
                    MAX=$(cat "$b/max_brightness" 2>/dev/null || echo 0)
                    STEP=$((MAX / 20))
                    [ "$STEP" -lt 1 ] && STEP=1
                    NEW=$((CUR + STEP))
                    [ "$NEW" -gt "$MAX" ] && NEW=$MAX
                    echo "$NEW" > "$b/brightness" 2>/dev/null || true
                    PCT=$((NEW * 100 / MAX))
                    if command -v notify-send >/dev/null 2>&1; then
                        notify-send -a "Display" -h string:x-canonical-private-synchronous:brightness -h "int:value:$PCT" -t 1200 "Brightness: ${PCT}%"
                    fi
                    break
                fi
            done
        fi
        ;;
    brightness-down)
        # Brightness Down: brightnessctl / light / sysfs kernel fallback (Steam Deck, Handhelds, Laptops)
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
            PCT=$(light -G 2>/dev/null | awk '{print int($1)}')
            if [ -n "$PCT" ] && command -v notify-send >/dev/null 2>&1; then
                notify-send -a "Display" -h string:x-canonical-private-synchronous:brightness -h "int:value:$PCT" -t 1200 "Brightness: ${PCT}%"
            fi
        else
            for b in /sys/class/backlight/*; do
                if [ -w "$b/brightness" ] && [ -f "$b/max_brightness" ]; then
                    CUR=$(cat "$b/brightness" 2>/dev/null || echo 0)
                    MAX=$(cat "$b/max_brightness" 2>/dev/null || echo 0)
                    STEP=$((MAX / 20))
                    [ "$STEP" -lt 1 ] && STEP=1
                    NEW=$((CUR - STEP))
                    [ "$NEW" -lt 1 ] && NEW=1
                    echo "$NEW" > "$b/brightness" 2>/dev/null || true
                    PCT=$((NEW * 100 / MAX))
                    if command -v notify-send >/dev/null 2>&1; then
                        notify-send -a "Display" -h string:x-canonical-private-synchronous:brightness -h "int:value:$PCT" -t 1200 "Brightness: ${PCT}%"
                    fi
                    break
                fi
            done
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
    exec)
        # a user command from the config (EXEC_<NAME>="CHORD:command"), with the session's env
        [ -n "$2" ] && exec sh -c "$2"
        ;;
    lock-screen)
        # Super+L: Lock screen (native infynitae-lock, hyprlock, swaylock, gtklock, waylock)
        LOCKER="${SCREEN_LOCKER:-auto}"
        case "$LOCKER" in
            auto|"")
                if command -v infynitae-lock >/dev/null 2>&1; then
                    exec infynitae-lock
                elif command -v hyprlock >/dev/null 2>&1; then
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
    notify-privacy-on)
        # OSD Toast: Privacy Mode Enabled
        if command -v notify-send >/dev/null 2>&1; then
            notify-send -a "Infynitae" -i security-high -h string:x-canonical-private-synchronous:privacy -t 1500 "Privacy Mode: ON" "Window hidden from screen share & capture" 2>/dev/null || true
        fi
        ;;
    notify-privacy-off)
        # OSD Toast: Privacy Mode Disabled
        if command -v notify-send >/dev/null 2>&1; then
            notify-send -a "Infynitae" -i security-low -h string:x-canonical-private-synchronous:privacy -t 1500 "Privacy Mode: OFF" "Window visible to screen capture" 2>/dev/null || true
        fi
        ;;
    notify-idle-inhibit-on)
        # OSD Toast: Idle Sleep Inhibited
        if command -v notify-send >/dev/null 2>&1; then
            notify-send -a "Infynitae" -i display -h string:x-canonical-private-synchronous:idle -t 1500 "Idle Inhibit: ON" "Stay Awake enabled (display sleep suppressed)" 2>/dev/null || true
        fi
        ;;
    notify-idle-inhibit-off)
        # OSD Toast: Idle Sleep Normal
        if command -v notify-send >/dev/null 2>&1; then
            notify-send -a "Infynitae" -i display -h string:x-canonical-private-synchronous:idle -t 1500 "Idle Inhibit: OFF" "Display sleep restored (normal timeout)" 2>/dev/null || true
        fi
        ;;
    toggle-idle-inhibit|inhibit-idle)
        # Send keypress or signal to compositor to toggle idle inhibit
        if [ -S "$XDG_RUNTIME_DIR/infynitae/shortcuts.sock" ]; then
            python3 -c "import socket; s=socket.socket(socket.AF_UNIX, socket.SOCK_STREAM); s.connect('$XDG_RUNTIME_DIR/infynitae/shortcuts.sock'); s.sendall(b'idle_inhibit'); s.close()" 2>/dev/null || true
        fi
        ;;
    toggle-privacy|privacy)
        # Send keypress or signal to compositor to toggle privacy
        if [ -S "$XDG_RUNTIME_DIR/infynitae/shortcuts.sock" ]; then
            python3 -c "import socket; s=socket.socket(socket.AF_UNIX, socket.SOCK_STREAM); s.connect('$XDG_RUNTIME_DIR/infynitae/shortcuts.sock'); s.sendall(b'privacy'); s.close()" 2>/dev/null || true
        fi
        ;;
    save-replay|clip-save|clip)
        # Super+Alt+C: Save rolling 20-minute hardware replay clip (zero game FPS hit)
        if [ -x "$HOME/.local/bin/infynitae-recorder" ]; then
            exec "$HOME/.local/bin/infynitae-recorder" save-replay
        elif [ -x "$HOME/infynitae/live/record/infynitae-recorder" ]; then
            exec "$HOME/infynitae/live/record/infynitae-recorder" save-replay
        fi
        ;;
    record-toggle|toggle-record)
        # Super+Alt+R: Toggle continuous screen recording
        if [ -x "$HOME/.local/bin/infynitae-recorder" ]; then
            exec "$HOME/.local/bin/infynitae-recorder" record-toggle
        elif [ -x "$HOME/infynitae/live/record/infynitae-recorder" ]; then
            exec "$HOME/infynitae/live/record/infynitae-recorder" record-toggle
        fi
        ;;
    record-start)
        if [ -x "$HOME/.local/bin/infynitae-recorder" ]; then
            exec "$HOME/.local/bin/infynitae-recorder" record-start
        elif [ -x "$HOME/infynitae/live/record/infynitae-recorder" ]; then
            exec "$HOME/infynitae/live/record/infynitae-recorder" record-start
        fi
        ;;
    record-stop)
        if [ -x "$HOME/.local/bin/infynitae-recorder" ]; then
            exec "$HOME/.local/bin/infynitae-recorder" record-stop
        elif [ -x "$HOME/infynitae/live/record/infynitae-recorder" ]; then
            exec "$HOME/infynitae/live/record/infynitae-recorder" record-stop
        fi
        ;;
    replay-start)
        if [ -x "$HOME/.local/bin/infynitae-recorder" ]; then
            exec "$HOME/.local/bin/infynitae-recorder" replay-start
        elif [ -x "$HOME/infynitae/live/record/infynitae-recorder" ]; then
            exec "$HOME/infynitae/live/record/infynitae-recorder" replay-start
        fi
        ;;
    replay-supervisor|record-supervisor)
        if [ -x "$HOME/.local/bin/infynitae-recorder" ]; then
            exec "$HOME/.local/bin/infynitae-recorder" supervisor
        elif [ -x "$HOME/infynitae/live/record/infynitae-recorder" ]; then
            exec "$HOME/infynitae/live/record/infynitae-recorder" supervisor
        fi
        ;;
    *)
        echo "Unknown action: $ACTION" >&2
        exit 1
        ;;
esac
