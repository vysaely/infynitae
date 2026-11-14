import sys, os, time
exec(open("/home/tester/g/everyday.py").read().split("names = sys.argv[1:]")[0])
# two questions at once (review of PR #77): the second waits its turn; both askers get an answer
BIN = "/home/tester/.local/bin"
sh(f"pkill -x infynitae-edge; pkill -f '^{BIN}/infynitae-edge'; true"); wait(0.8)
run_bg(f"{BIN}/infynitae-edge-apps"); wait(2.0)
for n in (1, 2):
    open(f"/tmp/ask{n}.sh", "w").write(f"printf 'Yes\\nNo\\n' | {BIN}/infynitae-edge-apps ask \"Question {n}\" > /tmp/ask{n}.out; echo $? > /tmp/ask{n}.rc\n")
    sh(f"rm -f /tmp/ask{n}.rc /tmp/ask{n}.out; chmod 755 /tmp/ask{n}.sh")
    run_bg(f"sh /tmp/ask{n}.sh")
wait(2.5); chord("enter"); wait(1.5); chord("enter"); wait(2.0)
rc = [open(f"/tmp/ask{n}.rc").read().strip() if os.path.exists(f"/tmp/ask{n}.rc") else "none" for n in (1, 2)]
out = [open(f"/tmp/ask{n}.out").read().strip() if os.path.exists(f"/tmp/ask{n}.out") else "" for n in (1, 2)]
print("PASS" if rc == ["0", "0"] else "FAIL", "two questions at once, both answered", rc, out, flush=True)
sh(f"pkill -f '^{BIN}/infynitae-edge-apps'; true"); run_bg(f"{BIN}/infynitae-edge")
