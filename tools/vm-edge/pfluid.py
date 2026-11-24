import sys, os, time
exec(open("/home/tester/papps.py").read().split("sh(f\"sed -i '/^PANEL_CONTROL=/d' {CFG}\")")[0])
# The edge from apps with real input: More and lists in place (Network's networks, from the stand-in
# nmcli of papps.py), the keyboard's selection, and panels left with nothing behind when they close
# (shots "z-*" are compared on the host with "z-base": the desktop before any panel opened). Hit ids
# from settings.rs (Network: row 0, More 2, networks from 10) and quick.rs (Do not disturb's switch 30).
NET = "settings/network"
def cards():
    out = {}
    for l in open(BOX):
        w = l.split()
        if w and w[0] == "card": out[w[1]] = list(map(float, w[2:6]))
    return out
def nets(): return [r for (c, i), r in hits().items() if c == NET and i >= 10]
def centre(r): return r[0] + r[2] / 2, r[1] + r[3] / 2
def timed(f):
    t = time.time(); f(); return round((time.time() - t) * 1000)
LINE = "clock volume wifi bluetooth dnd power replay nightlight tray notifications session"

sh(f"sed -i '/^PANEL_CONTROL=/d; /^PANEL_APPS=/d' {CFG}")
open(CFG, "a").write(f'PANEL_CONTROL="SUPER+A bottom right 380 {LINE}"\n')
apps_edge(); at(300, 300); wait(1.0); shot("z-base")

# 1. opening: the cards arrive and settle
chord("super+a"); wait(0.06); shot("a-1-arriving"); wait(1.0); d = boxes(); shot("a-2-open")
c, h = cards(), hits()
check("Super+A opens the control panel", d.get("state") == "locked" and (NET, 2) in h, str(sorted(c)))
# 2. pointing at Network shows More; More opens its list under it, in the panel
at(*centre(h[(NET, 0)])); shot("a-3-hover-network")
click_at(*mid(h[(NET, 2)])); wait(0.5); d2 = boxes(); c2, h2, n = cards(), hits(), nets(); shot("a-4-network-list")
p = d["panel"]
check("More opens Network's list inside the panel", bool(n) and all(p[0] <= r[0] and r[0] + r[2] <= p[0] + p[2] + 1 for r in n), str(n))
check("Network's row stayed under the pointer (Less where More was)", h2.get((NET, 0)) == h[(NET, 0)] and h2.get((NET, 2)) == h[(NET, 2)], f"{h[(NET, 0)]} -> {h2.get((NET, 0))}")
row = h[(NET, 0)]
check("the list is next to Network's row", bool(n) and all(r[1] >= row[1] + row[3] - 1 or r[1] + r[3] <= row[1] + 1 for r in n), f"{n} {row}")
below = [k for k in c if k != NET and c[k][1] > c[NET][1] + 5]
moved = [k for k in below if c2.get(k, [0, 0])[1] != c[k][1]]
check("what is under it made room (or the panel grew upward)", moved or d2["panel"][1] < d["panel"][1], f"moved {moved} panel {d['panel']} -> {d2['panel']}")
# 3. Do not disturb still switches with the list open, and the list stays
click_at(*mid(h2[("settings/quick", 30)])); wait(0.5)
check("another switch works and the list stays open", bool(nets()))
shot("a-5-dnd-switched"); click_at(*mid(hits()[("settings/quick", 30)])); wait(0.4)
# 4. Less folds it, everything back
click_at(*mid(hits()[(NET, 2)])); wait(0.6); d3 = boxes()
check("Less folds the list away", not nets())
check("and the panel is as it was", cards() == c and d3["panel"] == d["panel"], f"{d3['panel']} vs {d['panel']}")
shot("a-6-folded")
# 5. right click on Network's row: its list in place
click_at(*centre(hits()[(NET, 0)]), "right"); wait(0.6); shot("a-7-network-list")
check("right click on Network opens its list in place", bool(nets()) and hits().get((NET, 0)) == row, f"{nets()} row {row} -> {hits().get((NET, 0))}")
# 6. Escape with a list open: the panel closes, nothing left on screen
chord("esc"); check("Escape closes the panel, list and all", closed()); wait(0.4); at(300, 300); wait(0.5); shot("z-after-esc-list")

# 7. the apps panel: no selection lit on opening; a key lights it; the pointer takes over
sh(f"sed -i '/^PANEL_APPS=/d' {CFG}")
open(CFG, "a").write('PANEL_APPS="SUPER+W bottom center search favorites apps"\n')
apps_edge(); at(300, 300); wait(0.5)
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
        r = cards().get(NET)
        if r: at(*centre(r))
    chord("super+a"); wait(0.12)
wait(0.6); at(300, 300); wait(0.4); shot("z-after-cycles")
check("ten quick cycles end closed", closed())

# 10. a list open, then the panel toggled by its key while the list is still growing
chord("super+a"); wait(0.9); at(*centre(hits()[(NET, 0)])); click_at(*mid(hits()[(NET, 2)])); wait(0.08)
chord("super+a"); wait(0.6); at(300, 300); wait(0.4); shot("z-after-mid-grow")
check("closed while a list grew", closed())
chord("super+a"); wait(0.9); shot("a-9-reopened")
check("opened again: no list left open", not nets(), str(nets()))
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
# a list left open from part 10 is folded first, so the click below opens it
if nets(): click_at(*mid(hits()[(NET, 2)])); wait(0.6)
at(*mid(hits()[(NET, 2)])); emit((1, BTN["left"], 1)); wait(0.03); emit((1, BTN["left"], 0))
t0 = time.time(); until(lambda: bool(nets()), 3, 0.01); ms2 = round((time.time() - t0) * 1000)
print(f"TIMING open to first frame {ms} ms (with input), More to list {ms2} ms", flush=True)
chord("esc"); wait(0.5)

# 11. opened by pointing at the screen's edge (the owner's way): a list in place, then the pointer
# leaves: the panel closes with it (a list in place is inside the panel)
sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}")
open(CFG, "a").write(f'PANEL_CONTROL="SUPER+A bottom right 380 hover {LINE}"\n')
apps_edge(); at(300, 300); wait(0.8)
move(3000, 3000, 30); move(-150, 0, 10); wait(1.2)
check("pointing at the bottom right edge opens the panel", not closed())
# straight up from the edge (the pointer is at 1769, 1079), over the panel, to Network's More
tx, ty = mid(hits()[(NET, 2)])
move(0, -30, 5); move(int(tx - 1769), int(ty - 1049), 30); wait(0.4)
check("the way up to it kept it open", not closed())
click(); wait(0.6); shot("c-1-hover-list")
check("More opened the list", bool(nets()))
at(300, 300); wait(0.8); shot("z-after-hover-leave")
check("leaving the panel closes it, list and all", closed())

sh(f"rm -f {BIN}/nmcli {BIN}/powerprofilesctl; sed -i '/^PANEL_CONTROL=/d; /^PANEL_APPS=/d' {CFG}")
sh("pkill -f '[i]nfynitae-edge-apps'; true"); restart_edge()
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok: print("  failed:", n)
