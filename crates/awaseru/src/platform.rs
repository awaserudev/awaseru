//! Which backends this build has, and how to open one — §7.1.
//!
//! Platform support is a trait implemented by one crate per platform, linked in
//! and **selected by name at run time** (§7.1). So this is a registry lookup,
//! and the only place in the host that mentions a platform crate at all.
//!
//! The adapter for each entry does the version check (§16.1) itself, and does
//! it **before** the software is loaded. That ordering is the point: a
//! configuration that names the wrong version should be refused in the time it
//! takes to open a library, not after a cartridge and a debugger have been
//! brought up.

use std::error::Error;
use std::path::Path;

use awaseru_core::Platform;

use crate::version;

/// What opening a reference needs.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    /// The backend library, from the machine-local configuration (§6.1).
    pub library: &'a Path,
    /// A directory the backend may keep its own files in. Handed in rather
    /// than chosen, because where a tool writes on somebody's machine is not
    /// the tool's decision.
    pub home: &'a Path,
    pub software: &'a Path,
    /// The version the configuration declares (§16.1).
    pub declared_version: &'a str,
}

/// Any error from inside a backend crate, carried without the host having to
/// know its type. Its `Display` is what reaches the user, which is why every
/// one of those messages is written to be read.
pub type Failed = Box<dyn Error + Send + Sync>;

/// One backend this build can use.
pub struct Entry {
    /// Matched against the configuration's `project.platform`.
    pub platform: &'static str,
    /// Matched against an emulator's `backend`.
    pub backend: &'static str,
    /// What that crate declares it has been run against (§16.3).
    pub supported_versions: &'static [&'static str],
    pub open: fn(Request<'_>) -> Result<Box<dyn Platform>, Failed>,
}

impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Entry({}/{} supporting {:?})",
            self.platform, self.backend, self.supported_versions
        )
    }
}

/// Every backend linked into this build.
pub const REGISTRY: &[Entry] = &[Entry {
    platform: "snes",
    backend: "mesence",
    supported_versions: awaseru_snes::SUPPORTED_VERSIONS,
    open: open_snes,
}];

/// The entry for a platform and a backend, or `None`.
///
/// Both have to match. A configuration naming a backend that exists for another
/// platform is a different mistake from naming one that does not exist, and
/// `describe` is what turns either into a message.
pub fn find(platform: &str, backend: &str) -> Option<&'static Entry> {
    REGISTRY
        .iter()
        .find(|e| e.platform == platform && e.backend == backend)
}

/// What this build has, for a refusal to name.
pub fn describe() -> String {
    if REGISTRY.is_empty() {
        return "nothing — this build has no backends linked in".to_string();
    }
    REGISTRY
        .iter()
        .map(|e| format!("{}/{}", e.platform, e.backend))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The adapter for the first backend.
///
/// Three steps, in this order and for this reason: open the library, ask it
/// what version it is and refuse if that is not what the configuration says
/// (§16.1), and only then hand it the software.
fn open_snes(request: Request<'_>) -> Result<Box<dyn Platform>, Failed> {
    let backend = awaseru_snes::Backend::open(request.library)?;
    let reported = backend.version().to_string();
    version::check(
        request.declared_version,
        awaseru_snes::SUPPORTED_VERSIONS,
        &reported,
    )?;
    let reference = awaseru_snes::Reference::adopt(backend, request.home, request.software)?;
    Ok(Box::new(reference))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_backend_is_found_by_platform_and_backend_together() {
        let entry = find("snes", "mesence").expect("this build has it");
        assert_eq!(entry.platform, "snes");
        assert!(
            !entry.supported_versions.is_empty(),
            "an entry supporting no versions could never be opened"
        );

        assert!(
            find("snes", "another-backend").is_none(),
            "the backend has to match too"
        );
        assert!(
            find("another-platform", "mesence").is_none(),
            "and so does the platform — a backend is for one platform"
        );
    }

    /// Two entries for one platform and backend would make the lookup's answer
    /// depend on the order of the list.
    #[test]
    fn no_two_entries_claim_the_same_platform_and_backend() {
        for (i, a) in REGISTRY.iter().enumerate() {
            for b in &REGISTRY[i + 1..] {
                assert!(
                    a.platform != b.platform || a.backend != b.backend,
                    "{}/{} is listed twice",
                    a.platform,
                    a.backend
                );
            }
        }
    }

    #[test]
    fn the_refusal_can_name_what_this_build_has() {
        let said = describe();
        assert!(said.contains("snes/mesence"), "said: {said}");
    }
}
