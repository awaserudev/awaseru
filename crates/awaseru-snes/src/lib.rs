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

pub mod fixture;
pub mod memory;
pub mod reference;

/// The versions of the backend this crate has been run against — §16.3.
///
/// A list, never a range. An open range is a promise about builds that do not
/// exist yet: the next release may move something this binding transcribed by
/// hand, and the version refusing to match is how that is caught instead of a
/// corrupted read (see `ffi`'s header).
///
/// It is additive (§16.4). A version that works keeps working when another is
/// added, because a reimplementation verified against one reference was
/// verified against *that* reference.
pub const SUPPORTED_VERSIONS: &[&str] = &["2.2.1"];

pub use ffi::{Backend, LoadError, Version};
pub use memory::{MAPPINGS, Mapping};
pub use reference::{OpenError, Origin, Reference, Startup};
