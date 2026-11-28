import json, os, sys, time
exec(open("/home/tester/vmlib.py").read())
# VM verification for infynitae-health (docs/HEALTH.md), the monitor/health edge card, and the
# Health row in settings/about.
BIN = "/home/tester/.local/bin"
EDGE = f"{BIN}/infynitae-edge-apps"
HEALTH = f"{BIN}/infynitae-health"
P = f"^{BIN}/infynitae-edge"
START = "SUPER+A top right 440 monitor/health:l settings/about"


def apps_edge():
    sh(f"pkill -x infynitae-edge; pkill -f '{P}-rs'; pkill -f '{P}-apps'; true")
    wait(0.8)
    run_bg(f"INFYNITAE_EDGE_BOXES={BOX} {EDGE}")
    wait(2.0)


def read_boxes():
    wait(0.5)
    cards = {}
    hits = {}
    notes = []
    if not os.path.exists(BOX):
        return cards, hits, notes
    for raw in open(BOX):
        line = raw.strip()
        w = line.split()
        if not w:
            continue
        if w[0] == "card" and len(w) >= 6:
            cards[w[1]] = list(map(float, w[2:6]))
        elif w[0] == "hit" and len(w) >= 7:
            hits[(w[1], int(w[2]))] = list(map(float, w[3:7]))
        elif w[0] in ("note", "label"):
            notes.append(line)
    return cards, hits, notes


def mid(r):
    return r[0] + r[2] / 2, r[1] + r[3] / 2


# 1. Collector JSON contract: now --json, log --since 24h --json, sample --no-notify
now_raw = sh(f"{HEALTH} now --json")
now_ok = False
now_detail = now_raw[:200]
try:
    snap = json.loads(now_raw)
    now_ok = (
        snap.get("schema_version") == 1
        and snap.get("state") in ("ok", "watch", "problem")
        and isinstance(snap.get("items"), list)
        and len(snap["items"]) > 0
        and isinstance(snap.get("disks"), list)
        and isinstance(snap.get("memory"), dict)
    )
    now_detail = f"state={snap.get('state')} items={len(snap.get('items', []))} disks={len(snap.get('disks', []))}"
except Exception as e:
    now_detail = f"parse error: {e}"
check("infynitae-health now --json parses with schema_version 1", now_ok, now_detail)

log_raw = sh(f"{HEALTH} log --since 24h --json")
log_ok = False
log_detail = log_raw[:200]
try:
    rep = json.loads(log_raw)
    log_ok = rep.get("schema_version") == 1 and isinstance(rep.get("events"), list)
    log_detail = f"events={len(rep.get('events', []))}"
except Exception as e:
    log_detail = f"parse error: {e}"
check("infynitae-health log --since 24h --json parses", log_ok, log_detail)

sh(f"{HEALTH} sample --no-notify")
hdir = "/home/tester/.local/state/infynitae/health"
jsonl_files = sorted(f for f in os.listdir(hdir) if f.endswith(".jsonl")) if os.path.isdir(hdir) else []
sample_ok = False
if jsonl_files:
    last_line = open(os.path.join(hdir, jsonl_files[-1])).read().strip().splitlines()[-1]
    sample_ok = json.loads(last_line).get("schema_version") == 1
check("infynitae-health sample writes a valid JSONL history line", sample_ok, str(jsonl_files))

# 2. Edge UI: monitor/health card and settings/about Health row
sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}")
open(CFG, "a").write(f'PANEL_CONTROL="{START}"\n')
apps_edge()
at(300, 300)
wait(0.4)
run_bg(f"{EDGE} control")
wait(2.0)
shot("health-1-card")

cards, hits, notes = read_boxes()
check("monitor/health card renders in the panel", "monitor/health" in cards, str(sorted(cards)))
check(
    "monitor/health renders Recent log section",
    any("Recent log" in n for n in notes),
    str(notes[:12]),
)
check(
    "settings/about shows the Health row",
    ("settings/about", 11) in hits,
    str(sorted(h for (c, h) in hits if c == "settings/about")),
)

# Click the Health row in settings/about and verify it expands in place.
about_before = cards.get("settings/about", [0, 0, 0, 0])[3]
health_hit = hits.get(("settings/about", 11))
if health_hit:
    click_at(*mid(health_hit))
    wait(1.0)
shot("health-2-about-open")
cards_after, _, _ = read_boxes()
about_after = cards_after.get("settings/about", [0, 0, 0, 0])[3]
check(
    "clicking Health in settings/about expands it in place",
    about_after > about_before + 30,
    f"height {about_before} -> {about_after}",
)
check("the edge is still running", sh(f"pgrep -f '^{EDGE}$' | head -1") != "")

sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}; pkill -f '{P}-apps'; true")
restart_edge()
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok:
        print("  failed:", n)
