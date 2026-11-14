import os, re, sys, time
exec(open("/home/tester/g/everyday.py").read().split("names = sys.argv[1:]")[0])
# LATENCY_STATS across app input (review item 3): key presses that commit after an empty render
# still count in `[infynitae] Latency ... (N frames)` instead of being dropped at once.
BIN = "/home/tester/.local/bin"
SET = "sh /home/tester/infynitae/live/settings.sh"
LOG = "/home/tester/.local/state/infynitae/session.log"
prev = sh(f"{SET} get LATENCY_STATS").strip().strip("'\"") or "false"
start_len = os.path.getsize(LOG) if os.path.exists(LOG) else 0
sh(f"{SET} set LATENCY_STATS true")
run_bg("xterm -title latency-test -e sh -c 'cat >/dev/null'")
wait(1.5)
for _ in range(10):
    text("a")
    wait(1.1)
wait(2.0)
sh("pkill -f 'xterm -title latency-test'; true")
sh(f"{SET} set LATENCY_STATS {prev}")
tail = open(LOG, errors="replace").read()[start_len:] if os.path.exists(LOG) else ""
m = list(re.finditer(r"Latency [^:]+: input to screen p50 [0-9.]+ ms, p95 [0-9.]+ ms, max [0-9.]+ ms \((\d+) frames\)", tail))
samples = sum(int(x.group(1)) for x in m)
print("PASS" if samples >= 3 else "FAIL", "DRM input-to-screen sampled across app commits", f"lines={len(m)} samples={samples}", flush=True)
