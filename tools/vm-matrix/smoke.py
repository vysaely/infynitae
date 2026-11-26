import argparse, fcntl, os, struct, sys, time
# The short smoke check of one VM shape (tools/vm-matrix/run copies it in and runs it as root in the
# session, `sudo -E python3 ~/vm-matrix-smoke.py ...`): the session is up with the screens and scales the
# shape asks for, a terminal opens, Super+A opens the control menu, a screenshot, lock and unlock; the
# laptop shape adds a battery and a touchpad tap, the no-XWayland shape an X11 app that must not start.
# One line per check: RESULT<TAB>check<TAB>detail<TAB>screenshot, RESULT is PASS, FAIL or SKIP.
p = argparse.ArgumentParser()
p.add_argument("--screens", type=int, default=1)
p.add_argument("--scales", default="1")
p.add_argument("--mode", default="1920x1080")
p.add_argument("--laptop", action="store_true")
p.add_argument("--no-xwayland", action="store_true")
args = p.parse_args()
sys.argv = sys.argv[:1]  # everyday.py reads its own arguments as test names
exec(open("/home/tester/g/everyday.py").read().split("names = sys.argv[1:]")[0])

def check(name, ok, detail="", picture=""):
    res = "SKIP" if ok is None else ("PASS" if ok else "FAIL")
    path = f"{OUT}/{picture}.png" if picture else ""
    print(f"{res}\t{name}\t{str(detail).replace(chr(9), ' ')[:300]}\t{path}", flush=True)

def locked(): return snap().get("locked") is True

def compositor(): return sh("P=''; F=\"$HOME/.local/state/infynitae/compositor.pid\"; [ -r \"$F\" ] && read -r P < \"$F\"; case \"$P\" in \"\"|*[!0-9]*) P='' ;; *) { [ -d \"/proc/$P\" ] && read -r C < \"/proc/$P/comm\" 2>/dev/null && case \"$C\" in infynitae|infynitae.*) true ;; *) false ;; esac; } || P='' ;; esac; [ -n \"$P\" ] && echo \"$P\" || pgrep -f -- 'infynitae --tty' | head -1")

def png_size(path):
    try:
        with open(path, "rb") as f:
            h = f.read(24)
    except OSError:
        return None
    return struct.unpack(">II", h[16:24]) if h[:8] == b"\x89PNG\r\n\x1a\n" else None

def wholly_on_screen(e, outs):
    # the window's rectangle inside one screen's view, in canvas units (check.sh's sum, per screen)
    (ax, ay), (ew, eh) = e["anchor"], e["size"]
    for o in outs:
        if not o.get("camera"):
            continue
        (cx, cy), z = o["camera"]["center"], o["camera"]["zoom"]
        vw, vh = o["mode"][0] / o["scale"] / z, o["mode"][1] / o["scale"] / z
        if ax >= cx - vw / 2 and ay >= cy - vh / 2 and ax + ew <= cx + vw / 2 and ay + eh <= cy + vh / 2:
            return o["name"]
    return None

# 1. the session: up, and unlocked with the password at its first lock screen
pid = compositor()
s = snap()
check("session up", bool(pid) and "error" not in s, f"compositor pid {pid or 'none'} {s.get('error', '')}")
if locked() or sh("pgrep -x infynitae-lock") != "":
    text("tester"); chord("enter")
check("unlocks at start", until(lambda: not locked(), 10), f"locked {snap().get('locked')}")
close_all()

# 2. the screens, their modes and scales
outs = sorted(snap().get("outputs", []), key=lambda o: o["name"])
want_scales = [float(x) for x in args.scales.split(",")]
want_w, want_h = (int(x) for x in args.mode.split("x"))
seen = [(o["name"], o["mode"], o["scale"], o["position"]) for o in outs]
check("screens", len(outs) == args.screens, f"want {args.screens}, seen {seen}")
check("scale", len(outs) == len(want_scales) and all(abs(o["scale"] - k) < 0.01 for o, k in zip(outs, want_scales)),
      f"want {want_scales}, seen {[o['scale'] for o in outs]}")
check("mode of the first screen", bool(outs) and outs[0]["mode"] == [want_w, want_h], f"want {want_w}x{want_h}, seen {seen[:1]}")
if args.screens > 1 and len(outs) > 1:
    (x0, y0), (x1, y1) = outs[0]["position"], outs[1]["position"]
    check("side by side", x1 != x0 and y1 == y0, f"positions {outs[0]['position']} {outs[1]['position']}")

# 3. a terminal by its key, wholly on a screen
before = len(els())
chord("super+q")
opened = until(lambda: len(els()) > before, 10)
wait(1.5); shot("mx-terminal")
new = els()[before:] if opened else []
where = wholly_on_screen(new[0], snap().get("outputs", [])) if new else None
check("Super+Q opens a terminal", bool(opened), str([(e.get("app_id"), e.get("size")) for e in new]), "mx-terminal")
check("the terminal is wholly on a screen", where is not None, f"on {where}" if where else str([(e.get("anchor"), e.get("size")) for e in new]))
close_all()

# 4. Super+A: the edge, restarted with its box dump (as tools/vm-edge/pflow.py does), draws the control menu
BOX = "/tmp/vm-matrix-boxes"
sh(f"rm -f {BOX}; pkill -x infynitae-edge; true"); wait(1.0)
run_bg(f"INFYNITAE_EDGE_BOXES={BOX} infynitae-edge"); wait(2.5)
def boxes():
    try: return open(BOX).read()
    except OSError: return ""
def menu_open(): return any(l.split()[0] in ("panel", "card") for l in boxes().splitlines() if l.split())
chord("super+a")
shown = until(menu_open, 6)
wait(0.8); shot("mx-control-menu")
check("Super+A opens the control menu", bool(shown), (boxes().splitlines() or ["no box dump: the edge did not write one"])[0], "mx-control-menu")
chord("esc")
check("Escape closes it", until(lambda: boxes().strip() == "closed", 4), (boxes().splitlines() or [""])[0])

# 5. a screenshot through the compositor's capture
grim = f"{OUT}/mx-screenshot.png"
sh(f"rm -f {grim}; grim {grim}")
got = png_size(grim)
want = (want_w, want_h) if args.screens == 1 else None
check("screenshot (grim)", got is not None and (want is None or got == want), f"size {got}, want {want or 'any'}", "mx-screenshot")

# 6. lock and unlock with real keys
chord("super+shift+l")
check("Super+Shift+L locks", until(lambda: locked() and sh("pgrep -x infynitae-lock") != "", 8), f"locked {snap().get('locked')}")
wait(1.5); shot("mx-locked")
text("tester"); chord("enter")
check("the password unlocks", until(lambda: not locked(), 10), f"locked {snap().get('locked')}", "mx-locked")

# 7. the laptop: a battery from the kernel's test_power module, and a touchpad tap
if args.laptop:
    PS, PAR = "/sys/class/power_supply", "/sys/module/test_power/parameters"
    r = subprocess.run(["modprobe", "test_power"], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    if r.returncode != 0:
        check("battery (test_power)", None, f"modprobe test_power: {r.stdout.strip()}")
    else:
        present = until(lambda: os.path.exists(f"{PS}/test_battery"), 5)
        check("battery (test_power) seen by the kernel", bool(present), str(sorted(os.listdir(PS))))
        for k, v in (("ac_online", "off"), ("battery_status", "discharging"), ("battery_capacity", "5")):
            open(f"{PAR}/{k}", "w").write(v)
        wait(4)
        up = sh("upower -i /org/freedesktop/UPower/devices/battery_test_battery 2>&1 | grep -E 'percentage|state' | tr -s ' '")
        check("on battery at 5%: the compositor runs on", compositor() == pid, f"pid {pid} -> {compositor() or 'none'}; upower: {up or 'no answer'}")
        shot("mx-battery-low")
        open(f"{PAR}/ac_online", "w").write("on")
    bl = sorted(os.listdir("/sys/class/backlight")) if os.path.isdir("/sys/class/backlight") else []
    check("backlight", True if bl else None, f"{bl}" if bl else "no backlight device: the kernel has no fake one, only real hardware shows it")
    # a touchpad through uinput: absolute multitouch axes, finger tools and a button pad, which libinput
    # takes for a touchpad. A tap outside the open control menu must close it (the tap is a click), as
    # a mouse click there does: that is tried first, else the tap has nothing to show
    chord("super+a"); until(menu_open, 6)
    move(-3000, 3000, 30); click()
    mouse_closes = until(lambda: boxes().strip() == "closed", 4)
    chord("esc")
    UI_SET_EVBIT, UI_SET_KEYBIT, UI_SET_ABSBIT, UI_SET_PROPBIT = 0x40045564, 0x40045565, 0x40045567, 0x4004556E
    UI_DEV_SETUP, UI_ABS_SETUP, UI_DEV_CREATE, UI_DEV_DESTROY = 0x405C5503, 0x401C5504, 0x5501, 0x5502
    ABS_X, ABS_Y, ABS_MT_SLOT, ABS_MT_POSITION_X, ABS_MT_POSITION_Y, ABS_MT_TRACKING_ID = 0x00, 0x01, 0x2F, 0x35, 0x36, 0x39
    BTN_LEFT, BTN_TOOL_FINGER, BTN_TOUCH, BTN_TOOL_DOUBLETAP = 0x110, 0x145, 0x14A, 0x14D
    TW, TH, PER_MM = 1200, 800, 12  # a 100 x 67 mm pad
    tp = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
    for ev in (0, 1, 3):
        fcntl.ioctl(tp, UI_SET_EVBIT, ev)
    for k in (BTN_LEFT, BTN_TOOL_FINGER, BTN_TOUCH, BTN_TOOL_DOUBLETAP):
        fcntl.ioctl(tp, UI_SET_KEYBIT, k)
    fcntl.ioctl(tp, UI_SET_PROPBIT, 0)  # INPUT_PROP_POINTER
    fcntl.ioctl(tp, UI_SET_PROPBIT, 2)  # INPUT_PROP_BUTTONPAD
    for code, hi in ((ABS_X, TW), (ABS_Y, TH), (ABS_MT_SLOT, 1), (ABS_MT_POSITION_X, TW), (ABS_MT_POSITION_Y, TH), (ABS_MT_TRACKING_ID, 65535)):
        fcntl.ioctl(tp, UI_SET_ABSBIT, code)
        res = PER_MM if code in (ABS_X, ABS_Y, ABS_MT_POSITION_X, ABS_MT_POSITION_Y) else 0
        fcntl.ioctl(tp, UI_ABS_SETUP, struct.pack("<Hxxiiiiii", code, 0, 0, hi, 0, 0, res))
    fcntl.ioctl(tp, UI_DEV_SETUP, struct.pack("<4H80sI", 3, 0x1234, 0x5679, 1, b"vm-matrix touchpad", 0))
    fcntl.ioctl(tp, UI_DEV_CREATE); wait(2)
    def tp_emit(*ev):
        for t, c, v in ev: os.write(tp, struct.pack("<qqHHi", 0, 0, t, c, v))
        os.write(tp, struct.pack("<qqHHi", 0, 0, 0, 0, 0))
    track = [0]
    def finger(x0, y0, x1, y1, steps, hold):
        track[0] += 1
        tp_emit((3, ABS_MT_SLOT, 0), (3, ABS_MT_TRACKING_ID, track[0]), (3, ABS_MT_POSITION_X, x0), (3, ABS_MT_POSITION_Y, y0),
                (3, ABS_X, x0), (3, ABS_Y, y0), (1, BTN_TOUCH, 1), (1, BTN_TOOL_FINGER, 1))
        for i in range(1, steps + 1):
            x, y = x0 + (x1 - x0) * i // steps, y0 + (y1 - y0) * i // steps
            tp_emit((3, ABS_MT_POSITION_X, x), (3, ABS_MT_POSITION_Y, y), (3, ABS_X, x), (3, ABS_Y, y)); wait(0.012)
        wait(hold)
        tp_emit((3, ABS_MT_TRACKING_ID, -1), (1, BTN_TOUCH, 0), (1, BTN_TOOL_FINGER, 0)); wait(0.3)
    chord("super+a"); until(menu_open, 6)
    for _ in range(6):  # swipes toward the bottom left corner, away from the menu
        finger(TW - 100, 100, 100, TH - 100, 30, 0.05)
    open_after_moves = boxes().strip() != "closed"
    shot("mx-touchpad-before-tap")
    finger(TW // 2, TH // 2, TW // 2, TH // 2, 1, 0.04)  # a tap: down and up in place, briefly
    closed = until(lambda: boxes().strip() == "closed", 4)
    shot("mx-touchpad-after-tap")
    check("touchpad tap clicks (closes the control menu)", (open_after_moves and bool(closed)) if mouse_closes else None,
          f"menu open after the swipes: {open_after_moves}; closed after the tap: {bool(closed)}" if mouse_closes
          else "a mouse click outside does not close the control menu either: the tap has nothing to show", "mx-touchpad-after-tap")
    chord("esc")
    fcntl.ioctl(tp, UI_DEV_DESTROY)

# 8. without XWayland: an X11 app finds no X server, the session runs on
if args.no_xwayland:
    close_all()
    run_bg("xterm -T vm-matrix-x11"); wait(5)
    sat = sh("pgrep -x xwayland-satell; pgrep -x Xwayland; true")
    check("XWAYLAND=false: no X server, no X11 window", not has("vm-matrix-x11") and sat == "", f"windows {ids()}; X servers {sat or 'none'}")
    check("XWAYLAND=false: the compositor runs on", compositor() == pid, f"pid {pid} -> {compositor() or 'none'}")
    close_all()

sh("pkill -x infynitae-edge; true"); wait(0.8); run_bg("infynitae-edge")
fcntl.ioctl(fd, 0x5502)
