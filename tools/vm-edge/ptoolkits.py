import json, os, sys, time
exec(open("/home/tester/g/everyday.py").read().split("names = sys.argv[1:]")[0])
# Real toolkits under patch 0006 (review item 5): map, popup, hide/re-show without Wayland protocol
# error (xdg_surface::Error::UnconfiguredBuffer). Runs whichever toolkits are installed in the VM.
BIN = "/home/tester/.local/bin"

def run_py(name, env, code):
    # run_bg wraps its command in single quotes: the program goes in a file instead
    open(f"/tmp/{name}.py", "w").write(code)
    sh(f"rm -f /tmp/{name}.rc")
    run_bg(f"{env} python3 /tmp/{name}.py; echo $? > /tmp/{name}.rc")

def snap_apps():
    return [str(e.get("app_id", "")) for e in json.loads(sh(f"{BIN}/infynitae msg snapshot"))["snapshot"].get("elements", [])]

checks = []
gtk3 = sh("python3 -c 'import gi; gi.require_version(\"Gtk\",\"3.0\"); from gi.repository import Gtk' 2>/dev/null && echo ok").strip()
if gtk3 == "ok":
    run_py("p5-gtk3", "GDK_BACKEND=wayland", 'import gi\ngi.require_version("Gtk","3.0")\nfrom gi.repository import Gtk,GLib\nw=Gtk.Window(title="p5-gtk3"); w.add(Gtk.Label(label="ok")); w.show_all()\nGLib.timeout_add(300,w.hide); GLib.timeout_add(600,w.show_all); GLib.timeout_add(1100,Gtk.main_quit)\nGtk.main()\n')
    wait(1.6)
    checks.append(("gtk3 wayland", open("/tmp/p5-gtk3.rc").read().strip() if os.path.exists("/tmp/p5-gtk3.rc") else "none"))

qt6 = sh("python3 -c 'import PyQt6.QtWidgets' 2>/dev/null && echo ok").strip()
if qt6 == "ok":
    run_py("p5-qt6", "QT_QPA_PLATFORM=wayland", 'import sys\nfrom PyQt6.QtWidgets import QApplication,QWidget\nfrom PyQt6.QtCore import QTimer\napp=QApplication(sys.argv); w=QWidget(); w.show()\nQTimer.singleShot(300,w.hide); QTimer.singleShot(600,w.show); QTimer.singleShot(1100,app.quit)\nsys.exit(app.exec())\n')
    wait(1.6)
    checks.append(("qt6 wayland", open("/tmp/p5-qt6.rc").read().strip() if os.path.exists("/tmp/p5-qt6.rc") else "none"))

open("/tmp/p5-x11.sh", "w").write("xterm -title p5-x11 -e sleep 3\n")
run_bg("sh /tmp/p5-x11.sh"); wait(1.5)
checks.append(("x11 via xwayland-satellite", "0" if any("xterm" in a.lower() for a in snap_apps()) else "missing"))
sh("pkill -f 'xterm -title p5-x11'; rm -f /tmp/p5-*; true")
for name, rc in checks:
    print("PASS" if rc == "0" else "FAIL", name, f"rc={rc}", flush=True)
print("SUMMARY", sum(1 for _, rc in checks if rc == "0"), "of", len(checks), flush=True)
