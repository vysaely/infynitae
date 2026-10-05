//! Output facts handed to Core by a backend, and Core's model of connected outputs (INV-OUTPUT-2).

/// Runtime output identity, allocated by Core and never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OutputId(u64);

/// Persistence identity of an output (DRM device, connector, EDID make/model/serial when present).
/// Not a physical-monitor identity. INC-1: opaque; persistence and collisions are INC-2.
pub struct OutputKey {
    _private: (),
}

impl OutputKey {
    /// A backend-derived key. INC-1 carries no contents: nothing compares keys before INC-2.
    pub fn opaque() -> Self {
        OutputKey { _private: () }
    }
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

/// A connected output as Core models it (OutputModel, C2.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutputModel {
    pub id: OutputId,
    /// `phys_O`: damage tracker and framebuffer only (F-VIEW-1).
    pub phys: (u32, u32),
    pub transform: Transform,
    pub scale: f64,
    pub refresh_mhz: u32,
}

impl OutputModel {
    /// Validates a descriptor: non-empty mode, positive scale.
    pub(crate) fn from_descriptor(id: OutputId, d: &OutputDescriptor) -> Option<Self> {
        (d.mode.0 > 0 && d.mode.1 > 0 && d.scale_120 > 0).then(|| OutputModel {
            id,
            phys: d.mode,
            transform: d.transform,
            scale: d.scale_120 as f64 / 120.0,
            refresh_mhz: d.refresh_mhz,
        })
    }

    /// `view_O = transform_size(phys_O)`: every view, cull and hit computation uses this.
    pub fn view_size(&self) -> (u32, u32) {
        if self.transform.swaps() {
            (self.phys.1, self.phys.0)
        } else {
            self.phys
        }
    }
}

/// Allocates `OutputId`s; never reuses one.
#[derive(Debug, Default)]
pub(crate) struct OutputIds {
    last: u64,
}

impl OutputIds {
    /// The id `next` will return.
    pub(crate) fn peek(&self) -> OutputId {
        OutputId(self.last + 1)
    }

    pub(crate) fn next(&mut self) -> OutputId {
        self.last += 1;
        OutputId(self.last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desc(mode: (u32, u32), transform: Transform, scale_120: u32) -> OutputDescriptor {
        OutputDescriptor {
            key: OutputKey::opaque(),
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
            let m = OutputModel::from_descriptor(ids.next(), &desc((2560, 1440), t, 120)).unwrap();
            assert_eq!(m.view_size(), want, "{t:?}");
            assert_eq!(m.phys, (2560, 1440), "phys_O is never transformed");
        }
    }

    #[test]
    fn invalid_descriptors_are_rejected() {
        let mut ids = OutputIds::default();
        assert!(
            OutputModel::from_descriptor(ids.next(), &desc((0, 1080), Transform::Normal, 120))
                .is_none()
        );
        assert!(
            OutputModel::from_descriptor(ids.next(), &desc((1920, 1080), Transform::Normal, 0))
                .is_none()
        );
    }

    #[test]
    fn ids_are_never_reused() {
        let mut ids = OutputIds::default();
        assert_ne!(ids.next(), ids.next());
    }
}
