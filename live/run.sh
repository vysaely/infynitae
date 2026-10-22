#!/bin/sh
# Starts Infynitae as a window on the KDE desktop, with the bar and foot inside it (session.sh).
# Close the window to end it. Log: ~/infynitae/live/logs/last.log
BIN="$HOME/.cache/infynitae-integration/target-tty/release/infynitae"
mkdir -p "$HOME/infynitae/live/logs"
exec "$BIN" --nested --width 1600 --height 900 "$@" \
    --xwayland-satellite /usr/bin/xwayland-satellite \
    --exec "$HOME/infynitae/live/session.sh" > "$HOME/infynitae/live/logs/last.log" 2>&1
