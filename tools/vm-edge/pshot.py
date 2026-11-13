import sys, os, time, struct, subprocess
exec(open("/home/tester/g/feat.py").read().split("which = sys.argv[1:]")[0])
# infynitae-shot with real input: the whole screen, a dragged region, a click on a window, Escape,
# and the clipboard holding the PNG afterwards.
SHOT = "/home/tester/.local/bin/infynitae-shot"
D = "/tmp/shots"
sh(f"rm -rf {D}; mkdir -p {D}; chmod 777 {D}")
def size(path):
    try:
        with open(path, "rb") as f:
            h = f.read(24)
        return struct.unpack(">II", h[16:24]) if h[:8] == b"\x89PNG\r\n\x1a\n" else None
    except OSError:
        return None
def bg(cmd):
    run_bg(cmd + " ; echo $? > " + D + "/rc")
def rc():
    try: return open(D + "/rc").read().strip()
    except OSError: return None
def at(x, y): move(-3000, -3000, 30); move(int(x), int(y), 30); wait(0.3)

# 1. the whole screen
sh(f"rm -f {D}/rc"); bg(f"{SHOT} screen {D}/screen.png"); wait(2.0)
check("screen: the whole screen in output pixels, exit 0", size(f"{D}/screen.png") == (1920, 1080) and rc() == "0", f"{size(f'{D}/screen.png')} rc {rc()}")
clip = sh("wl-paste -l 2>/dev/null", 10)
check("and the clipboard offers image/png", "image/png" in clip, clip)
head = sh("wl-paste -t image/png | head -c 8 | od -An -tx1", 10)
check("which is a PNG", "89 50 4e 47" in head, head)

# 2. a dragged region (frozen moment): 400 x 300 from (200, 200)
sh(f"rm -f {D}/rc")
bg(f"{SHOT} region {D}/region.png"); wait(1.2); shot("s-1-overlay")
at(200, 200); emit((1, BTN["left"], 1)); wait(0.1); move(200, 150, 20); move(200, 150, 20); wait(0.3); shot("s-2-dragging")
emit((1, BTN["left"], 0)); wait(0.3); chord("enter"); wait(1.0)
check("region: the dragged rectangle, exit 0", size(f"{D}/region.png") == (400, 300) and rc() == "0", f"{size(f'{D}/region.png')} rc {rc()}")

# 3. Escape: nothing written, exit 1
sh(f"rm -f {D}/rc")
bg(f"{SHOT} region {D}/esc.png"); wait(1.2); chord("esc"); wait(0.8)
check("Escape cancels: no file, exit 1", not os.path.exists(f"{D}/esc.png") and rc() == "1", f"rc {rc()}")

# 4. a click on a window: that window's rectangle
clean(); run_bg("alacritty --title shotwin"); until(lambda: find("shotwin") is not None, 10); wait(1.0)
e = find("shotwin"); c = snap()["outputs"][0]["camera"]; z = c["zoom"]; cx, cy = c["center"]
wx, wy = 960 + (e["anchor"][0] - cx) * z, 540 + (e["anchor"][1] - cy) * z
ww, wh = round(e["size"][0] * z), round(e["size"][1] * z)
sh(f"rm -f {D}/rc")
bg(f"{SHOT} window {D}/window.png"); wait(1.2); at(wx + ww / 2, wy + wh / 2); wait(0.3); shot("s-3-window-hover")
click(); wait(0.3); chord("enter"); wait(1.0)
got = size(f"{D}/window.png")
check("window: the clicked window's size, exit 0", rc() == "0" and got is not None and abs(got[0] - ww) <= 2 and abs(got[1] - wh) <= 2, f"{got} want {ww}x{wh} rc {rc()}")
clean()
# 5. the clipboard still holds the last one after the tool exited (its background copy serves it)
wait(1.0)
head = sh("wl-paste -t image/png | head -c 8 | od -An -tx1", 10)
check("after it exited, the clipboard still serves the last PNG", "89 50 4e 47" in head, head)

print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok: print("  failed:", n)
