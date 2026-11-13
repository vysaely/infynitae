#!/bin/sh
# Infynitae's shortcut actions (the compositor runs: launch.sh <action>)
# Dispatches shortcuts: terminal (Super+Q), file-manager (Super+E), apps (Super+W), windows (Super+L)
# The keys follow the owner's rule (2026-10-31): often used on the left hand (Super+Tab, Q, W, E, D,
# A, S, C, V), rare on the right (Super+Shift+L, Shift+O, Shift+P, Shift+I, Alt+P, Shift+BackSpace).
ACTION="$1"

# Initialize decoupled standalone desktop environment
if [ -f "$HOME/infynitae/live/env.sh" ]; then
    . "$HOME/infynitae/live/env.sh"
fi

# env.sh loaded the user's config (live/config.sh) and resolved WAYLAND_DISPLAY, DISPLAY and
# XAUTHORITY; a second copy of both ran here on every Super+key until 2026-11-06

# Allow complete user-script overrides in ~/.config/infynitae/launch.sh if present
USER_SCRIPT="$HOME/.config/infynitae/launch.sh"
if [ -x "$USER_SCRIPT" ]; then
    exec "$USER_SCRIPT" "$ACTION"
fi

# The terminal: TERMINAL from the config, else the first installed (kitty, alacritty, foot…)
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

INFYNITAE_BIN="${INFYNITAE_BIN:-}"
if [ -z "$INFYNITAE_BIN" ]; then
    if command -v infynitae >/dev/null 2>&1; then
        INFYNITAE_BIN="infynitae"
    else
        INFYNITAE_BIN="$HOME/.local/bin/infynitae"
    fi
fi

# The edge (ENABLE_EDGE) serves Super+W, Super+L, Super+A and Super+V when it is on and installed
# (EDGE_BIN; its name before 2026-10-31, infynitae-island, until an update builds the new one)
edge_on() {
    case "${ENABLE_EDGE:-true}" in
        0|false|no|off|FALSE|False) return 1 ;;
    esac
    for EDGE_BIN in "$HOME/.local/bin/infynitae-edge" "$HOME/.local/bin/infynitae-island"; do
        [ -x "$EDGE_BIN" ] && return 0
    done
    return 1
}

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
        # the convertible folded (on) or opened (off). The on-screen keyboard no longer follows
        # it: it opens when a text field is tapped (the compositor asks, `osk` below; 2026-11-07).
        # Opened, a keyboard that is showing goes away.
        [ "$2" = off ] && sh "$0" osk hide
        ;;
    osk)
        # the on-screen keyboard (squeekboard) shown or hidden; the compositor asks when a text
        # field is pressed or goes away, the panel's switch by hand. squeekboard's own showing
        # (for every active field) stays off. The session starts it hidden (session.sh); one
        # that is not running (OSK="off", or it ended) starts here and keeps running.
        command -v squeekboard >/dev/null 2>&1 || {
            [ "$2" = show ] && notify-send -a Infynitae "On-screen keyboard" \
                "It needs squeekboard: sudo pacman -S squeekboard" 2>/dev/null
            exit 0
        }
        osk_call() {
            gdbus call --session --dest sm.puri.OSK0 --object-path /sm/puri/OSK0 \
                --method sm.puri.OSK0.SetVisible "$1" >/dev/null 2>&1
        }
        if [ "$2" = show ]; then
            gsettings set org.gnome.desktop.a11y.applications screen-keyboard-enabled false 2>/dev/null
            if ! pgrep -x squeekboard >/dev/null; then
                squeekboard >/dev/null 2>&1 &
                # on the bus within about two seconds
                for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
                    osk_call true && exit 0
                    sleep 0.1
                done
            fi
            osk_call true
        else
            pgrep -x squeekboard >/dev/null && osk_call false
        fi
        ;;
    game-mode)
        # a fullscreen game started ("on <pid>") or ended ("off <pid>"), shown directly or not:
        # Feral's gamemode, when installed, runs the CPU at full speed while it lasts. An X11
        # game's process is xwayland-satellite's, which is enough for the CPU mode
        # "on <pid> quiet": a fullscreen window or one that plays like a game (not a maximised
        # browser): notifications wait too
        read -r gm_mode gm_pid gm_quiet <<GM_ARGS
$2
GM_ARGS
        [ "$gm_mode" = on ] && call=RegisterGame || call=UnregisterGame
        gdbus call --session --dest com.feralinteractive.GameMode \
            --object-path /com/feralinteractive/GameMode \
            --method "com.feralinteractive.GameMode.$call" "$gm_pid" >/dev/null 2>&1
        # GAME_QUIET: a notification drawn over the game ends its direct path to the screen
        # (and tearing) until it goes, so none show until the game ends (they are still listed).
        # The edge holds them while the game-quiet file is there; mako's do-not-disturb does
        # for mako. One the owner turned on stays on (only the one set here is removed)
        if [ "${GAME_QUIET:-true}" = true ]; then
            gm_dir="${XDG_RUNTIME_DIR:-/tmp}/infynitae"
            gm_mark="$gm_dir/game-quiet"
            gm_mako="$gm_dir/game-quiet-mako"
            if [ "$gm_mode" = on ] && [ "$gm_quiet" = quiet ]; then
                mkdir -p "$gm_dir" && : > "$gm_mark"
                if command -v makoctl >/dev/null 2>&1 && pidof mako >/dev/null 2>&1 \
                    && ! makoctl mode 2>/dev/null | grep -qx do-not-disturb; then
                    makoctl mode -a do-not-disturb >/dev/null 2>&1 && : > "$gm_mako"
                fi
            elif [ "$gm_mode" = off ]; then
                rm -f "$gm_mark"
                if [ -e "$gm_mako" ]; then
                    makoctl mode -r do-not-disturb >/dev/null 2>&1
                    rm -f "$gm_mako"
                fi
            fi
        fi
        ;;
    system-monitor)
        # Super+Shift+Escape: a window of its own if one is installed, else in the terminal
        SM="${SYSTEM_MONITOR:-auto}"
        case "$SM" in
            auto|"")
                for m in missioncenter gnome-system-monitor plasma-systemmonitor; do
                    command -v "$m" >/dev/null 2>&1 && exec "$m"
                done
                for m in btop htop top; do
                    command -v "$m" >/dev/null 2>&1 && exec $TERM_CMD -e "$m"
                done
                ;;
            *)
                exec sh -c "$SM"
                ;;
        esac
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
                exec sh -c "$FM"
                ;;
        esac
        ;;
    windows)
        # Super+L: the open windows in the edge; without the edge the key does nothing
        edge_on && exec "$EDGE_BIN" toggle
        ;;
    apps|launcher|edge|island)
        # Super+W: the apps in the edge, else a launcher; LAUNCHER: any command, which is also
        # how another shell (Quickshell, eww, AGS…) takes the key. A compositor from before
        # 2026-11-01 sends "launcher" for Super+W, maybe with an edge that does not know "apps".
        L="${LAUNCHER:-auto}"
        case "$L" in
            auto|""|edge|island)
                if edge_on; then
                    [ "$ACTION" = apps ] && exec "$EDGE_BIN" apps
                    grep -aq 'Search apps' "$EDGE_BIN" && exec "$EDGE_BIN" apps
                    exec "$EDGE_BIN" toggle
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
                exec sh -c "$L"
                ;;
        esac
        ;;
    overview|radar)
        # Super+Shift+Tab: the overview, a menu of the windows to go to
        if command -v "$INFYNITAE_BIN" >/dev/null 2>&1; then
            exec "$INFYNITAE_BIN" msg overview
        elif command -v fuzzel >/dev/null 2>&1; then
            exec fuzzel
        fi
        ;;
    reload|reload-shell|restart-shell)
        # Super+Shift+BackSpace: Live reload settings, shell UI, and desktop environment
        # 1. Re-apply settings and reload all configured helpers (infynitae-reload; windows stay open)
        "$HOME/infynitae/live/infynitae-reload" all >/dev/null 2>&1 || true

        # 2. Reload third-party bars/daemons if running
        if pidof waybar >/dev/null 2>&1; then
            killall -SIGUSR2 waybar 2>/dev/null || true
        fi
        if pidof mako >/dev/null 2>&1; then
            makoctl reload 2>/dev/null || true
        fi

        # 5. Notify user if notification service is present
        if command -v notify-send >/dev/null 2>&1; then
            notify-send -a Infynitae -u low "Settings reloaded" 2>/dev/null || true
        fi
        ;;
    screenshot)
        # Super+Shift+S / PrintScreen: Screenshot dispatcher (the whole screen)
        SHOT_DIR="${SCREENSHOT_DIR:-$HOME/Pictures/Screenshots}"
        mkdir -p "$SHOT_DIR"
        SHOT_FILE="$SHOT_DIR/shot-$(date +%Y%m%d-%H%M%S).png"
        SHOT_BIN="$HOME/.local/bin/infynitae-shot"
        if [ -n "$SCREENSHOT_CMD" ]; then
            exec sh -c "$SCREENSHOT_CMD \"\$1\"" -- "$SHOT_FILE"
        elif [ -x "$SHOT_BIN" ]; then
            if "$SHOT_BIN" screen "$SHOT_FILE" >/dev/null && [ -f "$SHOT_FILE" ]; then
                if command -v notify-send >/dev/null 2>&1; then
                    notify-send -a Infynitae "Screenshot saved" "$SHOT_FILE"
                fi
            fi
        elif command -v grim >/dev/null 2>&1; then
            grim "$SHOT_FILE"
            if [ -f "$SHOT_FILE" ] && command -v wl-copy >/dev/null 2>&1; then
                wl-copy < "$SHOT_FILE"
            fi
            if [ -f "$SHOT_FILE" ] && command -v notify-send >/dev/null 2>&1; then
                notify-send -a Infynitae "Screenshot saved" "$SHOT_FILE"
            fi
        elif command -v spectacle >/dev/null 2>&1; then
            spectacle -b -n -o "$SHOT_FILE" 2>/dev/null || true
            if [ -f "$SHOT_FILE" ] && command -v wl-copy >/dev/null 2>&1; then
                wl-copy < "$SHOT_FILE"
            fi
        fi
        ;;
    force-close-x11)
        # Super+Shift+Delete on an X11 window: its own process, found from the X11 window (all X11
        # windows share xwayland-satellite's connection)
        "$HOME/.local/bin/infynitae-xkill" "$2" "$3"
        ;;
    region-screenshot)
        # Super+S (Super+Shift+R too): Interactive region screenshot of the moment the key was
        # pressed
        SHOT_DIR="${SCREENSHOT_DIR:-$HOME/Pictures/Screenshots}"
        mkdir -p "$SHOT_DIR"
        SHOT_FILE="$SHOT_DIR/region-$(date +%Y%m%d-%H%M%S).png"
        SHOT_BIN="$HOME/.local/bin/infynitae-shot"
        if [ -x "$SHOT_BIN" ]; then
            LIVE_FLAG=""
            if [ "${FREEZE_SCREENSHOT:-true}" != true ]; then
                LIVE_FLAG="--live"
            fi
            if "$SHOT_BIN" region $LIVE_FLAG "$SHOT_FILE" >/dev/null && [ -f "$SHOT_FILE" ]; then
                if command -v notify-send >/dev/null 2>&1; then
                    notify-send -a Infynitae "Screenshot saved" "$SHOT_FILE"
                fi
            fi
        elif command -v slurp >/dev/null 2>&1 && command -v grim >/dev/null 2>&1; then
            # the moment of the key press (FREEZE_SCREENSHOT): each screen is captured first and
            # shown frozen above every window while the region is chosen, then cut from it
            FREEZE_DIR=""
            FREEZERS=""
            FREEZER="$HOME/.local/bin/infynitae-wallpaper"
            if [ "${FREEZE_SCREENSHOT:-true}" = true ] && [ -x "$FREEZER" ]; then
                FREEZE_DIR=$(mktemp -d "${XDG_RUNTIME_DIR:-/tmp}/infynitae-freeze.XXXXXX")
                # the output names are the only "name" keys of the snapshot
                for out in $("$INFYNITAE_BIN" msg snapshot 2>/dev/null \
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
                    notify-send -a Infynitae "Screenshot saved" "$SHOT_FILE"
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
        b="${BROWSER:-auto}"
        [ "$b" = "auto" ] && b="xdg-open"
        if [ "$b" = "xdg-open" ] || [ "$b" = "gio open" ]; then
            exec gtk-launch "$(xdg-settings get default-web-browser 2>/dev/null || echo firefox.desktop)"
        fi
        exec sh -c "$b"
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
                    [ "$MAX" -gt 0 ] 2>/dev/null || continue
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
                    [ "$MAX" -gt 0 ] 2>/dev/null || continue
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
        # Super+Shift+L: Lock screen (native infynitae-lock, hyprlock, swaylock, gtklock, waylock)
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
                exec sh -c "$LOCKER"
                ;;
        esac
        ;;
    control-menu)
        # Super+A: the control flyout (or CONTROL_MENU if set)
        case "${CONTROL_MENU:-auto}" in
            auto|"")
                if edge_on; then
                    exec "$EDGE_BIN" control
                fi
                for m in "$HOME/.local/bin/infynitae-control-menu" "$HOME/infynitae/live/control-menu"; do
                    if [ -x "$m" ]; then exec "$m"; fi
                done
                ;;
            *) exec sh -c "$CONTROL_MENU" ;;
        esac
        ;;
    clipboard-history)
        # Super+V: Clipboard history picker (native infynitae-edge clipboard mode, or CLIPBOARD_PICKER / MENU_PICKER / fuzzel)
        if command -v cliphist >/dev/null 2>&1; then
            if edge_on && [ -z "${CLIPBOARD_PICKER:-}" ] && [ -z "${MENU_PICKER:-}" ]; then
                exec "$EDGE_BIN" clipboard
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
            "$INFYNITAE_BIN" msg shortcut mute 2>/dev/null || true
        fi
        ;;
    deafen|toggle-deafen)
        # Trigger Discord / App Global Shortcuts portal (in-app deafen only, zero OS hardware muting)
        if [ -S "$XDG_RUNTIME_DIR/infynitae/shortcuts.sock" ]; then
            "$INFYNITAE_BIN" msg shortcut deafen 2>/dev/null || true
        fi
        ;;
    config-check)
        # the config changed while running: what it has that was skipped (unknown, unreadable)
        # keeps its default, and a notification names it
        exec "$HOME/infynitae/live/config-check" --notify >/dev/null 2>&1
        ;;
    remember-window)
        # a per-window toggle kept for the window's app (the compositor, Super+Shift+P / Super+Shift+K):
        # remember-window KEY APP on|off adds the app to the comma list KEY in the config, or takes it out
        key="${2:-}" app="${3:-}" state="${4:-}"
        case "$key" in PRIVACY_APPS|KEEP_ON_TOP) ;; *) exit 1 ;; esac
        [ -n "$app" ] || exit 1
        cfg="$HOME/.config/infynitae/config"
        [ -f "$cfg" ] || exit 0
        tmp=$(mktemp "$cfg.XXXXXX") || exit 1
        RW_KEY="$key" RW_APP="$app" RW_ON="$state" awk '
            function trim(s) { gsub(/^[ \t]+|[ \t]+$/, "", s); return s }
            BEGIN { k = ENVIRON["RW_KEY"]; app = tolower(ENVIRON["RW_APP"]); on = ENVIRON["RW_ON"] == "on"; done = 0 }
            !done && substr($0, 1, length(k) + 2) == k "=\"" {
                v = substr($0, length(k) + 3); sub(/".*$/, "", v)
                # PRIVACY_APPS takes commas or spaces; KEEP_ON_TOP holds titles with spaces: commas only
                n = split(v, parts, k == "PRIVACY_APPS" ? "[, \t]+" : ","); out = ""; have = 0
                for (i = 1; i <= n; i++) {
                    p = trim(parts[i]); if (p == "") continue
                    if (tolower(p) == app) { have = 1; if (!on) continue }
                    out = out (out == "" ? "" : ", ") p
                }
                if (on && !have) out = out (out == "" ? "" : ", ") app
                print k "=\"" out "\""; done = 1; next
            }
            { print }
            END { if (!done && on) print k "=\"" app "\"" }' "$cfg" > "$tmp" && mv -f "$tmp" "$cfg" || rm -f "$tmp"
        ;;
    notify-privacy-on)
        # OSD Toast: Privacy Mode Enabled
        if command -v notify-send >/dev/null 2>&1; then
            notify-send -a "Infynitae" -i security-high -h string:x-canonical-private-synchronous:privacy -t 1500 "This window is hidden from captures" 2>/dev/null || true
        fi
        ;;
    notify-privacy-off)
        # OSD Toast: Privacy Mode Disabled
        if command -v notify-send >/dev/null 2>&1; then
            notify-send -a "Infynitae" -i security-low -h string:x-canonical-private-synchronous:privacy -t 1500 "This window shows in captures again" 2>/dev/null || true
        fi
        ;;
    notify-capture-hidden)
        # the capture dot steps aside over a fullscreen window: a capture is still never silent
        if command -v notify-send >/dev/null 2>&1; then
            notify-send -a "Infynitae" -i camera-web -h string:x-canonical-private-synchronous:capture -t 4000 "Screen is being captured" "The red dot is hidden while a window is fullscreen" 2>/dev/null || true
        fi
        ;;
    notify-tearing-on)
        # Super+Shift+O (Super+Shift+T too): the focused window tears while fullscreen
        if command -v notify-send >/dev/null 2>&1; then
            notify-send -a "Infynitae" -i video-display -h string:x-canonical-private-synchronous:tearing -t 1500 "Tearing is on for this window" "Fullscreen frames show at once: lowest latency" 2>/dev/null || true
        fi
        ;;
    notify-tearing-off)
        if command -v notify-send >/dev/null 2>&1; then
            notify-send -a "Infynitae" -i video-display -h string:x-canonical-private-synchronous:tearing -t 1500 "Tearing is off for this window" "Frames wait for the screen's refresh" 2>/dev/null || true
        fi
        ;;
    notify-idle-inhibit-on)
        # OSD Toast: Idle Sleep Inhibited
        if command -v notify-send >/dev/null 2>&1; then
            notify-send -a "Infynitae" -i display -h string:x-canonical-private-synchronous:idle -t 1500 "Stay awake is on" "The screens stay on until you turn it off" 2>/dev/null || true
        fi
        ;;
    notify-idle-inhibit-off)
        # OSD Toast: Idle Sleep Normal
        if command -v notify-send >/dev/null 2>&1; then
            notify-send -a "Infynitae" -i display -h string:x-canonical-private-synchronous:idle -t 1500 "Stay awake is off" "The screens turn off after the usual time" 2>/dev/null || true
        fi
        ;;
    toggle-idle-inhibit|inhibit-idle)
        # Send keypress or signal to compositor to toggle idle inhibit
        if [ -S "$XDG_RUNTIME_DIR/infynitae/shortcuts.sock" ]; then
            "$INFYNITAE_BIN" msg shortcut idle_inhibit 2>/dev/null || true
        fi
        ;;
    toggle-privacy|privacy)
        # Send keypress or signal to compositor to toggle privacy
        if [ -S "$XDG_RUNTIME_DIR/infynitae/shortcuts.sock" ]; then
            "$INFYNITAE_BIN" msg shortcut privacy 2>/dev/null || true
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
        # Super+Alt+P (Super+Alt+R too): Toggle continuous screen recording
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
