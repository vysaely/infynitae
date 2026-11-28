import json, os, sys, time
exec(open("/home/tester/vmlib.py").read())
# Infynitae portal file picker (infynitae-files --portal via infynitae-portal) end to end in the
# test VM with real /dev/uinput keys: opens a file, cancels with Escape, saves a file with an
# inline overwrite check via `zenity --file-selection` through xdg-desktop-portal, verifies no
# xdg-desktop-portal-gtk runs during normal use, and checks runtime fallback when the picker fails.
# Named pfilepicker.py (docs/FILE_PICKER.md section 9) so tools/vm-edge/ppicker.py (the accent
# colour picker VM test) stays untouched.
BIN = "/home/tester/.local/bin"
FILES_BIN = f"{BIN}/infynitae-files"
RUN = "/run/user/1000"
WORK = "/tmp/pfilepicker-work"

def wait_for_window(app_id, timeout=8.0):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            snap = json.loads(sh(f"{BIN}/infynitae msg snapshot"))["snapshot"]
            for el in snap.get("elements", []):
                if str(el.get("app_id", "")) == app_id:
                    return el
        except Exception:
            pass
        wait(0.2)
    return None

def run_zenity(tag, args):
    out_file = f"/tmp/pfc-{tag}.out"
    rc_file = f"/tmp/pfc-{tag}.rc"
    sh(f"rm -f {out_file} {rc_file}")
    script = f"/tmp/pfc-{tag}.sh"
    open(script, "w").write(
        f"XDG_RUNTIME_DIR={RUN} GTK_USE_PORTAL=1 GDK_DEBUG=portals "
        f"zenity --file-selection {args} > {out_file} 2>/tmp/pfc-{tag}.err; "
        f"echo $? > {rc_file}\n"
    )
    run_bg(f"sh {script}")
    return out_file, rc_file

def wait_result(out_file, rc_file, timeout=8.0):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if os.path.exists(rc_file):
            rc = open(rc_file).read().strip()
            out = open(out_file).read().strip() if os.path.exists(out_file) else ""
            return rc, out
        wait(0.15)
    return "timeout", ""

# Prepare deterministic test fixture folder and files.
sh(f"rm -rf {WORK} && mkdir -p {WORK}")
open(f"{WORK}/alpha.txt", "w").write("hello from infynitae picker test\n")
open(f"{WORK}/beta.txt", "w").write("second file\n")

# Save current config and switch FILE_PICKER="infynitae" for this run.
cfg_before = open(CFG).read() if os.path.exists(CFG) else ""
try:
    sh(f"sed -i '/^FILE_PICKER=/d' {CFG}")
    open(CFG, "a").write('FILE_PICKER="infynitae"\n')
    sh(f"sh /home/tester/infynitae/live/env.sh >/dev/null 2>&1; {BIN}/infynitae-reload portal >/dev/null 2>&1")
    wait(1.2)
    sh("pkill -x xdg-desktop-portal-gtk 2>/dev/null || true")
    wait(0.4)

    # 1. Open file via zenity --file-selection through the portal; pick alpha.txt with Enter.
    out_f, rc_f = run_zenity("open", f"--filename='{WORK}/'")
    win = wait_for_window("infynitae-files")
    check("OpenFile shows infynitae-files window", win is not None, str(win))
    shot("pfilepicker-1-open")
    if win is not None:
        # Navigate with Ctrl+L to exact file path so ordering/focus is unambiguous, then Enter.
        chord("ctrl+l"); wait(0.25)
        text(f"{WORK}/alpha.txt"); wait(0.15)
        chord("enter"); wait(0.35)
        chord("enter")
    rc, out = wait_result(out_f, rc_f)
    check("zenity --file-selection returns chosen path", rc == "0" and out == f"{WORK}/alpha.txt", f"rc={rc} out={out!r}")

    # 2. Cancel with Escape.
    out_f, rc_f = run_zenity("cancel", f"--filename='{WORK}/'")
    win = wait_for_window("infynitae-files")
    if win is not None:
        chord("esc")
    rc, out = wait_result(out_f, rc_f)
    check("Escape cancels zenity cleanly (rc 1)", rc == "1" and out == "", f"rc={rc} out={out!r}")

    # 3. Save file (new name) through the portal.
    out_f, rc_f = run_zenity("save", f"--save --filename='{WORK}/draft.txt'")
    win = wait_for_window("infynitae-files")
    check("SaveFile shows infynitae-files window", win is not None, str(win))
    shot("pfilepicker-2-save")
    if win is not None:
        # Stem "draft" is pre-selected before ".txt"; typing "saved" gives "saved.txt".
        text("saved"); wait(0.2)
        chord("enter")
    rc, out = wait_result(out_f, rc_f)
    check("zenity --save returns saved.txt", rc == "0" and out == f"{WORK}/saved.txt", f"rc={rc} out={out!r}")

    # 4. xdg-desktop-portal-gtk was not started during normal picker runs.
    gtk_proc = sh("pgrep -fa xdg-desktop-portal-gtk || true").strip()
    check("no xdg-desktop-portal-gtk spawned during normal runs", gtk_proc == "", gtk_proc)

    # 5. Runtime fallback: broken infynitae-files falls back to GTK instead of hanging zenity.
    backup = f"{FILES_BIN}.vmtest.bak"
    sh(f"mv '{FILES_BIN}' '{backup}' && printf '#!/bin/sh\\nexit 1\\n' > '{FILES_BIN}' && chmod 755 '{FILES_BIN}'")
    try:
        out_f, rc_f = run_zenity("fallback", f"--filename='{WORK}/'")
        wait(1.5)
        gtk_fb = sh("pgrep -fa xdg-desktop-portal-gtk || true").strip()
        check("broken picker falls back to xdg-desktop-portal-gtk", gtk_fb != "", gtk_fb)
        chord("esc")
        wait_result(out_f, rc_f, timeout=4.0)
    finally:
        sh(f"mv -f '{backup}' '{FILES_BIN}'")
finally:
    open(CFG, "w").write(cfg_before)
    sh(f"sh /home/tester/infynitae/live/env.sh >/dev/null 2>&1; {BIN}/infynitae-reload portal >/dev/null 2>&1")
    sh(f"rm -rf {WORK} /tmp/pfc-*")

print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok: print("  failed:", n)
