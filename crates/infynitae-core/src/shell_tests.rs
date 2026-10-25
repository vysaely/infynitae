//! Tests for `Core::shell_command` (C7.2, INV-SHELL-1, INV-STAGE-3, INV-CAMERA-1). Expected values
//! are hand-derived literals, not values computed by the code under test.

use crate::geom::{CanvasPoint, CanvasPos, LocalPoint, LocalRect, LocalSize};
use crate::navigation::PersistedCameras;
use crate::output::{OutputDescriptor, OutputId, Transform};
use crate::protocol::ToplevelRequest;
use crate::scale::InteractionKind;
use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceKey, SurfaceRole, SurfaceTree};
use crate::stage::{ElementId, ElementRef};
use crate::{
    Core, InputPort, NavigateTo, OutputLifecycleToken, ShellCommand, ShellError, ShellPort,
    ShellStream,
};

const Q: i64 = 1 << 16;

fn win(k: u64, w: i32, h: i32, mapped: bool) -> SurfaceTree {
    SurfaceTree {
        geometry: LocalRect {
            loc: LocalPoint::default(),
            size: LocalSize { w, h },
        },
        surfaces: vec![SnapshotSurface {
            key: SurfaceKey::from_raw(std::num::NonZeroU64::new(k).unwrap()),
            role: SurfaceRole::Root,
            size: if mapped {
                LocalSize { w, h }
            } else {
                LocalSize::default()
            },
            mapped,
            input: InputRegion::Infinite,
            frame_requested: false,
        }],
    }
}

fn map_win(c: &mut Core, k: u64, w: i32, h: i32) -> ElementId {
    let id = c.toplevel_created();
    c.toplevel_committed(id, win(k, w, h, false)).unwrap();
    c.toplevel_committed(id, win(k, w, h, true)).unwrap();
    id
}

fn out(c: &mut Core, t: &mut OutputLifecycleToken, name: &str) -> OutputId {
    c.output_connected(
        t,
        OutputDescriptor {
            key: crate::output::tests::key(name),
            mode: (1920, 1080),
            transform: Transform::Normal,
            scale_120: 120,
            refresh_mhz: 60_000,
        },
    )
    .unwrap()
    .0
}

struct Fixture {
    c: Core,
    #[allow(dead_code)]
    i: InputPort,
    s: ShellPort,
    o: OutputId,
}

fn fixture() -> Fixture {
    let (mut c, i, s, mut t) = Core::new(PersistedCameras::default());
    let o = out(&mut c, &mut t, "A");
    Fixture { c, i, s, o }
}

fn r(c: &Core, id: ElementId) -> ElementRef {
    ElementRef {
        instance: c.stage().instance(),
        element: id,
    }
}

/// Everything a refused command must leave as it was.
fn state(c: &Core) -> String {
    let elements: Vec<_> = c
        .stage()
        .z_order()
        .iter()
        .map(|id| (*id, c.stage().anchor(*id), c.stage().state(*id)))
        .collect();
    format!(
        "{:?}",
        (
            elements,
            c.cameras().center(0),
            c.cameras().zoom(0),
            c.cameras().history(0).map(|h| h.len()),
            c.focus().target(),
            c.focus().attention(),
        )
    )
}

fn navigate(output: OutputId, to: NavigateTo) -> ShellCommand {
    ShellCommand::Navigate {
        output,
        to,
        stream: ShellStream::Atomic,
    }
}

/// C1.5 / C7.4: the entry refuses a port of another Core, and changes nothing (verify-architecture
/// runs this as the runtime port-binding check for `shell_command`).
#[test]
fn shell_command_refuses_a_foreign_port() {
    let mut f = fixture();
    let (_other, _i, mut foreign, _t) = Core::new(PersistedCameras::default());
    let before = state(&f.c);
    let cmd = navigate(f.o, NavigateTo::Canvas(500.0, 500.0));
    assert_eq!(
        f.c.shell_command(&mut foreign, cmd),
        Err(ShellError::ForeignPort)
    );
    assert_eq!(
        f.c.end_shell_stream(&mut foreign),
        Err(ShellError::ForeignPort)
    );
    assert_eq!(state(&f.c), before);
    assert!(
        f.c.shell_command(&mut f.s, cmd).is_ok(),
        "its own port works"
    );
}

/// Navigate to a Canvas position: the centre is the position quantised once (exact here), the zoom
/// is unchanged, and no history entry is made (OBJ-30 records fits).
#[test]
fn navigate_centres_the_camera_and_keeps_its_zoom() {
    let mut f = fixture();
    let zoom = f.c.cameras().zoom(0);
    f.c.shell_command(&mut f.s, navigate(f.o, NavigateTo::Canvas(1000.5, -200.25)))
        .unwrap();
    assert_eq!(
        f.c.cameras().center(0),
        Some(CanvasPos {
            x: 1000 * Q + Q / 2,
            y: -200 * Q - Q / 4
        })
    );
    assert_eq!(f.c.cameras().zoom(0), zoom);
    assert_eq!(f.c.cameras().history(0).map(|h| h.len()), Some(0));
    f.c.shell_command(&mut f.s, navigate(f.o, NavigateTo::Delta(-0.5, 0.25)))
        .unwrap();
    assert_eq!(
        f.c.cameras().center(0),
        Some(CanvasPos {
            x: 1000 * Q,
            y: -200 * Q
        })
    );
}

/// Navigate to an element: the centre of its window geometry, exactly.
#[test]
fn navigate_to_an_element_centres_its_window_geometry() {
    let mut f = fixture();
    let w = map_win(&mut f.c, 1, 401, 300);
    let a = f.c.stage().anchor(w).unwrap();
    let to = NavigateTo::Element(r(&f.c, w));
    f.c.shell_command(&mut f.s, navigate(f.o, to)).unwrap();
    assert_eq!(
        f.c.cameras().center(0),
        Some(CanvasPos {
            x: a.x * Q + 200 * Q + Q / 2,
            y: a.y * Q + 150 * Q
        })
    );
}

/// Invalid numbers and targets beyond the stored Canvas (±2³⁶ px) are rejected; nothing changes.
#[test]
fn navigate_outside_the_canvas_or_with_non_finite_values_changes_nothing() {
    let mut f = fixture();
    let limit = (1i64 << 36) as f64;
    let before = state(&f.c);
    for to in [
        NavigateTo::Canvas(limit + 1.0, 0.0),
        NavigateTo::Canvas(0.0, -limit - 1.0),
        NavigateTo::Canvas(f64::NAN, 0.0),
        NavigateTo::Canvas(0.0, f64::INFINITY),
        NavigateTo::Delta(f64::NAN, 0.0),
    ] {
        assert_eq!(
            f.c.shell_command(&mut f.s, navigate(f.o, to)),
            Err(ShellError::InvalidArgument),
            "{to:?}"
        );
    }
    f.c.shell_command(&mut f.s, navigate(f.o, NavigateTo::Canvas(limit, 0.0)))
        .unwrap();
    let at_edge = state(&f.c);
    assert_ne!(at_edge, before, "the edge itself is valid");
    assert_eq!(
        f.c.shell_command(&mut f.s, navigate(f.o, NavigateTo::Delta(1.0, 0.0))),
        Err(ShellError::InvalidArgument),
        "a delta past the edge"
    );
    assert_eq!(state(&f.c), at_edge);
}

/// INV-STAGE-3: references of another instance or to a destroyed element fail with StaleElement and
/// never alias a later element; the instance differs between Cores and fits a JSON number exactly.
#[test]
fn stale_references_fail_and_never_alias() {
    let mut f = fixture();
    let w = map_win(&mut f.c, 1, 100, 100);
    let (other, ..) = Core::new(PersistedCameras::default());
    assert_ne!(other.stage().instance(), f.c.stage().instance());
    for c in [&f.c, &other] {
        let v = c.stage().instance().get();
        assert!(v != 0 && v < 1 << 53, "{v}");
    }
    let foreign = ElementRef {
        instance: other.stage().instance(),
        element: w,
    };
    let before = state(&f.c);
    assert_eq!(
        f.c.shell_command(&mut f.s, ShellCommand::Focus { element: foreign }),
        Err(ShellError::StaleElement)
    );
    let old = r(&f.c, w);
    f.c.toplevel_destroyed(w);
    let fresh = map_win(&mut f.c, 2, 100, 100);
    assert_ne!(fresh, w, "ids are never reused");
    let before_destroyed = state(&f.c);
    for cmd in [
        ShellCommand::Focus { element: old },
        ShellCommand::AcknowledgeAttention { element: old },
        ShellCommand::ViewportFit {
            output: f.o,
            element: old,
        },
        ShellCommand::MoveElement {
            element: old,
            to: (0.0, 0.0),
            stream: ShellStream::Atomic,
        },
        navigate(f.o, NavigateTo::Element(old)),
    ] {
        assert_eq!(
            f.c.shell_command(&mut f.s, cmd),
            Err(ShellError::StaleElement),
            "{cmd:?}"
        );
    }
    assert_eq!(state(&f.c), before_destroyed);
    assert_ne!(before, before_destroyed);
    assert_eq!(ElementRef::from_raw(1, 0), None, "element 0 names nothing");
}

/// The shell's fit and restore are the M8 actions: one history entry, then the exact prior value.
#[test]
fn a_shell_fit_and_restore_round_trip() {
    let mut f = fixture();
    let w = map_win(&mut f.c, 1, 400, 300);
    let (c0, z0) = (f.c.cameras().center(0), f.c.cameras().zoom(0));
    assert_eq!(
        f.c.shell_command(&mut f.s, ShellCommand::ViewportRestore { output: f.o }),
        Err(ShellError::Refused),
        "an empty history"
    );
    let element = r(&f.c, w);
    f.c.shell_command(
        &mut f.s,
        ShellCommand::ViewportFit {
            output: f.o,
            element,
        },
    )
    .unwrap();
    assert_eq!(f.c.cameras().history(0).map(|h| h.len()), Some(1));
    assert_ne!(
        f.c.cameras().zoom(0),
        z0,
        "a 400×300 window fits a 1920×1080 view by zooming"
    );
    f.c.shell_command(&mut f.s, ShellCommand::ViewportRestore { output: f.o })
        .unwrap();
    assert_eq!((f.c.cameras().center(0), f.c.cameras().zoom(0)), (c0, z0));
    assert_eq!(f.c.cameras().history(0).map(|h| h.len()), Some(0));
}

/// C12.F17 Q1 B scope 2: no shell camera write on an output that presents; MoveElement refuses a
/// fullscreen element (C7.2); focus never moves to an element a presentation hides (A-8).
#[test]
fn shell_commands_are_refused_while_the_output_presents() {
    let mut f = fixture();
    let hidden = map_win(&mut f.c, 9, 300, 200);
    let w = map_win(&mut f.c, 1, 400, 300);
    // a history entry for the restore below; the fit keeps `hidden` in the camera's view
    f.c.shell_command(
        &mut f.s,
        ShellCommand::ViewportFit {
            output: f.o,
            element: r(&f.c, hidden),
        },
    )
    .unwrap();
    // only the focused element's fullscreen request is granted
    f.c.shell_command(
        &mut f.s,
        ShellCommand::Focus {
            element: r(&f.c, w),
        },
    )
    .unwrap();
    f.c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(f.o) })
        .unwrap();
    assert_eq!(f.c.presentation(f.o), Some(w));
    assert!(f.c.hidden_by_presentation(hidden));
    let before = state(&f.c);
    let (rw, rh) = (r(&f.c, w), r(&f.c, hidden));
    for cmd in [
        navigate(f.o, NavigateTo::Canvas(0.0, 0.0)),
        ShellCommand::ViewportFit {
            output: f.o,
            element: rh,
        },
        ShellCommand::ViewportRestore { output: f.o },
        ShellCommand::MoveElement {
            element: rw,
            to: (0.0, 0.0),
            stream: ShellStream::Atomic,
        },
        ShellCommand::Focus { element: rh },
    ] {
        assert_eq!(
            f.c.shell_command(&mut f.s, cmd),
            Err(ShellError::Presenting),
            "{cmd:?}"
        );
    }
    assert_eq!(state(&f.c), before);
}

/// MoveElement goes through `stage::place` with R3 (round half to even) and the ±2³⁶ bound; an
/// unmapped element is refused. Its size is never written.
#[test]
fn move_element_places_the_anchor_by_r3_within_the_canvas() {
    let mut f = fixture();
    let w = map_win(&mut f.c, 1, 400, 300);
    let element = r(&f.c, w);
    let mv = |to| ShellCommand::MoveElement {
        element,
        to,
        stream: ShellStream::Atomic,
    };
    f.c.shell_command(&mut f.s, mv((10.5, 11.5))).unwrap();
    assert_eq!(f.c.stage().anchor(w), Some(CanvasPoint { x: 10, y: 12 }));
    f.c.shell_command(&mut f.s, mv((-3.5, -2.5))).unwrap();
    assert_eq!(f.c.stage().anchor(w), Some(CanvasPoint { x: -4, y: -2 }));
    let before = state(&f.c);
    let limit = (1i64 << 36) as f64;
    for to in [(limit + 1.0, 0.0), (0.0, f64::NAN)] {
        assert_eq!(
            f.c.shell_command(&mut f.s, mv(to)),
            Err(ShellError::InvalidArgument)
        );
    }
    assert_eq!(state(&f.c), before);
    assert_eq!(
        f.c.stage().tree(w).map(|t| t.geometry.size),
        Some(LocalSize { w: 400, h: 300 })
    );
    let pending = f.c.toplevel_created();
    let unmapped = r(&f.c, pending);
    assert_eq!(
        f.c.shell_command(
            &mut f.s,
            ShellCommand::MoveElement {
                element: unmapped,
                to: (0.0, 0.0),
                stream: ShellStream::Atomic,
            }
        ),
        Err(ShellError::NotMapped)
    );
}

/// OBJ-04: an activation without a validated token marks attention; the shell acknowledges it
/// (cleared, no focus change), and the shell's Focus is an explicit focus action.
#[test]
fn the_shell_acknowledges_attention_and_focuses() {
    let mut f = fixture();
    let a = map_win(&mut f.c, 1, 400, 300);
    let w = map_win(&mut f.c, 2, 300, 200);
    f.c.shell_command(
        &mut f.s,
        ShellCommand::Focus {
            element: r(&f.c, a),
        },
    )
    .unwrap();
    assert_eq!(f.c.focus().target(), Some(a));
    f.c.activation_request(w, false).unwrap();
    assert_eq!(f.c.focus().attention(), &[w]);
    f.c.shell_command(
        &mut f.s,
        ShellCommand::AcknowledgeAttention {
            element: r(&f.c, w),
        },
    )
    .unwrap();
    assert!(f.c.focus().attention().is_empty());
    assert_eq!(
        f.c.focus().target(),
        Some(a),
        "acknowledging moves no focus"
    );
    let fx =
        f.c.shell_command(
            &mut f.s,
            ShellCommand::Focus {
                element: r(&f.c, w),
            },
        )
        .unwrap();
    assert_eq!(f.c.focus().target(), Some(w));
    assert!(
        fx.contains(&crate::protocol::Effect::KeyboardFocus(Some(w))),
        "{fx:?}"
    );
}

/// INV-SCALE-3 `ShellStream`: the steps of a stream form one open epoch on the elements they touch;
/// the explicit end settles it. Outside a stream a step opens no stream epoch.
#[test]
fn a_shell_stream_is_one_epoch_until_its_end() {
    let mut f = fixture();
    let w = map_win(&mut f.c, 1, 400, 300);
    let element = r(&f.c, w);
    let open = |c: &Core| {
        c.scale_state(w)
            .is_some_and(|s| s.epoch.open && s.epoch.kind == Some(InteractionKind::ShellStream))
    };
    let mv = |x: f64, stream| ShellCommand::MoveElement {
        element,
        to: (x, 0.0),
        stream,
    };
    f.c.shell_command(&mut f.s, mv(1.0, ShellStream::Atomic))
        .unwrap();
    assert!(!open(&f.c), "an atomic command is no stream");
    f.c.shell_command(&mut f.s, mv(2.0, ShellStream::Begin))
        .unwrap();
    assert!(open(&f.c));
    f.c.shell_command(&mut f.s, mv(3.0, ShellStream::Step))
        .unwrap();
    assert!(open(&f.c));
    f.c.shell_command(&mut f.s, mv(4.0, ShellStream::End))
        .unwrap();
    assert!(!open(&f.c), "the end settles the epoch");
    // a stream the shell never ends is ended for it when its connection closes
    f.c.shell_command(&mut f.s, mv(5.0, ShellStream::Begin))
        .unwrap();
    assert!(open(&f.c));
    f.c.end_shell_stream(&mut f.s).unwrap();
    assert!(!open(&f.c));
}

/// D9: the title and app_id are kept as the client set them, each cut to 4096 bytes at a
/// character boundary.
#[test]
fn toplevel_metadata_is_kept_and_bounded() {
    let mut f = fixture();
    let w = map_win(&mut f.c, 1, 100, 100);
    assert_eq!(f.c.stage().meta(w), None);
    f.c.toplevel_metadata(w, Some("Terminal"), None).unwrap();
    let m = f.c.stage().meta(w).unwrap();
    assert_eq!(
        (m.title.as_deref(), m.app_id.as_deref()),
        (Some("Terminal"), None)
    );
    // 4095 ASCII bytes and then a 2-byte character: the character would end at byte 4097
    let long = format!("{}é", "a".repeat(4095));
    f.c.toplevel_metadata(w, Some(&long), Some("org.example.App"))
        .unwrap();
    let m = f.c.stage().meta(w).unwrap();
    assert_eq!(m.title.as_deref().map(str::len), Some(4095));
    assert_eq!(m.app_id.as_deref(), Some("org.example.App"));
}
