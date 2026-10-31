//! The shell's view (C7.2 Queries and Events, INV-SHELL-2): an immutable copy of Stage metadata,
//! cameras, outputs, focus and attention, read through Core's public readers only, and the events
//! that turn one copy into the next. Everything here is derived; nothing is authority.

use std::collections::{BTreeMap, BTreeSet};

use infynitae_core::Core;
use infynitae_core::output::Transform;
use infynitae_core::stage::{ElementId, Lifecycle};

use crate::json::Value;

/// One copy of what the shell may know. Elements are bottom to top (Stage z-order).
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub instance: u64,
    pub outputs: Vec<(u64, Value)>,
    pub elements: Vec<(u64, Value)>,
    pub focus: Option<u64>,
    pub attention: Vec<u64>,
    /// The session is locked (`ext-session-lock`), as the compositor says; the sleep lock asks
    /// this instead of looking for a process name (M1, owner 2026-10-31).
    pub locked: bool,
}

/// An element as the shell names it: `[instance, element]` (D-B4, INV-STAGE-3).
pub fn element_ref(instance: u64, id: u64) -> Value {
    Value::Arr(vec![instance.into(), id.into()])
}

fn transform_name(t: Transform) -> &'static str {
    match t {
        Transform::Normal => "normal",
        Transform::R90 => "90",
        Transform::R180 => "180",
        Transform::R270 => "270",
        Transform::Flipped => "flipped",
        Transform::Flipped90 => "flipped-90",
        Transform::Flipped180 => "flipped-180",
        Transform::Flipped270 => "flipped-270",
    }
}

fn pair(a: f64, b: f64) -> Value {
    Value::Arr(vec![a.into(), b.into()])
}

impl Model {
    /// The current copy. Allocates; it runs outside Core, only for a shell that asked for it.
    pub fn of(core: &Core) -> Model {
        let instance = core.stage().instance().get();
        let q = (1u64 << 16) as f64;
        let outputs = core
            .outputs()
            .iter()
            .map(|o| {
                let camera = core.cameras().camera_for_output(o.id).and_then(|c| {
                    Some(Value::obj([
                        (
                            "center",
                            core.cameras()
                                .center(c)
                                .map(|p| pair(p.x as f64 / q, p.y as f64 / q))?,
                        ),
                        ("zoom", core.cameras().zoom(c)?.into()),
                    ]))
                });
                let presenting = core
                    .presentation(o.id)
                    .map(|e| element_ref(instance, e.get().get()));
                let v = Value::obj([
                    ("id", o.id.get().into()),
                    ("name", o.key.connector.as_str().into()),
                    ("make", o.key.make.as_str().into()),
                    ("model", o.key.model.as_str().into()),
                    ("mode", pair(o.phys.0 as f64, o.phys.1 as f64)),
                    ("scale", o.scale.into()),
                    ("transform", transform_name(o.transform).into()),
                    ("refresh_mhz", u64::from(o.refresh_mhz).into()),
                    ("position", pair(o.arrangement.0, o.arrangement.1)),
                    ("camera", camera.unwrap_or(Value::Null)),
                    ("presenting", presenting.unwrap_or(Value::Null)),
                ]);
                (o.id.get(), v)
            })
            .collect();
        let elements = core
            .stage()
            .z_order()
            .iter()
            .map(|&id| (id.get().get(), element(core, instance, id)))
            .collect();
        Model {
            instance,
            outputs,
            elements,
            focus: core.focus().target().map(|e| e.get().get()),
            attention: core
                .focus()
                .attention()
                .iter()
                .map(|e| e.get().get())
                .collect(),
            locked: false,
        }
    }

    /// The copy with the session's lock state, which the adapter holds (not Core).
    pub fn with_locked(mut self, locked: bool) -> Model {
        self.locked = locked;
        self
    }

    /// The Snapshot body (C7.2 Queries).
    pub fn to_value(&self) -> Value {
        let i = self.instance;
        Value::obj([
            ("instance", i.into()),
            (
                "outputs",
                Value::Arr(self.outputs.iter().map(|(_, v)| v.clone()).collect()),
            ),
            (
                "elements",
                Value::Arr(self.elements.iter().map(|(_, v)| v.clone()).collect()),
            ),
            (
                "focus",
                self.focus.map_or(Value::Null, |e| element_ref(i, e)),
            ),
            (
                "attention",
                Value::Arr(self.attention.iter().map(|&e| element_ref(i, e)).collect()),
            ),
            ("locked", self.locked.into()),
        ])
    }

    /// The events that turn `self` into `next`, in a fixed order: outputs removed, added, changed;
    /// elements removed, added, changed; the stacking order; focus; attention. Empty when nothing
    /// the shell can see changed.
    pub fn events_to(&self, next: &Model) -> Vec<Value> {
        let i = next.instance;
        let mut ev = Vec::new();
        let event =
            |name: &str, key: &str, v: Value| Value::obj([("event", name.into()), (key, v)]);
        diff(
            &self.outputs,
            &next.outputs,
            &mut ev,
            |id| event("output_removed", "output", id.into()),
            |v| event("output_added", "output", v.clone()),
            |v| event("output_changed", "output", v.clone()),
        );
        diff(
            &self.elements,
            &next.elements,
            &mut ev,
            |id| event("element_removed", "element", element_ref(i, id)),
            |v| event("element_added", "element", v.clone()),
            |v| event("element_changed", "element", v.clone()),
        );
        let order = |m: &Model| m.elements.iter().map(|(id, _)| *id).collect::<Vec<_>>();
        let (before, after) = (order(self), order(next));
        // the stacking of the elements both copies hold, compared without the ones that came or went
        let (in_before, in_after): (BTreeSet<_>, BTreeSet<_>) =
            (before.iter().collect(), after.iter().collect());
        let kept: Vec<_> = before.iter().filter(|id| in_after.contains(id)).collect();
        let kept_after: Vec<_> = after.iter().filter(|id| in_before.contains(id)).collect();
        if kept != kept_after {
            ev.push(event(
                "stacking",
                "order",
                Value::Arr(after.iter().map(|&e| element_ref(i, e)).collect()),
            ));
        }
        if self.focus != next.focus {
            ev.push(event(
                "focus",
                "element",
                next.focus.map_or(Value::Null, |e| element_ref(i, e)),
            ));
        }
        if self.attention != next.attention {
            ev.push(event(
                "attention",
                "elements",
                Value::Arr(next.attention.iter().map(|&e| element_ref(i, e)).collect()),
            ));
        }
        if self.locked != next.locked {
            ev.push(event("lock", "locked", next.locked.into()));
        }
        ev
    }
}

fn element(core: &Core, instance: u64, id: ElementId) -> Value {
    let stage = core.stage();
    let state = match stage.state(id) {
        Some(Lifecycle::Mapped) => "mapped",
        Some(Lifecycle::Unmapped) => "unmapped",
        _ => "pending",
    };
    let meta = stage.meta(id);
    Value::obj([
        ("id", element_ref(instance, id.get().get())),
        ("state", state.into()),
        (
            "anchor",
            stage
                .anchor(id)
                .map_or(Value::Null, |a| pair(a.x as f64, a.y as f64)),
        ),
        (
            "size",
            stage.tree(id).map_or(Value::Null, |t| {
                pair(f64::from(t.geometry.size.w), f64::from(t.geometry.size.h))
            }),
        ),
        // maximized by Super+Enter: the place and size it had before (session restore)
        (
            "maximized",
            stage.saved_floating(id).map_or(Value::Null, |(a, s)| {
                Value::obj([
                    ("anchor", pair(a.x as f64, a.y as f64)),
                    ("size", pair(f64::from(s.w), f64::from(s.h))),
                ])
            }),
        ),
        (
            "title",
            meta.and_then(|m| m.title.as_deref())
                .map_or(Value::Null, Into::into),
        ),
        (
            "app_id",
            meta.and_then(|m| m.app_id.as_deref())
                .map_or(Value::Null, Into::into),
        ),
        (
            "fullscreen",
            core.fullscreen_of(id)
                .map_or(Value::Null, |f| f.output.get().into()),
        ),
        // the window wants attention (opened without the focus, or its activation was refused):
        // the edge marks its tile, Super+Tab goes to it first (owner, 2026-10-31)
        ("attention", core.focus().attention().contains(&id).into()),
    ])
}

fn diff(
    before: &[(u64, Value)],
    after: &[(u64, Value)],
    ev: &mut Vec<Value>,
    removed: impl Fn(u64) -> Value,
    added: impl Fn(&Value) -> Value,
    changed: impl Fn(&Value) -> Value,
) {
    // by id, so a refresh of n elements costs O(n log n), not O(n²)
    let b: BTreeMap<u64, &Value> = before.iter().map(|(k, v)| (*k, v)).collect();
    let a: BTreeMap<u64, &Value> = after.iter().map(|(k, v)| (*k, v)).collect();
    for id in b.keys() {
        if !a.contains_key(id) {
            ev.push(removed(*id));
        }
    }
    for (id, v) in after {
        match b.get(id) {
            None => ev.push(added(v)),
            Some(old) if *old != v => ev.push(changed(v)),
            Some(_) => {}
        }
    }
}

/// The `layers` diagnostic: for each mapped window, its geometry and surfaces (role, offset,
/// size, mapped) as the last snapshot holds them, and per output the physical rects those
/// surfaces are drawn at (the render list, built here without recording it as a frame). For
/// finding a surface drawn where the app did not put it.
pub fn layers(core: &Core) -> Value {
    use infynitae_core::render::ProjectionFrame;
    use infynitae_core::snapshot::SurfaceRole;
    let instance = core.stage().instance().get();
    let stage = core.stage();
    let frames: Vec<(u64, ProjectionFrame)> = core
        .outputs()
        .iter()
        .filter_map(|o| Some((o.id.get(), ProjectionFrame::build(core.view(o.id)?, stage))))
        .collect();
    let windows = stage
        .z_order()
        .iter()
        .filter_map(|&id| {
            let tree = stage.tree(id)?;
            let anchor = stage.anchor(id)?;
            let g = tree.geometry;
            let surfaces = tree
                .surfaces
                .iter()
                .map(|s| {
                    let (role, parent, offset) = match s.role {
                        SurfaceRole::Root => ("root", Value::Null, (0, 0)),
                        SurfaceRole::Subsurface { parent, offset } => {
                            ("subsurface", (parent as u64).into(), (offset.x, offset.y))
                        }
                        SurfaceRole::Popup { position, .. } => {
                            ("popup", Value::Null, (position.x, position.y))
                        }
                    };
                    Value::obj([
                        ("key", s.key.raw().get().into()),
                        ("role", role.into()),
                        ("parent", parent),
                        ("offset", pair(f64::from(offset.0), f64::from(offset.1))),
                        ("size", pair(f64::from(s.size.w), f64::from(s.size.h))),
                        ("mapped", s.mapped.into()),
                    ])
                })
                .collect();
            let drawn = frames
                .iter()
                .flat_map(|(out, f)| {
                    f.entries.iter().filter(|e| e.element == id).map(move |e| {
                        Value::obj([
                            ("output", (*out).into()),
                            ("key", e.surface.raw().get().into()),
                            ("origin", pair(e.origin.x as f64, e.origin.y as f64)),
                            ("size", pair(f64::from(e.size.w), f64::from(e.size.h))),
                            (
                                "rect",
                                Value::Arr(vec![
                                    (e.rect.x0 as f64).into(),
                                    (e.rect.y0 as f64).into(),
                                    (e.rect.x1 as f64).into(),
                                    (e.rect.y1 as f64).into(),
                                ]),
                            ),
                        ])
                    })
                })
                .collect();
            let app = stage.meta(id).and_then(|m| m.app_id.clone());
            Some(Value::obj([
                ("id", element_ref(instance, id.get().get())),
                ("app_id", app.map_or(Value::Null, |a| a.as_str().into())),
                ("anchor", pair(anchor.x as f64, anchor.y as f64)),
                (
                    "geometry",
                    Value::Arr(vec![
                        f64::from(g.loc.x).into(),
                        f64::from(g.loc.y).into(),
                        f64::from(g.size.w).into(),
                        f64::from(g.size.h).into(),
                    ]),
                ),
                ("surfaces", Value::Arr(surfaces)),
                ("drawn", Value::Arr(drawn)),
            ]))
        })
        .collect();
    Value::obj([("windows", Value::Arr(windows))])
}

#[cfg(test)]
mod tests {
    use super::*;
    use infynitae_core::navigation::PersistedCameras;

    /// M1 (owner, 2026-10-31): the snapshot says whether the session is locked, and a change of
    /// it is an event, so the sleep lock asks the compositor instead of a process name.
    #[test]
    fn the_snapshot_says_whether_the_session_is_locked() {
        let (core, _input, _shell, _token) = Core::new(PersistedCameras::default());
        let open = Model::of(&core);
        assert_eq!(open.to_value().get("locked"), Some(&Value::Bool(false)));
        let locked = Model::of(&core).with_locked(true);
        assert_eq!(locked.to_value().get("locked"), Some(&Value::Bool(true)));
        let ev = open.events_to(&locked);
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].get("event"), Some(&Value::Str("lock".into())));
        assert_eq!(ev[0].get("locked"), Some(&Value::Bool(true)));
    }
}
