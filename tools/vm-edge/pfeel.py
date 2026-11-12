import sys, os, re, time, glob
exec(open("/home/tester/g/feat.py").read().split("which = sys.argv[1:]")[0])
# Everyday use of the edge, start to end, with real input (2026-11-10): open it, use its
# modules, edit it, and how long each click takes to show. A screenshot at every step for a
# person to look at (results/u-*.png).
CFG = "/home/tester/.config/infynitae/config"
BOX = "/tmp/edge-boxes"
BACKUP = "/home/tester/.config/infynitae/panels.backup"
ALL = ["search", "favorites", "apps", "windows", "clipboard", "clock", "volume", "media", "wifi", "bluetooth", "dnd",
       "nightlight", "keyboard", "osk", "power", "replay", "tray", "notifications", "session", "calendar", "memory", "system"]
SLOW = []
def line(name="CONTROL"):
    for l in open(CFG):
        if l.startswith(f"PANEL_{name}="): return l.strip()
    return ""
def boxes():
    wait(0.25)
    d = {"modules": {}, "child": None, "bar": None}
    d["state"] = "closed"
    for l in open(BOX):
        w = l.split()
        if not w: continue
        if w[0] == "panel": d["panel"] = list(map(float, w[2:6])); d["state"] = w[6]
        elif w[0] == "module": d["modules"][w[1]] = list(map(float, w[2:6]))
        elif w[0] == "child": d["child"] = list(map(float, w[1:5]))
        elif w[0] == "bar": d["bar"] = list(map(float, w[1:5]))
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
def drag(x, y, dx, dy):
    at(x, y); emit((1, BTN["left"], 1)); wait(0.15)
    move(int(dx / 2), int(dy / 2), 30); wait(0.2); move(int(dx / 2), int(dy / 2), 30); wait(0.3)
    emit((1, BTN["left"], 0)); wait(0.8)
def restart_edge():
    sh("pkill -x infynitae-edge; true"); wait(1.0); run_bg(f"INFYNITAE_EDGE_BOXES={BOX} infynitae-edge"); wait(2.0)
# where the edge drew each button and Add row (its box dump: barbutton NAME x y w h, cardbutton MODULE
# NAME x y w h, addrow MODULE x y w h)
def drawn():
    wait(0.3)
    out = {}
    for l in open(BOX):
        w = l.split()
        if w and w[0] == "barbutton": out[("bar", w[1].lower())] = list(map(float, w[2:6]))
        elif w and w[0] == "cardbutton": out[(w[1], w[2].lower())] = list(map(float, w[3:7]))
        elif w and w[0] == "addrow": out[("add", w[1])] = list(map(float, w[2:6]))
        elif w and w[0] == "add": out[("add", "")] = list(map(float, w[1:5]))
    return out
def middle(r): return r[0] + r[2] / 2, r[1] + r[3] / 2
def bar_button(name): return middle(drawn()[("bar", name)])
def add_row(word, panel_words): return middle(drawn()[("add", word)])
def card_button(module, name): return middle(drawn()[(module, name)])
def hover_card(r): at(r[0] + 30, r[1] + r[3] - 8)
def words():
    l = sh("infynitae-edge --print-panels").splitlines()
    c = [x for x in l if x.startswith("control ")][0].split()
    return [w.split(":")[0].split("@")[0] for w in c if w.split(":")[0].split("@")[0] in ALL]

sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}; rm -f {BACKUP}"); restart_edge()
sh("pkill -f fakempris; true"); run_bg("python3 /home/tester/g/fakempris.py"); wait(2.0)

# --- A. everyday use
ms = timed("Super+A", lambda: chord("super+a")); wait(1.0); d = boxes(); m = d["modules"]; shot("u-01-open")
check(f"Super+A opens the panel ({ms:.0f} ms)", d["state"] in ("locked", "open", "hover") and "clock" in m, d.get("state"))
check("the music card is there while something plays", "media" in m, sorted(m))
sh("pactl set-sink-volume @DEFAULT_SINK@ 30%"); wait(1.0)
vo = m["volume"]; before = sh("pactl get-sink-volume @DEFAULT_SINK@ | grep -o '[0-9]*%' | head -1")
drag(vo[0] + 30, vo[1] + 38, vo[2] - 80, 0); after = sh("pactl get-sink-volume @DEFAULT_SINK@ | grep -o '[0-9]*%' | head -1"); shot("u-02-volume")
check("dragging the volume slider changes the volume", before != after, f"{before} -> {after}")
dn = m["dnd"]; ms = click_at(dn[0] + 40, dn[1] + dn[3] / 2, name="Do not disturb"); shot("u-03-dnd")
check(f"Do not disturb switches ({ms:.0f} ms)", "do-not-disturb" in sh("makoctl mode 2>/dev/null") or os.path.exists("/run/user/1000/infynitae/do-not-disturb"), sh("makoctl mode"))
click_at(dn[0] + 40, dn[1] + dn[3] / 2, name="Do not disturb off")
sh("notify-send -a Test 'A message' 'It arrived while the panel was open'"); wait(1.5); shot("u-04-notification")
check("a notification shows in the panel while it is open", "notifications" in boxes()["modules"])
md = boxes()["modules"]["media"]; t0 = sh("busctl --user get-property org.mpris.MediaPlayer2.fake /org/mpris/MediaPlayer2 org.mpris.MediaPlayer2.Player PlaybackStatus 2>/dev/null")
click_at(md[0] + md[2] / 2, md[1] + md[3] - 40, name="play/pause"); t1 = sh("busctl --user get-property org.mpris.MediaPlayer2.fake /org/mpris/MediaPlayer2 org.mpris.MediaPlayer2.Player PlaybackStatus 2>/dev/null"); shot("u-05-media")
check("the music card's middle button plays or pauses", t0 != t1 or t0 == "", f"{t0} -> {t1}")
net = boxes()["modules"]["wifi"]; ms = click_at(net[0] + 40, net[1] + net[3] / 2, name="Network card"); shot("u-06-network")
check(f"Network opens its card ({ms:.0f} ms)", boxes()["child"] is not None)
click_at(300, 900, name="outside"); d = boxes()
check("a click outside closes the card and the panel", d["child"] is None and d["state"] not in ("open", "locked"), d.get("state"))
chord("super+a"); wait(1.0); chord("esc"); wait(0.6)
check("Escape closes the panel", boxes()["state"] not in ("open", "locked"), boxes()["state"])

# --- B. editing
chord("super+a"); wait(1.2); d0 = boxes(); start = words()
s = d0["modules"]["session"]; ms = click_at(s[0] + 4, s[1] + 4, "right", name="edit"); d = boxes(); shot("u-07-edit")
check(f"a right click starts editing ({ms:.0f} ms)", d["state"] == "editing" and d["bar"] is not None)
ms = click_at(*bar_button("add"), name="Add"); shot("u-08-add")
a = drawn().get(("add", "")); b = boxes()["bar"]
check(f"Add opens its list beside the panel, the bar's buttons stay ({ms:.0f} ms)", a is not None and abs(b[1] - d["bar"][1]) < 1 and (a[0] + a[2] <= d["panel"][0] or a[0] >= d["panel"][0] + d["panel"][2]), f"{a} bar {d['bar']} -> {b}")
click_at(*add_row("calendar", start), name="add Calendar"); d = boxes(); shot("u-09-calendar")
check("Calendar added at the bottom", "calendar" in d["modules"] and d["modules"]["calendar"][1] > d["modules"]["session"][1], str(d["modules"].get("calendar")))
click_at(*bar_button("add"), name="close Add")
# held near the panel's top edge the panel scrolls up: the calendar can go to the very top
cal = d["modules"]["calendar"]; top_y = d["panel"][1] + 6
drag(cal[0] + cal[2] / 2, cal[1] + 40, 0, top_y - (cal[1] + 40)); d = boxes(); shot("u-10-dragged")
check("the calendar dragged to the top", d["modules"]["calendar"][1] < d["modules"]["volume"][1], f"{d['modules']['calendar']}")
ck = d["modules"]["clock"]; hover_card(ck); click_at(*card_button("clock", "half"), name="Half"); d = boxes(); shot("u-11-half")
check("Half puts the clock beside its neighbour", d["modules"]["clock"][2] < d["panel"][2] / 2 + 1, str(d["modules"]["clock"]))
# the screen edge the panel is on (the box dump's panel line ends with it: a panel as tall as the screen
# looks the same on two edges)
side = lambda: [l for l in open(BOX) if l.startswith("panel ")][0].split()[-1]
p0 = d["panel"]; s0 = side()
ms = click_at(*bar_button("edge"), name="Edge"); d = boxes(); shot("u-12-edge")
check(f"Edge moves the panel to the next screen edge ({ms:.0f} ms)", side() != s0, f"{s0} -> {side()} {d['panel']}")
click_at(*bar_button("undo"), name="Undo edge"); d = boxes()
check("Undo puts it back", side() == s0 and d["panel"] == p0, f"{side()} {d['panel']}")
tr = d["modules"].get("tray") or d["modules"].get("notifications"); key = "tray" if "tray" in d["modules"] else "notifications"
hover_card(tr); click_at(*card_button(key, "remove"), name="remove"); d = boxes(); shot("u-13-removed")
check(f"x takes the {key} card out", key not in d["modules"])
click_at(*bar_button("done"), name="Done"); wait(0.8); l = line(); shot("u-14-saved")
check("Done saves it, calendar first, clock half, no places", l.find("calendar") < l.find("volume") and "clock@half" in l and not re.search(r"@\d", l), l)
chord("esc"); wait(0.5); chord("super+a"); wait(1.2); d = boxes(); shot("u-15-reopened")
check("reopened, it is as saved", d["modules"]["calendar"][1] < d["modules"]["volume"][1] and key not in d["modules"])
# Reset asks first, Cancel throws it away
s = d["modules"]["session"]; click_at(s[0] + 4, s[1] + 4, "right")
click_at(*bar_button("reset"), name="Reset"); shot("u-16-sure")
check("Reset asks first", "calendar" in boxes()["modules"])
click_at(*bar_button("reset"), name="Reset sure"); d = boxes(); shot("u-17-reset")
check("the second press resets to the built-in panel", "calendar" not in d["modules"], sorted(d["modules"]))
click_at(*bar_button("cancel"), name="Cancel"); wait(0.6)
check("Cancel throws the reset away: the saved line stays", line() == l, line())
chord("esc"); wait(0.5)
sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}"); sh("pkill -f fakempris; true")
print("slow (over 300 ms):", SLOW or "none", flush=True)
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok: print("  failed:", n)
