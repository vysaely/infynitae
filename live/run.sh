#!/bin/sh
# Starts Infynitae as a nested window on the desktop, with session.sh inside it.
# Close the window to end it. Log: ~/infynitae/live/logs/last.log
BIN="$HOME/.local/bin/infynitae"
if [ ! -x "$BIN" ] && [ -x "$HOME/infynitae/target/release/infynitae" ]; then
    BIN="$HOME/infynitae/target/release/infynitae"
fi
XWLS=""
if command -v xwayland-satellite >/dev/null 2>&1; then
    XWLS="--xwayland-satellite $(command -v xwayland-satellite)"
fi
mkdir -p "$HOME/infynitae/live/logs"
exec "$BIN" --nested --width 1600 --height 900 $XWLS "$@" \
    --exec "$HOME/infynitae/live/session.sh" > "$HOME/infynitae/live/logs/last.log" 2>&1
