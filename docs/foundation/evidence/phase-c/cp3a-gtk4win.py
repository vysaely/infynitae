import gi, sys
gi.require_version('Gtk', '4.0')
from gi.repository import Gtk, GLib
app = Gtk.Application(application_id="org.infynitae.probe4")
def act(a):
    w = Gtk.ApplicationWindow(application=a, title="GTK4PROBE")
    w.set_default_size(600, 400)
    w.set_child(Gtk.Label(label="probe"))
    w.present()
app.connect("activate", act)
app.run([])
