import sys, time, json, os
exec(open("/home/tester/g/everyday.py").read().split("names = sys.argv[1:]")[0])
# Super+Tab as Alt+Tab on Windows (2026-11-28) with real keys in the VM: a tap goes to the window used
# before; held, each Tab goes one further in the order of use and the camera follows; Shift+Tab steps
# back; letting Super go keeps the chosen window; Escape goes back to the window and camera of before;
# a minimized window comes last and comes back only on release. Needs this branch's compositor in
# ~/.local/bin/infynitae. Screenshots st-*.png in ~/results.
res = []
def ok(name, good, detail=""):
    res.append(bool(good)); print(("PASS " if good else "FAIL ") + name, detail, flush=True)
def title(ref):
    return next((e.get("title") for e in els() if e.get("id") == ref), None)
def focus_title(): return title(focused())
def cam():
    c = snap()["outputs"][0]["camera"]
    return (round(c["center"][0]), round(c["center"][1]), round(c["zoom"], 3))
def win(t): return next(e for e in els() if e.get("title") == t)
def key(k, v): emit((1, KEYS[k], v)); wait(0.05)
def tab():
    key("tab", 1); key("tab", 0); wait(0.5)
def move_to(t, x, y):
    # the shell socket's move_element (`infynitae msg` has no move)
    req = json.dumps({"id": 1, "cmd": "move_element", "element": win(t)["id"], "to": [x, y]})
    sh("python3 -c 'import socket,sys,os; s=socket.socket(socket.AF_UNIX); "
       "s.connect(os.environ[\"XDG_RUNTIME_DIR\"]+\"/infynitae/shell.sock\"); "
       "s.sendall(sys.argv[1].encode()+b\"\\n\"); print(s.recv(4096).decode())' " + "'" + req + "'")

close_all()
for t in ("st-A", "st-B", "st-C"):
    run_bg(f"xterm -T {t}"); until(lambda: any(e.get("title") == t for e in els()), 10); wait(0.8)
# far apart, so the camera has to move to each; used A, B, C in that order
for i, t in enumerate(("st-A", "st-B", "st-C")):
    move_to(t, i * 3000, 0)
wait(0.5)
ok("the windows are apart", len({win(t)["anchor"][0] for t in ("st-A", "st-B", "st-C")}) == 3, str([win(t)["anchor"] for t in ("st-A", "st-B", "st-C")]))
for t in ("st-A", "st-B", "st-C"):
    inst, el = win(t)["id"]; sh(f"infynitae msg focus {inst} {el}"); wait(0.3)
move(-3000, -3000, 30); wait(0.4)
ok("C is focused to start", focus_title() == "st-C", str(focus_title()))

# 1. a tap: the window used before; again: back
chord("super+tab"); wait(0.6)
ok("a tap goes to B", focus_title() == "st-B", str(focus_title()))
chord("super+tab"); wait(0.6)
ok("a second tap returns to C", focus_title() == "st-C", str(focus_title()))

# 2. held: B, A, then round to C; the camera follows each
key("super", 1)
seen, cams = [], []
for n in range(3):
    tab(); seen.append(focus_title()); cams.append(cam()); shot(f"st-held-{n}")
ok("held Tabs visit B, A, C", seen == ["st-B", "st-A", "st-C"], str(seen))
ok("the camera moved at each step", len(set(cams)) == 3, str(cams))
tab()  # on B
key("super", 0); wait(0.6)
ok("letting Super go keeps B", focus_title() == "st-B", str(focus_title()))
chord("super+tab"); wait(0.6)
ok("the next tap goes to C: B went first, C second", focus_title() == "st-C", str(focus_title()))

# 3. Shift+Tab steps back
key("super", 1); tab(); tab()
ok("two Tabs from C reach A", focus_title() == "st-A", str(focus_title()))
key("shift", 1); tab(); key("shift", 0)
ok("Shift+Tab steps back to B", focus_title() == "st-B", str(focus_title()))
key("super", 0); wait(0.6)

# 4. Escape goes back to the window and camera of before the first Tab
start, start_cam = focus_title(), cam()
key("super", 1); tab(); tab()
moved = cam()
key("esc", 1); key("esc", 0); wait(1.0)
ok("Escape returns the focus", focus_title() == start, f"{start} -> {focus_title()}")
ok("Escape returns the camera", cam() == start_cam and moved != start_cam, f"{start_cam} {moved} {cam()}")
key("super", 0); wait(0.6)
ok("still there after Super is let go", focus_title() == start, str(focus_title()))

# 5. a minimized window comes last and only comes back on release
inst, el = win("st-A")["id"]; sh(f"infynitae msg focus {inst} {el}"); wait(0.3)
sh(f"infynitae msg minimize {inst} {el}"); wait(0.8)
ok("A is minimized", win("st-A").get("minimized") is True, str(win("st-A")))
# with strict focus nothing may be focused after the minimize; then the first step is the last used
start = focus_title()
key("super", 1)
seen = []
while len(seen) < 4 and (not seen or seen[-1] != "st-A"):
    tab(); seen.append(focus_title())
others = [t for t in ("st-B", "st-C") if t != start]
ok("A is the last step", seen[-1:] == ["st-A"] and all(t in seen[:-1] for t in others), f"from {start}: {seen}")
ok("A stays hidden while Super is held", win("st-A").get("minimized") is True)
shot("st-minimized-held")
key("super", 0); wait(1.0)
ok("A is back on release", win("st-A").get("minimized") is False and focus_title() == "st-A", str(win("st-A")))
shot("st-minimized-released")
close_all()
print(f"SUMMARY {sum(res)} of {len(res)}", flush=True)
