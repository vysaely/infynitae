//! Output facts handed to Core by a backend, and Core's model of connected outputs (OBJ-10,
//! INV-OUTPUT-2).

/// Runtime output identity, allocated by Core and never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OutputId(u64);

/// Persistence identity of an output (INV-OUTPUT-2): derived by the backend from the DRM device,
/// the connector and the EDID make, model and serial (when present). It is not a physical-monitor
/// identity (two identical monitors, or a missing serial, can share a key) and not the runtime
/// identity (`OutputId`). Keys compare by value; collisions follow INV-CAMERA-3.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OutputKey {
    pub device: String,
    pub connector: String,
    pub make: String,
    pub model: String,
    pub serial: Option<String>,
}

/// wl_output.transform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transform {
    Normal,
    R90,
    R180,
    R270,
    Flipped,
    Flipped90,
    Flipped180,
    Flipped270,
}

impl Transform {
    /// Whether the transform exchanges width and height (90°/270° classes).
    pub fn swaps(self) -> bool {
        matches!(
            self,
            Transform::R90 | Transform::R270 | Transform::Flipped90 | Transform::Flipped270
        )
    }
}

/// Hardware facts only. There is deliberately no field for camera position, centre, zoom or
/// binding: a caller cannot choose camera state (probes p8, p12).
pub struct OutputDescriptor {
    pub key: OutputKey,
    /// Physical mode in pixels, before the transform (`phys_O`).
    pub mode: (u32, u32),
    pub transform: Transform,
    /// Output scale in 1/120 units.
    pub scale_120: u32,
    pub refresh_mhz: u32,
}

/// A connected output as Core models it (OutputModel, C2.1, OBJ-10).
#[derive(Clone, Debug, PartialEq)]
pub struct OutputModel {
    pub id: OutputId,
    pub key: OutputKey,
    /// `phys_O`: damage tracker and framebuffer only (F-VIEW-1).
    pub phys: (u32, u32),
    pub transform: Transform,
    pub scale: f64,
    pub refresh_mhz: u32,
    /// Arrangement origin (OutputLogical units), fixed at connect. The configured Arrangement is
    /// Level D; Foundation 01 places each new output to the right of those present.
    pub arrangement: (f64, f64),
}

impl OutputModel {
    pub(crate) fn from_descriptor(
        id: OutputId,
        d: &OutputDescriptor,
        arrangement: (f64, f64),
    ) -> Self {
        OutputModel {
            id,
            key: d.key.clone(),
            phys: d.mode,
            transform: d.transform,
            scale: d.scale_120 as f64 / 120.0,
            refresh_mhz: d.refresh_mhz,
            arrangement,
        }
    }

    /// `view_O = transform_size(phys_O)`: every view, cull and hit computation uses this.
    pub fn view_size(&self) -> (u32, u32) {
        if self.transform.swaps() {
            (self.phys.1, self.phys.0)
        } else {
            self.phys
        }
    }

    /// OutputLogical extent: `view_O / scale_O`.
    pub fn logical_extent(&self) -> (f64, f64) {
        let (w, h) = self.view_size();
        (w as f64 / self.scale, h as f64 / self.scale)
    }
}

/// A descriptor Core accepts: non-empty mode, positive scale.
pub(crate) fn valid(d: &OutputDescriptor) -> bool {
    d.mode.0 > 0 && d.mode.1 > 0 && d.scale_120 > 0
}

#[cfg(test)]
impl OutputId {
    pub(crate) fn for_test(n: u64) -> Self {
        OutputId(n)
    }
}

/// Allocates `OutputId`s; never reuses one.
#[derive(Debug, Default)]
pub(crate) struct OutputIds {
    last: u64,
}

impl OutputIds {
    pub(crate) fn next(&mut self) -> OutputId {
        self.last += 1;
        OutputId(self.last)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn key(connector: &str) -> OutputKey {
        OutputKey {
            device: "headless".into(),
            connector: connector.into(),
            make: "Infynitae".into(),
            model: "Virtual".into(),
            serial: None,
        }
    }

    fn desc(mode: (u32, u32), transform: Transform, scale_120: u32) -> OutputDescriptor {
        OutputDescriptor {
            key: key("A"),
            mode,
            transform,
            scale_120,
            refresh_mhz: 60_000,
        }
    }

    #[test]
    fn view_extent_swaps_exactly_for_quarter_turns() {
        let mut ids = OutputIds::default();
        for (t, want) in [
            (Transform::Normal, (2560, 1440)),
            (Transform::R90, (1440, 2560)),
            (Transform::R180, (2560, 1440)),
            (Transform::R270, (1440, 2560)),
            (Transform::Flipped, (2560, 1440)),
            (Transform::Flipped90, (1440, 2560)),
            (Transform::Flipped180, (2560, 1440)),
            (Transform::Flipped270, (1440, 2560)),
        ] {
            let m =
                OutputModel::from_descriptor(ids.next(), &desc((2560, 1440), t, 120), (0.0, 0.0));
            assert_eq!(m.view_size(), want, "{t:?}");
            assert_eq!(m.phys, (2560, 1440), "phys_O is never transformed");
        }
    }

    #[test]
    fn invalid_descriptors_are_rejected() {
        assert!(!valid(&desc((0, 1080), Transform::Normal, 120)));
        assert!(!valid(&desc((1920, 1080), Transform::Normal, 0)));
        assert!(valid(&desc((1920, 1080), Transform::Normal, 120)));
    }

    #[test]
    fn ids_are_never_reused() {
        let mut ids = OutputIds::default();
        assert_ne!(ids.next(), ids.next());
    }

    #[test]
    fn keys_compare_by_value() {
        assert_eq!(key("A"), key("A"));
        assert_ne!(key("A"), key("B"));
        let mut with_serial = key("A");
        with_serial.serial = Some("1234".into());
        assert_ne!(with_serial, key("A"));
    }
}
