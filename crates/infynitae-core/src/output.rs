//! Output facts handed to Core by a backend (INV-OUTPUT-2).

/// Runtime output identity, allocated by Core and never reused. INC-0: no output is ever
/// connected, so no id is ever allocated.
pub struct OutputId(#[expect(dead_code, reason = "INC-0 shape; allocated from INC-2")] u64);

/// Persistence identity of an output (DRM device, connector, EDID make/model/serial when present).
/// Not a physical-monitor identity. INC-0: opaque; its contents arrive with INC-2.
pub struct OutputKey {
    _private: (),
}

#[cfg(test)]
impl OutputKey {
    pub(crate) fn for_test() -> Self {
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

/// Hardware facts only. There is deliberately no field for camera position, centre, zoom or
/// binding: a caller cannot choose camera state (probes p8, p12).
pub struct OutputDescriptor {
    pub key: OutputKey,
    /// Physical mode in pixels, before the transform.
    pub mode: (u32, u32),
    pub transform: Transform,
    /// Output scale in 1/120 units.
    pub scale_120: u32,
    pub refresh_mhz: u32,
}
