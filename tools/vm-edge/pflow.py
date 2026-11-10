import sys, os, re, json
exec(open("/home/tester/g/feat.py").read().split("which = sys.argv[1:]")[0])
# The panel edit mode of 2026-11-10 (live/edge-rs/src/modules/flow.rs), with real input.
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
def click_at(x, y, b="left"): at(x, y); click(b); wait(0.6)
def drag(x, y, dx, dy):
    at(x, y); emit((1, BTN["left"], 1)); wait(0.15)
    move(int(dx / 2), int(dy / 2), 30); wait(0.2); move(int(dx / 2), int(dy / 2), 30); wait(0.3)
    emit((1, BTN["left"], 0)); wait(0.8)
def restart_edge():
    sh("pkill -x infynitae-edge; true"); wait(1.0); run_bg(f"INFYNITAE_EDGE_BOXES={BOX} infynitae-edge"); wait(2.0)
# the bar's buttons: Undo Reset Add - + Hover Cancel Done, weighted as flow::BAR_BUTTONS
W = [("undo", 1.0), ("reset", 1.0), ("add", 0.9), ("narrower", 0.55), ("wider", 0.55), ("hover", 1.15), ("cancel", 1.1), ("done", 1.0)]
def bar_button(name):
    b = boxes()["bar"]; inner = b[2] - 10 - 4 * (len(W) - 1); tot = sum(u for _, u in W); x = b[0] + 5
    for k, u in W:
        w = inner * u / tot
        if k == name: return x + w / 2, b[1] + 5 + 13
        x += w + 4
# a card's bar: right to left Remove, Up (not on the first), Down (not on the last), Half, Look
def card_button(r, name, first=False, last=False):
    sx, sw = r[0] + 5, r[2] - 10; y = r[1] + 4 + 17
    order = [("remove", 24)] + ([] if first else [("up", 24)]) + ([] if last else [("down", 24)]) + [("half", 40)]
    x = sx + sw - 1
    for k, w in order:
        x -= w
        if k == name: return x + w / 2, y
        x -= 4
def hover_card(r): at(r[0] + 30, r[1] + r[3] - 8)

sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}; rm -f {BACKUP}")
restart_edge()
chord("super+a"); wait(1.5)
d0 = boxes(); m0 = d0["modules"]
print("modules:", sorted(m0), "panel", d0["panel"], flush=True)
shot("f-0-open")
# 1. a right click on the power buttons' corner (they use no right click): editing
s = m0["session"]; click_at(s[0] + 4, s[1] + 4, "right"); d = boxes(); shot("f-1-editing")
check("a right click on an unused spot starts editing", d["state"] == "editing" and "bar" in d, d.get("state"))
wi, bt = d["modules"]["wifi"], d["modules"]["bluetooth"]
check("nothing frozen: the switches stay one card, two to a row", abs(wi[1] - bt[1]) < 1 and bt[0] > wi[0] and wi[2] == m0["wifi"][2], f"{wi} {bt}")
check("an empty module keeps a card while editing", "tray" in d["modules"] and d["modules"]["tray"][3] > 0, d["modules"].get("tray"))
# 2. a click outside the panel and its bar: still editing
click_at(200, 500); check("a click outside leaves editing alone", boxes()["state"] == "editing")
# 3. the clock's Down: under the sound card
c = boxes()["modules"]["clock"]; hover_card(c); shot("f-2-clock-bar")
click_at(*card_button(c, "down", first=True)); d = boxes()
check("Down puts the clock under the sound card", d["modules"]["clock"][1] > d["modules"]["volume"][1], f"clock {d['modules']['clock']} volume {d['modules']['volume']}")
# 4. Undo
click_at(*bar_button("undo")); d = boxes()
check("Undo puts it back", d["modules"]["clock"][1] < d["modules"]["volume"][1])
# 5. drag the sound card under the notifications
v = d["modules"]["volume"]; n = d["modules"]["notifications"] if "notifications" in d["modules"] else d["modules"]["session"]
drag(v[0] + v[2] / 2, v[1] + v[3] / 2, 0, (n[1] + n[3] - 4) - (v[1] + v[3] / 2)); d = boxes(); shot("f-3-dragged")
check("a dragged card lands where the line was", d["modules"]["volume"][1] > n[1], f"volume {d['modules']['volume']} notifications {n}")
click_at(*bar_button("undo"))
# 6. Half on the clock pairs it with the sound card
c = boxes()["modules"]["clock"]; hover_card(c); click_at(*card_button(c, "half", first=True)); d = boxes(); shot("f-4-halves")
cl, vo = d["modules"]["clock"], d["modules"]["volume"]
check("Half: the clock and the sound card side by side", abs(cl[1] - vo[1]) < 1 and cl[2] < d["panel"][2] / 2 and vo[0] > cl[0], f"{cl} {vo}")
# 7. Cancel puts it all back, nothing saved
click_at(*bar_button("cancel")); d = boxes()
check("Cancel: as before, nothing saved", d["state"] != "editing" and line() == "" and d["modules"]["clock"] == m0["clock"], line())
# 8. again: the clock down, then Done saves a line with no places
chord("esc"); wait(0.5); chord("super+a"); wait(1.2)
s = boxes()["modules"]["session"]; click_at(s[0] + 4, s[1] + 4, "right")
c = boxes()["modules"]["clock"]; hover_card(c); click_at(*card_button(c, "down", first=True))
click_at(*bar_button("narrower")); click_at(*bar_button("done")); wait(0.8); d = boxes(); shot("f-5-saved")
l = line()
check("Done saves the new order and width, no places", "volume clock" in l and not re.search(r"@\d", l) and " 360 " in l, l)
check("the line before it is in panels.backup", os.path.exists(BACKUP), open(BACKUP).read()[-200:] if os.path.exists(BACKUP) else "none")
sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}")
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok: print("  failed:", n)
