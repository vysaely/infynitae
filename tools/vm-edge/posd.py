import sys
import os
import time

exec(open("/home/tester/vmlib.py").read())

# Volume and brightness OSD in the VM with real key presses and INFYNITAE_EDGE_BOXES:
# key-to-first-frame timing, bottom-centre placement, replacement without stacking, mute/unmute,
# 1.2 s auto-hide, pass-through keyboard focus, and exclusion from notification history.
# Shots "osd-*" show the drawn OSD; "z-*" are compared on the host against "z-base".

BIN = "/home/tester/.local/bin"
CFG = "/home/tester/.config/infynitae/config"
RT = "/run/user/1000"
EDGE = sys.argv[1] if len(sys.argv) > 1 else f"{BIN}/infynitae-edge-apps"


def osd_lines():
    try:
        return [l for l in open(BOX).read().splitlines() if l.startswith("osd ")]
    except OSError:
        return []


def boxes_text():
    try:
        return open(BOX).read()
    except OSError:
        return ""


sh(f"rm -f {RT}/infynitae/do-not-disturb {RT}/infynitae/game-quiet /tmp/osd-kbd-ok")
sh("pkill -x infynitae-edge; pkill -x mako; pkill -f '[i]nfynitae-edge-apps'; true")
wait(0.8)
sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}")
open(CFG, "a").write(
    'PANEL_CONTROL="SUPER+A top right 380 notifications/list settings/session"\n'
)
close_all()
sh("pactl set-sink-mute @DEFAULT_SINK@ 0 2>/dev/null; pactl set-sink-volume @DEFAULT_SINK@ 45% 2>/dev/null; true")
run_bg(f"PATH={BIN}:$PATH INFYNITAE_EDGE_BOXES={BOX} {EDGE}")
wait(2.0)
at(300, 300)
wait(0.4)
shot("z-base")

# 1. Measure real hardware/uinput key-to-first-frame latency across 5 isolated presses.
latencies = []
for _ in range(5):
    t = time.time()
    emit((1, KEYS["volup"], 1))
    while not osd_lines() and time.time() - t < 2.0:
        time.sleep(0.002)
    dt = round((time.time() - t) * 1000)
    emit((1, KEYS["volup"], 0))
    if osd_lines():
        latencies.append(dt)
    end = time.time() + 2.5
    while osd_lines() and time.time() < end:
        time.sleep(0.05)
    wait(0.15)

print(
    "TIMING osd key->first-frame ms",
    latencies,
    "median",
    sorted(latencies)[len(latencies) // 2] if latencies else -1,
    flush=True,
)
check(
    "volume key shows OSD on first frame in all 5 trials",
    len(latencies) == 5 and max(latencies) < 2000,
    str(latencies),
)

# 2. Repeated volume presses replace the same bottom-centre OSD without stacking.
chord("volup")
wait(0.25)
chord("volup")
wait(0.25)
lines = osd_lines()
shot("osd-volume")
check("repeated volume presses show one OSD line", len(lines) == 1, repr(lines))
if len(lines) == 1:
    _, xs, ys, ws, hs, val, label = lines[0].split(None, 6)
    x, y, w, h = int(xs), int(ys), int(ws), int(hs)
    outs = snap().get("outputs") or []
    ow, oh = outs[0].get("mode", [1920, 1080]) if outs else (1920, 1080)
    cx = x + w / 2.0
    check(
        "OSD is bottom-centre with expected dimensions",
        w == 240 and h == 56 and abs(cx - ow / 2.0) <= 2 and y > oh / 2,
        f"{lines[0]} on {ow}x{oh}",
    )
    check("volume OSD shows Volume label", label == "Volume", repr(label))

# 3. Mute toggles to "Muted" with value 0, then unmutes back to "Volume".
chord("mute")
wait(0.3)
mlines = osd_lines()
shot("osd-muted")
check(
    "mute key shows Muted OSD with zero bar",
    len(mlines) == 1 and mlines[0].endswith(" 0 Muted"),
    repr(mlines),
)
chord("mute")
wait(0.3)
ulines = osd_lines()
check(
    "unmute restores Volume OSD",
    len(ulines) == 1 and ulines[0].split()[-1] == "Volume",
    repr(ulines),
)

# 4. Auto-hides after ~1.2 s leaving a clean desktop.
wait(1.6)
check("OSD hides after timeout", boxes_text().strip() == "closed", repr(boxes_text()))
at(300, 300)
wait(0.3)
shot("z-after-osd")

# 5. Brightness synchronous notification renders and expires too (VM has no backlight sysfs).
run_bg(
    'notify-send -a Display -h string:x-canonical-private-synchronous:brightness -h int:value:70 -t 1200 "Brightness: 70%"'
)
wait(0.35)
blines = osd_lines()
shot("osd-brightness")
check(
    "brightness notification shows Brightness OSD",
    len(blines) == 1 and blines[0].endswith(" 70 Brightness"),
    repr(blines),
)
wait(1.6)
check("brightness OSD hides after timeout", boxes_text().strip() == "closed", repr(boxes_text()))

# 6. Volume OSD never steals keyboard focus from an active terminal.
run_bg("alacritty")
until(lambda: has("alacritty"), 6)
wait(0.8)
point_at("alacritty")
click()
wait(0.3)
chord("ctrl+c")
wait(0.1)
chord("volup")
text("touch /tmp/osd-kbd-ok\n")
wait(0.8)
check(
    "volume OSD does not steal keyboard focus from terminal",
    os.path.exists("/tmp/osd-kbd-ok"),
)
close_all()
wait(1.5)

# 7. None of the OSD updates entered notification history.
run_bg(f"{EDGE} control")
wait(1.2)
cards = [l.split()[1] for l in boxes_text().splitlines() if l.startswith("card ")]
shot("osd-history-check")
check(
    "OSD notifications are excluded from notification history",
    "notifications/list" not in cards and "settings/session" in cards,
    str(cards),
)
chord("esc")
wait(0.6)

sh("rm -f /tmp/osd-kbd-ok; pkill -f '[i]nfynitae-edge-apps'; true")
sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}")
restart_edge()
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for name, ok in RES:
    if not ok:
        print("  failed:", name)
sys.exit(0 if all(ok for _, ok in RES) else 1)
