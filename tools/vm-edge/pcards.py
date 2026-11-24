import sys, os, re, glob
exec(open("/home/tester/g/feat.py").read().split("which = sys.argv[1:]")[0])
# Replay in the Quick settings card of the edge from apps (live/edge-apps/src/cards/quick.rs), with
# real input: its switch turns the buffer on, Save a clip writes a clip. (Network's list in place is
# in papps.py and pfluid.py.) Its first part is the helpers pmic.py runs first.
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
open(CFG, "a").write('PANEL_CONTROL="SUPER+A top right 380 settings/quick=replay"\n')
# the box dump's hit lines (hit OWNER/KIND ID x y w h); Quick settings: row 10 + i, its switch 30 + i,
# Save a clip 50 (quick.rs)
def hits():
    wait(0.3)
    out = {}
    for l in open(BOX):
        w = l.split()
        if w and w[0] == "hit": out[(w[1], int(w[2]))] = list(map(float, w[3:7]))
    return out
def mid(r): return r[0] + r[2] / 2, r[1] + r[3] / 2
def closed(): wait(0.4); return open(BOX).read().startswith("closed")
restart_edge()
chord("super+a"); wait(1.5)
h = hits(); shot("k-1-replay-off")
print("hits:", sorted(k for k in h if k[0] == "settings/quick"), flush=True)
check("the Replay switch is in the panel, no Save a clip while it is off", ("settings/quick", 30) in h and ("settings/quick", 50) not in h, str(sorted(h)))
if ("settings/quick", 30) in h: click_at(*mid(h[("settings/quick", 30)]))
wait(4.0); h = hits(); shot("k-2-replay-on")
on = "true" in sh(f"grep '^ENABLE_REPLAY_BUFFER=' {CFG}")
check("the switch turns the buffer on", on, sh(f"grep '^ENABLE_REPLAY_BUFFER=' {CFG}"))
check("the panel stays open, Save a clip under the switch", not closed() and ("settings/quick", 50) in h, str(sorted(k for k in h if k[0] == "settings/quick")))
before = set(glob.glob(CLIPS + "/*"))
wait(20.0)  # let the buffer hold some seconds
h = hits()
if ("settings/quick", 50) in h:
    click_at(*mid(h[("settings/quick", 50)]))
    for _ in range(30):
        wait(1.0)
        if set(glob.glob(CLIPS + "/*")) - before: break
new = sorted(set(glob.glob(CLIPS + "/*")) - before)
check("Save a clip writes a clip into Videos/Clips", bool(new), str(new) + " " + sh("tail -3 /tmp/infynitae-recorder.log 2>/dev/null"))
chord("esc")
check("Escape closes the panel", closed())
sh(f"sed -i '/^PANEL_CONTROL=/d; /^ENABLE_REPLAY_BUFFER=/d' {CFG}; echo 'ENABLE_REPLAY_BUFFER=\"false\"' >> {CFG}; infynitae-reload recorder >/dev/null 2>&1; true")
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok: print("  failed:", n)
