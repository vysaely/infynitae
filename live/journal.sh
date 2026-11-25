#!/bin/sh
# The desktop journal, for Infynitae's scripts (sourced): journal_write EVENT PID DETAIL OUTCOME
# appends one line to $XDG_STATE_HOME/infynitae/journal/journal.tsv in the compositor's format
# (crates/infynitae-wayland/src/journal.rs): UTC time, event, program (/proc/PID/exe), pid,
# detail, outcome, separated by tabs. An empty PID is unknown. The compositor rotates the file.

# a field with backslash, tab, carriage return and newline escaped
_journal_escape() {
    printf '%s' "$1" | awk '{ gsub(/\\/, "\\\\"); gsub(/\t/, "\\t"); gsub(/\r/, "\\r");
        printf "%s%s", (NR > 1 ? "\\n" : ""), $0 }'
}

journal_write() {
    _jdir="${XDG_STATE_HOME:-$HOME/.local/state}/infynitae/journal"
    mkdir -p "$_jdir" 2>/dev/null || return 0
    _jprog=""
    [ -n "$2" ] && _jprog=$(readlink "/proc/$2/exe" 2>/dev/null)
    # only the user reads it, as the compositor makes it (0600)
    (umask 077 && printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$1" \
        "$(_journal_escape "$_jprog")" "$2" "$(_journal_escape "$3")" "$4" \
        >> "$_jdir/journal.tsv") 2>/dev/null || true
}
