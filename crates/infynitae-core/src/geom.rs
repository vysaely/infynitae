//! Canvas geometry values.

/// A Canvas position in Q16 fixed point (ADR-02).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanvasPos {
    pub x: i64,
    pub y: i64,
}
