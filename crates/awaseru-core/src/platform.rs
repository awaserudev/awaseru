//! The platform boundary — §7.
//!
//! A backend implements this. Nothing here names a console, a memory or a
//! manufacturer: the host asks what regions exist and addresses them by the
//! names it is given (§2.7), which is what makes a second platform an addition
//! rather than a rewrite.

use crate::region::{Region, Regions, SpanError};
use crate::run::{Bound, Stop};

/// A backend's own version, as the library reports it — §16.1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendVersion {
    /// What the backend says, in the form it says it.
    pub reported: String,
    /// When it was built, where the backend can say.
    pub built: Option<String>,
}

/// Why a read did not produce bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// No region of that name. Distinct from every other failure, because
    /// §3.5 says absent is not equal and a comparison must be able to tell
    /// this apart from a read that went wrong.
    Absent { region: String },
    /// The region exists but may not be read (§3.1).
    NotReadable { region: String },
    /// The span is not a span of that region.
    Span(SpanError),
    /// The backend failed for a reason of its own.
    Backend { why: String },
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Absent { region } => {
                write!(f, "this backend exposes no region named `{region}`")
            }
            ReadError::NotReadable { region } => {
                write!(f, "the region `{region}` is write-only")
            }
            ReadError::Span(e) => write!(f, "{e}"),
            ReadError::Backend { why } => write!(f, "the backend could not read it: {why}"),
        }
    }
}

impl std::error::Error for ReadError {}

impl From<SpanError> for ReadError {
    fn from(e: SpanError) -> Self {
        ReadError::Span(e)
    }
}

/// Why a run could not be started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    /// Nothing is loaded to run.
    NothingLoaded,
    Backend { why: String },
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::NothingLoaded => write!(f, "no software is loaded in the backend"),
            RunError::Backend { why } => write!(f, "the backend could not run: {why}"),
        }
    }
}

impl std::error::Error for RunError {}

/// What a backend must do.
///
/// # What is not here yet, and why
///
/// §7.2's mandatory list also has writing a region, reading and writing the
/// processor state, and saving and loading an opaque state blob. They are
/// absent here because nothing yet calls them, and because their shapes are
/// genuine unknowns — what a processor state *is*, across platforms that do not
/// share a register file, is the question §7.6 leaves open until a second
/// backend exists to answer it with. Declaring them now would be guessing at a
/// signature, which is the one thing §2.4 says not to do.
///
/// What is here is what M0 exercises, and the three decisions that keep the
/// rest open are already taken (§7.5): regions are named and enumerated, the
/// trait lives in this crate rather than the binary, and no platform name
/// crosses this line.
pub trait Platform {
    /// The version the loaded backend reports about itself.
    fn version(&self) -> BackendVersion;

    /// Everything this backend exposes. The host asks; it does not assume.
    fn regions(&self) -> Regions;

    /// A whole region.
    fn read(&self, region: &str) -> Result<Vec<u8>, ReadError>;

    /// A span of one.
    fn read_span(&self, region: &str, offset: usize, len: usize) -> Result<Vec<u8>, ReadError>;

    /// Advances, bounded, and says where it stopped and why (§4.2, §4.3).
    ///
    /// A bound this backend cannot honour is `Reason::Refused`, never an
    /// approximation of it.
    fn run(&mut self, bound: Bound) -> Result<Stop, RunError>;
}

/// Checks a read against the region set, so that every backend does not repeat
/// the same four refusals — and so that they refuse in the same words.
///
/// Returns the region when the read is allowed.
pub fn check_read<'a>(
    regions: &'a Regions,
    name: &str,
    span: Option<(usize, usize)>,
) -> Result<&'a Region, ReadError> {
    let region = regions.get(name).ok_or_else(|| ReadError::Absent {
        region: name.to_string(),
    })?;
    if !region.access.readable() {
        return Err(ReadError::NotReadable {
            region: name.to_string(),
        });
    }
    if let Some((offset, len)) = span {
        region.span(offset, len)?;
    }
    Ok(region)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::Access;

    fn set() -> Regions {
        Regions::new(vec![
            Region::bytes("readable", 16, Access::ReadOnly),
            Region::bytes("writeonly", 16, Access::WriteOnly),
        ])
    }

    #[test]
    fn a_region_that_is_not_there_is_absent_and_says_its_name() {
        let err = check_read(&set(), "nowhere", None).expect_err("not there");
        assert_eq!(
            err,
            ReadError::Absent {
                region: "nowhere".into()
            }
        );
        assert!(err.to_string().contains("nowhere"));
    }

    /// Absent and unreadable are different answers. Folded into one, a
    /// comparison could not tell "the backend has no such thing" from "it has
    /// it and will not show you", and §3.5 turns only the first into
    /// `NotDetermined(RegionAbsent)`.
    #[test]
    fn write_only_is_refused_as_itself_and_not_as_absence() {
        let err = check_read(&set(), "writeonly", None).expect_err("not readable");
        assert_eq!(
            err,
            ReadError::NotReadable {
                region: "writeonly".into()
            }
        );
        assert_ne!(
            err,
            ReadError::Absent {
                region: "writeonly".into()
            },
            "a write-only region exists; saying it is absent would be a different claim"
        );
    }

    #[test]
    fn a_span_past_the_end_is_refused_through_the_same_door() {
        let err = check_read(&set(), "readable", Some((8, 16))).expect_err("past the end");
        assert!(matches!(err, ReadError::Span(_)), "got {err:?}");
        assert!(err.to_string().contains("readable"), "and names the region");
    }

    #[test]
    fn a_readable_region_and_a_span_inside_it_are_allowed() {
        let set = set();
        assert_eq!(check_read(&set, "readable", None).unwrap().size, 16);
        assert!(check_read(&set, "readable", Some((0, 16))).is_ok());
        assert!(check_read(&set, "readable", Some((16, 0))).is_ok());
    }
}
