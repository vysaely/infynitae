import sys, os, re, json
exec(open("/home/tester/g/feat.py").read().split("which = sys.argv[1:]")[0])
CFG = "/home/tester/.config/infynitae/config"
BOX = "/tmp/edge-boxes"
BACKUP = "/home/tester/.config/infynitae/panels.backup"
def line(name="CONTROL"):
    for l in open(CFG):
        if l.startswith(f"PANEL_{name}="): return l.strip()
    return ""
def boxes():
    wait(0.3)
    d = {"modules": {}}
    for l in open(BOX):
        w = l.split()
        if w[0] == "panel": d["panel"] = list(map(float, w[2:6])); d["state"] = w[6]
        elif w[0] == "module": d["modules"][w[1]] = list(map(float, w[2:6]))
        elif w[0] == "bar": d["bar"] = list(map(float, w[1:5]))
    return d
def at(x, y): move(-3000, -3000, 30); move(int(x), int(y), 30); wait(0.3)
def drag(x, y, dx, dy, b="left", sup=False):
    at(x, y)
    if sup: emit((1, KEYS["super"], 1)); wait(0.1)
    emit((1, BTN[b], 1)); wait(0.15)
    move(int(dx), int(dy), 30); wait(0.3)
    emit((1, BTN[b], 0)); wait(0.2)
    if sup: emit((1, KEYS["super"], 0))
    wait(0.6)
def centre(r): return r[0] + r[2] / 2, r[1] + r[3] / 2
def overlap(a, b): return a[0] < b[0] + b[2] and b[0] < a[0] + a[2] and a[1] < b[1] + b[3] and b[1] < a[1] + a[3]
def restart_edge():
    sh("pkill -x infynitae-edge; true"); wait(1.0); run_bg(f"INFYNITAE_EDGE_BOXES={BOX} infynitae-edge"); wait(2.0)
def bar_button(i, n=6):
    b = boxes()["bar"]; w = (b[2] - 10 - (n - 1) * 4) / n
    return b[0] + 5 + i * (w + 4) + w / 2, b[1] + 5 + 12
def click_at(x, y, b="left"): at(x, y); click(b); wait(0.6)
def background(d):
    # a point of the panel no module covers (3 px inside its left edge, the gap nearest its middle)
    p = d["panel"]; x = p[0] + 3; mid = p[1] + p[3] / 2
    free = [y for y in range(int(p[1]) + 2, int(p[1] + p[3]) - 2)
            if not any(m[0] <= x <= m[0] + m[2] and m[1] <= y <= m[1] + m[3] for m in d["modules"].values())]
    return x, min(free, key=lambda y: abs(y - mid)) if free else mid

sh(f"sed -i '/^PANEL_CONTROL=/d; /^PANELS_SNAP=/d; /^PANELS_OVERLAP=/d' {CFG}; rm -f {BACKUP}")
restart_edge()
chord("super+a"); wait(1.5)
d0 = boxes(); m0 = d0["modules"]
print("modules:", sorted(m0), "panel", d0["panel"], flush=True)
shot("c-0-open")
# 1. locked: dragging a module, with or without Super, changes nothing
wx, wy = centre(m0["wifi"])
drag(wx, wy, 0, 120); drag(wx, wy, 0, 120, sup=True)
check("locked: drags move nothing", boxes()["modules"]["wifi"] == m0["wifi"] and line() == "", line())
chord("esc"); wait(0.5); chord("super+a"); wait(1.2)
# 2. right click on the background: editing
bx, by = background(boxes()); click_at(bx, by, "right"); d = boxes(); shot("c-1-editing")
check("right click on the background starts editing", d["state"] == "editing" and "bar" in d)
# 3. a click on a module in edit mode changes nothing
click_at(wx, wy); check("a click alone moves nothing", boxes()["modules"]["wifi"] == m0["wifi"])
# 4. move Wi-Fi down 300 px: the panel grows
drag(wx, wy, 0, 300); d = boxes(); shot("c-2-moved")
w1 = d["modules"]["wifi"]
check("Wi-Fi moved down (the panel grows with it)", w1[1] > m0["wifi"][1] + 250 and d["panel"][3] > d0["panel"][3], f"{m0['wifi']} -> {w1}, panel {d0['panel'][3]} -> {d['panel'][3]}")
# 5. overlap off: Bluetooth let go on Wi-Fi goes to a free spot
bt = d["modules"]["bluetooth"]; bcx, bcy = centre(bt); wcx, wcy = centre(w1)
drag(bcx, bcy, wcx - bcx + 10, wcy - bcy + 5); d = boxes(); shot("c-3-overlap-off")
check("overlap off: let go over Wi-Fi, Bluetooth goes beside it", not overlap(d["modules"]["bluetooth"], d["modules"]["wifi"]), f"{d['modules']['bluetooth']} {d['modules']['wifi']}")
# 6. overlap on, then Undo
click_at(*bar_button(3)); dn = boxes()["modules"]["dnd"]; dcx, dcy = centre(dn); w1 = boxes()["modules"]["wifi"]; wcx, wcy = centre(w1)
drag(dcx, dcy, wcx - dcx + 10, wcy - dcy + 5); d = boxes(); shot("c-4-overlap-on")
check("overlap on: Do not disturb may cover Wi-Fi", overlap(d["modules"]["dnd"], d["modules"]["wifi"]), f"{d['modules']['dnd']}")
click_at(*bar_button(0)); d = boxes()
check("Undo puts it back", d["modules"]["dnd"] == dn, f"{dn} -> {d['modules']['dnd']}")
click_at(*bar_button(3))  # overlap off again
# 7. resize the clock: right drag
ck = boxes()["modules"]["clock"]; ccx, ccy = centre(ck)
drag(ccx, ccy, -150, 70, "right"); d = boxes(); shot("c-5-clock-resized")
c1 = d["modules"]["clock"]
check("right drag resizes the clock (narrower; not taller, Volume is right under it)", c1[2] < ck[2] - 100 and c1[3] == ck[3], f"{ck} -> {c1}")
# with room under it (Volume moved away first), it grows taller too
vo = boxes()["modules"]["volume"]; low = max(r[1] + r[3] for r in boxes()["modules"].values())
drag(*centre(vo), 0, low + 30 - vo[1]); ck2 = boxes()["modules"]["clock"]
drag(ck2[0] + ck2[2] - 30, ck2[1] + ck2[3] / 2, 0, 70, "right"); c2 = boxes()["modules"]["clock"]; shot("c-5b-clock-taller")
check("with room under it the clock grows taller and shows the time large", c2[3] > ck2[3] + 50 and c2[1] == ck2[1], f"{ck2} -> {c2}")
c1 = c2; d = boxes()
check("resizing keeps it in its place (overlap off: it stops at its neighbours)", c1[0] == ck2[0] and c1[1] == ck2[1] and not any(overlap(c1, r) for k, r in d["modules"].items() if k != "clock"), f"{ck} -> {c1}")
# 8. snapping: Night light let go 5 px right of Wi-Fi's left edge lines up with it
d = boxes(); nl = d["modules"]["nightlight"]; w1 = d["modules"]["wifi"]
lowest = max(r[1] + r[3] for r in d["modules"].values())
pl = d["panel"][0] + 8
tx, ty = pl + 5, lowest + 20
drag(*centre(nl), tx - nl[0], ty - nl[1]); d = boxes(); shot("c-6-snapped")
check("snapping lines Night light up with the panel's edge", d["modules"]["nightlight"][0] == pl, f"{d['modules']['nightlight']} vs {pl}")
# 9. Remove Replay, Undo brings it back
rp = boxes()["modules"]["replay"]
click_at(rp[0] + rp[2] - 3 - 29, rp[1] + 3 + 9, "right"); check("a right click where Remove would be removes nothing", "replay" in boxes()["modules"])
click_at(rp[0] + rp[2] - 3 - 29, rp[1] + 3 + 9); check("hovering or a first click shows no Remove: nothing removed", "replay" in boxes()["modules"]); shot("c-7a-selected")
click_at(rp[0] + rp[2] - 3 - 29, rp[1] + 3 + 9); d = boxes(); shot("c-7-removed")
check("Remove takes Replay out", "replay" not in d["modules"])
click_at(*bar_button(0)); check("Undo brings Replay back", "replay" in boxes()["modules"])
# 10. Add: Search
click_at(*bar_button(4)); b = boxes()["bar"]; shot("c-8-add-list")
w4 = (b[2] - 10 - 12) / 4
click_at(b[0] + 5 + w4 / 2, b[1] + 34 + 4 + 13); d = boxes(); shot("c-9-added")
check("Add puts Search in the panel", "search" in d["modules"], str(sorted(d["modules"])))
click_at(*bar_button(4))
# 11. resize the panel: right drag on its background, 100 px left (it is held at the right)
p0 = boxes()["panel"]; drag(*background(boxes()), -100, 0, "right"); p1 = boxes()["panel"]; shot("c-10-panel-wider")
check("right drag on the background makes the panel wider", p1[2] > p0[2] + 80, f"{p0} -> {p1}")
# 12. move the panel to the bottom left and back
drag(*background(boxes()), -1500, 900); d = boxes(); shot("c-11-panel-moved")
check("left drag on the background moves the panel to the bottom left", d["panel"][0] < 100 and d["panel"][1] + d["panel"][3] > 1000, str(d["panel"]))
drag(*background(boxes()), 1500, -900); d = boxes()
check("and back to the top right", d["panel"][1] < 100 and d["panel"][0] + d["panel"][2] > 1800, str(d["panel"]))
# 13. Done saves; the line before is in the backup
before_done = boxes()["modules"]
click_at(*bar_button(5)); d = boxes(); shot("c-12-done")
l = line()
check("Done saves the places", d["state"] == "locked" and "@" in l, l[:160])
check("the line before is kept in panels.backup", os.path.exists(BACKUP) and "panel_control=" in open(BACKUP).read().lower(), open(BACKUP).read()[:200] if os.path.exists(BACKUP) else "none")
# 14. locked again
w2 = boxes()["modules"]["wifi"]; drag(*centre(w2), 0, -100)
check("after Done it is locked again", boxes()["modules"]["wifi"] == w2)
# 15. after a restart, the same places
chord("esc"); wait(0.5); restart_edge(); chord("super+a"); wait(1.5); d = boxes(); shot("c-13-after-restart")
same = all(d["modules"].get(k) == v for k, v in before_done.items() if k in d["modules"])
check("after a restart the panel is as it was left", same and d["state"] == "locked", str({k: (v, d['modules'].get(k)) for k, v in before_done.items() if d['modules'].get(k) != v})[:300])
# 16. Escape saves too
click_at(*background(d), "right"); w3 = boxes()["modules"]["wifi"]
drag(w3[0] + w3[2] - 20, w3[1] + w3[3] / 2, -30, 0, "right")  # narrower: works whatever the panel's height
chord("esc"); wait(0.8)
check("Escape ends editing and saves", boxes()["state"] == "locked" and line() != l, line()[:120])
# 17. Reset goes back to the default arrangement
click_at(*background(boxes()), "right"); click_at(*bar_button(1)); d = boxes(); shot("c-14-reset")
rel = lambda r, pnl: (round(r[0] - pnl[0]), round(r[1] - pnl[1]), r[2], r[3])
same = all(rel(d["modules"][k], d["panel"]) == rel(m0[k], d0["panel"]) for k in m0 if k in d["modules"])
check("Reset: back to the default arrangement", same, str([(k, m0[k], d['modules'].get(k)) for k in m0 if k not in d['modules'] or rel(d['modules'][k], d['panel']) != rel(m0[k], d0['panel'])][:4]))
click_at(*bar_button(5))
# 18. a click on a switch works again after editing (Do not disturb on, then off)
dn = boxes()["modules"]["dnd"]; click_at(*centre(dn)); wait(0.8); shot("c-15-dnd-click")
on = sh("makoctl mode 2>/dev/null")
click_at(*centre(dn)); wait(0.5)
check("after editing a click toggles a switch again", "do-not-disturb" in on, on)
chord("esc")
sh(f"sed -i '/^PANEL_CONTROL=/d; /^PANELS_SNAP=/d; /^PANELS_OVERLAP=/d' {CFG}"); restart_edge()
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok: print("  failed:", n)
