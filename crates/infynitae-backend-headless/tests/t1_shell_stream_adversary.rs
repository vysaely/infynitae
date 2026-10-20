//! OB-53, INC-11 part ("ShellStream epochs (INC-11)"): an adversarial T1 client under a bounded
//! shell stream of `MoveElement` commands (INV-SCALE-3: "a bounded shell stream of navigation or
//! `MoveElement` commands" is one epoch, ended by its explicit end-of-stream). Pass criterion of
//! OB-53: "hint changes ≤ (rungs crossed + 1 + one P7 per non-client stimulus) per epoch; the run
//! ends quiescent; no camera mutation from client geometry (INV-CAMERA-1)".
//!
//! Setup: output A (1920 × 1080, scale 1) and output B (1920 × 1080, scale 2), B's camera moved
//! by the shell to Canvas (100 000, 0) so the two views share nothing; D_max = 2000 px. W (800 ×
//! 600, fractional-scale, viewport) starts on A (ρ = 1). The shell streams 20 steps alternating W
//! between B (ρ = 2) and A (ρ = 1), the last on A, then ends the stream. The adversary answers
//! every hint: wide (1300 px) while its hint is above 2000/1300, narrow (800 px) otherwise.
//!
//! Expected (INV-SCALE-3/4, F-SCALE-3): inside the epoch decreases are suppressed and increases
//! happen only in the epoch's direction, so the alternation cannot make the hint follow it: at most
//! the rise to B's rung, its P7 decrease (the adversary grows past D_max), and the settlement at the
//! end — 3 changes for 20 alternating steps; then quiescence. The same 20 steps sent as atomic
//! commands (each a one-step epoch) make the hint follow the moves: that run changes it many times
//! (the scenario is a real stimulus, and the stream is what bounds it).

mod common;

use common::Lockstep;
use infynitae_core::stage::ElementRef;
use infynitae_core::{NavigateTo, ShellCommand, ShellStream};

const D_MAX: u32 = 2000;
const STEPS: usize = 20;

struct Run {
    ls: Lockstep,
    w: usize,
    vp: wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport,
    element: ElementRef,
}

fn shell(ls: &mut Lockstep, cmd: ShellCommand) {
    let fx = ls
        .state
        .core_mut()
        .shell_command(&mut ls.shell, cmd)
        .expect("the shell command runs");
    ls.state.apply(&fx);
    ls.pump();
}

fn frame(ls: &mut Lockstep) {
    let t = ls.now + std::time::Duration::from_micros(16_667);
    ls.run_until(t);
}

fn setup() -> Run {
    let mut ls = Lockstep::new(Some(common::output_k("A", (1920, 1080), 120, 60_000)));
    let b = ls.connect(&common::output_k("B", (1920, 1080), 240, 60_000));
    let fx = ls
        .state
        .core_mut()
        .set_renderer_device_max(D_MAX)
        .expect("valid D_max");
    ls.state.apply(&fx);
    shell(
        &mut ls,
        ShellCommand::Navigate {
            output: b,
            to: NavigateTo::Canvas(100_000.0, 0.0),
            stream: ShellStream::Atomic,
        },
    );
    let w = ls
        .probe
        .toplevel_with_fractional_scale(&ls.qh, "W", (800, 600), 0);
    ls.pump();
    ls.probe.draw(&ls.qh, w);
    ls.pump();
    let vp = ls.probe.get_viewport(&ls.qh, w).expect("viewport");
    vp.set_destination(800, 600);
    ls.probe.surfaces[w].wl.commit();
    ls.pump();
    frame(&mut ls);
    let core = ls.state.core();
    let id = *core.stage().mapped_elements().last().expect("W is mapped");
    let element = ElementRef {
        instance: core.stage().instance(),
        element: id,
    };
    Run { ls, w, vp, element }
}

impl Run {
    fn hints(&self) -> Vec<u32> {
        self.ls.probe.preferred_scales_of("W")
    }

    /// The adversary's answer to the hint in force: one commit with new window geometry.
    fn answer(&mut self) {
        let hint = *self.hints().last().unwrap();
        let side = if u64::from(hint) * 1300 > u64::from(D_MAX) * 120 {
            1300
        } else {
            800
        };
        self.vp.set_destination(side, 600);
        self.ls.probe.surfaces[self.w].wl.commit();
        self.ls.pump();
        frame(&mut self.ls);
        let now = *self.hints().last().unwrap();
        assert!(
            side == 800 || u64::from(now) * 1300 <= u64::from(D_MAX) * 120,
            "a {side}-px buffer at hint {now}/120 exceeds D_max (P7)"
        );
    }

    /// Step `i` of the alternation: even steps put W on B, odd ones back on A (the last, odd).
    fn step(&mut self, i: usize, stream: ShellStream) {
        let x = if i.is_multiple_of(2) {
            100_000.0 - 400.0
        } else {
            -400.0
        };
        let cameras = self.cameras();
        shell(
            &mut self.ls,
            ShellCommand::MoveElement {
                element: self.element,
                to: (x, -300.0),
                stream,
            },
        );
        assert_eq!(self.cameras(), cameras, "a Stage write moved a camera");
        self.answer();
        assert_eq!(
            self.cameras(),
            cameras,
            "client geometry moved a camera (INV-CAMERA-1)"
        );
    }

    fn cameras(&self) -> Vec<String> {
        let c = self.ls.state.core().cameras();
        (0..c.len())
            .map(|i| format!("{:?} {:?}", c.center(i), c.zoom(i)))
            .collect()
    }
}

#[test]
fn an_adversarial_client_under_a_shell_stream_changes_the_hint_a_bounded_number_of_times() {
    let mut r = setup();
    let start = r.hints().len();
    for i in 0..STEPS {
        let stream = match i {
            0 => ShellStream::Begin,
            i if i == STEPS - 1 => ShellStream::End,
            _ => ShellStream::Step,
        };
        r.step(i, stream);
    }
    let changes: Vec<u32> = r.hints()[start..].to_vec();
    // the rise to B's rung (240/120 = 2); P7 at once, the 1300-px answer exceeding D_max (the
    // largest hint with 1300 · h ≤ 2000 on the ladder: 184/120); the settlement on A (120/120)
    assert_eq!(
        changes,
        [240, 184, 120],
        "{STEPS} alternating steps in one stream: the rise, its P7 decrease and the settlement"
    );
    // quiescence: the adversary keeps answering, nothing changes any more
    let settled = r.hints().len();
    for _ in 0..10 {
        r.answer();
    }
    assert_eq!(
        r.hints().len(),
        settled,
        "the run must end quiescent: {:?}",
        r.hints()
    );
}

#[test]
fn the_same_steps_as_atomic_commands_make_the_hint_follow_them() {
    let mut r = setup();
    let start = r.hints().len();
    for i in 0..STEPS {
        r.step(i, ShellStream::Atomic);
    }
    let changes = r.hints().len() - start;
    assert!(
        changes >= STEPS,
        "atomic steps are one-step epochs: the hint follows W between the rungs ({changes} changes)"
    );
}
