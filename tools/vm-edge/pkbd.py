import sys, os, time
exec(open("/home/tester/papps.py").read().split("sh(f\"sed -i '/^PANEL_CONTROL=/d' {CFG}\")")[0])
# An open panel holds the keyboard only on demand (PR 149). Open the panel by hover, then by its
# key's command; each time click inside it, click a terminal and type: all the text, its first key
# too, must reach the terminal. Run with the edge's path as argument; NOIN=1 skips the click inside.
EDGE = sys.argv[1] if len(sys.argv) > 1 else f"{BIN}/infynitae-edge-apps"
sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}")
open(CFG, "a").write('PANEL_CONTROL="SUPER+A bottom right 380 hover settings/network settings/bluetooth settings/power"\n')
sh("pkill -x infynitae-edge; pkill -f '[i]nfynitae-edge-apps'; pkill -f '/home/tester/[e]dge-'; true"); wait(0.8)
run_bg(f"PATH={BIN}:$PATH INFYNITAE_EDGE_BOXES={BOX} {EDGE}"); wait(1.5)
close_all(); sh("rm -f /tmp/kbd-*")
run_bg("alacritty"); until(lambda: has("alacritty")); wait(1.0)
point_at("alacritty"); click(); wait(0.4)
for name, how in (("hover", None), ("key", "control")):
    if how: run_bg(f"{EDGE} {how}"); wait(1.5)
    else:
        at(1700, 1079); wait(0.5); move(0, 5, 2); wait(1.5)
    d = boxes(); check(f"{name}: the panel opens", "panel" in d and d.get("state") != "closed", str(d.get("state")))
    if "panel" not in d: continue
    p = d["panel"]; (os.environ.get("NOIN") or click_at(p[0] + p[2] / 2, p[1] + 20)); wait(0.4)
    point_at("alacritty"); click(); wait(0.4)
    text(f"touch /tmp/kbd-{name}\n"); wait(1.0)
    check(f"{name}: typing after a click on the terminal reaches it", os.path.exists(f"/tmp/kbd-{name}"), "panel " + boxes().get("state", "?"))
    shot(f"kbd-{name}")
    chord("esc"); at(300, 300); wait(0.6)
close_all()
sh("pkill -f /home/tester/[e]dge-; true")
