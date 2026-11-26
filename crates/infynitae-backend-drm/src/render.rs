//! Drawing a frame: the renderer's recovery, the scaling filter, the tearing policy, the render pass and the frame waits.

use super::*;

/// The GPU lost the renderer's context (a reset, or out of GPU memory): a new renderer on the same
/// GPU takes its place, and the screens draw everything again. Clients keep their windows; their
/// buffers are imported again by the new renderer. At most once a second: until GPU memory frees
/// up, frames draw nothing and the screens keep their last picture.
pub(super) fn recover_renderer(data: &mut LoopData) {
    let now = std::time::Instant::now();
    if data
        .renderer_recovered_at
        .is_some_and(|t| now.duration_since(t) < Duration::from_secs(1))
    {
        return;
    }
    data.renderer_recovered_at = Some(now);
    log_line!(
        "[infynitae] The GPU reset or ran out of memory: making a new renderer (windows stay open)"
    );
    let mut renderer = match gles_renderer(&data.factory.gbm) {
        Ok(r) => r,
        Err(e) => {
            log_line!("[infynitae] No new renderer yet ({e}): trying again in a second");
            return;
        }
    };
    data.zoom_cache.clear();
    let tex_filter = if data.scaling_filter.uses_nearest_upscale() {
        TextureFilter::Nearest
    } else {
        TextureFilter::Linear
    };
    let _ = renderer.upscale_filter(tex_filter);
    data.zoom_programs = smooth::zoom_programs(&mut renderer);
    data.state.corners_drawn = smooth::init_corners(&mut renderer);
    // the copy thread may still read the old renderer's mappings
    data.capture_worker.wait();
    data.renderer = renderer;
    // copies started on the lost renderer cannot be read: their captures fail (the client asks again)
    for c in data.capture_copies.drain(..) {
        for req in c.reqs {
            req.fail();
        }
    }
    for id in data.capture_readback_deferred.drain(..) {
        data.state.screencopy.wake_waiting(id);
    }
    for s in &mut data.screens {
        s.screencopy_texture = None;
        let _ = s.compositor.reset_state();
    }
    if let Some(v) = data.virtual_screen.as_mut() {
        v.screencopy_texture = None;
    }
    data.session.request_frame();
}

fn apply_scaling_filter<K>(
    current: &mut infynitae_wayland::ZoomFilter,
    next: infynitae_wayland::ZoomFilter,
    renderer: &mut GlesRenderer,
    zoom_cache: &mut smooth::ZoomCache,
    screencopy_damage: &Mutex<infynitae_wayland::screencopy::CaptureDamage<K>>,
) -> bool {
    if next == *current {
        return false;
    }
    *current = next;
    let tex_filter = if next.uses_nearest_upscale() {
        TextureFilter::Nearest
    } else {
        TextureFilter::Linear
    };
    if let Err(e) = renderer.upscale_filter(tex_filter) {
        log_line!("[infynitae] Warning: failed to update upscale filter: {e:?}");
    }
    if !next.averages_zoom_out() {
        zoom_cache.clear_mips();
    }
    if !next.is_fsr() {
        zoom_cache.clear_fsr();
    }
    *screencopy_damage.lock().unwrap() = Default::default();
    true
}

pub(super) fn sync_scaling_filter(data: &mut LoopData) {
    if !apply_scaling_filter(
        &mut data.scaling_filter,
        data.state.scaling_filter,
        &mut data.renderer,
        &mut data.zoom_cache,
        &data.state.screencopy.damage,
    ) {
        return;
    }
    for s in &mut data.screens {
        // Redraw with the new filter without resetting DRM surface state (which forces a blocking
        // mode set on the next flip).
        s.compositor.reset_buffers();
    }
}

/// Async page flips refused in a row by the kernel before a screen pauses tearing until
/// [`TEARING_RETRY`] or the window stops tearing.
pub(super) const TEARING_REFUSALS: u32 = 60;
/// How long frames wait for the refresh after repeated kernel refusals, before trying again.
pub(super) const TEARING_RETRY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TearingPolicyInput {
    pub direct_scanout: bool,
    pub tears_requested: bool,
    pub overlay_planes: usize,
    pub cursor_plane_in_use: bool,
    pub pointer_hidden: bool,
    pub pointer_locked: bool,
    pub driver_in_cooldown: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TearingRefusal {
    NotEligible,
    DriverCooldown,
    OverlayPlanesInUse(usize),
    CursorPlaneInUse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TearingPolicyDecision {
    Allow,
    Refuse(TearingRefusal),
}

pub(crate) fn evaluate_tearing_policy(input: TearingPolicyInput) -> TearingPolicyDecision {
    if !input.direct_scanout || !input.tears_requested {
        return TearingPolicyDecision::Refuse(TearingRefusal::NotEligible);
    }
    if input.driver_in_cooldown {
        return TearingPolicyDecision::Refuse(TearingRefusal::DriverCooldown);
    }
    if input.overlay_planes > 0 {
        return TearingPolicyDecision::Refuse(TearingRefusal::OverlayPlanesInUse(
            input.overlay_planes,
        ));
    }
    if input.cursor_plane_in_use && !(input.pointer_hidden || input.pointer_locked) {
        return TearingPolicyDecision::Refuse(TearingRefusal::CursorPlaneInUse);
    }
    TearingPolicyDecision::Allow
}

fn record_tearing_refusal_once<G: PartialEq>(
    last_game: &mut Option<G>,
    logged: &mut Vec<String>,
    game: Option<G>,
    detail: String,
) -> bool {
    if let Some(w) = game
        && last_game.as_ref() != Some(&w)
    {
        *last_game = Some(w);
        logged.clear();
    }
    if logged.contains(&detail) {
        return false;
    }
    logged.push(detail);
    true
}

/// One frame on every screen that is not waiting for its vblank. A screen whose frame changed
/// nothing completes its frame callbacks now; a queued one at its vblank.
pub(super) fn render(data: &mut LoopData, turn: &mut TurnClock) {
    let locked = data.state.is_locked();
    let clear = if locked {
        convert::LOCKED_CLEAR
    } else {
        data.state.clear_color()
    };
    for i in 0..data.screens.len() {
        let id = data.screens[i].id;
        let size = data.screens[i].size;

        // Screencopy frame capture if requested (Discord, OBS, WebRTC, RustDesk).
        // Screencopy renders to an offscreen buffer and does not depend on DRM page flip / vblank.
        // Handling this first ensures pending screencopy requests are consumed immediately,
        // preventing the event loop from busy-spinning while DRM page flip is pending.
        serve_captures(
            &mut data.state,
            &mut data.renderer,
            &mut data.screens[i].screencopy_texture,
            &mut data.capture_copies,
            &mut data.capture_readback_deferred,
            &mut data.capture_log,
            turn,
            data.zoom_programs.as_ref(),
            &mut data.zoom_cache,
            data.scaling_filter,
            id,
            size,
            clear,
        );

        if data.screens[i].pending {
            continue;
        }

        if data.dpms_active || data.screens[i].power_off {
            // While DPMS or power-off is active, do not render or queue frames to the screen.
            // Blanked screens count as showing no window.
            data.state.set_fitted_window(id, None);
            let s = &mut data.screens[i];
            s.shows_locked = true;
            s.latency.drop_pending();
            // a frame held back for its start time is not waited for either
            s.deferred = None;
            let now = infynitae_wayland::presentation_clock_now();
            data.state.frame_tick(id, now);
            continue;
        }

        // just in time for the next refresh (LATENCY): held back until its start time, unless the
        // window on it tears, a fullscreen game runs under VRR, or a composited frame in Direct
        // mode hits a safety guard (GPU relief, a game on another screen, or a slow previous turn)
        let started = infynitae_wayland::presentation_clock_now();
        let glide_at;
        let relief_active = data.state.gpu_relief && data.screens.iter().any(|sc| sc.relief_on);
        let game_on_other = data
            .screens
            .iter()
            .any(|other| other.id != id && data.state.exclusive_window(other.id).is_some());
        let skip_composited_late = skip_composited_late_start(
            data.state.latency_mode,
            data.screens[i].scanned_out,
            relief_active,
            game_on_other,
            data.last_turn_slow,
        );
        {
            let s = &mut data.screens[i];
            let m = s.compositor.pending_mode();
            let mhz = refresh_mhz(m.clock(), m.hsync().2, m.vsync().2, m.vrefresh()).max(1);
            s.schedule.set_period(std::time::Duration::from_nanos(
                1_000_000_000_000 / u64::from(mhz),
            ));
            s.schedule.set_mode(data.state.latency_mode);
            s.schedule.set_scanned_out(s.scanned_out);
            s.schedule.set_skip_late(skip_composited_late);
            // a frame the GPU has to draw needs more of the refresh than one it only shows
            s.schedule.set_guard_percent(if s.scanned_out {
                data.state.frame_guard_percent / 4
            } else {
                data.state.frame_guard_percent
            });
            let cursor_off_plane =
                data.state.cursor_is_hidden_on(id) || data.state.pointer_locked_on(id);
            let may_tear =
                data.state.tears(id) && cursor_off_plane && s.tearing_refused < TEARING_REFUSALS;
            let immediate = may_tear || (s.vrr_on && data.state.exclusive_window(id).is_some());
            // a glide is drawn where it is when the frame is shown; a frame that tears or runs
            // under VRR is shown as soon as it is done
            glide_at = if immediate {
                started
            } else {
                s.schedule.shown_at(started)
            };
            match s
                .schedule
                .start_for(started, s.deferred)
                .filter(|_| !immediate)
            {
                Some(at) => {
                    s.deferred = Some(at);
                    continue;
                }
                None => s.deferred = None,
            }
        }

        turn.mark("frame schedule");
        // commits timed up to the refresh this frame reaches go into it
        data.state.apply_timed_commits_for(id, glide_at);
        data.state.zoom_tick(id, glide_at);
        if locked {
            data.state.set_fitted_window(id, None);
        }
        // locked: only the lock surface (lock.rs); never a window
        let frame = match convert::lock_elements(&data.state, &mut data.renderer, id) {
            Ok(Some(f)) => {
                turn.mark("lock elements");
                Ok(f)
            }
            Ok(None) => {
                let enabled = turn.is_enabled();
                let mut record = |name: &'static str| turn.mark(name);
                let mark = enabled.then_some(&mut record as &mut dyn FnMut(&'static str));
                convert::output_elements_with_marks(&mut data.state, &mut data.renderer, id, mark)
                    .map_err(|e| format!("{e:?}"))
            }
            Err(e) => Err(format!("{e:?}")),
        };
        let (elements, k) = match frame {
            Ok(r) => r,
            Err(e) => {
                log_line!("[infynitae] Frame elements: {e}");
                continue;
            }
        };
        let elements = smooth::prepare_elements(
            &mut data.renderer,
            &data.state.rounding,
            data.zoom_programs.as_ref(),
            &mut data.zoom_cache,
            elements,
            k,
            data.screens[i].size,
            data.scaling_filter,
        );
        turn.mark("smooth wrap");
        // VRR auto: on while this screen shows a fullscreen window
        let want_vrr = data.screens[i]
            .vrr_mode
            .unwrap_or_else(|| data.state.exclusive_window(id).is_some());
        let s = &mut data.screens[i];
        if s.vrr_supported && s.vrr_on != want_vrr && s.compositor.use_vrr(want_vrr).is_ok() {
            s.vrr_on = want_vrr;
        }
        if s.k != k {
            s.compositor
                .set_output_mode_source(OutputModeSource::Static {
                    size: s.size.into(),
                    scale: smithay::utils::Scale::from(k),
                    transform: s.transform,
                });
            s.k = k;
        }
        // Direct scanout: a fullscreen client's buffer on the primary plane (games). Overlay
        // planes only on request: amdgpu refused page flips (EINVAL, hundreds per Electron
        // start) for viewport-scaled subsurfaces there, although Smithay's test commit passed.
        // The cursor goes on the cursor plane (HARDWARE_CURSOR): it moves without a redraw, and
        // a fullscreen game under a cursor can still be scanned out. Smithay test-commits it and
        // draws the cursor itself when the plane is refused.
        let scanout_banned = s
            .no_scanout_until
            .is_some_and(|t| std::time::Instant::now() < t);
        let mut frame_flags = match (
            data.direct_scanout && !scanout_banned,
            data.state.overlay_planes,
        ) {
            (false, _) => FrameFlags::empty(),
            // any format: a game's 10-bit buffer is scanned out too (test-committed first)
            (true, false) => {
                FrameFlags::ALLOW_PRIMARY_PLANE_SCANOUT
                    | FrameFlags::ALLOW_PRIMARY_PLANE_SCANOUT_ANY
            }
            (true, true) => {
                FrameFlags::ALLOW_PRIMARY_PLANE_SCANOUT
                    | FrameFlags::ALLOW_PRIMARY_PLANE_SCANOUT_ANY
                    | FrameFlags::ALLOW_OVERLAY_PLANE_SCANOUT
            }
        };
        if data.state.hardware_cursor {
            frame_flags |= FrameFlags::ALLOW_CURSOR_PLANE_SCANOUT;
        }
        // behind a fullscreen window the screen is black: Smithay scans out a window that does
        // not say it is opaque (most games and players) only over a black background
        let core = data.state.core();
        let presenting = !locked
            && (data.state.fitted_window(id).is_some()
                || (core.presentation(id).is_some() && !core.is_presentation_suppressed(id)));
        let clear = if presenting {
            smithay::backend::renderer::Color32F::BLACK
        } else {
            clear
        };
        scanout_log::watch(presenting);
        turn.mark("scanout decision");
        let render_result =
            s.compositor
                .render_frame(&mut data.renderer, &elements, clear, frame_flags);
        // a slow one says whether the GPU drew it: most of the 63 turns of 50 to 437 ms on
        // 2026-11-22 came in the same stretches as slow capture draws or GPU relief
        turn.mark(match &render_result {
            Ok(r) if r.is_empty => "render_frame, nothing new",
            Ok(r)
                if matches!(
                    r.primary_element,
                    smithay::backend::drm::compositor::PrimaryPlaneElement::Element(_)
                ) =>
            {
                "render_frame, scanned out"
            }
            Ok(_) => "render_frame, composited",
            Err(_) => "render_frame, failed",
        });
        let queued = match render_result {
            Ok(result) if !result.is_empty => {
                use smithay::backend::drm::compositor::PrimaryPlaneElement;
                let scanout = matches!(result.primary_element, PrimaryPlaneElement::Element(_));
                s.scanned_out = scanout;
                s.schedule.set_scanned_out(scanout);
                data.state.fullscreen_frame_shown(id);
                // a change of (fullscreen shown, scanned out) is logged; composited while
                // fullscreen says what else is drawn over or around the window
                if s.scanout_logged != Some((presenting, scanout)) {
                    let was = s.scanout_logged.replace((presenting, scanout));
                    match (presenting, scanout) {
                        (true, true) => log_line!(
                            "[infynitae] Fullscreen on screen {id:?}: its frames go to the screen as they are (direct scanout)"
                        ),
                        (true, false) => {
                            let plane_formats = &s.compositor.surface().plane_info().formats;
                            let buffer = core_presentation_format(&data.state, id);
                            let buffer_note = match buffer {
                                Some(f) if plane_formats.contains(&f) => {
                                    format!("; its buffer {f:?} is one the screen takes")
                                }
                                Some(f) => {
                                    format!("; its buffer {f:?} is NOT one the screen takes")
                                }
                                None => "; its buffer is not a GPU buffer".to_string(),
                            };
                            let geometries: Vec<_> = elements
                                .iter()
                                .map(|e| {
                                    smithay::backend::renderer::element::Element::geometry(
                                        e,
                                        smithay::utils::Scale::from(k),
                                    )
                                })
                                .collect();
                            // what Smithay checks without saying: a GPU buffer to show at all
                            let storage: Vec<_> = elements
                                .iter()
                                .map(|e| {
                                    smithay::backend::renderer::element::RenderElement::underlying_storage(
                                        e,
                                        &mut data.renderer,
                                    )
                                    .is_some()
                                })
                                .collect();
                            log_line!(
                                "[infynitae] Fullscreen on screen {id:?}: composited, not scanned out ({} elements{}): {geometries:?}{buffer_note}; buffers offered {storage:?}",
                                elements.len(),
                                if data.direct_scanout && !scanout_banned {
                                    ""
                                } else {
                                    ", direct scanout off"
                                }
                            );
                        }
                        (false, _) if was.is_some_and(|(p, _)| p) => {
                            log_line!("[infynitae] Fullscreen off on screen {id:?}")
                        }
                        (false, _) => {}
                    }
                }
                let (overlays, cursor) = (
                    result.overlay_elements.len(),
                    result.cursor_element.is_some(),
                );
                // Check for a kernel async refusal emitted asynchronously during frame_submitted().
                if let Some(detail) = s.compositor.take_async_flip_refusal() {
                    s.tearing_refused = s.tearing_refused.saturating_add(1);
                    let game = data.state.exclusive_window(id);
                    if record_tearing_refusal_once(
                        &mut s.last_tearing_game,
                        &mut s.tearing_refusals_logged,
                        game,
                        detail.clone(),
                    ) {
                        log_line!("[infynitae] Tearing refused on screen {id:?}: {detail}");
                    }
                    if s.tearing_refused == TEARING_REFUSALS {
                        s.tearing_retry_at = Some(std::time::Instant::now() + TEARING_RETRY);
                        log_line!(
                            "[infynitae] Tearing refused {TEARING_REFUSALS} times in a row on screen {id:?}: frames wait for the refresh, trying again in {} s",
                            TEARING_RETRY.as_secs()
                        );
                    }
                }
                // tearing: a scanned-out frame of a window that tears flips at once (async)
                // when overlays are unused and no visible unlocked cursor sits on the cursor plane.
                let tears_now = data.state.tears(id);
                let pointer_hidden = data.state.cursor_is_hidden_on(id);
                let pointer_locked = data.state.pointer_locked_on(id);
                let game = data.state.exclusive_window(id);
                if let Some(w) = game
                    && Some(w) != s.last_tearing_game
                {
                    s.last_tearing_game = Some(w);
                    s.tearing_refusals_logged.clear();
                }
                // a composited frame says nothing of the driver; only a window that stops
                // tearing, or the wait's end, starts the count again
                if !tears_now
                    || s.tearing_retry_at
                        .is_some_and(|t| std::time::Instant::now() >= t)
                {
                    s.tearing_refused = 0;
                    s.tearing_retry_at = None;
                }
                let decision = evaluate_tearing_policy(TearingPolicyInput {
                    direct_scanout: scanout,
                    tears_requested: tears_now,
                    overlay_planes: overlays,
                    cursor_plane_in_use: cursor,
                    pointer_hidden,
                    pointer_locked,
                    driver_in_cooldown: s.tearing_refused >= TEARING_REFUSALS,
                });
                let tear = decision == TearingPolicyDecision::Allow;
                if let TearingPolicyDecision::Refuse(refusal) = decision {
                    let detail = match refusal {
                        TearingRefusal::OverlayPlanesInUse(n) => {
                            Some(format!("{n} overlay plane(s) in use (keeping vblank)"))
                        }
                        TearingRefusal::CursorPlaneInUse => Some(
                            "cursor plane in use (visible unlocked pointer; keeping vblank)"
                                .to_string(),
                        ),
                        TearingRefusal::NotEligible | TearingRefusal::DriverCooldown => None,
                    };
                    if let Some(detail) = detail
                        && record_tearing_refusal_once(
                            &mut s.last_tearing_game,
                            &mut s.tearing_refusals_logged,
                            game,
                            detail.clone(),
                        )
                    {
                        log_line!("[infynitae] Tearing refused on screen {id:?}: {detail}");
                    }
                }
                let tearing_state = tear;
                if tearing_state != s.tearing_logged {
                    s.tearing_logged = tearing_state;
                    log_line!(
                        "[infynitae] Tearing {} on screen {id:?}",
                        if tearing_state { "on" } else { "off" }
                    );
                }
                if tear != s.compositor.async_flip() {
                    s.compositor.set_async_flip(tear);
                }
                let queued = s.compositor.queue_frame(());
                if queued.is_ok() {
                    let submitted = infynitae_wayland::presentation_clock_now();
                    s.schedule.rendered(started, submitted);
                    s.latency.drawn(submitted);
                }
                if tear {
                    if let Some(detail) = s.compositor.take_async_flip_refusal() {
                        s.tearing_refused = s.tearing_refused.saturating_add(1);
                        if record_tearing_refusal_once(
                            &mut s.last_tearing_game,
                            &mut s.tearing_refusals_logged,
                            game,
                            detail.clone(),
                        ) {
                            log_line!("[infynitae] Tearing refused on screen {id:?}: {detail}");
                        }
                        if s.tearing_refused == TEARING_REFUSALS {
                            s.tearing_retry_at = Some(std::time::Instant::now() + TEARING_RETRY);
                            log_line!(
                                "[infynitae] Tearing refused {TEARING_REFUSALS} times in a row on screen {id:?}: frames wait for the refresh, trying again in {} s",
                                TEARING_RETRY.as_secs()
                            );
                        }
                    } else if s.compositor.async_flip() {
                        s.tearing_refused = 0;
                    }
                }
                match queued {
                    Ok(()) => {
                        s.flip_failures = 0;
                        s.queued_locked = locked;
                        true
                    }
                    Err(e) => {
                        s.flip_failures += 1;
                        if s.flip_failures % 100 == 1 {
                            log_line!(
                                "[infynitae] Page flip refused ({} in a row): primary={} overlays={overlays} cursor={cursor} elements={}: {e:?}",
                                s.flip_failures,
                                if scanout {
                                    "client buffer"
                                } else {
                                    "composited"
                                },
                                elements.len()
                            );
                            // which element the kernel refused, if the refusal names a
                            // rectangle (`journalctl -k` with drm.debug)
                            let geometries: Vec<_> = elements
                                .iter()
                                .map(|e| {
                                    smithay::backend::renderer::element::Element::geometry(
                                        e,
                                        smithay::utils::Scale::from(k),
                                    )
                                })
                                .collect();
                            log_line!("[infynitae] Refused frame's elements: {geometries:?}");
                        }
                        if scanout {
                            // the client's buffer may be what the kernel refuses: compose for a while
                            s.no_scanout_until =
                                Some(std::time::Instant::now() + Duration::from_secs(5));
                        }
                        if s.flip_failures.is_multiple_of(3) {
                            let _ = s.compositor.reset_state();
                        }
                        // retry after a refresh period, not at once (a busy loop that stalled
                        // the whole machine while Discord's updater drew)
                        let crtc = s.crtc;
                        let period = data
                            .state
                            .core()
                            .output(id)
                            .map(|m| m.refresh_mhz)
                            .filter(|r| *r > 0)
                            .map_or(Duration::from_millis(16), |r| {
                                Duration::from_nanos(1_000_000_000_000 / u64::from(r))
                            });
                        s.pending = true;
                        let _ = data.loop_handle.insert_source(
                            smithay::reexports::calloop::timer::Timer::from_duration(period),
                            move |_, _, data| {
                                if let Some(s) = data.screens.iter_mut().find(|s| s.crtc == crtc) {
                                    s.pending = false;
                                }
                                data.session.request_frame();
                                smithay::reexports::calloop::timer::TimeoutAction::Drop
                            },
                        );
                        false
                    }
                }
            }
            Ok(_) => {
                // nothing changed: the screen already shows these elements
                if locked {
                    s.shows_locked = true;
                }
                false
            }
            Err(e) => {
                log_line!("[infynitae] Render: {e:?}");
                false
            }
        };
        let s = &mut data.screens[i];
        if s.flip_failures > 0 && !queued {
            // waiting for the retry timer (above); its frame callbacks complete as usual
            let now = infynitae_wayland::presentation_clock_now();
            data.state.frame_tick(id, now);
            turn.mark("queue");
            continue;
        }
        s.pending = queued;
        if queued {
            data.session.count_frame();
        } else {
            let now = infynitae_wayland::presentation_clock_now();
            // nothing to show, so no vblank will come. The first such frame after a refresh rests
            // for nothing: the app is still drawing, and what it sends goes to the very next
            // refresh. A second means a client is sending nothing new: it waits, or would spin the loop.
            if s.empty_since_refresh {
                let wait = s.schedule.until_next_refresh(now);
                data.session.frame_rests(wait);
            }
            s.empty_since_refresh = true;
            // keep the input across empty renders up to 100 ms while an app commits its frame
            s.latency.expire_pending(now);
            data.state.frame_tick(id, now);
        }
        turn.mark("queue");
    }
    if let Some(v) = data.virtual_screen.as_mut() {
        serve_captures(
            &mut data.state,
            &mut data.renderer,
            &mut v.screencopy_texture,
            &mut data.capture_copies,
            &mut data.capture_readback_deferred,
            &mut data.capture_log,
            turn,
            data.zoom_programs.as_ref(),
            &mut data.zoom_cache,
            data.scaling_filter,
            v.id,
            VIRTUAL_SCREEN_SIZE,
            clear,
        );
        let now = infynitae_wayland::presentation_clock_now();
        data.state.frame_tick(v.id, now);
        turn.mark("virtual screen");
    }
    data.zoom_cache.gc();
    confirm_lock(data);
    // Watch newly started GPU readbacks so `deliver_copies` runs once the GPU finishes them.
    for c in &mut data.capture_copies {
        if !c.watched {
            c.watched = true;
            watch_copy_sync(&data.loop_handle, &c.sync, &mut data.session);
        }
    }
    // a zoom glide draws on every refresh until it rests
    if data.state.core().zoom_gliding() {
        data.session.request_frame();
    }
    turn.mark("post render");
    data.session.flush();
    turn.mark("flush");
    if renderer_lost(&mut data.renderer) {
        recover_renderer(data);
        turn.mark("renderer recover");
    }
}

/// Locked, and no screen shows a window any more: the lock client is told it holds the lock.
/// A frame counts once its vblank has come (a queued or refused one is not on screen).
pub(super) fn confirm_lock(data: &mut LoopData) {
    if data.state.is_locked() && data.screens.iter().all(|s| s.shows_locked) {
        data.state.lock_frame_drawn();
    }
}

/// Clients' frames still being drawn (`WaylandState::take_frame_waits`): each is applied when the
/// GPU has finished it. Until then the window shows its previous frame and the screen keeps its
/// refresh, instead of a redraw waiting for the GPU and going out a refresh late.
pub(super) fn watch_frame_waits(data: &mut LoopData) {
    use smithay::reexports::calloop::Interest;
    for wait in data.state.take_frame_waits() {
        let (done, ready, client) = wait.into_parts();
        let (flag, who) = (ready.clone(), client.clone());
        let inserted = match done {
            FrameDone::Buffer(dmabuf) => match dmabuf.generate_blocker(Interest::READ) {
                Ok((_, source)) => data
                    .loop_handle
                    .insert_source(source, move |_, _, data| {
                        flag.store(true, std::sync::atomic::Ordering::Release);
                        data.state.frame_ready(&who);
                        Ok(())
                    })
                    .is_ok(),
                // finished already
                Err(_) => false,
            },
            FrameDone::Acquire(point) => match point.generate_blocker() {
                Ok((_, source)) => data
                    .loop_handle
                    .insert_source(source, move |_, _, data| {
                        flag.store(true, std::sync::atomic::Ordering::Release);
                        data.state.frame_ready(&who);
                        Ok(())
                    })
                    .is_ok(),
                Err(e) => {
                    // the frame is shown as it is rather than holding the window forever
                    log_line!("[infynitae] Explicit sync: cannot wait for a frame: {e}");
                    false
                }
            },
        };
        if !inserted {
            ready.store(true, std::sync::atomic::Ordering::Release);
            data.state.frame_ready(&client);
        }
    }
}

/// The format of the buffer the fullscreen window on `id` shows (a diagnosis when it is composited).
fn core_presentation_format(
    state: &WaylandState,
    id: OutputId,
) -> Option<smithay::backend::allocator::Format> {
    state.window_buffer_format(state.exclusive_window(id)?)
}

/// Skips late scheduling on a composited desktop frame under `Direct` while the GPU is under
/// relief, a fullscreen window owns another screen, or the previous loop turn ran over one refresh.
fn skip_composited_late_start(
    mode: infynitae_wayland::frame_schedule::LatencyMode,
    scanned_out: bool,
    relief_active: bool,
    game_on_other_screen: bool,
    last_turn_slow: bool,
) -> bool {
    mode == infynitae_wayland::frame_schedule::LatencyMode::Direct
        && !scanned_out
        && (relief_active || game_on_other_screen || last_turn_slow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composited_late_start_respects_mode_scanout_and_safety_guards() {
        use infynitae_wayland::frame_schedule::LatencyMode;
        assert!(!super::skip_composited_late_start(
            LatencyMode::Direct,
            false,
            false,
            false,
            false,
        ));
        assert!(super::skip_composited_late_start(
            LatencyMode::Direct,
            false,
            true,
            false,
            false,
        ));
        assert!(super::skip_composited_late_start(
            LatencyMode::Direct,
            false,
            false,
            true,
            false,
        ));
        assert!(super::skip_composited_late_start(
            LatencyMode::Direct,
            false,
            false,
            false,
            true,
        ));
        // Direct scanout (fullscreen game) and explicit Balanced mode are unaffected.
        assert!(!super::skip_composited_late_start(
            LatencyMode::Direct,
            true,
            true,
            true,
            true,
        ));
        assert!(!super::skip_composited_late_start(
            LatencyMode::Balanced,
            false,
            true,
            true,
            true,
        ));
    }

    #[test]
    fn scaling_filter_change_resets_buffers_and_forces_full_redraw() {
        use infynitae_wayland::ZoomFilter;
        use smithay::backend::allocator::dmabuf::AsDmabuf;
        use smithay::backend::allocator::gbm::{GbmAllocator, GbmBufferFlags, GbmDevice};
        use smithay::backend::allocator::{Fourcc, Modifier, Swapchain};
        use smithay::backend::renderer::Bind;
        use smithay::backend::renderer::damage::OutputDamageTracker;
        use smithay::backend::renderer::element::solid::SolidColorRenderElement;
        use smithay::backend::renderer::element::{Id, Kind};
        use smithay::utils::{DeviceFd, Transform};

        let Some(gbm) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/dri/renderD128")
            .ok()
            .and_then(|f| {
                GbmDevice::new(DrmDeviceFd::new(DeviceFd::from(
                    std::os::fd::OwnedFd::from(f),
                )))
                .ok()
            })
        else {
            eprintln!("no GPU render node: skipped");
            return;
        };
        let Ok(mut renderer) = gles_renderer(&gbm) else {
            eprintln!("no GLES renderer: skipped");
            return;
        };

        let mut current = ZoomFilter::Sharp;
        let mut zoom_cache = smooth::ZoomCache::default();
        let damage = Mutex::new(infynitae_wayland::screencopy::CaptureDamage::default());
        let elem = SolidColorRenderElement::new(
            Id::new(),
            Rectangle::from_size((16, 16).into()),
            0,
            [0.3, 0.5, 0.7, 1.0],
            Kind::Unspecified,
        );
        let elements = [elem];
        let now = std::time::Instant::now();
        assert!(
            damage
                .lock()
                .unwrap()
                .damage(&"c", (16, 16), 1.0, Transform::Normal, &elements, now)
                .is_some()
        );
        assert!(
            damage
                .lock()
                .unwrap()
                .damage(&"c", (16, 16), 1.0, Transform::Normal, &elements, now)
                .is_none()
        );

        // Unchanged filter is a no-op and preserves screencopy damage state.
        assert!(!apply_scaling_filter(
            &mut current,
            ZoomFilter::Sharp,
            &mut renderer,
            &mut zoom_cache,
            &damage,
        ));
        assert!(
            damage
                .lock()
                .unwrap()
                .damage(&"c", (16, 16), 1.0, Transform::Normal, &elements, now)
                .is_none()
        );

        // Changed filter updates state and clears screencopy damage so captures redraw too.
        assert!(apply_scaling_filter(
            &mut current,
            ZoomFilter::Smooth,
            &mut renderer,
            &mut zoom_cache,
            &damage,
        ));
        assert_eq!(current, ZoomFilter::Smooth);
        assert!(
            damage
                .lock()
                .unwrap()
                .damage(&"c", (16, 16), 1.0, Transform::Normal, &elements, now)
                .is_some()
        );

        // Resetting swapchain buffers forces age=0 and a full redraw on identical elements.
        let allocator = GbmAllocator::new(gbm, GbmBufferFlags::RENDERING);
        let mut swapchain = Swapchain::new(
            allocator,
            16,
            16,
            Fourcc::Argb8888,
            vec![Modifier::Linear, Modifier::Invalid],
        );
        let mut tracker = OutputDamageTracker::new((16, 16), 1.0, Transform::Normal);

        // Seed 4 slots so the next acquire returns a buffer with age > 0 and zero damage.
        for _ in 0..4 {
            let slot = swapchain.acquire().expect("acquire").expect("slot");
            let age = usize::from(slot.age());
            let mut dmabuf = slot.export().expect("dmabuf");
            let mut fb = renderer.bind(&mut dmabuf).expect("bind");
            let _ = tracker
                .render_output(&mut renderer, &mut fb, age, &elements, [0.0, 0.0, 0.0, 1.0])
                .expect("render");
            swapchain.submitted(&slot);
        }
        let steady = swapchain.acquire().expect("acquire").expect("slot");
        assert!(steady.age() > 0);
        let mut dmabuf = steady.export().expect("dmabuf");
        let mut fb = renderer.bind(&mut dmabuf).expect("bind");
        let steady_res = tracker
            .render_output(
                &mut renderer,
                &mut fb,
                usize::from(steady.age()),
                &elements,
                [0.0, 0.0, 0.0, 1.0],
            )
            .expect("render");
        assert!(steady_res.damage.is_none(), "steady frame has no damage");
        drop(fb);
        swapchain.submitted(&steady);
        drop(steady);

        swapchain.reset_buffers();
        let redrawn = swapchain.acquire().expect("acquire").expect("slot");
        assert_eq!(redrawn.age(), 0);
        let mut dmabuf = redrawn.export().expect("dmabuf");
        let mut fb = renderer.bind(&mut dmabuf).expect("bind");
        let redrawn_res = tracker
            .render_output(
                &mut renderer,
                &mut fb,
                usize::from(redrawn.age()),
                &elements,
                [0.0, 0.0, 0.0, 1.0],
            )
            .expect("render");
        assert!(
            redrawn_res.damage.is_some(),
            "reset_buffers forces a full redraw even when elements are unchanged"
        );
    }

    #[test]
    fn tearing_policy_allows_hidden_and_locked_pointer_but_refuses_visible_pointer_and_overlays() {
        use super::{
            TearingPolicyDecision, TearingPolicyInput, TearingRefusal, evaluate_tearing_policy,
        };

        let base = TearingPolicyInput {
            direct_scanout: true,
            tears_requested: true,
            overlay_planes: 0,
            cursor_plane_in_use: false,
            pointer_hidden: false,
            pointer_locked: false,
            driver_in_cooldown: false,
        };

        assert_eq!(
            evaluate_tearing_policy(TearingPolicyInput {
                pointer_hidden: true,
                ..base
            }),
            TearingPolicyDecision::Allow
        );

        assert_eq!(
            evaluate_tearing_policy(TearingPolicyInput {
                pointer_locked: true,
                cursor_plane_in_use: true,
                ..base
            }),
            TearingPolicyDecision::Allow,
            "locked pointer allows tearing"
        );

        assert_eq!(
            evaluate_tearing_policy(TearingPolicyInput {
                cursor_plane_in_use: true,
                ..base
            }),
            TearingPolicyDecision::Refuse(TearingRefusal::CursorPlaneInUse),
            "visible unlocked pointer on cursor plane stays on vblank"
        );

        assert_eq!(
            evaluate_tearing_policy(TearingPolicyInput {
                overlay_planes: 1,
                pointer_hidden: true,
                ..base
            }),
            TearingPolicyDecision::Refuse(TearingRefusal::OverlayPlanesInUse(1)),
            "overlay plane in use stays on vblank"
        );

        assert_eq!(
            evaluate_tearing_policy(TearingPolicyInput {
                driver_in_cooldown: true,
                pointer_hidden: true,
                ..base
            }),
            TearingPolicyDecision::Refuse(TearingRefusal::DriverCooldown)
        );
    }

    #[test]
    fn record_tearing_refusal_once_logs_once_per_game_and_resets_on_new_game() {
        use super::record_tearing_refusal_once;

        let mut last_game = None;
        let mut logged = Vec::new();
        let game1 = Some(1u64);
        let game2 = Some(2u64);

        assert!(record_tearing_refusal_once(
            &mut last_game,
            &mut logged,
            game1,
            "cursor plane in use".to_string()
        ));
        assert!(!record_tearing_refusal_once(
            &mut last_game,
            &mut logged,
            game1,
            "cursor plane in use".to_string()
        ));
        assert!(record_tearing_refusal_once(
            &mut last_game,
            &mut logged,
            game1,
            "1 overlay plane(s) in use".to_string()
        ));
        assert!(record_tearing_refusal_once(
            &mut last_game,
            &mut logged,
            game2,
            "cursor plane in use".to_string()
        ));
    }

    #[test]
    fn async_flip_commit_builder_emits_only_primary_fb_id_and_rejects_other_planes_or_props() {
        use smithay::backend::drm::{
            AsyncFlipCommit, AsyncFlipCommitError, AsyncFlipPlaneProps, AsyncFlipPropValue,
            PlaneConfig, PlaneState, build_async_flip_commit,
        };
        use smithay::reexports::drm::control::{framebuffer, plane, property};
        use smithay::utils::{Buffer, Physical, Rectangle, Transform};
        use std::num::NonZeroU32;

        let primary = plane::Handle::from(NonZeroU32::new(10).unwrap());
        let cursor = plane::Handle::from(NonZeroU32::new(11).unwrap());
        let overlay = plane::Handle::from(NonZeroU32::new(12).unwrap());
        let fb = framebuffer::Handle::from(NonZeroU32::new(42).unwrap());

        let base_cfg = PlaneConfig {
            src: Rectangle::<f64, Buffer>::from_size((1920.0, 1080.0).into()),
            dst: Rectangle::<i32, Physical>::from_size((1920, 1080).into()),
            transform: Transform::Normal,
            alpha: 1.0,
            damage_clips: Some(property::Value::Blob(999)),
            fb,
            fence: None,
        };
        let prev = AsyncFlipPlaneProps::from_config(&base_cfg);

        let plan = build_async_flip_commit(
            primary,
            Some(&prev),
            &[PlaneState {
                handle: primary,
                config: Some(base_cfg.clone()),
            }],
        )
        .expect("primary-only compatible flip succeeds");
        assert_eq!(
            plan,
            AsyncFlipCommit {
                plane: primary,
                fb,
                properties: vec![("FB_ID", AsyncFlipPropValue::Framebuffer(fb))],
            }
        );

        assert_eq!(
            build_async_flip_commit(
                primary,
                Some(&prev),
                &[
                    PlaneState {
                        handle: primary,
                        config: Some(base_cfg.clone()),
                    },
                    PlaneState {
                        handle: cursor,
                        config: None,
                    },
                ],
            ),
            Err(AsyncFlipCommitError::NonPrimaryPlane { plane: cursor })
        );

        assert_eq!(
            build_async_flip_commit(
                primary,
                Some(&prev),
                &[
                    PlaneState {
                        handle: primary,
                        config: Some(base_cfg.clone()),
                    },
                    PlaneState {
                        handle: overlay,
                        config: Some(base_cfg.clone()),
                    },
                ],
            ),
            Err(AsyncFlipCommitError::NonPrimaryPlane { plane: overlay })
        );

        let mut moved = base_cfg.clone();
        moved.dst.loc.x = 1;
        assert_eq!(
            build_async_flip_commit(
                primary,
                Some(&prev),
                &[PlaneState {
                    handle: primary,
                    config: Some(moved),
                }],
            ),
            Err(AsyncFlipCommitError::PropertyChanged {
                plane: primary,
                property: "CRTC_RECT",
            })
        );

        let mut rotated = base_cfg.clone();
        rotated.transform = Transform::_90;
        assert_eq!(
            build_async_flip_commit(
                primary,
                Some(&prev),
                &[PlaneState {
                    handle: primary,
                    config: Some(rotated),
                }],
            ),
            Err(AsyncFlipCommitError::PropertyChanged {
                plane: primary,
                property: "rotation",
            })
        );

        let pipe = std::os::unix::net::UnixStream::pair().expect("pipe");
        use std::os::fd::AsFd;
        let mut fenced = base_cfg.clone();
        fenced.fence = Some(pipe.0.as_fd());
        assert_eq!(
            build_async_flip_commit(
                primary,
                Some(&prev),
                &[PlaneState {
                    handle: primary,
                    config: Some(fenced),
                }],
            ),
            Err(AsyncFlipCommitError::PropertyChanged {
                plane: primary,
                property: "IN_FENCE_FD",
            })
        );

        assert_eq!(
            build_async_flip_commit(primary, Some(&prev), &[]),
            Err(AsyncFlipCommitError::MissingPrimaryPlane { primary })
        );
    }
}
