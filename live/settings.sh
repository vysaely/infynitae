#!/bin/sh
# infynitae-settings: reads and changes ~/.config/infynitae/config and applies it
# to the toolkits (GTK, Qt) and apps that keep their own files; no daemon runs for it.
set -e
umask 077

CONFIG_FILE="${CONFIG_FILE:-$HOME/.config/infynitae/config}"

# Ensure config directory and file exist (a fresh install has none yet: `set KEY VAL` read it with
# awk and sourced it, and stopped with an error before changing anything)
mkdir -p "$(dirname "$CONFIG_FILE")"
[ -f "$CONFIG_FILE" ] || : > "$CONFIG_FILE"

# A path from the config with ~, $HOME or ${HOME} at its start made absolute, without running the
# value as shell code (it was eval'd before: a $(...) in it would have run)
expand_path() {
    case "$1" in
        "~/"*) printf '%s' "$HOME/${1#\~/}" ;;
        '$HOME/'*) printf '%s' "$HOME/${1#\$HOME/}" ;;
        '${HOME}/'*) printf '%s' "$HOME/${1#\$\{HOME\}/}" ;;
        *) printf '%s' "$1" ;;
    esac
}

# The look shared by everything Infynitae draws (docs/EDGE_RUST.md, stage 7: CORNERS, THEME_SCHEME,
# CARD_OPACITY), as the Rust edge resolves it (infynitae-edge-rs --print-theme): LOOK_CORNERS,
# LOOK_CARD, LOOK_CARD_AA, LOOK_EDGE, LOOK_EDGE_AA, LOOK_TEXT, LOOK_ACCENT... Without the Rust edge,
# today's values from the colours and SQUARE_CORNERS. Its lines are checked, never run as code.
look_load() {
    case "${SQUARE_CORNERS:-true}" in
        1|true|yes|on|TRUE|True) LOOK_CORNERS=0 ;;
        *) LOOK_CORNERS=8 ;;
    esac
    case "${CORNERS:-}" in
        '' | *[!0-9]*) ;;
        *)
            # as a number (a leading 0 is not octal here), at most 28
            LOOK_CORNERS=${CORNERS#"${CORNERS%%[!0]*}"}
            LOOK_CORNERS=${LOOK_CORNERS:-0}
            [ "${#LOOK_CORNERS}" -le 2 ] && [ "$LOOK_CORNERS" -le 28 ] || LOOK_CORNERS=28
            ;;
    esac
    LOOK_CARD="${SURFACE_COLOR:-#1e2030}" LOOK_CARD_AA=f7
    LOOK_EDGE="${BORDER_INACTIVE:-#333b54}" LOOK_EDGE_AA=e6
    LOOK_TEXT="${TEXT_COLOR:-#e6e6e6}" LOOK_ACCENT="${BORDER_ACTIVE:-#7aa2f7}"
    look_edge="$HOME/.local/bin/infynitae-edge-rs"
    [ -x "$look_edge" ] || return 0
    look_out=$("$look_edge" --print-theme 2>/dev/null) || return 0
    while IFS= read -r look_line; do
        case "$look_line" in
            LOOK_[A-Z_]*=\"*\")
                look_name=${look_line%%=*} look_val=${look_line#*=}
                look_val=${look_val#\"} look_val=${look_val%\"}
                case "$look_name$look_val" in
                    *[!A-Za-z0-9_#]*) ;;
                    *) eval "$look_name=\$look_val" ;;
                esac
                ;;
        esac
    done << LOOK_END
$look_out
LOOK_END
}

# The resolved look saved for the helpers that draw themselves (the lock screen): one file of
# LOOK_*="value" lines, rewritten when it changes
look_save() {
    look_file="${XDG_CACHE_HOME:-$HOME/.cache}/infynitae/look"
    mkdir -p "${look_file%/*}"
    printf 'LOOK_CORNERS="%s"\nLOOK_CARD="%s"\nLOOK_EDGE="%s"\nLOOK_TEXT="%s"\nLOOK_ACCENT="%s"\n' \
        "$LOOK_CORNERS" "$LOOK_CARD" "$LOOK_EDGE" "$LOOK_TEXT" "$LOOK_ACCENT" > "$look_file.new" \
        && mv -f "$look_file.new" "$look_file"
}

# Load current config if present
if [ -f "$CONFIG_FILE" ]; then
    . "$CONFIG_FILE"
fi

# Fallback defaults
CURSOR_THEME="${CURSOR_THEME:-${XCURSOR_THEME:-Adwaita}}"
CURSOR_SIZE="${CURSOR_SIZE:-${XCURSOR_SIZE:-24}}"
FONT_NAME="${FONT_NAME:-sans-serif}"
# Terminals' font: TERMINAL_FONT, else FONT_NAME when that is a monospace font (as before
# 2026-11-06, when one font served both), else monospace
if [ -z "${TERMINAL_FONT:-}" ]; then
    case "$FONT_NAME" in
        monospace|Monospace) TERMINAL_FONT="$FONT_NAME" ;;
        sans-serif|sans|Sans|"Sans Serif"|serif|Serif) TERMINAL_FONT="monospace" ;;
        *)
            if command -v fc-list >/dev/null 2>&1 && [ -n "$(fc-list ":family=$FONT_NAME:spacing=100" family 2>/dev/null)" ]; then
                TERMINAL_FONT="$FONT_NAME"
            else
                TERMINAL_FONT="monospace"
            fi
            ;;
    esac
fi
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
PAN_MODE="${PAN_MODE:-free}"
SCALING_FILTER="${SCALING_FILTER:-nearest}"
OUTPUT_SCALE="${OUTPUT_SCALE:-1.0}"
XKB_DEFAULT_LAYOUT="${XKB_DEFAULT_LAYOUT:-us}"
XKB_DEFAULT_OPTIONS="${XKB_DEFAULT_OPTIONS:-}"

# Writes stdin to $1 only when it differs: an unchanged login touches no file, so the apps that
# watch their config (kitty, alacritty, mako) are not reloaded for nothing. PUT_CHANGED says
# whether it changed; it always succeeds (this script runs with set -e).
put_file() {
    PUT_CHANGED=false
    cat > "$1.new.$$" || { rm -f "$1.new.$$"; return 0; }
    if cmp -s "$1.new.$$" "$1"; then
        rm -f "$1.new.$$"
    else
        mv -f "$1.new.$$" "$1" && PUT_CHANGED=true
    fi
    return 0
}

# Writes KEY="VALUE" (a value without quotes in it, e.g. a colour) over the line that sets KEY, or
# at the end; the comment lines under it stay.
config_write() {
    export CW_KEY="$1" CW_VAL="$2"
    awk 'BEGIN { k = ENVIRON["CW_KEY"]; v = ENVIRON["CW_VAL"]; found = 0 }
        substr($0, 1, length(k) + 1) == k "=" { print k "=\"" v "\""; found = 1; next }
        { print }
        END { if (!found) print k "=\"" v "\"" }' "$CONFIG_FILE" > "$CONFIG_FILE.tmp.$$" \
        && mv -f "$CONFIG_FILE.tmp.$$" "$CONFIG_FILE"
}

# The theme colours from an image (infynitae-wallpaper --colors) into the config: the accent alone
# ("accent") or the accent, surface, canvas and borders ("all"). Returns 1 when the image cannot be
# read or the helper is missing; the config is then unchanged.
wallpaper_colors() {
    wc_image="$1" wc_what="${2:-all}"
    wc_bin="$HOME/.local/bin/infynitae-wallpaper"
    [ -x "$wc_bin" ] && [ -n "$wc_image" ] || return 1
    wc_out=$("$wc_bin" --colors "$wc_image" 2>/dev/null) || return 1
    printf '%s\n' "$wc_out" | while IFS='=' read -r wc_key wc_val; do
        wc_val=$(printf '%s' "$wc_val" | tr -d '"')
        case "$wc_val" in \#[0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F]) ;; *) continue ;; esac
        [ "$wc_what" = accent ] && [ "$wc_key" != BORDER_ACTIVE ] && continue
        config_write "$wc_key" "$wc_val"
    done
    return 0
}

# Folders in the theme's colour (FOLDER_COLOR="accent", the default; "theme": the icon theme's
# own). For Papirus, whose folders are drawn in two blues: a small icon theme of the user's,
# Infynitae-<icon theme>, with those folders redrawn in BORDER_ACTIVE (its side 82%, its sign 35%), every
# other icon inherited; made again only when the icon theme or the colour changes. Its name is
# printed; the icon theme as set when there is nothing to do.
folder_icon_theme() {
    case "${FOLDER_COLOR:-accent}" in accent) ;; *) echo "$ICON_THEME"; return 0 ;; esac
    case "$ICON_THEME" in Papirus|Papirus-Dark|Papirus-Light) ;; *) echo "$ICON_THEME"; return 0 ;; esac
    src=""
    for d in "$HOME/.local/share/icons/$ICON_THEME" /usr/share/icons/"$ICON_THEME"; do
        [ -f "$d/index.theme" ] && { src="$d"; break; }
    done
    hex=$(printf '%s' "$BORDER_ACTIVE" | tr -d '#' | tr 'A-F' 'a-f')
    case "$hex" in [0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]) ;; *) src="" ;; esac
    [ -n "$src" ] || { echo "$ICON_THEME"; return 0; }
    name="Infynitae-$ICON_THEME"
    dst="$HOME/.local/share/icons/$name"
    want="$ICON_THEME #$hex 3"
    if [ -f "$dst/.made-for" ] && [ "$(cat "$dst/.made-for")" = "$want" ]; then
        echo "$name"
        return 0
    fi
    # the side of the folder: the colour 18% darker
    r=$(printf '%d' "0x${hex%????}"); g=$(printf '%d' "0x$(printf '%s' "$hex" | cut -c3-4)"); b=$(printf '%d' "0x${hex#????}")
    dark=$(printf '%02x%02x%02x' $((r * 82 / 100)) $((g * 82 / 100)) $((b * 82 / 100)))
    # the sign on a folder (home, downloads): 35% of it, as Papirus's own
    deep=$(printf '%02x%02x%02x' $((r * 35 / 100)) $((g * 35 / 100)) $((b * 35 / 100)))
    tmp="$dst.new.$$"
    rm -rf "$tmp"
    mkdir -p "$tmp"
    find -L "$src" -mindepth 2 -maxdepth 2 -type d -name places 2>/dev/null | while read -r places; do
        size=$(basename "$(dirname "$places")")
        mkdir -p "$tmp/$size/places"
        # every folder drawn in Papirus's blue (a link to a -blue- one, or one of its own), but the
        # ones named blue: they stay blue
        grep -lis '#5294e2' "$places"/*.svg 2>/dev/null | while read -r icon; do
            case "$(basename "$icon")" in *-blue*) continue ;; esac
            sed -e "s/#5294e2/#$hex/gI" -e "s/#4877b1/#$dark/gI" -e "s/#1d344f/#$deep/gI" "$(readlink -f "$icon")" > "$tmp/$size/places/$(basename "$icon")" 2>/dev/null || true
        done
    done
    sed -e "s/^Name=.*/Name=$name/" -e "s/^Inherits=.*/Inherits=$ICON_THEME,hicolor/" "$src/index.theme" > "$tmp/index.theme"
    grep -q '^Inherits=' "$tmp/index.theme" || sed -i "/^Name=/a Inherits=$ICON_THEME,hicolor" "$tmp/index.theme"
    command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t -f "$tmp" >/dev/null 2>&1 || true
    printf '%s\n' "$want" > "$tmp/.made-for"
    rm -rf "$dst"
    mv "$tmp" "$dst"
    echo "$name"
}

apply_settings() {
    # THEME_FROM_WALLPAPER: a new wallpaper (path or file changed) gives the theme its colours
    case "${THEME_FROM_WALLPAPER:-false}" in
        accent|all|true|1|yes|on)
            twf_what=all
            [ "$THEME_FROM_WALLPAPER" = accent ] && twf_what=accent
            twf_wp=$(expand_path "${WALLPAPER:-}")
            if [ -f "$twf_wp" ]; then
                twf_stamp="${XDG_CACHE_HOME:-$HOME/.cache}/infynitae/theme-from-wallpaper"
                twf_now="$twf_what $(stat -c '%s %Y' "$twf_wp" 2>/dev/null) $twf_wp"
                if [ "$(cat "$twf_stamp" 2>/dev/null)" != "$twf_now" ] && wallpaper_colors "$twf_wp" "$twf_what"; then
                    mkdir -p "$(dirname "$twf_stamp")"
                    printf '%s\n' "$twf_now" > "$twf_stamp"
                    . "$CONFIG_FILE"
                    SURFACE_COLOR="${SURFACE_COLOR:-#111317}" BORDER_ACTIVE="${BORDER_ACTIVE:-#7aa2f7}"
                fi
            fi
            ;;
    esac
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
    put_file "$HOME/.icons/default/index.theme" << EOF
[Icon Theme]
Name=Default
Comment=Default Cursor Theme
Inherits=$CURSOR_THEME
EOF

    # 3. GTK 3 and GTK 4: settings.ini, and gtk.css in the system's colours (THEME_SYNC). One set of
    # colour names for both (GTK 3's theme_*, libadwaita's window_* / view_* / accent_*); GTK 3 also
    # gets a few widget rules, since its stock theme draws headerbars, buttons and entries in fixed
    # colours. GTK 4 does not: libadwaita takes its look from the names and the Settings portal.
    look_load
    look_save
    # GTK's widgets a little rounder than square, less than the cards (8 gives 6, as before)
    gtk_radius=$((LOOK_CORNERS * 3 / 4))
    # (after the colours are known: the wallpaper may have just given them)
    ICON_THEME=$(folder_icon_theme)
    for gtk_dir in "$HOME/.config/gtk-3.0" "$HOME/.config/gtk-4.0"; do
        mkdir -p "$gtk_dir"
        put_file "$gtk_dir/settings.ini" << EOF
[Settings]
gtk-cursor-theme-name=$CURSOR_THEME
gtk-cursor-theme-size=$CURSOR_SIZE
gtk-font-name=$FONT_NAME $FONT_SIZE
gtk-icon-theme-name=$ICON_THEME
gtk-application-prefer-dark-theme=$([ "$COLOR_SCHEME" = "prefer-dark" ] && echo 1 || echo 0)
gtk-enable-animations=false
EOF
        gtk_css="$gtk_dir/gtk.css"
        case "${THEME_SYNC:-true}" in
            0|false|no|off|FALSE|False) continue ;;
        esac
        [ ! -f "$gtk_css" ] || grep -q 'managed by infynitae' "$gtk_css" 2>/dev/null || continue
        {
            cat << EOF
/* managed by infynitae — delete this line to keep custom GTK CSS */
@define-color theme_bg_color $SURFACE_COLOR;
@define-color theme_fg_color $TEXT_COLOR;
@define-color theme_base_color mix($SURFACE_COLOR, $CANVAS_COLOR, 0.5);
@define-color theme_text_color $TEXT_COLOR;
@define-color theme_selected_bg_color $BORDER_ACTIVE;
@define-color theme_selected_fg_color $SURFACE_COLOR;
@define-color borders $BORDER_INACTIVE;
@define-color window_bg_color $SURFACE_COLOR;
@define-color window_fg_color $TEXT_COLOR;
@define-color view_bg_color mix($SURFACE_COLOR, $CANVAS_COLOR, 0.5);
@define-color view_fg_color $TEXT_COLOR;
@define-color headerbar_bg_color shade($SURFACE_COLOR, 1.15);
@define-color headerbar_fg_color $TEXT_COLOR;
@define-color headerbar_backdrop_color $SURFACE_COLOR;
@define-color sidebar_bg_color shade($SURFACE_COLOR, 1.1);
@define-color sidebar_fg_color $TEXT_COLOR;
@define-color card_bg_color mix($SURFACE_COLOR, $TEXT_COLOR, 0.05);
@define-color card_fg_color $TEXT_COLOR;
@define-color popover_bg_color shade($SURFACE_COLOR, 1.2);
@define-color popover_fg_color $TEXT_COLOR;
@define-color dialog_bg_color shade($SURFACE_COLOR, 1.15);
@define-color dialog_fg_color $TEXT_COLOR;
@define-color accent_bg_color $BORDER_ACTIVE;
@define-color accent_color $BORDER_ACTIVE;
@define-color accent_fg_color $SURFACE_COLOR;
EOF
            [ "$gtk_dir" = "$HOME/.config/gtk-4.0" ] || cat << EOF
window, dialog, popover, menu, .background { background-color: @theme_bg_color; color: @theme_fg_color; }
headerbar, .titlebar, toolbar, menubar {
    background: @headerbar_bg_color; color: @theme_fg_color; border-color: @borders; box-shadow: none;
}
headerbar:backdrop, .titlebar:backdrop { background: @headerbar_backdrop_color; }
.view, iconview, treeview, textview text, list, scrolledwindow viewport {
    background-color: @theme_base_color; color: @theme_text_color;
}
.sidebar, placessidebar, stacksidebar { background-color: @sidebar_bg_color; border-color: @borders; }
button {
    background: mix(@theme_bg_color, @theme_fg_color, 0.08); color: @theme_fg_color;
    border: 1px solid @borders; border-radius: ${gtk_radius}px; box-shadow: none; text-shadow: none;
}
button:hover { background: mix(@theme_bg_color, @theme_fg_color, 0.14); }
button:active, button:checked { background: mix(@theme_bg_color, @accent_bg_color, 0.35); }
button.flat:not(:hover):not(:checked) { background: transparent; border-color: transparent; }
entry, spinbutton {
    background: @theme_base_color; color: @theme_text_color;
    border: 1px solid @borders; border-radius: ${gtk_radius}px; box-shadow: none;
}
entry:focus, spinbutton:focus { border-color: @accent_bg_color; }
*:selected, selection, .view:selected, treeview:selected, row:selected {
    background-color: @theme_selected_bg_color; color: @theme_selected_fg_color;
}
scrollbar { background: transparent; border: none; }
scrollbar slider { background: mix(@theme_bg_color, @theme_fg_color, 0.3); border: none; }
separator { background: @borders; }
tooltip { background: @popover_bg_color; color: @theme_fg_color; border: 1px solid @borders; }
EOF
        } | put_file "$gtk_css"
    done

    # 3b. Synchronize Mako, Fuzzel, Foot, Kitty & Alacritty theme files (when THEME_SYNC="true")
    case "${THEME_SYNC:-true}" in
        0|false|no|off|FALSE|False) ;;
        *)
    s_hex="${SURFACE_COLOR#\#}"
    c_hex="${CANVAS_COLOR#\#}"
    ba_hex="${BORDER_ACTIVE#\#}"
    bi_hex="${BORDER_INACTIVE#\#}"
    t_hex="${TEXT_COLOR#\#}"
    # one look with the edge and the lock screen (2026-11-06): sans-serif 13 px, the hairline
    # outline, rounded unless SQUARE_CORNERS
    # (the cards' corners and colours: look_load)
    radius=$LOOK_CORNERS
    card_hex="${LOOK_CARD#\#}${LOOK_CARD_AA}" edge_hex="${LOOK_EDGE#\#}${LOOK_EDGE_AA}"
    lt_hex="${LOOK_TEXT#\#}" la_hex="${LOOK_ACCENT#\#}"

    mako_cfg="$HOME/.config/mako/config"
    if [ ! -f "$mako_cfg" ] || grep -q 'managed by infynitae' "$mako_cfg" 2>/dev/null; then
        mkdir -p "$HOME/.config/mako"
        put_file "$mako_cfg" << EOF
# managed by infynitae — notifications: one border, gone after 5 s, kept in the history
font=sans-serif 10
markup=1
format=<b>%s</b>\n<span alpha="68%%" size="small">%b</span>
background-color=#${card_hex}
text-color=#${lt_hex}
border-color=#${edge_hex}
border-size=1
border-radius=${radius}
width=360
height=110
margin=12
padding=12,14
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
        if [ "$PUT_CHANGED" = true ] && command -v makoctl >/dev/null 2>&1 && pgrep -x mako >/dev/null 2>&1; then
            makoctl reload >/dev/null 2>&1 || true
        fi
    fi

    fuzzel_cfg="$HOME/.config/fuzzel/fuzzel.ini"
    if [ ! -f "$fuzzel_cfg" ] || grep -q 'managed by infynitae' "$fuzzel_cfg" 2>/dev/null; then
        mkdir -p "$HOME/.config/fuzzel"
        put_file "$fuzzel_cfg" << EOF
# managed by infynitae — delete this line to keep custom fuzzel settings
[main]
font=sans-serif:size=11
prompt="› "
terminal=$HOME/.local/bin/xdg-terminal-exec
layer=overlay
width=44
horizontal-pad=12
vertical-pad=8
inner-pad=4

[colors]
background=${card_hex}
text=${lt_hex}ff
match=${la_hex}ff
selection=${la_hex}2e
selection-text=${lt_hex}ff
selection-match=${la_hex}ff
border=${edge_hex}

[border]
width=1
radius=${radius}
EOF
    fi

    foot_cfg="$HOME/.config/foot/foot.ini"
    if [ ! -f "$foot_cfg" ] || grep -q 'managed by infynitae' "$foot_cfg" 2>/dev/null; then
        mkdir -p "$HOME/.config/foot"
        put_file "$foot_cfg" << EOF
# managed by infynitae — delete this line to keep custom foot settings
[main]
font=${TERMINAL_FONT}:size=${FONT_SIZE}
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
    put_file "$kitty_theme" << EOF
# managed by infynitae
font_family      $TERMINAL_FONT
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
        put_file "$kitty_cfg" << 'EOF'
# managed by infynitae — delete this line to keep custom kitty settings
include infynitae-theme.conf
EOF
    fi

    alacritty_theme="$HOME/.config/alacritty/infynitae-theme.toml"
    mkdir -p "$HOME/.config/alacritty"
    put_file "$alacritty_theme" << EOF
# managed by infynitae
[window]
padding = { x = 8, y = 8 }
dynamic_padding = true

[font]
size = $(case "$FONT_SIZE" in *.*) echo "$FONT_SIZE" ;; *) echo "$FONT_SIZE.0" ;; esac)
normal = { family = "$TERMINAL_FONT", style = "Regular" }

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
        put_file "$alacritty_cfg" << 'EOF'
# managed by infynitae — delete this line to keep custom alacritty settings
general.import = ["~/.config/alacritty/infynitae-theme.toml"]
EOF
    # your own alacritty.toml: the import goes first, unless the file has a [general] table or a
    # general.* key of its own (a second definition of the table makes TOML reject the whole file)
    elif ! grep -q 'infynitae-theme\.toml' "$alacritty_cfg" 2>/dev/null \
        && ! grep -qE '^[[:space:]]*(\[general\]|general\.|import[[:space:]]*=)' "$alacritty_cfg"; then
        # Prepend import while preserving the user's existing alacritty.toml rules
        {
            echo 'general.import = ["~/.config/alacritty/infynitae-theme.toml"]'
            cat "$alacritty_cfg"
        } > "$alacritty_cfg.tmp.$$" && mv -f "$alacritty_cfg.tmp.$$" "$alacritty_cfg"
    fi
        ;;
    esac

    # 3c. Firefox in the system's colours (owner, 2026-11-06: its tab strip and toolbar kept
    # Firefox's own greys). A style sheet in each profile (chrome/infynitae.css, imported by
    # userChrome.css) sets Firefox's colour variables from the palette; user.js turns on reading
    # it. Firefox reads both at its start. A userChrome.css of the user's own gets the import as its
    # first line (kept otherwise); delete the "managed by infynitae" line of infynitae.css to stop.
    case "${THEME_SYNC:-true}" in
        0|false|no|off|FALSE|False) ;;
        *)
    for ff_root in "$HOME/.config/mozilla/firefox" "$HOME/.mozilla/firefox"; do
        [ -f "$ff_root/profiles.ini" ] || continue
        sed -n 's/^Path=//p' "$ff_root/profiles.ini" | while IFS= read -r ff_path; do
            case "$ff_path" in /*) ff_dir="$ff_path" ;; *) ff_dir="$ff_root/$ff_path" ;; esac
            [ -d "$ff_dir" ] || continue
            mkdir -p "$ff_dir/chrome"
            ff_css="$ff_dir/chrome/infynitae.css"
            if [ ! -f "$ff_css" ] || grep -q 'managed by infynitae' "$ff_css" 2>/dev/null; then
                put_file "$ff_css" << EOF
/* managed by infynitae — delete this line to keep your own colours here */
:root {
  --toolbox-bgcolor: color-mix(in srgb, $SURFACE_COLOR 88%, $CANVAS_COLOR) !important;
  --toolbox-non-lwt-bgcolor: color-mix(in srgb, $SURFACE_COLOR 88%, $CANVAS_COLOR) !important;
  --toolbox-textcolor: $TEXT_COLOR !important;
  --toolbar-bgcolor: color-mix(in srgb, $SURFACE_COLOR 92%, $TEXT_COLOR) !important;
  --toolbar-color: $TEXT_COLOR !important;
  --tab-selected-bgcolor: color-mix(in srgb, $SURFACE_COLOR 85%, $TEXT_COLOR) !important;
  --tab-selected-textcolor: $TEXT_COLOR !important;
  --toolbar-field-background-color: color-mix(in srgb, $SURFACE_COLOR 70%, $CANVAS_COLOR) !important;
  --toolbar-field-focus-background-color: color-mix(in srgb, $SURFACE_COLOR 70%, $CANVAS_COLOR) !important;
  --toolbar-field-color: $TEXT_COLOR !important;
  --toolbar-field-focus-color: $TEXT_COLOR !important;
  --toolbar-field-focus-border-color: $BORDER_ACTIVE !important;
  --focus-outline-color: $BORDER_ACTIVE !important;
  --arrowpanel-background: color-mix(in srgb, $SURFACE_COLOR 95%, $TEXT_COLOR) !important;
  --arrowpanel-color: $TEXT_COLOR !important;
  --arrowpanel-border-color: $BORDER_INACTIVE !important;
  --urlbarView-highlight-background: color-mix(in srgb, $BORDER_ACTIVE 35%, $SURFACE_COLOR) !important;
  --sidebar-background-color: $SURFACE_COLOR !important;
  --chrome-content-separator-color: $BORDER_INACTIVE !important;
  accent-color: $BORDER_ACTIVE !important;
}
#TabsToolbar, #titlebar, #navigator-toolbox { background-color: var(--toolbox-bgcolor) !important; }
.tab-background:is([selected], [multiselected]) { background-color: var(--tab-selected-bgcolor) !important; }
.tab-background[selected] { box-shadow: inset 0 -2px 0 $BORDER_ACTIVE !important; }
EOF
            fi
            ff_uc="$ff_dir/chrome/userChrome.css"
            if [ ! -f "$ff_uc" ]; then
                printf '@import "infynitae.css"; /* managed by infynitae */\n' > "$ff_uc"
            elif ! grep -q 'infynitae.css' "$ff_uc"; then
                { printf '@import "infynitae.css"; /* managed by infynitae */\n'; cat "$ff_uc"; } > "$ff_uc.tmp.$$" \
                    && mv -f "$ff_uc.tmp.$$" "$ff_uc"
            fi
            ff_js="$ff_dir/user.js"
            grep -q 'toolkit.legacyUserProfileCustomizations.stylesheets' "$ff_js" 2>/dev/null \
                || printf 'user_pref("toolkit.legacyUserProfileCustomizations.stylesheets", true); // infynitae: chrome/infynitae.css\n' >> "$ff_js"
        done
    done
        ;;
    esac

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

    # 4a. Qt apps in the system's colours (owner, 2026-11-06: "i want all the apps to take the theme
    # from the system"): a qt6ct / qt5ct palette from SURFACE_COLOR, TEXT_COLOR, BORDER_ACTIVE and
    # CANVAS_COLOR, the Fusion style, ICON_THEME and the font. env.sh makes Qt load qt6ct when it is
    # installed. Each qt*ct.conf is written while it is ours (infynitae_managed); qt6ct's own window
    # saving it drops that key, and then it is the user's.
    case "${THEME_SYNC:-true}" in
        0|false|no|off|FALSE|False) ;;
        *)
    # mix A B T: A and B (#rrggbb) blended, T percent of B; printed as #ffrrggbb
    qt_mix() {
        awk -v a="${1#\#}" -v b="${2#\#}" -v t="$3" 'function h(s, i) { return index("0123456789abcdef", tolower(substr(s, i, 1))) - 1 }
            function c(s, i) { return h(s, i) * 16 + h(s, i + 1) }
            BEGIN { printf "#ff"; for (i = 1; i <= 5; i += 2) printf "%02x", int(c(a, i) * (100 - t) / 100 + c(b, i) * t / 100 + 0.5); print "" }'
    }
    q_win=$(qt_mix "$SURFACE_COLOR" "$SURFACE_COLOR" 0)
    q_base=$(qt_mix "$SURFACE_COLOR" "$CANVAS_COLOR" 50)
    q_alt=$(qt_mix "$SURFACE_COLOR" "$TEXT_COLOR" 4)
    q_btn=$(qt_mix "$SURFACE_COLOR" "$TEXT_COLOR" 8)
    q_light=$(qt_mix "$SURFACE_COLOR" "$TEXT_COLOR" 22)
    q_mid_light=$(qt_mix "$SURFACE_COLOR" "$TEXT_COLOR" 14)
    q_mid=$(qt_mix "$SURFACE_COLOR" "$BORDER_INACTIVE" 100)
    q_dark=$(qt_mix "$SURFACE_COLOR" "#000000" 45)
    q_shadow=$(qt_mix "$SURFACE_COLOR" "#000000" 75)
    q_text=$(qt_mix "$TEXT_COLOR" "$TEXT_COLOR" 0)
    q_dim=$(qt_mix "$TEXT_COLOR" "$SURFACE_COLOR" 45)
    q_off=$(qt_mix "$TEXT_COLOR" "$SURFACE_COLOR" 60)
    q_acc=$(qt_mix "$BORDER_ACTIVE" "$BORDER_ACTIVE" 0)
    q_acc_off=$(qt_mix "$BORDER_ACTIVE" "$SURFACE_COLOR" 55)
    q_on_acc=$(qt_mix "$SURFACE_COLOR" "#000000" 20)
    q_link=$(qt_mix "$BORDER_ACTIVE" "$TEXT_COLOR" 25)
    q_visited=$(qt_mix "$BORDER_ACTIVE" "$TEXT_COLOR" 55)
    # QPalette roles in order: WindowText Button Light Midlight Dark Mid Text BrightText ButtonText
    # Base Window Shadow Highlight HighlightedText Link LinkVisited AlternateBase NoRole ToolTipBase
    # ToolTipText PlaceholderText Accent
    qt_row() {  # $1 text, $2 highlight
        echo "$1, $q_btn, $q_light, $q_mid_light, $q_dark, $q_mid, $1, #ffffffff, $1, $q_base, $q_win, $q_shadow, $2, $q_on_acc, $q_link, $q_visited, $q_alt, $q_win, $q_btn, $q_text, $q_dim, $2"
    }
    for qt in qt6ct qt5ct; do
        mkdir -p "$HOME/.config/$qt/colors"
        put_file "$HOME/.config/$qt/colors/infynitae.conf" << EOF
[ColorScheme]
active_colors=$(qt_row "$q_text" "$q_acc")
disabled_colors=$(qt_row "$q_off" "$q_acc_off")
inactive_colors=$(qt_row "$q_text" "$q_acc")
EOF
        qt_conf="$HOME/.config/$qt/$qt.conf"
        if [ ! -f "$qt_conf" ] || grep -q '^infynitae_managed=true' "$qt_conf" 2>/dev/null; then
            put_file "$qt_conf" << EOF
[Appearance]
color_scheme_path=$HOME/.config/$qt/colors/infynitae.conf
custom_palette=true
icon_theme=$ICON_THEME
infynitae_managed=true
standard_dialogs=default
style=Fusion

[Fonts]
fixed="$TERMINAL_FONT,$FONT_SIZE,-1,5,400,0,0,0,0,0,0,0,0,0,0,1"
general="$FONT_NAME,$FONT_SIZE,-1,5,400,0,0,0,0,0,0,0,0,0,0,1"

[Interface]
activate_item_on_single_click=0
dialog_buttons_have_icons=1
menus_have_icons=true
show_shortcuts_in_context_menus=true
underline_shortcut=1
wheel_scroll_lines=3
EOF
        fi
    done
        ;;
    esac

    # 4b. "Open Terminal Here" in file managers: xdg-terminal-exec (Infynitae's, in
    # ~/.local/bin), which runs TERMINAL. Each pointer is set only where there is none of the
    # user's own. Thunar's stock action goes through exo-open, which needs xfce4-settings.
    term_exec="$HOME/.local/bin/xdg-terminal-exec"
    uca="$HOME/.config/Thunar/uca.xml"
    if [ ! -f "$uca" ] && [ -f /etc/xdg/Thunar/uca.xml ]; then
        mkdir -p "$HOME/.config/Thunar"
        cp /etc/xdg/Thunar/uca.xml "$uca"
    fi
    if [ -f "$uca" ] && grep -q 'exo-open --working-directory %f --launch TerminalEmulator' "$uca"; then
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
        gsettings set org.gnome.desktop.interface monospace-font-name "$TERMINAL_FONT $FONT_SIZE" 2>/dev/null || true
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
        echo "==> Settings applied."
        ;;
    wallpaper-colors)
        # infynitae-settings wallpaper-colors [accent|all] [IMAGE]: the theme from the wallpaper
        wc_what=all wc_img=""
        for a in "${2:-}" "${3:-}"; do
            case "$a" in "") ;; accent|all) wc_what="$a" ;; *) wc_img="$a" ;; esac
        done
        [ -n "$wc_img" ] || wc_img=$(expand_path "${WALLPAPER:-}")
        if [ ! -f "$wc_img" ]; then
            echo "No image: set WALLPAPER, or name one (infynitae-settings wallpaper-colors IMAGE)" >&2
            exit 1
        fi
        cp -f "$CONFIG_FILE" "$CONFIG_FILE.bak-colors"
        if ! wallpaper_colors "$wc_img" "$wc_what"; then
            echo "Could not read colours from $wc_img (is infynitae-wallpaper up to date?)" >&2
            exit 1
        fi
        . "$CONFIG_FILE"
        grep -E '^(BORDER_ACTIVE|SURFACE_COLOR|CANVAS_COLOR|BORDER_INACTIVE)=' "$CONFIG_FILE"
        apply_settings
        echo "==> Colours from $wc_img (before: $CONFIG_FILE.bak-colors). Panels follow at once; apps on their next start."
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
    timezone)
        # infynitae-settings timezone [Area/City]: the zone, or menus to pick it in a terminal
        . "$HOME/infynitae/live/install-lib.sh"
        if [ -n "${2:-}" ]; then
            set_timezone "$2"
        elif [ -t 0 ] && command -v whiptail >/dev/null 2>&1 && tz=$(pick_timezone); then
            set_timezone "$tz"
        else
            echo "Time zone: $(current_timezone) ($(date '+%H:%M'))"
            echo "Set it:    infynitae-settings timezone Europe/Kyiv   (timedatectl list-timezones)"
        fi
        ;;
    reload)
        exec "$HOME/infynitae/live/infynitae-reload" all
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
        echo "infynitae-settings"
        echo "Usage: infynitae-settings <command>"
        echo ""
        echo "Commands:"
        echo "  apply                Sync current ~/.config/infynitae/config across all toolkits"
        echo "  wallpaper-colors [accent|all] [IMAGE]"
        echo "                       Take the theme's colours from the wallpaper (or IMAGE)"
        echo "  update               Pull latest changes and recompile release binary"
        echo "  restore              Reset canvas zoom/viewport back to default 100% native scale"
        echo "  set <KEY> <VALUE>    Update setting and sync immediately (e.g. set CURSOR_THEME Bibata-Modern-Classic)"
        echo "  get <KEY>            Print current setting value"
        echo "  list cursors         List all installed cursor themes"
        echo "  list fonts           List recommended monospace/system fonts"
        echo "  edit                 Open ~/.config/infynitae/config in editor"
        echo "  reload               Restart the helpers in place (edge, wallpaper, portal…)"
        ;;
esac
