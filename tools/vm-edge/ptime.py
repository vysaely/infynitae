import sys, os, time
exec(open("/home/tester/pflow.py").read().split("sh(f\"sed -i '/^PANEL_CONTROL=/d' {CFG}; rm -f {BACKUP}\")")[0])
# How long from the key to the panel's first frame, and from Escape to closed (the edge's box dump is
# written right after each frame it draws).
sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}")
open(CFG, "a").write('PANEL_CONTROL="SUPER+A bottom right 380 clock volume wifi bluetooth dnd power replay nightlight tray notifications session"\n')
restart_edge(); at(300, 300); wait(1.0)
def state(): return open(BOX).read()[:6]
opens, closes = [], []
for i in range(8):
    emit((1, KEYS["super"], 1)); wait(0.03)
    t = time.time(); emit((1, KEYS["a"], 1))
    while state().startswith("closed") and time.time() - t < 2: time.sleep(0.002)
    opens.append(round((time.time() - t) * 1000)); emit((1, KEYS["a"], 0)); emit((1, KEYS["super"], 0)); wait(0.8)
    t = time.time(); emit((1, KEYS["esc"], 1))
    while not state().startswith("closed") and time.time() - t < 2: time.sleep(0.002)
    closes.append(round((time.time() - t) * 1000)); emit((1, KEYS["esc"], 0)); wait(0.6)
print("TIMING open ms", opens, "median", sorted(opens)[4], flush=True)
print("TIMING close ms", closes, "median", sorted(closes)[4], flush=True)
sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}"); restart_edge()
