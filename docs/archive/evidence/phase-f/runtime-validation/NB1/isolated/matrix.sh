#!/bin/sh
cd "$(dirname "$0")"
R=matrix-results.txt; : > $R
echo "S1 main #3: $(T_HIDE=3 T_REVEAL=8 DUR=12 WATCHDOG=40 sh ./host-repro.sh infynitae-main s1-main-3)" >> $R
for i in 1 2 3; do for b in main fix; do
  echo "S2 hide3-noreveal-dur8 $b #$i: $(T_HIDE=3 DUR=8 WATCHDOG=30 sh ./host-repro.sh infynitae-$b s2-$b-$i)" >> $R
done; done
for i in 1 2 3; do for b in r2 r2fix; do
  echo "S3 hide3-term6-noreveal $b #$i: $(T_HIDE=3 T_TERM=6 DUR=60 WATCHDOG=30 sh ./host-repro.sh infynitae-$b s3-$b-$i)" >> $R
done; done
echo DONE >> $R
