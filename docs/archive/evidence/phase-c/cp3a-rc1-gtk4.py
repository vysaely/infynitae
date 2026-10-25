# RC-1 toolkit probe: two GTK4 windows; prints every key event GTK delivers, with modifier state.
import gi, sys
gi.require_version('Gtk', '4.0'); gi.require_version('Gdk', '4.0')
from gi.repository import Gtk, Gdk
def log(*a): print("[gtk]", *a, flush=True)
app = Gtk.Application(application_id="org.infynitae.rc1")
def mk(a, n):
    w = Gtk.ApplicationWindow(application=a, title=f"RC1-W{n}")
    w.set_default_size(300, 200)
    k = Gtk.EventControllerKey()
    k.connect("key-pressed", lambda c, kv, kc, st: log(f"W{n} key-pressed {Gdk.keyval_name(kv)} code={kc} state={st.value_names}") or False)
    k.connect("key-released", lambda c, kv, kc, st: log(f"W{n} key-released {Gdk.keyval_name(kv)} code={kc} state={st.value_names}"))
    k.connect("modifiers", lambda c, st: log(f"W{n} modifiers {st.value_names}") or False)
    w.add_controller(k)
    w.connect("notify::is-active", lambda w, p: log(f"W{n} is-active={w.is_active()}"))
    w.connect("destroy", lambda w: log(f"W{n} destroyed"))
    w.present()
def act(a):
    mk(a, 1); mk(a, 2)
app.connect("activate", act)
app.run([])
