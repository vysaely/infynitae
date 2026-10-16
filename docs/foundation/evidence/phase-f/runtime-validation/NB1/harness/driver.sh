#!/bin/sh
unset WAYLAND_DEBUG
[ -n "${NB1_HANG:-}" ] && { echo "harness self-test: hanging" > $LOGDIR/driver.log; exec sleep 300; }
# The session command: runs every step against this session only (WAYLAND_DISPLAY, DISPLAY and
# XAUTHORITY are the ones the compositor gave it), then ends the session with SIGTERM.
L=$LOGDIR; P="python3 $APPS/gtkprobe.py"
exec > $L/driver.log 2>&1
step() { echo "=== $1 $(date +%T.%N | cut -c1-12)"; }
q() { timeout "$@"; echo "rc=$?"; }
step S0-env; env | grep -E '^(WAYLAND_DISPLAY|DISPLAY|XAUTHORITY|XDG_RUNTIME_DIR|DBUS_SESSION_BUS_ADDRESS|GDK_BACKEND|GSK_RENDERER|LIBGL)=' | sort
step S1-wl-copy-paste
printf 'wl text' | wl-copy --foreground & C=$!; sleep 1.5
echo "paste=[$(timeout 4 wl-paste --no-newline)] rc=$?"; echo "types=$(timeout 4 wl-paste --list-types | tr '\n' ' ')"; kill $C 2>/dev/null; wait $C 2>/dev/null
step S2-gtk3-copy-gtk4-paste
GDK_BACKEND=wayland $P 3 copy "from gtk3" & C=$!; sleep 2
GDK_BACKEND=wayland q 10 $P 4 paste; kill $C 2>/dev/null; wait $C 2>/dev/null
step S3-gtk4-copy-gtk3-paste
GDK_BACKEND=wayland $P 4 copy "from gtk4" & C=$!; sleep 2.5
GDK_BACKEND=wayland q 10 $P 3 paste; kill $C 2>/dev/null; wait $C 2>/dev/null
step S4-window-churn
GDK_BACKEND=wayland q 20 $P 3 windows 30
GDK_BACKEND=wayland q 20 $P 4 windows 30
step S5-fullscreen
GDK_BACKEND=wayland q 10 $P 3 fullscreen
GDK_BACKEND=wayland q 10 $P 4 fullscreen
step S6-popup
GDK_BACKEND=wayland q 10 $P 3 popup
GDK_BACKEND=wayland q 10 $P 4 popup
step S7-mpv-fullscreen-shm
q 10 mpv --no-config --really-quiet --vo=wlshm --ao=null --fs --length=2 'av://lavfi:testsrc=size=320x240:rate=30'
step S8-qt-kwrite
WAYLAND_DEBUG=1 QT_QPA_PLATFORM=wayland timeout 6 kwrite > $L/kwrite.wl 2>&1; echo "rc=$? attaches=$(grep -c 'wl_surface#[0-9]*.attach(wl_buffer' $L/kwrite.wl) toplevel=$(grep -c 'xdg_wm_base.*get_xdg_surface' $L/kwrite.wl)"
step S9-x11-auth
q 5 xdpyinfo | grep -E "^name of display|^version number|^vendor string"
echo "intruder without cookie:"; XAUTHORITY=/dev/null q 5 xdpyinfo 2>&1 | head -2
q 5 xmessage -timeout 2 "x11 through the satellite"
step S10-x11-clipboard-x11-to-wayland
GDK_BACKEND=x11 $P 3 copy "from x11" & C=$!; sleep 3
GDK_BACKEND=wayland q 10 $P 4 paste; kill $C 2>/dev/null; wait $C 2>/dev/null
step S11-wayland-to-x11
GDK_BACKEND=wayland $P 4 copy "to x11" & C=$!; sleep 2.5
GDK_BACKEND=x11 q 10 $P 3 paste; kill $C 2>/dev/null; wait $C 2>/dev/null
step S12-x11-fullscreen-popup
GDK_BACKEND=x11 q 10 $P 3 fullscreen
GDK_BACKEND=x11 q 10 $P 3 popup
step S13-satellite-restart
xmessage -timeout 30 "victim" & X=$!; sleep 2
S=$(pgrep -f "^/.*/xwayland-satellite :" | head -1); echo "satellite pid=$S"; kill -KILL $S; sleep 1.5
kill -0 $X 2>/dev/null && echo "x client still alive" || echo "x client ended with the satellite"
wait $X 2>/dev/null
q 8 xdpyinfo | grep "^name of display"; echo "new satellite pid=$(pgrep -f "^/.*/xwayland-satellite :" | head -1)"
step S14-shutdown
ps -eo pid,ppid,comm | sed 's/^/  /'
date +%s.%N > $L/term-sent
kill -TERM $PPID
exec sleep 10
