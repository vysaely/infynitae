# Infynitae vs Windows 11 for gaming on this PC: performance and latency review (2026-11-12)

Read-only review. Nothing in `~/infynitae` was built, run or edited. Code references are to the low-latency branch `/mnt/II/vysaely/wt-lowlat` (PR #50, head `1c5001d`) unless marked otherwise; `drm` means `crates/infynitae-backend-drm/src/lib.rs` and `wl/` means `crates/infynitae-wayland/src/`. System facts were read from `/sys`, `/proc`, `pacman`, `vulkaninfo`, `modetest`, `edid-decode` and `~/.local/state/infynitae/session.log`. Points already covered by `REVIEW-2026-11-11-window-states.md` are not repeated.

Statements marked **(inferred)** come from my knowledge of Xwayland, Mesa or DXVK behaviour and were not confirmed on this machine.

## Headline findings

- **The X11 path costs no copy and does not block scanout or tearing.** xwayland-satellite cannot pass a game's tearing request on, but `TEARING="always"` makes that irrelevant.
- **The replay recorder is the largest standing cost.** It runs all the time at 60 fps and makes the compositor draw and read back the whole screen for every captured frame, even while a game is scanned out directly. Windows has no equivalent load unless Game DVR is on.
- **VRR is off at the connector.** `vrr_capable` is 0 on DP-1 although the EDID advertises 48-165 Hz. This is almost certainly a monitor menu setting, and Windows will have VRR if it is enabled.
- **AMD Anti-Lag is installed but not enabled.** The Mesa layer needs one environment variable, and Proton-CachyOS ships the pieces that use it.
- **The compositor has no real-time priority.** Only its main thread is at nice -10; the machine has no rtkit and an rtprio limit of 0.
- **`LATENCY="direct"` loses its benefit after a few missed refreshes.** The margin grows by 0.75 ms per miss and takes about a minute of perfect frames to shrink back.
- **Tearing was refused by amdgpu in long runs** early in the session log (340 runs of 60 refusals). The cause is still unknown; the branch adds the diagnostic line that should identify it.

## 1. The X11 game path (xwayland-satellite)

**How it is started.**
- Infynitae owns the X display and its sockets, and starts satellite at the first X11 connection (`wl/xwayland.rs` `start_for_session`, `reserve_for_session`).
- The running processes were `~/.local/bin/xwayland-satellite :1 -listenfd … -auth …` and `Xwayland :1 -rootless -force-xrandr-emulation -wm 8 …`.
- The binary is the patched v0.8.3 build (commit `b83eab9`, `live/build-satellite.sh`), not the system package (`xwayland-satellite 0.8.3-1` is also installed). Xwayland is `xorg-xwayland 24.1.14-1`.

**What satellite passes through.** Its global list is fixed (`~/.cache/infynitae-satellite/src/src/server/mod.rs:380-390`): `wl_compositor`, `wl_shm`, `wl_seat`, `wl_output`, relative pointer, `wl_drm`, `zwp_linux_dmabuf_v1`, xdg-output, pointer constraints, tablet and `wp_linux_drm_syncobj_manager_v1`.

| Question | Answer | Evidence |
| :--- | :--- | :--- |
| Extra copy? | No. GPU buffers are forwarded as dmabufs by reference. | `server/dispatch.rs:620-628` re-creates the buffer with `create_immed` on the compositor connection |
| Blocks direct scanout? | No. | Session log: "Game mode on for process 2768700" (satellite's pid) followed by "its frames go to the screen as they are (direct scanout)" |
| Extra frame of latency? | Not from satellite. It forwards `frame` and `commit` one for one (`dispatch.rs:131-140`). The cost is one extra process hop per message. | Code read; hop cost not measured |
| Tearing hint passed on? | No. The satellite source has no tearing-control code at all, although Xwayland 24.1.14 supports it (`strings /usr/bin/Xwayland` lists `wp_tearing_control_manager_v1`). | grep of satellite source |
| Presentation timing passed on? | No. `wp_presentation` is not in the list, so Xwayland times X11 Present from frame callbacks. | Same list |
| Explicit sync? | Satellite supports it; Infynitae does not offer `linux-drm-syncobj-v1` (no such global in `wl/lib.rs`). Smithay 0.7 has the module (`vendor/smithay-0.7.0/src/wayland/drm_syncobj`). | Low priority on AMD |

**What this means.**
- The header comment in `wl/tearing.rs` ("X11 games, which cannot ask") is correct for satellite 0.8.3. This settles item 14 of the previous review.
- With `TEARING="always"` (`wl/tearing.rs:99`, `t.always`), an X11 game that owns the screen tears without asking, so the missing hint does not matter.
- A game with vsync off under Xwayland is not held back by frame callbacks **(inferred)**.
- `-force-xrandr-emulation` means a game that picks a non-native resolution is scaled with a viewport. I did not check whether such a buffer still gets scanned out; run games at 1920x1080 for comparisons.

**Compared with the alternatives.**
- **Gamescope** is not installed. Nested inside Infynitae it adds a second compositor and one more composition step for X11 games. It is useful for scaling or frame limiting, not for latency here.
- **Native Wayland in Proton** is available: Proton-CachyOS `cachyos-11.0-20261015` ships `winewayland.drv` and reads `PROTON_ENABLE_WAYLAND=1` (`proton:2039`, `2301`). It removes both Xwayland and satellite from the path, and Mesa can then send the tearing hint and use presentation feedback directly. Expected gain is small (two fewer socket hops per frame and per input event); it is worth testing per game because winewayland still has game-specific problems.
- **CS2, Dota 2, Deadlock and TF2 are native Linux games.** CS2's launch script forces X11 (`cs2.sh:76-77` sets `SDL_VIDEO_DRIVER=x11` when unset), and your launch options for apps 730 and 440 force it as well.

**What to change:** nothing in the compositor. Test `PROTON_ENABLE_WAYLAND=1` on one Proton game and `SDL_VIDEO_DRIVER=wayland` on CS2, and compare with section 6's method.

## 2. Frame pacing and latency features

**Versions.** Mesa `1:26.2.6-1` (RADV, 64- and 32-bit), kernel `7.2.14-arch1-1`, Proton-CachyOS with DXVK `v3.1.1-97` and vkd3d-proton `vkd3d-1.1-5656`.

**AMD Anti-Lag.**
- The layer is installed for both architectures: `/usr/share/vulkan/implicit_layer.d/VkLayer_MESA_anti_lag.json`, `/usr/lib/` and `/usr/lib32/libVkLayer_MESA_anti_lag.so`.
- It is off by default. The manifest has `enable_environment: ENABLE_LAYER_MESA_ANTI_LAG=1`.
- vkd3d-proton in this Proton uses it (`d3d12core.dll` contains `VK_AMD_anti_lag` and `d3d12_amd_ext_anti_lag_UpdateAntiLagState`), and Proton ships `amdxc64.dll` with `IAmdExtAntiLagApi`. So a D3D12 game with an Anti-Lag 2 option should be able to use it once the layer is enabled.
- DXVK's `d3d11.dll` has no Anti-Lag strings. It has `VK_NV_low_latency2` (Reflex) and its own `dxvk.latencySleep` and `dxvk.latencyTolerance` options.

**How to enable, per game in Steam launch options.**
- D3D12 games with an Anti-Lag 2 setting: `ENABLE_LAYER_MESA_ANTI_LAG=1 %command%`, then turn Anti-Lag on in the game.
- D3D11 and D3D9 games: `DXVK_CONFIG="dxvk.latencySleep = True" %command%`. This is DXVK's built-in pacing and needs no game support **(inferred from the option name and my knowledge of DXVK; test it)**.
- Proton-CachyOS extras, present on disk:
  - `PROTON_DXVK_LOWLATENCY=1` and `PROTON_VKD3D_LOWLATENCY=1` switch to the bundled low-latency forks (`files/lib/wine/dxvk-low-latency`, `vkd3d-low-latency`; `proton:1198`, `1214`, `2035-2036`).
  - `LOW_LATENCY_LAYER=1` loads `VK_LAYER_KORTHOS_low_latency` (`proton:2082-2084`).
  - I did not test any of these. Use one method at a time.
- Native Vulkan games (CS2): none of the above applies. The in-game frame cap is the tool.

**How Infynitae's frame timing interacts.**
- Frame callbacks and presentation feedback are sent from the page-flip event (`drm:1477-1519`, `wl/outputs.rs:563-590`).
- Feedback is sent with empty flags (`wl/outputs.rs:578-586`): no VSYNC, HW_COMPLETION or ZERO_COPY. Clients that read the flags get less information than on other compositors. Small fix.
- Under tearing the flip event arrives at once, so callbacks fire per flip. A game with in-game vsync on would then run uncapped and tear under `TEARING="always"` **(inferred from the code; test with one vsync-on game)**. If confirmed, `always` should skip windows whose client explicitly sent the `vsync` hint.
- `wp_fifo_v1` and `wp_commit_timing_v1` are not offered, though Smithay 0.7 has both modules. Mesa 26 uses them for pacing when present. Medium value, mainly for vsync-on games.
- **`LATENCY="direct"` margin problem.**
  - The last scanned-out episode in the log shows "budget 5.45 ms, margin 3.85 ms" against a 6.06 ms refresh. The compositor was latching the game's frame about 5.5 ms before the refresh, which is almost no better than `off`.
  - Cause: each missed refresh adds 0.75 ms (`MISS_STEP`), and the margin shrinks by 0.075 ms per 240 good frames (`wl/frame_schedule.rs:65-69`, `280-296`). Going from 4 ms back to 0.5 ms takes about 11,000 frames, roughly 68 s at 165 Hz.
  - The misses in that episode coincided with "Slow turn 8.3 ms: element building" lines at the moment scanout began.
  - This only matters when the game does not tear (tearing draws at once, `drm:2178-2184`), so it affects vsync-on play and any time amdgpu refuses async flips.

**What to change:**
- Enable the Anti-Lag layer and the DXVK option per game (owner, no sudo).
- In `frame_schedule.rs`, shrink the margin faster (for example one full step per 240 good frames) and do not count misses in the first few frames after a switch to scanout.
- Find what makes "element building" take 8-19 ms (94 such slow turns in the log).

## 3. CPU and scheduling

**Measured state.**

| Item | Value | Source |
| :--- | :--- | :--- |
| CPU | Ryzen 5 7600X3D, 6 cores / 12 threads, SMT on | `/proc/cpuinfo` |
| Kernel | 7.2.14-arch1-1, `PREEMPT_DYNAMIC`, `HZ=1000`, `NO_HZ_FULL` built | `uname`, `/proc/config.gz` |
| Preempt mode in use | Not readable without root (`/sys/kernel/debug/sched/preempt`); no `preempt=` on the command line | `/proc/cmdline` |
| Frequency driver | `amd-pstate-epp`, status `active`, governor `powersave`, EPP `performance` on all 12 CPUs, boost on | `/sys/devices/system/cpu/…` |
| power-profiles-daemon | 0.30, profile `balanced` | `powerprofilesctl get` |
| gamemode | 1.8.2 installed, daemon running, no config file, `lib32-gamemode` missing | `pacman`, `/etc/gamemode.ini` absent |
| sched_ext | Built in, state `disabled`; `scx-scheds` not installed | `/sys/kernel/sched_ext/state` |
| ntsync | Module exists, not loaded (`/dev/ntsync` absent, `/etc/modules-load.d/` empty) | `modinfo ntsync` |
| Split-lock mitigation | `kernel.split_lock_mitigate = 1` | `sysctl` |
| irqbalance, ananicy, tuned | Not installed | `pacman` |
| Real-time limits | `ulimit -r` is 0; no rtkit, no `realtime-privileges` | `pacman`, `ulimit` |

EPP reading `performance` while power-profiles-daemon says `balanced` is unexplained. `gamemoded -s` said "active" during the first check, and later `ClientCount` was 0. I did not find what set EPP, so do not assume it is always `performance`.

**Compositor priority.**
- No scheduling calls exist in the compositor for itself. The only `setpriority` use lowers background apps (`wl/background.rs:345-399`).
- `live/session.sh:13-17` runs `renice -n -10 -p "$PPID"`. That changes the main thread only: the thread list showed the main thread at -10 and `infynitae:gl0`, `infynitae-log` and the Mesa threads at 0.
- No SCHED_RR or SCHED_FIFO, no I/O priority.
- The GPU context is high priority (`drm:665-670`; the log says "GPU context priority: Some(High)" at every start).

**Game mode.**
- Infynitae registers the game with Feral gamemode over D-Bus (`live/launch.sh:108-118`).
- For an X11 game the registered pid is satellite's (the comment at `launch.sh:110-111` says so, and the log shows pid 2768700). gamemode's per-process actions therefore do not reach the game; only the global governor change applies.
- Any fullscreen window counts: the last "Game mode on" in the log was for Discord (pid 2768857).
- Your CS2 and TF2 launch options already use `gamemoderun`, which registers the real game process. A 32-bit game would need `lib32-gamemode`.

**Windows equivalents.**

| Windows | Linux equivalent here | State |
| :--- | :--- | :--- |
| MMCSS raises the compositor and audio threads to real-time classes | SCHED_RR for the compositor's main thread, via rtkit or `CAP_SYS_NICE` | Missing |
| Game Mode: foreground game gets CPU preference, background work is held back | `BACKGROUND_PRIORITY` (nice 10 for hidden apps), gamemode | Present |
| "Ultimate/High performance" power plan | EPP `performance`, or `powerprofilesctl set performance` | Currently `performance`, source unclear |
| Timer resolution 1 ms or better | `HZ=1000` plus high-resolution timers | Already better |
| Fast user-mode synchronisation in the NT kernel | ntsync (or fsync) | ntsync not loaded |

**What to change:**
- Give the main thread SCHED_RR priority 1 with reset-on-fork at startup in `crates/infynitae/src/main.rs`, falling back silently when refused. It needs either `rtkit` installed (owner, sudo) or `setcap cap_sys_nice+ep` on the binary done by `install.sh`.
- Load ntsync: `echo ntsync | sudo tee /etc/modules-load.d/ntsync.conf`. Expect a small gain in some games and none in others.
- Add `/etc/gamemode.ini` with `renice=10` and `ioprio=0` so `gamemoderun` games get a priority boost (your user is in the `gamemode` group; the limit file allows nice -10).
- Install `lib32-gamemode` if any 32-bit game uses `gamemoderun`.
- Background load matters more than all of the above. During this review a lab job (python at 180% CPU, nice 0) and a QEMU VM (580% CPU, nice 19) were running. A lab job that uses the GPU while a game runs will dominate any measurement.

## 4. GPU side

**Measured state.**

| Item | Value | Source |
| :--- | :--- | :--- |
| GPU | RX 9070 XT (1002:7550), RADV GFX1201 | `vulkaninfo --summary` |
| Resizable BAR | On: BAR 0 is 16 GB and visible VRAM equals total VRAM (17,095,983,104 bytes) | `lspci -vv`, `mem_info_vis_vram_total` |
| Power profile | `BOOTUP_DEFAULT` selected; `3D_FULL_SCREEN` available | `pp_power_profile_mode` |
| Performance level | `auto` | `power_dpm_force_performance_level` |
| VRR | `vrr_capable` = 0 on DP-1 | `modetest -M amdgpu -c` (the sysfs file does not exist on this kernel) |
| Monitor | ViewSonic VX2718 series, 2021; EDID range 48-165 Hz, "supports continuous frequencies", 1920x1080 at 165.000 Hz | `edid-decode` |

**VRR.** The EDID has what amdgpu needs, yet the driver reports not capable. On DisplayPort the driver also requires the monitor to report over the DP link that it accepts variable timing, and monitors clear that when Adaptive-Sync/FreeSync is off in their menu **(inferred; reading the link data needs root)**. Turn FreeSync/Adaptive-Sync on in the monitor's menu, then check that `modetest -M amdgpu -c | grep -A3 vrr_capable` shows 1 and the session log no longer says "no VRR". This matters most for the heavier games (Stalker 2, Arc Raiders, Elite Dangerous) that run below 165 fps.

**Does the compositor compete with the game?**
- Scanned out: the compositor does no drawing; it only flips.
- Composited (anything above the game): one GLES pass on a high-priority context. Recent composited frames show "refresh to submit p50 0.05 ms".
- **The recorder is the exception.** With `REPLAY_BACKEND="infynitae"`, `RECORD_FPS="60"` and `REPLAY_BUFFER_MINUTES="20"`:
  - `infynitae-record-stream --fps 60` captures through wlr-screencopy with shared-memory buffers (`live/record/infynitae-record-stream.c:70-110`, `172-192`).
  - It pipes raw 1920x1080 BGR0 frames (8 MB each, about 500 MB/s) into `ffmpeg … -vf hwupload,scale_vaapi … -c:v hevc_vaapi -b:v 14M`.
  - For every captured frame the compositor renders the screen off-screen and reads it back (`drm` `serve_captures`, called at the top of `render`, 2132-2141), even when the game itself is scanned out.
  - Measured on an idle desktop: compositor 9.6%, record-stream 9.6% and ffmpeg 9.6% of a core, all the time.
  - So during a game there is a full-screen GPU draw, a GPU-to-CPU readback, a CPU-to-GPU upload and a hardware encode 60 times a second. The readback runs on the compositor's main thread, the same thread that flips the game's frames.

**Tearing refusals.**
- The session log has 823 "Tearing on" lines and 340 "refused 60 times in a row" lines, all in the oldest part of the file.
- A single refusal right after scanout starts is normal (seen as "Async page flip refused, flipping on the vblank … Invalid argument").
- Runs of 60 are not explained. The branch logs overlay and cursor plane use on the first refusal (`drm:2392-2400`); read that line after the next session.

**What to change:**
- Capture with dmabuf instead of shared memory in the recorder and pass the frame to VAAPI without a CPU round trip. The compositor already has a dmabuf capture path (`drm:2724-2744`), and the recorder already receives the `linux_dmabuf` event (`infynitae-record-stream.c:251`).
- Until then, a `REPLAY_IN_GAMES` setting or a drop to 30 fps while game mode is on. For benchmarks, turn the recorder off.
- Test `echo high | sudo tee /sys/class/drm/card1/device/power_dpm_force_performance_level` in light games such as CS2, where the GPU clocks down between frames. I did not measure a gain; it costs idle power, so apply it only while a game runs (a gamemode start/end script can do that).

## 5. Input path

**Path in Infynitae.**
1. libinput is an event source on the main loop. Each event is handled as it arrives, not once per frame (`drm:1527-1534`, `handle_input` at 3155).
2. Pointer motion is turned into both accelerated and unaccelerated deltas with the kernel's microsecond timestamp (`drm:3255-3272`).
3. Core decides the target; the result is sent as `wl_pointer.motion` and `zwp_relative_pointer_v1.relative_motion` followed by one `frame` (`wl/pointer.rs:285-355`).
4. Clients are flushed at the end of the same loop turn (`drm:2037`).

**Findings.**
- No per-frame batching or coalescing of motion: a 1000 Hz or 8000 Hz mouse produces that many events to the game. Only an unchanged absolute position is dropped (`wl/pointer.rs:291-297`), which does not affect relative motion.
- The flush comes after `render` in the turn (`drm:2023` then `2037`). When the turn also composites a frame, the event waits for that draw: typically under 1 ms, but as long as the slow turns above. With direct scanout the wait is negligible.
- X11 games get relative motion through satellite and Xwayland (both protocols are in satellite's list), so raw input works, with two extra socket hops.
- `MOUSE_ACCEL_PROFILE="flat"` in your config, and games use the unaccelerated deltas anyway.
- USB polling: the Endgame Gear dongle and the keyboard report 125 µs intervals (8000 Hz capable); `usbhid.mousepoll` is 0, which means "use the device's own rate". Nothing to change.

**Compared with Windows raw input:** equivalent in design. Windows delivers `WM_INPUT` per report to the game's thread; here it is one compositor thread wake per report. The difference is that Infynitae's input, rendering and capture share one thread, so anything slow on that thread delays input.

**What to change:**
- Flush the focused client right after input is handled, before `render` (a few lines in the loop around `drm:2021-2023`).
- Longer term, move the capture readback off the main thread.

## 6. Measurement plan for a fair comparison

**Make the two sides equal first.**
- Same game build, settings file, resolution (1920x1080), 165 Hz, same map or benchmark, shaders already compiled (throw away the first run).
- VRR on on both sides, or off on both.
- Tearing equal: Windows with vsync off in a fullscreen game tears; Linux with `TEARING="always"` does too. Check that the session log says "Tearing on" and has no "refused" lines for the run.
- Recorder equal: Infynitae replay off and Xbox Game Bar capture off, or both on.
- No lab job, no VM, Discord closed or on on both.
- On Windows, set AMD Adrenalin Anti-Lag to match what you enable on Linux.
- Three runs each; compare medians.

**Tools.**

| Purpose | Windows | Linux |
| :--- | :--- | :--- |
| Frame times | PresentMon (or CapFrameX, which wraps it) | MangoHud logging: install `mangohud` and `lib32-mangohud`, launch with `MANGOHUD_CONFIG=output_folder=…,log_duration=60,autostart_log=5 mangohud %command%` |
| Did the frame bypass composition? | PresentMon `PresentMode` column: "Hardware: Independent Flip" is the direct path | Session log: "its frames go to the screen as they are (direct scanout)" and "Tearing on" |
| Compositor timing | (none needed) | `LATENCY_STATS="true"` (already on): the "Latency" and "Frame timing" lines every 10 s |
| CPU and GPU busy | PresentMon GPU busy columns | MangoHud log columns (cpu_load, gpu_load, frametime) |

**What to measure.**
- **Average fps, 1% low and 0.1% low** from the frame time list. Compute them the same way on both sides (for example with the same script over both CSV files), because tools define "1% low" differently.
- **Frame time p50, p99 and max.** p99 and max are what stutter feels like.
- **Latency proxy.** There is no software number that is the same on both systems:
  - PresentMon reports click-to-photon estimates only on Windows.
  - Infynitae's "input to screen" covers compositor input to refresh, not the game's own processing.
  - Useful software proxy: at equal fps, with tearing on both sides, the remaining difference is the render queue. Compare GPU load: a game pinned at 99% GPU has about one extra frame of queue, which is what Anti-Lag removes.
- **Real input-to-photon.** Film the mouse and screen with a phone at 240 fps (or better) and count frames from click to muzzle flash, 20 samples per system. Resolution is about 4 ms per camera frame, enough to see a one-refresh difference.

**How to read the results.**
- Linux within 3% on average fps and equal or better p99: equal in practice.
- Worse p99 with equal average: look for compositor slow turns, recorder load, or shader compilation.
- "Frame timing … submit to refresh" near 6 ms means frames wait a full refresh (not tearing or not just-in-time); near 0-3 ms means the low-latency path is working.
- Read the count of "frames missed their refresh" before and after each run; the difference is the run's missed frames.

## Ranked changes

Ordered by expected gain for how games feel. Size is the effort. Gains are estimates; none were measured in this review.

| # | Change | Where | Size | Needs |
| :--- | :--- | :--- | :--- | :--- |
| 1 | Turn FreeSync/Adaptive-Sync on in the monitor menu; confirm `vrr_capable` is 1 | Monitor OSD | Small | Owner |
| 2 | Recorder: stop the per-frame draw, readback and re-upload during games (dmabuf capture to VAAPI; until then pause or 30 fps while game mode is on) | `live/record/infynitae-record-stream.c`, `live/record/infynitae-recorder`, `drm` `serve_captures` | Medium | Code |
| 3 | Enable Anti-Lag: `ENABLE_LAYER_MESA_ANTI_LAG=1` for D3D12 games with the option, `DXVK_CONFIG="dxvk.latencySleep = True"` for D3D11 | Steam launch options | Small | Owner, no sudo |
| 4 | Keep heavy background jobs off while gaming (lab runs, VMs); for example have game mode pause the lab queue | Lab scripts, `live/launch.sh` `game-mode` | Small | Owner decision, then code |
| 5 | Real-time priority for the compositor's main thread (SCHED_RR 1, reset on fork) | `crates/infynitae/src/main.rs`; `install.sh` for `setcap cap_sys_nice+ep`, or install `rtkit` | Small | Code plus sudo once |
| 6 | Find and fix "element building" stalls of 8-19 ms | `wl/convert.rs` `output_elements`; the `turn.mark` log already names the phase | Medium | Code |
| 7 | Read the new "Tearing refused … overlay plane(s) in use, cursor plane …" line after a session and fix the cause of 60-in-a-row refusals | `drm:2392-2408` | Small to medium | Code, after one play session |
| 8 | `LATENCY` margin: shrink faster after a miss and ignore misses in the first frames after scanout starts | `wl/frame_schedule.rs` (`MISS_STEP`, `SHRINK_AFTER`, `vblank`) | Small | Code |
| 9 | Flush the focused client right after input, before drawing | `drm` main loop near 2021-2037 | Small | Code |
| 10 | Load ntsync at boot | `/etc/modules-load.d/ntsync.conf` | Small | Owner, sudo |
| 11 | gamemode config (`renice=10`, `ioprio=0`), install `lib32-gamemode`; register the real game pid for X11 games instead of satellite's | `/etc/gamemode.ini`; `wl/background.rs:182-209` | Small | Owner (sudo) and code |
| 12 | Test `PROTON_ENABLE_WAYLAND=1` and CS2 with `SDL_VIDEO_DRIVER=wayland`; keep whichever measures better per game | Steam launch options | Small | Owner |
| 13 | Test GPU `power_dpm_force_performance_level=high` during light games; keep only if p99 improves | sysfs, via a gamemode start/end script | Small | Owner, sudo |
| 14 | Offer `wp_fifo_v1` and `wp_commit_timing_v1`; send real flags in presentation feedback | `wl/lib.rs` (new globals), `wl/outputs.rs:578-586` | Medium | Code |
| 15 | `TEARING="always"`: do not force tearing on a window whose client explicitly asked for vsync, if the uncapped-vsync test in section 2 confirms the problem | `wl/tearing.rs:88-103` | Small | Code, after the test |
| 16 | Install `mangohud` and `lib32-mangohud` for the comparison | pacman | Small | Owner, sudo |
| 17 | Offer `linux-drm-syncobj-v1` (explicit sync) | `wl/lib.rs`, Smithay `drm_syncobj` | Medium | Code; low value on AMD |

**Not worth doing for latency:** Gamescope nested inside Infynitae, a sched_ext scheduler, or irqbalance. I found nothing on this machine that they would fix; revisit only if section 6's measurements show a gap that the list above does not close.
