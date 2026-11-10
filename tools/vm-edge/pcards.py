import sys, os, re, glob
exec(open("/home/tester/g/feat.py").read().split("which = sys.argv[1:]")[0])
# The Network and Replay cards (2026-11-10), with real input.
CFG = "/home/tester/.config/infynitae/config"
BOX = "/tmp/edge-boxes"
CLIPS = "/home/tester/Videos/Clips"
def boxes():
    wait(0.3)
    d = {"modules": {}, "child": None}
    for l in open(BOX):
        w = l.split()
        if w[0] == "panel": d["panel"] = list(map(float, w[2:6])); d["state"] = w[6]
        elif w[0] == "module": d["modules"][w[1]] = list(map(float, w[2:6]))
        elif w[0] == "child": d["child"] = list(map(float, w[1:5]))
    return d
def at(x, y): move(-3000, -3000, 30); move(int(x), int(y), 30); wait(0.3)
def click_at(x, y, b="left"): at(x, y); click(b); wait(0.8)
def restart_edge():
    sh("pkill -x infynitae-edge; true"); wait(1.0); run_bg(f"INFYNITAE_EDGE_BOXES={BOX} infynitae-edge"); wait(2.0)
# a card's row k: PAD_Y 16 + title 22 + GAP 12, rows of 32
def row(c, k): return c[0] + 40, c[1] + 16 + 22 + 12 + 32 * k + 16

sh(f"sed -i '/^PANEL_CONTROL=/d; /^ENABLE_REPLAY_BUFFER=/d' {CFG}; echo 'ENABLE_REPLAY_BUFFER=\"false\"' >> {CFG}")
sh("infynitae-reload recorder >/dev/null 2>&1; true"); wait(2.0)
restart_edge()
chord("super+a"); wait(1.5)
d = boxes(); m = d["modules"]
print("modules:", sorted(m), flush=True)
net = m["wifi"]
click_at(net[0] + 40, net[1] + net[3] / 2); d = boxes(); shot("k-1-network")
check("a click on Network opens its card", d["child"] is not None, str(d["child"]))
nm = sh("nmcli -t -f TYPE,STATE device 2>/dev/null")
print("nmcli devices:", nm.replace("\n", " "), flush=True)
click_at(net[0] + 40, net[1] + net[3] / 2); d = boxes()
check("a second click closes it", d["child"] is None)
rp = m["replay"]
click_at(rp[0] + 40, rp[1] + rp[3] / 2); d = boxes(); c = d["child"]; shot("k-2-replay-off")
check("a click on Replay opens its card", c is not None, str(c))
# off: Open the clips folder, Keep the last minutes (row 1)
click_at(*row(c, 1)); wait(4.0); d = boxes(); shot("k-3-replay-on")
on = "true" in sh(f"grep '^ENABLE_REPLAY_BUFFER=' {CFG}")
check("Keep the last minutes turns the buffer on", on, sh(f"grep '^ENABLE_REPLAY_BUFFER=' {CFG}"))
check("the card stays open, Save a clip now first", d["child"] is not None and d["child"][3] > c[3], f"{c} -> {d['child']}")
before = set(glob.glob(CLIPS + "/*"))
wait(20.0)  # let the buffer hold some seconds
c = boxes()["child"]
if c:
    click_at(*row(c, 0));
    for _ in range(30):
        wait(1.0)
        if set(glob.glob(CLIPS + "/*")) - before: break
new = sorted(set(glob.glob(CLIPS + "/*")) - before)
check("Save a clip writes a clip into Videos/Clips", bool(new), str(new) + " " + sh("tail -3 /tmp/infynitae-recorder.log 2>/dev/null"))
# Escape closes the card first, then the panel
chord("esc"); wait(0.6)
check("Escape closes the card first, the panel stays", boxes()["child"] is None and boxes()["state"] != "closed")
chord("esc"); wait(0.6)
# right click opens it too
chord("super+a"); wait(1.5)
rp = boxes()["modules"]["replay"]; click_at(rp[0] + 40, rp[1] + rp[3] / 2, "right"); d = boxes()
check("a right click on Replay opens its card too", d["child"] is not None)
chord("esc"); wait(0.6)
sh(f"sed -i '/^ENABLE_REPLAY_BUFFER=/d' {CFG}; echo 'ENABLE_REPLAY_BUFFER=\"false\"' >> {CFG}; infynitae-reload recorder >/dev/null 2>&1; true")
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok: print("  failed:", n)
