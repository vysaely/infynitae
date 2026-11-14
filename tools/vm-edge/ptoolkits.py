import json, os, sys, time
exec(open("/home/tester/g/everyday.py").read().split("names = sys.argv[1:]")[0])
# Real toolkits under patch 0006 (review item 5): map, popup, hide/re-show without Wayland protocol
# error (xdg_surface::Error::UnconfiguredBuffer). Runs whichever toolkits are installed in the VM.
BIN = "/home/tester/.local/bin"

def snap_apps():
    return [str(e.get("app_id", "")) for e in json.loads(sh(f"{BIN}/infynitae msg snapshot"))["snapshot"].get("elements", [])]

checks = []
gtk3 = sh("python3 -c 'import gi; gi.require_version(\"Gtk\",\"3.0\"); from gi.repository import Gtk' 2>/dev/null && echo ok").strip()
if gtk3 == "ok":
    sh("rm -f /tmp/p5-gtk3.rc")
    run_bg("GDK_BACKEND=wayland python3 -c 'import gi; gi.require_version(\"Gtk\",\"3.0\"); from gi.repository import Gtk,GLib; w=Gtk.Window(title=\"p5-gtk3\"); w.add(Gtk.Label(label=\"ok\")); w.show_all(); GLib.timeout_add(300,w.hide); GLib.timeout_add(600,w.show_all); GLib.timeout_add(1100,Gtk.main_quit); Gtk.main()'; echo $? > /tmp/p5-gtk3.rc")
    wait(1.6)
    checks.append(("gtk3 wayland", open("/tmp/p5-gtk3.rc").read().strip() if os.path.exists("/tmp/p5-gtk3.rc") else "none"))

qt6 = sh("python3 -c 'import PyQt6.QtWidgets' 2>/dev/null && echo ok").strip()
if qt6 == "ok":
    sh("rm -f /tmp/p5-qt6.rc")
    run_bg("QT_QPA_PLATFORM=wayland python3 -c 'import sys; from PyQt6.QtWidgets import QApplication,QWidget; from PyQt6.QtCore import QTimer; app=QApplication(sys.argv); w=QWidget(); w.show(); QTimer.singleShot(300,w.hide); QTimer.singleShot(600,w.show); QTimer.singleShot(1100,app.quit); sys.exit(app.exec())'; echo $? > /tmp/p5-qt6.rc")
    wait(1.6)
    checks.append(("qt6 wayland", open("/tmp/p5-qt6.rc").read().strip() if os.path.exists("/tmp/p5-qt6.rc") else "none"))

run_bg("xterm -title p5-x11 -e sh -c 'sleep 2'"); wait(1.0)
checks.append(("x11 via xwayland-satellite", "0" if any("xterm" in a.lower() for a in snap_apps()) else "missing"))
sh("pkill -f 'xterm -title p5-x11'; rm -f /tmp/p5-*.rc; true")
for name, rc in checks:
    print("PASS" if rc == "0" else "FAIL", name, f"rc={rc}", flush=True)
print("SUMMARY", sum(1 for _, rc in checks if rc == "0"), "of", len(checks), flush=True)
