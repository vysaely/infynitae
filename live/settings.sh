#!/bin/sh
# infynitae-settings: reads and changes ~/.config/infynitae/config and applies it
# to the toolkits (GTK, Qt) and apps that keep their own files; no daemon runs for it.
set -e
umask 077

CONFIG_FILE="${CONFIG_FILE:-$HOME/.config/infynitae/config}"
SCRIPT_DIR=$(cd "$(dirname "$0")" 2>/dev/null && pwd || echo "$HOME/infynitae/live")

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

# Reads the config with config.sh's syntax guard and legacy renames when available, falling back
# to sourcing the file directly if config.sh is missing.
load_config() {
    for _lc_sh in "${SCRIPT_DIR:-}/config.sh" "${SCRIPT_DIR:-}/live/config.sh" "${SCRIPT_DIR:-}/../live/config.sh" "$HOME/infynitae/live/config.sh"; do
        if [ -f "$_lc_sh" ]; then
            _infy_renames="${_lc_sh%/*}/config-renames"
            . "$_lc_sh"
            unset _lc_sh _infy_renames
            return 0
        fi
    done
    unset _lc_sh
    if [ -f "$CONFIG_FILE" ]; then
        . "$CONFIG_FILE"
    fi
}

# Fallback defaults for settings read outside config.sh (re-applied after config reloads)
apply_defaults() {
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
    ACCENT="${ACCENT:-${BORDER_ACTIVE:-#7aa2f7}}"
    BORDER_INACTIVE="${BORDER_INACTIVE:-#333b54}"
    TEXT_COLOR="${TEXT_COLOR:-#eef2f8}"
    MOUSE_ACCEL_PROFILE="${MOUSE_ACCEL_PROFILE:-flat}"
    MOUSE_ACCEL_SPEED="${MOUSE_ACCEL_SPEED:-0.0}"
    KEYBOARD_REPEAT_DELAY="${KEYBOARD_REPEAT_DELAY:-200}"
    KEYBOARD_REPEAT_RATE="${KEYBOARD_REPEAT_RATE:-35}"
    PAN_MODE="${PAN_MODE:-free}"
    SCALING_FILTER="${SCALING_FILTER:-sharp}"
    OUTPUT_SCALE="${OUTPUT_SCALE:-1.0}"
    XKB_DEFAULT_LAYOUT="${XKB_DEFAULT_LAYOUT:-us}"
    XKB_DEFAULT_OPTIONS="${XKB_DEFAULT_OPTIONS:-}"
}

# The look shared by everything Infynitae draws (docs/EDGE_RUST.md, stage 7: CORNERS, THEME_SCHEME,
# CARD_OPACITY), as the Rust edge resolves it (infynitae-edge-rs --print-theme): LOOK_CORNERS,
# LOOK_CARD, LOOK_CARD_AA, LOOK_EDGE, LOOK_EDGE_AA, LOOK_TEXT, LOOK_ACCENT... Without the Rust edge,
# today's values from the colours and CORNERS. Its lines are checked, never run as code.
look_load() {
    # CORNERS, else WINDOW_CORNERS (one corner for windows and the interface, owner 2026-11-13),
    # else 8; SQUARE_CORNERS only tells apps to draw square
    LOOK_CORNERS=8
    case "${CORNERS:-${WINDOW_CORNERS:-}}" in
        '' | *[!0-9]*) ;;
        *)
            # as a number (a leading 0 is not octal here), at most 28
            look_c=${CORNERS:-${WINDOW_CORNERS:-}}
            LOOK_CORNERS=${look_c#"${look_c%%[!0]*}"}
            LOOK_CORNERS=${LOOK_CORNERS:-0}
            [ "${#LOOK_CORNERS}" -le 2 ] && [ "$LOOK_CORNERS" -le 28 ] || LOOK_CORNERS=28
            ;;
    esac
    LOOK_SMALL_RADIUS=$(( (LOOK_CORNERS * 55 + 50) / 100 ))
    LOOK_CARD="${SURFACE_COLOR:-#111317}" LOOK_CARD_AA=e6
    LOOK_EDGE="${BORDER_INACTIVE:-#333b54}" LOOK_EDGE_AA=e6
    LOOK_TEXT="${TEXT_COLOR:-#eef2f8}" LOOK_TEXT_AA=ff
    LOOK_ACCENT="${ACCENT:-${BORDER_ACTIVE:-#7aa2f7}}" LOOK_ACCENT_AA=ff
    LOOK_MUTED="" LOOK_MUTED_AA=ff
    LOOK_ON_ACCENT="" LOOK_ON_ACCENT_AA=ff
    LOOK_ACCENT_TEXT="" LOOK_ACCENT_TEXT_AA=ff
    LOOK_ALERT="" LOOK_ALERT_AA=ff
    LOOK_TRACK="" LOOK_TRACK_AA=1f
    LOOK_FONT="${FONT_NAME:-sans-serif}"

    # the edge EDGE_IMPL picked (its link), else the Rust one: the C edge prints no theme
    look_edge="$HOME/.local/bin/$(readlink "$HOME/.local/bin/infynitae-edge" 2>/dev/null || true)"
    case "$look_edge" in */infynitae-edge-rs | */infynitae-edge-apps) ;; *) look_edge="$HOME/.local/bin/infynitae-edge-rs" ;; esac
    [ -x "$look_edge" ] || look_edge="$HOME/.local/bin/infynitae-edge-apps"

    look_printed_font="" look_got_small=false look_got_card_aa=false
    if [ -x "$look_edge" ] && look_out=$("$look_edge" --print-theme 2>/dev/null); then
        while IFS= read -r look_line; do
            case "$look_line" in
                LOOK_[A-Z0-9_]*=\"*\")
                    look_name=${look_line%%=*} look_val=${look_line#*=}
                    look_val=${look_val#\"} look_val=${look_val%\"}
                    case "$look_name" in
                        LOOK_CARD|LOOK_CARD_AA|LOOK_EDGE|LOOK_EDGE_AA|LOOK_TEXT|LOOK_TEXT_AA|LOOK_MUTED|LOOK_MUTED_AA|LOOK_ACCENT|LOOK_ACCENT_AA|LOOK_ON_ACCENT|LOOK_ON_ACCENT_AA|LOOK_ACCENT_TEXT|LOOK_ACCENT_TEXT_AA|LOOK_ALERT|LOOK_ALERT_AA|LOOK_TRACK|LOOK_TRACK_AA|LOOK_CORNERS|LOOK_SMALL_RADIUS)
                            case "$look_val" in
                                *[!A-Za-z0-9_#]*) ;;
                                ?*)
                                    eval "$look_name=\$look_val"
                                    case "$look_name" in
                                        LOOK_SMALL_RADIUS) look_got_small=true ;;
                                        LOOK_CARD_AA) look_got_card_aa=true ;;
                                    esac
                                    ;;
                            esac
                            ;;
                        LOOK_FONT)
                            case "$look_val" in
                                *[!A-Za-z0-9_.,+\ -]*) ;;
                                ?*) look_printed_font="$look_val" ;;
                            esac
                            ;;
                    esac
                    ;;
            esac
        done << LOOK_END
$look_out
LOOK_END
        [ "$look_got_small" = true ] || LOOK_SMALL_RADIUS=$(( (LOOK_CORNERS * 55 + 50) / 100 ))
        # Keep the configured FONT_NAME until --print-theme reads FONT_NAME from config
        if [ -n "$look_printed_font" ] && { [ "$look_printed_font" != "sans-serif" ] || [ "${FONT_NAME:-sans-serif}" = "sans-serif" ]; }; then
            LOOK_FONT="$look_printed_font"
        fi
    fi

    if [ -z "$LOOK_MUTED" ] || [ -z "$LOOK_ON_ACCENT" ] || [ -z "$LOOK_ACCENT_TEXT" ] || [ -z "$LOOK_ALERT" ] || [ "$look_got_card_aa" != true ]; then
        IFS=' ' read -r look_aa look_muted look_on_acc look_acc_text look_alert << EOF
$(LC_ALL=C awk -v card="${LOOK_CARD#\#}" -v text="${LOOK_TEXT#\#}" -v acc="${LOOK_ACCENT#\#}" -v op="${CARD_OPACITY:-90}" -v scheme="${COLOR_SCHEME:-prefer-dark}" '
    function abs(v) { return (v < 0) ? -v : v }
    function max3(a, b, c,   m) { m = (a > b) ? a : b; return (m > c) ? m : c }
    function h(s, i) { return index("0123456789abcdef", tolower(substr(s, i, 1))) - 1 }
    function c(s, i) { return h(s, i) * 16 + h(s, i + 1) }
    function mix(a, b, t) {
        return sprintf("%02x%02x%02x",
            int(c(a, 1) * (1 - t) + c(b, 1) * t + 0.5),
            int(c(a, 3) * (1 - t) + c(b, 3) * t + 0.5),
            int(c(a, 5) * (1 - t) + c(b, 5) * t + 0.5))
    }
    function lin(v,   x) { x = v / 255.0; return (x <= 0.04045) ? x / 12.92 : ((x + 0.055) / 1.055) ^ 2.4 }
    function lum(s) { return 0.2126 * lin(c(s, 1)) + 0.7152 * lin(c(s, 3)) + 0.0722 * lin(c(s, 5)) }
    function contrast(a, b,   la, lb, hi, lo) {
        la = lum(a); lb = lum(b)
        hi = (la > lb) ? la : lb; lo = (la < lb) ? la : lb
        return (hi + 0.05) / (lo + 0.05)
    }
    function hsl_hex(hue, sat, lig,   ch, hp, x, m, r, g, b, sec) {
        ch = (1.0 - abs(2.0 * lig - 1.0)) * sat
        hue = hue % 360.0; if (hue < 0) hue += 360.0
        hp = hue / 60.0
        x = ch * (1.0 - abs((hp % 2.0) - 1.0))
        sec = int(hp)
        if (sec == 0)      { r = ch; g = x;  b = 0 }
        else if (sec == 1) { r = x;  g = ch; b = 0 }
        else if (sec == 2) { r = 0;  g = ch; b = x }
        else if (sec == 3) { r = 0;  g = x;  b = ch }
        else if (sec == 4) { r = x;  g = 0;  b = ch }
        else               { r = ch; g = 0;  b = x }
        m = lig - ch / 2.0
        return sprintf("%02x%02x%02x",
            int((r + m) * 255.0 + 0.5),
            int((g + m) * 255.0 + 0.5),
            int((b + m) * 255.0 + 0.5))
    }
    function make_alert(hue, sat, lig,   base, i, cand) {
        base = mix(hsl_hex(hue, sat, lig), text, 0.08)
        for (i = 0; i <= 10; i++) {
            cand = mix(base, text, i / 10.0)
            if (contrast(cand, card) >= 4.5) return cand
        }
        return text
    }
    BEGIN {
        if (op !~ /^[0-9]+(\.[0-9]+)?$/) op = 90
        if (op < 0) op = 0
        if (op > 100) op = 100
        aa = sprintf("%02x", int(op * 255 / 100 + 0.5))
        muted = mix(text, card, 0.32)
        on_lum = 0.2126 * c(acc, 1) / 255.0 + 0.7152 * c(acc, 3) / 255.0 + 0.0722 * c(acc, 5) / 255.0
        on_acc = (on_lum > 0.45) ? card : text
        acc_text = text
        for (i = 0; i <= 10; i++) {
            cand = mix(acc, text, i / 10.0)
            if (contrast(cand, card) >= 3.0) { acc_text = cand; break }
        }
        light_card = (scheme == "prefer-light" || lum(card) > 0.35)
        s = light_card ? 0.74 : 0.80
        l = light_card ? 0.42 : 0.65
        alert_c = make_alert(358.0, s, l)
        if (max3(abs(c(alert_c, 1) - c(acc, 1)),
                 abs(c(alert_c, 3) - c(acc, 3)),
                 abs(c(alert_c, 5) - c(acc, 5))) / 255.0 < 0.14) {
            alert_c = make_alert(28.0, s, l)
        }
        printf "%s #%s #%s #%s #%s\n", aa, muted, on_acc, acc_text, alert_c
    }')
EOF
        [ "$look_got_card_aa" = true ] || LOOK_CARD_AA="$look_aa"
        [ -n "$LOOK_MUTED" ] || LOOK_MUTED="$look_muted"
        [ -n "$LOOK_ON_ACCENT" ] || LOOK_ON_ACCENT="$look_on_acc"
        [ -n "$LOOK_ACCENT_TEXT" ] || LOOK_ACCENT_TEXT="$look_acc_text"
        [ -n "$LOOK_ALERT" ] || LOOK_ALERT="$look_alert"
    fi
    [ -n "$LOOK_TRACK" ] || LOOK_TRACK="$LOOK_TEXT"
}

# The resolved look saved for the helpers that draw themselves (the lock screen): one file of
# LOOK_*="value" lines, rewritten when it changes
look_save() {
    look_file="${XDG_CACHE_HOME:-$HOME/.cache}/infynitae/look"
    mkdir -p "${look_file%/*}"
    put_file "$look_file" << EOF
LOOK_CARD="${LOOK_CARD}"
LOOK_CARD_AA="${LOOK_CARD_AA}"
LOOK_EDGE="${LOOK_EDGE}"
LOOK_EDGE_AA="${LOOK_EDGE_AA}"
LOOK_TEXT="${LOOK_TEXT}"
LOOK_TEXT_AA="${LOOK_TEXT_AA}"
LOOK_MUTED="${LOOK_MUTED}"
LOOK_MUTED_AA="${LOOK_MUTED_AA}"
LOOK_ACCENT="${LOOK_ACCENT}"
LOOK_ACCENT_AA="${LOOK_ACCENT_AA}"
LOOK_ON_ACCENT="${LOOK_ON_ACCENT}"
LOOK_ON_ACCENT_AA="${LOOK_ON_ACCENT_AA}"
LOOK_ACCENT_TEXT="${LOOK_ACCENT_TEXT}"
LOOK_ACCENT_TEXT_AA="${LOOK_ACCENT_TEXT_AA}"
LOOK_ALERT="${LOOK_ALERT}"
LOOK_ALERT_AA="${LOOK_ALERT_AA}"
LOOK_TRACK="${LOOK_TRACK}"
LOOK_TRACK_AA="${LOOK_TRACK_AA}"
LOOK_CORNERS="${LOOK_CORNERS}"
LOOK_SMALL_RADIUS="${LOOK_SMALL_RADIUS}"
LOOK_FONT="${LOOK_FONT}"
EOF
}

# Load current config and fill defaults
load_config
apply_defaults

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
    tmp=$(mktemp "${CONFIG_FILE}.tmp.XXXXXX")
    trap 'rm -f "$tmp"' EXIT INT TERM
    awk 'BEGIN { k = ENVIRON["CW_KEY"]; v = ENVIRON["CW_VAL"]; found = 0 }
        substr($0, 1, length(k) + 1) == k "=" { print k "=\"" v "\""; found = 1; next }
        { print }
        END { if (!found) print k "=\"" v "\"" }' "$CONFIG_FILE" > "$tmp" \
        && mv -f "$tmp" "$CONFIG_FILE"
    trap - EXIT INT TERM
}

wallpaper_accents() {
    wa_bin="$HOME/.local/bin/infynitae-wallpaper"
    [ -x "$wa_bin" ] && [ -n "${1:-}" ] || return 1
    wa_out=$("$wa_bin" --accents "$1" 2>/dev/null) || return 1
    printf '%s\n' "$wa_out" | LC_ALL=C awk '
        $1 ~ /^#[0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F]$/ {
            if (NF == 1 || (NF == 3 && $2 ~ /^[0-9]+(\.[0-9]+)?$/ && $3 ~ /^[0-9]+(\.[0-9]+)?$/ && ($2 + 0) >= 0 && ($2 + 0) <= 1 && ($3 + 0) >= 0 && ($3 + 0) <= 1)) {
                ok = 1
                print
            }
        }
        END { exit !ok }
    '
}

pick_random_line() {
    pr_lines=$(cat)
    [ -n "$pr_lines" ] || return 1
    pr_count=$(printf '%s\n' "$pr_lines" | wc -l | tr -d ' \t\n')
    [ "$pr_count" -gt 0 ] || return 1
    pr_rnd=$(od -An -tu2 -N2 /dev/urandom | tr -d ' \t\n')
    pr_idx=$((pr_rnd % pr_count + 1))
    printf '%s\n' "$pr_lines" | sed -n "${pr_idx}p"
}

cmd_log() {
    case "${1:-}" in *[!A-Za-z0-9_]*|"") echo "Usage: infynitae-settings log <KEY> <OLD>" >&2; exit 1 ;; esac
    log_file="$HOME/.local/state/infynitae/settings-changes"
    mkdir -p "$(dirname "$log_file")"
    printf '%s\t%s\t%s\n' "$(date +%s)" "$1" "$(printf '%s' "${2:-}" | tr '\t\n' '  ')" >> "$log_file"
    tail -n 50 "$log_file" > "$log_file.new" && mv -f "$log_file.new" "$log_file"
}

# Validate a saved settings archive and unpack it into `$2`. Refuses non-archives,
# non-regular members (symlinks, hardlinks, directories), traversal, and any file
# other than config, panels.backup, and .face.
archive_validate_and_unpack() {
    av_arch="${1:-}" av_stage="${2:-}"
    if [ -z "$av_arch" ] || [ ! -f "$av_arch" ] || [ -L "$av_arch" ]; then
        echo "Settings file not found." >&2
        return 1
    fi
    if ! gzip -t "$av_arch" >/dev/null 2>&1; then
        echo "Not a valid settings archive." >&2
        return 1
    fi
    av_names=$(tar -tzf "$av_arch" 2>/dev/null) || {
        echo "Not a valid settings archive." >&2
        return 1
    }
    av_meta=$(tar -tvzf "$av_arch" 2>/dev/null) || {
        echo "Not a valid settings archive." >&2
        return 1
    }
    av_nc=$(printf '%s\n' "$av_names" | awk '
        BEGIN { ok = 1; n = 0; has_cfg = 0 }
        {
            n++
            if (n > 3) { ok = 0; exit }
            f = $0
            if (f != ".config/infynitae/config" && f != ".config/infynitae/panels.backup" && f != ".face") {
                ok = 0; exit
            }
            if (seen[f]++) { ok = 0; exit }
            if (f == ".config/infynitae/config") has_cfg = 1
        }
        END {
            if (!ok || n == 0 || !has_cfg) print "BAD"
            else print n
        }
    ')
    case "$av_nc" in
        [1-3]) ;;
        *)
            echo "Archive holds unexpected files or is missing config." >&2
            return 1
            ;;
    esac
    av_mc=$(printf '%s\n' "$av_meta" | awk '
        BEGIN { ok = 1; n = 0 }
        {
            n++
            if (substr($1, 1, 1) != "-") { ok = 0; exit }
            sz = -1
            if ($3 ~ /^[0-9]+$/) sz = $3
            else if ($5 ~ /^[0-9]+$/) sz = $5
            if (sz < 0 || sz > 33554432) { ok = 0; exit }
        }
        END { print (ok ? n : "BAD") }
    ')
    if [ "$av_mc" != "$av_nc" ]; then
        echo "Archive holds unsupported or oversized files." >&2
        return 1
    fi
    mkdir -p "$av_stage"
    if ! tar -xzf "$av_arch" -C "$av_stage" >/dev/null 2>&1; then
        echo "Not a valid settings archive." >&2
        return 1
    fi
    for av_f in .config/infynitae/config .config/infynitae/panels.backup .face; do
        if [ -L "$av_stage/$av_f" ]; then
            echo "Archive holds unsupported file type." >&2
            return 1
        fi
    done
    if [ ! -f "$av_stage/.config/infynitae/config" ]; then
        echo "Archive is missing the config file." >&2
        return 1
    fi
    av_bad=$(LC_ALL=C awk '
        function valid_sq(s,    n, i) {
            n = length(s)
            if (n < 2 || substr(s, 1, 1) != "\047") return 0
            i = 2
            while (i <= n) {
                if (substr(s, i, 1) != "\047") i++
                else if (i == n) return 1
                else if (substr(s, i, 4) == "\047\134\047\047" && i + 3 < n) i += 4
                else return 0
            }
            return 0
        }
        function valid_dq(s,    inner) {
            if (length(s) < 2 || substr(s, 1, 1) != "\"" || substr(s, length(s), 1) != "\"") return 0
            inner = substr(s, 2, length(s) - 2)
            # $HOME (and ${HOME}) is the one variable configs use ("$HOME/Pictures/Screenshots");
            # any other $, a backquote or a backslash could run or reach something else
            gsub(/\$\{HOME\}/, "", inner)
            gsub(/\$HOME\//, "/", inner)
            sub(/\$HOME$/, "", inner)
            return index(inner, "\"") == 0 && index(inner, "$") == 0 && index(inner, "\140") == 0 && index(inner, "\134") == 0
        }
        {
            if (index($0, "\r") > 0) { print NR; exit }
            line = $0
            sub(/^[ \t]+/, "", line)
            sub(/[ \t]+$/, "", line)
            if (line == "" || substr(line, 1, 1) == "#") next
            # "export KEY=VALUE" is a plain assignment too; the rest is checked the same way
            sub(/^export[ \t]+/, "", line)
            eq = index(line, "=")
            if (eq <= 1) { print NR; exit }
            key = substr(line, 1, eq - 1)
            if (key !~ /^[A-Z_][A-Z0-9_]*$/) { print NR; exit }
            val = substr(line, eq + 1)
            if (substr(val, 1, 1) == "\047") {
                if (!valid_sq(val)) { print NR; exit }
            } else if (substr(val, 1, 1) == "\"") {
                if (!valid_dq(val)) { print NR; exit }
            } else if (val !~ /^[A-Za-z0-9_.\/:+%@,-]+$/) {
                print NR; exit
            }
        }
    ' "$av_stage/.config/infynitae/config") || {
        echo "Failed to check config in archive." >&2
        return 1
    }
    if [ -n "$av_bad" ]; then
        echo "Archive config has unsafe line $av_bad." >&2
        return 1
    fi
    return 0
}

# Print changed config key names (sorted, one per line) comparing `$1` to `$2` without eval.
archive_diff_keys() {
    ad_cur="${1:-}" ad_new="${2:-}" ad_stage="${3:-}"
    { printf '#\n'; [ -f "$ad_cur" ] && cat "$ad_cur"; } > "$ad_stage/cur.in"
    awk '
        function trim(s) {
            sub(/^[ \t\r]+/, "", s)
            sub(/[ \t\r]+$/, "", s)
            return s
        }
        function parse_val(raw,    out, i, c) {
            raw = trim(raw)
            if (substr(raw, 1, 1) == "\047") {
                out = ""
                i = 2
                while (i <= length(raw)) {
                    if (substr(raw, i, 4) == "\047\\\047\047") {
                        out = out "\047"
                        i += 4
                    } else if (substr(raw, i, 1) == "\047") {
                        break
                    } else {
                        out = out substr(raw, i, 1)
                        i++
                    }
                }
                return out
            }
            if (substr(raw, 1, 1) == "\"") {
                out = ""
                i = 2
                while (i <= length(raw)) {
                    c = substr(raw, i, 1)
                    if (c == "\\" && i < length(raw)) {
                        out = out substr(raw, i + 1, 1)
                        i += 2
                    } else if (c == "\"") {
                        break
                    } else {
                        out = out c
                        i++
                    }
                }
                return out
            }
            sub(/[ \t]+#.*$/, "", raw)
            sub(/^#.*$/, "", raw)
            return trim(raw)
        }
        FNR == 1 { pass++ }
        {
            line = $0
            sub(/\r$/, "", line)
            sub(/^[ \t]+/, "", line)
            if (line == "" || substr(line, 1, 1) == "#") next
            eq = index(line, "=")
            if (eq <= 1) next
            k = substr(line, 1, eq - 1)
            if (k !~ /^[A-Za-z_][A-Za-z0-9_]*$/) next
            v = parse_val(substr(line, eq + 1))
            seen[k] = 1
            if (pass == 1) { cur_has[k] = 1; cur_val[k] = v }
            else { new_has[k] = 1; new_val[k] = v }
        }
        END {
            for (k in seen) {
                if (!(k in cur_has) || !(k in new_has) || cur_val[k] != new_val[k]) {
                    print k
                }
            }
        }
    ' "$ad_stage/cur.in" "$ad_new" | LC_ALL=C sort
}

cmd_export() {
    exp_out="${1:-}"
    if [ ! -f "$CONFIG_FILE" ]; then
        echo "No settings file to save." >&2
        exit 1
    fi
    if [ -z "$exp_out" ]; then
        exp_dir="$HOME/Documents/Infynitae"
        mkdir -p "$exp_dir"
        exp_out="$exp_dir/settings-$(date +%Y-%m-%d-%H%M).tar.gz"
    else
        mkdir -p "$(dirname "$exp_out")"
    fi
    exp_stage=$(mktemp -d "${TMPDIR:-/tmp}/infynitae-export.XXXXXX") || exit 1
    exp_tmp=$(mktemp "$(dirname "$exp_out")/.settings-export.XXXXXX") || { rm -rf "$exp_stage"; exit 1; }
    mkdir -p "$exp_stage/.config/infynitae"
    cat "$CONFIG_FILE" > "$exp_stage/.config/infynitae/config"
    set -- ".config/infynitae/config"
    exp_cfg_dir=$(dirname "$CONFIG_FILE")
    if [ -f "$exp_cfg_dir/panels.backup" ]; then
        cat "$exp_cfg_dir/panels.backup" > "$exp_stage/.config/infynitae/panels.backup"
        set -- "$@" ".config/infynitae/panels.backup"
    fi
    if [ -f "$HOME/.face" ]; then
        cat "$HOME/.face" > "$exp_stage/.face"
        set -- "$@" ".face"
    fi
    if ! tar -czf "$exp_tmp" -C "$exp_stage" "$@"; then
        rm -rf "$exp_stage" "$exp_tmp"
        echo "Could not write settings archive." >&2
        exit 1
    fi
    mv -f "$exp_tmp" "$exp_out"
    rm -rf "$exp_stage"
    if command -v notify-send >/dev/null 2>&1; then
        notify-send -a Infynitae "Settings saved" "Saved to $exp_out" >/dev/null 2>&1 || true
    fi
    printf '%s\n' "$exp_out"
}

cmd_import_preview() {
    ip_arch="${1:-}"
    if [ -z "$ip_arch" ]; then
        echo "Usage: infynitae-settings import-preview <ARCHIVE>" >&2
        exit 1
    fi
    ip_stage=$(mktemp -d "${TMPDIR:-/tmp}/infynitae-preview.XXXXXX") || exit 1
    if ! archive_validate_and_unpack "$ip_arch" "$ip_stage"; then
        rm -rf "$ip_stage"
        exit 1
    fi
    archive_diff_keys "$CONFIG_FILE" "$ip_stage/.config/infynitae/config" "$ip_stage"
    rm -rf "$ip_stage"
}

cmd_import() {
    im_arch="${1:-}"
    if [ -z "$im_arch" ]; then
        echo "Usage: infynitae-settings import <ARCHIVE>" >&2
        exit 1
    fi
    im_stage=$(mktemp -d "${TMPDIR:-/tmp}/infynitae-import.XXXXXX") || exit 1
    if ! archive_validate_and_unpack "$im_arch" "$im_stage"; then
        rm -rf "$im_stage"
        exit 1
    fi
    im_cfg_dir=$(dirname "$CONFIG_FILE")
    mkdir -p "$im_cfg_dir"
    if [ -f "$CONFIG_FILE" ]; then
        cat "$CONFIG_FILE" > "$im_cfg_dir/config.before-load"
    else
        : > "$im_cfg_dir/config.before-load"
    fi
    im_tmp=$(mktemp "${CONFIG_FILE}.tmp.XXXXXX") || { rm -rf "$im_stage"; exit 1; }
    cat "$im_stage/.config/infynitae/config" > "$im_tmp" && mv -f "$im_tmp" "$CONFIG_FILE"
    if [ -f "$im_stage/.config/infynitae/panels.backup" ]; then
        im_tmp=$(mktemp "$im_cfg_dir/.panels.backup.tmp.XXXXXX") || { rm -rf "$im_stage"; exit 1; }
        cat "$im_stage/.config/infynitae/panels.backup" > "$im_tmp" && mv -f "$im_tmp" "$im_cfg_dir/panels.backup"
    fi
    if [ -f "$im_stage/.face" ]; then
        if [ -f "$HOME/.face" ]; then
            cat "$HOME/.face" > "$HOME/.face.bak"
        fi
        im_tmp=$(mktemp "$HOME/.face.tmp.XXXXXX") || { rm -rf "$im_stage"; exit 1; }
        cat "$im_stage/.face" > "$im_tmp" && mv -f "$im_tmp" "$HOME/.face"
    fi
    rm -rf "$im_stage"
    load_config
    apply_defaults
    apply_settings --force >/dev/null 2>&1 || true
}

# The theme colours from an image (infynitae-wallpaper --colors) into the config: the accent alone
# ("accent"), a random candidate accent with backgrounds and borders ("shuffle"), or the first
# candidate with backgrounds and borders ("all"). Returns 1 when the image cannot be read or the
# helper is missing; the config is then unchanged.
wallpaper_colors() {
    wc_image="$1" wc_what="${2:-all}"
    wc_bin="$HOME/.local/bin/infynitae-wallpaper"
    [ -x "$wc_bin" ] && [ -n "$wc_image" ] || return 1
    wc_out=$("$wc_bin" --colors "$wc_image" 2>/dev/null) || return 1
    wc_pick=""
    if [ "$wc_what" = shuffle ]; then
        wc_pick=$(wallpaper_accents "$wc_image" | awk '{print $1}' | pick_random_line) || wc_pick=""
    fi
    printf '%s\n' "$wc_out" | while IFS='=' read -r wc_key wc_val; do
        wc_val=$(printf '%s' "$wc_val" | tr -d '"')
        case "$wc_val" in \#[0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F]) ;; *) continue ;; esac
        [ "$wc_key" != BORDER_ACTIVE ] || wc_key=ACCENT
        if [ "$wc_what" = accent ] && [ "$wc_key" != ACCENT ]; then continue; fi
        if [ "$wc_key" = ACCENT ] && [ -n "$wc_pick" ]; then wc_val="$wc_pick"; fi
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
    hex=$(printf '%s' "${LOOK_ACCENT:-${ACCENT:-${BORDER_ACTIVE:-#7aa2f7}}}" | tr -d '#' | tr 'A-F' 'a-f')
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
    SETTINGS_SKIPPED=false
    force_apply=false
    for arg in "$@"; do
        case "$arg" in
            --force|-f) force_apply=true ;;
        esac
    done
    case "$(cat /proc/$PPID/comm 2>/dev/null) $(tr '\0' ' ' < /proc/$PPID/cmdline 2>/dev/null)" in
        *reload*) force_apply=true ;;
    esac

    # THEME_FROM_WALLPAPER: a new wallpaper (path or file changed) gives the theme its colours
    case "${THEME_FROM_WALLPAPER:-false}" in
        accent|all|shuffle|true|1|yes|on)
            twf_what=all
            case "$THEME_FROM_WALLPAPER" in accent|shuffle) twf_what="$THEME_FROM_WALLPAPER" ;; esac
            twf_wp=$(expand_path "${WALLPAPER:-}")
            if [ -f "$twf_wp" ]; then
                twf_stamp="${XDG_CACHE_HOME:-$HOME/.cache}/infynitae/theme-from-wallpaper"
                twf_now="$twf_what $(stat -c '%s %Y' "$twf_wp" 2>/dev/null) $twf_wp"
                if [ "$(cat "$twf_stamp" 2>/dev/null)" != "$twf_now" ] && wallpaper_colors "$twf_wp" "$twf_what"; then
                    mkdir -p "$(dirname "$twf_stamp")"
                    printf '%s\n' "$twf_now" > "$twf_stamp"
                    load_config
                    apply_defaults
                fi
            fi
            ;;
    esac

    # Skip when neither config nor settings.sh changed since the last apply
    stamp_file="${XDG_CACHE_HOME:-$HOME/.cache}/infynitae/settings-applied"
    cfg_stamp=$(cksum < "$CONFIG_FILE" 2>/dev/null || echo 0)
    self_file="$0"
    [ -L "$self_file" ] && self_file=$(readlink -f "$self_file" 2>/dev/null || echo "$0")
    [ -f "$self_file" ] || self_file="${SCRIPT_DIR:-$HOME/infynitae/live}/settings.sh"
    self_stamp=$(cksum < "$self_file" 2>/dev/null || echo 0)
    stamp_now="$cfg_stamp $self_stamp"

    if [ "$force_apply" != true ] && [ -f "$stamp_file" ] && [ "$(cat "$stamp_file" 2>/dev/null)" = "$stamp_now" ]; then
        SETTINGS_SKIPPED=true
        return 0
    fi

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
    case "${THEME_SYNC:-true}" in
        0|false|no|off|FALSE|False) ;;
        *)
            # Derive solid border fallback (for translucent LOOK_EDGE), scheme canvas, terminal
            # selection blend (30% accent into surface), and all Qt palette colours in one awk call.
            IFS=' ' read -r edge_solid app_canvas sel_hex \
                q_win q_base q_alt q_btn q_light q_mid_light q_mid q_dark q_shadow \
                q_text q_dim q_off q_acc q_acc_off q_on_acc q_link q_visited << EOF
$(LC_ALL=C awk \
    -v card="${LOOK_CARD#\#}" \
    -v text="${LOOK_TEXT#\#}" \
    -v acc="${LOOK_ACCENT#\#}" \
    -v on_acc="${LOOK_ON_ACCENT#\#}" \
    -v edge="${LOOK_EDGE#\#}" \
    -v edge_aa="${LOOK_EDGE_AA:-e6}" \
    -v canvas="${CANVAS_COLOR#\#}" \
    -v scheme="${THEME_SCHEME:-yours}" '
    function h(s, i) { return index("0123456789abcdef", tolower(substr(s, i, 1))) - 1 }
    function c(s, i) { return h(s, i) * 16 + h(s, i + 1) }
    function mix(a, b, t) {
        return sprintf("%02x%02x%02x",
            int(c(a, 1) * (100 - t) / 100 + c(b, 1) * t / 100 + 0.5),
            int(c(a, 3) * (100 - t) / 100 + c(b, 3) * t / 100 + 0.5),
            int(c(a, 5) * (100 - t) / 100 + c(b, 5) * t / 100 + 0.5))
    }
    BEGIN {
        ea = h(edge_aa, 1) * 16 + h(edge_aa, 2)
        edge_sol = (ea < 128) ? mix(card, edge, ea * 100 / 255) : tolower(edge)
        app_canv = (scheme == "" || scheme == "yours") ? tolower(canvas) : mix(card, "000000", 38)
        sel = mix(card, acc, 30)
        printf "#%s #%s %s #ff%s #ff%s #ff%s #ff%s #ff%s #ff%s #ff%s #ff%s #ff%s #ff%s #ff%s #ff%s #ff%s #ff%s #ff%s #ff%s #ff%s\n",
            edge_sol, app_canv, sel,
            tolower(card), mix(card, app_canv, 50), mix(card, text, 4), mix(card, text, 8),
            mix(card, text, 22), mix(card, text, 14), edge_sol, mix(card, "000000", 45),
            mix(card, "000000", 75), tolower(text), mix(text, card, 45), mix(text, card, 60),
            tolower(acc), mix(acc, card, 55), tolower(on_acc), mix(acc, text, 25),
            mix(acc, text, 55)
    }')
EOF
            ;;
    esac
    # (after the colours are known: the wallpaper may have just given them)
    ICON_THEME=$(folder_icon_theme)
    for gtk_dir in "$HOME/.config/gtk-3.0" "$HOME/.config/gtk-4.0"; do
        mkdir -p "$gtk_dir"
        put_file "$gtk_dir/settings.ini" << EOF
[Settings]
gtk-cursor-theme-name=$CURSOR_THEME
gtk-cursor-theme-size=$CURSOR_SIZE
gtk-font-name=$LOOK_FONT $FONT_SIZE
gtk-icon-theme-name=$ICON_THEME
gtk-application-prefer-dark-theme=$([ "$COLOR_SCHEME" = "prefer-dark" ] && echo 1 || echo 0)
gtk-enable-animations=true
EOF
        gtk_css="$gtk_dir/gtk.css"
        case "${THEME_SYNC:-true}" in
            0|false|no|off|FALSE|False) continue ;;
        esac
        [ ! -f "$gtk_css" ] || grep -q 'managed by infynitae' "$gtk_css" 2>/dev/null || continue
        {
            cat << EOF
/* managed by infynitae — delete this line to keep custom GTK CSS */
@define-color theme_bg_color $LOOK_CARD;
@define-color theme_fg_color $LOOK_TEXT;
@define-color theme_base_color mix($LOOK_CARD, $app_canvas, 0.5);
@define-color theme_text_color $LOOK_TEXT;
@define-color theme_selected_bg_color $LOOK_ACCENT;
@define-color theme_selected_fg_color $LOOK_ON_ACCENT;
@define-color borders $edge_solid;
@define-color window_bg_color $LOOK_CARD;
@define-color window_fg_color $LOOK_TEXT;
@define-color view_bg_color mix($LOOK_CARD, $app_canvas, 0.5);
@define-color view_fg_color $LOOK_TEXT;
@define-color headerbar_bg_color shade($LOOK_CARD, 1.15);
@define-color headerbar_fg_color $LOOK_TEXT;
@define-color headerbar_backdrop_color $LOOK_CARD;
@define-color sidebar_bg_color shade($LOOK_CARD, 1.1);
@define-color sidebar_fg_color $LOOK_TEXT;
@define-color card_bg_color mix($LOOK_CARD, $LOOK_TEXT, 0.05);
@define-color card_fg_color $LOOK_TEXT;
@define-color popover_bg_color shade($LOOK_CARD, 1.2);
@define-color popover_fg_color $LOOK_TEXT;
@define-color dialog_bg_color shade($LOOK_CARD, 1.15);
@define-color dialog_fg_color $LOOK_TEXT;
@define-color accent_bg_color $LOOK_ACCENT;
@define-color accent_color $LOOK_ACCENT_TEXT;
@define-color accent_fg_color $LOOK_ON_ACCENT;
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
    border: 1px solid @borders; border-radius: ${LOOK_SMALL_RADIUS}px; box-shadow: none; text-shadow: none;
}
button:hover { background: mix(@theme_bg_color, @theme_fg_color, 0.08); }
button:active, button:checked { background: mix(@theme_bg_color, @accent_bg_color, 0.35); }
button.flat:not(:hover):not(:checked) { background: transparent; border-color: transparent; }
entry, spinbutton {
    background: @theme_base_color; color: @theme_text_color;
    border: 1px solid @borders; border-radius: ${LOOK_SMALL_RADIUS}px; box-shadow: none;
}
entry:focus, spinbutton:focus { border-color: @accent_bg_color; }
*:selected, selection {
    background-color: @theme_selected_bg_color; color: @theme_selected_fg_color;
}
.view:selected, treeview:selected, row:selected {
    background-color: alpha(@accent_bg_color, 0.16); color: @theme_fg_color;
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
    # (the cards' corners and colours: look_load)
    radius=$LOOK_CORNERS
    lc_hex="${LOOK_CARD#\#}"
    card_hex="${lc_hex}${LOOK_CARD_AA}" edge_hex="${LOOK_EDGE#\#}${LOOK_EDGE_AA}"
    lt_hex="${LOOK_TEXT#\#}" la_hex="${LOOK_ACCENT#\#}"
    on_acc_hex="${LOOK_ON_ACCENT#\#}"

    if command -v mako >/dev/null 2>&1 || [ -d "$HOME/.config/mako" ]; then
        mako_cfg="$HOME/.config/mako/config"
        if [ ! -f "$mako_cfg" ] || grep -q 'managed by infynitae' "$mako_cfg" 2>/dev/null; then
            mkdir -p "$HOME/.config/mako"
            put_file "$mako_cfg" << EOF
# managed by infynitae — notifications: one border, gone after 5 s, kept in the history
font=${LOOK_FONT} 13px
markup=1
format=<b>%s</b>\n<span alpha="68%%" size="small">%b</span>
background-color=#${card_hex}
text-color=#${lt_hex}
border-color=#${edge_hex}
border-size=1
border-radius=${radius}
width=360
height=110
margin=24
padding=16,18
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
    fi

    if command -v fuzzel >/dev/null 2>&1 || [ -d "$HOME/.config/fuzzel" ]; then
        fuzzel_cfg="$HOME/.config/fuzzel/fuzzel.ini"
        if [ ! -f "$fuzzel_cfg" ] || grep -q 'managed by infynitae' "$fuzzel_cfg" 2>/dev/null; then
            mkdir -p "$HOME/.config/fuzzel"
            put_file "$fuzzel_cfg" << EOF
# managed by infynitae — delete this line to keep custom fuzzel settings
[main]
font=${LOOK_FONT}:pixelsize=13
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
selection=${la_hex}29
selection-text=${lt_hex}ff
selection-match=${la_hex}ff
border=${edge_hex}

[border]
width=1
radius=${radius}
EOF
        fi
    fi

    if command -v foot >/dev/null 2>&1 || [ -d "$HOME/.config/foot" ]; then
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
background=${lc_hex}
foreground=${lt_hex}
regular0=151820
regular1=f7768e
regular2=9ece6a
regular3=e0af68
regular4=${la_hex}
regular5=bb9af7
regular6=7dcfff
regular7=c0caf5
EOF
        fi
    fi

    if command -v kitty >/dev/null 2>&1 || [ -d "$HOME/.config/kitty" ]; then
        kitty_theme="$HOME/.config/kitty/infynitae-theme.conf"
        mkdir -p "$HOME/.config/kitty"
        put_file "$kitty_theme" << EOF
# managed by infynitae
font_family      $TERMINAL_FONT
font_size        $FONT_SIZE
background       #$lc_hex
foreground       #$lt_hex
cursor           #$la_hex
selection_background #$sel_hex
selection_foreground #$lt_hex
window_padding_width 8
EOF
        kitty_cfg="$HOME/.config/kitty/kitty.conf"
        if [ ! -f "$kitty_cfg" ] || grep -q 'managed by infynitae' "$kitty_cfg" 2>/dev/null; then
            put_file "$kitty_cfg" << 'EOF'
# managed by infynitae — delete this line to keep custom kitty settings
include infynitae-theme.conf
EOF
        fi
    fi

    if command -v alacritty >/dev/null 2>&1 || [ -d "$HOME/.config/alacritty" ]; then
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
background = "#$lc_hex"
foreground = "#$lt_hex"

[colors.cursor]
text = "#$on_acc_hex"
cursor = "#$la_hex"

[colors.selection]
text = "#$lt_hex"
background = "#$sel_hex"
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
  --toolbox-bgcolor: color-mix(in srgb, $LOOK_CARD 88%, $app_canvas) !important;
  --toolbox-non-lwt-bgcolor: color-mix(in srgb, $LOOK_CARD 88%, $app_canvas) !important;
  --toolbox-textcolor: $LOOK_TEXT !important;
  --toolbar-bgcolor: color-mix(in srgb, $LOOK_CARD 92%, $LOOK_TEXT) !important;
  --toolbar-color: $LOOK_TEXT !important;
  --tab-selected-bgcolor: color-mix(in srgb, $LOOK_CARD 85%, $LOOK_TEXT) !important;
  --tab-selected-textcolor: $LOOK_TEXT !important;
  --toolbar-field-background-color: color-mix(in srgb, $LOOK_CARD 70%, $app_canvas) !important;
  --toolbar-field-focus-background-color: color-mix(in srgb, $LOOK_CARD 70%, $app_canvas) !important;
  --toolbar-field-color: $LOOK_TEXT !important;
  --toolbar-field-focus-color: $LOOK_TEXT !important;
  --toolbar-field-focus-border-color: $LOOK_ACCENT !important;
  --focus-outline-color: $LOOK_ACCENT !important;
  --arrowpanel-background: color-mix(in srgb, $LOOK_CARD 95%, $LOOK_TEXT) !important;
  --arrowpanel-color: $LOOK_TEXT !important;
  --arrowpanel-border-color: $edge_solid !important;
  --urlbarView-highlight-background: color-mix(in srgb, $LOOK_ACCENT 35%, $LOOK_CARD) !important;
  --sidebar-background-color: $LOOK_CARD !important;
  --chrome-content-separator-color: $edge_solid !important;
  accent-color: $LOOK_ACCENT !important;
}
#TabsToolbar, #titlebar, #navigator-toolbox { background-color: var(--toolbox-bgcolor) !important; }
.tab-background:is([selected], [multiselected]) { background-color: var(--tab-selected-bgcolor) !important; }
.tab-background[selected] { box-shadow: inset 0 -2px 0 $LOOK_ACCENT !important; }
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
    # from the system"): a qt6ct / qt5ct palette from the resolved LOOK_* palette, the Fusion style,
    # ICON_THEME and the font. env.sh makes Qt load qt6ct when it is installed. Each qt*ct.conf is
    # written while it is ours (infynitae_managed); qt6ct's own window saving it drops that key, and
    # then it is the user's.
    case "${THEME_SYNC:-true}" in
        0|false|no|off|FALSE|False) ;;
        *)
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
general="$LOOK_FONT,$FONT_SIZE,-1,5,400,0,0,0,0,0,0,0,0,0,0,1"

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
        escaped_exec=$(printf '%s\n' "$term_exec" | sed 's/[&|\\]/\\&/g')
        sed -i "s|<command>exo-open --working-directory %f --launch TerminalEmulator</command>|<command>$escaped_exec --dir=%f</command>|" "$uca"
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
        gsettings set org.gnome.desktop.interface font-name "$LOOK_FONT $FONT_SIZE" 2>/dev/null || true
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

    # 7. Record what was applied (a checksum of the config and of this script: a change within the same second, same size, still counts) (~/.cache/infynitae/settings-applied)
    mkdir -p "$(dirname "$stamp_file")"
    cfg_stamp=$(cksum < "$CONFIG_FILE" 2>/dev/null || echo 0)
    printf '%s %s\n' "$cfg_stamp" "$self_stamp" > "$stamp_file"
}

# Keys whose changes reach GTK/Qt/apps/terminals/mako/fuzzel/Firefox or ~/.cache/infynitae/look.
# Other keys (e.g. TOUCHPAD_TAP) do not rewrite app themes until the next login.
key_needs_apply() {
    case "${1:-}" in
        CURSOR_THEME|XCURSOR_THEME|CURSOR_SIZE|XCURSOR_SIZE|\
        FONT_NAME|TERMINAL_FONT|FONT_SIZE|ICON_THEME|FOLDER_COLOR|\
        COLOR_SCHEME|THEME_SCHEME|THEME_SYNC|THEME_FROM_WALLPAPER|WALLPAPER|\
        CANVAS_COLOR|SURFACE_COLOR|BORDER_ACTIVE|ACCENT|BORDER_INACTIVE|TEXT_COLOR|\
        CORNERS|WINDOW_CORNERS|CARD_OPACITY)
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

cmd_set() {
    KEY="${1:-}"
    VAL="${2:-}"
    # an empty value is allowed when given (set KEY ""): it puts a key like WALLPAPER back to its
    # default; a missing one is a mistake
    if [ -z "$KEY" ] || [ $# -lt 2 ]; then
        echo "Usage: infynitae-settings set <KEY> <VALUE> [OLD]" >&2
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
    tmp=$(mktemp "${CONFIG_FILE}.tmp.XXXXXX")
    trap 'rm -f "$tmp"' EXIT INT TERM
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
    ' "$CONFIG_FILE" > "$tmp" && mv -f "$tmp" "$CONFIG_FILE"
    trap - EXIT INT TERM

    if [ $# -ge 3 ]; then
        cmd_log "$KEY" "$3"
    fi

    echo "==> Updated $KEY in $CONFIG_FILE"
    if key_needs_apply "$KEY"; then
        load_config
        apply_defaults
        apply_settings --force
        echo "==> Applied."
    fi
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
            echo "Cursor themes:"
            find /usr/share/icons "$HOME/.icons" "$HOME/.local/share/icons" \
                -maxdepth 2 -type d -name "cursors" 2>/dev/null \
                | sed 's|/cursors$||' | xargs -r -d '\n' -n1 basename | sort -u \
                | while read -r theme; do
                    if [ "$theme" = "$CURSOR_THEME" ]; then
                        echo "  * $theme (active)"
                    else
                        echo "    $theme"
                    fi
                done
            ;;
        fonts|font)
            echo "Monospace and system fonts:"
            if command -v fc-list >/dev/null 2>&1; then
                fc-list : family | tr ',' '\n' | sed 's/^[ \t]*//' | sort -u | grep -iE 'mono|code|hack|jetbrains|nerd|dejavu|inter|noto' | head -40
            else
                find /usr/share/fonts -type f \( -name "*.ttf" -o -name "*.otf" \) 2>/dev/null | xargs -r -d '\n' -n1 basename | sed 's/\.[^.]*$//' | sort -u | head -40
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
        shift
        apply_settings "$@"
        if [ "${SETTINGS_SKIPPED:-false}" = true ]; then
            echo "==> Settings unchanged."
        else
            echo "==> Settings applied."
        fi
        ;;
    export)
        shift
        cmd_export "$@"
        ;;
    import-preview)
        shift
        cmd_import_preview "$@"
        ;;
    import)
        shift
        cmd_import "$@"
        ;;
    wallpaper-colors)
        # infynitae-settings wallpaper-colors [accent|all|accents|shuffle] [IMAGE]: theme from the wallpaper
        wc_what=all wc_img=""
        for a in "${2:-}" "${3:-}"; do
            case "$a" in "") ;; accent|all|accents|shuffle) wc_what="$a" ;; *) wc_img="$a" ;; esac
        done
        [ -n "$wc_img" ] || wc_img=$(expand_path "${WALLPAPER:-}")
        if [ ! -f "$wc_img" ]; then
            echo "No image: set WALLPAPER, or name one (infynitae-settings wallpaper-colors IMAGE)" >&2
            exit 1
        fi
        if [ "$wc_what" = accents ]; then
            if ! wallpaper_accents "$wc_img"; then
                echo "Could not read accents from $wc_img (is infynitae-wallpaper up to date?)" >&2
                exit 1
            fi
            exit 0
        fi
        if [ "$wc_what" = shuffle ]; then
            if ! wc_all=$(wallpaper_accents "$wc_img"); then
                echo "Could not read accents from $wc_img (is infynitae-wallpaper up to date?)" >&2
                exit 1
            fi
            wc_all=$(printf '%s\n' "$wc_all" | awk '{print $1}')
            wc_old="-" wc_cur=""
            if grep -q '^ACCENT=' "$CONFIG_FILE" 2>/dev/null; then
                wc_old=$(cmd_get ACCENT)
                wc_cur="$wc_old"
            fi
            if [ -z "$wc_cur" ]; then
                wc_cur=$(cmd_get BORDER_ACTIVE 2>/dev/null || printf '%s' "${ACCENT:-${BORDER_ACTIVE:-}}")
            fi
            wc_cur=$(printf '%s' "$wc_cur" | tr '[:upper:]' '[:lower:]')
            wc_avail=$(printf '%s\n' "$wc_all" | grep -ivFx "$wc_cur" || true)
            if [ -z "$wc_avail" ]; then
                echo "No other accent candidate in $wc_img (keeping ${wc_cur:-current})."
                exit 0
            fi
            wc_pick=$(printf '%s\n' "$wc_avail" | pick_random_line)
            cp -f "$CONFIG_FILE" "$CONFIG_FILE.bak-colors"
            cmd_set ACCENT "$wc_pick" "$wc_old"
        else
            cp -f "$CONFIG_FILE" "$CONFIG_FILE.bak-colors"
            if ! wallpaper_colors "$wc_img" "$wc_what"; then
                echo "Could not read colours from $wc_img (is infynitae-wallpaper up to date?)" >&2
                exit 1
            fi
            load_config
            apply_defaults
            apply_settings --force
        fi
        load_config
        grep -E '^(ACCENT|BORDER_ACTIVE|SURFACE_COLOR|CANVAS_COLOR|BORDER_INACTIVE)=' "$CONFIG_FILE" || true
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
    log)
        # log KEY OLD: a change from a Settings card, for its Undo ("-": the key had no line);
        # the last 50 kept in ~/.local/state/infynitae/settings-changes
        shift
        cmd_log "$@"
        ;;
    unset)
        # unset KEY: its line commented out, so the default applies again (Undo of a key that had
        # no line); applied like set
        shift
        case "${1:-}" in *[!A-Za-z0-9_]*|"") echo "Usage: infynitae-settings unset <KEY>" >&2; exit 1 ;; esac
        if grep -q "^$1=" "$CONFIG_FILE" 2>/dev/null; then
            tmp=$(mktemp "${CONFIG_FILE}.tmp.XXXXXX")
            sed "s/^$1=/# $1=/" "$CONFIG_FILE" > "$tmp" && mv -f "$tmp" "$CONFIG_FILE"
            eval "unset $1"
            if key_needs_apply "$1"; then
                load_config
                apply_defaults
                apply_settings --force
            fi
        fi
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
        echo "==> Canvas reset to 100% (zoom 1.0)."
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
        echo "  apply [--force]      Apply ~/.config/infynitae/config to apps"
        echo "  wallpaper-colors [accent|all|accents|shuffle] [IMAGE]"
        echo "                       Take, list, or shuffle theme colours from the wallpaper (or IMAGE)"
        echo "  update               Pull updates and rebuild the release binary"
        echo "  restore              Reset the canvas zoom to 100%"
        echo "  set <KEY> <VALUE> [OLD]"
        echo "                       Save a setting, optionally log OLD, and apply look changes"
        echo "  get <KEY>            Print a setting's value"
        echo "  list cursors         List installed cursor themes"
        echo "  list fonts           List monospace and system fonts"
        echo "  edit                 Open ~/.config/infynitae/config in your editor"
        echo "  reload               Restart the helpers in place (edge, wallpaper, portal…)"
        ;;
esac
