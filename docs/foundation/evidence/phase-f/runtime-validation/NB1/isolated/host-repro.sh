#!/bin/sh
# usage: host-repro.sh <test-bin> <label> ; env T_HIDE T_REVEAL DUR WATCHDOG
set -u
BIN=$(readlink -f "$1"); LABEL=$2; HERE=$(cd "$(dirname "$0")" && pwd); HOST=$HERE/infynitae-main
LOG=$HERE/runs/$LABEL; rm -rf "$LOG"; mkdir -p "$LOG"
echo "test $BIN $(sha256sum "$BIN" | cut -c1-16) hide=$T_HIDE reveal=${T_REVEAL:-never} term=${T_TERM:-none} duration=$DUR start $(date -Is)" > "$LOG/trial.txt"
T_HIDE=$T_HIDE T_REVEAL=${T_REVEAL:-} T_TERM=${T_TERM:-} DUR=$DUR setsid unshare -Urnmpf --mount-proc sh "$HERE/host-sandbox.sh" "$HOST" "$BIN" "$LOG" </dev/null >"$LOG/sandbox.out" 2>&1 &
U=$!
INIT=""; i=0; while [ -z "$INIT" ] && [ $i -lt 100 ]; do INIT=$(cat /proc/$U/task/*/children 2>/dev/null | awk '{print $1}'); i=$((i+1)); sleep 0.05; done
T0=$(date +%s); verdict=ok
while kill -0 $U 2>/dev/null; do [ $(( $(date +%s) - T0 )) -ge ${WATCHDOG:-45} ] && { verdict=STALL; break; }; sleep 1; done
if [ $verdict = STALL ]; then
  C=$(for t in /proc/$INIT/task/*; do for c in $(cat $t/children 2>/dev/null); do [ "$(readlink /proc/$c/exe)" = "$BIN" ] && [ "$(tr '\0' ' ' < /proc/$c/cmdline | grep -c wl-nested)" = 1 ] && echo $c; done; done | head -1)
  [ -n "$C" ] && timeout 60 gdb -p $C -batch -ex "thread 1" -ex "bt 22" > "$LOG/gdb.txt" 2>&1
  kill -KILL $INIT; sleep 1
fi
wait $U 2>/dev/null
python3 - "$LOG" "$verdict" >> "$LOG/trial.txt" <<'PY'
import sys,re
log,verdict=sys.argv[1],sys.argv[2]
t={}
for l in open(f"{log}/times"):
    k,*v=l.split(); t[k]=v
t0=float(t["t0"][0]); rel=lambda x: round(float(x)-t0,2)
ne=t.get("nested_exit"); hide=rel(t["hide"][0]); rev=rel(t["reveal"][0]) if "reveal" in t else None
# nested -> host commits (the nested compositor's frames), timestamps are wall clock in WAYLAND_DEBUG: use counts per phase by order
TS=re.compile(r"^\[(\d+):(\d+):(\d+\.\d+)\]")
lines=[l for l in open(f"{log}/nested.log",errors="replace").read().splitlines() if TS.match(l)]
secs=lambda l: (lambda m: int(m[1])*3600+int(m[2])*60+float(m[3]))(TS.match(l))
cm=[l for l in lines if re.search(r"-> wl_surface#\d+\.commit\(\)",l)]
from collections import Counter
win=Counter(re.search(r"(wl_surface#\d+)\.commit",l)[1] for l in cm).most_common(1)
commits=[l for l in cm if win and win[0][0] in l]
dones=[l for l in lines if re.search(r"wl_callback#\d+\.done",l)]
first=secs(lines[0]) if lines else 0
c_rel=[secs(l)-first for l in commits]
def count(a,b): return sum(1 for x in c_rel if a<=x<b)
tt=rel(t["term"][0]) if "term" in t else None
print(f"term_at={tt} ",end="")
print(f"verdict {verdict} nested_exit={'%s at %ss'%(ne[0],rel(ne[1])) if ne else 'none (killed)'} hide_at={hide} reveal_at={rev} "
      f"commits_visible={count(0,hide)} commits_hidden={count(hide+0.3, rev if rev else 999)} "
      f"commits_after_reveal={count(rev+0.3,999) if rev else '-'} total_commits={len(commits)} frame_dones={len(dones)}")
PY
tail -1 "$LOG/trial.txt"
