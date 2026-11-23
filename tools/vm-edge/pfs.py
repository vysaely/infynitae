import sys, os, time
exec(open("/home/tester/papps.py").read().split("sh(f\"sed -i '/^PANEL_CONTROL=/d' {CFG}\")")[0])
# 03b: under a fullscreen window the closed edge goes below the windows instead of unmapping.
# Toggle fullscreen 5 times; after each, the edge must still open by key, and the log counts
# null-buffer attaches (old way) and layer changes (new way).
# Run with a copy named infynitae-edge-apps under ~/fsx/<name>/ (control finds the edge by name;
# a path under /home/tester/edge-* would match the pkill below and end this script).
EDGE = sys.argv[1]
sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}")
open(CFG, "a").write('PANEL_CONTROL="SUPER+A bottom right 380 hover settings/network settings/bluetooth settings/power"\n')
sh("pkill -x infynitae-edge; pkill -f '/home/tester/[e]dge-'; pkill -f '^/home/tester/fs[x]/'; true"); wait(0.8)
run_bg(f"PATH={BIN}:$PATH WAYLAND_DEBUG=1 INFYNITAE_EDGE_BOXES={BOX} {EDGE} 2>/tmp/edge-fs.log"); wait(1.5)
close_all(); run_bg("alacritty"); until(lambda: has("alacritty")); wait(1.0)
log = "/home/tester/.local/state/infynitae/session.log"; n0 = len(open(log).readlines())
point_at("alacritty"); click(); wait(0.3)
ok = 0
for i in range(5):
    point_at("alacritty"); wait(0.3)
    chord("super+shift+enter"); wait(1.2)
    full = (els() or [{}])[0].get("size") == [1920, 1080]
    chord("super+shift+enter"); wait(1.2)
    run_bg(f"{EDGE} control"); wait(1.5)
    d = boxes(); opened = "panel" in d and d.get("state") != "closed"
    ok += full and opened
    if i == 4: shot("fs-after")
    chord("esc"); at(300, 300); wait(0.8)
check("fullscreen 5 times, then the panel opens by key each time", ok == 5, f"{ok}/5")
lines = open(log).readlines()[n0:]
print("slow turns:", sum("Slow turn" in l for l in lines), "| with layers above:", sum("Slow turn" in l and "layers above" in l for l in lines))
for l in lines:
    if "Slow turn" in l: print("  ", l.strip()[:160])
w = open("/tmp/edge-fs.log").read()
print("null attaches:", sum(w.count(k) for k in ("attach(nil", "attach(null", "attach(None", "attach(0")), "| set_layer:", w.count(".set_layer("))
close_all(); sh("pkill -f '^/home/tester/fs[x]/'; true")
