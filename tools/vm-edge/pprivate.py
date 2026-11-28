import sys, os, time, subprocess
exec(open("/home/tester/vmlib.py").read())
# Notifications of an app in PRIVACY_APPS (2026-11-28): on the screen as always, left out of
# captures. A pop-up from Secretapp (listed) and one from Plainapp show together; a screenshot
# (grim, through screencopy, the way the recorder, screen sharing and remote desktop capture too)
# has Plainapp's card and, where Secretapp's is, what is behind it. The Notifications card then
# lists both: its Secretapp entry is not in the screenshot either.
# A screenshot cannot show what the screen shows here, by design: while "look at the VM window"
# is printed, both pop-ups (then both entries) must be seen on the VM's own display.
D = "/tmp/private-out"
sh(f"rm -rf {D}; mkdir -p {D}; chmod 777 {D}")
sh("pkill -x mako; true"); wait(0.5)
sh(f"sed -i '/^NOTIFICATIONS=/d; /^PANEL_CONTROL=/d; /^PRIVACY_APPS=/d' {CFG}")
open(CFG, "a").write('NOTIFICATIONS="auto"\nPRIVACY_APPS="secretapp"\n'
                     'PANEL_CONTROL="SUPER+A top right 380 notifications"\n')
restart_edge(); at(300, 600); wait(1.0)

def grab(name, x, y, w, h):
    # the region's RGB rows from a capture (PPM: a text header, then the pixels)
    path = f"{D}/{name}.ppm"
    sh(f"su tester -c 'grim -t ppm -g \"{x},{y} {w}x{h}\" {path}'", 10)
    data = open(path, "rb").read()
    return [data[len(data) - w * h * 3 + r * w * 3:][:w * 3] for r in range(h)]

def same_rows(a, b):
    return sum(1 for ra, rb in zip(a, b) if ra == rb)

# the pop-ups' column (POPUP_W 360, MARGIN 24 from the right of 1920)
X0, W = 1920 - 24 - 360, 360
base_pop = grab("base-popups", X0, 0, W, 400)

# 1. the pop-ups: Plainapp is the newest, at the top (24 to 107: 2*14 + 18 + 20 + 17); Secretapp's
# is under it (119 to 202)
run_bg('notify-send -a Secretapp -t 6000 "Private one" "Not in any capture"'); wait(0.4)
run_bg('notify-send -a Plainapp -t 6000 "Plain one" "In every capture"'); wait(0.8)
print("look at the VM window: two pop-ups at the top right, Plain one above Private one", flush=True)
shot("n-private-popups")
pop = grab("popups", X0, 0, W, 400)
check("the capture has Plainapp's card", same_rows(pop[30:100], base_pop[30:100]) == 0,
      f"{same_rows(pop[30:100], base_pop[30:100])} of 70 rows as without it")
check("and where Secretapp's is, what is behind it", same_rows(pop[125:195], base_pop[125:195]) == 70,
      f"{same_rows(pop[125:195], base_pop[125:195])} of 70 rows as without it")

# 2. the history, once the pop-ups went: the card lists both, Plainapp's first
wait(6.0)
chord("super+a"); wait(1.2); d = boxes()
card = d["modules"].get("notifications")
check("the Notifications card shows", card is not None, str(sorted(d["modules"])))
if card:
    x, y, w, h = (int(v) for v in card)
    print("look at the VM window: the card lists Plain one and Private one", flush=True)
    shot("n-private-card"); wait(3.0)
    # a column inside the card's body, where an entry spans it
    now = grab("card", x + 40, y, w - 80, h)
    chord("esc"); wait(0.8)
    before = grab("card-base", x + 40, y, w - 80, h)
    # an entry is at least 36 px tall; without this change no row of the card is as without it
    check("the capture of the card has a gap where Secretapp's entry is", same_rows(now, before) >= 30,
          f"{same_rows(now, before)} of {h} rows as without the panel")
else:
    chord("esc"); wait(0.8)

sh(f"sed -i '/^NOTIFICATIONS=/d; /^PANEL_CONTROL=/d; /^PRIVACY_APPS=/d' {CFG}"); restart_edge()
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok: print("  failed:", n)
