import sys, os, time
exec(open("/home/tester/pflow.py").read().split("sh(f\"sed -i '/^PANEL_CONTROL=/d' {CFG}; rm -f {BACKUP}\")")[0])
# Editing a panel of the edge from apps (live/edge-apps/src/edit.rs) with real input in the VM: a
# right click starts it, a card's buttons move and remove it, Undo, Add a card adds one, Done saves the
# PANEL_ line (the old one in panels.backup), Cancel puts it back, Escape saves, a drag moves a card.
BIN = "/home/tester/.local/bin"
EDGE = f"{BIN}/infynitae-edge-apps"
P = f"^{BIN}/infynitae-edge"
START = "SUPER+A top right 380 calendar/clock sound/volume settings/power notifications/list settings/session"
def apps_edge():
    sh(f"pkill -x infynitae-edge; pkill -f '{P}-rs'; pkill -f '{P}-apps'; true"); wait(0.8)
    run_bg(f"INFYNITAE_EDGE_BOXES={BOX} {EDGE}"); wait(2.0)
def dump():
    wait(0.35)
    d = {"cards": [], "cb": {}, "bb": {}, "add": [], "state": None}
    for l in open(BOX):
        w = l.split()
        if not w: continue
        if w[0] == "panel": d["state"] = w[6]; d["panel"] = list(map(float, w[2:6]))
        elif w[0] == "card": d["cards"].append((w[1], list(map(float, w[2:6]))))
        elif w[0] == "cardbutton": d["cb"][(w[1], w[2])] = list(map(float, w[3:7]))
        elif w[0] == "barbutton": d["bb"][w[1]] = list(map(float, w[2:6]))
        elif w[0] == "addrow": d["add"].append((w[1], list(map(float, w[2:6]))))
    return d
def order(): return [c for c, _ in dump()["cards"]]
def mid(r): return r[0] + r[2] / 2, r[1] + r[3] / 2
def press(d, key):
    r = d["cb"].get(key)
    if r: click_at(*mid(r))
    return r is not None
def bar(name):
    r = dump()["bb"].get(name)
    if r: click_at(*mid(r))
    return r is not None

sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}; rm -f {BACKUP}")
open(CFG, "a").write(f'PANEL_CONTROL="{START}"\n')
apps_edge(); at(300, 300); wait(0.4); shot("z-base")

# 1. a right click on a card's empty spot starts editing
run_bg(f"{EDGE} control"); wait(1.5)
d = dump(); first = d["cards"][0][1]
click_at(first[0] + first[2] - 12, first[1] + first[3] - 8, "right"); d = dump(); shot("ed-1")
check("a right click starts editing", d["state"] == "editing", str(d["state"]))
check("the bar has Done, Cancel, Undo and Add", all(k in d["bb"] for k in ("Done", "Cancel", "Undo", "Add")), str(sorted(d["bb"])))

# 2. Down moves the first card one place; Undo puts it back
before = order()
r = d["cards"][0][1]; at(r[0] + 40, r[1] + 20); d = dump()
check("Down moves a card one place", press(d, (before[0], "Down")) and order()[1] == before[0], str(order()))
check("Undo puts it back", bar("Undo") and order() == before, str(order()))

# 3. Remove takes a card out
d = dump(); r = dict(d["cards"])["settings/power"]; at(r[0] + 40, r[1] + 20); d = dump()
check("Remove takes a card out", press(d, ("settings/power", "Remove")) and "settings/power" not in order(), str(order()))

# 4. Add a card: the list, a provider, its kind
bar("Add"); d = dump(); shot("ed-2-add")
owners = [o for o, _ in d["add"]]
check("Add a card lists what can be added", len(owners) >= 3 and "settings" in owners, str(owners[:12]))
check("cards already there are not offered", "notifications/list" not in owners, "")
if "settings" in owners:
    click_at(*mid(dict(d["add"])["settings"])); d = dump()
    kinds = [o for o, _ in d["add"] if o.startswith("settings/")]
    check("choosing Settings shows its kinds", "settings/power" in kinds, str(kinds))
    if "settings/power" in kinds:
        click_at(*mid(dict(d["add"])["settings/power"]))
check("adding puts the card in the panel", "settings/power" in order(), str(order()))

# 5. Done saves the line, the old one goes to panels.backup
bar("Done"); wait(0.6)
line_now = [l for l in open(CFG) if l.startswith("PANEL_CONTROL=")]
check("Done ends editing", dump()["state"] != "editing")
check("Done writes the panel's line", line_now and line_now[0].strip() != f'PANEL_CONTROL="{START}"', str(line_now))
# (the backup holds the line as the edge writes it: the same cards, "hover" spelled out)
bak = open(BACKUP).read() if os.path.exists(BACKUP) else ""
check("the line from before is in panels.backup", "calendar/clock sound/volume settings/power notifications/list" in bak, bak[-200:])

# 6. Cancel puts it back
saved = line_now[0] if line_now else ""
chord("esc"); wait(0.4); run_bg(f"{EDGE} control"); wait(1.5)
d = dump(); r = d["cards"][0][1]; click_at(r[0] + r[2] - 12, r[1] + r[3] - 8, "right"); d = dump()
r = d["cards"][0][1]; at(r[0] + 40, r[1] + 20); d = dump(); press(d, (d["cards"][0][0], "Remove"))
bar("Cancel"); wait(0.6)
check("Cancel leaves the line as it was", [l for l in open(CFG) if l.startswith("PANEL_CONTROL=")] == [saved], "")

# 7. Escape saves; a drag moves a card down
d = dump()
if d["state"] != "editing":
    if d["state"] is None: run_bg(f"{EDGE} control"); wait(1.5); d = dump()
    r = d["cards"][0][1]; click_at(r[0] + r[2] - 12, r[1] + r[3] - 8, "right"); d = dump()
before = order(); a = dict(d["cards"])[before[0]]; b = dict(d["cards"])[before[2]]
drag(a[0] + 60, a[1] - 14, 0, (b[1] + b[3]) - a[1] + 4); shot("ed-3-drag")
check("a drag moves the card down", order().index(before[0]) >= 2, f"{before} -> {order()}")
moved = order()
chord("esc"); wait(0.6)
line = [l for l in open(CFG) if l.startswith("PANEL_CONTROL=")]
check("Escape saves the new order", line and line[0].index(moved[0]) < line[0].index(before[0]) if line else False, str(line))
check("the edge is still running", sh(f"pgrep -f '^{EDGE}$' | head -1") != "")

sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}; rm -f {BACKUP}; pkill -f '{P}-apps'; true"); restart_edge()
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok: print("  failed:", n)
