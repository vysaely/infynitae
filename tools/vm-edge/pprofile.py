import glob
import os
import struct
import sys
import time
import zlib

exec(
    open("/home/tester/pflow.py")
    .read()
    .split("sh(f\"sed -i '/^PANEL_CONTROL=/d' {CFG}; rm -f {BACKUP}\")")[0]
)

# Picture picker and settings save/load on the real VM edge: pick a picture (~/.face 256x256),
# save settings to ~/Documents/Infynitae, change a setting, load the saved archive, and Undo load.
BIN = "/home/tester/.local/bin"
EDGE = f"{BIN}/infynitae-edge-apps"
P = f"^{BIN}/infynitae-edge"
SET = "sh /home/tester/infynitae/live/settings.sh"
START = "SUPER+A top right 440 settings/appearance settings/games settings/about"
HOME = "/home/tester"
FACE = f"{HOME}/.face"
PIC_DIR = f"{HOME}/Pictures"
EXPORT_DIR = f"{HOME}/Documents/Infynitae"
TEST_PIC = f"{PIC_DIR}/000-pprofile-test.png"


def get(k):
    return sh(f"{SET} get {k}").strip().strip("'\"")


def apps_edge():
    sh(f"pkill -x infynitae-edge; pkill -f '{P}-rs'; pkill -f '{P}-apps'; true")
    wait(0.8)
    run_bg(f"INFYNITAE_EDGE_BOXES={BOX} {EDGE}")
    wait(2.0)


def hits():
    wait(0.4)
    d = {}
    for l in open(BOX):
        w = l.split()
        if w and w[0] == "hit":
            d[(w[1], int(w[2]))] = list(map(float, w[3:7]))
    return d


def mid(r):
    return r[0] + r[2] / 2, r[1] + r[3] / 2


def press(card, hid):
    r = hits().get((card, hid))
    if r:
        click_at(*mid(r))
        wait(0.8)
    return r is not None


def panel():
    run_bg(f"{EDGE} control")
    wait(1.5)


def write_test_png(path, w, h, rgba):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    row = b"\x00" + bytes(rgba) * w
    raw = zlib.compress(row * h)

    def chunk(tag, data):
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    ihdr = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", raw) + chunk(b"IEND", b"")
    open(path, "wb").write(png)


def png_size(path):
    data = open(path, "rb").read(24)
    if len(data) < 24 or data[:8] != b"\x89PNG\r\n\x1a\n" or data[12:16] != b"IHDR":
        return None
    return struct.unpack(">II", data[16:24])


old_gm = get("GAME_MODE")
old_face = open(FACE, "rb").read() if os.path.exists(FACE) else None
before_exports = set(glob.glob(f"{EXPORT_DIR}/settings-*.tar.gz"))
write_test_png(TEST_PIC, 320, 180, (30, 190, 80, 255))

sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}")
open(CFG, "a").write(f'PANEL_CONTROL="{START}"\n')
apps_edge()
at(300, 300)
wait(0.4)
panel()
shot("prof-1")

# 1. Pick a picture -> ~/.face changed and is 256x256 PNG
check("Your picture row is shown", press("settings/appearance", 900), str(sorted(hits())))
shot("prof-2-face-grid")
thumbs = sorted(h for (c, h) in hits() if c == "settings/appearance" and h >= 30000)
check("picture grid opens", len(thumbs) >= 1, str(thumbs[:6]))
if thumbs:
    press("settings/appearance", thumbs[0])
    wait(1.2)
new_face = open(FACE, "rb").read() if os.path.exists(FACE) else None
check("~/.face was written and changed", new_face is not None and new_face != old_face, str(len(new_face or b"")))
check("~/.face is 256x256 PNG", os.path.exists(FACE) and png_size(FACE) == (256, 256), str(png_size(FACE) if os.path.exists(FACE) else None))

# 2. Save my settings
check("Save my settings works", press("settings/about", 4), str(sorted(h for (c, h) in hits() if c == "settings/about")))
wait(1.2)
after_exports = sorted(set(glob.glob(f"{EXPORT_DIR}/settings-*.tar.gz")) - before_exports)
check("settings archive written", len(after_exports) == 1, str(after_exports))

# 3. Change a setting
press("settings/games", 101)
changed_gm = get("GAME_MODE")
check("setting changed", changed_gm != old_gm, f"{old_gm} -> {changed_gm}")

# 4. Load the saved file and check the setting is back
check("Load settings opens list", press("settings/about", 5), str(sorted(h for (c, h) in hits() if c == "settings/about")))
shot("prof-3-load-list")
items = sorted(h for (c, h) in hits() if c == "settings/about" and h >= 100)
check("saved archive listed", len(items) >= 1, str(items))
if items:
    press("settings/about", items[0])
    wait(1.0)
shot("prof-4-preview")
check("preview shows Apply and Cancel", ("settings/about", 7) in hits() and ("settings/about", 8) in hits(), str(sorted(hits())))
check("Apply loads settings", press("settings/about", 7), "apply hit missing")
wait(1.5)
check("loaded archive restored GAME_MODE", get("GAME_MODE") == old_gm, f"now={get('GAME_MODE')} want={old_gm}")

# 5. Undo load reverts and stays offered until panel closes
check("Undo load button is offered", ("settings/about", 6) in hits(), str(sorted(hits())))
shot("prof-5-undo-load")
check("Undo load reverts setting", press("settings/about", 6) and get("GAME_MODE") == changed_gm, f"now={get('GAME_MODE')} want={changed_gm}")
chord("esc")
wait(0.6)
panel()
wait(1.0)
check("Undo load gone after closing panel", ("settings/about", 6) not in hits(), str(sorted(hits())))

# Cleanup VM state
sh(f"{SET} set GAME_MODE '{old_gm}' >/dev/null 2>&1")
if os.path.exists(TEST_PIC):
    os.remove(TEST_PIC)
for p in after_exports:
    if os.path.exists(p):
        os.remove(p)
if old_face is None:
    sh(f"rm -f {FACE} {FACE}.bak")
else:
    open(FACE, "wb").write(old_face)
sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}; pkill -f '{P}-apps'; true")
restart_edge()
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok:
        print("  failed:", n)
