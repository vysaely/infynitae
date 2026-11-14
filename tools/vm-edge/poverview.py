import os, sys, time
exec(open("/home/tester/g/everyday.py").read().split("names = sys.argv[1:]")[0])
# `infynitae msg overview` (review item 4): asks the running edge for `panel windows`, and falls
# back to MENU_PICKER / fuzzel / rofi / wofi when no edge handles it.
BIN = "/home/tester/.local/bin"
BOXES = "/tmp/edge-boxes"
sh(f"pkill -x infynitae-edge; pkill -f '^{BIN}/infynitae-edge'; true"); wait(0.8)
sh(f"rm -f {BOXES}"); run_bg(f"INFYNITAE_EDGE_BOXES={BOXES} {BIN}/infynitae-edge-apps"); wait(2.0)
sh(f"rm -f {BOXES}")
rc_edge = int(sh(f"{BIN}/infynitae msg overview >/dev/null 2>&1; echo $?").strip() or 1)
wait(0.8)
boxes = open(BOXES, errors="replace").read() if os.path.exists(BOXES) else ""
panel_shown = rc_edge == 0 and "panel windows" in boxes.lower()
chord("escape"); wait(0.5)

sh(f"pkill -f '^{BIN}/infynitae-edge'; true"); wait(0.8)
out_pick = "/tmp/overview-fallback.txt"
sh(f"rm -f {out_pick}")
run_bg("xterm -title overview-win -e sh -c 'sleep 6'"); wait(1.2)
rc_fb = int(sh(f"MENU_PICKER='cat > {out_pick}' {BIN}/infynitae msg overview >/dev/null 2>&1; echo $?").strip() or 1)
picked_in = open(out_pick, errors="replace").read() if os.path.exists(out_pick) else ""
sh("pkill -f 'xterm -title overview-win'; rm -f " + out_pick + "; true")
run_bg(f"{BIN}/infynitae-edge")

print("PASS" if panel_shown else "FAIL", "overview opens edge windows panel", f"rc={rc_edge}", flush=True)
print("PASS" if (rc_fb == 0 and "[" in picked_in) else "FAIL", "overview reaches fallback picker without edge", repr(picked_in[:120]), flush=True)
