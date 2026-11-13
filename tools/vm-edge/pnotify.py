import sys, os, time, subprocess
exec(open("/home/tester/pflow.py").read().split("sh(f\"sed -i '/^PANEL_CONTROL=/d' {CFG}; rm -f {BACKUP}\")")[0])
# The edge's own notifications (2026-11-13) with real input: it holds the bus name, pop-ups show
# and go, an action button reaches the sender, the x dismisses, a level replaces itself in place,
# Do not disturb and a game hold them back, the card lists them, nothing is left on screen.
# Shots "n-*" are looked at; "z-*" are compared on the host with "z-base".
RT = "/run/user/1000"
NOUT = "/tmp/notify-out"
sh(f"rm -rf {NOUT}; mkdir -p {NOUT}; chmod 777 {NOUT}; rm -f {RT}/infynitae/do-not-disturb {RT}/infynitae/game-quiet")
sh("pkill -x mako; true"); wait(0.5)
sh(f"sed -i '/^NOTIFICATIONS=/d; /^PANEL_CONTROL=/d' {CFG}")
open(CFG, "a").write('NOTIFICATIONS="auto"\nPANEL_CONTROL="SUPER+A top right 380 clock wifi bluetooth dnd notifications session"\n')
restart_edge(); at(300, 600); wait(1.0)
shot("z-base")

info = sh("gdbus call --session --dest org.freedesktop.Notifications --object-path /org/freedesktop/Notifications --method org.freedesktop.Notifications.GetServerInformation", 10)
check("the edge holds org.freedesktop.Notifications", "Infynitae" in info, info)

# the first pop-up's place (POPUP_W 360, MARGIN 24, strip 400 at the right of 1920)
X0, Y0, W = 1920 - 24 - 360, 24, 360
def notify(args, name):
    run_bg(f"notify-send {args} > {NOUT}/{name} 2>&1")

# 1. one shows, then goes after its time (-t 1500)
notify("-a Test -t 1500 \"Hello\" \"First line of the body\"", "n1"); wait(0.6); shot("n-1-popup")
wait(1.6); at(300, 600); wait(0.4); shot("z-after-expire")

# 2. an action button reaches the sender (--wait prints the action's key)
notify("-a Chat --wait -A reply=Reply -A mute=Mute \"Message from a friend\" \"Are you there?\"", "n2"); wait(0.8)
at(X0 + 40, Y0 + 40); wait(0.4); shot("n-2-hover-actions")
# the first button: under the body, PAD_X 18 in; the card is 2*14 + 18 + 20 + 17 + 8 + 28 = 119 tall
click_at(X0 + 18 + 25, Y0 + 119 - 14 - 14); wait(1.0)
got = open(f"{NOUT}/n2").read().strip() if os.path.exists(f"{NOUT}/n2") else ""
check("the Reply button tells the sender (ActionInvoked reply)", got == "reply", repr(got))
at(300, 600); wait(0.4); shot("z-after-action")

# 3. the x dismisses (shown while pointed at): the sender's --wait ends with no action
notify("-a Mail --wait -t 0 \"Stays until dismissed\" \"Critical-like: no timeout\"", "n3"); wait(0.8)
at(X0 + 100, Y0 + 30); wait(0.4); shot("n-3-hover-x")
click_at(X0 + W - 18 - 18 + 11, Y0 + 14 - 3 + 11); wait(1.0)
check("the x dismisses it and the sender is told (its --wait ended)", os.path.exists(f"{NOUT}/n3") and sh("pgrep -f '[S]tays until dismissed' | head -1") == "", sh(f"cat {NOUT}/n3"))
at(300, 600); wait(0.4); shot("z-after-dismiss")

# 4. the volume keys: one bar that moves, not a stack
for v in (30, 45, 60):
    notify(f"-a Audio -h string:x-canonical-private-synchronous:volume -h int:value:{v} -t 1500 \"Volume: {v}%\"", f"v{v}"); wait(0.25)
wait(0.3); shot("n-4-volume-60")
wait(2.0); at(300, 600); wait(0.4); shot("z-after-volume")

# 5. Do not disturb: a normal one held back, a critical one shows; a game: none
sh(f"mkdir -p {RT}/infynitae; : > {RT}/infynitae/do-not-disturb")
notify("-a Test -t 1500 \"Held back by Do not disturb\" \"x\"", "d1"); wait(0.8); shot("z-dnd-normal-held")
notify("-a Test -u critical -t 1500 \"Critical shows through Do not disturb\" \"x\"", "d2"); wait(0.8); shot("n-5-dnd-critical")
sh(f"rm -f {RT}/infynitae/do-not-disturb; : > {RT}/infynitae/game-quiet")
wait(2.0); notify("-a Test -u critical -t 1500 \"Held back during a game\" \"x\"", "g1"); wait(0.8); at(300, 600); wait(0.3); shot("z-game-held")
sh(f"rm -f {RT}/infynitae/game-quiet")

# the screen after each pop-up went is the desktop as before (z-* against z-base on the host)
# 6. the card lists them (newest first); Clear all empties it
chord("super+a"); wait(1.2); d = boxes(); shot("n-6-card")
check("the Notifications card lists what came", "notifications" in d["modules"], str(sorted(d["modules"])))
chord("esc"); wait(0.6)

sh(f"sed -i '/^NOTIFICATIONS=/d; /^PANEL_CONTROL=/d' {CFG}"); restart_edge()
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok: print("  failed:", n)
