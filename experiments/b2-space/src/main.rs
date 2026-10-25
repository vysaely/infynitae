//! B2 experiment: what Smithay 0.7.0 `Space` actually owns and computes.
//!
//! Uses a dummy `SpaceElement` (no Wayland client needed) so `Space`'s own
//! bookkeeping can be observed directly. Output recorded under
//! docs/archive/evidence/phase-b/b2-space-output.txt.

use std::sync::{Arc, Mutex};

use smithay::{
    desktop::{space::SpaceElement, Space},
    output::{Mode, Output, PhysicalProperties, Scale, Subpixel},
    utils::{IsAlive, Logical, Point, Rectangle, Size, Transform},
};

type Log = Arc<Mutex<Vec<String>>>;

#[derive(Clone)]
struct Dummy {
    name: &'static str,
    /// window geometry, relative to the root surface origin
    geometry: Rectangle<i32, Logical>,
    /// bounding box of the whole surface tree, relative to the root surface origin
    bbox: Rectangle<i32, Logical>,
    log: Log,
}
impl PartialEq for Dummy {
    fn eq(&self, o: &Self) -> bool {
        self.name == o.name
    }
}
impl IsAlive for Dummy {
    fn alive(&self) -> bool {
        true
    }
}
impl SpaceElement for Dummy {
    fn geometry(&self) -> Rectangle<i32, Logical> {
        self.geometry
    }
    fn bbox(&self) -> Rectangle<i32, Logical> {
        self.bbox
    }
    fn is_in_input_region(&self, p: &Point<f64, Logical>) -> bool {
        self.bbox.to_f64().contains(*p)
    }
    fn set_activate(&self, a: bool) {
        self.log.lock().unwrap().push(format!("{}: set_activate({a})", self.name));
    }
    fn output_enter(&self, o: &Output, overlap: Rectangle<i32, Logical>) {
        self.log.lock().unwrap().push(format!(
            "{}: output_enter({}, overlap loc=({},{}) size={}x{})",
            self.name,
            o.name(),
            overlap.loc.x,
            overlap.loc.y,
            overlap.size.w,
            overlap.size.h
        ));
    }
    fn output_leave(&self, o: &Output) {
        self.log.lock().unwrap().push(format!("{}: output_leave({})", self.name, o.name()));
    }
}

fn output(name: &str, w: i32, h: i32, mhz: i32, scale: f64) -> Output {
    let o = Output::new(
        name.into(),
        PhysicalProperties { size: (0, 0).into(), subpixel: Subpixel::Unknown, make: "x".into(), model: "x".into() },
    );
    o.change_current_state(
        Some(Mode { size: (w, h).into(), refresh: mhz }),
        Some(Transform::Normal),
        Some(Scale::Fractional(scale)),
        None,
    );
    o
}

fn drain(log: &Log, title: &str) {
    println!("-- {title}");
    let mut l = log.lock().unwrap();
    if l.is_empty() {
        println!("   (no element callbacks)");
    }
    for line in l.drain(..) {
        println!("   {line}");
    }
}

fn rect(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Logical> {
    Rectangle::new((x, y).into(), Size::from((w, h)))
}

fn main() {
    let log: Log = Default::default();
    let mut space: Space<Dummy> = Space::default();

    // W: CSD window, 10px invisible shadow on every side -> geometry inset in bbox.
    let w = Dummy { name: "W", geometry: rect(10, 10, 800, 600), bbox: rect(0, 0, 820, 620), log: log.clone() };
    // F: surface tree extends LEFT/UP of the root surface (e.g. popup or subsurface at negative offset).
    let f = Dummy { name: "F", geometry: rect(0, 0, 400, 300), bbox: rect(-30, -30, 460, 360), log: log.clone() };

    let a = output("A", 1920, 1080, 165_000, 1.0);
    let b = output("B", 3840, 2160, 144_000, 1.5);

    println!("== S1: map elements; Space stores ONE i32 Logical location per element (geometry-origin anchored) ==");
    space.map_element(w.clone(), (1000, 500), true);
    space.map_element(f.clone(), (100, 100), false);
    drain(&log, "callbacks from map_element");
    println!("   element_location(W) = {:?}", space.element_location(&w));
    println!("   element_geometry(W) = {:?}   (location == geometry origin)", space.element_geometry(&w));
    println!("   element_bbox(W)     = {:?}   (bbox.loc = location - geometry.loc + bbox.loc)", space.element_bbox(&w));
    println!("   element_bbox(F)     = {:?}", space.element_bbox(&f));

    println!("\n== S2: outputs mapped at OVERLAPPING positions (Space's only 'camera' = output location, i32) ==");
    space.map_output(&a, (0, 0));
    space.map_output(&b, (500, 200));
    println!("   output_geometry(A) = {:?}", space.output_geometry(&a));
    println!("   output_geometry(B) = {:?}   (logical size = mode / 1.5, ceil)", space.output_geometry(&b));
    space.refresh();
    drain(&log, "refresh with A and B overlapping");
    let mut outs: Vec<String> = space.outputs_for_element(&w).iter().map(|o| o.name()).collect();
    outs.sort();
    println!("   outputs_for_element(W) = {:?}", outs);

    println!("\n== S3: element_under / output_under operate in ONE Space coordinate system ==");
    let p = Point::<f64, Logical>::from((1100.5, 650.25));
    let under = space.element_under(p).map(|(e, loc)| (e.name, loc));
    let mut ou: Vec<String> = space.output_under(p).map(|o| o.name()).collect();
    ou.sort();
    println!("   element_under({:?}) = {:?}  (returned loc = render location = surface origin)", p, under);
    println!("   output_under({:?}) = {:?}  -> one Space point is on BOTH outputs; Space cannot say which camera the pointer looks through", p, ou);

    println!("\n== S4: 'independent camera' move = remap output B only ==");
    space.map_output(&b, (5000, 5000));
    space.refresh();
    drain(&log, "refresh after moving B away");

    println!("\n== S5: overlap convention passed to output_enter (EV-S12) ==");
    // A's left edge cuts through F. F's root surface origin in Space = (100,100); bbox spans x 70..530.
    space.map_output(&a, (120, 0));
    space.refresh();
    drain(&log, "refresh with A at x=120 (cuts F)");
    println!("   F root surface origin in Space: (100,100). Visible part of F's tree starts at Space x=120,");
    println!("   i.e. surface-origin-relative x = 20. Space passed overlap relative to BBOX origin (x=70) instead.");
    println!("   Window::refresh -> output_update interprets overlap relative to the ROOT SURFACE origin (0,0).");

    println!("\n== S6: Space remap replaces location (no second position possible) ==");
    space.map_element(w.clone(), (1001, 500), false);
    println!("   element_location(W) after remap = {:?}", space.element_location(&w));
    println!("   count of W in space.elements() = {}", space.elements().filter(|e| e.name == "W").count());

    println!("\n== S7: z-order lives in Space ==");
    let order: Vec<&str> = space.elements().map(|e| e.name).collect();
    println!("   back-to-front before raise = {:?}", order);
    space.raise_element(&f, true);
    let order: Vec<&str> = space.elements().map(|e| e.name).collect();
    println!("   back-to-front after raise(F) = {:?}", order);
    drain(&log, "callbacks from raise_element(F, activate=true)");
}
