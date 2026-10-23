#!/bin/sh
# Infynitae Universal Settings & Tooling Engine
# Zero-bloat, daemonless settings synchronization across Wayland, GTK, Qt, and Xwayland.
set -e
umask 077

CONFIG_FILE="$HOME/.config/infynitae/config"

# Ensure config directory exists
mkdir -p "$HOME/.config/infynitae"

# Load current config if present
if [ -f "$CONFIG_FILE" ]; then
    . "$CONFIG_FILE"
fi

# Fallback defaults
CURSOR_THEME="${CURSOR_THEME:-${XCURSOR_THEME:-Adwaita}}"
CURSOR_SIZE="${CURSOR_SIZE:-${XCURSOR_SIZE:-24}}"
FONT_NAME="${FONT_NAME:-monospace}"
FONT_SIZE="${FONT_SIZE:-10}"
ICON_THEME="${ICON_THEME:-Adwaita}"
COLOR_SCHEME="${COLOR_SCHEME:-prefer-dark}"
MOUSE_ACCEL_PROFILE="${MOUSE_ACCEL_PROFILE:-flat}"
MOUSE_ACCEL_SPEED="${MOUSE_ACCEL_SPEED:-0.0}"
KEYBOARD_REPEAT_DELAY="${KEYBOARD_REPEAT_DELAY:-200}"
KEYBOARD_REPEAT_RATE="${KEYBOARD_REPEAT_RATE:-35}"
PAN_MODE="${PAN_MODE:-grab}"
SCALING_FILTER="${SCALING_FILTER:-nearest}"
OUTPUT_SCALE="${OUTPUT_SCALE:-1.0}"
XKB_DEFAULT_LAYOUT="${XKB_DEFAULT_LAYOUT:-us}"
XKB_DEFAULT_OPTIONS="${XKB_DEFAULT_OPTIONS:-}"

apply_settings() {
    # 1. Export standard environment variables
    export XCURSOR_THEME="$CURSOR_THEME"
    export XCURSOR_SIZE="$CURSOR_SIZE"
    export XKB_DEFAULT_LAYOUT="$XKB_DEFAULT_LAYOUT"
    export XKB_DEFAULT_OPTIONS="$XKB_DEFAULT_OPTIONS"
    export MOUSE_ACCEL_PROFILE="$MOUSE_ACCEL_PROFILE"
    export MOUSE_ACCEL_SPEED="$MOUSE_ACCEL_SPEED"
    export KEYBOARD_REPEAT_DELAY="$KEYBOARD_REPEAT_DELAY"
    export KEYBOARD_REPEAT_RATE="$KEYBOARD_REPEAT_RATE"
    export PAN_MODE="$PAN_MODE"
    export SCALING_FILTER="$SCALING_FILTER"
    export OUTPUT_SCALE="$OUTPUT_SCALE"

    # 2. Synchronize Xwayland & Legacy X11 fallback cursor (~/.icons/default/index.theme)
    mkdir -p "$HOME/.icons/default"
    cat << EOF > "$HOME/.icons/default/index.theme"
[Icon Theme]
Name=Default
Comment=Default Cursor Theme
Inherits=$CURSOR_THEME
EOF

    # 3. Synchronize GTK 3 & GTK 4 settings.ini (zero daemons, pure disk file)
    for gtk_dir in "$HOME/.config/gtk-3.0" "$HOME/.config/gtk-4.0"; do
        mkdir -p "$gtk_dir"
        cat << EOF > "$gtk_dir/settings.ini"
[Settings]
gtk-cursor-theme-name=$CURSOR_THEME
gtk-cursor-theme-size=$CURSOR_SIZE
gtk-font-name=$FONT_NAME $FONT_SIZE
gtk-icon-theme-name=$ICON_THEME
gtk-application-prefer-dark-theme=$([ "$COLOR_SCHEME" = "prefer-dark" ] && echo 1 || echo 0)
gtk-enable-animations=false
EOF
    done

    # 4. Synchronize Qt platform settings (~/.config/kdeglobals) without clobbering existing KDE configs
    mkdir -p "$HOME/.config"
    if [ ! -f "$HOME/.config/kdeglobals" ]; then
        cat << EOF > "$HOME/.config/kdeglobals"
[Icons]
Theme=$ICON_THEME

[Mouse]
cursorTheme=$CURSOR_THEME
cursorSize=$CURSOR_SIZE
EOF
    fi

    # 5. Synchronize GSettings / D-Bus if present (optional)
    if command -v gsettings >/dev/null 2>&1; then
        gsettings set org.gnome.desktop.interface cursor-theme "$CURSOR_THEME" 2>/dev/null || true
        gsettings set org.gnome.desktop.interface cursor-size "$CURSOR_SIZE" 2>/dev/null || true
        gsettings set org.gnome.desktop.interface font-name "$FONT_NAME $FONT_SIZE" 2>/dev/null || true
        gsettings set org.gnome.desktop.interface icon-theme "$ICON_THEME" 2>/dev/null || true
        gsettings set org.gnome.desktop.interface color-scheme "$COLOR_SCHEME" 2>/dev/null || true
    fi

    # 6. Broadcast to systemd & D-Bus activation environments
    if command -v dbus-update-activation-environment >/dev/null 2>&1; then
        dbus-update-activation-environment --systemd \
            XCURSOR_THEME \
            XCURSOR_SIZE \
            XKB_DEFAULT_LAYOUT \
            XKB_DEFAULT_OPTIONS \
            MOUSE_ACCEL_PROFILE \
            MOUSE_ACCEL_SPEED \
            KEYBOARD_REPEAT_DELAY \
            KEYBOARD_REPEAT_RATE \
            PAN_MODE 2>/dev/null || true
    fi
}

cmd_set() {
    KEY="$1"
    VAL="$2"
    if [ -z "$KEY" ] || [ -z "$VAL" ]; then
        echo "Usage: infynitae-settings set <KEY> <VALUE>" >&2
        exit 1
    fi

    # Security: restrict KEY strictly to alphanumeric and underscores
    case "$KEY" in
        *[!A-Za-z0-9_]*|"")
            echo "Error: Invalid setting key '$KEY'. Keys may only contain letters, numbers, and underscores." >&2
            exit 1
            ;;
    esac

    # Security: escape delimiter pipes, double quotes and backslashes in VAL
    VAL_CLEAN=$(printf '%s' "$VAL" | sed -e 's/[\\&|]/\\&/g' -e 's/"/\\"/g')

    # Update or append in config file
    if grep -q "^${KEY}=" "$CONFIG_FILE" 2>/dev/null; then
        sed -i "s|^${KEY}=.*|${KEY}=\"${VAL_CLEAN}\"|" "$CONFIG_FILE"
    else
        echo "${KEY}=\"${VAL_CLEAN}\"" >> "$CONFIG_FILE"
    fi

    echo "==> Updated $KEY=\"$VAL\" in $CONFIG_FILE"
    . "$CONFIG_FILE"
    apply_settings
    echo "==> Settings synchronized successfully across all toolkits."
}

cmd_get() {
    KEY="$1"
    if [ -z "$KEY" ]; then
        echo "Usage: infynitae-settings get <KEY>" >&2
        exit 1
    fi

    # Security: restrict KEY strictly to alphanumeric and underscores
    case "$KEY" in
        *[!A-Za-z0-9_]*|"")
            echo "Error: Invalid setting key '$KEY'" >&2
            exit 1
            ;;
    esac

    # Safe retrieval without eval
    grep "^${KEY}=" "$CONFIG_FILE" 2>/dev/null | head -1 | cut -d'=' -f2- | sed -e 's/^"//' -e 's/"$//'
}

cmd_list() {
    WHAT="$1"
    case "$WHAT" in
        cursors|cursor)
            echo "Installed Cursor Themes:"
            find /usr/share/icons "$HOME/.icons" "$HOME/.local/share/icons" \
                -maxdepth 2 -type d -name "cursors" 2>/dev/null \
                | sed 's|/cursors$||' | xargs -r -n1 basename | sort -u \
                | while read -r theme; do
                    if [ "$theme" = "$CURSOR_THEME" ]; then
                        echo "  * $theme (active)"
                    else
                        echo "    $theme"
                    fi
                done
            ;;
        fonts|font)
            echo "Installed Monospace & System Fonts:"
            if command -v fc-list >/dev/null 2>&1; then
                fc-list : family | tr ',' '\n' | sed 's/^[ \t]*//' | sort -u | grep -iE 'mono|code|hack|jetbrains|nerd|dejavu|inter|noto' | head -40
            else
                find /usr/share/fonts -type f \( -name "*.ttf" -o -name "*.otf" \) 2>/dev/null | xargs -r -n1 basename | sed 's/\.[^.]*$//' | sort -u | head -40
            fi
            ;;
        *)
            echo "Usage: infynitae-settings list <cursors|fonts>" >&2
            exit 1
            ;;
    esac
}

case "$1" in
    apply)
        apply_settings
        echo "==> Infynitae universal settings applied cleanly."
        ;;
    set)
        shift
        cmd_set "$@"
        ;;
    get)
        shift
        cmd_get "$@"
        ;;
    list)
        shift
        cmd_list "$@"
        ;;
    edit)
        exec "${EDITOR:-nano}" "$CONFIG_FILE"
        ;;
    reload)
        killall qs 2>/dev/null || true
        echo "==> Quickshell UI reloaded."
        ;;
    restore|reset-zoom)
        python3 "$HOME/infynitae/live/infynitae-ipc.py" restore
        echo "==> Canvas viewport restored to default 100% scale (zoom 1.0)."
        ;;
    update)
        if command -v infynitae-update >/dev/null 2>&1; then
            exec infynitae-update "$@"
        elif [ -x "$HOME/infynitae/update.sh" ]; then
            exec "$HOME/infynitae/update.sh" "$@"
        fi
        ;;
    *)
        echo "Infynitae Universal Settings Engine (Zero-Bloat)"
        echo "Usage: infynitae-settings <command>"
        echo ""
        echo "Commands:"
        echo "  apply                Sync current ~/.config/infynitae/config across all toolkits"
        echo "  update               Pull latest changes and recompile release binary"
        echo "  restore              Reset canvas zoom/viewport back to default 100% native scale"
        echo "  set <KEY> <VALUE>    Update setting and sync immediately (e.g. set CURSOR_THEME Bibata-Modern-Classic)"
        echo "  get <KEY>            Print current setting value"
        echo "  list cursors         List all installed cursor themes"
        echo "  list fonts           List recommended monospace/system fonts"
        echo "  edit                 Open ~/.config/infynitae/config in editor"
        echo "  reload               Hot-reload Quickshell UI"
        ;;
esac
