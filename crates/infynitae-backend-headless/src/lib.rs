//! Infynitae Native headless backend: virtual outputs, virtual vblank and injected physical input,
//! so tests exercise the production core and adapter (FOUNDATION_ARCHITECTURE C1.1, NB-12).
//!
//! INC-0 state: empty crate; it exists so the dependency rules can be enforced (INV-ARCH-1).

#![deny(unsafe_code)]
