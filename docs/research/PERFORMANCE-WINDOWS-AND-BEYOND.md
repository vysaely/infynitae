# Performance: Windows 11, other Linux desktops, and what Infynitae can take from each

> Since this was written (2026-11-12, morning): items 1 and 2's blockers are addressed by PR #49 (closed edge
> panels unmap over a window that fills the screen) and PR #50 (`TEARING="always"`, tearing with a still cursor:
> item 8, `LATENCY="direct"` default: item 6); PR #52 makes `LATENCY`'s margin recover in seconds. Item 3 (VRR) is
> the monitor's menu. See also `PERFORMANCE-LINUX.md` for this PC's measured state.

Research date: 2026-11-12. Target machine: Ryzen 5 7600X3D, Radeon RX 9070 XT, 1920x1080 at 165 Hz over DisplayPort, kernel 7.2.14, Mesa 26.2.6, gamemode 1.8.2.

One refresh at 165 Hz lasts 6.06 ms. "A frame" below means that unless a source says otherwise.

Each claim is labelled:

- **Measured**: someone timed it with hardware or tooling.
- **Vendor claim**: stated by the maker, without independent numbers.
- **Design**: follows from documentation or source code, not timed.
- **Unverified**: I could not confirm it.

## Summary

- **Windows has no secret fast path.** Its low latency comes from letting a game's frames skip the desktop compositor ("independent flip") and from in-game latency limiters (Reflex, Anti-Lag 2). Linux has an equivalent for each piece.
- **The measured gap is small and goes both ways.** On other people's hardware, Linux ranges from about 4 ms behind Windows (NVIDIA, KDE, VRR) to ahead of it (one developer's AMD tests). I found no independent click-to-photon comparison on an RX 9070 XT.
- **Infynitae's largest gains are fixes, not new techniques.** The 2026-11-11 window-states review found three things:
  - direct scanout has been blocked for screen-filling windows since 2026-11-10;
  - tearing has almost stopped engaging;
  - the monitor reports no VRR.
- **Three system-level changes have measured gains elsewhere:** running Proton games on native Wayland instead of XWayland, an anti-lag layer in GPU-bound games, and VRR.
- **Only a click-to-photon test on this PC, on both systems, can settle whether Infynitae beats Windows.** Section 5 gives the method.

## 1. How Windows 11 gets low latency

| Mechanism | What it does | Cost or saving | Status |
|---|---|---|---|
| DWM composition | Every window is drawn into the desktop picture by the compositor (DWM) before it reaches the screen. | Microsoft says composed presentation adds "some overhead"; it gives no number. | Design ([Microsoft](https://devblogs.microsoft.com/directx/demystifying-full-screen-optimizations/)) |
| Flip-model swap chain | The game shares its finished frames with DWM instead of copying them. Required for everything below. | "Better performance, lower power usage". | Vendor claim ([Microsoft](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/for-best-performance--use-dxgi-flip-model)) |
| DirectFlip and independent flip | When the game covers the screen, its frames go straight to the display and DWM sleeps. Microsoft says this has "the same efficiency as fullscreen exclusive". | Removes the composition step. | Design (same page) |
| Multi-plane overlay (MPO) | The GPU's display hardware blends the game and the desktop itself, so independent flip survives in a window or under a notification. | Keeps the fast path in windowed mode. Community reports link MPO to flicker and stutter on some setups. | Design; problems are [user reports](https://sageinfinity.github.io/docs/FAQ/mpo) |
| Fullscreen optimizations | A game asking for old exclusive fullscreen is run as a borderless flip-model window. | "On average, as good or better" than exclusive fullscreen, from Microsoft's telemetry. | Vendor claim ([Microsoft](https://devblogs.microsoft.com/directx/demystifying-full-screen-optimizations/)) |
| Optimizations for windowed games | Moves DirectX 10/11 windowed games from the old copy model to the flip model. Also what makes VRR and Auto HDR work in a window. | "Reducing frame latency", no number. | Vendor claim ([Microsoft](https://support.microsoft.com/en-us/windows/optimizations-for-windowed-games-in-windows-11-3f006843-2c7e-4ed0-9a5e-f9389e535952)) |
| Waitable swap chain | The game waits until the system can take a frame, then reads input and draws. Without it, a game is often "waiting almost a full extra frame". | About one frame; "down to 1 frame of latency" in independent flip. | Design ([Microsoft](https://learn.microsoft.com/en-us/windows/uwp/gaming/reduce-latency-with-dxgi-1-3-swap-chains), [flip model page](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/for-best-performance--use-dxgi-flip-model)) |
| `DXGI_PRESENT_ALLOW_TEARING` | Vsync off in a window or borderless: the frame is shown at once, with a tear line. "Even lower latency than the waitable object". | Removes the wait for the next refresh (0 to 6.06 ms at 165 Hz, by arithmetic). | Design ([Microsoft](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/variable-refresh-rate-displays)) |
| VRR in a window | Needs the tearing flag and a flip-model swap chain; works in borderless. | No number from Microsoft. | Design (same page) |
| Game Mode | I could not retrieve Microsoft's page. | None found. | Unverified |
| MMCSS | Raises the priority of threads registered as "Games", "Audio" and similar. By default 20% of CPU stays reserved for low-priority work. | No latency number. | Design ([Microsoft](https://learn.microsoft.com/en-us/windows/win32/procthread/multimedia-class-scheduler-service)) |
| HAGS | Moves GPU scheduling from a CPU thread to the GPU. "Users should not notice any significant changes". | No controlled on/off latency measurement found. | Vendor claim ([Microsoft](https://devblogs.microsoft.com/directx/hardware-accelerated-gpu-scheduling/)) |
| Raw input (`WM_INPUT`) | The game reads mouse movement straight from the device, without pointer acceleration or screen-pixel rounding. | Precision, not milliseconds. | Design ([Microsoft](https://learn.microsoft.com/en-us/windows/win32/dxtecharts/taking-advantage-of-high-dpi-mouse-movement)) |
| Timer resolution | A game can ask for finer sleep timers. Since Windows 10 2004 this is per process; Windows 11 may drop it for hidden windows. | Costs power; no latency number. | Design ([Microsoft](https://learn.microsoft.com/en-us/windows/win32/api/timeapi/nf-timeapi-timebeginperiod)) |

### Anti-Lag 2 and Reflex

Both stop the CPU from queueing frames ahead of a busy GPU. They work from inside the game, so the game must support them.

**AMD Anti-Lag 2** (available in CS2 and Dota 2 on Windows):

- AMD's own figures for CS2 at 4K on an RX 7900 GRE: about 19 ms without, 16 ms with the old Anti-Lag, 11 ms with Anti-Lag 2 (vendor claim, via [Tom's Hardware](https://www.tomshardware.com/pc-components/gpu-drivers/amd-introduces-radeon-anti-lag-2-with-full-integration-for-counter-strike-2-the-nvidia-reflex-alternative-shouldnt-trigger-cheating-bans-this-time)).
- [PC Games Hardware](https://www.pcgameshardware.de/Counter-Strike-2-Spiel-74230/Specials/AMD-Radeon-Anti-Lag-2-1448467/) measured much less with LDAT in CS2: 12.7 ms down to 11.6 ms on an RX 6700 XT at 1080p, and 2.5 ms (300 FPS cap) to 4.1 ms (uncapped) saved on an RX 7900 XTX.
- I could not open the PCGH page; those figures come from a search summary.
- Whether Fortnite and Valorant support Anti-Lag 2 is unverified.

**NVIDIA Reflex** (for comparison; all three figures below are from search summaries, pages not opened):

- Overwatch: 25.7 ms down to 16.8 ms ([TechPowerUp](https://techpowerup.com/review/nvidia-reflex-review-test-ldat-v2/3.html)).
- Valorant: about 3 ms ([TechSpot](https://www.techspot.com/article/2123-nvidia-reflex-rtx/)).
- Fortnite: 45.3 ms down to 42.3 ms, from 20 clicks ([Tom's Hardware](https://www.tomshardware.com/news/nvidia-reflex-latency-analyzer)).

The pattern across these: the gain is large when the GPU is the limit and small at high frame rates.

## 2. Where Linux matches or beats it

### The building blocks

- **Direct scanout** is the same idea as independent flip: a fullscreen window's buffer goes to the screen and the compositor only flips. Infynitae has it (`DIRECT_SCANOUT`).
- **Tearing** is requested through [tearing-control-v1](https://wayland.app/protocols/tearing-control-v1) and done with an async page flip.
  - Atomic async flips need kernel 6.8 or newer, according to the [Hyprland wiki](https://wiki.hypr.land/0.51.0/Configuring/Tearing/).
  - The kernel allows only the picture itself to change in such a flip ([`drm_atomic_uapi.c`](https://github.com/torvalds/linux/blob/master/drivers/gpu/drm/drm_atomic_uapi.c)).
  - amdgpu allows async flips on overlay planes; cursor changes go through a separate async-update path, not the flip path ([`amdgpu_dm_plane.c`](https://github.com/torvalds/linux/blob/master/drivers/gpu/drm/amd/display/amdgpu_dm/amdgpu_dm_plane.c)).
  - The kernel source says unchanged values are "always allowed". So a cursor that is visible but did not move should not block a tear. I have not tested this.
- **X11 games can ask for tearing.** XWayland has supported tearing control since [23.2](https://www.phoronix.com/news/XWayland-23.2-RC1), and Mesa passes an X11 OpenGL game's vsync-off through it ([Mesa MR 26906](https://gitlab.freedesktop.org/mesa/mesa/-/merge_requests/26906)). The header of Infynitae's `tearing.rs` says they cannot. Whether xwayland-satellite forwards the hint is unverified (review item 14).
- **Present timing.** Mesa has had proper vsync queueing on Wayland since 24.3 through the fifo and commit-timing protocols ([Phoronix](https://phoronix.com/news/Mesa-24.3-Vulkan-FIFO)). `VK_EXT_present_timing` arrived around Mesa 26.1; sources disagree on the exact version.

### Measured results

**VRR** (Marco Nett, RTX 4070 Super, KDE 6.7.2, 500 Hz, 300 clicks per case, [his write-up](https://marco-nett.de/blog/measuring-input-latency-on-linux-x11-vs-wayland-vrr-dxvk/)):

- VRR lowered median click-to-photon by 0.26 to 0.45 ms.
- It narrowed the spread from 2.6–3.0 ms to 2.1–2.2 ms.

**VRR and anti-lag on an RX 9070 XT** (Hans-Kristian Arntzen, timed with `VK_EXT_present_timing`, [July 2026](https://themaister.net/blog/2026/07/)):

- Under VRR, "GPU finished" to "on screen" averaged 0.45 ms.
- With mailbox presentation in a KDE window it averaged 6.45 ms.
- In GPU-bound Cyberpunk 2077 at about 60 FPS, anti-lag cut input-to-present from 75.9 ms to 50.0 ms.

**Native Wayland against XWayland:**

- Nett: XWayland added 3.13 ms (8.06 ms against 4.93 ms).
- The [dxvk-low-latency README](https://github.com/netborg-afps/dxvk-low-latency) (NVIDIA Reflex Analyzer): 11.9–14.7 ms for vsynced XWayland on Plasma, against 1.7–4.6 ms for Wine's Wayland driver with VRR.
- Both are on KWin. Infynitae uses xwayland-satellite, which nobody has measured.
- Proton's Wayland driver is opt-in (`PROTON_ENABLE_WAYLAND=1`). Its 2026 default status is unverified.

**Anti-lag layers:**

- Mesa has shipped `VK_AMD_anti_lag` as a layer since 25.3. On this PC it is installed and switched on with `ENABLE_LAYER_MESA_ANTI_LAG=1` (read from `/usr/share/vulkan/implicit_layer.d/VkLayer_MESA_anti_lag.json`).
- Mesa 26.3 moves it into the RADV driver ([Phoronix](https://phoronix.com/news/Mesa-26.3-RADV-VK_AMD_anti_lag)).
- The author of [low_latency_layer](https://github.com/Korthos-Software/low_latency_layer) (RX 7900 XTX, KDE 6.6, NVIDIA Reflex Analyzer) reports that Mesa's layer "appeared to be a no-op" and that his own layer matches or beats Windows Anti-Lag 2 in CS2 and THE FINALS.
- Those are the developer's own tests, and the README gives charts but no millisecond figures in text.

**DXVK frame pacing:**

- The dxvk-low-latency fork saved 0.10–0.29 ms when capped and 0.84 ms uncapped (Nett).
- `VKD3D_SWAPCHAIN_LATENCY_FRAMES=1` was the only DirectX 12 switch with a measurable effect in [farnoy's tests](https://farnoy.dev/posts/linux-latency).

**Linux against Windows:**

- **Latency, NVIDIA.** farnoy (RTX 40-series, KDE 6.6.4, 120 Hz, Teensy light sensor): Windows was "consistently lower", with a gap of about 4 ms under VRR before his KWin patches. The per-configuration numbers are only in charts I could not read.
- **Latency, AMD.** A [Reddit test](https://reddit.zackpollard.pro/r/linux_gaming/comments/1mejrig/linuxwayland_vs_win11_click_to_photon_latency) on an RX 7700 XT exists. I could not open it; a search summary says Windows had a slight edge and Linux was less consistent. Unverified.
- **Frame rate, RX 9070 XT.** ComputerBase (March 2026, CachyOS, Mesa 26.0.1, 19 games) had Linux about 2% slower at 1440p and 4% at 4K, and about 10% slower with ray tracing. Those figures come from a search summary; the [page I opened](https://www.computerbase.de/artikel/betriebssysteme/linux-mesa-26-windows-11-gaming-benchmarks-2026-q1.96102/) did not show them. It does say MangoHud logging is less exact than the Windows tools.
- **Not found.** No Digital Foundry or Phoronix gaming comparison on this card.
- **Handhelds.** SteamOS beat Windows by up to 56% on the Legion Go S per Ars Technica, read only through secondary coverage ([TechSpot](https://techspot.com/news/108468-new-benchmarks-show-steamos-outperforming-windows-11-lenovo.html)). That is a different class of machine.

### Scheduling and power

- **gamemode** changes the CPU governor, niceness, I/O priority and GPU performance mode ([README](https://github.com/FeralInteractive/gamemode)). Infynitae already triggers it (`GAME_MODE`).
- **CPU frequency hint.** This PC runs `amd_pstate` active with the energy hint at `balance_performance`. A kernel proposal that raises the hint on busy cores improved 1% lows by 31.8% in one game on a Steam Deck ([LWN](https://lwn.net/Articles/1085871/)). It is not merged, and the result is one game on one device.
- **sched-ext schedulers** (scx_lavd, scx_bpfland): I found Steam Deck claims but no desktop frame-time data. The state file on this PC reads "disabled". The effects of EEVDF and lazy preemption on games were not checked.
- **amdgpu power level** is `auto` here. The dxvk-low-latency README warns that a GPU clocked down under a low FPS cap can raise latency. No measurement found.

## 3. Where other Linux compositors stand

| Compositor | Direct scanout | Tearing | VRR | Frame timing | Worth taking |
|---|---|---|---|---|---|
| **KWin** (Plasma 6.x) | Yes, with regressions in 6.5 ([bug 510967](https://bugs.kde.org/show_bug.cgi?id=510967)) | Yes, per-window rule; the cursor then follows the game's frame rate ([bug 512591](https://bugs.kde.org/show_bug.cgi?id=512591)) | Yes | Predicted 11.35 ms for a 2.06 ms render (farnoy, measured); commit-timing merged ([Phoronix](https://phoronix.com/news/KDE-KWin-Wayland-Commit-Timing)) | Overlay planes, opt-in in 6.5 ([XDC 2025 slides](https://indico.freedesktop.org/event/10/contributions/424/attachments/275/363/XDC2025.pdf)); farnoy's timer fix |
| **Mutter** (GNOME 50) | Yes | [Issue 2517](https://gitlab.gnome.org/GNOME/mutter/-/issues/2517) asks for it; no evidence it shipped (unverified) | No longer experimental in 50 ([Phoronix](https://phoronix.com/news/mutter-50-vrr-deadline)) | VRR deadline and commit-timing work in 50 | Little for games |
| **Hyprland** | `render:direct_scanout` | Master switch plus an `immediate` rule per game; only when the game is alone on screen ([wiki](https://wiki.hypr.land/0.51.0/Configuring/Tearing/)) | Yes | Not checked | The "alone on screen" rule is the same as Infynitae's |
| **Gamescope** | "Directly flip game frames to the screen, even when stretching or when notifications are up" ([README](https://github.com/ValveSoftware/gamescope)) | `--immediate-flips` | `--adaptive-sync` | Composites with async Vulkan compute when it must | Flipping under notifications and scaling. Nested on a desktop it adds a copy (forum reports, unmeasured) |
| **niri** | Always on for the primary plane; overlay planes behind a flag ([wiki](https://github.com/niri-wm/niri/wiki/Configuration:-Debug-Options)) | No tearing option found (unverified) | Always, or on demand per window rule ([Arch wiki](https://wiki.archlinux.org/title/Variable_refresh_rate)) | Not checked | VRR per window rule instead of "any fullscreen window" |
| **Sway** | Yes | `allow_tearing` since 1.10 | `adaptive_sync` | `max_render_time`, set by hand in ms ([man page](https://man.archlinux.org/man/sway-output.5.en)) | Simplicity |

Infynitae already predicts its own render time from measurements (`frame_schedule.rs`). That is the part of KWin farnoy had to patch.

The KWin developer's [2026 hackfest notes](https://planet.kde.org/xavers-blog-2026-06-24-display-next-hackfest-2026/) add two points:

- KWin aims 1 ms before the refresh because the kernel sometimes wakes it hundreds of microseconds late.
- Slow amdgpu commits, the stutter that keeps overlay planes off by default, are being fixed.

## 4. What would let one setup beat both

Ranked by expected gain on this PC and how sure the evidence is. "Arithmetic" means derived from the 6.06 ms refresh, not measured.

| # | Technique | Expected gain | Evidence | Difficulty |
|---|---|---|---|---|
| 1 | **One "owns the screen" decision, and make it engage.** Fix the three faults in the review: the edge blocking scanout for screen-filling windows, buffers with an alpha channel never counting, and Wine windows never counting. | A composited game loses tearing and just-in-time flips and pays a GPU copy while the GPU is already full. Up to one refresh (arithmetic). | Review: scanout blocked since 11-10; Microsoft's design does exactly this. | Medium. Already designed as `screen_owner` in the review. |
| 2 | **Find out why tearing stopped engaging.** | Removes the 0–6.06 ms wait for the refresh (arithmetic) when vsync is off. | Review: "Tearing on" lines fell from 391 and 415 a day to 0, 1, 8, 0. | Unknown until the log is read after the 11-11 changes. |
| 3 | **Get VRR working.** The log says "no VRR (FreeSync off on the monitor, or not supported)". Check the monitor's menu first. | 0.26–0.45 ms median and a tighter spread at 500 Hz (Nett); likely more at 165 Hz. | Measured elsewhere, not here. | Trivial if it is the monitor setting. |
| 4 | **Proton games on native Wayland.** | 3.13 ms (Nett); roughly 10 ms in the fork's vsynced case. | Measured on KWin with NVIDIA; unmeasured with xwayland-satellite. | Low to try per game; some games break. |
| 5 | **Anti-lag for GPU-bound games** (Elite Dangerous at high settings; less for CS2). Try Mesa's layer, then RADV's own in 26.3, then low_latency_layer. | 26 ms at 60 FPS GPU-bound on this same GPU model (Arntzen); 1–4 ms at high frame rates (PCGH). | Measured, but one developer found Mesa's layer did nothing. | Low: an environment variable. Test each. |
| 6 | **`LATENCY="direct"` as the default.** Flip a directly shown game as late as possible instead of on its first frame after a refresh. | Up to one refresh when a vsynced game outruns the screen; nothing under tearing or VRR (arithmetic, from the config comments and `drm` scheduling code). | Same idea as the waitable swap chain; farnoy reached about 3 ms input-to-present on KWin at 120 Hz. | Built. Needs a missed-refresh check under GPU load. |
| 7 | **`LATENCY="balanced"` on the desktop.** | "About a frame less delay" per the config comment. | Recent `LATENCY_STATS` lines show p50 of 10.3–11.5 ms with `off`. | Built; marked experimental, misses more refreshes while the GPU trains. |
| 8 | **Tear while a cursor is visible but still.** | Tearing in games that always show a cursor, such as Dota 2. | Kernel source allows unchanged values; Infynitae currently refuses whenever a cursor is on the cursor plane (`drm` line 2332). | Medium; needs a test on amdgpu. A moving cursor remains unsolved, and KWin has the same limit. |
| 9 | **Offer fifo, commit-timing and explicit sync.** The vendored Smithay 0.7 has all three; Infynitae does not use them (no matches in `crates/`). | Steadier pacing, not raw milliseconds. | Mesa uses fifo and commit-timing since 24.3; full present timing needs commit-timing ([KDE bug 513283](https://bugs.kde.org/show_bug.cgi?id=513283)). | Medium. |
| 10 | **CPU and GPU tuning.** Keep gamemode; try the amdgpu "high" power level and a sched-ext scheduler as experiments. | Unknown on this PC. | Thin: one Steam Deck result, one README warning. | Low, but measure before keeping. |
| 11 | **Overlay planes for windowed games** (the MPO equivalent). | Fast path in a window. | `OVERLAY_PLANES` is off because it dropped frames on AMD; KWin reports the same driver issue being fixed. | Wait for the kernel fix. |

**Recommendation:** do 1 to 3 first. They restore what Infynitae already had and cost nothing in new design. Items 4 and 5 are system settings to try per game. Change the `LATENCY` default (6 and 7) only after measurement shows the missed-refresh rate stays near today's.

## 5. Measuring it fairly on this PC

Only CS2, Dota 2 and Elite Dangerous run on both systems. Valorant and Fortnite cannot be compared.

### Hardware method

Software cannot see the screen light up, so the comparison needs a sensor.

- **Light sensor.** A small board acts as a USB mouse, clicks, and times a light sensor on the screen. Two open designs: [click2photon](https://github.com/marconett/click2photon) (Nett) and the Teensy sketch farnoy used. Nett took 300 clicks per case.
- **High-speed camera.** A 240 FPS phone camera resolves 4.2 ms per frame, too coarse against a 6.06 ms refresh. It needs 960 FPS or more, an LED wired to the mouse button, and many repeats.

### Keeping it fair

- **Same hardware:** monitor, cable, port, monitor settings, mouse and USB port, and sensor position on the screen.
- **Same game setup:** map, spot, settings, FPS cap.
- **Order:** alternate the two systems rather than testing one fully first.
- **Confirm the fast path is really on:**
  - Windows: PresentMon's `PresentMode` column.
  - Infynitae: the scanout and "Tearing on" lines in the session log.

Test each game in three modes:

1. vsync off with tearing, uncapped;
2. VRR with a cap a few FPS under 165;
3. vsync on.

### Software tools

- **Windows:**
  - [PresentMon](https://github.com/GameTechDev/PresentMon/blob/main/README-CaptureApplication.md) logs every frame: `FrameTime`, `DisplayLatency`, `MsClickToPhotonLatency`, `MsPCLatency`.
  - AMD's [Frame Latency Meter](https://www.tomshardware.com/video-games/pc-gaming/amd-introduces-free-and-open-source-frame-latency-meter-utility-works-with-all-gpus-no-high-speed-camera-or-sensor-required) measures mouse-to-frame latency without hardware. Windows only.
- **Linux:**
  - MangoHud for frame times. Set `log_interval=0`; the default differs between versions.
  - `VK_EXT_present_timing` layers such as Arntzen's.
- **Infynitae:** `LATENCY_STATS` measures from input arriving to the refresh that showed the next frame the compositor put up. For a directly shown game that frame may not yet contain the input. Treat it as the compositor's share, not the game's latency. This is my reading of `frame_schedule.rs`.

### Numbers to compare

- **Click-to-photon:** median, 5th to 95th percentile spread, and 99th percentile.
- **Frame time:** average, 1% low and 0.1% low.
- **Missed refreshes** (Infynitae logs these).

A difference under about 0.5 ms is inside the run-to-run spread Nett saw (2 to 3 ms from 5th to 95th percentile).

## What I could not verify

- Microsoft's Game Mode description.
- Any measured HAGS effect.
- Anti-Lag 2 support in Fortnite and Valorant.
- Whether CS2 and Dota 2 call anti-lag on Linux.
- The PCGH, TechPowerUp, TechSpot, Tom's Hardware and ComputerBase figures (search summaries only).
- The Reddit RX 7700 XT test.
- Mutter and niri tearing status.
- Proton's Wayland default in 2026.
- The kernel version that allowed overlay-plane async flips on amdgpu.
- Whether xwayland-satellite forwards tearing hints.
- Windows' PresentMode value names.
- Hyprland's cursor and VRR options.
- Nested Gamescope's added latency.
