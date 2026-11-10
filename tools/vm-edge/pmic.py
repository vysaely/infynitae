import sys, os
exec(open("/home/tester/pcards.py").read().split("sh(f\"sed -i '/^PANEL_CONTROL=/d; /^ENABLE_REPLAY_BUFFER")[0])
# The microphone row in the Sound card and the password prompt the Network card uses
# (2026-11-10), with real input.
sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}")
srcs = sh("pactl list short sources 2>/dev/null")
print("sources:", srcs.replace("\n", " | "), flush=True)
real = [l for l in srcs.splitlines() if l.strip() and ".monitor" not in l]
if not real:
    # a virtual microphone so the row has something to show
    sh("pactl load-module module-null-sink sink_name=vmic media.class=Audio/Source/Virtual >/dev/null 2>&1; pactl set-default-source vmic 2>/dev/null; true")
sinks = len([l for l in sh("pactl list short sinks 2>/dev/null").splitlines() if l.strip()])
restart_edge(); chord("super+a"); wait(1.5)
vo = boxes()["modules"]["volume"]
click_at(vo[0] + 30, vo[1] + 20, "right"); d = boxes(); c = d["child"]; shot("m-1-sound")
check("a right click on the sound card opens it", c is not None, str(c))
before = sh("pactl get-source-mute @DEFAULT_SOURCE@")
# rows: the outputs, the Microphone heading, the microphone
if c: click_at(c[0] + 30, c[1] + 16 + 22 + 12 + 32 * (sinks + 1) + 16); wait(1.0)
after = sh("pactl get-source-mute @DEFAULT_SOURCE@"); shot("m-2-muted")
check("a click on the microphone's name mutes or unmutes it", before != after and after != "", f"{before} -> {after}")
if before != after and c: click_at(c[0] + 30, c[1] + 16 + 22 + 12 + 32 * (sinks + 1) + 16)
chord("esc"); wait(0.4); chord("esc"); wait(0.6)
# the password prompt: hidden typing, the answer printed
open("/tmp/ask-pw.sh", "w").write('#!/bin/sh\ninfynitae-edge ask --text --secret "Password for Test" > /tmp/pw.out\n')
sh("rm -f /tmp/pw.out; chmod 755 /tmp/ask-pw.sh"); run_bg("sh /tmp/ask-pw.sh"); wait(2.0); shot("m-3-ask")
text("hunter2"); wait(0.5); shot("m-4-typed"); chord("enter"); wait(1.0)
got = sh("cat /tmp/pw.out 2>/dev/null")
check("the password prompt returns what was typed", got == "hunter2", repr(got))
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok: print("  failed:", n)
