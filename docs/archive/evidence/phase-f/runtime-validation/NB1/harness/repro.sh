#!/bin/sh
# usage: repro.sh <binary> <label> <n> [minimise-ms or empty]
set -u
BIN=$1; LABEL=$2; N=$3; MIN=${4:-}
HERE=$(cd "$(dirname "$0")" && pwd); LOG=$HERE/repro/$LABEL-$N; rm -rf "$LOG"; mkdir -p "$LOG"
echo "binary $BIN $(sha256sum "$BIN" | cut -c1-16) minimise_ms=${MIN:-none} start $(date -Is) load $(cut -d' ' -f1 /proc/loadavg)" > "$LOG/trial.txt"
NB1_MIN_MS=$MIN NB1_NOTERM=${NB1_NOTERM:-} setsid unshare -Urnmpf --mount-proc sh "$HERE/repro-sandbox.sh" "$BIN" "$LOG" </dev/null >"$LOG/sandbox.out" 2>&1 &
U=$!
INIT=""; i=0; while [ -z "$INIT" ] && [ $i -lt 100 ]; do INIT=$(cat /proc/$U/task/*/children 2>/dev/null | awk '{print $1}'); i=$((i+1)); sleep 0.05; done
comp_pid() { for t in /proc/$INIT/task/*; do for c in $(cat $t/children 2>/dev/null); do [ "$(readlink /proc/$c/exe 2>/dev/null)" = "$(readlink -f "$BIN")" ] && echo $c; done; done | head -1; }
T0=$(date +%s); verdict=ok
while kill -0 $U 2>/dev/null; do
  [ $(( $(date +%s) - T0 )) -ge ${NB1_WATCHDOG_S:-30} ] && { verdict=STALL; break; }
  sleep 1
done
if [ $verdict = STALL ]; then
  C=$(comp_pid)
  for t in /proc/$C/task/*; do echo "$(basename $t) $(cat $t/comm) $(awk '{print $3}' $t/stat) wchan=$(cat $t/wchan)"; done > "$LOG/threads.txt" 2>&1
  timeout 90 gdb -p $C -batch -ex "set pagination off" -ex "thread 1" -ex "bt 25" > "$LOG/gdb.txt" 2>&1
  kill -KILL $INIT; sleep 1
fi
wait $U 2>/dev/null
el() { python3 -c "print(round($(cat $LOG/$1 2>/dev/null || echo 0)-$(cat $LOG/t-start),2))" 2>/dev/null; }
left=$(ps -eo pid=,args= | awk -v b="$(readlink -f "$BIN")" '$2==b' | wc -l)
echo "verdict $verdict $(cat $LOG/exit 2>/dev/null) exit_at=$(el t-end)s term_at=$(el t-term)s minimised=$(grep -c 'NB-1 reproducer' $LOG/compositor.log) last_frame_done=$(grep -E 'wl_callback#[0-9]+\.done' $LOG/compositor.log | tail -1 | cut -c2-16) left=$left" >> "$LOG/trial.txt"
tail -1 "$LOG/trial.txt"
