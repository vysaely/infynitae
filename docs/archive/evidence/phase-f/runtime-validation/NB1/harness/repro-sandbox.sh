#!/bin/sh
# NB-1 deterministic reproducer (namespace init). The session command is an animating client
# (mpv, shm, 60 fps) so the compositor draws every frame; the experiment hook minimises the
# nested window after $NB1_MIN_MS; SIGTERM is sent at 8 s; --duration 15.
set -u
BIN=$1; LOG=$2
mount -t tmpfs tmpfs /run/systemd/journal && mount -t tmpfs tmpfs /dev/dri && mount -t tmpfs tmpfs /tmp && mkdir -m 700 /tmp/rt || exit 9
export XDG_RUNTIME_DIR=/tmp/rt DBUS_SESSION_BUS_ADDRESS=disabled: LIBGL_ALWAYS_SOFTWARE=1
export WAYLAND_DISPLAY=/run/user/1000/wayland-0
unset DISPLAY XAUTHORITY NOTIFY_SOCKET WAYLAND_SOCKET
date +%s.%N > "$LOG/t-start"
INFYNITAE_NB1_MINIMIZE_MS=${NB1_MIN_MS:-} WAYLAND_DEBUG=client "$BIN" --socket wl-repro --duration 15 --width 640 --height 400 \
  --title "infynitae NB-1 reproducer (automatic, closes itself)" \
  --exec "mpv --no-config --really-quiet --vo=wlshm --ao=null --loop=inf av://lavfi:testsrc=size=320x240:rate=60" > "$LOG/compositor.log" 2>&1 &
C=$!
if [ -z "${NB1_NOTERM:-}" ]; then sleep 8; date +%s.%N > "$LOG/t-term"; kill -TERM $C 2>/dev/null; fi
wait $C; echo "exit=$?" > "$LOG/exit"; date +%s.%N > "$LOG/t-end"
