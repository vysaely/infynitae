# OB-9 T1 client (INC-12): a GTK3 window, run under GDK_BACKEND=x11 through xwayland-satellite,
# whose 300x200 button pops a menu up at the button's bottom centre (an override-redirect X11
# window) once the window is mapped. Prints, one per line: MAPPED, POPPED, ACTIVATED <item>,
# DEACTIVATED. Exits after an activation, or after the timeout (ms) given as argument.
import sys

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gdk, GLib, Gtk  # noqa: E402

win = Gtk.Window(title="OB9")
win.set_default_size(300, 200)
win.set_resizable(False)
button = Gtk.Button(label="menu")
win.add(button)
menu = Gtk.Menu()
for name in ("First", "Second", "Third"):
    item = Gtk.MenuItem(label=name)
    item.connect("activate", lambda _w, n=name: (print("ACTIVATED", n, flush=True), Gtk.main_quit()))
    menu.append(item)
menu.show_all()
menu.connect("deactivate", lambda *_: print("DEACTIVATED", flush=True))


def pop():
    menu.popup_at_widget(button, Gdk.Gravity.SOUTH, Gdk.Gravity.NORTH_WEST, None)
    print("POPPED", flush=True)
    return False


def mapped(*_):
    print("MAPPED", flush=True)
    GLib.timeout_add(300, pop)


win.connect("map-event", mapped)
win.connect("destroy", Gtk.main_quit)
win.show_all()
GLib.timeout_add(int(sys.argv[1]) if len(sys.argv) > 1 else 30000, Gtk.main_quit)
Gtk.main()
