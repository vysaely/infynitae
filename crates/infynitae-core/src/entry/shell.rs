//! Shell commands (`infynitae msg`): validation and execution (C7.2, INV-SHELL-1).

use super::*;

impl Core {
    /// C7.2 / INV-SHELL-1: one semantic command of the trusted shell, validated in full before
    /// anything is written (C7.3: atomic; an error changes nothing) and executed through the same
    /// single-writer paths user input uses. A command that moves a camera consumes a proof minted
    /// here, per command (INV-CAMERA-1). Effects are returned for the adapter to apply.
    pub fn shell_command(
        &mut self,
        port: &mut ShellPort,
        cmd: ShellCommand,
    ) -> Result<Vec<Effect>, ShellError> {
        if port.core_id != self.id {
            return Err(ShellError::ForeignPort);
        }
        let mut out = Vec::new();
        match cmd {
            ShellCommand::Navigate {
                output,
                to,
                zoom,
                stream,
            } => {
                if let NavigateTo::Element(element) = to {
                    let id = self.shell_chosen(element, &mut out)?;
                    self.show_chosen(id, &mut out);
                }
                let camera = self.shell_camera(output)?;
                let center = self.navigate_center(camera, to)?;
                self.shell_stream_step(stream);
                let record = matches!(to, NavigateTo::Element(_) | NavigateTo::Canvas(_, _))
                    && matches!(stream, ShellStream::Atomic | ShellStream::Begin);
                if record {
                    let zoom = match zoom {
                        Some(z) => z.clamp(crate::geom::ZOOM_MIN, crate::geom::ZOOM_MAX),
                        None => self.cameras.zoom(camera).unwrap_or(1.0),
                    };
                    self.cameras.jump(
                        authority::Authorized::mint(),
                        camera,
                        crate::navigation::CameraValue { center, zoom },
                    );
                } else {
                    match zoom {
                        Some(z) => {
                            let z = z.clamp(crate::geom::ZOOM_MIN, crate::geom::ZOOM_MAX);
                            self.cameras.set_zoom_and_center(
                                authority::Authorized::mint(),
                                camera,
                                z,
                                center,
                            );
                        }
                        None => {
                            self.cameras
                                .navigate(authority::Authorized::mint(), camera, center);
                        }
                    }
                }
                self.camera_written(output, camera, &mut out);
                self.shell_stream_end(stream, &mut out);
            }
            ShellCommand::ViewportFit { output, element } => {
                let element = self
                    .stage
                    .resolve(element)
                    .map_err(|_| ShellError::StaleElement)?;
                self.shell_camera(output)?;
                self.fit_on(authority::Authorized::mint(), output, element, &mut out)?;
            }
            ShellCommand::ViewportRestore { output } => {
                let camera = self.shell_camera(output)?;
                if self.cameras.history(camera).is_none_or(|h| h.is_empty()) {
                    return Err(ShellError::Refused);
                }
                if self
                    .cameras
                    .glide_restore(authority::Authorized::mint(), camera, self.zoom_feel)
                {
                    self.overview_at.retain(|o| o.0 != output);
                    self.camera_written(output, camera, &mut out);
                }
            }
            ShellCommand::Focus { element } => {
                let id = self.shell_chosen(element, &mut out)?;
                // a window picked from the list is the user's choice: a fullscreen window steps
                // aside for it (owner, 2026-11-02; A-8 still holds for focus nobody asked for)
                self.show_chosen(id, &mut out);
                // and comes to the front: focused while buried, the window over it kept the clicks
                // where they overlap (AUDIT_PRESENTATION B11)
                self.raise_and_focus(id, &mut out);
            }
            ShellCommand::AcknowledgeAttention { element } => {
                let id = self
                    .stage
                    .resolve(element)
                    .map_err(|_| ShellError::StaleElement)?;
                self.focus.acknowledge_attention(id);
            }
            ShellCommand::MoveElement {
                element,
                to,
                stream,
            } => {
                let id = self.shell_mapped(element)?;
                if self.fullscreen_of(id).is_some() {
                    return Err(ShellError::Presenting);
                }
                let anchor = shell_canvas_pos(to)
                    .and_then(|p| p.round_to_point())
                    .ok_or(ShellError::InvalidArgument)?;
                self.shell_stream_step(stream);
                self.place_by_shell(id, anchor, &mut out)
                    .map_err(|_| ShellError::StaleElement)?;
                // what lies under the pointer may have changed: M10, as after a camera write
                if matches!(self.pointer, PointerState::Located { .. })
                    && !self.input.interaction.active()
                {
                    out.extend(self.reconcile_constraints());
                    if !self.locked() {
                        self.true_position_into(&mut out);
                    }
                }
                self.shell_stream_end(stream, &mut out);
            }
            ShellCommand::ResizeElement {
                element,
                size,
                maximized,
            } => {
                let id = self.shell_mapped(element)?;
                // a fullscreen window's size is the screen's
                let presenting = self.fullscreen_of(id).is_some();
                if presenting {
                    return Err(ShellError::Presenting);
                }
                let valid =
                    |s: (i32, i32)| (1..=32768).contains(&s.0) && (1..=32768).contains(&s.1);
                if !valid(size) || maximized.is_some_and(|(_, s)| !valid(s)) {
                    return Err(ShellError::InvalidArgument);
                }
                let mut size = crate::geom::LocalSize {
                    w: size.0,
                    h: size.1,
                };
                match maximized {
                    Some((at, before)) => {
                        let at = shell_canvas_pos(at)
                            .and_then(|p| p.round_to_point())
                            .ok_or(ShellError::InvalidArgument)?;
                        let output = self.outputs.first().map(|o| o.id);
                        let usable = output.and_then(|o| self.usable_area(o));
                        let (Some(output), Some((_, _, uw, uh))) = (output, usable) else {
                            return Err(ShellError::UnknownOutput);
                        };
                        // the usable area now, not the one it was saved at
                        size = crate::geom::LocalSize {
                            w: uw.round().max(1.0) as i32,
                            h: uh.round().max(1.0) as i32,
                        };
                        let before = crate::geom::LocalSize {
                            w: before.0,
                            h: before.1,
                        };
                        self.stage.set_saved_floating(id, Some((at, before)));
                        self.set_maximized(id, Some(output));
                        if let Some(anchor) = self.stage.anchor(id) {
                            self.maximized_at.retain(|(e, _)| *e != id);
                            self.maximized_at.push((id, anchor));
                        }
                    }
                    None => {
                        self.stage.set_saved_floating(id, None);
                        self.set_maximized(id, None);
                    }
                }
                out.push(Effect::ToplevelConfigureSize { element: id, size });
            }
            ShellCommand::FullscreenElement {
                element,
                output,
                fullscreen,
            } => {
                let id = self.shell_mapped(element)?;
                if fullscreen {
                    // A fullscreen window restored on a screen that no longer exists comes back normal.
                    let target_output = match output {
                        Some(o) => {
                            if !self.outputs.iter().any(|out| out.id == o) {
                                return Ok(out);
                            }
                            Some(o)
                        }
                        None => self
                            .pointer_output()
                            .or_else(|| self.outputs.first().map(|o| o.id)),
                    };
                    self.left_fullscreen.retain(|e| *e != id);
                    let request = crate::protocol::ToplevelRequest::SetFullscreen {
                        output: target_output,
                    };
                    if self.focus.target() != Some(id) {
                        out.extend(self.focus.set_focus_at(self.input.clock.now_us(), Some(id)));
                        out.extend(self.reconcile_constraints());
                    }
                    if let Ok((_, t_fx)) = self.toplevel_request(id, request) {
                        out.extend(t_fx);
                    }
                    if let Some(o) = target_output {
                        self.invalidate_frame(o);
                    }
                    out.extend(self.visibility_pass());
                } else if self.fullscreen_of(id).is_some() {
                    out.extend(self.leave_fullscreen(id, None));
                }
            }
            ShellCommand::KeepOnTop { element, on } => {
                let id = self.shell_mapped(element)?;
                let on = on.unwrap_or_else(|| !self.stage.is_above(id));
                self.set_on_top(id, on);
                if self.stage.is_above(id) == on {
                    out.push(Effect::KeptOnTop { element: id, on });
                }
            }
            ShellCommand::MinimizeElement { element } => {
                let id = self.shell_mapped(element)?;
                if self.fullscreen_of(id).is_some() {
                    return Err(ShellError::Presenting);
                }
                if self.input.interaction.active() {
                    return Err(ShellError::Busy);
                }
                if self.stage.set_minimized(id, true).is_ok() {
                    let live: Vec<_> = self
                        .focusable_elements()
                        .into_iter()
                        .filter(|&e| e != id)
                        .collect();
                    out.extend(self.lose_focus_target(id, &live));
                    self.invalidate_all_frames();
                    self.membership_pass_element(id, &mut out);
                    out.extend(self.visibility_pass());
                }
            }
            ShellCommand::MaximizeElement { element, maximized } => {
                let id = self.shell_mapped(element)?;
                if self.fullscreen_of(id).is_some() {
                    return Err(ShellError::Presenting);
                }
                if self.input.interaction.active() {
                    return Err(ShellError::Busy);
                }
                let output = self
                    .pointer_output()
                    .or_else(|| self.outputs.first().map(|o| o.id))
                    .ok_or(ShellError::UnknownOutput)?;
                let is_on = self.is_maximized(id);
                let want = maximized.unwrap_or(!is_on);
                if want != is_on {
                    self.maximize_toggle(None, output, id, true, &mut out);
                }
            }
            ShellCommand::CloseElement { element } => {
                let id = self
                    .stage
                    .resolve(element)
                    .map_err(|_| ShellError::StaleElement)?;
                out.push(Effect::CloseRequest(id));
            }
            ShellCommand::ForceCloseElement { element } => {
                let id = self
                    .stage
                    .resolve(element)
                    .map_err(|_| ShellError::StaleElement)?;
                out.push(Effect::ForceClose(id));
            }
            ShellCommand::ArrangeCluster { element, layout } => {
                let id = match element {
                    Some(element) => self.shell_mapped(element)?,
                    None => self.focus.target().ok_or(ShellError::Refused)?,
                };
                if self.input.interaction.active() {
                    return Err(ShellError::Busy);
                }
                if !self.arrange_cluster(id, layout, Some(authority::Authorized::mint()), &mut out)
                {
                    return Err(ShellError::Refused);
                }
            }
        }
        Ok(out)
    }

    /// Ends an open shell stream (its connection closed, C7.3): its epoch settles. No camera or
    /// Stage write; nothing happens when no stream is open.
    pub fn end_shell_stream(&mut self, port: &mut ShellPort) -> Result<Vec<Effect>, ShellError> {
        if port.core_id != self.id {
            return Err(ShellError::ForeignPort);
        }
        let mut out = Vec::new();
        self.shell_stream_end(ShellStream::End, &mut out);
        Ok(out)
    }

    /// The camera a shell command may write on `output`: the output is connected and has one, it
    /// presents no protocol-fullscreen element (C12.F17 Q1 B scope 2), and no interaction owns
    /// the pointer (a drag is moving a camera).
    fn shell_camera(&self, output: OutputId) -> Result<usize, ShellError> {
        let camera = self
            .output(output)
            .and_then(|_| self.cameras.camera_for_output(output))
            .ok_or(ShellError::UnknownOutput)?;
        if let Some(pres) = self.presentation(output)
            && self.focus.target() == Some(pres)
            && !self.is_presentation_suppressed(output)
        {
            return Err(ShellError::Presenting);
        }
        if self.input.interaction.active() {
            return Err(ShellError::Busy);
        }
        Ok(camera)
    }

    /// The element a reference names, mapped.
    /// A window the user picked (from the windows list): one that was minimized comes back first.
    fn shell_chosen(
        &mut self,
        element: crate::stage::ElementRef,
        out: &mut Vec<Effect>,
    ) -> Result<crate::stage::ElementId, ShellError> {
        if let Ok(id) = self.stage.resolve(element)
            && self.stage.is_minimized(id)
        {
            self.restore_minimized(id, out);
        }
        self.shell_mapped(element)
    }

    fn shell_mapped(
        &self,
        element: crate::stage::ElementRef,
    ) -> Result<crate::stage::ElementId, ShellError> {
        let id = self
            .stage
            .resolve(element)
            .map_err(|_| ShellError::StaleElement)?;
        self.stage
            .mapped(id)
            .map(|_| id)
            .ok_or(ShellError::NotMapped)
    }

    /// Where a `Navigate` puts the camera's centre (its zoom stays): within the stored Canvas,
    /// ±2³⁶ px, or the command is invalid.
    fn navigate_center(
        &self,
        camera: usize,
        to: NavigateTo,
    ) -> Result<crate::geom::CanvasPos, ShellError> {
        use crate::geom::{CANVAS_LIMIT_PX, CanvasPos, Q16_ONE};
        let center = match to {
            NavigateTo::Element(r) => {
                let id = self.shell_mapped(r)?;
                let (Some(a), Some(tree)) = (self.stage.anchor(id), self.stage.tree(id)) else {
                    return Err(ShellError::NotMapped);
                };
                // the centre of the window geometry, exact in Q16 (Q16_ONE is even)
                let size = tree.geometry.size;
                CanvasPos {
                    x: a.x * Q16_ONE + size.w as i64 * (Q16_ONE / 2),
                    y: a.y * Q16_ONE + size.h as i64 * (Q16_ONE / 2),
                }
            }
            NavigateTo::Canvas(x, y) => {
                shell_canvas_pos((x, y)).ok_or(ShellError::InvalidArgument)?
            }
            NavigateTo::Delta(dx, dy) => {
                let d = shell_canvas_pos((dx, dy)).ok_or(ShellError::InvalidArgument)?;
                let c = self
                    .cameras
                    .center(camera)
                    .ok_or(ShellError::UnknownOutput)?;
                CanvasPos {
                    x: c.x.saturating_add(d.x),
                    y: c.y.saturating_add(d.y),
                }
            }
        };
        let limit = CANVAS_LIMIT_PX * Q16_ONE;
        if center.x.abs() > limit || center.y.abs() > limit {
            return Err(ShellError::InvalidArgument);
        }
        Ok(center)
    }

    /// A step of a shell stream opens it (INV-SCALE-3 `ShellStream`).
    fn shell_stream_step(&mut self, stream: ShellStream) {
        if stream != ShellStream::Atomic {
            self.shell_stream = true;
        }
    }

    /// The explicit end-of-stream: the stream's epoch settles (INV-SCALE-3).
    fn shell_stream_end(&mut self, stream: ShellStream, out: &mut Vec<Effect>) {
        if stream == ShellStream::End && self.shell_stream {
            self.shell_stream = false;
            out.extend(self.settle_interaction(crate::scale::InteractionKind::ShellStream));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Core;
    use crate::entry::tests::*;
    use crate::navigation::PersistedCameras;

    #[test]
    fn shell_navigate_element_records_history_for_atomic_and_stream_begin() {
        use crate::stage::ElementRef;
        use crate::{NavigateTo, ShellCommand, ShellStream};
        let (mut c, _, mut s, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let ws = map_windows(&mut c, a, 2);
        let mut fx = Vec::new();
        c.place_by_user(ws[0], crate::geom::CanvasPoint { x: 0, y: 0 }, &mut fx)
            .unwrap();
        c.place_by_user(ws[1], crate::geom::CanvasPoint { x: 5000, y: 0 }, &mut fx)
            .unwrap();
        c.render_list(a).unwrap();
        let cam = camera_of(&c, a);
        let origin = (
            c.cameras().center(cam).unwrap(),
            c.cameras().zoom(cam).unwrap(),
        );
        let cref = |c: &Core, element| ElementRef {
            instance: c.stage().instance(),
            element,
        };

        c.shell_command(
            &mut s,
            ShellCommand::Navigate {
                output: a,
                to: NavigateTo::Element(cref(&c, ws[1])),
                zoom: Some(1.5),
                stream: ShellStream::Atomic,
            },
        )
        .unwrap();
        assert_eq!(c.cameras().zoom(cam), Some(1.5));
        assert_eq!(c.cameras().history(cam).unwrap().len(), 1);
        let after_atomic = (
            c.cameras().center(cam).unwrap(),
            c.cameras().zoom(cam).unwrap(),
        );

        for stream in [ShellStream::Begin, ShellStream::Step, ShellStream::End] {
            c.shell_command(
                &mut s,
                ShellCommand::Navigate {
                    output: a,
                    to: NavigateTo::Element(cref(&c, ws[0])),
                    zoom: None,
                    stream,
                },
            )
            .unwrap();
            assert_eq!(
                c.cameras().history(cam).unwrap().len(),
                2,
                "stream {stream:?} must only push at Begin"
            );
        }

        c.shell_command(&mut s, ShellCommand::ViewportRestore { output: a })
            .unwrap();
        assert_eq!(c.cameras().center(cam), Some(after_atomic.0));
        assert_eq!(c.cameras().zoom(cam), Some(after_atomic.1));
        c.shell_command(&mut s, ShellCommand::ViewportRestore { output: a })
            .unwrap();
        assert_eq!(c.cameras().center(cam), Some(origin.0));
        assert_eq!(c.cameras().zoom(cam), Some(origin.1));
    }
}
