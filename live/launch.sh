#!/bin/sh
# Universal action launcher for Infynitae
# Dispatches shortcuts: terminal (Super+Q), file-manager (Super+E), launcher (Super+W)
ACTION="$1"

# Initialize decoupled standalone desktop environment
if [ -f "$HOME/infynitae/live/env.sh" ]; then
    . "$HOME/infynitae/live/env.sh"
fi

# Load user configuration if present
if [ -f "$HOME/infynitae/live/config.sh" ]; then
    . "$HOME/infynitae/live/config.sh"
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
        # Super+Q: Terminal emulator (auto-detects kitty, alacritty, foot, or uses $TERMINAL);
        # with a command and its arguments (xdg-terminal-exec): that command in it
        if [ -n "$TERM_CMD" ] && [ $# -gt 1 ]; then
            shift
            exec $TERM_CMD -e "$@"
        elif [ -n "$TERM_CMD" ]; then
            exec $TERM_CMD
        else
            echo "[infynitae] Error: No terminal emulator found. Please install kitty, alacritty, or foot." >&2
        fi
        ;;
    tablet-mode)
        # the convertible folded (on) or opened (off): with OSK="auto" the on-screen keyboard
        # runs in tablet mode only; opened, nothing stands between the keyboard and the apps
        case "${OSK:-auto}" in
            auto|"")
                if [ "$2" = on ]; then
                    command -v squeekboard >/dev/null 2>&1 || exit 0
                    gsettings set org.gnome.desktop.a11y.applications screen-keyboard-enabled true 2>/dev/null
                    pgrep -x squeekboard >/dev/null || squeekboard >/dev/null 2>&1 &
                else
                    pkill -x squeekboard 2>/dev/null
                fi
                ;;
        esac
        ;;
    game-mode)
        # a fullscreen game shown directly started ("on <pid>") or ended ("off <pid>"): Feral's
        # gamemode, when installed, runs the CPU at full speed while it lasts. An X11 game's
        # process is xwayland-satellite's, which is enough for the CPU mode
        set -- $2
        [ "$1" = on ] && call=RegisterGame || call=UnregisterGame
        gdbus call --session --dest com.feralinteractive.GameMode \
            --object-path /com/feralinteractive/GameMode \
            --method "com.feralinteractive.GameMode.$call" "$2" >/dev/null 2>&1
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
        # Super+W: Application launcher / Dynamic Island
        L="${LAUNCHER:-auto}"
        case "$L" in
            auto|""|island)
                if [ "${ENABLE_ISLAND:-true}" != "false" ] && [ "${ENABLE_ISLAND:-true}" != "0" ] && [ -x "$HOME/.local/bin/infynitae-island" ]; then
                    exec "$HOME/.local/bin/infynitae-island" toggle
                elif [ "${ENABLE_QUICKSHELL:-false}" = "true" ] && command -v qs >/dev/null 2>&1 && [ -f "$HOME/infynitae/live/quickshell/shell.qml" ]; then
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
        elif command -v infynitae >/dev/null 2>&1; then
            exec infynitae msg overview
        elif command -v fuzzel >/dev/null 2>&1; then
            exec fuzzel
        fi
        ;;
    reload|reload-shell|restart-shell)
        # Super+Shift+BackSpace: Live reload settings, shell UI, and desktop environment
        # 1. Re-apply settings and reload all configured helpers (infynitae-reload; windows stay open)
        "$HOME/infynitae/live/infynitae-reload" all >/dev/null 2>&1 || true

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
    force-close-x11)
        # Super+Shift+C on an X11 window: its own process, found from the X11 window (all X11
        # windows share xwayland-satellite's connection)
        "$HOME/.local/bin/infynitae-xkill" "$2" "$3"
        ;;
    region-screenshot)
        # Super+Shift+R: Interactive region screenshot (slurp + grim) of the moment the key was
        # pressed
        SHOT_DIR="${SCREENSHOT_DIR:-$HOME/Pictures/Screenshots}"
        mkdir -p "$SHOT_DIR"
        SHOT_FILE="$SHOT_DIR/region-$(date +%Y%m%d-%H%M%S).png"
        if command -v slurp >/dev/null 2>&1 && command -v grim >/dev/null 2>&1; then
            # the moment of the key press (FREEZE_SCREENSHOT): each screen is captured first and
            # shown frozen above every window while the region is chosen, then cut from it
            FREEZE_DIR=""
            FREEZERS=""
            FREEZER="$HOME/.local/bin/infynitae-wallpaper"
            if [ "${FREEZE_SCREENSHOT:-true}" = true ] && [ -x "$FREEZER" ]; then
                FREEZE_DIR=$(mktemp -d "${XDG_RUNTIME_DIR:-/tmp}/infynitae-freeze.XXXXXX")
                # the output names are the only "name" keys of the snapshot
                for out in $("${INFYNITAE_BIN:-$HOME/.local/bin/infynitae}" msg snapshot 2>/dev/null \
                    | grep -o '"name":"[^"]*"' | cut -d'"' -f4); do
                    grim -o "$out" "$FREEZE_DIR/$out.png" 2>/dev/null || continue
                    "$FREEZER" --overlay --output "$out" "$FREEZE_DIR/$out.png" stretch &
                    FREEZERS="$FREEZERS $!"
                done
                # the frozen pictures on screen before slurp opens above them
                [ -n "$FREEZERS" ] && sleep 0.2
            fi
            unfreeze() {
                [ -n "$FREEZERS" ] && kill $FREEZERS 2>/dev/null
                FREEZERS=""
                [ -n "$FREEZE_DIR" ] && rm -rf "$FREEZE_DIR"
            }
            trap unfreeze EXIT INT TERM
            GEOM=$(slurp)
            if [ -n "$GEOM" ]; then
                # what is on screen now is the frozen moment: the region is cut from it
                grim -g "$GEOM" "$SHOT_FILE"
                unfreeze
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
        if [ "$BROWSER" = "xdg-open" ] || [ "$BROWSER" = "gio open" ]; then
            exec gtk-launch "$(xdg-settings get default-web-browser 2>/dev/null || echo firefox.desktop)"
        fi
        exec sh -c "$BROWSER"
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
    control-menu)
        # Super+A: the control flyout (or CONTROL_MENU if set)
        case "${CONTROL_MENU:-auto}" in
            auto|"")
                if [ "${ENABLE_ISLAND:-true}" != "false" ] && [ "${ENABLE_ISLAND:-true}" != "0" ] && [ -x "$HOME/.local/bin/infynitae-island" ]; then
                    exec "$HOME/.local/bin/infynitae-island" control
                fi
                for m in "$HOME/.local/bin/infynitae-control-menu" "$HOME/infynitae/live/control-menu"; do
                    if [ -x "$m" ]; then exec "$m"; fi
                done
                ;;
            *) exec sh -c "$CONTROL_MENU" ;;
        esac
        ;;
    clipboard-history)
        # Super+V: Clipboard history picker (native infynitae-island clipboard mode, or CLIPBOARD_PICKER / MENU_PICKER / fuzzel)
        if command -v cliphist >/dev/null 2>&1; then
            if [ "${ENABLE_ISLAND:-true}" != "false" ] && [ "${ENABLE_ISLAND:-true}" != "0" ] && [ -z "${CLIPBOARD_PICKER:-}" ] && [ -z "${MENU_PICKER:-}" ] && [ -x "$HOME/.local/bin/infynitae-island" ]; then
                exec "$HOME/.local/bin/infynitae-island" clipboard
            fi
            PICKER="${CLIPBOARD_PICKER:-${MENU_PICKER:-fuzzel --dmenu}}"
            # Escape in the picker selects nothing: the clipboard is left as it was
            PICK=$(cliphist list | $PICKER)
            [ -n "$PICK" ] && printf '%s\n' "$PICK" | cliphist decode | wl-copy
        elif command -v notify-send >/dev/null 2>&1; then
            notify-send "Clipboard history needs cliphist: sudo pacman -S cliphist"
            exit 0
        fi
        ;;
    mute|toggle-mute)
        # Trigger Discord / App Global Shortcuts portal (in-app mute only, zero OS hardware muting)
        if [ -S "$XDG_RUNTIME_DIR/infynitae/shortcuts.sock" ]; then
            infynitae msg shortcut mute 2>/dev/null || true
        fi
        ;;
    deafen|toggle-deafen)
        # Trigger Discord / App Global Shortcuts portal (in-app deafen only, zero OS hardware muting)
        if [ -S "$XDG_RUNTIME_DIR/infynitae/shortcuts.sock" ]; then
            infynitae msg shortcut deafen 2>/dev/null || true
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
    notify-capture-hidden)
        # the capture dot steps aside over a fullscreen window: a capture is still never silent
        if command -v notify-send >/dev/null 2>&1; then
            notify-send -a "Infynitae" -i camera-web -h string:x-canonical-private-synchronous:capture -t 4000 "Screen is being captured" "The red dot is hidden while a window is fullscreen" 2>/dev/null || true
        fi
        ;;
    notify-tearing-on)
        # Super+Shift+T: the focused window tears while fullscreen
        if command -v notify-send >/dev/null 2>&1; then
            notify-send -a "Infynitae" -i video-display -h string:x-canonical-private-synchronous:tearing -t 1500 "Tearing: ON" "Fullscreen frames show at once: lowest latency" 2>/dev/null || true
        fi
        ;;
    notify-tearing-off)
        if command -v notify-send >/dev/null 2>&1; then
            notify-send -a "Infynitae" -i video-display -h string:x-canonical-private-synchronous:tearing -t 1500 "Tearing: OFF" "Frames wait for the screen's refresh" 2>/dev/null || true
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
            infynitae msg shortcut idle_inhibit 2>/dev/null || true
        fi
        ;;
    toggle-privacy|privacy)
        # Send keypress or signal to compositor to toggle privacy
        if [ -S "$XDG_RUNTIME_DIR/infynitae/shortcuts.sock" ]; then
            infynitae msg shortcut privacy 2>/dev/null || true
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
