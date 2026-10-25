#!/bin/sh
# One bounded NB-1 trial with an independent watchdog (this script, outside the sandbox).
# usage: trial.sh <binary> <label> <n> [nested|headless]
set -u
BIN=$1; LABEL=$2; N=$3; MODE=${4:-nested}
HERE=$(cd "$(dirname "$0")" && pwd)
LOG=$HERE/trials/$LABEL-$N; rm -rf "$LOG"; mkdir -p "$LOG"
export SAT=$HOME/.cache/infynitae-integration/third-party/sat-target/release/xwayland-satellite
echo "binary $BIN $(sha256sum "$BIN" | cut -c1-16) mode $MODE start $(date -Is) load $(cut -d" " -f1-3 /proc/loadavg)" > "$LOG/trial.txt"
setsid unshare -Urnmpf --mount-proc sh "$HERE/sandbox.sh" "$BIN" "$LOG" "$MODE" </dev/null >"$LOG/sandbox.out" 2>&1 &
U=$!
# the sandbox's namespace init: the child of unshare
INIT=""; i=0; while [ -z "$INIT" ] && [ $i -lt 100 ]; do INIT=$(cat /proc/$U/task/*/children 2>/dev/null | awk '{print $1}'); i=$((i+1)); sleep 0.05; done
echo "unshare $U init $INIT" >> "$LOG/trial.txt"
comp_pid() { for t in /proc/$INIT/task/*; do for c in $(cat $t/children 2>/dev/null); do [ "$(readlink /proc/$c/exe 2>/dev/null)" = "$(readlink -f "$BIN")" ] && echo $c; done; done | head -1; }
T0=$(date +%s); last_size=-1; last_change=$T0; verdict=ok
while kill -0 $U 2>/dev/null; do
  now=$(date +%s); C=$(comp_pid)
  sz=$(stat -c %s "$LOG/driver.log" 2>/dev/null || echo 0)
  [ "$sz" != "$last_size" ] && { last_size=$sz; last_change=$now; }
  if [ -n "$C" ]; then
    m=$(cat /proc/$C/stat 2>/dev/null | awk '{print $3}'); w=$(cat /proc/$C/wchan 2>/dev/null)
    echo "$(date +%T.%N | cut -c1-12) main=$m:$w drv=$sz" >> "$LOG/samples.txt"
  fi
  if [ $((now - last_change)) -ge 30 ] || [ $((now - T0)) -ge 120 ]; then verdict=STALL; break; fi
  sleep 2
done
if [ $verdict = STALL ]; then
  C=$(comp_pid); echo "STALL at $(date -Is) compositor=$C elapsed=$(( $(date +%s) - T0 ))s" >> "$LOG/trial.txt"
  if [ -n "$C" ]; then
    for t in /proc/$C/task/*; do echo "$(basename $t) $(cat $t/comm) $(awk '{print $3}' $t/stat) wchan=$(cat $t/wchan) syscall=$(cat $t/syscall 2>/dev/null)"; done > "$LOG/threads.txt" 2>&1
    timeout 90 gdb -p $C -batch -ex "set pagination off" -ex "thread apply all bt 40" > "$LOG/gdb.txt" 2>&1
    timeout 20 nsenter -t $C -U -n --preserve-credentials ss -xpe > "$LOG/ss.txt" 2>&1 || timeout 20 nsenter -t $C -U -n ss -xpe > "$LOG/ss.txt" 2>&1
    ls -l /proc/$C/fd > "$LOG/fds.txt" 2>&1
    ps -o pid,ppid,stat,etime,args --ppid $INIT > "$LOG/ps.txt" 2>&1
  fi
  kill -KILL $INIT 2>/dev/null; sleep 1
fi
wait $U 2>/dev/null
left=$(ps -eo pid=,args= | awk -v b="$(readlink -f "$BIN")" '$2==b' | wc -l)
echo "verdict $verdict elapsed $(( $(date +%s) - T0 ))s results $(grep -c '^RESULT' "$LOG/driver.log" 2>/dev/null) left_compositors $left end $(date -Is)" >> "$LOG/trial.txt"
tail -1 "$LOG/trial.txt"
