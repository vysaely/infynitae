#!/usr/bin/env python3
"""Self-driving GTK probe for nested runtime validation (no input injection needed).

usage: gtkprobe.py <3|4> <mode> [arg]
modes: copy TEXT   map a window, take the clipboard (and primary) once focused, close the
                   window, keep the selection for 8 s
       paste       map a window, read the clipboard once focused, print it, exit
       windows N   create and destroy N windows one after another, exit
       fullscreen  map, request fullscreen, report the state, unfullscreen, exit
       popup       map, open a popover/menu programmatically, report, exit
Every result line starts with "RESULT ". Exit status 0 = the probe completed.
"""
import sys

ver, mode = sys.argv[1], sys.argv[2]
arg = sys.argv[3] if len(sys.argv) > 3 else None
import gi

gi.require_version("Gtk", "3.0" if ver == "3" else "4.0")
if ver == "3":
    gi.require_version("Gdk", "3.0")
from gi.repository import GLib, Gtk  # noqa: E402

if ver == "3":
    from gi.repository import Gdk  # noqa: E402


def out(*a):
    print("RESULT", *a, flush=True)


done = {"rc": 1}
loop = GLib.MainLoop()


def finish(rc=0):
    done["rc"] = rc
    loop.quit()
    return False


def new_window(title):
    w = Gtk.Window(title=title)
    w.set_default_size(320, 200)
    label = Gtk.Label(label=title)
    if ver == "3":
        w.add(label)
        w.show_all()
    else:
        w.set_child(label)
        w.present()
    return w


def when_focused(w, fn, limit_ms=4000):
    """Calls fn once the window is active (keyboard focus), or reports a timeout."""
    state = {"t": 0}

    def poll():
        if w.is_active():
            fn()
            return False
        state["t"] += 50
        if state["t"] >= limit_ms:
            out("TIMEOUT waiting for focus")
            finish(2)
            return False
        return True

    GLib.timeout_add(50, poll)


def run_copy():
    w = new_window(f"gtk{ver} copy")

    def go():
        if ver == "3":
            Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD).set_text(arg, -1)
            Gtk.Clipboard.get(Gdk.SELECTION_PRIMARY).set_text(arg + " (primary)", -1)
        else:
            w.get_clipboard().set(arg)
            w.get_primary_clipboard().set(arg + " (primary)")
        out("COPIED", repr(arg))
        # the window goes, the process (and with it the selection) stays: the next window then
        # gets the initial focus (INV-INPUT-8: a new window never takes the focus from another)
        GLib.timeout_add(300, lambda: (w.destroy(), out("COPY window closed, source kept"), False)[-1])
        GLib.timeout_add(8000, finish, 0)

    when_focused(w, go)


def run_paste():
    w = new_window(f"gtk{ver} paste")

    def go():
        if ver == "3":
            def got(_cb, text, which):
                out("PASTE", which, repr(text))
                if which == "primary":
                    finish(0)
            Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD).request_text(got, "clipboard")
            Gtk.Clipboard.get(Gdk.SELECTION_PRIMARY).request_text(got, "primary")
        else:
            def got(cb, res, which):
                try:
                    text = cb.read_text_finish(res)
                except GLib.Error as e:
                    text = f"<error {e.message}>"
                out("PASTE", which, repr(text))
                if which == "primary":
                    finish(0)
            w.get_clipboard().read_text_async(None, got, "clipboard")
            w.get_primary_clipboard().read_text_async(None, got, "primary")

    when_focused(w, go)


def run_windows():
    n = int(arg)
    state = {"i": 0, "w": None}

    def step():
        if state["w"] is not None:
            state["w"].destroy()
            state["w"] = None
        if state["i"] == n:
            out("WINDOWS created+destroyed", n)
            finish(0)
            return False
        state["i"] += 1
        state["w"] = new_window(f"churn {state['i']}")
        return True

    GLib.timeout_add(40, step)


def run_fullscreen():
    w = new_window(f"gtk{ver} fullscreen")
    seen = []

    def fs_state():
        if ver == "3":
            st = w.get_window().get_state() if w.get_window() else 0
            return bool(st & Gdk.WindowState.FULLSCREEN)
        return w.is_fullscreen()

    def check(phase, want, nxt):
        state = {"t": 0}

        def poll():
            if fs_state() == want:
                a = w.get_allocated_width(), w.get_allocated_height()
                out("FULLSCREEN", phase, "ok", "size", a)
                seen.append(phase)
                nxt()
                return False
            state["t"] += 50
            if state["t"] >= 3000:
                out("FULLSCREEN", phase, "TIMEOUT")
                finish(2)
                return False
            return True

        GLib.timeout_add(50, poll)

    def go():
        w.fullscreen()
        check("enter", True, lambda: (w.unfullscreen(), check("exit", False, lambda: finish(0))))

    when_focused(w, go)


def run_popup():
    w = new_window(f"gtk{ver} popup")

    def go():
        if ver == "3":
            pop = Gtk.Popover()
            pop.set_relative_to(w.get_child())
            pop.add(Gtk.Label(label="popover"))
            pop.show_all()
            pop.popup()
        else:
            pop = Gtk.Popover()
            pop.set_child(Gtk.Label(label="popover"))
            pop.set_parent(w.get_child())
            pop.popup()

        def report():
            out("POPUP visible", pop.get_visible(), "mapped", pop.get_mapped())
            pop.popdown()
            GLib.timeout_add(300, lambda: (out("POPUP after popdown visible", pop.get_visible()), finish(0)))
            return False

        GLib.timeout_add(700, report)

    when_focused(w, go)


{"copy": run_copy, "paste": run_paste, "windows": run_windows,
 "fullscreen": run_fullscreen, "popup": run_popup}[mode]()
GLib.timeout_add(15000, lambda: (out("TIMEOUT overall"), finish(3)))
loop.run()
sys.exit(done["rc"])
