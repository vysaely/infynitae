#!/bin/sh
# Starts Infynitae as a nested window on the desktop, with session.sh inside it.
# Close the window to end it. Log: live/logs/last.log in the checkout

# the checkout these scripts run from: the one install.sh recorded, else ~/infynitae
INFYNITAE_DIR=; { IFS= read -r INFYNITAE_DIR < "$HOME/.config/infynitae/checkout"; } 2>/dev/null || :
INFYNITAE_DIR="${INFYNITAE_DIR:-$HOME/infynitae}"
BIN="$HOME/.local/bin/infynitae"
if [ ! -x "$BIN" ] && [ -x "$INFYNITAE_DIR/target/release/infynitae" ]; then
    BIN="$INFYNITAE_DIR/target/release/infynitae"
fi
XWLS=""
if command -v xwayland-satellite >/dev/null 2>&1; then
    XWLS="--xwayland-satellite $(command -v xwayland-satellite)"
fi
mkdir -p "$INFYNITAE_DIR/live/logs"
exec "$BIN" --nested --width 1600 --height 900 $XWLS "$@" \
    --exec "$INFYNITAE_DIR/live/session.sh" > "$INFYNITAE_DIR/live/logs/last.log" 2>&1
