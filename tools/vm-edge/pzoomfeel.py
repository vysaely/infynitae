#!/usr/bin/env python3
"""
pzoomfeel.py — measure pointer canvas-point drift and frames-to-settle for each ZOOM_FEEL.

In an Infynitae VM session: cycles ZOOM_FEEL through instant, quick, smooth, floaty
(restoring the config file on exit), injects Super+Shift+Wheel in and out at a fixed
pointer position, reads every camera update over shell.sock, and prints the largest
canvas-point drift and frames to settle.

Usage:
  python3 tools/vm-edge/pzoomfeel.py
  python3 tools/vm-edge/pzoomfeel.py --self-test
"""

from __future__ import annotations

import argparse
import fcntl
import json
import math
import os
import re
import select
import socket
import struct
import subprocess
import tempfile
import time
from pathlib import Path
from typing import Dict, List, Optional, Tuple

FEELS = ("instant", "quick", "smooth", "floaty")

# Linux uinput / evdev constants
EV_SYN, EV_KEY, EV_REL = 0x00, 0x01, 0x02
SYN_REPORT = 0x00
REL_X, REL_Y, REL_WHEEL, REL_WHEEL_HI_RES = 0x00, 0x01, 0x08, 0x0B
KEY_LEFTSHIFT, KEY_LEFTMETA = 42, 125
UI_SET_EVBIT = 0x40045564
UI_SET_KEYBIT = 0x40045565
UI_SET_RELBIT = 0x40045566
UI_DEV_SETUP = 0x405C5503
UI_DEV_CREATE = 0x5501
UI_DEV_DESTROY = 0x5502
BUS_VIRTUAL = 0x06


class UInputWheel:
    """Minimal virtual mouse+modifiers for Super+Shift+Wheel at a fixed screen point."""

    def __init__(self) -> None:
        self.fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
        for ev in (EV_KEY, EV_REL):
            fcntl.ioctl(self.fd, UI_SET_EVBIT, ev)
        for key in (KEY_LEFTSHIFT, KEY_LEFTMETA, 0x110):
            fcntl.ioctl(self.fd, UI_SET_KEYBIT, key)
        for rel in (REL_X, REL_Y, REL_WHEEL, REL_WHEEL_HI_RES):
            fcntl.ioctl(self.fd, UI_SET_RELBIT, rel)
        setup = struct.pack("HHHH80sI", BUS_VIRTUAL, 1, 1, 1, b"pzoomfeel", 0)
        fcntl.ioctl(self.fd, UI_DEV_SETUP, setup)
        fcntl.ioctl(self.fd, UI_DEV_CREATE)
        time.sleep(0.25)

    def _emit(self, etype: int, code: int, value: int) -> None:
        os.write(self.fd, struct.pack("qqHHi", 0, 0, etype, code, value))

    def _syn(self) -> None:
        self._emit(EV_SYN, SYN_REPORT, 0)

    def move_to_origin_then(self, x: int, y: int) -> None:
        for _ in range(4):
            self._emit(EV_REL, REL_X, -2000)
            self._emit(EV_REL, REL_Y, -2000)
            self._syn()
            time.sleep(0.01)
        self._emit(EV_REL, REL_X, x)
        self._emit(EV_REL, REL_Y, y)
        self._syn()
        time.sleep(0.05)

    def wheel_zoom(self, notches: int) -> None:
        self._emit(EV_KEY, KEY_LEFTMETA, 1)
        self._emit(EV_KEY, KEY_LEFTSHIFT, 1)
        self._syn()
        time.sleep(0.02)
        step = 1 if notches > 0 else -1
        for _ in range(abs(notches)):
            self._emit(EV_REL, REL_WHEEL, step)
            self._emit(EV_REL, REL_WHEEL_HI_RES, step * 120)
            self._syn()
            time.sleep(0.016)
        self._emit(EV_KEY, KEY_LEFTSHIFT, 0)
        self._emit(EV_KEY, KEY_LEFTMETA, 0)
        self._syn()

    def close(self) -> None:
        try:
            fcntl.ioctl(self.fd, UI_DEV_DESTROY)
        finally:
            os.close(self.fd)


class ShellClient:
    """Line-delimited JSON client for Infynitae shell.sock."""

    def __init__(self, sock_path: Path) -> None:
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.connect(str(sock_path))
        self.sock.setblocking(False)
        self._buf = bytearray()
        self._pending_events: List[Dict[str, object]] = []
        self._next_id = 1

    def _read_lines(self, timeout: float) -> List[Dict[str, object]]:
        out: List[Dict[str, object]] = []
        r, _, _ = select.select([self.sock], [], [], timeout)
        if r:
            chunk = self.sock.recv(65536)
            if chunk:
                self._buf.extend(chunk)
        while True:
            nl = self._buf.find(b"\n")
            if nl < 0:
                break
            line = bytes(self._buf[:nl]).decode("utf-8", errors="replace")
            del self._buf[: nl + 1]
            if line.strip():
                out.append(json.loads(line))
        return out

    def request(self, cmd: str, timeout: float = 2.0) -> Dict[str, object]:
        req_id = self._next_id
        self._next_id += 1
        self.sock.sendall((json.dumps({"id": req_id, "cmd": cmd}) + "\n").encode("utf-8"))
        deadline = time.monotonic() + timeout
        reply: Optional[Dict[str, object]] = None
        while time.monotonic() < deadline and reply is None:
            for msg in self._read_lines(max(0.01, deadline - time.monotonic())):
                if msg.get("id") == req_id and reply is None:
                    reply = msg
                else:
                    self._pending_events.append(msg)
        if reply is None:
            raise TimeoutError(f"IPC timeout on {cmd}")
        if not reply.get("ok", False):
            raise RuntimeError(f"IPC {cmd} failed: {reply}")
        return reply

    def drain_events(self, idle_wait: float, max_wait: float = 2.0) -> List[Dict[str, object]]:
        events: List[Dict[str, object]] = list(self._pending_events)
        self._pending_events.clear()
        last = time.monotonic()
        end = last + max_wait
        while time.monotonic() < end:
            msgs = self._read_lines(min(0.05, idle_wait))
            if msgs:
                events.extend(msgs)
                last = time.monotonic()
            elif time.monotonic() - last >= idle_wait:
                break
        return events

    def close(self) -> None:
        self.sock.close()


def canvas_point(center: Tuple[float, float], zoom: float, p: Tuple[float, float], extent: Tuple[float, float]) -> Tuple[float, float]:
    return (
        center[0] + (p[0] - extent[0] * 0.5) / zoom,
        center[1] + (p[1] - extent[1] * 0.5) / zoom,
    )


def measure_samples(
    samples: List[Tuple[Tuple[float, float], float]],
    initial: Tuple[Tuple[float, float], float],
    p: Tuple[float, float],
    extent: Tuple[float, float],
) -> Tuple[float, int]:
    """Return (max_drift_px, frames_to_settle) over camera samples."""
    q0 = canvas_point(initial[0], initial[1], p, extent)
    max_drift = 0.0
    for c, z in samples:
        q = canvas_point(c, z, p, extent)
        max_drift = max(max_drift, math.hypot(q[0] - q0[0], q[1] - q0[1]))
    return max_drift, len(samples)


def set_config_feel(cfg_path: Path, feel: str) -> None:
    text = cfg_path.read_text(encoding="utf-8") if cfg_path.exists() else ""
    line = f'ZOOM_FEEL="{feel}"'
    if re.search(r"^ZOOM_FEEL=", text, flags=re.MULTILINE):
        updated = re.sub(r"^ZOOM_FEEL=.*$", line, text, flags=re.MULTILINE)
    else:
        updated = text.rstrip("\n") + ("\n" if text else "") + line + "\n"
    cfg_path.write_text(updated, encoding="utf-8")
    subprocess.run(["infynitae-reload"], check=True)
    time.sleep(0.15)


def extract_camera_samples(events: List[Dict[str, object]], output_id: int) -> List[Tuple[Tuple[float, float], float]]:
    samples: List[Tuple[Tuple[float, float], float]] = []
    for ev in events:
        out = ev.get("output") if isinstance(ev, dict) else None
        if isinstance(out, dict) and out.get("id") == output_id:
            cam = out.get("camera")
            if isinstance(cam, dict):
                c = cam.get("center")
                z = cam.get("zoom")
                if isinstance(c, list) and len(c) == 2 and isinstance(z, (int, float)):
                    samples.append(((float(c[0]), float(c[1])), float(z)))
    return samples


def default_socket_path() -> Path:
    rt = os.environ.get("XDG_RUNTIME_DIR") or f"/run/user/{os.getuid()}"
    return Path(rt) / "infynitae" / "shell.sock"


def default_config_path() -> Path:
    cfg = os.environ.get("XDG_CONFIG_HOME") or os.path.expanduser("~/.config")
    return Path(cfg) / "infynitae" / "config"


def run_self_test() -> int:
    with tempfile.TemporaryDirectory(prefix="pzoomfeel-test-") as td:
        sock_path = Path(td) / "shell.sock"
        srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        srv.bind(str(sock_path))
        srv.listen(1)

        import threading

        def server() -> None:
            conn, _ = srv.accept()
            with conn:
                f = conn.makefile("r", encoding="utf-8")
                for line in f:
                    req = json.loads(line)
                    resp: Dict[str, object] = {"id": req["id"], "ok": True, "generation": 1}
                    if req["cmd"] == "snapshot":
                        resp["snapshot"] = {
                            "outputs": [
                                {
                                    "id": 1,
                                    "mode": [1920.0, 1080.0],
                                    "scale": 1.0,
                                    "camera": {"center": [0.0, 0.0], "zoom": 1.0},
                                }
                            ]
                        }
                    conn.sendall((json.dumps(resp) + "\n").encode("utf-8"))
                    if req["cmd"] == "subscribe":
                        p = (1320.0, 310.0)
                        ext = (1920.0, 1080.0)
                        q0 = canvas_point((0.0, 0.0), 1.0, p, ext)
                        for z in (1.1, 1.25, 1.4, 1.5):
                            c = (q0[0] - (p[0] - ext[0] / 2) / z, q0[1] - (p[1] - ext[1] / 2) / z)
                            ev = {"event": "output_changed", "output": {"id": 1, "camera": {"center": [c[0], c[1]], "zoom": z}}}
                            conn.sendall((json.dumps(ev) + "\n").encode("utf-8"))
                        break

        th = threading.Thread(target=server, daemon=True)
        th.start()
        cli = ShellClient(sock_path)
        snap = cli.request("snapshot")["snapshot"]
        out0 = snap["outputs"][0]  # type: ignore[index]
        ext = (float(out0["mode"][0]) / float(out0["scale"]), float(out0["mode"][1]) / float(out0["scale"]))  # type: ignore[index]
        c0 = (float(out0["camera"]["center"][0]), float(out0["camera"]["center"][1]))  # type: ignore[index]
        z0 = float(out0["camera"]["zoom"])  # type: ignore[index]
        cli.request("subscribe")
        evs = cli.drain_events(idle_wait=0.1, max_wait=1.0)
        samples = extract_camera_samples(evs, 1)
        drift, frames = measure_samples(samples, (c0, z0), (1320.0, 310.0), ext)
        cli.close()
        srv.close()
        assert drift < 1e-9, drift
        assert frames == 4, frames
        print(f"self-test ok: max_drift={drift:.6f} px frames={frames}")
        return 0


def run_live(args: argparse.Namespace) -> int:
    cfg_path = Path(args.config)
    had_cfg = cfg_path.exists()
    backup = cfg_path.read_text(encoding="utf-8") if had_cfg else None
    client = ShellClient(Path(args.socket))
    uinput = UInputWheel()
    try:
        snap = client.request("snapshot")["snapshot"]
        out = snap["outputs"][0]  # type: ignore[index]
        out_id = int(out["id"])  # type: ignore[index]
        scale = float(out["scale"])  # type: ignore[index]
        extent = (float(out["mode"][0]) / scale, float(out["mode"][1]) / scale)  # type: ignore[index]
        p = (round(extent[0] * 0.68), round(extent[1] * 0.28))
        uinput.move_to_origin_then(int(p[0]), int(p[1]))
        client.request("subscribe")

        for feel in FEELS:
            set_config_feel(cfg_path, feel)
            snap_now = client.request("snapshot")["snapshot"]
            o_now = [o for o in snap_now["outputs"] if o["id"] == out_id][0]  # type: ignore[index]
            c0 = (float(o_now["camera"]["center"][0]), float(o_now["camera"]["center"][1]))  # type: ignore[index]
            z0 = float(o_now["camera"]["zoom"])  # type: ignore[index]
            client.drain_events(idle_wait=0.05, max_wait=0.2)

            uinput.wheel_zoom(3)
            ev_in = client.drain_events(idle_wait=0.35, max_wait=2.5)
            s_in = extract_camera_samples(ev_in, out_id)
            drift_in, n_in = measure_samples(s_in, (c0, z0), p, extent)

            c_mid, z_mid = s_in[-1] if s_in else (c0, z0)
            uinput.wheel_zoom(-3)
            ev_out = client.drain_events(idle_wait=0.35, max_wait=2.5)
            s_out = extract_camera_samples(ev_out, out_id)
            drift_out, n_out = measure_samples(s_out, (c_mid, z_mid), p, extent)

            print(
                f"{feel:<8} max_drift={max(drift_in, drift_out):.4f} px  "
                f"frames_in={n_in}  frames_out={n_out}"
            )
    finally:
        uinput.close()
        client.close()
        if backup is not None:
            cfg_path.write_text(backup, encoding="utf-8")
            subprocess.run(["infynitae-reload"], check=False)
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="Measure ZOOM_FEEL drift and frames to settle.")
    ap.add_argument("--socket", default=str(default_socket_path()), help="Path to shell.sock")
    ap.add_argument("--config", default=str(default_config_path()), help="Path to infynitae config")
    ap.add_argument("--self-test", action="store_true", help="Run offline socket/drift self-test")
    args = ap.parse_args()
    return run_self_test() if args.self_test else run_live(args)


if __name__ == "__main__":
    raise SystemExit(main())
