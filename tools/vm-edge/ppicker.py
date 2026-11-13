import sys, os, time
exec(open("/home/tester/pflow.py").read().split("sh(f\"sed -i '/^PANEL_CONTROL=/d' {CFG}; rm -f {BACKUP}\")")[0])
# The accent picker (live/edge-apps/src/cards/settings_look.rs) with real input in the VM: a
# wallpaper colour square sets ACCENT to that candidate, Shuffle sets another candidate, a
# drag in "Any colour" writes once on release (one log line), a style word sets THEME_SCHEME.
# Needs ~/.local/bin/infynitae-wallpaper with --accents and ~/Pictures/Wallpapers/aurora.png.
BIN = "/home/tester/.local/bin"
EDGE = f"{BIN}/infynitae-edge-apps"
P = f"^{BIN}/infynitae-edge"
SET = "sh /home/tester/infynitae/live/settings.sh"
LOG = "/home/tester/.local/state/infynitae/settings-changes"
WP = "/home/tester/Pictures/Wallpapers/aurora.png"
START = "SUPER+A top right 440 settings/appearance"
ACCENT_ROW, TAB_ANY, SHUFFLE, SV, SQ0, SCHEME0 = 110, 20002, 20003, 20006, 20030, 20070
def get(k): return sh(f"{SET} get {k} 2>/dev/null").strip().strip("'\"").lower()
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
def press(hid):
    r = hits().get(("settings/appearance", hid))
    if r: click_at(*mid(r)); wait(1.0)
    return r is not None
def log_lines(): return open(LOG).read().count("\tACCENT\t") if os.path.exists(LOG) else 0

before = {k: get(k) for k in ("ACCENT", "THEME_SCHEME", "WALLPAPER")}
cands = [l.split()[0].lower() for l in sh(f"{BIN}/infynitae-wallpaper --accents {WP}").splitlines() if l.startswith("#")]
sh(f"{SET} set WALLPAPER '{WP}' >/dev/null 2>&1; {SET} set ACCENT '#cc70a9' >/dev/null 2>&1")
sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}")
open(CFG, "a").write(f'PANEL_CONTROL="{START}"\n')
apps_edge(); at(300, 300); wait(0.4)
run_bg(f"{EDGE} control"); wait(1.5)
check("the wallpaper has several colours", len(cands) >= 2, str(cands))
press(ACCENT_ROW); shot("pk-1-wallpaper")
sq = sorted(h for (c, h) in hits() if c == "settings/appearance" and SQ0 <= h < SQ0 + 10)
check("the picker shows the wallpaper's colours", len(sq) == len(cands), f"{sq} vs {len(cands)}")

# 1. a square sets that colour
press(SQ0 + 1)
check("a colour square sets the accent", get("ACCENT") == (cands[1] if len(cands) > 1 else ""), get("ACCENT"))

# 2. Shuffle: another of the wallpaper's colours
was = get("ACCENT")
press(SHUFFLE); wait(1.5)
now = get("ACCENT")
check("Shuffle picks another wallpaper colour", now != was and now in cands, f"{was} -> {now}")

# 3. Any colour: one drag, one change written
press(TAB_ANY); shot("pk-2-any")
r = hits().get(("settings/appearance", SV))
n = log_lines()
if r:
    drag(r[0] + r[2] * 0.2, r[1] + r[3] * 0.3, r[2] * 0.5, r[3] * 0.4)
    wait(1.5)
check("a drag in the colour square changes the accent", r is not None and get("ACCENT") != now, get("ACCENT"))
check("the drag is written once", log_lines() == n + 1, f"{n} -> {log_lines()}")

# 4. a style word
press(SCHEME0 + 1)
check("a style word sets the panels' colours", get("THEME_SCHEME") == "monochrome", get("THEME_SCHEME"))
check("the edge is still running", sh(f"pgrep -f '^{EDGE}$' | head -1") != "")

for k, v in before.items():
    sh(f"{SET} set {k} '{v}' >/dev/null 2>&1" if v else f"{SET} unset {k} >/dev/null 2>&1")
sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}; pkill -f '{P}-apps'; true"); restart_edge()
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n_, ok in RES:
    if not ok: print("  failed:", n_)
