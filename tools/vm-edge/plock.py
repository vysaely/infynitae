import sys, os, time
exec(open("/home/tester/g/everyday.py").read().split("names = sys.argv[1:]")[0])
# The lock screen with real keys in the VM (live/lock/infynitae-lock.c): Super+Shift+L locks, a
# wrong password keeps it locked, the right one unlocks, and the lock draws (a screenshot).
def live_lock(): return sh("ps -o stat= -C infynitae-lock | grep -vc Z || true").strip() not in ("0", "")
RES = []
def rec(name, ok, info=""):
    RES.append((name, ok)); print(("PASS " if ok else "FAIL ") + name, info, flush=True)
chord("super+shift+l"); locked = False
for _ in range(30):
    if live_lock(): locked = True; break
    wait(0.2)
rec("Super+Shift+L locks", locked); wait(1.5); shot("lock-1")
text("wrong"); chord("enter"); wait(4)
rec("a wrong password stays locked", live_lock()); shot("lock-2-wrong")
text("tester"); chord("enter")
for _ in range(40):
    if not live_lock(): break
    wait(0.25)
rec("the right password unlocks", not live_lock())
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
