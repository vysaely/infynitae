//! Screen captures (screencopy): serving them, the GPU readback and handing finished copies to clients.

use super::*;

/// Serves the captures waiting for `output_id`: the screen's (its frame, as shown), and the
/// windows captured alone (each drawn by itself, `convert::window_capture_elements`).
#[allow(clippy::too_many_arguments)]
pub(super) fn serve_captures(
    state: &mut WaylandState,
    renderer: &mut GlesRenderer,
    screencopy_texture: &mut Option<GlesTexture>,
    copies: &mut Vec<PendingCopy>,
    capture_readback_deferred: &mut Vec<OutputId>,
    capture_log: &mut CaptureSeen<(
        Option<smithay::reexports::wayland_server::backend::ClientId>,
        Option<infynitae_wayland::screencopy::Consumer>,
    )>,
    turn: &mut TurnClock,
    zoom_programs: Option<&std::sync::Arc<smooth::ZoomPrograms>>,
    zoom_cache: &mut smooth::ZoomCache,
    scaling_filter: infynitae_wayland::ZoomFilter,
    output_id: OutputId,
    size: (i32, i32),
    clear: smithay::backend::renderer::Color32F,
) {
    // the screen redraws: captures waiting for a change look again
    state.screencopy.wake_waiting(output_id);
    if !state.screencopy.has_pending(output_id) {
        return;
    }
    let (windows, pending): (Vec<_>, Vec<_>) = state
        .screencopy
        .take_pending(output_id)
        .into_iter()
        .partition(|p| p.window.is_some());
    let mut deferred_shm = Vec::new();
    for req in windows {
        let Some(window) = req.window else { continue };
        let is_dmabuf = smithay::wayland::dmabuf::get_dmabuf(&req.buffer).is_ok();
        if !is_dmabuf && has_pending_readback(copies, output_id) {
            deferred_shm.push(req);
            continue;
        }
        match convert::window_capture_elements(state, renderer, window) {
            Ok(Some((elements, k, size))) => {
                turn.mark("capture elements");
                // a window's own transparency is kept
                let mut texture = None;
                handle_screencopy(
                    renderer,
                    &mut texture,
                    copies,
                    capture_log,
                    turn,
                    output_id,
                    vec![req],
                    &elements,
                    size,
                    k,
                    smithay::backend::renderer::Color32F::TRANSPARENT,
                );
                // the window draws its next frame for the next capture, on screen or not
                let now = infynitae_wayland::presentation_clock_now();
                state.window_captured(window, output_id, now);
            }
            _ => {
                turn.mark("capture elements");
                req.fail();
            }
        }
    }
    let pending = if has_pending_readback(copies, output_id) {
        let (gpu, shm): (Vec<_>, Vec<_>) = pending
            .into_iter()
            .partition(|req| smithay::wayland::dmabuf::get_dmabuf(&req.buffer).is_ok());
        deferred_shm.extend(shm);
        gpu
    } else {
        pending
    };
    if !deferred_shm.is_empty() {
        if !capture_readback_deferred.contains(&output_id) {
            capture_readback_deferred.push(output_id);
        }
        state.screencopy.defer(deferred_shm);
    }
    if pending.is_empty() {
        return;
    }

    let needs_cursor = !state.screencopy.hide_cursor() && pending.iter().any(|p| p.overlay_cursor);
    // locked: a capture shows the lock screen, never a window
    let clear = if state.is_locked() {
        convert::LOCKED_CLEAR
    } else {
        clear
    };
    let frame = match convert::lock_elements(state, renderer, output_id) {
        Ok(Some(f)) => Ok(f),
        Ok(None) => convert::output_elements_screencopy(state, renderer, output_id, needs_cursor)
            .map_err(|e| format!("{e:?}")),
        Err(e) => Err(format!("{e:?}")),
    };
    let (elements, k) = match frame {
        Ok(res) => res,
        Err(e) => {
            turn.mark("capture elements");
            log_line!("[screencopy] Failed to get elements: {e}");
            for p in pending {
                p.fail();
            }
            return;
        }
    };
    let elements = smooth::prepare_elements(
        renderer,
        &state.rounding,
        zoom_programs,
        zoom_cache,
        elements,
        k,
        size,
        scaling_filter,
    );
    // a capture that may wait is served once its consumer's last capture differs, with what
    // changed as its damage (whole for a region or a turned screen); else it waits
    let now = std::time::Instant::now();
    let mut serve = Vec::new();
    let mut waiting = Vec::new();
    {
        let mut damage = state.screencopy.damage.lock().unwrap();
        for mut req in pending {
            if let Some(consumer) = req.consumer.clone() {
                let draw_transform = convert::output_transform(req.transform);
                let Some(rects) = damage.damage(&consumer, size, k, draw_transform, &elements, now)
                else {
                    waiting.push(req);
                    continue;
                };
                let exact = req.region.is_none()
                    && req.transform == infynitae_core::output::Transform::Normal;
                req.damage = exact.then_some(rects);
            }
            serve.push(req);
        }
    }
    let pending = serve;
    state.screencopy.defer(waiting);
    turn.mark("capture elements");
    if pending.is_empty() {
        return;
    }
    handle_screencopy(
        renderer,
        screencopy_texture,
        copies,
        capture_log,
        turn,
        output_id,
        pending,
        &elements,
        size,
        k,
        clear,
    );
}

const CAPTURE_SEEN_MAX: usize = 128;

/// Tracks active/recent `(client, buffer path)` pairs so each new capture logs once without
/// repeating on subsequent frames of a stream.
pub(super) struct CaptureSeen<K> {
    pub(super) seen: VecDeque<(K, bool)>,
}

impl<K> Default for CaptureSeen<K> {
    fn default() -> Self {
        Self {
            seen: VecDeque::new(),
        }
    }
}

impl<K: PartialEq> CaptureSeen<K> {
    pub(super) fn note(&mut self, key: K, gpu: bool) -> bool {
        if self.seen.iter().any(|(k, g)| *k == key && *g == gpu) {
            return false;
        }
        if self.seen.len() >= CAPTURE_SEEN_MAX {
            self.seen.pop_front();
        }
        self.seen.push_back((key, gpu));
        true
    }
}

fn format_capture_log(program: Option<&str>, path: Option<&str>, gpu: bool) -> String {
    let who = match (program, path) {
        (Some(prog), Some(p)) if p.starts_with(prog) => p.to_string(),
        (Some(prog), Some(p)) => format!("{prog} ({p})"),
        (Some(prog), None) => prog.to_string(),
        (None, Some(p)) => p.to_string(),
        (None, None) => "unknown client".to_string(),
    };
    let via = if gpu {
        "a GPU buffer (dmabuf)"
    } else {
        "memory (shm), read back from the GPU"
    };
    format!("[infynitae] Capture started by {who}: into {via}")
}

/// Logs once per capture client/session which program captures, its path (or sandbox id), and
/// whether it takes dmabuf or shm.
fn note_capture_start(
    capture_log: &mut CaptureSeen<(
        Option<smithay::reexports::wayland_server::backend::ClientId>,
        Option<infynitae_wayland::screencopy::Consumer>,
    )>,
    req: &infynitae_wayland::screencopy::PendingFrame,
    gpu: bool,
) {
    use smithay::reexports::wayland_server::Resource;
    let client = req.buffer.client();
    let client_id = client
        .as_ref()
        .map(smithay::reexports::wayland_server::Client::id);
    if !capture_log.note((client_id, req.consumer.clone()), gpu) {
        return;
    }
    let identity = client
        .as_ref()
        .and_then(|c| c.get_data::<infynitae_wayland::ClientState>())
        .and_then(|s| s.identity());
    let program = identity.map(|id| id.app_name());
    let path = identity.map(|id| id.describe());
    log_line!(
        "{}",
        format_capture_log(program.as_deref(), path.as_deref(), gpu)
    );
}

/// Renders `elements` (`size` px at `k`) into each waiting capture's buffer: a dmabuf on the GPU,
/// a shm buffer through a copy (U-008).
#[allow(unsafe_code, clippy::too_many_arguments)]
fn handle_screencopy<E>(
    renderer: &mut GlesRenderer,
    screencopy_texture: &mut Option<GlesTexture>,
    copies: &mut Vec<PendingCopy>,
    capture_log: &mut CaptureSeen<(
        Option<smithay::reexports::wayland_server::backend::ClientId>,
        Option<infynitae_wayland::screencopy::Consumer>,
    )>,
    turn: &mut TurnClock,
    output_id: OutputId,
    pending: Vec<infynitae_wayland::screencopy::PendingFrame>,
    elements: &[E],
    size: (i32, i32),
    k: f64,
    clear: smithay::backend::renderer::Color32F,
) where
    E: smithay::backend::renderer::element::RenderElement<GlesRenderer>,
{
    let now = infynitae_wayland::presentation_clock_now();
    // a rotated screen is captured in the panel's orientation (its buffer, `size`), drawn
    // through its rotation; `view` is what the screen shows. One screen per call: one transform.
    let transform = pending
        .first()
        .map_or(infynitae_core::output::Transform::Normal, |p| p.transform);
    let draw_transform = convert::output_transform(transform);
    let view = convert::capture_buffer_size(size, transform);

    // Direct DMA-BUF GPU render (PipeWire, OBS, gpu-screen-recorder) before the CPU/shm fallback.
    let mut remaining = Vec::new();
    let mut drew_dmabuf = false;

    for req in pending {
        let gpu = smithay::wayland::dmabuf::get_dmabuf(&req.buffer).is_ok();
        note_capture_start(capture_log, &req, gpu);
        if let Ok(dmabuf) = smithay::wayland::dmabuf::get_dmabuf(&req.buffer) {
            drew_dmabuf = true;
            use smithay::backend::renderer::ImportDma;
            let mut dmabuf_rendered = false;
            if let Ok(mut dmabuf_tex) = renderer.import_dmabuf(dmabuf, None)
                && let Ok(mut dmabuf_fb) = renderer.bind(&mut dmabuf_tex)
            {
                // the region (or the whole view) in what the screen shows, its buffer turned
                let view_target = req.region.map(|(_, _, w, h)| (w, h)).unwrap_or(view);
                let target_size = convert::capture_buffer_size(view_target, transform);
                let (rx, ry) = screencopy_region_offset(req.region);
                let mut damage_tracker = OutputDamageTracker::new(
                    target_size,
                    smithay::utils::Scale::from(k),
                    draw_transform,
                );
                let render_res = if rx != 0 || ry != 0 {
                    let relocated: Vec<_> = elements
                        .iter()
                        .map(|e| {
                            smithay::backend::renderer::element::utils::RelocateRenderElement::from_element(
                                e,
                                smithay::utils::Point::from((-rx, -ry)),
                                smithay::backend::renderer::element::utils::Relocate::Relative,
                            )
                        })
                        .collect();
                    damage_tracker.render_output(renderer, &mut dmabuf_fb, 0, &relocated, clear)
                } else {
                    damage_tracker.render_output(renderer, &mut dmabuf_fb, 0, elements, clear)
                };
                if render_res.is_ok() {
                    dmabuf_rendered = true;
                    req.ready((target_size.0 as u32, target_size.1 as u32), now);
                }
            }
            if !dmabuf_rendered {
                // Non-GPU-importable dmabufs fail: CPU fallback into client mmaps risks SIGBUS
                req.fail();
            }
        } else {
            remaining.push(req);
        }
    }
    if drew_dmabuf {
        turn.mark("capture draw");
    }

    if remaining.is_empty() {
        return;
    }

    // Fallback for wl_shm or CPU buffers when a request was not fulfilled on GPU.
    if screencopy_texture
        .as_ref()
        .is_none_or(|t| t.width() != size.0 as u32 || t.height() != size.1 as u32)
    {
        match Offscreen::<GlesTexture>::create_buffer(renderer, Fourcc::Argb8888, size.into()) {
            Ok(b) => *screencopy_texture = Some(b),
            Err(e) => {
                log_line!("[screencopy] Failed to create offscreen buffer: {e:?}");
                for p in remaining {
                    p.fail();
                }
                return;
            }
        }
    }

    let offscreen = screencopy_texture.as_mut().unwrap();
    let mut framebuffer = match renderer.bind(offscreen) {
        Ok(fb) => fb,
        Err(e) => {
            log_line!("[screencopy] Failed to bind offscreen buffer: {e:?}");
            for p in remaining {
                p.fail();
            }
            return;
        }
    };

    let mut damage_tracker =
        OutputDamageTracker::new(size, smithay::utils::Scale::from(k), draw_transform);
    let render_res = damage_tracker.render_output(renderer, &mut framebuffer, 0, elements, clear);
    turn.mark("capture draw");
    if let Err(e) = render_res {
        log_line!("[screencopy] Failed to render output: {e:?}");
        for p in remaining {
            p.fail();
        }
        return;
    }

    let mapping = ExportMem::copy_framebuffer(
        renderer,
        &framebuffer,
        Rectangle::from_size(size.into()),
        Fourcc::Argb8888,
    );
    turn.mark("capture readback");
    let mapping = match mapping {
        Ok(m) => m,
        Err(e) => {
            log_line!("[screencopy] Failed to copy framebuffer: {e:?}");
            for p in remaining {
                p.fail();
            }
            return;
        }
    };
    drop(framebuffer);
    let sync = readback_sync(renderer);

    // the GPU copies in the background; `deliver_copies` maps it only once `sync` has signalled
    copies.push(PendingCopy {
        mapping,
        sync,
        watched: false,
        job: None,
        output_id,
        reqs: remaining,
        size,
        view,
        transform,
        now,
    });
}

/// Inserts and flushes an EGL fence after `copy_framebuffer` so we know when the GPU has finished
/// writing into the PBO before mapping it.
#[allow(unsafe_code)]
fn readback_sync(renderer: &mut GlesRenderer) -> SyncPoint {
    if renderer.capabilities().contains(&Capability::ExportFence) {
        let display = renderer.egl_context().display().clone();
        if let Ok(Ok(fence)) = renderer.with_context(|gl| {
            let fence = EGLFence::create(&display);
            if fence.is_ok() {
                unsafe {
                    gl.Flush();
                }
            }
            fence
        }) {
            return SyncPoint::from(fence);
        }
    }
    SyncPoint::signaled()
}

/// Wakes the event loop when `sync` signals so a completed capture copy is delivered without polling.
pub(super) fn watch_copy_sync(
    loop_handle: &smithay::reexports::calloop::LoopHandle<'static, LoopData>,
    sync: &SyncPoint,
    session: &mut Session,
) {
    if sync.is_reached() {
        session.request_frame();
        return;
    }
    if let Some(fd) = sync.export() {
        let source = Generic::new(fd, Interest::READ, Mode::Level);
        if loop_handle
            .insert_source(source, |_, _, _| Ok(PostAction::Remove))
            .is_ok()
        {
            return;
        }
    }
    let _ = loop_handle.insert_source(
        Timer::from_duration(Duration::from_millis(2)),
        |_, _, data| {
            if data.capture_copies.iter().all(|c| c.sync.is_reached()) {
                TimeoutAction::Drop
            } else {
                TimeoutAction::ToDuration(Duration::from_millis(2))
            }
        },
    );
}

fn has_pending_readback(copies: &[PendingCopy], output_id: OutputId) -> bool {
    copies.iter().any(|c| c.output_id == output_id)
}

/// A capture into memory whose copy out of the GPU was started: handed over by `deliver_copies`.
pub(super) struct PendingCopy {
    pub(super) mapping: <GlesRenderer as ExportMem>::TextureMapping,
    pub(super) sync: SyncPoint,
    pub(super) watched: bool,
    /// The copy thread's job filling the clients' buffers, sent once the GPU finished.
    pub(super) job: Option<u64>,
    pub(super) output_id: OutputId,
    pub(super) reqs: Vec<infynitae_wayland::screencopy::PendingFrame>,
    pub(super) size: (i32, i32),
    pub(super) view: (i32, i32),
    pub(super) transform: infynitae_core::output::Transform,
    pub(super) now: std::time::Duration,
}

/// A finished readback's mapped pixels, read by the copy thread.
struct Pixels {
    ptr: *const u8,
    len: usize,
}

// SAFETY: the pixels are only read, and stay mapped while a job reads them: the `PendingCopy`
// owning the mapping is dropped only after its job is reported done (`deliver_copies`),
// `recover_renderer` waits for the copy thread before the renderer goes, and `CopyWorker` is
// dropped (its thread joined) before the renderer (`LoopData`'s field order).
#[allow(unsafe_code)]
unsafe impl Send for Pixels {}

impl Pixels {
    #[allow(unsafe_code)]
    fn slice(&self) -> &[u8] {
        // SAFETY: see `Send for Pixels`
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }
}

type CopyJob = Box<dyn FnOnce() -> Vec<bool> + Send>;

/// Copies finished readbacks into the clients' memory on a thread of its own: a 2560x1440 screen
/// is 14.7 MB a frame, copied in the compositor's turn before.
pub(super) struct CopyWorker {
    jobs: Option<std::sync::mpsc::Sender<(u64, CopyJob)>>,
    done: std::sync::mpsc::Receiver<(u64, Vec<bool>)>,
    thread: Option<std::thread::JoinHandle<()>>,
    next: u64,
    in_flight: usize,
    /// Jobs done, each with whether each buffer was filled, not yet taken by `finished`.
    finished: Vec<(u64, Vec<bool>)>,
}

impl CopyWorker {
    /// `wake` is pinged after each job, so the loop hands its captures over.
    pub(super) fn new(wake: smithay::reexports::calloop::ping::Ping) -> std::io::Result<Self> {
        let (jobs, todo) = std::sync::mpsc::channel::<(u64, CopyJob)>();
        let (report, done) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("capture copy".into())
            .spawn(move || {
                for (id, job) in todo {
                    if report.send((id, job())).is_err() {
                        return;
                    }
                    wake.ping();
                }
            })?;
        Ok(Self {
            jobs: Some(jobs),
            done,
            thread: Some(thread),
            next: 0,
            in_flight: 0,
            finished: Vec::new(),
        })
    }

    /// The job's id, or `None` when the thread is gone (it panicked).
    fn send(&mut self, job: CopyJob) -> Option<u64> {
        let id = self.next;
        self.next += 1;
        self.jobs.as_ref()?.send((id, job)).ok()?;
        self.in_flight += 1;
        Some(id)
    }

    fn finished(&mut self) -> Vec<(u64, Vec<bool>)> {
        self.finished.extend(self.done.try_iter());
        self.in_flight = self.in_flight.saturating_sub(self.finished.len());
        std::mem::take(&mut self.finished)
    }

    /// Blocks until every job sent so far is done: before the mappings they read go away.
    pub(super) fn wait(&mut self) {
        while self.in_flight > 0 {
            let Ok(done) = self.done.recv() else { break };
            self.finished.push(done);
            self.in_flight -= 1;
        }
        self.in_flight = 0;
    }
}

impl Drop for CopyWorker {
    fn drop(&mut self) {
        // the thread ends after the jobs already sent
        drop(self.jobs.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Hands over the captures the copy thread filled, and sends it those whose GPU fence has
/// signalled, leaving unfinished readbacks in `copies` so `map_texture` never stalls the loop.
pub(super) fn deliver_copies(
    renderer: &mut GlesRenderer,
    copies: &mut Vec<PendingCopy>,
    worker: &mut CopyWorker,
    woken_outputs: &mut Vec<OutputId>,
) {
    for (id, copied) in worker.finished() {
        // gone: failed when the renderer was made again (`recover_renderer`)
        let Some(pos) = copies.iter().position(|c| c.job == Some(id)) else {
            continue;
        };
        // the mapping is let go here, on the renderer's thread
        let copy = copies.remove(pos);
        if !woken_outputs.contains(&copy.output_id) {
            woken_outputs.push(copy.output_id);
        }
        for (req, filled) in copy.reqs.into_iter().zip(copied) {
            if filled {
                let target_size = req
                    .region
                    .map(|(_, _, w, h)| convert::capture_buffer_size((w, h), copy.transform))
                    .unwrap_or(copy.size);
                req.ready((target_size.0 as u32, target_size.1 as u32), copy.now);
            } else {
                req.fail();
            }
        }
    }

    let mut i = 0;
    while i < copies.len() {
        let c = &mut copies[i];
        if c.job.is_some() || !c.sync.is_reached() {
            i += 1;
            continue;
        }
        c.job = match ExportMem::map_texture(renderer, &c.mapping) {
            Ok(slice) => {
                let pixels = Pixels {
                    ptr: slice.as_ptr(),
                    len: slice.len(),
                };
                let targets: Vec<_> = c
                    .reqs
                    .iter()
                    .map(|r| (r.buffer.clone(), r.region))
                    .collect();
                let (size, view, transform) = (c.size, c.view, c.transform);
                let job = worker.send(Box::new(move || {
                    targets
                        .iter()
                        .map(|(buffer, region)| {
                            copy_into_shm(pixels.slice(), buffer, *region, size, view, transform)
                        })
                        .collect()
                }));
                if job.is_none() {
                    log_line!("[screencopy] The capture copy thread stopped");
                }
                job
            }
            Err(e) => {
                log_line!("[screencopy] Failed to map texture: {e:?}");
                None
            }
        };
        if c.job.is_some() {
            i += 1;
            continue;
        }
        let copy = copies.remove(i);
        if !woken_outputs.contains(&copy.output_id) {
            woken_outputs.push(copy.output_id);
        }
        for p in copy.reqs {
            p.fail();
        }
    }
}

/// Copies the readback `slice` (`size` px, the panel's orientation) into a client's shm
/// `buffer`: all of it, or the `region` of `view`. False if the buffer cannot take it.
#[allow(unsafe_code)]
fn copy_into_shm(
    slice: &[u8],
    buffer: &smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer,
    region: Option<(i32, i32, i32, i32)>,
    size: (i32, i32),
    view: (i32, i32),
    transform: infynitae_core::output::Transform,
) -> bool {
    let src_stride = (size.0 * 4) as usize;
    // the region's place in the whole-screen buffer (the panel's orientation)
    let (rx, ry) = region
        .map(|r| convert::capture_region_in_buffer(r, view, transform))
        .map(|(x, y, _, _)| (x.max(0) as usize, y.max(0) as usize))
        .unwrap_or((0, 0));

    let copy_result = with_buffer_contents_mut(buffer, |ptr, len, buffer_data| {
        if buffer_data.format
            != smithay::reexports::wayland_server::protocol::wl_shm::Format::Argb8888
            && buffer_data.format
                != smithay::reexports::wayland_server::protocol::wl_shm::Format::Xrgb8888
        {
            return false;
        }
        let h = (buffer_data.height.max(0) as usize).min(size.1 as usize);
        let w_bytes = (buffer_data.width.max(0) as usize).min(size.0 as usize) * 4;
        let dest_stride = buffer_data.stride.max(0) as usize;
        let offset = buffer_data.offset.max(0) as usize;

        if dest_stride < w_bytes {
            return false;
        }
        let total_required = match dest_stride
            .checked_mul(h.saturating_sub(1))
            .and_then(|v| v.checked_add(w_bytes))
            .and_then(|v| v.checked_add(offset))
        {
            Some(bytes) => bytes,
            None => return false,
        };
        if total_required > len {
            return false;
        }

        // SAFETY: bounds verified: `dest_stride >= w_bytes`, `total_required <= len`,
        // and `src_offset + w_bytes <= slice.len()`.
        unsafe {
            let dest_base = ptr.add(offset);
            for row in 0..h {
                let src_row_idx = (ry + row).min(size.1.max(0) as usize);
                let src_offset = src_row_idx * src_stride + rx * 4;
                if src_offset + w_bytes <= slice.len() {
                    let src_row = &slice[src_offset..(src_offset + w_bytes)];
                    let dest_row = dest_base.add(row * dest_stride);
                    std::ptr::copy_nonoverlapping(src_row.as_ptr(), dest_row, w_bytes);
                }
            }
        }
        true
    });

    matches!(copy_result, Ok(true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_log_reports_client_path_and_buffer_kind_once_per_capture() {
        let mut seen = super::CaptureSeen::default();
        assert!(seen.note((1u64, None::<u64>), true));
        assert!(!seen.note((1u64, None::<u64>), true));
        // Buffer path change or new session/client logs again.
        assert!(seen.note((1u64, None::<u64>), false));
        assert!(seen.note((2u64, None::<u64>), true));

        assert_eq!(
            super::format_capture_log(Some("obs"), Some("/usr/bin/obs"), true),
            "[infynitae] Capture started by obs (/usr/bin/obs): into a GPU buffer (dmabuf)"
        );
        assert_eq!(
            super::format_capture_log(
                Some("org.obsproject.Studio"),
                Some("org.obsproject.Studio (sandboxed by flatpak)"),
                false,
            ),
            "[infynitae] Capture started by org.obsproject.Studio (sandboxed by flatpak): into memory (shm), read back from the GPU"
        );
    }

    #[test]
    fn copy_thread_runs_jobs_while_the_loop_goes_on() {
        let (ping, _wake) = smithay::reexports::calloop::ping::make_ping().expect("ping");
        let mut worker = CopyWorker::new(ping).expect("copy thread");
        let (release, gate) = std::sync::mpsc::channel::<()>();
        let first = worker
            .send(Box::new(move || {
                gate.recv().expect("released");
                vec![true, false]
            }))
            .expect("sent");
        let second = worker.send(Box::new(|| vec![true])).expect("sent");
        // the loop is not held while a copy runs
        assert!(worker.finished().is_empty());
        release.send(()).expect("release");
        worker.wait();
        assert_eq!(
            worker.finished(),
            [(first, vec![true, false]), (second, vec![true])]
        );
        assert!(worker.finished().is_empty());
    }

    #[test]
    fn capture_readback_sync_signals_and_delivers_when_gpu_finishes() {
        use smithay::backend::allocator::Fourcc;
        use smithay::backend::allocator::gbm::GbmDevice;
        use smithay::backend::renderer::gles::{Capability, GlesTexture};
        use smithay::backend::renderer::{Bind, ExportMem, Frame, Offscreen, Renderer};
        use smithay::utils::{DeviceFd, Rectangle, Transform};

        let Some(mut renderer) = std::fs::OpenOptions::new()
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
            .and_then(|gbm| gles_renderer(&gbm).ok())
        else {
            eprintln!("no GPU render node: skipped");
            return;
        };

        let size = (4, 4);
        let mut offscreen: GlesTexture =
            Offscreen::create_buffer(&mut renderer, Fourcc::Argb8888, size.into())
                .expect("offscreen");
        let mapping = {
            let mut target = renderer.bind(&mut offscreen).expect("bind");
            let mut frame = renderer
                .render(&mut target, size.into(), Transform::Normal)
                .expect("frame");
            frame
                .clear(
                    [0.2, 0.4, 0.6, 1.0].into(),
                    &[Rectangle::from_size(size.into())],
                )
                .expect("clear");
            let _ = frame.finish().expect("finish");
            ExportMem::copy_framebuffer(
                &mut renderer,
                &target,
                Rectangle::from_size(size.into()),
                Fourcc::Argb8888,
            )
            .expect("copy_framebuffer")
        };

        let sync = readback_sync(&mut renderer);
        if renderer.capabilities().contains(&Capability::ExportFence) {
            assert!(sync.contains_fence(), "readback should carry an EGL fence");
            if sync.is_exportable() {
                assert!(sync.export().is_some());
            }
        }
        sync.wait().expect("fence wait");
        assert!(sync.is_reached());

        let (mut core, _, _, mut token) = Core::new(Default::default());
        let make_output = |connector: &str| OutputDescriptor {
            key: OutputKey {
                device: "test".into(),
                connector: connector.into(),
                make: "Test".into(),
                model: "Test".into(),
                serial: None,
            },
            mode: (1920, 1080),
            transform: infynitae_core::output::Transform::Normal,
            scale_120: 120,
            refresh_mhz: 60_000,
        };
        let (id1, _) = core
            .output_connected(&mut token, make_output("DP-1"))
            .expect("output 1");
        let (id2, _) = core
            .output_connected(&mut token, make_output("DP-2"))
            .expect("output 2");

        let mut copies = vec![PendingCopy {
            mapping,
            sync,
            watched: false,
            job: None,
            output_id: id1,
            reqs: Vec::new(),
            size,
            view: size,
            transform: infynitae_core::output::Transform::Normal,
            now: Duration::ZERO,
        }];
        assert!(has_pending_readback(&copies, id1));
        assert!(!has_pending_readback(&copies, id2));

        let (ping, _wake) = smithay::reexports::calloop::ping::make_ping().expect("ping");
        let mut worker = CopyWorker::new(ping).expect("copy thread");
        let mut woken = Vec::new();
        // mapped and sent to the copy thread; handed over on a later turn
        deliver_copies(&mut renderer, &mut copies, &mut worker, &mut woken);
        assert!(copies[0].job.is_some());
        assert!(woken.is_empty());
        worker.wait();
        deliver_copies(&mut renderer, &mut copies, &mut worker, &mut woken);
        assert!(copies.is_empty());
        assert_eq!(woken, [id1]);
    }
}
