#!/bin/sh
# NB-1 isolated reproducer (namespace init). No desktop involved: the host is a headless
# Infynitae (`main`), the compositor under test runs nested as its client. A second host client
# (mpv --fs) presents fullscreen at T_HIDE, hiding the nested window (on main a window hidden by
# a presentation gets no frame callbacks: INC-7 residual R-10); it is killed at T_REVEAL.
# usage: host-sandbox.sh <host-bin> <test-bin> <log> ; env T_HIDE T_REVEAL DUR (seconds)
set -u
HOST=$1; BIN=$2; LOG=$3
mount -t tmpfs tmpfs /run/systemd/journal && mount -t tmpfs tmpfs /dev/dri && mount -t tmpfs tmpfs /tmp && mkdir -m 700 /tmp/rt || exit 9
export XDG_RUNTIME_DIR=/tmp/rt DBUS_SESSION_BUS_ADDRESS=disabled: LIBGL_ALWAYS_SOFTWARE=1
unset DISPLAY XAUTHORITY NOTIFY_SOCKET WAYLAND_SOCKET WAYLAND_DISPLAY
MPV="mpv --no-config --really-quiet --ao=null --vo=wlshm --loop=inf"
ts() { python3 -c "import time;print(round(time.monotonic(),3))"; }
"$HOST" --headless --socket wl-host --duration 120 > "$LOG/host.log" 2>&1 &
HP=$!
i=0; while [ ! -S /tmp/rt/wl-host ] && [ $i -lt 100 ]; do sleep 0.05; i=$((i+1)); done
T0=$(ts); echo "t0 $T0" > "$LOG/times"
WAYLAND_DISPLAY=wl-host WAYLAND_DEBUG=client "$BIN" --socket wl-nested --duration "$DUR" --width 640 --height 400 \
  --exec "env -u WAYLAND_DEBUG $MPV av://lavfi:testsrc=size=320x240:rate=60" > "$LOG/nested.log" 2>&1 &
NP=$!
sleep "$T_HIDE"; echo "hide $(ts)" >> "$LOG/times"
WAYLAND_DISPLAY=wl-host $MPV --fs "av://lavfi:testsrc=size=320x240:rate=30" > "$LOG/hider.log" 2>&1 &
MP=$!
if [ -n "${T_TERM:-}" ]; then
  sleep $(python3 -c "print($T_TERM-$T_HIDE)"); echo "term $(ts)" >> "$LOG/times"; kill -TERM $NP
fi
if [ -n "${T_REVEAL:-}" ]; then
  sleep $(python3 -c "print($T_REVEAL-$T_HIDE)"); echo "reveal $(ts)" >> "$LOG/times"; kill $MP
fi
wait $NP; echo "nested_exit $? $(ts)" >> "$LOG/times"
kill $MP $HP 2>/dev/null; wait 2>/dev/null
