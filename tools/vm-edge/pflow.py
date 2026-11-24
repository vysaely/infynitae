import sys, os, re, json
exec(open("/home/tester/g/feat.py").read().split("which = sys.argv[1:]")[0])
# Editing the default control panel of the edge from apps (live/edge-apps/src/edit.rs) with real
# input: what pedit.py does not cover (a hidden card's place, a click outside, Half, the width, Cancel
# and Done on a panel that has no line of its own). Its first part, up to the line that removes
# PANEL_CONTROL, is the helpers other scripts here run first.
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
# where the edge drew each button (its box dump: barbutton NAME x y w h, cardbutton MODULE NAME x y w h)
def buttons():
    wait(0.3)
    out = {}
    for l in open(BOX):
        w = l.split()
        if w and w[0] == "barbutton": out[("bar", w[1].lower())] = list(map(float, w[2:6]))
        elif w and w[0] == "cardbutton": out[(w[1], w[2].lower())] = list(map(float, w[3:7]))
    return out
def middle(r): return r[0] + r[2] / 2, r[1] + r[3] / 2
def bar_button(name): return middle(buttons()[("bar", name)])
def card_button(module, name): return middle(buttons()[(module, name)])
def hover_card(r): at(r[0] + 30, r[1] + r[3] - 8)

sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}; rm -f {BACKUP}")
restart_edge()
# the box dump's card lines (card OWNER/KIND x y w h), in the panel's order
def dump():
    wait(0.35)
    d = {"cards": [], "cb": {}, "bb": {}, "state": None}
    for l in open(BOX):
        w = l.split()
        if not w: continue
        if w[0] == "panel": d["state"] = w[6]; d["panel"] = list(map(float, w[2:6]))
        elif w[0] == "card": d["cards"].append((w[1], list(map(float, w[2:6]))))
        elif w[0] == "cardbutton": d["cb"][(w[1], w[2])] = list(map(float, w[3:7]))
        elif w[0] == "barbutton": d["bb"][w[1]] = list(map(float, w[2:6]))
    return d
def order(): return [c for c, _ in dump()["cards"]]
def menu_rect():
    for l in open(BOX):
        w = l.split()
        if w and w[0] == "editmenu": return list(map(float, w[1:5]))
    return None
def start_edit(card):
    # a right click on a card's empty corner offers "Edit panel"; choosing it starts editing
    r = dict(dump()["cards"])[card]; click_at(r[0] + r[2] - 12, r[1] + r[3] - 8, "right"); wait(0.4)
    m = menu_rect()
    if m: click_at(m[0] + 20, m[1] + m[3] / 2)
    return dump()
def press(card, name):
    d = dump(); r = dict(d["cards"])[card]; hover_card(r); b = dump()["cb"].get((card, name))
    if b: click_at(*middle(b))
    return b is not None
def bar(name):
    r = dump()["bb"].get(name)
    if r: click_at(*middle(r))
    return r is not None

chord("super+a"); wait(1.5)
d0 = dump(); c0 = dict(d0["cards"])
print("cards:", [c for c, _ in d0["cards"]], "panel", d0.get("panel"), flush=True)
shot("f-0-open")
# 1. editing from the clock card's menu
d = start_edit("calendar/clock"); shot("f-1-editing")
check("a right click and Edit panel start editing", d["state"] == "editing" and "Done" in d["bb"], d["state"])
# a card with nothing to show (no notification came since the edge started) has no box until editing
nl, ne = c0.get("notifications/list"), dict(d["cards"]).get("notifications/list")
check("a hidden card keeps a place while editing", (nl is None or nl[3] == 0) and ne is not None and ne[3] > 0, f"{nl} -> {ne}")
# 2. a click outside the panel and its bar: still editing
click_at(200, 500); check("a click outside leaves editing alone", dump()["state"] == "editing")
# 3. the clock's Down: after the sound card
before = order()
check("Down puts the clock after the sound card", press("calendar/clock", "Down") and order().index("calendar/clock") > order().index("sound/volume"), str(order()))
# 4. Undo
check("Undo puts it back", bar("Undo") and order() == before, str(order()))
# 5. drag the sound card under the power card (from the strip over the card, as pedit.py; the
# cards after Power are under the screen's bottom while editing on 1080 lines)
d = dump(); v = dict(d["cards"])["sound/volume"]; n = dict(d["cards"])["settings/power"]
drag(v[0] + 60, v[1] - 14, 0, (n[1] + n[3]) - v[1] + 4); shot("f-3-dragged")
o = order()
check("a dragged card lands where the line was", o.index("sound/volume") > o.index("settings/power"), str(o))
bar("Undo")
# 6. Half on Network pairs it with Bluetooth, the next card that can be half (the panel scrolled back
# to its top first: the drag above may have scrolled it)
def to_top():
    p = dump()["panel"]; at(p[0] + p[2] / 2, p[1] + p[3] / 2)
    for _ in range(30): emit((2, 8, 1)); wait(0.02)
    wait(0.4)
to_top()
press("settings/network", "Half"); d = dump(); shot("f-4-halves")
nw, bt = dict(d["cards"])["settings/network"], dict(d["cards"])["settings/bluetooth"]
check("Half: Network and Bluetooth side by side", abs(nw[1] - bt[1]) < 1 and nw[2] < d["panel"][2] / 2 and bt[0] > nw[0], f"{nw} {bt}")
# 7. Cancel puts it all back, nothing saved
bar("Cancel"); d = dump()
check("Cancel: as before, nothing saved", d["state"] != "editing" and line() == "" and dict(d["cards"]).get("calendar/clock") == c0.get("calendar/clock"), line())
# 8. again: the clock down, one step narrower, then Done saves a line with no places
chord("esc"); wait(0.5); chord("super+a"); wait(1.2)
start_edit("calendar/clock"); press("calendar/clock", "Down")
bar("Narrower"); bar("Done"); wait(0.8); shot("f-5-saved")
l = line()
check("Done saves the new order and width, no places", re.search(r"sound/volume calendar/clock", l) and not re.search(r"@\d", l) and " 360 " in l, l)
check("the line before it is in panels.backup", os.path.exists(BACKUP), open(BACKUP).read()[-200:] if os.path.exists(BACKUP) else "none")
chord("esc"); wait(0.5)
sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}")
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok: print("  failed:", n)
