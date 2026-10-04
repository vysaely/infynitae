#!/bin/sh
# runs under the harness: WAYLAND_DISPLAY/XDG_RUNTIME_DIR point at the private harness socket
xwayland-satellite :97 > "$XDG_RUNTIME_DIR/sat.log" 2>&1 & S=$!
sleep 1.5
DISPLAY=:97 LIBGL_ALWAYS_SOFTWARE=1 kitty --config NONE -o linux_display_server=x11 -o confirm_os_window_close=0 --title X11KITTY sh -c "sleep 30" & K=$!
sleep 3
WID=$(DISPLAY=:97 xdotool search --name X11KITTY | head -1)
echo "[script] X11 window id: $WID"
echo "[script] X11 geometry before move:"; DISPLAY=:97 xdotool getwindowgeometry "$WID"
echo "[script] X11 client requests a move to 777,333 (xdotool windowmove)"
DISPLAY=:97 xdotool windowmove "$WID" 777 333
sleep 0.5
echo "[script] X11 geometry after move request:"; DISPLAY=:97 xdotool getwindowgeometry "$WID"
echo "[script] WM_CLASS: $(DISPLAY=:97 xprop -id "$WID" WM_CLASS 2>&1)"
sleep 0.5
echo "[script] killing xwayland-satellite (pid $S) to simulate a crash"
kill -9 $S
sleep 2
echo "[script] X11 kitty alive after satellite crash: $(kill -0 $K 2>/dev/null && echo yes || echo no)"
echo "[script] satellite log (filtered):"; grep -v "xkbcomp\|Warning: \|  Using \|Could not resolve\|Errors from" "$XDG_RUNTIME_DIR/sat.log" | head -20
kill $K 2>/dev/null; wait 2>/dev/null
