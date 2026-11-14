import os, sys, time
exec(open("/home/tester/g/everyday.py").read().split("names = sys.argv[1:]")[0])
# `infynitae msg overview` (review item 4): asks the running edge for `panel windows`, and falls
# back to MENU_PICKER / fuzzel / rofi / wofi when no edge handles it.
BIN = "/home/tester/.local/bin"
BOXES = "/tmp/edge-boxes"
sh(f"pkill -x infynitae-edge; pkill -f '^{BIN}/infynitae-edge'; true"); wait(0.8)
sh(f"rm -f {BOXES}"); run_bg(f"INFYNITAE_EDGE_BOXES={BOXES} {BIN}/infynitae-edge-apps"); wait(2.0)
sh(f"rm -f {BOXES}")
# with the session's runtime dir and display (the harness's sh runs as the session user)
def as_user(cmd):
    open("/tmp/poverview-cmd.sh", "w").write(cmd + "\n")
    return sh("XDG_RUNTIME_DIR=/run/user/1000 WAYLAND_DISPLAY=wayland-1 sh /tmp/poverview-cmd.sh")
rc_edge = int(as_user(f"{BIN}/infynitae msg overview >/dev/null 2>&1; echo $?").strip() or 1)
wait(0.8)
boxes = open(BOXES, errors="replace").read() if os.path.exists(BOXES) else ""
panel_shown = rc_edge == 0 and "panel windows" in boxes.lower()
chord("esc"); wait(0.5)

sh(f"pkill -f '^{BIN}/infynitae-edge'; true"); wait(0.8)
out_pick = "/tmp/overview-fallback.txt"
sh(f"rm -f {out_pick}")
open("/tmp/poverview-win.sh", "w").write("alacritty -T overview-win -e sleep 8\n")
run_bg("sh /tmp/poverview-win.sh"); wait(1.5)
rc_fb = int(as_user(f"MENU_PICKER='cat > {out_pick}' {BIN}/infynitae msg overview >/dev/null 2>&1; echo $?").strip() or 1)
picked_in = open(out_pick, errors="replace").read() if os.path.exists(out_pick) else ""
sh("pkill -f 'alacritty -T overview-win'; rm -f /tmp/poverview-*.sh " + out_pick + "; true")
run_bg(f"{BIN}/infynitae-edge")

print("PASS" if panel_shown else "FAIL", "overview opens edge windows panel", f"rc={rc_edge}", flush=True)
print("PASS" if (rc_fb == 0 and "[" in picked_in) else "FAIL", "overview reaches fallback picker without edge", repr(picked_in[:120]), flush=True)
