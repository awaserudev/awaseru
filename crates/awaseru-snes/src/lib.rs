//! awaseru's first platform backend.
//!
//! It implements the trait in `awaseru-core` by driving an existing emulator
//! (§15). It does not emulate anything itself (§1.3).
//!
//! See `spec.md`.

// §17.1 — `ffi` is the one module that crosses the foreign-function line, and
// the only one allowed `unsafe`. Everything else in this crate is ordinary safe
// Rust built on top of what it exposes.
#[allow(unsafe_code)]
pub mod ffi;

pub mod memory;
pub mod reference;

pub use ffi::{Backend, LoadError, Version};
pub use memory::{MAPPINGS, Mapping};
pub use reference::{OpenError, Reference};
