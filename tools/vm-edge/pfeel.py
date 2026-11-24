import sys, os, re, time, glob
exec(open("/home/tester/g/feat.py").read().split("which = sys.argv[1:]")[0])
# Everyday use of the edge from apps, start to end, with real input: open it, use its cards, edit
# it, and how long each click takes to show. A screenshot at every step for a person to look at
# (results/u-*.png). Cards and their controls are read from the box dump's card and hit lines.
CFG = "/home/tester/.config/infynitae/config"
BOX = "/tmp/edge-boxes"
BACKUP = "/home/tester/.config/infynitae/panels.backup"
RT = "/run/user/1000"
SLOW = []
def line(name="CONTROL"):
    for l in open(CFG):
        if l.startswith(f"PANEL_{name}="): return l.strip()
    return ""
def boxes():
    wait(0.25)
    d = {"cards": {}, "order": [], "hits": {}, "cb": {}, "bb": {}, "add": None, "addrow": {}, "bar": None, "state": "closed"}
    for l in open(BOX):
        w = l.split()
        if not w: continue
        if w[0] == "panel": d["panel"] = list(map(float, w[2:6])); d["state"] = w[6]
        elif w[0] == "card": d["cards"][w[1]] = list(map(float, w[2:6])); d["order"].append(w[1])
        elif w[0] == "hit": d["hits"][(w[1], int(w[2]))] = list(map(float, w[3:7]))
        elif w[0] == "cardbutton": d["cb"][(w[1], w[2])] = list(map(float, w[3:7]))
        elif w[0] == "barbutton": d["bb"][w[1]] = list(map(float, w[2:6]))
        elif w[0] == "bar": d["bar"] = list(map(float, w[1:5]))
        elif w[0] == "add": d["add"] = list(map(float, w[1:5]))
        elif w[0] == "addrow": d["addrow"][w[1]] = list(map(float, w[2:6]))
    return d
def at(x, y): move(-3000, -3000, 30); move(int(x), int(y), 30); wait(0.25)
def timed(name, act):
    """Does act, then how long until the edge has drawn something new (its box dump)."""
    mt = lambda: os.path.getmtime(BOX) if os.path.exists(BOX) else 0
    m0 = mt(); t0 = time.time(); act()
    while time.time() - t0 < 2.0 and mt() == m0: time.sleep(0.01)
    ms = (time.time() - t0) * 1000
    if ms > 300: SLOW.append(f"{name} {ms:.0f} ms")
    return ms
def click_at(x, y, b="left", name="click"):
    at(x, y); return timed(name, lambda: click(b))
def drag(x, y, dx, dy, hold=0.3):
    at(x, y); emit((1, BTN["left"], 1)); wait(0.15)
    move(int(dx / 2), int(dy / 2), 30); wait(0.2); move(int(dx / 2), int(dy / 2), 30); wait(hold)
    emit((1, BTN["left"], 0)); wait(0.8)
def to_end():
    # the wheel over the panel scrolls it to its last card
    p = boxes()["panel"]; at(p[0] + p[2] / 2, p[1] + p[3] / 2)
    for _ in range(30): emit((2, 8, -1)); wait(0.02)
    wait(0.4)
def restart_edge():
    sh("pkill -x infynitae-edge; true"); wait(1.0); run_bg(f"INFYNITAE_EDGE_BOXES={BOX} infynitae-edge"); wait(2.0)
def middle(r): return r[0] + r[2] / 2, r[1] + r[3] / 2
def bar_button(name): return middle(boxes()["bb"][name])
def card_button(card, name):
    at(*middle(boxes()["cards"][card])); return middle(boxes()["cb"][(card, name)])
def start_edit(card):
    # a right click on a card's empty corner offers "Edit panel"; choosing it starts editing
    r = boxes()["cards"][card]; ms = click_at(r[0] + r[2] - 12, r[1] + r[3] - 8, "right", name="edit menu")
    for l in open(BOX):
        w = l.split()
        if w and w[0] == "editmenu":
            m = list(map(float, w[1:5])); ms += click_at(m[0] + 20, m[1] + m[3] / 2, name="Edit panel")
    return ms

sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}; rm -f {BACKUP}"); restart_edge()
sh("pkill -f fakempris; true"); run_bg("python3 /home/tester/g/fakempris.py"); wait(2.0)

# --- A. everyday use (the default control panel; hit ids from sound.rs, quick.rs, music.rs)
ms = timed("Super+A", lambda: chord("super+a")); wait(1.0); d = boxes(); c = d["cards"]; shot("u-01-open")
check(f"Super+A opens the panel ({ms:.0f} ms)", d["state"] == "locked" and "calendar/clock" in c, d.get("state"))
check("the music card is there while something plays", c.get("music/playing", [0, 0, 0, 0])[3] > 0, sorted(c))
sh("pactl set-sink-volume @DEFAULT_SINK@ 30%"); wait(1.0)
vo = boxes()["hits"][("sound/volume", 1)]; before = sh("pactl get-sink-volume @DEFAULT_SINK@ | grep -o '[0-9]*%' | head -1")
drag(vo[0] + 10, vo[1] + vo[3] / 2, vo[2] - 20, 0); after = sh("pactl get-sink-volume @DEFAULT_SINK@ | grep -o '[0-9]*%' | head -1"); shot("u-02-volume")
check("dragging the volume slider changes the volume", before != after, f"{before} -> {after}")
dn = boxes()["hits"][("settings/quick", 30)]; ms = click_at(*middle(dn), name="Do not disturb"); wait(0.5); shot("u-03-dnd")
check(f"Do not disturb switches ({ms:.0f} ms)", os.path.exists(f"{RT}/infynitae/do-not-disturb"))
click_at(*middle(boxes()["hits"][("settings/quick", 30)]), name="Do not disturb off")
sh("notify-send -a Test 'A message' 'It arrived while the panel was open'"); wait(1.5); shot("u-04-notification")
check("a notification shows in the panel while it is open", boxes()["cards"].get("notifications/list", [0, 0, 0, 0])[3] > 0)
pp = boxes()["hits"].get(("music/playing", 5)); prop = "busctl --user get-property org.mpris.MediaPlayer2.fakeplayer /org/mpris/MediaPlayer2 org.mpris.MediaPlayer2.Player PlaybackStatus 2>/dev/null"
t0 = sh(prop)
if pp: click_at(*middle(pp), name="play/pause")
t1 = sh(prop); shot("u-05-media")
check("the music card's middle button plays or pauses", pp is not None and t0 != t1, f"{t0} -> {t1}")
# since PR 134 a panel opened by its key stays open while the desktop around it is used
click_at(300, 900, name="outside"); d = boxes()
check("a click outside leaves the panel open", d["state"] == "locked", d.get("state"))
chord("super+a"); wait(0.6)
check("its key closes it", boxes()["state"] == "closed", boxes()["state"])
chord("super+a"); wait(1.0); chord("esc"); wait(0.6)
check("Escape closes the panel", boxes()["state"] == "closed", boxes()["state"])

# --- B. editing
chord("super+a"); wait(1.2); d0 = boxes()
ms = start_edit("calendar/clock"); d = boxes(); shot("u-07-edit")
check(f"a right click and Edit panel start editing ({ms:.0f} ms)", d["state"] == "editing" and d["bar"] is not None)
ms = click_at(*bar_button("Add"), name="Add"); shot("u-08-add")
b = boxes(); a = b["add"]
check(f"Add opens its list beside the panel, the bar's buttons stay ({ms:.0f} ms)", a is not None and b["bar"] is not None and abs(b["bar"][1] - d["bar"][1]) < 1 and (a[0] + a[2] <= d["panel"][0] or a[0] >= d["panel"][0] + d["panel"][2]), f"{a} bar {d['bar']} -> {b['bar']}")
# Calendar is there already (its clock): its month is offered under it
if "calendar" in b["addrow"]: click_at(*middle(b["addrow"]["calendar"]), name="Calendar")
r = boxes()["addrow"].get("calendar/month")
if r: click_at(*middle(r), name="add the month")
d = boxes(); shot("u-09-calendar")
check("the month added at the end", d["order"][-1:] == ["calendar/month"], str(d["order"]))
click_at(*bar_button("Add"), name="close Add")
# the month dragged from the strip over it (as pedit.py) and held at the panel's top edge: the panel
# scrolls up while it is held there (drag_autoscroll), so it can go to the very top
d = boxes(); mo = d["cards"]["calendar/month"]; top_y = d["panel"][1] + 6
drag(mo[0] + 60, mo[1] - 14, 0, top_y - (mo[1] - 14), hold=2.5); d = boxes(); shot("u-10-dragged")
check("the month dragged to the top", d["order"].index("calendar/month") < d["order"].index("sound/volume"), str(d["order"]))
click_at(*card_button("calendar/clock", "Half"), name="Half"); d = boxes(); shot("u-11-half")
check("Half puts the clock beside its neighbour", d["cards"]["calendar/clock"][2] < d["panel"][2] / 2 + 1, str(d["cards"]["calendar/clock"]))
# Remove takes a card out (the notifications card, with the message from part A in it; the last
# cards are under the screen's bottom while editing on 1080 lines)
to_end(); click_at(*card_button("notifications/list", "Remove"), name="remove"); d = boxes(); shot("u-13-removed")
check("Remove takes the notifications card out", "notifications/list" not in d["order"], str(d["order"]))
click_at(*bar_button("Done"), name="Done"); wait(0.8); l = line(); shot("u-14-saved")
check("Done saves it, the month first, the clock half, no places", 0 <= l.find("calendar/month") < l.find("sound/volume") and re.search(r"calendar/clock\S*@half", l) and "notifications/list" not in l and not re.search(r"@\d", l), l)
chord("esc"); wait(0.5); chord("super+a"); wait(1.2); d = boxes(); shot("u-15-reopened")
check("reopened, it is as saved", "calendar/month" in d["order"] and d["order"].index("calendar/month") < d["order"].index("sound/volume"), str(d["order"]))
# Reset asks first, Cancel throws it away
start_edit("calendar/clock")
click_at(*bar_button("Reset"), name="Reset"); shot("u-16-sure")
check("Reset asks first", "calendar/month" in boxes()["order"])
click_at(*bar_button("Reset"), name="Reset sure"); d = boxes(); shot("u-17-reset")
check("the second press resets to the built-in panel", "calendar/month" not in d["order"], str(d["order"]))
click_at(*bar_button("Cancel"), name="Cancel"); wait(0.6)
check("Cancel throws the reset away: the saved line stays", line() == l, line())
chord("esc"); wait(0.5)
sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}"); sh("pkill -f fakempris; true")
print("slow (over 300 ms):", SLOW or "none", flush=True)
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok: print("  failed:", n)
