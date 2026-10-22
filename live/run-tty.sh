#!/bin/sh
# Starts Infynitae on this text console (log in on tty2 with Ctrl+Alt+F2 first). It takes the
# screen, keyboard and mouse. Ctrl+Alt+Backspace quits; Ctrl+Alt+F1 goes back to KDE.
# Extra arguments are passed on, e.g.:  run-tty.sh --duration 90   (quits by itself after 90 s)
#                                       run-tty.sh --scale 1.5
# Log: ~/infynitae/live/logs/tty.log

# Ensure standalone environment before starting compositor on TTY
if [ -f "$HOME/infynitae/live/env.sh" ]; then
    . "$HOME/infynitae/live/env.sh"
fi

BIN="$HOME/.local/bin/infynitae"
if [ ! -x "$BIN" ] && [ -x "$HOME/infynitae/target/release/infynitae" ]; then
    BIN="$HOME/infynitae/target/release/infynitae"
fi
XWLS=""
if command -v xwayland-satellite >/dev/null 2>&1; then
    XWLS="--xwayland-satellite $(command -v xwayland-satellite)"
fi
mkdir -p "$HOME/infynitae/live/logs"
LOG="$HOME/infynitae/live/logs/tty.log"
echo "Infynitae on the console. Ctrl+Alt+Backspace quits, Ctrl+Alt+F1..F12 switches VT. Log: $LOG"
"$BIN" --tty $XWLS \
    --exec "$HOME/infynitae/live/session.sh" "$@" > "$LOG" 2>&1
code=$?
echo "Infynitae exited ($code). Last lines of the log:"
tail -n 15 "$LOG"
