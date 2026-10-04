//! Surface-tree snapshot extraction using ONLY Smithay 0.7.0 public API (this crate is external to
//! Smithay, so compiling it is the proof). Stand-in for the adapter's extractor (C2.1).
//!
//! Output = normalized spatial data, no handles: per surface an opaque key (here the protocol id),
//! kind, RootSurfaceLocal origin, logical size, mapped flag and input region rectangles, in
//! FRONT-TO-BACK order (popup trees first, each traversed downward; then the root tree downward).
//! Plus the window geometry in RootSurfaceLocal coordinates.
//!
//! Re-entrancy rule: all per-surface data is read from the `SurfaceData` handed to the traversal
//! callbacks; no `with_states` call is made for a surface inside its own traversal.

use smithay::{
    backend::renderer::utils::RendererSurfaceStateUserData,
    desktop::{PopupManager, Window},
    reexports::wayland_server::{protocol::wl_surface::WlSurface, Resource},
    utils::{Logical, Point, Rectangle},
    wayland::compositor::{with_surface_tree_downward, RectangleKind, SurfaceAttributes, TraversalAction},
};

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Root,
    Sub,
    Popup,
    PopupSub,
}

#[derive(Clone, Debug)]
pub struct SnapSurface {
    pub key: u32,
    pub kind: Kind,
    /// origin in RootSurfaceLocal (logical, integral)
    pub origin: Point<i32, Logical>,
    pub size: (i32, i32),
    pub mapped: bool,
    /// (add?, rect) in surface-local coordinates; None = infinite (whole surface)
    pub input: Option<Vec<(bool, Rectangle<i32, Logical>)>>,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub geometry: Rectangle<i32, Logical>,
    pub surfaces: Vec<SnapSurface>,
}

fn tree(surface: &WlSurface, base: Point<i32, Logical>, top: Kind, child: Kind, out: &mut Vec<SnapSurface>) {
    with_surface_tree_downward(
        surface,
        base,
        |_, states, loc| {
            let off = states
                .data_map
                .get::<RendererSurfaceStateUserData>()
                .and_then(|d| d.lock().unwrap().view())
                .map(|v| v.offset)
                .unwrap_or_default();
            // the root's own offset is (0,0); children add their subsurface offset
            TraversalAction::DoChildren(*loc + off)
        },
        |s, states, loc| {
            let view = states.data_map.get::<RendererSurfaceStateUserData>().and_then(|d| d.lock().unwrap().view());
            let off = view.map(|v| v.offset).unwrap_or_default();
            let size = view.map(|v| (v.dst.w, v.dst.h)).unwrap_or((0, 0));
            let mapped = states.data_map.get::<RendererSurfaceStateUserData>().map(|d| d.lock().unwrap().buffer().is_some()).unwrap_or(false);
            let input = states.cached_state.get::<SurfaceAttributes>().current().input_region.clone().map(|r| {
                r.rects.iter().map(|(k, rect)| (matches!(k, RectangleKind::Add), *rect)).collect::<Vec<_>>()
            });
            out.push(SnapSurface {
                key: s.id().protocol_id(),
                kind: if s == surface { top.clone() } else { child.clone() },
                origin: *loc + off,
                size,
                mapped,
                input,
            });
        },
        |_, _, _| true,
    );
}

pub fn extract(window: &Window) -> Snapshot {
    let geometry = window.geometry();
    let root = window.toplevel().expect("wayland toplevel").wl_surface().clone();
    let mut surfaces = Vec::new();
    // popups first (front), same iteration and offset formula as Smithay's Window render path
    for (popup, loc) in PopupManager::popups_for_surface(&root) {
        let origin = geometry.loc + loc - popup.geometry().loc;
        tree(popup.wl_surface(), origin, Kind::Popup, Kind::PopupSub, &mut surfaces);
    }
    tree(&root, (0, 0).into(), Kind::Root, Kind::Sub, &mut surfaces);
    Snapshot { geometry, surfaces }
}

/// Hit test on the snapshot alone (what core would do): front-to-back, first mapped surface whose
/// bounds contain the point and whose input region (if any) contains it.
pub fn hit(snap: &Snapshot, p: Point<f64, Logical>) -> Option<(u32, Point<i32, Logical>)> {
    hit_with(snap, p, false)
}

/// `smithay_rounding = true` emulates Smithay 0.7.0's input-region test, which rounds the point to
/// the nearest integer (`desktop/wayland/utils.rs:25-50`, W-SM-11) instead of half-open containment.
pub fn hit_with(snap: &Snapshot, p: Point<f64, Logical>, smithay_rounding: bool) -> Option<(u32, Point<i32, Logical>)> {
    for s in &snap.surfaces {
        if !s.mapped {
            continue;
        }
        let local = p - s.origin.to_f64();
        let inside = local.x >= 0.0 && local.y >= 0.0 && local.x < s.size.0 as f64 && local.y < s.size.1 as f64;
        if !inside {
            continue;
        }
        let ok = match &s.input {
            None => true,
            Some(rects) => {
                let pi = if smithay_rounding {
                    Point::<i32, Logical>::from((local.x.round() as i32, local.y.round() as i32))
                } else {
                    Point::<i32, Logical>::from((local.x.floor() as i32, local.y.floor() as i32))
                };
                let mut r = false;
                for (add, rect) in rects {
                    if rect.contains(pi) {
                        r = *add;
                    }
                }
                r
            }
        };
        if ok {
            return Some((s.key, s.origin));
        }
    }
    None
}

/// Adapter-side registry stand-in: key -> WlSurface for every surface in the window's trees.
/// (Handles stay on the adapter side; only keys go into the snapshot.)
pub fn registry(window: &Window) -> Vec<(u32, WlSurface)> {
    let mut out = Vec::new();
    let root = window.toplevel().expect("wayland toplevel").wl_surface().clone();
    let mut visit = |s: &WlSurface| {
        with_surface_tree_downward(s, (), |_, _, _| TraversalAction::DoChildren(()), |s, _, _| out.push((s.id().protocol_id(), s.clone())), |_, _, _| true);
    };
    for (popup, _) in PopupManager::popups_for_surface(&root) {
        visit(popup.wl_surface());
    }
    visit(&root);
    out
}
