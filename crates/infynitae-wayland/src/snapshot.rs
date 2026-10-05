//! Surface-tree snapshot extraction (OBJ-05, CP2-2) through Smithay 0.7.0 public API only.
//!
//! Order is front to back: each popup tree (in `PopupManager::popups_for_surface` order), then the
//! root tree, each traversed by `with_surface_tree_downward` (nearest first), as Smithay's own
//! `Window` render path does (C3.3, X-15).
//!
//! Dispatch safety: per-surface data is read only from the `SurfaceData` handed to the traversal
//! callbacks; no `with_states` call is made for a surface inside its own traversal.

use infynitae_core::geom::{LocalPoint, LocalRect, LocalSize};
use infynitae_core::snapshot::{InputRegion, RegionOp, SnapshotSurface, SurfaceRole, SurfaceTree};
use smithay::backend::renderer::utils::RendererSurfaceStateUserData;
use smithay::desktop::{PopupManager, Window};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::wayland::compositor::{
    RectangleKind, SurfaceAttributes, TraversalAction, with_surface_tree_downward,
};

use crate::convert;
use crate::registry::SurfaceRegistry;

/// What the traversal records before keys and parent indices are resolved.
struct Raw {
    surface: WlSurface,
    parent: Option<WlSurface>,
    popup: Option<(LocalPoint, LocalPoint)>,
    offset: LocalPoint,
    size: LocalSize,
    mapped: bool,
    input: InputRegion,
}

fn tree(root: &WlSurface, popup: Option<(LocalPoint, LocalPoint)>, out: &mut Vec<Raw>) {
    with_surface_tree_downward(
        root,
        None::<WlSurface>,
        |s, _, _| TraversalAction::DoChildren(Some(s.clone())),
        |s, states, parent| {
            let state = states.data_map.get::<RendererSurfaceStateUserData>();
            let (view, mapped) = state
                .map(|d| {
                    let d = d.lock().unwrap_or_else(|e| e.into_inner());
                    (d.view(), d.buffer().is_some())
                })
                .unwrap_or((None, false));
            let input = match states
                .cached_state
                .get::<SurfaceAttributes>()
                .current()
                .input_region
                .as_ref()
            {
                None => InputRegion::Infinite,
                Some(r) => InputRegion::Ops(
                    r.rects
                        .iter()
                        .map(|(kind, rect)| RegionOp {
                            add: matches!(kind, RectangleKind::Add),
                            rect: convert::local_rect(*rect),
                        })
                        .collect(),
                ),
            };
            let is_root = s == root;
            out.push(Raw {
                surface: s.clone(),
                parent: if is_root { None } else { parent.clone() },
                popup: if is_root { popup } else { None },
                offset: view
                    .map(|v| convert::local_point(v.offset))
                    .unwrap_or_default(),
                size: view.map(|v| convert::local_size(v.dst)).unwrap_or_default(),
                mapped,
                input,
            });
        },
        |_, _, _| true,
    );
}

/// The snapshot of a toplevel's whole surface tree, with every surface registered.
pub(crate) fn extract(window: &Window, registry: &mut SurfaceRegistry) -> Option<SurfaceTree> {
    let root = window.toplevel()?.wl_surface().clone();
    let geometry = window.geometry();
    let mut raw = Vec::new();
    for (popup, position) in PopupManager::popups_for_surface(&root) {
        let geometry_loc = popup.geometry().loc;
        tree(
            popup.wl_surface(),
            Some((
                convert::local_point(position),
                convert::local_point(geometry_loc),
            )),
            &mut raw,
        );
    }
    tree(&root, None, &mut raw);

    let index_of = |s: &WlSurface| raw.iter().position(|r| &r.surface == s);
    let mut surfaces = Vec::with_capacity(raw.len());
    for r in &raw {
        let role = match (&r.parent, r.popup) {
            (_, Some((position, geometry_loc))) => SurfaceRole::Popup {
                position,
                geometry_loc,
            },
            (None, None) => SurfaceRole::Root,
            (Some(p), None) => SurfaceRole::Subsurface {
                parent: index_of(p)?,
                offset: r.offset,
            },
        };
        surfaces.push(SnapshotSurface {
            key: registry.key_for(&r.surface),
            role,
            size: r.size,
            mapped: r.mapped,
            input: r.input.clone(),
        });
    }
    Some(SurfaceTree {
        geometry: LocalRect {
            loc: convert::local_point(geometry.loc),
            size: convert::local_size(geometry.size),
        },
        surfaces,
    })
}
