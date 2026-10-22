#!/bin/sh
# Toggles the Infynitae Dynamic Island Start Launcher via Quickshell IPC
if [ -z "$WAYLAND_DISPLAY" ]; then
    export WAYLAND_DISPLAY=$(ls -t "$XDG_RUNTIME_DIR"/wayland-* 2>/dev/null | head -1 | xargs -r basename)
fi
exec qs ipc -p "$HOME/infynitae/live/quickshell/shell.qml" call island toggle
