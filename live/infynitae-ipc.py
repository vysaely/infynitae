#!/usr/bin/env python3
"""Infynitae Shell IPC Helper.
Communicates with $XDG_RUNTIME_DIR/infynitae/shell.sock for compositor inspection and control.
"""

import os
import sys
import json
import socket

def get_socket_path():
    runtime_dir = os.environ.get("XDG_RUNTIME_DIR", f"/run/user/{os.getuid()}")
    return os.path.join(runtime_dir, "infynitae", "shell.sock")

def send_request(req_obj):
    sock_path = get_socket_path()
    if not os.path.exists(sock_path):
        return {"ok": False, "error": "socket_not_found", "path": sock_path}
    try:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.settimeout(1.0)
        sock.connect(sock_path)
        payload = json.dumps(req_obj) + "\n"
        sock.sendall(payload.encode("utf-8"))
        
        # Read response line
        buf = b""
        while b"\n" not in buf:
            chunk = sock.recv(4096)
            if not chunk:
                break
            buf += chunk
        sock.close()
        
        line = buf.split(b"\n")[0].decode("utf-8", errors="replace").strip()
        if not line:
            return {"ok": False, "error": "empty_response"}
        return json.loads(line)
    except Exception as e:
        return {"ok": False, "error": str(e)}

def get_primary_output():
    snap = send_request({"id": 1, "cmd": "snapshot"})
    if snap.get("ok") and snap.get("snapshot", {}).get("outputs"):
        return snap["snapshot"]["outputs"][0]["id"]
    return 1

def main():
    if len(sys.argv) < 2:
        print("Usage: infynitae-ipc.py [snapshot|focus <inst> <elem>|navigate <x> <y>|restore [output_id]]", file=sys.stderr)
        sys.exit(1)
        
    cmd = sys.argv[1]
    if cmd == "snapshot":
        res = send_request({"id": 1, "cmd": "snapshot"})
        print(json.dumps(res))
    elif cmd == "focus" and len(sys.argv) >= 4:
        inst = int(sys.argv[2])
        elem = int(sys.argv[3])
        res = send_request({"id": 1, "cmd": "focus", "element": [inst, elem]})
        print(json.dumps(res))
    elif cmd == "navigate":
        out_id = get_primary_output()
        if len(sys.argv) >= 4 and sys.argv[2] == "element":
            inst = int(sys.argv[3])
            elem = int(sys.argv[4])
            res = send_request({"id": 1, "cmd": "navigate", "output": out_id, "to": {"element": [inst, elem]}})
            print(json.dumps(res))
        elif len(sys.argv) >= 4:
            x = float(sys.argv[2])
            y = float(sys.argv[3])
            res = send_request({"id": 1, "cmd": "navigate", "output": out_id, "to": {"canvas": [x, y]}})
            print(json.dumps(res))
        else:
            print("Usage: navigate <x> <y> | navigate element <inst> <elem>", file=sys.stderr)
            sys.exit(1)
    elif cmd in ("restore", "viewport_restore"):
        if len(sys.argv) >= 3:
            out_id = int(sys.argv[2])
        else:
            out_id = get_primary_output()
        res = send_request({"id": 1, "cmd": "viewport_restore", "output": out_id})
        print(json.dumps(res))
    else:
        print(f"Unknown command: {cmd}", file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()
