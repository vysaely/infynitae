# Audit: Animation and Frame Timing (`AUDIT_FRAME_TIMING`)

Status (Claude's review, 2026-11-15): finding 1 for the zoom glide was fixed by PR #84 (FrameSchedule::shown_at; the animated cursor part is open). Finding 2 fixed in fix/glide-first-step (a glide's first step is one refresh). Open: 1 (cursor), 3-11.

## Ranked Findings Table

| # | Finding | Evidence (`file:line`) | How sure | What the user would see | Fix (1–2 sentences) | Size |
|---|---|---|---|---|---|---|
| 1 | Bug: `zoom_tick` and animated `cursor_image` evaluate at render start instead of the target refresh timestamp | `crates/infynitae-backend-drm/src/lib.rs:2287,2332`; `crates/infynitae-backend-winit/src/lib.rs:672-673`; `crates/infynitae-wayland/src/cursor.rs:589-594` | checked in code | Camera glides start unevenly when frame 2 defers under `LATENCY` (up to ~1.67x step at 165 Hz) and lag one render budget behind scanout; cursor frame flips near vblank arrive a refresh late. | Pass `started + s.schedule.until_next_refresh(started)` into `zoom_tick` and `state.set_current_time(...)` before drawing in `backend-drm` (and target vblank in `backend-winit`). | S |
| 2 | Bug: `tick_glide` freezes frame 1 on an un-ticked camera and leaps up to `2 * frame_period` after a quiet gap | `crates/infynitae-core/src/navigation.rs:696,766,808,865-895` | proved by a test you wrote | First refresh after triggering a glide on a camera with no prior `zoom_tick` moves nothing; after a quiet interval (`<= 2 * period`), frame 1 jumps up to 2x a normal step regardless of when input arrived. | Anchor each glide at its trigger/start timestamp (or step frame 1 by `frame_period` when `last_frame_tick` is `None` or older than one refresh) rather than `now - prev_tick`. | S |
| 3 | Bug: `backend-drm` calls `frame_tick` without any display scanout (DPMS/power-off, refused flip, empty render, virtual screen) and `handlers.rs` flags `new_buffer` on bufferless commits | `crates/infynitae-backend-drm/src/lib.rs:1872-1873,2270-2282,2648-2654,2658-2671,2690-2691`; `crates/infynitae-wayland/src/handlers.rs:451-462` | proved by a test you wrote (`handlers.rs`); checked in code (`backend-drm` paths) | Clients wait for scanout or send `wl_surface.frame` and get `wp_presentation.presented` at CPU render time even with monitors powered off or a refused page flip; bufferless commits wake rested screens into an unpaced DPMS loop. | Set `new_buffer` only when `NewBuffer` was attached on that commit; do not emit `wp_presentation.presented` when no flip queued, and rest under DPMS/power-off instead of ticking every wake. | M |
| 4 | Bug: `window_captured` emits `wp_presentation.presented` + `sync_output` for off-screen windows | `crates/infynitae-wayland/src/image_copy.rs:104-114`; `crates/infynitae-wayland/src/outputs.rs:587-615`; `crates/infynitae-wayland/src/frames.rs:4-5` | proved by a test you wrote | An off-screen captured window is falsely told its frame scanned out to a physical display and its pending presentation feedback is consumed (violating INV-FRAME-5). | Complete only `wl_surface.frame` callbacks (`complete_withheld(window, now)`) for captured off-screen windows instead of `complete_frames`. | S |
| 5 | Bug: Layer-shell, session-lock, client cursor, and IME popup surfaces never receive `wp_presentation` feedback | `crates/infynitae-wayland/src/layers.rs:185-208`; `crates/infynitae-wayland/src/lock.rs:405-418`; `crates/infynitae-wayland/src/cursor.rs:652-668`; `crates/infynitae-wayland/src/input_method.rs:99-114` | proved by a test you wrote (`layers.rs`); checked in code (lock/cursor/IME) | Panels (`edge-apps`), lock screens, client cursors, and IME popups asking for `wp_presentation.feedback` hang without `presented` (only implicitly `discarded` if recommitted). | Take and complete `take_presentation_feedback_surface_tree` alongside `send_frames_surface_tree` in all four helpers using the output's presentation metadata. | S |
| 6 | Bug: `complete_frames` always sends `Refresh::Fixed`, `seq = 0`, and `Kind::empty()`, ignoring DRM metadata, VRR, and tearing | `crates/infynitae-wayland/src/outputs.rs:587-615`; `crates/infynitae-backend-drm/src/lib.rs:1500-1504,1539,2374-2375,2536-2546` | proved by a test you wrote (`Kind::empty()`); checked in code (DRM/VRR/tearing) | Clients never see `Vsync`, `HwClock`, `HwCompletion`, `ZeroCopy`, or MSC `sequence`, cannot tell tearing async flips from vblank flips, and get a fixed `Refresh::Fixed` rate while VRR is active. | Pass vblank flags, kernel `sequence`, and VRR mode into `frame_tick` / `complete_frames`, sending `Refresh::Variable` when VRR is active and accurate `Kind` flags on every flip. | M |
| 7 | Bug: `FrameSchedule::start_at` and `rendered` trust stale `last_vblank` past `GUARD_TRUSTED` and iterate linearly over elapsed refreshes | `crates/infynitae-wayland/src/frame_schedule.rs:356-365,372,397-412,446-453` | proved by a test you wrote (`start_at` past `GUARD_TRUSTED`) | First frame after DPMS wake or long idle under `LATENCY` defers against a stale phase (`> 1 s` old) and runs a `while` loop proportional to idle duration / period. | Return `None` in `start_at` (and ignore `last_vblank` in `until_next_refresh`) when `now - last > GUARD_TRUSTED`, and compute phase alignment in O(1). | S |
| 8 | Bug: VRR and tearing flips feed fixed-period `FrameSchedule` and `gpu_relief`, miscounting VRR cadence as missed refreshes and shifting fixed phase on async flips | `crates/infynitae-backend-drm/src/lib.rs:1504,1515,2317,2370-2375,2536-2546`; `crates/infynitae-wayland/src/frame_schedule.rs:446-494` | checked in code | Under VRR, frame rates below max refresh (e.g., 60 FPS on a 165 Hz panel) increment `misses`, widen margins to `MARGIN_MAX`, pause late scheduling, and trip `gpu_relief`; tearing flips overwrite `last_vblank` mid-refresh; desktop `vrr_mode == Some(true)` stays deferred without an exclusive window. | Skip fixed-period miss accounting, `last_vblank` phase anchoring, and `gpu_relief` miss triggers while VRR or async tearing flips are active, and treat `s.vrr_on` consistently in `immediate`. | M |
| 9 | Improvement: `cursor_animation_due` truncates to milliseconds and wakes at boundary instead of ahead of target refresh | `crates/infynitae-wayland/src/cursor.rs:98,109-130,611-648`; `crates/infynitae-wayland/src/session.rs:558-560`; `crates/infynitae-backend-drm/src/lib.rs:2100-2101` | checked in code | Sub-millisecond cursor epochs drift by up to 1 ms, and waking right at the animation boundary misses a late-scheduled frame until the next refresh. | Compute sub-ms remainder and target the wake before the refresh crossing the cursor deadline once `cursor_image` uses predicted refresh time. | S |
| 10 | Improvement: Async SHM screencopy reports pre-render timestamp (`now`) at later GPU readback delivery (`deliver_copies`) | `crates/infynitae-backend-drm/src/lib.rs:2983,3112-3123,3304` | checked in code | Recorded capture frame timestamps reflect pre-render submission rather than readback completion or prior scanout. | Timestamp capture readiness at submission/fence completion consistently with the capture protocol's expectation (or document scanout vs submit clock explicitly). | S |
| 11 | Improvement: `maximize_toggle` snaps camera immediately instead of using `zoom_feel` glide; hidden layers have no fallback frame timer | `crates/infynitae-core/src/entry.rs:1009`; `crates/infynitae-wayland/src/layers.rs:196-207`; `crates/infynitae-wayland/src/frames.rs:44-91` | checked in code | Overview, fit, slots, and wheel zoom glide cleanly while `maximize_toggle` jumps in one frame; hidden layer panels receive zero `wl_surface.frame` callbacks until unhidden. | Route `maximize_toggle` through `glide_to` / `glide_restore` when `zoom_feel != Instant`, and decide whether occluded layers should share a 1 Hz fallback policy. | S |

---

## Inventory: Drawn Animations, Clocks, and Frame / Presentation Paths

All client-facing frame and presentation timestamps in `infynitae-wayland` use `CLOCK_MONOTONIC` (`presentation_clock_now()` in `crates/infynitae-wayland/src/lib.rs:754`, `outputs.rs:587`). Non-inspected external components: `libinput` kernel/device event clock internals, GPU driver internals behind DRM/EGL, and `xwayland-satellite` internals are **not checked**.

| Drawn visual / feedback path | Source (`file:line`) | Clock & timestamp currently used | Expected timestamp / behavior |
|---|---|---|---|
| **Zoom / overview / fit / slot / restore camera glide** | `infynitae-core/src/navigation.rs:865-895`, `entry.rs:1876-1890`, `backend-drm/src/lib.rs:2287,2332`, `backend-winit/src/lib.rs:672-673` | **Render start** (`presentation_clock_now()`) on `CLOCK_MONOTONIC` minus `last_frame_tick` | Should position the camera for the **predicted vblank** the frame lands on (`started + until_next_refresh(started)`), with first-frame `dt` independent of stale `last_frame_tick` |
| **Window / workspace maximize toggle** | `infynitae-core/src/entry.rs:1009` | None (instant `set_zoom_and_center` snap) | Instant even when `zoom_feel != Instant` |
| **Fades & window moves** | `grep` across `crates/` | None (no compositor opacity fades or interpolated window moves exist; camera pans apply `dx, dy` directly) | N/A |
| **Animated Xcursor** | `infynitae-wayland/src/cursor.rs:87-130,493-498,589-648`, `backend-drm/src/lib.rs:2100-2101`, `backend-winit/src/lib.rs:494-495` | **Render start** (`presentation_clock_now()` because `set_current_time` is never called by DRM/Winit); timer via `Instant::now() + due` | Should sample cursor frame at the **predicted vblank** (`set_current_time`) and align wakeup before that refresh |
| **Capture indicator** (`Dot` / `Edge`) | `infynitae-wayland/src/capture_indicator.rs:1-85`, `backend-drm/src/lib.rs:1684-1687,1736-1742` | `std::time::Instant::now()` (`HOLD = 3s` binary state toggle, not interpolated) | Binary visibility timeout only; triggers `request_frame()` when toggled |
| **Scale settle timer** (`ScaleInteractionEpoch`) | `infynitae-core/src/scale.rs:29` (`DEFAULT_T_SETTLE_MS = 300`) | Input / session monotonic time (`SessionTime`) | Triggers discrete integer scale reconfigure after interaction settles; not interpolated visually |
| **Visible windows (`wl_surface.frame` + `wp_presentation`)** | `outputs.rs:587-615`, `backend-drm/src/lib.rs:1500-1503,1539,2280,2651,2670,2691`, `backend-winit/src/lib.rs:716-717` | `DrmEvent::VBlank` (`DrmEventTime::Monotonic`, else `now`) on queued flip; **`now` (`render_start`)** on DPMS, refused flip, empty render, virtual screen, Winit | Physical scanout only: kernel vblank timestamp + MSC `seq` + `Kind` flags (`Vsync`, `HwClock`, `HwCompletion`, `ZeroCopy`) + `Refresh::Variable` under VRR |
| **Off-screen windows (`wl_surface.frame`)** | `frames.rs:1-92`, `infynitae-core/src/frames.rs`, `image_copy.rs:104-114` | `presentation_clock_now()` on input/configure kick and 1 Hz fallback (`frame_fallback`) | `wl_surface.frame` completes on kick/1 Hz fallback/`frame_captured`; `wp_presentation_feedback` must remain pending (violated by `window_captured`) |
| **Layer-shell, lock, client cursor, IME popups** | `layers.rs:185-208`, `lock.rs:405-418`, `cursor.rs:652-668`, `input_method.rs:99-114` | Output `frame_tick` timestamp via `send_frames_surface_tree(..., throttle: None, ...)` | Receives `wl_surface.frame` only; **`wp_presentation_feedback` is never answered** |

---

## Bug

### Bug 1: `zoom_tick` and animated `cursor_image` sample at render start instead of target refresh time

In `crates/infynitae-backend-drm/src/lib.rs:2287,2332`, `render()` captures `let started = infynitae_wayland::presentation_clock_now();`, defers via `s.schedule.start_for(started, s.deferred)`, and then calls `data.state.zoom_tick(id, started);` using render-start `started` rather than `started + s.schedule.until_next_refresh(started)` (note: this worktree `HEAD` `a46f427` still has `zoom_tick(id, started)` unfixed, and `crates/infynitae-backend-winit/src/lib.rs:672-673` likewise passes render-start `tick_now = presentation_clock_now()`). Neither `backend-drm` nor `backend-winit` calls `WaylandState::set_current_time(...)` before drawing, so `WaylandState::cursor_image` (`crates/infynitae-wayland/src/cursor.rs:589-594`) also falls back to `presentation_clock_now()` at render start.

Under `LatencyMode::{Balanced, Lowest, Direct}`, when a glide starts right after a vblank, frame 1 runs immediately (`~0.5 ms` after `last_vblank`), while frame 2 defers until `refresh - budget` (`~2 ms` before the third refresh at 165 Hz / 6.06 ms period). Passing `started` gives frame 2 `dt ~ 10.1 ms` (`1.67x` of one period), and every frame renders camera zoom and Xcursor animation state `budget` ms earlier than the refresh on which it scans out. Checked in code (`drm`/`winit` loops do not run under headless tests).

### Bug 2: `Core` `tick_glide` stalls for 1 refresh without prior `last_frame_tick` and leaps up to `2 * frame_period` after a quiet gap

In `crates/infynitae-core/src/navigation.rs:865-895`:
- When `c.last_frame_tick` is `None` (no prior `zoom_tick` has run on that camera), `(false, None)` produces `dt = Duration::ZERO`, sets `g.primed = true`, and returns `false`, causing zero camera movement on the first refresh after a zoom/fit/overview trigger (forcing callers such as `entry.rs:6467` to call `zoom_tick` beforehand while idle).
- When `c.last_frame_tick` is `Some(prev)` from an earlier render before the glide began and `raw = now - prev <= 2 * frame_period`, `(false, Some(prev))` advances frame 1 by `raw` (up to `2.0 * frame_period`), even if the input event arrived immediately before `now`. Furthermore, `glide_restore`, `glide_zoom_step`, and `glide_zoom_to` (`navigation.rs:696,766,808`) reset `primed: false`, re-entering that branch on subsequent wheel ticks.

Proved by tests in `crates/infynitae-core/src/navigation.rs`:
- `tick_glide_first_tick_without_prior_frame_moves_camera` failed: `first tick after glide trigger should move camera, got zoom=1`.
- `tick_glide_first_step_after_quiet_gap_does_not_exceed_one_refresh` failed: `first frame after quiet gap stepped 0.08155 vs 1-refresh 0.04641`.

### Bug 3: `backend-drm` fires `frame_tick` without hardware presentation; `handlers.rs` marks `new_buffer` on bufferless commits

`crates/infynitae-backend-drm/src/lib.rs` calls `data.state.frame_tick(id, presentation_clock_now())` without queueing or completing a page flip in four places:
1. **DPMS / `power_off`** (`2270-2282`): clears `s.deferred = None`, does **not** call `data.session.frame_rests(...)`, and calls `frame_tick(id, now)`.
2. **Refused page flip** (`2648-2654`): when `s.flip_failures > 0 && !queued`, calls `frame_tick(id, now)`.
3. **Empty / failed render** (`2658-2671`): when nothing is queued (`!queued`), calls `frame_tick(id, now)`.
4. **Virtual screen** (`2690-2691`): calls `frame_tick(v.id, now)` unconditionally on every render loop.

At the same time, `crates/infynitae-wayland/src/handlers.rs:451-462` sets `self.new_buffer |= has_buffer;` from `RendererSurfaceState::buffer().is_some()`, which remains `true` from earlier commits whenever an already-mapped surface commits anything (for example `wl_surface.frame()` + `wl_surface.commit()` with no `wl_surface.attach`). In `backend-drm/src/lib.rs:1872-1873`, `take_new_buffer()` calls `data.session.frame_wake()`, cancelling `frame_rests`. Together, a client committing frame callbacks on a blanked DPMS screen or after empty renders receives `wp_presentation.presented` at render-loop CPU time on a dark or unflipped screen and wakes the event loop without any new buffer.

Proved by test (`bufferless_frame_commit_does_not_mark_new_buffer` in `t1_audit_frame_timing.rs` failed: `commit without attaching a buffer must not mark a new buffer`); DRM paths checked in code.

### Bug 4: `window_captured` emits `wp_presentation.presented` for unobserved off-screen windows

`crates/infynitae-wayland/src/frames.rs:4-5` specifies that an unobserved window's presentation feedback stays pending until shown or superseded (INV-FRAME-5). However, `crates/infynitae-wayland/src/image_copy.rs:104-114` (`window_captured`) calls `self.complete_frames(output, &[window], now)` whenever `self.core.frame_captured(window)` is `true`, and `complete_frames` (`outputs.rs:602-613`) drains `wp_presentation_feedback` and emits `sync_output(&o)` + `presented(now, ...)` as though the window scanned out on physical `output`.

Proved by test (`window_captured_offscreen_must_not_emit_presentation_feedback` in `t1_audit_frame_timing.rs` failed: `off-screen capture must not claim physical scanout (INV-FRAME-5): ["sync #1 45", "presented #1", "frame W"]`).

### Bug 5: Non-window surfaces never receive `wp_presentation` feedback

`WaylandState::frame_tick` (`crates/infynitae-wayland/src/lib.rs:1129-1140`) only extracts `wp_presentation_feedback` inside `complete_frames` (`outputs.rs:587-615`, windows only). The four sibling frame completion functions:
- `complete_layer_frames` (`crates/infynitae-wayland/src/layers.rs:185-208`),
- `complete_lock_frames` (`crates/infynitae-wayland/src/lock.rs:405-418`),
- `complete_cursor_frames` (`crates/infynitae-wayland/src/cursor.rs:652-668`),
- `complete_input_method_frames` (`crates/infynitae-wayland/src/input_method.rs:99-114`)

call only `send_frames_surface_tree`. Neither `presented` nor `discarded` is ever sent for feedback attached to a layer, lock, cursor, or IME surface on vblank (`PresentationFeedbackCachedState::merge_into` at `vendor/smithay-0.7.0/src/wayland/presentation/mod.rs:322-336` only discards an older feedback if the surface commits another feedback).

Proved by test (`layer_surface_receives_wp_presentation_feedback_on_vblank` in `t1_audit_frame_timing.rs` failed: `visible layer surface must receive wp_presentation_feedback.presented: [..., "enter L 45", "frame L"]`); lock/cursor/IME paths checked in code.

### Bug 6: `complete_frames` hardcodes `Refresh::Fixed`, `seq = 0`, and `Kind::empty()`

In `crates/infynitae-wayland/src/outputs.rs:593-611`:
- `period` always uses `Refresh::Fixed(1s / refresh_mhz)` even though Smithay's `PresentationState::new` advertises `wp_presentation` v2 (`vendor/smithay-0.7.0/src/wayland/presentation/mod.rs:105`) and `PresentationFeedbackCallback::presented` already handles `Refresh::Variable(...)` (`mod.rs:268`).
- `seq` is hardcoded to `0` (`outputs.rs:609`) while `DrmEvent::VBlank` (`backend-drm/src/lib.rs:1500-1503,1539`) discards `meta.sequence` (`vendor/smithay-0.7.0/src/backend/drm/device/mod.rs:508`).
- `flags` is hardcoded to `wp_presentation_feedback::Kind::empty()` (`outputs.rs:605,611`), never setting `Vsync`, `HwClock` (when `DrmEventTime::Monotonic` came from DRM), `HwCompletion`, or `ZeroCopy` (direct scanout), nor distinguishing sync flips from async tearing flips (`drm/lib.rs:2536-2546`).

Proved by test (`complete_frames_sets_vsync_flag_on_vblank_presentation` in `t1_audit_frame_timing.rs` failed: `vblank presentation omitted Vsync flag: Kind(0x0)`); DRM `meta.sequence` / VRR / tearing flag isolation checked in code.

### Bug 7: `FrameSchedule::start_at` and `until_next_refresh` ignore `GUARD_TRUSTED` and loop linearly over elapsed time

`guarded_start` (`crates/infynitae-wayland/src/frame_schedule.rs:370-377`) refuses to trust `last_vblank` once `now - last > GUARD_TRUSTED` (`1 s`), documented as *"long after the last one its time is a guess"*. By contrast, `start_at` (`397-412`) and `until_next_refresh` (`356-365`) never check `GUARD_TRUSTED`, so under `LATENCY` Balanced/Lowest/Direct after `> 1 s` idle or DPMS, `start_at` defers wake frames using the stale phase. Worse, `start_at` (`403-406`) and `rendered` (`448-451`) step `while refresh < ... { refresh += period; }` linearly rather than O(1) modular arithmetic.

Proved by test (`frame_schedule_start_at_ignores_stale_last_vblank_past_guard_trusted` in `t1_audit_frame_timing.rs` failed: `assertion left == right failed: left: Some(12.00386s), right: None`).

### Bug 8: `backend-drm` feeds VRR and tearing flips into fixed-period `FrameSchedule` / `gpu_relief` and gates `immediate` on `exclusive_window`

In `crates/infynitae-backend-drm/src/lib.rs`:
- **VRR miss misclassification (`1504,1515,2304,2317,2370-2375`):** `s.schedule.period` stays set to max refresh (`m.vrefresh()`), yet `DrmEvent::VBlank` passes every VRR flip timestamp into `s.schedule.vblank(shown)` and feeds `s.schedule.counts()` into `s.relief.step(...)` (`1515`). In `frame_schedule.rs:438,446-491`, any VRR flip interval `< 4 * period` that crosses `target + period / 2` (for example a game running 60–100 FPS on a 165–240 Hz VRR output, or variable GPU completion times) counts as a missed refresh, widening margins to `MARGIN_MAX`, pausing late scheduling, and activating `gpu_relief`.
- **Desktop VRR still deferred (`2317,2370-2372`):** `immediate = may_tear || (s.vrr_on && data.state.exclusive_window(id).is_some())`. When `vrr_mode == Some(true)` (VRR enabled outside an exclusive fullscreen window), `immediate` is `false`, so composited frames defer to `last_vblank + k * min_period` anchored on sporadic VRR flips.
- **Tearing overwrites fixed-refresh `last_vblank` (`2535-2543`, `1504`):** Async flips (`set_async_flip(true)`) still call `s.schedule.rendered(started, submitted)` (`2541`) and feed flip completion into `s.schedule.vblank(shown)` (`1504`), setting `self.last_vblank = Some(time)` (`frame_schedule.rs:494`) to a mid-refresh tear time and throwing off phase prediction if tearing ends or misses. (Whether DRM emits `DrmEvent::VBlank` vs flip-complete timestamp semantics in the kernel for async flips in `vendor/smithay-0.7.0` `legacy`/`atomic` backend was **not checked**; Smithay `compositor/mod.rs:2554-2574` shows `self.async_flip` falls back to `false` on refusal and warns `"Async page flip refused, flipping on the vblank"`, while `commit_pending()` at `2554` takes `commit` without checking `async_flip`.)

Checked in code (DRM session not runnable under headless tests).

---

## Improvement

### 1. Replace `while` loops in `FrameSchedule::start_at` and `rendered` with O(1) division/modulo

`crates/infynitae-wayland/src/frame_schedule.rs:403-406` and `448-451` increment `Duration` by `period` in a `while` loop until passing `now + budget` or `submitted`. Computing the next aligned multiple with integer nanosecond arithmetic (matching `until_next_refresh` at `360-361`) prevents long wake stalls after multi-hour idle gaps even when `last_vblank` is refreshed or guarded.

### 2. Align `cursor_animation_due` precision and wakeup phase with target refresh

`LoadedCursor::frame_at` and `animation_due` (`crates/infynitae-wayland/src/cursor.rs:98,109-130`) cast elapsed time with `.as_millis() as u64` and schedule `Duration::from_millis(remaining_ms)` via a second `Instant::now()` inside `Session::set_cursor_animation_due` (`crates/infynitae-wayland/src/session.rs:558-560`). Once `cursor_image` evaluates at predicted vblank time (Bug 1), computing the timer wakeup ahead of the first refresh whose predicted vblank reaches the next frame prevents waking mid-frame after `render()` has already drawn the old cursor frame.

### 3. Align async SHM screencopy frame timestamp

`serve_captures` (`crates/infynitae-backend-drm/src/lib.rs:2983,3112-3123,3304`) captures `now = presentation_clock_now()` before rendering and stores it inside `PendingCopy`, so `deliver_copies` passes that pre-render timestamp to `req.ready(..., now)` after the GPU EGL fence signals on a later event-loop turn (while the DMA-BUF branch at `3032` calls `req.ready` right after `render_output`). Aligning captured timestamps consistently (e.g. post-submit/fence vs last presented vblank depending on capture intent) avoids stale timestamps across slow SHM readbacks.

### 4. Consistent `maximize_toggle` camera glide and hidden layer frame policy

- `Core::maximize_toggle` (`crates/infynitae-core/src/entry.rs:1009`) calls `set_zoom_and_center` instead of `glide_to` / `glide_restore`, making it the one camera transition that ignores `zoom_feel`.
- `complete_layer_frames` (`crates/infynitae-wayland/src/layers.rs:196-207`) only ticks layers in `core.layer_frame(output).above / below`, so layers hidden behind a fullscreen window receive no `wl_surface.frame` callbacks or fallback timer ticks (`frames.rs:44-91` only covers windows; `core/src/layer.rs` policy not checked in detail).
