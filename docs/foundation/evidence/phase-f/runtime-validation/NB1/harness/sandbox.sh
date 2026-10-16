#!/bin/sh
# NB-1 sandbox (namespace init): as nested-logs/run.sh, plus the compositor's own host-side
# Wayland protocol trace (WAYLAND_DEBUG=client: its connection to the desktop compositor only;
# the session's clients get it unset by driver.sh).
# Run as: unshare -Urnmpf --mount-proc sh sandbox.sh <binary> <logdir> <nested|headless>
set -u
BIN=$1; LOG=$2; MODE=$3; HERE=$(cd "$(dirname "$0")" && pwd)
mount -t tmpfs tmpfs /run/systemd/journal && mount -t tmpfs tmpfs /dev/dri && mount -t tmpfs tmpfs /tmp && mkdir -m 700 /tmp/rt || exit 9
export XDG_RUNTIME_DIR=/tmp/rt DBUS_SESSION_BUS_ADDRESS=disabled: LIBGL_ALWAYS_SOFTWARE=1 NO_AT_BRIDGE=1 GSK_RENDERER=cairo
export LOGDIR=$LOG APPS=$HERE
if [ "$MODE" = nested ]; then export WAYLAND_DISPLAY=/run/user/1000/wayland-0; M=""; else unset WAYLAND_DISPLAY; M=--headless; fi
unset DISPLAY XAUTHORITY NOTIFY_SOCKET WAYLAND_SOCKET
date +%s.%N > "$LOG/t-start"
WAYLAND_DEBUG=${NB1_DEBUG:-client} "$BIN" $M --socket wl-nested --duration 150 --width 1280 --height 800 \
  --title "infynitae NB-1 test (automatic, closes itself)" \
  --xwayland-satellite "$SAT" --exec "sh $HERE/driver.sh" > "$LOG/compositor.log" 2>&1
echo "exit=$?" > "$LOG/exit"
date +%s.%N > "$LOG/t-end"
