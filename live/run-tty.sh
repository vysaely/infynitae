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

BIN="$HOME/.cache/infynitae-integration/target-tty/release/infynitae"
mkdir -p "$HOME/infynitae/live/logs"
LOG="$HOME/infynitae/live/logs/tty.log"
echo "Infynitae on the console. Ctrl+Alt+Backspace quits, Ctrl+Alt+F1 = KDE. Log: $LOG"
"$BIN" --tty --xwayland-satellite /usr/bin/xwayland-satellite \
    --exec "$HOME/infynitae/live/session.sh" "$@" > "$LOG" 2>&1
code=$?
echo "Infynitae exited ($code). Last lines of the log:"
tail -n 15 "$LOG"
