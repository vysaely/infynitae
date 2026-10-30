#!/bin/sh
# Infynitae Universal Settings & Tooling Engine
# Zero-bloat, daemonless settings synchronization across Wayland, GTK, Qt, and Xwayland.
set -e
umask 077

CONFIG_FILE="${CONFIG_FILE:-$HOME/.config/infynitae/config}"

# Ensure config directory exists
mkdir -p "$(dirname "$CONFIG_FILE")"

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
CANVAS_COLOR="${CANVAS_COLOR:-#0b0f14}"
SURFACE_COLOR="${SURFACE_COLOR:-#111317}"
BORDER_ACTIVE="${BORDER_ACTIVE:-#7aa2f7}"
BORDER_INACTIVE="${BORDER_INACTIVE:-#333b54}"
TEXT_COLOR="${TEXT_COLOR:-#eef2f8}"
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

    # 3. Synchronize GTK 3 & GTK 4 settings.ini + unified gtk.css (zero daemons, pure disk file)
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
        gtk_css="$gtk_dir/gtk.css"
        if [ ! -f "$gtk_css" ] || grep -q 'managed by infynitae' "$gtk_css" 2>/dev/null; then
            cat << EOF > "$gtk_css"
/* managed by infynitae — delete this line to keep custom GTK CSS */
@define-color theme_bg_color $SURFACE_COLOR;
@define-color theme_fg_color $TEXT_COLOR;
@define-color theme_base_color $CANVAS_COLOR;
@define-color theme_text_color $TEXT_COLOR;
@define-color theme_selected_bg_color $BORDER_ACTIVE;
@define-color theme_selected_fg_color $SURFACE_COLOR;
@define-color borders $BORDER_INACTIVE;
window, dialog, popover, menu, .background {
    background-color: $SURFACE_COLOR;
    color: $TEXT_COLOR;
    border-radius: 0px;
}
EOF
        fi
    done

    # 3b. Synchronize Mako, Fuzzel, Foot, Kitty & Alacritty theme files (only when missing or managed)
    s_hex="${SURFACE_COLOR#\#}"
    c_hex="${CANVAS_COLOR#\#}"
    ba_hex="${BORDER_ACTIVE#\#}"
    bi_hex="${BORDER_INACTIVE#\#}"
    t_hex="${TEXT_COLOR#\#}"

    mako_cfg="$HOME/.config/mako/config"
    if [ ! -f "$mako_cfg" ] || grep -q 'managed by infynitae' "$mako_cfg" 2>/dev/null; then
        mkdir -p "$HOME/.config/mako"
        cat << EOF > "$mako_cfg"
# managed by infynitae — minimalist single-border notification toast (5s auto-expire + history)
font=Sans 8.5
markup=1
format=<b>%s</b>\n<span alpha="68%" size="small">%b</span>
background-color=#${s_hex}f7
text-color=#${t_hex}
border-color=#${bi_hex}e6
border-size=1
border-radius=0
width=288
height=84
margin=8
padding=8,11
default-timeout=5000
ignore-timeout=1
max-history=20
max-visible=4
sort=-time
layer=overlay
anchor=top-right

[mode=do-not-disturb]
invisible=1
EOF
        if command -v makoctl >/dev/null 2>&1 && pgrep -x mako >/dev/null 2>&1; then
            makoctl reload >/dev/null 2>&1 || true
        fi
    fi

    fuzzel_cfg="$HOME/.config/fuzzel/fuzzel.ini"
    if [ ! -f "$fuzzel_cfg" ] || grep -q 'managed by infynitae' "$fuzzel_cfg" 2>/dev/null; then
        mkdir -p "$HOME/.config/fuzzel"
        cat << EOF > "$fuzzel_cfg"
# managed by infynitae — delete this line to keep custom fuzzel settings
[main]
font=${FONT_NAME}:size=${FONT_SIZE}
prompt="› "
terminal=$HOME/.local/bin/xdg-terminal-exec
layer=overlay
width=44
horizontal-pad=12
vertical-pad=8
inner-pad=4

[colors]
background=${s_hex}f7
text=${t_hex}ff
match=${ba_hex}ff
selection=262b38ff
selection-text=${t_hex}ff
selection-match=${ba_hex}ff
border=${bi_hex}e6

[border]
width=1
radius=0
EOF
    fi

    foot_cfg="$HOME/.config/foot/foot.ini"
    if [ ! -f "$foot_cfg" ] || grep -q 'managed by infynitae' "$foot_cfg" 2>/dev/null; then
        mkdir -p "$HOME/.config/foot"
        cat << EOF > "$foot_cfg"
# managed by infynitae — delete this line to keep custom foot settings
[main]
font=${FONT_NAME}:size=${FONT_SIZE}
pad=8x8

[colors]
alpha=0.97
background=${s_hex}
foreground=${t_hex}
regular0=151820
regular1=f7768e
regular2=9ece6a
regular3=e0af68
regular4=${ba_hex}
regular5=bb9af7
regular6=7dcfff
regular7=c0caf5
EOF
    fi

    kitty_theme="$HOME/.config/kitty/infynitae-theme.conf"
    mkdir -p "$HOME/.config/kitty"
    cat << EOF > "$kitty_theme"
# managed by infynitae
font_family      $FONT_NAME
font_size        $FONT_SIZE
background       #$s_hex
foreground       #$t_hex
cursor           #$ba_hex
selection_background #262b38
selection_foreground #$t_hex
window_padding_width 6
EOF
    kitty_cfg="$HOME/.config/kitty/kitty.conf"
    if [ ! -f "$kitty_cfg" ] || grep -q 'managed by infynitae' "$kitty_cfg" 2>/dev/null; then
        cat << 'EOF' > "$kitty_cfg"
# managed by infynitae — delete this line to keep custom kitty settings
include infynitae-theme.conf
EOF
    fi

    alacritty_theme="$HOME/.config/alacritty/infynitae-theme.toml"
    mkdir -p "$HOME/.config/alacritty"
    cat << EOF > "$alacritty_theme"
# managed by infynitae
[window]
padding = { x = 8, y = 8 }
dynamic_padding = true

[font]
size = ${FONT_SIZE}.0
normal = { family = "$FONT_NAME", style = "Regular" }

[colors.primary]
background = "#$s_hex"
foreground = "#$t_hex"

[colors.cursor]
text = "#$s_hex"
cursor = "#$ba_hex"

[colors.selection]
text = "#$t_hex"
background = "#262b38"
EOF
    alacritty_cfg="$HOME/.config/alacritty/alacritty.toml"
    if [ ! -f "$alacritty_cfg" ] || grep -q 'managed by infynitae' "$alacritty_cfg" 2>/dev/null; then
        cat << 'EOF' > "$alacritty_cfg"
# managed by infynitae — delete this line to keep custom alacritty settings
general.import = ["~/.config/alacritty/infynitae-theme.toml"]
EOF
    elif ! grep -q 'infynitae-theme\.toml' "$alacritty_cfg" 2>/dev/null; then
        # Prepend import while preserving the user's existing alacritty.toml rules
        {
            echo 'general.import = ["~/.config/alacritty/infynitae-theme.toml"]'
            cat "$alacritty_cfg"
        } > "$alacritty_cfg.tmp.$$" && mv -f "$alacritty_cfg.tmp.$$" "$alacritty_cfg"
    fi

    # 4. Synchronize Qt platform settings (~/.config/kdeglobals) without clobbering existing KDE configs
    mkdir -p "$HOME/.config"
    if [ ! -f "$HOME/.config/kdeglobals" ]; then
        cat << EOF > "$HOME/.config/kdeglobals"
[General]
TerminalApplication=$HOME/.local/bin/xdg-terminal-exec

[Icons]
Theme=$ICON_THEME

[Mouse]
cursorTheme=$CURSOR_THEME
cursorSize=$CURSOR_SIZE
EOF
    fi

    # 4b. "Open Terminal Here" in file managers: xdg-terminal-exec (Infynitae's, in
    # ~/.local/bin), which runs TERMINAL. Each pointer is set only where there is none of the
    # user's own. Thunar's stock action goes through exo-open, which needs xfce4-settings.
    term_exec="$HOME/.local/bin/xdg-terminal-exec"
    uca="$HOME/.config/Thunar/uca.xml"
    if [ ! -f "$uca" ] && [ -f /etc/xdg/Thunar/uca.xml ]; then
        mkdir -p "$HOME/.config/Thunar"
        cp /etc/xdg/Thunar/uca.xml "$uca"
    fi
    if [ -f "$uca" ]; then
        sed -i "s|<command>exo-open --working-directory %f --launch TerminalEmulator</command>|<command>$term_exec --dir=%f</command>|" "$uca"
    fi
    # Dolphin (KDE): its terminal, unless kdeglobals names one
    if [ -f "$HOME/.config/kdeglobals" ] && ! grep -q '^TerminalApplication=' "$HOME/.config/kdeglobals"; then
        printf '\n[General]\nTerminalApplication=%s\n' "$term_exec" >> "$HOME/.config/kdeglobals"
    fi
    # Nemo (Cinnamon): its terminal, while it is the stock one
    if command -v gsettings >/dev/null 2>&1 \
        && gsettings get org.cinnamon.desktop.default-applications.terminal exec 2>/dev/null \
            | grep -qx "'x-terminal-emulator'\|'gnome-terminal'"; then
        gsettings set org.cinnamon.desktop.default-applications.terminal exec "$term_exec" 2>/dev/null || true
    fi

    # 4c. KDE apps (Dolphin's "Open With", file associations) list apps through a menu file only
    # KDE Plasma installs: the standard one that includes every app, unless there is one
    if [ ! -f "$HOME/.config/menus/applications.menu" ] && [ ! -f /etc/xdg/menus/applications.menu ]; then
        mkdir -p "$HOME/.config/menus"
        cat << 'EOF' > "$HOME/.config/menus/applications.menu"
<!DOCTYPE Menu PUBLIC "-//freedesktop//DTD Menu 1.0//EN"
 "http://www.freedesktop.org/standards/menu-spec/1.0/menu.dtd">
<Menu>
  <Name>Applications</Name>
  <DefaultAppDirs/>
  <DefaultDirectoryDirs/>
  <Include><All/></Include>
</Menu>
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

    # Escape embedded single quotes as '\'' for shell-safe single-quoted value
    VAL_ESCAPED=$(printf '%s\n' "$VAL" | sed "s/'/'\\\\''/g")
    VAL_QUOTED="'${VAL_ESCAPED}'"

    # Update or append in config file without sed injection
    export KEY VAL_QUOTED
    awk '
    BEGIN {
        key = ENVIRON["KEY"]
        val = ENVIRON["VAL_QUOTED"]
        found = 0
    }
    substr($0, 1, length(key) + 1) == key "=" {
        print key "=" val
        found = 1
        next
    }
    { print }
    END {
        if (!found) {
            print key "=" val
        }
    }
    ' "$CONFIG_FILE" > "$CONFIG_FILE.tmp.$$" && mv -f "$CONFIG_FILE.tmp.$$" "$CONFIG_FILE"

    echo "==> Updated $KEY in $CONFIG_FILE"
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
    RAW=$(grep "^${KEY}=" "$CONFIG_FILE" 2>/dev/null | head -1 | cut -d'=' -f2-)
    case "$RAW" in
        \'*\')
            RAW="${RAW#\'}"
            RAW="${RAW%\'}"
            printf '%s\n' "$RAW" | sed "s/'\\\\''/'/g"
            ;;
        \"*\")
            RAW="${RAW#\"}"
            RAW="${RAW%\"}"
            printf '%s\n' "$RAW"
            ;;
        *)
            printf '%s\n' "$RAW"
            ;;
    esac
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
        infynitae msg restore
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
