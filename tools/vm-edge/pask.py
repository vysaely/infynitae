import sys, os, time
exec(open("/home/tester/pflow.py").read().split("sh(f\"sed -i '/^PANEL_CONTROL=/d' {CFG}; rm -f {BACKUP}\")")[0])
# `infynitae-edge ask` end to end with real keys and clicks: the asking program gets the answer and
# ends (a copy of the reply pipe held by the edge once kept it waiting after the answer until the
# edge exited). The edge to test: the first argument (default infynitae-edge-rs).
#   sudo -E python3 pask.py [infynitae-edge-rs | infynitae-edge-apps]
BIN = "/home/tester/.local/bin"
EDGE = f"{BIN}/{sys.argv[1] if len(sys.argv) > 1 else 'infynitae-edge-rs'}"
RUN = "/run/user/1000"
# patterns anchored at the binary's path: an unanchored one also matched the shell running this
# script (its command line names the edge) and ended the session
P = f"^{BIN}/infynitae-edge"
def stop_edges(): sh(f"pkill -x infynitae-edge; pkill -f '{P}-rs'; pkill -f '{P}-apps'; true")
stop_edges(); wait(0.8)
run_bg(f"INFYNITAE_EDGE_BOXES={BOX} {EDGE}"); wait(2.0)
def ask_bg(n, script):
    # run_bg wraps its command in single quotes: the asking script goes in a file
    open(f"/tmp/pask{n}.sh", "w").write(f"XDG_RUNTIME_DIR={RUN}; export XDG_RUNTIME_DIR\n{script} > /tmp/pask{n} 2>&1; echo $? >> /tmp/pask{n}\n")
    sh(f"rm -f /tmp/pask{n}"); run_bg(f"sh /tmp/pask{n}.sh"); wait(1.2)
def result(n):
    # the asking program must have ended by itself within a second of the answer
    for _ in range(10):
        if os.path.exists(f"/tmp/pask{n}") and open(f"/tmp/pask{n}").read().strip(): break
        wait(0.1)
    return open(f"/tmp/pask{n}").read().split() if os.path.exists(f"/tmp/pask{n}") else ["(none)"]
def ask_box():
    wait(0.3)
    for l in open(BOX):
        w = l.split()
        if w and w[0] == "ask": return list(map(float, w[1:5]))
    return None

ask_bg(1, f"printf 'one\\ntwo\\nthree\\n' | {EDGE} ask 'Pick a number'")
wait(1.0)  # the first question after the edge starts: its keyboard comes a moment after it shows
chord("down"); wait(0.2); chord("enter")
r = result(1); check("a choice by Down and Enter: the asker prints it and ends", r == ["two", "0"], str(r))

ask_bg(2, f"{EDGE} ask --text 'Name for the drive' < /dev/null")
text("Stick"); chord("enter")
r = result(2); check("a text typed and Enter: the asker prints it and ends", r == ["Stick", "0"], str(r))

ask_bg(3, f"printf 'a\\nb\\n' | {EDGE} ask 'Cancel me'")
chord("esc")
r = result(3); check("Escape: the asker ends cancelled (exit 1)", r == ["1"], str(r))

ask_bg(4, f"printf 'first\\nsecond\\n' | {EDGE} ask 'Click one'")
b = ask_box()
if b:
    # the second row: under the title (about 28 px) and one row (32 px) down
    click_at(b[0] + b[2] / 2, b[1] + 16 + 28 + 32 * 1.5)
r = result(4); check("a click on a choice: the asker prints it and ends", r == ["second", "0"], str(r))

ask_bg(5, f"{EDGE} ask --text --secret 'Password' < /dev/null")
text("hunter2"); chord("enter")
r = result(5); check("a secret: typed and Enter, the asker gets it and ends", r == ["hunter2", "0"], str(r))
sh("rm -f /tmp/pask5 /tmp/pask5.sh")

check("no asker left waiting", sh(f"pgrep -f '{P}-[a-z]* ask' | head -1") == "")
check("the edge is still running", sh(f"pgrep -f '^{EDGE}$' | head -1") != "")
sh(f"pkill -f '{P}-[a-z]* ask'; true"); stop_edges(); restart_edge()
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok: print("  failed:", n)
