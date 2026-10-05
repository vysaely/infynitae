//! `SurfaceKey ↔ WlSurface` registry (OBJ-06). The adapter mints keys; core only ever sees keys.

use std::num::NonZeroU64;

use infynitae_core::snapshot::SurfaceKey;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;

#[derive(Debug, Default)]
pub struct SurfaceRegistry {
    last: u64,
    entries: Vec<(SurfaceKey, WlSurface)>,
}

impl SurfaceRegistry {
    /// The key of a surface, minted on first sight and never reused.
    pub(crate) fn key_for(&mut self, surface: &WlSurface) -> SurfaceKey {
        if let Some((k, _)) = self.entries.iter().find(|(_, s)| s == surface) {
            return *k;
        }
        self.last += 1;
        let key = SurfaceKey::from_raw(NonZeroU64::new(self.last).expect("keys start at 1"));
        self.entries.push((key, surface.clone()));
        key
    }

    pub fn surface(&self, key: SurfaceKey) -> Option<&WlSurface> {
        self.entries.iter().find(|(k, _)| *k == key).map(|(_, s)| s)
    }

    pub fn key(&self, surface: &WlSurface) -> Option<SurfaceKey> {
        self.entries
            .iter()
            .find(|(_, s)| s == surface)
            .map(|(k, _)| *k)
    }

    /// Role/surface destruction ends the association.
    pub(crate) fn remove(&mut self, surface: &WlSurface) {
        self.entries.retain(|(_, s)| s != surface);
    }
}
