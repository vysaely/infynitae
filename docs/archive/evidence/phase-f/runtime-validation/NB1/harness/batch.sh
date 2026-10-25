#!/bin/sh
# usage: batch.sh <rounds> <tag>  — alternates r2 (stalled before) and r1 (never stalled), nested
cd "$(dirname "$0")"
for i in $(seq 1 $1); do
  for b in nrt3:r2 nrt2:r1; do
    bin=../nested-logs/infynitae-${b%%:*}; lab=${b##*:}
    echo "$(date +%T) $2 $lab-$i $(sh ./trial.sh $bin $2-$lab $i nested 2>/dev/null | tail -1) load=$(cut -d' ' -f1 /proc/loadavg)" >> batch-$2.txt
  done
done
echo DONE >> batch-$2.txt
