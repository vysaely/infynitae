import sys, os, time
exec(open("/home/tester/pflow.py").read().split("sh(f\"sed -i '/^PANEL_CONTROL=/d' {CFG}; rm -f {BACKUP}\")")[0])
# Settings cards of the edge from apps (live/edge-apps/src/cards/settings_*.rs) with real input in the
# VM: a click on a switch, a pick and a word button changes the key in the config (read back with
# settings.sh get), each change is logged, and About's Undo puts the last one back.
BIN = "/home/tester/.local/bin"
EDGE = f"{BIN}/infynitae-edge-apps"
P = f"^{BIN}/infynitae-edge"
SET = "sh /home/tester/infynitae/live/settings.sh"
START = "SUPER+A top right 440 settings/games settings/windows settings/about"
WATCH = ("GAME_MODE", "TEARING", "SUPER_ENTER_ACTION")
def get(k): return sh(f"{SET} get {k}").strip().strip("'\"")
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
def panel():
    run_bg(f"{EDGE} control"); wait(1.5)

LOG = "/home/tester/.local/state/infynitae/settings-changes"
before = {"GAME_MODE": "true", "TEARING": "always", "SUPER_ENTER_ACTION": "maximize"}
for k, v in before.items():
    sh(f"{SET} set {k} '{v}' >/dev/null 2>&1")
sh(f"rm -f {LOG}")
sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}")
open(CFG, "a").write(f'PANEL_CONTROL="{START}"\n')
apps_edge(); at(300, 300); wait(0.4)
panel(); shot("set-1")
d = hits()
check("the Games and Windows rows are there", ("settings/games", 101) in d and ("settings/windows", 132) in d, str(sorted(d)[:12]))

# 1. a switch: Game mode
gm = get("GAME_MODE")
press("settings/games", 101)
check("a switch writes its key", get("GAME_MODE") == ("false" if gm == "true" else "true"), f"{gm} -> {get('GAME_MODE')}")

# 2. a pick: Tearing opens its list, Never writes false
press("settings/games", 120); shot("set-2-list")
listed = [h for (c, h) in hits() if c == "settings/games" and 5000 <= h < 9000]
check("a pick opens its list", len(listed) == 3, str(listed))
press("settings/games", 5002)
check("choosing in the list writes the key", get("TEARING") == "false", get("TEARING"))

# 3. word buttons: Super+Enter Fit
press("settings/windows", 132)
check("a word button writes its key", get("SUPER_ENTER_ACTION") == "fit", get("SUPER_ENTER_ACTION"))

# 4. every change is in the log, and About's Undo puts the last one back
log = open(LOG).read() if os.path.exists(LOG) else ""
check("each change is logged", all(k in log for k in WATCH), log[-300:])
chord("esc"); wait(0.8); panel(); wait(1.0); shot("set-3-about")
check("About offers Undo", press("settings/about", 3), str(sorted(h for (c, h) in hits() if c == "settings/about")))
check("Undo puts the last change back", get("SUPER_ENTER_ACTION") == before["SUPER_ENTER_ACTION"], get("SUPER_ENTER_ACTION"))
# 5. the Settings index (Super+I): a card opens in place under its row and works there
chord("esc"); wait(0.6); run_bg(f"{EDGE} settings"); wait(2.0); shot("set-4-index")
rows = sorted(h for (c, h) in hits() if c == "settings/all" and h < 1000)
check("the index lists the Settings cards", len(rows) >= 9, str(rows))
# group rows are 1 + group (settings_all.rs GROUPS): Games is group 6; its card is child 8, whose
# own hits come as CHILD_STRIDE (100000) * (8 + 1) + id
press("settings/all", 7); shot("set-5-games")
inside = sorted(h for (c, h) in hits() if c == "settings/all" and h >= 100000)
check("a row opens its card in place", len(inside) >= 3, str(inside[:10]))
gm = get("GAME_MODE")
press("settings/all", 900101)
check("a switch inside the index writes its key", get("GAME_MODE") != gm, f"{gm} -> {get('GAME_MODE')}")
check("the edge is still running", sh(f"pgrep -f '^{EDGE}$' | head -1") != "")

for k, v in before.items():
    sh(f"{SET} set {k} '{v}' >/dev/null 2>&1")
sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}; pkill -f '{P}-apps'; true"); restart_edge()
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok: print("  failed:", n)
