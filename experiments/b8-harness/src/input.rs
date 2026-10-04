//! X-8: the input-ownership model under test (B10/B11), minimal.
//!
//! This is NOT an input subsystem. It implements exactly the rules being tested:
//!  * ownership of every physical button is decided at press time, BEFORE any client dispatch,
//!    and lasts until that button's release (no unmatched client button state);
//!  * a CameraPan interaction may only be acquired when the client owns no pressed button;
//!  * while CameraPan owns the pointer, physical deltas go to the camera only: no wl_pointer
//!    motion and no relative motion is sent;
//!  * the pan is accumulated from the gesture origin (B1 rule), the camera is a view (Stage
//!    untouched);
//!  * reconciliation at termination: locked pointers keep their surface-local lock point,
//!    free pointers get a re-hit-test at the unchanged screen position.
//!
//! Physical events enter through `Intake::button` / `Intake::motion`.

use std::collections::BTreeSet;

use smithay::{
    backend::input::ButtonState,
    desktop::WindowSurfaceType,
    input::pointer::{ButtonEvent, MotionEvent, RelativeMotionEvent},
    reexports::wayland_server::{protocol::wl_surface::WlSurface, Resource},
    utils::{Logical, Point, SERIAL_COUNTER},
    wayland::pointer_constraints::{with_pointer_constraint, PointerConstraint},
};

use crate::server::Server;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PanFocusMode {
    /// pointer focus is kept during the pan; deliveries are simply suppressed
    KeepFocus,
    /// pointer focus is cleared (wl_pointer.leave) at acquisition, restored at termination
    LeaveFocus,
}

#[derive(Clone, Copy, Debug)]
pub struct PanBinding {
    pub button: u32,
    pub needs_modifier: bool,
}

#[derive(Debug)]
pub enum Interaction {
    Normal,
    CameraPan {
        output: usize,
        initiator: u32,
        start_camera: (i32, i32),
        sum: (f64, f64),
        /// set if the focused surface had an ACTIVE lock at acquisition: (surface, Canvas lock point).
        /// The pan never moves elements (I2), so this Canvas point stays the lock point.
        lock: Option<(WlSurface, Point<f64, Logical>)>,
    },
}

pub struct Intake {
    pub binding: PanBinding,
    pub modifier: bool,
    pub mode: PanFocusMode,
    pub interaction: Interaction,
    /// buttons whose press was delivered to a client and whose release has not happened yet
    pub client_buttons: BTreeSet<u32>,
    /// buttons whose press was consumed by the compositor; their releases are consumed too
    pub owned_buttons: BTreeSet<u32>,
    /// authoritative pointer state (B2.5): output index and OutputLogical position
    pub output: usize,
    pub p: (f64, f64),
    pub utime: u64,
    pub log: Vec<(u64, String)>,
}

fn surface_origin_and_target(srv: &Server, q: Point<f64, Logical>) -> Option<(WlSurface, Point<f64, Logical>)> {
    // top-most first; B2.5 chain: q -> RootSurfaceLocal -> Window::surface_under -> Canvas origin
    for e in srv.elements.iter().rev() {
        if !e.mapped {
            continue;
        }
        let Some(anchor) = e.anchor else { continue };
        let geo = e.window.geometry();
        let root = Point::<f64, Logical>::from(((anchor.0 - geo.loc.x) as f64, (anchor.1 - geo.loc.y) as f64));
        let r = q - root;
        if let Some((s, loc)) = e.window.surface_under(r, WindowSurfaceType::ALL) {
            return Some((s, root + loc.to_f64()));
        }
    }
    None
}

impl Intake {
    pub fn new(binding: PanBinding, mode: PanFocusMode, output: usize, p: (f64, f64)) -> Self {
        Intake {
            binding,
            modifier: false,
            mode,
            interaction: Interaction::Normal,
            client_buttons: BTreeSet::new(),
            owned_buttons: BTreeSet::new(),
            output,
            p,
            utime: 0,
            log: Vec::new(),
        }
    }

    fn note(&mut self, srv: &Server, s: String) {
        self.log.push((srv.now_us, s));
    }

    /// Canvas point under the authoritative pointer (T5 at zoom 1, integer harness camera).
    pub fn canvas(&self, srv: &Server) -> Point<f64, Logical> {
        let v = srv.outputs[self.output].camera;
        Point::from((v.0 as f64 + self.p.0, v.1 as f64 + self.p.1))
    }

    fn active_lock(srv: &Server, surface: &WlSurface) -> bool {
        let ptr = srv.seat.get_pointer().expect("pointer");
        with_pointer_constraint(surface, &ptr, |c| matches!(c.as_deref(), Some(PointerConstraint::Locked(_))) && c.map(|c| c.is_active()).unwrap_or(false))
    }

    fn activate_constraint_if_any(srv: &mut Server, surface: &WlSurface) -> bool {
        if !srv.activation_allowed {
            return false;
        }
        let ptr = srv.seat.get_pointer().expect("pointer");
        let activated = with_pointer_constraint(surface, &ptr, |c| match c {
            Some(c) if !c.is_active() => {
                c.activate();
                true
            }
            _ => false,
        });
        if activated {
            srv.note("constraint activated on focus".into());
        }
        activated
    }

    /// Deliver the pointer's true position to the seat (normal routing / reconciliation).
    fn deliver_position(&mut self, srv: &mut Server) {
        let ptr = srv.seat.get_pointer().expect("pointer");
        let q = self.canvas(srv);
        let focus = surface_origin_and_target(srv, q);
        let serial = SERIAL_COUNTER.next_serial();
        ptr.motion(srv, focus.clone(), &MotionEvent { location: q, serial, time: (self.utime / 1000) as u32 });
        ptr.frame(srv);
        if let Some((s, _)) = focus {
            Self::activate_constraint_if_any(srv, &s);
        }
    }

    pub fn button(&mut self, srv: &mut Server, code: u32, pressed: bool) {
        let ptr = srv.seat.get_pointer().expect("pointer");
        if pressed {
            // 1. an interaction already owns the pointer: it owns this press too
            if matches!(self.interaction, Interaction::CameraPan { .. }) {
                self.owned_buttons.insert(code);
                self.note(srv, format!("intake: press {code} consumed (owned by CameraPan)"));
                return;
            }
            // 2. gesture resolution BEFORE client dispatch
            let binding_matches = code == self.binding.button && (!self.binding.needs_modifier || self.modifier);
            if binding_matches && self.client_buttons.is_empty() {
                let lock = ptr.current_focus().filter(|s| Self::active_lock(srv, s)).map(|s| (s, self.canvas(srv)));
                self.interaction = Interaction::CameraPan {
                    output: self.output,
                    initiator: code,
                    start_camera: srv.outputs[self.output].camera,
                    sum: (0.0, 0.0),
                    lock,
                };
                self.owned_buttons.insert(code);
                srv.gesture_active = true;
                self.note(srv, format!("intake: press {code} ACQUIRES CameraPan (mode {:?})", self.mode));
                if self.mode == PanFocusMode::LeaveFocus {
                    let serial = SERIAL_COUNTER.next_serial();
                    let q = self.canvas(srv);
                    ptr.motion(srv, None, &MotionEvent { location: q, serial, time: (self.utime / 1000) as u32 });
                    ptr.frame(srv);
                }
                return;
            }
            if binding_matches {
                self.note(srv, format!("intake: press {code} matches binding but client owns {:?}: delivered to client (no acquisition)", self.client_buttons));
            }
            // 3. client-owned press
            self.client_buttons.insert(code);
            let serial = SERIAL_COUNTER.next_serial();
            ptr.button(srv, &ButtonEvent { serial, time: (self.utime / 1000) as u32, button: code, state: ButtonState::Pressed });
            ptr.frame(srv);
        } else {
            if self.owned_buttons.remove(&code) {
                self.note(srv, format!("intake: release {code} consumed (compositor-owned)"));
                let ends = matches!(self.interaction, Interaction::CameraPan { initiator, .. } if initiator == code);
                if ends {
                    self.end_pan(srv);
                }
                return;
            }
            if self.client_buttons.remove(&code) {
                let serial = SERIAL_COUNTER.next_serial();
                ptr.button(srv, &ButtonEvent { serial, time: (self.utime / 1000) as u32, button: code, state: ButtonState::Released });
                ptr.frame(srv);
            }
        }
    }

    /// Physical relative motion (accelerated and unaccelerated deltas).
    pub fn motion(&mut self, srv: &mut Server, d: (f64, f64), d_unaccel: (f64, f64)) {
        self.utime += 1000;
        if let Interaction::CameraPan { output, start_camera, sum, .. } = &mut self.interaction {
            // camera-owned delta: accumulate from the gesture origin, never per event (B1)
            sum.0 += d.0;
            sum.1 += d.1;
            let cam = (start_camera.0 - sum.0.round_ties_even() as i32, start_camera.1 - sum.1.round_ties_even() as i32);
            let o = *output;
            srv.outputs[o].camera = cam;
            srv.refresh_visibility();
            return; // consumed: no wl_pointer.motion, no relative motion
        }
        let ptr = srv.seat.get_pointer().expect("pointer");
        let focus_surface = ptr.current_focus();
        let locked = focus_surface.as_ref().map(|s| Self::active_lock(srv, s)).unwrap_or(false);
        if !locked {
            let (w, h) = srv.outputs[self.output].size;
            self.p = ((self.p.0 + d.0).clamp(0.0, w as f64 - 1.0), (self.p.1 + d.1).clamp(0.0, h as f64 - 1.0));
            self.deliver_position(srv);
        }
        // relative motion is delivered to the current focus (locked or not): the physical delta
        let focus = ptr.current_focus().map(|s| {
            let q = self.canvas(srv);
            let origin = surface_origin_and_target(srv, q).filter(|(t, _)| *t == s).map(|(_, o)| o).unwrap_or(q);
            (s, origin)
        });
        ptr.relative_motion(srv, focus, &RelativeMotionEvent { delta: d.into(), delta_unaccel: d_unaccel.into(), utime: self.utime });
        ptr.frame(srv);
    }

    fn end_pan(&mut self, srv: &mut Server) {
        let Interaction::CameraPan { lock, sum, .. } = std::mem::replace(&mut self.interaction, Interaction::Normal) else { return };
        srv.gesture_active = false;
        srv.refresh_visibility(); // deferred scale decreases etc.
        self.note(srv, format!("intake: CameraPan ends (accumulated {:.3},{:.3})", sum.0, sum.1));
        match lock {
            Some((s, q_lock)) if s.is_alive() => {
                // Locked reconciliation: keep the lock point; derive the screen position from it.
                let v = srv.outputs[self.output].camera;
                let (w, h) = srv.outputs[self.output].size;
                let p = (q_lock.x - v.0 as f64, q_lock.y - v.1 as f64);
                if p.0 >= 0.0 && p.1 >= 0.0 && p.0 < w as f64 && p.1 < h as f64 {
                    self.p = p;
                    self.note(srv, format!("intake: lock kept; pointer re-derived from Canvas lock point -> output-local ({:.3},{:.3}); no client event", p.0, p.1));
                    return;
                }
                // lock point not observable on the pointer's output: the lock must end (protocol permits
                // compositor deactivation). W-SM-4 ordering rule: Smithay's leave() deactivates
                // unconditionally, so deactivate explicitly ONLY if pointer focus will not change.
                let ptr = srv.seat.get_pointer().expect("pointer");
                let new_focus = surface_origin_and_target(srv, self.canvas(srv)).map(|(t, _)| t);
                if new_focus.as_ref() == Some(&s) {
                    with_pointer_constraint(&s, &ptr, |c| {
                        if let Some(c) = c {
                            c.deactivate();
                        }
                    });
                    self.note(srv, "intake: lock point unobservable, focus unchanged -> explicit deactivate".into());
                } else {
                    self.note(srv, "intake: lock point unobservable, focus will change -> deactivation left to leave()".into());
                }
                self.deliver_position(srv);
            }
            _ => {
                // Free reconciliation: the screen position never moved; the Canvas under it did.
                self.deliver_position(srv);
                self.note(srv, "intake: free pointer re-hit-tested at unchanged output-local position".into());
            }
        }
    }
}
