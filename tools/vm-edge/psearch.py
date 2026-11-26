# Launcher search VM test: checks app, window, settings row, clipboard, and plocate file
# results (present and absent) via uinput and INFYNITAE_EDGE_BOXES.

import os
import shutil
import subprocess
import sys

exec(open("/home/tester/vmlib.py").read())

BIN = "/home/tester/.local/bin"
EDGE = f"{BIN}/infynitae-edge-apps"
LOCATE_TAG = f"psearch-loc-{os.getpid()}"
LOCATE_FILE = f"/home/tester/{LOCATE_TAG}.txt"
NO_PLOCATE_BIN = "/tmp/psearch-no-plocate"
NO_PLOCATE_MSG = "Searching files by name needs plocate, which is not installed"


def parse_boxes():
    try:
        raw = open(BOX).read()
    except OSError:
        return {"raw": "", "state": None, "panel": None, "query": "", "hits": {}, "labels": [], "notes": []}
    s = raw.strip()
    if s == "closed":
        return {"raw": raw, "state": "closed", "panel": None, "query": "", "hits": {}, "labels": [], "notes": []}
    out = {"raw": raw, "state": None, "panel": None, "query": "", "hits": {}, "labels": [], "notes": []}
    for line in raw.splitlines():
        w = line.split()
        if not w:
            continue
        if w[0] == "panel" and len(w) >= 6:
            out["state"] = "open"
            out["panel"] = (w[1], float(w[2]), float(w[3]), float(w[4]), float(w[5]))
        elif w[0] == "query":
            out["query"] = line[6:] if len(line) > 6 else ""
        elif w[0] == "hit" and len(w) >= 7:
            out["hits"][(w[1], int(w[2]))] = tuple(float(x) for x in w[3:7])
        elif w[0] == "label":
            parts = line.split(" ", 3)
            if len(parts) == 4:
                out["labels"].append((parts[1], int(parts[2]), parts[3]))
        elif w[0] == "note":
            parts = line.split(" ", 2)
            if len(parts) == 3:
                out["notes"].append((parts[1], parts[2]))
    return out


def mid(r):
    return (r[0] + r[2] / 2, r[1] + r[3] / 2)


def find_label(d, kind, needle, exact=False):
    for k, hid, txt in d["labels"]:
        if k != kind:
            continue
        if (txt == needle if exact else needle.lower() in txt.lower()):
            if (k, hid) in d["hits"]:
                return (hid, txt, d["hits"][(k, hid)])
    return None


def focused_el():
    s = snap()
    fid = s.get("focus")
    for e in s.get("elements", []):
        if e.get("id") == fid:
            return e
    return None


def start_edge(path_spec=None):
    sh("pkill -x infynitae-edge; pkill -x infynitae-edge-apps; pkill -f '[i]nfynitae-edge-apps'; true")
    wait(0.8)
    sh(f"rm -f {BOX}")
    if path_spec is None:
        tester_path = sh('printf "%s" "$PATH"', 5).strip()
        path_spec = f"{BIN}:{tester_path}" if BIN not in tester_path.split(":") else tester_path
    run_bg(f"PATH={path_spec} INFYNITAE_EDGE_BOXES={BOX} {EDGE}")
    wait(2.0)


def open_panel(name):
    run_bg(f"{EDGE} {name}")
    return until(lambda: (parse_boxes().get("panel") or ("",))[0] == name, 6, 0.15)


def close_panel():
    for _ in range(3):
        if parse_boxes().get("state") == "closed":
            return True
        chord("esc")
        wait(0.35)
    return until(lambda: parse_boxes().get("state") == "closed", 4, 0.15)


def clear_windows():
    close_all()
    until(lambda: not els(), 6, 0.2)


def run_updatedb():
    cmd = ["updatedb"] if os.geteuid() == 0 else ["sudo", "-n", "updatedb"]
    return subprocess.run(cmd, capture_output=True, text=True, timeout=120)


def build_no_plocate_path(dst):
    shutil.rmtree(dst, ignore_errors=True)
    os.makedirs(dst, exist_ok=True)
    seen = set()
    tester_path = sh('printf "%s" "$PATH"', 5).strip()
    for d in [BIN] + tester_path.split(":"):
        if not d or not os.path.isdir(d):
            continue
        for name in os.listdir(d):
            if name == "plocate" or name in seen:
                continue
            seen.add(name)
            os.symlink(os.path.join(d, name), os.path.join(dst, name))


started_clip_watch = False
clear_windows()
start_edge()

try:
    # 1. App search: query shows app row in apps/list and Enter starts it.
    clear_windows()
    opened = open_panel("apps")
    text("alacr")
    seen_app = until(
        lambda: find_label(parse_boxes(), "apps/list", "alacritty") is not None,
        6,
        0.2,
    )
    d = parse_boxes()
    app_labels = [(hid, txt) for k, hid, txt in d["labels"] if k == "apps/list"]
    top_matches = bool(app_labels) and "alacritty" in app_labels[0][1].lower()
    chord("enter")
    launched = until(lambda: parse_boxes().get("state") == "closed" and has("alacritty"), 8, 0.2)
    check(
        "app search shows Alacritty first in apps/list and Enter starts it",
        opened and seen_app and top_matches and launched,
        f"opened={opened} seen={seen_app} labels={app_labels[:3]} ids={ids()}",
    )
    close_panel()

    # 2. Window search: two windows with different titles, query matches target, Enter focuses it.
    clear_windows()
    run_bg("alacritty -T psearch-win-one -e sleep 600")
    win1 = until(lambda: any(e.get("title") == "psearch-win-one" for e in els()), 8, 0.2)
    run_bg("alacritty -T psearch-win-two -e sleep 600")
    win2 = until(lambda: (focused_el() or {}).get("title") == "psearch-win-two", 8, 0.2)
    opened = open_panel("apps")
    text("psearch-win-one")
    seen_win = until(
        lambda: find_label(parse_boxes(), "apps/results", "windows psearch-win-one", exact=True) is not None,
        6,
        0.2,
    )
    d = parse_boxes()
    has_other = find_label(parse_boxes(), "apps/results", "psearch-win-two") is not None
    chord("enter")
    focused_one = until(
        lambda: parse_boxes().get("state") == "closed"
        and (focused_el() or {}).get("title") == "psearch-win-one",
        6,
        0.2,
    )
    check(
        "window search shows psearch-win-one and Enter focuses it over psearch-win-two",
        win1 and win2 and opened and seen_win and not has_other and focused_one,
        f"win1={win1} win2={win2} seen={seen_win} other={has_other} focus={focused_el()} labels={d['labels']}",
    )
    close_panel()
    clear_windows()

    # 3. Settings row search: "night light" shows row and Enter opens Settings at Night light warmth.
    opened = open_panel("apps")
    text("night light")
    seen_set = until(
        lambda: find_label(parse_boxes(), "apps/results", "settings Night light warmth", exact=True) is not None,
        6,
        0.2,
    )
    d = parse_boxes()
    res_labels = [(hid, txt) for k, hid, txt in d["labels"] if k == "apps/results"]
    app_stolen = any(k == "apps/list" for k, _, _ in d["labels"])
    chord("enter")
    in_settings = until(
        lambda: (parse_boxes().get("panel") or ("",))[0] == "settings"
        and parse_boxes().get("query") == "Night light warmth"
        and find_label(parse_boxes(), "settings/all", "Night light warmth", exact=True) is not None,
        8,
        0.2,
    )
    sd = parse_boxes()
    check(
        "settings row search for 'night light' opens Settings at Night light warmth",
        opened and seen_set and not app_stolen and in_settings,
        f"seen={seen_set} stolen={app_stolen} res={res_labels} panel={sd.get('panel')} query={sd.get('query')!r} labels={sd['labels'][:5]}",
    )
    close_panel()

    # 4. Clipboard search: query shows copied entry and Enter restores it to the clipboard.
    sh("cliphist wipe; true")
    if not sh("pgrep -af '[w]l-paste.*cliphist store' | grep -v -- '--type image' || true"):
        run_bg("wl-paste --type text --watch cliphist store")
        started_clip_watch = True
        wait(1.0)
    run_bg("printf psearch-clip-one | wl-copy")
    c1 = until(lambda: "psearch-clip-one" in sh("cliphist list 2>/dev/null", 5), 6, 0.2)
    run_bg("printf psearch-clip-two | wl-copy")
    c2 = until(
        lambda: "psearch-clip-two" in sh("cliphist list 2>/dev/null", 5)
        and sh("wl-paste -n -t text/plain 2>/dev/null", 5) == "psearch-clip-two",
        6,
        0.2,
    )
    opened = open_panel("apps")
    text("psearch-clip-one")
    seen_clip = until(
        lambda: find_label(parse_boxes(), "apps/results", "clipboard psearch-clip-one", exact=True) is not None,
        6,
        0.2,
    )
    d = parse_boxes()
    chord("enter")
    restored = until(
        lambda: parse_boxes().get("state") == "closed"
        and sh("wl-paste -n -t text/plain 2>/dev/null", 5) == "psearch-clip-one",
        6,
        0.2,
    )
    pasted = sh("wl-paste -n -t text/plain 2>/dev/null", 5)
    check(
        "clipboard search shows entry and Enter restores it onto the clipboard",
        c1 and c2 and opened and seen_clip and restored and pasted == "psearch-clip-one",
        f"c1={c1} c2={c2} seen={seen_clip} pasted={pasted!r} labels={d['labels']}",
    )
    close_panel()

    # 5. File search with plocate installed (updatedb first): query shows file row, Enter opens it.
    clear_windows()
    sh(f"printf 'psearch locate check\\n' > {LOCATE_FILE}")
    db = run_updatedb()
    indexed = db.returncode == 0 and LOCATE_FILE in sh(f"plocate {LOCATE_TAG} 2>/dev/null", 10)
    opened = open_panel("apps")
    text(LOCATE_TAG)
    seen_file = until(
        lambda: find_label(parse_boxes(), "apps/results", f"files {LOCATE_TAG}.txt", exact=True) is not None,
        6,
        0.2,
    )
    d = parse_boxes()
    chord("enter")
    opened_win = until(lambda: parse_boxes().get("state") == "closed" and len(els()) >= 1, 10, 0.2)
    check(
        "plocate search shows file row after updatedb and Enter opens it",
        indexed and opened and seen_file and opened_win,
        f"db_rc={db.returncode} err={db.stderr.strip()!r} seen={seen_file} els={ids()} labels={d['labels']}",
    )
    close_panel()
    clear_windows()

    # 6. plocate absent: no files row in launcher search, and Settings > Apps notes plocate is missing.
    build_no_plocate_path(NO_PLOCATE_BIN)
    has_no_plocate = sh(f"PATH={NO_PLOCATE_BIN} command -v plocate >/dev/null && echo yes || echo no", 5).strip() == "no"
    start_edge(path_spec=NO_PLOCATE_BIN)

    opened = open_panel("apps")
    text(LOCATE_TAG)
    until(lambda: parse_boxes().get("query") == LOCATE_TAG, 5, 0.15)
    wait(1.0)
    d = parse_boxes()
    file_rows = [txt for k, _, txt in d["labels"] if k == "apps/results" and txt.startswith("files ")]
    check(
        "without plocate, launcher search shows no files row",
        has_no_plocate and opened and d.get("query") == LOCATE_TAG and not file_rows,
        f"no_plocate={has_no_plocate} query={d.get('query')!r} file_rows={file_rows} labels={d['labels']}",
    )
    close_panel()

    opened_set = open_panel("settings")
    saw_apps_group = until(
        lambda: find_label(parse_boxes(), "settings/all", "Apps", exact=True) is not None,
        6,
        0.2,
    )
    apps_hit = find_label(parse_boxes(), "settings/all", "Apps", exact=True)
    if apps_hit:
        click_at(*mid(apps_hit[2]))
    saw_note = until(
        lambda: ("settings/all", NO_PLOCATE_MSG) in parse_boxes()["notes"],
        6,
        0.2,
    )
    nd = parse_boxes()
    check(
        "without plocate, Settings > Apps notes plocate is missing",
        has_no_plocate and opened_set and saw_apps_group and saw_note,
        f"opened={opened_set} group={saw_apps_group} notes={nd['notes']}",
    )
    close_panel()
finally:
    sh(f"rm -f {LOCATE_FILE}; true")
    shutil.rmtree(NO_PLOCATE_BIN, ignore_errors=True)
    sh("pkill -x wl-copy; true")
    if started_clip_watch:
        sh("pkill -f '[w]l-paste.*cliphist store'; true")
    sh("pkill -f '[i]nfynitae-edge-apps'; true")
    clear_windows()
    restart_edge()

print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
sys.exit(0 if RES and all(ok for _, ok in RES) else 1)
