import sys, os, time
exec(open("/home/tester/pflow.py").read().split("sh(f\"sed -i '/^PANEL_CONTROL=/d' {CFG}; rm -f {BACKUP}\")")[0])
# The edge from apps (live/edge-apps, stage 1) with real input in the VM. Stand-ins for nmcli and
# powerprofilesctl answer as the real ones and log what they were asked (the VM has neither), so a
# click is checked by the command it ran. Shots "z-*" are compared on the host with "z-base".
BIN = "/home/tester/.local/bin"
LOG = "/tmp/stand-in.log"
open(f"{BIN}/nmcli", "w").write("""#!/bin/sh
echo "nmcli $*" >> /tmp/stand-in.log
case "$*" in
  "radio wifi") echo enabled ;;
  "-t -f TYPE,STATE device") printf 'ethernet:unavailable\\nwifi:connected\\n' ;;
  "-t -f IN-USE,SIGNAL,SECURITY,SSID dev wifi list --rescan no") printf '*:72:WPA2:HomeNet\\n :40::Cafe Free\\n' ;;
  "-t -f NAME,TYPE connection show") printf 'HomeNet:802-11-wireless\\n' ;;
esac
""")
open(f"{BIN}/powerprofilesctl", "w").write("""#!/bin/sh
echo "powerprofilesctl $*" >> /tmp/stand-in.log
[ "$1" = list ] && printf '  performance:\\n\\n* balanced:\\n\\n  power-saver:\\n'
""")
os.chmod(f"{BIN}/nmcli", 0o755); os.chmod(f"{BIN}/powerprofilesctl", 0o755)
sh(f"rm -f {LOG}; touch {LOG}; chmod 666 {LOG}")
def apps_edge():
    sh("pkill -x infynitae-edge; pkill -f '[i]nfynitae-edge-apps'; true"); wait(0.8)
    run_bg(f"PATH={BIN}:$PATH INFYNITAE_EDGE_BOXES={BOX} {BIN}/infynitae-edge-apps"); wait(1.5)
def hits():
    out = {}
    for l in open(BOX):
        w = l.split()
        if w and w[0] == "hit": out[(w[1], int(w[2]))] = list(map(float, w[3:7]))
    return out
def closed(): wait(0.4); return open(BOX).read().startswith("closed")
def logged(s): wait(0.6); return s in open(LOG).read()
def mid(r): return r[0] + r[2] / 2, r[1] + r[3] / 2

sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}")
open(CFG, "a").write('PANEL_CONTROL="SUPER+A bottom right 380 hover settings/network settings/bluetooth settings/power"\n')
apps_edge(); at(300, 300); wait(0.5); shot("z-base")

# 1. by its key's command, then Escape
run_bg(f"{BIN}/infynitae-edge-apps control"); wait(1.5); d = boxes(); shot("e-1-open")
check("the panel opens by its command", d.get("state") == "locked" and "panel" in d, str(d.get("panel")))
chord("esc"); check("Escape closes it", closed()); at(300, 300); wait(0.4); shot("z-after-esc")

# 2. open, the Network list by More, a network, the Wi-Fi switch, a power mode
run_bg(f"{BIN}/infynitae-edge-apps control"); wait(2.5)
h = hits()
check("the cards' controls are on the screen", ("settings/network", 2) in h and ("settings/power", 2) in h, str(sorted(h))[:200])
at(*mid(h[("settings/network", 0)])); wait(0.4); shot("e-2-hover-network")
click_at(*mid(h[("settings/network", 2)])); wait(0.8); h = hits(); shot("e-3-network-list")
check("More opens the networks in place", ("settings/network", 11) in h, str([k for k in h if k[0] == "settings/network"]))
if ("settings/network", 11) in h:
    click_at(*mid(h[("settings/network", 11)]))
    check("a click on Cafe Free (open) joins it", logged("nmcli dev wifi connect Cafe Free"))
h = hits()
click_at(*mid(h[("settings/network", 1)]))
check("the switch turns Wi-Fi off", logged("nmcli radio wifi off"))
h = hits()
click_at(*mid(h[("settings/power", 2)]))
check("Performance is set", logged("powerprofilesctl set performance"))
shot("e-4-after-clicks")
click_at(200, 200); check("a click outside closes", closed()); at(300, 300); wait(0.4); shot("z-after-outside")

# 3. by hover: from the bottom edge up into it, then away
move(3000, 3000, 30); move(-150, 0, 10); wait(1.2)
check("pointing at the bottom right edge opens it", not closed())
move(0, -60, 10); wait(0.5); check("the way up keeps it open", not closed())
at(300, 300); wait(0.8); check("leaving closes it", closed()); shot("z-after-hover")

# 4. quick cycles, and it is still running
for i in range(10):
    run_bg(f"{BIN}/infynitae-edge-apps control"); wait(0.12 + 0.03 * i)
    run_bg(f"{BIN}/infynitae-edge-apps control"); wait(0.12)
wait(1.0); at(300, 300); wait(0.4); shot("z-after-cycles")
check("ten quick cycles end closed", closed())
check("the edge is still running", sh("pgrep -f '[i]nfynitae-edge-apps$' | head -1") != "")

sh(f"rm -f {BIN}/nmcli {BIN}/powerprofilesctl; sed -i '/^PANEL_CONTROL=/d' {CFG}")
sh("pkill -f '[i]nfynitae-edge-apps'; true"); restart_edge()
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok: print("  failed:", n)
