//! The platform boundary — §7.
//!
//! A backend implements this. Nothing here names a console, a memory or a
//! manufacturer: the host asks what regions exist and addresses them by the
//! names it is given (§2.7), which is what makes a second platform an addition
//! rather than a rewrite.

use crate::blob::{Blob, StateError};
use crate::region::{Region, Regions, SpanError};
use crate::run::{Bound, Stop};
use crate::snapshot::Processor;

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

/// Why a write did not happen.
///
/// Separate from `ReadError` rather than shared with it, because the cases do
/// not line up: a region can be readable and not writable, and the thing a
/// caller does about "this region is read-only" is nothing like what it does
/// about "this region does not exist".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteError {
    /// No region of that name (§3.5).
    Absent { region: String },
    /// The region exists and may not be written (§3.1). A cartridge's program
    /// data is the ordinary case: the console cannot write it, and a comparison
    /// that wrote to it would be changing its subject rather than measuring it.
    NotWritable { region: String },
    /// The span is not a span of that region.
    Span(SpanError),
    /// The reference is running, so a write would land in memory its own thread
    /// is also writing.
    ///
    /// Measured rather than assumed: the first backend's reads are torn the
    /// same way, and a blob loaded into a running machine is overtaken by
    /// execution before anything can be read back (`doc/backend.md`).
    NotStopped,
    Backend { why: String },
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::Absent { region } => {
                write!(f, "this backend exposes no region named `{region}`")
            }
            WriteError::NotWritable { region } => {
                write!(f, "the region `{region}` cannot be written")
            }
            WriteError::Span(e) => write!(f, "{e}"),
            WriteError::NotStopped => write!(
                f,
                "the reference is running, so a write would land in memory it is writing too"
            ),
            WriteError::Backend { why } => write!(f, "the backend could not write it: {why}"),
        }
    }
}

impl std::error::Error for WriteError {}

impl From<SpanError> for WriteError {
    fn from(e: SpanError) -> Self {
        WriteError::Span(e)
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

/// How a reference came up — §4.12.
///
/// Carried on the trait rather than left to each backend's own type, because
/// §4.12 requires every run to say this and a requirement that depends on the
/// host knowing which backend it has is a requirement that will be skipped.
///
/// Nothing here names a platform (§2.7): the settled memories are named by the
/// backend and the host only passes the names along.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Beginning {
    /// Whether it began at a position it can be returned to.
    ///
    /// `false` means `return_to_origin` will refuse and a definition cannot be
    /// replayed — so an anchor cannot be demonstrated, and §2.5 is not
    /// available.
    pub reproducible: bool,
    /// Memories settled on the way up, by the backend's names for them. Empty
    /// when none were, which is what the hardware does and is **not**
    /// reproducible between processes on at least one backend.
    pub settled: Vec<String>,
}

impl std::fmt::Display for Beginning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.reproducible, self.settled.is_empty()) {
            (true, false) => write!(
                f,
                "began at a position it can return to, with {} memories settled — reproducible, \
                 and not what the hardware does",
                self.settled.len()
            ),
            (true, true) => write!(
                f,
                "began at a position it can return to, with memory left as the backend filled \
                 it — which does NOT repeat between processes"
            ),
            (false, false) => write!(
                f,
                "began where the backend happened to be, with {} memories settled — the position \
                 does NOT repeat",
                self.settled.len()
            ),
            (false, true) => write!(
                f,
                "began where the backend happened to be, with memory left as it was — NOTHING \
                 about this run repeats"
            ),
        }
    }
}

impl Beginning {
    /// Whether a run from here can be compared with a run from anywhere else
    /// (§2.5). A report that did not say this would be a report somebody
    /// trusts.
    pub fn repeats(&self) -> bool {
        self.reproducible && !self.settled.is_empty()
    }
}

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

    /// How this reference came up — §4.12. A report says it next to the result.
    fn beginning(&self) -> Beginning;

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

    /// A whole region, written.
    ///
    /// **This does not seed the machine**, and callers reaching for it to do so
    /// are the reason the warning is here rather than in a guide. A console is
    /// not its memories: its video and audio hardware, its transfer units, its
    /// timers and its master clock are not in any region, so a machine with
    /// memory written into it is a machine in a state it could not have reached
    /// by running. §4.7 is why arriving somewhere uses a blob instead, and
    /// §5.3's perturbation is what this verb is for.
    fn write(&mut self, region: &str, bytes: &[u8]) -> Result<(), WriteError>;

    /// A span of one, written.
    fn write_span(&mut self, region: &str, offset: usize, bytes: &[u8])
    -> Result<(), WriteError>;

    /// The processor state (§3.3).
    ///
    /// Opaque in this version — §7.6's question about what a processor state is
    /// across platforms that do not share a register file is still open, and
    /// §2.4 says not to answer it by guessing a shape.
    fn read_processor(&self) -> Result<Processor, ReadError>;

    /// The processor state, written. Carries the same warning as `write`.
    fn write_processor(&mut self, processor: &Processor) -> Result<(), WriteError>;

    /// Takes the machine's whole state, opaquely (§7.2, §4.7).
    ///
    /// Unlike `write`, this **is** the way to put a machine somewhere: a blob
    /// holds everything, including the parts §3's model does not name.
    ///
    /// Implementations must give the blob the position the machine is at
    /// **after** the save, because on at least one backend saving advances it
    /// (`doc/backend.md`).
    fn save_state(&mut self) -> Result<Blob, StateError>;

    /// Returns the reference to the reproducible position it came up at, so
    /// that a definition can be replayed from the beginning.
    ///
    /// This is what makes §4.8's demonstration possible: replaying a definition
    /// three times and comparing the results needs a beginning to replay from,
    /// and it must be the *same* beginning every time or the comparison is of
    /// three different things.
    ///
    /// A backend whose starting position is not reproducible must refuse
    /// rather than return somewhere near it. §2.5 is the whole reason this
    /// verb exists, and a near miss would defeat it silently.
    fn return_to_origin(&mut self) -> Result<(), RunError>;

    /// Puts one back, and checks that it arrived.
    ///
    /// The check is not optional politeness. A backend may report nothing at
    /// all about a load that failed (§13's Q12), so an implementation that did
    /// not compare where it landed against what the blob recorded would leave
    /// its caller unable to tell a resumed machine from an untouched one.
    fn load_state(&mut self, blob: &Blob) -> Result<(), StateError>;
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

/// Checks a write against the region set, the mirror of `check_read` — and
/// separate from it, because `Access` answers two questions and a function that
/// answered one of them for both would have to pick which.
///
/// Returns the region when the write is allowed.
pub fn check_write<'a>(
    regions: &'a Regions,
    name: &str,
    span: Option<(usize, usize)>,
) -> Result<&'a Region, WriteError> {
    let region = regions.get(name).ok_or_else(|| WriteError::Absent {
        region: name.to_string(),
    })?;
    if !region.access.writable() {
        return Err(WriteError::NotWritable {
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

    /// §4.12. The four ways a reference can begin read differently, and the
    /// three that do not repeat say so in capitals — because the line a reader
    /// skims is the one that has to be unskimmable when it is bad.
    #[test]
    fn a_beginning_says_whether_the_run_repeats_and_the_bad_ones_shout() {
        let beginning = |reproducible, settled: &[&str]| Beginning {
            reproducible,
            settled: settled.iter().map(|s| (*s).to_string()).collect(),
        };

        let good = beginning(true, &["work-ram"]);
        assert!(good.repeats());
        assert!(good.to_string().contains("reproducible"), "{good}");
        assert!(
            good.to_string().contains("not what the hardware does"),
            "even the good case admits the divergence: {good}"
        );

        for bad in [
            beginning(true, &[]),
            beginning(false, &["work-ram"]),
            beginning(false, &[]),
        ] {
            assert!(!bad.repeats(), "{bad}");
            let said = bad.to_string();
            assert!(
                said.contains("NOT") || said.contains("NOTHING"),
                "a beginning that does not repeat must say so plainly: {said}"
            );
        }

        let all: Vec<String> = [
            beginning(true, &["x"]),
            beginning(true, &[]),
            beginning(false, &["x"]),
            beginning(false, &[]),
        ]
        .iter()
        .map(|b| b.to_string())
        .collect();
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a, b, "the four must not read alike");
            }
        }
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

    /// Readable and writable are different questions, and the two checks give
    /// different answers to them. A single check reused for both would have to
    /// decide which of `Access`'s two halves it meant, and would then be wrong
    /// about every region that is one and not the other.
    #[test]
    fn a_read_only_region_can_be_read_and_not_written() {
        let set = Regions::new(vec![
            Region::bytes("rom", 16, Access::ReadOnly),
            Region::bytes("ram", 16, Access::ReadWrite),
        ]);

        assert!(check_read(&set, "rom", None).is_ok());
        let err = check_write(&set, "rom", None).expect_err("read-only");
        assert_eq!(
            err,
            WriteError::NotWritable {
                region: "rom".into()
            }
        );
        assert_ne!(
            err,
            WriteError::Absent {
                region: "rom".into()
            },
            "a read-only region exists; saying it is absent would be a different claim"
        );

        assert!(check_write(&set, "ram", None).is_ok());
        assert!(check_write(&set, "ram", Some((8, 8))).is_ok());
    }

    #[test]
    fn a_write_is_refused_through_the_same_three_doors_as_a_read() {
        let set = Regions::new(vec![Region::bytes("ram", 16, Access::ReadWrite)]);
        assert!(matches!(
            check_write(&set, "nowhere", None).expect_err("absent"),
            WriteError::Absent { .. }
        ));
        let err = check_write(&set, "ram", Some((8, 16))).expect_err("past the end");
        assert!(matches!(err, WriteError::Span(_)), "got {err:?}");
        assert!(err.to_string().contains("ram"), "and names the region");
    }

    /// A write-only region is writable. §3.1 has the variant because hardware
    /// registers that latch a value and read back as something else are
    /// ordinary, and the write check must not refuse them for being unreadable.
    #[test]
    fn a_write_only_region_can_be_written_and_not_read() {
        let set = Regions::new(vec![Region::bytes("latch", 16, Access::WriteOnly)]);
        assert!(check_write(&set, "latch", None).is_ok());
        assert!(check_read(&set, "latch", None).is_err());
    }

    #[test]
    fn a_readable_region_and_a_span_inside_it_are_allowed() {
        let set = set();
        assert_eq!(check_read(&set, "readable", None).unwrap().size, 16);
        assert!(check_read(&set, "readable", Some((0, 16))).is_ok());
        assert!(check_read(&set, "readable", Some((16, 0))).is_ok());
    }
}
