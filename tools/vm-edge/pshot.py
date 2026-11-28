import sys, os, time, struct, subprocess
exec(open("/home/tester/g/feat.py").read().split("which = sys.argv[1:]")[0])
# infynitae-shot with real input: the whole screen, a dragged region, a click on a window, Escape,
# the clipboard holding the PNG afterwards, and the markup editor after a pick (its INFYNITAE_SHOT_BOXES
# dump: `editor MS` from the end of the pick to its first frame, `image X Y W H`, `hit NAME X Y W H`),
# the first frame's dim and hover outline before the pointer moves, and each markup tool.
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
BOX = D + "/boxes"
def boxes():
    try: lines = open(BOX).read().split("\n")
    except OSError: return None
    out = {}
    for l in lines:
        w = l.split()
        if len(w) >= 2: out[w[0] if w[0] != "hit" else "hit " + w[1]] = [float(v) for v in w[1 if w[0] != "hit" else 2:]]
    return out if "image" in out else None
def editor_cmd(name): return f"INFYNITAE_SHOT_BOXES={BOX} {SHOT} region {D}/{name}.png"
def pick(x, y, w, h):
    # a region picked with the mouse; returns when the editor drew its first frame (None if it did not)
    sh(f"rm -f {D}/rc {BOX}")
    at(x, y); emit((1, BTN["left"], 1)); wait(0.1); move(w, h, 20); wait(0.2)
    t = time.time(); emit((1, BTN["left"], 0))
    b = until(boxes, 5, 0.005)
    return b, round((time.time() - t) * 1000)
def drag(x0, y0, x1, y1):
    at(x0, y0); emit((1, BTN["left"], 1)); wait(0.1); move(int(x1 - x0), int(y1 - y0), 20); wait(0.1); emit((1, BTN["left"], 0)); wait(0.3)
def pixels(path):
    # pixels of an 8-bit RGB or RGBA PNG (the helper saves RGB, cairo writes RGBA)
    import zlib
    data = open(path, "rb").read(); pos = 8; idat = b""; w = h = 0
    while pos < len(data):
        n, kind = struct.unpack(">I4s", data[pos:pos + 8]); body = data[pos + 8:pos + 8 + n]; pos += 12 + n
        if kind == b"IHDR": w, h = struct.unpack(">II", body[:8]); bpp = 4 if body[9] == 6 else 3
        elif kind == b"IDAT": idat += body
    raw = zlib.decompress(idat); rows = []; prev = bytearray(w * bpp); i = 0
    for _ in range(h):
        f = raw[i]; line = bytearray(raw[i + 1:i + 1 + w * bpp]); i += 1 + w * bpp
        for x in range(w * bpp):
            a = line[x - bpp] if x >= bpp else 0; b = prev[x]; c = prev[x - bpp] if x >= bpp else 0
            if f == 1: line[x] = (line[x] + a) & 255
            elif f == 2: line[x] = (line[x] + b) & 255
            elif f == 3: line[x] = (line[x] + (a + b) // 2) & 255
            elif f == 4:
                p = a + b - c; pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[x] = (line[x] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        rows.append(line); prev = line
    return lambda x, y: tuple(rows[y][x * bpp:x * bpp + 3])
def look(key, v):
    # a colour of the look file, else the helper's default `v`
    try:
        for l in open("/home/tester/.cache/infynitae/look"):
            if l.startswith(key + "="): v = l.split("=", 1)[1].strip().strip("\"'").lstrip("#")
    except OSError: pass
    return tuple(int(v[i:i + 2], 16) for i in (0, 2, 4))
def accent(): return look("LOOK_ACCENT", "7aa2f7")
def pixel(x, y):
    # one pixel of the screen as it is now
    sh(f"rm -f {D}/px.png; grim -g '{int(x)},{int(y)} 1x1' {D}/px.png")
    try: return pixels(f"{D}/px.png")(0, 0)
    except OSError: return None
def near(a, b, tol=3): return a is not None and b is not None and all(abs(p - q) <= tol for p, q in zip(a, b))

# 1. the whole screen
sh(f"rm -f {D}/rc"); bg(f"{SHOT} screen {D}/screen.png"); wait(2.0)
check("screen: the whole screen in output pixels, exit 0", size(f"{D}/screen.png") == (1920, 1080) and rc() == "0", f"{size(f'{D}/screen.png')} rc {rc()}")
clip = sh("wl-paste -l 2>/dev/null", 10)
check("and the clipboard offers image/png", "image/png" in clip, clip)
head = sh("wl-paste -t image/png | head -c 8 | od -An -tx1", 10)
check("which is a PNG", "89 50 4e 47" in head, head)

# 2. a dragged region (frozen moment): 400 x 300 from (200, 200); the editor opens, Escape keeps it
# as it is
sh(f"rm -f {D}/rc {BOX}")
bg(editor_cmd("region")); wait(1.2); shot("s-1-overlay")
at(200, 200); emit((1, BTN["left"], 1)); wait(0.1); move(200, 150, 20); move(200, 150, 20); wait(0.3); shot("s-2-dragging")
emit((1, BTN["left"], 0)); b = until(boxes, 5, 0.01); wait(0.3); shot("s-2-editor")
check("the editor opens after the pick, the picture at 1:1", b is not None and b["image"][2:] == [400, 300], str(b))
chord("esc"); wait(1.0)
check("region: Escape in the editor saves the dragged rectangle, exit 0", size(f"{D}/region.png") == (400, 300) and rc() == "0", f"{size(f'{D}/region.png')} rc {rc()}")

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
# Shift held as the pick ends: no editor, saved at once
emit((1, KEYS["shift"], 1)); wait(0.1); click(); emit((1, KEYS["shift"], 0)); wait(1.0)
got = size(f"{D}/window.png")
check("window with Shift: the clicked window's size, no editor, exit 0", rc() == "0" and got is not None and abs(got[0] - ww) <= 2 and abs(got[1] - wh) <= 2, f"{got} want {ww}x{wh} rc {rc()}")
clean()
# 5. the clipboard still holds the last one after the tool exited (its background copy serves it)
wait(1.0)
head = sh("wl-paste -t image/png | head -c 8 | od -An -tx1", 10)
check("after it exited, the clipboard still serves the last PNG", "89 50 4e 47" in head, head)

# 6. the editor: time to its first frame, a box saved with Control+S
acc = accent()
times, walls = [], []
for i in range(5):
    bg(editor_cmd(f"t{i}")); wait(1.2)
    b, wall = pick(300, 300, 400, 300)
    if b: times.append(b["editor"][0]); walls.append(wall)
    chord("esc"); wait(0.8)
print("TIMING pick end to editor frame, in the helper (ms)", times, flush=True)
print("TIMING release to the dump seen, from outside (ms)", walls, flush=True)
check("the editor drew 5 times out of 5", len(times) == 5, str(times))
bg(editor_cmd("marked")); wait(1.2)
b, _ = pick(300, 300, 400, 300)
x, y = b["image"][:2] if b else (300, 300)
chord("b"); drag(x + 50, y + 50, x + 150, y + 150); shot("s-4-box")
chord("ctrl+s"); wait(1.0)
check("Control+S saves the marked picture, exit 0", size(f"{D}/marked.png") == (400, 300) and rc() == "0", f"{size(f'{D}/marked.png')} rc {rc()}")
if size(f"{D}/marked.png"):
    p = pixels(f"{D}/marked.png")
    check("the box's left side is drawn in the accent", p(50, 100) == acc, f"{p(50, 100)} want {acc}")

# 7. crop, then Enter: copied at the crop's size, no file
bg(editor_cmd("cropped")); wait(1.2)
b, _ = pick(300, 300, 400, 300)
x, y = b["image"][:2] if b else (300, 300)
chord("c"); drag(x + 100, y + 100, x + 300, y + 200); wait(0.3); shot("s-5-cropped")
chord("enter"); wait(1.0)
sh(f"wl-paste -t image/png > {D}/clip.png", 10)
check("Enter copies the cropped picture, writes no file, exit 0", not os.path.exists(f"{D}/cropped.png") and size(f"{D}/clip.png") == (200, 100) and rc() == "0", f"{size(f'{D}/clip.png')} rc {rc()}")

# 8. undo: a box, then Control+Z, then Control+S: the picture as picked
bg(editor_cmd("undone")); wait(1.2)
b, _ = pick(300, 300, 400, 300)
x, y = b["image"][:2] if b else (300, 300)
chord("b"); drag(x + 50, y + 50, x + 150, y + 150); chord("ctrl+z"); chord("ctrl+s"); wait(1.0)
if size(f"{D}/undone.png"):
    p = pixels(f"{D}/undone.png")
    check("Control+Z took the box back", p(50, 100) != acc, f"{p(50, 100)}")
else:
    check("Control+Z took the box back", False, "no file")

# 9. Control+A while picking: the whole screen into the editor
bg(editor_cmd("all")); wait(1.2); sh(f"rm -f {BOX}"); at(500, 500); chord("ctrl+a")
b = until(boxes, 5, 0.01); chord("esc"); wait(1.0)
check("Control+A takes the whole screen", size(f"{D}/all.png") == (1920, 1080) and rc() == "0", f"{size(f'{D}/all.png')} rc {rc()}")

# 10. the plain path: Shift held as the drag ends, the file is there without the editor
plain = []
for i in range(3):
    sh(f"rm -f {D}/rc"); bg(f"{SHOT} region {D}/plain{i}.png"); wait(1.2)
    at(300, 300); emit((1, BTN["left"], 1)); wait(0.1); move(400, 300, 20); wait(0.2)
    emit((1, KEYS["shift"], 1)); wait(0.1); t = time.time(); emit((1, BTN["left"], 0))
    until(lambda: rc() is not None, 5, 0.005); plain.append(round((time.time() - t) * 1000)); emit((1, KEYS["shift"], 0)); wait(0.5)
    if i == 0:
        check("Shift held: saved without the editor, exit 0", size(f"{D}/plain0.png") == (400, 300) and rc() == "0", f"{size(f'{D}/plain0.png')} rc {rc()}")
print("TIMING Shift: release to exit (ms)", plain, flush=True)

# 11. the first frame, frozen and --live, the pointer still: the dim is the look's card at 40% over
# the screen (once: a second dim or a see-through frozen picture gives another colour), and the
# window under the pointer has its accent outline before any motion
card = look("LOOK_CARD", "111317")
clean(); run_bg("alacritty --title shotfirst"); until(lambda: find("shotfirst") is not None, 10); wait(1.0)
e = find("shotfirst"); c = snap()["outputs"][0]["camera"]; z = c["zoom"]; cx, cy = c["center"]
wx, wy = round(960 + (e["anchor"][0] - cx) * z), round(540 + (e["anchor"][1] - cy) * z)
ww, wh = round(e["size"][0] * z), round(e["size"][1] * z)
# a desktop point beside the window, away from the pointer
dx, dy = (wx - 40, wy + wh // 2) if wx > 80 else (wx + ww + 40, wy + wh // 2)
for name, flag in (("frozen", ""), ("live", " --live")):
    at(wx + ww / 2, wy + wh / 2); wait(0.5)
    base = pixel(dx, dy)
    want = tuple(round(b * 0.6 + k * 0.4) for b, k in zip(base, card)) if base else None
    sh(f"rm -f {D}/rc"); t = time.time(); bg(f"{SHOT} region{flag} {D}/first-{name}.png")
    until(lambda: pixel(dx, dy) != base, 3, 0.005); dim_ms = round((time.time() - t) * 1000)
    got = pixel(dx, dy)
    outlined = until(lambda: any(pixel(wx + d, wy + wh // 2) == acc for d in (-1, 0, 1)), 2, 0.01)
    shot(f"s-6-first-{name}")
    print(f"TIMING {name}: start to the dim on screen (ms, grim polling)", dim_ms, flush=True)
    check(f"{name}: the first frame dims once with the card at 40%", near(got, want), f"base {base} got {got} want {want}")
    check(f"{name}: the hovered window is outlined before the pointer moves", bool(outlined), f"edge x {wx} y {wy + wh // 2}")
    chord("esc"); wait(0.8)
    check(f"{name}: Escape cancels, exit 1", rc() == "1", f"rc {rc()}")
clean()

# 12. the other tools on a picked 400 x 300 rectangle, saved with Control+S: arrow and pen leave the
# accent on their line, text in the accent near where it was clicked, pixelate keeps the size
def marked(name, tool, act):
    bg(editor_cmd(name)); wait(1.2)
    b, _ = pick(300, 300, 400, 300)
    x, y = b["image"][:2] if b else (300, 300)
    chord(tool); act(x, y); wait(0.3); shot(f"s-7-{name}")
    chord("ctrl+s"); wait(1.0)
    ok = size(f"{D}/{name}.png") == (400, 300) and rc() == "0"
    check(f"{name}: saved at the picked size, exit 0", ok, f"{size(f'{D}/{name}.png')} rc {rc()}")
    return pixels(f"{D}/{name}.png") if ok else None
for name, tool in (("arrow", "a"), ("pen", "p")):
    p = marked(name, tool, lambda x, y: drag(x + 50, y + 150, x + 250, y + 150))
    check(f"{name}: the line is in the accent", p is not None and near(p(120, 150), acc, 8), f"{p(120, 150) if p else None} want {acc}")
def type_ab(x, y):
    at(x + 50, y + 150); click(); wait(0.2); chord("a"); chord("b")
p = marked("text", "t", type_ab)
hit = p is not None and any(near(p(px, py), acc, 8) for px in range(48, 90) for py in range(130, 160))
check("text: the typed letters are in the accent", hit, "no accent pixel in (48..90, 130..160)")
marked("pixelate", "x", lambda x, y: drag(x + 50, y + 50, x + 250, y + 200))

print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok: print("  failed:", n)
