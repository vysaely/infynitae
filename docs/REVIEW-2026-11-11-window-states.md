# Infynitae review: window properties and "a window that fills the screen behaves like fullscreen"

Read-only review of `~/infynitae` at main `562879d`; nothing was built or run. I also read `~/.local/state/infynitae/session.log` and the live config as evidence.

## Headline findings

- **The feature already exists.** `FIT_IS_FULLSCREEN` (`crates/infynitae-wayland/src/fitted.rs`, on in your config) treats a window that exactly fills the screen as fullscreen.
- **The Rust edge has blocked it since 2026-11-10.** Direct scanout by day, for windows treated as filling the screen:

  | Day | Episodes | Got direct scanout |
  | :--- | :--- | :--- |
  | 11-05 | 77 | 51 |
  | 11-06 | 90 | 53 |
  | 11-08 | 119 | 97 |
  | 11-09 | 44 | 33 |
  | 11-10 | 124 | 0 |

  - On 11-10 every composited frame had an extra 1920x1080 element in the list.
  - `live/edge-rs/src/wayland.rs:1051-1068` creates a Top-layer surface over the whole output, and `clear_and_commit` (328-355) keeps a transparent buffer attached while closed.
  - Smithay scans out only when the window is the last element and nothing above it was drawn on the primary plane (`vendor/smithay-0.7.0/src/backend/drm/compositor/mod.rs:1999-2018`).
  - A real fullscreen window avoids this because `layer_frame` hides every layer but Overlay (`crates/infynitae-core/src/layer.rs:343-348`).
- **Tearing has almost stopped engaging.** "Tearing on" lines: 391 (11-05), 415 (11-06), then 0, 1, 8, 0, although direct scanout happened 100+ times on each of 11-08 to 11-10.
  - On the two days it worked, Super+Shift+O was pressed 4 times each; it was not pressed after.
  - That toggle is stored per window id (`tearing.rs:32-36`) and is lost when the game closes.
  - Why the games' own hints do not turn it on is not determined from the log. Hint handling was changed on 11-11 (`8e02e66`, `5e28834`, `09c8efe`), so check the log after the next session.
- **VRR is unavailable on this monitor right now.** The log says "Screen DP-1: no VRR (FreeSync off on the monitor, or not supported)" at every start (`crates/infynitae-backend-drm/src/lib.rs:1003-1009`). Check the monitor's menu first.

## 1. Map of per-window properties

Paths are under `crates/`; `wl/` is `infynitae-wayland/src/`, `core/` is `infynitae-core/src/`, `drm` is `infynitae-backend-drm/src/lib.rs`.

| Property | Decided at | Inputs | Re-evaluated | Survives reopen / restart |
| :--- | :--- | :--- | :--- | :--- |
| Fullscreen (exclusive) | `core/protocol.rs:1275-1308`; key in `core/entry.rs:792` | App `set_fullscreen` (only if focused or before first map, and not in `left_fullscreen`); Super+Shift+Enter | On request, output change, unplug | Restart: yes (`infynitae/src/restore.rs:164`, 627-662). Reopen: app's choice |
| Fullscreen stepped aside | `core/lib.rs:762-801` | Pan with `FULLSCREEN_PAN`, navigation | On navigation | No |
| Maximized | `core/entry.rs` `maximize_toggle`, `maximize_opened` (1616) | Super+Enter, app maximize | Output or bar change | Restart: yes (`restore.rs:165`, 604-613) |
| Fit (camera zoom) | `core/entry.rs` `fit_toggle` | Super+Enter with `SUPER_ENTER_ACTION="fit"` | One-shot | Camera is saved; the fit is not a state |
| Fills the screen | `wl/convert.rs:763-767` calling `wl/fitted.rs:22-37` and 78-83 | Frame geometry, `FIT_IS_FULLSCREEN`, lock, buffer format, Wine rule | Every rendered frame | Derived, nothing to save |
| On top | `core/protocol.rs:2387`; by name 1196-1204, 2354-2362; key `entry.rs:792-798` | `KEEP_ON_TOP` (exact title or app id), Super+Shift+K | Config rule only when the title or app id changes | Config rule: yes. Key toggle: no |
| Private | `wl/lib.rs:1583-1601`; toggle 1604-1618 | `PRIVACY_APPS`, `PRIVACY_TITLES` (substring), Super+Shift+P | Every capture frame | Config: yes. Toggle: no |
| Transparent | `wl/lib.rs:1694-1716` | `WINE_TRANSPARENCY`, app id contains "wine" or ends ".exe", `FORCE_TRANSPARENT_APPS` | Every frame | Config only |
| Tearing | `wl/tearing.rs:83-98`; toggle 120-139 | `TEARING`, app hint, Super+Shift+O/T, lock, `exclusive_window` | Every frame | Hint: per surface. Toggle: no |
| Async flip actually used | `drm:2327-2352`, back-off 2094-2096 | Scanned out, tears, no cursor element on the cursor plane, fewer than 60 refusals | Every frame | n/a |
| VRR | `drm:2199-2206` | `VRR_ENABLED` (read at start, `infynitae/src/main.rs:477`), display tools, `exclusive_window` | Every frame | Per screen, config |
| Scanout allowed | `drm:2222-2243`, Smithay 1999 | `DIRECT_SCANOUT`, `OVERLAY_PLANES`, `HARDWARE_CURSOR`, 5 s ban after a refused flip (2416) | Every frame | n/a |
| Black behind the window | `drm:2246-2253` | Fitted or fullscreen | Every frame | n/a |
| Just-in-time frames | `drm:2155-2175`, `wl/frame_schedule.rs:17-49` | `LATENCY`, last frame scanned out, tears, VRR | Every frame | Config |
| Game mode | `wl/background.rs:182-209`, end 233-246 | `GAME_MODE`; fullscreen, or fitted **and** tearing | Each frame shown; 5 s hold to end | No |
| GPU relief exemption | `core/lib.rs:1294-1319`, set at `drm:1501` | `GPU_RELIEF`, focus, fullscreen, fitted | Each vblank | n/a |
| Capture dot hidden | `wl/capture_indicator.rs:138-143` | Real fullscreen only | Every frame | n/a |
| Layers hidden | `core/layer.rs:343-348` | Real fullscreen only | Every frame | n/a |
| Border and clip off | `wl/convert.rs:831-844`, 896 | Fullscreen or fitted | Every frame | n/a |
| Idle inhibit | `wl/idle_inhibit.rs:54-64`, `wl/lib.rs:1620-1636` | App protocol, logind, Super+Shift+I | On idle check | No |
| Content type | `wl/handlers.rs:1247-1250` | `wp_content_type_v1` | Read by nothing | n/a |
| Trust (`ALLOW_*`) | `wl/lib.rs:1105-1107` | Config | Only for clients that connect afterwards | Config |
| Placement | `core/protocol.rs:1838`; see `docs/WINDOWS.md` | `PLACEMENT_POLICY`, pointer, parent | At first map | Anchor and size saved every 15 s |

## 2. Inconsistencies and bugs

1. **"Has the screen to itself" is decided in five places with different rules.**
   - `exclusive_window` (`fitted.rs:71-76`) counts fullscreen or fitted.
   - `drm:2246-2248` repeats the same test by hand.
   - Game mode adds "and tears" for fitted (`background.rs:191`).
   - The capture dot and `layer_frame` use real fullscreen only.
   - Relief uses `presentation()` without the stepped-aside check (`core/lib.rs:1308`).
   - Scenario: a maximized game gets VRR and the black clear, but no game mode unless it tears, and the edge and capture dot stay drawn over it and stop scanout.
2. **The edge blocks scanout for fitted windows** (headline).
3. **The capture dot blocks scanout for fitted windows.** It is a solid element above the window (`convert.rs:778-796`). Before 11-10 most composited fitted episodes had the dot in the frame. This is deliberate (comment at `capture_indicator.rs:139` cites `docs/SECURITY.md`), so it needs your decision.
4. **Buffers with an alpha channel never count as fitted.** `window_has_no_alpha` looks only at the format (`fitted.rs:86-108`), not at the app's opaque region. The log shows fullscreen games sending `AR24`.
5. **Wine windows never count as fitted.** With `WINE_TRANSPARENCY="true"`, any app id containing "wine" or ending ".exe" is excluded (`fitted.rs:81`). In real fullscreen the same windows are given alpha 0.9999 and no opaque region (`convert.rs:364-390`). Proton games whose app id is not of that form are unaffected; I did not check what ids your games have.
6. **Fit requires exact pixels.** `fills_the_view` (`fitted.rs:22-37`) needs:
   - zoom exactly 1;
   - the root surface rect equal to the screen, so a window with shadow margins fails although its geometry fits;
   - one surface only, so a subsurface (video players) or an own popup ends it;
   - nothing in front, so a picture-in-picture kept on top ends it.
   
   A window one pixel off after a free pan fails. Only Super+Space lines it up.
7. **Maximize can place the window out of view** when the top-left of the view is taken (`docs/WINDOWS.md`). It then fills nothing until you go to it.
8. **Runtime toggles are not remembered.** Tearing, privacy and on-top are keyed by window id and never saved; `restore.rs` saves none of them.
9. **`KEEP_ON_TOP` on reload** only applies when a window next changes its title or app id (`protocol.rs:1196`). Removing a name does not un-pin. It also matches exactly, while `PRIVACY_*` match by substring.
10. **Config reload treats a removed line two ways.**
    - `TEARING`, `GAME_MODE`, `GPU_RELIEF` and `BACKGROUND_PRIORITY` fall back to the default (`wl/lib.rs:1118-1120`, 1470-1473).
    - `FIT_IS_FULLSCREEN`, `LATENCY`, `DIRECT_SCANOUT`, `HARDWARE_CURSOR`, `OVERLAY_PLANES`, `KEEP_ON_TOP`, `PRIVACY_*` and `WINE_TRANSPARENCY` keep the old value until restart.
    - `VRR_ENABLED` is not reloaded at all.
11. **VRR "auto" turns on for any opaque maximized window**, not only games (`drm:2200`), with no hold. Each pan away and back switches VRR.
12. **Tearing is skipped whenever a cursor element is on the cursor plane** (`drm:2332`), not only when it moved. A game that shows a cursor never tears.
13. **Fitted state lags one frame.** It is a side effect of building the frame (`convert.rs:767`), while `tears()` and `immediate` are read before that (`drm:2162`). Harmless today, but it is state written from the render path.
14. **The `tearing.rs` header says X11 games cannot ask for tearing.** I could not confirm that from this repo; Xwayland may be able to send the hint for its own surfaces. Worth one test.

## 3. Design: one predicate, "the window owns the screen"

**What needs real fullscreen today:** hiding Top/Bottom/Background layers, hiding the capture dot, the presentation view showing only that window, and game mode without tearing. Everything else already keys off `exclusive_window`.

**Where:** keep it in `wl/fitted.rs`, as `WaylandState::screen_owner(output) -> Option<Owner { window, kind: Fullscreen | Covers, game: bool }>`. Compute it once per frame before any consumer reads it, not inside element building. The geometry half should be a pure function in Core so headless tests can cover it.

**Conditions for `Covers`:**
- Session unlocked and screen on.
- Zoom exactly 1.
- The frontmost window's geometry (not its root surface) equals the screen rect.
- No other window in front.
- Opaque: no alpha in the format, or its opaque region covers the surface, or it is a game.
- `game` is true if it tears, or has a pointer lock or confine, or declares content type "game".

**Getting to exact:** snap. When a pan ends at zoom 1 with a screen-sized window within about 24 px of alignment, line the camera up. Maximize of a game should bring the camera to it.

**Hysteresis:**
- Scanout and tearing follow the predicate each frame. They must drop the moment a pan shows anything else.
- Policy consumers hold: game mode keeps its 5 s, VRR auto gets about 2 s before turning off, relief about 1 s.
- Its own popup pauses scanout but does not end ownership.

**Consumers to switch:**
- `tears` (`tearing.rs:88`);
- VRR and `immediate` (`drm:2163`, 2200);
- the black clear (`drm:2246`);
- game mode (`background.rs:186-192`, 241), using `game` in place of "tears";
- relief (`core/lib.rs:1308`);
- border and clip (`convert.rs:831`, 896);
- the capture dot (`capture_indicator.rs:138`);
- `layer_frame` (`layer.rs:343`);
- the client-minimize refusal for a fullscreen window (`protocol.rs:2301`).

**Risks:**
- **Panning:** ownership ends on the first moved frame. Fine with the holds above.
- **Edge opening over a game:** scanout pauses while it is open. This is correct, but hover-open at the screen edge would fire mid-game. I suggest hiding the Top layer while `game` is true, as real fullscreen does.
- **Discord and Steam overlays, notifications:** each one composites while shown. Steam's overlay draws inside the game, so only its cursor matters.
- **Capture dot:** hiding it for an owner changes the `docs/SECURITY.md` promise. The existing "capture hidden" notification could cover it. Your decision.
- **Multi-monitor:** per output. A cursor on the other screen does not block tearing here.

## 4. Compared with Windows' low-latency path

| Windows | Infynitae today | Missing or worse | To match or beat |
| :--- | :--- | :--- | :--- |
| Flip model with independent flip; "fullscreen optimizations" for borderless windows | Primary-plane scanout (`drm:2222-2243`) plus `FIT_IS_FULLSCREEN` | Blocked by the edge surface, the capture dot, alpha formats and Wine ids | Changes 1, 3 and 4 below. After that the compositor only flips, the same as Windows |
| Multiplane overlay: the game stays direct while other content goes on another plane | `OVERLAY_PLANES`, off because amdgpu refused flips (`drm:2216-2218`) | Anything above the game forces composition; shared-memory surfaces cannot go on a plane | Large: draw the compositor's own UI into a separate overlay-plane buffer. Keeping things off the screen gets most of the benefit |
| `DXGI_PRESENT_ALLOW_TEARING` | `wp_tearing_control_v1` and a manual key (`tearing.rs`); async flip via `patches/0002` (`drm:2342-2352`) | Not remembered; off with any visible cursor; 60 refusals give a 5 s wait; engaging rarely since 11-07 | `TEARING_APPS` or `TEARING="always"` for a game that owns the screen; skip tearing only on frames where the cursor changed |
| VRR in windowed mode | VRR auto for `exclusive_window` (`drm:2199-2206`), drawn at once under VRR (2163) | Monitor reports no VRR; setting read only at start; no hold | Enable FreeSync on the monitor; add the hold; reload the setting |
| Reflex / Anti-Lag (the game delays its own frame start) | `LATENCY` delays only the compositor's draw (`frame_schedule.rs`); frame callbacks go out at vblank (`drm:1501-1503`) | A vsynced game starts right after vblank and its frame waits about 6 ms at 165 Hz; `LATENCY` is `off` in your config | Set `LATENCY="direct"`; delay the owner's frame callback to refresh minus its measured frame time minus a margin |
| Hardware cursor | Cursor plane (`drm:2241`) | Blocks tearing while visible | As in the tearing row |
| Waiting for the GPU before showing a frame | Frame waits (`drm:2935-2962`) | No `linux-drm-syncobj-v1`, no fifo or commit-timing protocols | Low priority on AMD |

For in-game Anti-Lag on AMD, check whether your Mesa and DXVK versions expose it. I did not verify that from this repo.

## 5. Ranked changes

1. **Small. Edge: no buffer while closed.**
   - In `live/edge-rs/src/wayland.rs` `clear_and_commit`, attach no buffer when closed, or use a small hover-strip surface instead of the whole output.
   - This alone should restore scanout for maximized games.
2. **Small. Remember tearing per app.**
   - Add `TEARING_APPS`, or `TEARING="always"` meaning any game that owns the screen.
   - Read it in `tearing.rs` `tears`, and add the reload case in `wl/lib.rs`.
3. **Small. Opacity.**
   - `fitted.rs` `window_has_no_alpha` should accept a full opaque region.
   - Drop the blanket Wine exclusion in `fit_counts_as_fullscreen` when the window covers the screen.
4. **Medium. `screen_owner`.**
   - One predicate as in section 3, computed before the frame in the `drm` `render` function.
   - Switch the consumers listed there, including `layer_frame` and `capture_mark_hidden` once you decide on the dot.
5. **Small. Tear with a still cursor.**
   - At `drm:2332`, compare the cursor plane state with the last frame instead of `!cursor`.
6. **Small. Set `LATENCY="direct"`** in your config.
   - The code path exists (`frame_schedule.rs:42-49`) and has not been used on your machine.
7. **Medium. Snap and maximize-in-view.**
   - Add the pan-end snap in `core/navigation.rs`.
   - `maximize_toggle` should bring the camera to a game.
   - Use window geometry, not the root surface, in `fills_the_view`.
8. **Medium. Late frame callbacks for the owner.**
   - In `wl/frames.rs` and the `drm` vblank handler.
9. **Small. VRR.**
   - Add a hold before turning it off at `drm:2200`.
   - Handle `VRR_ENABLED` in `reload_config`.
   - Enable FreeSync on the monitor.
10. **Medium. Remember toggles.**
    - Save on-top, private and tearing by app id and title in `restore.rs`.
    - Make `KEEP_ON_TOP` re-evaluate on reload.
    - Make removed config lines fall back to defaults everywhere.
11. **Large. Overlay-plane UI.**
    - The multiplane equivalent, so a notification or panel does not drop the game to composition.