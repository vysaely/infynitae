#!/usr/bin/env python3
"""Interactive Wayland Window Overview & Switcher for Infynitae.
Pipes active windows into fuzzel/rofi and focuses the selected window.
"""

import os
import sys
import json
import shutil
import subprocess

def main():
    script_dir = os.path.dirname(os.path.realpath(__file__))
    ipc_script = os.path.join(script_dir, "infynitae-ipc.py")
    
    # 1. Fetch snapshot from IPC
    try:
        raw = subprocess.check_output(["python3", ipc_script, "snapshot"], text=True)
        data = json.loads(raw)
    except Exception as e:
        sys.exit(1)
        
    elements = data.get("snapshot", {}).get("elements", [])
    if not elements:
        # If no windows, run default launcher
        launcher = os.environ.get("LAUNCHER", "auto")
        if launcher == "auto" or not launcher:
            launcher = "fuzzel"
        os.execvp(launcher, [launcher])
        return

    # 2. Build menu lines
    lines = []
    mapping = {}
    for idx, el in enumerate(elements):
        inst, elem_id = el["id"]
        app = el.get("app_id") or "window"
        title = el.get("title") or "Untitled"
        line = f"[{idx + 1}] {app} — {title}"
        lines.append(line)
        mapping[line] = (inst, elem_id)

    # 3. Detect menu binary (fuzzel, rofi, wofi)
    menu_bin = None
    for cand in ["fuzzel", "rofi", "wofi"]:
        if shutil.which(cand):
            menu_bin = cand
            break

    if not menu_bin:
        sys.exit(1)

    menu_cmd = [menu_bin, "--dmenu", "-p", "Overview: "]
    if menu_bin == "rofi":
        menu_cmd = ["rofi", "-dmenu", "-p", "Overview"]

    proc = subprocess.Popen(menu_cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    selected, _ = proc.communicate(input="\n".join(lines))
    selected = selected.strip()

    if selected and selected in mapping:
        inst, elem_id = mapping[selected]
        # Focus selected window and navigate camera to it
        subprocess.run(["python3", ipc_script, "focus", str(inst), str(elem_id)])
        subprocess.run(["python3", ipc_script, "navigate", "element", str(inst), str(elem_id)])

if __name__ == "__main__":
    main()
