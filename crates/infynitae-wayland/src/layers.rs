//! `zwlr_layer_shell_v1` (owner decisions 2026-10-21 L1–L8): the protocol objects of layer
//! surfaces. Core owns what they are — where they lie, what they reserve, who has the keyboard;
//! this module turns their requests into Core's protocol entries and Core's effects into protocol
//! events. Any client may create one (L8).

use smithay::delegate_layer_shell;
use smithay::reexports::wayland_server::protocol::wl_output::WlOutput;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::wayland::compositor::with_states;
use smithay::wayland::shell::wlr_layer::{
    self, ExclusiveZone, LayerSurface, LayerSurfaceCachedState, WlrLayerShellHandler,
    WlrLayerShellState,
};

use infynitae_core::layer::{Anchors, KeyboardInteractivity, Layer, LayerId, LayerState};

use crate::{WaylandState, snapshot};

/// A layer surface and its Core identity.
#[derive(Debug, Clone)]
pub(crate) struct LayerHandle {
    pub id: LayerId,
    pub surface: LayerSurface,
}

fn layer_of(l: wlr_layer::Layer) -> Layer {
    match l {
        wlr_layer::Layer::Background => Layer::Background,
        wlr_layer::Layer::Bottom => Layer::Bottom,
        wlr_layer::Layer::Top => Layer::Top,
        wlr_layer::Layer::Overlay => Layer::Overlay,
    }
}

/// The double-buffered state of the surface's last commit, in Core's terms.
fn state_of(c: &LayerSurfaceCachedState) -> LayerState {
    use wlr_layer::Anchor;
    LayerState {
        layer: Some(layer_of(c.layer)),
        anchors: Anchors {
            top: c.anchor.contains(Anchor::TOP),
            bottom: c.anchor.contains(Anchor::BOTTOM),
            left: c.anchor.contains(Anchor::LEFT),
            right: c.anchor.contains(Anchor::RIGHT),
        },
        size: (c.size.w.max(0) as u32, c.size.h.max(0) as u32),
        margins: (c.margin.top, c.margin.right, c.margin.bottom, c.margin.left),
        exclusive_zone: match c.exclusive_zone {
            ExclusiveZone::Exclusive(z) => z.min(i32::MAX as u32) as i32,
            ExclusiveZone::Neutral => 0,
            ExclusiveZone::DontCare => -1,
        },
        keyboard: match c.keyboard_interactivity {
            wlr_layer::KeyboardInteractivity::None => KeyboardInteractivity::None,
            wlr_layer::KeyboardInteractivity::Exclusive => KeyboardInteractivity::Exclusive,
            wlr_layer::KeyboardInteractivity::OnDemand => KeyboardInteractivity::OnDemand,
        },
    }
}

impl WaylandState {
    pub(crate) fn layer_index_by_root(&self, root: &WlSurface) -> Option<usize> {
        self.layers
            .iter()
            .position(|h| h.surface.wl_surface() == root)
    }

    fn layer_handle(&self, id: LayerId) -> Option<&LayerHandle> {
        self.layers.iter().find(|h| h.id == id)
    }

    /// The root surface of a layer surface (`None`: not one of ours).
    pub fn layer_surface(&self, id: LayerId) -> Option<WlSurface> {
        self.layer_handle(id)
            .map(|h| h.surface.wl_surface().clone())
    }

    /// A commit of a layer surface's root, or of a surface in its tree: Core gets the committed
    /// state and the tree. A buffer before the initial configure is the client's protocol error
    /// (Smithay posts it), and is not passed on.
    pub(crate) fn layer_committed(&mut self, index: usize) {
        let LayerHandle { id, surface } = self.layers[index].clone();
        let root = surface.wl_surface().clone();
        let state = with_states(&root, |states| {
            state_of(
                states
                    .cached_state
                    .get::<LayerSurfaceCachedState>()
                    .current(),
            )
        });
        let Some(tree) = snapshot::extract_layer(&root, &mut self.registry) else {
            return;
        };
        let has_buffer = tree.root().is_some_and(|r| r.mapped);
        if has_buffer && !surface.ensure_configured() {
            return;
        }
        if let Ok(fx) = self.core.layer_committed(id, state, tree) {
            self.apply(&fx);
        }
    }

    /// Core's layer effects. Returns `false` for an effect that is not one.
    pub(crate) fn apply_layer(&mut self, effect: &infynitae_core::protocol::Effect) -> bool {
        use infynitae_core::protocol::Effect;
        match *effect {
            Effect::LayerConfigure { layer, size } => {
                if let Some(h) = self.layer_handle(layer) {
                    h.surface
                        .with_pending_state(|s| s.size = Some((size.w, size.h).into()));
                    h.surface.send_configure();
                }
            }
            Effect::LayerClosed(layer) => {
                if let Some(i) = self.layers.iter().position(|h| h.id == layer) {
                    let h = self.layers.remove(i);
                    h.surface.send_close();
                }
            }
            Effect::LayerKeyboardFocus(layer) => {
                if let Some(surface) = self.layer_surface(layer) {
                    let serial = smithay::utils::SERIAL_COUNTER.next_serial();
                    let kb = self.keyboard_handle();
                    kb.set_focus(self, Some(surface), serial);
                }
            }
            Effect::LayerPreferredScale {
                layer,
                scale_120,
                integer_scale,
            } => {
                if let Some(surface) = self.layer_surface(layer) {
                    crate::convert::send_preferred_scale_to_tree(
                        &surface,
                        scale_120,
                        integer_scale,
                    );
                }
            }
            _ => return false,
        }
        true
    }

    /// The frame callbacks of the layer surfaces Core shows on `output` complete at its vblank
    /// (a hidden one — below a presentation — gets none until it is shown again).
    pub(crate) fn complete_layer_frames(
        &mut self,
        output: infynitae_core::output::OutputId,
        now: std::time::Duration,
    ) {
        if self.layers.is_empty() {
            return;
        }
        let Some(o) = self.smithay_output(output).cloned() else {
            return;
        };
        let frame = self.core.layer_frame(output);
        for e in frame.above.iter().chain(frame.below.iter()) {
            if let Some(surface) = self.registry.surface(e.surface) {
                smithay::desktop::utils::send_frames_surface_tree(
                    surface,
                    &o,
                    now,
                    None,
                    |_, _| Some(o.clone()),
                );
            }
        }
    }
}

impl WlrLayerShellHandler for WaylandState {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell
    }

    fn new_layer_surface(
        &mut self,
        surface: LayerSurface,
        output: Option<WlOutput>,
        _layer: wlr_layer::Layer,
        _namespace: String,
    ) {
        let output = output.as_ref().and_then(|o| self.output_id_of(o));
        match self.core.layer_created(output) {
            Some(id) => self.layers.push(LayerHandle { id, surface }),
            // no output to put it on: closed at once, as the protocol allows
            None => surface.send_close(),
        }
    }

    fn layer_destroyed(&mut self, surface: LayerSurface) {
        if let Some(i) = self.layers.iter().position(|h| h.surface == surface) {
            let h = self.layers.remove(i);
            let fx = self.core.layer_destroyed(h.id);
            self.apply(&fx);
        }
    }
}

delegate_layer_shell!(WaylandState);
