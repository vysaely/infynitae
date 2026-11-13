import sys, os, time
exec(open("/home/tester/pflow.py").read().split("sh(f\"sed -i '/^PANEL_CONTROL=/d' {CFG}; rm -f {BACKUP}\")")[0])
# The Keys card (live/edge-apps/src/cards/settings_keys.rs) and the compositor's chord capture, with
# real keys in the VM: while the card listens a bound chord does nothing (Super+Shift+L does not
# lock), a taken chord offers Swap, Swap writes both lines and the new keys work at once, a capture
# nobody answers ends by itself after 10 s and the keyboard is free again. Needs this branch's
# compositor in ~/.local/bin/infynitae.
BIN = "/home/tester/.local/bin"
EDGE = f"{BIN}/infynitae-edge-apps"
P = f"^{BIN}/infynitae-edge"
SET = "sh /home/tester/infynitae/live/settings.sh"
START = "SUPER+A top right 440 settings/keys"
ROW_TERMINAL, SWAP = 10, 1
def get(k): return sh(f"{SET} get {k} 2>/dev/null").strip().strip("'\"")
# a live lock screen (an exited one can linger as a zombie of session.sh)
def locked(): return sh("ps -o stat= -C infynitae-lock | grep -vc Z || true").strip() not in ("0", "")
def terminals(): return int(sh("pgrep -c -x 'kitty|foot|alacritty' || true").strip() or 0)
def apps_edge():
    sh(f"pkill -x infynitae-edge; pkill -f '{P}-rs'; pkill -f '{P}-apps'; true"); wait(0.8)
    run_bg(f"INFYNITAE_EDGE_BOXES={BOX} {EDGE}"); wait(2.0)
def hits():
    wait(0.4)
    d = {}
    for l in open(BOX):
        w = l.split()
        if w and w[0] == "hit": d[(w[1], int(w[2]))] = list(map(float, w[3:7]))
    return d
def mid(r): return r[0] + r[2] / 2, r[1] + r[3] / 2
def press(card, hid):
    r = hits().get((card, hid))
    if r: click_at(*mid(r)); wait(0.8)
    return r is not None

sh(f"sed -i '/^BIND_TERMINAL=/d;/^BIND_LOCK=/d;/^PANEL_CONTROL=/d' {CFG}")
open(CFG, "a").write(f'PANEL_CONTROL="{START}"\n')
apps_edge(); at(300, 300); wait(0.4)
run_bg(f"{EDGE} control"); wait(1.5); shot("k-1")
check("the Keys card lists Open a terminal", press("settings/keys", ROW_TERMINAL), str(sorted(h for (c, h) in hits() if c == "settings/keys")[:12]))
shot("k-2-listening")

# 1. while listening, Lock's chord does not lock; it is taken, so Swap is offered
chord("super+shift+l"); wait(1.0); shot("k-3-conflict")
check("a bound chord does nothing while the card listens", not locked())
check("a taken chord offers Swap", ("settings/keys", SWAP) in hits(), str(sorted(h for (c, h) in hits() if c == "settings/keys")[:8]))

# 2. Swap writes both lines; the compositor takes them at once
press("settings/keys", SWAP); wait(1.5)
check("Swap gives Open a terminal the new keys", get("BIND_TERMINAL") == "SUPER+SHIFT+L", get("BIND_TERMINAL"))
check("Swap gives Lock the old keys", get("BIND_LOCK") == "SUPER+Q", get("BIND_LOCK"))
chord("esc"); wait(0.8)
n = terminals()
chord("super+shift+l"); wait(2.5)
check("the new keys open a terminal", terminals() > n and not locked(), f"{n} -> {terminals()}, locked {locked()}")
sh("pkill -n -x 'kitty|foot|alacritty'; true"); wait(1)

# 3. a capture nobody answers ends by itself and frees the keyboard
run_bg(f"{EDGE} control"); wait(1.5)
press("settings/keys", ROW_TERMINAL); wait(11.5)
out = "/tmp/pkeys-capture"
sh(f"rm -f {out}")
run_bg(f"{BIN}/infynitae msg capture-chord > {out} 2>&1")
wait(1.0); chord("esc"); wait(1.0)
reply = open(out).read() if os.path.exists(out) else ""
check("after 10 s the capture is over (a new one is accepted, Escape cancels it)", "busy" not in reply.lower() and reply != "", reply.strip()[:120])
check("the edge is still running", sh(f"pgrep -f '^{EDGE}$' | head -1") != "")

sh(f"sed -i '/^BIND_TERMINAL=/d;/^BIND_LOCK=/d;/^PANEL_CONTROL=/d' {CFG}; pkill -f '{P}-apps'; true"); restart_edge()
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n_, ok in RES:
    if not ok: print("  failed:", n_)
