import sys, os, time
exec(open("/home/tester/pflow.py").read().split("sh(f\"sed -i '/^PANEL_CONTROL=/d' {CFG}; rm -f {BACKUP}\")")[0])
# The fluid edge of 2026-11-13 with real input: More on hover and lists in place, the keyboard's
# selection, and panels left with nothing behind when they close (shots "z-*" are compared on the
# host with "z-base": the desktop before any panel opened).
def child():
    for l in open(BOX):
        w = l.split()
        if w[0] == "child": return list(map(float, w[1:5]))
    return None
def closed():
    wait(0.4); return open(BOX).read().startswith("closed")
def centre(r): return r[0] + r[2] / 2, r[1] + r[3] / 2
def more_of(r): return r[0] + r[2] - 21, r[1] + r[3] / 2
def timed(f):
    t = time.time(); f(); return round((time.time() - t) * 1000)

sh(f"sed -i '/^PANEL_CONTROL=/d; /^PANEL_APPS=/d' {CFG}")
open(CFG, "a").write('PANEL_CONTROL="SUPER+A bottom right 380 clock volume wifi bluetooth dnd power replay nightlight tray notifications session"\n')
restart_edge(); at(300, 300); wait(1.0); shot("z-base")

# 1. opening: the cards arrive and settle
chord("super+a"); wait(0.06); shot("a-1-arriving"); wait(1.0); d = boxes(); shot("a-2-open")
m = d["modules"]
check("Super+A opens the control panel", d.get("state") == "locked" and "bluetooth" in m, str(sorted(m)))
# 2. pointing at Bluetooth shows More; More opens its list under it, in the panel
bt = m["bluetooth"]; at(*centre(bt)); shot("a-3-hover-bluetooth")
click_at(*more_of(bt)); wait(0.5); d2 = boxes(); c = child(); shot("a-4-bluetooth-list")
check("More opens Bluetooth's list inside the panel", c is not None and d["panel"][0] <= c[0] and c[0] + c[2] <= d["panel"][0] + d["panel"][2] + 1, str(c))
bt_now = d2["modules"]["bluetooth"]
check("Bluetooth's row stayed under the pointer (Less where More was)", bt_now == bt, f"{bt} -> {bt_now}")
check("the list is next to Bluetooth's row", c is not None and (c[1] + c[3] <= bt_now[1] + 1 or c[1] >= bt_now[1] + bt_now[3] - 1), f"{c} {bt_now}")
below = [k for k in ("dnd", "power", "replay", "nightlight", "tray", "notifications", "session") if k in m and m[k][1] > bt[1] + 5]
moved = [k for k in below if d2["modules"].get(k, [0, 0])[1] != m[k][1]]
check("what is under it made room (or the panel grew upward)", moved or d2["panel"][1] < d["panel"][1], f"moved {moved} panel {d['panel']} -> {d2['panel']}")
# 3. Do not disturb still switches with the list open, and the list stays
dnd = d2["modules"]["dnd"]; click_at(*centre(dnd)); wait(0.5)
check("another switch works and the list stays open", child() is not None)
shot("a-5-dnd-switched"); click_at(*centre(boxes()["modules"]["dnd"])); wait(0.4)
# 4. Less folds it, everything back
bt2 = boxes()["modules"]["bluetooth"]; click_at(*more_of(bt2)); wait(0.6); d3 = boxes()
check("Less folds the list away", child() is None)
check("and the panel is as it was", d3["modules"] == m and d3["panel"] == d["panel"], f"{d3['panel']} vs {d['panel']}")
shot("a-6-folded")
# 5. right click on Network: its list in place; another list (Power) takes its place
wi = d3["modules"]["wifi"]; click_at(*centre(wi), "right"); wait(0.6); c1 = child(); shot("a-7-network-list")
check("right click on Network opens its list in place", c1 is not None and boxes()["modules"]["wifi"] == wi, f"{c1} wifi {wi} -> {boxes()['modules']['wifi']}")
rp = boxes()["modules"]["replay"]; click_at(*centre(rp), "right"); wait(0.6); c2 = child(); shot("a-8-replay-list")
check("right click on Replay: its list replaces Network's", c2 is not None and c2 != c1 and boxes()["modules"]["replay"] == rp, f"{c1} -> {c2}")
# 6. Escape with a list open: nothing left on screen
chord("esc"); wait(0.4)
check("Escape first closes the list, the panel stays", child() is None and not closed())
chord("esc"); check("Escape again closes the panel", closed()); wait(0.4); at(300, 300); wait(0.5); shot("z-after-esc-list")

# 7. the apps panel: no selection lit on opening; a key lights it; the pointer takes over
sh(f"sed -i '/^PANEL_APPS=/d' {CFG}")
open(CFG, "a").write('PANEL_APPS="SUPER+W bottom center search favorites apps"\n')
restart_edge(); at(300, 300); wait(0.5)
chord("super+w"); wait(1.2); shot("b-1-apps-open")
chord("down"); wait(0.4); shot("b-2-after-down")
chord("down"); wait(0.4); shot("b-2b-after-second-down")
at(960, 900); wait(0.4); shot("b-3-pointer-moved")
chord("esc"); wait(0.5); at(300, 300); wait(0.4); shot("z-after-apps")

# 8. one panel to another and back, fast
for i in range(3):
    chord("super+a"); wait(0.35); chord("super+w"); wait(0.35)
chord("esc"); wait(0.6); at(300, 300); wait(0.4); shot("z-after-switching")
check("panels switched fast then closed", closed())

# 9. ten quick open / close cycles, pointing at cards on the way
for i in range(10):
    chord("super+a"); wait(0.15 + 0.03 * i)
    if i % 3 == 0:
        try:
            b = boxes()["modules"]["bluetooth"]; at(*centre(b))
        except Exception:
            pass
    chord("super+a"); wait(0.12)
wait(0.6); at(300, 300); wait(0.4); shot("z-after-cycles")
check("ten quick cycles end closed", closed())

# 10. a list open, then the panel toggled by its key while the list is still growing
chord("super+a"); wait(0.9); b = boxes()["modules"]["bluetooth"]; at(*centre(b)); click_at(*more_of(b)); wait(0.08)
chord("super+a"); wait(0.6); at(300, 300); wait(0.4); shot("z-after-mid-grow")
check("closed while a list grew", closed())
chord("super+a"); wait(0.9); shot("a-9-reopened")
check("opened again: no list left open", child() is None)
chord("esc"); wait(0.5)

# timings: the panel open to drawn (boxes written), More to the list
def open_once():
    chord("super+a"); until(lambda: not open(BOX).read().startswith("closed"), 3, 0.01)
ms_all = []
for _ in range(5):
    at(300, 300); ms_all.append(timed(open_once)); wait(0.8); chord("esc"); wait(0.6)
ms = sorted(ms_all)[2]
print("TIMING opens", ms_all, flush=True)
chord("super+a"); wait(0.8)
b = boxes()["modules"]["bluetooth"]; at(*more_of(b)); emit((1, BTN["left"], 1)); wait(0.03); emit((1, BTN["left"], 0))
t0 = time.time(); until(lambda: child() is not None, 3, 0.01); ms2 = round((time.time() - t0) * 1000)
print(f"TIMING open to first frame {ms} ms (with input), More to list {ms2} ms", flush=True)
chord("esc"); wait(0.5)

# 11. opened by pointing at the screen's edge (the owner's way): a list in place, then the pointer
# leaves: the panel closes with it (a list in place is inside the panel)
sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}")
open(CFG, "a").write('PANEL_CONTROL="SUPER+A bottom right 380 hover clock volume wifi bluetooth dnd power replay nightlight tray notifications session"\n')
restart_edge(); at(300, 300); wait(0.8)
move(3000, 3000, 30); move(-150, 0, 10); wait(1.2)
check("pointing at the bottom right edge opens the panel", not closed())
# straight up from the edge (the pointer is at 1769, 1079), over the panel, to Bluetooth's More
b = boxes()["modules"]["bluetooth"]; tx, ty = b[0] + b[2] - 21, b[1] + b[3] / 2
move(0, -30, 5); move(int(tx - 1769), int(ty - 1049), 30); wait(0.4)
check("the way up to it kept it open", not closed())
click(); wait(0.6); shot("c-1-hover-list")
check("More opened the list", child() is not None)
at(300, 300); wait(0.8); shot("z-after-hover-leave")
check("leaving the panel closes it, list and all", closed())

sh(f"sed -i '/^PANEL_CONTROL=/d; /^PANEL_APPS=/d' {CFG}"); restart_edge()
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok: print("  failed:", n)
