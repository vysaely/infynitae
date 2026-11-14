import json, os, sys, time
exec(open("/home/tester/pflow.py").read().split("sh(f\"sed -i '/^PANEL_CONTROL=/d' {CFG}; rm -f {BACKUP}\")")[0])
# Canvas history and saved places with real input in the VM: jump to a window from the Windows
# card, then Back returns the camera to where it was; save a place, move away, and jump back to
# it by its PLACE_ chord and from the Windows card.
BIN = "/home/tester/.local/bin"
EDGE = f"{BIN}/infynitae-edge-apps"
P = f"^{BIN}/infynitae-edge"
BAK = "/tmp/pcanvas.cfg.bak"

def apps_edge():
    sh(f"pkill -x infynitae-edge; pkill -f '{P}-rs'; pkill -f '{P}-apps'; true"); wait(0.8)
    run_bg(f"INFYNITAE_EDGE_BOXES={BOX} {EDGE}"); wait(2.0)

def hits():
    wait(0.35)
    d = {}
    for l in open(BOX):
        w = l.split()
        if w and w[0] == "hit":
            d[(w[1], int(w[2]))] = list(map(float, w[3:7]))
    return d

def mid(r):
    return r[0] + r[2] / 2, r[1] + r[3] / 2

def snap():
    return json.loads(sh(f"{BIN}/infynitae msg snapshot"))["snapshot"]

def cam():
    c = snap()["outputs"][0]["camera"]
    return (c["center"][0], c["center"][1], c["zoom"])

def near(a, b, tol=4.0):
    return abs(a[0] - b[0]) <= tol and abs(a[1] - b[1]) <= tol and abs(a[2] - b[2]) <= 0.05

def close_windows():
    sh("pkill -x xterm; true"); wait(0.5)

sh(f"cp {CFG} {BAK}")
sh(f"sed -i '/^PANEL_WINDOWS=/d; /^BIND_BACK=/d; /^PLACE_CANVASTEST=/d' {CFG}")
open(CFG, "a").write('PANEL_WINDOWS="SUPER+L top center 420 search windows/list"\nBIND_BACK="SUPER+H"\n')
wait(2.5)
apps_edge(); at(300, 300); wait(0.4)

# --- 1. Jump to a window from the Windows card, then Back returns
close_windows()
run_bg("xterm"); wait(1.8)
els = [e for e in snap().get("elements", []) if "xterm" in str(e.get("app_id", "")).lower() or "xterm" in str(e.get("title", "")).lower()]
check("opened a window to jump to", len(els) >= 1, str(snap().get("elements", [])))
if els:
    e = els[0]
    win_center = (e["anchor"][0] + e["size"][0] / 2, e["anchor"][1] + e["size"][1] / 2, 1.0)
    sh(f"{BIN}/infynitae msg navigate 2400 -1200 1"); wait(0.5)
    away = cam()
    check("camera moved away from the window", not near(away, win_center), f"{away} vs {win_center}")

    run_bg(f"{EDGE} windows"); wait(1.5)
    text("xterm", 0.05); wait(0.4)
    h = hits(); shot("c-1-windows-card")
    row0 = h.get(("windows/list", 1))
    check("Windows card lists the window row", row0 is not None, str(sorted(h)))
    if row0:
        click_at(*mid(row0)); wait(0.6); shot("c-2-window-jump")
        jumped = cam()
        check("picking a window moves the camera to it", abs(jumped[0] - win_center[0]) <= 8 and abs(jumped[1] - win_center[1]) <= 8, f"{away} -> {jumped} (want {win_center})")
        chord("super+h"); wait(0.6); shot("c-3-back-from-window")
        back = cam()
        check("Back returns the camera to where it was", near(back, away), f"{jumped} -> {back} (want {away})")

# --- 2. Save a place, move away, jump back to it (PLACE_ chord and Windows card)
sh(f"{BIN}/infynitae msg navigate -900 700 1.25"); wait(0.6)
saved_line = sh(f"{BIN}/infynitae msg place CANVASTEST").strip()
check("infynitae msg place saves PLACE_CANVASTEST", saved_line.startswith("PLACE_CANVASTEST="), saved_line)
saved = cam()

# Bind SUPER+G to the saved place and let the compositor reload config
coords = saved_line.split("=", 1)[1].strip().strip('"').split()[0]
sh(f"sed -i '/^PLACE_CANVASTEST=/d' {CFG}")
open(CFG, "a").write(f'PLACE_CANVASTEST="{coords} SUPER+G"\n')
wait(2.5)
apps_edge(); at(300, 300); wait(0.4)

sh(f"{BIN}/infynitae msg navigate 1800 900 1"); wait(0.5); shot("c-4-moved-away")
moved = cam()
check("moved away from saved place", not near(moved, saved), f"{moved} vs {saved}")

chord("super+g"); wait(0.6); shot("c-5-place-chord")
check("PLACE_ chord jumps to the saved place", near(cam(), saved), f"{cam()} (want {saved})")
chord("super+h"); wait(0.6)
check("Back returns from the PLACE_ chord jump", near(cam(), moved), f"{cam()} (want {moved})")

run_bg(f"{EDGE} windows"); wait(1.5)
text("canvastest", 0.05); wait(0.5); chord("enter"); wait(0.6); shot("c-6-place-card")
check("Windows card jumps to the saved place", near(cam(), saved), f"{cam()} (want {saved})")
chord("super+h"); wait(0.6)
check("Back returns from the Windows card place jump", near(cam(), moved), f"{cam()} (want {moved})")

close_windows()
sh(f"mv {BAK} {CFG}; pkill -f '{P}-apps'; true"); wait(1.0); restart_edge()
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok:
        print("  failed:", n)
