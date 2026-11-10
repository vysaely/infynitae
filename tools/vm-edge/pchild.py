import sys, os
exec(open("/home/tester/g/pcanvas.py").read().split("sh(f\"sed -i '/^PANEL_CONTROL=/d;")[0])
def child():
    for l in open(BOX):
        w = l.split()
        if w[0] == "child": return list(map(float, w[1:5]))
    return None
sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}")
open(CFG, "a").write('PANEL_CONTROL="SUPER+A top right 360x300 hover clock@8,8,344x34 volume@8,48,344x32 wifi@8,86,169x46 bluetooth@183,86,169x46 dnd@8,138,169x46"\n')
restart_edge(); chord("super+a"); wait(1.5); d = boxes()
check("the placed panel reads its places", d["modules"]["wifi"][1] - d["panel"][1] == 86, str(d["modules"].get("wifi")))
ck = d["modules"]["clock"]; click_at(*centre(ck)); wait(0.5); c = boxes() and child(); shot("h-1-calendar")
check("a click on the clock opens the month beside the panel", c is not None and c[0] + c[2] <= d["panel"][0], str(c))
check("and nothing in the panel moved", boxes()["modules"] == d["modules"])
# a click inside the card (Later) keeps it open
if c: click_at(c[0] + c[2] - 40, c[1] + 44); wait(0.4); shot("h-2-later")
check("a click in the month keeps it open", child() is not None and boxes()["state"] == "locked")
click_at(*centre(ck)); wait(0.5); boxes()
check("the clock again closes it", child() is None)
vo = d["modules"]["volume"]; click_at(vo[0] + 30, vo[1] + vo[3] / 2, "right"); wait(0.8); boxes(); c = child(); shot("h-3-outputs")
check("right click on the volume: its outputs beside the panel, level with it", c is not None and abs(c[1] - vo[1]) <= 1, str(c))
chord("esc"); wait(0.6)
check("Escape closes the panel and the card", boxes()["state"] == "locked" and sh("cat /tmp/edge-boxes | head -1") != "")
sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}"); restart_edge()
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok: print("  failed:", n)
