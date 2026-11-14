import sys, os, time
exec(open("/home/tester/pflow.py").read().split("sh(f\"sed -i '/^PANEL_CONTROL=/d' {CFG}; rm -f {BACKUP}\")")[0])
# The edge from apps (live/edge-apps), stage 1 part 2, with real input in the VM: the launcher's
# search and Enter, the windows panel, the clipboard, the notification server and its card, the
# ask prompt, Quick settings, and the keyboard's selection on a card. A stand-in powerprofilesctl
# logs what it was asked. Shots "z-*" are compared on the host with "z-base".
BIN = "/home/tester/.local/bin"
LOG = "/tmp/stand-in.log"
RUN = "/run/user/1000"
open(f"{BIN}/powerprofilesctl", "w").write("""#!/bin/sh
echo "powerprofilesctl $*" >> /tmp/stand-in.log
[ "$1" = list ] && printf '  performance:\\n\\n* balanced:\\n\\n  power-saver:\\n'
""")
os.chmod(f"{BIN}/powerprofilesctl", 0o755)
sh(f"rm -f {LOG}; touch {LOG}; chmod 666 {LOG}")
EDGE = f"{BIN}/infynitae-edge-apps"
def apps_edge():
    sh("pkill -x infynitae-edge; pkill -x mako; pkill -f '[i]nfynitae-edge-apps'; true"); wait(0.8)
    run_bg(f"PATH={BIN}:$PATH INFYNITAE_EDGE_BOXES={BOX} {EDGE}"); wait(2.0)
def hits():
    wait(0.3)
    out = {}
    for l in open(BOX):
        w = l.split()
        if w and w[0] == "hit": out[(w[1], int(w[2]))] = list(map(float, w[3:7]))
    return out
def cards():
    wait(0.3)
    return [l.split()[1] for l in open(BOX) if l.startswith("card ")]
def closed(): wait(0.4); return open(BOX).read().startswith("closed")
def logged(s): wait(0.6); return s in open(LOG).read()
def mid(r): return r[0] + r[2] / 2, r[1] + r[3] / 2
def panel(name): run_bg(f"{EDGE} {name}"); wait(1.5)

sh(f"sed -i '/^PANEL_CONTROL=/d; /^FAVORITES=/d; /^NOTIFICATIONS=/d' {CFG}")
open(CFG, "a").write('PANEL_CONTROL="SUPER+A top right 380 settings/quick=dnd,nightlight notifications/list settings/power"\nFAVORITES="alacritty"\n')
close_all()
apps_edge(); at(300, 300); wait(0.5); shot("z-base")

# 1. the launcher: type, the list follows, Enter starts the best match
panel("apps"); shot("a-1-open")
check("the apps panel opens with its search and list", "apps/search" in cards() and "apps/list" in cards(), str(cards()))
text("alacr"); wait(0.6); shot("a-2-typed")
h = hits()
check("typing leaves only matching apps in the list", sum(1 for k in h if k[0] == "apps/list") == 1, str([k for k in h if k[0] == "apps/list"]))
chord("enter")
check("Enter starts the best match", until(lambda: has("alacritty"), 8))
check("and the panel closes", closed())

# 2. no match hides the list; Backspace brings it back; Escape clears, then closes
panel("apps"); text("zzqq"); wait(0.5)
check("a query with no match hides the list", "apps/list" not in cards(), str(cards()))
for _ in range(4): chord("bs")
wait(0.4); check("Backspace brings it back", "apps/list" in cards(), str(cards()))
text("x"); chord("esc"); wait(0.4)
check("the first Escape clears the query (the panel stays)", not closed())
chord("esc"); check("the second closes it", closed())

# 3. the windows panel: type a name, Enter focuses it; its x closes it
# (run_bg wraps its command in single quotes: none inside)
run_bg("alacritty -T edge-second -e sleep 600")
until(lambda: has("edge-second"), 10); wait(0.5)
panel("windows"); shot("a-3-windows")
h = hits()
check("the windows panel lists the open windows", sum(1 for k in h if k[0] == "windows/list") >= 2, str([k for k in h if k[0] == "windows/list"]))
text("alacr"); wait(0.4); chord("enter"); wait(0.8)
check("typing a name and Enter focuses that window", "alacritty" in str(focused()).lower(), str(focused()))
panel("windows"); h = hits()
rows = sorted(k for k in h if k[0] == "windows/list")
n_before = len(els())
# a row's close mark: the even hit ids of the card (row 1, close 2, row 3, close 4...)
close_ids = [k for k in rows if k[1] % 2 == 0]
if close_ids:
    at(*mid(h[rows[0]])); wait(0.3); h = hits()
    click_at(*mid(h[close_ids[0]])); wait(1.0)
check("a row's x closes its window", len(els()) < n_before, f"{n_before} -> {len(els())}")
chord("esc"); wait(0.3)

# 4. the clipboard: a copied text is listed; a click copies it back and closes
sh("pgrep -f 'wl-paste --watch cliphist' >/dev/null || true")
sh("cliphist wipe; true")
run_bg("wl-paste --watch cliphist store"); wait(1.2)
run_bg("printf edge-first-text | wl-copy"); wait(1.5)
run_bg("printf edge-second-text | wl-copy"); wait(1.5)
panel("clipboard"); shot("a-4-clipboard")
h = hits(); rows = sorted(k for k in h if k[0] == "clipboard/history")
# (the watcher also stores what was on the clipboard when it started: two or more)
check("the clipboard lists what was copied", len(rows) >= 2, str(rows))
if len(rows) >= 2:
    click_at(*mid(h[rows[1]]))
    check("a click copies that one back and closes", closed() and sh("wl-paste -n -t text/plain", 5) == "edge-first-text",
          sh("wl-paste -n -t text/plain", 5))

# 5. notifications: the edge serves them; the card lists, the x dismisses
run_bg("notify-send -a \"Edge test\" \"First note\" \"its body\""); wait(1.0); shot("a-5-popup")
run_bg("notify-send -a \"Edge test\" \"Second note\""); wait(1.0)
owner = sh("busctl --user call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus GetConnectionUnixProcessID s org.freedesktop.Notifications 2>/dev/null | awk '{print $2}'")
check("the edge holds org.freedesktop.Notifications", owner != "" and "infynitae-edge-apps" in sh(f"cat /proc/{owner or 0}/cmdline | tr '\\0' ' '"), owner)
panel("control"); shot("a-6-control")
h = hits(); notes = sorted(k for k in h if k[0] == "notifications/list" and 100 <= k[1] < 1000)
check("the Notifications card lists both", len(notes) == 2, str(notes))
if notes:
    at(*mid(h[notes[0]])); wait(0.3); h = hits()
    if ("notifications/list", 1000) in h:
        click_at(*mid(h[("notifications/list", 1000)]))
    h = hits(); left = sorted(k for k in h if k[0] == "notifications/list" and 100 <= k[1] < 1000)
    check("its x dismisses one", len(left) == 1, str(left))

# 6. Quick settings: the do-not-disturb switch writes its file
h = hits()
sw = [k for k in h if k[0] == "settings/quick" and k[1] == 30]
if sw: click_at(*mid(h[sw[0]]))
check("the do-not-disturb switch turns it on", until(lambda: os.path.exists(f"{RUN}/infynitae/do-not-disturb"), 4))
h = hits()
if sw: click_at(*mid(h[sw[0]]))
check("and off again", until(lambda: not os.path.exists(f"{RUN}/infynitae/do-not-disturb"), 4))

# 7. the keyboard: the first arrow shows the selection, Enter presses it (a power mode)
at(300, 300)  # the pointer away from the panel; a later move would hide the selection
sh(f": > {LOG}")
# Shift+Tab from nothing selected lands on the last control: the Power card's last choice,
# Performance (the modes go from saver to performance); Enter presses it
if not closed(): chord("esc"); wait(0.3)
panel("control")
chord("shift+tab"); wait(0.2); chord("enter")
check("Shift+Tab then Enter sets a power mode by keyboard", logged("powerprofilesctl set performance"), open(LOG).read()[-200:])
chord("esc"); wait(0.3)

# 8. the ask prompt: a choice by arrows, a text, a cancel
def ask_bg(n, script):
    open(f"/tmp/ask{n}.sh", "w").write(f"XDG_RUNTIME_DIR={RUN}; export XDG_RUNTIME_DIR\n{script} > /tmp/ask{n} 2>&1; echo $? >> /tmp/ask{n}\n")
    sh(f"rm -f /tmp/ask{n}"); run_bg(f"sh /tmp/ask{n}.sh"); wait(1.2)
def answer(n): wait(0.8); return open(f"/tmp/ask{n}").read().split() if os.path.exists(f"/tmp/ask{n}") else ["(none)"]
ask_bg(1, f"printf 'one\\ntwo\\nthree\\n' | {EDGE} ask 'Pick a number'"); shot("a-7-ask")
chord("down"); wait(0.2); chord("enter")
a = answer(1); check("a choice: Down and Enter answer the second", a == ["two", "0"], str(a))
ask_bg(2, f"{EDGE} ask --text 'Name for the drive' < /dev/null")
text("Stick"); chord("enter")
a = answer(2); check("a text: typed and Enter", a == ["Stick", "0"], str(a))
ask_bg(3, f"printf 'a\\nb\\n' | {EDGE} ask 'Cancel me'")
chord("esc")
a = answer(3); check("Escape cancels: exit 1, no answer", a == ["1"], str(a))
at(300, 300); wait(0.4); shot("z-after-all")
check("the edge is still running", sh("pgrep -f '[i]nfynitae-edge-apps$' | head -1") != "")

sh(f"rm -f {BIN}/powerprofilesctl; sed -i '/^PANEL_CONTROL=/d; /^FAVORITES=/d' {CFG}")
sh("pkill -f '[i]nfynitae-edge-apps'; pkill -f 'wl-paste --watch'; true"); close_all(); restart_edge()
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok: print("  failed:", n)
