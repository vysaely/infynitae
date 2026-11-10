# Desktop widgets (edge stage 5) with real input in the VM. Needs the Rust edge as infynitae-edge.
import sys, os, json
exec(open("/home/tester/g/feat.py").read().split("which = sys.argv[1:]")[0])
CFG = "/home/tester/.config/infynitae/config"
BOX = "/tmp/edge-boxes"
WBOX = BOX + ".widgets"
BACKUP = "/home/tester/.config/infynitae/panels.backup"
RES = []
def check(name, ok, detail=""):
    RES.append((name, ok)); print(("PASS " if ok else "FAIL ") + name + (" " + str(detail) if detail else ""), flush=True)
def wline():
    for l in open(CFG):
        if l.startswith("WIDGETS="): return l.strip()
    return ""
def wboxes():
    wait(0.4)
    d = {"modules": {}, "state": None}
    if not os.path.exists(WBOX): return d
    for l in open(WBOX):
        w = l.split()
        if not w: continue
        if w[0] == "widgets": d["size"] = list(map(float, w[1:3])); d["state"] = w[3]
        elif w[0] == "module": d["modules"][w[1]] = list(map(float, w[2:6]))
        elif w[0] == "bar": d["bar"] = list(map(float, w[1:5]))
    return d
def at(x, y): move(-3000, -3000, 30); move(int(x), int(y), 30); wait(0.3)
def drag(x, y, dx, dy, b="left"):
    at(x, y); emit((1, BTN[b], 1)); wait(0.15); move(int(dx), int(dy), 30); wait(0.3); emit((1, BTN[b], 0)); wait(0.8)
def click_at(x, y, b="left"): at(x, y); click(b); wait(0.6)
def centre(r): return r[0] + r[2] / 2, r[1] + r[3] / 2
def bar_button(i, n=6):
    b = wboxes()["bar"]; w = (b[2] - 10 - (n - 1) * 4) / n
    return b[0] + 5 + i * (w + 4) + w / 2, b[1] + 5 + 12
def restart_edge():
    sh("pkill -x infynitae-edge; true"); wait(1.0); sh(f"rm -f {WBOX}")
    run_bg(f"INFYNITAE_EDGE_BOXES={BOX} /home/tester/.local/bin/infynitae-edge"); wait(2.5)

sh("pkill -x melodyae; pkill -x melodyae-new; pkill -x pcmanfm-qt; pkill -x alacritty; true"); wait(1)
sh(f"sed -i '/^WIDGETS=/d' {CFG}; rm -f {BACKUP}")
sh(f"echo 'WIDGETS=\"clock:big@60,60,360x150 calendar@60,230,360x330 media@460,60,380x234\"' >> {CFG}")
restart_edge()
d = wboxes(); shot("w-0-shown")
check("the widgets are on the desktop where the line puts them",
      d["modules"].get("clock", [None])[:2] == [60.0, 60.0] and "calendar" in d["modules"] and d["state"] == "locked", d)
# a right click on the empty desktop is not the widgets' (the input region is the widgets only:
# unit-tested in the edge; here: it starts no editing)
click_at(1500, 900, "right")
check("a right click on the empty desktop starts no editing", wboxes()["state"] == "locked")
# a click on the calendar's next-month arrow does what it does in a panel (nothing breaks, still locked)
c = wboxes()["modules"]["calendar"]; click_at(c[0] + c[2] - 30, c[1] + 30); wait(0.3)
check("a click on a widget acts on it, the widgets stay locked", wboxes()["state"] == "locked")
# right click on a widget: editing
click_at(*centre(wboxes()["modules"]["clock"]), "right"); d = wboxes(); shot("w-1-editing")
check("right click on a widget starts editing", d["state"] == "editing" and "bar" in d, d.get("state"))
# move the clock 400 px right
ck = d["modules"]["clock"]; drag(*centre(ck), 0, 0)  # select
drag(*centre(ck), 400, 300); d = wboxes(); shot("w-2-moved")
c2 = d["modules"]["clock"]
check("the clock moved", c2[0] > ck[0] + 300 and c2[1] > ck[1] + 200, f"{ck} -> {c2}")
# Done saves the WIDGETS line
click_at(*bar_button(5)); d = wboxes(); l = wline(); shot("w-3-done")
check("Done saves the places", d["state"] == "locked" and f"clock:big@{int(c2[0])},{int(c2[1])}" in l.replace(" ", " "), l)
check("the line before is kept in panels.backup", os.path.exists(BACKUP) and "WIDGETS" in open(BACKUP).read())
# after a restart, the same places
before = d["modules"]; restart_edge(); d = wboxes()
check("after a restart the widgets are where they were left", d["modules"] == before, {k: (v, d['modules'].get(k)) for k, v in before.items() if d['modules'].get(k) != v})
# the command starts editing; Escape ends it
run_bg("/home/tester/.local/bin/infynitae-edge widgets"); out = ""; wait(1.5)
st = wboxes()["state"]
check("infynitae-edge widgets starts editing", st == "editing", f"{st} {out}")
chord("esc"); wait(0.8)
check("Escape ends editing", wboxes()["state"] == "locked")
# a panel still opens and works with widgets on the desktop
chord("super+a"); wait(1.5)
check("Super+A still opens the panel", os.path.exists(BOX) and "panel" in open(BOX).read())
chord("esc"); wait(0.5)
# no widgets: no surface (zero cost)
sh(f"sed -i '/^WIDGETS=/d' {CFG}"); restart_edge()
check("no WIDGETS line: no widgets drawn", not wboxes()["modules"])
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok: print("  failed:", n)
