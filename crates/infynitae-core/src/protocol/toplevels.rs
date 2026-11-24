//! The toplevel lifecycle (M1): requests, activation, commits, tree changes, destruction, popup targets (M3).

use super::*;

impl Core {
    /// A user action on a window brings it to the top of the z-order: a click on it, or Super+Tab
    /// to it (the C4 focus model: "click → raise + keyboard focus"; the focus model requests the
    /// raise of the Stage, FOUNDATION_BLOCKERS z-order row). Written here, in the Stage's single
    /// writer (INV-STAGE-1). Activation requests of clients never raise (ADR-01: z-order is not
    /// coupled to activation). Nothing happens for an unmapped element or the top one; otherwise
    /// every output's frame is stale. Returns whether the order changed.
    pub(crate) fn raise_by_user(&mut self, id: ElementId) -> bool {
        // allocation-free: the click path is a hot path (OB-42)
        if self.stage.mapped(id).is_none() || self.stage.top_mapped() == Some(id) {
            return false;
        }
        self.stage.raise(id);
        self.invalidate_all_frames();
        true
    }

    /// `xdg_toplevel.set_title` / `set_app_id` (owner decision 2026-10-20 D9): the
    /// toplevel's title and app_id, replaced wholesale, kept as display metadata for the shell.
    /// Each is cut to [`TOPLEVEL_META_MAX`] bytes at a character boundary, so a client cannot make
    /// Core hold an unbounded string. Decides nothing, emits nothing.
    ///
    /// A window that turns out to be an app's notification only now (its title came after it
    /// appeared: Steam's toasts through xwayland-satellite) is moved to the notification corner;
    /// the returned effects are that move's.
    pub fn toplevel_metadata(
        &mut self,
        id: ElementId,
        title: Option<&str>,
        app_id: Option<&str>,
    ) -> Result<Vec<Effect>, ProtocolError> {
        let cut = |s: &str| {
            let mut end = s.len().min(TOPLEVEL_META_MAX);
            while !s.is_char_boundary(end) {
                end -= 1;
            }
            s[..end].to_owned()
        };
        let was_notification = self.is_notification(id);
        let was_on_top = self.stage.is_above(id);
        self.stage
            .set_meta(
                id,
                crate::stage::ToplevelMeta {
                    title: title.map(cut),
                    app_id: app_id.map(cut),
                },
            )
            .map_err(|_| ProtocolError::StaleElement)?;
        let mut out = Vec::new();
        if !was_notification
            && self.is_notification(id)
            && self.stage.mapped(id).is_some()
            && let Some(tree) = self.stage.tree(id).cloned()
            && let Some(at) = self.notification_anchor(id, &tree)
        {
            self.place_by_shell(id, at, &mut out)?;
        }
        // picture-in-picture says what it is by its title: kept on top from then on, and put
        // in the corner if it was already showing somewhere else
        if !was_on_top && self.named_on_top(id) {
            self.set_on_top(id, true);
            if self.stage.mapped(id).is_some()
                && let Some(tree) = self.stage.tree(id).cloned()
                && let Some(at) = self.on_top_anchor(&tree)
            {
                self.place_by_shell(id, at, &mut out)?;
            }
        }
        Ok(out)
    }

    /// B10.7 / INV-INPUT-9: an `xdg_activation_v1` request for `id`. `validated`: its token
    /// passed the adapter's provenance check (W-SM-8: created by the client that held the
    /// keyboard focus, with a serial of this seat no older than the last focus change, unused,
    /// unexpired). Outcome (`ActivationPolicy`; PROV-10, OB-40): under the default
    /// `Strict` policy a validated request for an element that may take keyboard focus — mapped
    /// and not hidden by a presentation (INV-INPUT-8) — focuses it, an activation grant; every
    /// other request for a mapped element marks it for attention, which only the shell renders;
    /// an element that is not mapped is ignored. It never moves a camera or an anchor: nothing
    /// here can name one.
    pub fn activation_request(
        &mut self,
        id: ElementId,
        validated: bool,
    ) -> Result<Vec<Effect>, ProtocolError> {
        let state = self.stage.state(id).ok_or(ProtocolError::StaleElement)?;
        if state != Lifecycle::Mapped {
            return Ok(Vec::new());
        }
        let eligible = !self.hidden_by_presentation(id);
        // a minimized window the user asks for (a validated activation: its app's own window
        // brought forward, or chosen from a taskbar) comes back
        let mut back = Vec::new();
        if validated && eligible && self.stage.is_minimized(id) {
            self.restore_minimized(id, &mut back);
        }
        let mut fx = self.focus.activation(id, validated, eligible);
        fx.splice(0..0, back);
        if !fx.is_empty() {
            fx.extend(self.reconcile_constraints());
        }
        Ok(fx)
    }

    /// M2 / M9: a client's `xdg_toplevel` request. The policy, in one place:
    ///
    /// - `set_maximized` / `unset_maximized`: declined, one configure; `maximized` is never
    ///   granted (INV-PROTO-1).
    /// - `set_fullscreen`: granted if the element holds ActiveElementFocus, or before its first
    ///   map; otherwise declined at once, nothing stored (INV-PRES-1). Before the initial commit
    ///   it is recorded and decided in the initial configure. Either way exactly one configure
    ///   answers it.
    /// - `unset_fullscreen`: ends fullscreen with one configure offering the saved size; when
    ///   not fullscreen, one configure.
    /// - `set_minimized` goes through [`Core::client_minimize`] (2026-11-07); here it stays
    ///   ignored. `show_window_menu`: ignored, there is none.
    /// - `set_maximized`, `unset_maximized`, `move`, `resize` from an app go through
    ///   [`Core::client_maximize`] and [`Core::client_drag`] (owner, 2026-11-02); here they stay
    ///   as declined or ignored, for a request with nothing to act on.
    ///
    /// Nothing is answered while the toplevel is unmapped. No request moves an anchor, a camera
    /// or the focus.
    pub fn toplevel_request(
        &mut self,
        id: ElementId,
        request: ToplevelRequest,
    ) -> Result<(ToplevelDecision, Vec<Effect>), ProtocolError> {
        use ToplevelDecision::*;
        use ToplevelRequest::*;
        let state = self.stage.state(id).ok_or(ProtocolError::StaleElement)?;
        let answerable = matches!(state, Lifecycle::AwaitingFirstBuffer | Lifecycle::Mapped);
        let declined = |answerable: bool| match answerable {
            true => (Declined, vec![Effect::ToplevelConfigure(id)]),
            false => (Ignored, Vec::new()),
        };
        Ok(match request {
            SetMaximized | UnsetMaximized => declined(answerable),
            SetMinimized | Move | Resize | ShowWindowMenu => (Ignored, Vec::new()),
            SetFullscreen { output } => match state {
                Lifecycle::AwaitingInitialCommit => {
                    self.premap_fullscreen.retain(|(e, _)| *e != id);
                    self.premap_fullscreen.push((id, output));
                    (Recorded, Vec::new())
                }
                Lifecycle::Unmapped => (Ignored, Vec::new()),
                Lifecycle::AwaitingFirstBuffer | Lifecycle::Mapped => {
                    let before_first_map = !self.stage.mapped_before(id);
                    let focused = self.focus.target() == Some(id);
                    // a window asking as it opens: mapped last, nothing pressed since and the
                    // focus where it was (owner's report, 2026-11-19: a game that maps, then asks
                    // a frame later, was declined for want of the focus it never had the time to
                    // get, and stayed a window wherever it had opened)
                    let opening = state == Lifecycle::Mapped
                        && self.opening == Some((id, self.focus.target()));
                    let allowed = (before_first_map || focused || opening)
                        && !self.left_fullscreen.contains(&id);
                    match self.fullscreen_output(id, output).filter(|_| allowed) {
                        Some(o) => {
                            // the last windowed size: one still on offer, else the geometry
                            let saved = self
                                .restore_offers
                                .iter()
                                .find(|(e, _)| *e == id)
                                .map(|(_, size)| *size)
                                .or_else(|| {
                                    self.stage.mapped(id).map(|(_, tree, _)| tree.geometry.size)
                                });
                            let before = self.presentations();
                            let replaced = self.store_fullscreen(id, o, saved);
                            let mut fx = vec![Effect::ToplevelConfigure(id)];
                            fx.extend(replaced);
                            fx.extend(self.presentation_changed_since(&before));
                            // it has the screen: the keys go to it too, not to the window it
                            // hides (an unfocused grant is one asked as it opens)
                            if !focused && !before_first_map {
                                fx.extend(self.focus_opener(id));
                            }
                            Self::hints_before_configures(&mut fx);
                            (Granted, fx)
                        }
                        None => declined(true),
                    }
                }
            },
            UnsetFullscreen => match state {
                Lifecycle::AwaitingInitialCommit => {
                    self.premap_fullscreen.retain(|(e, _)| *e != id);
                    (Ignored, Vec::new())
                }
                Lifecycle::Unmapped => (Ignored, Vec::new()),
                Lifecycle::AwaitingFirstBuffer | Lifecycle::Mapped => {
                    match self.fullscreen.iter().position(|f| f.element == id) {
                        Some(i) => {
                            let before = self.presentations();
                            let old = self.fullscreen.remove(i);
                            let mut fx = vec![self.fullscreen_ended(old)];
                            fx.extend(self.presentation_changed_since(&before));
                            Self::hints_before_configures(&mut fx);
                            (Released, fx)
                        }
                        None => declined(true),
                    }
                }
            },
        })
    }

    /// M1 `new_toplevel`: mints an `ElementId`; no anchor yet.
    pub fn toplevel_created(&mut self) -> ElementId {
        self.stage.create()
    }

    /// A committed surface-tree state of an element, for a caller that does not track the
    /// client's acknowledgements: the commit is taken to carry the state Core decided last (a
    /// client that acknowledges every configure before it commits). The adapter calls
    /// [`Core::toplevel_committed_acked`].
    pub fn toplevel_committed(
        &mut self,
        id: ElementId,
        tree: SurfaceTree,
    ) -> Result<Vec<Effect>, ProtocolError> {
        let mut out = EffectBuf::new();
        self.toplevel_committed_into(id, tree, &mut out)?;
        Ok(out.into_vec())
    }

    /// [`Core::toplevel_committed`] into the caller's reusable [`EffectBuf`] (C14.3).
    pub fn toplevel_committed_into(
        &mut self,
        id: ElementId,
        tree: SurfaceTree,
        out: &mut EffectBuf,
    ) -> Result<(), ProtocolError> {
        let acked_fullscreen = self.fullscreen_of(id).is_some();
        self.toplevel_committed_acked_into(id, tree, acked_fullscreen, out)
    }

    /// [`Core::toplevel_committed_acked`] into the caller's reusable [`EffectBuf`] (C14.3): the
    /// effects are appended in order; on an error nothing is appended.
    pub fn toplevel_committed_acked_into(
        &mut self,
        id: ElementId,
        tree: SurfaceTree,
        acked_fullscreen: bool,
        out: &mut EffectBuf,
    ) -> Result<(), ProtocolError> {
        let start = out.fx.len();
        let requested = tree.frame_requested();
        let result = self.committed(id, tree, acked_fullscreen, &mut out.fx, start);
        if result.is_err() {
            out.fx.truncate(start);
        } else {
            // OBJ-17: what the commit left pending (M16)
            self.frame_liveness.committed(id, requested);
        }
        result
    }

    /// A committed surface-tree state of an element (written wholesale per commit, OBJ-05).
    /// Drives M1 from the root surface's buffer state. `acked_fullscreen`: whether the state
    /// this commit applies — the last configure the client acknowledged before it — contains
    /// `fullscreen` (xdg-shell applies an acknowledged configure's state at the next commit).
    /// A rejected commit changes nothing.
    pub fn toplevel_committed_acked(
        &mut self,
        id: ElementId,
        tree: SurfaceTree,
        acked_fullscreen: bool,
    ) -> Result<Vec<Effect>, ProtocolError> {
        let mut out = EffectBuf::new();
        self.toplevel_committed_acked_into(id, tree, acked_fullscreen, &mut out)?;
        Ok(out.into_vec())
    }

    /// The body of [`Core::toplevel_committed_acked`]: appends to `out` from `start`. The
    /// presentation snapshot and the touched outputs use reused storage (C14.3); an error leaves
    /// that storage empty, which costs only its capacity.
    fn committed(
        &mut self,
        id: ElementId,
        tree: SurfaceTree,
        acked_fullscreen: bool,
        out: &mut Vec<Effect>,
        start: usize,
    ) -> Result<(), ProtocolError> {
        tree.validate().map_err(ProtocolError::Snapshot)?;
        let state = self.stage.state(id).ok_or(ProtocolError::StaleElement)?;
        let has_buffer = tree.root().is_some_and(|r| r.mapped);
        let stale = |_| ProtocolError::StaleElement;
        let mut before = std::mem::take(&mut self.presentations_buf);
        self.fill_presentations(&mut before);
        match (state, has_buffer) {
            (Lifecycle::AwaitingInitialCommit | Lifecycle::Unmapped, true) => {
                return Err(ProtocolError::BufferBeforeConfigure);
            }
            (Lifecycle::AwaitingInitialCommit, false) => {
                let anchor = self.initial_anchor();
                self.stage.place(id, anchor).map_err(stale)?;
                self.stage.set_tree(id, Some(tree.clone())).map_err(stale)?;
                self.stage
                    .set_state(id, Lifecycle::AwaitingFirstBuffer)
                    .map_err(stale)?;
                // M2 pre-configure: a recorded fullscreen request is decided here, in the initial
                // configure (before the first map, INV-PRES-1); placement ran first (C5.2)
                let grant = match self.premap_fullscreen.iter().position(|(e, _)| *e == id) {
                    Some(i) => {
                        let (_, requested) = self.premap_fullscreen.remove(i);
                        self.fullscreen_output(id, requested)
                    }
                    None => None,
                };
                // M15 / F-SCALE-3: Initial scale prediction & emission before InitialConfigure
                let views: Vec<_> = self
                    .outputs
                    .iter()
                    .filter_map(|o| self.view(o.id).map(|v| (o.id, v)))
                    .collect();
                let granted = grant.and_then(|o| self.outputs.iter().find(|m| m.id == o));
                let rho_pred = match granted {
                    // C12.F12: the presentation the grant starts at the
                    // first map (scale_O, zoom 1) and the other outputs' cameras that see the rect
                    Some(o) => {
                        let others: Vec<_> =
                            views.iter().copied().filter(|(v, _)| *v != o.id).collect();
                        crate::scale::predict_rho_observed(
                            anchor,
                            tree.geometry,
                            &self.outputs,
                            &others,
                        )
                        .map_or(o.scale, |r| r.max(o.scale))
                    }
                    None => crate::scale::predict_rho(anchor, tree.geometry, &self.outputs, &views),
                };
                // C12.F11: the configured fullscreen size bounds it too
                let configured = granted.map_or(0, |o| {
                    let (w, h) = o.logical_extent();
                    (w.floor().max(0.0) as u32).max(h.floor().max(0.0) as u32)
                });
                let s_max = crate::scale::s_max_for_tree(Some(&tree)).max(configured);
                let h_dev = crate::scale::h_dev(s_max, self.scale_config.d_max);
                let h_max =
                    crate::scale::h_max(self.scale_config.h_cfg, s_max, self.scale_config.d_max);
                let ladder = self.ladders.get(&self.outputs, h_max, self.scale_config.r);
                let initial_h =
                    crate::scale::target(rho_pred, ladder, h_max, self.scale_config.sigma);
                let s120 = crate::scale::preferred_scale_120(initial_h);
                let b = crate::scale::preferred_buffer_scale(initial_h, h_dev);

                let mut st = crate::scale::ScaleState::new(initial_h);
                st.last_emitted_scale_120 = Some(s120);
                st.last_emitted_buffer_scale = Some(b);
                st.last_rho = Some(rho_pred);
                self.scales.set(id, st);

                out.extend([
                    Effect::PreferredScale {
                        element: id,
                        scale_120: s120,
                        integer_scale: b,
                    },
                    Effect::InitialConfigure(id),
                ]);
                if let Some(o) = grant {
                    out.extend(self.store_fullscreen(id, o, None));
                }
            }
            (Lifecycle::Unmapped, false) => {
                // anchor kept (INV-STAGE-5)
                self.stage.set_tree(id, Some(tree)).map_err(stale)?;
                self.stage
                    .set_state(id, Lifecycle::AwaitingFirstBuffer)
                    .map_err(stale)?;
                out.push(Effect::InitialConfigure(id));
            }
            (Lifecycle::AwaitingFirstBuffer, false) => {
                self.stage.set_tree(id, Some(tree)).map_err(stale)?;
            }
            (Lifecycle::Mapped, true) => {
                // the rect before this commit: what a window that grew newly reaches
                let old_size = self.stage.tree(id).map(|old| old.geometry.size);
                let old_rect = self
                    .stage
                    .anchor(id)
                    .zip(old_size)
                    .map(|(a, s)| (a.x, a.y, a.x + i64::from(s.w), a.y + i64::from(s.h)));
                // a resize drag on it (taken before the drag's last commit ends the track)
                let resizing = self.resize.is_some_and(|r| r.element == id);
                // a window being resized from its left or top keeps its opposite corner in place
                if let Some(anchor) = self.resize_committed(id, tree.geometry.size, out) {
                    self.stage.place(id, anchor).map_err(stale)?;
                }
                // a window a layout asked for a size is centred in its cell at the size it drew
                let laid_out = self.laid_out_anchor(id, tree.geometry.size);
                if let Some(at) = laid_out {
                    self.stage.place(id, at).map_err(stale)?;
                }
                // An element that left fullscreen without ever having been a window is placed at
                // its first windowed presentation (P9b): the first commit after it acknowledged a
                // configure without the fullscreen state, whatever its size. Frames committed
                // before that acknowledgement are fullscreen frames, whatever their size.
                let arrived = !acked_fullscreen && self.windowed_placement_pending.contains(&id);
                if arrived && self.fullscreen_of(id).is_none() {
                    self.place_by_default(id, &tree)?;
                }
                // the client redrew at another size: a size still on offer is spent
                if old_size.is_some_and(|old| old != tree.geometry.size) {
                    self.restore_offers.retain(|(e, _)| *e != id);
                }
                self.stage.set_tree(id, Some(tree)).map_err(stale)?;
                // back from fullscreen: its first windowed frame keeps its place or finds a free one
                let returned = !acked_fullscreen
                    && self.fullscreen_of(id).is_none()
                    && self.returning.contains(&id);
                if returned {
                    self.returning.retain(|e| *e != id);
                    if let Some(at) = self.returned_from_fullscreen(id) {
                        self.stage.place(id, at).map_err(stale)?;
                        self.invalidate_all_frames();
                    }
                }
                // a window that grew by itself finds room; a fullscreen one never does: it is
                // shown over the whole screen (F-PRESENT-1), not at its place on the canvas. A
                // frame drawn for an acknowledged fullscreen is one, whether or not the grant has
                // reached Core yet
                if self.placement == PlacementPolicy::ClusterTiling
                    && self.fullscreen_of(id).is_none()
                    && !acked_fullscreen
                    && !returned
                    && let (Some(old), Some(a), Some(t)) =
                        (old_rect, self.stage.anchor(id), self.stage.tree(id))
                    && old.2 > old.0
                    && old.3 > old.1
                {
                    let new = (
                        a.x,
                        a.y,
                        a.x + i64::from(t.geometry.size.w),
                        a.y + i64::from(t.geometry.size.h),
                    );
                    // a size the user or the compositor asked for (a resize, a maximize, a
                    // session restore) moves no other window: only the window itself moves
                    // (owner, 2026-11-06: "maximize moves the window itself, not the other
                    // windows"; until then a maximize or a resize pushed the row aside). A
                    // maximized window stays where the maximize put it. One that took a size by
                    // itself moves off what it would reach (owner, 2026-10-31)
                    let asked = self.held_size(id) == Some(t.geometry.size);
                    let maximized = self.maximized.iter().any(|(e, _)| *e == id);
                    if !asked
                        && !resizing
                        && laid_out.is_none()
                        && !maximized
                        && let Some(at) = self.self_resized(id, old, new)
                    {
                        self.stage.place(id, at).map_err(stale)?;
                        self.invalidate_all_frames();
                    }
                }
            }
            (Lifecycle::AwaitingFirstBuffer, true) => {
                // The size is known now, so the placement policy can use it: default placement
                // is resolved at the element's first windowed presentation. A map in protocol
                // fullscreen is not one and leaves the opportunity for later; a remap of a
                // placed element keeps its anchor (INV-STAGE-5).
                let remap = self.stage.default_placed(id);
                if self.fullscreen_of(id).is_none() {
                    self.place_by_default(id, &tree)?;
                } else if !remap && let Some(at) = self.fullscreen_map_spot(id, &tree) {
                    self.stage.place(id, at).map_err(stale)?;
                }
                // a window shown again (Discord back from the tray, a game restarting its window)
                // keeps its place unless another window took it meanwhile: then it moves to the
                // nearest free spot instead of opening over that window (owner, 2026-11-16)
                if remap && let Some(at) = self.free_place_on_remap(id, &tree) {
                    self.stage.place(id, at).map_err(stale)?;
                    self.invalidate_all_frames();
                }
                self.stage.set_tree(id, Some(tree)).map_err(stale)?;
                self.stage.set_state(id, Lifecycle::Mapped).map_err(stale)?;
                self.stage.raise(id);
                self.opening = Some((id, self.focus.target()));
                if self.key_opened {
                    self.key_opened = false;
                    self.keyed_focus = Some(id);
                }
                // a grant made before this map takes its output now (A-3)
                out.extend(self.take_output(id));
            }
            (Lifecycle::Mapped, false) => {
                // null-buffer commit: unmap; identity and anchor kept, snapshot dropped
                let rest = self.cluster_left_by(id);
                self.cancel_element_touches(id, out);
                self.input.interaction.orphan_element(id);
                if self.resize.is_some_and(|r| r.element == id) {
                    self.resize = None;
                }
                self.stage.set_tree(id, None).map_err(stale)?;
                self.stage
                    .set_state(id, Lifecycle::Unmapped)
                    .map_err(stale)?;
                // the successor is chosen among the elements focus may move to (A-8)
                let live: Vec<ElementId> = self
                    .focusable_elements()
                    .into_iter()
                    .filter(|&e| e != id)
                    .collect();
                // M1: "fullscreen dropped (M9)"; the client must request it again after a remap
                self.drop_fullscreen(id);
                out.extend(self.lose_focus_target(id, &live));
                self.tidy_after_leave(&rest, out);
            }
        }
        self.membership_pass_element(id, out);
        // A presentation that started or ended in this commit (a fullscreen element's first map;
        // an older grant replaced), and any commit of a presented element: its view depends on
        // its geometry (F-PRESENT-1).
        let mut touched = std::mem::take(&mut self.touched_buf);
        touched.clear();
        touched.extend(
            before
                .iter()
                .filter(|(o, p)| self.presentation(*o) != *p)
                .map(|(o, _)| *o),
        );
        // a presentation that started or ended is a non-client stimulus (C14.4 SC-E); a commit
        // alone is a client response
        let presentation_moved = !touched.is_empty();
        if presentation_moved {
            // C12.F13: classified per element
            self.presentation_stimulus(&before);
        }
        let client_response = true;
        if let Some(fs) = self.fullscreen_of(id)
            && self.presentation(fs.output) == Some(id)
            && !touched.contains(&fs.output)
        {
            touched.push(fs.output);
        }
        if touched.is_empty() {
            out.extend(self.reconcile_constraints());
        } else {
            self.presentation_membership(&touched, out);
        }
        self.touched_buf = touched;
        self.presentations_buf = before;
        out.extend(self.evaluate_scale_policy(client_response));
        if presentation_moved {
            Self::hints_before_configures(&mut out[start..]);
        }
        Ok(())
    }

    /// A change of a mapped element's surface tree that is NOT a commit of its root surface: a
    /// child surface (popup, desynchronised subsurface) committed, or a popup was destroyed. It
    /// replaces the snapshot of a Mapped element (hit-testing and rendering follow, INV-HIT-1) and
    /// never drives M1: only the root surface's own commit (`toplevel_committed`) may map, unmap
    /// or configure an element. An element that is not Mapped has no hit-testable tree, so the
    /// change is ignored until its root commits. A rejected tree changes nothing.
    pub fn toplevel_tree_changed(
        &mut self,
        id: ElementId,
        tree: SurfaceTree,
    ) -> Result<Vec<Effect>, ProtocolError> {
        tree.validate().map_err(ProtocolError::Snapshot)?;
        let state = self.stage.state(id).ok_or(ProtocolError::StaleElement)?;
        // OBJ-17: a child surface's commit may leave a callback pending (M16)
        self.frame_liveness.committed(id, tree.frame_requested());
        // The root's mapped state is committed state of the root surface; only its own commit
        // changes it, so a tree without a mapped root is never applied here.
        if state != Lifecycle::Mapped || !tree.root().is_some_and(|r| r.mapped) {
            return Ok(Vec::new());
        }
        self.stage
            .set_tree(id, Some(tree))
            .map_err(|_| ProtocolError::StaleElement)?;
        // P7 for the new tree is the first rule of the batch-end evaluation (ScaleState::step)
        let mut fx = Vec::new();
        self.membership_pass_element(id, &mut fx);
        // the presentation view of a fullscreen element depends on its geometry (F-PRESENT-1)
        match self.fullscreen_of(id) {
            Some(fs) if self.presentation(fs.output) == Some(id) => {
                self.presentation_membership(&[fs.output], &mut fx)
            }
            _ => fx.extend(self.reconcile_constraints()),
        }
        fx.extend(self.evaluate_scale_policy(true));
        Ok(fx)
    }

    /// M1 destroy: purges every reference; idempotent and order-independent (INV-STAGE-4). The
    /// dead client gets no leave: its membership rows are dropped silently. Target loss triggers
    /// successor focus transition (K7, M11).
    pub fn toplevel_destroyed(&mut self, id: ElementId) -> Vec<Effect> {
        // destroyed while mapped (an unmap first tidied already)
        let rest = self.cluster_left_by(id);
        self.remember_closed(id);
        self.maximize_on_map.retain(|&e| e != id);
        self.scales.remove(id);
        // its own parent link, and its children's (they have no parent any more)
        self.toplevel_parents.retain(|(c, p)| *c != id && *p != id);
        // a drag on it ends with the release; nothing is sized or moved any more
        self.input.interaction.orphan_element(id);
        if self.resize.is_some_and(|r| r.element == id) {
            self.resize = None;
        }
        // only the destroyed element's own constraints: another surface's persist (its object lives)
        self.pending_constraints
            .retain(|c| self.stage.element_containing_surface(c.surface) != Some(id));
        for slot in &mut self.frames {
            slot.frame.entries.retain(|e| e.element != id);
        }
        let before = self.presentations();
        let mut fx = Vec::new();
        self.cancel_element_touches(id, &mut fx);
        let existed = self.stage.destroy(id);
        if !existed {
            return Vec::new();
        }
        self.frame_liveness.destroyed(id);
        let live: Vec<ElementId> = self.focusable_elements();
        fx.extend(self.lose_focus_target(id, &live));
        self.drop_fullscreen(id);
        self.tidy_after_leave(&rest, &mut fx);
        let ended: Vec<OutputId> = before
            .iter()
            .filter(|(_, p)| *p == Some(id))
            .map(|(o, _)| *o)
            .collect();
        if !ended.is_empty() {
            // C12.F13: the elements it revealed may rise, not fall
            self.presentation_stimulus(&before);
            self.presentation_membership(&ended, &mut fx);
        }
        // Evaluated as client-caused: the destruction of an element is not one of the
        // non-client stimuli of SC-E (C14.4), so it must not reverse a hint another element
        // is holding (INV-SCALE-4 d). The end of a presentation it held is one (fullscreen exit).
        let mut vis = Vec::new();
        self.membership_pass_element(id, &mut vis);
        vis.extend(self.evaluate_scale_policy(true));
        // The pass also evaluates the scale policy of the surviving elements (INV-SCALE-5), so
        // it may return a hint that a client-caused evaluation is allowed to move. The destroyed
        // element itself has no row left to diff.
        debug_assert!(
            !vis.iter()
                .any(|e| matches!(e, Effect::Enter { .. } | Effect::Leave { .. })),
            "a destroyed element produced enter or leave effects"
        );
        fx.extend(vis);
        fx.extend(self.reconcile_constraints());
        fx
    }

    /// Cancels touches targeting `id` (unmapped or destroyed while a finger is down).
    fn cancel_element_touches(&mut self, id: ElementId, out: &mut Vec<Effect>) {
        if self
            .touches
            .window
            .as_ref()
            .is_some_and(|g| g.element == id)
        {
            self.touches.end_window();
        }
        let on_element = self.touches.any_point(|tp| {
            tp.anchor == crate::touch::Anchor::Canvas
                && self.stage.element_containing_surface(tp.target.surface) == Some(id)
        });
        if on_element && self.touches.clear() {
            out.push(Effect::TouchCancel);
        }
        self.touch_resize_settle(out);
    }

    /// M3: the constraint target for a popup of `parent`: the anchor output's viewport,
    /// inward-rounded (R8), relative to the parent's window-geometry origin. The anchor output is
    /// (D-B6-BOUNDARY):
    /// - Rule 1: output on which causing input event occurred, if parent is visible on it;
    /// - Rule 2: else parent's frame-clock output (highest refresh observing output);
    /// - Rule 3: else none (`None`: no constraint adjustment).
    ///
    /// `None` also when the target does not fit protocol integers.
    pub fn popup_constraint_target(
        &self,
        parent: ElementId,
        causing_output: Option<OutputId>,
    ) -> Option<LocalRect> {
        let anchor = self.stage.anchor(parent)?;
        // C5.2: the popups of a fullscreen element are constrained to its fullscreen output
        let presented = self
            .fullscreen_of(parent)
            .map(|f| f.output)
            .filter(|o| self.presentation(*o) == Some(parent));
        let output = match (presented, causing_output) {
            (Some(o), _) => o,
            (None, Some(o)) if self.membership.observers(parent).contains(&o) => o,
            _ => self.clock_output_of(parent)?,
        };
        let view = self.view(output)?;
        let extent = |px: u32| crate::geom::q16(px as f64 / view.k);
        let (x0q, y0q) = (view.c_eff.x, view.c_eff.y);
        let (x1q, y1q) = (x0q + extent(view.size.0), y0q + extent(view.size.1));
        // R8: ceiling at the minimum edges, floor at the maximum edges
        let ceil = |v: i64| -(-v).div_euclid(Q16_ONE);
        let floor = |v: i64| v.div_euclid(Q16_ONE);
        let to_i32 = |v: i64| i32::try_from(v).ok();
        let x0 = to_i32(ceil(x0q) - anchor.x)?;
        let y0 = to_i32(ceil(y0q) - anchor.y)?;
        let x1 = to_i32(floor(x1q) - anchor.x)?;
        let y1 = to_i32(floor(y1q) - anchor.y)?;
        Some(LocalRect {
            loc: LocalPoint { x: x0, y: y0 },
            size: LocalSize {
                w: x1.checked_sub(x0)?.max(0),
                h: y1.checked_sub(y0)?.max(0),
            },
        })
    }

    /// An app's minimize button (`xdg_toplevel.set_minimized`; an X11 app's through the bridge):
    /// the window is hidden, still open, until it is picked again (the windows list, Super+W's
    /// apps, or the app asking for it). Only the focused window, as its maximize button, and
    /// never a fullscreen one: Wine asks to minimize a game when it loses the focus (the black
    /// screens of 2026-11-02), which is then not the focused window and is left alone.
    pub fn client_minimize(&mut self, id: ElementId) -> Vec<Effect> {
        let mut out = Vec::new();
        if self.focus.target() != Some(id)
            || self.stage.mapped(id).is_none()
            || self.fullscreen_of(id).is_some()
            || self.input.interaction.active()
        {
            return out;
        }
        let rest = self.cluster_left_by(id);
        if self.stage.set_minimized(id, true).is_err() {
            return out;
        }
        let live: Vec<ElementId> = self
            .focusable_elements()
            .into_iter()
            .filter(|&e| e != id)
            .collect();
        out.extend(self.lose_focus_target(id, &live));
        self.invalidate_all_frames();
        self.membership_pass_element(id, &mut out);
        out.extend(self.visibility_pass());
        // the window now focused may have asked for a pointer lock or confine: it starts now, not
        // at the next motion (AUDIT_FOCUS_HOVER improvement 10)
        out.extend(self.reconcile_constraints());
        self.tidy_after_leave(&rest, &mut out);
        out
    }

    /// A client asked for the compositor's window menu (`xdg_toplevel.show_window_menu`) at
    /// `location` in `id`'s window geometry, or the user triggered `KeyAction::WindowMenu`.
    /// Records the mapped window and its output-logical screen position `(x, y)` for the shell.
    /// Only the focused window may ask: the request answers the user's own click, and a window
    /// in the background must not open menus over what the user is doing.
    pub fn show_window_menu(&mut self, id: ElementId, location: LocalPoint) {
        if self.focus.target() != Some(id) {
            return;
        }
        let Some(anchor) = self.stage.anchor(id) else {
            return;
        };
        if self.stage.mapped(id).is_none() {
            return;
        }
        let q = CanvasPos {
            x: anchor
                .x
                .saturating_add(i64::from(location.x))
                .saturating_mul(Q16_ONE),
            y: anchor
                .y
                .saturating_add(i64::from(location.y))
                .saturating_mul(Q16_ONE),
        };
        let output = self
            .pointer_output()
            .or_else(|| self.clock_output_of(id))
            .or_else(|| self.outputs.first().map(|o| o.id));
        let pos = output
            .and_then(|o| self.view(o))
            .map(|v| {
                let p = v.project(q);
                (p.x, p.y)
            })
            .unwrap_or((f64::from(location.x), f64::from(location.y)));
        self.window_menu = Some((id, pos));
    }

    /// The effect that ends a client's popup grab made with the press its window menu opened on
    /// (the adapter matches the press: `show_window_menu`).
    pub fn end_popup_grab_for_window_menu(&self, id: ElementId) -> Vec<Effect> {
        vec![Effect::DismissPopupGrab(id)]
    }

    /// A minimized window comes back where it was, on top.
    pub(crate) fn restore_minimized(&mut self, id: ElementId, out: &mut Vec<Effect>) {
        if self.stage.set_minimized(id, false).is_err() {
            return;
        }
        self.stage.raise(id);
        self.invalidate_all_frames();
        self.membership_pass_element(id, &mut *out);
        out.extend(self.visibility_pass());
    }

    /// The adapter's report of an element's `xdg_toplevel` parent (`None`: none), before each
    /// commit of its root.
    pub fn set_toplevel_parent(&mut self, child: ElementId, parent: Option<ElementId>) {
        self.toplevel_parents.retain(|(c, _)| *c != child);
        if let Some(p) = parent.filter(|p| *p != child) {
            self.toplevel_parents.push((child, p));
        }
    }

    /// An element's `xdg_toplevel` parent, if it has one.
    pub fn toplevel_parent(&self, child: ElementId) -> Option<ElementId> {
        self.toplevel_parents
            .iter()
            .find(|(c, _)| *c == child)
            .map(|(_, p)| *p)
    }

    #[cfg(test)]
    pub fn placement_policy(&self) -> PlacementPolicy {
        self.placement
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation::PersistedCameras;
    use crate::output::{OutputDescriptor, Transform};
    use crate::protocol::tests::*;
    use crate::snapshot::tests::key;

    /// B1: a surface-tree change that is not a root commit never drives M1 — in every lifecycle
    /// state the state, anchor and effects are those of a no-op, except that a Mapped element's
    /// snapshot is replaced (literal expectations, independent of `toplevel_committed`).
    #[test]
    fn tree_change_never_drives_the_lifecycle() {
        let mut core = connected(120);
        let id = core.toplevel_created();
        // AwaitingInitialCommit: no placement, no configure
        assert_eq!(core.toplevel_tree_changed(id, tree(false)), Ok(vec![]));
        assert_eq!(core.toplevel_tree_changed(id, tree(true)), Ok(vec![]));
        assert_eq!(
            core.stage().state(id),
            Some(Lifecycle::AwaitingInitialCommit)
        );
        assert_eq!(core.stage().anchor(id), None);
        core.toplevel_committed(id, tree(false)).unwrap();
        // AwaitingFirstBuffer: a mapped-looking tree does not map it
        assert_eq!(core.toplevel_tree_changed(id, tree(true)), Ok(vec![]));
        assert_eq!(core.stage().state(id), Some(Lifecycle::AwaitingFirstBuffer));
        core.toplevel_committed(id, tree(true)).unwrap();
        // Mapped: an unmapped-looking tree does not unmap it; a mapped tree replaces the snapshot
        assert_eq!(core.toplevel_tree_changed(id, tree(false)), Ok(vec![]));
        assert_eq!(core.stage().state(id), Some(Lifecycle::Mapped));
        assert!(core.stage().tree(id).is_some());
        assert_eq!(core.toplevel_tree_changed(id, tree(true)), Ok(vec![]));
        assert_eq!(core.stage().state(id), Some(Lifecycle::Mapped));
        core.toplevel_committed(id, tree(false)).unwrap();
        // Unmapped: no re-arming of the initial configure, and the snapshot stays dropped even for
        // a mapped-looking tree (M1: an unmapped element has no hit-testable tree)
        assert_eq!(core.toplevel_tree_changed(id, tree(false)), Ok(vec![]));
        assert_eq!(core.toplevel_tree_changed(id, tree(true)), Ok(vec![]));
        assert_eq!(core.stage().state(id), Some(Lifecycle::Unmapped));
        assert!(core.stage().tree(id).is_none());
        // a destroyed element is stale
        core.toplevel_destroyed(id);
        assert_eq!(
            core.toplevel_tree_changed(id, tree(true)),
            Err(ProtocolError::StaleElement)
        );
    }

    #[test]
    fn m1_map_unmap_remap_destroy() {
        let mut core = connected(120);
        let o = core.outputs()[0].id;
        let enter = Effect::Enter {
            output: o,
            surface: key(1),
        };
        let leave = Effect::Leave {
            output: o,
            surface: key(1),
        };
        let id = core.toplevel_created();
        assert_eq!(
            core.stage().state(id),
            Some(Lifecycle::AwaitingInitialCommit)
        );
        assert_eq!(core.stage().anchor(id), None);
        assert_eq!(
            core.toplevel_committed(id, tree(false)),
            Ok(vec![
                Effect::PreferredScale {
                    element: id,
                    scale_120: 120,
                    integer_scale: 1,
                },
                Effect::InitialConfigure(id),
            ])
        );
        // INV-STAGE-1 (C12.F1): a provisional anchor at the initial commit, the camera centre
        let provisional = core.stage().anchor(id).expect("placed at initial commit");
        assert_eq!(provisional, CanvasPoint { x: 0, y: 0 }, "camera centre");
        assert_eq!(core.toplevel_committed(id, tree(false)), Ok(vec![]));
        // first buffer: mapped, on top, and its surface enters the output (F-MEMBER-1)
        assert_eq!(core.toplevel_committed(id, tree(true)), Ok(vec![enter]));
        // default placement resolved at the first map: 100x50 centred on (0,0)
        let anchor = core.stage().anchor(id).expect("placed");
        assert_eq!(anchor, CanvasPoint { x: -50, y: -25 }, "F-PLACE-1");
        assert_eq!(core.stage().state(id), Some(Lifecycle::Mapped));
        assert_eq!(core.stage().z_order(), &[id]);
        assert_eq!(
            core.toplevel_committed(id, tree(true)),
            Ok(vec![]),
            "no repeated enter"
        );
        // unmap keeps id and anchor, drops the snapshot, and leaves every output (M1)
        assert_eq!(core.toplevel_committed(id, tree(false)), Ok(vec![leave]));
        assert_eq!(core.stage().state(id), Some(Lifecycle::Unmapped));
        assert_eq!(core.stage().anchor(id), Some(anchor));
        assert!(core.stage().tree(id).is_none());
        // remap: initial commit again, anchor kept, on top
        let other = core.toplevel_created();
        core.toplevel_committed(other, tree(false)).unwrap();
        core.toplevel_committed(other, tree(true)).unwrap();
        assert_eq!(
            core.toplevel_committed(id, tree(false)),
            Ok(vec![Effect::InitialConfigure(id)])
        );
        assert_eq!(core.toplevel_committed(id, tree(true)), Ok(vec![enter]));
        assert_eq!(core.stage().anchor(id), Some(anchor));
        assert_eq!(
            core.stage().z_order(),
            &[other, id],
            "remap re-shows on top"
        );
        // destroy: no event to the dead client; membership rows dropped
        let _fx = core.toplevel_destroyed(id);
        assert!(!core.membership().rows().iter().any(|(e, _, _)| *e == id));
        assert!(
            core.toplevel_destroyed(id).is_empty(),
            "destroy is idempotent"
        );
        assert_eq!(
            core.toplevel_committed(id, tree(true)),
            Err(ProtocolError::StaleElement)
        );
    }

    #[test]
    fn m1_illegal_transitions_are_rejected_without_mutation() {
        let mut core = connected(120);
        let id = core.toplevel_created();
        assert_eq!(
            core.toplevel_committed(id, tree(true)),
            Err(ProtocolError::BufferBeforeConfigure)
        );
        assert_eq!(
            core.stage().state(id),
            Some(Lifecycle::AwaitingInitialCommit)
        );
        assert_eq!(core.stage().anchor(id), None);
        let mut bad = tree(false);
        bad.surfaces.clear();
        assert!(matches!(
            core.toplevel_committed(id, bad),
            Err(ProtocolError::Snapshot(_))
        ));
        assert_eq!(
            core.stage().state(id),
            Some(Lifecycle::AwaitingInitialCommit)
        );
        // after unmap, a buffer before the new initial configure is rejected too
        core.toplevel_committed(id, tree(false)).unwrap();
        core.toplevel_committed(id, tree(true)).unwrap();
        core.toplevel_committed(id, tree(false)).unwrap();
        assert_eq!(
            core.toplevel_committed(id, tree(true)),
            Err(ProtocolError::BufferBeforeConfigure)
        );
        assert_eq!(core.stage().state(id), Some(Lifecycle::Unmapped));
    }

    /// Protocol activity never touches cameras (INV-CAMERA-1).
    #[test]
    fn protocol_events_never_move_cameras() {
        let mut core = connected(150);
        let before = (
            core.cameras().center(0),
            core.cameras().zoom(0),
            core.cameras().len(),
        );
        let id = core.toplevel_created();
        core.toplevel_committed(id, tree(false)).unwrap();
        core.toplevel_committed(id, tree(true)).unwrap();
        core.toplevel_destroyed(id);
        assert_eq!(
            before,
            (
                core.cameras().center(0),
                core.cameras().zoom(0),
                core.cameras().len()
            )
        );
    }

    /// M3 / R8 / D-B6-BOUNDARY: the target is the anchor output's viewport (here the parent's
    /// clock output), inward-rounded, relative to the parent's geometry origin (the anchor).
    /// Expected values are hand-derived with exact rational arithmetic, not with production code
    /// (docs/archive/evidence/phase-f/INC-1/corrective/unproject_reference.py).
    #[test]
    fn popup_target_is_the_viewport_inward_rounded() {
        // k = 1, camera centre (0,0), 1920x1080 → viewport [−960,960) × [−540,540). The target is
        // parent-relative: before the first map the anchor is the camera centre; after it the
        // 100x50 window is centred, anchor (−50,−25), and the viewport is at (−910,−515)
        let mut core = connected(120);
        let id = core.toplevel_created();
        core.toplevel_committed(id, tree(false)).unwrap();
        assert_eq!(
            core.popup_constraint_target(id, None),
            None,
            "unobserved parent: no anchor output (rule 3)"
        );
        core.toplevel_committed(id, tree(true)).unwrap();
        let t = core.popup_constraint_target(id, None).unwrap();
        assert_eq!(
            (t.loc, t.size),
            (
                LocalPoint { x: -910, y: -515 },
                LocalSize { w: 1920, h: 1080 }
            )
        );
        // k = 1.75: c_eff = (−35951177, −20222537) Q16; the viewport's fractional edges
        // (−548.57…, 548.57…) × (−308.57…, 308.57…) round inward to [−548, 548) × [−308, 308);
        // relative to the anchor (−50,−25) that is (−498,−283)
        let mut core = connected(210);
        let id = core.toplevel_created();
        core.toplevel_committed(id, tree(false)).unwrap();
        core.toplevel_committed(id, tree(true)).unwrap();
        let t = core.popup_constraint_target(id, None).unwrap();
        assert_eq!(
            (t.loc, t.size),
            (
                LocalPoint { x: -498, y: -283 },
                LocalSize { w: 1096, h: 616 }
            )
        );
        core.toplevel_destroyed(id);
        assert_eq!(
            core.popup_constraint_target(id, None),
            None,
            "a destroyed parent has no target"
        );
    }

    #[test]
    fn popup_target_causing_output_rule1_and_rule2_fallback() {
        use crate::navigation::PersistedCameras;
        use crate::output::tests::key as okey;

        // Setup outputs: o1, o2, and o3 (with far-away camera)
        let persisted = PersistedCameras::from_entries(vec![(
            okey("out3"),
            crate::geom::CanvasPos {
                x: 100_000 << 16,
                y: 100_000 << 16,
            },
            1.0,
        )])
        .unwrap();
        let (mut core, _, _, mut tok) = Core::new(persisted);
        let (o1, _) = core
            .output_connected(
                &mut tok,
                OutputDescriptor {
                    key: okey("out1"),
                    mode: (1920, 1080),
                    transform: Transform::Normal,
                    scale_120: 120,
                    refresh_mhz: 60_000,
                },
            )
            .unwrap();
        let (o2, _) = core
            .output_connected(
                &mut tok,
                OutputDescriptor {
                    key: okey("out2"),
                    mode: (800, 600),
                    transform: Transform::Normal,
                    scale_120: 120,
                    refresh_mhz: 120_000, // higher refresh -> would be clock output (rule 2)
                },
            )
            .unwrap();
        let (o3, _) = core
            .output_connected(
                &mut tok,
                OutputDescriptor {
                    key: okey("out3"),
                    mode: (640, 480),
                    transform: Transform::Normal,
                    scale_120: 120,
                    refresh_mhz: 30_000,
                },
            )
            .unwrap();

        let id = core.toplevel_created();
        core.toplevel_committed(id, tree(false)).unwrap();
        core.toplevel_committed(id, tree(true)).unwrap();

        // If parent is visible on both o1 and o2:
        // Rule 1: causing_output = o1 selects o1, even though o2 has higher refresh.
        if core.membership().observers(id).contains(&o1)
            && core.membership().observers(id).contains(&o2)
        {
            let t_o1 = core.popup_constraint_target(id, Some(o1)).unwrap();
            let t_o2 = core.popup_constraint_target(id, Some(o2)).unwrap();
            assert_eq!(t_o1.size, LocalSize { w: 1920, h: 1080 });
            assert_eq!(t_o2.size, LocalSize { w: 800, h: 600 });
        }

        // Rule 2 fallback: None selects clock output (o2)
        if core.membership().observers(id).contains(&o2) {
            let t_default = core.popup_constraint_target(id, None).unwrap();
            assert_eq!(t_default.size, LocalSize { w: 800, h: 600 });
        }

        // Rule 1 visibility guard (ATK-12): o3 does not observe parent id.
        // Passing Some(o3) does not select o3 (640x480), but falls back to Rule 2 (o2, 800x600).
        assert!(!core.membership().observers(id).contains(&o3));
        let t_unobserved = core.popup_constraint_target(id, Some(o3)).unwrap();
        assert_eq!(t_unobserved.size, LocalSize { w: 800, h: 600 });
    }

    /// M2 as a table for an element that does not hold the focus: for every request in every
    /// lifecycle state, the decision and the effects. Stage, cameras and focus never change.
    #[test]
    fn toplevel_requests_are_answered_by_policy_and_change_nothing() {
        use ToplevelDecision::*;
        use ToplevelRequest::*;
        let reach = [
            Lifecycle::AwaitingInitialCommit,
            Lifecycle::AwaitingFirstBuffer,
            Lifecycle::Mapped,
            Lifecycle::Unmapped,
        ];
        for state in reach {
            for request in ToplevelRequest::ALL {
                let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
                out(&mut c, &mut t, "A", (1920, 1080), 120);
                let id = c.toplevel_created();
                if state != Lifecycle::AwaitingInitialCommit {
                    c.toplevel_committed(id, win(1, 100, 50, false)).unwrap();
                }
                if matches!(state, Lifecycle::Mapped | Lifecycle::Unmapped) {
                    c.toplevel_committed(id, win(1, 100, 50, true)).unwrap();
                }
                if state == Lifecycle::Unmapped {
                    c.toplevel_committed(id, win(1, 100, 50, false)).unwrap();
                }
                // another window opened after it and has the focus: `id` is not opening any more
                // (a window asking as it opens is granted, see the headless t1_start_fullscreen)
                let other = map_win(&mut c, 9, 300, 200);
                focus(&mut c, &mut i, other);
                assert_eq!(c.stage().state(id), Some(state));
                let configured =
                    matches!(state, Lifecycle::AwaitingFirstBuffer | Lifecycle::Mapped);
                let want = match (request, state) {
                    (SetFullscreen { .. }, Lifecycle::AwaitingInitialCommit) => Recorded,
                    // before the first map: granted without the focus
                    (SetFullscreen { .. }, Lifecycle::AwaitingFirstBuffer) => Granted,
                    (SetMaximized | UnsetMaximized | SetFullscreen { .. } | UnsetFullscreen, _)
                        if configured =>
                    {
                        Declined
                    }
                    _ => Ignored,
                };
                let before = authority_state(&c);
                let (decision, fx) = c.toplevel_request(id, request).unwrap();
                assert_eq!(decision, want, "{request:?} in {state:?}");
                assert_eq!(
                    fx,
                    match want {
                        Granted | Declined => vec![Effect::ToplevelConfigure(id)],
                        Released => unreachable!(),
                        Recorded | Ignored => vec![],
                    },
                    "{request:?} in {state:?}"
                );
                assert_eq!(authority_state(&c), before, "{request:?} in {state:?}");
                assert_eq!(
                    c.fullscreen_of(id).is_some(),
                    want == Granted,
                    "{request:?} in {state:?}: stored state"
                );
            }
        }
    }

    /// A request on a destroyed element is stale, whatever it asks.
    #[test]
    fn a_toplevel_request_on_a_destroyed_element_is_stale() {
        let mut c = connected(120);
        let id = map_win(&mut c, 1, 100, 50);
        c.toplevel_destroyed(id);
        for request in ToplevelRequest::ALL {
            assert_eq!(
                c.toplevel_request(id, request),
                Err(ProtocolError::StaleElement)
            );
        }
    }

    /// What is advertised is what the policy can grant: fullscreen, and nothing else. Found by
    /// trying every request in every lifecycle state, with and without the focus.
    #[test]
    fn advertised_capabilities_are_exactly_what_the_policy_grants() {
        let mut granted = Vec::new();
        for focused in [false, true] {
            for stage in 0..4 {
                for request in ToplevelRequest::ALL {
                    let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
                    out(&mut c, &mut t, "A", (1920, 1080), 120);
                    let id = c.toplevel_created();
                    if stage >= 1 {
                        c.toplevel_committed(id, win(1, 100, 50, false)).unwrap();
                    }
                    if stage >= 2 {
                        c.toplevel_committed(id, win(1, 100, 50, true)).unwrap();
                        if focused {
                            focus(&mut c, &mut i, id);
                        }
                    }
                    if stage == 3 {
                        c.toplevel_committed(id, win(1, 100, 50, false)).unwrap();
                    }
                    if c.toplevel_request(id, request).unwrap().0 == ToplevelDecision::Granted {
                        granted.push(request);
                    }
                }
            }
        }
        granted.dedup();
        assert!(granted.iter().all(|r| *r == FS), "{granted:?}");
        let caps = connected(120).toplevel_capabilities();
        assert_eq!(caps.fullscreen, granted.contains(&FS));
        assert!(caps.maximize, "answered by Core::client_maximize, not here");
        assert!(caps.minimize, "answered by Core::client_minimize, not here");
        assert!(
            caps.window_menu,
            "answered by Core::show_window_menu, not here"
        );
        assert_eq!(
            caps,
            ToplevelCapabilities {
                fullscreen: true,
                maximize: true,
                minimize: true,
                window_menu: true,
            }
        );
    }

    /// The cameras and anchors: what an activation must never change (INV-INPUT-9, B10.7).
    fn spatial_state(c: &Core) -> String {
        let cams: Vec<_> = (0..c.cameras().len())
            .map(|i| (c.cameras().center(i), c.cameras().zoom(i)))
            .collect();
        let anchors: Vec<_> = c
            .stage()
            .z_order()
            .iter()
            .map(|id| c.stage().anchor(*id))
            .collect();
        format!("{cams:?} {anchors:?}")
    }

    /// INV-INPUT-9 (PROV-10): a validated activation of a window that may
    /// take focus is a focus grant: the focus moves, with its configures, and no camera or
    /// anchor changes.
    #[test]
    fn an_activation_grants_focus_and_moves_nothing_spatial() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        out(&mut c, &mut t, "A", (1920, 1080), 120);
        let a = map_win(&mut c, 1, 400, 300);
        let w = map_win(&mut c, 2, 300, 200);
        focus(&mut c, &mut i, a);
        let before = spatial_state(&c);
        let fx = c.activation_request(w, true).unwrap();
        assert_eq!(c.focus().target(), Some(w));
        assert!(fx.contains(&Effect::KeyboardFocus(Some(w))), "{fx:?}");
        assert!(
            fx.contains(&Effect::ToplevelConfigure(a)),
            "a loses `activated`"
        );
        assert_eq!(spatial_state(&c), before);
        assert!(c.focus().attention().is_empty());
        assert!(
            c.activation_request(w, true).unwrap().is_empty(),
            "already focused: nothing"
        );
    }

    /// INV-INPUT-8 + INV-INPUT-9: a window hidden by a presentation may not take focus
    /// (A-8), so its activation asks for attention instead; focus stays; the attention ends when
    /// the window gets focus by an explicit action.
    #[test]
    fn an_activation_of_a_hidden_window_asks_for_attention() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let x = map_win(&mut c, 9, 300, 200);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(a) })
            .unwrap();
        assert!(c.hidden_by_presentation(x));
        let before = spatial_state(&c);
        let fx = c.activation_request(x, true).unwrap();
        assert!(fx.is_empty(), "{fx:?}");
        assert_eq!(c.focus().target(), Some(w), "focus stays");
        assert_eq!(c.focus().attention(), [x]);
        assert!(c.activation_request(x, true).unwrap().is_empty());
        assert_eq!(c.focus().attention(), [x], "marked once");
        assert_eq!(spatial_state(&c), before);
        c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        focus(&mut c, &mut i, x);
        assert!(c.focus().attention().is_empty(), "focused: no attention");
    }

    /// An activation of an element that is not mapped is ignored; of a destroyed one,
    /// refused as stale; an element that asked for attention and is destroyed leaves no trace.
    #[test]
    fn an_activation_of_an_unmapped_or_destroyed_window_changes_nothing() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let w = map_win(&mut c, 1, 400, 300);
        let u = c.toplevel_created();
        assert!(
            c.activation_request(u, true).unwrap().is_empty(),
            "not mapped"
        );
        assert_eq!(c.focus().target(), None);
        let x = map_win(&mut c, 9, 300, 200);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(a) })
            .unwrap();
        c.activation_request(x, true).unwrap();
        assert_eq!(c.focus().attention(), [x]);
        c.toplevel_destroyed(x);
        assert!(
            c.focus().attention().is_empty(),
            "no attention for the dead"
        );
        assert_eq!(
            c.activation_request(x, true),
            Err(ProtocolError::StaleElement)
        );
    }

    /// Activation policy: under the default `Strict` policy only a validated
    /// request focuses; an unvalidated one (no token from the focused client) only asks for
    /// attention. `AttentionOnly` never focuses; `Permissive` focuses whatever the token. No
    /// policy moves a camera or an anchor.
    #[test]
    fn the_activation_policy_decides_who_may_take_focus() {
        use crate::input::focus::ActivationPolicy;
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        out(&mut c, &mut t, "A", (1920, 1080), 120);
        let a = map_win(&mut c, 1, 400, 300);
        let w = map_win(&mut c, 2, 300, 200);
        focus(&mut c, &mut i, a);
        let before = spatial_state(&c);
        assert_eq!(
            c.focus().activation_policy(),
            ActivationPolicy::Strict,
            "default"
        );
        assert!(c.activation_request(w, false).unwrap().is_empty());
        assert_eq!(
            (c.focus().target(), c.focus().attention()),
            (Some(a), &[w][..])
        );
        c.set_activation_policy(ActivationPolicy::AttentionOnly);
        assert!(c.activation_request(w, true).unwrap().is_empty());
        assert_eq!(c.focus().target(), Some(a), "never under AttentionOnly");
        c.set_activation_policy(ActivationPolicy::Permissive);
        c.activation_request(w, false).unwrap();
        assert_eq!(c.focus().target(), Some(w), "any request under Permissive");
        assert!(c.focus().attention().is_empty());
        assert_eq!(spatial_state(&c), before);
    }
}
