//! The virtual-mouse rig for INC-3 T1 tests: the production stack (Lockstep) driven through the
//! headless backend's input injection, with the client's pointer wire log as the oracle.

use super::{Lockstep, output_k};
use infynitae_backend_headless::{VirtualOutput, pointer_button, pointer_motion};
use infynitae_core::geom::CanvasPos;
use infynitae_core::input::bindings::Modifiers;

pub const BTN_LEFT: u32 = 0x110;
pub const BTN_RIGHT: u32 = 0x111;
pub const BTN_MIDDLE: u32 = 0x112;

pub fn logo() -> Modifiers {
    Modifiers {
        logo: true,
        ..Modifiers::default()
    }
}

pub fn none() -> Modifiers {
    Modifiers::default()
}

pub fn pos(x: i64, y: i64) -> CanvasPos {
    CanvasPos {
        x: x << 16,
        y: y << 16,
    }
}

pub fn a_1080() -> VirtualOutput {
    output_k("A", (1920, 1080), 120, 60_000)
}

/// The production stack driven by a virtual mouse.
pub struct Rig {
    pub ls: Lockstep,
    pub t: u64,
    pub meta_held: bool,
}

impl Rig {
    pub fn new(ls: Lockstep) -> Rig {
        Rig {
            ls,
            t: 0,
            meta_held: false,
        }
    }

    /// x8 setup: W mapped on A (centred, anchor (−400, −300)), pointer moved by (210, 200) from
    /// the view centre onto W at W-local (630, 520).
    pub fn x8() -> Rig {
        let mut r = Rig::new(Lockstep::new(Some(a_1080())));
        r.map_w();
        r.motion((200.0, 200.0));
        r.motion((10.0, 0.0));
        r
    }

    pub fn map_w(&mut self) {
        let qh = self.ls.qh.clone();
        self.ls.probe.toplevel(&qh, "W", (800, 600), 20);
        self.ls.pump();
        self.frame();
    }

    /// Advances virtual time by one 60 Hz period: every output produces a frame.
    pub fn frame(&mut self) {
        let t = self.ls.now + std::time::Duration::from_micros(16_667);
        self.ls.run_until(t);
    }

    /// A relative motion with unaccelerated delta 0.9 × accelerated (x8).
    pub fn motion(&mut self, d: (f64, f64)) {
        self.t += 1000;
        pointer_motion(
            &mut self.ls.state,
            &mut self.ls.input,
            d,
            (0.9 * d.0, 0.9 * d.1),
            self.t,
        )
        .expect("motion");
        self.ls.pump();
        self.frame();
    }

    pub fn button(&mut self, b: u32, pressed: bool, mods: Modifiers) {
        if mods.logo && pressed && !self.meta_held {
            self.t += 1000;
            let _ = infynitae_backend_headless::keyboard_key(
                &mut self.ls.state,
                &mut self.ls.input,
                infynitae_core::input::keys::KEY_LEFTMETA,
                true,
                self.t,
            );
            self.meta_held = true;
        }
        self.t += 1000;
        pointer_button(
            &mut self.ls.state,
            &mut self.ls.input,
            b,
            pressed,
            mods,
            self.t,
        )
        .expect("button");
        if !pressed && self.meta_held {
            self.t += 1000;
            let _ = infynitae_backend_headless::keyboard_key(
                &mut self.ls.state,
                &mut self.ls.input,
                infynitae_core::input::keys::KEY_LEFTMETA,
                false,
                self.t,
            );
            self.meta_held = false;
        }
        self.ls.pump();
        self.frame();
    }

    pub fn mark(&self) -> usize {
        self.ls.probe.ptr.len()
    }

    pub fn since(&self, m: usize) -> Vec<&str> {
        self.ls.probe.ptr[m..].iter().map(|s| s.as_str()).collect()
    }

    pub fn camera(&self) -> CanvasPos {
        self.ls.state.core().cameras().center(0).expect("camera 0")
    }

    pub fn anchor(&self) -> Option<infynitae_core::geom::CanvasPoint> {
        let core = self.ls.state.core();
        let id = core.stage().z_order().first().copied()?;
        core.stage().anchor(id)
    }

    /// INV-INPUT-2 oracle from the client's wire: the buttons the client believes pressed. A
    /// `leave` ends the client's view of every pressed button (wl_pointer semantics).
    pub fn client_buttons_down(&self) -> Vec<String> {
        let mut down: Vec<String> = Vec::new();
        for l in &self.ls.probe.ptr {
            if l.starts_with("leave") {
                down.clear();
            }
            if let Some(b) = l.strip_prefix("button ") {
                let (code, st) = b.split_once(' ').expect("button line");
                if st == "pressed" {
                    down.push(code.to_string());
                } else {
                    down.retain(|c| c != code);
                }
            }
        }
        down
    }

    pub fn smithay_grabbed(&self) -> bool {
        self.ls
            .state
            .seat()
            .get_pointer()
            .expect("pointer")
            .is_grabbed()
    }
}

/// The wl_fixed value a client receives for `v`: wayland-scanner 0.31 encodes
/// `(v * 256.) as i32` (truncation toward zero, `common.rs` `Type::Fixed`).
pub fn wl_fixed(v: f64) -> f64 {
    ((v * 256.0) as i32) as f64 / 256.0
}

pub fn x8_deltas(n: usize) -> Vec<(f64, f64)> {
    vec![(7.3, -2.1); n]
}

/// Asserts that no motion, relative motion, button or axis event reached the client in `lines`
/// (INV-INPUT-3). The message is the bypass control's detection marker (tools/verify-architecture).
pub fn assert_nothing_delivered(lines: &[&str], what: &str) {
    let leaked: Vec<&&str> = lines
        .iter()
        .filter(|l| {
            l.starts_with("motion")
                || l.starts_with("relative")
                || l.starts_with("button")
                || l.starts_with("axis")
                || l.starts_with("enter")
                || l.starts_with("leave")
        })
        .collect();
    assert!(
        leaked.is_empty(),
        "INV-INPUT-3 violated ({what}): pan-originated client events {leaked:?}"
    );
}
