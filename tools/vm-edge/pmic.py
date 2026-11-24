import sys, os
exec(open("/home/tester/pcards.py").read().split("sh(f\"sed -i '/^PANEL_CONTROL=/d; /^ENABLE_REPLAY_BUFFER")[0])
# The microphone row in the Sound card of the edge from apps (live/edge-apps/src/cards/sound.rs, its
# switch is hit 4) and the password prompt the Network card uses, with real input.
def hits():
    wait(0.3)
    out = {}
    for l in open(BOX):
        w = l.split()
        if w and w[0] == "hit": out[(w[1], int(w[2]))] = list(map(float, w[3:7]))
    return out
sh(f"sed -i '/^PANEL_CONTROL=/d' {CFG}")
srcs = sh("pactl list short sources 2>/dev/null")
print("sources:", srcs.replace("\n", " | "), flush=True)
real = [l for l in srcs.splitlines() if l.strip() and ".monitor" not in l]
if not real:
    # a virtual microphone so the row has something to show
    sh("pactl load-module module-null-sink sink_name=vmic media.class=Audio/Source/Virtual >/dev/null 2>&1; pactl set-default-source vmic 2>/dev/null; true")
restart_edge(); chord("super+a"); wait(1.5)
mic = hits().get(("sound/volume", 4)); shot("m-1-sound")
check("the sound card shows the microphone's switch", mic is not None, str(sorted(k for k in hits() if k[0] == "sound/volume")))
before = sh("pactl get-source-mute @DEFAULT_SOURCE@")
if mic: click_at(mic[0] + mic[2] - 20, mic[1] + mic[3] / 2); wait(1.0)
after = sh("pactl get-source-mute @DEFAULT_SOURCE@"); shot("m-2-muted")
check("a click on the microphone's switch mutes or unmutes it", before != after and after != "", f"{before} -> {after}")
if before != after and mic: click_at(mic[0] + mic[2] - 20, mic[1] + mic[3] / 2)
chord("esc"); wait(0.6)
# the password prompt: hidden typing, the answer printed
open("/tmp/ask-pw.sh", "w").write('#!/bin/sh\ninfynitae-edge ask --text --secret "Password for Test" > /tmp/pw.out\n')
sh("rm -f /tmp/pw.out; chmod 755 /tmp/ask-pw.sh"); run_bg("sh /tmp/ask-pw.sh"); wait(2.0); shot("m-3-ask")
text("hunter2"); wait(0.5); shot("m-4-typed"); chord("enter"); wait(1.0)
got = sh("cat /tmp/pw.out 2>/dev/null")
check("the password prompt returns what was typed", got == "hunter2", repr(got))
print("SUMMARY", sum(1 for _, ok in RES if ok), "of", len(RES), flush=True)
for n, ok in RES:
    if not ok: print("  failed:", n)
