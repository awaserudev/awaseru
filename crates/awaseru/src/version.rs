//! The backend's version, checked three ways short of one — §16.1, §16.3.
//!
//! Nothing here knows which backend it is about. The set of versions a backend
//! has adaptations for is the backend crate's to declare, and the version a
//! library reports is the library's to answer; this module is only the rule
//! that relates them to the configuration.

/// Why a backend's version is not acceptable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionError {
    /// The configuration names a version this build has no adaptations for
    /// (§16.3). Refused rather than attempted: an unknown version is a build
    /// whose behaviour nobody here has seen.
    Unsupported {
        declared: String,
        supported: Vec<String>,
    },
    /// The configuration and the library disagree (§16.1).
    Mismatch { declared: String, reported: String },
}

impl std::fmt::Display for VersionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VersionError::Unsupported {
                declared,
                supported,
            } => write!(
                f,
                "the configuration asks for backend version {declared}, which this build has no \
                 adaptations for. It supports {}. §16.3: the supported versions are a list, not a \
                 range — a newer build is not assumed to behave like an older one",
                if supported.is_empty() {
                    "no versions at all".to_string()
                } else {
                    supported.join(", ")
                }
            ),
            VersionError::Mismatch { declared, reported } => write!(
                f,
                "the configuration declares backend version {declared} and the loaded library \
                 reports {reported}. Comparisons made against the wrong reference cannot be \
                 interpreted, so this stops here rather than producing them (§16.1)"
            ),
        }
    }
}

impl std::error::Error for VersionError {}

/// Checks a declared version against what a backend supports and what the
/// loaded library says it is.
///
/// Both legs matter and they answer different questions (§16.2): the first is
/// whether this build of awaseru knows that version of the backend at all, the
/// second is whether the library on this machine is the one the configuration
/// was written against.
pub fn check(declared: &str, supported: &[&str], reported: &str) -> Result<(), VersionError> {
    if !supported.contains(&declared) {
        return Err(VersionError::Unsupported {
            declared: declared.to_string(),
            supported: supported.iter().map(|s| s.to_string()).collect(),
        });
    }
    if declared != reported {
        return Err(VersionError::Mismatch {
            declared: declared.to_string(),
            reported: reported.to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUPPORTED: &[&str] = &["2.2.0", "2.2.1"];

    #[test]
    fn a_declared_and_reported_version_in_the_set_passes() {
        assert_eq!(check("2.2.1", SUPPORTED, "2.2.1"), Ok(()));
        assert_eq!(check("2.2.0", SUPPORTED, "2.2.0"), Ok(()));
    }

    /// The check §16.1 exists for. Without it the tool runs happily against a
    /// reference other than the one its results claim.
    #[test]
    fn a_library_that_is_not_the_declared_version_is_refused() {
        let err = check("2.2.1", SUPPORTED, "2.2.0").expect_err("they disagree");
        assert_eq!(
            err,
            VersionError::Mismatch {
                declared: "2.2.1".into(),
                reported: "2.2.0".into()
            }
        );
        let said = err.to_string();
        assert!(
            said.contains("2.2.1") && said.contains("2.2.0"),
            "the message must name both, said: {said}"
        );
    }

    /// §16.3, which is the one a range would get wrong: a version newer than
    /// anything supported is refused, not accepted for being newer.
    #[test]
    fn a_version_outside_the_set_is_refused_however_new_it_is() {
        for declared in ["2.1.9", "2.2.2", "3.0.0", "9.9.9"] {
            // Declared *and* reported, so the only thing that can refuse it is
            // the set — a mismatch would not catch this.
            let err = check(declared, SUPPORTED, declared)
                .expect_err("{declared} is not in the supported set");
            assert!(
                matches!(err, VersionError::Unsupported { .. }),
                "{declared} should be refused as unsupported, got {err:?}"
            );
            let said = err.to_string();
            assert!(
                said.contains("2.2.0") && said.contains("2.2.1"),
                "and the refusal must name what is supported, said: {said}"
            );
        }
    }

    /// A build with no adaptations at all refuses everything, and says so in
    /// words rather than offering an empty list.
    #[test]
    fn a_backend_with_no_supported_versions_says_so() {
        let err = check("2.2.1", &[], "2.2.1").expect_err("nothing is supported");
        let said = err.to_string();
        assert!(said.contains("no versions at all"), "said: {said}");
    }
}
