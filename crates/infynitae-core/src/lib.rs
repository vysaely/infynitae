//! Infynitae Native core: the spatial and policy authority (FOUNDATION_ARCHITECTURE C1.2).
//!
//! INC-0 state: authority **shape** only. The types and entry points below exist so that the
//! architecture probes in `enforcement/probes` and `tools/verify-architecture` compile against the
//! real production boundary (OB-19). No camera, output or input behaviour exists yet; later
//! increments populate these boundaries (FOUNDATION_IMPLEMENTATION_PLAN INC-1 onwards).
//!
//! Authority model (INV-CAMERA-1, INV-OUTPUT-2, C1.5):
//! * camera state is mutable only inside [`navigation`], and every mutation consumes a fresh
//!   [`authority::Authorized`] proof;
//! * a proof can be minted only inside the port-carrying entry points (module `entry`);
//! * [`Core::new`] is the only place that creates [`InputPort`], [`ShellPort`] and
//!   [`OutputLifecycleToken`]; they have private fields, are not `Clone`, are `!Send` and are
//!   bound to the Core that minted them.

#![forbid(unsafe_code)]

pub mod geom;
pub mod navigation;
pub mod output;
pub mod projection;
pub mod protocol;
pub mod render;
pub mod snapshot;
pub mod stage;

mod entry;
pub use entry::authority;

#[cfg(any(
    feature = "probe_protocol_moves_camera",
    feature = "probe_protocol_refits_camera",
    feature = "probe_protocol_creates_camera",
    feature = "probe_unsafe_block",
    feature = "probe_unsafe_allow_override"
))]
mod probes;

/// Marker that makes a type `!Send` and `!Sync`. Ports, the lifecycle token and the proof carry it,
/// so protocol state asserted `Send` cannot contain them (C7.4, E0277 on violation).
type NotSend = std::marker::PhantomData<*const ()>;

/// Authority to submit physical input (C1.5). Minted only by [`Core::new`].
pub struct InputPort {
    core_id: u64,
    _not_send: NotSend,
}

/// Authority to submit shell commands (C1.5, C7). Minted only by [`Core::new`].
pub struct ShellPort {
    core_id: u64,
    _not_send: NotSend,
}

/// Authority for output connect, reconfiguration and disconnect (INV-OUTPUT-2). Distinct from
/// [`InputPort`]; exactly one per Core, returned once by [`Core::new`] (C12.R2.3).
pub struct OutputLifecycleToken {
    core_id: u64,
    _not_send: NotSend,
}

/// Physical input produced by a backend. No variants until INC-3 (pointer intake).
pub enum PhysicalEvent {}

/// A semantic command from the trusted shell (C7.2). No variants until INC-11 (shell IPC).
pub enum ShellCommand {}

/// Why an entry point refused a request.
#[derive(Debug, PartialEq, Eq)]
pub enum EntryError {
    /// The port or token was minted by another Core (C1.5, C12.R2.3).
    ForeignPort,
    /// The request is outside the implemented increments: a second output, reconfiguration and
    /// disconnect arrive with INC-2.
    Unsupported,
    /// The output descriptor is empty or has a non-positive scale.
    InvalidDescriptor,
}

/// Why no frame could be produced for an output.
#[derive(Debug, PartialEq, Eq)]
pub enum FrameError {
    /// The output is not connected.
    UnknownOutput,
    /// The output has no bound camera or its view parameters are invalid.
    NoView,
}

pub struct Core {
    id: u64,
    stage: stage::Stage,
    cameras: navigation::CameraSet,
    outputs: Vec<output::OutputModel>,
    output_ids: output::OutputIds,
    /// ProjectionFrame per output (OBJ-15): the last list produced for that output.
    frames: Vec<(output::OutputId, render::ProjectionFrame)>,
    /// NEGATIVE PROBE (PD-F2-01): Core retaining a proof for a later mutation. Must make the
    /// protocol state fail its `Send` assertion (built through infynitae-wayland's probe feature).
    #[cfg(feature = "probe_sticky_fit_retains_authority")]
    sticky_fit: Option<authority::Authorized>,
}

impl Core {
    /// Composition root only: the Core, both authority ports and the output-lifecycle token.
    pub fn new() -> (Core, InputPort, ShellPort, OutputLifecycleToken) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        (
            Core {
                id,
                stage: stage::Stage::new(stage::InstanceId(id)),
                cameras: navigation::CameraSet::empty(),
                outputs: Vec::new(),
                output_ids: output::OutputIds::default(),
                frames: Vec::new(),
                #[cfg(feature = "probe_sticky_fit_retains_authority")]
                sticky_fit: None,
            },
            InputPort {
                core_id: id,
                _not_send: std::marker::PhantomData,
            },
            ShellPort {
                core_id: id,
                _not_send: std::marker::PhantomData,
            },
            OutputLifecycleToken {
                core_id: id,
                _not_send: std::marker::PhantomData,
            },
        )
    }

    /// Read access to the cameras. There is no mutable accessor.
    pub fn cameras(&self) -> &navigation::CameraSet {
        &self.cameras
    }

    /// Read access to the Stage. There is no mutable accessor.
    pub fn stage(&self) -> &stage::Stage {
        &self.stage
    }

    pub fn outputs(&self) -> &[output::OutputModel] {
        &self.outputs
    }

    /// View(O) from the camera bound to the output (F-VIEW-1, F-LATTICE-1).
    pub fn view(&self, output: output::OutputId) -> Option<projection::View> {
        let model = self.outputs.iter().find(|o| o.id == output)?;
        let camera = self.cameras.camera_for_output(output)?;
        projection::View::new(projection::ViewParams {
            centre: self.cameras.center(camera)?,
            zoom: self.cameras.zoom(camera)?,
            scale: model.scale,
            view_size: model.view_size(),
        })
        .ok()
    }

    /// Produces the render list of an output and records it as that output's ProjectionFrame.
    pub fn render_list(
        &mut self,
        output: output::OutputId,
    ) -> Result<&render::ProjectionFrame, FrameError> {
        if !self.outputs.iter().any(|o| o.id == output) {
            return Err(FrameError::UnknownOutput);
        }
        let view = self.view(output).ok_or(FrameError::NoView)?;
        let frame = render::ProjectionFrame::build(view, &self.stage);
        self.frames.retain(|(o, _)| *o != output);
        self.frames.push((output, frame));
        Ok(&self.frames.last().expect("just pushed").1)
    }

    /// F-HIT-1 against the last frame shown on the output (built on demand if none exists yet).
    /// An entry whose surface no longer exists fails closed: no delivery until the next frame.
    pub fn hit_test(
        &mut self,
        output: output::OutputId,
        p: geom::OutputLogicalPoint,
    ) -> Option<render::Hit> {
        if !self.frames.iter().any(|(o, _)| *o == output) {
            self.render_list(output).ok()?;
        }
        let frame = &self.frames.iter().find(|(o, _)| *o == output)?.1;
        let hit = frame.select(p)?;
        let alive = self
            .stage
            .tree(hit.element)
            .is_some_and(|t| t.surfaces.iter().any(|s| s.key == hit.surface));
        alive.then_some(hit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::geom::{LocalPoint, LocalRect, LocalSize, OutputLogicalPoint};
    use crate::output::{OutputDescriptor, OutputKey, Transform};
    use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceRole, SurfaceTree, tests::key};

    fn tree(root: u64, sub: Option<u64>) -> SurfaceTree {
        let s = |k, role, w, h| SnapshotSurface {
            key: key(k),
            role,
            size: LocalSize { w, h },
            mapped: true,
            input: InputRegion::Infinite,
        };
        let mut surfaces = Vec::new();
        if let Some(k) = sub {
            surfaces.push(s(
                k,
                SurfaceRole::Subsurface {
                    parent: 1,
                    offset: LocalPoint { x: 10, y: 10 },
                },
                20,
                20,
            ));
        }
        surfaces.push(s(root, SurfaceRole::Root, 200, 100));
        SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint::default(),
                size: LocalSize { w: 200, h: 100 },
            },
            surfaces,
        }
    }

    fn mapped(core: &mut Core, t: SurfaceTree) -> stage::ElementId {
        let id = core.toplevel_created();
        let mut first = t.clone();
        first.surfaces.iter_mut().for_each(|s| s.mapped = false);
        core.toplevel_committed(id, first).unwrap();
        core.toplevel_committed(id, t).unwrap();
        id
    }

    fn connected() -> (Core, output::OutputId) {
        let (mut core, _i, _s, mut token) = Core::new();
        let o = core
            .output_connected(
                &mut token,
                OutputDescriptor {
                    key: OutputKey::opaque(),
                    mode: (1920, 1080),
                    transform: Transform::Normal,
                    scale_120: 120,
                    refresh_mhz: 60_000,
                },
            )
            .unwrap();
        (core, o)
    }

    /// INV-RENDER-2: elements front to back (reverse z-order), then snapshot order within each.
    #[test]
    fn render_list_is_front_to_back() {
        let (mut core, o) = connected();
        let a = mapped(&mut core, tree(1, Some(2)));
        let b = mapped(&mut core, tree(3, None));
        let order: Vec<_> = core
            .render_list(o)
            .unwrap()
            .entries
            .iter()
            .map(|e| (e.element, e.surface.raw().get()))
            .collect();
        assert_eq!(order, vec![(b, 3), (a, 2), (a, 1)]);
    }

    /// PD-F1-07: an entry of the last frame whose surface no longer exists fails closed (no
    /// delivery, no fall-through to the surface below) until the next frame is produced.
    #[test]
    fn dead_frame_entries_fail_closed_until_the_next_frame() {
        let (mut core, o) = connected();
        let a = mapped(&mut core, tree(1, Some(2)));
        core.render_list(o).unwrap();
        // anchor (0,0) is at the view centre (960,540); the subsurface covers (970..990, 550..570)
        let p = OutputLogicalPoint { x: 975.5, y: 555.5 };
        assert_eq!(core.hit_test(o, p).map(|h| h.surface.raw().get()), Some(2));
        // the client destroys the subsurface; the snapshot no longer has it
        core.toplevel_committed(a, tree(1, None)).unwrap();
        assert_eq!(
            core.hit_test(o, p),
            None,
            "a dead entry must not fall through"
        );
        core.render_list(o).unwrap();
        assert_eq!(core.hit_test(o, p).map(|h| h.surface.raw().get()), Some(1));
    }

    #[test]
    fn no_camera_exists_before_an_output_is_connected() {
        let (core, _input, _shell, _token) = Core::new();
        assert_eq!(core.cameras().center(0), None);
    }
}
