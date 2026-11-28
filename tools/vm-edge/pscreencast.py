#!/usr/bin/env python3
# A real screen share through xdg-desktop-portal in the test VM (PR #171: infynitae-portal shares by itself):
# CreateSession, SelectSources (a screen, then a window), Start, OpenPipeWireRemote, then the PipeWire stream
# is read with GStreamer for a few seconds while a terminal scrolls. Checks that frames arrive, their size, and
# the frame rate. Run in the session as the user (not root): XDG_RUNTIME_DIR, WAYLAND_DISPLAY set, the config's
# SCREENSHARE_ASK="false" (no question). Needs python-gobject, gst-plugin-pipewire.
import os, random, subprocess, sys, time
import gi
gi.require_version("Gst", "1.0")
from gi.repository import Gio, GLib, Gst

Gst.init(None)
bus = Gio.bus_get_sync(Gio.BusType.SESSION)
DEST, PATH = "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop"
IFACE = "org.freedesktop.portal.ScreenCast"
sender = bus.get_unique_name()[1:].replace(".", "_")
results = []

def check(name, ok, detail=""):
    results.append(bool(ok))
    print(("PASS " if ok else "FAIL ") + name, detail, flush=True)

def request(method, args, timeout=20):
    """Calls a portal method that answers through a Request object; returns (response, results)."""
    token = f"t{random.randrange(1 << 30)}"
    handle = f"/org/freedesktop/portal/desktop/request/{sender}/{token}"
    loop, out = GLib.MainLoop(), {}
    def on_resp(_c, _s, _p, _i, _sig, params):
        out["r"] = params.unpack(); loop.quit()
    sub = bus.signal_subscribe(DEST, "org.freedesktop.portal.Request", "Response", handle, None,
                               Gio.DBusSignalFlags.NO_MATCH_RULE, on_resp)
    a = list(args)
    a[-1]["handle_token"] = GLib.Variant("s", token)
    sig = {"CreateSession": "(a{sv})", "SelectSources": "(oa{sv})", "Start": "(osa{sv})"}[method]
    bus.call_sync(DEST, PATH, IFACE, method, GLib.Variant(sig, tuple(a)), None, 0, -1, None)
    GLib.timeout_add_seconds(timeout, loop.quit)
    loop.run()
    bus.signal_unsubscribe(sub)
    return out.get("r", (None, {}))

def share(kind, label):
    code, res = request("CreateSession", [{"session_handle_token": GLib.Variant("s", f"s{random.randrange(1<<30)}")}])
    if code != 0:
        check(f"{label}: session", False, f"response {code}"); return
    session = res["session_handle"]
    code, _ = request("SelectSources", [session, {"types": GLib.Variant("u", kind), "multiple": GLib.Variant("b", False),
                                                    "cursor_mode": GLib.Variant("u", 1)}])
    check(f"{label}: sources selected", code == 0, f"response {code}")
    code, res = request("Start", [session, "", {}])
    streams = res.get("streams", []) if code == 0 else []
    check(f"{label}: started with one stream", code == 0 and len(streams) == 1, f"response {code} streams {streams}")
    if not streams:
        return
    node, props = streams[0]
    fdl = Gio.UnixFDList()
    reply, out_fds = bus.call_with_unix_fd_list_sync(DEST, PATH, IFACE, "OpenPipeWireRemote",
                                                     GLib.Variant("(oa{sv})", (session, {})), None, 0, -1, fdl, None)
    fd = out_fds.get(reply.unpack()[0])
    pipe = Gst.parse_launch(f"pipewiresrc fd={fd} path={node} do-timestamp=true ! videoconvert ! appsink name=sink emit-signals=true sync=false")
    sink = pipe.get_by_name("sink")
    frames, caps = [], {}
    def on_sample(s):
        smp = s.emit("pull-sample")
        frames.append(time.monotonic())
        if not caps:
            st = smp.get_caps().get_structure(0)
            caps["w"], caps["h"] = st.get_value("width"), st.get_value("height")
        return Gst.FlowReturn.OK
    sink.connect("new-sample", on_sample)
    pipe.set_state(Gst.State.PLAYING)
    loop = GLib.MainLoop()
    GLib.timeout_add_seconds(6, loop.quit)
    loop.run()
    pipe.set_state(Gst.State.NULL)
    secs = (frames[-1] - frames[0]) if len(frames) > 1 else 0
    fps = (len(frames) - 1) / secs if secs else 0
    check(f"{label}: frames arrive", len(frames) > 30, f"{len(frames)} frames, {fps:.1f} fps, size {caps.get('w')}x{caps.get('h')} (stream size {props.get('size')})")
    bus.call_sync(DEST, session, "org.freedesktop.portal.Session", "Close", None, None, 0, -1, None)

# something that keeps changing on the screen, so frames keep coming
mover = subprocess.Popen(["sh", "-c", "exec xterm -geometry 100x30 -e sh -c 'while :; do date +%N; sleep 0.01; done'"],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
time.sleep(2)
share(1, "a screen")
share(2, "a window")
mover.terminate()
helper = subprocess.run(["pgrep", "-fa", "xdg-desktop-portal-wlr"], capture_output=True, text=True).stdout.strip()
check("no xdg-desktop-portal-wlr was used", helper == "", helper)
print(f"SUMMARY {sum(results)} of {len(results)}", flush=True)
