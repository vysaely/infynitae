import sys, os, time
exec(open("/home/tester/vmlib.py").read())
# Shortcut help overlay (live/edge-apps/src/shortcuts.rs, Super+/): opens with user BIND_ /
# EXEC_ overrides, shrinks/filters while typing, closes on Escape / Super+/ / outside click, and
# "Change shortcuts" opens Settings > Keys. Needs this branch's compositor and
# infynitae-edge-apps in ~/.local/bin.
BIN = "/home/tester/.local/bin"
EDGE = f"{BIN}/infynitae-edge-apps"
P = f"^{BIN}/infynitae-edge"
KEY_CFG = 'BIND_TERMINAL="SUPER+T"'
EXEC_CFG = 'EXEC_SHORTCUTTEST="SUPER+B:true"'

def apps_edge():
    sh(f"pkill -x infynitae-edge; pkill -f '{P}-rs'; pkill -f '{P}-apps'; true"); wait(0.8)
    run_bg(f"INFYNITAE_EDGE_BOXES={BOX} {EDGE}"); wait(2.0)

def dump_state():
    wait(0.4)
    box = None
    panel = None
    d = {}
    for l in open(BOX):
        w = l.split()
        if not w:
            continue
        if w[0] == "shortcuts":
            box = list(map(float, w[1:5]))
        elif w[0] == "panel":
            panel = w[1]
        elif w[0] == "hit":
            d[(w[1], int(w[2]))] = list(map(float, w[3:7]))
    raw = open(BOX).read().strip()
    return raw, box, panel, d

def mid(r):
    return r[0] + r[2] / 2, r[1] + r[3] / 2

sh(f"sed -i '/^BIND_TERMINAL=/d;/^EXEC_SHORTCUTTEST=/d' {CFG}")
open(CFG, "a").write(f"{KEY_CFG}\n{EXEC_CFG}\n")
apps_edge(); at(200, 200); wait(0.4)

# 1. Super+/ opens the shortcut help overlay
chord("super+/"); wait(1.0); shot("sh-1-open")
raw, box_full, _, d = dump_state()
check("Super+/ opens the shortcut help overlay", box_full is not None and ("shortcuts/change", 1) in d, raw[:200])

# 2. Type to filter narrows the list (overlay height shrinks) and matches user remapped chord
text("super t"); wait(0.5); shot("sh-2-filter")
raw_f, box_filt, _, _ = dump_state()
check(
    "typing a user-remapped chord filters the overlay",
    box_filt is not None and box_full is not None and box_filt[3] < box_full[3],
    f"{box_full} -> {box_filt}",
)

# 3. Escape closes
chord("esc"); wait(0.8)
raw_c, box_c, _, _ = dump_state()
check("Escape closes the shortcut help overlay", raw_c == "closed" and box_c is None, raw_c)

# 4. Same key (Super+/) toggles open then closed
chord("super+/"); wait(0.8)
_, box_reopen, _, _ = dump_state()
chord("super+/"); wait(0.8)
raw_toggled, _, _, _ = dump_state()
check(
    "Super+/ opens and closes the overlay",
    box_reopen is not None and raw_toggled == "closed",
    f"reopen={box_reopen} after={raw_toggled}",
)

# 5. Clicking outside closes
chord("super+/"); wait(0.8)
_, box_out, _, _ = dump_state()
if box_out:
    out_x = max(5.0, box_out[0] - 20.0)
    out_y = max(5.0, box_out[1] + 20.0)
    click_at(out_x, out_y); wait(0.8)
raw_outside, _, _, _ = dump_state()
check("clicking outside closes the overlay", raw_outside == "closed", raw_outside)

# 6. Clicking "Change shortcuts" closes overlay and opens Settings > Keys
chord("super+/"); wait(0.8)
_, _, _, d2 = dump_state()
btn = d2.get(("shortcuts/change", 1))
if btn:
    click_at(*mid(btn)); wait(1.5); shot("sh-3-keys")
raw_keys, box_after, panel_name, hits_keys = dump_state()
check(
    "Change shortcuts opens Settings with Keys expanded",
    box_after is None and panel_name == "settings" and ("settings/all", 700010) in hits_keys,
    f"panel={panel_name} keys_hit={('settings/all', 700010) in hits_keys}",
)

chord("esc"); wait(0.6)
sh(f"sed -i '/^BIND_TERMINAL=/d;/^EXEC_SHORTCUTTEST=/d' {CFG}; pkill -f '{P}-apps'; true"); restart_edge()
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n_, ok in RES:
    if not ok:
        print("  failed:", n_)
