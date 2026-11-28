import sys, time, json, os
exec(open("/home/tester/g/everyday.py").read().split("names = sys.argv[1:]")[0])
# The compositor keeps the clipboard and its history (PR #173), with no wl-clip-persist or cliphist running:
# a copy outlives the app that made it (Wayland and X11), pastes across both, and the history lists it.
res = []
def ok(name, good, detail=""):
    res.append(bool(good)); print(("PASS " if good else "FAIL ") + name, detail, flush=True)
def paste(): return sh("timeout 3 wl-paste -n 2>&1").strip()
ok("the compositor answers for the clipboard", sh("infynitae msg clipboard >/dev/null 2>&1 && echo yes") == "yes")
ok("no wl-clip-persist running", sh("pgrep -x wl-clip-persist") == "")
for i in range(3):
    close_all(); sh("rm -rf ~/.local/share/Mousepad ~/.config/Mousepad; true")
    run_bg("mousepad"); until(lambda: has("mousepad"), 10); wait(1.5)
    t = f"wl{i}copy{int(time.time()) % 10000}"
    text(t, 0.04); chord("ctrl+a"); chord("ctrl+c"); wait(0.6)
    ok(f"round {i}: a Wayland copy reaches wl-paste", paste() == t, paste())
    sh("pkill -x mousepad; true"); wait(1.5)
    ok(f"round {i}: it outlives the Wayland app", paste() == t, paste())
    t2 = f"x11copy{i}"
    # an X11 copy reaches Wayland while an X11 window has focus (the bridge takes the selection then)
    run_bg("xterm -T pclip-x11"); until(lambda: has("pclip-x11"), 10); wait(1.0)
    run_bg(f"printf {t2} | xclip -selection clipboard -i"); wait(1.2)
    ok(f"round {i}: an X11 copy reaches wl-paste", paste() == t2, paste())
    sh("pkill -x xclip; pkill -f 'xterm -T pclip-x11'; true"); wait(1.5)
    ok(f"round {i}: it outlives the X11 app", paste() == t2, paste())
hist = sh("infynitae msg clipboard 2>&1")
ok("the history holds both kinds", "x11copy2" in hist and "wl2copy" in hist, hist.splitlines()[:4])
print(f"SUMMARY {sum(res)} of {len(res)}", flush=True)
