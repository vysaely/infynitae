//! EXPERIMENTAL validation client: an ordinary Wayland client, in its own process, that requests
//! and releases protocol fullscreen on a script. It is a test fixture for a compositor under
//! test, not part of the product.
//!
//! ```text
//! fs_client [--premap] [--keep-size] [--size WxH] --script "MS:VERB;MS:VERB;…"
//! ```
//!
//! Verbs, at MS milliseconds after the first buffer was committed: `fullscreen`, `unfullscreen`,
//! `maximize`, `unmap`, `remap`, `destroy`, `exit`, and for popups `popup` (a 120x80 popup on the
//! window at 20,20, grabbing with the last button serial if there is one), `subpopup` (the same on
//! the newest popup), `closepopup` (destroys the newest popup). A popup is yellow, a popup of a
//! popup magenta; `popup_done` destroys the popup it names. Pointer motion and buttons are printed
//! with the time the compositor stamped on them. `--premap` sends `set_fullscreen`
//! before the initial commit. Every configure is acknowledged and answered with a buffer of the configured
//! size (or the window's own size when the configure names none, or always with `--keep-size`).
//! The buffer is red while the last configure carried `fullscreen` and green otherwise, with a
//! blue 40x40 square in its top-left corner, so a capture shows state and orientation.
//! Everything received is printed.

use std::os::fd::AsFd;
use std::os::unix::fs::FileExt;
use std::time::{Duration, Instant};

use wayland_client::protocol::{
    wl_buffer::WlBuffer, wl_compositor::WlCompositor, wl_keyboard::WlKeyboard, wl_pointer,
    wl_pointer::WlPointer, wl_registry, wl_seat, wl_seat::WlSeat, wl_shm, wl_shm::WlShm,
    wl_shm_pool::WlShmPool, wl_surface::WlSurface,
};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols::xdg::shell::client::{
    xdg_popup::{self, XdgPopup},
    xdg_positioner::{self, XdgPositioner},
    xdg_surface::{self, XdgSurface},
    xdg_toplevel::{self, XdgToplevel},
    xdg_wm_base::{self, XdgWmBase},
};

const RED: u32 = 0xFFFF_0000;
const GREEN: u32 = 0xFF00_FF00;
const BLUE: u32 = 0xFF00_00FF;
const YELLOW: u32 = 0xFFFF_FF00;
const MAGENTA: u32 = 0xFFFF_00FF;
const POPUP: (i32, i32) = (120, 80);

/// One popup of the chain; `popups[0]` is the window's, each next one its predecessor's.
struct Popup {
    surface: WlSurface,
    xdg: XdgSurface,
    popup: XdgPopup,
}

#[derive(Default)]
struct App {
    compositor: Option<WlCompositor>,
    shm: Option<WlShm>,
    wm_base: Option<XdgWmBase>,
    surface: Option<WlSurface>,
    xdg: Option<XdgSurface>,
    toplevel: Option<XdgToplevel>,
    /// The window's own size, used when a configure names none.
    own: (i32, i32),
    keep_size: bool,
    /// The last xdg_toplevel.configure, applied at the xdg_surface.configure that follows.
    pending: ((i32, i32), Vec<u32>),
    /// A buffer is attached after each configure while the window is meant to be shown.
    shown: bool,
    first_buffer: Option<Instant>,
    started: Option<Instant>,
    seat: Option<WlSeat>,
    pointer: Option<WlPointer>,
    /// The serial of the last button press: what a popup grab must name.
    press_serial: Option<u32>,
    /// Slot n holds popup number n + 1 (the number is the object's user data) until it is gone.
    popups: Vec<Option<Popup>>,
}

impl App {
    fn at(&self) -> u128 {
        self.started.map_or(0, |s| s.elapsed().as_millis())
    }

    fn draw(&mut self, qh: &QueueHandle<Self>) {
        let ((cw, ch), states) = self.pending.clone();
        let fullscreen = states.contains(&2);
        let (w, h) = if cw > 0 && ch > 0 && !self.keep_size {
            (cw, ch)
        } else {
            self.own
        };
        let colour = if fullscreen { RED } else { GREEN };
        let mut px = vec![colour; (w * h) as usize];
        for y in 0..40.min(h) {
            for x in 0..40.min(w) {
                px[(y * w + x) as usize] = BLUE;
            }
        }
        let surface = self.surface.clone().expect("surface");
        self.commit_pixels(qh, &surface, (w, h), &px);
        self.first_buffer.get_or_insert_with(Instant::now);
        println!(
            "[{:>6} ms] buffer {w}x{h} {}",
            self.at(),
            if fullscreen {
                "red (fullscreen)"
            } else {
                "green (windowed)"
            }
        );
    }
}

impl App {
    fn commit_pixels(
        &self,
        qh: &QueueHandle<Self>,
        surface: &WlSurface,
        (w, h): (i32, i32),
        px: &[u32],
    ) {
        let bytes: Vec<u8> = px.iter().flat_map(|p| p.to_le_bytes()).collect();
        let fd = smithay::reexports::rustix::fs::memfd_create(
            "fs-client",
            smithay::reexports::rustix::fs::MemfdFlags::CLOEXEC,
        )
        .expect("memfd");
        let file = std::fs::File::from(fd);
        file.write_all_at(&bytes, 0).expect("fill");
        let shm = self.shm.as_ref().expect("wl_shm");
        let pool = shm.create_pool(file.as_fd(), bytes.len() as i32, qh, ());
        let buffer = pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Argb8888, qh, ());
        surface.attach(Some(&buffer), 0, 0);
        surface.damage_buffer(0, 0, w, h);
        surface.commit();
    }

    /// A popup on the window (`on_popup` false) or on the newest popup, at 20,20 of its parent,
    /// grabbing if a button press has given a serial. The buffer follows its configure.
    fn open_popup(&mut self, qh: &QueueHandle<Self>, on_popup: bool) {
        let (Some(compositor), Some(wm_base)) = (self.compositor.clone(), self.wm_base.clone())
        else {
            return;
        };
        let parent = if on_popup {
            match self.popups.iter().rev().flatten().next() {
                Some(p) => p.xdg.clone(),
                None => {
                    println!("[{:>6} ms] subpopup: no popup to open it on", self.at());
                    return;
                }
            }
        } else {
            match self.xdg.clone() {
                Some(x) => x,
                None => return,
            }
        };
        let number = self.popups.len() + 1;
        let positioner = wm_base.create_positioner(qh, ());
        positioner.set_size(POPUP.0, POPUP.1);
        positioner.set_anchor_rect(20, 20, 1, 1);
        positioner.set_anchor(xdg_positioner::Anchor::BottomRight);
        positioner.set_gravity(xdg_positioner::Gravity::BottomRight);
        let surface = compositor.create_surface(qh, ());
        let xdg = wm_base.get_xdg_surface(&surface, qh, number);
        let popup = xdg.get_popup(Some(&parent), &positioner, qh, number);
        positioner.destroy();
        let grab = match (self.seat.as_ref(), self.press_serial) {
            (Some(seat), Some(serial)) => {
                popup.grab(seat, serial);
                format!("grab with serial {serial}")
            }
            _ => "no grab (no button serial yet)".to_string(),
        };
        surface.commit();
        println!(
            "[{:>6} ms] popup {number} created on {}, {grab}",
            self.at(),
            if on_popup {
                "the newest popup"
            } else {
                "the window"
            }
        );
        self.popups.push(Some(Popup {
            surface,
            xdg,
            popup,
        }));
    }

    fn destroy_popup(&mut self, number: usize, why: &str) {
        if let Some(p) = self.popups.get_mut(number - 1).and_then(Option::take) {
            p.popup.destroy();
            p.xdg.destroy();
            p.surface.destroy();
            println!("[{:>6} ms] popup {number} destroyed ({why})", self.at());
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for App {
    fn event(
        st: &mut Self,
        registry: &wl_registry::WlRegistry,
        ev: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = ev
        {
            match interface.as_str() {
                "wl_compositor" => {
                    st.compositor = Some(registry.bind(name, version.min(6), qh, ()))
                }
                "wl_shm" => st.shm = Some(registry.bind(name, 1, qh, ())),
                "xdg_wm_base" => st.wm_base = Some(registry.bind(name, version.min(6), qh, ())),
                "wl_seat" => st.seat = Some(registry.bind(name, version.min(5), qh, ())),
                _ => {}
            }
        }
    }
}

delegate_noop!(App: ignore WlCompositor);
delegate_noop!(App: ignore WlSurface);
delegate_noop!(App: ignore WlShm);
delegate_noop!(App: ignore WlShmPool);
delegate_noop!(App: ignore WlBuffer);
delegate_noop!(App: ignore XdgPositioner);
delegate_noop!(App: ignore WlKeyboard);

impl Dispatch<WlSeat, ()> for App {
    fn event(
        st: &mut Self,
        seat: &WlSeat,
        ev: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities { capabilities } = ev
            && let wayland_client::WEnum::Value(caps) = capabilities
            && caps.contains(wl_seat::Capability::Pointer)
            && st.pointer.is_none()
        {
            st.pointer = Some(seat.get_pointer(qh, ()));
        }
    }
}

impl Dispatch<WlPointer, ()> for App {
    fn event(
        st: &mut Self,
        _: &WlPointer,
        ev: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let on = |st: &Self, s: &WlSurface| -> String {
            if st.surface.as_ref() == Some(s) {
                return "the window".to_string();
            }
            st.popups
                .iter()
                .position(|p| p.as_ref().is_some_and(|p| &p.surface == s))
                .map_or("an unknown surface".to_string(), |i| {
                    format!("popup {}", i + 1)
                })
        };
        match ev {
            wl_pointer::Event::Enter {
                surface,
                surface_x,
                surface_y,
                ..
            } => println!(
                "[{:>6} ms] pointer enters {} at {surface_x:.0},{surface_y:.0}",
                st.at(),
                on(st, &surface)
            ),
            wl_pointer::Event::Leave { surface, .. } => {
                println!("[{:>6} ms] pointer leaves {}", st.at(), on(st, &surface))
            }
            wl_pointer::Event::Motion {
                time,
                surface_x,
                surface_y,
            } => println!(
                "[{:>6} ms] pointer motion at {surface_x:.0},{surface_y:.0} time {time}",
                st.at()
            ),
            wl_pointer::Event::Button {
                serial,
                time,
                state: wayland_client::WEnum::Value(state),
                ..
            } => {
                let pressed = state == wl_pointer::ButtonState::Pressed;
                if pressed {
                    st.press_serial = Some(serial);
                }
                println!(
                    "[{:>6} ms] button {} serial {serial} time {time}",
                    st.at(),
                    if pressed { "pressed" } else { "released" }
                );
            }
            _ => {}
        }
    }
}

impl Dispatch<XdgSurface, usize> for App {
    fn event(
        st: &mut Self,
        xdg: &XdgSurface,
        ev: xdg_surface::Event,
        number: &usize,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = ev {
            xdg.ack_configure(serial);
            let Some(Some(p)) = st.popups.get(*number - 1) else {
                return;
            };
            // a popup of the window is yellow, a popup of a popup magenta
            let nested = st.popups[..*number - 1].iter().any(Option::is_some);
            let colour = if nested { MAGENTA } else { YELLOW };
            let px = vec![colour; (POPUP.0 * POPUP.1) as usize];
            let surface = p.surface.clone();
            st.commit_pixels(qh, &surface, POPUP, &px);
            println!(
                "[{:>6} ms] popup {number} ack_configure serial {serial}, buffer {}x{} {}",
                st.at(),
                POPUP.0,
                POPUP.1,
                if nested { "magenta" } else { "yellow" }
            );
        }
    }
}

impl Dispatch<XdgPopup, usize> for App {
    fn event(
        st: &mut Self,
        _: &XdgPopup,
        ev: xdg_popup::Event,
        number: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match ev {
            xdg_popup::Event::Configure {
                x,
                y,
                width,
                height,
            } => println!(
                "[{:>6} ms] popup {number} configure {x},{y} {width}x{height}",
                st.at()
            ),
            xdg_popup::Event::PopupDone => {
                println!("[{:>6} ms] popup {number} popup_done", st.at());
                st.destroy_popup(*number, "popup_done");
            }
            _ => {}
        }
    }
}

impl Dispatch<XdgWmBase, ()> for App {
    fn event(
        _: &mut Self,
        wm: &XdgWmBase,
        ev: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = ev {
            wm.pong(serial);
        }
    }
}

impl Dispatch<XdgSurface, ()> for App {
    fn event(
        st: &mut Self,
        xdg: &XdgSurface,
        ev: xdg_surface::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = ev {
            xdg.ack_configure(serial);
            println!("[{:>6} ms] ack_configure serial {serial}", st.at());
            if st.shown {
                st.draw(qh);
            }
        }
    }
}

impl Dispatch<XdgToplevel, ()> for App {
    fn event(
        st: &mut Self,
        _: &XdgToplevel,
        ev: xdg_toplevel::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let words = |b: &[u8]| -> Vec<u32> {
            b.as_chunks::<4>()
                .0
                .iter()
                .map(|c| u32::from_ne_bytes(*c))
                .collect()
        };
        match ev {
            xdg_toplevel::Event::Configure {
                width,
                height,
                states,
            } => {
                let mut states = words(&states);
                states.sort_unstable();
                let names: Vec<&str> = states
                    .iter()
                    .map(|s| match s {
                        1 => "maximized",
                        2 => "fullscreen",
                        3 => "resizing",
                        4 => "activated",
                        _ => "other",
                    })
                    .collect();
                println!("[{:>6} ms] configure {width}x{height} {names:?}", st.at());
                st.pending = ((width, height), states);
            }
            xdg_toplevel::Event::WmCapabilities { capabilities } => {
                println!(
                    "[{:>6} ms] wm_capabilities {:?}",
                    st.at(),
                    words(&capabilities)
                );
            }
            xdg_toplevel::Event::ConfigureBounds { width, height } => {
                println!("[{:>6} ms] configure_bounds {width}x{height}", st.at());
            }
            xdg_toplevel::Event::Close => println!("[{:>6} ms] close", st.at()),
            _ => {}
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let (mut premap, mut keep_size, mut own, mut script) =
        (false, false, (400, 300), String::new());
    while let Some(a) = args.next() {
        match a.as_str() {
            "--premap" => premap = true,
            "--keep-size" => keep_size = true,
            "--size" => {
                let v = args.next().ok_or("--size WxH")?;
                let (w, h) = v.split_once('x').ok_or("--size WxH")?;
                own = (w.parse()?, h.parse()?);
            }
            "--script" => script = args.next().ok_or("--script TEXT")?,
            other => return Err(format!("unknown argument {other}").into()),
        }
    }
    let mut steps: Vec<(u64, String)> = Vec::new();
    for part in script.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let (ms, verb) = part.split_once(':').ok_or("MS:VERB")?;
        steps.push((ms.parse()?, verb.to_string()));
    }
    steps.sort_by_key(|(ms, _)| *ms);

    let runtime = std::env::var("XDG_RUNTIME_DIR")?;
    let display = std::env::var("WAYLAND_DISPLAY")?;
    let stream =
        std::os::unix::net::UnixStream::connect(std::path::Path::new(&runtime).join(display))?;
    stream.set_nonblocking(true)?;
    let conn = Connection::from_socket(stream)?;
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    let mut app = App {
        own,
        keep_size,
        started: Some(Instant::now()),
        ..App::default()
    };
    let _registry = conn.display().get_registry(&qh, ());

    let mut created = false;
    let mut next = 0;
    loop {
        queue.dispatch_pending(&mut app)?;
        conn.flush()?;
        if !created
            && let (Some(compositor), Some(_), Some(wm_base)) = (
                app.compositor.clone(),
                app.shm.as_ref(),
                app.wm_base.clone(),
            )
        {
            created = true;
            let surface = compositor.create_surface(&qh, ());
            let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
            let toplevel = xdg.get_toplevel(&qh, ());
            toplevel.set_title("fs_client".to_string());
            toplevel.set_app_id("org.infynitae.fs_client".to_string());
            if premap {
                println!(
                    "[{:>6} ms] set_fullscreen before the initial commit",
                    app.at()
                );
                toplevel.set_fullscreen(None);
            }
            app.shown = true;
            surface.commit();
            (app.surface, app.xdg, app.toplevel) = (Some(surface), Some(xdg), Some(toplevel));
        }
        if let Some(t0) = app.first_buffer {
            while next < steps.len() && t0.elapsed() >= Duration::from_millis(steps[next].0) {
                let verb = steps[next].1.as_str();
                next += 1;
                println!("[{:>6} ms] step: {verb}", app.at());
                match (verb, app.toplevel.as_ref(), app.surface.as_ref()) {
                    ("fullscreen", Some(t), _) => t.set_fullscreen(None),
                    ("unfullscreen", Some(t), _) => t.unset_fullscreen(),
                    ("maximize", Some(t), _) => t.set_maximized(),
                    ("popup", _, _) => app.open_popup(&qh, false),
                    ("subpopup", _, _) => app.open_popup(&qh, true),
                    ("closepopup", _, _) => match app.popups.iter().rposition(Option::is_some) {
                        Some(i) => app.destroy_popup(i + 1, "closepopup"),
                        None => println!("[{:>6} ms] closepopup: no popup", app.at()),
                    },
                    ("unmap", _, Some(s)) => {
                        app.shown = false;
                        s.attach(None, 0, 0);
                        s.commit();
                    }
                    ("remap", _, Some(s)) => {
                        // the initial commit again; the buffer follows the configure
                        app.shown = true;
                        app.pending = ((0, 0), Vec::new());
                        s.commit();
                    }
                    ("destroy", _, _) => {
                        app.shown = false;
                        if let Some(t) = app.toplevel.take() {
                            t.destroy();
                        }
                        if let Some(x) = app.xdg.take() {
                            x.destroy();
                        }
                        if let Some(s) = app.surface.take() {
                            s.destroy();
                        }
                    }
                    ("exit", _, _) => {
                        conn.flush()?;
                        println!("[{:>6} ms] exit", app.at());
                        return Ok(());
                    }
                    (other, _, _) => println!("[{:>6} ms] step {other} not applicable", app.at()),
                }
                conn.flush()?;
            }
        }
        match conn.prepare_read().map(|guard| guard.read()) {
            Some(Err(wayland_client::backend::WaylandError::Io(e)))
                if e.kind() == std::io::ErrorKind::WouldBlock =>
            {
                std::thread::sleep(Duration::from_millis(4));
            }
            Some(Err(e)) => return Err(format!("connection: {e}").into()),
            _ => {}
        }
    }
}
