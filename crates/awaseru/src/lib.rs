//! awaseru — the host.
//!
//! It reads the configuration (§6), selects a platform backend by name (§7.1),
//! drives a reference and reports. Nothing here names a console.
//!
//! See `spec.md`, which is normative.

pub mod arrive;
pub mod cache;
pub mod config;
pub mod differ;
pub mod frame;
pub mod digest;
pub mod localise;
pub mod perturb;
pub mod platform;
pub mod protocol;
pub mod routine;
pub mod session;
pub mod version;
