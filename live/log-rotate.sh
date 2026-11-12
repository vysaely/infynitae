#!/bin/sh
# Log rotation for Infynitae append-only logs.
# Rotates a log file to .old when its size exceeds the limit (default 2 MB).
# Replaces any existing .old.

rotate_log() {
    _log="$1"
    _max="${2:-2097152}"
    [ -n "$_log" ] || return 0
    if [ -f "$_log" ]; then
        _size=$(wc -c < "$_log" 2>/dev/null || echo 0)
        if [ "$_size" -gt "$_max" ]; then
            mv -f "$_log" "$_log.old"
        fi
    fi
}

case "${0##*/}" in
    log-rotate.sh)
        [ $# -gt 0 ] && rotate_log "$@"
        ;;
esac
